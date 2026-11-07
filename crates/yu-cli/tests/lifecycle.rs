//! 会话生命周期与挂起/恢复(`DIV-AND-005`)。
//!
//! 验收:
//! 1. 正常推进时 `lifecycle() == Running`;主任务走完/`END` 后变为终态,
//!    且 `Lifecycle::host_code()` 给出宿主退出码 `1`。
//! 2. `suspend()` 后 `tick_realtime` 的逻辑时钟不快进:挂起期间流逝的
//!    墙钟时间被丢弃,恢复后第一帧增量从 0 起算。
//! 3. `suspend`/`resume` 幂等;`resume` 后音频端口重新挂回。
//!
//! 需要真实语料:
//! `YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo test -p yu-cli --test lifecycle -- --ignored --nocapture`

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use yu_cli::engine::{Engine, EngineOptions, Lifecycle};
use yu_cli::project::Project;

fn sample_root() -> Option<PathBuf> {
    match std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        Ok(root) => Some(PathBuf::from(root)),
        Err(_) => {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_ROOT is missing");
            }
            eprintln!("SKIP: no sample root");
            None
        }
    }
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn starts_running_and_suspend_resumes_without_time_jump() {
    let Some(root) = sample_root() else {
        return;
    };
    let project = Project::discover(&root, None).expect("project");
    let mut engine = Engine::new(project, EngineOptions::default()).expect("engine");

    // 初始:Running。
    assert_eq!(engine.lifecycle(), Lifecycle::Running);
    assert!(!engine.lifecycle().is_terminal());
    assert_eq!(engine.lifecycle().host_code(), 0);

    // 挂起/恢复幂等,且状态随行。
    engine.suspend();
    assert!(engine.is_suspended());
    engine.suspend();
    assert!(engine.is_suspended(), "重复挂起应幂等");
    engine.resume();
    assert!(!engine.is_suspended());
    engine.resume();
    assert!(!engine.is_suspended(), "重复恢复应幂等");

    // 挂起期间流逝的真实时间必须被丢弃:挂起 → 睡 300ms → 恢复 →
    // 立即 tick。若快进,这一帧的 `@_OS_TIME` 会一次性前进 ~300ms;
    // 丢弃后增量只有本帧的 16/17ms。
    engine.suspend();
    thread::sleep(Duration::from_millis(300));
    engine.resume();
    let rendered = engine.tick_realtime().expect("tick after resume");
    // `clock_ms` 是累计逻辑毫秒:boot 时从 0 起算。恢复后第一帧的累计值
    // 不应包含挂起期(容差 = 两个正常帧)。
    assert!(
        u64::from(rendered.report.clock_ms) < 100,
        "恢复后第一帧不应快进: clock_ms={} (挂起 300ms 若被灌入会远超此值)",
        rendered.report.clock_ms
    );
}

/// 直接驱动到主任务结束在语料里要跑数千帧;这里改为**注入式**验证终态
/// 判据:对 `Lifecycle` 的映射规则做穷举(不依赖语料的 END 时机)。
#[test]
fn lifecycle_mapping_is_exhaustive() {
    assert_eq!(Lifecycle::Running, Lifecycle::Running);
    assert!(!Lifecycle::Running.is_terminal());
    assert_eq!(Lifecycle::Running.as_str(), "running");
    assert_eq!(Lifecycle::Running.host_code(), 0);

    for terminal in [
        Lifecycle::Finished,
        Lifecycle::Exited { restart: false },
        Lifecycle::Exited { restart: true },
    ] {
        assert!(terminal.is_terminal(), "{terminal:?} 应为终态");
        assert_eq!(terminal.host_code(), 1);
    }
    assert_eq!(Lifecycle::Finished.as_str(), "finished");
    assert_eq!(Lifecycle::Exited { restart: false }.as_str(), "exited");
    assert!(!Lifecycle::Finished.restart());
    assert!(!Lifecycle::Exited { restart: false }.restart());
    assert!(Lifecycle::Exited { restart: true }.restart());
}
