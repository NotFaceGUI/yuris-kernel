//! 可赋值系统变量写入的语料回归:标题点击/推进时 `es_Button` 写
//! `@_INP(...)=2`,不得再被当成「写入语义未取证」挂号(DIV-RT-007)。
//!
//! 语料事实:
//! - `ERIS/Button/es_Button.yst:3417` 每次检测到按下都执行
//!   `@_INP(<button code>) = 2`(消费按下,避免同帧重复触发);
//! - `ERIS/Sprite/es_Sprite.yst:851..854` 用 `_INP(...)=11` 限制连发;
//! - `ERIS/Text/es_Text.yst:1287`、`es_TXExec.yst:9/119` 写
//!   `@_KEYLAST=0`/`@_MOUSELAST=0` 取消 skip。
//!
//! REA 0.495 写分派:`_INP`(case 0x90)→ `0x446efc` → `0x447014`
//! (n>0,按物理状态钳制);`_KEYLAST`/`_MOUSELAST`(0x91/0x92)直接写
//! `DAT_00801908/0x80190c`。
//!
//! 运行:`YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo test -p yu-cli
//! --test sysvar_write -- --ignored --nocapture`。

use std::path::PathBuf;

use yu_cli::engine::{Engine, EngineOptions};
use yu_cli::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn sample_root() -> Option<PathBuf> {
    match std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        Ok(root) => Some(PathBuf::from(root)),
        Err(_) => {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_ROOT is missing");
            }
            eprintln!("SKIP sysvar_write: no sample root");
            None
        }
    }
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn click_and_advance_do_not_report_sysvar_write() {
    let Some(root) = sample_root() else { return };
    let project = Project::discover(&root, None).expect("open project");
    let mut engine = Engine::new(project, EngineOptions::default()).expect("engine");

    // 标题 スタート → 本篇正文点击推进。正文点击会走 `es_Button` 的
    // 按下消费(`@_INP(...)=2`),即用户报「点击/推进就打印」的那条路径。
    let mut timeline = InputTimeline::new();
    for (frame, x, y) in [(300u64, 217i64, 281i64), (440, 400, 300), (520, 400, 300)] {
        timeline.set(frame, InputFrame::new(["MOUSE_L"]).with_cursor(x, y));
        timeline.set(
            frame + 1,
            InputFrame::new(Vec::<String>::new()).with_cursor(x, y),
        );
    }
    engine.set_timeline(timeline);
    for _ in 0..620 {
        engine.tick().expect("tick");
    }

    let diags = engine.take_diags();
    let hits: Vec<String> = diags
        .iter()
        .filter(|d| d.code.contains("sysvar.write"))
        .map(|d| d.detail.clone())
        .collect();
    assert!(
        hits.is_empty(),
        "点击/推进触发的可赋值系统变量写入不得挂号: {hits:?}"
    );
}
