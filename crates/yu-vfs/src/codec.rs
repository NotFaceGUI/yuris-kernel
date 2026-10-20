//! 字节级行扫描参数。
//!
//! 路径归一和名字比较是**字节级**操作,但多字节字符的步进表随编解码器不同:
//! CP932 的 0xA1–0xDF 是半角片名单字节,GBK 同区间却是前导字节;更关键的是
//! CP932 的 trail 字节可以取 0x5C(`\`),若按裸字节折分隔符会把一个日文字符
//! 劈成两段。这里只提供步进/尾字节判据;完整码表归 `yu-format`(任务书 A §3)。

/// 字节扫描使用的编解码器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Cp932,
    Gbk,
}

impl Codec {
    pub const fn as_str(self) -> &'static str {
        match self {
            Codec::Cp932 => "cp932",
            Codec::Gbk => "gbk",
        }
    }

    /// 该字节是否是多字节字符的前导字节。
    pub const fn is_lead(self, b: u8) -> bool {
        match self {
            Codec::Cp932 => (b >= 0x81 && b <= 0x9f) || (b >= 0xe0 && b <= 0xfc),
            Codec::Gbk => b >= 0x81 && b <= 0xfe,
        }
    }

    /// 该字节是否可作为多字节字符的尾字节(用于诊断与单测)。
    pub const fn is_trail(self, b: u8) -> bool {
        match self {
            Codec::Cp932 => (b >= 0x40 && b <= 0x7e) || (b >= 0x80 && b <= 0xfc),
            Codec::Gbk => b >= 0x40 && b <= 0xfe && b != 0x7f,
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
    }
}
