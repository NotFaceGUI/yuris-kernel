//! 字节级行扫描参数。
//!
//! 路径归一和名字比较是**字节级**操作,但多字节字符的步进表随编解码器不同:
//! CP932 的 0xA1–0xDF 是半角片名单字节,GBK 同区间却是前导字节;更关键的是
//! CP932 的 trail 字节可以取 0x5C(`\`),若按裸字节折分隔符会把一个日文字符
//! 劈成两段。这里只提供步进/尾字节判据;完整码表归 `yu-format`(任务书 A §3),
//! 需要 encode/decode 时经 [`Codec::to_format`] 调用。

/// 字节扫描使用的编解码器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Cp932,
    Gbk,
    /// Windows CP950 / Big5(繁中)。前导区间与 GBK 相同,尾字节区间不同
    /// (含 0x40..=0x7E,因此 0x5C `\` 同样可能是尾字节)。
    Big5,
}

impl Codec {
    pub const fn as_str(self) -> &'static str {
        match self {
            Codec::Cp932 => "cp932",
            Codec::Gbk => "gbk",
            Codec::Big5 => "big5",
        }
    }

    /// 该字节是否是多字节字符的前导字节。
    pub const fn is_lead(self, b: u8) -> bool {
        match self {
            Codec::Cp932 => (b >= 0x81 && b <= 0x9f) || (b >= 0xe0 && b <= 0xfc),
            Codec::Gbk | Codec::Big5 => b >= 0x81 && b <= 0xfe,
        }
    }

    /// 该字节是否可作为多字节字符的尾字节(用于诊断与单测)。
    pub const fn is_trail(self, b: u8) -> bool {
        match self {
            Codec::Cp932 => (b >= 0x40 && b <= 0x7e) || (b >= 0x80 && b <= 0xfc),
            Codec::Gbk => b >= 0x40 && b <= 0xfe && b != 0x7f,
            // CP950 尾字节 = 0x40..=0x7E | 0xA1..=0xFE(0x7F..=0xA0 是空洞)。
            Codec::Big5 => (b >= 0x40 && b <= 0x7e) || (b >= 0xa1 && b <= 0xfe),
        }
    }

    /// 映射到 `yu-format` 的同名编解码器(encode/decode 调用面)。
    pub(crate) fn to_format(self) -> yu_format::Codec {
        match self {
            Codec::Cp932 => yu_format::Codec::Cp932,
            Codec::Gbk => yu_format::Codec::Gbk,
            Codec::Big5 => yu_format::Codec::Big5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp932_halfwidth_kana_is_not_a_lead() {
        assert!(!Codec::Cp932.is_lead(0xa1));
        assert!(!Codec::Cp932.is_lead(0xdf));
        assert!(Codec::Cp932.is_lead(0x81));
        assert!(Codec::Cp932.is_lead(0xe0));
        assert!(!Codec::Cp932.is_lead(0xfd));
    }

    #[test]
    fn gbk_covers_0x81_through_0xfe() {
        assert!(Codec::Gbk.is_lead(0x81));
        assert!(Codec::Gbk.is_lead(0xfe));
        assert!(!Codec::Gbk.is_lead(0x80));
        assert!(!Codec::Gbk.is_lead(0xff));
    }

    #[test]
    fn cp932_backslash_can_be_a_trail_byte() {
        assert!(Codec::Cp932.is_trail(0x5c));
        assert!(Codec::Gbk.is_trail(0x5c));
        assert!(Codec::Big5.is_trail(0x5c), "Big5 尾字节含 0x40..=0x7E");
    }

    #[test]
    fn big5_shares_gbk_lead_range_but_not_trail_holes() {
        assert!(Codec::Big5.is_lead(0x81));
        assert!(Codec::Big5.is_lead(0xfe));
        assert!(!Codec::Big5.is_lead(0x80));
        // CP950 的 0x80..=0xA0 尾字节区是空洞,GBK 在 0x80 段合法。
        assert!(!Codec::Big5.is_trail(0x80));
        assert!(!Codec::Big5.is_trail(0xa0));
        assert!(Codec::Big5.is_trail(0xa1));
        assert!(Codec::Big5.is_trail(0xfe));
        assert_eq!(Codec::Big5.to_format(), yu_format::Codec::Big5);
    }
}
