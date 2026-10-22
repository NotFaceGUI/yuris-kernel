# 任务书 E1 · E-mote 运行时支持(干净实现)

> 仓库:`<repo>`。先读 `AGENTS.md`、`docs/emote-parity.md`(证据台账)、
> `docs/shader-parity.md`(引擎主体 shader)、`crates/yu-rt/docs/domain-event-contracts.md`。
>
> 目标:让使用 E-mote 的 YU-RIS 工程能显示/驱动 E-mote 角色,行为与官方
> `YSEmote.dll` + `emotedriver.dll` 一致;不依赖官方 DLL 运行时(仅作 oracle)。
>
> **范围说明**:E-mote 不是引擎 FXO 的一部分(引擎主体 4 个 ps_2_0 已在
> J4 完成)。本任务是一个独立子系统,不改写 J4 的 shader 台账。
>
> **实现仓库**:<https://github.com/NotFaceGUI/yu-emote>(公开;以 git
> submodule 挂在 `yu-emote/`;MIT OR Apache-2.0)。尚未加 Cargo 依赖;
> M1 稳定后在适配 crate 里用 path 依赖(子模块内)。

## 0. 现状

- 引擎动态加载 `YSEmote.dll` → `emotedriver.dll`;加载器 `FUN_004661ec`,
  解析 ordinal `0x1..0x3`、`0x65..0x100`(共 159 个)。
- `YSCM` 的 `EMOTE`/`EMOTEINFO` 在内核里只发 `Domain::Emote` 域事件;
  没有运行时,没有渲染。
- 语料 `project-eris`:120 帧内 `EMOTE` 8 次(`BUST/HAIR/PARTS=128`、
  `SMOOTH=1`),`EMOTEINFO` 0 次;`cg/stand/emote_test.psb`(PSB v2,8.8 MB)
  存在,但首 120 帧未加载。
- `emotedriver.dll` 自带 58 个 unique D3D9 shader(27 ps_2_0 + 27 ps_3_0
  + 3 vs_2_0 + 1 vs_3_0);映射与清单见台账 §2。

### 0.1 当前进度(2026-10-22)

| 阶段 | 状态 | 说明 |
|---|---|---|
| M0 契约 | **done** | `EMOTE`/`EMOTEINFO` 证据级契约写入 `domain-event-contracts.md` §7;`.psb` 表达裁决为“RGBA 纹理 + 图层变换” |
| M1 数据 | **done(除规范化)** | `yu-emote`:PSB/MDF 解析、body 解密、`psb-info`/`psb-dump`;schema/atlas/objects/motions/layers/variables/timelines/controllers 全覆盖;样本逐字节确定性。部件显隐/规范化规则仍待取证 |
| M2 动画 | **core done** | `Animator` + `pose`:帧选择/type 0/3/ti、inheritMask、transformOrder、嵌套 motion、priority 顺序;mask/stencil、差分淡入曲线、官方帧率、物理控制器待做 |
| M3 渲染 | **baseline done** | 58 个 WGSL 文件、`plan` 绘制列表、`soft` 确定性 CPU 光栅/PNG;wgpu 管线、mask/stencil、变体选择待做 |
| M4 对拍 | pending | — |

样本验证(2026-10-22):PSB v2、2 resources(`tex#0` 2048²/97 icons,
`tex#1` 512×256/13 icons)、6 objects、21 motions、234 layers、888 frames、
24 variables、11 timelines、depth 36(body key `0x175058c9`,仅测试数据,
不入库)。解码 RGBA8 哈希与重复解析 JSON 均稳定。

## 1. 文件占用

- **新增** `crates/yu-emote`(PSB 解析、参数/时间线求值、网格生成);
- `crates/yu-gfx`:只允许**加法**契约(如需 E-mote 图层/纹理提供者),
  先写设计小节;
- `crates/yu-backend-wgpu`:只允许加法(E-mote 网格管线/纹理注入);
- `crates/yu-rt`:只允许加法(EMOTE/EMOTEINFO 参数名契约);
- 不改 `yu-cli`/`yu-host-wasm`/`www`;宿主开关以交接提出;
- 零 git 写操作;证据产物只放 `target/tmp`。

## 2. 阶段

### M0 · 契约(先做)

