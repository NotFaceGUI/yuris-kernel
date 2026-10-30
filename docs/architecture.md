# yuris-kernel 架构

> 状态:实现现状 v1.3(2026-10-22 审查后修订)。上位文档:`README.md`(目标)、
> `docs/sdk-eris-analysis.md`(语义一手分析)、`docs/oracle.md`(对拍方法)、
> `CONTEXT.md`(术语)。
>
> **v1.3 修订**(以 `PROGRESS.md` 成果 K1/J3 与 `docs/audit-2026-10-22.md` 为准):
> - 实际 crate 数为 **14**(新增 `yu-cli`、`yu-host-desktop`、`yu-host-wasm`、`yu-sound`、`yu-platform-input`);不再有“wasm 跑内核”的
>   `yu-backend-wasm` 规划——**架构 B(2026-10-21 主人决定)**:Rust 原生读本机数据
>   并跑内核,wasm/JS 只做呈现端。
> - **`RenderBackend` trait 不存在**:两个后端(`yu-backend-soft`、
>   `yu-backend-wgpu`)消费同一个可序列化 `CompositorFrame`,各自实现一帧渲染;
>   契约是**数据**(`CompositorFrame`),不是 trait。§5 已按此改写。
> - `yu-snd` 已以 `yu-sound` 落地(N 任务);`yu-save` 已以 `yu-save` 落地(P4 任务,
  存档容器在 `yu-format::yssd`);`yu-inp` 尚未建 crate(输入语义在 `yu-rt::input`);
>   内核输入语义/时间线在 `yu-rt::input`,宿主侧平台翻译在 `yu-platform-input`。
> - 目录分层:`crates/yu-render/{soft,wgpu}` 放渲染后端,`crates/yu-platform/{host-desktop,host-wasm,input}`
>   放宿主与平台输入;其余 crate 在 `crates/` 下。**包名不变**,`cargo -p` 命令不用改。
> - 帧契约字段以 `crates/yu-gfx/src/frame.rs` 为准(含 `dx_mode`,
>   见 §5.1 的范围说明)。
>
> **v1.4 补充(2026-10-27)**:两个后端是**互相独立的实现** —— `soft` = CPU 模式
> (兼语义参考与逐像素对拍基线),`wgpu` = GPU 模式且**像素工作必须由 GPU 完成**;
> 运行期不得把像素算法委托给 soft,也不得在 GPU 初始化失败时自动降级到 CPU。
> 判定规则、现状账本、迁移顺序与耦合审查见
> [`architecture/backends.md`](architecture/backends.md)。
>
> **v2 提案(2026-10-25,待主人定稿)**:§2.5 跨平台目标架构 —— 桌面并入
> `yu-cli --mode desktop`(运行期开关,不再单独编译)、wasm/Android 等 =
> 同一代码基的**编译期**派生(各自壳 crate)、cpal 声音下沉平台层、
> `@_OS_SOUND` 注入与实际出声解耦。定稿后升版并改写 §2/§3。

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

**13 个 crate,依赖严格向下,边界=契约**(2026-10-22 实测现状):

