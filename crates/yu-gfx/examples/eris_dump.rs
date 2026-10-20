//! 把 ERIS 首屏事件流喂给 `GfxState`,打印合成契约摘要。
//!
//! 用法:
//!   YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo run -p yu-gfx --example eris_dump -- 1
use std::collections::BTreeMap;

use yu_gfx::{json, GfxConfig, GfxState, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn eris_root() -> String {
    // 唯一约定:工程数据入口是 <工程根>/data;ysbin 位于 <工程根>/data/ysbin。
    std::env::var("YURIS_SAMPLE_ERIS_ROOT")
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").map(|p| {
                std::path::Path::new(&p)
                    .ancestors()
                    .nth(2)
                    .expect("ysbin/../..")
                    .display()
                    .to_string()
            })
        })
        .expect("set YURIS_SAMPLE_ERIS_ROOT or YURIS_SAMPLE_ERIS_YSBIN")
}

fn checksum(s: &str) -> u64 {
    let mut h = 1469598103934665603u64;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn main() {
    let root = eris_root();
    let frames: u32 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    // 数据入口两种合法形态:封包 data.ypf 优先,散盘 data/ 为备。
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    };
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        record_events: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let mut gfx = GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).expect("gfx");
    let timeline = InputTimeline::new();
    let mut events = Vec::new();

    for f in 0..frames {
        let r = vm.tick_at(f * 16, &timeline);
        events.clear();
        vm.drain_events(&mut events);
        gfx.consume_events(&events);
        let frame = gfx.frame();
        let display = json::to_display_json(&frame);
        let full = json::to_json(&frame);
        if let Ok(path) = std::env::var("GFX_DUMP") {
            std::fs::write(&path, &full).expect("write full json");
            println!("  copy log ({}):", gfx.copies().len());
            for c in gfx.copies() {
                println!("    copy {} -> {}", c.id.0, c.dest.display());
            }
        }
        let mut diag_codes: BTreeMap<&str, usize> = BTreeMap::new();
        for d in &frame.diagnostics {
            *diag_codes.entry(d.code.as_str()).or_insert(0) += 1;
        }
        let mut ops = 0usize;
        for p in &frame.passes {
            ops += p.ops.len();
        }
        println!(
            "frame {f}: cmds={} errors={} events={} layers={} passes={} ops={} text={} diags={} display_len={} display_fnv={:016x} full_len={} full_fnv={:016x}",
            r.commands_pumped,
            r.errors.len(),
            events.len(),
            frame.layers.len(),
            frame.passes.len(),
            ops,
            frame.text.len(),
            frame.diagnostics.len(),
            display.len(),
            checksum(&display),
            full.len(),
            checksum(&full),
        );
        for (code, n) in &diag_codes {
            println!("  diag {n:6}  {code}");
        }
        let visible = frame.layers.iter().filter(|l| l.visible).count();
        let groups = frame.layers.iter().filter(|l| l.is_group()).count();
        println!(
            "  visible={visible} groups={groups} viewport={}x{} filter={}/{}/{} probe={:?}",
            frame.viewport.width,
            frame.viewport.height,
            frame.filter.mag.as_str(),
            frame.filter.min.as_str(),
            frame.filter.mip.as_str(),
            frame
                .image_probe_order
                .iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
        );
        for p in &frame.passes {
            println!("  pass {} ops={}", p.target.name(), p.ops.len());
        }
        if f == 0 || f + 1 == frames {
            println!("  visible layers:");
            for l in frame.layers.iter().filter(|l| l.visible).take(40) {
                println!(
                    "    {} z={} seq={} A={} mode={} group={} tex={:?} status={:?}",
                    l.id.display(),
                    l.z,
                    l.seq,
                    l.alpha,
                    l.blend.name(),
                    l.is_group(),
                    l.texture,
                    l.status
                );
            }
        }
    }
}
