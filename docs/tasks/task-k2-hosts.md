# 任务书 K2 · hosts:`yu-cli`(桌面/dev 服务) + `yu-host-wasm`(浏览器宿主 + HMR)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`PROGRESS.md` 最新成果、
> `docs/architecture.md` §2/§5、`crates/yu-backend-soft/examples/first_frame.rs`
> (启动样板)。
>
> 独占新建:`crates/yu-cli`、`crates/yu-host-wasm`、`www/`、`cases/`、
> `scripts/`(仅新增)。**只允许对 `crates/yu-vfs` 做加法式扩展**
> (`YpfSource::from_bytes`);其余 crate 只读消费。`yu-rt` / `yu-gfx` /
> `yu-backend-*` 正被其它会话占用,需要改它们 ⇒ 停下写报告。
> 零 git 写操作;`PROGRESS.md` 只追加自己的“成果 K1…”。

## 0. 架构定位(按现有分层,不发明新层)

```text
后端(共享):  yu-backend-soft / yu-backend-wgpu     ← 只执行 CompositorFrame
宿主(平台):  yu-cli        = 桌面宿主 + dev 服务
             yu-host-wasm = 浏览器宿主(wasm-bindgen)
             (Android 后续)
```

- 宿主只做:窗口/canvas、输入、时钟、数据装载(HTTP/OPFS)、热重启编排;
  **不得解释渲染语义**。
- 宿主不得被任何 kernel crate 依赖(与 `yu-trace` 同级的旁挂地位)。
- 本任务书里的“热重启”定义为:**修改 Rust 源码 → 重建 wasm → 浏览器自动
  加载新版本并重新启动游戏**;不是进程内保留状态的快进。

## 0.1 硬性总则

1. 只消费公开 API;发现契约缺口先写报告,不打补丁进别人的 crate。
2. 性能阈值**先声明后测量**:P0 量出基线写进 README,后续验收对照,
   不许测完再挑数。
3. 错误统一走 `yu-log::Diag`;`src/` 零 `println!/eprintln!/dbg!`
   (`yu-cli` 作为终端工具的产品输出走专用输出模块,不算违规;
   `yu-host-wasm` 日志走 `console` 接口)。
4. 被跟踪文件一律占位符(`<repo>`/`<side-repo>`/`<project-eris>`);
   语料/截图/wasm 产物不入库;零 git 写操作。

## 0.2 当前事实基线(2026-10-21 实测,先复核再引用)

- 样本包 `data.ypf` = 31.5 MB,浏览器 fetch 进内存可行;
- 工具链已装:`wasm32-unknown-unknown`、`wasm-bindgen`、`wasm-pack`、
  `node`/`npm`;
- `cargo check -p yu-vfs --target wasm32-unknown-unknown` 已通过
  (缺的只是运行时入口 `YpfSource::from_bytes`);
- boot 只解析元数据表 + 入口脚本,137 个脚本懒加载
  (`TableSet::load_script`);首帧 tick 才是大头(初始化帧 129 万命令,
  `record_events=false` 是现成快路径);
- 跨重启可复用:`DataSource` 句柄、`TextureStore` 纹理缓存、
  `SoftRenderer`/`WgpuRenderer` 实例、`FontSource`;
  重启单元只有 `Vm` + `GfxState`;
- `BootConfig` 已有宿主注入字段:`screen_color` / `dx_mode` /
  `os_sound` / `media_readonly` / `paths` / `record_events`。

## 1. Phase P0 · 度量与声明(先做)

写 `yu-cli` 的 `dev --bench`(或临时 example)测:
- boot 元数据表 + 入口脚本耗时;
- 首帧 tick 耗时(`record_events` on/off 各一次);
- wasm release / debug 增量构建耗时;
- wgpu device 初始化耗时。

在 `crates/yu-cli/README.md` 声明阈值,后续验收对照:
- 原生热重启(re-boot 后可 tick)< 1s;
- wasm 增量构建 + browser reload 端到端 ≤ 15s(基线数字写实);
- 浏览器首帧(含 31.5MB fetch)≤ 5s。

## 2. Phase P1 · `yu-cli run`(桌面 headless 入口)

```text
cargo run -p yu-cli -- run <project-root> [--backend soft|wgpu] \
  [--entry LABEL] [--frames N] [--until-visible] \
  [--shot target/tmp/shot.png] [--json target/tmp/frame.json] \
  [--record-events on|off]
```

- 数据入口:封包 `data.ypf` 优先,散盘 `data/` 为备,哪个存在用哪个;
  这是 `first_frame` 示例的正式化,不是重写;
- `GfxHost` 同步路径;`BootConfig` 宿主字段显式给值并在帮助里说明;
- wgpu 模式沿用“两后端 skipped/executable 计数必须一致”的对账纪律,
  不一致 exit 2;缺样本 exit 3;正常 exit 0。

## 3. Phase P2 · `yu-cli dev`(wasm HMR 开发服务,本任务核心)

```text
cargo run -p yu-cli -- dev <project-root> \
  [--vite] [--port 4173] [--no-open] [--data <data.ypf|data-dir>]
```

流程:
1. 初次 `cargo build -p yu-host-wasm --target wasm32-unknown-unknown`
   (dev=debug;`--release` 可选)→ `wasm-bindgen --target web` → `www/pkg/`;