```mermaid
flowchart TD
    subgraph PLATFORM["yu-platform 宿主层"]
        HOSTWASM["yu-platform/host-wasm<br/>浏览器呈现端(仅 present RGBA)"]
        HOSTDESKTOP["yu-platform/host-desktop<br/>Windows 窗口 + wgpu 上屏 + egui"]
    end
    CLI["yu-cli<br/>原生引擎宿主 + 本地服务 + HMR + 用例"]
    GFX["yu-gfx 图层/CG 状态机<br/>+ CompositorFrame"]
    SND["yu-sound 声音域宿主<br/>SOUND/INFO/END 层状态机 + headless mixer"]
    subgraph RENDER["yu-render 渲染后端"]
        SOFT["yu-render/soft · yu-backend-soft<br/>确定性参考后端 + PNG"]
        WGPU["yu-render/wgpu · yu-backend-wgpu<br/>主后端(wgpu)"]
    end
    RT["yu-rt 运行核<br/>任务调度·命令分派·表达式·16槽寄存器·变量域<br/>(A5: 零硬编码,全表来自游戏数据)"]
    FMT["yu-format 容器解码<br/>ybn·yssd·ymv·yscfg · 纯字节进出"]
    VFS["yu-vfs 数据源<br/>DirSource · YpfSource · MemSource · Mount · 归一"]
    FONT["yu-font 字体发现/栅格/样式<br/>(独立;尚未接入 gfx 主线)"]
    TRC["yu-trace oracle<br/>边界记录/回放/diff"]
    LOG["yu-log Diag/错误码/LogSink"]

    HOSTWASM -.->|WebSocket 帧| CLI
    HOSTDESKTOP --> CLI
    HOSTDESKTOP -.->|窗口上屏| WGPU
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
| `yu-format` | ybn 家族容器与结构解码(YSCM/YSLB/YSVR/YSTD/YSTL、YSTB 脚本组/窗口)+ YSSD 存档容器(解码 + 官方逐字节写路径;压缩层两代 = zlib level 1 / Google Snappy)+ `yssfs.dat` 明文记录 + CP932/GBK 编解码器;bytes 进 structs 出 | log, flate2(C zlib 后端 + libz-sys=1.1.22 钉版;yssd 写路径逐字节) | 碰 IO;碰执行语义;**不碰 ypf(包布局归 yu-vfs)** |
| `yu-rt` | 命令组 VM:任务调度(让出=WAIT/显示文本)、按名分派、表达式求值(i64/f64/串)、16 槽 P/R 寄存器、变量域、输入时间线 | vfs, format, log | 渲染、音频、平台调用;**任何游戏/版本常量(A5)** |
| `yu-gfx` | 图层树/Z/混合的**语义状态机**;产出可序列化 **`CompositorFrame`**(图层清单 + passes + 诊断);`GfxHost` 回答 INFO 同步查询 | rt, log | 绕过 rt 直接读数据;含平台代码 |
| `yu-sound` | 声音域宿主:层表/循环/淡变/PAN/PITCH 状态机、OGG/WAVE 解码、headless 确定性混音;`SoundHost` 回答 SOUNDINFO | rt, log(以及 hound/lewton) | 碰文件系统/ypf;平台设备输出进 golden |
| `yu-save` | 存档/文件域宿主:槽位表(盘上事实)+ `SAVE/LOAD/SAVEEND/FILEACT` 事件事实消费;`sv.*` 码表 | rt, format, log | 写半忠实存档(DIV-RT-042 门禁);执行外部程序;越过 save 根的路径 |
| `yu-font` | 字体发现/加载、字形栅格化、文本样式;不含换行/对齐 | log | 平台窗口 |
| `yu-render/soft` · `yu-backend-soft` | `CompositorFrame` 的**确定性参考后端**:纹理解析 + RGBA8 合成 + 快照 + 图像加工 + PNG | gfx, log, vfs | 含游戏语义;读 rt 内部 |
| `yu-render/wgpu` · `yu-backend-wgpu` | 同一 `CompositorFrame` 的 **wgpu 主后端**:离屏 RGBA8 + 读回;纹理/管线/bind group/快照缓存;独立 WGSL 文件 | gfx, soft(复用像素算法与报告类型), log | 含游戏语义 |
| `yu-platform/host-wasm` · `yu-host-wasm` | 浏览器**呈现端**(wasm-bindgen):WebGPU shim 优先、Canvas2D 回退;不读游戏数据、不解释渲染语义 | 仅 web-sys/wasm-bindgen | 依赖 rt/gfx/后端/vfs |
| `yu-platform/input` · `yu-platform-input` | 宿主侧输入翻译:**唯一**的物理键码→引擎按钮名、指针/滚轮→按钮、`MOUSE[CURSOR]`→中性形状、光标可见性策略;键表以 `yu-rt` ABI 为准 | rt | 脚本语义;平台窗口/设备 |
| `yu-platform/host-desktop` · `yu-host-desktop` | **原生桌面宿主**(Windows 首版):winit 窗口 + wgpu 上屏 + egui 调试面板;复用 `yu-cli::Engine`,事件翻译走 `yu-platform-input`,不复制引擎语义 | cli, render/wgpu, platform-input, egui/winit | 含游戏语义;直接读数据 |
| `yu-cli` | 原生装配:输入时间线 → rt 泵 → 事件分流 → gfx → 后端呈现;本地 HTTP/WS 服务、HMR 监督、用例执行器;默认 `device` feature 接入 cpal 声音输出(`--no-default-features` 为纯 headless) | 全部 | 含游戏语义 |
| `yu-trace` | 四条边界(①vfs 读 ②事件流 ③帧描述 ④平台命令)JSONL 记录/回放/diff;schema v3 | 全部(旁挂) | 成为运行时依赖 |

## 2.5 跨平台目标架构(v2 提案,2026-10-25,待主人定稿)

> 决策来源(2026-10-25 主人指示):① 桌面与 wasm 都由 cli 派生,桌面经
> `--mode desktop` 运行期开关、不单独编译;② 声音由各平台宿主播放,cli 不播;
> ③ wasm 暂不出声。技术修正(已与主人对过):wasm 无法是同一 exe 的运行期
> 开关(浏览器无 argv、wasm32 是另一编译目标),「cli 派生」在 wasm 上
> = **同一代码基、编译期选目标**。
>
> 同日追加(主人):**Android 等移动编译目标**。落位模式与 wasm 相同——
> L3 新宿主 + L1 端口实现,内核与 `yu-engine` 零改动(R2 的意义);引擎侧
> 唯一新需求 = **挂起/恢复生命周期**(Android 可暂停/杀进程,见 H7)。
> iOS 等后续目标照此模式追加,不新增架构机制。

### 2.5.1 分层图(目标态)

```mermaid
flowchart TD
    subgraph SHELLS["L4 · 入口层 —— 同一代码基 · 桌面开关 + 多编译目标"]
        CLI["yu-cli(exe,桌面唯一入口)<br/>headless: run/case/bench/serve/dev<br/>窗口: --mode desktop(feature = desktop)"]
        WEB["yu-host-web(wasm32-unknown-unknown)<br/>wasm-bindgen 导出 · 无 argv<br/>编译期派生,非运行期开关"]
        ANDR["yu-host-android(aarch64-linux-android 等 NDK 目标)<br/>android_main 入口 · 无 argv<br/>与 wasm 同为编译期派生"]
    end

    subgraph HOSTS["L3 · 宿主层 —— 平台外设,可替换"]
        HDESK["host-desktop(lib,不再单独出 exe)<br/>winit 窗口 + wgpu 上屏 + egui 调试<br/>cpal AudioSink · 原生 fs/存档 · win32 能力(B7)"]
        HWEB["host-web(真 wasm 壳,新;命名待定)<br/>canvas/WebGPU present · fetch data.ypf→MemSource<br/>DOM 输入 · WebAudio(暂 no-op)· OPFS 存档(后)"]
        HWS["host-wasm(WS 呈现端 = 现状架构 B)<br/>调试期保留: serve 推帧<br/>真 wasm 对拍达成后退役"]
        HADR["host-android(新)<br/>winit(android-activity)+ wgpu(Vulkan/GLES)<br/>cpal/oboe AudioSink · AAssetManager→MemSource<br/>内部存储存档 · 触摸→指针 · 挂起/恢复生命周期"]
    end

    ENG["L2 · yu-engine(新 crate)<br/>Engine / EngineOptions / Project / boot / tick / restart<br/>(现 yu-cli/src/engine.rs + project.rs 上移;解 CLI↔桌面宿主依赖环)"]

    subgraph PORTS["L1 · 能力端口 —— 宿主实现,引擎消费"]
        ASSET["AssetSource(数据树:fs 发现 | fetch→Mem)"]
        AUDIO["AudioSink(混音 PCM 出口)"]
        CLOCK["Clock(墙钟注入,A3)"]
        SAVEIO["SaveIo(存档落盘)"]
        PSVC["PlatformSvc(剪贴板/对话框/托盘/字体枚举… B7)"]
    end

    subgraph CORE["L0 · 内核层 —— 平台中立:禁 fs/墙钟/设备 IO"]
        RT["yu-rt"]
        GFX["yu-gfx + CompositorFrame"]
        SND["yu-sound(解码/状态/混音,无设备)"]
        SAVE["yu-save"]
        SOFT["yu-render/soft(纯 CPU = wasm 基线)"]
        WGPUR["yu-render/wgpu(native;web 可选增强)"]
        VFS["yu-vfs(Dir/Ypf/Mem + Mount)"]
    end

    CLI -->|"headless 直装"| ENG
    CLI -->|"--mode desktop 内嵌"| HDESK
    HDESK --> ENG
    WEB --> HWEB
    HWEB --> ENG
    ANDR --> HADR
    HADR --> ENG
    HWS -.->|"WS RGBA 帧(调试)"| CLI

    HDESK -.->|"实现端口"| PORTS
    HWEB -.->|"实现端口"| PORTS
    HADR -.->|"实现端口"| PORTS
    ENG -->|"消费(注入)"| PORTS

    ENG --> RT
    ENG --> GFX
    ENG --> SND
    ENG --> SAVE
    ENG --> SOFT
    ENG --> WGPUR
    GFX --> RT
    SND --> RT
    SOFT --> GFX
    WGPUR --> GFX
    RT --> VFS
