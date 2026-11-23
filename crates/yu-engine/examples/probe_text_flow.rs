//! 临时探针:逐帧观察正文文本状态与 `CGACT[TEXT]` 绘制项的生命周期。
//! 用法:`probe_text_flow <project-root> [frames]`
//!
//! 配合 `YU_CSEL_SKIP=1`(Engine 内置,1500 帧写 CSEL 槽)与固定点点击,
//! 目标帧段进入正文后观察 `text_draws` 是累积还是被清空。
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::patch::PatchSelection;
use yu_engine::project::Project;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(2600);
    let patch = std::env::var("PROBE_PATCH").unwrap_or_else(|_| "big5".to_string());
    let project = Project::discover(Path::new(&root), None).expect("project");
    let opts = EngineOptions {
        backend: Backend::Soft,
        patch: PatchSelection::parse(&patch).expect("patch"),
        // 可选:PROBE_TRANSLATE=<translate.json> 与 CLI `--translate` 同义。
        translate: std::env::var("PROBE_TRANSLATE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(std::path::PathBuf::from),
        ..Default::default()
    };
    let mut engine = Engine::new(project, opts).expect("engine");
    let mut prev_draws = usize::MAX;
    for f in 0..frames {
        // CSEL/标题按钮:按层名定位可见按钮层,点其几何中心。
        if let Some(frame) = engine.last_frame() {
            let mut button = None;
            for layer in frame.layers.iter() {
                let id = layer.id.display();
                if !layer.visible {
                    continue;
                }
                if id.contains("CSEL") || id.contains("TITLE.BTN.START") {
                    let size = match layer.kind {
                        yu_gfx::LayerKind::Image { size } => size,
                        yu_gfx::LayerKind::Group { size } => size,
                    };
                    let (w, h) = size
                        .map(|s| (f64::from(s.width), f64::from(s.height)))
                        .unwrap_or((0.0, 0.0));
                    button = Some((
                        layer.transform.x + w / 2.0,
                        layer.transform.y + h / 2.0,
                    ));
                    if f % 30 == 0 {
                        println!("btn f{f} {id} at ({}, {}) size={size:?}", layer.transform.x, layer.transform.y);
                    }
                    break;
                }
            }
            if let (Some((x, y)), true) = (button, f % 25 == 0) {
                engine.set_cursor(x as i64, y as i64);
                engine.pulse_input("MOUSE_L");
            }
        }
        if f >= 20 && f % 40 == 0 && engine.last_frame().is_none() {
            let (x, y) = match (f / 40) % 3 {
                0 => (576, 360),
                1 => (576, 520),
                _ => (576, 620),
            };
            engine.set_cursor(x, y);
            engine.pulse_input("MOUSE_L");
        }
        if let Err(e) = engine.tick() {
            eprintln!("ERR f{f}: {e:?}");
            break;
        }
        let Some(frame) = engine.last_frame() else {
            continue;
        };
        let draws = frame.text_draws.len();
        let (buf_len, flags_len) = match engine.gfx_state() {
            Some(g) => {
                let (b, fl, _, _) = g.text_state();
                (b.len(), fl.map(<[u8]>::len))
            }
            None => (0, None),
        };
        let changed = draws != prev_draws;
        if changed || f % 60 == 0 {
            let layout = frame.text_layout.as_ref().map(|l| {
                (
                    l.chars,
                    l.target.as_ref().map(yu_gfx::LayerId::display),
                )
            });
            println!(
                "f{f} draws={draws} buf={buf_len} flags={flags_len:?} text_runs={} layout={layout:?}",
                frame.text.len()
            );
            for d in frame.text_draws.iter().take(6) {
                println!(
                    "   draw target={} bytes={} x={} y={} set={}",
                    d.target.display(),
                    String::from_utf8_lossy(&d.bytes),
                    d.x,
                    d.y,
                    d.set
                );
            }
            if draws > 6 {
                println!("   ... +{} draws", draws - 6);
            }
        }
        prev_draws = draws;
    }
    if let Some(stats) = engine.translation_stats() {
        println!(
            "translate: word {}/{} scen {}/{} units={} calls {}/{} rewrite_errors={}",
            stats.word_hits,
            stats.word_misses,
            stats.scenario_line_hits,
            stats.scenario_line_misses,
            stats.scenario_units,
            stats.call_hits,
            stats.call_misses,
            stats.scenario_rewrite_errors
        );
    }
    println!("done frames={frames}");
}
