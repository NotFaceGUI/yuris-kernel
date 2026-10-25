//! 取证探针:逐帧收集 `RenderReport.skipped` 的**完整**信息
//! (`kind` / `reason` / `target` / 首末帧),并 dump 结束帧的可疑图层。
//!
//! 用途:定位「CG 黑屏」的直接原因 —— 是哪个图层的哪条 op 被跳过,
//! 以及该图层最终的 `texture` / `status` / `alpha` / 尺寸。
//!
//! 用法:
//!   `cargo run -p yu-host-desktop --example probe_skips -- <project-root>
//!    [--frames N] [--click F,X,Y]... [--wgpu] [--dump-layer SUBSTR]`
use std::collections::BTreeMap;
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

#[derive(Default)]
struct SkipStat {
    count: u64,
    first: u64,
    last: u64,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("probe_skips: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args
        .first()
        .ok_or("用法: probe_skips <project-root> [--frames N] [--click F,X,Y]... [--wgpu]")?;
    let mut frames = 2950u32;
    let mut clicks: Vec<(u64, i64, i64)> = Vec::new();
    let mut backend = Backend::Soft;
    let mut dump = String::new();
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
            "--wgpu" => {
                backend = Backend::Wgpu;
                i += 1;
            }
            "--dump-layer" => {
                dump = args[i + 1].clone();
                i += 2;
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

    let mut stats: BTreeMap<(String, String, String), SkipStat> = BTreeMap::new();
    for host_frame in 1..=u64::from(frames) {
        engine.tick().map_err(|e| e.message)?;
        let Some(report) = engine.render_report() else {
            continue;
        };
        for skip in &report.skipped {
            let key = (
                skip.kind.to_string(),
                skip.reason.clone(),
                skip.target.clone(),
            );
            let e = stats.entry(key).or_insert_with(|| SkipStat {
                count: 0,
                first: host_frame,
                last: host_frame,
            });
            e.count += u64::from(skip.count);
            e.last = host_frame;
        }
    }

    println!("== 逐帧 skip 汇总(按 target 分)== backend={:?}", backend);
    let mut rows: Vec<_> = stats.into_iter().collect();
    rows.sort_by(|a, b| b.1.count.cmp(&a.1.count));
    for ((kind, reason, target), s) in rows.iter().take(40) {
        println!(
            "  x{:<7} f{}..f{:<6} {kind}:{reason}\n      target={target}",
            s.count, s.first, s.last
        );
    }
    if rows.is_empty() {
        println!("  (无 skip)");
    }

    // 结束帧:可疑图层全貌(纹理/状态/可见性/尺寸/xy)
    let Some(frame) = engine.last_frame() else {
        return Err("没有帧".into());
    };
    let filter = if dump.is_empty() { "" } else { dump.as_str() };
    println!("\n== 结束帧图层(texture 非 None 或名字含 `{filter}`)==");
    let mut layers: Vec<_> = frame
        .layers
        .iter()
        .filter(|l| !matches!(l.texture, yu_gfx::TextureRef::None))
        .filter(|l| filter.is_empty() || l.id.display().contains(filter))
        .collect();
    layers.sort_by_key(|l| (l.z, l.seq));
    for l in layers.iter().take(60) {
        let tex = match &l.texture {
            yu_gfx::TextureRef::Asset(k) => format!(
                "asset:{}",
                yu_format::Codec::Cp932.decode(k, k).unwrap_or_default()
            ),
            other => format!("{other:?}"),
        };
        println!(
            "  z={:<12} vis={:<5} a={:<4} xy=({},{}) size={:?} status={:?}\n      {} tex={tex}",
            l.z,
            l.visible,
            l.alpha,
            l.transform.x,
            l.transform.y,
            l.image_size(),
            l.status,
            l.id.display()
        );
    }

    // 目标为 SPL 的图像 op:看源是否存在
    println!("
== 图像 op(texture pass,按 target 过滤 SPL)==");
    for pass in &frame.passes {
        for op in &pass.ops {
            if let yu_gfx::Op::Image(i) = op {
                let tgt = i.target.as_ref().map(|t| t.display()).unwrap_or_default();
                if !tgt.contains("SPL") {
                    continue;
                }
                let src_live = frame.layers.iter().any(|l| l.id == i.source);
                let src2 = i
                    .source2
                    .as_ref()
                    .map(|s| {
                        format!(
                            "{} (live={})",
                            s.display(),
                            frame.layers.iter().any(|l| &l.id == s)
                        )
                    })
                    .unwrap_or_default();
                println!(
                    "  {:?} src={} live={} src2={src2} tgt={tgt} sx={:?} sy={:?} status={:?}",
                    i.action, i.source.display(), src_live, i.sx, i.sy, i.status
                );
            }
        }
    }

    // 快照链:图层 `Snapshot(CopyId)` → `Op::Copy` → 源层是否还活着。
    // wgpu 的 `resolve_cpu_source` 用惰性重建,soft 用早期捕获的 snapshots 表;
    // 若 COPY 源层已被 CGEND 删除,惰性链就断在这里。
    println!("\n== 快照链(图层 Snapshot → Copy op → 源层存活)==");
    for pass in &frame.passes {
        for op in &pass.ops {
            let yu_gfx::Op::Copy(c) = op else { continue };
            let src = match &c.source {
                yu_gfx::CopySource::Layer(l) => {
                    let live = frame.layers.iter().any(|x| &x.id == l);
                    format!("Layer {} live={live}", l.display())
                }
                yu_gfx::CopySource::Group(g) => format!("Group {}", g.display()),
            };
            let users: Vec<String> = frame
                .layers
                .iter()
                .filter(|l| matches!(&l.texture, yu_gfx::TextureRef::Snapshot(id) if *id == c.id))
                .map(|l| l.id.display())
                .collect();
            println!(
                "  CopyId({}) dest={} src={src} frozen={} status={:?}\n      users={users:?}",
                c.id.0,
                c.dest.display(),
                c.source_texture.is_some(),
                op.status()
            );
        }
    }

    // 屏幕 pass 里到底画了什么
    println!("\n== 结束帧屏幕 pass ==");
    for pass in &frame.passes {
        if !matches!(pass.target, yu_gfx::PassTarget::Screen) {
            continue;
        }
        println!("  ops={}", pass.ops.len());
        for op in &pass.ops {
            match op {
                yu_gfx::Op::Layer(l) => println!(
                    "    layer {} vis={} a={} xy=({},{}) size={:?} tex={:?} status={:?}",
                    l.id.display(),
                    l.visible,
                    l.alpha,
                    l.transform.x,
                    l.transform.y,
                    l.image_size(),
                    l.texture,
                    l.status
                ),
                yu_gfx::Op::Transition(t) => println!(
                    "    transition {} from={:?} to={:?} fade={} status={:?}",
                    t.layer.id.display(),
                    t.from,
                    t.to,
                    t.fade,
                    t.layer.status
                ),
                yu_gfx::Op::Group(g) => println!("    group {}", g.layer.id.display()),
                yu_gfx::Op::Mask(m) => println!("    mask {}", m.layer.id.display()),
                yu_gfx::Op::Copy(c) => println!("    copy -> {}", c.dest.display()),
                yu_gfx::Op::Image(i) => println!(
                    "    image {:?} {} -> {}",
                    i.action,
                    i.source.display(),
                    i.target.as_ref().map(|t| t.display()).unwrap_or_default()
                ),
            }
        }
    }
    Ok(())
}
