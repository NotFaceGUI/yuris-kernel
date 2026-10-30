//! `yssfs.dat` —— 「セーブファイル設定.exe」写出的存档路径设置(明文定长记录)。
//!
//! # 布局(0.495 引擎实测)
//!
//! ```text
//! +0x00 .. +0x7F   0x20 个 u32 小端(整文件 128 字节)
//! +0x54            `_SAVE_PATH_TYPE`(puVar2[0x15])
//! ```
//!
//! # 证据
//!
//! 0.495 `FUN_00460c80`(存档路径初始化):
//!
//! ```text
//! path = <app>\yssfs.dat;
//! h = FUN_00469df2(path, "rb");            // → FUN_00469dc1(path, mode, 0x40)
//! if (h) {
//!     rec = malloc(0x80);                   // 0x20 dword
//!     FUN_00469ebe(rec, 4, 0x20, h);        // CRT fread: 4 字节 × 0x20 项
//!     v = rec[0x15];                        // 字节偏移 0x54
//!     if (v == 0..3) DAT_00801324 = v;      // 其余值不动(初值 0)
//! }
//! // 之后按 DAT_00801324 组路径:默认 "save\";1/2/3 前置
//! // %APPDATA% 类前缀 + "\<name>_SaveData\<name>\save\",3 再插 "\My Games"
//! ```
//!
//! `FUN_00469ebe` = 加锁 + `FUN_00469eed` + 解锁,而 `FUN_00469eed` 就是标准
//! 缓冲 `fread`(`FUN_0046950e` = `_read`、`FUN_004705f9` = `_filbuf`)——
//! **没有解密**。所以本文件是明文定长记录,直接按偏移取字段即可。
//!
//! 读到的值再交给运行核的系统变量 `@_SAVE_PATH_TYPE`(int getter
//! `FUN_00427350` case 112 = `MOVSX EBP,byte ptr [0x801324]`)。

/// 整文件长度(0x20 个 u32)。
pub const LEN: usize = 0x80;

/// `_SAVE_PATH_TYPE` 的字节偏移(`puVar2[0x15]`)。
pub const SAVE_PATH_TYPE_OFFSET: usize = 0x54;

/// 从 `yssfs.dat` 字节解出存档路径类型。
///
/// 官方只接受 0..=3(逐个 `if (v == n) global = n;`),其余值保持全局初值 0;
/// 文件缺失/过短同样得 0 —— 三种情况在官方都等价于 0,这里不区分。
#[must_use]
pub fn save_path_type(buf: &[u8]) -> u32 {
    let Some(cell) = buf.get(SAVE_PATH_TYPE_OFFSET..SAVE_PATH_TYPE_OFFSET + 4) else {
        return 0;
    };
    match u32::from_le_bytes([cell[0], cell[1], cell[2], cell[3]]) {
        v @ 0..=3 => v,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with(value: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; LEN];
        bytes[SAVE_PATH_TYPE_OFFSET..SAVE_PATH_TYPE_OFFSET + 4]
            .copy_from_slice(&value.to_le_bytes());
        bytes
    }

    #[test]
    fn reads_field_at_0x54_and_accepts_only_0_to_3() {
        for v in 0..=3u32 {
            assert_eq!(save_path_type(&file_with(v)), v);
        }
        // 其余值官方不写全局 ⇒ 0。
        assert_eq!(save_path_type(&file_with(4)), 0);
        assert_eq!(save_path_type(&file_with(0xFFFF_FFFF)), 0);
    }

    #[test]
    fn missing_or_short_file_is_zero() {
        assert_eq!(save_path_type(&[]), 0);
        assert_eq!(save_path_type(&[0u8; SAVE_PATH_TYPE_OFFSET]), 0);
        // 实机样本(全零 128 字节)⇒ 0。
        assert_eq!(save_path_type(&[0u8; LEN]), 0);
    }
}
