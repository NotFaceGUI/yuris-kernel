//! `yu-trace` · 旁挂对拍基建(边界①vfs 读 / ②事件流)。
//!
//! # 定位
//!
//! 本 crate **旁挂**:可依赖 `yu-log` / `yu-vfs` / `yu-rt`,但任何 crate 都不得
//! 依赖它(architecture §2)。它只做两件事:
//!
//! 1. [`TracingSource`] 包装任意 [`yu_vfs::DataSource`],记录 `open/exists/list`
//!    的调用事实与结果身份(边界①);
//! 2. [`EventRecorder`] 拉取 [`yu_rt::Vm::take_events`],把 [`yu_rt::VmEvent`]
//!    映射为稳定记录(边界②,穷尽 match,新增变体即编译失败)。
//!
//! ③帧描述 / ④平台命令只保留 [`record::Sub::Frame`] / [`record::Sub::Platform`]
//! 与 [`record::Kind::FrameDescription`] / [`record::Kind::PlatformCall`] 占位,
//! 本版本拒绝其记录(等 M3)。
//!
//! # 确定性
//!
//! - 输出中不存在墙钟、线程调度、`HashMap` 迭代序与绝对路径;
//! - `seq` 是录制顺序的唯一序,`frame` 只来自注入的帧号;
//! - 内容身份用本地 SHA-256(见 [`hash`]),不引入新依赖。
//!
//! # 错误
//!
//! 全部走 [`yu_log::Diag`],码域 `trace.*`(见 [`error::code`]);本 crate
//! 没有任何写标准输出的路径。
//!
//! 协议定义见 crate 根 `SCHEMA.md`。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod diff;
pub mod error;
pub mod event;
pub mod hash;
mod json;
pub mod record;
pub mod replay;
pub mod source;

pub use diff::{diff_logs, diff_text, DiffFailure, DiffReport};
pub use error::code;
pub use error::sample_missing;
pub use event::{
    domain_name, header_from_vm, input_summary, input_summary_range, jump_kind, map_event,
    shared_log, trace_value, EventRecorder, SharedLog,
};
pub use hash::sha256_hex;
pub use record::{
    argv_summary, paths_summary, BootSummary, HeaderPayload, HostSummary, InputSummary, Kind,
    ListEntry, Payload, Record, Sub, TraceArg, TraceLog, TraceValue, SCHEMA_VERSION,
};
pub use replay::{
    codec_from_name, record_events, verify_input, verify_input_range, VmTrace,
};
pub use source::{BoxedSource, FrameHandle, TracingSource};

/// 本 crate 版本(写入 header 的 `kernel` 字段)。
pub fn kernel_version() -> &'static str {
    concat!("yu-trace ", env!("CARGO_PKG_VERSION"))
}

/// 录制文件目录:优先 `YURIS_TRACE_DIR`,否则 `target/tmp/trace`。
///
/// 目录只决定文件写到哪里,不进入任何记录内容(确定性约束)。
/// 返回的路径可能是相对路径;调用方负责创建目录。
pub fn trace_dir() -> std::path::PathBuf {
    match std::env::var_os("YURIS_TRACE_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::path::PathBuf::from("target").join("tmp").join("trace"),
    }
}

/// 把日志写到 [`trace_dir`] 下的 `name`(目录自动创建)。
///
/// `name` 只允许单层文件名,避免把任意路径带进调用点。
pub fn write_trace(log: &TraceLog, name: &str) -> Result<std::path::PathBuf, yu_log::Diag> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
    {
        return Err(yu_log::Diag::fatal(error::code::IO, yu_log::Boundary::Data)
            .with_detail(format!("trace name must be a plain file name, got {name:?}")));
    }
    let path = trace_dir().join(name);
    log.write_to(&path)?;
    Ok(path)
}
