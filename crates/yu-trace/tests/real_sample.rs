//! D5 验收:官方 `data.ypf` 冒烟(env gate)。
//!
//! gate: `YURIS_SAMPLE_ERIS_YPF=<official data.ypf>`。
//! 未设置时**显式跳过并计数**:产一条 `trace.sample.missing` Info 诊断,
//! 并把本次跳过记入 `SKIPPED`,在同一个测试里断言计数为 1
//! (不是"静默通过":缺少样本这件事本身会被测试代码检查)。
//!
//! 样本存在时验证:
//! 1. `open("ysbin\ysc.ybn")` 经 `TracingSource` 与直接 `yu-vfs` 读取
//!    **字节与 SHA-256 完全一致**;
//! 2. trace 里的记录条数 = 实际调用条数,且内容身份来自真实字节;
//! 3. 真实包可启动 `VmTrace`,帧推进后事件入流、录制流可逐字节往返、
//!    同输入重跑 diff 为 0。

mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use yu_log::{LogSink, MemorySink};
use yu_rt::input::InputTimeline;
use yu_rt::BootConfig;
use yu_trace::record::{Payload, TraceLog};
use yu_trace::{diff_text, sample_missing, TracingSource, VmTrace};
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

/// 本测试二进制里因缺样本而跳过的用例数(必须恰好等于 1)。
static SKIPPED: AtomicUsize = AtomicUsize::new(0);

/// 显式跳过:产 Info 诊断 + 计数;返回 `true` 表示调用方应立即返回。
///
/// `YURIS_REQUIRE_SAMPLES=1` 时改为硬失败(缺样本不得静默通过)。
fn skip_if_missing(sample: &str, env: &str) -> bool {
    if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
        panic!("YURIS_REQUIRE_SAMPLES=1 but sample unavailable: {sample} ({env})");
    }
    let mut sink = MemorySink::new();
    sink.emit(&sample_missing(sample, env))
        .expect("memory sink");
    assert_eq!(sink.len(), 1, "skip must emit exactly one diag");
    assert_eq!(
        sink.records()[0].code.as_ref(),
        yu_trace::code::SAMPLE_MISSING
    );
    assert!(
        sink.records()[0].detail.contains("sample not found, skip"),
        "skip detail must say so: {}",
        sink.records()[0].detail
    );
    SKIPPED.fetch_add(1, Ordering::Relaxed);
    true
}

