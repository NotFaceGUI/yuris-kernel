//! E2E 计时:boot + N 帧墙钟。
use std::time::Instant;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let t0 = Instant::now();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        record_events: std::env::args().nth(3).map(|v| v != "0").unwrap_or(true),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    let boot = t0.elapsed();
    let t1 = Instant::now();
    let mut total = 0u64;
    for f in 0..frames {
        let r = vm.tick_at(f * 16, &yu_rt::input::InputTimeline::new());
        total += r.commands_pumped;
    }
    let run = t1.elapsed();
    println!(
        "boot={boot:?} frames={frames} cmds={total} run={run:?} ({:.0} cmds/s)",
        total as f64 / run.as_secs_f64()
    );
}
