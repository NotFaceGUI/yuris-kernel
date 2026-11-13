# yu-host-android

Android 宿主:NDK `cdylib` + JNI 入口。与桌面 / 浏览器宿主共用同一份
`yu-engine` 装配;本 crate 只做会话管理、上屏、音频设备与输入转发。

Java 壳的构建、目录选择与实机验证见 [`android/README.md`](../../../android/README.md);
so 的 JSON 配置与 ABI 见 [`android/NATIVE_API.md`](../../../android/NATIVE_API.md)。

## 入口

公开 Rust API(`init_session` / `tick` / `frame_rgba` / `set_input` /
`shutdown` 等)可在开发机上直接测试;`src/lib.rs` 的 `jni_entry` 模块里
`Java_org_yuris_kernel_NativeHost_native*` 只是薄壳。

```text
nativeInitJson(json)      → RunConfig → Project::discover + Engine::new
nativeTick()              → Engine::tick_realtime();0 继续 / 1 游戏结束 /
                            4 宿主销毁请求 / -1 未初始化 / -2 失败
nativeFrameSize()         → [w, h]
nativeCopyFrame(int[])    → RGBA → ARGB_8888(CPU 回退路径)
nativeAttachSurface(...)  → wgpu 直接呈现到 ANativeWindow(零拷贝路径)
nativeSetInput(...)       → Engine::set_input
nativeSetCursor(x, y)     → Engine::set_cursor
nativeSuspend/Resume()    → 生命周期挂起/恢复
nativeLifecycle()         → 跨平台退出信号
nativeStatus/Version()    → 诊断摘要 / 构建目标标记
nativeShutdown()          → drop Engine
```

## 渲染

- **壳固定走 `wgpu`**:`TextureView` 的 Surface 经 `ANativeWindow_fromSurface`
  交给 wgpu,同 device 直接采样上屏,免掉 GPU→CPU 读回与重上传。
  壳内 CPU 按钮已停用;单次启动 GPU 不可用时显式失败、不静默降级,
  失败返回配置界面后由壳把重试参数切到 `soft`。
- `soft` 仍是 so 的默认后端(不传 `backend` 时),也是开发机会话测试与
  桌面参考路径;它把 RGBA 交给 Java 侧画 Bitmap。
- 线程模型:`Engine` 内部是 `Rc`/`RefCell`,除 `nativeShutdown` /
  `nativeResizeSurface` / `nativeSuspend` / `nativeResume`(只登记原子请求,
  可从任意线程调用)外,其余 `native*` 必须在**同一渲染线程**调用;
  会话状态放在 `thread_local`。

## 音频

`src/sound.rs` 是交付层,不解释声音语义:

```text
混音线程(THREAD_PRIORITY_AUDIO)     oboe 回调(实时线程)
  lock(SoundState) → render_and_advance   pop(无锁 SPSC 环) → 设备
  → push(无锁 SPSC 环)                    欠载补静音,绝不阻塞
```

- 回调零锁、零分配、零解码;解码与混音在 tick / mixer 侧。
- `short`(交付欠载)/ `rtsil`(混音源未就绪)/ `clipped`(削波)分开计数。
- 后台挂起经进程级闸门立即生效:设备回调补静音、mixer 停取数并清空环,
  不依赖渲染线程下一帧。
- 流监督者每 300ms 检查设备/采样率,AAudio 断开后重建(否则永久静音)。

## 输入与生命周期

- 触摸 = `MOUSE_L`;双指上下滑 = 滚轮;虚拟光标模式由壳的启动设置决定,
  游戏内切换只作用于本次会话。
- `onPause` → `nativeSuspend()`;`onResume` → `nativeResume()`;
  `nativeTick()` 返回 `1`(主脚本走完或脚本 `END`)表示游戏结束,壳据此
  回配置界面;`nativeLifecycle()` 给出细分代号(1 走完 / 2 END / 3 END+RESTART)。

## 验证

```bash
cargo test -p yu-host-android
cargo check -p yu-host-android --target aarch64-linux-android
BUILD_TYPE=release tools/android/build.sh arm64-v8a x86_64
```

`tests/session.rs` 在开发机上用真实语料跑通「读取 → tick → 帧 / 输入 /
存档」整条链路;缺样本显式跳过并计数。

证据:`docs/cross-platform.md`、`docs/divergences/DIV-AND-*.md`、
`PROGRESS.md` 成果 AND-SHELL / AND-PERF-AUDIT。
