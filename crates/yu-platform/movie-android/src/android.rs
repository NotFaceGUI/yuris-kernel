//! Android 系统解码后端的实现(仅 `cfg(target_os = "android")`)。
//!
//! 全部 NDK 调用集中在本模块;`lib.rs` 对外只暴露安全类型。资源用 RAII
//! 包装(`Extractor`/`Codec`/`Reader`/`TempFile`),`open` 中途失败时局部
//! 变量按逆序释放,不会泄漏 codec 或临时文件。

use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::raw::c_char;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::atomic::{AtomicU64, Ordering};

use ndk_sys::{
    media_status_t, AMediaCodec, AMediaCodecBufferInfo, AMediaCodec_configure,
    AMediaCodec_createDecoderByType, AMediaCodec_delete, AMediaCodec_dequeueInputBuffer,
    AMediaCodec_dequeueOutputBuffer, AMediaCodec_getInputBuffer, AMediaCodec_getOutputBuffer,
    AMediaCodec_getOutputFormat, AMediaCodec_queueInputBuffer, AMediaCodec_releaseOutputBuffer,
    AMediaCodec_start, AMediaExtractor, AMediaExtractor_advance, AMediaExtractor_delete,
    AMediaExtractor_getSampleFlags, AMediaExtractor_getSampleTime,
    AMediaExtractor_getSampleTrackIndex, AMediaExtractor_getTrackCount,
    AMediaExtractor_getTrackFormat, AMediaExtractor_new, AMediaExtractor_readSampleData,
    AMediaExtractor_selectTrack, AMediaExtractor_setDataSourceFd, AMediaFormat,
    AMediaFormat_delete, AMediaFormat_getFloat, AMediaFormat_getInt32, AMediaFormat_getString,
    AMediaFormat_setInt32, AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
    AMEDIACODEC_BUFFER_FLAG_KEY_FRAME, AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED,
    AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED, AMEDIACODEC_INFO_TRY_AGAIN_LATER,
};

use yu_log::{Boundary, Diag};
use yu_rt::event::{MovieAudioInfo, MovieFrame, MovieGeometry, MovieHost};

use crate::convert::{downmix_to_stereo, yuv420_buffer_to_rgba};
use crate::MovieCodecSupport;

/// NDK 格式键(硬编码 C 串,避免 `static mut` 的 API 级别问题)。
const K_MIME: &[u8] = b"mime\0";
const K_WIDTH: &[u8] = b"width\0";
const K_HEIGHT: &[u8] = b"height\0";
const K_FRAME_RATE: &[u8] = b"frame-rate\0";
const K_SAMPLE_RATE: &[u8] = b"sample-rate\0";
const K_CHANNEL_COUNT: &[u8] = b"channel-count\0";
const K_PCM_ENCODING: &[u8] = b"pcm-encoding\0";
const K_COLOR_FORMAT: &[u8] = b"color-format\0";
const K_STRIDE: &[u8] = b"stride\0";
const K_SLICE_HEIGHT: &[u8] = b"slice-height\0";
const K_CROP_LEFT: &[u8] = b"crop-left\0";
const K_CROP_TOP: &[u8] = b"crop-top\0";
const K_CROP_RIGHT: &[u8] = b"crop-right\0";
const K_CROP_BOTTOM: &[u8] = b"crop-bottom\0";

/// 外置后端可尝试的视频 MIME(顺序固定;探测与日志逐字节稳定)。
const VIDEO_MIMES: &[&str] = &[
    "video/avc",
    "video/hevc",
    "video/mp4v-es",
    "video/3gpp",
    "video/x-vnd.on2.vp8",
    "video/x-vnd.on2.vp9",
    "video/av01",
    "video/mpeg2",
];

/// 每 tick 最多喂给解码器的样本数(防一条慢影片把 tick 拖长)。
const FEED_BUDGET: usize = 24;
/// 解复用最多提前播放时钟多少秒(同时限制视频帧队列内存)。
const FEED_LEAD_SECONDS: f64 = 0.35;
/// 每 tick 最多排空的视频输出数。
const VIDEO_DRAIN_BUDGET: usize = 8;
/// 每 tick 最多排空的音频输出数(MP2/AAC 一包约 24ms)。
const AUDIO_DRAIN_BUDGET: usize = 16;
/// 视频帧队列上限(每帧已转 RGBA;800×600 约 1.9MB)。
const FRAME_QUEUE_LIMIT: usize = 12;
/// 视频帧队列的常驻字节预算;1080p 的 RGBA 一帧约 8.3MB,固定 12 帧
/// 会到 ~100MB,按分辨率把队列压到预算内。
const FRAME_QUEUE_BYTES: usize = 32 * 1024 * 1024;
/// 音频预解码提前量(秒),与内置 pl_mpeg 后端同一口径。
const AUDIO_LEAD_SECONDS: f64 = 0.20;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn fail(detail: impl Into<String>) -> Diag {
    Diag::recoverable("movie.decode", Boundary::Platform).with_detail(detail)
}

