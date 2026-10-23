//! fault replay:离线重演报告与逐 tick 摘要比较。
//!
//! 从 `fault.rs` 纯搬运(2026-10-23);行为不变。

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultReplayReport {
    pub ticks_replayed: u64,
    pub frames_replayed: u64,
    pub fault_tick: Option<u64>,
    pub fault_frame: u64,
}

impl std::fmt::Display for FaultReplayReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fault replay OK: {} ticks / {} frames to fault tick {:?} (frame {})",
            self.ticks_replayed, self.frames_replayed, self.fault_tick, self.fault_frame
        )
    }
}

/// `ClockSummary` → 逐 tick 原始序列(`verify_clock` 的输入)。
///
/// `tick` 模式不携带列表,但每一项都是内部时钟(`None`);用 dump 里
/// 实际的 tick 数补出来,不能把"没有列表"当成"没有 tick"。
pub(crate) fn clock_ticks(clock: &ClockSummary, tick_count: u64) -> Vec<Option<u32>> {
    match clock.mode {
        crate::record::ClockMode::None => Vec::new(),
        crate::record::ClockMode::Tick => vec![None; tick_count as usize],
        crate::record::ClockMode::TickAt | crate::record::ClockMode::Mixed => clock.now_ms.clone(),
    }
}

pub(crate) fn clock_at(clock: &ClockSummary, index: usize) -> Result<Option<u32>, Diag> {
    match clock.mode {
        crate::record::ClockMode::None => Err(Diag::fatal(error::code::REPLAY, Boundary::Domain)
            .with_detail(format!(
                "fault dump has no recorded clock but tick {index} exists"
            ))),
        crate::record::ClockMode::Tick => Ok(None),
        crate::record::ClockMode::TickAt | crate::record::ClockMode::Mixed => {
            clock.now_ms.get(index).copied().ok_or_else(|| {
                Diag::fatal(error::code::REPLAY, Boundary::Domain).with_detail(format!(
                    "clock sequence has no entry for tick {index} (len {})",
                    clock.now_ms.len()
                ))
            })
        }
    }
}

/// 逐 tick 比较;返回首个差异的 `path expected actual`,零差异返回 `None`。
pub(crate) fn compare_ticks(expected: &TickSummary, actual: &TickSummary) -> Option<String> {
    let check = |path: &str, expected: String, actual: String| {
        if expected == actual {
            None
        } else {
            Some(format!("path={path} expected={expected} actual={actual}"))
        }
    };
    if let Some(diff) = check(
        "/index",
        expected.index.to_string(),
        actual.index.to_string(),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/first_frame",
        expected.first_frame.to_string(),
        actual.first_frame.to_string(),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/frames",
        expected.frames.to_string(),
        actual.frames.to_string(),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/now_ms",
        format!("{:?}", expected.now_ms),
        format!("{:?}", actual.now_ms),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/command_count",
        expected.command_count.to_string(),
        actual.command_count.to_string(),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/command_digest",
        expected.command_digest.clone(),
        actual.command_digest.clone(),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/event_digest",
        expected.event_digest.clone(),
        actual.event_digest.clone(),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/event_counts",
        format!("{:?}", expected.event_counts),
        format!("{:?}", actual.event_counts),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/diags",
        format!("{:?}", expected.diags),
        format!("{:?}", actual.diags),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/diags_dropped",
        expected.diags_dropped.to_string(),
        actual.diags_dropped.to_string(),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/inputs",
        format!("{:?}", expected.inputs),
        format!("{:?}", actual.inputs),
    ) {
        return Some(diff);
    }
    if let Some(diff) = check(
        "/inputs_dropped",
        expected.inputs_dropped.to_string(),
        actual.inputs_dropped.to_string(),
    ) {
        return Some(diff);
    }
    // 呈现层字段(frames_desc / displays_dropped / bytes / over_budget)不参与
    // 重演比较:离线 VM 不重建画面,这些字段只作为 dump 现场保留。参与比较的
    // 每个字段仍精确相等,不是容差。
    check(
        "/retained",
        expected.retained.to_string(),
        actual.retained.to_string(),
    )
}
