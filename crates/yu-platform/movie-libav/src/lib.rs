//! `MOVIE` 播放后端:**内嵌 FFmpeg 软解**(libavformat + libavcodec)。
//!
//! # 为什么需要它
//!
//! YU-RIS 5xx 的 OP/ED 是 **ASF(WMV)容器 + VC-1 Advanced + WMA**,
//! Android 系统媒体栈(MediaExtractor/MediaCodec)不提供 ASF 解复用与
//! VC-1 解码,`yu-movie-android` 的 `open` 必然失败;内置 pl_mpeg 只解
//! MPEG-1。桌面一直能播是因为桌面注入的是 `yu-movie-ffmpeg`(系统
//! ffmpeg 子进程)。本 crate 把同一套 FFmpeg 解码**静态链进 APK**,
//! 覆盖系统栈不支持的容器/编码。
//!
//! # 分层
//!
//! FFmpeg 的 `AVFrame`/`AVPacket` 布局随大版本变化,直接把结构体抄进
//! Rust 很脆。这里把全部 FFmpeg 调用收在 [`src/shim.c`](shim.c) 的 C ABI
//! 后面,Rust 只见不透明句柄与定长错误串:
//!
//! - 解复用 + 解码:`avformat`/`avcodec` 的 send/receive API;
//! - 像素:`swscale` → RGBA8(行优先,alpha=255);
//! - 采样:`swresample` → 交错 s16 立体声。
//!
//! 时钟/帧队列/暂停语义与 `yu-movie-android` 保持一致(内核的
//! `MovieHost` 契约不变)。
//!
//! # 限制
//!
//! - 只随 APK 带 LGPL 组件;库由 `tools/android/build-ffmpeg.sh` 构建,
//!   换 FFmpeg 版本时同步改脚本里的版本与 sha256。
//! - 解码在渲染线程上同步进行;1080p VC-1 在低端机上可能追不上 60fps,
//!   落后时按 PTS 丢帧(与 Android 后端同一策略)。

#![cfg(target_os = "android")]

