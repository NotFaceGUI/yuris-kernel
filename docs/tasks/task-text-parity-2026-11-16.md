# 任务书 · 文本严格 1:1:待显示行缓冲 + 已提交表面

- 状态:**implemented(2026-11-16;严格模型已落地,残余见 DIV-GFX-056/076)**
- 目标域:`TEXT` / `TEXTINFO` / `WORD` / `_` / `RETURNCODE` / 自动文本层 /
  手动 `CGACT[TEXT]`
- 证据基线:官方 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`,REA/Ghidra)、
  手册 `text/text.html`、`text/textinfo.html`、官方语料 `<project-eris>`
- 相关台账:`DIV-GFX-075`(待显示行 vs 表面)、`DIV-GFX-056`(排版度量)、
  `DIV-GFX-057`(无文本层叠加)、`DIV-GFX-055`(`CGACT[TEXT]` 合成)
- 前置任务书:`docs/tasks/task-text-auto-pipeline.md`(状态结构、写入序、
  `+0x6c` 游标、CLEAR/P 语义已取证;本任务不再重复推翻,只做严格模型)

## 0. 一句话目标

把内核当前的“累积显示缓冲近似”改成官方的两层模型:

```text
已提交表面(已画像素) + 待显示行缓冲(+0x38/+0x3c)
```

使自动模式的一行一页(`C,R,P,E`)、`P` 清页、行完成只清缓冲、长行折行/
禁则、行内逐字符样式与 `TEXTINFO[CX/CY]` 光标推进都按官方语义工作,且不
破坏 ERIS 手动模式 `CGACT[TEXT]` 主路径。

## 1. 为什么现在做

当前文本实现的主路径可用,但仍是近似:

- `TextState.bytes/flags` 是**累积显示缓冲**,行完成不清已累计文本;
- `layout_text` 把 `'R'/'P'/'E'` 统一当“有字才换行”,`P` 不清屏;
- 自动模式默认 `C,R,P,E` 本应“一行一页”,当前会累积多行;
- 长行没有按窗口宽度折行/标点禁则;
- 一行内 `FONT`/颜色变化只保留整段单一样式;
- `TEXTINFO[CX/CY]` 依赖宿主注入,`R/P` 后的光标推进未复刻。

影响:非 ERIS 正文的自动文本层、AUTOPAGE=0/1、长行、行内样式场景会与
官方不一致;ERIS 正文因走手动 `CGACT[TEXT]` 暂时未暴露,但不能据此宣称
文本已 1:1。

## 2. 目标 / 非目标

### 目标

1. 自动模式严格区分“待显示行缓冲”和“已提交表面”。
2. `C/R/P/E` 逐标志语义、默认一行一页、AUTOPAGE/AUTORETURN/AUTOCLICK。
3. `TEXT[CLEAR]` 只清待显示缓冲;`P` 才清表面并复位光标。
4. 文本窗口宽度折行 + 标点行首/行尾禁则。
5. 逐字符样式快照(前景/背景/宽/高/样式字节),行内变化可显示。
6. `TEXTINFO[CX/CY]` 与官方光标推进一致。
7. soft/wgpu 同帧逐字节一致;ERIS 手动路径回归不退化。

### 非目标

- 不重做 `yu-font` 字形栅格化/GDI 字迹逐位一致(仍按 DIV-FONT-002 口径)。
- 不把 E-mote 文本/PSB 字体纳入本任务(E-mote `deferred(master)`)。
- 不一次性重写所有渲染层;先立契约,再逐段替换。
- 不为没有证据的“官方像素外形”猜数值;度量不确定的项标 `Likely`,并用
  oracle 收口或保留差异。

## 3. 真值与证据要求

优先级按仓库 `AGENTS.md` §0:

1. 官方引擎可观测 trace(窗口/命令/像素);
2. SDK 0.495 官方二进制 REA;
3. 原版游戏数据自证;
4. 旧线 `<repo-old>` 只作回归参考,不是真值。

必须引用的 REA 锚点(已有取证,先复核再写码):

| 主题 | 锚点 |
|---|---|
| 行完成清缓冲 + 恢复 | `FUN_004046fc`;`0x404824` 置 `+0x36=1`;`0x404828` 调 `FUN_004638e4` |
| 待显示缓冲清空 | `FUN_004638e4`(`0x4638e4`):清 `+0x60/+0x64/+0x68/+0x6c` 与十块堆数组,不动表面 |
| 逐标志处理 | `FUN_00404914`:`R/r`、`P/p`、`C`、`E`;`P` 的 `zero_fill(surface,w*4*h)` |
| 宽度折行/禁则 | `FUN_00404c60`:窗口 `+0x78/+0x7c`、光标 `+0x14/+0x18`、每字符宽 `+0x40`、`strcmp` 常量表 |
| 文本/标志追加 | `WORD 0x458d44`、`_ 0x4567c0`;每字符 `+0x40/+0x44/+0x48/+0x4c/+0x50/+0x54/+0x58/+0x5c` |
| `TEXT` 参数 | `0x453448`;`OUTPUTMODE +0x1c`、`WAIT +0x04`、`AUTO*`、`CLICK/RETURN/PAGE` 标志 |
| `TEXTINFO` | `0x4538cc`:OUTPUTMODE/CX/CY/CLICK/AUTO* |
| 自动/手动分派 | `+0x1c != 1` 不绘制自动层;手动模式 `CGACT[TEXT]` 才画 |

像素度量(行高/基线/字距)若无官方截图,先按 `DIV-GFX-056` 的
`Likely` 参数实现,并把差异登记;不得用截图缺失当不实现理由(AU5)。

## 4. 现状地图

先读代码再动手,不要只按文档假设:

- `crates/yu-rt/src/text.rs`
  - `TextState`、`append_text`、`advance_reveal`、`finish_line`、`clear`;
  - 当前 `bytes/flags` 累积语义;`line_start_*`、`committed_chars`。
- `crates/yu-rt/src/vm/handlers.rs`
  - `TEXT`/`TEXTINFO`/`WORD`/`_`/`RETURNCODE` 处理体;
  - `Vm::text_state` / `sync` 路径。
- `crates/yu-gfx/src/text.rs`
  - `TextStyle`、`TextRun`、`TextGlyph`、`layout_text`;
  - 当前 `R/P/E` 统一换行、单一样式、无折行。
- `crates/yu-gfx/src/state.rs`
  - `TextDraw`、`clear_text`、`text_layers`、`snapshot` 相关。
- `crates/yu-engine/src/engine.rs`
  - `sync_text_state`、自动/手动分派、`set_text_reveal`。
- `crates/yu-render/{core,soft,wgpu}` 的文本合成与字形图集。

## 5. 工作分解

### Phase 0 · 证据与现状冻结

- 复核 `task-text-auto-pipeline.md` 与 DIV-GFX-075/056/057 的冲突项;
- 用 `<side-repo>` 工具/官方引擎采集最小 trace/截图(可行时):
  - 手动模式 `CGACT[TEXT]`;
  - 自动模式 `OUTPUTMODE=1`,默认 `C,R,P,E`;
  - `AUTOPAGE=0/1`;
  - 长行(超窗口宽 + 标点);
  - 行内 `FONT`/颜色变化;
  - 无文本层(`TEXT[ID]` 不存在)。
- 产出 `docs/tasks/text-oracle-<date>.md` 或追加台账:命令、窗口、截图、
  像素/光标数据;明确哪些是 observed、哪些仍 Likely。

**验收**:证据清单与“可测/不可测”结论;不得以“手册缺页”挂账。

### Phase 1 · 契约设计(先评审再写码)

- 定义两层状态的所有权:
  - VM(`yu-rt`)保留官方 `TextState` 字段与游标;
  - gfx 只接收“已提交表面绘制项 + 待显示段”;
  - 引擎负责边界同步,不让后端自行计时。
- 明确迁移策略:手动 `CGACT[TEXT]` 路径保持现有行为;自动层新模型
  独立接入,避免一次性重写。
- 写一份短设计记录(状态字段、所有权、生命周期、清屏/换行事件顺序),
  主人确认后再进入 Phase 2。

**验收**:设计记录 + 不变量(何时清表面、何时清缓冲、何时提交行)。

### Phase 2 · `C/R/P/E` 与表面生命周期

- `C`:置点击等待 `+0x35`,停止推进,等输入;不得提前消费后续标志。
- `R/r`:光标 `X=0`,`Y += 当前标志行高`;不碰表面。
- `P/p`:清目标文本表面(`zero_fill`),光标归零;然后再处理后续标志。
- `E`:仅推进游标/结束,不产生排版副作用。
- 行完成:`+0x36=1` → `FUN_004638e4` 清待显示缓冲,已提交像素保留。
- `TEXT[CLEAR]`:只清待显示缓冲,不清表面;与 `P` 严格区分。
- `TEXTINFO[CX/CY]`:从同一游标状态回答。

**验收**:自动模式默认一行一页;点击后上一行像素保留,下一页 `P` 才清;
手动模式 ERIS 正文不变;soft/wgpu 同帧一致。

### Phase 3 · 折行、禁则、逐字符样式

- 按窗口宽 `+0x78`、当前 X、每字符宽 `+0x40` 决定折行;
- 补 `FUN_00404c60` 的行首/行尾禁则表(先按 REA 常量表实现,再截图校准);
- `TextRun`/`TextGlyph` 支持逐字符样式(颜色/背景/宽/高/样式字节),
  替换整段单快照;
- 行高/基线按 `DIV-GFX-056`:默认 `SY`/ascent;有 oracle 则按截图校准;
- 无文本层场景按 `DIV-GFX-057` 的 `Likely` 行为保留,截图后决定是否改为
  官方行为。

**验收**:长行不溢出窗口;禁则位置与官方截图一致(或差异登记);
行内样式变化可见;soft/wgpu 像素一致。

### Phase 4 · 后端与性能

- soft:按新契约合成“已提交表面 + 待显示段”;
- wgpu:同一契约,文本图集/逐字上传不回归;
- 保持现有字形图集缓存、逐字局部上传、文本层缓存收益;
- 不在后端自建计时器/reveal;所有推进来自 VM/引擎状态。

**验收**:两后端同帧逐字节一致;长文本页无逐帧全页重烤;
`text_gpu`/`text_base_cache` 开关行为不变。

### Phase 5 · 回归与 golden

- 补最小合成用例(手动/自动/长行/禁则/行内样式/无层);
- 用 `<project-eris>` 跑正文、菜单、存档、标题回归;
- 更新 `cases/*.json`/golden 时先确认差异是预期的官方对齐;
- 不得用“空跑通过”或缺少样本静默跳过。

## 6. 验收标准

- 自动模式:默认 `C,R,P,E` 一行一页;`AUTOPAGE=0` 累积;点击后 `P` 清页。
- 手动模式:ERIS `CGACT[TEXT]` 正文、字体、换行不退化。
- `TEXT[CLEAR]` 不出现在“清表面”语义里。
- `TEXTINFO[CX/CY]` 在 `R/P` 后与官方光标一致(有 trace/截图证据)。
- 长行按窗口宽折行,标点禁则与官方一致或差异入档。
- 行内 `FONT`/颜色变化逐字符生效。
- soft/wgpu 同帧逐字节一致。
- `DIV-GFX-075/056/057` 状态更新:`fixed` 或 `fixed(no-manual)` /
  `unsupported-by-official` / `Likely` 带明确残余;不得留 `unimplemented`。
- `PROGRESS.md` 追加成果(验证命令 + 原始结果)。

## 7. 测试矩阵

```text
cargo test -p yu-rt --test fixture text_
cargo test -p yu-gfx --test text_pipeline
cargo test -p yu-engine --test text_sync
cargo test -p yu-backend-soft --test render text
cargo test -p yu-backend-wgpu --test wgpu text_
cargo test -p yu-backend-soft -p yu-backend-wgpu -- --test-threads=1
cargo test --workspace
cargo build --workspace --all-targets
```

真实语料回归至少覆盖:

- `<project-eris>` 正文/菜单/存档/标题;
- 自动文本层最小工程(手动/自动/AUTOPAGE 0/1);
- 若无官方截图/样本,显式记录 `SKIP` 原因与计数。

## 8. 约束

- 净室:不复制 `<repo-old>` 代码;事实可引用,注明出处。
- 未经主人指令不执行 git 写操作;提交信息按 `AGENTS.md` §4.1。
- 被跟踪文件只用占位符(`<repo>`/`<side-repo>`/`<project-eris>` 等)。
- 构建环境:`TEMP/TMP` 指向可写沙箱;Windows 下 `CL=-utf-8`。
- 不得把 E-mote 或无关媒体/平台功能混入本任务。
- 不以“手册缺页”拒绝实现;缺页时按 REA/引擎证据实现并标注。
- 每个提交在 HEAD 上必须通过 `cargo build --workspace --all-targets`。

## 9. 交付物

- 代码 + 单元/集成/后端对拍用例;
- 证据记录(oracle 文档或台账追加);
- `DIV-GFX-075/056/057` 与 `docs/todo-inventory.md`/README 待办同步;
- `PROGRESS.md` 成果条目;
- 最终报告:改了什么、证据、验证原始结果、残余风险、未做项。

## 10. 建议提交拆分

1. `docs(text): 补记文本两层模型与官方 C/R/P/E 取证`
2. `refactor(text): 拆分待显示缓冲与已提交表面状态`
3. `feat(text): 按官方 R/P/C/E 推进光标与清页`
4. `feat(text): 按窗口宽度折行并保留逐字符样式`
5. `test(text): 补自动/手动/长行/行内样式回归`
6. `docs(text): 更新文本台账与 PROGRESS`

> 拆分只为审查方便;同一文件不拆到多个提交,底层先提交,使用方后提交。
