//! 回放闭环。
//!
//! 两条路径:
//!
//! - **离线** [`record_events`]:把一段 `Vec<VmEvent>` 喂给录制器。用于事件流
//!   的单元级对拍,不启动运行核。
//! - **在线** [`VmTrace`]:用录制时的 [`yu_rt::BootConfig`] 与原数据重跑
//!   `yu_rt::Vm`,逐帧拉取事件并记录。运行核侧只使用公开 API
//!   (`Vm::boot` / `Vm::boot_with_aux` / `Vm::tick_at` / `Vm::take_events`),
//!   不在 `yu-rt` 内加任何埋点。
//!
//! 两者的产物都是 [`TraceLog`];用 [`crate::diff_text`] 与录制流逐行比较。
//!
//! # 已知限制
//!
//! `Vm::tick_at` 一次调用可能跑多个逻辑帧(任务链),而 vfs 读取发生在该调用
//! 内部、事件却在其后可见。因此在线路径把**一次 tick 内的 vfs 读取**标注为
//! 该 tick 的首个逻辑帧;若将来需要逐帧精确归属,vfs 侧应改为按帧注入读取
//! (需 `yu-rt` 提供帧钩子,本 crate 不修改 `yu-rt`)。
//!
//! 录制会话关闭运行核的事件丢弃阈值([`TRACE_EVENT_CAPACITY`]):trace 的职责
//! 是完整记录,队列容量只是丢弃阈值、不预分配,实际内存等于该 tick 产生的事件量。
//! 若单 tick 事件量确实超出进程内存,应缩小录制粒度,而不是静默丢事件。

use yu_log::{Boundary, Diag};
use yu_rt::event::VmEvent;
use yu_rt::input::InputTimeline;
use yu_rt::task::ChainReport;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource};

use crate::error;
use crate::event::{header_from_vm, input_summary, input_summary_range, EventRecorder};
use crate::record::{HeaderPayload, InputSummary, TraceLog};
use crate::source::{BoxedSource, FrameHandle, TracingSource};

/// 录制会话的事件队列不设丢弃阈值。
///
/// 该值只是 [`yu_rt::Vm::with_event_capacity`] 的丢弃阈值,**不会预分配**;
/// 实际内存等于单次 tick 产生的事件量。官方 ERIS 首帧约 130 万命令,默认
/// `1<<20` 会在 trace 拉取前丢掉最旧事件;录制路径必须完整,故取 `usize::MAX`。
pub const TRACE_EVENT_CAPACITY: usize = usize::MAX;

/// `header.codec` → [`Codec`];未知名字取 `trace.schema`(拒绝猜测)。
pub fn codec_from_name(name: &str) -> Result<Codec, Diag> {
    match name {
        "cp932" => Ok(Codec::Cp932),
        "gbk" => Ok(Codec::Gbk),
        other => Err(Diag::fatal(error::code::SCHEMA, Boundary::Format)
            .with_detail(format!("unknown codec `{other}` in trace header"))),
    }
}

/// 校验回放用的输入时间线/帧窗口与录制时一致。
///
/// 只需时间线本身(摘要不可能反推出时间线),摘要用于**拒绝**不一致的回放。
/// 摘要不符 ⇒ `trace.replay`(Fatal)。
pub fn verify_input(
    header: &HeaderPayload,
    timeline: &InputTimeline,
    frames: u64,
) -> Result<(), Diag> {
    verify_input_range(header, timeline, 1, frames)
}

/// 同 [`verify_input`],但显式给出录制时的首个逻辑帧号(非新建 VM 的回放)。
pub fn verify_input_range(
    header: &HeaderPayload,
    timeline: &InputTimeline,
    first: u64,
    frames: u64,
) -> Result<(), Diag> {
    let actual = input_summary_range(timeline, first, frames);
    if actual.events != header.input.events || actual.sha256 != header.input.sha256 {
        return Err(Diag::fatal(error::code::REPLAY, Boundary::Domain).with_detail(
            format!(
                "replay input mismatch: recorded {} events sha256 {}, supplied {} events sha256 {} over frames [{first}, {})",
                header.input.events,
                header.input.sha256,
                actual.events,
                actual.sha256,
                first.saturating_add(frames)
            ),
        ));
    }
    Ok(())
}

/// 离线:把事件序列录制为日志(header + 事件)。
pub fn record_events(header: HeaderPayload, events: &[VmEvent]) -> TraceLog {
    let recorder = EventRecorder::with_header(header);
    recorder.record_all(events.iter());
    recorder.snapshot()
}

/// 在线:包装数据源 + 启动运行核 + 逐帧录制。
pub struct VmTrace {
    vm: Vm,
    recorder: EventRecorder,
    frame: FrameHandle,
    codec: Codec,
    cfg: BootConfig,
    /// 下一次 tick 的首个逻辑帧号(`Scheduler::frame() + 1`)。
    next_frame: u64,
}

impl VmTrace {
    /// 单根启动(数据与编译器表同根)。
    pub fn boot(
        source: Box<dyn DataSource>,
        cfg: BootConfig,
        codec: Codec,
    ) -> Result<Self, Diag> {
        Self::boot_with_aux(source, None, cfg, codec)
    }

