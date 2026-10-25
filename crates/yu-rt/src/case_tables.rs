//! `HANTOZEN` / `ZENTOHAN` 的官方转换表(DIV-RT-013 收口)。
//!
//! # 证据(0.495 官方 `yu-ris.exe`,sha256 `4f5eefa3…`)
//!
//! 表字节直接取自官方产物,`data/case-tables-0495.tsv` 是可审计的转写
//! (生成脚本逻辑与地址写在文件头注释里):
//!
//! | 表 | 地址 | 项数 | 语义 |
//! |---|---:|---:|---|
//! | A | `0x5b5170` | 95 | `ASCII 0x20..0x7e → 全角 2 字节` |
//! | K | `0x808840` | 63 | `半角片假名 0xa1..0xdf → 全角 2 字节` |
//! | S | `0x5b5230` | 126 | `全角 → 半角`:`src u8, span u8, dst u16` |
//!
//! 算法逐条对照处理体:
//! - `HANTOZEN`(VARACT 分支,`0x4546aa..`):
//!   `0x5b40c0[byte]+1` 得字符长度;2/3/4 字节原样拷贝;单字节时
//!   `0xa1..=0xdf` 查 K(`0x4546ee`),否则查 `0x46b327`(ASCII→A;
//!   `0xdc→0x838f`;`0xa1..=0xdf→S`),结果低字节等于原字节则原样输出,
//!   否则输出两字节;
//! - `ZENTOHAN`(VARACT 分支,`0x4548cf..`):单字节原样;双字节先把
//!   `(b0<<8)|b1` 交给 `0x46b395`(先精确查 A 表得 `index+0x20`,再按
//!   `code <= 0x8396` 查 S 的区间 `dst..dst+span-1`),结果等于原码则原样
//!   输出两字节,否则只输出结果的低字节。
//!
//! 表是**数据**,不是猜的规则:任何一项都能回指官方 exe 的字节。

use std::sync::OnceLock;

/// 全角 → 半角:`(src, dst, span)`;`dst..dst+span-1` 覆盖同一半角码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WideToNarrow {
    pub src: u8,
    pub dst: u16,
    pub span: u8,
}

/// 三张表的只读视图(首次访问时从内嵌数据解析)。
#[derive(Debug)]
pub struct CaseTables {
    pub ascii_full: Vec<u16>,
    pub kana_half: Vec<u16>,
    pub wide_to_narrow: Vec<WideToNarrow>,
}

static TABLES: OnceLock<CaseTables> = OnceLock::new();

impl CaseTables {
    /// 官方表(内嵌 `data/case-tables-0495.tsv`)。
    pub fn get() -> &'static CaseTables {
        TABLES.get_or_init(|| Self::parse(include_str!("../data/case-tables-0495.tsv")))
    }

    fn parse(text: &str) -> Self {
        let mut ascii_full = Vec::new();
        let mut kana_half = Vec::new();
        let mut wide_to_narrow = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split('\t');
            match it.next() {
                Some("A") => {
                    if let Some(v) = it.next().and_then(|s| u16::from_str_radix(s, 16).ok()) {
                        ascii_full.push(v);
                    }
                }
                Some("K") => {
                    if let Some(v) = it.next().and_then(|s| u16::from_str_radix(s, 16).ok()) {
                        kana_half.push(v);
                    }
                }
                Some("S") => {
                    let (Some(src), Some(dst), Some(span)) = (
                        it.next().and_then(|s| u8::from_str_radix(s, 16).ok()),
                        it.next().and_then(|s| u16::from_str_radix(s, 16).ok()),
                        it.next().and_then(|s| s.parse::<u8>().ok()),
                    ) else {
                        continue;
                    };
                    wide_to_narrow.push(WideToNarrow { src, dst, span });
                }
                _ => {}
            }
        }
        Self {
            ascii_full,
            kana_half,
            wide_to_narrow,
        }
    }
}

/// `0x5b40c0` 的字符长度(1 或 2;CP932 无 3/4 字节项)。
///
/// 单一真源 = [`crate::cp932_text::is_lead`](与 `yu-format` 码表、`VARACT`
/// 的字符步进同一口径)。
fn char_len(b: u8) -> usize {
    if crate::cp932_text::is_lead(b) {
        2
    } else {
        1
    }
}

