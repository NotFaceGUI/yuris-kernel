//! CRC-32(IEEE 反射,多项式 `0xEDB88320`)。
//!
//! 引擎 `MATH[HASH]` 的取值来源(旁线 `format-vm-opcodes.md` §3.20):
//! 对 `SETSTR` 字符串逐字节查表,表在 0.495 exe `0x5b41c0`,**与标准
//! `0xEDB88320` 表 256 项逐项相同**(初值 `0xFFFFFFFF`、末尾取反),
//! 检查值 `"123456789" → 0xCBF43926`。

/// 标准 CRC-32 的 256 项查表(编译期展开;与按位实现逐位相同)。
///
/// 引擎本身就是查 `0x5b41c0` 的 256 项表(与标准表逐项相同),这里的表
/// 就是同一张表,只是不再运行时按位算。
static TABLE: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// 标准 CRC-32(zlib/PNG/PkZip 同款)。
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc = (crc >> 8) ^ TABLE[((crc ^ u32::from(b)) & 0xff) as usize];
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

    /// 查表实现必须与旧的按位实现在所有单字节/短串上逐位相同。
    #[test]
    fn table_matches_bitwise_reference() {
        fn bitwise(data: &[u8]) -> u32 {
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
        let samples: &[&[u8]] = &[
            b"",
            b"a",
            b"abc",
            b"123456789",
            b"\x00\x01\x02\x03\xff\xfe",
            b"The quick brown fox jumps over the lazy dog",
            &[0x82u8, 0xa0, 0x82, 0xa2, 0x82, 0xa4],
        ];
        for s in samples {
            assert_eq!(super::crc32(s), bitwise(s), "样本 {s:?}");
        }
    }
}
