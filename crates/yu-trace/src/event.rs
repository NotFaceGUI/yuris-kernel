//! 边界②:`VmEvent` → 记录的穷尽映射与录制器。
//!
//! # 为什么是穷尽 match
//!
//! `yu-rt` 新增一个事件变体时,本文件必须编译失败——这会强制做一次协议评审,
//! 而不是让新事件悄悄漏出 trace。因此这里**没有** `_ =>` 兜底分支,
//! 连 `JumpKind` / `Domain` 也由本 crate 自己映射(字符串归本 crate 所有)。
//!
//! # 接入方式
//!
//! 只拉取 [`yu_rt::Vm::take_events`],不在 `yu-rt` 内写任何 trace 代码。
//! 事件按 `take_events` 返回顺序入流,`seq` 是唯一序。

use std::sync::{Arc, Mutex, MutexGuard};

use yu_rt::event::{Domain, JumpKind, VmEvent};

use crate::fault::{FaultDumpPaths, FaultInfo, FaultRing, RingConfig, TickSummary};
use crate::hash::sha256_hex;
use crate::record::{
    BootSummary, ClockSummary, FrameDescriptionInput, HeaderPayload, InputSummary, Payload,
    ProducerInfo, TraceArg, TraceLog, TraceValue,
};

/// 共享日志句柄。`DataSource: Send`(音频线程需要),录制源也必须可跨线程
/// 传递;内部用 Mutex 保护日志。
pub type SharedLog = Arc<Mutex<TraceLog>>;

/// 新建共享日志。
pub fn shared_log() -> SharedLog {
    Arc::new(Mutex::new(TraceLog::new()))
}

/// 锁中毒时取回内部值:故障快照在 panic 路径上执行,不能因为前一个
/// 持有者 panic 而拒绝写 dump。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// 从 [`yu_rt::BootConfig`] 与 [`yu_rt::Vm`] 构造头部。
///
/// `tables` 来自 `Vm::table_provenance()`(只含相对路径与表版本);
/// `input` 由调用方用 [`input_summary`] 从同一条时间线算出。
pub fn header_from_vm(
    vm: &yu_rt::Vm,
    cfg: &yu_rt::BootConfig,
    codec: yu_vfs::Codec,
    input: InputSummary,
) -> HeaderPayload {
    header_from_vm_with_clock(vm, cfg, codec, input, ClockSummary::none())
}

/// 同 [`header_from_vm`],并写入录制时钟序列(回放闭包)。
pub fn header_from_vm_with_clock(
    vm: &yu_rt::Vm,
    cfg: &yu_rt::BootConfig,
    codec: yu_vfs::Codec,
    input: InputSummary,
    clock: ClockSummary,
) -> HeaderPayload {
    HeaderPayload {
        codec: codec.as_str().to_string(),
        producer: ProducerInfo::kernel(),
        tables: vm.table_provenance(),
        boot: BootSummary::from_config(cfg),
        input,
        clock,
        kernel: format!("yu-trace {}", env!("CARGO_PKG_VERSION")),
    }
}

/// 输入时间线摘要:覆盖 `[first, first + frames)` 每帧的**有效**输入
/// (按键集合 + 光标)。
///
/// 规范字节序(全部小端):
/// `frame:u64, x:i64, y:i64, count:u64, (len:u64, name bytes)*`。
/// `events` 是时间线中显式设定的帧数(`InputTimeline::len`),用于区分
/// "空时间线"与"有设定但都为空帧"。
pub fn input_summary_range(
    timeline: &yu_rt::InputTimeline,
    first: u64,
    frames: u64,
) -> InputSummary {
    let mut bytes = Vec::new();
    for frame in first..first.saturating_add(frames) {
        let state = timeline.effective(frame);
        // 单帧脉冲(滚轮)只在该精确帧算“按下”;不能沿用 effective 的保持。
        let mut pressed = state.pressed.clone();
        if let Some(exact) = timeline.at(frame) {
            pressed.extend(exact.pulses.iter().cloned());
        }
        bytes.extend_from_slice(&frame.to_le_bytes());
        bytes.extend_from_slice(&state.cursor.0.to_le_bytes());
        bytes.extend_from_slice(&state.cursor.1.to_le_bytes());
        bytes.extend_from_slice(&(pressed.len() as u64).to_le_bytes());
        for name in &pressed {
            bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
        }
    }
    InputSummary {
        events: timeline.len() as u64,
        sha256: sha256_hex(&bytes),
    }
}

