# 任务书 J2 · 清理 Title 链路 TODO:正常渲染与动画播放(取证征召版)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`docs/fidelity-ledger-j.md`、
> `docs/render-parity.md`、`docs/gfx-unimplemented-todo.md`、`PROGRESS.md`
> 成果 J1/M1/M2/L1。
>
> 独占:`crates/yu-gfx`、`crates/yu-backend-soft`、`crates/yu-backend-wgpu`
> 及其文档。**不改** `yu-rt` / `yu-vfs` / `yu-cli` / `yu-host-wasm`
> (K2 会话占用宿主;J2 只消费宿主 `/api/report` 与 native 示例)。
> 零 git 写操作;PROGRESS 只追加“成果 J2”。

## 0.1 交付形态:清理 TODO(不是“让画面亮一下”)

本任务的验收 = 下列条目在台账/清单里**关闭**(状态从 `open`/`unimplemented`
变为 `fixed`/`executable`,或被硬阻塞证据证明当前不可做):

| 目标 | 关闭标准 |
|---|---|
| DIV-GFX-006 过渡配对/推进者 | 双纹理 pass 可执行;配对缺失行为有证据 |
| DIV-GFX-007 COPY/COPY2 | 时序/区域/生存期有证据;`Op::Copy` 可执行 |
| DIV-GFX-013 图像加工剩余动作 | SPLTRBUF/分屏等有证据;无证据子项标 `blocked` + 失败路径 |
| DIV-GFX-017 COPY 的 Z 属性 | 重新锚定后修正或明确保持 |
| DIV-GFX-008/041 组 RT alpha/主组直绘 | 若 title 合成需要:证据 + 契约确认后关闭 |
| DIV-GFX-012/033/034/035/040 文本链路 | 若 title 文字/按钮依赖:关闭或写明硬阻塞 |
| 按钮状态后缀 `_na/_off/_on/_over` | 新增 DIV-GFX-043,证据决定归属并实现 |
| SPLTRBUF/分屏命令语义 | 若与 013 分开:新增 DIV-GFX-044,证据 + 实现 |

关闭时同步更新 `docs/gfx-unimplemented-todo.md`、
`crates/yu-gfx/docs/divergences/README.md`、`docs/todo-inventory.md`;
只允许用“硬阻塞证据”保持 open,不允许以“未取证”四个字了事。

## 0.2 硬性总则(与其它任务书的关键区别)

1. **“未取证”不是停机理由,是工作任务。** 本次任务的明确目标就是
   **去取证并实现**:仓库纪律仍是“无证据不猜”,但你必须主动去手册 /
   REA / 官方 SDK / 官方引擎微场景把证据拿回来,而不是把条目挂账了事。
   只有**硬阻塞**(REA 工具不可用、官方引擎跑不起来、样本缺失)才允许
   挂账,并必须在报告中列出已尝试的取证路径。
2. 每个语义点先建台账行(`docs/title-motion-ledger.md`),四件套:
   ①手册页原文 ②真实脚本/产物 ③0.495 REA 地址+摘录 ④官方截图/微场景。
   代码注释用 `EVID-T-xx` / `EVID-B-xx` 反查。
3. 与 gfx 契约冲突的改动(COPY 快照表面、组 RT alpha、按钮状态归属)
   先写设计小节,等主人确认;不得边改边猜。
4. 被跟踪文件一律占位符;语料/截图/微场景产物不入库。

## 1. 已知现状(2026-10-21,来自 K2 会话报告,动手前逐条复核)

日志已把范围缩成三层,先复核这些断言与行号:

1. **转场(动画本体)**
   - `yu-gfx::transition::TransitionDraw` 已有 `layer/from/to/fade/tid`,
     状态可以是 `Executable`(`crates/yu-gfx/src/transition.rs`);
   - 两个后端**无条件跳过**:
     `crates/yu-backend-soft/src/render.rs:325`、
     `crates/yu-backend-wgpu/src/passes.rs:254`;
     日志 reason = `transition.unsupported`(fallback),说明 gfx 并没有拒绝;
   - `SPL101`/`SPL401` 是 boot 的转场层,所以黑屏帧只有清屏。
2. **COPY 与分屏图像加工**
   - `Op::Copy` 在 `crates/yu-gfx/src/state.rs:1246` 恒标
     `Unimplemented("gfx.copy.unsupported")`(DIV-GFX-007);
   - `Op::Image` 只放行 RECTPAINT/2/3(已取证),`SPLTRBUF101/401` 走
     `gfx.imageop.unsupported`;
   - 按纪律跳过并计数,画面保持黑。
3. **Title 按钮**
   - 日志 `asset.missing: cgsys/title/btn_bgmmode` 不是缺文件:包内实际是
     `btn_bgmmode_na/_off/_on/_over.png`;
   - gfx 发的是基名,没有按按钮状态拼后缀;10 条 missing 全来自此。

**非问题(已验证,不要再查)**:宿主 60fps 在 tick;wasm 画面与 native
逐字节一致;`data.ypf` 可读;title 背景/人物资源都在。

## 2. 目标

- **Title 稳定帧**:背景 + 人物 + LOGO + 按钮(含正确状态图)全部渲染;
  title 相关 `asset.missing` = 0。
- **动画正常播放**:转场按 `T/TA` 进度逐帧推进;分屏/缓冲类加工可见;
  不再出现“只有清屏”的整段黑帧。
