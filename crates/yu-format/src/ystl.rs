//! `yst_list.ybn`(魔数 `YSTL`)· 脚本清单:记录下标 → 源脚本名 + 统计。
//!
//! 布局(ERIS 语料实测闭合;旁线 `format-ybn.md` §4 的记录骨架 +
//! RxYuris/lib/YurisStaticLibrary/YSTL.cpp `YSTL_V5` 的尾字段解释——
//! **两者综合后的字段定性以本仓库实测为准**,见 `docs/divergences/DIV-005`):
//!
//! ```text
//! +0x00 char[4] "YSTL"
//! +0x04 u32     版本
//! +0x08 u32     条数(0.495 语料 = 141)
//! +0x0C 记录[条数](头部仅 12 字节;ERIS 语料逐字节实证):
//!        +0x00 u32        index      脚本槽位号 = yst%05d 的编号
//!        +0x04 u32        name_len
//!        +0x08 byte[n]    name       源脚本相对路径(CP932 字节)
//!        +0x08+n u32      filetime_high   ┐
//!        +0x0C+n u32      filetime_low   │ 修改时间(FILETIME)
//!        +0x10+n u32      var_count     ┐ 各脚本统计;
//!        +0x14+n u32      label_count   │ 与 ysl/ysv 逐条互证(成果 B2)
//!        +0x18+n u32      text_count    ┘
//! 步长 = 0x1C + name_len
//! ```
//!
//! 旁线旧笔记把 `+0x18+n` 记作"脚本文件号"——**实测否定**:该字段值域
//! 0..=151 且仅 14 个不同值,不可能承担 141 个脚本的编号;编号 = `index`
//! (0..140 与磁盘 yst 文件一一对应)。`var_count`/`label_count` 对无脚本
//! 产物的条目(纯声明文件)取哨兵 `u32::MAX`。

use crate::container::{read_header, truncated};
use crate::code;
use yu_log::{Boundary, Diag};

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
    /// 容器头版本(`+0x04`;ERIS 语料 495。R12 起 yu-rt 适配层需要)。
    pub version: u32,
    /// 记录(文件顺序;`index` 在 0.495 语料中恰为升序 = 下标)。
    pub entries: Vec<Entry>,
}

impl ScriptList {
    /// 解码 `yst_list.ybn` 字节。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSTL")?;
        let count = header.word08 as usize;
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
            let name_len = u32::from_le_bytes([
                buf[at + 4],
                buf[at + 5],
                buf[at + 6],
                buf[at + 7],
            ]) as usize;
            let name_at = at
                .checked_add(8)
                .ok_or_else(|| limit(origin, at as u64, "YSTL 名字起点"))?;
            let tail_at = name_at
                .checked_add(name_len)
                .ok_or_else(|| limit(origin, name_at as u64, "YSTL 名字长度"))?;
            let end = tail_at
                .checked_add(0x14)
                .ok_or_else(|| limit(origin, tail_at as u64, "YSTL 记录尾"))?;
            if end > buf.len() {
                return Err(truncated(origin, end, buf.len(), "YSTL 记录尾"));
            }
            let mut tail = [0u8; 0x14];
            tail.copy_from_slice(&buf[tail_at..end]);
            let word = |i: usize| {
                u32::from_le_bytes([tail[i], tail[i + 1], tail[i + 2], tail[i + 3]])
            };
            entries.push(Entry {
                index,
                name: buf[name_at..tail_at].to_vec(),
                file_time: ((word(0) as u64) << 32) | word(4) as u64,
                var_count: word(8),
                label_count: word(12),
                text_count: word(16),
            });
            at = end;
        }
        Ok(Self {
            version: header.version,
            entries,
        })
    }
}

/// 记录长度/偏移超出可表示范围 = `fmt.limit`(32 位目标不回绕)。
fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}
