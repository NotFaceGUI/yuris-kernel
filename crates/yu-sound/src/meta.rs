//! 容器头部时长/格式探测(不整段解码)。
//!
//! 官方 `SOUNDINFO[TOTALTIME]` 不是解码帧数,而是 `ov_time_total`:0.495
//! OGG 装载 `0x422ab0` 只 `ov_open` 后调用 `FUN_004806c5`(返回秒 ×1000 写
//! `+0x120`),随后 `0x422c65` 只建 0.5s DirectSound 环 —— **不整段解码**。
//! 总时长在容器头部就有精确答案:
//!
//! - **OGG** :每个 page 头部 8 字节 granule(该 page 结束时已解出的 PCM
//!   样本数,按声道计)。首个逻辑流的最后一个 granule ≥ 0 的 page 即总帧数;
//!   ident header(`\x01vorbis`)给出声道数与采样率。
//! - **WAVE**:`fmt ` 给 tag/channels/rate/bits/block_align,`data` 长度 /
//!   block_align = 帧数(PCM `tag==1`)。非 PCM(tag≠1)官方走 ACM 转换,
//!   转换后的格式/长度只有实际转换才知道 ⇒ 本模块给 `total_frames=None`,
//!   调用方退回同步解码(不变语义)。
//!
//! 时长换算与 [`crate::decode::Pcm::total_ms`] **逐位同式**
//! (`(frames*1000 + rate/2) / rate`,四舍五入)。granule 与 lewton 末包裁剪
//! 在少量文件上不一致(语料 98/6966,≤36ms);`stream` 以官方 granule 为总长,
//! 解码样本多出截断、不足补静音,保证 TIME/END/LOOP 与官方自洽。

use yu_log::Diag;

use crate::decode::Codec;
use crate::error::{self, code};

/// 头部可得的音频元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioMeta {
    pub codec: Codec,
    pub channels: u16,
    pub bits: u16,
    pub sample_rate: u32,
    /// 精确总帧数(按声道计);`None` = 头部无法确定(ACM/畸形)。
    pub total_frames: Option<u64>,
}

impl AudioMeta {
    /// `TOTALTIME`(毫秒);与 `Pcm::total_ms` 同一换算。
    pub fn total_ms(&self) -> Option<i64> {
        let frames = self.total_frames?;
        if self.sample_rate == 0 {
            return Some(0);
        }
        let ms = (u128::from(frames) * 1000 + u128::from(self.sample_rate) / 2)
            / u128::from(self.sample_rate);
        Some(ms as i64)
    }

    /// 是否具备「不整段解码即可给出精确 TOTALTIME」的条件。
    pub fn is_exact(&self) -> bool {
        self.total_frames.is_some() && self.sample_rate != 0
    }
}

/// 探测容器头部。错误语义与 [`crate::decode::decode`] 同型(容器不可识别)。
pub fn probe(name: &[u8], bytes: &[u8]) -> Result<AudioMeta, Diag> {
    if bytes.len() >= 4 && &bytes[0..4] == b"OggS" {
        return probe_ogg(name, bytes);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return probe_wav(name, bytes);
    }
    Err(error::recoverable(
        code::DECODE_FAILED,
        format!(
            "无法识别的声音容器（{} 字节；只取证 OGG/RIFF WAVE）",
            bytes.len()
        ),
    ))
}

