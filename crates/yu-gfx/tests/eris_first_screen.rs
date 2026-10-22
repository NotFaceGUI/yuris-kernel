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
use yu_vfs::{Codec, DirSource, YpfSource};

fn eris_root() -> Option<String> {
    // 唯一约定:工程数据入口是 <工程根>/data;ysbin 位于 <工程根>/data/ysbin。
    if let Ok(root) = std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        return Some(root);
    }
    std::env::var("YURIS_SAMPLE_ERIS_YSBIN").ok().map(|p| {
        std::path::Path::new(&p)
            .ancestors()
            .nth(2)
            .expect("ysbin/../..")
            .display()
            .to_string()
    })
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
    // 数据入口两种合法形态:封包 data.ypf 优先,散盘 data/ 为备。
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    };
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        // 首帧 130 万 Command 事件会挤爆有界队列;gfx 只需要 DomainCall。
        record_events: false,
        // 标题锚点用工程配置的运行值 `@_DEBUGMODE=0`
        // (`<project-eris>/save/data/config/projectconfig.txt` DEBUGMODE=0)。
        // 对照:debug_mode=true 时内核在 1200 tick 内不进 title(黑帧,debug UI),
        // 而官方 title 截图可见 ⇒ 本测试锚点取 0;窗口标题证据只作旁证。
        // 见 docs/title-motion-ledger.md §9.2。
        debug_mode: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    assert_eq!(viewport.width, 800);
    assert_eq!(viewport.height, 600);

    let mut gfx = GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).expect("gfx");
    // C1:视口以工程 yscfg 为准,不被调用方的占位值覆盖。
    let probe = GfxState::from_tables(vm.tables(), GfxConfig::new(Viewport::new(1, 1)))
        .expect("gfx viewport probe");
    assert_eq!(probe.frame().viewport.width, 800);
    assert_eq!(probe.frame().viewport.height, 600);
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
    // 精确 census(会话 F;与 docs/render-parity.md §2 同步)。
    //
    // J3 复核:旧值(visible 34 / ops 44 / W0 30 / invisible 238)来自
    // `771cf79`,早于 `1365918` 的 YSVR 初值/GOSUB 块栈修正;现在的启动
    // 路径生成的可见集更小(272 层不变、8 可见、18 op、W0 4 op、264 不可见)。
    // 该 census 与 title 稳定帧对官方的像素对照同时成立(见任务 J3 台账 §9),
    // 因此按当前实现更新 golden,而不是用 UPDATE_GFX_GOLDEN 掩盖回归。
    //
    // J3 追加(DIV-GFX-008/041,主组直绘):主窗口组不再产出离屏 pass,
    // 子层直接进屏幕 pass(passes 15 → 14,total ops 18 → 17)。
    //
    // AU1 追加(DIV-GFX-053):按钮状态快照的源层 `BT.OFF.TMP` 在同一帧末尾
    // 被 `CGEND` 删除;命令时刻捕获源纹理后,68 条 `COPY2` 不再被丢弃。
    // 因此 texture passes 12 → 80(69 copy + 11 image)、total ops 17 → 85。
    assert_eq!(f.layers.len(), 272, "layers");
    assert_eq!(f.layers.iter().filter(|l| l.visible).count(), 8, "visible");
    assert_eq!(f.passes.len(), 82, "passes");
    let total_ops: usize = f.passes.iter().map(|p| p.ops.len()).sum();
    assert_eq!(total_ops, 85, "total ops");
    let texture_passes = f
        .passes
        .iter()
        .filter(|p| matches!(p.target, yu_gfx::PassTarget::Texture(_)))
        .count();
    assert_eq!(texture_passes, 80, "texture passes");
    let ops_of = |target: &str| {
        f.passes
            .iter()
            .find(|p| match &p.target {
                yu_gfx::PassTarget::Offscreen(id) => id.display() == target,
                _ => false,
            })
            .map(|p| p.ops.len())
    };
    assert_eq!(ops_of(":/W0"), None, "main group is drawn directly");
    assert_eq!(ops_of(":/W1"), Some(1), "W1 offscreen ops");
    let screen = f
        .passes
        .iter()
        .find(|p| matches!(p.target, yu_gfx::PassTarget::Screen))
        .expect("screen pass");
    // 主组直绘:屏幕 pass 直接含 W0 的子层(首屏为 4 个 op)。
    assert_eq!(screen.ops.len(), 4);
    assert!(
        screen
            .ops
            .iter()
            .all(|op| !matches!(op, yu_gfx::Op::Group(_))),
        "main group must not be composed through an offscreen RT"
    );
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

    // F3:真实 MODE 2 / A=0 / E=0 图层。
    let mode2 = f
        .layers
        .iter()
        .find(|l| l.blend == yu_gfx::BlendMode::Subtract)
        .expect("real MODE=2 layer");
    assert_eq!(mode2.id.display(), ":/W0/SPLDST2");
    assert_eq!(mode2.alpha, 0);
    assert!(!mode2.visible, "E=0");
    assert_eq!(mode2.texture, yu_gfx::TextureRef::SolidBlack1x1);
    assert_eq!(
        f.layers.iter().filter(|l| !l.visible).count(),
        264,
        "E=0 count"
    );

    // A2:真实 FILE="" + SX/SY 黑幕保存显式尺寸。
    let dbg = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W1/DBG")
        .expect("W1/DBG");
    assert_eq!(
        dbg.kind,
        yu_gfx::LayerKind::Image {
            size: Some(yu_gfx::SizeU32 {
                width: 260,
                height: 600
            })
        }
    );
    let fontl = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/FONTL2")
        .expect("FONTL2");
    assert_eq!(
        fontl.kind,
        yu_gfx::LayerKind::Image {
            size: Some(yu_gfx::SizeU32 {
                width: 233,
                height: 27
            })
        }
    );
    // A3:主窗口默认显示 :/W0(脚本未给 NO=0 CGID)。
    assert_eq!(f.windows.len(), 1);
    assert_eq!(f.windows[0].number, 0);
    assert_eq!(f.windows[0].layer_set.as_ref().unwrap().display(), ":/W0");
    assert!(f.windows[0].visible);
    assert!(f.diagnostics.is_empty(), "first screen should be diag-free");

    let a = json::to_display_json(&f);
    let b = json::to_display_json(&f);
    assert_eq!(a, b, "two serializations must be byte-identical");
    write_or_compare_golden(&a);
}

