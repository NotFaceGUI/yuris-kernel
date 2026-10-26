//! 取证探针:按前缀倾倒图层状态(位置/尺寸/纹理/alpha/可见),用于
//! 「立绘消失 / 抽搐 / 闪烁」类症状的分帧定位。
//!
//! 用法:
//! ```text
//! probe_layers <root> <from> <to> <prefix> [--click f,x,y,interval]
//! ```
//!
//! - `<from>..<to>`:宿主帧区间(含端点);
//! - `<prefix>`:图层 id 子串(如 `SPL6` 一次覆盖 100 个精灵子层);
//! - `--click f,x,y,interval`:周期性点击(与 `cases/*.json` 的 `inputs`
//!   同语义),用于把场景推进到目标演出;
//! - 环境变量 `YU_SAMPLE_STEP` 控制采样步长(默认 1 = 每帧)。
//!
//! 只读 `Engine` 的公开 API,不改引擎语义。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn main() {
    if let Err(e) = run() {
        eprintln!("probe_layers: {e}");
        std::process::exit(2);
    }
}

struct Args {
    root: String,
    from: u64,
    to: u64,
    prefix: String,
    clicks: Vec<(u64, i64, i64)>,
}

fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.len() < 4 {
        return Err(
            "用法: probe_layers <root> <from> <to> <prefix> [--click f,x,y,interval]".to_string(),
        );
    }
    let root = argv[0].clone();
    let from: u64 = argv[1].parse().map_err(|_| "from 需要帧号".to_string())?;
    let to: u64 = argv[2].parse().map_err(|_| "to 需要帧号".to_string())?;
    let prefix = argv[3].clone();
    let mut clicks = Vec::new();
    let mut i = 4;
    while i < argv.len() {
        if argv[i] != "--click" {
            return Err(format!("未知参数 `{}`", argv[i]));
        }
        let spec = argv.get(i + 1).ok_or("--click 缺参数 f,x,y,interval")?;
        let mut parts = spec.split(',');
        let frame: u64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or("--click 帧号无效")?;
        let x: i64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or("--click x 无效")?;
        let y: i64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or("--click y 无效")?;
        let interval: u64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut f = frame;
        while f <= to {
            clicks.push((f, x, y));
            if interval == 0 {
                break;
            }
            f += interval;
        }
        i += 2;
    }
    Ok(Args {
        root,
        from,
        to,
        prefix,
        clicks,
    })
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

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let project = Project::discover(Path::new(&args.root), None).map_err(|e| e.message)?;
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend: Backend::Wgpu,
            entry: "SYSTEM_START".to_string(),
            ..EngineOptions::default()
        },
    )
    .map_err(|e| e.message)?;
    engine.set_timeline(timeline(&args.clicks, args.to));

    let step: u64 = std::env::var("YU_SAMPLE_STEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);

    for host_frame in 1..=args.to {
        engine.tick().map_err(|e| e.message)?;
        let diags = engine.take_diags();
        if host_frame < args.from || (host_frame % step != 0 && host_frame != args.to) {
            continue;
        }
        for diag in &diags {
            println!(
                "  DIAG {} {} {:?} {}",
                diag.severity.as_str(),
                diag.code,
                diag.where_,
                diag.detail
            );
        }
        if let Some(report) = engine.render_report() {
            if host_frame == args.to || host_frame % step == 0 {
                println!("  report: ops={} skipped={}", report.ops_total, report.skipped_ops_total);
            }
            for skip in &report.skipped {
                println!(
                    "  SKIP {} {} {} x{}",
                    skip.kind, skip.reason, skip.target, skip.count
                );
            }
        }
        if let Some(state) = engine.gfx_state() {
            for image in state.images() {
                let t = image
                    .target
                    .as_ref()
                    .map(|t| t.display())
                    .unwrap_or_else(|| "<self>".into());
                if t.contains("SPLBUF602") || t.contains("sp.tmpB") || image.source.display().contains("sp.tmpB") {
                    println!(
                        "  IMG-LOG {} {} -> {} status={:?} set={:?} region={:?}",
                        image.action.name(), image.source.display(), t, image.status, image.set,
                        (image.x, image.y, image.x2, image.y2)
                    );
                }
            }
            for copy in state.copies() {
                if (340..=352).contains(&copy.id.0) {
                    println!(
                        "  COPY-LOG {} <- {:?} src_tex={:?} status={:?} region={:?}",
                        copy.dest.display(), copy.source, copy.source_texture, copy.status, copy.region
                    );
                }
            }
        }
        let Some(frame) = engine.last_frame() else {
            continue;
        };
        let mut lines = Vec::new();
        for layer in &frame.layers {
            let id = layer.id.as_str_lossy();
            if !id.contains(&args.prefix) {
                continue;
            }
            lines.push(format!(
                "  {id:<12} E={} A={:<4} Z={:<9} xy=({:.3},{:.3}) size={:?} tex={:?} status={:?}",
                u8::from(layer.visible),
                layer.alpha,
                layer.z,
                layer.transform.x,
                layer.transform.y,
                layer.image_size(),
                layer.texture,
                layer.status
            ));
        }
        println!("== host_frame {host_frame} engine_frame {}", frame.frame);
        for line in lines {
            println!("{line}");
        }
    }
    Ok(())
}
