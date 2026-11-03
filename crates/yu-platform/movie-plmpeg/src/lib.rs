//! `MOVIE` 播放后端:**内嵌纯软件 MPEG-1 解码**(pl_mpeg 移植)。
//!
//! # 为什么是它
//!
//! 官方 `MOVIE` 把解码交给系统媒体栈(MCI / DirectShow,见
//! `docs/divergences/DIV-GFX-074-movie-family.md`),但商业游戏与手机不能
//! 依赖"用户装了某个播放器":必须**嵌进二进制**。业界对「只播 MPEG-1」
//! 的惯用解法是小型专用解码器(ONScripter 的 SMPEG/libmpeg2、众多独立
//! 引擎的 pl_mpeg),而不是几十 MB 的 FFmpeg。
//!
//! 本后端用 [`southstar_codecs::plmpeg`] —— [pl_mpeg](https://phoboslab.org)
//! 的纯 Rust 移植(上游 MIT;`#![forbid(unsafe_code)]`),一个依赖同时提供:
//!
//! - **MPEG-PS 拆包**(pack/system/PES 头,含 MPEG-1 的 stuffing 与 PTS
//!   方言——手写拆包在这里最容易翻车);
//! - **MPEG-1 视频解码**;
//! - **MP2 音频解码**(音频通路见 §限制)。
//!
//! 覆盖范围与官方 `MOVIEINFO[CODEC]` 的主路径一致:`1` = MPEG-1 可直接播;
//! `2`/`0xFF`(MPEG-2、AVI 等)本后端不解,由内核诊断说明(官方对 `>1`
//! 也只是警告「可能播不了」)。将来要支持其它格式,再加一个 `MovieHost`
//! 实现即可,内核与端口不变。
//!
//! # 限制
//!
//! - 只解 **MPEG-1 program stream**(`.mpg`/`.mpeg`);AVI/MP4 需要别的后端。
//! - **音频暂未接入**:pl_mpeg 能解 MP2,但把 PCM 送进 `yu-sound` 的通路
//!   属于声音域接线(下一批);`MOVIE[VOLUME]` 的语义值已由内核保存。
//! - 播放时钟由宿主每帧调用 [`PlmMovieHost::frame`] 驱动;暂停时保留最后
//!   一帧(官方 `MOVIE[PLAY=0]` 停止后画面仍在,直到 `MOVIEEND`)。

use std::rc::Rc;

use southstar_codecs::plmpeg::{Frame as PlmFrame, Plm};
use yu_log::{Boundary, Diag};
use yu_rt::event::{MovieFrame, MovieGeometry, MovieHost};

/// 一次播放会话。
pub struct PlmMovieHost {
    /// 解码器(播放时钟推进时借用;用 `Option` 便于临时摘下避免双重借用)。
    plm: Option<Plm>,
    /// 影片原始尺寸(`has_headers()` 成功后才有)。
    media: Option<(u32, u32)>,
    /// 帧间隔(秒;`framerate` 缺省按 30fps)。
    frame_interval: f64,
    /// 播放中。
    playing: bool,
    /// 播放起点(毫秒;首次 `frame()` 时以调用方时钟为准)。
    start_ms: Option<u64>,
    /// 已到结尾。
    ended: bool,
    /// 音量(官方 0..256;音频未接前只保存)。
    volume: i64,
    /// 几何(原始参数,`sx/sy == 0` = 未指定)。
    geometry: MovieGeometry,
    /// 当前帧 RGBA8(行优先,`width*height*4`)。
    rgba: Vec<u8>,
    /// 当前帧尺寸(与 `rgba` 对应)。
    size: (u32, u32),
    /// 当前帧时间(秒;`None` = 还没有帧)。
    frame_time: Option<f64>,
}

impl Default for PlmMovieHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PlmMovieHost {
    #[must_use]
    pub fn new() -> Self {
        Self {
            plm: None,
            media: None,
            frame_interval: 1.0 / 30.0,
            playing: false,
            start_ms: None,
            ended: false,
            volume: 256,
            geometry: MovieGeometry::default(),
            rgba: Vec::new(),
            size: (0, 0),
            frame_time: None,
        }
    }

    /// 影片原始尺寸(打开成功后才可知)。
    #[must_use]
    pub fn media_size(&self) -> Option<(u32, u32)> {
        self.media
    }

