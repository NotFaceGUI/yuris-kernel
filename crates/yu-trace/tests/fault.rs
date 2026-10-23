//! P5 验收:故障快照(ring buffer / dump / 离线重演)与帧描述投影。
//!
//! 覆盖:
//! 1. ring 捕获命令摘要、Diag(含 Unimplemented 挂号)、输入、frame_description;
//! 2. `dump_on_fault` 原子写出 JSONL + 伴生时间线,读回校验 trace 摘要;
//! 3. 离线 replay 逐 tick 复现到故障帧,零差异;篡改摘要立即 `trace.replay`;
//! 4. 容量淘汰与字节预算:丢弃必须有计数,不静默;
//! 5. 屏幕级帧描述的显式投影(占位计数不参与,像素参与)。

mod support;

use std::path::PathBuf;

use support::{ex, fresh_dir, header_for, GroupSpec, Init, ProjectBuilder, VarSpec};
use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::BootConfig;
use yu_trace::{
    diff_aligned_logs, sha256_hex, AlignedDiffOptions, EventRecorder, FaultDump, FaultInfo,
    FrameDescriptionInput, FrameFieldPolicy, FrameLevel, FrameSummary, HeaderCompare, Kind,
    Projection, Sub, TraceLog, VmTrace,
};
use yu_vfs::Codec;

const OP_END: u8 = 14;
const OP_FPS: u8 = 29;
const OP_INT: u8 = 51;
const OP_LET: u8 = 54;
const OP_TEXT: u8 = 96;
const OP_WAIT: u8 = 102;

fn project() -> ProjectBuilder {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_INT)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(7)),
        GroupSpec::new(OP_LET).window(0, ex::cat(&[ex::get(0x40, sys), ex::i8_(2), ex::op(0x2a)])),
        GroupSpec::new(OP_FPS).window(0, ex::i8_(30)),
        GroupSpec::new(OP_TEXT).window(0, ex::s("hello")),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    b
}

fn cfg() -> BootConfig {
    BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        start_ms: 0,
        fps: 60,
        ..BootConfig::default()
    }
}

fn timeline() -> InputTimeline {
    let mut timeline = InputTimeline::new();
    timeline.set(0, InputFrame::new(["KEY_A"]).with_cursor(11, 22));
    timeline.set(2, InputFrame::new(["MOUSE_L"]).with_pulse("WHEEL_UP"));
    timeline
}

fn boot_ring(config: yu_trace::RingConfig) -> VmTrace {
    let mut trace = VmTrace::boot(Box::new(project().build()), cfg(), Codec::Cp932)
        .expect("synthetic project boots");
    trace
        .enable_fault_ring(config, None)
        .expect("enable fault ring");
    trace
}

fn frame_input(frame: u64, display: &[u8]) -> FrameDescriptionInput {
    FrameDescriptionInput {
        frame,
        level: FrameLevel::Event,
        pass_count: 1,
        op_count: 4,
        executable_ops: 3,
        skipped_ops: 1,
        display: Some(display.to_vec()),
        display_sha256: None,
    }
}

fn only_temp_files(dir: &std::path::Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("read dump dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
        })
        .collect()
}

