//! 会话 F 的确认项回归:A1/A2/A3/A6/A7/C4/E1/E3/E7。
//!
//! 每条测试对应 `docs/gfx-verification-ledger.md` 的 Confirmed 行。

mod support;

use support::*;
use yu_gfx::{
    json, ImageAction, LayerKind, LayerStatus, Op, OpStatus, PassTarget, TextureRef, Transition,
};

fn layer<'a>(f: &'a yu_gfx::CompositorFrame, id: &str) -> &'a yu_gfx::Layer {
    f.layers
        .iter()
        .find(|l| l.id.display() == id)
        .unwrap_or_else(|| panic!("layer {id} missing"))
}

fn screen(f: &yu_gfx::CompositorFrame) -> &yu_gfx::Pass {
    f.passes
        .iter()
        .find(|p| matches!(p.target, PassTarget::Screen))
        .expect("screen pass")
}

#[test]
fn empty_tid_clears_pairing_instead_of_w0() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("a")),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(256)),
                arg(&table, "CG", "TID", s("B")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("B")),
                arg(&table, "CG", "FILE", s("b")),
            ],
        ),
        // 脚本的“结束过渡/初始化”写法:Button/es_Button.yst:4526。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "TID", s("")),
                arg(&table, "CG", "T", i(256)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert!(
        !f.passes
            .iter()
            .flat_map(|p| p.ops.iter())
            .any(|o| matches!(o, Op::Transition(_))),
        "TID=\"\" must not become :/W0 transition"
    );
    let a = layer(&f, ":/W0/A");
    assert_eq!(a.transition.as_ref().and_then(|t| t.tid.clone()), None);
    assert!(f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .any(|o| matches!(o, Op::Layer(l) if l.id.display() == ":/W0/A")));
}

#[test]
fn solid_black_with_sx_sy_draws_at_that_size() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("CGS1")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(800)),
                arg(&table, "CG", "SY", i(600)),
                arg(&table, "CG", "E", i(1)),
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let l = layer(&f, ":/W0/CGS1");
    assert_eq!(
        l.kind,
        LayerKind::Image {
            size: Some(yu_gfx::SizeU32 {
                width: 800,
                height: 600
            })
        }
    );
    assert!(matches!(l.status, LayerStatus::Executable));
    assert!(f.passes.iter().flat_map(|p| p.ops.iter()).any(
        |o| matches!(o, Op::Layer(x) if x.id.display() == ":/W0/CGS1")
            && o.status() == OpStatus::Executable
    ));
    let js = json::to_json(&f);
    assert!(js.contains("\"kind\":{\"kind\":\"image\",\"size\":{\"width\":800,\"height\":600}}"));
}

#[test]
fn asset_with_sx_sy_stays_unimplemented() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("PIC")),
                arg(&table, "CG", "FILE", s("pic")),
                arg(&table, "CG", "SX", i(100)),
                arg(&table, "CG", "SY", i(50)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let l = layer(&f, ":/W0/PIC");
    assert!(matches!(
        l.status,
        LayerStatus::Unimplemented("gfx.image.size.unsupported")
    ));
    assert!(f
        .diagnostics
        .iter()
        .any(|d| d.code == "gfx.image.size.unsupported"));
}

#[test]
fn cgact_size_keeps_sx_sy_and_resize_trim_are_recognized() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BG")),
                arg(&table, "CG", "FILE", s("bg")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BG")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(315)),
                arg(&table, "CGACT", "SY", i(190)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let img = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Image(i) => Some(i),
            _ => None,
        })
        .unwrap();
    assert_eq!(img.action, ImageAction::SizeNearest);
    assert_eq!(img.sx, Some(315));
    assert_eq!(img.sy, Some(190));

    gfx.consume_events(&[
        frame_start(2),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BG")),
                arg(&table, "CGACT", "RESIZE", i(1)),
                arg(&table, "CGACT", "X", i(10)),
                arg(&table, "CGACT", "Y", i(20)),
                arg(&table, "CGACT", "X2", i(-30)),
                arg(&table, "CGACT", "Y2", i(-40)),
            ],
        ),
        frame_end(2),
    ]);
    let f = gfx.frame();
    assert!(f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .any(|o| { matches!(o, Op::Image(i) if i.action == ImageAction::Resize) }));

    gfx.consume_events(&[
        frame_start(3),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BG")),
                arg(&table, "CGACT", "TRIM", i(1)),
                arg(&table, "CGACT", "SET", i(15)),
            ],
        ),
        frame_end(3),
    ]);
    let f = gfx.frame();
    assert!(f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .any(|o| { matches!(o, Op::Image(i) if i.action == ImageAction::Trim) }));
}

