# 跨平台能力矩阵(2026-11-01)

> 上位文档:`docs/architecture.md` §2.5(v2 跨平台目标架构)。
> 本文件记录**实际落地**的可移植性状态,不是规划。

## 分层落位

```text
L0 内核(平台中立)      yu-rt / yu-gfx / yu-sound / yu-save / yu-format /
                        yu-vfs / yu-render-soft / yu-render-wgpu / yu-font
L1 能力端口             SaveFs(存档文件系统) · HostPaths(路径注入) ·
                        FontBytes(字体字节) · AudioOut(混音出口) · DataSource(数据树)
L2 唯一装配             yu-engine(Engine / EngineOptions / Project / boot / tick)
L3 宿主                 yu-cli(--mode desktop / headless) · host-android
                        · host-wasm(浏览器呈现端,调试链路)
```

## 能力矩阵

| 能力 | 桌面 | Android | 浏览器 wasm |
|---|---|---|---|
| 数据读取(ypf / 散盘) | 真实 fs | 真实 fs(app 私有 / 外置卡) | **待做**:需 `YpfSource::from_bytes` |
| 存档读 | 真实 fs | 真实 fs(app 私有目录) | **待做**:需 `SaveFs` 注入实现 |
| 存档写 | 真实 fs,字节级一致 | 真实 fs,字节级一致 | **待做**(同上) |
| 系统字体枚举 | `fontdb` 系统枚举 | `fontdb` 空操作 ⇒ 补 `/system/fonts` + `/product/fonts` | 无系统字体 |
| 宿主字体注入 | 可用 | 可用(`<data>/fonts/`) | **必需**(唯一来源) |
| 路径系统变量 | `USERPROFILE`/`HOME` | 宿主显式给(app 私有目录) | 宿主显式给 |
| 声音设备 | cpal | cpal(oboe 后端,已验证编译) | 暂不出声(架构决定) |
| 渲染 | soft / wgpu | soft(第一期) | 待做 |
| 输入 | winit → `yu-platform-input` | 触摸 → `MOUSE_L` | DOM 事件 → 同层 |

## 已收口的硬编码(见 DIV-AND-002)

| 原实现 | 现实现 |
|---|---|
| `SaveState` 内 8 处 `std::fs` 直连 | `SaveFs` 端口(`NativeSaveFs` 默认 = 旧行为) |
| `ensure_save_root` 的 `cfg(target_arch = "wasm32")` 分支 | `ensure_save_root_with(&dyn SaveFs)` |
| `host_paths` 硬编码 `USERPROFILE` + `\Documents` | `PlatformHome` + `HostPathOverrides` |
| `acm::build_pcm` 未按平台门控(非 Windows 编译中断) | `cfg(windows)` |

## 尚未端口化(有意保留)

| 位置 | 原因 |
|---|---|
| `yu-vfs::DirSource` / `YpfSource` 走 `std::fs` | 数据源**定义**就是文件系统/封包;浏览器目标由内存构造(`from_bytes` / `MemSource`),不是把 fs 抽象化 |
| `yu-font::FontSource::from_file` | 桌面/Android 专用;wasm 用 `from_bytes` |
| `yu-engine::read_save_path_type` / `detect_block_codec` | 读游戏根的 `yssfs.dat` / `YSSnp.dll` 信号;无 fs 的目标按缺失等价处理(值 = 0 / zlib),与官方"文件不存在"同义 |

## wasm 占位说明(本轮不实现)

浏览器目标需要三件事,均未做:

1. `yu-vfs::YpfSource::from_bytes(Vec<u8>, Codec)` —— 把 fetch 到的封包装成内存数据源;
2. `SaveFs` 的浏览器实现(OPFS / localStorage 同步桥),`SaveState::with_fs` 已留好注入口;
3. 字体字节注入路径(`EngineOptions::font_bytes` 已就绪,宿主侧需读文件选择器/内置字体)。

本轮**只预留**上述接缝(端口 + 注入口),不写浏览器实现。已有 wasm 分支的
实验改动独立存放,不在本分支。

## 验证入口

```bash
# 桌面全量
cargo test --workspace

# Android 宿主会话(真实语料,开发机可跑)
YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo test -p yu-host-android --test session -- --nocapture

# Android so 交叉编译
tools/android/build.sh arm64-v8a
```