```

(内核层内部完整依赖与 v1.3 §2 图相同,此处省略;`yu-format`/`yu-font`/
`yu-log`/`yu-trace` 归属不变。)

**新平台接入模式固定**:L3 一个宿主 crate + L1 一组端口实现,内核与
`yu-engine` 零改动(R2)。iOS 等后续目标照此追加,不新增架构机制。

### 2.5.2 目标态流程图(启动派发 · 一帧主循环 · 生命周期)

结构图(§2.5.1)回答「谁依赖谁」;本图回答「跑起来怎么流」。要点:
**平台差异只出现在 ①端口装配 与 ④出口/生命周期,②③ 四平台逐字同一**。

```mermaid
flowchart TD
    subgraph BOOT["① 启动派发:平台只决定外设从哪来(端口装配)"]
        direction LR
        H["yu-cli headless<br/>run/case/bench/serve"] --> HP["端口: fs 资源 · 无音频<br/>虚时钟(回放)/墙钟 · 存档只读"]
        D["yu-cli --mode desktop"] --> DP["端口: fs · cpal · Instant · save 根"]
        W["wasm 页面加载"] --> WP["端口: fetch→Mem · no-op 音频<br/>performance.now · OPFS(后)"]
        An["android_main"] --> AP["端口: AAsset→Mem · oboe<br/>内部存储 · 触摸→指针"]
    end

    HP --> NEW
    DP --> NEW
    WP --> NEW
    AP --> NEW
    NEW["② Engine::new —— 四平台同一份代码<br/>boot: 装表(A5,缺表拒绝)→ VM → Gfx/Sound/Save 状态"]

    NEW --> START

    subgraph LOOP["③ 一帧主循环(平台零差异)"]
        START("帧起点") --> IN["宿主输入/事件<br/>(yu-platform-input 翻译)"]
        IN --> TICK["clock.now_ms → tick_realtime"]
        TICK --> PUMP["VM 泵至让出 → VmEvent 流"]
        PUMP --> DOM["Gfx/Sound/SaveHost 同步消费<br/>(事件只消费一次)"]
        DOM --> FRAME["CompositorFrame"]
        FRAME --> REN["render: soft(基线) | wgpu"]
        REN --> PRES["present: 窗口 / canvas / GLSurface"]
        PRES --> START
        DOM -.->|"共享 SoundState"| AUD["AudioSink 回调(桌面/安卓)<br/>SoundState::render → PCM(音频线程即时钟)"]
    end

    PRES -.->|"trace ①②③④ 旁挂"| TRC["yu-trace JSONL(对拍)"]
    PRES --> GATE{"④ 每帧检查:出口/生命周期"}
    GATE -->|"headless: frames N"| OUT1["PNG/JSON → 退出"]
    GATE -->|"桌面: 主任务 END"| OUT2["关窗退出"]
    GATE -->|"android 挂起 / wasm 页面隐藏"| SUS["suspend: 停时钟停泵 · 释放呈现器"]
    SUS -->|"resume / 页面可见"| RESUME["重建呈现器"]
    RESUME --> START