/// OGG Vorbis 头部:ident header + 首个逻辑流的最后 granule。
fn probe_ogg(name: &[u8], bytes: &[u8]) -> Result<AudioMeta, Diag> {
    let fail = |detail: String| {
        error::recoverable(
            code::DECODE_FAILED,
            format!("OGG 头解析失败({}): {detail}", display(name)),
        )
    };
    let page = ogg_scan(bytes).ok_or_else(|| fail("page 链不完整".to_string()))?;
    let payload = page
        .first_payload
        .ok_or_else(|| fail("缺首个 page 载荷".to_string()))?;
    // ident packet: [0x01]["vorbis"][version u32][channels u8][rate u32]…
    if payload.len() < 16 || payload[0] != 0x01 || &payload[1..7] != b"vorbis" {
        return Err(fail("首个包不是 Vorbis ident header".to_string()));
    }
    let channels = u16::from(payload[11]);
    let sample_rate = u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]);
    if channels == 0 || sample_rate == 0 {
        return Err(error::fatal(
            code::DECODE_FAILED,
            format!(
                "OGG 头非法({}): channels={channels} rate={sample_rate}",
                display(name)
            ),
        ));
    }
    // 首个逻辑流(serial)的最后一个有效 granule。分链 OGG(chained)在语料
    // 里不存在;若出现第二个含音频的逻辑流,`ogg_scan` 会把 total 置 None,
    // 调用方退回同步解码,不猜。
    Ok(AudioMeta {
        codec: Codec::Ogg,
        channels,
        // Vorbis 解出的是 16-bit PCM(lewton API 即 i16)。
        bits: 16,
        sample_rate,
        total_frames: page.total_frames,
    })
}

struct OggScan<'a> {
    first_payload: Option<&'a [u8]>,
    total_frames: Option<u64>,
}

/// 扫描 Ogg page 链:首个 page 的载荷(ident) + 首个逻辑流的总帧数。
///
/// 快速路径:直接从文件尾向前找最后一个完整 page 的 granule(官方
/// `ov_time_total` 也是读最后 page 的 `pcmlengths`,不必顺序扫全网)。
/// 只有尾页不可识别(截断/分链/尾部垃圾)时才退回顺序扫描。
fn ogg_scan(bytes: &[u8]) -> Option<OggScan<'_>> {
    let first = first_page(bytes)?;
    let first_payload = Some(first.payload);
    // 尾页 granule 快速路径:先看最后 8KB(绝大多数 EOS page 很小),
    // 再扩到 160KB(单 page body 上限 65,307 字节)。
    if let Some(total) = last_granule(bytes, first.serial, 8 * 1024)
        .or_else(|| last_granule(bytes, first.serial, 160 * 1024))
    {
        return Some(OggScan {
            first_payload,
            total_frames: Some(total),
        });
    }
    // 慢路径:顺序扫描整条链,同时检测分链(第二个含音频的逻辑流)。
    scan_forward(bytes, first_payload)
}

struct FirstPage<'a> {
    serial: u32,
    payload: &'a [u8],
}

/// 首个 page(允许开头有 ID3 等杂字节)。
fn first_page(bytes: &[u8]) -> Option<FirstPage<'_>> {
    let search_end = bytes.len().min(64 * 1024);
    let mut off = 0usize;
    while off + 27 <= search_end {
        if &bytes[off..off + 4] == b"OggS" {
            if let Some(page) = parse_page(bytes, off) {
                return Some(FirstPage {
                    serial: page.serial,
                    payload: &bytes[page.payload_start..page.payload_end],
                });
            }
        }
        off += 1;
    }
    None
}

struct Page {
    serial: u32,
    granule: i64,
    payload_start: usize,
    payload_end: usize,
}

fn parse_page(bytes: &[u8], off: usize) -> Option<Page> {
    if off + 27 > bytes.len() || &bytes[off..off + 4] != b"OggS" {
        return None;
    }
    let granule = i64::from_le_bytes([
        bytes[off + 6],
        bytes[off + 7],
        bytes[off + 8],
        bytes[off + 9],
        bytes[off + 10],
        bytes[off + 11],
        bytes[off + 12],
        bytes[off + 13],
    ]);
    let serial = u32::from_le_bytes([
        bytes[off + 14],
        bytes[off + 15],
        bytes[off + 16],
        bytes[off + 17],
    ]);
    let segments = usize::from(bytes[off + 26]);
    if segments == 0 || off + 27 + segments > bytes.len() {
        return None;
    }
    let mut body = 0usize;
    for &lace in &bytes[off + 27..off + 27 + segments] {
        body += usize::from(lace);
    }
    let payload_start = off + 27 + segments;
    let payload_end = payload_start.checked_add(body)?;
    if payload_end > bytes.len() {
        return None;
    }
    Some(Page {
        serial,
        granule,
        payload_start,
        payload_end,
    })
}

