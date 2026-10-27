//! 临时取证探针:按输入时间线跑引擎,逐帧打印命令调用点(script/pc/行)
//! 与关键图层状态,用于定位「读档确认后卡在对话框」。
//!
//! 用法:
//! ```text
//! probe_flow <root> <to> <f,x,y;f,x,y;...> [站点帧列表: 600,650,700]
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn parse_clicks(spec: &str) -> Result<Vec<(u64, i64, i64)>, String> {
    let mut out = Vec::new();
    for part in spec.split(';').filter(|s| !s.is_empty()) {
        let mut it = part.split(',');
        let f: u64 = it.next().and_then(|v| v.parse().ok()).ok_or("click frame")?;
        let x: i64 = it.next().and_then(|v| v.parse().ok()).ok_or("click x")?;
        let y: i64 = it.next().and_then(|v| v.parse().ok()).ok_or("click y")?;
        out.push((f, x, y));
    }
    Ok(out)
}

fn timeline(clicks: &[(u64, i64, i64)], to: u64) -> InputTimeline {
    let mut presses: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    let mut cursors: BTreeMap<u64, (i64, i64)> = BTreeMap::new();
    for (frame, x, y) in clicks {
        presses.entry(*frame).or_default().insert("MOUSE_L".to_string());
        presses.entry(frame + 1).or_default().remove("MOUSE_L");
        cursors.insert(*frame, (*x, *y));
    }
    let mut frames: BTreeSet<u64> = presses.keys().copied().collect();
    frames.extend(cursors.keys().copied());
    let mut timeline = InputTimeline::new();
    let mut held: BTreeSet<String> = BTreeSet::new();
    let mut cursor = (0i64, 0i64);
    for frame in frames.into_iter().filter(|f| *f <= to) {
        if let Some(set) = presses.get(&frame) {
            held = set.clone();
        }
        if let Some(&(x, y)) = cursors.get(&frame) {
            cursor = (x, y);
        }
        timeline.set(
            frame,
            InputFrame::new(held.iter().cloned()).with_cursor(cursor.0, cursor.1),
        );
    }
    timeline
}

fn main() {
    if let Err(e) = run() {
        eprintln!("probe_flow: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.len() < 3 {
        return Err("用法: probe_flow <root> <to> <f,x,y;...> [站点帧: 600,650]".into());
    }
    let root = &argv[0];
    let to: u64 = argv[1].parse().map_err(|_| "to 需要帧号")?;
    let clicks = parse_clicks(&argv[2])?;
    let site_frames: BTreeSet<u64> = argv
        .get(3)
        .map(|s| s.split(',').filter_map(|v| v.trim().parse().ok()).collect())
        .unwrap_or_default();

    let project = Project::discover(Path::new(root), None).map_err(|e| e.message)?;
    let backend = match std::env::var("PROBE_BACKEND").as_deref() {
        Ok("wgpu") => Backend::Wgpu,
        _ => Backend::Soft,
    };
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend,
            entry: "SYSTEM_START".to_string(),
            ..EngineOptions::default()
        },
    )
    .map_err(|e| e.message)?;
    engine.set_timeline(timeline(&clicks, to));
    engine.set_command_profile(true);

    let shot = std::env::var("PROBE_SHOT").ok();
    for host_frame in 1..=to {
        engine.reset_command_profile();
        engine.tick().map_err(|e| e.message)?;
        let sites = engine.take_profile_sites(12);
        let varact = engine.take_varact_sites(3);
        let diags = engine.take_diags();
        if !site_frames.is_empty() && !site_frames.contains(&host_frame) {
            continue;
        }
        println!(
            "== hf={host_frame} ef={} ops={} skip={}",
            engine.frame(),
            engine.render_report().map(|r| r.ops_total).unwrap_or(0),
            engine.render_report().map(|r| r.skipped_ops_total).unwrap_or(0)
        );
        for d in &diags {
            println!("  DIAG {} {} {:?} {}", d.severity.as_str(), d.code, d.where_, d.detail);
        }
        for d in engine.pending_dialogs() {
            println!(
                "  DIALOG id={} buttons={:?} caption={:?} text={:?}",
                d.id, d.buttons, d.caption, d.text
            );
        }
        if !sites.is_empty() {
            for (op, script, pc, hits) in &sites {
                let name = engine.script_name(*script);
                let line = engine
                    .script_position(*script, *pc)
                    .map(|l| l.to_string())
                    .unwrap_or_else(|| "?".into());
                println!("  SITE {hits:>6}x {op:<10} {name} pc={pc} line={line}");
            }
        }
        for ((script, pc), hits) in &varact {
            let name = engine.script_name(*script);
            println!("  VARIACT {hits:>5}x {name} pc={pc}");
        }
        if let Some(frame) = engine.last_frame() {
            let mut lines: Vec<String> = Vec::new();
            for layer in &frame.layers {
                let id = layer.id.as_str_lossy();
                if !(id.contains("LOADCONF") || id.contains("SAVECONF")) {
                    continue;
                }
                lines.push(format!(
                    "  LAYER {id} E={} A={} xy=({:.0},{:.0}) size={:?}",
                    u8::from(layer.visible),
                    layer.alpha,
                    layer.transform.x,
                    layer.transform.y,
                    layer.image_size()
                ));
            }
            lines.sort();
            for l in lines {
                println!("{l}");
            }
            if std::env::var_os("PROBE_ALL_LAYERS").is_some() {
                let visible: Vec<&yu_gfx::Layer> =
                    frame.layers.iter().filter(|l| l.visible).collect();
                println!("  visible_total={}", visible.len());
                for layer in visible.iter().take(400) {
                    let id = layer.id.as_str_lossy();
                    if id.contains("DIALOG") || id.contains("LOADCONF") || id.contains("SAVE") {
                        println!("  VL {id} E=1");
                    }
                }
            }
        }
    }
    if let Some(frame) = engine.last_frame() {
        let visible = frame.layers.iter().filter(|l| l.visible).count();
        let conf = frame
            .layers
            .iter()
            .filter(|l| l.id.as_str_lossy().contains("LOADCONF"))
            .count();
        let ops: usize = frame.passes.iter().map(|p| p.ops.len()).sum();
        println!(
            "end-state: frame={} layers={} visible={} loadconf={} ops={}",
            frame.frame,
            frame.layers.len(),
            visible,
            conf,
            ops
        );
    }
    if let Some(surface) = engine.last_surface() {
        let mut h: u64 = 1469598103934665603;
        for b in &surface.rgba {
            h ^= u64::from(*b);
            h = h.wrapping_mul(1099511628211);
        }
        println!("end-surface: {}x{} fp={h:016x}", surface.width, surface.height);
    }
    if let Some(path) = shot {
        engine
            .write_png(std::path::Path::new(&path))
            .map_err(|e| e.message)?;
        println!("shot: {path}");
    }
    Ok(())
}
