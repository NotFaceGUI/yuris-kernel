//! `yu-save` · YU-RIS 存档/文件域宿主(SAVE / LOAD / SAVEEND / FILEACT)。
//!
//! 分层(与 `yu-sound` 同范式,受任务书 P4 文件占用约束的变体):
//!
//! | 模块 | 文件 | 内容 |
//! |---|---|---|
//! | [`error`] | 诊断码 | `sv.*` 稳定码表 |
//! | [`state`] | 状态机 | 槽表(盘上事实)+ SAVE/LOAD/SAVEEND/FILEACT 事实消费 |
//! | [`host`] | 接线 | `SaveHost`(Engine 侧驱动,不加 `yu-rt` trait) |
//!
//! 字节级事实归 [`yu_format::yssd`](yu_format::yssd)(存档容器,官方
//! 产物逐字节 roundtrip);本 crate 只做域状态与事件事实。
//!
//! # 本轮硬边界(不猜)
//!
//! - `SAVE[SET]` / `LOAD[LET]` 的**变量引用身份**在 `DomainCall` 事件层
//!   丢失(DIV-RT-042)⇒ 写刷盘显式拒绝(`sv.write.refused`),读侧照常;
//! - `SAVEINFO` 查询未取证(REA `0x44e1c8`,语料 0 次)⇒ 挂账;
//! - `FILEACT[EXEC]` 拒绝执行;图层导出挂账 H-P4-02;
//! - FILEACT 落盘仅在显式可写 save 根下允许(默认只读观察)。
//!
//! `#![forbid(unsafe_code)]`;src 零 println/eprintln/dbg。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod error;
pub mod host;
pub mod state;

pub use host::SaveHost;
pub use state::{ReservedFact, SaveSnapshot, SaveState, SlotFile};