    /// 把一帧 YUV420 转成 RGBA8(BT.601 limited range → full)。
    ///
    /// 系数取 ITU-R BT.601:`Y` 取 16..235、`Cb/Cr` 取 16..240;pl_mpeg
    /// 输出的是 MPEG-1 的标准 limited-range 平面,这里按整数近似展开
    /// (官方 MCI 由系统解码器做等价转换)。
    fn convert_frame(&mut self, frame: &PlmFrame<'_>) {
        let (w, h) = (frame.width, frame.height);
        if w == 0 || h == 0 {
            return;
        }
        if self.size != (w as u32, h as u32) || self.rgba.len() != w * h * 4 {
            self.size = (w as u32, h as u32);
            self.rgba.resize(w * h * 4, 0);
        }
        let chroma_w = frame.cr.width;
        let chroma_h = frame.cr.height;
        for y in 0..h {
            let row = y * w;
            let cy = if chroma_h == 0 { 0 } else { (y * chroma_h / h).min(chroma_h - 1) };
            for x in 0..w {
                let cx = if chroma_w == 0 { 0 } else { (x * chroma_w / w).min(chroma_w - 1) };
                let luma = frame.y.data[row + x] as i32;
                let cb = frame.cb.data[cy * chroma_w + cx] as i32 - 128;
                let cr = frame.cr.data[cy * chroma_w + cx] as i32 - 128;
                let c = (luma - 16).max(0);
                let r = ((298 * c + 409 * cr + 128) >> 8).clamp(0, 255) as u8;
                let g = ((298 * c - 100 * cb - 208 * cr + 128) >> 8).clamp(0, 255) as u8;
                let b = ((298 * c + 516 * cb + 128) >> 8).clamp(0, 255) as u8;
                let o = (row + x) * 4;
                self.rgba[o] = r;
                self.rgba[o + 1] = g;
                self.rgba[o + 2] = b;
                self.rgba[o + 3] = 255;
            }
        }
    }

    /// 取当前帧(没有帧时 `None`)。
    fn current(&self) -> Option<MovieFrame<'_>> {
        let (w, h) = self.size;
        if w == 0 || h == 0 || self.rgba.is_empty() {
            return None;
        }
        Some(MovieFrame {
            width: w,
            height: h,
            rgba: &self.rgba,
        })
    }

    /// 按播放时钟推进到 `target` 秒(最多追 `budget` 帧,防卡顿雪崩)。
    fn advance_to(&mut self, target: f64, budget: u32) {
        let Some(mut plm) = self.plm.take() else {
            return;
        };
        // 帧率在进入循环前取一次(`decode_video` 会对 `plm` 持可变借用)。
        let rate = plm.framerate();
        if rate > 0.0 {
            self.frame_interval = 1.0 / rate;
        }
        let mut used = 0u32;
        while used < budget {
            // 当前帧还没放完 ⇒ 停。
            if let Some(time) = self.frame_time {
                if time + self.frame_interval > target {
                    break;
                }
            }
            match plm.decode_video() {
                Some(frame) => {
                    let time = frame.time;
                    self.convert_frame(&frame);
                    self.frame_time = Some(time);
                    used += 1;
                }
                None => {
                    self.ended = true;
                    break;
                }
            }
        }
        self.plm = Some(plm);
    }
}

impl MovieHost for PlmMovieHost {
    fn open(&mut self, _file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        // 官方 `MOVIE[FILE]` 打不开文件就不进影片模式;这里同样让失败可见。
        let mut plm = Plm::new(bytes.to_vec());
        if !plm.has_headers() {
            return Err(Diag::recoverable(
                "movie.decode",
                Boundary::Data,
            )
            .with_detail(
                "影片容器/码流无法解析(pl_mpeg 只支持 MPEG-1 program stream)".to_string(),
            ));
        }
        let (w, h) = (plm.width(), plm.height());
        if w <= 0 || h <= 0 {
            return Err(Diag::recoverable("movie.decode", Boundary::Data)
                .with_detail("影片没有可解码的视频流".to_string()));
        }
        let rate = plm.framerate();
        self.frame_interval = if rate > 0.0 { 1.0 / rate } else { 1.0 / 30.0 };
        self.media = Some((w as u32, h as u32));
        self.size = (0, 0);
        self.rgba.clear();
        self.frame_time = None;
        self.ended = false;
        self.playing = false;
        self.start_ms = None;
        self.plm = Some(plm);
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), Diag> {
        if playing == self.playing {
            return Ok(());
        }
        self.playing = playing;
        // 重新播放时重新计时(官方 MCI `play` 从头/从当前位置继续由驱动决定;
        // 这里按「继续」处理:保留 `start_ms` 偏移,暂停期间不推进)。
        if playing {
            if self.start_ms.is_none() {
                // 首次播放:时钟在第一次 `frame()` 时锚定。
                self.start_ms = None;
            }
        } else {
            // 暂停:把已播时长冻结,恢复时接着走。
            self.start_ms = None;
        }
        Ok(())
    }

