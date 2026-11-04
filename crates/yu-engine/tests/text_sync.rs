//! H-P2-04 · Engine 级文本同步回归(合成工程,无真实语料依赖)。
//!
//! 证据:
//! - 手册 `text/text.html`(`TEXT[ID]` = 自动输出层;`CLEAR` 清内部缓冲);
//! - REA `FUN_004046fc`(`0x4046fc`,每帧推进文本)、`TEXT[CLEAR]`
//!   `0x453894` → `FUN_004638e4`(`0x4638e4`);
//! - 图层路径规范化 `FUN_004141e4`、查找 `FUN_00414559`(逐字节)。
//!
//! 关键语义(2026-11-04 实机回归):手动模式(`OUTPUTMODE=0`)官方
//! **不自动绘制** `+0x38`(ERIS 由脚本 `CGACT[TEXT]` 画文本框);宿主若仍
//! 同步自动层,同一行会在 `(0,0)` 多画一遍。见
//! `docs/divergences/DIV-GFX-075-text-surface-vs-pending.md` §C6。

#[path = "../../yu-rt/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};

use support::{ex, GroupSpec, ProjectBuilder};
use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_vfs::{DataSource, EntryKind, LookupKey, MemSource};

const OP_END: u8 = 14;
const OP_CG: u8 = 1;
const OP_RETURNCODE: u8 = 79;
const OP_TEXT: u8 = 96;
const OP_WORD: u8 = 106;

fn base() -> ProjectBuilder {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b
}

/// 手动模式:ERIS 的真实输出模式。`TEXT[ID]` 只给脚本读 `$_TEXT`,
/// 引擎不自动绘制;下方文本框由 `CGACT[TEXT]` 画。
fn manual_project() -> ProjectBuilder {
    let mut b = base();
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    b.scripts = vec![vec![
        GroupSpec::new(OP_CG).window(0, ex::s("MSG")),
        text(0, ex::s("MSG")),
        text(1, ex::i32_(0)), // OUTPUTMODE=0 手动
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    b
}

/// 自动模式:引擎自己绘制 `+0x38`;两行 + CLEAR。
fn auto_project() -> ProjectBuilder {
    let mut b = base();
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    b.scripts = vec![vec![
        GroupSpec::new(OP_CG).window(0, ex::s("MSG")),
        text(0, ex::s("MSG")),
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_WORD).window(0, b"B".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(2, Vec::new()),
        text(16, ex::i32_(1)), // TEXT[CLEAR=1](0.495 slot 16)
        GroupSpec::new(OP_END),
    ]];
    b
}

/// 把 `MemSource` 落到临时工程目录(开发工程形态:`<root>/data/...`)。
fn materialize(src: &MemSource, dir: &Path) {
    fn walk(src: &MemSource, key: &LookupKey, dir: &Path) {
        for entry in src.list(key).expect("list") {
            let mut raw = key.as_bytes().to_vec();
            if !raw.is_empty() {
                raw.push(b'/');
            }
            raw.extend_from_slice(&entry.name);
            let child_key = LookupKey::new(&raw, src.codec()).expect("child key");
            let child = dir.join(String::from_utf8_lossy(&entry.name).as_ref());
            match entry.kind {
                EntryKind::Dir => {
                    std::fs::create_dir_all(&child).expect("mkdir");
                    walk(src, &child_key, &child);
                }
                EntryKind::File => {
                    std::fs::write(&child, src.open(&child_key).expect("read"))
                        .expect("write");
                }
            }
        }
    }
    std::fs::create_dir_all(dir).expect("root");
    walk(src, &LookupKey::root(), dir);
}

fn engine_for(builder: &ProjectBuilder, name: &str) -> (Engine, PathBuf) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/engine-text-sync")
        .join(format!("pid{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    materialize(&builder.build(), &root.join("data"));
    let project = Project::discover(&root, None).expect("project");
    let engine = Engine::new(
        project,
        EngineOptions {
            entry: "SYSTEM_START".to_string(),
            backend: Backend::Soft,
            os_sound: false,
            save_root: Some(root.join("save")),
            ..EngineOptions::default()
        },
    )
    .expect("engine");
    (engine, root)
}

#[test]
fn engine_manual_mode_does_not_draw_auto_text_layer() {
    // 2026-11-04 实机回归:ERIS 手动模式,左上角曾多出一行正文——宿主把
    // `_TEXT` 同步到自动层并画在 (0,0)。官方 `FUN_004046fc` 对
    // `[+0x1c] != 1` 直接清+恢复,不调用 `FUN_00404914`。
    let (mut engine, root) = engine_for(&manual_project(), "manual");
    engine.tick().expect("tick 1");
    let frame = engine.last_frame().expect("frame 1");
    assert!(
        frame.text.is_empty() && frame.text_layout.is_none(),
        "手动模式不得出现自动文本层(否则 CGACT[TEXT] 之外多画一遍)"
    );

    // 手动模式下一 tick 自动恢复;仍不应出现自动层。
    engine.tick().expect("tick 2");
    let frame = engine.last_frame().expect("frame 2");
    assert!(frame.text.is_empty() && frame.text_layout.is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn engine_auto_mode_reveals_and_clears_text_layer() {
    let (mut engine, root) = engine_for(&auto_project(), "auto");

    // tick 1(0ms):CG + TEXT[ID] + WORD A + RETURNCODE 让出。自动层已同步,
    // 但本 tick 尚未推进 reveal ⇒ 可见 0 字。
    engine.tick().expect("tick 1");
    let frame = engine.last_frame().expect("frame 1");
    let layout = frame.text_layout.as_ref().expect("auto layout 1");
    assert_eq!(
        layout.target,
        Some(yu_gfx::path::normalize(b"MSG", None, false).path),
        "TEXT[ID] 必须命中 CG 建的规范化图层"
    );
    assert_eq!(layout.glyphs.len(), 1);
    assert_eq!(layout.reveal, Some(0), "行首字符下一 tick 才显示");

    // tick 2(16ms):A 立即显示,随后遇 'C' 停;等待推进输入。
    engine.tick().expect("tick 2");
    let layout = engine
        .last_frame()
        .expect("frame 2")
        .text_layout
        .clone()
        .expect("layout 2");
    assert_eq!(layout.reveal, Some(1), "A 可见");

    // tick 3:点击恢复 → 提交 A → 执行 WORD B(新段尚未显示)。
    engine.set_input("MOUSE_L", true);
    engine.tick().expect("tick 3");
    engine.set_input("MOUSE_L", false);
    let layout = engine
        .last_frame()
        .expect("frame 3")
        .text_layout
        .clone()
        .expect("layout 3");
    assert_eq!(layout.glyphs.len(), 2, "缓冲已有 A/B");
    assert_eq!(layout.reveal, Some(1), "B 属于新段,尚未显示");

    // tick 4:释放后推进 B,遇 'C' 停。
    engine.tick().expect("tick 4");
    let layout = engine
        .last_frame()
        .expect("frame 4")
        .text_layout
        .clone()
        .expect("layout 4");
    assert_eq!(layout.reveal, Some(2), "A+B 可见");

    // tick 5:点击恢复 → 提交 B → TEXT[CLEAR] → END;显示层清空。
    engine.set_input("MOUSE_L", true);
    engine.tick().expect("tick 5");
    let frame = engine.last_frame().expect("frame 5");
    assert!(frame.text.is_empty(), "TEXT[CLEAR] 后当前层为空");
    assert!(frame.text_layout.is_none());
    let _ = std::fs::remove_dir_all(&root);
}
