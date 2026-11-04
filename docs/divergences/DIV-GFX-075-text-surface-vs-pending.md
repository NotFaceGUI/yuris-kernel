# DIV-GFX-075 · 文本“待显示行缓冲” vs 图层表面

- 状态: **open(已登记;内核以累积显示缓冲近似)**
- 影响面: `crates/yu-rt/src/text.rs`、`crates/yu-gfx/src/state.rs`、
  `crates/yu-engine/src/engine.rs`、自动文本层(`TEXT[ID]`)
- 证据基线: 官方 0.495 `yu-ris.exe` sha256 `4f5eefa3…`(Ghidra 12.1.4)、
  手册 `text/text.html`、`<project-eris>` 141 脚本

## 1. 官方模型(逐指令复核)

### 1.1 `+0x38` 是“待显示行缓冲”,不是显示面

- `WORD`(`0x458d44`)把文本追加到 `+0x38`、标志到 `+0x3c`,并把每字符
  宽/高/延迟写入 `+0x40/+0x44/+0x48`;`_`(`0x4567c0`)同型。
- `FUN_004046fc`(调度器 `0x404423` 每帧对 `task+9` 任务调用)在自动
  模式推进 `FUN_00404914`;当 `+0x6c >= +0x64`(标志消费完)时:
  `0x404824: +0x36=1` → `0x404828: call FUN_004638e4` → `task+9=0`。
- `FUN_004638e4`(`0x4638e4`)只清 `+0x60/+0x64/+0x68/+0x6c` 与十块堆
  数组(`+0x38..+0x5c`),**不触碰** `+0x14/+0x18` 光标、`+0x00` 当前层
  对象、`+0x04..+0x1c`,也不触碰图层表面。
- 已画像素在 `task+0x34` 指向的文本窗口表面上;`'R'` 只把 `X=0`、
  `Y += +0x44[flag]`;`'P'` 才 `zero_fill(surface, w*4*h)` 并复位 `X/Y`
  (`FUN_00404914` case `0x50/0x70`)。

**结论**:官方的“清文本”与“清屏幕”是两件事。行完成清的是待显示缓冲;
默认 `AUTOPAGE=1` 时 `RETURNCODE` 追加 `C,R,P,E`,点击后 `P` 清页 ⇒
默认自动模式是**一行一页**。

### 1.2 手动模式不自动绘制

`FUN_004046fc` 先判 `[state+0x1c] != 1` → 直接 `0x404828`(清+恢复),
不调用 `FUN_00404914`。手动模式的 `+0x38` 只供脚本读
(`$_TEXT`/`$_TEXT_FLAG`);ERIS 的真实正文走 `CGACT[TEXT]`。

### 1.3 折行与每字符样式

- `FUN_00404c60` 按窗口对象 `+0x78/+0x7c` 的宽高、当前 `+0x14/+0x18`、
  每字符宽 `+0x40` 决定换行,并对标点做行首/行尾禁则(函数内多组
  `strcmp` + `0x8080xx` 常量表);溢出时 `+0x68 = +0x60` 并返回 -1。
- `WORD`/`_` 每字符写 `+0x4c/+0x50`(前景/背景色)、`+0x54/+0x58/+0x5c`
  (样式字节)、`+0x40/+0x44`(宽/高)。官方允许一行内样式变化。

## 2. 内核当前实现(近似)

- `TextState` 的 `bytes/flags` 是**累积显示缓冲**:行完成/手动恢复不清
  已累计文本(只由 `TEXT[CLEAR]` 的 `clear_seq` 清显示层)。
- `advance_reveal(now_ms)` 按 `FUN_00404c0c` 推进 `reveal_flags/
  reveal_bytes`(全/半角 WAIT、u32 回绕、`'C'` 停);`finish_line` 提交
  当前显示段。`TextSnapshot.reveal_chars` 是“已完成段 + 本段”的可见
  字符总数,gfx 用它截断。
- gfx `layout_text` 把 `'R'/'r'/'P'/'p'/'E'` 统一当“有字才换行”,
  **不**清屏;没有窗口宽度折行/禁则;`TextRun.style` 是整段单快照。
- `TEXT[CLEAR]` 通过 `clear_seq` 清当前层显示缓冲(官方不清表面);
  这是为了内核“显示缓冲”抽象自洽而做的近似。
- `skip_mode` 字段已预留,但当前没有输入置位路径;跳过输入仍走
  `text_advance_requested` → `finish_line` 立即提交整段,不是官方
  `0x801c70` 的“逐帧快进到 `'C'`/段尾”。
- `advance_reveal` 不更新 `+0x14/+0x18`;`TEXTINFO[CX/CY]` 仍依赖
  `Vm::set_text_cursor` 宿主注入,`'R'/'P'` 后的官方光标推进未复刻。

## 3. 可观测差异

0. **手动模式自动层(2026-11-04 实机回归,已修)**:Engine 曾无条件
   `sync_text_state`,ERIS 手动模式在 `(0,0)` 多画一行正文。官方
   `FUN_004046fc` 对 `[+0x1c] != 1` 直接清+恢复,不调用
   `FUN_00404914`;已改为只在 `output_mode == 1` 同步自动层,手动模式
   `clear_text()`(不触碰 `CGACT[TEXT]` 的 `text_draws`)。回归
   `yu-engine/tests/text_sync.rs::engine_manual_mode_does_not_draw_auto_text_layer`。
   `text_probe` 仍强制同步自动层,是**合成诊断探针**,不代表官方手动路径。
1. 自动模式多行:官方默认一行一页(点击后清屏);内核会累积多行。
   语料 ERIS 不使用自动层(`OUTPUTMODE=0`),因此主路径未暴露。
2. `TEXT[CLEAR]`:官方保留已画像素,内核清当前层。
3. 长行:官方按窗口宽度折行 + 禁则;内核只在 `R/P/E` 处换行,可能溢出。
4. 一行内 `FONT`/颜色变化:官方按字符快照;内核整段用最后样式重排。

## 4. 处置

- 本轮(H-P2-04/H-P2-02)不重构表面模型(主人裁决:大范围重构不可取,
  统一语义即可);先保证 VM→gfx 的权威状态、字节 ID、CLEAR 代数、reveal
  曲线正确,并把差异显式登记。
- 后续若要做严格 1:1:在 gfx 引入“已提交表面字形 + 待显示段”两层,
  `'R'` 提交基线、`'P'` 清表面、行完成提交;`layout_text` 的宽度折行
  需按 `FUN_00404c60` 的禁则表补;`TextRun.style` 改为逐字符样式引用。
- 验收方向:官方最小脚本 + 截图/trace(自动模式多行、`AUTOPAGE=0/1`、
  长行折行、行内 `FONT` 变化),见 `H-P2-03` 的官方文本 oracle。

## 5. 复核命令

```bash
cargo test -p yu-rt --test fixture text_
cargo test -p yu-gfx --test text_pipeline
cargo test -p yu-engine --test text_sync
cargo test -p yu-backend-soft --test render text
cargo test -p yu-backend-wgpu --test wgpu text_layout_frames_match_soft_exactly
```