fn require_ptr<T>(ptr: *mut T, what: &str) -> Result<NonNull<T>, Diag> {
    NonNull::new(ptr).ok_or_else(|| fail(format!("{what} 创建失败")))
}

/// 临时文件守卫:drop 时删除(Android app 私有 cache 目录)。
struct TempFile {
    _file: File,
    path: PathBuf,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct Extractor(NonNull<AMediaExtractor>);

impl Drop for Extractor {
    fn drop(&mut self) {
        // SAFETY: 指针由 AMediaExtractor_new 创建且只在本守卫内使用。
        unsafe {
            AMediaExtractor_delete(self.0.as_ptr());
        }
    }
}

struct Codec(NonNull<AMediaCodec>);

impl Drop for Codec {
    fn drop(&mut self) {
        // SAFETY: 指针由 AMediaCodec_createDecoderByType 创建;delete 会
        // 停止并释放编解码器,不需要先手动 stop。
        unsafe {
            AMediaCodec_delete(self.0.as_ptr());
        }
    }
}

/// 一条已解码、已转 RGBA 的视频帧。
struct Frame {
    pts: f64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// 视频解码器 ByteBuffer 输出的布局参数(输出格式变更时刷新)。
#[derive(Clone, Copy, Debug)]
struct VideoBufferLayout {
    color_format: i32,
    stride: usize,
    slice_height: usize,
    crop_left: usize,
    crop_top: usize,
    crop_w: usize,
    crop_h: usize,
}

impl VideoBufferLayout {
    fn fallback(width: u32, height: u32) -> Self {
        Self {
            color_format: 0,
            stride: width as usize,
            slice_height: height as usize,
            crop_left: 0,
            crop_top: 0,
            crop_w: width as usize,
            crop_h: height as usize,
        }
    }
}

struct VideoTrack {
    codec: Codec,
    track_index: usize,
    width: u32,
    height: u32,
    input_eos: bool,
    output_eos: bool,
    /// 首个视频样本 PTS(µs),用于把时间轴归零。
    base_pts_us: Option<i64>,
    /// 当前输出布局(ByteBuffer,不经过 Surface/ImageReader)。
    layout: VideoBufferLayout,
    /// 布局诊断只打一次(格式变更时再打)。
    logged_layout: bool,
    /// 转换失败也只打一次(避免每帧刷日志)。
    logged_convert_error: bool,
    /// 首个成功转换的帧只打一次(确认取帧链路真的通了)。
    logged_first_frame: bool,
}

struct AudioTrack {
    codec: Codec,
    track_index: usize,
    input_eos: bool,
    output_eos: bool,
    sample_rate: u32,
    channels: u16,
    /// 首个音频样本 PTS(µs)。
    base_pts_us: Option<i64>,
    /// 已解码音频覆盖到的媒体时间(秒)。
    decoded_time: f64,
}

impl AudioTrack {
    /// 从输出格式刷新采样率/声道数(某些编码要到 `INFO_OUTPUT_FORMAT_CHANGED`
    /// 才有正确值)。
    fn read_output_format(&mut self, format: *mut AMediaFormat) {
        // SAFETY: format 来自 AMediaCodec_getOutputFormat,调用方随后 delete。
        unsafe {
            let mut rate = 0i32;
            if AMediaFormat_getInt32(format, K_SAMPLE_RATE.as_ptr().cast(), &mut rate) && rate > 0 {
                self.sample_rate = rate as u32;
            }
            let mut channels = 0i32;
            if AMediaFormat_getInt32(format, K_CHANNEL_COUNT.as_ptr().cast(), &mut channels)
                && channels > 0
            {
                self.channels = channels.min(i32::from(u16::MAX)) as u16;
            }
        }
    }
}

struct Session {
    extractor: Extractor,
    video: VideoTrack,
    audio: Option<AudioTrack>,
    /// 帧队列按 PTS 升序;`current` 是已经到点、最近显示的一帧。
    frames: VecDeque<Frame>,
    current: Option<Frame>,
    audio_out: Vec<i16>,
    ended: bool,
    /// 【必须最后声明】文件要在 extractor/codec 释放后再删。
    _temp: TempFile,
}

impl Session {
    fn open(temp_dir: &Path, file: &[u8], bytes: &[u8]) -> Result<Self, Diag> {
        if bytes.is_empty() {
            return Err(fail(format!("影片为空:{}", String::from_utf8_lossy(file))));
        }
        std::fs::create_dir_all(temp_dir).map_err(|error| {
            fail(format!(
                "创建影片临时目录 {} 失败: {error}",
                temp_dir.display()
            ))
        })?;
        let serial = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = temp_dir.join(format!("yu-movie-{}-{serial}.tmp", std::process::id()));
        let mut fs_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| fail(format!("创建影片临时文件 {} 失败: {error}", path.display())))?;
        if let Err(error) = fs_file.write_all(bytes) {
            let _ = std::fs::remove_file(&path);
            return Err(fail(format!(
                "写影片临时文件 {} 失败: {error}",
                path.display()
            )));
        }
        fs_file
            .flush()
            .map_err(|error| fail(format!("刷影片临时文件 {} 失败: {error}", path.display())))?;
        let temp = TempFile {
            _file: fs_file,
            path,
        };

