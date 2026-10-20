//! YSTB 分段异或密钥与 CRC-32。
//!
//! 参考 RxYuris/lib/YurisStaticLibrary/YSTB.cpp `YSTB_Coder::Xor`(分段语义,
//! 已与本仓库实证一致)+ 旁线 RE 取证(`<side-repo>/docs/format-ybn.md` §2:
//! 引擎启动时以表驱动 CRC-32(反射多项式 0xEDB88320)计算 `CRC32("Yu-ris")`
//! = `0xD36FAC96`,**按大端**写入内存 `DAT_00801354..57`,得到字节序列
//! D3 6F AC 96;`FUN_004518e9` 以该 4 字节对四区段分别从下标 0 异或)。
//!
//! 因此参与异或的密钥字节 = `[D3, 6F, AC, 96]`,即 u32 `0x96AC6FD3` 的
//! 小端字节序(CRC 值的字节倒序)。两种描述同指一字节序列,以字节为准。

/// 默认异或密钥(u32 形态 = 引擎内存中的小端读数;小端字节即密钥序列)。
pub const DEFAULT_KEY_U32: u32 = 0x96AC_6FD3;

/// 密钥字节序列(`CRC32("Yu-ris")` 的大端字节;异或循环实际使用)。
pub const DEFAULT_KEY_BYTES: [u8; 4] = DEFAULT_KEY_U32.to_le_bytes();

/// 标准 CRC-32(反射,多项式 0xEDB88320,初值/终值取反)。
///
/// 与引擎的表驱动实现语义一致(旁线 `format-ybn.md` §2):
/// `crc32(b"Yu-ris") == 0xD36F_AC96`。
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

/// 用 4 字节密钥对分段做原地异或,每个分段从 `key[0]` 重新开始。
///
/// 参考 RxYuris `YSTB_Coder::Xor`:四个区段(inst index / args index /
/// args data / line numbers)各自独立从头异或。异或是对合运算,
/// 同一函数既解密也加密(往返测试依赖这一点)。
pub fn xor_segment(seg: &mut [u8], key: &[u8; 4]) {
    for (i, b) in seg.iter_mut().enumerate() {
        *b ^= key[i & 3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_key_is_crc32_of_yu_ris() {
        assert_eq!(crc32(b"Yu-ris"), 0xD36F_AC96);
        // 引擎把 CRC 值按大端落进内存;按小端读回 = 0x96AC6FD3
        assert_eq!(DEFAULT_KEY_U32.to_le_bytes(), [0xD3, 0x6F, 0xAC, 0x96]);
        assert_eq!(DEFAULT_KEY_BYTES, [0xD3, 0x6F, 0xAC, 0x96]);
        // 大端字节序 = CRC 值本身
        assert_eq!(DEFAULT_KEY_U32.swap_bytes(), crc32(b"Yu-ris"));
    }

    #[test]
    fn xor_is_involutive_and_restarts_per_segment() {
        let key = DEFAULT_KEY_BYTES;
        let mut a = [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05];
        let orig = a;
        xor_segment(&mut a, &key);
        assert_ne!(a, orig);
        xor_segment(&mut a, &key);
        assert_eq!(a, orig);
        // 密钥按位循环:第 5 字节(下标 4)回到 key[0]
        let mut one = [0xFF];
        xor_segment(&mut one, &key);
        assert_eq!(one[0], 0xFF ^ key[0]);
    }
}
