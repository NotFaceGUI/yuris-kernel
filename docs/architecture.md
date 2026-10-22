# yuris-kernel 架构

> 状态:实现现状 v1.3(2026-10-22 审查后修订)。上位文档:`README.md`(目标)、
> `docs/sdk-eris-analysis.md`(语义一手分析)、`docs/oracle.md`(对拍方法)、
> `CONTEXT.md`(术语)。
>
> **v1.3 修订**(以 `PROGRESS.md` 成果 K1/J3 与 `docs/audit-2026-10-22.md` 为准):
> - 实际 crate 数为 **12**(新增 `yu-cli`、`yu-host-wasm`、`yu-sound`);不再有“wasm 跑内核”的
>   `yu-backend-wasm` 规划——**架构 B(2026-10-21 主人决定)**:Rust 原生读本机数据
>   并跑内核,wasm/JS 只做呈现端。
> - **`RenderBackend` trait 不存在**:两个后端(`yu-backend-soft`、
>   `yu-backend-wgpu`)消费同一个可序列化 `CompositorFrame`,各自实现一帧渲染;
>   契约是**数据**(`CompositorFrame`),不是 trait。§5 已按此改写。
> - `yu-snd` 已以 `yu-sound` 落地(N 任务);`yu-inp`/`yu-save` 尚未建 crate;
>   输入时间线目前在 `yu-rt::input`。
> - 帧契约字段以 `crates/yu-gfx/src/frame.rs` 为准(含 `dx_mode`,
>   见 §5.1 的范围说明)。

## 0. 不变式

| # | 不变式 | 含义 |
|---|---|---|
| A1 | **数据源不可见** | 散盘项目(SDK/ERIS 现状)与 .ypf 封包游戏,在 VFS 边界之上必须是同一个程序;两者差异只存在于 `yu-vfs` 一层 |
| A2 | 数据只读 | 游戏数据(散盘或封包)只读;唯一可写 = 真实存档目录(永不写进包) |
| A3 | 确定性 | 相同数据 + 相同输入时间线 ⇒ 相同事件流/帧序列/存档字节;时钟与 RNG 注入 |
| A4 | 真值=官方行为 | 一切语义对齐官方引擎可观测行为;旧线 golden 仅回归参考 |
| A5 | **通用运行时零硬编码** | 运行核与域状态机**禁止任何游戏/版本特定常量**:命令序号、命令名集合、参数关键字、资源路径、层 ID、坐标、分辨率、默认帧率、字体路径、编码……一律从游戏工程数据装载(ysc 命令表/ysv 变量表/yscfg 设置/yst_list 清单);**缺表 = 显式拒绝启动或 `Unimplemented`,禁止内置兜底表**。允许进代码的只有引擎算法语义(指令语义、混合公式、CRC/异或算法、调度规则)——那是"引擎是什么",不是"这个游戏是什么" |

## 1. 核心决策 A:数据源抽象(未封包 → 真封包的迁移轴)

```rust
pub trait DataSource {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>>;   // 或稀疏读句柄
    fn exists(&self, path: &LookupKey) -> bool;
    fn list(&self, dir: &LookupKey) -> Vec<Entry>;
}
```

三个实现,一个挂载栈:

```text
Mount(叠加栈, 后挂者覆盖先挂者):
  [ DirSource  散盘目录 ]      ← SDK/ERIS 项目: 现在
  [ YpfSource  .ypf 封包 ]     ← 商业游戏: base 包 + update1..9.ypf 依次覆盖 [REA 已证挂载顺序]
  [ MemSource  内存 ]          ← 测试夹具(合成微型 ypf/目录)
```

