//! 故障快照(fault dump):最近 N tick 的 ring buffer + 离线重演。
//!
//! # 为什么不是"把全量 trace 留着"
//!
//! 官方 ERIS 首帧约 130 万条命令;长时间运行的全量 trace 会迅速到 GB 级,
//! 而排障真正需要的是**故障前最近若干 tick**的现场。ring buffer 只保留:
//!
//! - 每 tick 的**命令流摘要**(增量 SHA-256 + 事件种类计数,不保留命令本体);
//! - `Diag` 记录(含 `rt.cmd.unimplemented` 等挂号;有上限,丢弃计数);
//! - 输入时间线的显式帧(稀疏;用于离线重演);
//! - `frame_description`(有字节预算;超预算时退化为摘要,不静默丢帧)。
//!
//! # dump 的两份文件
//!
//! 1. `<path>`:schema v4 trace(header + ring 里保留的 diag / frame_description /
//!    vfs 失败记录)。`header` 的 `input` 摘要与 `clock` 序列按**故障帧**重算,
//!    样本 hash 由调用方在启动时写入 `producer.sample_sha256`。
//! 2. `<path>.timeline.json`:工具级伴生文件(trace schema 之外,不递增 schema)。
//!    schema v4 没有"输入时间线原始值"字段,而离线重演必须拿到原始输入;
//!    这里把时间线、每 tick 摘要与故障信息放在伴生文件,JSONL 保持纯 trace。
//!
//! 两个文件都先写临时文件再 `rename`,JSONL 最后落地(它是提交标记)。
//! 读取时校验 `trace_sha256`,不允许"读到半份 dump 还继续回放"。
//!
//! # 回放
//!
//! [`FaultDump::replay`] 用 dump 的 header 重建 `VmTrace`,按 dump 的 clock
//! 序列推进到故障 tick,并逐 tick 比较摘要。任何字段不一致立即 `trace.replay`
//! 失败(不做容差);零差异才返回报告。ring 必须在 VM 启动后、第一 tick 前
//! 挂上(`VmTrace::enable_fault_ring`),否则 dump 明确标注不可回放。

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use yu_log::{Boundary, Diag};
use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};

use crate::error;
use crate::event::{domain_name, jump_kind};
use crate::hash::{sha256_hex, Sha256};
use crate::json::{self, Fields, Json};
use crate::record::{
    payload_to_json, ClockSummary, FrameSummary, HeaderPayload, InputSummary, Kind, Payload,
    TraceLog,
};
use crate::replay::VmTrace;
use yu_vfs::DataSource;

/// 伴生文件版本(工具级 schema,与 trace schema 无关)。
pub const FAULT_MANIFEST_SCHEMA: u32 = 1;

/// ring buffer 容量与内存口径。
///
/// 默认值来自任务书 P5 §5:单 tick 摘要 ≤ 8 MiB,总预算 64 MiB。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RingConfig {
    /// 保留最近多少个 tick。
    pub ticks: usize,
    /// 单 tick 摘要字节预算(display 过大时先退化为摘要)。
    pub max_tick_bytes: usize,
    /// ring 总字节预算(超出时淘汰最旧 tick,并计数)。
    pub max_total_bytes: usize,
    /// 启动链(帧外)vfs 记录保留条数。
    pub max_boot_records: usize,
    /// 单 tick 保留的 Diag 条数(其余只计数)。
    pub max_diags_per_tick: usize,
    /// 单 tick 保留的输入帧条数(其余只计数)。
    pub max_inputs_per_tick: usize,
    /// 单条 frame_description 的 display JSON 上限(超出退化为摘要)。
    pub max_display_bytes: usize,
    /// 时间线显式帧上限(超出 ⇒ dump 标注不可回放)。
    pub max_timeline_entries: usize,
    /// 时钟序列上限(超出 ⇒ dump 标注不可回放)。
    pub max_clock_ticks: usize,
}

impl Default for RingConfig {
    fn default() -> Self {
        Self {
            ticks: 300,
            max_tick_bytes: 8 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
            max_boot_records: 1024,
            max_diags_per_tick: 64,
            max_inputs_per_tick: 4096,
            max_display_bytes: 1024 * 1024,
            max_timeline_entries: 200_000,
            max_clock_ticks: 1_000_000,
        }
    }
}