#[test]
fn ring_dump_captures_diags_input_and_frame_description_and_replays() {
    let dir = fresh_dir("fault-roundtrip");
    let mut trace = boot_ring(yu_trace::RingConfig::default());
    let timeline = timeline();
    trace.tick_at(0, &timeline);
    trace.tick_at(16, &timeline);

    let recorder = trace.recorder().clone();
    // 第二 tick 仍开着:故障现场的帧描述挂到它上面。
    recorder
        .record_frame(frame_input(2, br#"{"layers":2,"ops":[{"op":"TEXT"}]}"#))
        .expect("frame description accepted");

    let paths = recorder
        .dump_on_fault(
            dir.join("fault.jsonl"),
            FaultInfo::panic("synthetic panic: P5 fault dump demo"),
        )
        .expect("dump");
    assert!(paths.trace.is_file(), "trace file exists");
    assert!(paths.manifest.is_file(), "timeline sidecar exists");
    assert_eq!(
        only_temp_files(&dir),
        Vec::<PathBuf>::new(),
        "atomic write must not leave temp files"
    );

    let dump = FaultDump::read(&paths.trace).expect("dump reads back");
    assert!(dump.manifest.replayable(), "dump from boot is replayable");
    assert_eq!(dump.manifest.fault_tick, Some(1));
    assert_eq!(dump.manifest.ticks.len(), 2);
    assert_eq!(dump.manifest.clock.mode, yu_trace::ClockMode::TickAt);
    assert_eq!(dump.manifest.clock.now_ms, vec![Some(0), Some(16)]);
    // 故障前显式输入帧都在,含 frame 0(影响 effective(1))。
    let frames: Vec<u64> = dump
        .manifest
        .input
        .entries
        .iter()
        .map(|entry| entry.frame)
        .collect();
    assert_eq!(frames, vec![0, 2]);

    // trace 里能看到 frame_description(真实现场,不是摘要)。
    let text = std::fs::read_to_string(&paths.trace).unwrap();
    assert!(text.contains("\"kind\":\"frame_description\""), "{text}");
    let tick = &dump.manifest.ticks[1];
    assert_eq!(tick.frames_desc.len(), 1);
    assert!(tick.frames_desc[0].display_retained);
    assert!(
        dump.manifest
            .ticks
            .iter()
            .any(|tick| tick.command_count > 0),
        "at least one tick ran commands"
    );
    assert!(!tick.command_digest.is_empty());

    // 离线重演:同一工程 + 同一时间线,逐 tick 摘要必须零差异。
    let report = dump
        .replay(
            Box::new(project().build()),
            None,
            Vec::new(),
            yu_rt::vm::HostPaths::default(),
        )
        .expect("replay reaches the fault frame");
    assert_eq!(report.ticks_replayed, 2);
    assert_eq!(report.fault_tick, Some(1));
    assert!(report.frames_replayed > 0);
    assert!(report.to_string().contains("fault replay OK"));
}

#[test]
fn catch_unwind_fault_is_dumpable_and_replayable() {
    // H-P5-01 的 catch_unwind 路径:宿主捕获 panic 后调用 dump_on_fault,
    // 故障 tick 的现场仍完整(不需要全局 panic hook)。
    let dir = fresh_dir("fault-catch-unwind");
    let mut trace = boot_ring(yu_trace::RingConfig::default());
    let timeline = timeline();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for tick in 0..3u64 {
            trace.tick_at((tick as u32) * 16, &timeline);
        }
        panic!("P5 synthetic catch_unwind fault");
    }));
    assert!(outcome.is_err(), "synthetic panic must be caught");
    let recorder = trace.recorder().clone();
    let paths = recorder
        .dump_on_fault(
            dir.join("fault.jsonl"),
            FaultInfo::caught("P5 synthetic catch_unwind fault"),
        )
        .expect("dump after catch_unwind");
    let dump = FaultDump::read(&paths.trace).expect("dump reads back");
    assert!(dump.manifest.replayable());
    assert_eq!(dump.manifest.fault.kind, yu_trace::FaultKind::CatchUnwind);
    let report = dump
        .replay(
            Box::new(project().build()),
            None,
            Vec::new(),
            yu_rt::vm::HostPaths::default(),
        )
        .expect("replay after catch_unwind reaches the fault frame");
    assert_eq!(report.ticks_replayed, 3);
}

#[test]
fn ring_captures_unimplemented_diag_as_evidence() {
    let dir = fresh_dir("fault-diag");
    let recorder =
        EventRecorder::with_ring(header_for(Vec::new()), yu_trace::RingConfig::default())
            .expect("ring");
    recorder.begin_tick(1, Some(0));
    recorder.record(&VmEvent::Diag {
        frame: 1,
        code: "rt.cmd.unimplemented".to_string(),
        detail: "TEST command is registered but has no handler".to_string(),
    });
    let tick = recorder.finish_fault_tick().expect("tick");
    assert_eq!(tick.diags.len(), 1);
    assert!(tick.diags[0].unimplemented);
    assert_eq!(tick.diags[0].frame, 1);
    let paths = recorder
        .dump_on_fault(dir.join("fault.jsonl"), FaultInfo::panic("unimplemented"))
        .expect("dump");
    let dump = FaultDump::read(&paths.trace).expect("read");
    assert_eq!(dump.manifest.ticks[0].diags.len(), 1);
    let text = std::fs::read_to_string(&paths.trace).unwrap();
    assert!(text.contains("\"kind\":\"diag\""), "{text}");
    assert!(text.contains("rt.cmd.unimplemented"), "{text}");
}

