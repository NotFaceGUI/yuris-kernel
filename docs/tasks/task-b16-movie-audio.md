# 任务书 B16 · 影片音频(MP2 → 声音域 → 平台输出)

> 状态:**已实现(2026-11-11)**。范围 = `MOVIE` 全屏影片的 MP2 音轨;
> 继续沿用 B14 的 `MovieHost` 端口与 `yu-sound` 混音,不新增平台专属代码。
>
> 仓库:`<repo>`。实现:
> `yu-rt/src/event.rs`(`MovieHost::take_audio`)、
> `yu-platform/movie-plmpeg/src/lib.rs`(MP2 解码)、
> `yu-sound/src/movie.rs`(环形缓冲/重采样/混音)、
> `yu-sound/src/{mixer,state}.rs`、`yu-engine/src/engine.rs`(每 tick 交付)。

## 1. 官方语义(0.495 `yu-ris.exe` sha256 `4f5eefa3…138f`)

| 事实 | 锚点 |
|---|---|
| `MOVIE[VOLUME=n]` 存 `0x5b48a0`;COM 路径 `IBasicAudio::put_Volume` | `0x44b329..0x44b35a`;`0x44b664` 的 `[ecx+0x1c]` |
| COM 音量曲线:`n=0 → -10000`;否则 `round(4000*log10(n/256))`,钳 `[-10000,0]`(1/100 dB) ⇒ 振幅 `(n/256)^2` | `0x44b664`;常量 `0x58ee40=1/256`、`0x58ee48=40.0`、`0x58ee50=100.0` |
| MCI 回退:`volume to 1000*n/256` | `0x44b7cc`;格式串 `0x808578` |
| `MOVIE[PLAY=1]` 启动/继续;`PLAY=0` 停止(MCI `stop` / COM `Stop`,`0x463544`/`0x463558`) | `0x44b34e..0x44b3a0` |
| `MOVIEEND` 停止并重置音量 256 | `0x44ba0f` |
| 音频由系统媒体栈与画面一起播放(MPEGVideo/MCI 或 COM) | DIV-GFX-074 §2.4 |

内核与声音域统一采用 **COM/DirectSound 曲线**(`yu-sound::volume_gain`:
`(min(max(v,0),256)/256)^2`),与 SOUND 层同一条增益口径;MCI 线性回退
在现代 Windows 不是默认路径,差异记录在此。

## 2. 分层实现

```text
MP2 packet(pl_mpeg) → PlmMovieHost.audio_out(i16 交错)
  → Engine::sync_movie_audio(每 tick,复用 buffer)
  → SoundState::movie_audio_push(环形缓冲 2^18 样本,约 2.7s@48k)
  → mixer 线程/设备回调 render_and_advance → 平台 AudioOut
```

- `MovieHost::take_audio(&mut Vec<i16>) -> Option<MovieAudioInfo>`:
  默认 `None`;`PlmMovieHost` 返回 `sample_rate`/`channels=2`。
- `PlmMovieHost`:`has_headers()` 后读 `num_audio_streams()`/`samplerate()`;
  `frame(now_ms)` 在推进视频后调用 `pump_audio(target)`:
  **提前 200ms**、每 tick 最多 32 包(MP2 一包 1152 帧 = 24ms@48k)。
  `decode_audio` 与 `decode_video` 共用 demux,视频包由 `Feed` 分流。
- `SoundState::MovieAudio`:交错 i16 环;`push` 分块写入、满了丢最旧整帧;
  `render` 线性插值到设备率、按 `volume_gain` 加进 f32 总线;
  音量 0 也继续消费,避免恢复时播放陈旧样本。
- `Engine::sync_movie_audio`:`MOVIE[FILE]` 变化/`MOVIEEND` 清队列;
  每 tick 下发 `playing`/`volume` 与 PCM。图形状态与音频解耦:
  `render=false` 时仍推进影片时钟与音频。

## 3. 跨平台

| 平台 | 出口 | 状态 |
|---|---|---|
| Windows/Linux/macOS | `host-desktop/src/sound_device.rs`(cpal,回调直接锁 `SoundState`) | ✅ 桌面实机试听(96kHz 设备,内部 48k→96k 重采样) |
| Android | `host-android/src/sound.rs`(cpal→oboe;mixer 线程→无锁 SPSC 环→回调) | ✅ `arm64-v8a + x86_64` APK 交叉编译通过(27MB);真机试听待做 |
| headless | 无 `AudioOut`;PCM 解码后由引擎丢弃(环有界) | ✅ |

