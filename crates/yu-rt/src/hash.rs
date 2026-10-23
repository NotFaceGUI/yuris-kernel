//! CRC-32(IEEE 反射,多项式 `0xEDB88320`)。
//!
//! 引擎 `MATH[HASH]` 的取值来源(旁线 `format-vm-opcodes.md` §3.20):
//! 对 `SETSTR` 字符串逐字节查表,表在 0.495 exe `0x5b41c0`,**与标准
//! `0xEDB88320` 表 256 项逐项相同**(初值 `0xFFFFFFFF`、末尾取反),
//! 检查值 `"123456789" → 0xCBF43926`。此处按标准多项式即时计算,不落表。

/// 标准 CRC-32(zlib/PNG/PkZip 同款)。
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    #[test]
    fn standard_check_value() {
        // 引擎表与标准表逐项相同 ⇒ 标准检查值必须成立。
        assert_eq!(super::crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(super::crc32(b""), 0);
    }
}
