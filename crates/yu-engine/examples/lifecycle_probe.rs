//! 探针:逐帧打印 `Engine::lifecycle()` 的状态迁移。
//!
//! 用法:`cargo run -p yu-engine --example lifecycle_probe -- <project-root> [frames]`
//!
//! 只读;不写盘。用来判定"主任务走完(`Finished`)"是否与"脚本 `END`
//! (`Exited`)"发生在同一时刻 —— 前者是主脚本自然结束,官方**不**据此退出进程。

use std::path::Path;

use yu_engine::engine::{Engine, EngineOptions, Lifecycle};
use yu_engine::project::Project;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("用法: lifecycle_probe <project-root> [frames]");
    let frames: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(600);

    let project = Project::discover(Path::new(&root), None).expect("project");
    let mut engine = Engine::new(project, EngineOptions::default()).expect("engine");

    let mut last = Lifecycle::Running;
    for i in 0..frames {
        match engine.tick() {
            Ok(_) => {}
            Err(e) => {
                println!("frame {i}: tick 失败: {e}");
                return;
            }
        }
        let now = engine.lifecycle();
        if now != last {
            println!("frame {i}: lifecycle {last:?} -> {now:?}");
            last = now;
        }
    }
    println!(
        "final after {frames} frames: lifecycle={:?} main_finished={} restart={}",
        engine.lifecycle(),
        engine.main_finished(),
        engine.restart_requested()
    );
}