#[test]
fn replay_refuses_tampered_tick_summary() {
    let dir = fresh_dir("fault-tamper");
    let mut trace = boot_ring(yu_trace::RingConfig::default());
    let timeline = timeline();
    trace.tick_at(0, &timeline);
    trace.tick_at(16, &timeline);
    let recorder = trace.recorder().clone();
    let paths = recorder
        .dump_on_fault(dir.join("fault.jsonl"), FaultInfo::panic("boom"))
        .expect("dump");
    let mut dump = FaultDump::read(&paths.trace).expect("dump reads back");

    // 篡改一个 tick 的命令摘要:回放必须 fail-fast,不能"差不多就算过"。
    dump.manifest.ticks[0].command_digest = sha256_hex(b"not the real command stream");
    let err = dump
        .replay(
            Box::new(project().build()),
            None,
            Vec::new(),
            yu_rt::vm::HostPaths::default(),
        )
        .expect_err("tampered digest must be rejected");
    assert_eq!(err.code.as_ref(), yu_trace::code::REPLAY);
    assert!(err.detail.contains("/command_digest"), "{}", err.detail);

    // 篡改 trace 文本本身:读回时 trace_sha256 不符。
    let text = std::fs::read_to_string(&paths.trace).unwrap();
    let mutated = text.replacen("\"frame\":1", "\"frame\":2", 1);
    assert_ne!(mutated, text, "tamper must change the trace text");
    std::fs::write(&paths.trace, mutated).unwrap();
    let err = FaultDump::read(&paths.trace).expect_err("tampered trace rejected");
    assert_eq!(err.code.as_ref(), yu_trace::code::SCHEMA);
}

#[test]
fn ring_evicts_old_ticks_with_explicit_counter() {
    let dir = fresh_dir("fault-evict");
    let recorder = EventRecorder::with_ring(
        header_for(Vec::new()),
        yu_trace::RingConfig {
            ticks: 2,
            ..yu_trace::RingConfig::default()
        },
    )
    .expect("ring");
    for tick in 0..5u64 {
        recorder.begin_tick(tick * 2 + 1, Some((tick * 16) as u32));
        recorder.record(&VmEvent::Command {
            frame: tick * 2 + 2,
            script: 1,
            pc: tick as u32,
            name: "WAIT".to_string(),
            args: Vec::new(),
        });
        recorder.finish_fault_tick();
    }
    let paths = recorder
        .dump_on_fault(dir.join("fault.jsonl"), FaultInfo::panic("evict"))
        .expect("dump");
    let dump = FaultDump::read(&paths.trace).expect("read");
    assert_eq!(dump.manifest.ticks.len(), 2);
    assert_eq!(dump.manifest.ticks[0].index, 3);
    assert_eq!(dump.manifest.ticks[1].index, 4);
    assert_eq!(dump.manifest.evicted_ticks, 3);
    assert!(
        !dump.manifest.replayable(),
        "evicted ticks cannot be replayed from boot"
    );
    let err = dump
        .replay(
            Box::new(project().build()),
            None,
            Vec::new(),
            yu_rt::vm::HostPaths::default(),
        )
        .expect_err("non-replayable dump is explicit");
    assert_eq!(err.code.as_ref(), yu_trace::code::REPLAY);
}

