//! D3/D4 验收:事件流录制闭环。
//!
//! 用 `yu-rt` 的合成工程夹具(经 `Vm::boot` 生产路径)跑一帧:
//! recorder 落盘 → 读回 → 再写 → 两次字节相等;与独立重跑 diff 为 0;
//! 手工改一个字段 ⇒ 在该行报首分歧。
//!
//! 注:当前 `yu-rt` 的处理器只发射 `FrameStart`(尚无 `FrameEnd`),
//! `TEXT` 走 `SystemCall`、`_` 走 `Diag`;本文件只断言**实际发射**的事件,
//! 全部 13 个 `VmEvent` 变体的映射覆盖在 `src/event.rs` 的单元测试里。

mod support;

use support::{ex, GroupSpec, Init, ProjectBuilder, VarSpec};
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::task::MAIN_TASK_ID;
use yu_rt::{BootConfig, Vm};
use yu_trace::record::{Payload, TraceLog};
use yu_trace::{diff_text, error, VmTrace};
use yu_vfs::Codec;

const OP_END: u8 = 14;
const OP_FPS: u8 = 29;
const OP_INT: u8 = 51;
const OP_LET: u8 = 54;
const OP_TEXT: u8 = 96;
const OP_WAIT: u8 = 102;

/// 一帧内可确定完成的一条小脚本:声明变量 → 赋值 → FPS → TEXT → WAIT → END。
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
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::cat(&[ex::get(0x40, sys), ex::i8_(2), ex::op(0x2a)])),
        // FPS[SET=30]:只有一个 kind=0 窗口,避免撞上范围校验。
        GroupSpec::new(OP_FPS).window(0, ex::i8_(30)),
        GroupSpec::new(OP_TEXT).window(0, ex::s("hello")),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    b
}

fn config() -> BootConfig {
    BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        start_ms: 0,
        fps: 60,
        ..BootConfig::default()
    }
}

fn boot() -> Vm {
    Vm::boot(Box::new(project().build()), config()).expect("boot fixture")
}

fn timeline() -> InputTimeline {
    let mut timeline = InputTimeline::new();
    timeline.set(0, InputFrame::new(["KEY_A"]));
    timeline.set(1, InputFrame::new(["KEY_A"]));
    timeline
}

fn record_frame() -> TraceLog {
    let mut vm = boot();
    let log = yu_trace::shared_log();
    let recorder = yu_trace::EventRecorder::new(log.clone());
    // 启动链事件(含 TaskSpawn)先入流。
    recorder.drain_vm(&mut vm);
    // 第一帧推进到 WAIT 让出;第二帧醒来执行 END。
    let first = vm.tick_at(0, &timeline());
    recorder.drain_vm(&mut vm);
    let second = vm.tick_at(16, &timeline());
    recorder.drain_vm(&mut vm);
    let frames = (first.logical_frames + second.logical_frames) as u64;
    assert!(frames >= 2);

    let header = yu_trace::header_from_vm(
        &vm,
        &config(),
        Codec::Cp932,
        yu_trace::input_summary(&timeline(), frames),
    );
    let mut log = log.lock().unwrap();
    log.prepend_header(header);
    log.clone()
}

#[test]
fn event_stream_roundtrips_and_diffs_to_zero() {
    let log = record_frame();
    let text = log.to_jsonl();

    // 头部在 seq 0;事件流按录制顺序连续。
    assert_eq!(log.records()[0].kind().as_str(), "header");
    assert_eq!(log.records()[0].seq(), 0);
    let kinds: Vec<&str> = log.records()[1..]
        .iter()
        .map(|record| record.kind().as_str())
        .collect();
    assert!(kinds.contains(&"task_spawn"), "启动事件必须入流: {kinds:?}");
    assert!(kinds.contains(&"frame_start"));
    assert!(kinds.contains(&"command"));
    assert!(kinds.contains(&"fps"));
    assert!(kinds.contains(&"system_call"), "kinds: {kinds:?}");
    assert!(kinds.contains(&"wait"));
    assert!(kinds.contains(&"end"));

    // 落盘 → 读回 → 再写:逐字节一致。
    let dir = support::fresh_dir("event_stream");
    let path = dir.join("frame.jsonl");
    log.write_to(&path).expect("write");
    let first = std::fs::read(&path).expect("read");
    let parsed = TraceLog::read_from(&path).expect("parse");
    parsed.write_to(&path).expect("rewrite");
    let second = std::fs::read(&path).expect("read again");
    assert_eq!(first, second);
    assert_eq!(String::from_utf8(first).unwrap(), text);

    // 独立重跑:同一输入 ⇒ 同一流(逐行 diff 为 0)。
    let replay = record_frame();
    assert_eq!(
        diff_text(&replay.to_jsonl(), &text),
        Ok(()),
        "same input must produce a zero diff"
    );
    assert_eq!(replay.to_jsonl(), text);
}