        // SAFETY: 以下 NDK 调用都只使用本函数内创建、尚未共享的句柄;
        // 失败路径由 RAII 守卫释放。
        unsafe {
            let extractor = require_ptr(AMediaExtractor_new(), "AMediaExtractor")?;
            let extractor = Extractor(extractor);
            let status = AMediaExtractor_setDataSourceFd(
                extractor.0.as_ptr(),
                temp._file.as_raw_fd(),
                0,
                i64::try_from(bytes.len()).unwrap_or(i64::MAX),
            );
            if status.0 != media_status_t::AMEDIA_OK.0 {
                return Err(fail(format!(
                    "设备媒体栈打不开影片容器:{}",
                    String::from_utf8_lossy(file)
                )));
            }

            // 第一遍:只选轨 + 记录元数据,不保留 track format 指针。
            let track_count = AMediaExtractor_getTrackCount(extractor.0.as_ptr());
            let mut video_pick: Option<(usize, String, u32, u32, f64)> = None;
            let mut audio_pick: Option<(usize, String)> = None;
            for index in 0..track_count {
                let format = AMediaExtractor_getTrackFormat(extractor.0.as_ptr(), index);
                if format.is_null() {
                    continue;
                }
                let mime = format_string(format, K_MIME);
                let width = format_i32(format, K_WIDTH);
                let height = format_i32(format, K_HEIGHT);
                let frame_rate = format_f32(format, K_FRAME_RATE)
                    .or_else(|| format_i32(format, K_FRAME_RATE).map(|value| value as f32));
                AMediaFormat_delete(format);
                let Some(mime) = mime else {
                    continue;
                };
                if video_pick.is_none() && mime.starts_with("video/") {
                    video_pick = Some((
                        index,
                        mime,
                        width.unwrap_or(0).max(0) as u32,
                        height.unwrap_or(0).max(0) as u32,
                        frame_rate.unwrap_or(0.0) as f64,
                    ));
                } else if audio_pick.is_none() && mime.starts_with("audio/") {
                    audio_pick = Some((index, mime));
                }
            }
            let Some((video_index, video_mime, width, height, _frame_rate)) = video_pick else {
                return Err(fail(format!(
                    "影片没有视频轨(设备 extractor 只认容器/编码):{}",
                    String::from_utf8_lossy(file)
                )));
            };
            if width == 0 || height == 0 {
                return Err(fail(format!(
                    "视频轨缺少宽高({video_mime});设备 extractor 未给出可解码尺寸"
                )));
            }

            let select = AMediaExtractor_selectTrack(extractor.0.as_ptr(), video_index);
            if select.0 != media_status_t::AMEDIA_OK.0 {
                return Err(fail(format!("选择视频轨失败(track={video_index})")));
            }

            // 视频解码器:**ByteBuffer 输出**(不传 Surface)。
            //
            // 高通等平台的 codec 在 Surface/ImageReader 路径会选 UBWC
            // (0x7fa30c06),AImage 的 U/V 平面拿不到 CPU 指针;ByteBuffer
            // 输出是线性 YUV,由 `yuv420_buffer_to_rgba` 按 stride/slice
            // 转换,跨设备更稳。
            let video_codec = create_decoder(&video_mime)?;
            let video_format = AMediaExtractor_getTrackFormat(extractor.0.as_ptr(), video_index);
            if video_format.is_null() {
                return Err(fail("读取视频轨格式失败"));
            }
            let status = AMediaCodec_configure(
                video_codec.0.as_ptr(),
                video_format,
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            );
            AMediaFormat_delete(video_format);
            if status.0 != media_status_t::AMEDIA_OK.0 {
                return Err(fail(format!(
                    "配置视频解码器失败({video_mime});设备可能不支持 ByteBuffer 输出/该分辨率"
                )));
            }
            if AMediaCodec_start(video_codec.0.as_ptr()).0 != media_status_t::AMEDIA_OK.0 {
                return Err(fail(format!("启动视频解码器失败({video_mime})")));
            }
            let layout = read_video_layout(video_codec.0.as_ptr(), width, height);
            log::info!(
                target: "yu-kernel",
                "[movie] 外置视频 ByteBuffer: color-format=0x{:x} stride={} slice={} crop={}x{}+{},{}",
                layout.color_format,
                layout.stride,
                layout.slice_height,
                layout.crop_w,
                layout.crop_h,
                layout.crop_left,
                layout.crop_top
            );

            // 音频轨可选:失败只记日志,视频照播(没有音频也比整片放弃好)。
            let mut audio = None;
            if let Some((audio_index, audio_mime)) = audio_pick {
                match build_audio_track(extractor.0.as_ptr(), audio_index, &audio_mime) {
                    Ok(track) => {
                        if AMediaExtractor_selectTrack(extractor.0.as_ptr(), audio_index).0
                            != media_status_t::AMEDIA_OK.0
                        {
                            log::warn!(
                                target: "yu-kernel",
                                "[movie] 选择音频轨失败(track={audio_index}),按无音频处理"
                            );
                        } else {
                            audio = Some(track);
                        }
                    }
                    Err(error) => {
                        log::warn!(
                            target: "yu-kernel",
                            "[movie] 音频解码不可用({audio_mime}): {error}"
                        );
                    }
                }
            }

            Ok(Self {
                extractor,
                video: VideoTrack {
                    codec: video_codec,
                    track_index: video_index,
                    width,
                    height,
                    input_eos: false,
                    output_eos: false,
                    base_pts_us: None,
                    layout,
                    logged_layout: true,
                    logged_convert_error: false,
                    logged_first_frame: false,
                },
                audio,
                frames: VecDeque::new(),
                current: None,
                audio_out: Vec::new(),
                ended: false,
                _temp: temp,
            })
        }
    }