```

- 对拍不变量(A3):同数据 + 同输入时间线 + 同虚时钟 ⇒ 任何平台同一帧序列;
  平台差异若造成 golden 分歧,只可能在 ① 或 ④ 出现 —— 排查面被结构收窄。
- 桌面双时钟说明:逻辑时间走 `Clock` 端口(`tick_realtime`),声音渲染由
  音频回调驱动(H2 下沉时明确权威关系,避免声音/逻辑漂移)。

### 2.5.3 与 v1.3 的差异

| 项 | v1.3 现状 | v2 目标 | 动因 |
|---|---|---|---|
| `yu-cli` | 引擎装配(1342 行 engine.rs)+ cpal(`device` feature)+ 全部入口 | 薄壳入口;装配上移 `yu-engine`;cpal 移出、`device` feature 删除 | 跨平台复用同一装配;纯 headless |
| `yu-engine`(新) | — | 唯一引擎装配 crate,所有宿主嵌它 | 解 `yu-cli`↔`host-desktop` 依赖环;wasm 复用 |
| `host-desktop` | 独立 `yu-desktop` exe + lib | 仅 lib,由 `yu-cli` `desktop` feature 内嵌;吸收 cpal AudioSink | 桌面 = 运行期开关,不单独编译 |
| `host-wasm` | WS 呈现端(架构 B 调试胶水) | 保留为调试链路;真 wasm 达成对拍后退役 | 过渡 |
| `host-web`(新,命名待定) | — | 真 wasm 壳:引擎 + soft 编进 wasm32,页面自跑 | 浏览器目标 |
| `host-android`(新) | — | winit(android-activity)壳:wgpu(Vulkan/GLES)+ cpal/oboe 音频 + AAssetManager→MemSource + 内部存储存档 + 挂起/恢复生命周期 | Android 编译目标 |
| 声音设备输出 | 在 `yu-cli`(`engine/sound_device.rs`,cpal) | 移入 `host-desktop`;wasm 端 NullSink(暂不出声) | 声音平台所有 |
| `@_OS_SOUND` | 桌面与 `--no-sound` 绑定(`os_sound=!no_sound`) | 注入与出声解耦:对拍口径恒 `true`;`--no-sound` 只关 sink | A3/A4 对拍不受平台能力影响 |
| 渲染后端 | soft/wgpu 双后端(契约=CompositorFrame) | 不变;soft = wasm 基线,wgpu-web 可选增强 | 契约是数据,天然可移植 |
| 存档 | fs save 根 | 桌面不变;wasm 走 `SaveIo` 端口(OPFS 等,后) | 跨平台 |

### 2.5.4 平台矩阵

| 目标 | 入口/编译 | 渲染 | 音频 | 资源 | 存档 |
|---|---|---|---|---|---|
| Windows 桌面(首版) | `yu-cli --mode desktop` | wgpu(失败回退 soft) | cpal | fs(data.ypf 或散盘) | save 根目录 |
| Linux/macOS 桌面 | 同一 exe(随 winit/wgpu 验收) | wgpu/soft | cpal | 同 | 同 |
| 浏览器 | `host-web` 编 wasm32 | soft 基线(wgpu-web 可选) | WebAudio(暂 no-op) | fetch `data.ypf` → MemSource | OPFS/局部存储(后) |
| Android | `host-android` 编 aarch64-linux-android(cargo-ndk;APK) | wgpu(Vulkan/GLES) | cpal/oboe | APK assets(AAssetManager)→MemSource,或下载数据包→内部存储 | 应用内部存储(SaveIo fs 实现复用) |
| CI/headless | `yu-cli run/case/bench` | soft | 无(NullSink) | fs | 只读观察(现状) |

iOS/其它后续目标按同模式追加(L3 新宿主 + L1 端口实现),不另设机制。

### 2.5.5 能力端口(契约示意;内核只认端口,不认平台)

```rust
trait AssetSource { fn mount(&self) -> Box<dyn DataSource>; }
//   桌面 = Project::discover(fs);web = fetch 单 ypf → MemSource
trait AudioSink   { fn attach(&mut self, snd: Arc<Mutex<SoundState>>); fn detach(&mut self); }
//   桌面 = cpal(回调即时钟,语义随迁);web = no-op;headless = Null
trait Clock       { fn now_ms(&mut self) -> u32; }
//   桌面 = Instant;headless = 虚时钟(回放);web = performance.now
trait SaveIo      { /* 槽位读写 */ }        // 桌面 = 目录;web = OPFS(后)
trait PlatformSvc { /* B7:剪贴板/对话框/托盘/FONTINFO 枚举 */ }
```

### 2.5.6 设计规则(候选不变式,主人可升格 A6/A7)

- **R1 内核零平台代码**:L0 禁 `std::fs`/墙钟/设备 IO,一切经端口注入
  (现有 A3 时钟注入的推广)。
- **R2 唯一装配**:所有宿主嵌同一 `yu-engine`;golden/case 语义 = 装配语义,
  平台差异只允许存在于端口实现。
- **R3 契约 = 数据 + 端口**:`CompositorFrame` 契约不变;宿主外设以端口交付,
  禁止宿主反向伸进内核。
- **R4 单一桌面入口**:桌面是 `yu-cli` 的运行期模式;wasm/Android 等是
  同一代码基的编译目标(各自壳 crate、各自入口 ABI,均无 argv),不存在
  运行期 `--mode wasm/android`。
- **R5 声音平台所有**:`@_OS_SOUND` 注入恒按对拍口径,与是否真出声解耦。
- **R6 wasm 基线 = soft**(确定性参考天然可移植);wgpu-web 是可选增强,
  不作为验收门槛。

### 2.5.7 是否最优:方案对比与取舍(2026-10-25 评估)

**结论先行:在「多平台 + 对拍为真值 + 净室 + 单代码库」四约束下,v2 是
结构最优;若任一约束放松(比如只做 Windows),更简单的方案反而更好。
它是约束下的最优,不是无条件的最简。**

备选方案对比:

| 备选 | 结构 | 为什么落选 |
|---|---|---|
| v1.3 现状 | 装配寄居 `yu-cli`,host-desktop 复用 `yu_cli::Engine` | 依赖环堵死「桌面并入 CLI」;wasm/android 复用装配要么循环依赖要么复制;cpal 在 CLI 使 headless 不纯 |
| A. 单 crate 巨石 + `cfg(target)` 全覆盖 | 不抽中间 crate | cfg 组合随平台数爆炸;wasm 与 native 依赖树纠缠(cpal/winit vs web-sys);装配无法单独测试,违反「边界=契约」(§2) |
| B. 全面插件化/宿主全接口 trait 对象 | 一切皆插件 | 对拍要的是**唯一确定装配**;插件化制造组合不确定性,与 A3 相克;抽象税大于收益,过度设计 |
| C. 每平台自持装配(N 份 Engine/main) | 各平台独立 | N 份漂移,golden 只锁得住一份;修一处改 N 处(旧接手线教训) |
| D. 现成跨平台壳(Qt/Flutter/Tauri)包内核 | 框架出 UI | 像素契约(CompositorFrame→逐字段一致 RenderReport)必须自己搬;框架合成器是不可对拍层,体量与净室纪律冲突 |
| **v2(选定)** | 唯一装配 + 端口 + 薄壳 | 见下方论证与真实成本 |

为什么这四约束指向 v2:

1. **对拍是第一公民**(真值层级 §0):「同输入 ⇒ 同帧」必须在每个平台可
   验证 ⇒ 唯一装配 + 平台差异囚于端口,是结构上的最小充分条件;任何把
   平台差异引进装配的方案都直接劣化对拍。
2. **净室/零硬编码(A5)**:装配与端口不携带平台/游戏常量;A、C 都守不住。
3. **目标数 ≥ 4**(桌面/浏览器/安卓/headless,后续可加 iOS)且共享同一
   内核语义 ⇒ 复用同一装配的边际收益最大;目标越多,v2 相对 C 的优势
   越大(维护量 O(1) vs O(N))。
4. **迁移是搬不是重写**:H1 抽 crate 是单文件机械搬移(engine.rs 1342 行),
   H2–H4 是删代码/搬代码;没有一步引入新算法语义 ⇒ 风险可控、可逐字节验收。

真实成本与非最优处(诚实账,不藏):

- 多一层 crate + 端口 trait 的间接(编译时间、跳转、API 面变大);
- 桌面**双时钟**(Clock 端口推逻辑时间 vs 音频回调驱动声音)必须在 H2
  定权威关系,否则声音/逻辑漂移 —— 已列入流程图注记;
- wasm/安卓**首版体验上限**:soft 基线是 CPU 合成,低端机大分辨率可能吃力
  (wgpu-web / Vulkan 是增强路径,不锁死);
- Android 生命周期(suspend/resume)是唯一渗入装配的新语义 —— 以端口
  调用进入,属最小侵入,但确实扩大了 `yu-engine` API;
- egui 调试 UI 在安卓触摸下未验证(调试件,不阻塞主线)。

### 2.5.8 迁移步骤(退出条件可验证)

| # | 内容 | 退出条件 |
|---|---|---|
| H1 | 抽 `yu-engine`(engine.rs + project.rs 上移;`yu-cli`/`host-desktop` 改依赖,host-desktop 残余 `yu_cli::output/args` 引用一并改指) | 全测试绿;`host-desktop` 不再依赖 `yu-cli` |
| H2 | 声音下沉:cpal SoundSink 移入 `host-desktop`;删 `device` feature;`@_OS_SOUND` 与 `--no-sound` 解耦 | headless 构建零声卡依赖;golden 逐字节不变 |
| H3 | 桌面并入 CLI:`desktop` feature + `--mode desktop`;退役 `yu-desktop` bin | 同一 exe 两模式跑同一工程;`--shot` 与旧 exe 逐字节一致 |
| H4 | 装配端口化:AssetSource/Clock/SaveIo/PlatformSvc 注入;headless 虚时钟 + NullSink | A3 回放测试全绿;serve/dev 不回归 |
| H5 | 真 wasm 壳(host-web):MemSource + soft + present;www 双模式(本地 wasm / WS 调试) | 浏览器离线跑通同一工程;首 N 帧 hash 与 native soft 一致 |
| H6 | 退役 WS 呈现链路(host-wasm);多平台对拍记录入台账 | 台账归档 |
| H7 | `host-android` 壳(cargo-ndk + winit/android-activity + wgpu + cpal/oboe;AAssetManager→MemSource;**生命周期接线**:挂起=停时钟停泵、恢复=重建呈现器,复用 restart/DIV-HOST-001 机制;触摸→指针;APK 打包)。依赖 H4,可与 H5 并行 | 真机跑通同一工程,首 N 帧 hash 与 native soft 一致;挂起/恢复不丢状态、存档即时落盘 |

### 2.5.9 开放问题(待主人定夺)

1. 真 wasm 壳的 crate 命名与落位(新名 `host-web`?还是吸收现 `host-wasm` 名字)。
2. www 页面双模式切换方式(路由/参数);WS 调试链路保留多久。
3. wasm 存档选型(OPFS / localStorage / IndexedDB)与官方槽位大小约束。
4. wgpu-web 是否纳入 wasm 目标(增强),还是长期 soft-only。
5. `desktop` 是否进 `yu-cli` default features(单体分发 vs 分开 slim 构建)。
6. Android 壳的组织:独立 `host-android`,还是把 `host-desktop` 泛化成
   `host-native`(winit 家族)以 cfg 分支共享事件循环/呈现器(生命周期差异
   是主要分叉点)。
7. Android 分发形态:游戏数据内嵌 APK vs 首启下载到内部存储(影响
   AssetSource 实现与包体;商业包体量需 Play asset delivery 评估)。
8. armeabi-v7a 是否保留(Play 只强制 64 位)。
9. 触摸交互:点击=鼠标左键之外,虚拟按键/手势映射是否要对齐官方
   PDA 目标机语义(取证后定)。
10. iOS 是否同模式启动(winit 有 UIKit 后端;音频 AVAudio;同
    「新宿主 + 端口」模式,待主人排期)。

## 3. 一帧数据流(实现现状,架构 B)

```text
输入时间线(录制/回放/真实输入;`yu_rt::input::InputTimeline`)
  → yu-rt: 泵所有任务至让出(WAIT/显示文本);任务全让出 = 帧边界
  → 事件流(VmEvent: cg/window/text/save/scriptpos…)
  → yu-gfx: GfxHost 同步消费 → 图层树/Z/passes → CompositorFrame
  → yu-backend-soft | yu-backend-wgpu: 执行 passes → RGBA8 屏幕
  → yu-cli: PNG / 本地 WS(YUF1 帧头 + RGBA8) / 用例比对
  → yu-platform/host-desktop: winit 窗口上屏(复用 yu-cli::Engine)
  → yu-platform/host-wasm + www: canvas 呈现(WebGPU shim 优先,Canvas2D 回退)
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
| M3d | hosts(K1/K3):`yu-cli` 原生宿主 + `yu-host-desktop` Windows 窗口 + `yu-host-wasm` 呈现端 | 窗口与 native 同帧;浏览器 canvas 与 native 同 hash;HMR |
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
