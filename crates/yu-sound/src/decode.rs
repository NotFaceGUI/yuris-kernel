//! 解码：OGG Vorbis（lewton）、Ogg Opus（opus-pure）、RIFF WAVE PCM（hound）
//! 与 ACM WAVE（win32 ACM 转换，`acm` 模块）。
//!
//! 证据：
//! - 手册 `sound/sound.html`：可播放 OGG/WAVE；“特殊コーデック形式の WAVE
//!   には対応していません”。**该「不支持」句系过时**：REA 证明 0.495 对
//!   非 PCM WAVE 完整走 ACM 转换链（`0x422e04` else 分支），终态按
//!   fixed(no-manual) 实现并标注（DIV-SND-004）。
//! - 0.495 `yu-ris.exe`：静态链接 `vorbis`/`vorbisfile`（字符串 `vorbis`
//!   @ `0x5b8e5c`）；WAVE `fmt tag == 1` 走 PCM 直解，**其余全部 tag
//!   （含 3=float、0xFFFE=extensible）走 ACM**：`acmFormatSuggest(NULL,
//!   src, dst=PCM, 0x12, ACM_FORMATSUGGESTF_WFORMATTAG)` → `acmStreamOpen
//!   (…, NONREALTIME)` → `acmStreamSize` → `PrepareHeader` → `Convert`
//!   （整块一次）→ `Unprepare` → `Close`，`+0x107` 写 2；任一步失败 =
//!   装载失败（返回负错误码，层保持旧元数据）。7 个 ACM 导入的全部调用点
//!   都在该函数内（`0x423086..0x4232a4`，IAT 跳板 `0x47f86e..0x47f892`）。
//! - CODEC 取值（手册）：`0=WAVE / 1=OGG / 2=ACM WAVE`；REA 侧内部字段
//!   `+0x107` 的写入点：OGG `0x422ab0` 写 1，PCM WAVE `0x422e04` 写 0，
//!   ACM 分支写 2。
//! - **0.500 的 Ogg 流可以是 Opus**：官方 exe 的 12 项扩展名表
//!   （`0x1d95e0`）含 `.opus`，本作 `yscfg.sound_ext_order = [4,…]`、
//!   25,307 个 `.opus` 资产。Opus 仍在 Ogg 容器内，`CODEC` 仍报 1（OGG），
//!   只是音频编码是 Opus（RFC 6716/7845）。

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