#[test]
fn oversized_display_degrades_to_digest_with_counter() {
    let dir = fresh_dir("fault-budget");
    let recorder = EventRecorder::with_ring(
        header_for(Vec::new()),
        yu_trace::RingConfig {
            max_display_bytes: 16,
            ..yu_trace::RingConfig::default()
        },
    )
    .expect("ring");
    recorder.begin_tick(1, None);
    recorder
        .record_frame(frame_input(
            1,
            br#"{"long":"this display is definitely over the configured limit"}"#,
        ))
        .unwrap();
    let tick = recorder.finish_fault_tick().expect("tick");
    assert_eq!(tick.displays_dropped, 1);
    assert_eq!(tick.frames_desc.len(), 1);
    assert!(!tick.frames_desc[0].display_retained);
    assert_eq!(tick.frames_desc[0].frame, 1);
    assert_eq!(tick.frames_desc[0].display_sha256.len(), 64);

    let paths = recorder
        .dump_on_fault(dir.join("fault.jsonl"), FaultInfo::panic("budget"))
        .expect("dump");
    let dump = FaultDump::read(&paths.trace).expect("read");
    // 记录仍在 trace 里,但 display 已退化为 null(摘要保留)。
    let text = std::fs::read_to_string(&paths.trace).unwrap();
    assert!(text.contains("\"display\":null"), "{text}");
    assert!(dump.manifest.ticks[0].frames_desc[0].display_sha256.len() == 64);
}

#[test]
fn single_tick_over_total_budget_keeps_placeholder_with_counter() {
    let dir = fresh_dir("fault-placeholder");
    let recorder = EventRecorder::with_ring(
        header_for(Vec::new()),
        yu_trace::RingConfig {
            ticks: 4,
            max_total_bytes: 1,
            ..yu_trace::RingConfig::default()
        },
    )
    .expect("ring");
    recorder.begin_tick(1, None);
    recorder.record(&VmEvent::Command {
        frame: 1,
        script: 1,
        pc: 0,
        name: "WAIT".to_string(),
        args: Vec::new(),
    });
    recorder.finish_fault_tick();
    let paths = recorder
        .dump_on_fault(dir.join("fault.jsonl"), FaultInfo::panic("placeholder"))
        .expect("dump");
    let dump = FaultDump::read(&paths.trace).expect("read");
    assert_eq!(dump.manifest.ticks.len(), 1);
    assert!(!dump.manifest.ticks[0].retained);
    assert!(dump.manifest.ticks[0].over_budget);
    assert_eq!(dump.manifest.evicted_ticks, 1);
}

#[test]
fn screen_frame_projection_keeps_pixels_and_drops_placeholder_counts() {
    let mut header = header_for(Vec::new());
    header.clock = yu_trace::ClockSummary::none();
    let pixels = "a".repeat(64);
    let make = |op_count: u64, pixels: &str| {
        FrameSummary::from_input(FrameDescriptionInput {
            frame: 5,
            level: FrameLevel::Screen,
            pass_count: 0,
            op_count,
            executable_ops: 0,
            skipped_ops: 0,
            display: Some(
                format!(r#"{{"width":800,"height":600,"pixels_sha256":"{pixels}"}}"#).into_bytes(),
            ),
            display_sha256: None,
        })
        .unwrap()
    };
    let mut expected = TraceLog::new();
    expected.push_header(header.clone());
    expected.push_frame(make(0, &pixels));
    let mut actual = TraceLog::new();
    actual.push_header(header);
    actual.push_frame(make(9999, &pixels));

    let options = AlignedDiffOptions {
        header: HeaderCompare::Ignore,
        projection: Projection::with_subs(&[Sub::Frame]),
        payload_fields: Vec::new(),
        frame_fields: FrameFieldPolicy::Screen,
        context: 0,
    };
    assert!(diff_aligned_logs(&expected, &actual, &options).is_ok());

    let mut wrong = TraceLog::new();
    wrong.push_header(expected.header().expect("header").clone());
    wrong.push_frame(make(0, &"b".repeat(64)));
    let failure = diff_aligned_logs(&expected, &wrong, &options).unwrap_err();
    let report = failure.aligned_report().expect("aligned report");
    assert_eq!(report.key.kind, Kind::FrameDescription);
    assert_eq!(report.path, "/payload/display/pixels_sha256");
}
