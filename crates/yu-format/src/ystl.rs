//! `yst_list.ybn`(魔数 `YSTL`)· 脚本清单:记录下标 → 源脚本名 + 统计。
//!
//! 两个记录尾布局,由**结构闭合**判定:
//!
//! - `Old16`:`filetime(8) + var_count(4) + label_count(4)`(16 字节)。
//!   参考 RxYuris `YSTL_Entry_V2`;v474 商业包 232/232 条精确闭合。
//! - `New20`:同上再追加 `text_count(4)`(20 字节)。参考 RxYuris
//!   `YSTL_Entry_V5`;0.495 ERIS 141/141 条精确闭合。
//!
//! 工具对版本阈值互相矛盾(yuri `VScrNew=474`、RxYuris V2<=466/V5>=300、
//! Script-Editor >=300),而真包 v474 实际是 `Old16` —— 所以版本只记录,
//! 布局由"条数走完 + 恰好闭合"裁决。
//!
//! ```text
//! +0x00 char[4] "YSTL"
//! +0x04 u32     版本(诊断字段;不参与布局选择)
//! +0x08 u32     条数
//! +0x0C 记录[条数]:
//!        u32  index      脚本槽位号 = yst%05d 的编号
//!        u32  name_len
//!        u8[] name       源脚本相对路径(CP932 字节)
//!        u32  filetime_high
//!        u32  filetime_low
//!        u32  var_count
//!        u32  label_count
//!        [u32 text_count]  仅 New20
//! ```
//!
//! `Old16` 没有 `text_count` 字段,`Entry::text_count` 固定为 0。
//! `var_count`/`label_count` 对无脚本产物的条目(纯声明文件)取哨兵
//! `u32::MAX`。

use crate::code;
use crate::container::{read_header, truncated};
use yu_log::{Boundary, Diag};

/// YSTL 记录尾布局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YstlLayout {
    /// 16 字节尾:filetime + var_count + label_count(无 text_count)。
    Old16,
    /// 20 字节尾:filetime + var_count + label_count + text_count。
    New20,
}

impl YstlLayout {
    fn tail_len(self) -> usize {
        match self {
            YstlLayout::Old16 => 0x10,
            YstlLayout::New20 => 0x14,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            YstlLayout::Old16 => "old16",
            YstlLayout::New20 => "new20",
        }
    }
}

/// 一条脚本清单记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 脚本槽位号,即 `yst%05d.ybn` 的编号。
    pub index: u32,
    /// 源脚本相对路径(原始 CP932 字节,如 `data\script\eris\es_First.yst`)。
    pub name: Vec<u8>,
    /// 源文件修改时间(FILETIME 64 位拼合)。
    pub file_time: u64,
    /// 该脚本变量声明数(无脚本产物时 = `u32::MAX` 哨兵)。
    pub var_count: u32,
    /// 该脚本标签数(与 ysl 逐条互证;无脚本产物时 = `u32::MAX`)。
    pub label_count: u32,
    /// 该脚本显示文本数(无脚本产物时 = 0)。
    pub text_count: u32,
}

impl Entry {
    /// 是否存在对应的 `yst%05d.ybn`(无哨兵值即视为有)。
    #[must_use]
    pub fn has_script(&self) -> bool {
        self.var_count != u32::MAX && self.label_count != u32::MAX
    }
}

/// 已解码的脚本清单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptList {
    /// 容器头版本(`+0x04`;诊断字段,不参与布局选择)。
    pub version: u32,
    /// 判定出的记录尾布局。
    pub layout: YstlLayout,
    /// 记录(文件顺序;`index` 在 0.495 语料中恰为升序 = 下标)。
    pub entries: Vec<Entry>,
}

impl ScriptList {
    /// 解码 `yst_list.ybn` 字节。
    ///
    /// `Old16` / `New20` 都作为候选;只有"条数走完 + 恰好闭合(允许尾部
    /// 零填充)"的布局才被接受。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSTL")?;
        let count = header.word08 as usize;