impl RingConfig {
    /// 校验:容量为 0 的 ring 没有意义,显式拒绝而不是运行时静默。
    pub fn validate(&self) -> Result<(), Diag> {
        if self.ticks == 0 {
            return Err(Diag::fatal(error::code::SCHEMA, Boundary::Format)
                .with_detail("ring config: ticks must be >= 1"));
        }
        for (name, value) in [
            ("max_tick_bytes", self.max_tick_bytes),
            ("max_total_bytes", self.max_total_bytes),
            ("max_display_bytes", self.max_display_bytes),
            ("max_timeline_entries", self.max_timeline_entries),
            ("max_clock_ticks", self.max_clock_ticks),
        ] {
            if value == 0 {
                return Err(Diag::fatal(error::code::SCHEMA, Boundary::Format)
                    .with_detail(format!("ring config: {name} must be >= 1")));
            }
        }
        Ok(())
    }
}

/// 触发 dump 的故障类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// `std::panic` hook。
    Panic,
    /// `catch_unwind` 捕获。
    CatchUnwind,
    /// 运行核 Fatal `Diag`。
    FatalDiag,
}

impl FaultKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            FaultKind::Panic => "panic",
            FaultKind::CatchUnwind => "catch_unwind",
            FaultKind::FatalDiag => "fatal_diag",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "panic" => Some(FaultKind::Panic),
            "catch_unwind" => Some(FaultKind::CatchUnwind),
            "fatal_diag" => Some(FaultKind::FatalDiag),
            _ => None,
        }
    }
}

/// dump 的故障描述(宿主在 panic hook / catch_unwind / Fatal Diag 处构造)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultInfo {
    pub kind: FaultKind,
    /// 稳定的短原因(如 `panic` / `rt.cmd.unimplemented`)。
    pub reason: String,
    /// 原始文本(panic message / Diag detail);只写伴生文件,不进 trace 记录。
    pub detail: String,
}

impl FaultInfo {
    pub fn new(kind: FaultKind, reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
            detail: detail.into(),
        }
    }

    /// panic hook 的 `PanicHookInfo::to_string()`。
    pub fn panic(detail: impl Into<String>) -> Self {
        Self::new(FaultKind::Panic, "panic", detail)
    }

    /// `catch_unwind` 捕获到的 panic 文本。
    pub fn caught(detail: impl Into<String>) -> Self {
        Self::new(FaultKind::CatchUnwind, "panic", detail)
    }

    /// 运行核 Fatal `Diag`。
    pub fn fatal(diag: &Diag) -> Self {
        Self::new(
            FaultKind::FatalDiag,
            diag.code.to_string(),
            diag.detail.clone(),
        )
    }
}

/// 一条 Diag 摘要(含 Unimplemented 挂号标记)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagEntry {
    pub frame: u64,
    pub code: String,
    pub detail: String,
    /// `code` 含 `unimplemented`(运行核的"有名字但未实现"挂号)。
    pub unimplemented: bool,
}

/// 一条输入帧(显式设定的帧;稀疏)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputEntry {
    pub frame: u64,
    pub cursor: (i64, i64),
    pub pressed: Vec<String>,
    pub pulses: Vec<String>,
}

impl InputEntry {
    pub fn from_input(frame: u64, input: &InputFrame) -> Self {
        Self {
            frame,
            cursor: input.cursor,
            pressed: input.pressed.iter().cloned().collect(),
            pulses: input.pulses.iter().cloned().collect(),
        }
    }

    pub fn to_input(&self) -> InputFrame {
        InputFrame {
            pressed: self.pressed.iter().cloned().collect(),
            pulses: self.pulses.iter().cloned().collect(),
            cursor: self.cursor,
        }
    }
}

/// tick 内保留的 frame_description 摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameTickEntry {
    pub frame: u64,
    pub display_sha256: String,
    /// `false` = 因字节预算被退化为纯摘要(display 已丢弃)。
    pub display_retained: bool,
    pub pass_count: u32,
    pub op_count: u64,
    pub executable_ops: u64,
    pub skipped_ops: u64,
}

