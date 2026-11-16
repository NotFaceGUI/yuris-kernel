# yu-host-ios

iOS 宿主骨架:C ABI + 复用平台中立会话核心。与 Android / 桌面共用同一份
`yu-engine` 装配;本 crate 不解释游戏语义,只做会话管理与平台出口。

> **现状(2026-11-16)**:`soft` 后端完整会话可用(启动 / tick / RGBA 帧 /
> 输入 / DIALOG / 挂起恢复 / 存档);Metal 零拷贝上屏**尚未接线**,
> `yu_ios_attach_layer` 固定返回 `-3`。C ABI 见
> [`include/yu_ios.h`](include/yu_ios.h),Swift 壳见 [`ios/`](../../../ios/)。

## 为什么能复用 Android 宿主

`yu-host-android` 的 JNI 导出(`src/lib.rs` 的 `jni_entry`)只是薄壳;
真正的跨平台逻辑是 `init_session` / `start_session_json` / `tick` /
`frame_rgba` / `set_input` / `set_cursor` / `suspend` / `resume` /
`pending_dialogs_json` / `answer_dialog` / `probe_game_root` /
`list_font_families` 等公开 Rust 函数。该 crate 的
[`tests/session.rs`](../host-android/tests/session.rs) 在开发机上直接对真实
语料跑这条链路,不依赖 Android 设备。

本 crate 目前以 `yu-host-android` 为**临时桥**;等平台中立会话抽成
`yu-host-mobile` 后,本 crate 改为依赖公共核心,C ABI 与 Swift 壳不变。

## C ABI

| 函数 | 作用 |
|---|---|
| `yu_ios_init_json(json)` | 启动会话(JSON 键与 Android `nativeInitJson` 相同) |
| `yu_ios_tick()` | 推进一帧;`0` 继续 / `1` 游戏结束 / `4` 宿主销毁 / `<0` 失败 |
| `yu_ios_frame_size(&w, &h)` | 当前帧尺寸 |
| `yu_ios_copy_frame(out, cap)` | 拉 RGBA(行优先,4 字节/像素);返回字节数 |
| `yu_ios_set_input(button, pressed)` | 输入(`MOUSE_L` / `MOUSE_R` / `KEY_ENTER` …) |
| `yu_ios_set_cursor(x, y)` | 光标(游戏视口像素) |
| `yu_ios_suspend/resume()` | 生命周期 |
| `yu_ios_pending_dialogs(out, cap)` | 待答 DIALOG 的 JSON;空串 = 没有 |
| `yu_ios_answer_dialog(id, button, text)` | 回答 DIALOG |
| `yu_ios_probe_root(path, out, cap)` | 探测数据入口(`Project::discover` 同一判据) |
| `yu_ios_status/list_fonts/last_error(...)` | 诊断 |
| `yu_ios_attach_layer(layer, w, h)` | **预留** Metal 上屏;当前 `-3` |

字符串约定:入参 UTF-8 + NUL;出参由调用方给缓冲,函数返回所需字节数
(含 NUL),缓冲不足不写。不需要 free。

## 线程模型

与 `host-android` 相同:会话状态在 `thread_local`,`Engine` 是
`Rc`/`RefCell`。除 `suspend` / `resume` / `pending_dialogs_json` /
`answer_dialog` / `shutdown` 登记外,所有调用必须在**同一条 OS 线程**。
Swift 骨架先放主线程;后续可改为专用渲染线程。

## 数据与存档

iOS 是沙箱:游戏数据由用户经「文件」App / Finder 放进 app 的
`Documents/`;存档放 `Library/Application Support/...`,不跟游戏数据混在
一起。`$_SYSTEM_PATH` 由宿主显式指 `data_root`(不按存档根父目录推导),
所以存档搬出游戏目录不会让脚本找不到游戏文件。

## 构建与验证

Windows / Linux(不需要 Mac):

```bash
cargo build -p yu-host-ios --all-targets
YURIS_TEST_TMP=<可写目录> \
YURIS_SAMPLE_ERIS_ROOT=<含 data.ypf 的游戏根> \
  cargo test -p yu-host-ios -- --test-threads=1 --nocapture
```

macOS:

```bash
bash tools/ios/build.sh                      # device + simulator 静态库
YURIS_IOS_XCODE=1 bash tools/ios/build.sh    # 再生成 Xcode 工程并构建模拟器 app
```

## Metal 上屏:接线步骤(待 Mac)

1. `host-android` 抽 `Presenter` 端口:`size` / `present`,把
   `Session.presenter` 改为 `Option<Box<dyn Presenter>>`,
   `attach_surface` 泛化为 `attach_surface<P: Presenter + 'static>`。
   Android 的 `NativePresenter` 实现该 trait,行为零变化。
2. 本 crate 实现 `CaPresenter::new(layer: *mut c_void, gpu: &GpuContext)`:
   `wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer)`(wgpu 30
   `src/api/surface.rs:430`,默认 `metal` feature 开启)。
3. blit 管线与 `present.wgsl` 同源;建议把 Android `present.rs` 的管线/
   采样器/绑定组抽成共享的 `present-blit`。
4. 验证:同一工程首 N 帧与 `soft` 逐字节 hash 对齐,再比较帧率/功耗。

## 证据

- `docs/architecture.md` §2.5:新平台 = 新宿主 + 端口实现,内核零改动。
- wgpu 30 `SurfaceTargetUnsafe::CoreAnimationLayer`;cpal 0.15 iOS
  CoreAudio 后端;fontdb 0.24 `target_os = "ios"` CoreText 分支。
- `PROGRESS.md` 成果 IOS-SCAFFOLD。
