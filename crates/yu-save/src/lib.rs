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
//! # 本轮边界(B1 后)
//!
//! - SAVE 预约/刷盘与 LOAD 读块走 `yu-rt::event::SaveHost` 同步契约
//!   (B1 裁 1-A/裁 2-A);**默认可写**(2026-10-24 主人令;只读为显式
//!   构造态,拒绝时挂号 `sv.write.refused`);
//! - 预约 = 命令时刻值快照(REA `0x44d73d`),`flush` 不清预约,
//!   `save_end` 清(REA `0x44e1a0`/语料 es_GSD 同型);
//! - `SAVEINFO` 查询未取证(REA `0x44e1c8`,语料 0 次)⇒ 挂账 B7;
//! - `FILEACT[EXEC]` 拒绝执行;图层导出挂账 H-P4-02;
//! - FILEACT 落盘默认允许;只读为显式构造态(单测)。
//!
//! `#![forbid(unsafe_code)]`;src 零 println/eprintln/dbg。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod error;
pub mod fs;
pub mod host;
pub mod image_block;
pub mod state;

pub use fs::{FsEntry, NativeSaveFs, SaveFs, SaveFsHandle};
pub use host::SaveHost;
pub use state::{ReservedFact, SaveSnapshot, SaveState, SlotFile};

/// 存档根物化(平台端口):目标平台有真实文件系统时实际建目录
/// (缺目录 = SAVE 刷盘与 kidoku 副档写入全部静默失败,2026-10-25 实测)。
///
/// 走注入的 [`SaveFs`] 端口,因此不依赖 `std::fs` 是否可用:
/// - 桌面 / Android:`NativeSaveFs` 建目录(与旧行为一致);
/// - 无文件系统的宿主(浏览器 wasm)由宿主注入自己的实现,或在此 no-op
///   后按落盘失败对齐(平台分歧 `docs/divergences/DIV-WEB-002`)。
pub fn ensure_save_root(dir: &std::path::Path) -> Result<(), String> {
    ensure_save_root_with(dir, &NativeSaveFs)
}

/// [`ensure_save_root`] 的端口版本。
pub fn ensure_save_root_with(dir: &std::path::Path, fs: &dyn SaveFs) -> Result<(), String> {
    fs.create_dir_all(dir)
        .map_err(|e| format!("create_dir_all({}) 失败: {e}", dir.display()))
}