/// 从新建 `Vm` 起跑的前 `frames` 个逻辑帧的输入摘要。
///
/// `yu_rt::Vm::tick` / `tick_at` 每帧先 `advance(scheduler.frame() + 1)`,
/// 所以新建 VM 的首个逻辑帧是 **1** 而不是 0。摘要必须覆盖 VM 实际消费的帧
/// (`[1, 1 + frames)`),否则会漏掉最后一帧、多算一个未被消费的帧。
pub fn input_summary(timeline: &yu_rt::InputTimeline, frames: u64) -> InputSummary {
    input_summary_range(timeline, 1, frames)
}

/// 宿主/任务书里的 `TraceRecorder` 别名(本 crate 的实现类型是
/// [`EventRecorder`];功能相同,不另造类型)。
pub type TraceRecorder = EventRecorder;

/// 事件录制器。持有共享日志,可在 VFS 包装器与 `Vm` 事件拉取之间共用。
///
/// # ring buffer 模式
///
/// [`EventRecorder::enable_ring`] 后,带帧的记录不再进入无界 `TraceLog`,
/// 而是进入 [`FaultRing`]:只保留最近 N tick 的摘要与故障现场。ring 槽位
/// 是克隆间共享的,`TracingSource` 在启用前后都能看到同一份 ring。
#[derive(Clone)]
pub struct EventRecorder {
    log: SharedLog,
    ring: Arc<Mutex<Option<FaultRing>>>,
}

impl EventRecorder {
    pub fn new(log: SharedLog) -> Self {
        Self {
            log,
            ring: Arc::new(Mutex::new(None)),
        }
    }

    /// 新建日志并写入头部。
    pub fn with_header(header: HeaderPayload) -> Self {
        let log = shared_log();
        log.lock().unwrap().push_header(header);
        Self::new(log)
    }

    /// 新建日志并启用 ring buffer(头部由宿主在启动后提供)。
    pub fn with_ring(header: HeaderPayload, config: RingConfig) -> Result<Self, yu_log::Diag> {
        let recorder = Self::with_header(header.clone());
        let mut ring = FaultRing::new(config)?;
        ring.set_header(header);
        *lock(&recorder.ring) = Some(ring);
        Ok(recorder)
    }

    /// 启用(或重配)ring buffer。
    ///
    /// 必须在第一 tick 前调用,否则 `start_frame != 1`,dump 会明确标注
    /// 不可回放(不假装能重演)。
    pub fn enable_ring(
        &self,
        config: RingConfig,
        header: Option<HeaderPayload>,
    ) -> Result<(), yu_log::Diag> {
        let mut ring = FaultRing::new(config)?;
        if let Some(header) = header {
            ring.set_header(header);
        }
        *lock(&self.ring) = Some(ring);
        Ok(())
    }

    pub fn has_ring(&self) -> bool {
        lock(&self.ring).is_some()
    }

