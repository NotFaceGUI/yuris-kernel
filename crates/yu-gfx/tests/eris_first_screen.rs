//! 真实 ERIS 首屏:命令流 → CompositorFrame → 显示列表 golden。
//!
//! 样本 gate:
//! - `YURIS_SAMPLE_ERIS_YSBIN`(或 `YURIS_SAMPLE_ERIS_ROOT`);
//! - 缺样本且 `YURIS_REQUIRE_SAMPLES=1` ⇒ 显式失败;
//! - 缺样本且未要求 ⇒ 可见 skip(打印原因,不空跑)。
//!
//! 首屏定义:官方入口链 `SYSTEM_START → es.ERIS` 的**第一个有可见层的
//! 逻辑帧**(初始化帧之后的画面;VM 的逻辑帧号以事件里的 `frame` 为准)。

use std::path::PathBuf;

use yu_gfx::{json, GfxConfig, GfxState, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

fn eris_root() -> Option<String> {
    let root = if let Ok(root) = std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        root
    } else {
        std::env::var("YURIS_SAMPLE_ERIS_YSBIN").ok().map(|p| {
            std::path::Path::new(&p)
                .ancestors()
                .nth(2)
                .expect("ysbin/../..")
                .display()
                .to_string()
        })?
    };
    resolve_project_root(root)
}

/// 语料从 `<project>/data` 迁到 `<project>/save/data` 后,两种工程根都接受。
fn resolve_project_root(root: String) -> Option<String> {
    let base = std::path::Path::new(&root);
    if base.join("data").join("ysbin").join("ysc.ybn").exists() {
        return Some(root);
    }
    let save = base.join("save");
    if save.join("data").join("ysbin").join("ysc.ybn").exists() {
        return Some(save.display().to_string());
    }
    Some(root)
}

fn write_or_compare_golden(actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("eris_first_screen_display.json");
    if std::env::var("UPDATE_GFX_GOLDEN").ok().as_deref() == Some("1") {
        std::fs::create_dir_all(path.parent().unwrap()).expect("golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden {} ({e}); run with UPDATE_GFX_GOLDEN=1",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "ERIS first-screen display list drifted; review then UPDATE_GFX_GOLDEN=1"
    );
}

#[test]
fn eris_first_screen_display_list() {
    let Some(root) = eris_root() else {
        if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
            panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_YSBIN/ROOT is missing");
        }
        eprintln!("SKIP eris_first_screen_display_list: set YURIS_SAMPLE_ERIS_YSBIN/ROOT");
        return;
    };
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data");
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        // 首帧 130 万 Command 事件会挤爆有界队列;gfx 只需要 DomainCall。
        record_events: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).expect("boot");
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    assert_eq!(viewport.width, 800);
    assert_eq!(viewport.height, 600);

    let mut gfx = GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).expect("gfx");
    let timeline = InputTimeline::new();
    let mut events = Vec::new();
    for frame in 0..=1u32 {
        let r = vm.tick_at(frame * 16, &timeline);
        assert!(r.errors.is_empty(), "frame {frame} errors: {:?}", r.errors);
        events.clear();
        vm.drain_events(&mut events);
        gfx.consume_events(&events);
    }
    let f = gfx.frame();
    assert!(f.layers.len() > 100, "layers={}", f.layers.len());
    assert!(
        f.layers.iter().filter(|l| l.visible).count() > 10,
        "visible={}",
        f.layers.iter().filter(|l| l.visible).count()
    );
    assert!(f.passes.len() >= 2, "passes={}", f.passes.len());
    assert!(
        !f.diagnostics.iter().any(|d| d.severity == "Fatal"),
        "unexpected fatal diag: {:?}",
        f.diagnostics
            .iter()
            .filter(|d| d.severity == "Fatal")
            .collect::<Vec<_>>()
    );
    assert!(
        f.layers
            .iter()
            .any(|l| matches!(&l.texture, yu_gfx::TextureRef::Asset(_))),
        "no asset texture in first screen"
    );
    assert!(f.layers.iter().all(|l| matches!(
        l.blend,
        yu_gfx::BlendMode::Normal | yu_gfx::BlendMode::Subtract
    )));

    let a = json::to_display_json(&f);
    let b = json::to_display_json(&f);
    assert_eq!(a, b, "two serializations must be byte-identical");
    write_or_compare_golden(&a);
}