use std::collections::VecDeque;
use std::ffi::{c_char, c_int, CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use yu_log::{Boundary, Diag};
use yu_rt::event::{MovieAudioInfo, MovieFrame, MovieGeometry, MovieHost};

/// 每 tick 最多读的包数(防一条慢影片把 tick 拖长)。
const FEED_BUDGET: usize = 24;
/// 解复用最多提前播放时钟多少秒(同时限制帧队列内存)。
const FEED_LEAD_SECONDS: f64 = 0.35;
/// 每 tick 最多排空的视频输出数。
const VIDEO_DRAIN_BUDGET: usize = 8;
/// 每 tick 最多排空的音频输出数。
const AUDIO_DRAIN_BUDGET: usize = 16;
/// 视频帧队列上限(每帧已转 RGBA)。
const FRAME_QUEUE_LIMIT: usize = 12;
/// 视频帧队列的常驻字节预算(1080p 的 RGBA 一帧约 8.3MB)。
const FRAME_QUEUE_BYTES: usize = 32 * 1024 * 1024;
/// 一次 `ye_movie_audio` 的输出上限(i16 个数;WMA 一包 ≤ 2048 帧 × 2)。
const AUDIO_CHUNK_I16: usize = 16 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct YeMovie {
    _private: [u8; 0],
}

extern "C" {
    fn ye_movie_open(path: *const c_char, err: *mut c_char, err_len: c_int) -> *mut YeMovie;
    fn ye_movie_close(movie: *mut YeMovie);
    fn ye_movie_feed(movie: *mut YeMovie) -> c_int;
    fn ye_movie_video(
        movie: *mut YeMovie,
        rgba: *mut u8,
        rgba_cap: c_int,
        out_pts: *mut f64,
        out_w: *mut c_int,
        out_h: *mut c_int,
    ) -> c_int;
    fn ye_movie_audio(
        movie: *mut YeMovie,
        out: *mut i16,
        cap_i16: c_int,
        out_pts: *mut f64,
    ) -> c_int;
    fn ye_movie_info(
        movie: *const YeMovie,
        width: *mut c_int,
        height: *mut c_int,
        fps: *mut f64,
        sample_rate: *mut c_int,
        channels: *mut c_int,
        duration: *mut f64,
    ) -> c_int;
    fn ye_movie_version() -> *const c_char;
}

fn fail(detail: impl Into<String>) -> Diag {
    Diag::recoverable("movie.decode", Boundary::Platform).with_detail(detail.into())
}

/// 临时文件守卫:drop 时删除(app 私有 cache 目录)。
///
/// extractor/libav 都只接受路径;整段影片落在 cache,`close`/Drop 清理。
struct TempFile {
    _file: File,
    path: PathBuf,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl TempFile {
    fn write(temp_dir: &Path, bytes: &[u8]) -> Result<Self, Diag> {
        std::fs::create_dir_all(temp_dir).map_err(|error| {
            fail(format!(
                "创建影片临时目录 {} 失败: {error}",
                temp_dir.display()
            ))
        })?;
        let serial = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = temp_dir.join(format!("yu-movie-{}-{serial}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| fail(format!("创建影片临时文件 {} 失败: {error}", path.display())))?;
        if let Err(error) = file.write_all(bytes) {
            let _ = std::fs::remove_file(&path);
            return Err(fail(format!(
                "写影片临时文件 {} 失败: {error}",
                path.display()
            )));
        }
        file.flush()
            .map_err(|error| fail(format!("刷影片临时文件 {} 失败: {error}", path.display())))?;
        Ok(Self { _file: file, path })
    }
}

/// FFmpeg 会话句柄(RAII)。
struct MovieHandle {
    ptr: *mut YeMovie,
}

// 句柄只在渲染线程上使用;显式声明避免以后被误跨线程移动。
impl Drop for MovieHandle {
    fn drop(&mut self) {
        unsafe { ye_movie_close(self.ptr) };
        self.ptr = std::ptr::null_mut();
    }
}

struct Session {
    handle: MovieHandle,
    /// 【必须最后声明】文件要在 handle 释放后再删。
    _temp: TempFile,
}

/// 一条已解码、已转 RGBA 的视频帧。
struct Frame {
    pts: f64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Session {
    fn open(temp_dir: &Path, file: &[u8], bytes: &[u8]) -> Result<Self, Diag> {
        if bytes.is_empty() {
            return Err(fail(format!("影片为空:{}", String::from_utf8_lossy(file))));
        }
        let temp = TempFile::write(temp_dir, bytes)?;
        let path = CString::new(temp.path.as_os_str().as_bytes())
            .map_err(|_| fail("影片临时路径含 NUL".to_string()))?;
        let mut err = [0 as c_char; 256];
        // SAFETY: path 是有效 C 字符串;失败时 err 由 shim 写入 NUL 结尾文本。
        let ptr = unsafe { ye_movie_open(path.as_ptr(), err.as_mut_ptr(), 256) };
        if ptr.is_null() {
            let detail = unsafe { CStr::from_ptr(err.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(fail(format!(
                "libav 打不开影片 {}: {detail}",
                String::from_utf8_lossy(file)
            )));
        }
        Ok(Self {
            handle: MovieHandle { ptr },
            _temp: temp,
        })
    }

    fn info(&self) -> MovieInfo {
        let (mut width, mut height, mut fps) = (0, 0, 0.0);
        let (mut rate, mut channels, mut duration) = (0, 0, 0.0);
        // SAFETY: 句柄有效;只写调用方提供的标量。
        unsafe {
            ye_movie_info(
                self.handle.ptr,
                &mut width,
                &mut height,
                &mut fps,
                &mut rate,
                &mut channels,
                &mut duration,
            );
        }
        MovieInfo {
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            fps,
            sample_rate: rate.max(0) as u32,
            channels: channels.max(0) as u32,
            duration,
        }
    }
}

struct MovieInfo {
    width: u32,
    height: u32,
    fps: f64,
    sample_rate: u32,
    channels: u32,
    duration: f64,
}

/// Android 上的 libav 影片后端。
pub struct LibavMovieHost {
    temp_dir: PathBuf,
    session: Option<Session>,
    media: Option<(u32, u32)>,
    audio: Option<MovieAudioInfo>,
    playing: bool,
    ended: bool,
    video_eof: bool,
    /// 本次播放已经过的媒体毫秒(暂停时冻结;恢复时从该点继续)。
    position_ms: u64,
    /// 播放起点(宿主单调时钟毫秒)。`None` = 下一次 `frame` 重新锚定。
    start_ms: Option<u64>,
    frames: VecDeque<Frame>,
    current: Option<Frame>,
    audio_out: Vec<i16>,
    /// 官方 `MOVIE[VOLUME]` 原值(混音在声音域;这里保存以备诊断)。
    _volume: i64,
    /// 官方 `MOVIE` 几何(呈现尺寸由内核换算;这里保存以备将来直出)。
    _geometry: MovieGeometry,
    /// 一次性告警(转换/解码错误不刷日志)。
    warned: bool,
    /// 连续视频解码错误数(单个坏帧可跳过;连续多次才按结束处理)。
    video_errors: u8,
}

impl LibavMovieHost {
    /// `temp_dir`:app 私有 cache 目录(宿主从 `getCacheDir()` 传入)。
    #[must_use]
    pub fn new(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir,
            session: None,
            media: None,
            audio: None,
            playing: false,
            ended: false,
            video_eof: false,
            position_ms: 0,
            start_ms: None,
            frames: VecDeque::new(),
            current: None,
            audio_out: Vec::new(),
            _volume: 256,
            _geometry: MovieGeometry::default(),
            warned: false,
            video_errors: 0,
        }
    }

    fn handle(&self) -> Option<*mut YeMovie> {
        self.session.as_ref().map(|session| session.handle.ptr)
    }

    fn frame_queue_limit(&self) -> usize {
        let (w, h) = self.media.unwrap_or((0, 0));
        let frame_bytes = (w as usize)
            .saturating_mul(h as usize)
            .saturating_mul(4)
            .max(1);
        (FRAME_QUEUE_BYTES / frame_bytes).clamp(2, FRAME_QUEUE_LIMIT)
    }

    /// 推进到 `target` 秒:喂包 → 排空输出 → 选帧。
    fn pump(&mut self, target: f64) {
        let Some(ptr) = self.handle() else {
            return;
        };
        let lead = target + FEED_LEAD_SECONDS;
        // 视频解码器已经 EOS 后不再喂包:继续喂只会反复拿 `-1/-2`。
        if !self.video_eof {
            for _ in 0..FEED_BUDGET {
                if self.frames.back().is_some_and(|frame| frame.pts > lead) {
                    break;
                }
                // SAFETY: 句柄在 session 生命周期内有效。
                let fed = unsafe { ye_movie_feed(ptr) };
                if fed == -2 {
                    // 解复用/解码的致命错误必须收敛成“结束”:否则
                    // `video_eof` 永远不置位,`MOVIEINFO[PLAY]` 的等待
                    // 循环会卡死。剩余已解出的帧照常播完。
                    self.video_eof = true;
                    if !self.warned {
                        self.warned = true;
                        log::warn!(
                            target: "yu-kernel",
                            "[movie] libav 解复用/解码出错,按播放结束处理"
                        );
                    }
                    break;
                }
                self.drain_video(VIDEO_DRAIN_BUDGET);
                self.drain_audio(AUDIO_DRAIN_BUDGET);
                if fed == -1 {
                    break;
                }
            }
        }
        self.drain_video(VIDEO_DRAIN_BUDGET);
        self.drain_audio(AUDIO_DRAIN_BUDGET);
        self.advance_current(target);
    }

    fn drain_video(&mut self, budget: usize) {
        let Some(ptr) = self.handle() else {
            return;
        };
        for _ in 0..budget {
            if self.frames.len() >= self.frame_queue_limit() {
                break;
            }
            let (w, h) = self.media.unwrap_or((0, 0));
            if w == 0 || h == 0 {
                break;
            }
            let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
            let mut rgba = vec![0u8; need];
            let (mut pts, mut fw, mut fh) = (0.0f64, 0, 0);
            // SAFETY: rgba 至少 need 字节;句柄有效。
            let ret = unsafe {
                ye_movie_video(
                    ptr,
                    rgba.as_mut_ptr(),
                    need as c_int,
                    &mut pts,
                    &mut fw,
                    &mut fh,
                )
            };
            match ret {
                1 => {
                    let (fw, fh) = (fw.max(0) as u32, fh.max(0) as u32);
                    let used = (fw as usize).saturating_mul(fh as usize).saturating_mul(4);
                    if used == 0 || used > rgba.len() {
                        continue;
                    }
                    rgba.truncate(used);
                    if self.media != Some((fw, fh)) {
                        log::info!(
                            target: "yu-kernel",
                            "[movie] libav 视频尺寸变更:{}x{} → {fw}x{fh}",
                            w,
                            h
                        );
                        self.media = Some((fw, fh));
                    }
                    self.video_errors = 0;
                    self.frames.push_back(Frame {
                        pts,
                        width: fw,
                        height: fh,
                        rgba,
                    });
                }
                0 => break,
                -1 => {
                    self.video_eof = true;
                    break;
                }
                -3 => {
                    // 实际帧比码流头报的尺寸大:扩大后重试下一帧(丢这一帧)。
                    let (fw, fh) = (fw.max(0) as u32, fh.max(0) as u32);
                    if fw > 0 && fh > 0 {
                        self.media = Some((fw, fh));
                    }
                    if !self.warned {
                        self.warned = true;
                        log::warn!(
                            target: "yu-kernel",
                            "[movie] libav 帧尺寸 {fw}x{fh} 超过预估缓冲,已调整"
                        );
                    }
                }
                _ => {
                    // 单个坏帧让 FFmpeg 自己跳过;连续多次才认定解码结束,
                    // 避免 `MOVIEINFO[PLAY]` 的等待循环永远退不出去。
                    self.video_errors = self.video_errors.saturating_add(1);
                    if self.video_errors >= 8 {
                        self.video_eof = true;
                        if !self.warned {
                            self.warned = true;
                            log::warn!(
                                target: "yu-kernel",
                                "[movie] libav 连续视频解码错误,按播放结束处理"
                            );
                        }
                    }
                    break;
                }
            }
        }
    }

    fn drain_audio(&mut self, budget: usize) {
        let Some(ptr) = self.handle() else {
            return;
        };
        let Some(info) = self.audio else {
            return;
        };
        let cap = (info.sample_rate as usize)
            .saturating_mul(2)
            .saturating_mul(2);
        for _ in 0..budget {
            if self.audio_out.len() > cap {
                break;
            }
            let mut buf = vec![0i16; AUDIO_CHUNK_I16];
            let mut pts = 0.0f64;
            // SAFETY: buf 容量与 cap_i16 一致;句柄有效。
            let ret = unsafe {
                ye_movie_audio(ptr, buf.as_mut_ptr(), AUDIO_CHUNK_I16 as c_int, &mut pts)
            };
            match ret {
                n if n > 0 => {
                    buf.truncate(n as usize);
                    self.audio_out.extend_from_slice(&buf);
                }
                0 => break,
                _ => break,
            }
        }
        // 防止长时间没人取(无声音域)时无限增长:最多保留约 2 秒。
        if self.audio_out.len() > cap {
            let drop = self.audio_out.len() - cap;
            self.audio_out.drain(..drop - drop % 2);
        }
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
}

impl MovieHost for LibavMovieHost {
    fn open(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        self.close();
        let session = Session::open(&self.temp_dir, file, bytes)?;
        let info = session.info();
        if info.width == 0 || info.height == 0 {
            return Err(fail("libav 未给出可解码的视频尺寸".to_string()));
        }
        let version = unsafe { CStr::from_ptr(ye_movie_version()) }
            .to_string_lossy()
            .into_owned();
        let audio = if info.sample_rate > 0 {
            format!("{}Hz/{}ch", info.sample_rate, info.channels)
        } else {
            "no".to_string()
        };
        log::info!(
            target: "yu-kernel",
            "[movie] libav {version} 解码:{} ({}x{}, fps={:.3}, 时长={:.1}s, 音频={audio})",
            String::from_utf8_lossy(file),
            info.width,
            info.height,
            if info.fps > 0.0 { info.fps } else { 0.0 },
            info.duration,
        );
        self.media = Some((info.width, info.height));
        self.audio = (info.sample_rate > 0).then(|| MovieAudioInfo {
            sample_rate: info.sample_rate,
            channels: 2,
        });
        self.session = Some(session);
        self.playing = false;
        self.ended = false;
        self.video_eof = false;
        self.video_errors = 0;
        self.position_ms = 0;
        self.start_ms = None;
        self.warned = false;
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), Diag> {
        if playing == self.playing {
            return Ok(());
        }
        self.playing = playing;
        // 暂停:把当前进度留在 position_ms(由 frame 最后写入);恢复时
        // 用 now - position 重新锚定。
        self.start_ms = None;
        if !playing {
            self.audio_out.clear();
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
        // 没有 session = `open` 失败或已 `close`;此时不能报“正在播放”,
        // 否则 `MOVIEINFO[PLAY]` 的等待循环无法退出。
        self.session.is_some() && self.playing && !self.ended
    }

    fn media_size(&mut self) -> Option<(u32, u32)> {
        self.media
    }

    fn frame(&mut self, now_ms: u64) -> Option<MovieFrame<'_>> {
        if self.session.is_none() {
            return None;
        }
        if self.playing && !self.ended {
            let start = *self
                .start_ms
                .get_or_insert(now_ms.saturating_sub(self.position_ms));
            let target = now_ms.saturating_sub(start) as f64 / 1000.0;
            self.position_ms = (target * 1000.0) as u64;
            self.pump(target);
            if self.video_eof && self.frames.is_empty() {
                self.ended = true;
                self.playing = false;
            }
        }
        self.current.as_ref().map(|frame| MovieFrame {
            width: frame.width,
            height: frame.height,
            rgba: &frame.rgba,
        })
    }

    fn take_audio(&mut self, out: &mut Vec<i16>) -> Option<MovieAudioInfo> {
        let info = self.audio?;
        out.extend_from_slice(&self.audio_out);
        self.audio_out.clear();
        Some(info)
    }

    fn backend_label(&self) -> &'static str {
        "libav"
    }

    fn close(&mut self) {
        self.session = None;
        self.media = None;
        self.audio = None;
        self.playing = false;
        self.ended = false;
        self.video_eof = false;
        self.video_errors = 0;
        self.position_ms = 0;
        self.start_ms = None;
        self.frames.clear();
        self.current = None;
        self.audio_out.clear();
        self.warned = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `open` 失败(无 session)后 `PLAY=1` 不能报“正在播放”。
    #[test]
    fn play_without_session_is_not_playing() {
        let mut host = LibavMovieHost::new(std::env::temp_dir());
        host.set_playing(true).expect("PLAY 本身不应报错");
        assert!(!host.is_playing(), "没有 session 不能报告正在播放");
        assert!(host.frame(1000).is_none());
    }
}
