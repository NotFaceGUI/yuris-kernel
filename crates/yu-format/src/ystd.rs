//! `yst.ybn`(魔数 `YSTD`)· 变量槽位表头(0.495 语料中恰好 16 字节)。
//!
//! 布局(旁线 `format-ybn.md` §10,引擎装载器 `FUN_0045d650`):
//!
//! ```text
//! +0x00 char[4] "YSTD"
//! +0x04 u32     版本 —— 0.495 引擎显式校验 == 495(0x19a28);商业构建各有
//!               自己的值(实测 0.481 = 481)。**读取不校验**,只记录
//! +0x08 u32     varCount(槽位数 = varCount + 1,下标 0 保留;每槽 64 字节)
//! +0x0C u32     未知(引擎存入 DAT_0080174c;原样保留)
//! ```
//!
//! 版本是构建戳而不是布局选择器:实测版本的头部同构,合法性由 `varCount`
//! 与 `ysv` 槽位上界决定,不按版本等值拒绝。

use crate::container::{read_header, u32_at};
use crate::code;
use yu_log::{Boundary, Diag};

/// 已解码的槽位表头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YstdHeader {
    /// 版本(只记录,不校验;0.495 = 495,0.481 商业包 = 481)。
    pub version: u32,
    /// 变量槽数基数(实际槽位数 = `var_count + 1`)。
    pub var_count: u32,
    /// 保留字段(引擎存 `DAT_0080174c`;原样保留)。
    pub unknown: u32,
}

impl YstdHeader {
    /// 解码 `yst.ybn` 字节(头 + `varCount` 自洽;版本只记录)。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSTD")?;
        if buf.len() < 16 {
            return Err(crate::container::truncated(origin, 16, buf.len(), "YSTD 头"));
        }
        let var_count = u32_at(buf, 8);
        if var_count == u32::MAX {
            // 槽位数 = var_count + 1;u32::MAX 无法表示 +1,引擎侧此值也非法。
            return Err(Diag::fatal(code::LIMIT, Boundary::Format)
                .at_data(origin, 8)
                .with_detail("YSTD varCount = u32::MAX,槽位数 varCount+1 溢出"));
        }
        Ok(Self {
            version: header.version,
            var_count,
            unknown: u32_at(buf, 0x0C),
        })
    }

    /// 槽位数(= `var_count + 1`,下标 0 保留)。构造时已拒绝溢出值。
    #[must_use]
    pub const fn slot_count(&self) -> u32 {
        // `parse` 拒绝 u32::MAX;直接构造的 `YstdHeader` 可能是任意值,
        // 此时饱和而不是 panic(`slot_count` 是只读查询)。
        self.var_count.saturating_add(1)
    }
}
