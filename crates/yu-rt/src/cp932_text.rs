//! CP932 字符边界工具 —— **所有按字符水平处理脚本字符串的操作必须走这里**。
//!
//! 背景(2026-10-24,实测):
//! 核内的脚本字符串一律是 **CP932 原始字节**(`Value::Str(Vec<u8>)`),不是
//! UTF-8,也不是 `String`。凡是对它逐字节做 ASCII 大小写/取子串的实现,
//! 都会把双字节字符的**尾字节**当成半角字母改写或切断:
//!
//! 本模块把「按 CP932 字符推进」收敛成唯一实现:lead byte 区间
//! `0x81..=0x9F`(JIS X 0208 区)与 `0xE0..=0xFC`(外字/机种依存区),
//! 与 `yu-format` 的码表构造口径一致。

/// 是否为 CP932 双字节字符的 lead byte。
///
/// 区间按 CP932(Windows-31J)定义:
/// - `0x81..=0x9F`:JIS X 0208 汉字/假名/符号;
/// - `0xE0..=0xFC`:外字与机种依存区。
///
/// `0x80`/`0xA0`/`0xFD..=0xFF` 不是合法 lead(CP932 中未定义),按单字节处理。
#[inline]
pub fn is_lead(byte: u8) -> bool {
    (0x81..=0x9F).contains(&byte) || (0xE0..=0xFC).contains(&byte)
}

/// 从 `i` 开始的字符长度(字节数):lead + 有后续字节 ⇒ 2,否则 1。
///
/// 截断的尾 lead(位于字符串末尾、后面没有尾字节)按 1 处理,保证遍历一定
/// 前进(不越界、不死循环)。这与官方引擎逐字节读缓冲、遇到流尾即停的行为
/// 同型。
#[inline]
pub fn char_len_at(bytes: &[u8], i: usize) -> usize {
    if i < bytes.len() && is_lead(bytes[i]) && i + 1 < bytes.len() {
        2
    } else {
        1
    }
}

/// 依次产出每个字符的 `(start, end)` 字节区间。
pub fn spans(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let step = char_len_at(bytes, i);
        out.push((i, i + step));
        i += step;
    }
    out
}

/// 字符个数(CP932 计)。
pub fn char_count(bytes: &[u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < bytes.len() {
        i += char_len_at(bytes, i);
        n += 1;
    }
    n
}

/// 按**字符**位置取子串(0 起算,`end` 不含)。
///
/// 超界部分自动收敛;`start >= end` 或 `start` 越界 ⇒ 空。
pub fn slice_chars(bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    if start >= end {
        return Vec::new();
    }
    let spans = spans(bytes);
    let start = start.min(spans.len());
    let end = end.min(spans.len());
    if start >= end {
        return Vec::new();
    }
    let begin = spans[start].0;
    let stop = spans[end - 1].1;
    bytes[begin..stop].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 片头 LOGO 真名:必须按 4 字符处理,不能被当成 8 个半角字节。
    ///
    /// 字节值来自**实测 CP932 编码**,不是源码里的 UTF-8(`"サークル"` 在
    /// Rust 源码里 `as_bytes()` 得到的是 UTF-8,与引擎侧字节不同)。
    #[test]
    fn cp932_lead_bytes_are_two_byte_chars() {
        let s = vec![0x83, 0x54, 0x81, 0x5b, 0x83, 0x4e, 0x83, 0x8b];
        assert_eq!(char_count(&s), 4, "「サークル」是 4 个 CP932 字符");
        // 取前 4 字符 = 整个字符串(不是前 4 字节)。
        assert_eq!(slice_chars(&s, 0, 4), s);
        // 取前 1 字符 = サ(完整两字节)。
        assert_eq!(slice_chars(&s, 0, 1), vec![0x83, 0x54]);
        assert_eq!(slice_chars(&s, 1, 2), vec![0x81, 0x5b]);
    }

    /// 混合 ASCII 与双字节:边界必须落在字符之间。
    #[test]
    fn mixed_ascii_and_double_byte() {
        // 'a' + サ(0x83 0x54) + 'b'
        let s = vec![b'a', 0x83, 0x54, b'b'];
        assert_eq!(char_count(&s), 3);
        assert_eq!(slice_chars(&s, 0, 2), vec![b'a', 0x83, 0x54]);
        assert_eq!(slice_chars(&s, 1, 3), vec![0x83, 0x54, b'b']);
    }

    /// 末尾截断的 lead byte 不得导致越界或死循环。
    #[test]
    fn truncated_trailing_lead_is_single() {
        let s = vec![b'A', 0x83];
        assert_eq!(char_count(&s), 2);
        assert_eq!(spans(&s), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn out_of_range_slices_are_empty() {
        let s = vec![0x83, 0x54];
        assert!(slice_chars(&s, 5, 9).is_empty());
        assert!(slice_chars(&s, 2, 1).is_empty());
    }
}
