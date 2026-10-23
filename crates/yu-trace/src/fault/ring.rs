//! fault ring:配置校验、tick 摘要/事件哈希、`FaultRing` 状态机。
//!
//! 从 `fault.rs` 纯搬运(2026-10-23);行为不变。

use super::*;

struct TickBuilder {
    index: u64,
    first_frame: u64,
    now_ms: Option<u32>,
    last_frame: u64,
    command_count: u64,
    command: Sha256,
    events: Sha256,
    event_counts: BTreeMap<Kind, u64>,
    entries: Vec<(u64, Payload)>,
    diags: Vec<DiagEntry>,
    diags_dropped: u64,
    inputs: Vec<InputEntry>,
    inputs_dropped: u64,
    displays_dropped: u64,
    bytes: usize,
}

impl TickBuilder {
    fn new(index: u64, first_frame: u64, now_ms: Option<u32>, last_frame: u64) -> Self {
        Self {
            index,
            first_frame,
            now_ms,
            last_frame,
            command_count: 0,
            command: Sha256::new(),
            events: Sha256::new(),
            event_counts: BTreeMap::new(),
            entries: Vec::new(),
            diags: Vec::new(),
            diags_dropped: 0,
            inputs: Vec::new(),
            inputs_dropped: 0,
            displays_dropped: 0,
            // 固定开销:两个摘要 + 计数 + 帧号等(不含记录本体)。
            bytes: 256,
        }
    }

    fn feed_event(&mut self, event: &VmEvent) {
        let frame = event.frame();
        self.last_frame = self.last_frame.max(frame);
        let kind = match event {
            VmEvent::FrameStart { .. } => Kind::VmFrameStart,
            VmEvent::FrameEnd { .. } => Kind::VmFrameEnd,
            VmEvent::Command { .. } => Kind::VmCommand,
            VmEvent::Jump { .. } => Kind::VmJump,
            VmEvent::TaskSpawn { .. } => Kind::VmTaskSpawn,
            VmEvent::TaskFinish { .. } => Kind::VmTaskFinish,
            VmEvent::Wait { .. } => Kind::VmWait,
            VmEvent::Text { .. } => Kind::VmText,
            VmEvent::ReturnCode { .. } => Kind::VmReturnCode,
            VmEvent::Fps { .. } => Kind::VmFps,
            VmEvent::SystemCall { .. } => Kind::VmSystemCall,
            VmEvent::DomainCall { .. } => Kind::VmDomainCall,
            VmEvent::End { .. } => Kind::VmEnd,
            VmEvent::Diag { .. } => Kind::VmDiag,
        };
        *self.event_counts.entry(kind).or_insert(0) += 1;
        feed_event(&mut self.events, event);
        if let VmEvent::Command {
            frame,
            script,
            pc,
            name,
            args,
        } = event
        {
            self.command_count += 1;
            put_u64(&mut self.command, *frame);
            put_u32(&mut self.command, *script);
            put_u32(&mut self.command, *pc);
            put_bytes(&mut self.command, name.as_bytes());
            put_u64(&mut self.command, args.len() as u64);
            for arg in args {
                self.command.update(&[arg.slot]);
                feed_value(&mut self.command, &arg.value);
            }
        }
    }