- **路径归一只做一次**:在 `yu-vfs` 内把请求折叠成查找键(分隔符 `/`=`\`、仅 ASCII A–Z 折叠、
  CP932 字节键),各数据源只做精确匹配。这是官方语义(Windows 文件系统行为),也是旁线被测试逼出来的契约。
- `ysbin\*.ybn`、`data/cg/...` 全部走同一 VFS——封包游戏里 ysbin 在包内(`yst%05d.ybn`),散盘项目里在目录里,上层无感。
- **等价性测试(架构自证,永久生效)**:同一棵文件树,分别以 DirSource 与"该树打包成的合成 YpfSource"
  挂载,`open/list/exists` 结果必须逐字节一致。这条测试在,散盘→封包的迁移就永远是零改动。
- 存档走独立的 `SaveDir`(真实目录),与 VFS 完全分离(A2)。

## 2. 核心决策 B:分层与 crate 划分

**12 个 crate,依赖严格向下,边界=契约**(2026-10-22 实测现状):

```mermaid
flowchart TD
    HOSTWASM["yu-host-wasm<br/>浏览器呈现端(仅 present RGBA)"]
    CLI["yu-cli<br/>原生引擎宿主 + 本地服务 + HMR + 用例"]
    GFX["yu-gfx 图层/CG 状态机<br/>+ CompositorFrame"]
    SND["yu-sound 声音域宿主<br/>SOUND/INFO/END 层状态机 + headless mixer"]
    SOFT["yu-backend-soft<br/>确定性参考后端 + PNG"]
    WGPU["yu-backend-wgpu<br/>主后端(wgpu)"]
    RT["yu-rt 运行核<br/>任务调度·命令分派·表达式·16槽寄存器·变量域<br/>(A5: 零硬编码,全表来自游戏数据)"]
    FMT["yu-format 容器解码<br/>ybn·yssd·ymv·yscfg · 纯字节进出"]
    VFS["yu-vfs 数据源<br/>DirSource · YpfSource · MemSource · Mount · 归一"]
    FONT["yu-font 字体发现/栅格/样式<br/>(独立;尚未接入 gfx 主线)"]
    TRC["yu-trace oracle<br/>边界记录/回放/diff"]
    LOG["yu-log Diag/错误码/LogSink"]

    HOSTWASM -.->|WebSocket 帧| CLI
    CLI --> SOFT
    CLI --> WGPU
    CLI --> RT
    CLI --> GFX
    CLI --> SND
    SND --> RT
    SOFT --> GFX
    WGPU --> GFX
    WGPU --> SOFT
    GFX --> RT
    RT --> FMT
    RT --> VFS
    TRC -.->|旁挂| CLI
    TRC -.-> GFX
    TRC -.-> RT
    TRC -.-> FMT
    GFX -.-> LOG
    SND -.-> LOG
    SOFT -.-> LOG
    WGPU -.-> LOG
    RT -.-> LOG
    FMT -.-> LOG
    VFS -.-> LOG
    TRC -.-> LOG
```

| crate | 职责 | 依赖 | 不许做 |
|---|---|---|---|
| `yu-log` | **错误+日志一体包**:Diag/错误码/LogSink/JSONL;所有 crate 的错误与日志唯一出口 | 无 | 成为旁挂(trace 从它消费,不是反向) |
| `yu-vfs` | DataSource 三实现 + Mount 叠加 + 路径归一;ypf 懒索引偏移直读 | log | 读真实文件系统之外的一切语义 |
| `yu-format` | ybn 家族容器与结构解码(YSCM/YSLB/YSVR/YSTD/YSTL、YSTB 脚本组/窗口)+ CP932/GBK 编解码器;bytes 进 structs 出 | log(纯) | 碰 IO;碰执行语义;**不碰 ypf(包布局归 yu-vfs)** |
| `yu-rt` | 命令组 VM:任务调度(让出=WAIT/显示文本)、按名分派、表达式求值(i64/f64/串)、16 槽 P/R 寄存器、变量域、输入时间线 | vfs, format, log | 渲染、音频、平台调用;**任何游戏/版本常量(A5)** |
| `yu-gfx` | 图层树/Z/混合的**语义状态机**;产出可序列化 **`CompositorFrame`**(图层清单 + passes + 诊断);`GfxHost` 回答 INFO 同步查询 | rt, log | 绕过 rt 直接读数据;含平台代码 |
| `yu-sound` | 声音域宿主:层表/循环/淡变/PAN/PITCH 状态机、OGG/WAVE 解码、headless 确定性混音;`SoundHost` 回答 SOUNDINFO | rt, log(以及 hound/lewton) | 碰文件系统/ypf;平台设备输出进 golden |
| `yu-font` | 字体发现/加载、字形栅格化、文本样式;不含换行/对齐 | log | 平台窗口 |
| `yu-backend-soft` | `CompositorFrame` 的**确定性参考后端**:纹理解析 + RGBA8 合成 + 快照 + 图像加工 + PNG | gfx, log, vfs | 含游戏语义;读 rt 内部 |
| `yu-backend-wgpu` | 同一 `CompositorFrame` 的 **wgpu 主后端**:离屏 RGBA8 + 读回;纹理/管线/bind group/快照缓存;独立 WGSL 文件 | gfx, soft(复用像素算法与报告类型), log | 含游戏语义 |
| `yu-host-wasm` | 浏览器**呈现端**(wasm-bindgen):WebGPU shim 优先、Canvas2D 回退;不读游戏数据、不解释渲染语义 | 仅 web-sys/wasm-bindgen | 依赖 rt/gfx/后端/vfs |
| `yu-cli` | 原生装配:输入时间线 → rt 泵 → 事件分流 → gfx → 后端呈现;本地 HTTP/WS 服务、HMR 监督、用例执行器;默认 `device` feature 接入 cpal 声音输出(`--no-default-features` 为纯 headless) | 全部 | 含游戏语义 |
| `yu-trace` | 四条边界(①vfs 读 ②事件流 ③帧描述 ④平台命令)JSONL 记录/回放/diff;schema v3 | 全部(旁挂) | 成为运行时依赖 |

## 3. 一帧数据流(实现现状,架构 B)

```text
输入时间线(录制/回放/真实输入;`yu_rt::input::InputTimeline`)
  → yu-rt: 泵所有任务至让出(WAIT/显示文本);任务全让出 = 帧边界
  → 事件流(VmEvent: cg/window/text/save/scriptpos…)
  → yu-gfx: GfxHost 同步消费 → 图层树/Z/passes → CompositorFrame
  → yu-backend-soft | yu-backend-wgpu: 执行 passes → RGBA8 屏幕
  → yu-cli: PNG / 本地 WS(YUF1 帧头 + RGBA8) / 用例比对
  → yu-host-wasm + www: canvas 呈现(WebGPU shim 优先,Canvas2D 回退)
  → yu-trace 在 ①②③④ 落 JSONL(旁挂)
```

注意:**事件只消费一次**。`yu-cli` 里 `GfxHost` 在 `Vm::tick_at` 期间同步
消费事件;`take_events()` 的 drain 只用于统计(见 `engine.rs::tick` 注释),
不是第二路消费。

## 4. 对齐要点(细节见 sdk-eris-analysis §7)

- 运行核只吃**已解码命令流**(.ybn),运行期零字符串解析;命令**按名**分派(ycd 表),跨版本不对序号。
- 宏不在运行核:宏是 **.yst→.ybn 编译器**(YSCom)的事。内核的自研编译器是独立工具(非运行时),
  用途仅一个:产"已知 .yst ↔ 官方 YSCom 编出的 .ybn"对拍语料; corpora 也可直接用商业游戏封包。
- 调用约定=PINT/PFLT/PSTR×16 + RINT/RFLT/RSTR×16 + PFLG/RFLG + 任务局部 T*;GOSUB 嵌套 64。
- 输入=带符号帧计数器;`TASK[SCRIPTPOS]`=剧本推进锚点,存档围绕它。

## 5. 渲染层与平台后端(契约是数据,不是 trait)

**渲染契约 = 可序列化的 `CompositorFrame`**,不是 trait。`yu-gfx` 产出:

```text
CompositorFrame {
    frame, viewport, dx_mode, clear, color_space, filter,
    image_probe_order, windows, layers[], passes[], text[], diagnostics[]
}
Pass { target: Screen | Offscreen(LayerId) | Texture(LayerId), ops[] }
Op   { Layer | Group | Transition | Mask | Copy | Image }
```

后端各自实现“把 passes 忠实搬到像素”,共享的是**数据与像素算法**,不是接口:

- `yu-backend-soft`:CPU RGBA8,确定性参考;`apply_rect_paint`/`scale_region`/
  `apply_reverse`/`apply_horizontal` 与 `Surface`/`RenderReport` 被 wgpu 复用。
- `yu-backend-wgpu`:GPU 执行;管线按 `BlendMode` 缓存,过渡用独立 WGSL
  (`shaders/transition_fixed|psc|psd.wgsl`),快照用 `copy_texture_to_texture`。
- 两个后端必须输出**逐字段相同**的 `RenderReport`(ops/drawn/skipped/
  passes/offscreen/missing_assets);`yu-cli --reconcile` 首帧对账,
  `frame_walk`/`wgpu_frame_walk` 逐帧比对。
- 浏览器侧:`yu-host-wasm` 只做 `present(rgba, w, h)`;合成仍在 native。
  页面 shim 用 `alphaMode:'opaque'`,与 native 的屏幕 alpha 归一一一致。

### 5.1 `dx_mode` 的范围(重要)

`CompositorFrame.dx_mode` **不是引擎的渲染开关**:

- 官方引擎的 effect 始终创建;psC 的触发条件是“有配对且 `TA>=1`”,psD 是
  “有配对且 `TA<1`”(`docs/shader-parity.md` §3)。
- `@_DXMODE` 只是脚本变量(`es_first.yst` 分支决定走 `CGACT[TRANSITION]`/
  `TID` 还是 `DXBUF`)。
- 因此 `dx_mode → psC/psD` 是**内核扩展契约**(默认 0 不改变现有行为),
  用于 A/B 两条内核转场实现;它不声称与引擎某条路径 1:1。
  文档旧称“引擎双路径”作废(见 `docs/audit-2026-10-22.md` §B1)。

### 5.2 已知未对齐(2026-10-22)

- 主组直绘后首帧 `COPY ID=:/W0/` 无源(DIV-GFX-051);
- `image_probe_order` 空值兜底违反 A5(DIV-GFX-049,open);
- 重启不清渲染器状态(DIV-HOST-001);
- 1px 半像素(DIV-GFX-042)、组 RT 子组 alpha(DIV-GFX-041)仍挂账。

## 6. 里程碑(每步退出条件可验证)

| 阶段 | 内容 | 退出条件 |
|---|---|---|
| M0 | yu-vfs:Mem/Dir/合成 Ypf + Mount + 归一 | **等价性测试**(§1)通过 |
| M1 | yu-format:ybn 家族解码 + 编解码器表(cp932/gbk) | 商业游戏真实 .ybn 语料解析成功且结构自洽 |
| M2 | yu-rt 泵核:任务/让出/分派/寄存器/变量域(零硬编码,全表装载) | 真实脚本事件流 golden 化;时钟注入可回放;**缺表拒绝启动**可测 |
| M3a | yu-gfx `CompositorFrame` 语义显示列表 | 首屏 census 稳定 |
| M3b | `yu-backend-soft` 确定性参考 | 600 帧与 wgpu 逐帧一致 |
| M3c | `yu-backend-wgpu` 主后端 | title/转场/COPY/按钮;与 native 同帧一致 |
| M3d | hosts(K1):`yu-cli` 原生宿主 + `yu-host-wasm` 呈现端 | 浏览器 canvas 与 native 同 hash;HMR |
| M4 | yu-save + 编解码器写回 | 官方 .sd 读→写回字节一致 |
| M5 | 真封包游戏端到端 | 同一内核二进制,换挂载栈即跑商业游戏 |

## 7. 文本编码策略(统一编解码器)

**内部一律 Unicode(Rust `String`);"编码"只是字节边界上的编解码器参数。** 字节↔Unicode 转换
只允许发生在四处边界,内核深处不存在"当前编码"状态:

| 边界 | 方向 | 说明 |
|---|---|---|
| `yu-vfs` 查找键 | bytes→bytes | 资源名匹配是**字节级**操作,但多字节跳步表(前导字节范围)随编解码器不同:CP932 的 0xA1–0xDF 是半角片假名单字节,GBK 里却是前导字节——扫描必须选对跳步表 |
| `yu-format` 文本字段 | bytes↔String | ybn 指令流内字符串常量、yssd 存档文本、名字表,按编解码器解码成 Unicode 进入运行时 |
| `yu-save` 写回 | String→bytes | **存档必须按原编解码器写回**(A1 字节级兼容:GBK 汉化游戏的 .sd 写回还得是 GBK) |
| 文本呈现(gfx) | String→字形 | 字体只面对 Unicode;编解码器差异=字体覆盖需求(kana+汉字),不是渲染逻辑差异 |

- CLI 参数:`--codec cp932|gbk|auto`(缺省 auto;后续可扩 big5/utf8)。
- **编解码器是运行配置的一部分,进 trace 与 golden 头部**——同一次运行绝不二次探测、绝不中途切换(A3 确定性)。

### 自动识别(确定性,可复现)

auto 模式在启动早期、任何解码发生之前执行,顺序固定、平局裁决固定 ⇒ 同一输入永远同一结论:

1. 固定顺序采样三池字节:ybn 指令流内字符串常量 → 资源名字表 → 伴生 config 文本。
2. 双解码打分:分别按 CP932 与 GBK 解码,统计 (a) 非法序列数,(b) 字符构成——
   假名/半角假名密度 ⇒ CP932 权重;汉字且罕见日文假名 ⇒ GBK 权重。
   半角假名连(0xA1–0xDF 单字节 run)是 CP932 的强信号;GBK 2 字节覆盖率是 GBK 的强信号。
3. 裁决:得分差超阈值取高者;低于阈值**缺省 CP932**(日文为原生形态,汉化是少数)。
4. 结论 + 证据分数写入 trace/golden 头;`--codec` 显式指定时跳过探测并记录"用户指定"。

实现取向:自带 ~百行评分器(两个编解码器的合法序列表 + 构成统计),零依赖、可审计;
不引入通用探测库(其不确定性与本仓库 A3 冲突)。解码表用 `codepage` 映射静态表
(CP932/GBK 各一张,存 `yu-format`,与 yssd/ybn 的字节级往返共用)。

## 8. 错误系统与日志 = `yu-log`(排查即架构)

**错误与日志同包:`yu-log`**(类型 + 错误码 + 日志汇,零依赖,所有 crate 可依赖;trace 仍旁挂,
不进运行时依赖)。错误是结构化数据,不是字符串:

```rust
pub struct Diag {
    pub code: &'static str,      // 稳定命名空间码,如 "fmt.ybn.magic" / "rt.cmd.unknown" / "table.missing"
    pub severity: Severity,      // Fatal | Recoverable | Unimplemented | Info
    pub boundary: Boundary,      // Data | Format | Table | Script | Domain | Platform
    pub where_: Loc,             // 脚本:文件/标签/pc/命令名;数据:路径/偏移;表:表名
    pub detail: String,          // 人读详情(含期望 vs 实际)
    pub causes: Vec<Diag>,       // 因果链(底层错误逐层包裹)
}
```

- **七类边界码域**(与契约边界对齐并细分):`vfs.*` `fmt.*` `table.*`(A5 缺表→拒绝启动)
  `rt.*`(脚本执行:未知命令/缺槽/标签不存在/类型错)`gfx.* snd.* inp.*` `save.*` `platform.*`。
  码是数据:可计数、可进 golden、可直链 `docs/divergences/` 档案。
- **严重级语义对齐官方**:`Unimplemented`=已知语义未实装(挂号,留痕,按官方语义继续);
  `Recoverable`=官方引擎可继续(策略按官方观测填充——官方有 `ERROR[COM_XX=n]` 按命令族设
  错误等级 [yst 一手:es_First.yst],错误策略本身是脚本可见语义,必须建模);
  `Fatal`=引擎无法继续,产结构化报告(上下文链+最近 N 条日志)。
- **错误也是事件**:`yu-log` 的 JSONL 与 `yu-trace` 边界流同格式;trace 消费 log 事件入流
  (带帧号/域标),回放 golden 时错误序列一并比对——错误行为本身就是对齐对象。
- **日志**:统一走 `yu-log::LogSink`;CLI 端渲染人读行。**禁止散落 `println!`/`eprintln!`/`dbg!`**,
  CI grep 守卫(与 A5 守卫同族)。
  - 现状例外(2026-10-22 审查,待清):`yu-backend-wgpu` 的 `passes.rs`/`lib.rs`
    仍有受环境变量门控的 `eprintln!`(统计/`YURIS_DBG_SIZEMISS`)。见
    `docs/audit-2026-10-22.md` §C2。
- 排查路径:错误码 → divergences 档案/trace 上下文 → 边界二分。每个 Fatal 报告自带
  "哪条边界、哪个表、哪个脚本位置",不需要人再猜。

## 9. 明确不做(现阶段)

- .yst 自研编译器不进运行时(见 §4;作为独立工具仅在需要产"源码↔官方产物"对拍语料时启动)。
- E-mote/Flash 按目标游戏需要再定。
- 硬编码审计(A5 的执行手段):CI 级 grep 守卫禁止运行核/域状态机出现版本序号、内置命令表、
  内置资源路径常量;表装载事件全部进 trace,可审计"每一张表来自哪个游戏文件"。

## 10. 审查与待办入口

- `docs/audit-2026-10-22.md`:本次改动的逐项审查(确定错误/文档失真/工程回归/已复核正确)。
- `docs/todo-inventory.md`:待办总账(状态快照,真相在各台账)。
- `docs/gfx-unimplemented-todo.md` + `crates/yu-gfx/docs/divergences/README.md`:
  渲染侧 DIV-GFX-001..052。
- `docs/divergences/DIV-GFX-051-main-group-copy-first-frame.md`、
  `docs/divergences/DIV-HOST-001-restart-renderer-state.md`:本次审查新增的两个 P0 档案。
