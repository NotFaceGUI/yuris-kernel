//! 故障快照演示(P5/H-P5-01):合成 panic → `dump_on_fault` → 离线重演到故障帧。
//!
//! 这是宿主接线的参考实现:真实宿主在启动时挂 ring,在 panic hook /
//! `catch_unwind` / Fatal Diag 处调用 `EventRecorder::dump_on_fault`。
//! 桌面宿主(winit)的接线归 P1/P4,见交接件 H-P5-01。
//!
//! 用法:
//! ```text
//! cargo run -q -p yu-cli --example fault_dump -- \
//!   --root <project root(含 data.ypf)> --out target/tmp/fault-demo/fault.jsonl \
//!   --fault-tick 6
//! ```
//!
//! 缺 `data.ypf` 时**显式失败**,不空跑。

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::process::ExitCode;

use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::BootConfig;
use yu_trace::{
    header_from_vm_with_clock, sha256_hex, ClockSummary, FaultDump, FaultInfo, InputSummary,
    RingConfig, VmTrace,
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

fn open_sources(root: &str) -> Result<(YpfSource, DirSource), String> {
    let pack = format!("{root}/data.ypf");
    if !std::path::Path::new(&pack).is_file() {
        return Err(format!(
            "missing {pack}; pass --root pointing at a project with data.ypf \
             (see AGENTS.md corpus contract)"
        ));
    }
    let src =
        YpfSource::open(&pack, Codec::Cp932).map_err(|diag| format!("open {pack}: {diag}"))?;
    let aux =
        DirSource::open(root, Codec::Cp932).map_err(|diag| format!("open aux {root}: {diag}"))?;
    Ok((src, aux))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = arg(&args, "--root").or_else(|| std::env::var("YURIS_SAMPLE_ERIS_ROOT").ok())
    else {
        eprintln!("missing --root <project root with data.ypf>");
        return ExitCode::from(2);
    };
    let out = PathBuf::from(
        arg(&args, "--out").unwrap_or_else(|| "target/tmp/fault-demo/fault.jsonl".to_string()),
    );
    let fault_tick = parse_u64(&args, "--fault-tick", 6);
    let sample_sha = match std::fs::read(format!("{root}/data.ypf")) {
        Ok(bytes) => sha256_hex(&bytes),
        Err(error) => {
            eprintln!("read {root}/data.ypf: {error}");
            return ExitCode::from(2);
        }
    };
    let (src, aux) = match open_sources(&root) {
        Ok(sources) => sources,
        Err(detail) => {
            eprintln!("{detail}");
            return ExitCode::from(2);
        }
    };
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        screen_color: 32,
        debug_mode: true,
        ..BootConfig::default()
    };
    let mut trace = match VmTrace::boot_with_aux(
        Box::new(src),
        Some(Box::new(aux)),
        cfg.clone(),
        Codec::Cp932,
    ) {
        Ok(trace) => trace,
        Err(diag) => {
            eprintln!("boot: {diag}");
            return ExitCode::from(2);
        }
    };
    // 头部里写样本身份;dump 时会按故障帧重算 input/clock,不会覆盖 producer。
    let mut header = header_from_vm_with_clock(
        trace.vm(),
        &cfg,
        Codec::Cp932,
        InputSummary {
            events: 0,
            sha256: sha256_hex(&[]),
        },
        ClockSummary::none(),
    );
    header.producer = header.producer.with_sample_sha256(sample_sha.clone());
    if let Err(diag) = trace.enable_fault_ring(RingConfig::default(), None) {
        eprintln!("enable ring: {diag}");
        return ExitCode::from(2);
    }
    // enable_fault_ring 会用当前 VM 重建基础头部;样本身份在启用之后覆盖。
    trace.recorder().set_fault_header(header);

    let mut timeline = InputTimeline::new();
    timeline.set(
        6,
        InputFrame::new(["MOUSE_L", "ENTER"]).with_cursor(410, 275),
    );
    timeline.set(7, InputFrame::default());

    let recorder = trace.recorder().clone();
    let dump_path = out.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let detail = info.to_string();
        match recorder.dump_on_fault(&dump_path, FaultInfo::panic(detail.clone())) {
            Ok(paths) => eprintln!(
                "fault dump written: {} (+ {})",
                paths.trace.display(),
                paths.manifest.display()
            ),
            Err(diag) => eprintln!("fault dump failed: {diag} (panic: {detail})"),
        }
        previous(info);
    }));

    eprintln!("running {fault_tick} ticks, then deliberately panicking (P5 demo)");
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        for frame in 0..fault_tick {
            trace.tick_at((frame as u32).saturating_mul(16), &timeline);
        }
        panic!("P5 synthetic fault: deliberate panic for fault-dump demo");
    }));
    if outcome.is_ok() {
        eprintln!("expected a synthetic panic, but none occurred");
        return ExitCode::from(2);
    }

    let dump = match FaultDump::read(&out) {
        Ok(dump) => dump,
        Err(diag) => {
            eprintln!("read fault dump {}: {diag}", out.display());
            return ExitCode::from(2);
        }
    };
    println!(
        "dump: path={} trace_records={} ticks={} fault_tick={:?} fault_frame={} replayable={}",
        dump.path.display(),
        dump.log.len(),
        dump.manifest.ticks.len(),
        dump.manifest.fault_tick,
        dump.manifest.fault_frame,
        dump.manifest.replayable()
    );
    if !dump.manifest.replayable() {
        eprintln!("dump is not replayable from boot (ring attached late?)");
        return ExitCode::from(2);
    }

    let (src, aux) = match open_sources(&root) {
        Ok(sources) => sources,
        Err(detail) => {
            eprintln!("{detail}");
            return ExitCode::from(2);
        }
    };
    match dump.replay(
        Box::new(src),
        Some(Box::new(aux)),
        Vec::new(),
        yu_rt::vm::HostPaths::default(),
    ) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(diag) => {
            eprintln!("offline replay failed: {diag}");
            ExitCode::from(1)
        }
    }
}