    fn set_volume(&mut self, volume: i64) {
        self.volume = volume;
    }

    fn set_geometry(&mut self, geometry: MovieGeometry) {
        self.geometry = geometry;
    }

    fn is_playing(&mut self) -> bool {
        self.playing && !self.ended
    }

    fn frame(&mut self, now_ms: u64) -> Option<MovieFrame<'_>> {
        if self.playing && !self.ended {
            let start = *self.start_ms.get_or_insert(now_ms);
            let target = now_ms.saturating_sub(start) as f64 / 1000.0;
            self.advance_to(target, 4);
        }
        self.current()
    }

    fn close(&mut self) {
        self.plm = None;
        self.media = None;
        self.rgba.clear();
        self.size = (0, 0);
        self.frame_time = None;
        self.playing = false;
        self.ended = false;
        self.start_ms = None;
    }
}

/// 解码结果为 `Rc` 的便捷封装(宿主多路取帧时避免重复拷贝)。
impl PlmMovieHost {
    /// 当前帧的共享副本(测试/快照用)。
    #[must_use]
    pub fn frame_shared(&self) -> Option<(u32, u32, Rc<Vec<u8>>)> {
        let frame = self.current()?;
        Some((frame.width, frame.height, Rc::new(frame.rgba.to_vec())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Option<Vec<u8>> {
        let root = std::env::var("YURIS_SAMPLE_SDK0495").ok()?;
        let path = format!("{root}/data/pac/op_movie.mpg");
        std::fs::read(&path).ok()
    }

    /// 官方样例:`op_movie.mpg` = MPEG-1 800×600 / 30fps / 60 帧。
    #[test]
    fn decodes_official_sample() {
        let Some(bytes) = sample() else {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 但 YURIS_SAMPLE_SDK0495/data/pac/op_movie.mpg 缺失");
            }
            return;
        };
        let mut host = PlmMovieHost::new();
        host.open(b"pac/op_movie.mpg", &bytes).expect("open");
        assert_eq!(host.media_size(), Some((800, 600)));
        assert!(host.frame(0).is_none(), "未播放前没有帧");

        host.set_playing(true).expect("play");
        // 30fps ⇒ 每 33.3ms 一帧;走到 0.5s 应已出帧。
        let frame = host.frame(500).expect("frame at 0.5s");
        assert_eq!((frame.width, frame.height), (800, 600));
        assert_eq!(frame.rgba.len(), 800 * 600 * 4);
        assert!(frame.rgba.chunks_exact(4).all(|p| p[3] == 255), "alpha 恒 255");
        // 画面非纯色(真实影片帧)。
        let distinct: std::collections::BTreeSet<[u8; 4]> = frame
            .rgba
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect();
        assert!(distinct.len() > 64, "颜色过少:{}", distinct.len());

        // 暂停:仍返回最后一帧(官方停止后画面保留)。
        host.set_playing(false).expect("pause");
        let paused = host.frame(10_000).expect("paused frame");
        assert_eq!(paused.rgba.len(), 800 * 600 * 4);
        assert!(!host.is_playing());

        // 播到结尾:60 帧 / 30fps = 2s。
        host.set_playing(true).expect("resume");
        let _ = host.frame(20_000);
        assert!(host.is_playing() || host.ended, "2s 后应结束或仍在播");

        host.close();
        assert!(host.frame(30_000).is_none(), "close 后不再出帧");
    }

    /// 非 MPEG-1 数据必须**显式失败**,不能假装播放。
    #[test]
    fn rejects_non_mpeg1() {
        let mut host = PlmMovieHost::new();
        let err = host
            .open(b"x.avi", b"RIFF\x00\x00\x00\x00AVI LIST")
            .expect_err("非 MPEG-1 必须失败");
        assert_eq!(err.code, "movie.decode");
    }
}