    fn audio_info(&self) -> Option<MovieAudioInfo> {
        self.audio.as_ref().map(|audio| MovieAudioInfo {
            sample_rate: audio.sample_rate.max(1),
            channels: 2,
        })
    }

    fn current_frame(&self) -> Option<MovieFrame<'_>> {
        let frame = self.current.as_ref()?;
        Some(MovieFrame {
            width: frame.width,
            height: frame.height,
            rgba: &frame.rgba,
        })
    }

    fn media_size(&self) -> (u32, u32) {
        (self.video.width, self.video.height)
    }

    fn is_ended(&self) -> bool {
        self.ended
    }

    /// 推进到 `target` 秒:喂样本 → 排空输出 → 选帧。
    fn pump(&mut self, target: f64) {
        let feed_until_us = (target + FEED_LEAD_SECONDS) * 1_000_000.0;
        for _ in 0..FEED_BUDGET {
            // 先看下一个样本的时间:太超前就停,避免把整部影片解进内存。
            // SAFETY: extractor 在 Session 生命周期内有效。
            let next_pts = unsafe { AMediaExtractor_getSampleTime(self.extractor.0.as_ptr()) };
            if next_pts >= 0 && (next_pts as f64) > feed_until_us + self.video_base_us() {
                break;
            }
            match self.feed_one() {
                FeedResult::Fed => {}
                FeedResult::Full | FeedResult::Eos | FeedResult::Error => break,
            }
        }
        self.drain_video(VIDEO_DRAIN_BUDGET);
        self.drain_audio(AUDIO_DRAIN_BUDGET, target);
        self.advance_current(target);
        self.refresh_ended();
    }

    fn video_base_us(&self) -> f64 {
        self.video.base_pts_us.unwrap_or(0) as f64
    }

    /// 喂一个样本;`Full` = 输入缓冲暂满,下 tick 再试。
    fn feed_one(&mut self) -> FeedResult {
        // SAFETY: extractor/codec 句柄均在 Session 生命周期内。
        unsafe {
            let extractor = self.extractor.0.as_ptr();
            let track = AMediaExtractor_getSampleTrackIndex(extractor);
            if track < 0 {
                self.feed_eos();
                return FeedResult::Eos;
            }
            let track = track as usize;
            let codec = if track == self.video.track_index {
                self.video.codec.0.as_ptr()
            } else if let Some(audio) = self
                .audio
                .as_ref()
                .filter(|audio| audio.track_index == track)
            {
                audio.codec.0.as_ptr()
            } else {
                // 未选中的轨(理论上不会出现):跳过。
                AMediaExtractor_advance(extractor);
                return FeedResult::Fed;
            };
            let input = AMediaCodec_dequeueInputBuffer(codec, 0);
            if input < 0 {
                return FeedResult::Full;
            }
            let input = input as usize;
            let mut capacity = 0usize;
            let buffer = AMediaCodec_getInputBuffer(codec, input, &mut capacity);
            if buffer.is_null() || capacity == 0 {
                return FeedResult::Full;
            }
            let size = AMediaExtractor_readSampleData(extractor, buffer, capacity);
            if size < 0 {
                // `-1` 既可能是 EOS,也可能是“样本大于 codec 输入缓冲”。
                // 用当前样本时间区分:没有样本时间才是真 EOS;否则跳过该
                // 样本(不能当成 EOS,否则整片会提前结束)。
                if AMediaExtractor_getSampleTime(extractor) < 0 {
                    AMediaCodec_queueInputBuffer(
                        codec,
                        input,
                        0,
                        0,
                        0,
                        AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
                    );
                    self.mark_input_eos(track);
                    AMediaExtractor_advance(extractor);
                    return FeedResult::Eos;
                }
                log::warn!(
                    target: "yu-kernel",
                    "[movie] 样本超过解码器输入缓冲({capacity}B),已跳过"
                );
                AMediaExtractor_advance(extractor);
                return FeedResult::Fed;
            }
            let pts = AMediaExtractor_getSampleTime(extractor);
            let pts = pts.max(0);
            let sample_flags = AMediaExtractor_getSampleFlags(extractor);
            let flags = if sample_flags & 1 != 0 {
                AMEDIACODEC_BUFFER_FLAG_KEY_FRAME
            } else {
                0
            };
            if AMediaCodec_queueInputBuffer(codec, input, 0, size as usize, pts as u64, flags).0
                != media_status_t::AMEDIA_OK.0
            {
                log::warn!(
                    target: "yu-kernel",
                    "[movie] queueInputBuffer 失败(track={track}, size={size}, pts={pts})"
                );
                return FeedResult::Error;
            }
            if track == self.video.track_index {
                if self.video.base_pts_us.is_none() {
                    self.video.base_pts_us = Some(pts);
                }
            } else if let Some(audio) = self.audio.as_mut() {
                if audio.base_pts_us.is_none() {
                    audio.base_pts_us = Some(pts);
                }
            }
            AMediaExtractor_advance(extractor);
            FeedResult::Fed
        }
    }