struct RecordingHost {
    inner: yu_gfx::GfxHost,
    queries: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

impl yu_rt::event::GraphicsHost for RecordingHost {
    fn apply_event(&mut self, ev: &yu_rt::VmEvent) {
        self.inner.apply_event(ev);
    }

    fn query(
        &mut self,
        command: &str,
        args: &[yu_rt::event::ArgValue],
    ) -> Result<yu_rt::Value, yu_rt::Diag> {
        self.queries.borrow_mut().push(command.to_string());
        self.inner.query(command, args)
    }
}

/// 同步宿主模式:真实 ERIS 的 `CGINFO/WINDOWINFO` 在脚本执行期查询并写 LET。
/// 关键验收:首屏 census 与旧的“tick 后喂事件”模式一致;查询确实发生。
#[test]
fn eris_first_screen_with_synchronous_info_queries() {
    let Some(root) = eris_root() else {
        if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
            panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_YSBIN/ROOT is missing");
        }
        eprintln!("SKIP eris_first_screen_with_synchronous_info_queries");
        return;
    };
    // 数据入口两种合法形态:封包 data.ypf 优先,散盘 data/ 为备。
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    };
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        record_events: false,
        debug_mode: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let state = std::rc::Rc::new(std::cell::RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).expect("gfx"),
    ));
    let queries = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    vm.set_graphics_host(Box::new(RecordingHost {
        inner: yu_gfx::GfxHost::new(state.clone()),
        queries: queries.clone(),
    }));

    let timeline = InputTimeline::new();
    let mut events = Vec::new();
    for frame in 0..=1u32 {
        let r = vm.tick_at(frame * 16, &timeline);
        assert!(r.errors.is_empty(), "frame {frame} errors: {:?}", r.errors);
        // 事件已同步应用;这里只清空队列,不得再喂给同一 state。
        events.clear();
        vm.drain_events(&mut events);
    }
    let f = state.borrow().frame();
    assert_eq!(f.layers.len(), 272, "layers");
    assert_eq!(f.layers.iter().filter(|l| l.visible).count(), 8, "visible");
    // 1 屏 + 1 组离屏 + 80 纹理 pass(69 copy + 11 image)。
    //
    // 69 条 copy = 1 条主组 `CGS100` + 68 条按钮状态快照;后者的源
    // `BT.OFF.TMP` 在同一帧末尾被 `CGEND` 删除,靠 `CopyOp.source_texture`
    // 在命令时刻捕获(DIV-GFX-053)。旧实现在这里丢掉了 68 条。
    assert_eq!(f.passes.len(), 82, "passes");
    let texture_passes = f
        .passes
        .iter()
        .filter(|p| matches!(p.target, yu_gfx::PassTarget::Texture(_)))
        .count();
    assert_eq!(texture_passes, 80, "texture passes (copies + images)");
    // 80 纹理 op + 4 屏幕 op(主组直绘)+ 1 离屏 DBG 层。
    assert_eq!(
        f.passes.iter().map(|p| p.ops.len()).sum::<usize>(),
        85,
        "total ops"
    );
    let screen = f
        .passes
        .iter()
        .find(|p| matches!(p.target, yu_gfx::PassTarget::Screen))
        .expect("screen pass");
    // 主组直绘:屏幕 pass 直接含 W0 子层,不再是单个 Group op。
    assert_eq!(screen.ops.len(), 4, "W0 children drawn directly");
    assert!(screen
        .ops
        .iter()
        .all(|op| !matches!(op, yu_gfx::Op::Group(_))));

    let q = queries.borrow();
    assert!(q.iter().any(|c| c == "CGINFO"), "ERIS must query CGINFO");
    assert!(
        q.iter().any(|c| c == "WINDOWINFO"),
        "ERIS must query WINDOWINFO at boot"
    );
}