无 Android 专属代码:影片音频在 `SoundState` 里混音,oboe 回调仍只做
无锁环拷贝。

## 4. 验证(原始结果)

```text
cargo build --workspace --all-targets                    → 0 error / 0 warning
cargo test -p yu-sound -p yu-movie-plmpeg -p yu-engine -p yu-rt --lib
  → 17 + 3 + 32 + 94 passed,0 failed
YURIS_SAMPLE_MOVIE_MP2=<trance_Ignis.mpg> \
  cargo test -p yu-movie-plmpeg                           → 3 passed
  (真实 1280×720 MPEG-1 + 48kHz 立体声 MP2;90 tick 交付 >0.8s 音频)
```

微基准(release):

```text
MP2 解码:1,282,176 帧 / 93.93ms = 73.3 ns/帧
  ⇒ 60Hz tick(48k 下 800 帧)≈ 0.059ms
MovieAudio::push 2s(48k,1152 帧/包)= 0.066ms ⇒ 0.0006ms/tick
MovieAudio::render 2s@96k(960 帧/块)= 0.245ms ⇒ 0.0012ms/块
```

同视频仅多音轨的 300 帧 bench:p95 54.64→54.70ms(噪声内);avg 差异
来自场景构建帧离群值,不是音频路径。

## 5. 审查记录(2026-11-11)

- **爆音(已修)**:混音总线是 f32 `[-1,1]`,影片 i16 必须 `/32768`;
  旧实现直接加原始 i16 → 削波成方波。`yu-sound::movie::render` 已归一化,
  单测断言 `1000/32768*gain`。
- **gfx 依赖(已修)**:`sync_movie_overlay` 原来在 `state=None` 时直接
  return,导致 `render=false` 时 `host.frame` 不推进、音频不产;现在先
  解码影片(音频),再按需投影画面。
- **尾部欠载计数(已修)**:插值相位把源耗尽时,剩余输出帧也计入
  `underrun_frames`。
- **分配/泄漏**:环 2^18 样本(512KB)首次有影片音频时惰性分配,
  `MOVIEEND` 后保留复用(有界缓存,非泄漏);引擎 `movie_audio_buf`
  每 tick 复用;host `audio_out` 由引擎每 tick 取走;无新增线程/句柄。
- **实时路径**:设备回调只做环读取 + 线性插值 + 加法,无分配/解码/锁外等待;
  解码在 tick 线程,不在回调。

## 6. 剩余待办(不阻塞本批)

| 项 | 说明 |
|---|---|
| 非 MPEG-1 编码 | MPEG-2/AVI/MP4 仍只探测并警告;需要新 `MovieHost`(FFmpeg 可选 feature) |
| AVI 作为 CG 图层 | 手册 `movie.html` 明示可用 CG;当前 CG 只解静态图 |
| `FLASH`/`FLASHINFO`/`FLASHEND` | 仅 `DomainCall`,无 SWF 宿主 |
| `MOVIEINFO[SX/SY]` 未指定 | 回 0,官方回影片原始尺寸(语料无此用法) |
| `MOVIE[PLAY=0]` | 官方 MCI `stop`(位置回起点);内核视频仍冻结在末帧,音频保留队列。语料无 `PLAY=0`,未对齐 |
| 影片时钟 | 视频用逻辑帧时钟、音频用设备实时时钟;桌面 <60fps 时有轻微漂移(2s OP 约 50ms) |
| 整段读入 | VM + `Plm` 各持一份影片字节;大影片 Android 峰值内存高 |
| Android 真机 | APK 已编译,未上设备试听 |

## 7. 推送前审查清单

- [x] 无逐命令/逐帧热路径改动(仅影片模式 tick 与音频回调)
- [x] 无每 tick 分配(movie_audio_buf/ring 复用)
- [x] 无无界增长(环丢最旧、host buffer 每 tick 取走、计数器 u64)
- [x] 音量曲线与官方 COM/DSBVOLUME 一致
- [x] 换片/`MOVIEEND` 清队列,不串片
- [x] `render=false` 下音频仍推进
- [x] Android 交叉编译通过
- [x] 新增回归:归一化/音量 0/重采样/欠载/MOVIEEND 清队列
