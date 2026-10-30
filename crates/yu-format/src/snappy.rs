//! Google Snappy 原始块解码(存档块压缩层)。
//!
//! 证据:商业包(引擎 0.481)的 `save/*.sd` 块载荷**不是** zlib,
//! 而 `YSSnp.dll` 导出 `YSSnp_Compress` / `YSSnp_Uncompress` /
//! `YSSnp_CompressedLength` / `YSSnp_UncompressedLength`,并内嵌
//! `snappy::ByteArraySource` / `snappy::Sink` 等符号(PDF 路径
//! `YSSnappy_DLL\YSSnappy\Release\YSSnappy.pdb`)⇒ 压缩层 = Google Snappy。
//!
//! 块首 = `varint32 未压长`,与块头的 `ulen` 逐条相等(实测 25023→524308、
//! 19→20 等)。0.495 SDK 语料仍是 zlib level 1(`78 01`),两代并存,
//! 由 [`crate::yssd::YssdEntry::decompress`] 按实际流分流。
//!
//! 格式(公开规格,无外部依赖):
//!
//! ```text
//! varint32 未压长
//! 标签流,每个标签低 2 位 = 类型:
//!   0 字面量: len = (tag>>2)+1;若 tag>>2 >= 60,随后 (tag>>2)-59 字节
//!             小端给出 len-1
//!   1 复制:   len = ((tag>>2)&7)+4; offset = ((tag>>5)<<8) | next_u8
//!   2 复制:   len = (tag>>2)+1;     offset = u16 LE
//!   3 复制:   len = (tag>>2)+1;     offset = u32 LE
//! ```
//!
//! 复制允许与输出重叠(逐字节前进),不允许 offset = 0 或越过已产出区。

/// 解一个 Snappy 原始块。`expected_len` = 块头 `ulen`(必须与 varint 一致)。
pub fn decompress(src: &[u8], expected_len: u32) -> Result<Vec<u8>, String> {
    let mut at = 0usize;
    let declared = read_varint(src, &mut at)?;
    if declared != expected_len {
        return Err(format!(
            "Snappy 未压长不符:varint 称 {declared},块头 ulen = {expected_len}"
        ));
    }
    let mut out: Vec<u8> = Vec::with_capacity(declared as usize);
    while at < src.len() {
        let tag = src[at];
        at += 1;
        match tag & 0x03 {
            0 => {
                // 长度字段 `n = tag>>2`:n < 60 ⇒ len = n+1;
                // n ∈ 60..=63 ⇒ 随后 n-59 字节小端给出 len-1。
                let field = usize::from(tag >> 2);
                let mut len = field + 1;
                if field >= 60 {
                    let extra = field - 59;
                    if at + extra > src.len() {
                        return Err("Snappy 字面量长度字段截断".to_string());
                    }
                    let mut v: usize = 0;
                    for i in 0..extra {
                        v |= usize::from(src[at + i]) << (8 * i);
                    }
                    at += extra;
                    len = v + 1;
                }
                if at + len > src.len() {
                    return Err("Snappy 字面量截断".to_string());
                }
                out.extend_from_slice(&src[at..at + len]);
                at += len;
            }
            kind => {
                let (len, offset) = match kind {
                    1 => {
                        if at >= src.len() {
                            return Err("Snappy 1 字节复制偏移截断".to_string());
                        }
                        let len = usize::from((tag >> 2) & 0x07) + 4;
                        let offset =
                            (usize::from(tag >> 5) << 8) | usize::from(src[at]);
                        at += 1;
                        (len, offset)
                    }
                    2 => {
                        if at + 2 > src.len() {
                            return Err("Snappy 2 字节复制偏移截断".to_string());
                        }
                        let len = usize::from(tag >> 2) + 1;
                        let offset =
                            usize::from(u16::from_le_bytes([src[at], src[at + 1]]));
                        at += 2;
                        (len, offset)
                    }
                    _ => {
                        if at + 4 > src.len() {
                            return Err("Snappy 4 字节复制偏移截断".to_string());
                        }
                        let len = usize::from(tag >> 2) + 1;
                        let offset = usize::try_from(u32::from_le_bytes([
                            src[at],
                            src[at + 1],
                            src[at + 2],
                            src[at + 3],
                        ]))
                        .map_err(|_| "Snappy 复制偏移超 32 位".to_string())?;
                        at += 4;
                        (len, offset)
                    }
                };
                if offset == 0 || offset > out.len() {
                    return Err(format!(
                        "Snappy 复制偏移越界:offset={offset},已产出 {} 字节",
                        out.len()
                    ));
                }
                let start = out.len() - offset;
                for i in 0..len {
                    let b = out[start + i];
                    out.push(b);
                }
            }
        }
    }
    if out.len() != declared as usize {
        return Err(format!(
            "Snappy 产出长度不符:声明 {declared},实际 {}",
            out.len()
        ));
    }
    Ok(out)
}

fn read_varint(src: &[u8], at: &mut usize) -> Result<u32, String> {
    let mut value: u32 = 0;
    for shift in 0..5 {
        let Some(&b) = src.get(*at) else {
            return Err("Snappy varint 截断".to_string());
        };
        *at += 1;
        // 第 5 字节只剩 4 个有效位;高位非零是超出 u32 的值,显式报错
        // 而不是靠 `<<` 静默丢位。
        if shift == 4 && b > 0x0F {
            return Err("Snappy varint 超出 32 位".to_string());
        }
        value |= u32::from(b & 0x7F) << (7 * shift);
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("Snappy varint 超过 5 字节".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 纯字面量编码(最小合法流;测试与"只读"场景够用)。
    fn literal_block(raw: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut n = raw.len() as u32;
        loop {
            let byte = (n & 0x7F) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        let mut rest = raw;
        while !rest.is_empty() {
            let take = rest.len().min(60);
            out.push(((take - 1) as u8) << 2);
            out.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
        out
    }

    #[test]
    fn literal_only_roundtrip() {
        let raw: Vec<u8> = (0..200u32).map(|i| (i * 7) as u8).collect();
        let block = literal_block(&raw);
        assert_eq!(decompress(&block, raw.len() as u32).unwrap(), raw);
    }

    #[test]
    fn copy_op_with_overlap() {
        // "ab" 字面量 + 复制 offset=2 len=6 ⇒ "abababab"。
        let mut block = vec![8]; // varint 8
        block.push(1 << 2); // 字面量 len=2
        block.extend_from_slice(b"ab");
        // 类型 2: len = (tag>>2)+1 = 6 ⇒ tag>>2 = 5;offset = 2。
        block.push((5 << 2) | 0x02);
        block.extend_from_slice(&2u16.to_le_bytes());
        assert_eq!(decompress(&block, 8).unwrap(), b"abababab");
    }

    #[test]
    fn rejects_wrong_declared_length_and_bad_offset() {
        let mut block = literal_block(b"hello");
        block[0] = 6; // 声明 6,实际 5
        assert!(decompress(&block, 6).is_err());
        // 复制 offset 越过已产出区。
        let bad = vec![4u8, (1 << 2) | 0x02, 9, 0];
        assert!(decompress(&bad, 4).is_err());
    }
}
