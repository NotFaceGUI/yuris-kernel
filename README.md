# yuris-kernel

YU-RIS 引擎的兼容内核，GPL-3.0-or-later。
相同游戏数据与相同输入时间线下，封包解析、脚本执行、渲染语义、存档读写等一切可观测行为
逐位一致。

## 立场

- **唯一真值 = 官方引擎可观测行为**（官方 trace / SDK 二进制逆向事实 / 原版数据自证）。
- 旧接手线 `<repo-old>` 只读参考，不搬运代码；其 golden 只作回归参考，**不是真值**。
- 无证据的行为显式 `Unimplemented`，禁止“看起来应该是这样”。
- 通用运行时零硬编码（A5）：命令表/变量表/设置全部运行期从游戏工程装载，缺表拒绝启动。

## 当前状态（2026-10-21）

Title 链路已跑通（ERIS，`_DXMODE=0` 固定管线路径）：

| 阶段 | 状态 |
|---|---|
| M3a 语义显示列表 `CompositorFrame` | 完成 |
| M3b `yu-backend-soft` 确定性参考 | 完成（600 帧与 wgpu 逐帧一致） |
| M3c `yu-backend-wgpu` 主后端 | 完成（title/转场/COPY/按钮；WebGPU 与 native 同帧一致） |
| M3d 桌面 / wasm / Android 宿主 | 桌面 + wasm 进行中（K2）；Android 未开始 |

实测（tick 300，title 稳定帧）：800×600、445 层、27 op、27 drawn、0 skipped；
10 个按钮 `_off/_on/_over/_na` 全部解析；soft/wgpu PNG 逐字节相同；
浏览器 canvas 与 native wgpu 同帧哈希相同；600 帧 soft/wgpu 0 差异。

**尚未 1:1 的部分**：引擎着色器路径（内核用 `dx_mode` 做 A/B，引擎本身没有这个
渲染开关，见 `docs/architecture.md` §5.1）；MODE 4/8/10..18 共 11 个着色器模式
未实现（MODE 3/5/6/7/9 已由状态表实现，两端可执行，仅缺官方像素对拍）；顶点
DIFFUSE 已按 A 灰度定案；MIP/各向异性、组 RT 子组 alpha、文本栅格化、MID/
多部件仍在挂账；主组直绘后首帧 `COPY ID=:/W0/` 无源（DIV-GFX-051）、
`image_probe_order` 空值兜底违反 A5（DIV-GFX-049）、重启不清渲染器状态
（DIV-HOST-001）。
详见 `docs/audit-2026-10-22.md`、`docs/todo-inventory.md`。

## 布局

| crate | 职责 |
|---|---|
| `yu-log` | 结构化诊断 + 日志；所有 crate 的错误唯一出口 |
| `yu-vfs` | 数据源抽象：DirSource / YpfSource / MemSource + 叠加挂载 + 归一路径 |
| `yu-format` | ybn 家族 / yssd / yscfg 等容器与元数据解码，纯字节进出 |
| `yu-rt` | 运行核：脚本 / VM / 任务 / 帧边界 / 16 槽寄存器 / 变量域 / 系统变量 ABI / InputTimeline |
| `yu-gfx` | 渲染语义层：`CompositorFrame`、图层 / 组 / 窗口 / 过渡 / 文本模型、GfxHost 查询 |
| `yu-sound` | 声音域宿主：`SOUND/INFO/END` 层状态机、OGG/WAVE 解码、headless 确定性混音；`SoundHost` 查询 |
| `yu-save` | 存档/文件域宿主：槽位表（盘上事实）+ `SAVE/LOAD/SAVEEND/FILEACT` 事件事实消费；写路径由 `yu-format::yssd` 逐字节对齐官方 |
| `yu-font` | 字体发现 / 加载、字形栅格化、完整文本样式；不含换行 / 对齐 |
| `yu-render/soft` · `yu-backend-soft` | `CompositorFrame` 的确定性软件参考后端：纹理管线 + RGBA8 合成 + PNG |
| `yu-render/wgpu` · `yu-backend-wgpu` | `CompositorFrame` 的 wgpu 主后端：`Rgba8Unorm` 离屏合成 + 读回；纹理/管线/bind group 缓存 |
| `yu-cli` | 原生引擎宿主 + 本地 HTTP/WS 服务 + HMR 监督 + 用例执行器（`run`/`serve`/`dev`/`case`/`bench`） |
| `yu-platform/host-desktop` · `yu-host-desktop` | Windows 原生窗口宿主：winit 0.30 + wgpu 30 上屏 + egui 调试面板；复用 `yu-cli::Engine`，不复制引擎语义 |
| `yu-platform/host-wasm` · `yu-host-wasm` | 浏览器**呈现端**（wasm-bindgen）：WebGPU shim 优先，Canvas2D 回退；不读游戏数据 |
| `yu-platform/input` · `yu-platform-input` | 宿主侧输入翻译：物理键码 / 指针 / 滚轮 → 引擎按钮名，`MOUSE[CURSOR]` → 中性形状，光标可见性策略；桌面与 Web 共用 |
| `yu-trace` | 边界录制 / 回放 / diff；schema v3（producer-neutral、frame_description） |

