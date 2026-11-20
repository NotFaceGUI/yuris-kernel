//! 补丁字体端到端(合成工程,无真实游戏依赖):
//! `hook.ini` + ttf → `Engine::new` 自动注入并**强制全部 FONT 请求**。
//!
//! 字体来源:优先真实补丁样本 `YURIS_SAMPLE_FONT_PATCH/qiandong.ttf`;
//! 否则找本机系统字体(Windows Arial / Linux DejaVu / macOS Arial)。
//! 两者都没有时显式跳过,不空跑冒充通过。

#[path = "../../yu-rt/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};

use support::{ex, GroupSpec, ProjectBuilder};
use yu_engine::engine::{Backend, Engine, EngineOptions, FontChoice};
use yu_engine::project::Project;
use yu_vfs::{DataSource, EntryKind, LookupKey, MemSource};

const OP_END: u8 = 14;
const OP_RETURNCODE: u8 = 79;
const OP_TEXT: u8 = 96;
const OP_WORD: u8 = 106;

fn minimal_project() -> ProjectBuilder {
    let mut builder = ProjectBuilder::new();
    builder.labels = vec![("SYSTEM_START", 0, 0, 0)];
    builder.entry(0, 0);
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    builder.scripts = vec![vec![
        text(0, ex::s("MSG")),
        text(1, ex::i32_(0)),
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    builder
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

fn temp_root(tag: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/patch-font-boot")
        .join(format!("pid{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// `(family, 字体字节)`;优先真实补丁样本,其次常见系统字体。
fn sample_font() -> Option<(String, Vec<u8>)> {
    if let Some(sample) = std::env::var_os("YURIS_SAMPLE_FONT_PATCH") {
        let path = Path::new(&sample).join("qiandong.ttf");
        if let Ok(bytes) = std::fs::read(path) {
            if let Some(family) = yu_font::families_from_bytes(&bytes).into_iter().next() {
                return Some((family, bytes));
            }
        }
    }
    for path in [
        r"C:\Windows\Fonts\arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            if let Some(family) = yu_font::families_from_bytes(&bytes).into_iter().next() {
                return Some((family, bytes));
            }
        }
    }
    None
}

fn boot_with(root: &Path, font: FontChoice, font_lock: bool) -> Engine {
    let project = Project::discover(root, None).expect("project");
    Engine::new(
        project,
        EngineOptions {
            entry: "SYSTEM_START".to_string(),
            backend: Backend::Soft,
            os_sound: false,
            save_root: Some(root.join("save")),
            font,
            font_lock,
            ..EngineOptions::default()
        },
    )
    .expect("engine")
}

#[test]
fn hook_ini_font_is_auto_loaded_and_forced() {
    let Some((family, bytes)) = sample_font() else {
        eprintln!("skip: 无补丁样本/系统字体,显式跳过");
        return;
    };
    eprintln!("[patch-font-boot] 样本字体 family = {family}");
    let root = temp_root("force");
    materialize(&minimal_project().build(), &root.join("data"));
    std::fs::write(root.join("patchfont.ttf"), &bytes).expect("font");
    std::fs::write(
        root.join("hook.ini"),
        format!("[FONT]\nFONTNAME={family}\nFONTFILENAME=patchfont.ttf\n"),
    )
    .expect("hook.ini");

    // 宿主未指定字体:补丁字体注入并强制。
    let engine = boot_with(&root, FontChoice::Script, false);
    assert_eq!(
        engine.forced_font(),
        Some(family.as_str()),
        "hook.ini 字体必须被自动加载并强制全部 FONT 请求"
    );

    // 宿主选了别的 family 但未锁:补丁字体接管(载体字只有它能解码)。
    let engine = boot_with(&root, FontChoice::Family("NoSuchFamilyXYZ".to_string()), false);
    assert_eq!(engine.forced_font(), Some(family.as_str()));

    // 宿主显式 `font_lock`:尊重宿主选择。
    let engine = boot_with(&root, FontChoice::Family("NoSuchFamilyXYZ".to_string()), true);
    assert_eq!(engine.forced_font(), Some("NoSuchFamilyXYZ"));

    let _ = std::fs::remove_dir_all(&root);
}
