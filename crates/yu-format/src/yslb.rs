//! `ysl.ybn`(魔数 `YSLB`)· 全局标签表:标签名 → 脚本号 + 位置等字段。
//!
//! 布局(ERIS 语料实测闭合;旁线 `format-ybn.md` §8b,引擎装载器
//! `FUN_0045e0d0`,引擎把 `+0x0C` 的 0x400 字节块整块拷入 `DAT_007ad600`
//! 并把首 4 字节清零):
//!
//! ```text
//! +0x00 char[4] "YSLB"
//! +0x04 u32     版本
//! +0x08 u32     条数(0.495 语料 = 1941)
//! +0x0C byte[0x400]  名字索引块(引擎整块拷入 DAT_007ad600 并清零首 4 字节;
//!                    ERIS 实测内容为递增 u32 序列,疑似桶偏移,语义待证)
//! +0x40C 记录[条数]:
//!        +0x00 u8        name_len
//!        +0x01 byte[n]   name        标签名(CP932 字节,如 `es.ERIS`)
//!        +1+n   u32      field_a     (疑似名字哈希;待证)
//!        +5+n   u32      field_b     (疑似脚本内指令位置;待证)
//!        +9+n   u16      script_no   脚本号 = yst%05d 编号(实证)
//!        +B+n   u8       field_d     (→ 引擎任务 +0x3E;待证)
//!        +C+n   u8       field_e     (→ 引擎任务 +0x244;待证)
//! 步长 = 0x0D + name_len
//! ```
//!
//! 头部 = 魔数 + 版本 + 条数(12 字节),索引块紧随其后(`+0x0C`),
//! 记录区在 `+0x40C`(ERIS 语料逐字节实证;旁线笔记口径一致)。

use crate::code;
use crate::container::{read_header, truncated};
use yu_log::{Boundary, Diag};

/// 名字索引块字节数(引擎整块拷入 `DAT_007ad600`)。
pub const HASH_BLOCK_BYTES: usize = 0x400;

/// 一条全局标签记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelRecord {
    /// 标签名(原始 CP932 字节;跨文件全局唯一,0.495 语料 1941 条无重名)。
    pub name: Vec<u8>,
    /// 保留字段 A(疑似名字哈希;待证,原样保留)。
    pub field_a: u32,
    /// 保留字段 B(疑似脚本内指令位置;待证,原样保留)。
    pub field_b: u32,
    /// 标签所在脚本号(= `yst%05d.ybn` 编号;引擎拷入任务 +0x3C)。
    pub script_no: u16,
    /// 保留字段 D(原样保留)。
    pub field_d: u8,
    /// 保留字段 E(原样保留)。
    pub field_e: u8,
}

/// 已解码的全局标签表(`ysl.ybn`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YslLabels {
    /// 容器头版本(`+0x04`;ERIS 语料 495。R12 起 yu-rt 适配层需要)。
    pub version: u32,
    /// 名字索引块(原样保留,首 4 字节引擎会清零)。
    pub hash_block: Box<[u8; HASH_BLOCK_BYTES]>,
    /// 标签记录(文件顺序)。
    pub entries: Vec<LabelRecord>,
}

impl YslLabels {
    /// 解码 `ysl.ybn` 字节。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSLB")?;
        let count = header.word08 as usize;
        let block_at = 0x0Cusize;
        let records_at = block_at + HASH_BLOCK_BYTES;
        if records_at > buf.len() {
            return Err(truncated(origin, records_at, buf.len(), "YSLB 索引块"));
        }
        let mut hash_block = Box::new([0u8; HASH_BLOCK_BYTES]);
        hash_block.copy_from_slice(&buf[block_at..records_at]);
        let mut at = records_at;
        let mut entries = Vec::with_capacity(count.min(8192));
        for _ in 0..count {
            let name_len = *buf.get(at).ok_or_else(|| {
                truncated(origin, at.saturating_add(1), buf.len(), "YSLB 名字长度")
            })? as usize;
            let name_at = at
                .checked_add(1)
                .ok_or_else(|| limit(origin, at as u64, "YSLB 名字起点"))?;
            let tail_at = name_at
                .checked_add(name_len)
                .ok_or_else(|| limit(origin, name_at as u64, "YSLB 名字长度"))?;
            let end = tail_at
                .checked_add(12)
                .ok_or_else(|| limit(origin, tail_at as u64, "YSLB 记录尾"))?;
            if end > buf.len() {
                return Err(truncated(origin, end, buf.len(), "YSLB 记录尾"));
            }
            let word = |i: usize| {
                u32::from_le_bytes(buf[tail_at + i..tail_at + i + 4].try_into().unwrap())
            };
            entries.push(LabelRecord {
                name: buf[name_at..tail_at].to_vec(),
                field_a: word(0),
                field_b: word(4),
                script_no: u16::from_le_bytes([buf[tail_at + 8], buf[tail_at + 9]]),
                field_d: buf[tail_at + 10],
                field_e: buf[tail_at + 11],
            });
            at = end;
        }
        Ok(Self {
            version: header.version,
            hash_block,
            entries,
        })
    }

    /// 按名字精确查找(字节级比较;标签名以 CP932 字节为键)。
    #[must_use]
    pub fn find(&self, name: &[u8]) -> Option<&LabelRecord> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// 记录长度/偏移超出可表示范围 = `fmt.limit`(32 位目标不回绕)。
fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}