> **目录分层**：`crates/yu-render/` 放渲染后端（soft/wgpu），`crates/yu-platform/` 放宿主
> 与平台输入（desktop/wasm/input）；其余 crate 直接在 `crates/` 下。**包名不变**，`cargo -p` 命令不用改。

### 模块进度（2026-10-21）

| 模块 | 状态 | 说明 / 主要缺口 |
|---|---|---|
| `yu-log` | 完成 | Diag / 码表 / JSONL sink |
| `yu-vfs` | 完成 | 散盘 / YPF / 内存源 + Mount；NUL-XOR 方言无真样本（DIV-002） |
| `yu-format` | 完成 | ybn 家族 / yssd / yscfg；YSTB v2 未实现；GBK 码表 partial |
| `yu-rt` | 主线完成 | 119 命令普查；TEXT 族、FILEINFO、系统变量 ABI；16 open + 3 blocked DIV、27 条未取证命令 |
| `yu-gfx` | 核心完成 | 转场 / COPY / RECTPAINT / REVERSE / HORIZONTAL / SIZE；MODE 3/5/6/7/9 状态表已实现；11 个 shader 模式、文本 / MID / 多部件 / 组 RT 子组 alpha 挂账 |
| `yu-font` | 完成 | 字体源 / 栅格 / 样式；GRADATION、ROTATION、BNO 等 Unverified |
| `yu-backend-soft` | 完成（参考） | 确定性 RGBA8 / PNG；600 帧与 wgpu 逐帧一致 |
| `yu-backend-wgpu` | 固定管线 title 完成 | 离屏 RGBA8 + 读回；MODE 0..9 可执行；psC/psD 转场独立 WGSL（`dx_mode` 为内核 A/B 契约，非引擎开关）；MODE 4/8/10..18 仍 Unimplemented |
| `yu-trace` | 完成（schema v3） | vfs/vm/frame 边界、回放、diff；官方 L1 已打通；DIV-L-002 open、官方 args 不可观测 |
| `yu-cli` / `yu-host-wasm` | 进行中（K2） | 桌面 / wasm 宿主、HMR、用例；Android 未开始 |
| `yu-host-desktop` | 完成（Windows 首版） | winit + wgpu 上屏；4:3 letterbox + 光标换算；egui 调试面板（F1）；音频回调拉取 |
| `yu-sound` | 完成（N） | 层状态机 / 循环 / 淡变 / PAN / PITCH / OGG+WAVE；负 START 延迟、ACM WAVE、DATALEFT/RIGHT 挂账（DIV-SND-002/004） |
| `yu-save` | 未开始 | 存档 |
| scenario 解释器 | 未开始 | 顺序脚本 / 分支 |

**没有 `yu-assets` 兄弟 crate**：纹理解析在装配 / 后端层，第一个实现是
`yu-backend-soft::TextureStore`；wgpu 后端复用同一 `TextureStore` 的 RGBA8 字节。

### 渲染链路

```text
yu-vfs → yu-format → yu-rt
                         │ VmEvent::DomainCall / Text
                         ▼
                  yu-gfx（CompositorFrame）
                         │
             ┌───────────┴───────────┐
             ▼                       ▼
   yu-backend-soft             yu-backend-wgpu
   确定性参考 / PNG / golden    主后端，同帧计数对账 / 差异账本
             │                       │
             └───────────┬───────────┘
                         ▼
     hosts：yu-platform/host-desktop（Windows）/ yu-platform/host-wasm（浏览器）；Android 未开始
```

## 快速开始

```bash
cargo test --workspace

# 真样本（仓库不附带游戏数据；缺失时测试显式失败 / 跳过并计数）
# 数据入口：封包 <project>/data.ypf 优先，散盘 <project>/data/ 为备，哪个存在用哪个。
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<project-eris>"

# headless 首帧
cargo run -p yu-backend-soft --example first_frame
# → target/tmp/first_frame.png / first_frame.display.json

cargo run -p yu-backend-wgpu --example wgpu_first_frame
# → target/tmp/wgpu_first_frame.png / wgpu_first_frame.report.json
# 可选：YURIS_WGPU_BACKEND=dx12 YURIS_WGPU_ADAPTER=5070

# 逐帧走查（600 帧哈希/计数/耗时）
cargo run -p yu-backend-soft --example frame_walk -- 600
cargo run -p yu-backend-wgpu --example wgpu_frame_walk -- 600

# 浏览器呈现（本地服务 + wasm 呈现端；浏览器只收 RGBA，不下载整包）
cargo build -p yu-cli --release
./target/release/yu-cli serve "<project-eris>" --backend wgpu --port 4173 --pause-at 303
# 打开 http://127.0.0.1:4173/  （--pause-at 暂停在指定帧，便于截图/对拍）
# Windows: .\target\release\yu-cli.exe serve "<project-eris>" --backend wgpu --port 4173

# 开发热重启（改 Rust 自动重建/重启，浏览器 WebSocket 自动重连）
./target/release/yu-cli dev "<project-eris>" --backend wgpu --port 4173

# Windows 原生窗口（winit + wgpu 上屏 + egui 调试面板）
cargo build --release -p yu-host-desktop
target/release/yu-desktop.exe "<project-eris>" --backend wgpu --debug-ui
```

