//! 统一文本边界:按 codec 推进字符、宽容解码、SJIS 隧道映射。
//!
//! 这是全仓**唯一**的“字节 ↔ 字符”边界。VM 的字符串操作、GFX 排版、
//! 渲染字形、声音文件名都只能通过这里选 codec，禁止各自写 CP932 区间。

use std::collections::BTreeMap;

use yu_log::{Boundary, Diag};

use crate::codec::Codec;
use crate::code;

/// 字符串语义域。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StringDomain {
    /// 普通脚本字符串(变量、比较、命令参数)。
    #[default]
    Script,
    /// 显示文本(WORD/选项/人名/文本绘制)。
    Text,
    /// 资源/包内路径。
    Path,
}

impl StringDomain {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            StringDomain::Script => "script",
            StringDomain::Text => "text",
            StringDomain::Path => "path",
        }
    }
}

/// 三域 codec。默认全 CP932(原版);补丁由 manifest/profile 覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodecConfig {
    pub script: Codec,
    pub text: Codec,
    pub name: Codec,
}

impl Default for CodecConfig {
    fn default() -> Self {
        Self {
            script: Codec::Cp932,
            text: Codec::Cp932,
            name: Codec::Cp932,
        }
    }
}

impl CodecConfig {
    #[must_use]
    pub const fn all(codec: Codec) -> Self {
        Self {
            script: codec,
            text: codec,
            name: codec,
        }
    }

    #[must_use]
    pub const fn for_domain(self, domain: StringDomain) -> Codec {
        match domain {
            StringDomain::Script => self.script,
            StringDomain::Text => self.text,
            StringDomain::Path => self.name,
        }
    }

    #[must_use]
    pub fn summary(self) -> String {
        format!(
            "script={} text={} name={}",
            self.script.as_str(),
            self.text.as_str(),
            self.name.as_str()
        )
    }
}

/// 显示层配置:全局 codec + 可选隧道表。
///
/// 与官方 CHS 同构:汉化补丁把整包脚本文本换成另一种编码,引擎用同一张
/// 首字节表解读全部文本,**不逐文件/逐载荷区分**。隧道是另一条路线
/// (`sjis_ext.bin`),仍保持 CP932 + 查表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayConfig {
    pub default: Codec,
    pub tunnel: Option<TunnelTable>,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            default: Codec::Cp932,
            tunnel: None,
        }
    }
}

impl DisplayConfig {
    #[must_use]
    pub const fn codec(&self) -> Codec {
        self.default
    }
}

/// SJIS 隧道表(`sjis_ext.bin`,UTF-16LE 字符序列)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelTable {
    chars: Vec<char>,
    map: BTreeMap<[u8; 2], char>,
    digest: u64,
}

impl TunnelTable {
    /// 解析 UTF-16LE 表;空表/奇数长度/孤立代理显式报错。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Diag> {
        if !bytes.len().is_multiple_of(2) {
            return Err(Diag::fatal(code::CODEC_INVALID, Boundary::Format).with_detail(
                format!(
                    "sjis_ext.bin 长度 {} 不是 2 的倍数(UTF-16LE)",
                    bytes.len()
                ),
            ));
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let mut chars = Vec::with_capacity(units.len());
        let mut i = 0usize;
        while i < units.len() {
            let unit = units[i];
            let ch = if (0xD800..=0xDBFF).contains(&unit) {
                let Some(low) = units.get(i + 1).copied() else {
                    return Err(Diag::fatal(code::CODEC_INVALID, Boundary::Format)
                        .with_detail(format!("sjis_ext.bin 在索引 {i} 处有孤立高代理")));
                };
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(Diag::fatal(code::CODEC_INVALID, Boundary::Format)
                        .with_detail(format!(
                            "sjis_ext.bin 在索引 {i} 处高代理后不是低代理:0x{low:04X}"
                        )));
                }
                let scalar = 0x1_0000
                    + ((u32::from(unit) - 0xD800) << 10)
                    + (u32::from(low) - 0xDC00);
                i += 2;
                char::from_u32(scalar).ok_or_else(|| {
                    Diag::fatal(code::CODEC_INVALID, Boundary::Format)
                        .with_detail(format!("sjis_ext.bin 代理对生成非法码位 U+{scalar:04X}"))
                })?
            } else if (0xDC00..=0xDFFF).contains(&unit) {
                return Err(Diag::fatal(code::CODEC_INVALID, Boundary::Format)
                    .with_detail(format!("sjis_ext.bin 在索引 {i} 处有孤立低代理")));
            } else {
                i += 1;
                char::from_u32(u32::from(unit)).ok_or_else(|| {
                    Diag::fatal(code::CODEC_INVALID, Boundary::Format)
                        .with_detail(format!("sjis_ext.bin 索引 {i} 处非法码位"))
                })?
            };
            chars.push(ch);
        }
        let mut map = BTreeMap::new();
        for (index, &ch) in chars.iter().enumerate() {
            map.insert(idx_to_code_point(index), ch);
        }
        Ok(Self {
            chars,
            map,
            digest: fnv1a64(bytes),
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    #[must_use]
    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 2 字节隧道码点 → 字符。
    #[must_use]
    pub fn char_for(&self, pair: [u8; 2]) -> Option<char> {
        self.map.get(&pair).copied()
    }
}

/// 文本边界:codec + 可选隧道表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextCodec<'a> {
    codec: Codec,
    tunnel: Option<&'a TunnelTable>,
}

impl<'a> TextCodec<'a> {
    #[must_use]
    pub const fn new(codec: Codec) -> Self {
        Self {
            codec,
            tunnel: None,
        }
    }