- 两个后端(soft/wgpu)报告计数一致;wasm 与 native 同帧像素哈希一致
  (K2 宿主的 `/api/report` 可作证据,但 J2 不改宿主)。

## 3. 阶段

### P0 · 复现与三张清单(先做)

1. 用 native `first_frame`/`run`(带 tick 区间)和 K2 宿主各跑一次 title,
   收集:skipped op 清单、asset.missing 清单、每帧 transition/copy/image
   计数、屏幕是否全黑。
2. 从真实脚本定位:title 的层树、`SPL101/SPL401` 的命令序列、按钮配置
   (哪个命令产生按钮、状态从哪来)。
3. 产出 `docs/title-motion-ledger.md` 初始表(三组:transition / split+copy /
   button),每条标“未知/待取证/已确认”。

**门禁 G0**:三张清单可复现,才进入 P1。

### P1 · 证据征召(本次任务的主体,禁止跳过)

**A. 转场**
- 手册:`cg/cg.html` 的 `T`/`TA`/`TID` 原文;是否另有转场命令页。
- REA:`FUN_00414ba4`(0x414ba4)行 905-909 的 fade 公式与双纹理取样;
  `TID` 配对缺失时的第二纹理来源;推进者 `T` vs `+0x1e8`;`TA` 的精度语义。
- 官方微场景(用 YSCom harvest 管线):两层 + `T=0/64/128/192/256` +
  `TA` 两组,逐级截图;确认 `from/to` 取样、混合公式、缺失配对行为。
- 交付 `EVID-T-01..`;每条含四件套。

**B. 分屏 / SPLTRBUF 类图像加工**
- 在工程 `ysc.ybn` 找真实命令名与参数(不要凭日志猜名字);
- 手册对应页;REA 处理体;微场景确认它是“区域填充/搬运”还是转场变体;
- 交付 `EVID-T-10..`。

**C. COPY/COPY2 收尾**
- 补齐 EVID-J-12(区域 `X/Y/X2/Y2`)与 EVID-J-13(快照生存期/RT 清空):
  受控微场景 + REA `FUN_0042f809` COPY 分支;
- 确认 layer 源与 group 源的时序差异(timing 字段当前写死
  `LastRenderedFrame`,但 J-11 已推翻 group 源的旧结论)。

**D. 按钮状态后缀**
- 资源侧:从 `data.ypf` 导出 title 目录清单,确认 `_na/_off/_on/_over`
  四种状态的真实命名与数量;
- 引擎侧:REA 找拼接点(字符串 `_off`/`_on`/`_over`/`_na` 的引用,
  或按钮绘制函数里对 asset 名的构造);手册 button/UI 页;
- 真实脚本:按钮配置里状态字段来源(哪个命令/变量);
- 归属判定:后缀是在 gfx 资产解析层拼,还是由宿主/按钮状态机在调用前给?
  **必须用证据决定**,不要默认塞进后端。

### P2 · 实现

按证据顺序落地,每项独立小循环(台账行 → 代码 → 回归):
1. 两端 `Op::Transition`:双纹理 pass、fade 取样、clip/alpha/blend 与
   普通层一致;soft/wgpu 计数一致。
2. 分屏/图像加工动作(按 P1-B 结论);无证据的子动作继续显式跳过。
3. COPY/COPY2 + 快照表面缓存(契约改动先经确认)。
4. 按钮状态后缀(按 P1-D 的归属结论;若归宿主,写清交接,不改 K2 的 crate)。
5. 组 RT alpha/主组直绘(若 title 合成需要;DIV-GFX-008/041,先确认契约)。

### P3 · 验收

- Title 稳定帧:按钮/背景/LOGO 区域与官方截图逐区域对比
  (阈值在测试前声明);asset.missing(title)=0。
- 动画:转场按官方微场景的 fade 级数逐帧对比;不再出现非预期黑帧。
- 报告:transition/copy/split 的可执行数从 0 上升;仍跳过项必须有
  `EVID-T-xx` 说明为何不可执行;soft/wgpu 计数一致。
- wasm 与 native 同帧像素哈希一致(K2 宿主提供数据)。

## 4. 验收命令(原始输出进 PROGRESS)

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<project-eris>"   # data.ypf 优先
cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu
cargo run -p yu-backend-soft  --example first_frame
cargo run -p yu-backend-wgpu --example first_frame
cargo clippy -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets
```
官方参照:official `capture_screen.py`(title 稳定态)+ YSCom 微场景
(转场逐级);截图/微场景产物只落 `target/tmp`,不入库。

## 5. 停止/上报条件

- **只有硬阻塞**才允许把条目挂账:REA 工具不可用、官方引擎无法启动、
  样本缺失;报告必须列出已尝试的取证路径与失败证据。
- 需要改 `yu-rt`/`yu-vfs`/宿主 ⇒ 停,写报告;宿主相关交 K2。
- 契约改动(COPY 快照表面、组 RT alpha、按钮状态归属)⇒ 先设计小节等确认。
- 工作区其它会话正在写同一文件 ⇒ 停手注明。

## 6. 环境

```bash
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"
export YURIS_SAMPLE_ERIS_YPF="<side-repo>/workbench/project-eris/data.ypf"  # 可选
export YURIS_WGPU_BACKEND=dx12   # 可选
```

REA/手册路径见 `docs/fidelity-ledger-j.md` §0;微场景编译流程见
`docs/eris-corpus.md` 与台账 §0.1。