/// A5:工程 `yscfg` 的 `image_ext_order` 全 0、宿主也没显式注入时,
/// `GfxState::from_tables` **必须拒绝构造**,不得内置兜底表。
///
/// 证据:`docs/architecture.md` A5「缺表 = 显式拒绝启动或 Unimplemented」;
/// `yscfg` 字段布局见 `crates/yu-format/src/yscf.rs`。本用例在真实表上把
/// 该字段清零后重放同一构造函数(不修改游戏文件,只改内存快照)。
#[test]
fn empty_image_ext_order_is_rejected_not_defaulted() {
    let Some(root) = eris_root() else {
        if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
            panic!("YURIS_REQUIRE_SAMPLES=1 but sample root is missing");
        }
        eprintln!("SKIP empty_image_ext_order_is_rejected_not_defaulted: no sample");
        return;
    };
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    };
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        record_events: false,
        debug_mode: false,
        ..BootConfig::default()
    };
    let vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );

    let mut tables = vm.tables().clone();
    tables.boot.config.image_ext_order = [0; 8];
    let config = GfxConfig::new(viewport);
    assert!(config.image_probe_order.is_empty(), "GfxConfig 不得内置探测序");
    let err = match GfxState::from_tables(&tables, config) {
        Ok(_) => panic!("yscfg 空序 + 宿主未注入 必须拒绝构造"),
        Err(diag) => diag,
    };
    assert_eq!(err.code, yu_gfx::error::code::CONFIG_PROBE_ORDER_MISSING);

    // 宿主显式注入(不是兜底)后允许构造。
    let mut injected = GfxConfig::new(viewport);
    injected.image_probe_order = vec![yu_gfx::ImageFormat::Png];
    let state = GfxState::from_tables(&tables, injected).expect("显式注入应允许构造");
    assert_eq!(state.frame().image_probe_order.len(), 1);
}
