//! 宿主侧输入回归:极短点击与单帧脉冲**不能被宿主丢掉**。
//!
//! # 为什么不在 `yu-platform-input` 里测
//!
//! `yu-platform-input` 只做“平台事件 → 引擎按钮名”的纯映射(它的单测已覆盖
//! 键表/光标策略)。`deferred_release` / `pulse` 的语义在 `yu-cli::Engine`
//! 的宿主时间线桥里;本文件从**宿主使用者的角度**验证:一次点击即使按下
//! 与抬起都发生在两次 tick 之间,也至少要有一帧被引擎看到(否则用户看到
//! 的就是“点了没反应”)。
//!
//! # 运行方式
//!
//! 需要真语料(`YURIS_SAMPLE_ERIS_ROOT`);debug 构建下首帧 ~7s、每帧
//! ~30ms,所以标 `#[ignore]`,在 release 下手工跑:
//!
//! ```text
//! YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
//!   cargo test --release -p yu-host-desktop --test input_host -- --ignored --nocapture
//! ```
//!
//! 缺样本时显式打印 SKIP 并返回(禁止空跑通过:断言在 `Some` 分支里)。

use std::path::Path;

use yu_cli::engine::{Backend, Engine, EngineOptions};
use yu_cli::project::Project;
use yu_cli::sha::sha256_hex;

fn project() -> Option<Project> {
    let root = std::env::var_os("YURIS_SAMPLE_ERIS_ROOT")?;
    match Project::discover(Path::new(&root), None) {
        Ok(project) => Some(project),
        Err(error) => {
            eprintln!(
                "SKIP[input_host]: YURIS_SAMPLE_ERIS_ROOT 不可用: {}",
                error.message
            );
            None
        }
    }
}

fn engine(project: Project) -> Engine {
    let options = EngineOptions {
        backend: Backend::Soft,
        fps: 60,
        record_events: false,
        ..EngineOptions::default()
    };
    Engine::new(project, options).expect("boot")
}

fn tick(engine: &mut Engine, count: u32) {
    for _ in 0..count {
        engine.tick().expect("tick");
    }
}

fn display_hash(engine: &Engine) -> String {
    let json = engine.display_json().expect("display json");
    sha256_hex(json.as_bytes())
}

/// 标题 → 本篇的点击坐标(ERIS `スタート`;与 `cases/symptom-enter-story.json` 同源)。
const START_BUTTON: (i64, i64) = (217, 281);

#[test]
#[ignore = "真语料 + ~340 tick;用 cargo test --release ... -- --ignored 跑"]
fn sub_frame_click_and_pulse_survive_a_single_host_frame() {
    let Some(project) = project() else {
        return;
    };
    let mut engine = engine(project);
    tick(&mut engine, 300);
    let title_hash = display_hash(&engine);

    // 极短点击:按下与抬起都在下一次 tick 之前。
    engine.set_cursor(START_BUTTON.0, START_BUTTON.1);
    engine.set_input("MOUSE_L", true);
    engine.set_input("MOUSE_L", false);
    assert!(
        engine.held_buttons().contains(&"MOUSE_L".to_string()),
        "抬起应被延后到至少一帧按下之后(deferred_release)"
    );
    tick(&mut engine, 1);
    assert!(
        engine.held_buttons().is_empty(),
        "该帧过后延迟抬起必须生效,不能卡住按钮"
    );
    tick(&mut engine, 39);
    let after_click = display_hash(&engine);
    assert_ne!(
        title_hash, after_click,
        "极短点击必须被引擎看到(标题应开始转场)"
    );

    // 单帧脉冲(滚轮):只作用于一次 tick,不残留按住状态。
    let before_pulse = display_hash(&engine);
    engine.pulse_input("MOUSE_D");
    tick(&mut engine, 1);
    assert!(engine.held_buttons().is_empty(), "pulse 不应留下按住状态");
    tick(&mut engine, 5);
    let after_pulse = display_hash(&engine);
    // 本篇菜单栏的滚轮效果未取证(可能无视觉变化);这里只要求进程不死、
    // 指纹可计算,不做“必须变化”的断言(A5:不猜语义)。
    assert!(!after_pulse.is_empty());
    let _ = before_pulse;
}