    /// 更新 ring 的头部(启动后补样本 hash / 表来源)。
    pub fn set_fault_header(&self, header: HeaderPayload) {
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.set_header(header);
        }
    }

    /// 开始一个 tick;上一 tick 自动收口。
    pub fn begin_tick(&self, first_frame: u64, now_ms: Option<u32>) {
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.begin_tick(first_frame, now_ms);
        }
    }

    /// 收口当前 tick,并返回它的摘要(无 tick 时 `None`)。
    pub fn finish_fault_tick(&self) -> Option<TickSummary> {
        let mut guard = lock(&self.ring);
        let ring = guard.as_mut()?;
        ring.finish_tick();
        ring.last_tick().cloned()
    }

    /// 记录显式输入帧(稀疏时间线;dump 回放的数据源)。
    pub fn record_timeline_entry(&self, frame: u64, input: &yu_rt::input::InputFrame) {
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.record_timeline_entry(frame, input);
        }
    }

    /// 记录本 tick 观察到的输入帧(排障视图,不决定回放)。
    pub fn record_tick_input(&self, frame: u64, input: &yu_rt::input::InputFrame) {
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.record_tick_input(frame, input);
        }
    }

    /// 标注本 tick 实际消费到的最后一个逻辑帧。
    pub fn note_tick_frames(&self, last_frame: u64) {
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.note_tick_frames(last_frame);
        }
    }

    /// 写故障快照(JSONL + 伴生时间线文件,均为原子写)。
    pub fn dump_on_fault(
        &self,
        path: impl AsRef<std::path::Path>,
        fault: FaultInfo,
    ) -> Result<FaultDumpPaths, yu_log::Diag> {
        let mut guard = lock(&self.ring);
        let ring = guard.as_mut().ok_or_else(|| {
            yu_log::Diag::fatal(crate::error::code::SCHEMA, yu_log::Boundary::Format)
                .with_detail("dump_on_fault requires an enabled ring buffer")
        })?;
        crate::fault::write_dump(ring, path.as_ref(), fault)
    }

    /// ring 配置快照(诊断用)。
    pub fn ring_config(&self) -> Option<RingConfig> {
        lock(&self.ring).as_ref().map(|ring| ring.config().clone())
    }

    pub fn shared(&self) -> SharedLog {
        Arc::clone(&self.log)
    }

    pub fn shared_log(&self) -> SharedLog {
        Arc::clone(&self.log)
    }

    /// 记录一条事件;ring 模式返回 0(不分配 trace seq)。
    pub fn record(&self, event: &VmEvent) -> u64 {
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.record_event(event);
            return 0;
        }
        let (frame, payload) = map_event(event);
        self.log.lock().unwrap().push(frame, payload)
    }

    /// 记录一条帧描述;校验失败原样返回 `trace.*`(不写半条记录)。
    pub fn record_frame(&self, input: FrameDescriptionInput) -> Result<u64, yu_log::Diag> {
        let summary = crate::record::FrameSummary::from_input(input)?;
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.record_frame(summary);
            return Ok(0);
        }
        Ok(self.log.lock().unwrap().push_frame(summary))
    }

    /// 边界①的记录入口(vfs 包装器使用)。
    ///
    /// ring 模式下成功读取只计数,失败读取保留记录;启动链(帧外)记录
    /// 按 `max_boot_records` 保留。
    pub(crate) fn record_payload(&self, frame: Option<u64>, payload: Payload) {
        if let Some(ring) = lock(&self.ring).as_mut() {
            ring.record_vfs(frame, payload);
            return;
        }
        self.log.lock().unwrap().push(frame, payload);
    }

    /// 按给定顺序记录一批事件;返回新增条数。
    pub fn record_all<'a>(&self, events: impl IntoIterator<Item = &'a VmEvent>) -> usize {
        let mut count = 0;
        for event in events {
            self.record(event);
            count += 1;
        }
        count
    }

    /// 从 `Vm` 拉取尚未消费的事件并记录(边界②的唯一接入点)。
    pub fn drain_vm(&self, vm: &mut yu_rt::Vm) -> usize {
        let events = vm.take_events();
        self.record_all(events.iter())
    }

    /// 当前日志快照(克隆)。
    pub fn snapshot(&self) -> TraceLog {
        self.log.lock().unwrap().clone()
    }
}

