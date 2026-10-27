//! 取证探针:按前缀倾倒图层状态(位置/尺寸/纹理/alpha/可见),用于
//! 「立绘消失 / 抽搐 / 闪烁」类症状的分帧定位。
//!
//! 用法:
//! ```text
//! probe_layers <root> <from> <to> <prefix> [--click f,x,y,interval]
//!              [--px x,y ...] [--backend soft|wgpu]
//! ```
//!
//! - `<from>..<to>`:宿主帧区间(含端点);
//! - `<prefix>`:图层 id 子串(如 `SPL6` 一次覆盖 100 个精灵子层);
//! - `--click f,x,y,interval`:周期性点击(与 `cases/*.json` 的 `inputs`
//!   同语义),用于把场景推进到目标演出;
//! - `--px x,y`:额外打印该点最终屏幕像素(可重复),并给**相邻帧差**
//!   `diff=/changed=`;判断「这一帧立绘到底有没有画出来」用像素,而不是
//!   图层状态 —— 图层可见、纹理也对,仍可能因为采样到空表面而画成背景色。
//!   单帧整层消失一定表现为孤立尖峰,比逐点采样更容易发现;
//! - `--backend`:后端(`soft` 是自建参考实现,`wgpu` 是产品路径);
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
    pixels: Vec<(i64, i64)>,
    backend: Backend,
}

fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.len() < 4 {
        return Err(
            "用法: probe_layers <root> <from> <to> <prefix> [--click f,x,y,interval] [--px x,y] [--backend soft|wgpu]"
                .to_string(),
        );
    }
    let root = argv[0].clone();
    let from: u64 = argv[1].parse().map_err(|_| "from 需要帧号".to_string())?;
    let to: u64 = argv[2].parse().map_err(|_| "to 需要帧号".to_string())?;
    let prefix = argv[3].clone();
    let mut clicks = Vec::new();
    let mut pixels = Vec::new();
    let mut backend = Backend::Wgpu;
    let mut i = 4;
    while i < argv.len() {
        match argv[i].as_str() {
            "--click" => {
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
            "--px" => {
                let spec = argv.get(i + 1).ok_or("--px 缺参数 x,y")?;
                let mut parts = spec.split(',');
                let x: i64 = parts
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--px x 无效")?;
                let y: i64 = parts
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--px y 无效")?;
                pixels.push((x, y));
                i += 2;
            }
            "--backend" => {
                backend = match argv.get(i + 1).ok_or("--backend 缺参数")?.as_str() {
                    "soft" => Backend::Soft,
                    "wgpu" => Backend::Wgpu,
                    other => return Err(format!("--backend 只接受 soft|wgpu,收到 `{other}`")),
                };
                i += 2;
            }
            other => return Err(format!("未知参数 `{other}`")),
        }
    }
    Ok(Args {
        root,
        from,
        to,
        prefix,
        clicks,
        pixels,
        backend,
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
            backend: args.backend,
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

    let mut previous: Option<Vec<u8>> = None;
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
        if let Some(frame) = engine.last_frame() {
            // SKIP 行补上 source:同一 target 可能有多条加工项指向不同源,
            // 只打 target 无法判断是哪一条被跳(`H-SPRITE-PARITY` §5)。
            for pass in &frame.passes {
                for op in &pass.ops {
                    if let yu_gfx::Op::Transition(draw) = op {
                        let id = draw.layer.id.display();
                        if id.contains(&args.prefix) {
                            println!(
                                "  TRANS {id} from={:?} mask={:?} fade={:.4} t={} ta={} from_layer={:?} to_layer={:?} size={:?}",
                                draw.from,
                                draw.to,
                                draw.fade,
                                draw.t,
                                draw.ta,
                                draw.from_layer.as_ref().map(|l| l.display()),
                                draw.to_layer.as_ref().map(|l| l.display()),
                                draw.layer.image_size()
                            );
                        }
                        continue;
                    }
                    if let yu_gfx::Op::Image(image) = op {
                        let target = image
                            .target
                            .as_ref()
                            .map(|t| t.display())
                            .unwrap_or_else(|| format!("<self:{}>", image.source.display()));
                        if target.contains(&args.prefix) || image.source.display().contains(&args.prefix)
                        {
                            println!(
                                "  IMG {} {} -> {} set={:?} region={:?} src_tex={:?} status={:?}",
                                image.action.name(),
                                image.source.display(),
                                target,
                                image.set,
                                (image.x, image.y, image.x2, image.y2),
                                image.source_texture,
                                op.status()
                            );
                        }
                    }
                }
            }
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
        let Some(frame) = engine.last_frame() else {
            continue;
        };
        let mut lines = Vec::new();
        if let Some(state) = engine.gfx_state() {
            for copy in state.copies() {
                let dest = copy.dest.display();
                let src = match &copy.source {
                    yu_gfx::CopySource::Layer(id) => id.display(),
                    yu_gfx::CopySource::Group(id) => format!("group:{}", id.display()),
                };
                if !dest.contains(&args.prefix) && !src.contains(&args.prefix) {
                    continue;
                }
                lines.push(format!(
                    "  COPY {:>4} {} <- {} src_tex={:?} region={:?} status={:?}",
                    copy.id.0, dest, src, copy.source_texture, copy.region, copy.status
                ));
            }
        }
        for layer in &frame.layers {
            let id = layer.id.as_str_lossy();
            if !id.contains(&args.prefix) {
                continue;
            }
            let transition = match &layer.transition {
                Some(t) => format!(" T={} TA={} TID={}", t.t, t.ta, t.tid.as_ref().map(|id| id.display()).unwrap_or_default()),
                None => String::new(),
            };
            lines.push(format!(
                "  {id:<12} E={} A={:<4} Z={:<9} xy=({:.3},{:.3}) size={:?} tex={:?} status={:?}{}",
                u8::from(layer.visible),
                layer.alpha,
                layer.z,
                layer.transform.x,
                layer.transform.y,
                layer.image_size(),
                layer.texture,
                layer.status,
                transition
            ));
        }
        println!("== host_frame {host_frame} engine_frame {}", frame.frame);
        for line in lines {
            println!("{line}");
        }
        if !args.pixels.is_empty() {
            match engine.last_surface() {
                Some(surface) => {
                    // 相邻帧差:单帧「整层消失」在像素上一定是巨大尖峰,
                    // 比逐点采样更容易发现归位/眨眼类的瞬时空白。
                    let mut diff = String::new();
                    if let Some(prev) = &previous {
                        if prev.len() == surface.rgba.len() {
                            let mut sum: u64 = 0;
                            let mut changed: u64 = 0;
                            for (a, b) in prev.chunks_exact(4).zip(surface.rgba.chunks_exact(4)) {
                                let delta = (0..3)
                                    .map(|i| u64::from(a[i].abs_diff(b[i])))
                                    .sum::<u64>();
                                sum += delta;
                                changed += u64::from(delta > 12);
                            }
                            diff = format!(" diff={sum} changed={changed}");
                        }
                    }
                    previous = Some(surface.rgba.clone());
                    let sampled: Vec<String> = args
                        .pixels
                        .iter()
                        .map(|&(x, y)| {
                            let inside = x >= 0
                                && y >= 0
                                && (x as u32) < surface.width
                                && (y as u32) < surface.height;
                            if !inside {
                                return format!("({x},{y})=oob");
                            }
                            let index = ((y as usize) * (surface.width as usize) + x as usize) * 4;
                            let px = &surface.rgba[index..index + 4];
                            format!("({x},{y})={},{},{},{}", px[0], px[1], px[2], px[3])
                        })
                        .collect();
                    println!("  PX {}{diff}", sampled.join(" "));
                }
                None => println!("  PX <no surface>"),
            }
        }
    }
    Ok(())
}