/// `0x46b327`:单字节 → 全角码(主表 A),未命中返回原值。
fn han_char(c: u8) -> u16 {
    let t = CaseTables::get();
    if (0x20..=0x7e).contains(&c) {
        return t.ascii_full[(c - 0x20) as usize];
    }
    if c == 0xdc {
        return 0x838f;
    }
    if (0xa1..=0xdf).contains(&c) {
        for e in &t.wide_to_narrow {
            if c == e.src {
                return e.dst;
            }
        }
    }
    u16::from(c)
}

/// `0x46b395`:全角码 → 半角码。
fn zen_code(code: u16) -> u16 {
    let t = CaseTables::get();
    if let Some(i) = t.ascii_full.iter().position(|&v| v == code) {
        return 0x20 + i as u16;
    }
    if code > 0x8396 {
        return code;
    }
    for e in &t.wide_to_narrow {
        if code == e.dst {
            return u16::from(e.src);
        }
        // 同一上位字节内、落在 [dst, dst+span) 的码映射到同一半角字符。
        if (code ^ e.dst) & 0xff00 == 0 && code > e.dst && code - e.dst < u16::from(e.span) {
            return u16::from(e.src);
        }
    }
    code
}

/// `VARACT[HANTOZEN]`:半角→全角。
pub fn hantozen(bytes: &[u8]) -> Vec<u8> {
    let t = CaseTables::get();
    let mut out = Vec::with_capacity(bytes.len() * 2);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let len = char_len(c);
        if len > 1 {
            // 2/3/4 字节字符原样拷贝(CP932 实际只有 2)。
            let end = (i + len).min(bytes.len());
            out.extend_from_slice(&bytes[i..end]);
            i = end;
            continue;
        }
        if (0xa1..=0xdf).contains(&c) {
            // 引擎按**两个字节**读表并原序写出(`0x4546ee/0x454700`);
            // TSV 里存的是这两字节的小端 u16 ⇒ 先低字节后高字节。
            let v = t.kana_half[(c - 0xa1) as usize];
            out.push((v & 0xff) as u8);
            out.push((v >> 8) as u8);
        } else {
            let e = han_char(c);
            if (e & 0xff) as u8 == c {
                out.push(c);
            } else {
                out.push((e >> 8) as u8);
                out.push((e & 0xff) as u8);
            }
        }
        i += 1;
    }
    out
}

/// `VARACT[ZENTOHAN]`:全角→半角。
pub fn zentohan(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if char_len(c) == 2 && i + 1 < bytes.len() {
            let code = (u16::from(c) << 8) | u16::from(bytes[i + 1]);
            let mapped = zen_code(code);
            if mapped == code {
                out.push(c);
                out.push(bytes[i + 1]);
            } else {
                out.push((mapped & 0xff) as u8);
            }
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_official_sizes() {
        let t = CaseTables::get();
        assert_eq!(t.ascii_full.len(), 95);
        assert_eq!(t.kana_half.len(), 63);
        assert_eq!(t.wide_to_narrow.len(), 126);
        assert_eq!(t.ascii_full[0], 0x8140, "ASCII 空格 → 全角空格");
    }

    #[test]
    fn hantozen_ascii_and_halfwidth_katakana() {
        // A → Ａ(0x8260),1 → １(0x8250)
        assert_eq!(hantozen(b"A"), vec![0x82, 0x60]);
        assert_eq!(hantozen(b"1"), vec![0x82, 0x50]);
        // ｱ(0xb1) → ア(0x8341)
        assert_eq!(hantozen(&[0xb1]), vec![0x83, 0x41]);
        // 已有双字节字符原样
        assert_eq!(hantozen(&[0x82, 0xa0]), vec![0x82, 0xa0]);
    }

    #[test]
    fn zentohan_reverses_both_tables() {
        assert_eq!(zentohan(&[0x82, 0x60]), b"A".to_vec());
        assert_eq!(zentohan(&[0x83, 0x41]), vec![0xb1]);
        // 未映射的双字节原样
        assert_eq!(zentohan(&[0x88, 0x9f]), vec![0x88, 0x9f]);
        // ASCII 原样
        assert_eq!(zentohan(b"abc"), b"abc".to_vec());
    }
}
