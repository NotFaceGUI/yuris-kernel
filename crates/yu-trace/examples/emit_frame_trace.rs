//! 把调用方(gfx/后端/官方采集器)提供的 frame JSON 或屏幕描述写成 schema v3 trace。
//!
//! 用法:
//! ```text
//! cargo run -p yu-trace --example emit_frame_trace -- \
//!   --display target/tmp/first_frame.display.json --out target/tmp/kernel_frame.jsonl \
//!   --frame 3 --level event --pass-count 15 --op-count 44 \
//!   --executable-ops 15 --skipped-ops 29 --producer kernel
//! ```
//!
//! 该 example 只消费 `yu-trace` 的公开 API;不依赖 `yu-gfx`/`yu-rt`。

use std::process::ExitCode;

use yu_trace::{
    sha256_hex, ClockSummary, FrameDescriptionInput, FrameLevel, FrameSummary, HeaderPayload,
    InputSummary, ProducerInfo, TraceLog,
};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|item| item == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_u64(args: &[String], name: &str, default: u64) -> u64 {
    arg(args, name)
        .map(|text| {
            text.parse::<u64>()
                .unwrap_or_else(|_| panic!("{name} must be an unsigned integer, got {text:?}"))
        })
        .unwrap_or(default)
}

fn parse_u32(args: &[String], name: &str, default: u32) -> u32 {
    parse_u64(args, name, default as u64) as u32
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(display_path) = arg(&args, "--display") else {
        eprintln!("missing --display <json>");
        return ExitCode::from(2);
    };
    let Some(out) = arg(&args, "--out") else {
        eprintln!("missing --out <trace.jsonl>");
        return ExitCode::from(2);
    };
    let frame = parse_u64(&args, "--frame", 0);
    let level = match arg(&args, "--level").as_deref() {
        Some("screen") => FrameLevel::Screen,
        _ => FrameLevel::Event,
    };
    let producer_kind = arg(&args, "--producer").unwrap_or_else(|| "kernel".to_string());
    let producer = match producer_kind.as_str() {
        "official" => {
            let name = arg(&args, "--producer-name").unwrap_or_else(|| "official".to_string());
            let version = arg(&args, "--producer-version").unwrap_or_else(|| "0.0.0".to_string());
            let mut producer = ProducerInfo::official(name, version);
            if let Some(sample) = arg(&args, "--sample-sha256") {
                producer = producer.with_sample_sha256(sample);
            }
            producer
        }
        "kernel" => {
            let mut producer = ProducerInfo::kernel();
            if let Some(sample) = arg(&args, "--sample-sha256") {
                producer = producer.with_sample_sha256(sample);
            }
            producer
        }
        other => {
            eprintln!("unknown --producer {other:?} (kernel|official)");
            return ExitCode::from(2);
        }
    };

    let display = match std::fs::read(&display_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("read {display_path}: {error}");
            return ExitCode::from(2);
        }
    };
    let summary = match FrameSummary::from_input(FrameDescriptionInput {
        frame,
        level,
        pass_count: parse_u32(&args, "--pass-count", 0),
        op_count: parse_u64(&args, "--op-count", 0),
        executable_ops: parse_u64(&args, "--executable-ops", 0),
        skipped_ops: parse_u64(&args, "--skipped-ops", 0),
        display: Some(display),
        display_sha256: arg(&args, "--display-sha256"),
    }) {
        Ok(summary) => summary,
        Err(diag) => {
            eprintln!("frame description rejected: {diag}");
            return ExitCode::from(2);
        }
    };

    let mut log = TraceLog::new();
    log.push_header(HeaderPayload {
        codec: "cp932".to_string(),
        producer,
        tables: Vec::new(),
        boot: yu_trace::BootSummary::from_config(&yu_rt::BootConfig::default()),
        input: InputSummary {
            events: 0,
            sha256: sha256_hex(&[]),
        },
        clock: ClockSummary::none(),
        kernel: yu_trace::kernel_version().to_string(),
    });
    log.push_frame(summary);
    if let Err(diag) = log.write_to(&out) {
        eprintln!("write {out}: {diag}");
        return ExitCode::from(2);
    }
    println!("wrote {out} ({} records)", log.len());
    ExitCode::SUCCESS
}
