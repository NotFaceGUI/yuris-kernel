# yu-engine

唯一引擎装配(架构 v2,H1):数据入口发现、boot / tick / restart,以及宿主
公共件。所有宿主 —— `yu-cli` 的 headless / desktop、Android JNI、浏览器
serve —— 嵌的是**同一份装配**;平台差异只能经端口进入。

## 负责

| 模块 | 内容 |
|---|---|
| `project` | 按官方挂载规则发现数据入口:开发工程 `data/`、发行版根 `*.ypf` + `pac/*.ypf`;`update1..9.ypf` 增量层;散盘叠层;`data.ypf` 只是单包特例 |
| `engine` | `Engine::new` / `tick` / `tick_realtime` / `restart` / `suspend`;每帧 VM → gfx 同步 → 后端合成,soft / wgpu 两后端 |
| `capture` | `SAVE[ID]` / `LOAD[ID]` 图层像素通道:wgpu 命令时刻按需读回,soft 镜像快照;`LOAD` 写回合成资产叠层 |
| `identity` | 游戏名、YU-RIS 版本、窗口图标、内核版本;同一份事实给所有宿主 |
| `patch` | `PatchKind`(auto / original / gbk / big5 / tunnel)+ `PatchSelection`,跨平台唯一取值 |
| `host_paths` | `$_SYSTEM_PATH` 等路径变量:平台默认解析 + 宿主显式覆盖 |
| `host_clock` / `keys` | `@_TIME` 本地时间;脚本密钥自举与显式解析 |
| `json` / `output` / `sha` | 宿主公共件(最小 JSON 写出、终端输出、摘要);`yu-cli` 仍 re-export 保持兼容 |
| `lib` | `Failure` 与退出码 `0` / `2` / `3` |

## 不负责

- CLI 参数解析、HTTP / WebSocket、窗口与设备(cpal、winit、web-sys);
- VM 命令语义(`yu-rt`)、渲染语义(`yu-gfx`)、后端像素实现(`yu-backend-*`);
- 平台专有行为。宿主只经端口注入:`AudioOut`、`HostPathOverrides`、
  `EngineOptions::gpu_context` / `font_bytes` / `tuning` / `save_root` 等。

## 装配顺序

```text
Project::discover(工程根, None)
  → Vm::boot_with_aux(表 + 入口)
  → GfxState::from_tables
  → GfxHost / SoundHost / SaveHost 装进 VM
  → 每帧 Engine::tick[_realtime]:Vm::tick_at → 域宿主同步 → CompositorFrame → 后端
```

宿主侧最小用法:

```rust
use std::path::Path;
use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;

let project = Project::discover(Path::new(root), None)?;
let mut engine = Engine::new(project, EngineOptions {
    backend: Backend::Wgpu,
    ..Default::default()
})?;
let frame = engine.tick_realtime()?;
```

完整选项与默认值见 [`EngineOptions`](src/engine.rs) 的字段注释。

## 验证

```bash
cargo test -p yu-engine
cargo run -p yu-engine --example lifecycle_probe -- <project>
cargo run -p yu-engine --example identity_probe -- <project>
```

`examples/` 另有 `analyze_game` / `list_assets` / `dump_script` /
`probe_cmd_params` 等取证工具;缺样本时显式失败或跳过并计数,不空跑通过。

## 证据

- 架构与端口分层:`docs/architecture.md` §2.5、`docs/cross-platform.md`。
- 数据入口规则:`docs/formats/ypf.md` §4;细节以 [`project`](src/project.rs)
  模块文档为准。
- 进度:`PROGRESS.md` 成果 H1 / HOST-TUNING / AND-SHELL。
