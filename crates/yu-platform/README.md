# yu-platform

平台宿主与平台端口。这里不是单个 crate,而是按"宿主/设备"拆开的五个包;
游戏语义仍全部在 `yu-engine` / `yu-rt` / `yu-gfx` / 后端,平台差异只允许
出现在端口实现。

| 路径 | crate | 定位 |
|---|---|---|
| [`host-desktop/`](host-desktop) | `yu-host-desktop` | winit + wgpu 窗口宿主、egui 调试面板、cpal 音频 |
| [`host-android/`](host-android) | `yu-host-android` | NDK cdylib + JNI 入口;wgpu 零拷贝上屏、oboe 音频 |
| [`host-wasm/`](host-wasm) | `yu-host-wasm` | 浏览器呈现端(调试链路,不是最终 wasm 包) |
| [`input/`](input) | `yu-platform-input` | 物理键码 / 指针 / 滚轮 → 引擎按钮名的唯一映射 |
| [`movie-plmpeg/`](movie-plmpeg) | `yu-movie-plmpeg` | `MOVIE` 播放后端(MPEG-1 PS + MP2),纯 Rust |

分层:

```text
yu-engine(唯一装配,含 AudioOut / HostPathOverrides / GPU 上下文等端口)
   ↑
平台宿主(窗口、设备、JNI、浏览器呈现端)
   ↑
端口实现(存档 fs / 音频出口 / 输入翻译 / MovieHost)
```

- 宿主**不解释脚本语义**,也不自行解析跨平台参数;取值表在
  `yu-log` / `yu-engine` / `yu-format` / `yu-render-core` /
  `yu-backend-wgpu` 各一处,宿主只传值。
- 同一份游戏数据在桌面与 Android 上走同一条读取链(A1/A2),存档字节
  逐字节一致。

跨平台矩阵与端口清单:`docs/cross-platform.md`;架构:`docs/architecture.md`
§2.5。
