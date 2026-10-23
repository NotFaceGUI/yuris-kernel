//! 内核侧事件 trace:同一 `data.ypf` 语料 + 注入输入,按帧窗口写 schema v4。
//!
//! 用途:与 `tools/official_trace/probe_dispatch.py` 采到的官方事件 trace 对拍。
//! 只消费 `yu-rt` / `yu-trace` 公开 API;输出 record 顺序与 `seq` 由 `yu-trace` 拥有。
//!
//! 用法:
//! ```text
//! cargo run -q -p yu-trace --example emit_kernel_events -- \
//!   --root <project root(含 data.ypf)> --out <trace.jsonl> \
//!   --frames 20 --first-frame 6 --last-frame 16 --click-frame 6
//! ```
//!
//! 输入语义:引擎按钮名 `MOUSE_L` / `ENTER`(与官方 PostMessage 的鼠标左键 +
//! 回车同形),光标 (410,275);只按下 1 帧,下一帧显式松开。

use std::process::ExitCode;

use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_trace::{
    header_from_vm_with_clock, input_summary_range, sha256_hex, ClockSummary, EventRecorder,
    HeaderPayload,
};
use yu_vfs::{Codec, DirSource, YpfSource};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|item| item == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_u64(args: &[String], name: &str, default: u64) -> u64 {
    arg(args, name)
        .map(|text| text.parse::<u64>().expect("unsigned integer"))
        .unwrap_or(default)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = arg(&args, "--root") else {
        eprintln!("missing --root <project root with data.ypf>");
        return ExitCode::from(2);
    };
    let Some(out) = arg(&args, "--out") else {
        eprintln!("missing --out <trace.jsonl>");
        return ExitCode::from(2);
    };
    let frames = parse_u64(&args, "--frames", 20);
    let first = parse_u64(&args, "--first-frame", 0);
    let last = parse_u64(&args, "--last-frame", frames.saturating_sub(1));
    let click_frame = parse_u64(&args, "--click-frame", 6);
    let debug_mode = arg(&args, "--debug-mode").as_deref() == Some("true");
    if last < first || last >= frames {
        eprintln!("require 0 <= first-frame <= last-frame < frames");
        return ExitCode::from(2);
    }

    let pack_path = format!("{root}/data.ypf");
    let pack_bytes = match std::fs::read(&pack_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("read {pack_path}: {error}");
            return ExitCode::from(2);
        }
    };
    let sample_sha = sha256_hex(&pack_bytes);
    let src = match YpfSource::open(&pack_path, Codec::Cp932) {
        Ok(src) => src,
        Err(diag) => {
            eprintln!("open {pack_path}: {diag}");
            return ExitCode::from(2);
        }
    };
    let aux = match DirSource::open(&root, Codec::Cp932) {
        Ok(aux) => aux,
        Err(diag) => {
            eprintln!("open aux {root}: {diag}");
            return ExitCode::from(2);
        }
    };
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        screen_color: 32,
        debug_mode,
        ..BootConfig::default()
    };
    let mut vm = match Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg.clone()) {
        Ok(vm) => vm,
        Err(diag) => {
            eprintln!("boot: {diag}");
            return ExitCode::from(2);
        }
    };

    let mut timeline = InputTimeline::new();
    timeline.set(
        click_frame,
        InputFrame::new(["MOUSE_L", "ENTER"]).with_cursor(410, 275),
    );
    timeline.set(click_frame + 1, InputFrame::default());

    let mut header: HeaderPayload = header_from_vm_with_clock(
        &vm,
        &cfg,
        Codec::Cp932,
        input_summary_range(&timeline, first, last - first + 1),
        ClockSummary::none(),
    );
    header.producer = header.producer.with_sample_sha256(sample_sha.clone());
    let recorder = EventRecorder::with_header(header);

    let mut total_events = 0usize;
    let mut recorded = 0usize;
    let mut commands = 0usize;
    let mut seen_frames = (u64::MAX, 0u64);
    for frame_index in 0..frames {
        let report = vm.tick_at(frame_index as u32 * 16, &timeline);
        let events = vm.take_events();
        total_events += events.len();
        for event in &events {
            let frame = event.frame();
            seen_frames.0 = seen_frames.0.min(frame);
            seen_frames.1 = seen_frames.1.max(frame);
            if frame < first || frame > last {
                continue;
            }
            recorder.record(event);
            recorded += 1;
            if matches!(event, VmEvent::Command { .. }) {
                commands += 1;
            }
        }
        let _ = report.main_finished;
    }
    let log = recorder.snapshot();
    if let Err(diag) = log.write_to(&out) {
        eprintln!("write {out}: {diag}");
        return ExitCode::from(2);
    }
    println!(
        "wrote {out}: records={} (recorded={recorded}) commands={} window=[{first},{last}] seen=[{},{}] total_events={total_events} sample={sample_sha}",
        log.len(),
        commands,
        seen_frames.0,
        seen_frames.1,
    );
    ExitCode::SUCCESS
}
