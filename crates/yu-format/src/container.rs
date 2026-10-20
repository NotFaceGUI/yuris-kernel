//! `.ybn` 家族通用容器头与字节读取工具。
//!
//! 家族成员(证据:ERIS 语料实测 + 旁线 `format-ybn.md` §1):
//! `YSCM`(ysc)/`YSCF`(yscfg)/`YSER`(yse)/`YSLB`(ysl)/`YSTD`(yst)/
//! `YSTL`(yst_list)/`YSVR`(ysv) 七张表为**明文魔数**;`YSTB`(yst%05d)
//! 头部明文、区段异或(见 [`crate::ystb`])。
//!
//! 头部通形:`+0x00` 魔数 `char[4]`,`+0x04` 版本 `u32`。各表版本字段
//! 在 0.495 语料中恒为 495;除 YSTD(引擎显式校验 ==495)外,本层只
//! 记录版本、不做范围断言(无引擎证据的校验一律不加)。

use crate::code;
use yu_log::{Boundary, Diag};

/// 已解析的 16 字节容器头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// `+0x04` 版本(ERIS 语料为 495)。
    pub version: u32,
    /// `+0x08` 第三个字(多数表 = 记录条数,语义见各表)。
    pub word08: u32,
    /// `+0x0C` 第四个字(ERIS 语料观测为 0)。
    pub word0c: u32,
}

/// 读容器头并校验魔数。`origin` 用于诊断定位(如 `ysbin/ysc.ybn`)。
pub fn read_header(buf: &[u8], origin: &[u8], magic: &[u8; 4]) -> Result<Header, Diag> {
    if buf.len() < 16 {
        return Err(truncated(origin, 16, buf.len(), "容器头"));
    }
    if &buf[0..4] != magic {
        return Err(Diag::fatal(code::MAGIC, Boundary::Format)
            .at_data(origin, 0)
            .with_detail(format!(
                "魔数不符:期望 {:?},实际 {:?}",
                std::str::from_utf8(magic).unwrap_or("?"),
                String::from_utf8_lossy(&buf[0..4])
            )));
    }
    Ok(Header {
        version: u32_at(buf, 4),
        word08: u32_at(buf, 8),
        word0c: u32_at(buf, 0x0C),
    })
}

pub(crate) fn truncated(origin: &[u8], need: usize, have: usize, what: &str) -> Diag {
    Diag::fatal(code::TRUNCATED, Boundary::Format)
        .at_data(origin, have as u64)
        .with_detail(format!("{what}:需要 {need} 字节,实际 {have} 字节"))
}

pub(crate) fn u32_at(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

pub(crate) fn u16_at(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([buf[at], buf[at + 1]])
}

/// 找 NUL 终止符;找不到 = 截断。
pub(crate) fn find_nul(buf: &[u8], from: usize, origin: &[u8], what: &str) -> Result<usize, Diag> {
    buf[from..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| from + p)
        .ok_or_else(|| truncated(origin, buf.len() + 1, buf.len(), what))
}