    /// 双根启动:编译产物在 `source`,`YSCom.ycd` 可回退到 `aux`。
    pub fn boot_with_aux(
        source: Box<dyn DataSource>,
        aux: Option<Box<dyn DataSource>>,
        cfg: BootConfig,
        codec: Codec,
    ) -> Result<Self, Diag> {
        let recorder = EventRecorder::new(crate::event::shared_log());
        let traced = TracingSource::from_recorder(BoxedSource::new(source), recorder.clone());
        let frame = traced.frame_handle();
        let traced_aux = aux.map(|aux| {
            TracingSource::from_recorder(BoxedSource::new(aux), recorder.clone())
        });

        let vm = match traced_aux {
            Some(traced_aux) => Vm::boot_with_aux(
                Box::new(traced),
                Some(Box::new(traced_aux)),
                cfg.clone(),
            )?,
            None => Vm::boot(Box::new(traced), cfg.clone())?,
        };
        // 录制路径不丢事件:默认 1<<20 的丢弃阈值对 ERIS 首帧(约 130 万命令)
        // 会在 trace 拉取前截断。capacity 只是阈值,不会预分配。
        let vm = vm.with_event_capacity(TRACE_EVENT_CAPACITY);

        let mut session = Self {
            vm,
            recorder,
            frame,
            codec,
            cfg,
            next_frame: 1,
        };
        // 启动链事件(表装载期间的 vfs 读取已在日志里,帧号为 null)。
        session.drain_events();
        session.next_frame = session.vm.scheduler().frame().saturating_add(1);
        Ok(session)
    }

    /// 用录制时的 header 重建会话(回放闭环的入口)。
    ///
    /// 编码决策、启动配置(入口/帧率/时钟/脚本密钥/调试位/宿主注入标量)全部取自
    /// header;`argv` 与宿主路径无法从摘要反推,由调用方提供并由
    /// [`crate::record::BootSummary::to_config`] 校验;输入时间线同样由调用方提供,
    /// 用 [`verify_input`] 校验摘要。
    ///
    /// 数据源必须与录制时同源;表一致性由随后的 diff 判定(表来源写在 header)。
    pub fn from_header(
        header: &HeaderPayload,
        source: Box<dyn DataSource>,
        aux: Option<Box<dyn DataSource>>,
        argv: Vec<Vec<u8>>,
        paths: yu_rt::vm::HostPaths,
    ) -> Result<Self, Diag> {
        let codec = codec_from_name(&header.codec)?;
        let cfg = header.boot.to_config(argv, paths)?;
        Self::boot_with_aux(source, aux, cfg, codec)
    }

    /// 推进一帧(时钟注入为 `now_ms`)。
    ///
    /// vfs 读取的帧标注见模块文档的已知限制。
    pub fn tick_at(&mut self, now_ms: u32, timeline: &InputTimeline) -> ChainReport {
        self.tick_with(|vm| vm.tick_at(now_ms, timeline))
    }

    /// 推进一帧(时钟由 `yu-rt` 的 fps/frame_ms 推进)。
    pub fn tick(&mut self, timeline: &InputTimeline) -> ChainReport {
        self.tick_with(|vm| vm.tick(timeline))
    }

    fn tick_with(&mut self, run: impl FnOnce(&mut Vm) -> ChainReport) -> ChainReport {
        let first = self.vm.scheduler().frame().saturating_add(1);
        self.frame.set(Some(first));
        let report = run(&mut self.vm);
        self.frame.set(None);
        self.next_frame = first.saturating_add(report.logical_frames as u64);
        self.drain_events();
        report
    }

    fn drain_events(&mut self) {
        // 启动事件已在 `Vm::boot` 内产生;必须用 `take_events` 拉走,
        // 否则回放时同一事件会被重复计数。
        self.recorder.drain_vm(&mut self.vm);
    }

    /// 已完成的逻辑帧数(下一次 tick 的首帧号 - 1)。
    pub fn frames(&self) -> u64 {
        self.next_frame.saturating_sub(1)
    }

    pub fn vm(&self) -> &Vm {
        &self.vm
    }

    pub fn vm_mut(&mut self) -> &mut Vm {
        &mut self.vm
    }

    pub fn recorder(&self) -> &EventRecorder {
        &self.recorder
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn config(&self) -> &BootConfig {
        &self.cfg
    }

    /// 结束录制:补写 header(`seq = 0`)并返回日志。
    ///
    /// 头部必须最后补:表装载期的 vfs 记录先入流,而 `table_provenance`
    /// 要等 `Vm::boot` 成功后才存在。
    pub fn finish(self, input: InputSummary) -> TraceLog {
        let header = header_from_vm(&self.vm, &self.cfg, self.codec, input);
        let log = self.recorder.shared_log();
        let mut log = log.borrow_mut();
        log.prepend_header(header);
        log.clone()
    }

    /// 用本次会话实际推进的帧数,从输入时间线生成摘要后收尾。
    pub fn finish_with_timeline(self, timeline: &InputTimeline) -> TraceLog {
        let frames = self.frames();
        self.finish(input_summary(timeline, frames))
    }

    /// 需要继续使用 `Vm` 时取回它(不补 header,调用方可自行
    /// [`TraceLog::prepend_header`])。
    pub fn into_parts(self) -> (Vm, TraceLog) {
        let log = self.recorder.snapshot();
        (self.vm, log)
    }
}
