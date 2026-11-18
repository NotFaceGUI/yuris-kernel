//! 取证探针:逐 tick 打印 VM 当前命令落点与影片后端状态。
//!
//! 用法:`probe_vm <project-root> [frames] [entry]`
//!
//! 环境变量:
//! - `PROBE_VM_LIST=1` 列出脚本号/路径(默认只列 scenario/movie/first,
//!   `PROBE_VM_MATCH=<子串>` 改过滤);
//! - `PROBE_VM_WAIT=1` 每帧打印调度器等待任务(脚本/pc/等待帧/文本)。
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let entry = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "SYSTEM_START".into());
    let project = Project::discover(Path::new(&root), None).expect("project");
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend: Backend::Wgpu,
            entry,
            record_events: false,
            ..EngineOptions::default()
        },
    )
    .expect("engine");
    if std::env::var_os("PROBE_VM_LIST").is_some() {
        let needle = std::env::var("PROBE_VM_MATCH").unwrap_or_default();
        for id in 0..4096u32 {
            let name = engine.script_name(id);
            let hit = if needle.is_empty() {
                name.contains("es_scenario")
                    || name.contains("es_movie")
                    || name.contains("es_first")
            } else {
                name.to_lowercase().contains(&needle.to_lowercase())
            };
            if hit {
                println!("sid={id} {name}");
            }
        }
    }
    for frame in 1..=frames {
        let rendered = engine.tick().expect("tick");
        if std::env::var_os("PROBE_VM_WAIT").is_some() {
            for task in engine.take_diag_report().waiting_tasks {
                println!(
                    "wait f={frame} id={} s{} pc{} frames={} deadline={:?} text={} main={}",
                    task.id,
                    task.script,
                    task.pc,
                    task.wait_frames,
                    task.deadline_ms,
                    task.text_rest,
                    task.main
                );
            }
        }
        if frame % 10 == 0 || frame <= 3 {
            let report = &rendered.report;
            let movie = engine.movie_backend_active();
            let pos = engine.current_position();
            println!(
                "f={frame} engine={} ops={}/{} layers={} vis={} movie={} pos={:?}",
                report.frame,
                report.executed,
                report.ops,
                report.layers,
                report.visible,
                movie,
                pos
            );
        }
    }
}