    fn into_summary(mut self, max_tick_bytes: usize) -> TickSummary {
        // 超预算:先丢 display(保留摘要),再截断 diag detail,最后标记 over_budget。
        let mut stripped = 0u64;
        while self.bytes > max_tick_bytes {
            let Some(index) = self.entries.iter().position(|(_, payload)| {
                matches!(payload, Payload::FrameDescription(summary) if summary.display.is_some())
            }) else {
                break;
            };
            let (_, payload) = &mut self.entries[index];
            if let Payload::FrameDescription(summary) = payload {
                if let Some(display) = summary.display.take() {
                    self.bytes = self.bytes.saturating_sub(display.canonical().len() + 64);
                    stripped += 1;
                }
            }
        }
        self.displays_dropped += stripped;
        if self.bytes > max_tick_bytes {
            for (_, payload) in &mut self.entries {
                if let Payload::VmDiag { detail, .. } = payload {
                    if detail.len() > 512 {
                        self.bytes = self.bytes.saturating_sub(detail.len() - 512);
                        detail.truncate(512);
                    }
                }
            }
        }
        let mut frames_desc = Vec::new();
        for (frame, payload) in &self.entries {
            match payload {
                Payload::FrameDescription(summary) => frames_desc.push(FrameTickEntry {
                    frame: *frame,
                    display_sha256: summary.display_sha256.clone(),
                    display_retained: summary.display.is_some(),
                    pass_count: summary.pass_count,
                    op_count: summary.op_count,
                    executable_ops: summary.executable_ops,
                    skipped_ops: summary.skipped_ops,
                }),
                Payload::VmDiag { .. } => {}
                _ => {}
            }
        }
        // 上限丢弃的 Diag 只体现在 `diags_dropped`;不伪造记录。
        let over_budget = self.bytes > max_tick_bytes;
        TickSummary {
            index: self.index,
            first_frame: self.first_frame,
            frames: self
                .last_frame
                .saturating_add(1)
                .saturating_sub(self.first_frame),
            last_frame: self.last_frame,
            now_ms: self.now_ms,
            command_count: self.command_count,
            command_digest: self.command.finish(),
            event_digest: self.events.finish(),
            event_counts: self
                .event_counts
                .into_iter()
                .map(|(kind, count)| (kind.as_str().to_string(), count))
                .collect(),
            diags: self.diags,
            diags_dropped: self.diags_dropped,
            inputs: self.inputs,
            inputs_dropped: self.inputs_dropped,
            frames_desc,
            displays_dropped: self.displays_dropped,
            bytes: self.bytes,
            over_budget,
            retained: true,
            entries: self.entries,
        }
    }
}

fn put_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(&value.to_le_bytes());
}

fn put_u32(hasher: &mut Sha256, value: u32) {
    hasher.update(&value.to_le_bytes());
}

fn put_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    put_u64(hasher, bytes.len() as u64);
    hasher.update(bytes);
}

fn feed_value(hasher: &mut Sha256, value: &yu_rt::Value) {
    match value {
        yu_rt::Value::Int(v) => {
            hasher.update(b"i");
            hasher.update(&v.to_le_bytes());
        }
        yu_rt::Value::Float(v) => {
            hasher.update(b"f");
            hasher.update(&v.to_bits().to_le_bytes());
        }
        yu_rt::Value::Str(bytes) => {
            hasher.update(b"s");
            put_bytes(hasher, bytes);
        }
    }
}

/// 全事件摘要(穷尽 match;新增 `VmEvent` 变体必须在这里评审)。
fn feed_event(hasher: &mut Sha256, event: &VmEvent) {
    match event {
        VmEvent::FrameStart { frame } => {
            hasher.update(b"frs");
            put_u64(hasher, *frame);
        }
        VmEvent::FrameEnd { frame } => {
            hasher.update(b"fre");
            put_u64(hasher, *frame);
        }
        VmEvent::Command {
            frame,
            script,
            pc,
            name,
            args,
        } => {
            hasher.update(b"cmd");
            put_u64(hasher, *frame);
            put_u32(hasher, *script);
            put_u32(hasher, *pc);
            put_bytes(hasher, name.as_bytes());
            put_u64(hasher, args.len() as u64);
            for arg in args {
                hasher.update(&[arg.slot]);
                feed_value(hasher, &arg.value);
            }
        }
        VmEvent::Jump {
            frame,
            script,
            pc,
            to_pc,
            to_script,
            kind,
        } => {
            hasher.update(b"jmp");
            put_u64(hasher, *frame);
            put_u32(hasher, *script);
            put_u32(hasher, *pc);
            put_u32(hasher, *to_script);
            put_u32(hasher, *to_pc);
            put_bytes(hasher, jump_kind(*kind).as_bytes());
        }
        VmEvent::TaskSpawn {
            frame,
            id,
            script,
            pc,
            enabled,
            main,
        } => {
            hasher.update(b"tsp");
            put_u64(hasher, *frame);
            put_bytes(hasher, id.as_bytes());
            put_u32(hasher, *script);
            put_u32(hasher, *pc);
            hasher.update(&[u8::from(*enabled), u8::from(*main)]);
        }
        VmEvent::TaskFinish { frame, id } => {
            hasher.update(b"tfn");
            put_u64(hasher, *frame);
            put_bytes(hasher, id.as_bytes());
        }
        VmEvent::Wait {
            frame,
            task,
            frames,
            time_ms,
        } => {
            hasher.update(b"wat");
            put_u64(hasher, *frame);
            put_bytes(hasher, task.as_bytes());
            match frames {
                Some(value) => {
                    hasher.update(&[1]);
                    put_u32(hasher, *value);
                }
                None => hasher.update(&[0]),
            }
            match time_ms {
                Some(value) => {
                    hasher.update(&[1]);
                    put_u32(hasher, *value);
                }
                None => hasher.update(&[0]),
            }
        }
        VmEvent::Text { frame, id, bytes } => {
            hasher.update(b"txt");
            put_u64(hasher, *frame);
            put_bytes(hasher, id.as_bytes());
            put_bytes(hasher, bytes);
        }
        VmEvent::ReturnCode { frame, line } => {
            hasher.update(b"rc");
            put_u64(hasher, *frame);
            put_u32(hasher, *line);
        }
        VmEvent::Fps { frame, fps, skip } => {
            hasher.update(b"fps");
            put_u64(hasher, *frame);
            put_u32(hasher, *fps);
            hasher.update(&[u8::from(*skip)]);
        }
        VmEvent::SystemCall { frame, name, args } => {
            hasher.update(b"sys");
            put_u64(hasher, *frame);
            put_bytes(hasher, name.as_bytes());
            feed_args(hasher, args);
        }
        VmEvent::DomainCall {
            frame,
            domain,
            name,
            args,
            ..
        } => {
            hasher.update(b"dom");
            put_u64(hasher, *frame);
            put_bytes(hasher, domain_name(*domain).as_bytes());
            put_bytes(hasher, name.as_bytes());
            feed_args(hasher, args);
        }
        VmEvent::End { frame, restart } => {
            hasher.update(b"end");
            put_u64(hasher, *frame);
            hasher.update(&[u8::from(*restart)]);
        }
        VmEvent::Diag {
            frame,
            code,
            detail,
        } => {
            hasher.update(b"dia");
            put_u64(hasher, *frame);
            put_bytes(hasher, code.as_bytes());
            put_bytes(hasher, detail.as_bytes());
        }
    }
}

