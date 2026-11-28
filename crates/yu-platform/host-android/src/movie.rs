//! Android 外置影片后端:**系统媒体栈(MediaCodec)优先,FFmpeg 软解兜底**。
//!
//! 两条路的覆盖面正好互补:
//!
//! - MediaCodec:MP4/MKV/WebM/TS 容器 + H.264/H.265/VP8/VP9/AV1(硬解);
//! - libav:ASF(WMV)/VC-1/WMA、AVI、MPEG-PS 等系统栈不认的容器与编码。
//!
//! YU-RIS 5xx 的 OP/ED 是 ASF + VC-1,系统栈必然打不开;桌面能播是因为
//! 桌面注入的是 `yu-movie-ffmpeg`(系统 ffmpeg 子进程)。这里按容器魔数
//! 先选更可能成功的后端,失败再换另一条;内核/脚本侧语义完全不变。

use std::path::PathBuf;

use yu_log::{Boundary, Diag};
use yu_movie_android::AndroidMovieHost;
use yu_movie_libav::LibavMovieHost;
use yu_rt::event::{MovieAudioInfo, MovieFrame, MovieGeometry, MovieHost};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Active {
    MediaCodec,
    Libav,
}

/// 组合后端。
pub struct AndroidMovieHosts {
    mc: AndroidMovieHost,
    libav: LibavMovieHost,
    active: Option<Active>,
}

impl AndroidMovieHosts {
    /// `temp_dir`:app 私有 cache 目录(宿主从 `getCacheDir()` 传入)。
    #[must_use]
    pub fn new(temp_dir: PathBuf) -> Self {
        Self {
            mc: AndroidMovieHost::new(temp_dir.clone()),
            libav: LibavMovieHost::new(temp_dir),
            active: None,
        }
    }

    fn active_mut(&mut self) -> Option<&mut (dyn MovieHost + '_)> {
        match self.active {
            Some(Active::MediaCodec) => Some(&mut self.mc),
            Some(Active::Libav) => Some(&mut self.libav),
            None => None,
        }
    }

    fn open_mc(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        self.mc.open(file, bytes)?;
        self.active = Some(Active::MediaCodec);
        Ok(())
    }

    fn open_libav(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        self.libav.open(file, bytes)?;
        self.active = Some(Active::Libav);
        Ok(())
    }
}

/// 判断容器是否在 Android 系统 extractor 的覆盖范围内。
///
/// 只认**容器魔数**,不猜编码:命中后仍可能因设备缺解码器失败,失败会
/// 回退 FFmpeg。ASF/AVI/MPEG-PS/FLV 与未知格式一律先走 FFmpeg。
fn media_codec_friendly(bytes: &[u8]) -> bool {
    // ISO BMFF(MP4/MOV/3GP):`[size]["ftyp"]`。
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return true;
    }
    // Matroska/WebM:EBML 头。
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return true;
    }
    // Ogg。
    if bytes.starts_with(b"OggS") {
        return true;
    }
    // MPEG-TS:0x47 同相(0 与 188)。
    if bytes.len() > 377 && bytes[0] == 0x47 && bytes[188] == 0x47 {
        return true;
    }
    false
}

impl MovieHost for AndroidMovieHosts {
    fn open(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        self.close();
        let prefer_mc = media_codec_friendly(bytes);
        if prefer_mc {
            match self.open_mc(file, bytes) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    log::info!(
                        target: "yu-kernel",
                        "[movie] 系统解码打不开 {}，改用 FFmpeg 软解: {error}",
                        String::from_utf8_lossy(file)
                    );
                    self.mc.close();
                    return self.open_libav(file, bytes);
                }
            }
        }
        match self.open_libav(file, bytes) {
            Ok(()) => Ok(()),
            Err(error) => {
                log::info!(
                    target: "yu-kernel",
                    "[movie] FFmpeg 软解打不开 {}，尝试系统解码: {error}",
                    String::from_utf8_lossy(file)
                );
                self.libav.close();
                match self.open_mc(file, bytes) {
                    Ok(()) => Ok(()),
                    Err(_) => {
                        // 两条都失败:以更可能是根因的 libav 错误为主。
                        self.active = None;
                        Err(Diag::recoverable("movie.decode", Boundary::Platform)
                            .with_detail(error.detail.clone()))
                    }
                }
            }
        }
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), Diag> {
        match self.active_mut() {
            Some(host) => host.set_playing(playing),
            None => Ok(()),
        }
    }

    fn set_volume(&mut self, volume: i64) {
        if let Some(host) = self.active_mut() {
            host.set_volume(volume);
        }
    }

    fn set_geometry(&mut self, geometry: MovieGeometry) {
        if let Some(host) = self.active_mut() {
            host.set_geometry(geometry);
        }
    }

    fn is_playing(&mut self) -> bool {
        self.active_mut().is_some_and(|host| host.is_playing())
    }

    fn media_size(&mut self) -> Option<(u32, u32)> {
        self.active_mut().and_then(|host| host.media_size())
    }

    fn frame(&mut self, now_ms: u64) -> Option<MovieFrame<'_>> {
        self.active_mut()?.frame(now_ms)
    }

    fn take_audio(&mut self, out: &mut Vec<i16>) -> Option<MovieAudioInfo> {
        self.active_mut()?.take_audio(out)
    }

    fn backend_label(&self) -> &'static str {
        match self.active {
            Some(Active::MediaCodec) => "android(mediacodec)",
            Some(Active::Libav) => "android(libav)",
            None => "android(none)",
        }
    }

    fn close(&mut self) {
        self.mc.close();
        self.libav.close();
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_sniff_prefers_ffmpeg_for_asf_and_avi() {
        // ASF GUID(SV 的 op.ypf 实际内容)。
        let asf = [
            0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62,
            0xCE, 0x6C,
        ];
        assert!(!media_codec_friendly(&asf));
        assert!(!media_codec_friendly(b"RIFF\x00\x00\x00\x00AVI "));
        assert!(!media_codec_friendly(&[0x00, 0x00, 0x01, 0xBA, 0x21]));
        // MP4 / Matroska 走系统栈。
        assert!(media_codec_friendly(b"\x00\x00\x00\x18ftypisom"));
        assert!(media_codec_friendly(&[0x1A, 0x45, 0xDF, 0xA3, 0x01]));
    }
}