    fn mark_input_eos(&mut self, track: usize) {
        if track == self.video.track_index {
            self.video.input_eos = true;
        }
        if let Some(audio) = self.audio.as_mut() {
            if audio.track_index == track {
                audio.input_eos = true;
            }
        }
    }

    /// 给还没收到 EOS 的解码器补 EOS(输入缓冲不足时下 tick 再试)。
    fn feed_eos(&mut self) {
        unsafe {
            if !self.video.input_eos {
                let codec = self.video.codec.0.as_ptr();
                let input = AMediaCodec_dequeueInputBuffer(codec, 0);
                if input >= 0 {
                    AMediaCodec_queueInputBuffer(
                        codec,
                        input as usize,
                        0,
                        0,
                        0,
                        AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
                    );
                    self.video.input_eos = true;
                }
            }
            if let Some(audio) = self.audio.as_mut() {
                if !audio.input_eos {
                    let codec = audio.codec.0.as_ptr();
                    let input = AMediaCodec_dequeueInputBuffer(codec, 0);
                    if input >= 0 {
                        AMediaCodec_queueInputBuffer(
                            codec,
                            input as usize,
                            0,
                            0,
                            0,
                            AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
                        );
                        audio.input_eos = true;
                    }
                }
            }
        }
    }

    fn drain_video(&mut self, budget: usize) {
        // SAFETY: codec 句柄在 Session 生命周期内有效。
        unsafe {
            let codec = self.video.codec.0.as_ptr();
            for _ in 0..budget {
                let mut info = AMediaCodecBufferInfo {
                    offset: 0,
                    size: 0,
                    presentationTimeUs: 0,
                    flags: 0,
                };
                let index = AMediaCodec_dequeueOutputBuffer(codec, &mut info, 0);
                if index >= 0 {
                    if info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0 {
                        self.video.output_eos = true;
                    }
                    if info.size > 0 {
                        let mut capacity = 0usize;
                        let buffer =
                            AMediaCodec_getOutputBuffer(codec, index as usize, &mut capacity);
                        if !buffer.is_null() {
                            let offset = info.offset.max(0) as usize;
                            let size = info.size.max(0) as usize;
                            if offset.saturating_add(size) <= capacity {
                                let bytes = slice::from_raw_parts(buffer.add(offset), size);
                                self.convert_video_buffer(bytes, info.presentationTimeUs);
                            }
                        }
                    }
                    // ByteBuffer 模式:已经拷贝/转换,不需要 render。
                    AMediaCodec_releaseOutputBuffer(codec, index as usize, false);
                } else if index == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED as isize {
                    let format = AMediaCodec_getOutputFormat(codec);
                    if !format.is_null() {
                        self.video.layout =
                            layout_from_format(format, self.video.width, self.video.height);
                        self.video.logged_layout = false;
                        AMediaFormat_delete(format);
                    }
                    self.log_video_layout_once();
                } else if index == AMEDIACODEC_INFO_TRY_AGAIN_LATER as isize {
                    break;
                } else if index == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED as isize {
                    continue;
                } else {
                    break;
                }
            }
        }
    }

    /// 把一个 ByteBuffer 输出样本转成 RGBA 并推进帧队列。
    fn convert_video_buffer(&mut self, bytes: &[u8], pts_us: i64) {
        if self.frames.len() >= self.frame_queue_limit() {
            return;
        }
        let layout = self.video.layout;
        let mut rgba = Vec::new();
        let Some((width, height, _used)) = yuv420_buffer_to_rgba(
            bytes,
            self.video.width as usize,
            self.video.height as usize,
            layout.color_format,
            layout.stride,
            layout.slice_height,
            layout.crop_left,
            layout.crop_top,
            layout.crop_w,
            layout.crop_h,
            &mut rgba,
        ) else {
            if !self.video.logged_convert_error {
                self.video.logged_convert_error = true;
                log::warn!(
                    target: "yu-kernel",
                    "[movie] ByteBuffer YUV 转换失败(后续同类不再重复): \
                     color-format=0x{:x} stride={} slice={} crop={}x{}+{},{} len={}",
                    layout.color_format,
                    layout.stride,
                    layout.slice_height,
                    layout.crop_w,
                    layout.crop_h,
                    layout.crop_left,
                    layout.crop_top,
                    bytes.len()
                );
            }
            return;
        };
        let pts = self.normalize_pts_us(pts_us);
        if !self.video.logged_first_frame {
            self.video.logged_first_frame = true;
            log::info!(
                target: "yu-kernel",
                "[movie] 外置首帧 {width}x{height} pts={pts:.3}s"
            );
        }
        self.frames.push_back(Frame {
            pts,
            width: width as u32,
            height: height as u32,
            rgba,
        });
        while self.frames.len() > self.frame_queue_limit() {
            self.frames.pop_front();
        }
    }

