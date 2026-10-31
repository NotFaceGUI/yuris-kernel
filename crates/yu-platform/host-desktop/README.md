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

## 帧步进器(debug-ui)

眨眼这类缺陷只出现在中间某几帧,实时跑按不住。步进器把「停住 → 定步
推进 → 把这一帧的全部证据落盘」做成宿主能力(实现见 `src/stepper.rs`)。

**两个暂停是两条独立通路**,任一为真引擎就不 tick(逻辑帧与逻辑时钟都
不走),面板分别显示 `LIVE`/`STEP`/`HALT`:

| 暂停 | 触发 | 语义 |
|---|---|---|
| 手动暂停 | 面板 / F5 / `--paused` | 「先别动」,随时可用 |
| 录制暂停 | 点「录制」(F8)自动进入 | 「要取现场但还没到那一刻」 |

| 快捷键 | 作用 |
|---|---|
| F5 | 暂停 / 恢复 |
| F6 | 推进 N 帧(默认 10,面板可改或 `--step-frames N`) |
| F7 | 推进 1 帧 |
| F8 | 录制开关(自动暂停) |
| F9 | 抓取:环形缓冲 → 磁盘 |

推进走**固定步长**(每帧 `1000/fps` 逻辑毫秒),所以同一脚本在任何机器上
的第 N 帧是同一个画面;从暂停恢复实时会重置实时时钟,暂停期间的墙钟不会
一次性灌进下一帧。

命令行自动化:

```bash
# 定步推进到逻辑帧 348 后停住,逐帧取证
target/release/yu-cli run "<project>" --mode desktop --backend wgpu \
  --debug-ui --step-to 348 --capture-dir target/tmp/blink

# 实时跑并录制(内存环形缓冲),F9 或退出时回溯落盘
target/release/yu-cli run "<project>" --mode desktop --backend wgpu \
  --debug-ui --record
```

产物:每帧一行 `frames.jsonl`(帧报告 / 渲染报告与跳过项 / gfx 诊断含脚本
pc / 输入按下沿 / 声音层 / op 目标分布 / GPU 占用 / 渲染阶段微秒 / 屏幕
指纹)+ 整屏 PNG + `session.json`。`engine_frame`(每次 tick +1)与
`gfx_frame`(合成帧序号)分开记,两者不保证逐帧相等。

## 上屏

`yu-cli::Engine` 产出 RGBA 帧;本 crate 上传到 `Rgba8Unorm` 纹理并按源画面
比例 `set_viewport` 上屏(多余区域黑边,letterbox/pillarbox)。鼠标坐标用同一
内容矩形换算。GPU 设备复用 `Engine::wgpu_renderer()` 的 instance/device/queue。

## 音频

不是本 crate 的职责:`yu-cli` 的 `SoundSink` 用 cpal 回调直接锁
`Arc<Mutex<SoundState>>` 渲染(音频线程是时钟),UI 线程阻塞/拖窗不影响声音。
