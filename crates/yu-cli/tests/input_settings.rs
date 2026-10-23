//! `INPUT` / `MOUSE` 启动设置的语料回归。
//!
//! `es_First.yst` 启动链写:
//! ```text
//! INPUT[MOUSE=1 KEY=1 PAD=@T(17,09)]
//! MOUSE[MODE=1 TIME=2500 CLICK=0 WHEEL=0]
//! ```
//! 内核必须把这些值落到脚本可见的输入设置上(REA `0x446564`/`0x44aaa0`)。
//!
//! 需要真实语料:
//! `YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo test -p yu-cli --test input_settings -- --ignored --nocapture`

use std::path::PathBuf;

use yu_cli::engine::{Engine, EngineOptions};
use yu_cli::project::Project;

fn sample_root() -> Option<PathBuf> {
    match std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        Ok(root) => Some(PathBuf::from(root)),
        Err(_) => {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_ROOT is missing");
            }
            eprintln!("SKIP startup_input_settings_match_es_first: no sample root");
            None
        }
    }
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn startup_input_settings_match_es_first() {
    let Some(root) = sample_root() else {
        return;
    };
    let project = Project::discover(&root, None).expect("project");
    let mut engine = Engine::new(
        project,
        EngineOptions {
            record_events: false,
            ..EngineOptions::default()
        },
    )
    .expect("engine");
    // 启动链在首帧执行;推进一帧让 INPUT/MOUSE 落地。
    engine.tick().expect("tick");
    let settings = engine.input_settings().expect("settings");
    assert!(settings.keyboard, "INPUT[KEY=1]");
    assert!(settings.mouse, "INPUT[MOUSE=1]");
    assert_eq!(settings.mouse_mode, 1, "MOUSE[MODE=1]");
    assert_eq!(settings.mouse_time_ms, 2500, "MOUSE[TIME=2500]");
    assert!(settings.move_reactive, "MOVE 缺省 1");
    assert!(!settings.click_reactive, "MOUSE[CLICK=0]");
    assert!(!settings.wheel_reactive, "MOUSE[WHEEL=0]");
}
