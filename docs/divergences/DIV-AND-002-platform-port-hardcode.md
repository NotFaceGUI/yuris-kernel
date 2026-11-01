# DIV-AND-002 · 跨平台导出/文件系统/路径的硬编码收口

- 状态:`fixed(port)`(2026-11-01)
- 影响面:`yu-save`、`yu-engine`;桌面行为逐字节不变
- 关联:架构 §2.5.5(L1 能力端口)、A5(通用运行时零硬编码)

## 问题

推进 Android 目标时暴露三处「把 Windows 桌面写死进内核/装配层」的实现:

| 位置 | 原实现 | 后果 |
|---|---|---|
| `yu-save/src/state.rs` | 8 处直连 `std::fs`(read/write/read_dir/copy/remove_file/remove_dir/canonicalize) | 无文件系统的目标(浏览器)只能整段跳过;换存储(OPFS/DB)必须改状态机 |
| `yu-save/src/lib.rs` | `ensure_save_root` 用 `cfg(target_arch = "wasm32")` 做平台分支 | 新增目标就要加一条 cfg,是"按平台数量长出来的分支" |
| `yu-engine/src/engine.rs` | `host_paths` 硬编码 `USERPROFILE` + `\Documents` | Linux/macOS 没有该变量 ⇒ 注入空;Android 更是没有用户主目录概念 ⇒ 脚本拼路径的支路失效 |

此外 `yu-sound/src/acm.rs` 的 `build_pcm` / `attach` / `display` 未按
`cfg(windows)` 门控,非 Windows 目标**编译失败**(不是行为差异,是构建
中断)——直到真正构建 `aarch64-linux-android` 才暴露。

## 处置

1. **`yu-save::SaveFs` 端口**(`crates/yu-save/src/fs.rs`):
   `read/read_prefix/write/list/copy/remove_file/remove_dir/create_dir_all/
   exists/canonicalize`。默认实现 `NativeSaveFs` = 旧 `std::fs` 行为
   (逐语义对齐,含 `ErrorKind::NotFound` 的幂等删除判据)。`SaveState::new`
   仍走原生端口,新增 `SaveState::with_fs` 供宿主注入。
2. **`ensure_save_root_with`**:物化目录走端口,不再有 `cfg(target_arch)`。
3. **`yu_engine::host_paths` 模块**:`PlatformHome`(平台默认解析:
   Windows `USERPROFILE`→`HOMEDRIVE`+`HOMEPATH`,其余 `HOME`)+
   `HostPathOverrides`(宿主显式覆盖:system/appdata/personal/project)。
   `EngineOptions::path_overrides` 是注入口;Android 宿主用 app 私有目录填。
4. **`acm` 门控修正**:`build_pcm`/`attach`/`display` 加 `cfg(windows)`。

## 能力等价性

- 桌面:路径输出与旧实现**逐字节一致**(正斜杠 + 目录后缀 `/`;
  `$_PROJECT_PATH` 仍为 `None`,手册无页不猜)。
- Android:`$_SYSTEM_PATH`/`$_SAVE_PATH` 是真实绝对路径,`$_APPDATA_PATH`/
  `$_PERSONAL_PATH` 由宿主给 app 私有目录 —— 脚本拼路径的所有支路可用,
  与官方 Windows 语义同构(官方也是"路径字符串拼接")。
- 存档字节:`.sd` 编解码完全没动,平台间同构。

## 验证

- `cargo test -p yu-save` 18 项全绿(含官方语料 `seld.sd` / `save00001.sd`
  读取与 flush→load roundtrip)。
- `cargo test -p yu-host-android --test session` 用真实 31.5MB 语料跑 12 帧。
- `cargo build -p yu-host-android --target {aarch64,x86_64}-linux-android
  --release` 通过(15MB / 14MB so)。
- 全仓 `cargo test --workspace` 全绿。

## 残留

- `yu-vfs::YpfSource::open` 仍接受 `impl AsRef<Path>`(封包必须落在文件系统上)。
  浏览器目标需要 `from_bytes` 内存构造 —— 那条路径由 wasm 分支负责,不在本轮。
- `EngineOptions::font_bytes` 目前对**两个**渲染后端都注入;若后续引入第三个
  后端,须同步接线(已在 `Renderer::add_font_bytes` 集中,加后端时编译器会报)。