    fn log_video_layout_once(&mut self) {
        if self.video.logged_layout {
            return;
        }
        self.video.logged_layout = true;
        let layout = self.video.layout;
        log::info!(
            target: "yu-kernel",
            "[movie] 外置视频 ByteBuffer: color-format=0x{:x} stride={} slice={} crop={}x{}+{},{}",
            layout.color_format,
            layout.stride,
            layout.slice_height,
            layout.crop_w,
            layout.crop_h,
            layout.crop_left,
            layout.crop_top
        );
    }

    /// 输出 PTS(µs)→ 相对首帧的秒;首帧被喂入时由 `feed_one` 记录基准。
    fn normalize_pts_us(&mut self, pts_us: i64) -> f64 {
        let base = *self.video.base_pts_us.get_or_insert(pts_us);
        ((pts_us - base).max(0) as f64) / 1_000_000.0
    }

    /// 按影片分辨率把 RGBA 帧队列压进 32MB 预算(至少 2 帧)。
    fn frame_queue_limit(&self) -> usize {
        let frame_bytes = (self.video.width as usize)
            .saturating_mul(self.video.height as usize)
            .saturating_mul(4)
            .max(1);
        (FRAME_QUEUE_BYTES / frame_bytes).clamp(2, FRAME_QUEUE_LIMIT)
    }

    fn drain_audio(&mut self, budget: usize, target: f64) {
        let Some(mut audio) = self.audio.take() else {
            return;
        };
        let mut decoded = Vec::new();
        // SAFETY: codec 句柄在 Session 生命周期内有效。
        unsafe {
            let codec = audio.codec.0.as_ptr();
            for _ in 0..budget {
                // 预解码提前量够了就停;不排空输出缓冲会让 codec 自然背压。
                if audio.decoded_time > target + AUDIO_LEAD_SECONDS {
                    break;
                }
                let mut info = AMediaCodecBufferInfo {
                    offset: 0,
                    size: 0,
                    presentationTimeUs: 0,
                    flags: 0,
                };
                let index = AMediaCodec_dequeueOutputBuffer(codec, &mut info, 0);
                if index >= 0 {
                    if info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0 {
                        audio.output_eos = true;
                    }
                    if info.size > 0 {
                        let mut capacity = 0usize;
                        let buffer =
                            AMediaCodec_getOutputBuffer(codec, index as usize, &mut capacity);
                        if !buffer.is_null() {
                            let offset = info.offset.max(0) as usize;
                            let size = info.size.max(0) as usize;
                            if offset.saturating_add(size) <= capacity {
                                let bytes = slice::from_raw_parts(buffer.add(offset), size);
                                let samples = pcm16le(bytes);
                                let mut stereo = Vec::with_capacity(
                                    (samples.len() / usize::from(audio.channels.max(1))) * 2,
                                );
                                downmix_to_stereo(
                                    &samples,
                                    usize::from(audio.channels.max(1)),
                                    &mut stereo,
                                );
                                if !stereo.is_empty() {
                                    let base =
                                        *audio.base_pts_us.get_or_insert(info.presentationTimeUs);
                                    let pts = ((info.presentationTimeUs - base).max(0) as f64)
                                        / 1_000_000.0;
                                    let frames = stereo.len() / 2;
                                    audio.decoded_time = audio.decoded_time.max(
                                        pts + frames as f64 / f64::from(audio.sample_rate.max(1)),
                                    );
                                    decoded.extend_from_slice(&stereo);
                                }
                            }
                        }
                    }
                    AMediaCodec_releaseOutputBuffer(codec, index as usize, false);
                } else if index == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED as isize {
                    let format = AMediaCodec_getOutputFormat(codec);
                    if !format.is_null() {
                        audio.read_output_format(format);
                        AMediaFormat_delete(format);
                    }
                } else if index == AMEDIACODEC_INFO_TRY_AGAIN_LATER as isize {
                    break;
                } else if index == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED as isize {
                    continue;
                } else {
                    break;
                }
            }
        }
        self.audio_out.extend_from_slice(&decoded);
        // 防止长时间没人取(无声音域)时无限增长:最多保留约 2 秒。
        let cap = (audio.sample_rate.max(1) as usize)
            .saturating_mul(2)
            .saturating_mul(2);
        if self.audio_out.len() > cap {
            let drop = self.audio_out.len() - cap;
            self.audio_out.drain(..drop - drop % 2);
        }
        self.audio = Some(audio);
    }

    fn advance_current(&mut self, target: f64) {
        while let Some(front) = self.frames.front() {
            if front.pts <= target + 1e-6 {
                self.current = self.frames.pop_front();
            } else {
                break;
            }
        }
    }

