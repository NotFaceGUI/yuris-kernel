//! 取证探针:逐帧观测 `CompositorFrame` 的 pass 增长与「被毒化的图层」。
//!
//! 目的:`CGACT` 图像加工失败时 `cgact_image` 会把 source/target 的
//! `texture_status` 置为 Unimplemented,该层从此**永不绘制**(CG/立绘消失)。
//! 本探针在真实 UI 路径上抓出:哪一层、什么原因、从哪一帧开始。
//!
//! 用法:
//!   `cargo run -p yu-host-desktop --example probe_layer_status -- <project-root>
//!    [--frames N] [--click F,X,Y]... [--watch PREFIX]`
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn main() {
    if let Err(e) = run() {
        eprintln!("probe_layer_status: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args.first().ok_or("用法: probe_layer_status <project-root> [--frames N] [--click F,X,Y]...")?;
    let mut frames = 1500u32;
    let mut clicks: Vec<(u64, i64, i64)> = Vec::new();
    let mut watch = String::new();
    let mut backend = Backend::Soft;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--frames" => {
                frames = args[i + 1].parse().map_err(|_| "frames 需要整数")?;
                i += 2;
            }
            "--click" => {
                let mut p = args[i + 1].split(',');
                let f: u64 = p.next().ok_or("click 缺 frame")?.parse().map_err(|_| "frame")?;
                let x: i64 = p.next().ok_or("click 缺 x")?.parse().map_err(|_| "x")?;
                let y: i64 = p.next().ok_or("click 缺 y")?.parse().map_err(|_| "y")?;
                clicks.push((f, x, y));
                i += 2;
            }
            "--watch" => {
                watch = args[i + 1].clone();
                i += 2;
            }
            "--wgpu" => {
                backend = Backend::Wgpu;
                i += 1;
            }
            other => return Err(format!("未知参数 {other}")),
        }
    }

    let mut timeline = InputTimeline::new();
    clicks.sort_by_key(|c| c.0);
    for &(f, x, y) in &clicks {
        if f > 0 {
            timeline.set(f - 1, InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y));
        }
        timeline.set(f, InputFrame::new(["MOUSE_L"]).with_cursor(x, y));
        timeline.set(f + 1, InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y));
    }

    let project = Project::discover(Path::new(root), None).map_err(|e| e.message)?;
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend,
            record_events: false,
            ..EngineOptions::default()
        },
    )
    .map_err(|e| e.message)?;
    engine.set_timeline(timeline);

    let mut prev_passes = 0usize;
    let mut poisoned: BTreeMap<String, (u64, String)> = BTreeMap::new();
    let mut prev_diag: BTreeSet<String> = BTreeSet::new();
    let mut pass_growth: BTreeMap<u64, usize> = BTreeMap::new();
    let mut pass_ops_seen = 0usize;

    for host_frame in 1..=u64::from(frames) {
        let rendered = engine.tick().map_err(|e| e.message)?;
        let _ = rendered;
        let report = engine.last_report().clone();
        let diags = engine.take_diags();
        let Some(frame) = engine.last_frame() else {
            continue;
        };
        // 1) pass 增长
        let passes = frame.passes.len();
        if passes > prev_passes {
            *pass_growth.entry(u64::from(host_frame)).or_insert(0) += passes - prev_passes;
            if host_frame % 20 == 0 || passes > prev_passes + 1 {
                println!(
                    "f{host_frame}: passes {prev_passes} -> {passes} (+{}) render={:.1}ms ops={}",
                    passes - prev_passes,
                    report.render_ms,
                    report.ops
                );
            }
            prev_passes = passes;
        }
        // 2) 被毒化图层
        for layer in &frame.layers {
            let reason = format!("{:?}", layer.status);
            if !reason.contains("Unimplemented") {
                continue;
            }
            if poisoned.contains_key(&layer.id.display()) {
                continue;
            }
            poisoned.insert(layer.id.display(), (u64::from(host_frame), reason.clone()));
            println!(
                "f{host_frame}: 图层毒化 {} z={} tex={:?} status={reason}",
                layer.id.display(),
                layer.z,
                layer.texture
            );
        }
        // 3) 失败的图像/复制操作(display JSON 里的 passes.op,按 target+原因 去重)
        if let Some(text) = engine.display_json() {
            if let Ok(value) = yu_engine::json::parse(&text) {
                if let Some(passes) = value.get("passes").and_then(|v| v.as_array()) {
                    for pass in passes {
                        let Some(ops) = pass.get("ops").and_then(|v| v.as_array()) else {
                            continue;
                        };
                        for op in ops {
                            let kind = op.get("op").and_then(|v| v.as_str()).unwrap_or("?");
                            if kind != "image" && kind != "copy" {
                                continue;
                            }
                            let status = op
                                .get("status")
                                .map(|v| format!("{v:?}"))
                                .unwrap_or_default();
                            let exec = !status.contains("unimplemented");
                            let action = op
                                .get("action")
                                .and_then(|v| v.as_str())
                                .unwrap_or(kind);
                            let src = op.get("source").map(|v| format!("{v:?}")).unwrap_or_default();
                            let tgt = op.get("target").map(|v| format!("{v:?}")).unwrap_or_default();
                            let key = format!("{kind}/{action} {src} -> {tgt}");
                            if prev_diag.insert(key.clone()) {
                                if exec && pass_ops_seen >= 8 {
                                    continue;
                                }
                                if exec {
                                    pass_ops_seen += 1;
                                }
                                println!(
                                    "f{host_frame}: op[{key}] exec={exec} sx={:?} sy={:?} status={status}",
                                    op.get("sx"), op.get("sy")
                                );
                            }
                        }
                    }
                }
            }
        }
        // 4) 新诊断
        for d in diags {
            let code = d.code.to_string();
            if prev_diag.insert(format!("{code}|{}", d.detail)) {
                let loc = match &d.where_ {
                    yu_log::Loc::Script { file, label, pc, cmd } => format!(
                        "{file}:{}@{pc} {}",
                        label.as_deref().unwrap_or("-"),
                        cmd.as_deref().unwrap_or("-")
                    ),
                    other => format!("{other:?}"),
                };
                println!("f{host_frame}: diag {code} @{loc} {}", d.detail);
            }
        }
        if host_frame % 200 == 0 {
            let imgs = frame
                .layers
                .iter()
                .filter(|l| format!("{:?}", l.status).contains("Unimplemented"))
                .count();
            println!(
                "  ... f{host_frame}: passes={passes} layers={} 毒化层={imgs} 累计毒化={}",
                frame.layers.len(),
                poisoned.len()
            );
        }
    }
    println!("\n== 汇总 ==");
    println!("毒化图层 {} 个:", poisoned.len());
    for (id, (f, reason)) in poisoned.iter().take(60) {
        println!("  f{f:<6} {id}  {reason}");
    }
    if !watch.is_empty() {
        println!("\n== watch `{watch}` ==");
        if let Some(frame) = engine.last_frame() {
            for l in frame.layers.iter().filter(|l| l.id.display().contains(&watch)) {
                println!(
                    "  {} z={} vis={} a={} tex={:?} status={:?} size={:?}",
                    l.id.display(),
                    l.z,
                    l.visible,
                    l.alpha,
                    l.texture,
                    l.status,
                    l.image_size()
                );
            }
        }
    }
    Ok(())
}