2. 静态服务 `www/`;`/data.ypf` 流式提供工程包(31.5 MB 本地);
3. 监视 `crates/**/*.rs`(notify 或 500ms 指纹轮询;排除 target,
   300ms 去抖)→ 变更后重建 + 重新 wasm-bindgen → 通过 **SSE `/events`**
   通知页面 `location.reload()`;
4. 页面 reload 后从 `localStorage` 恢复 `{project, entry, frame}`,
   自动快进到同一帧(建议实现:让“改完刷新还在原地”);
5. `--vite`:由 `www/package.json` 的 Vite 接管页面,提供
   `www/vite.config.ts` 插件(watch Rust → 触发构建 → `full-reload`);
   `yu dev` 只负责构建与 spawn。不引 React/Vue,保持原生 DOM,
   前端框架由使用者自行添加。

- 红线:核心开发流程**不依赖 Node**(Rust 静态服务器 + SSE 即可跑);
  Vite 是可选增强,缺 npm 时必须能降级运行。

**验收(自动化,不依赖浏览器)**:
- 修改 `crates/yu-host-wasm/src/lib.rs` 一个可观测字符串 → 断言
  ①wasm 产物 mtime/hash 变化 ②`/events` 收到 reload 事件
  ③端到端耗时 ≤ P0 阈值;连续 10 次修改无泄漏。

## 4. Phase P3 · `yu-host-wasm`(浏览器宿主)

- `crates/yu-host-wasm`(cdylib + wasm-bindgen):

```text
Host::new(ypf_bytes, opts) -> BootInfo{width,height,entry}
Host::tick()               -> FrameReport{frame,ops,skipped,executed,ms}
Host::render(canvas)       -> WebGPU(wgpu)优先;不可用回退 soft RGBA→ImageData
Host::restart()            -> 丢 Vm+GfxState;保留数据源/纹理/设备
Host::set_input(button, pressed)
```

- 数据装载:dev 走 `fetch('/data.ypf')` → `YpfSource::from_bytes`;
  生产预留 file-picker + OPFS(本次只留接口与 TODO);
- `www/index.html`:canvas + 状态浮层(frame/ops/skipped/ms/adapter)
  + Restart 按钮 + 输入转发(mousedown/up → `set_input`);
- 输入语义走 `InputTimeline`(真实按键),不许直接改变量;
- `yu-vfs` 加法:`YpfSource::from_bytes(Vec<u8>, Codec)`,解析逻辑复用,
  不拷贝实现;若 wasm 目标出现阻塞,记录并降级,不改解析语义。

**验收**:浏览器首帧渲染成功;Restart 后再次出首帧;状态浮层数字与
native `run` 的报告一致;`cargo check -p yu-host-wasm --target
wasm32-unknown-unknown` 通过。

## 5. Phase P4 · 测试用例(`yu-cli case`)

`cases/*.json`(格式进 README):

```json
{ "name":"first-screen", "entry":"SYSTEM_START", "frames":3,
  "until_visible":true,
  "inputs":[{"frame":2,"button":"CLICK","x":400,"y":300}],
  "expect":{"display_json_sha256":"…","png_sha256":"…",
            "ops_total":44,"skipped_ops_total":29} }
```

- `yu-cli case <root> --all|--case 名`;hash 不匹配 fail 并打印差分摘要;
  `--update` 重生成 expect 时打印 diff,人工审后入库;
- 首批用例:first-screen / until-visible / record-events-off 三条。

## 6. Phase P5 · 收尾

- `PROGRESS.md` 追加“成果 K1”:P0 阈值与实测、HMR 冒烟原始输出、
  浏览器验收清单、`yu-vfs` 契约变化(from_bytes);
- `crates/yu-cli/README.md` 写清 `run` / `dev` / `case` / Vite 用法;
- `cargo clippy -p yu-cli -p yu-host-wasm --all-targets` 0 新告警;
  `cargo test --workspace` 全绿;wasm 目标 check 通过;
- `.gitignore` 增补 `www/pkg/`、`www/node_modules/`、`target/tmp/` 下的
  截图与 wasm 产物;
- 被跟踪文件一律占位符。

## 7. 并行占用(与其它任务书的边界)

| 线 | 占用 | 内容 |
|---|---|---|
| J | `yu-gfx` + 双后端 | COPY / RECTPAINT / soft alpha |
| M | `crates/yu-rt/**` | 覆盖普查 + TEXT 族 |
| L | `tools/official_trace/**` + `yu-trace` 加法 | 官方 L1 |
| K2(本任务) | `yu-cli` / `yu-host-wasm` / `www` / `cases` / 加法 `yu-vfs` | 宿主 + HMR |

文件零交集;`PROGRESS.md` 各自写前重读文件尾,只追加自己的段落。

## 8. 环境

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<project-eris>"   # 数据入口:官方挂载规则
export YURIS_WGPU_BACKEND=dx12                   # 可选
```

## 9. 停止/上报条件

- wasm 需要 `yu-rt`/`yu-gfx`/后端改动 ⇒ 停,写报告;
- 浏览器 WebGPU 不可用且 soft 回退也阻塞 ⇒ 记录并给出降级清单;
- Node/Vite 引入后核心流程出现硬依赖 ⇒ 回退到 Rust 静态服务;
- 目标文件正被其它会话写入 ⇒ 停手注明。