fn ypf_path() -> Option<PathBuf> {
    let raw = std::env::var_os("YURIS_SAMPLE_ERIS_YPF")?;
    let path = PathBuf::from(raw);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn ysc_key() -> LookupKey {
    LookupKey::new(br"ysbin\ysc.ybn", Codec::Cp932).unwrap()
}

#[test]
fn official_data_ypf_smoke() {
    let Some(path) = ypf_path() else {
        // 缺样本:显式跳过并计数,本用例到此为止(不是空跑通过)。
        assert!(skip_if_missing("data.ypf", "YURIS_SAMPLE_ERIS_YPF"));
        assert_eq!(SKIPPED.load(Ordering::Relaxed), 1);
        return;
    };

    // ---- 1. open 与直接读取逐字节一致,记录带内容身份 ----
    let direct = YpfSource::open(&path, Codec::Cp932).expect("open ypf");
    let expected = direct.open(&ysc_key()).expect("ysc direct");

    let log = yu_trace::shared_log();
    let traced = TracingSource::new(
        YpfSource::open(&path, Codec::Cp932).expect("open ypf (traced)"),
        log,
    );
    let actual = traced.open(&ysc_key()).expect("ysc traced");
    assert_eq!(actual, expected, "wrapped open must be byte-identical");

    let log = traced.recorder().shared_log();
    {
        let records = log.borrow();
        assert_eq!(records.len(), 1, "one call = one record");
        match records.records()[0].payload() {
            Payload::VfsOpen {
                source,
                key,
                bytes,
                sha256,
                diag,
            } => {
                assert_eq!(source, "ypf");
                assert_eq!(key.as_slice(), b"ysbin/ysc.ybn");
                assert_eq!(*bytes, Some(expected.len() as u64));
                assert_eq!(
                    sha256.as_deref(),
                    Some(yu_trace::sha256_hex(&expected).as_str())
                );
                assert!(diag.is_none());
                assert!(!expected.is_empty());
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    // ---- 2. 目录列举也走同一记录通道 ----
    let ysbin = LookupKey::new(b"ysbin", Codec::Cp932).unwrap();
    let entries = traced.list(&ysbin).expect("list ysbin");
    assert!(entries.iter().any(|entry| entry.name == b"ysc.ybn"));
    assert_eq!(traced.recorder().snapshot().len(), 2);

    // ---- 3. 真实包启动 + 三帧录制 + 同输入重跑零差异 ----
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        start_ms: 0,
        fps: 60,
        ..BootConfig::default()
    };
    let run = || {
        let src = YpfSource::open(&path, Codec::Cp932).expect("open ypf");
        let mut trace = VmTrace::boot(Box::new(src), cfg.clone(), Codec::Cp932)
            .expect("official data.ypf boots");
        let timeline = InputTimeline::new();
        for frame in 0..3u32 {
            trace.tick_at(frame * 16, &timeline);
        }
        // 回归:录制路径必须完整。官方 ERIS 首帧约 130 万命令,运行核默认
        // `1<<20` 的事件队列会在 trace 拉取前丢事件;VmTrace 必须关闭该阈值。
        assert_eq!(
            trace.vm().events_dropped(),
            0,
            "trace must not drop events: ERIS frame 0 exceeds the default event cap"
        );
        trace.finish_with_timeline(&timeline)
    };
    let first = run();
    let second = run();

    let vfs_records = first
        .records()
        .iter()
        .filter(|record| record.sub() == yu_trace::Sub::Vfs)
        .count();
    let event_records = first
        .records()
        .iter()
        .filter(|record| record.sub() == yu_trace::Sub::Vm)
        .count();
    assert!(vfs_records >= 5, "启动链至少读 5 张表,实际 {vfs_records}");
    assert!(event_records > 0, "帧推进必须产生事件");
    assert!(
        event_records > (1 << 20),
        "回归必须真的超过运行核默认事件上限(1<<20),实际 {event_records}"
    );
    assert!(first.records().iter().any(|record| matches!(
        record.payload(),
        Payload::VfsOpen {
            sha256: Some(_),
            ..
        }
    )));

    let text = first.to_jsonl();
    assert_eq!(diff_text(&text, &second.to_jsonl()), Ok(()));
    assert_eq!(text, TraceLog::parse(&text).unwrap().to_jsonl());

    let header = first.header().expect("header");
    assert!(!header.tables.iter().any(|table| table.contains("D:")));
    assert_eq!(header.boot.entry, "SYSTEM_START");
    assert_eq!(header.codec, "cp932");

    // 样本存在 ⇒ 没有跳过。
    assert_eq!(SKIPPED.load(Ordering::Relaxed), 0);
}

#[test]
fn recorder_can_start_from_a_bare_log_without_header() {
    // 与样本无关:确认 `EventRecorder` 不强制 header,
    // 启动链 vfs 记录可以先入流,收尾时再 `prepend_header`。
    let log = yu_trace::shared_log();
    let recorder = yu_trace::EventRecorder::new(log);
    recorder.record(&yu_rt::VmEvent::FrameStart { frame: 1 });
    let mut snapshot = recorder.snapshot();
    assert_eq!(snapshot.len(), 1);
    assert!(snapshot.header().is_none());
    snapshot.prepend_header(support::header_for(vec!["ysc: v495 (105 命令)".into()]));
    assert_eq!(snapshot.records()[0].kind().as_str(), "header");
    assert_eq!(snapshot.records()[1].seq(), 1);
}
