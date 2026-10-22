//! 解码：OGG Vorbis（lewton）与 RIFF WAVE PCM（hound）。
//!
//! 证据：
//! - 手册 `sound/sound.html`：可播放 OGG/WAVE；“特殊コーデック形式の WAVE
//!   には対応していません”。
//! - 0.495 `yu-ris.exe`：静态链接 `vorbis`/`vorbisfile`（字符串 `vorbis`
//!   @ `0x5b8e5c`）；WAVE 普通 PCM 走 `mmio*`，非 PCM 走 ACM
//!   （`acmStreamOpen`/`acmFormatSuggest`/…）。
//! - CODEC 取值（手册）：`0=WAVE / 1=OGG / 2=ACM WAVE`；REA 侧内部字段
//!   `+0x107` 的写入点：OGG `0x422ab0` 写 1，PCM WAVE `0x422e04` 写 0，
//!   ACM 分支写 2。
//!
//! ACM WAVE 在 0.495 里由 ACM 转换后再播放；本 crate **不实现 ACM**
//! （缺 N0 级证据），返回 [`crate::error::code::CODEC_UNSUPPORTED`]。

use std::io::Cursor;

use yu_log::Diag;

use crate::error::{self, code};

/// 对外 CODEC 值（手册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Wav = 0,
    Ogg = 1,
    Acm = 2,
}

impl Codec {
    pub const fn value(self) -> i64 {
        self as i64
    }
}

/// 解码后的 PCM（i16 交错）。
#[derive(Clone, Debug)]
pub struct Pcm {
    pub samples: Vec<i16>,
    pub channels: u16,
    pub bits: u16,
    pub sample_rate: u32,
    pub codec: Codec,
}

impl Pcm {
    pub fn frames(&self) -> u64 {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() as u64 / u64::from(self.channels)
        }
    }

    /// `TOTALTIME`：手册“サウンドの長さ（単位：ミリ秒）”。
    /// 引擎实测按 `round(seconds*1000)`（0.495 WAVE 路径常量 `1000.0`）。
    pub fn total_ms(&self) -> i64 {
        if self.sample_rate == 0 {
            return 0;
        }
        let frames = self.frames() as u128;
        let ms = (frames * 1000 + u128::from(self.sample_rate) / 2) / u128::from(self.sample_rate);
        ms as i64
    }
}

/// 走 RIFF chunk 链找 `fmt `，不假设它是第一个 chunk（JUNK/LIST 前置也成立）。
fn wav_format_tag(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut off = 12usize;
    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let size = u32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]) as usize;
        let data = off + 8;
        if id == b"fmt " && size >= 2 && data + 2 <= bytes.len() {
            return Some(u16::from_le_bytes([bytes[data], bytes[data + 1]]));
        }
        // 防 0 长度 chunk 造成死循环；RIFF 的 chunk 按偶数字节对齐。
        if size == 0 && id != b"fmt " {
            off += 8;
        } else {
            off = data.saturating_add(size).saturating_add(size & 1);
        }
    }
    None
}

/// 解码入口。按魔数判容器（不依赖扩展名——探测序已由调用方完成）。
pub fn decode(name: &[u8], bytes: &[u8]) -> Result<Pcm, Diag> {
    if bytes.len() >= 4 && &bytes[0..4] == b"OggS" {
        return decode_ogg(name, bytes);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return decode_wav(name, bytes);
    }
    Err(error::unimplemented(
        code::DECODE_FAILED,
        format!(
            "无法识别的声音容器（{} 字节；只取证 OGG/RIFF WAVE）",
            bytes.len()
        ),
    ))
}

fn decode_ogg(name: &[u8], bytes: &[u8]) -> Result<Pcm, Diag> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes)).map_err(|e| {
        error::recoverable(
            code::DECODE_FAILED,
            format!("OGG 头解析失败({}): {e}", display(name)),
        )
    })?;
    let channels = u16::from(reader.ident_hdr.audio_channels);
    let sample_rate = reader.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();
    loop {
        match reader.read_dec_packet_itl() {
            Ok(Some(packet)) => samples.extend_from_slice(&packet),
            Ok(None) => break,
            Err(e) => {
                return Err(error::recoverable(
                    code::DECODE_FAILED,
                    format!("OGG 解码失败({}): {e}", display(name)),
                ))
            }
        }
    }
    if channels == 0 || sample_rate == 0 {
        return Err(error::fatal(
            code::DECODE_FAILED,
            format!(
                "OGG 头非法({}): channels={channels} rate={sample_rate}",
                display(name)
            ),
        ));
    }
    Ok(Pcm {
        samples,
        channels,
        // Vorbis 解出的是 16-bit PCM（lewton API 即 i16）。
        bits: 16,
        sample_rate,
        codec: Codec::Ogg,
    })
}

fn decode_wav(name: &[u8], bytes: &[u8]) -> Result<Pcm, Diag> {
    let tag = wav_format_tag(bytes);
    // 1 = PCM, 3 = IEEE float, 0xFFFE = extensible（交由 hound 判子格式）。
    if let Some(tag) = tag {
        if tag != 1 && tag != 3 && tag != 0xFFFE {
            return Err(error::unimplemented(
                code::CODEC_UNSUPPORTED,
                format!(
                    "WAVE 格式 0x{tag:04x}({}) 不在取证范围；ACM WAVE(CODEC=2)未实现",
                    display(name)
                ),
            ));
        }
    }
    let mut reader = match hound::WavReader::new(Cursor::new(bytes)) {
        Ok(reader) => reader,
        Err(hound::Error::Unsupported) => {
            return Err(error::unimplemented(
                code::CODEC_UNSUPPORTED,
                format!(
                    "WAVE 编码不受支持({})；ACM WAVE(CODEC=2)未实现",
                    display(name)
                ),
            ));
        }
        Err(e) => {
            return Err(error::recoverable(
                code::DECODE_FAILED,
                format!("WAVE 头解析失败({}): {e}", display(name)),
            ));
        }
    };

    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(error::fatal(
            code::DECODE_FAILED,
            format!(
                "WAVE 头非法({}): channels={} rate={}",
                display(name),
                spec.channels,
                spec.sample_rate
            ),
        ));
    }
    let mut samples: Vec<i16> = Vec::new();
    match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = if spec.bits_per_sample >= 32 {
                i32::MAX as f64
            } else {
                ((1i64 << (spec.bits_per_sample.max(1) - 1)) - 1) as f64
            };
            for s in reader.samples::<i32>() {
                let v = s.map_err(|e| {
                    error::recoverable(
                        code::DECODE_FAILED,
                        format!("WAVE 采样读取失败({}): {e}", display(name)),
                    )
                })?;
                samples.push(scale_to_i16(f64::from(v) / max));
            }
        }
        hound::SampleFormat::Float => {
            for s in reader.samples::<f32>() {
                let v = s.map_err(|e| {
                    error::recoverable(
                        code::DECODE_FAILED,
                        format!("WAVE 采样读取失败({}): {e}", display(name)),
                    )
                })?;
                samples.push(scale_to_i16(f64::from(v)));
            }
        }
    }
    Ok(Pcm {
        samples,
        channels: spec.channels,
        bits: spec.bits_per_sample,
        sample_rate: spec.sample_rate,
        codec: Codec::Wav,
    })
}

fn scale_to_i16(v: f64) -> i16 {
    let clamped = v.clamp(-1.0, 1.0);
    (clamped * 32767.0).round() as i16
}

fn display(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7f).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    s
}