#[test]
fn header_records_boot_input_and_table_provenance() {
    let log = record_frame();
    let header = log.header().expect("header");
    assert_eq!(header.boot.entry, "SYSTEM_START");
    assert_eq!(header.boot.fps, 60);
    assert_eq!(header.boot.start_ms, 0);
    assert_eq!(header.codec, "cp932");
    assert_eq!(header.input.events, 2, "时间线显式设定的帧数");
    // 表来源必须是相对路径/表版本,不得含绝对路径。
    for table in &header.tables {
        assert!(!table.contains("D:\\"), "absolute path leaked: {table}");
        assert!(!table.contains("D:/"), "absolute path leaked: {table}");
    }
    assert!(header.tables.iter().any(|table| table.starts_with("ysc:")));
    assert!(header.kernel.starts_with("yu-trace "));
}

#[test]
fn command_payload_keeps_evaluated_args() {
    let log = record_frame();
    let command = log
        .records()
        .iter()
        .find(|record| match record.payload() {
            Payload::VmCommand { name, .. } => name == "LET",
            _ => false,
        })
        .expect("LET command recorded");
    match command.payload() {
        Payload::VmCommand {
            script,
            pc,
            name,
            args,
        } => {
            assert_eq!(*script, 0);
            assert_eq!(*pc, 1);
            assert_eq!(name, "LET");
            // 第二个窗口是表达式 @v*2,求值后入流。
            let evaluated = args
                .iter()
                .find(|arg| matches!(arg.value, yu_trace::TraceValue::Int(14)))
                .expect("evaluated expression arg");
            assert_eq!(evaluated.slot, 0);
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn text_command_is_recorded_as_system_call() {
    // 当前 `yu-rt` 的 TEXT 处理器发射 SystemCall(name="TEXT");
    // 文本字节事件(VmEvent::Text)目前尚未由运行核发射,映射由单元测试覆盖。
    let log = record_frame();
    let call = log
        .records()
        .iter()
        .find_map(|record| match record.payload() {
            Payload::VmCall {
                domain: None,
                name,
                args,
            } if name == "TEXT" => Some(args.clone()),
            _ => None,
        })
        .expect("TEXT system call");
    assert!(call.is_empty());
}

#[test]
fn mutating_one_field_reports_first_divergence_on_that_line() {
    let log = record_frame();
    let text = log.to_jsonl();
    let (index, line) = log
        .records()
        .iter()
        .enumerate()
        .find(|(_, record)| matches!(record.payload(), Payload::VmFps { .. }))
        .map(|(index, record)| (index, record.to_line()))
        .expect("fps record");
    let mutated_line = line.replacen("\"fps\":30", "\"fps\":31", 1);
    assert_ne!(mutated_line, line);

    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines[index] = mutated_line;
    let mutated = format!("{}\n", lines.join("\n"));

    let failure = diff_text(&text, &mutated).expect_err("must diverge");
    assert_eq!(failure.code(), error::code::DIFF);
    let report = failure.report().expect("content divergence");
    assert_eq!(report.line, index as u64 + 1);
    assert_eq!(report.path, "/payload/fps");
    assert_eq!(report.expected, "30");
    assert_eq!(report.actual, "31");
}

#[test]
fn omitting_a_trailing_record_is_length_not_a_pass() {
    let log = record_frame();
    let text = log.to_jsonl();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.pop();
    let shortened = format!(
        "{}
",
        lines.join(
            "
"
        )
    );
    let failure = diff_text(&text, &shortened).expect_err("must not pass");
    assert_eq!(failure.code(), error::code::LENGTH);
    assert!(failure.report().is_none(), "count mismatch has no pointer");
}

#[test]
fn cutting_a_line_midway_is_truncated() {
    let log = record_frame();
    let text = log.to_jsonl();
    let cut = text[..text.len() - 7].to_string();
    let failure = diff_text(&text, &cut).expect_err("must not pass");
    assert_eq!(failure.code(), error::code::TRUNCATED);
}

/// 事件部分的规范投影(不含 `seq`:在线流前面多了启动链的 vfs 记录)。
fn event_shape(log: &TraceLog) -> Vec<(Option<u64>, &'static str, Payload)> {
    log.records()
        .iter()
        .filter(|record| record.sub() == yu_trace::Sub::Vm)
        .map(|record| {
            (
                record.frame(),
                record.kind().as_str(),
                record.payload().clone(),
            )
        })
        .collect()
}

#[test]
fn online_vm_trace_matches_offline_recording() {
    // 在线路径:同一工程走 `VmTrace`(vfs 读取也进同一条流);
    // 离线路径:只喂事件。两者的事件部分必须一致。
    let mut trace =
        VmTrace::boot(Box::new(project().build()), config(), Codec::Cp932).expect("boot traced");
    let report = trace.tick_at(0, &timeline());
    assert!(report.commands_pumped > 0);
    trace.tick_at(16, &timeline());
    let online = trace.finish_with_timeline(&timeline());

    let offline = record_frame();
    assert_eq!(event_shape(&online), event_shape(&offline));

    // vfs 记录存在且带内容身份(帧外 ⇒ frame = null)。
    let vfs: Vec<&yu_trace::Record> = online
        .records()
        .iter()
        .filter(|record| record.sub() == yu_trace::Sub::Vfs)
        .collect();
    assert!(!vfs.is_empty(), "启动链读取必须入流");
    assert!(vfs.iter().all(|record| record.frame().is_none()));
    assert!(vfs.iter().any(|record| matches!(
        record.payload(),
        Payload::VfsOpen {
            sha256: Some(_),
            ..
        }
    )));

    // 在线流自身可往返。
    let text = online.to_jsonl();
    assert_eq!(text, TraceLog::parse(&text).unwrap().to_jsonl());
}

#[test]
fn task_spawn_is_recorded_from_boot_events() {
    let log = record_frame();
    let spawn = log
        .records()
        .iter()
        .find_map(|record| match record.payload() {
            Payload::VmTaskSpawn { id, main, .. } => Some((id.clone(), *main)),
            _ => None,
        })
        .expect("task spawn");
    assert_eq!(spawn.0, MAIN_TASK_ID);
    assert!(spawn.1);
}

// ---------------------------------------------------------------- 回放闭环(D4)

fn record_two_frames() -> TraceLog {
    let timeline = timeline();
    let mut trace =
        VmTrace::boot(Box::new(project().build()), config(), Codec::Cp932).expect("boot");
    trace.tick_at(0, &timeline);
    trace.tick_at(16, &timeline);
    trace.finish_with_timeline(&timeline)
}

#[test]
fn header_replays_to_a_byte_identical_trace() {
    let recorded = record_two_frames();
    let header = recorded.header().expect("header").clone();
    assert_eq!(header.boot.script_key, yu_rt::tables::DEFAULT_SCRIPT_KEY);
    assert!(!header.boot.debug_mode);
    assert_eq!(header.boot.argv_len, 0);
    // 两次 `tick_at` 的 now_ms 序列必须进入 header。
    assert_eq!(header.clock.mode, yu_trace::ClockMode::TickAt);
    assert_eq!(header.clock.now_ms, vec![Some(0), Some(16)]);

    let timeline = timeline();
    let mut replayed = VmTrace::from_header_checked(
        &header,
        Box::new(project().build()),
        Codec::Cp932,
        None,
        None,
        Vec::new(),
        yu_rt::vm::HostPaths::default(),
    )
    .expect("boot from header");
    replayed.tick_at(0, &timeline);
    replayed.tick_at(16, &timeline);
    // 回放时钟序列必须与 header 一致。
    assert_eq!(
        yu_trace::verify_clock(&header, replayed.clock()),
        Ok(()),
        "replay clock must match header"
    );
    let replayed = replayed.finish_with_timeline(&timeline);

    assert_eq!(
        diff_text(&recorded.to_jsonl(), &replayed.to_jsonl()),
        Ok(()),
        "replaying from the recorded header must be byte-identical"
    );
    assert_eq!(recorded.to_jsonl(), replayed.to_jsonl());

    // 输入摘要必须能验证同一时间线,并拒绝不同时间线。
    assert_eq!(yu_trace::verify_input(&header, &timeline, 2), Ok(()));
    let mut different = InputTimeline::new();
    different.set(1, InputFrame::new(["KEY_B"]));
    let err = yu_trace::verify_input(&header, &different, 2).expect_err("must differ");
    assert_eq!(err.code, yu_trace::code::REPLAY);
}

#[test]
fn replay_rejects_unknown_codec_and_argv_mismatch() {
    let recorded = record_two_frames();
    let mut header = recorded.header().expect("header").clone();
    header.codec = "utf8".to_string();
    let err = VmTrace::from_header(
        &header,
        Box::new(project().build()),
        None,
        Vec::new(),
        yu_rt::vm::HostPaths::default(),
    )
    .err()
    .expect("unknown codec must fail");
    assert_eq!(err.code, yu_trace::code::SCHEMA);

    let mut header = recorded.header().expect("header").clone();
    header.boot.argv_sha256 = "0".repeat(64);
    header.boot.argv_len = 1;
    let err = VmTrace::from_header(
        &header,
        Box::new(project().build()),
        None,
        Vec::new(),
        yu_rt::vm::HostPaths::default(),
    )
    .err()
    .expect("argv mismatch must fail");
    assert_eq!(err.code, yu_trace::code::REPLAY);
}

#[test]
fn input_summary_window_matches_consumed_frames() {
    // VM 消费 1..=2;第 2 帧的输入必须影响摘要(旧实现按 0..2 会漏掉)。
    let mut late = InputTimeline::new();
    late.set(2, InputFrame::new(["KEY_Z"]));
    let mut other = InputTimeline::new();
    other.set(2, InputFrame::new(["KEY_Y"]));
    assert_ne!(
        yu_trace::input_summary(&late, 2).sha256,
        yu_trace::input_summary(&other, 2).sha256
    );
}

#[test]
fn replay_rejects_clock_mismatch_and_source_codec_mismatch() {
    let recorded = record_two_frames();
    let header = recorded.header().expect("header").clone();

    // 时钟序列不同(用 tick 而不是 tick_at)⇒ trace.replay。
    let err = yu_trace::verify_clock(&header, &[None, None]).expect_err("clock must differ");
    assert_eq!(err.code, yu_trace::code::REPLAY);
    let err = yu_trace::verify_clock(&header, &[Some(0), Some(17)]).expect_err("clock must differ");
    assert_eq!(err.code, yu_trace::code::REPLAY);

    // 数据源编码与 header.codec 不一致 ⇒ trace.replay。
    let err = VmTrace::from_header_checked(
        &header,
        Box::new(project().build()),
        Codec::Gbk,
        None,
        None,
        Vec::new(),
        yu_rt::vm::HostPaths::default(),
    )
    .err()
    .expect("codec mismatch must fail");
    assert_eq!(err.code, yu_trace::code::REPLAY);
}

#[test]
fn mixed_clock_modes_are_recorded_per_tick() {
    let timeline = timeline();
    let mut trace =
        VmTrace::boot(Box::new(project().build()), config(), Codec::Cp932).expect("boot");
    trace.tick(&timeline);
    trace.tick_at(64, &timeline);
    assert_eq!(trace.clock(), &[None, Some(64)]);
    let log = trace.finish_with_timeline(&timeline);
    let header = log.header().expect("header");
    assert_eq!(header.clock.mode, yu_trace::ClockMode::Mixed);
    assert_eq!(header.clock.now_ms, vec![None, Some(64)]);
    let text = log.to_jsonl();
    assert_eq!(TraceLog::parse(&text).unwrap().to_jsonl(), text);
}