/// 单个 tick 的摘要。replay 逐 tick 精确比较这个结构。
#[derive(Clone, Debug, PartialEq)]
pub struct TickSummary {
    pub index: u64,
    pub first_frame: u64,
    /// 本 tick 消费的逻辑帧数;`0` = 没有跑任何逻辑帧。
    pub frames: u64,
    /// 本 tick 最后一个逻辑帧(`first_frame + frames - 1`;`frames = 0` 时等于
    /// `first_frame - 1`,仅用于排序)。
    pub last_frame: u64,
    pub now_ms: Option<u32>,
    pub command_count: u64,
    pub command_digest: String,
    pub event_digest: String,
    /// 事件种类 → 条数(顺序稳定)。
    pub event_counts: BTreeMap<String, u64>,
    pub diags: Vec<DiagEntry>,
    pub diags_dropped: u64,
    pub inputs: Vec<InputEntry>,
    pub inputs_dropped: u64,
    pub frames_desc: Vec<FrameTickEntry>,
    pub displays_dropped: u64,
    /// 摘要自身字节数(display 按规范文本长度计)。
    pub bytes: usize,
    /// 超出 `max_tick_bytes` 且已无更多可裁剪项。
    pub over_budget: bool,
    /// `false` = 单个 tick 超出 ring 总预算,只保留占位摘要(明确计数)。
    pub retained: bool,
    /// dump 用的实际记录(diag / frame_description / vfs 失败);读取伴生文件时为空。
    pub(crate) entries: Vec<(u64, Payload)>,
}

impl TickSummary {
    fn placeholder(
        index: u64,
        first_frame: u64,
        frames: u64,
        bytes: usize,
        now_ms: Option<u32>,
    ) -> Self {
        Self {
            index,
            first_frame,
            frames,
            last_frame: first_frame.saturating_add(frames).saturating_sub(1),
            now_ms,
            command_count: 0,
            command_digest: sha256_hex(&[]),
            event_digest: sha256_hex(&[]),
            event_counts: BTreeMap::new(),
            diags: Vec::new(),
            diags_dropped: 0,
            inputs: Vec::new(),
            inputs_dropped: 0,
            frames_desc: Vec::new(),
            displays_dropped: 0,
            bytes,
            over_budget: true,
            retained: false,
            entries: Vec::new(),
        }
    }
}

/// 伴生文件中的输入时间线。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultInput {
    pub events: u64,
    pub sha256: String,
    pub entries: Vec<InputEntry>,
    pub truncated: bool,
}

/// 伴生文件内容。
#[derive(Clone, Debug, PartialEq)]
pub struct FaultManifest {
    pub trace_sha256: String,
    pub producer: crate::record::ProducerInfo,
    pub codec: String,
    pub fault: FaultInfo,
    /// 故障发生时正在记录的 tick(当前 tick 或最近已收口 tick)。
    pub fault_tick: Option<u64>,
    /// 故障时已消费的最后一个逻辑帧。
    pub fault_frame: u64,
    pub ring: RingConfig,
    pub input: FaultInput,
    pub clock: ClockSummary,
    /// ring 挂上时的首个逻辑帧;`1` = 从新建 VM 起录制(可回放)。
    pub start_frame: u64,
    pub last_frame: u64,
    pub orphan_events: u64,
    pub vfs_records: u64,
    pub vfs_dropped: u64,
    pub boot_records_dropped: u64,
    pub timeline_truncated: bool,
    pub clock_truncated: bool,
    pub evicted_ticks: u64,
    pub ticks: Vec<TickSummary>,
}

impl FaultManifest {
    /// dump 是否可以从新建 VM 起完整重演。
    ///
    /// 除了"从第一帧起录制",还要求 tick **从 0 起连续、一个都没被淘汰**:
    /// 否则回放到中间会找不到对应 tick,不能声称复现了故障。
    pub fn replayable(&self) -> bool {
        if self.start_frame != 1 || self.timeline_truncated || self.clock_truncated {
            return false;
        }
        let expected = self.fault_tick.map(|tick| tick + 1).unwrap_or(0);
        if self.ticks.len() as u64 != expected {
            return false;
        }
        self.ticks
            .iter()
            .enumerate()
            .all(|(index, tick)| tick.index == index as u64 && tick.retained)
    }
}

// ------------------------------------------------------------------ ring 内部

// ------------------------------------------------------------------ 子模块
//
// 2026-10-23 纯搬运拆分:ring(状态机/哈希)、dump(落盘/JSON)、
// replay(离线重演)。公开路径保持 `crate::fault::*` 不变。

mod dump;
mod replay;
mod ring;

pub(crate) use dump::write_dump;
pub use dump::{FaultDump, FaultDumpPaths};
pub use replay::FaultReplayReport;
pub(crate) use replay::{clock_at, clock_ticks, compare_ticks};
pub(crate) use ring::FaultRing;