    #[must_use]
    pub const fn with_tunnel(codec: Codec, tunnel: &'a TunnelTable) -> Self {
        Self {
            codec,
            tunnel: Some(tunnel),
        }
    }

    #[must_use]
    pub const fn codec(self) -> Codec {
        self.codec
    }

    #[must_use]
    pub const fn tunnel(self) -> Option<&'a TunnelTable> {
        self.tunnel
    }

    /// 字符宽度(隧道对优先按 2 字节)。
    #[must_use]
    pub fn char_len_at(self, bytes: &[u8], i: usize) -> usize {
        if let (Some(tunnel), Some(&next)) = (self.tunnel, bytes.get(i + 1)) {
            if tunnel.char_for([bytes[i], next]).is_some() {
                return 2;
            }
        }
        self.codec.char_len_at(bytes, i)
    }

    /// 解一个字符(无分配)。
    #[must_use]
    pub fn decode_char(self, bytes: &[u8]) -> Option<char> {
        match bytes {
            [] => None,
            [byte, next] => {
                if let Some(tunnel) = self.tunnel {
                    if let Some(ch) = tunnel.char_for([*byte, *next]) {
                        return Some(ch);
                    }
                }
                let (ch, width) = self.codec.next_char(bytes, 0)?;
                (width == 2).then_some(ch)
            }
            [_] => {
                // 单字节单元:ASCII 与 CP932 半角片假名(0xA1..0xDF)都是
                // codec 的合法单字节字符,不能只放行 ASCII。
                let (ch, width) = self.codec.next_char(bytes, 0)?;
                (width == 1).then_some(ch)
            }
            _ => None,
        }
    }

    /// 宽容解码:隧道对优先,再按 codec;非法序列 U+FFFD 计数。
    #[must_use]
    pub fn decode_lossy(self, bytes: &[u8]) -> (Vec<char>, usize) {
        let mut chars = Vec::with_capacity(bytes.len());
        let mut invalid = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if let (Some(tunnel), Some(&next)) = (self.tunnel, bytes.get(i + 1)) {
                if let Some(ch) = tunnel.char_for([bytes[i], next]) {
                    chars.push(ch);
                    i += 2;
                    continue;
                }
            }
            if let Some((ch, width)) = self.codec.next_char(bytes, i) {
                chars.push(ch);
                i += width;
            } else {
                chars.push(char::REPLACEMENT_CHARACTER);
                invalid += 1;
                i += self.codec.char_len_at(bytes, i).min(bytes.len() - i);
            }
        }
        (chars, invalid)
    }
}

/// 隧道码点分配算法(逐字复刻 `YU-RIS-TOOLS/internal/ybn/tunnel.go`)。
#[must_use]
pub fn idx_to_code_point(idx: usize) -> [u8; 2] {
    const AVOID: [u8; 5] = [0x09, 0x0A, 0x0D, 0x20, 0x2C];
    const PER_BLOCK: usize = 0x40 - AVOID.len() - 1;
    let hi = idx / PER_BLOCK;
    let lo = idx % PER_BLOCK;
    let lead = if hi < 0x1F {
        (0x81 + hi) as u8
    } else {
        (0xE0 + (hi - 0x1F)) as u8
    };
    let mut trail = (1 + lo) as u8;
    for &avoid in &AVOID {
        if trail >= avoid {
            trail += 1;
        }
    }
    [lead, trail]
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(chars: &[char]) -> Vec<u8> {
        let mut out = Vec::new();
        for &ch in chars {
            let mut buf = [0u16; 2];
            for unit in ch.encode_utf16(&mut buf) {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn gbk_and_cp932_step_differ_on_halfwidth_range() {
        // GBK `A1 41` 是双字节;CP932 把 A1 当单字节半角片假名。
        let bytes = [0xA1, 0x41];
        assert_eq!(Codec::Gbk.char_len_at(&bytes, 0), 2);
        assert_eq!(Codec::Cp932.char_len_at(&bytes, 0), 1);
    }

    #[test]
    fn tunnel_pair_wins_over_cp932() {
        let tunnel = TunnelTable::from_bytes(&utf16le(&['中', '文'])).unwrap();
        let pair = idx_to_code_point(0);
        let text = TextCodec::with_tunnel(Codec::Cp932, &tunnel);
        assert_eq!(text.decode_char(&pair), Some('中'));
        let (chars, invalid) = text.decode_lossy(&pair);
        assert_eq!(chars, vec!['中']);
        assert_eq!(invalid, 0);
        // 不查表时同一对不是合法 CP932。
        let (_, invalid) = TextCodec::new(Codec::Cp932).decode_lossy(&pair);
        assert!(invalid > 0);
    }

    #[test]
    fn tunnel_parses_surrogate_pairs() {
        let table = TunnelTable::from_bytes(&utf16le(&['中', '𠀀'])).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(table.chars(), &['中', '𠀀']);
    }

    #[test]
    fn codec_config_domains() {
        let cfg = CodecConfig {
            script: Codec::Gbk,
            text: Codec::Cp932,
            name: Codec::Cp932,
        };
        assert_eq!(cfg.for_domain(StringDomain::Script), Codec::Gbk);
        assert_eq!(cfg.for_domain(StringDomain::Text), Codec::Cp932);
    }
}