/// `VmEvent` → (帧号, 载荷)。穷尽匹配,新增变体必须在此评审。
pub fn map_event(event: &VmEvent) -> (Option<u64>, Payload) {
    match event {
        VmEvent::FrameStart { frame } => (Some(*frame), Payload::VmFrame { start: true }),
        VmEvent::FrameEnd { frame } => (Some(*frame), Payload::VmFrame { start: false }),
        VmEvent::Command {
            frame,
            script,
            pc,
            name,
            args,
        } => (
            Some(*frame),
            Payload::VmCommand {
                script: *script,
                pc: *pc,
                name: name.clone(),
                args: map_args(args),
            },
        ),
        VmEvent::Jump {
            frame,
            script,
            pc,
            to_pc,
            to_script,
            kind,
        } => (
            Some(*frame),
            Payload::VmJump {
                script: *script,
                pc: *pc,
                to_script: *to_script,
                to_pc: *to_pc,
                jump: jump_kind(*kind).to_string(),
            },
        ),
        VmEvent::TaskSpawn {
            frame,
            id,
            script,
            pc,
            enabled,
            main,
        } => (
            Some(*frame),
            Payload::VmTaskSpawn {
                id: id.clone(),
                script: *script,
                pc: *pc,
                enabled: *enabled,
                main: *main,
            },
        ),
        VmEvent::TaskFinish { frame, id } => {
            (Some(*frame), Payload::VmTaskFinish { id: id.clone() })
        }
        VmEvent::Wait {
            frame,
            task,
            frames,
            time_ms,
        } => (
            Some(*frame),
            Payload::VmWait {
                task: task.clone(),
                frames: *frames,
                time_ms: *time_ms,
            },
        ),
        VmEvent::Text { frame, id, bytes } => (
            Some(*frame),
            Payload::VmText {
                id: id.clone(),
                bytes: bytes.clone(),
            },
        ),
        VmEvent::ReturnCode { frame, line } => {
            (Some(*frame), Payload::VmReturnCode { line: *line })
        }
        VmEvent::Fps { frame, fps, skip } => (
            Some(*frame),
            Payload::VmFps {
                fps: *fps,
                skip: *skip,
            },
        ),
        VmEvent::SystemCall { frame, name, args } => (
            Some(*frame),
            Payload::VmCall {
                domain: None,
                name: name.clone(),
                args: map_args(args),
            },
        ),
        VmEvent::DomainCall {
            frame,
            domain,
            name,
            args,
            // 可选定位字段(script/pc)不进入 v4 payload;事件种类仍由本
            // crate 穷尽映射。新增可选字段不改变记录语义。
            ..
        } => (
            Some(*frame),
            Payload::VmCall {
                domain: Some(domain_name(*domain).to_string()),
                name: name.clone(),
                args: map_args(args),
            },
        ),
        VmEvent::End { frame, restart } => (Some(*frame), Payload::VmEnd { restart: *restart }),
        VmEvent::Diag {
            frame,
            code,
            detail,
        } => (
            Some(*frame),
            Payload::VmDiag {
                code: code.clone(),
                detail: detail.clone(),
            },
        ),
    }
}

fn map_args(args: &[yu_rt::event::ArgValue]) -> Vec<TraceArg> {
    args.iter().map(TraceArg::from_arg).collect()
}

/// 参数值 → 可观测形式。浮点保留 `f64` 原值,序列化时用最短往返表示。
pub fn trace_value(value: &yu_rt::Value) -> TraceValue {
    TraceValue::from_value(value)
}

/// 协议里的全部跳转种类字符串(唯一来源;解析器也用这张表校验)。
pub(crate) const JUMP_KINDS: &[&str] = &[
    "go",
    "gosub",
    "return",
    "if_false",
    "else",
    "loop_back",
    "loop_break",
    "loop_continue",
    "if_break",
    "if_continue",
    "task_start",
];

/// 协议里的全部域名(唯一来源;解析器也用这张表校验)。
pub(crate) const DOMAIN_NAMES: &[&str] = &[
    "graphics", "sound", "input", "file", "save", "menu", "window", "movie", "flash", "emote",
    "system", "other",
];

