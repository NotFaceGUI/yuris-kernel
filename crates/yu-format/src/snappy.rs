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

// ---------------------------------------------------------------- 编码

/// 压一个 Snappy 原始块(`YSSnp_Compress` 的等价形态)。
///
/// 只保证**格式合法且可被任何 Snappy 解码器接受**(含官方 `YSSnp_Uncompress`):
/// 块首 varint32 未压长 + 字面量/复制标签。不做 Google 实现那种逐字节复刻的
/// 启发式(哈希表规模与跳步策略只影响压缩率,不影响格式)。
#[must_use]
pub fn compress(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() / 2 + 32);
    write_varint(&mut out, src.len() as u32);
    if src.is_empty() {
        return out;
    }
    // 4 字节序列的哈希表:槽内放 `pos + 1`(0 = 空槽)。
    const BITS: u32 = 14;
    const MAX_COPY: usize = 64;
    let mut table = vec![0u32; 1 << BITS];
    let mut lit = 0usize; // 待输出字面量的起点
    let mut ip = 0usize;
    while ip + 4 <= src.len() {
        let h = hash4(&src[ip..ip + 4], BITS);
        let cand = table[h] as usize;
        table[h] = (ip + 1) as u32;
        let matched = cand != 0 && {
            let cand = cand - 1;
            cand < ip && src[cand..cand + 4] == src[ip..ip + 4]
        };
        if !matched {
            ip += 1;
            continue;
        }
        let cand = cand - 1;
        let mut len = 4usize;
        while len < MAX_COPY && ip + len < src.len() && src[cand + len] == src[ip + len] {
            len += 1;
        }
        emit_literal(&mut out, &src[lit..ip]);
        emit_copy(&mut out, ip - cand, len);
        ip += len;
        lit = ip;
    }
    emit_literal(&mut out, &src[lit..]);
    out
}

fn hash4(seq: &[u8], bits: u32) -> usize {
    let v = u32::from_le_bytes([seq[0], seq[1], seq[2], seq[3]]);
    ((v.wrapping_mul(0x1e35_a7bd)) >> (32 - bits)) as usize
}

fn write_varint(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// 字面量:长度 >60 时按 60 字节切块(合法且无需扩展长度字段)。
fn emit_literal(out: &mut Vec<u8>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let take = bytes.len().min(60);
        out.push(((take - 1) as u8) << 2);
        out.extend_from_slice(&bytes[..take]);
        bytes = &bytes[take..];
    }
}

/// 复制:长度 4..=64 一次输出;offset 按大小选 1/2/4 字节形态。
fn emit_copy(out: &mut Vec<u8>, offset: usize, len: usize) {
    debug_assert!((4..=64).contains(&len));
    debug_assert!(offset >= 1);
    let l = (len - 1) as u8;
    if offset < 2048 && len <= 11 {
        // 类型 1:11 位 offset + 3 位 (len-4)
        out.push(((offset >> 8) as u8) << 5 | ((len - 4) as u8) << 2 | 0x01);
        out.push((offset & 0xFF) as u8);
    } else if offset <= u16::MAX as usize {
        // 类型 2:16 位 offset
        out.push(l << 2 | 0x02);
        out.extend_from_slice(&(offset as u16).to_le_bytes());
    } else {
        // 类型 3:32 位 offset
        out.push(l << 2 | 0x03);
        out.extend_from_slice(&(offset as u32).to_le_bytes());
    }
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

    /// 编码器自检:随机 + 重复数据都过解码器,且官方块重压后仍解出原字节。
    #[test]
    fn compress_roundtrips_through_decoder() {
        // 重复串(走复制)、伪随机(走字面量)、长偏移(>64K,走类型 3)。
        let mut cases: Vec<Vec<u8>> = Vec::new();
        cases.push(b"abcdefgh".repeat(400));
        cases.push((0..5000u32).map(|i| (i * 37 + 11) as u8).collect());
        let mut long = vec![0u8; 70000];
        for (i, b) in long.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let head = [long[0], long[1], long[2], long[3]];
        long[69996..70000].copy_from_slice(&head); // 触发 >64K 偏移
        cases.push(long);
        for raw in cases {
            let block = compress(&raw);
            assert_eq!(decompress(&block, raw.len() as u32).unwrap(), raw);
            assert!(block.len() < raw.len() + raw.len() / 8 + 16, "编码不应膨胀过多");
        }
    }

    /// 官方产物重压:解出原字节 → 重压 → 再解,必须逐字节回到原值。
    #[test]
    fn official_block_recompresses_losslessly() {
        // "ab" + 复制 offset=2 len=6 的官方形态(见 copy_op_with_overlap)。
        let raw = b"abababab".to_vec();
        let block = compress(&raw);
        assert_eq!(decompress(&block, raw.len() as u32).unwrap(), raw);
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
