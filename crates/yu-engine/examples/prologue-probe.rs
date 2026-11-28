//! 取证探针:自动点击到剧情正文,命中含指定子串的帧并导出
//! display JSON + PNG。
//!
//! 用法:`prologue-probe <project-root> <frames> <out-dir> <needle>`

use std::path::{Path, PathBuf};

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::patch::{PatchKind, PatchSelection};
use yu_engine::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).expect("project root");
    let frames: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6000);
    let out = PathBuf::from(args.get(3).cloned().unwrap_or_else(|| "target/game-shots".into()));
    let needle = args
        .get(4)
        .expect("needle: 期望命中的 display JSON 子串(必填)");
    std::fs::create_dir_all(&out).expect("create out");

    let project = Project::discover(Path::new(root), None).expect("open project");
    let opts = EngineOptions {
        backend: Backend::Soft,
        patch: PatchSelection::new(PatchKind::Original),
        save_root: Some(PathBuf::from("target/game-save-es")),
        warm_text: true,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts).expect("engine");
    let mut timeline = InputTimeline::new();
    // 警告页/标题/正文:每 120 帧点一次屏幕中心,足够覆盖“按键或点击确认”。
    let mut frame = 1800u64;
    while frame <= frames {
        timeline.set(
            frame,
            InputFrame::new(["MOUSE_L"]).with_cursor(576, 360),
        );
        frame += 120;
    }
    engine.set_timeline(timeline);

    for n in 1..=frames {
        engine.tick().expect("tick");
        if n % 300 == 0 {
            let png = out.join(format!("probe-{n:05}.png"));
            let _ = engine.write_png(&png);
        }
        if n < 1800 || n % 60 != 0 {
            continue;
        }
        if let Some(json) = engine.display_json() {
            if json.contains(needle) {
                let json_path = out.join("prologue.json");
                std::fs::write(&json_path, json.as_bytes()).expect("write json");
                let png = out.join("prologue.png");
                let _ = engine.write_png(&png);
                println!("hit frame={n} json={} png={}", json_path.display(), png.display());
                return;
            }
        }
    }
    println!("no hit after {frames} frames");
}
