//! 真实 ERIS 语料上的声音域普查：装载/解码/推进/查询路径。
//!
//! 用法：
//! ```bash
//! YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
//!   cargo run -p yu-cli --example sound_census -- 300
//! ```
//! 缺样本时显式 SKIP，不空跑通过。

use yu_cli::engine::{Engine, EngineOptions};
use yu_cli::project::Project;

fn main() {
    let Ok(root) = std::env::var("YURIS_SAMPLE_ERIS_ROOT") else {
        eprintln!("SKIP sound_census: YURIS_SAMPLE_ERIS_ROOT not set");
        return;
    };
    let frames: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let project = Project::discover(std::path::Path::new(&root), None).expect("open project");
    let opts = EngineOptions {
        os_sound: true,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts).expect("engine");
    for _ in 0..frames {
        if let Err(e) = engine.tick() {
            eprintln!("tick failed: {e}");
            break;
        }
    }
    let snap = engine.sound_snapshot();
    println!("frames={frames} layers={}", snap.len());
    for l in &snap {
        println!(
            "id={} file={} state={:?} time={} total={} loop={} count={} vol={} pan={} pitch={} e={} pause={}",
            String::from_utf8_lossy(&l.id),
            String::from_utf8_lossy(&l.file),
            l.state,
            l.time_ms,
            l.total_ms,
            l.loop_setting,
            l.loop_count,
            l.volume,
            l.pan,
            l.pitch,
            l.enabled,
            l.paused
        );
    }
    let diags = engine.take_sound_diags();
    println!("snd_diags={}", diags.len());
    for d in diags.iter().take(20) {
        println!("diag {} {}", d.code, d.detail);
    }
}
