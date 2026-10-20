//! golden:合成工程的完整录制流(边界① + ②)入库为 <32KB 的 fixture。
//!
//! 更新方式(评审后提交):`UPDATE_GOLDEN=1 cargo test -p yu-trace --test golden`
//!
//! 与 `yu-rt/tests/golden` 的分工:那边是运行核自己的事件 JSONL;
//! 这边是 **yu-trace 协议**的完整流(header + vfs 读取 + 事件),用于
//! 守护"同一输入 ⇒ 逐字节相同的 trace"。

mod support;

use std::path::{Path, PathBuf};

use support::{ex, GroupSpec, Init, ProjectBuilder, VarSpec};
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::BootConfig;
use yu_trace::{diff_text, TraceLog, VmTrace};
use yu_vfs::Codec;

const OP_END: u8 = 14;
const OP_FPS: u8 = 29;
const OP_INT: u8 = 51;
const OP_LET: u8 = 54;
const OP_TEXT: u8 = 96;
const OP_WAIT: u8 = 102;

const MAX_FIXTURE_BYTES: usize = 32 * 1024;

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
        GroupSpec::new(OP_FPS).window(0, ex::i8_(30)),
        GroupSpec::new(OP_TEXT).window(0, ex::s("hello")),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    b
}

fn timeline() -> InputTimeline {
    let mut timeline = InputTimeline::new();
    timeline.set(0, InputFrame::new(["KEY_A"]));
    timeline
}

fn record() -> TraceLog {
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        start_ms: 0,
        fps: 60,
        ..BootConfig::default()
    };
    let mut trace = VmTrace::boot(Box::new(project().build()), cfg, Codec::Cp932).expect("boot");
    let timeline = timeline();
    trace.tick_at(0, &timeline);
    trace.tick_at(16, &timeline);
    trace.finish_with_timeline(&timeline)
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("synthetic_frame.jsonl")
}

#[test]
fn synthetic_trace_matches_golden() {
    let actual = record().to_jsonl();
    assert!(
        actual.len() < MAX_FIXTURE_BYTES,
        "golden fixture must stay under 32KB, got {} bytes",
        actual.len()
    );

    let path = golden_path();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read golden {} failed: {e} (run UPDATE_GOLDEN=1 first)",
            path.display()
        )
    });
    if let Err(failure) = diff_text(&expected, &actual) {
        panic!(
            "golden mismatch: {}; golden {} bytes, actual {} bytes",
            failure.diag,
            expected.len(),
            actual.len()
        );
    }
}

#[test]
fn golden_is_deterministic_across_recordings() {
    let first = record().to_jsonl();
    let second = record().to_jsonl();
    assert_eq!(first, second);
    // 头部与 vfs 记录都在流里,且不含绝对路径。
    assert!(first.starts_with(&format!(
        "{{\"schema\":{},\"seq\":0,\"frame\":null,\"sub\":\"meta\",\"kind\":\"header\"",
        yu_trace::SCHEMA_VERSION
    )));
    assert!(first.contains("\"sub\":\"vfs\""));
    assert!(first.contains("\"sub\":\"vm\""));
    assert!(!first.contains("D:\\\\"));
    assert!(!first.contains("D:/"));
}