/// 从文件尾向前找最后一条属于 `serial` 的完整 page,返回其 granule。
///
/// 候选页必须正好结束在文件尾(允许尾部 0 填充),避免把音频数据里偶然的
/// "OggS" 当成页头。
fn last_granule(bytes: &[u8], serial: u32, window: usize) -> Option<u64> {
    if bytes.len() < 27 {
        return None;
    }
    let start = bytes.len().saturating_sub(window);
    let mut i = bytes.len() - 4;
    loop {
        if i < start {
            return None;
        }
        if &bytes[i..i + 4] == b"OggS" {
            if let Some(page) = parse_page(bytes, i) {
                let trailing = &bytes[page.payload_end..];
                if page.serial == serial
                    && page.granule >= 0
                    && trailing.iter().all(|&b| b == 0)
                {
                    return Some(page.granule as u64);
                }
            }
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

/// 慢路径:顺序扫描整条链。
fn scan_forward<'a>(
    bytes: &'a [u8],
    first_payload: Option<&'a [u8]>,
) -> Option<OggScan<'a>> {
    let mut off = 0usize;
    let search_end = bytes.len().min(64 * 1024);
    while off + 4 <= search_end && &bytes[off..off + 4] != b"OggS" {
        off += 1;
    }
    let mut serial: Option<u32> = None;
    let mut last_granule: Option<u64> = None;
    let mut other_audio_stream = false;
    while off + 27 <= bytes.len() {
        let page = parse_page(bytes, off)?;
        match serial {
            None => serial = Some(page.serial),
            Some(first) => {
                if first != page.serial && page.granule > 0 {
                    // 分链 OGG:总时长是各流之和,但 lewton 的
                    // `OggStreamReader` 只读一条流 ⇒ 不猜,置 None。
                    other_audio_stream = true;
                }
            }
        }
        if Some(page.serial) == serial && page.granule >= 0 {
            last_granule = Some(page.granule as u64);
        }
        off = page.payload_end;
    }
    Some(OggScan {
        first_payload,
        total_frames: if other_audio_stream { None } else { last_granule },
    })
}

/// WAVE 头部:`fmt ` + `data` 长度。非 PCM 走 ACM,时长未知。
fn probe_wav(name: &[u8], bytes: &[u8]) -> Result<AudioMeta, Diag> {
    let (fmt_off, fmt_len) = wav_chunk(bytes, b"fmt ").ok_or_else(|| {
        error::recoverable(
            code::DECODE_FAILED,
            format!("WAVE 头解析失败({}): 缺 fmt chunk", display(name)),
        )
    })?;
    if fmt_len < 16 {
        return Err(error::recoverable(
            code::DECODE_FAILED,
            format!("WAVE fmt chunk 过短({}): {fmt_len} 字节", display(name)),
        ));
    }
    let tag = u16::from_le_bytes([bytes[fmt_off], bytes[fmt_off + 1]]);
    let channels = u16::from_le_bytes([bytes[fmt_off + 2], bytes[fmt_off + 3]]);
    let sample_rate = u32::from_le_bytes([
        bytes[fmt_off + 4],
        bytes[fmt_off + 5],
        bytes[fmt_off + 6],
        bytes[fmt_off + 7],
    ]);
    let block_align = u16::from_le_bytes([bytes[fmt_off + 12], bytes[fmt_off + 13]]);
    let bits = u16::from_le_bytes([bytes[fmt_off + 14], bytes[fmt_off + 15]]);
    if channels == 0 || sample_rate == 0 {
        return Err(error::fatal(
            code::DECODE_FAILED,
            format!(
                "WAVE 头非法({}): channels={channels} rate={sample_rate}",
                display(name)
            ),
        ));
    }
    if tag != 1 {
        // 官方 0.495:非 PCM 全部走 ACM 转换(`0x422e04` else 分支)。转换后的
        // 输出格式与长度只有 ACM 实际运行才知道 ⇒ 元数据/时长交回同步解码。
        return Ok(AudioMeta {
            codec: Codec::Acm,
            channels: 0,
            bits: 0,
            sample_rate: 0,
            total_frames: None,
        });
    }
    let total_frames = wav_chunk(bytes, b"data").and_then(|(_, len)| {
        let align = usize::from(block_align);
        (align > 0).then_some((len / align) as u64)
    });
    Ok(AudioMeta {
        codec: Codec::Wav,
        channels,
        bits,
        sample_rate,
        total_frames,
    })
}

/// 走 RIFF chunk 链找指定 chunk(`fmt `/`data`),不假设它是第一个 chunk。
/// 与 `decode` 的内部实现同口径(返回数据区 `(偏移, 长度)`)。
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
        if size == 0 && id != want {
            off += 8;
        } else {
            off = data.saturating_add(size).saturating_add(size & 1);
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小 RIFF PCM WAVE:`fmt ` + `data`,可指定帧数。
    fn wav(frames: u32, rate: u32, channels: u16) -> Vec<u8> {
        let block = channels * 2;
        let data = frames * u32::from(block);
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out.resize(out.len() + data as usize, 0);
        out
    }

    #[test]
    fn wav_total_frames_from_data_len() {
        let meta = probe(b"a", &wav(44100, 44100, 2)).expect("probe");
        assert_eq!(meta.codec, Codec::Wav);
        assert_eq!(meta.channels, 2);
        assert_eq!(meta.sample_rate, 44100);
        assert_eq!(meta.total_frames, Some(44100));
        assert_eq!(meta.total_ms(), Some(1000));
        assert!(meta.is_exact());
    }

    #[test]
    fn wav_total_matches_pcm_conversion_exactly() {
        // 逐值同式:非整毫秒的帧数也要一致。
        for (frames, rate) in [(1u32, 44100u32), (1000, 44100), (132300, 44100), (48001, 48000)] {
            let bytes = wav(frames, rate, 1);
            let meta = probe(b"a", &bytes).expect("probe");
            let pcm = crate::decode::decode(b"a", &bytes).expect("decode");
            assert_eq!(meta.total_ms(), Some(pcm.total_ms()), "frames={frames} rate={rate}");
        }
    }

    #[test]
    fn non_pcm_wav_reports_unknown_duration() {
        let mut bytes = wav(10, 44100, 1);
        // fmt tag 3 = IEEE float;官方走 ACM。
        bytes[20] = 3;
        let meta = probe(b"a", &bytes).expect("probe");
        assert_eq!(meta.codec, Codec::Acm);
        assert!(!meta.is_exact());
        assert_eq!(meta.total_ms(), None);
    }

    #[test]
    fn ogg_ident_and_granule_are_read_without_decoding() {
        // 手工最小 Ogg 链:两个 page,首个含 ident,末尾 granule=12345。
        fn page(serial: u32, granule: i64, payload: &[u8]) -> Vec<u8> {
            assert!(payload.len() <= 255);
            let mut out = Vec::new();
            out.extend_from_slice(b"OggS");
            out.push(0); // version
            out.push(if granule < 0 { 0x00 } else { 0x04 }); // header type
            out.extend_from_slice(&granule.to_le_bytes());
            out.extend_from_slice(&serial.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // seq
            out.extend_from_slice(&0u32.to_le_bytes()); // crc(不校验)
            out.push(1); // segments
            out.push(payload.len() as u8);
            out.extend_from_slice(payload);
            out
        }
        let mut ident = vec![0x01];
        ident.extend_from_slice(b"vorbis");
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.push(2);
        ident.extend_from_slice(&44100u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.push(0xB8);
        let mut bytes = page(7, 0, &ident);
        bytes.extend_from_slice(&page(7, 12345, &[0u8; 4]));
        let meta = probe(b"a.ogg", &bytes).expect("probe");
        assert_eq!(meta.codec, Codec::Ogg);
        assert_eq!(meta.channels, 2);
        assert_eq!(meta.sample_rate, 44100);
        assert_eq!(meta.total_frames, Some(12345));
        let expected = ((12345u128 * 1000 + 44100 / 2) / 44100) as i64;
        assert_eq!(meta.total_ms(), Some(expected));
    }

    #[test]
    fn unknown_container_fails_like_decode() {
        let err = probe(b"a", b"nope").expect_err("must fail");
        assert_eq!(err.code, code::DECODE_FAILED);
    }
}