/// `JumpKind` 的协议字符串(本 crate 拥有;新增变体在此编译失败)。
pub fn jump_kind(kind: JumpKind) -> &'static str {
    match kind {
        JumpKind::Go => "go",
        JumpKind::Gosub => "gosub",
        JumpKind::Return => "return",
        JumpKind::IfFalse => "if_false",
        JumpKind::Else => "else",
        JumpKind::LoopBack => "loop_back",
        JumpKind::LoopBreak => "loop_break",
        JumpKind::LoopContinue => "loop_continue",
        JumpKind::IfBreak => "if_break",
        JumpKind::IfContinue => "if_continue",
        JumpKind::TaskStart => "task_start",
    }
}

/// `Domain` 的协议字符串(本 crate 拥有;新增变体在此编译失败)。
pub fn domain_name(domain: Domain) -> &'static str {
    match domain {
        Domain::Graphics => "graphics",
        Domain::Sound => "sound",
        Domain::Input => "input",
        Domain::File => "file",
        Domain::Save => "save",
        Domain::Menu => "menu",
        Domain::Window => "window",
        Domain::Movie => "movie",
        Domain::Flash => "flash",
        Domain::Emote => "emote",
        Domain::System => "system",
        Domain::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yu_rt::Value;

    fn sample_events() -> Vec<VmEvent> {
        vec![
            VmEvent::FrameStart { frame: 1 },
            VmEvent::FrameEnd { frame: 1 },
            VmEvent::Command {
                frame: 1,
                script: 2,
                pc: 3,
                name: "WAIT".to_string(),
                args: vec![yu_rt::event::ArgValue::new(0, Value::Int(1))],
            },
            VmEvent::Jump {
                frame: 1,
                script: 2,
                pc: 3,
                to_pc: 4,
                to_script: 2,
                kind: JumpKind::Go,
            },
            VmEvent::TaskSpawn {
                frame: 1,
                id: "T".to_string(),
                script: 2,
                pc: 3,
                enabled: true,
                main: false,
            },
            VmEvent::TaskFinish {
                frame: 1,
                id: "T".to_string(),
            },
            VmEvent::Wait {
                frame: 1,
                task: "T".to_string(),
                frames: Some(1),
                time_ms: None,
            },
            VmEvent::Text {
                frame: 1,
                id: String::new(),
                bytes: b"abc".to_vec(),
            },
            VmEvent::Fps {
                frame: 1,
                fps: 60,
                skip: false,
            },
            VmEvent::SystemCall {
                frame: 1,
                name: "SYSTEM".to_string(),
                args: vec![],
            },
            VmEvent::DomainCall {
                frame: 1,
                domain: Domain::Graphics,
                name: "CG".to_string(),
                args: vec![],
                script: None,
                pc: None,
            },
            VmEvent::End {
                frame: 1,
                restart: false,
            },
            VmEvent::Diag {
                frame: 1,
                code: "rt.cmd.unknown".to_string(),
                detail: "x".to_string(),
            },
        ]
    }

    #[test]
    fn every_vm_event_variant_records_a_distinct_kind() {
        let recorder = EventRecorder::with_header(HeaderPayload {
            codec: "cp932".to_string(),
            producer: ProducerInfo::kernel(),
            tables: vec![],
            boot: BootSummary::from_config(&yu_rt::BootConfig::default()),
            input: InputSummary {
                events: 0,
                sha256: "0".repeat(64),
            },
            clock: ClockSummary::none(),
            kernel: "test".to_string(),
        });
        let events = sample_events();
        // 13 个变体各一条;FrameStart/FrameEnd 共用载荷形状但 kind 不同。
        assert_eq!(recorder.record_all(events.iter()), events.len());
        let log = recorder.snapshot();
        let kinds: Vec<&str> = log.records()[1..]
            .iter()
            .map(|record| record.kind().as_str())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "frame_start",
                "frame_end",
                "command",
                "jump",
                "task_spawn",
                "task_finish",
                "wait",
                "text",
                "fps",
                "system_call",
                "domain_call",
                "end",
                "diag",
            ]
        );
        let text = log.to_jsonl();
        let parsed = TraceLog::parse(&text).unwrap();
        assert_eq!(text, parsed.to_jsonl());
    }

    #[test]
    fn jump_and_domain_tables_cover_every_variant_exactly_once() {
        let jumps = [
            JumpKind::Go,
            JumpKind::Gosub,
            JumpKind::Return,
            JumpKind::IfFalse,
            JumpKind::Else,
            JumpKind::LoopBack,
            JumpKind::LoopBreak,
            JumpKind::LoopContinue,
            JumpKind::IfBreak,
            JumpKind::IfContinue,
            JumpKind::TaskStart,
        ];
        assert_eq!(jumps.len(), JUMP_KINDS.len());
        for kind in jumps {
            assert!(
                JUMP_KINDS.contains(&jump_kind(kind)),
                "jump_kind({kind:?}) = {:?} is not in JUMP_KINDS",
                jump_kind(kind)
            );
        }
        let domains = [
            Domain::Graphics,
            Domain::Sound,
            Domain::Input,
            Domain::File,
            Domain::Save,
            Domain::Menu,
            Domain::Window,
            Domain::Movie,
            Domain::Flash,
            Domain::Emote,
            Domain::System,
            Domain::Other,
        ];
        assert_eq!(domains.len(), DOMAIN_NAMES.len());
        for domain in domains {
            assert!(
                DOMAIN_NAMES.contains(&domain_name(domain)),
                "domain_name({domain:?}) = {:?} is not in DOMAIN_NAMES",
                domain_name(domain)
            );
        }
        // 唯一性:重复项会让解析器接受未定义的拼写。
        let unique: std::collections::BTreeSet<&&str> = JUMP_KINDS.iter().collect();
        assert_eq!(unique.len(), JUMP_KINDS.len());
        let unique: std::collections::BTreeSet<&&str> = DOMAIN_NAMES.iter().collect();
        assert_eq!(unique.len(), DOMAIN_NAMES.len());
    }

    #[test]
    fn args_keep_type_and_bytes() {
        let args = map_args(&[
            yu_rt::event::ArgValue::new(0, Value::Int(i64::MIN)),
            yu_rt::event::ArgValue::new(1, Value::Float(0.1)),
            yu_rt::event::ArgValue::new(2, Value::Str(vec![0x83, 0x5c, 0xff])),
        ]);
        assert_eq!(args[0].value, TraceValue::Int(i64::MIN));
        assert_eq!(args[1].value, TraceValue::Float(0.1));
        assert_eq!(args[2].value, TraceValue::Str(vec![0x83, 0x5c, 0xff]));
    }

    #[test]
    fn input_summary_is_stable_and_sensitive() {
        let mut timeline = yu_rt::InputTimeline::new();
        timeline.set(0, yu_rt::input::InputFrame::new(["KEY_A"]));
        let a = input_summary(&timeline, 2);
        let b = input_summary(&timeline, 2);
        assert_eq!(a, b);
        assert_eq!(a.events, 1);

        let mut other = yu_rt::InputTimeline::new();
        other.set(0, yu_rt::input::InputFrame::new(["KEY_B"]));
        assert_ne!(a.sha256, input_summary(&other, 2).sha256);

        // 帧窗口变化也应改变摘要。
        assert_ne!(a.sha256, input_summary(&timeline, 3).sha256);

        // 摘要必须覆盖 VM 消费的最后一帧(1..=N):第 N 帧的设定若不同,
        // 摘要必须不同。
        let mut late = yu_rt::InputTimeline::new();
        late.set(2, yu_rt::input::InputFrame::new(["KEY_Z"]));
        let mut late_other = yu_rt::InputTimeline::new();
        late_other.set(2, yu_rt::input::InputFrame::new(["KEY_Y"]));
        assert_ne!(
            input_summary(&late, 2).sha256,
            input_summary(&late_other, 2).sha256,
            "frame 2 is consumed by the second logical tick"
        );

        // 首帧是 1:显式帧 0 通过 `effective(1)` 参与,不应被丢弃。
        let mut at_zero = yu_rt::InputTimeline::new();
        at_zero.set(0, yu_rt::input::InputFrame::new(["KEY_Q"]));
        assert_ne!(a.sha256, input_summary(&at_zero, 2).sha256);
    }
}
