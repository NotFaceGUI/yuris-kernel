# yu-movie-plmpeg

`MOVIE` 播放后端:内嵌纯软件 MPEG-1 解码(pl_mpeg 的 Rust 移植),实现
`yu-rt::event::MovieHost`。桌面与 Android 同构,不依赖系统媒体栈。

## 为什么内嵌

官方 `MOVIE` 把解码交给 MCI / DirectShow,但商业游戏与手机不能假设用户
装了某个播放器。业界对「只播 MPEG-1」的惯用解法是小型专用解码器
(pl_mpeg / libmpeg2),不是几十 MB 的 FFmpeg。本 crate 用
[`southstar-codecs::plmpeg`](https://crates.io/crates/southstar-codecs)
(上游 pl_mpeg,MIT,纯 Rust,`#![forbid(unsafe_code)]`),一个依赖同时提供:

- MPEG-PS 拆包(pack / system / PES 头,含 PTS 方言);
- MPEG-1 视频解码;
- **MP2 音频解码**。

## 已接入

- **视频**:`MOVIE[FILE]` 装载后由 `frame()` 按播放时钟推进;暂停保留最后
  一帧,直到 `MOVIEEND`。几何(位置/缩放)与音量由内核保存,宿主只消费。
- **音频**:`take_audio()` 交付交错 PCM;`yu-sound::movie` 经环形缓冲、
  线性重采样与官方 COM 音量曲线混音,桌面 cpal / Android cpal→oboe 同一
  路径。`has_audio()` / `audio_rate()` / `audio_packets()` 供诊断。
- **CODEC 探测**:`1` = MPEG-1 可直接播;`2` / `0xFF`(MPEG-2、AVI 等)
  由内核诊断说明,官方对 `>1` 也只是警告。

## 限制(全部挂号,不近似)

| 项 | 现状 |
|---|---|
| 非 MPEG-1 | MPEG-2 / AVI / MP4 / FLASH 只探测并警告,需要新 `MovieHost` |
| AVI 作为 CG 图层 | `CG[FILE]` 目前只解静态图 |
| `MOVIE[PLAY=0]` | 官方 MCI `stop` 回起点;当前视频冻结末帧、音频保留队列 |
| 影片时钟 | 视频走逻辑帧时钟、音频走设备实时时钟;桌面 <60fps 有漂移 |
| 整段读入 | VM 与 `Plm` 各持一份影片字节,大影片 Android 峰值内存高 |
| Android 真机试听 | APK 已交叉编译通过,听感验证待做 |

细节与后续项:`docs/todo-inventory.md` §3.3、`docs/tasks/task-b16-movie-audio.md`。

## 验证

```bash
cargo test -p yu-movie-plmpeg
YURIS_REQUIRE_SAMPLES=1 YURIS_SAMPLE_MOVIE_MP2=<sample.mpg> \
  cargo test -p yu-movie-plmpeg
```

缺样本时显式 `SKIP` 并计数;`YURIS_REQUIRE_SAMPLES=1` 改为失败。

证据:`docs/divergences/DIV-GFX-074-movie-family.md`、
`docs/tasks/task-b14-movie-schedule.md`、`PROGRESS.md` 成果 MOVIE-MACRO。