/// 走 RIFF chunk 链找指定 chunk（`fmt `/`data`），不假设它是第一个 chunk
/// （JUNK/LIST 前置也成立）。返回 chunk 数据区的 `(偏移, 长度)`。
fn wav_chunk(bytes: &[u8], want: &[u8; 4]) -> Option<(usize, usize)> {
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
        if id == want && data + size <= bytes.len() {
            return Some((data, size));
        }
        // 防 0 长度 chunk 造成死循环；RIFF 的 chunk 按偶数字节对齐。
        if size == 0 && id != want {
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
    Err(error::recoverable(
        code::DECODE_FAILED,
        format!(
            "无法识别的声音容器（{} 字节；只取证 OGG/RIFF WAVE）",
            bytes.len()
        ),
    ))
}

fn decode_ogg(name: &[u8], bytes: &[u8]) -> Result<Pcm, Diag> {
    if crate::meta::ogg_is_opus(bytes) {
        return decode_opus(name, bytes);
    }
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

/// Ogg Opus 整段解码（非流式路径；流式见 `stream::Kind::Opus`）。
///
/// `Trim` 负责 RFC 7845 的 pre-skip 与末页 granule 的 end-trim；`decode_s16`
/// 与 libopus 的 `opus_decode` 同型（先 soft-clip 再转 16-bit）。
fn decode_opus(name: &[u8], bytes: &[u8]) -> Result<Pcm, Diag> {
    let mut reader = opus_pure::OggOpusReader::new(Cursor::new(bytes)).map_err(|e| {
        error::recoverable(
            code::DECODE_FAILED,
            format!("Ogg Opus 头解析失败({}): {e}", display(name)),
        )
    })?;
    let head = reader.head().clone();
    let channels = usize::from(head.channel_count);
    if channels == 0 {
        return Err(error::fatal(
            code::DECODE_FAILED,
            format!("Ogg Opus 头非法({}): channels=0", display(name)),
        ));
    }
    let mut decoder = head.decoder(48_000).map_err(|e| {
        error::recoverable(
            code::DECODE_FAILED,
            format!("Ogg Opus 解码器初始化失败({}): {e}", display(name)),
        )
    })?;
    let mut trim = opus_pure::Trim::new(&head, 48_000, channels).map_err(|e| {
        error::recoverable(
            code::DECODE_FAILED,
            format!("Ogg Opus pre-skip 解析失败({}): {e}", display(name)),
        )
    })?;
    let mut packet = opus_pure::OggPacket::new(Vec::new(), -1, false);
    let mut block = vec![0i16; opus_pure::MAX_PACKET_SAMPLES * channels];
    let mut samples: Vec<i16> = Vec::new();
    loop {
        match reader.read_packet_into(&mut packet) {
            Ok(false) => break,
            Ok(true) => {
                let n = decoder
                    .decode_s16(&packet.data, opus_pure::MAX_PACKET_SAMPLES, &mut block)
                    .map_err(|e| {
                        error::recoverable(
                            code::DECODE_FAILED,
                            format!("Ogg Opus 解码失败({}): {e}", display(name)),
                        )
                    })?;
                let audio = trim.keep(&packet, &block[..n * channels]);
                samples.extend_from_slice(audio);
            }
            Err(e) => {
                return Err(error::recoverable(
                    code::DECODE_FAILED,
                    format!("Ogg Opus 包读取失败({}): {e}", display(name)),
                ))
            }
        }
    }
    Ok(Pcm {
        samples,
        channels: channels as u16,
        bits: 16,
        // Opus 在 Ogg 里恒以 48 kHz 计 granule（RFC 7845 §4），解码也固定 48 kHz。
        sample_rate: 48_000,
        // 容器是 Ogg，官方 CODEC 仍报 1（手册 0=WAVE/1=OGG/2=ACM）。
        codec: Codec::Ogg,
    })
}

fn decode_wav(name: &[u8], bytes: &[u8]) -> Result<Pcm, Diag> {    // 官方 WAVE 装载（`0x422e04`）：`fmt tag == 1` 走 PCM 直解；
    // **其余全部 tag（含 3=float、0xFFFE=extensible）走 ACM 转换**，
    // 转换失败 = 装载失败（层保持旧元数据，N0-8）。手册的「不支持」句过时。
    match wav_chunk(bytes, b"fmt ") {
        Some((off, len)) if len >= 2 => {
            let tag = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
            if tag == 1 {
                decode_wav_pcm(name, bytes)
            } else {
                let (data_off, data_len) = wav_chunk(bytes, b"data").ok_or_else(|| {
                    error::recoverable(
                        code::DECODE_FAILED,
                        format!("WAVE 缺 data chunk({});官方 -0x2760 同型失败", display(name)),
                    )
                })?;
                crate::acm::decode_acm_wave(name, &bytes[off..off + len], &bytes[data_off..data_off + data_len])
            }
        }
        // 无 `fmt `：官方 `mmioDescend` 失败（-0x274c）。交 hound 产出
        // 同型失败诊断（保持既有文案路径）。
        _ => decode_wav_pcm(name, bytes),
    }
}

fn decode_wav_pcm(name: &[u8], bytes: &[u8]) -> Result<Pcm, Diag> {
    let mut reader = match hound::WavReader::new(Cursor::new(bytes)) {
        Ok(reader) => reader,
        // 仅 tag==1 到达此路（非 PCM 已走 ACM）；hound 拒收即 PCM 形态
        // 越界（如非法位深）＝解码失败，非「未实现」。
        Err(hound::Error::Unsupported) => {
            return Err(error::recoverable(
                code::DECODE_FAILED,
                format!("WAVE 编码不受支持({})", display(name)),
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