        let mut first_error: Option<Diag> = None;
        for layout in [YstlLayout::New20, YstlLayout::Old16] {
            match parse_records(buf, count, origin, layout) {
                Ok((entries, consumed)) => {
                    if buf[consumed..].iter().all(|&b| b == 0) {
                        return Ok(Self {
                            version: header.version,
                            layout,
                            entries,
                        });
                    }
                    if first_error.is_none() {
                        first_error = Some(
                            Diag::fatal(code::YSTL_FIELD, Boundary::Format)
                                .at_data(origin, consumed as u64)
                                .with_detail(format!(
                                    "{} 布局走完 {count} 条后仍有 {} 字节非零尾部",
                                    layout.as_str(),
                                    buf.len() - consumed
                                )),
                        );
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        Err(first_error.unwrap_or_else(|| {
            Diag::fatal(code::YSTL_FIELD, Boundary::Format)
                .at_data(origin, 0x0C)
                .with_detail("yst_list.ybn 无任何结构闭合的记录尾布局")
        }))
    }
}

fn parse_records(
    buf: &[u8],
    count: usize,
    origin: &[u8],
    layout: YstlLayout,
) -> Result<(Vec<Entry>, usize), Diag> {
    let tail_len = layout.tail_len();
    let mut at = 0x0Cusize;
    let mut entries = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let rec_end = at
            .checked_add(8)
            .ok_or_else(|| limit(origin, at as u64, "YSTL 记录头"))?;
        if rec_end > buf.len() {
            return Err(truncated(origin, rec_end, buf.len(), "YSTL 记录头"));
        }
        let index = u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]);
        let name_len =
            u32::from_le_bytes([buf[at + 4], buf[at + 5], buf[at + 6], buf[at + 7]]) as usize;
        let name_at = at
            .checked_add(8)
            .ok_or_else(|| limit(origin, at as u64, "YSTL 名字起点"))?;
        let tail_at = name_at
            .checked_add(name_len)
            .ok_or_else(|| limit(origin, name_at as u64, "YSTL 名字长度"))?;
        let end = tail_at
            .checked_add(tail_len)
            .ok_or_else(|| limit(origin, tail_at as u64, "YSTL 记录尾"))?;
        if end > buf.len() {
            return Err(truncated(origin, end, buf.len(), "YSTL 记录尾"));
        }
        let tail = &buf[tail_at..end];
        let word = |i: usize| u32::from_le_bytes([tail[i], tail[i + 1], tail[i + 2], tail[i + 3]]);
        entries.push(Entry {
            index,
            name: buf[name_at..tail_at].to_vec(),
            file_time: ((word(0) as u64) << 32) | word(4) as u64,
            var_count: word(8),
            label_count: word(12),
            text_count: if layout == YstlLayout::New20 {
                word(16)
            } else {
                0
            },
        });
        at = end;
    }
    Ok((entries, at))
}

/// 记录长度/偏移超出可表示范围 = `fmt.limit`(32 位目标不回绕)。
fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(count: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"YSTL");
        buf.extend_from_slice(&474u32.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf
    }

    #[test]
    fn old16_layout_parses_v474_shape() {
        let mut buf = header(2);
        for (idx, name, var, label) in [
            (
                0u32,
                &b"data\\script\\advlib\\adv.yst"[..],
                0x7bu32,
                0x63u32,
            ),
            (
                1,
                &b"data\\script\\advlib\\global.txt"[..],
                u32::MAX,
                u32::MAX,
            ),
        ] {
            buf.extend_from_slice(&idx.to_le_bytes());
            buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
            buf.extend_from_slice(name);
            buf.extend_from_slice(&0x01cd31e9u32.to_le_bytes()); // high
            buf.extend_from_slice(&0xeef37accu32.to_le_bytes()); // low
            buf.extend_from_slice(&var.to_le_bytes());
            buf.extend_from_slice(&label.to_le_bytes());
        }

        let list = ScriptList::parse(&buf, b"v474/yst_list.ybn").unwrap();
        assert_eq!(list.layout, YstlLayout::Old16);
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[0].name, b"data\\script\\advlib\\adv.yst");
        assert_eq!(list.entries[0].var_count, 0x7b);
        assert_eq!(list.entries[0].label_count, 0x63);
        assert_eq!(list.entries[0].text_count, 0, "Old16 无 text_count 字段");
        assert!(!list.entries[1].has_script());
    }

    #[test]
    fn new20_layout_still_parses() {
        let mut buf = header(1);
        let name = b"data\\script\\eris\\es_button.yst";
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
        buf.extend_from_slice(name);
        buf.extend_from_slice(&0x01dd5faeu32.to_le_bytes());
        buf.extend_from_slice(&0xdcd1564cu32.to_le_bytes());
        buf.extend_from_slice(&0x1cau32.to_le_bytes());
        buf.extend_from_slice(&0x88u32.to_le_bytes());
        buf.extend_from_slice(&7u32.to_le_bytes());

        let list = ScriptList::parse(&buf, b"v495/yst_list.ybn").unwrap();
        assert_eq!(list.layout, YstlLayout::New20);
        assert_eq!(list.entries[0].text_count, 7);
    }
}