本机沙箱、样本路径与占位符约定见 `AGENTS.md`（本机文件，不入库）。

## 文档

- `PROGRESS.md` — **唯一进度真相**（成果、勘误、验证命令与原始结果）
- `docs/audit-2026-10-22.md` — **2026-10-22 改动审查**（确定错误/文档失真/工程回归）
- `docs/architecture.md` — 实现现状 v1.3（与 `PROGRESS.md` 冲突时以后者为准）
- `docs/render-parity.md` — 渲染语义证据表与后端执行契约
- `docs/fidelity-ledger-j.md` — 任务 J 首帧保真台账（RECTPAINT/alpha/COPY 证据与阈值）
- `docs/title-motion-ledger.md` — title 链路台账（转场/COPY/按钮，EVID-T-01..22）
- `docs/todo-inventory.md` — 100+ 条 DIV + 命令级缺口总账（随台账增长）
- `docs/wgpu-shader-parity-review.md` — wgpu 着色器 / 通用性 / 1:1 审查报告
- `docs/tasks/task-j4-shader-parity.md` — 着色器 1:1 修复任务书
- `docs/gfx-verification-ledger.md` — gfx 复核台账（Confirmed / Refuted / Uncertain）
- `docs/gfx-unimplemented-todo.md` — 未实现模块 DIV 清单与解除条件
- `docs/font-contract.md` / `docs/font-provenance.md` — 字体接口与取证
- `docs/release.md` — tag 触发 Release 与更新日志；本地预演命令
- `docs/oracle.md` — 对拍方法论；`docs/divergences/` — 不一致档案

## TODO 总览

> 完整 100+ 条 DIV 与命令级缺口见 `docs/todo-inventory.md`；这里只列子系统级下一步。

| 子系统 | 下一步 | 依赖 / 阻塞 |
|---|---|---|
| ~~主组 COPY 首帧源~~ | ✅ 已修（AU1）：首帧用 `frame.clear` 合成后缓冲初值 | DIV-GFX-051 fixed；官方启动瞬间截图仍待采集 |
| ~~重启渲染器状态~~ | ✅ 已修（AU1）：两后端 `reset()` + `restart` 调用 + 真语料回归 | DIV-HOST-001 fixed |
| **用例门禁** | `case --all` 已重生成并通过（3/3）；保持为每次改动的提交前门禁 | CI 挂接待宿主 |
| ~~A5 兜底~~ | ✅ 已修（AU1）：探测序空值在 `GfxState::from_tables` 拒绝构造 | DIV-GFX-049 fixed |
| 引擎 shader | MODE 4/8/10..18 的公式/调用点取证 | 无 blob；证据在 `target/tmp/j4-shaders/` |
| 固定管线 3/5/6/7/9 | 官方微场景像素对拍（状态表已实现） | YSCom attach 阻塞 |
| 组 RT / 子组 alpha | 生产路径 RT 格式/写掩码取证 | DIV-GFX-041 partial |
| 文本链路 | gfx 文本布局 + 后端字形路径（yu-font 已就绪） | 依赖 M2 的 TEXT 状态 |
| 资产元数据 | 完整 provider（图像头已由 J3 覆盖 PNG/BMP/GIF/JPEG） | DIV-GFX-024 partial |
| 其它图像加工 | ROTATION / TRIM / MONO / BLUR 像素证据 | 逐动作取证 |
| MID / 多部件 / FX | 掩码 / 几何 / 仿射公式取证 | REA + 微场景 |
| 颜色空间 / 半像素 / 过滤 | 官方截图逐像素 + 引擎设置 | 需 oracle |
| `yu-rt` 剩余 | 16 个 open DIV、27 条未取证命令、系统变量缺口 | 手册 / REA 取证 |
| `yu-trace` | DIV-L-002 任务相位；官方 args 级 L1 | 探针扩展 |
| 宿主 | 桌面 / wasm 收尾（K2）；Android | K2 |
| `yu-sound` / `yu-save` | 新 crate + 证据 | 未开始 |
| 格式兼容 | YSTB v2、NUL-XOR 方言 | 仅按目标游戏需要 |

## 相关仓库

- [NotFaceGUI/yu-emote](https://github.com/NotFaceGUI/yu-emote) — E-mote
  (PSB/EMT) 数据层与运行时,以 **git submodule** 挂在 `yu-emote/`
  (公开仓库;M1 数据层已完成,台账见 `docs/emote-parity.md`,任务书见
  `docs/tasks/task-emote-runtime.md`)。

## 许可

GPL-3.0-or-later。净室实现；不包含原引擎代码或游戏数据。