    fn refresh_ended(&mut self) {
        if self.video.output_eos && self.frames.is_empty() {
            self.ended = true;
        }
    }
}

enum FeedResult {
    Fed,
    Full,
    Eos,
    Error,
}

/// 建一个 MIME 解码器;失败给出设备能力不足的诊断。
fn create_decoder(mime: &str) -> Result<Codec, Diag> {
    let c_mime = CString::new(mime).map_err(|_| fail(format!("非法 MIME(含 NUL):{mime}")))?;
    // SAFETY: 只传一个只读 C 字符串;返回的指针由 Codec 守卫释放。
    let codec = unsafe { AMediaCodec_createDecoderByType(c_mime.as_ptr()) };
    if codec.is_null() {
        return Err(fail(format!("设备没有 {mime} 解码器(系统媒体能力不足)")));
    }
    Ok(Codec(require_ptr(codec, "AMediaCodec")?))
}

fn build_audio_track(
    extractor: *mut AMediaExtractor,
    index: usize,
    mime: &str,
) -> Result<AudioTrack, Diag> {
    let codec = create_decoder(mime)?;
    // SAFETY: extractor/codec 均有效;track format 在 configure 后 delete。
    unsafe {
        let format = AMediaExtractor_getTrackFormat(extractor, index);
        if format.is_null() {
            return Err(fail("读取音频轨格式失败"));
        }
        let sample_rate = format_i32(format, K_SAMPLE_RATE).unwrap_or(0).max(0) as u32;
        let channels = format_i32(format, K_CHANNEL_COUNT).unwrap_or(0).max(0) as u16;
        // 强制 16-bit PCM(Android AudioFormat.ENCODING_PCM_16BIT = 2)。
        // API 28+ 的解码器认这个键;旧平台忽略未知键,默认同样是 16-bit。
        AMediaFormat_setInt32(format, K_PCM_ENCODING.as_ptr().cast(), 2);
        let status = AMediaCodec_configure(
            codec.0.as_ptr(),
            format,
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        );
        AMediaFormat_delete(format);
        if status.0 != media_status_t::AMEDIA_OK.0 {
            return Err(fail(format!("配置音频解码器失败({mime})")));
        }
        if AMediaCodec_start(codec.0.as_ptr()).0 != media_status_t::AMEDIA_OK.0 {
            return Err(fail(format!("启动音频解码器失败({mime})")));
        }
        Ok(AudioTrack {
            codec,
            track_index: index,
            input_eos: false,
            output_eos: false,
            sample_rate: if sample_rate > 0 { sample_rate } else { 48_000 },
            channels: if channels > 0 { channels } else { 2 },
            base_pts_us: None,
            decoded_time: 0.0,
        })
    }
}

unsafe fn format_string(format: *mut AMediaFormat, key: &[u8]) -> Option<String> {
    let mut out: *const c_char = ptr::null();
    if AMediaFormat_getString(format, key.as_ptr().cast(), &mut out) && !out.is_null() {
        Some(CStr::from_ptr(out).to_string_lossy().into_owned())
    } else {
        None
    }
}

unsafe fn format_i32(format: *mut AMediaFormat, key: &[u8]) -> Option<i32> {
    let mut out = 0i32;
    if AMediaFormat_getInt32(format, key.as_ptr().cast(), &mut out) {
        Some(out)
    } else {
        None
    }
}

unsafe fn format_f32(format: *mut AMediaFormat, key: &[u8]) -> Option<f32> {
    let mut out = 0f32;
    if AMediaFormat_getFloat(format, key.as_ptr().cast(), &mut out) {
        Some(out)
    } else {
        None
    }
}

/// 读取解码器当前输出格式(ByteBuffer)的 stride/slice/crop。
unsafe fn read_video_layout(codec: *mut AMediaCodec, width: u32, height: u32) -> VideoBufferLayout {
    let format = AMediaCodec_getOutputFormat(codec);
    if format.is_null() {
        return VideoBufferLayout::fallback(width, height);
    }
    let layout = layout_from_format(format, width, height);
    AMediaFormat_delete(format);
    layout
}

unsafe fn layout_from_format(
    format: *mut AMediaFormat,
    width: u32,
    height: u32,
) -> VideoBufferLayout {
    let width_i = width as i32;
    let height_i = height as i32;
    let max_x = width_i.saturating_sub(1).max(0);
    let max_y = height_i.saturating_sub(1).max(0);
    let color_format = format_i32(format, K_COLOR_FORMAT).unwrap_or(0);
    let stride = format_i32(format, K_STRIDE)
        .unwrap_or(width_i)
        .max(width_i)
        .max(1) as usize;
    let slice_height = format_i32(format, K_SLICE_HEIGHT)
        .unwrap_or(height_i)
        .max(height_i)
        .max(1) as usize;
    let crop_left = format_i32(format, K_CROP_LEFT).unwrap_or(0).clamp(0, max_x) as usize;
    let crop_top = format_i32(format, K_CROP_TOP).unwrap_or(0).clamp(0, max_y) as usize;
    let crop_right = format_i32(format, K_CROP_RIGHT)
        .unwrap_or(max_x)
        .clamp(0, max_x) as usize;
    let crop_bottom = format_i32(format, K_CROP_BOTTOM)
        .unwrap_or(max_y)
        .clamp(0, max_y) as usize;
    let crop_w = crop_right.saturating_sub(crop_left).saturating_add(1);
    let crop_h = crop_bottom.saturating_sub(crop_top).saturating_add(1);
    VideoBufferLayout {
        color_format,
        stride,
        slice_height,
        crop_left,
        crop_top,
        crop_w: crop_w.max(1),
        crop_h: crop_h.max(1),
    }
}

