# DIV-AND-004 · 宿主参数各自的取值解析重复,`@_TIME` 在 Android 恒 1970

- 状态:`fixed(unify)`(2026-11-07)
- 影响面:`yu-log`、`yu-format`、`yu-render/wgpu`、`yu-engine`、`yu-cli`、
  `yu-platform/host-android`;桌面像素/字节行为不变
- 关联:DIV-AND-002(跨平台硬编码收口)、`docs/cross-platform.md`

## 问题

DIV-AND-002 收口了**文件系统与路径**,但「宿主参数」这一层仍是每个宿主各写
一份。同一组用户可见取值在 2~4 处各有一套解析,取值集合与错误文案都漂移了:

| 参数 | 重复位置 | 漂移后果 |
|---|---|---|
| `log_level` | CLI `--log-level`、`MinLevel::from_level_value`、`FacadeLevel::parse`、Android `run-config.json` | Android 拒收 `debug`/`trace`(CLI 接受);`verbose` 在门面侧无对应项 ⇒ 落回 `warn`,**开了 verbose 依赖日志反而更少** |
| `backend` | `Backend::parse`、Android 本地 match、3 个 host-desktop 示例 | Android 侧先自己 `lower()` 再比;别名集各自增减 |
| `gpu_backend` | 引擎 `YURIS_WGPU_BACKEND`(`dx12\|vulkan\|gl\|primary\|all`)、Android 配置(`auto\|gles\|vulkan`) | 两套词表,同一概念两处维护 |
| `save_codec` | `yu-cli run`、`yu-cli desktop`、Android `RunConfig` | 三段逐字相同的 match |
| `script_key` | `yu-cli::run::parse_script_key`、Android `parse_script_key` | 两段逐字相同的清洗+校验 |
| `on|off` 旗标 | `ArgReader::on_off`、`logging::parse_on_off` | 两份实现,一份带 `trim` 一份不带 |
| `@_TIME` | 只有 `yu-cli::clock::local_now` | **Android 宿主从未调用** ⇒ `@_TIME` 恒 1970,存档菜单日期永远 1970-01-01(脚本可见差异,且不报错) |

`@_TIME` 这条是本档的核心:**它不是"重复",而是"漏了一个宿主"**。根因是
"读本地时间"被放在 CLI 而不是装配层,于是每个宿主必须自己记得接一次线。

## 处置

全部收成**单一实现**,宿主只负责把值传进来:

1. `yu_log::LogLevel`(`crates/yu-log/src/level.rs`):`quiet|error|warn|info|debug|trace|verbose`
   的唯一解析;`min_level()` 给内核 `Diag` 门槛,`facade_level()` 给依赖门面桥。
   `MinLevel::from_level_value` 与 `facade::level_from_env` 都转发到它。
2. `BlockCodec::parse/as_str`(`yu-format/src/yssd.rs`):`zlib|snappy`。
3. `GpuBackend`(`yu-render/wgpu/src/device.rs`):引擎环境变量与 Android 配置的
   并集词表 + `backends()`/`ladder()`;`auto` 的回退顺序由 `cfg(target_os)`
   在**一处**决定(Android = GLES→Vulkan,其余 = `PRIMARY`)。
4. `Backend::parse` 统一大小写/空白容错;三个 host-desktop 示例改调它。
5. `ScriptKey::parse_hex`(`yu-engine/src/keys.rs`):CLI 与 Android 共用。
6. `yu_cli::args::{parse_on_off_flag, save_codec_flag}`:CLI 内部去重。
7. `yu_engine::host_clock::local_now`(`crates/yu-engine/src/host_clock.rs`):
   本地时间读取上移到装配层;`yu-cli::clock` 退化为转发,Android `start_engine`
   显式注入 `local_time`。wasm 目标恒 `None`(浏览器无进程级本地时区 API)。

Android `RunConfig` 的 `backend`/`gpu_backend`/`save_codec`/`log_level` 由
`String` 改为枚举,解析发生在 JSON 边界,下游不再处理字符串。

## 行为变化(有意)

| 项 | 旧 | 新 |
|---|---|---|
| `--log-level verbose` 的依赖日志 | 落回 `warn`(比缺省 `info` 更紧) | 到 `Trace`(只多不少) |
| Android `log_level` 接受集 | 无 `debug`/`trace` | 与 CLI 完全相同 |
| Android `log_level=debug/trace` | 不退出发布模式 | 与 `verbose` 一样退出 `quiet` |
| Android `@_TIME` | 恒 1970 | 系统本地时间 |
| `--backend` 大小写 | 严格小写 | 大小写/空白容错 |

`quiet`/`error`/`warn` 仍是 `MinLevel::Quiet`(内核不分这三档),门面各自
放行 —— 这是有意的两档语义差,已在 `level.rs` 模块文档写明。

## 验证

```text
cargo test -p yu-log --lib          → 33 passed(新增 4 条 level 语义)
cargo test -p yu-format --lib       → 35 passed(新增 BlockCodec 往返)
cargo test -p yu-engine --lib       → 31 passed(新增 script_key 7 种写法)
cargo test -p yu-backend-wgpu --lib → 13 passed(新增 GpuBackend 词表/回退链)
cargo test -p yu-host-android --lib → 13 passed(新增取值集对齐 3 条)
cargo check -p yu-host-android --target aarch64-linux-android → 通过
cargo check -p yu-host-wasm --target wasm32-unknown-unknown    → 通过
cargo build --workspace --all-targets                          → 通过
三帧 headless(project-eris)
  → rgba sha256 a7aff0f8…d67e334e,与改动前逐字节一致
```

全量 `cargo test --workspace` 剩余失败均为既有/环境性(见 PROGRESS):

- `yu-gfx eris_first_screen` ×2(golden 199 vs 272)—— stash 对照复现;
- `yu-sound gated_ogg` ×2 —— 单独带样本 env 跑全绿,并行时争用;
- `yu-backend-wgpu diffusion_matches_soft_across_sizes` —— 单跑通过(GPU 争用)。

## 残留

- `yu-vfs` 的 `DirSource`/`YpfSource` 仍走 `std::fs`(DIV-AND-002 已说明:
  数据源定义就是文件系统/封包,浏览器目标由内存构造)。
- 浏览器目标 `local_now()` 恒 `None`;真 wasm 宿主接入时需经 JS
  `Intl.DateTimeFormat` 反查时区,**在 `host_clock` 内补**,不要回到各宿主。
- `EngineOptions` 仍有 `with_icon`/`warm_text` 这类平台策略位;它们不是
  "同一取值的多处解析",而是"宿主能力开关",保持原位。