fn feed_args(hasher: &mut Sha256, args: &[yu_rt::event::ArgValue]) {
    put_u64(hasher, args.len() as u64);
    for arg in args {
        hasher.update(&[arg.slot]);
        feed_value(hasher, &arg.value);
    }
}

/// ring buffer 本体。只在 `EventRecorder` 的互斥锁内使用。
pub(crate) struct FaultRing {
    config: RingConfig,
    header: Option<HeaderPayload>,
    boot: Vec<(Option<u64>, Payload)>,
    boot_dropped: u64,
    ticks: VecDeque<TickSummary>,
    current: Option<TickBuilder>,
    next_tick: u64,
    total_bytes: usize,
    evicted_ticks: u64,
    timeline: BTreeMap<u64, InputFrame>,
    clock: Vec<Option<u32>>,
    start_frame: u64,
    last_frame: u64,
    orphan_events: u64,
    vfs_records: u64,
    vfs_dropped: u64,
    timeline_truncated: bool,
    clock_truncated: bool,
}

impl FaultRing {
    pub(crate) fn new(config: RingConfig) -> Result<Self, Diag> {
        config.validate()?;
        Ok(Self {
            config,
            header: None,
            boot: Vec::new(),
            boot_dropped: 0,
            ticks: VecDeque::new(),
            current: None,
            next_tick: 0,
            total_bytes: 0,
            evicted_ticks: 0,
            timeline: BTreeMap::new(),
            clock: Vec::new(),
            start_frame: 0,
            last_frame: 0,
            orphan_events: 0,
            vfs_records: 0,
            vfs_dropped: 0,
            timeline_truncated: false,
            clock_truncated: false,
        })
    }

    pub(crate) fn config(&self) -> &RingConfig {
        &self.config
    }

    pub(crate) fn set_header(&mut self, header: HeaderPayload) {
        self.header = Some(header);
    }

    /// 开始一个 tick。当前 tick 若还开着,先收口。
    pub(crate) fn begin_tick(&mut self, first_frame: u64, now_ms: Option<u32>) {
        self.finish_tick();
        if self.start_frame == 0 {
            self.start_frame = first_frame;
        }
        if self.clock.len() < self.config.max_clock_ticks {
            self.clock.push(now_ms);
        } else {
            self.clock_truncated = true;
        }
        self.current = Some(TickBuilder::new(
            self.next_tick,
            first_frame,
            now_ms,
            first_frame.saturating_sub(1),
        ));
        self.next_tick += 1;
    }