/// 交错小端 i16 PCM。
fn pcm16le(bytes: &[u8]) -> Vec<i16> {
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    out
}

/// 设备能力探测:对固定 MIME 表逐个尝试建解码器,立即释放。
pub fn probe_video_codecs() -> Vec<MovieCodecSupport> {
    VIDEO_MIMES
        .iter()
        .map(|mime| {
            let Ok(c_mime) = CString::new(*mime) else {
                return MovieCodecSupport {
                    mime,
                    decoder: false,
                };
            };
            // SAFETY: 只创建并立即删除一个未配置的解码器。
            let codec = unsafe { AMediaCodec_createDecoderByType(c_mime.as_ptr()) };
            let decoder = !codec.is_null();
            if decoder {
                unsafe {
                    AMediaCodec_delete(codec);
                }
            }
            MovieCodecSupport { mime, decoder }
        })
        .collect()
}

/// Android 外置影片后端。
pub struct AndroidMovieHost {
    temp_dir: PathBuf,
    session: Option<Session>,
    playing: bool,
    ended: bool,
    /// 本次播放已经过的媒体毫秒(暂停时冻结;恢复时从该点继续)。
    position_ms: u64,
    /// 播放起点(宿主单调时钟毫秒)。`None` = 下一次 `frame` 重新锚定。
    start_ms: Option<u64>,
    /// 官方 `MOVIE[VOLUME]` 原值(混音在声音域;这里保存以备诊断)。
    _volume: i64,
    /// 官方 `MOVIE` 几何(呈现尺寸由内核换算;这里保存以备将来直出)。
    _geometry: MovieGeometry,
}

impl AndroidMovieHost {
    /// `temp_dir`:app 私有 cache 目录(宿主从 `getCacheDir()` 传入)。
    #[must_use]
    pub fn new(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir,
            session: None,
            playing: false,
            ended: false,
            position_ms: 0,
            start_ms: None,
            _volume: 256,
            _geometry: MovieGeometry::default(),
        }
    }
}

impl MovieHost for AndroidMovieHost {
    fn open(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        self.close();
        let session = Session::open(&self.temp_dir, file, bytes)?;
        log::info!(
            target: "yu-kernel",
            "[movie] 外置解码:{} ({}x{}, 音频={})",
            String::from_utf8_lossy(file),
            session.media_size().0,
            session.media_size().1,
            if session.audio.is_some() { "yes" } else { "no" }
        );
        self.session = Some(session);
        self.playing = false;
        self.ended = false;
        self.position_ms = 0;
        self.start_ms = None;
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), Diag> {
        if playing == self.playing {
            return Ok(());
        }
        self.playing = playing;
        // 暂停:把当前进度留在 position_ms(由 frame 最后写入);恢复时
        // 用 now - position 重新锚定,不会像 pl_mpeg 那样从头算 target。
        self.start_ms = None;
        if !playing {
            if let Some(session) = self.session.as_mut() {
                session.audio_out.clear();
            }
        }
        Ok(())
    }

    fn set_volume(&mut self, volume: i64) {
        // 音量由声音域按 COM/DSBVOLUME 曲线应用;这里只记录原值。
        self._volume = volume;
    }

    fn set_geometry(&mut self, geometry: MovieGeometry) {
        self._geometry = geometry;
    }

    fn is_playing(&mut self) -> bool {
        self.playing && !self.ended
    }

    fn media_size(&mut self) -> Option<(u32, u32)> {
        self.session.as_ref().map(Session::media_size)
    }

    fn frame(&mut self, now_ms: u64) -> Option<MovieFrame<'_>> {
        let session = self.session.as_mut()?;
        if self.playing && !self.ended {
            let start = *self
                .start_ms
                .get_or_insert(now_ms.saturating_sub(self.position_ms));
            let target = now_ms.saturating_sub(start) as f64 / 1000.0;
            self.position_ms = (target * 1000.0) as u64;
            session.pump(target);
            if session.is_ended() {
                self.ended = true;
                self.playing = false;
            }
        }
        session.current_frame()
    }

    fn take_audio(&mut self, out: &mut Vec<i16>) -> Option<MovieAudioInfo> {
        let session = self.session.as_mut()?;
        let info = session.audio_info()?;
        out.extend_from_slice(&session.audio_out);
        session.audio_out.clear();
        Some(info)
    }

    fn backend_label(&self) -> &'static str {
        "external"
    }

    fn close(&mut self) {
        self.session = None;
        self.playing = false;
        self.ended = false;
        self.position_ms = 0;
        self.start_ms = None;
    }
}
