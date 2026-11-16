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
        // 官方 `CG` 无 FILE/RID 且层不存在 ⇒ 返回 -1,不建节点
        // (REA `0x42a63d..0x42bbef`)。文本目标层必须由脚本显式创建;
        // `FILE=""` 是官方 1×1 黑图建层路径。
        GroupSpec::new(OP_CG)
            .window(0, ex::s("MSG"))
            .window(2, ex::s(""))
            .window(3, ex::i32_(320))
            .window(4, ex::i32_(120)),
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
        frame.text_layout.is_none(),
        "手动模式不得自动绘制(否则 CGACT[TEXT] 之外多画一遍)"
    );

    // 手动模式下一 tick 自动恢复;仍不应出现自动层。
    engine.tick().expect("tick 2");
    let frame = engine.last_frame().expect("frame 2");
    assert!(frame.text_layout.is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn engine_auto_mode_advances_one_line_per_page() {
    let (mut engine, root) = engine_for(&auto_project(), "auto");

    // tick 1:CG + TEXT[ID] + WORD A + RETURNCODE 让出;官方布局在下一
    // 帧的 `FUN_004046fc` 才推进,本帧没有已提交字形。
    engine.tick().expect("tick 1");
    let frame = engine.last_frame().expect("frame 1");
    assert!(frame.text_layout.is_none(), "布局在下一 tick 才推进");
    assert_eq!(
        frame.text.last().map(|t| t.bytes.clone()),
        Some(b"A".to_vec()),
        "待显示缓冲镜像"
    );

    // tick 2:A 立即显示,随后遇 'C' 停。
    engine.tick().expect("tick 2");
    let layout = engine
        .last_frame()
        .expect("frame 2")
        .text_layout
        .clone()
        .expect("layout 2");
    assert_eq!(
        layout.target,
        Some(yu_gfx::path::normalize(b"MSG", None, false).path),
        "TEXT[ID] 必须命中 CG 建的规范化图层"
    );
    assert_eq!(layout.glyphs.len(), 1, "A 已提交");
    assert_eq!((layout.glyphs[0].x, layout.glyphs[0].y), (0, 0));
    // 已提交字形必须真的进入层表面像素(不只是投影)。
    let ink = |engine: &Engine| {
        engine.last_surface().map_or(0, |s| {
            s.rgba
                .chunks_exact(4)
                .filter(|px| px[0] > 0 || px[1] > 0 || px[2] > 0)
                .count()
        })
    };
    assert!(ink(&engine) > 0, "自动文本必须可见");

    // tick 3:点击在 post-layout 只消费 'C'(官方 pre-layout 只查 SKIP
    // 按钮);本帧 A 仍留在表面,不立即清页。
    engine.set_input("MOUSE_L", true);
    engine.tick().expect("tick 3");
    engine.set_input("MOUSE_L", false);
    let frame = engine.last_frame().expect("frame 3");
    assert_eq!(
        frame.text_layout.as_ref().map(|l| l.glyphs.len()),
        Some(1),
        "点击当帧只消费 'C',R/P/E 下一帧"
    );
    assert!(ink(&engine) > 0);

    // tick 4:R/P/E → 默认 AUTOPAGE=1 的 'P' 清页;随后恢复并执行 WORD B。
    engine.tick().expect("tick 4");
    assert!(
        engine.last_frame().expect("frame 4").text_layout.is_none(),
        "默认一行一页:'P' 清掉 A 的像素"
    );
    assert_eq!(ink(&engine), 0, "'P' 后层表面没有文字像素");

    // tick 5:B 立即显示,遇 'C' 停。
    engine.tick().expect("tick 5");
    let layout = engine
        .last_frame()
        .expect("frame 5")
        .text_layout
        .clone()
        .expect("layout 5");
    assert_eq!(layout.glyphs.len(), 1, "B 已提交");

    // tick 6:点击消费 B 的 'C';tick 7:R/P/E → 'P' 清页 → TEXT[CLEAR] → END。
    engine.set_input("MOUSE_L", true);
    engine.tick().expect("tick 6");
    engine.set_input("MOUSE_L", false);
    engine.tick().expect("tick 7");
    let frame = engine.last_frame().expect("frame 7");
    assert!(frame.text_layout.is_none());
    assert!(frame.text.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}