#[test]
fn font_no_and_bno_are_recorded() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "NO", i(3)),
                arg(&table, "FONT", "BNO", i(1)),
            ],
        ),
        yu_rt::VmEvent::Text {
            frame: 1,
            id: "MSG".to_string(),
            bytes: b"abc".to_vec(),
        },
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert_eq!(f.text[0].style.font_no, Some(3));
    assert_eq!(f.text[0].style.font_bno, Some(1));
    let js = json::to_json(&f);
    assert!(js.contains("\"font_no\":3"));
    assert!(js.contains("\"font_bno\":1"));
}

#[test]
fn mode_update_recovers_executable_status() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("l")),
                arg(&table, "CG", "MODE", i(3)),
            ],
        ),
        frame_end(1),
    ]);
    assert!(matches!(
        layer(&gfx.frame(), ":/W0/L").status,
        LayerStatus::Unimplemented("gfx.blend.unverified")
    ));

    gfx.consume_events(&[
        frame_start(2),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "MODE", i(0)),
            ],
        ),
        frame_end(2),
    ]);
    assert!(matches!(
        layer(&gfx.frame(), ":/W0/L").status,
        LayerStatus::Executable
    ));
}

#[test]
fn persistent_unsupported_attributes_keep_layer_unimplemented() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("l")),
                arg(&table, "CG", "MODE", i(3)),
                arg(&table, "CG", "FX", i(100)),
            ],
        ),
        frame_end(1),
    ]);
    // 只做一次无参数更新,不得把持久 MODE/FX 的不可执行状态洗掉。
    gfx.consume_events(&[
        frame_start(2),
        call(2, "CG", vec![arg(&table, "CG", "ID", s("L"))]),
        frame_end(2),
    ]);
    let f = gfx.frame();
    assert!(matches!(
        layer(&f, ":/W0/L").status,
        LayerStatus::Unimplemented(_)
    ));
}

#[test]
fn file_replacement_clears_old_copy_status() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("src")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    assert!(matches!(
        layer(&gfx.frame(), ":/W0/DST").status,
        LayerStatus::Unimplemented("gfx.copy.unsupported")
    ));
    gfx.consume_events(&[
        frame_start(2),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("fresh")),
            ],
        ),
        frame_end(2),
    ]);
    let f = gfx.frame();
    let dst = layer(&f, ":/W0/DST");
    assert_eq!(dst.texture, TextureRef::Asset(b"fresh".to_vec()));
    assert!(matches!(dst.status, LayerStatus::Executable));
}

