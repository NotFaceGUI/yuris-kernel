# yu-host-desktop

Windows 原生窗口宿主(后续可跨桌面平台):**winit 0.30** 开窗,**wgpu 30** 上屏,
**egui 0.37** 调试面板。引擎语义仍全部在 `yu-rt`/`yu-gfx`/后端;本 crate 只做
窗口、输入转发、呈现和诊断。

## 运行

```bash
cargo build --release -p yu-host-desktop
target/release/yu-desktop.exe "<project>" --backend wgpu --debug-ui
```

`--debug-ui` 启动即开面板(F1 开关);不要加 `--no-sound`(默认有声)。
`--frames N --shot out.png` 用于无头验收。

## 上屏

`yu-cli::Engine` 产出 RGBA 帧;本 crate 上传到 `Rgba8Unorm` 纹理并按源画面
比例 `set_viewport` 上屏(多余区域黑边,letterbox/pillarbox)。鼠标坐标用同一
内容矩形换算。GPU 设备复用 `Engine::wgpu_renderer()` 的 instance/device/queue。

## 音频

不是本 crate 的职责:`yu-cli` 的 `SoundSink` 用 cpal 回调直接锁
`Arc<Mutex<SoundState>>` 渲染(音频线程是时钟),UI 线程阻塞/拖窗不影响声音。
