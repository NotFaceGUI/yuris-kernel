//! `yse.ybn`(魔数 `YSER`)· 名称表:`u32 id` + NUL 结尾名字。
//!
//! 布局(旁线 `format-ybn.md` §7,引擎装载器 `FUN_00465a54` 把 id 与名字
//! 装进两张并行表;ERIS 语料 121 条闭合,内容为引擎错误文案模板,如
//! 100010 = "メモリ不足です。"):
//!
//! ```text
//! +0x00 char[4] "YSER"
//! +0x04 u32     版本
//! +0x08 u32     条数
//! +0x0C u32     观测为 0
//! +0x10 记录[条数]: u32 id, char[] name(NUL 结尾,CP932)
//! ```

use crate::code;
use crate::container::{find_nul, read_header};
use yu_log::{Boundary, Diag};

/// 一条 id ↔ 名字记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 数字 id(如 100010)。
    pub id: u32,
    /// 名字(原始 CP932 字节;0.495 语料中为含 `%s` 占位的错误文案)。
    pub name: Vec<u8>,
}

/// 已解码的名称表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YserTable {
    /// 记录(文件顺序)。
    pub entries: Vec<Entry>,
}

impl YserTable {
    /// 解码 `yse.ybn` 字节。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSER")?;
        let count = header.word08 as usize;
        let mut at = 0x10usize;
        let mut entries = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            let name_at = at
                .checked_add(4)
                .ok_or_else(|| limit(origin, at as u64, "YSER 记录头"))?;
            if name_at > buf.len() {
                return Err(crate::container::truncated(
                    origin,
                    name_at,
                    buf.len(),
                    "YSER 记录头",
                ));
            }
            let id = crate::container::u32_at(buf, at);
            let end = find_nul(buf, name_at, origin, "YSER 名字")?;
            entries.push(Entry {
                id,
                name: buf[name_at..end].to_vec(),
            });
            at = end
                .checked_add(1)
                .ok_or_else(|| limit(origin, end as u64, "YSER 记录步进"))?;
        }
        Ok(Self { entries })
    }
}

/// 记录长度/偏移超出可表示范围 = `fmt.limit`(32 位目标不回绕)。
fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}
