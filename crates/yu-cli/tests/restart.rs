//! 重启语义回归(`DIV-HOST-001`)。
//!
//! 验收:跑到含 COPY 的帧 → `Engine::restart()` → 再跑到同一帧,
//! 两次的屏幕 RGBA 与 display JSON 必须逐字节相同。
//!
//! 背景:`GfxState.copy_seq` 在重启时归零,新 `CopyId` 从头分配;若渲染器
//! 保留旧快照,新复制会命中同 id 的旧纹理(冻结早退)并渲染出重启前的像素。
//!
//! 需要真实语料(播放 boot 需要一个真实工程):
//! `YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo test -p yu-cli --test restart -- --ignored --nocapture`

use std::path::PathBuf;

use yu_cli::engine::{Engine, EngineOptions};
use yu_cli::project::Project;
use yu_cli::sha::sha256_hex;

fn sample_root() -> Option<PathBuf> {
    match std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        Ok(root) => Some(PathBuf::from(root)),
        Err(_) => {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_ROOT is missing");
            }
            eprintln!("SKIP restart_is_deterministic_at_same_frame: no sample root");
            None
        }
    }
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn restart_is_deterministic_at_same_frame() {
    let Some(root) = sample_root() else {
        return;
    };
    let project = Project::discover(&root, None).expect("project");
    let opts = EngineOptions {
        record_events: false,
        ..EngineOptions::default()
    };

    // 参照组:**全新进程语义**。跑到首个含 COPY 的可见帧。
    const ANCHOR_TICKS: u32 = 3;
    let mut fresh = Engine::new(project.clone(), opts.clone()).expect("engine");
    let mut before = None;
    for _ in 0..ANCHOR_TICKS {
        let rendered = fresh.tick().expect("tick");
        before = Some((
            rendered.report.frame,
            sha256_hex(&rendered.surface.rgba),
            fresh.display_json().map(|s| sha256_hex(s.as_bytes())),
        ));
    }
    let before = before.expect("至少一帧");

    // 重启组:先跑到 **title**(后缓冲里有非黑内容),再 restart,然后跑同样
    // 的 ANCHOR_TICKS。
    //
    // 这是关键:若渲染器保留上一轮的 `last_screen`/快照,重启后第一帧的
    // `COPY ID=:/W0/` 会截到重启前的 title 画面(而不是设备初始化的黑屏),
    // `CGS100` 快照内容随之不同 ⇒ 锚点帧像素不同。只跑几帧再重启时前后
    // 后缓冲都是黑的,泄漏不可见(本测试最初的写法就漏掉了这种情况)。
    let mut engine = Engine::new(project, opts).expect("engine");
    for _ in 0..300 {
        engine.tick().expect("warmup tick");
    }
    engine.restart().expect("restart");

    let mut after = None;
    for _ in 0..ANCHOR_TICKS {
        let rendered = engine.tick().expect("tick after restart");
        after = Some((
            rendered.report.frame,
            sha256_hex(&rendered.surface.rgba),
            engine.display_json().map(|s| sha256_hex(s.as_bytes())),
        ));
    }
    let after = after.expect("至少一帧");

    assert_eq!(before.0, after.0, "重启后帧号应对齐");
    assert_eq!(
        before.1, after.1,
        "重启后同帧像素必须一致(旧快照/后缓冲泄漏会改变 SHA)"
    );
    assert_eq!(before.2, after.2, "重启后同帧 display JSON 必须一致");
}