    /// 收口当前 tick(下一 tick 开始前 / dump 时 / replay 逐 tick 比较时)。
    pub(crate) fn finish_tick(&mut self) -> Option<TickSummary> {
        let builder = self.current.take()?;
        let summary = builder.into_summary(self.config.max_tick_bytes);
        self.last_frame = self.last_frame.max(summary.last_frame);
        self.push_tick(summary);
        self.ticks.back().cloned()
    }

    fn push_tick(&mut self, summary: TickSummary) {
        let summary = if summary.bytes > self.config.max_total_bytes {
            // 单个 tick 就超过总预算:保留占位摘要,明确标注(不静默丢帧)。
            let placeholder = TickSummary::placeholder(
                summary.index,
                summary.first_frame,
                summary.frames,
                summary.bytes,
                summary.now_ms,
            );
            self.evicted_ticks += 1;
            placeholder
        } else {
            summary
        };
        self.total_bytes += summary.bytes;
        self.ticks.push_back(summary);
        while self.ticks.len() > self.config.ticks
            || (self.total_bytes > self.config.max_total_bytes && self.ticks.len() > 1)
        {
            if let Some(removed) = self.ticks.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(removed.bytes);
                self.evicted_ticks += 1;
            } else {
                break;
            }
        }
    }

    pub(crate) fn record_event(&mut self, event: &VmEvent) {
        let frame = event.frame();
        self.last_frame = self.last_frame.max(frame);
        let Some(builder) = self.current.as_mut() else {
            self.orphan_events += 1;
            return;
        };
        builder.feed_event(event);
        if let VmEvent::Diag {
            frame,
            code,
            detail,
        } = event
        {
            if builder.diags.len() < self.config.max_diags_per_tick {
                builder.diags.push(DiagEntry {
                    frame: *frame,
                    code: code.clone(),
                    detail: detail.clone(),
                    unimplemented: code.contains("unimplemented"),
                });
                let size = code.len() + detail.len() + 32;
                builder.bytes += size;
                builder.entries.push((
                    *frame,
                    Payload::VmDiag {
                        code: code.clone(),
                        detail: detail.clone(),
                    },
                ));
            } else {
                builder.diags_dropped += 1;
            }
        }
    }

    /// 帧内 vfs 记录:成功只计数,失败保留记录(排障最需要"缺哪个文件")。
    pub(crate) fn record_vfs(&mut self, frame: Option<u64>, payload: Payload) {
        let failed = match &payload {
            Payload::VfsOpen { diag, .. } => diag.is_some(),
            Payload::VfsList { diag, .. } => diag.is_some(),
            _ => false,
        };
        self.vfs_records += 1;
        match frame {
            None => {
                if self.boot.len() < self.config.max_boot_records {
                    self.boot.push((None, payload));
                } else {
                    self.boot_dropped += 1;
                }
            }
            Some(frame) => {
                if !failed {
                    return;
                }
                let Some(builder) = self.current.as_mut() else {
                    self.orphan_events += 1;
                    return;
                };
                let size = json::to_text(&payload_to_json(&payload)).len();
                builder.bytes += size;
                builder.entries.push((frame, payload));
            }
        }
    }

    /// 显式输入帧(稀疏时间线)。用于 dump 后离线重演。
    pub(crate) fn record_timeline_entry(&mut self, frame: u64, input: &InputFrame) {
        if self.timeline.len() >= self.config.max_timeline_entries
            && !self.timeline.contains_key(&frame)
        {
            self.timeline_truncated = true;
            return;
        }
        self.timeline.insert(frame, input.clone());
    }

    /// 单 tick 的输入摘要(供伴生文件检查;不参与重演)。
    pub(crate) fn record_tick_input(&mut self, frame: u64, input: &InputFrame) {
        let Some(builder) = self.current.as_mut() else {
            return;
        };
        if builder.inputs.len() >= self.config.max_inputs_per_tick {
            builder.inputs_dropped += 1;
            return;
        }
        if input.pressed.is_empty() && input.pulses.is_empty() && input.cursor == (0, 0) {
            return; // 空帧不占预算
        }
        let entry = InputEntry::from_input(frame, input);
        builder.bytes += entry.pressed.iter().map(String::len).sum::<usize>()
            + entry.pulses.iter().map(String::len).sum::<usize>()
            + 48;
        builder.inputs.push(entry);
    }

    pub(crate) fn record_frame(&mut self, mut summary: FrameSummary) {
        let display_len = summary
            .display
            .as_ref()
            .map(|display| display.canonical().len())
            .unwrap_or(0);
        let mut dropped = false;
        if display_len > self.config.max_display_bytes {
            summary.display = None;
            dropped = true;
        }
        let frame = summary.frame;
        let size = summary.display_sha256.len() + display_len + 96;
        let Some(builder) = self.current.as_mut() else {
            // 还没有 tick(首帧描述早于第一次 tick):挂到启动记录,不丢。
            if self.boot.len() < self.config.max_boot_records {
                self.boot
                    .push((Some(frame), Payload::FrameDescription(summary)));
            } else {
                self.boot_dropped += 1;
            }
            return;
        };
        if dropped {
            builder.displays_dropped += 1;
        }
        builder.bytes += size;
        builder
            .entries
            .push((frame, Payload::FrameDescription(summary)));
    }

    pub(crate) fn note_tick_frames(&mut self, last_frame: u64) {
        self.last_frame = self.last_frame.max(last_frame);
        if let Some(builder) = self.current.as_mut() {
            builder.last_frame = builder.last_frame.max(last_frame);
        }
    }

    pub(crate) fn last_tick(&self) -> Option<&TickSummary> {
        self.ticks.back()
    }

    pub(crate) fn current_tick_index(&self) -> Option<u64> {
        self.current.as_ref().map(|builder| builder.index)
    }

    pub(crate) fn manifest(
        &self,
        fault: FaultInfo,
        trace_sha256: String,
        header: &HeaderPayload,
    ) -> FaultManifest {
        let timeline = self.timeline();
        let input = InputSummary {
            events: timeline.len() as u64,
            sha256: crate::event::input_summary_range(&timeline, 1, self.last_frame).sha256,
        };
        FaultManifest {
            trace_sha256,
            producer: header.producer.clone(),
            codec: header.codec.clone(),
            fault,
            fault_tick: self
                .current_tick_index()
                .or_else(|| self.ticks.back().map(|tick| tick.index)),
            fault_frame: self.last_frame,
            ring: self.config.clone(),
            input: FaultInput {
                events: input.events,
                sha256: input.sha256,
                entries: self
                    .timeline
                    .iter()
                    .map(|(frame, input)| InputEntry::from_input(*frame, input))
                    .collect(),
                truncated: self.timeline_truncated,
            },
            clock: ClockSummary::from_ticks(&self.clock),
            start_frame: self.start_frame,
            last_frame: self.last_frame,
            orphan_events: self.orphan_events,
            vfs_records: self.vfs_records,
            vfs_dropped: self.vfs_dropped,
            boot_records_dropped: self.boot_dropped,
            timeline_truncated: self.timeline_truncated,
            clock_truncated: self.clock_truncated,
            evicted_ticks: self.evicted_ticks,
            ticks: self.ticks.iter().cloned().collect(),
        }
    }

    /// 从显式帧集合重建 `InputTimeline`。
    pub(crate) fn timeline(&self) -> InputTimeline {
        let mut timeline = InputTimeline::new();
        for (frame, input) in &self.timeline {
            timeline.set(*frame, input.clone());
        }
        timeline
    }

    /// 构造 dump 的 trace(header + 启动记录 + 保留 tick 的记录)。
    ///
    /// `header` 的 `input`/`clock` 必须已按故障帧重算(调用方负责)。
    pub(crate) fn build_log(&self, header: HeaderPayload) -> TraceLog {
        let mut log = TraceLog::new();
        log.push_header(header);
        for (frame, payload) in &self.boot {
            log.push(*frame, payload.clone());
        }
        for tick in &self.ticks {
            for (frame, payload) in &tick.entries {
                log.push(Some(*frame), payload.clone());
            }
        }
        log
    }

    pub(crate) fn dump_header(&self, fault: &FaultInfo) -> Result<HeaderPayload, Diag> {
        let Some(base) = &self.header else {
            return Err(
                Diag::fatal(error::code::SCHEMA, Boundary::Format).with_detail(
                    "fault dump requires a header: call EventRecorder::set_header / \
                 EventRecorder::with_ring before recording",
                ),
            );
        };
        let mut header = base.clone();
        let timeline = self.timeline();
        header.input = crate::event::input_summary_range(&timeline, 1, self.last_frame);
        header.clock = ClockSummary::from_ticks(&self.clock);
        // fault detail 只进伴生文件;header 保持 producer/table/boot 原样。
        let _ = fault;
        Ok(header)
    }
}