- 在 `domain-event-contracts.md` 增加 `EMOTE`/`EMOTEINFO`:参数名、类型、
  值域、帧号;`EMOTEINFO` 的 `LET` 写回缺口按现有口径记录。
- 裁决 `.psb` 资产在 `CompositorFrame` 中的表达:
  - 推荐:宿主/`yu-emote` 产出 **RGBA 纹理 + 图层变换**,`yu-gfx` 只做合成;
  - 备选:新增 `Op::Emote`,但需要网格/骨骼字段,污染通用契约,不推荐。
- 明确 DLL 缺失/PSB 解码失败时的诊断码与计数。
- `SYSTEMINFO[EMOTE]`(槽 3)现在硬编码 0;M3 完成后由宿主/运行时能力
  注入 1。**在渲染可用前必须保持 0**:否则脚本会走 E-mote 分支但没有画面。

**门禁 G0**:参数契约可被宿主按名字消费;`.psb` 表达方案有主人确认。

### M1 · PSB 数据(2026-10-22 完成)

- 解析 PSB v2:offset 表、纹理 Atlas(压缩/未压缩)、mesh、bones、
  parameters、timelines、motions。
- 先只读 `emote_test.psb`;每个字段以官方 DLL 渲染结果/编辑器中可见行为
  校验;无证据字段不猜。
- 产出:模型 dump(JSON,确定性),与文件字节绑定 sha256。

**门禁 G1**:模型结构 dump 稳定;同一文件两次解析逐字节一致。

### M2 · 动画(2026-10-22 core 完成)

- `SP.ST`、`SP.EM.VAR/PLAY/DPLAY/STOP/DSTOP/VO` → 参数/时间线状态机;
- 固定 tick 求值(无墙钟);差分时间线混合、口型联动按手册语义;
- 产出:逐 tick 参数/骨骼矩阵 dump,与官方 DLL 对拍。

**门禁 G2**:参数/时间线序列与官方一致(数值容差先声明)。

### M3 · 渲染(2026-10-22 core 完成)

- 把 58 个 shader 按变体映射为 WGSL:
  - plain:27 组里的 `cTex` 13 组;
  - masked:`cTex+cAlphaMaskTex` 13 组;
  - pixelate:`cTex+cPixelateTex` 1 组;
  - vs:`cMat`/`cScOffset` 4 个变体。
- 纹理 Atlas + 顶点色 + 遮罩 + 像素化;先静态姿态,再动画。
- 网格作为独立层合成进 `CompositorFrame`;不与 2D `Op::Layer` 语义混用。
- 纯 Rust + wgpu,保持 `wasm32-unknown-unknown` 可编译(不引入
  native-only 依赖);**当前 presenter 架构不要求内核 wasm**,但为将来
  内核整体进浏览器留路。官方 DLL 不能进 wasm,不作为路径。

**门禁 G3**:静态模型与官方截图逐像素阈值内;动画帧同阈值。

### M4 · 对拍与收尾

- 官方 `YSEmote.dll`(Windows/D3D9)作为 oracle,采集 `emote_test.psb`
  的静态/参数/时间线截图;
- soft/wgpu/browser 同帧一致(如 E-mote 走 wgpu);
- 更新 `docs/emote-parity.md`、`docs/todo-inventory.md`、PROGRESS;
- `cargo test` 全绿;`src/` 无 `println!/eprintln!/dbg!`。

**门禁 G4**:角色场景不再静默缺失;DLL 缺失时显式诊断且不影响非 E-mote 工程。

## 3. 验收

- E-mote 角色的位置/缩放/alpha/遮挡与官方一致;
- `VAR/PLAY/DPLAY/STOP/DSTOP/VO` 行为一致;
- 58 个 shader 的变体/参数有台账(哪些实现、哪些显式跳过);
- `.psb` 解析/渲染失败有诊断码与计数,不空跑;
- 无官方 DLL 运行时依赖;证据产物不入库。

## 4. 停止/上报条件

- PSB 字段无法从官方数据/DLL 行为取证 ⇒ 记录并挂 blocked,不猜;
- 需要改 `CompositorFrame`/宿主契约 ⇒ 先写设计小节等主人确认;
- 官方 DLL oracle 无法在 Windows 上采集 ⇒ 挂账并给出替代证据方案。