#[test]
fn window_cgid_selects_main_layer_set_and_e_zero_hides_screen() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(":/W1/")),
                arg(&table, "CG", "SX", i(320)),
                arg(&table, "CG", "SY", i(240)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(":/W1/X")),
                arg(&table, "CG", "FILE", s("x")),
            ],
        ),
        window_call(
            1,
            "WINDOW",
            vec![
                arg(&table, "WINDOW", "NO", i(0)),
                arg(&table, "WINDOW", "CGID", s(":/W1/")),
                arg(&table, "WINDOW", "X", i(11)),
                arg(&table, "WINDOW", "Y", i(22)),
                arg(&table, "WINDOW", "SX", i(640)),
                arg(&table, "WINDOW", "SY", i(480)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let win = &f.windows[0];
    assert_eq!(win.layer_set.as_ref().unwrap().display(), ":/W1");
    assert_eq!((win.x, win.y), (11, 22));
    assert_eq!(win.size.unwrap().width, 640);
    assert!(matches!(&screen(&f).ops[0], Op::Group(g) if g.offscreen.display() == ":/W1"));

    gfx.consume_events(&[
        frame_start(2),
        window_call(
            2,
            "WINDOW",
            vec![
                arg(&table, "WINDOW", "NO", i(0)),
                arg(&table, "WINDOW", "E", i(0)),
            ],
        ),
        frame_end(2),
    ]);
    let f = gfx.frame();
    assert!(screen(&f).ops.is_empty(), "E=0 hides main window content");
}

#[test]
fn image_children_are_drawn_before_the_parent() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("PARENT")),
                arg(&table, "CG", "FILE", s("p")),
                arg(&table, "CG", "Z", i(5)),
            ],
        ),
        // 引擎遍历对任意节点先递归子层(FUN_00413dc8 0x413e1a),图像层
        // 下的子层同样先画。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("PARENT/CHILD")),
                arg(&table, "CG", "FILE", s("c")),
                arg(&table, "CG", "Z", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let w0 = f
        .passes
        .iter()
        .find(|p| matches!(&p.target, PassTarget::Offscreen(id) if id.display() == ":/W0"))
        .expect("W0 pass");
    let pos = |name: &str| {
        w0.ops
            .iter()
            .position(|o| matches!(o, Op::Layer(l) if l.id.display() == name))
            .unwrap_or_else(|| panic!("{name} not drawn"))
    };
    assert!(
        pos(":/W0/PARENT/CHILD") < pos(":/W0/PARENT"),
        "children must be drawn before their parent image"
    );
}

#[test]
fn hidden_image_parent_still_draws_visible_children() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("P")),
                arg(&table, "CG", "FILE", s("p")),
                arg(&table, "CG", "E", i(0)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("P/C")),
                arg(&table, "CG", "FILE", s("c")),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let ops: Vec<&Op> = f.passes.iter().flat_map(|p| p.ops.iter()).collect();
    assert!(ops
        .iter()
        .any(|o| matches!(o, Op::Layer(l) if l.id.display() == ":/W0/P/C")));
    assert!(!ops
        .iter()
        .any(|o| matches!(o, Op::Layer(l) if l.id.display() == ":/W0/P")));
}

#[test]
fn deep_path_and_deep_group_nesting_do_not_recurse() {
    let table = table();
    let mut gfx = state();
    // 600 级路径 + 嵌套组:旧实现会在 ensure_parents / 组遍历递归爆栈。
    let mut id = String::new();
    for _ in 0..600 {
        id.push_str("g/");
    }
    id.push_str("leaf");
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(&id)),
                arg(&table, "CG", "FILE", s("leaf")),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert!(f.layers.iter().any(|l| l.id.display().ends_with("/leaf")));
    assert!(f
        .passes
        .iter()
        .any(|p| matches!(p.target, PassTarget::Screen)));
}

#[test]
fn diagnostics_are_per_frame_and_count_duplicates() {
    let mut gfx = state();
    let bad = || call(1, "NOT_A_COMMAND", vec![]);
    gfx.consume_events(&[frame_start(1), bad(), bad(), bad(), frame_end(1)]);
    let f = gfx.frame();
    let d = f
        .diagnostics
        .iter()
        .find(|d| d.code == "gfx.command.unknown")
        .expect("unknown-command diag");
    assert_eq!(d.count, 3);

    gfx.consume_events(&[frame_start(2), frame_end(2)]);
    assert!(
        gfx.frame().diagnostics.is_empty(),
        "diagnostics are per-frame"
    );
}

#[test]
fn missing_required_graphics_command_fails_validation() {
    assert!(table().validate().is_ok());
    for missing in ["CG", "CGACT", "CGEND"] {
        let t = table_without(missing);
        let err = t.validate().expect_err("must fail fast");
        assert_eq!(err.code, "gfx.table.missing");
        assert!(err.detail.contains(missing));
    }
}

#[test]
fn transition_defaults_to_t_256() {
    let t = Transition::default();
    assert_eq!(t.t, 256);
    assert_eq!(t.ta, 0);
    assert!(t.tid.is_none());
}
