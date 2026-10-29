//! 图层顺序 / 组 / E 级联 / 删除。

mod support;

use support::*;
use yu_gfx::{json, LayerId, Op, PassTarget};

fn cg(
    table: &yu_gfx::ParamTable,
    id: &str,
    z: i64,
    a: i64,
    e: i64,
    file: Option<&str>,
) -> yu_rt::event::VmEvent {
    let mut args = vec![
        arg(table, "CG", "ID", s(id)),
        arg(table, "CG", "Z", i(z)),
        arg(table, "CG", "A", i(a)),
        arg(table, "CG", "E", i(e)),
    ];
    if let Some(file) = file {
        args.push(arg(table, "CG", "FILE", s(file)));
    }
    call(1, "CG", args)
}

/// 主窗口组直绘(DIV-GFX-008/041):W0 的子层/子组现在出现在屏幕 pass,
/// 没有自己的离屏 pass。
fn w0_ops(frame: &yu_gfx::CompositorFrame) -> &[Op] {
    frame
        .passes
        .iter()
        .find(|p| matches!(p.target, PassTarget::Screen))
        .map(|p| p.ops.as_slice())
        .expect("screen pass")
}

fn layer_index(frame: &yu_gfx::CompositorFrame, id: &str) -> usize {
    frame
        .layers
        .iter()
        .position(|l| l.id.display() == id)
        .expect("layer exists")
}

#[test]
fn same_z_keeps_creation_order_and_z_reorders() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        cg(&table, "A", 10, 256, 1, Some("a")),
        cg(&table, "B", 10, 256, 1, Some("b")),
        cg(&table, "C", 10, 256, 1, Some("c")),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let ids: Vec<String> = w0_ops(&f)
        .iter()
        .filter_map(|o| match o {
            Op::Layer(l) => Some(l.id.display()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![":/W0/A", ":/W0/B", ":/W0/C"]);

    gfx.consume_events(&[
        frame_start(2),
        cg(&table, "A", 20, 256, 1, None),
        frame_end(2),
    ]);
    let f = gfx.frame();
    let ids: Vec<String> = w0_ops(&f)
        .iter()
        .filter_map(|o| match o {
            Op::Layer(l) => Some(l.id.display()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![":/W0/B", ":/W0/C", ":/W0/A"]);
}

#[test]
fn e_zero_hides_from_pass_but_keeps_state() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        cg(&table, "SHOWN", 1, 256, 1, Some("shown")),
        cg(&table, "HIDDEN", 2, 256, 0, Some("hidden")),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let ops = w0_ops(&f);
    assert!(ops
        .iter()
        .any(|o| matches!(o, Op::Layer(l) if l.id.display() == ":/W0/SHOWN")));
    assert!(!ops
        .iter()
        .any(|o| matches!(o, Op::Layer(l) if l.id.display() == ":/W0/HIDDEN")));
    assert!(f
        .layers
        .iter()
        .any(|l| l.id.display() == ":/W0/HIDDEN" && !l.visible));
}

#[test]
fn group_offscreen_pass_precedes_parent_and_alpha_is_on_group() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/")),
                arg(&table, "CG", "SX", i(400)),
                arg(&table, "CG", "SY", i(300)),
                arg(&table, "CG", "Z", i(5)),
                arg(&table, "CG", "A", i(128)),
            ],
        ),
        cg(&table, "G/X", 1, 256, 1, Some("x")),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let g_pass = f
        .passes
        .iter()
        .position(|p| matches!(&p.target, PassTarget::Offscreen(id) if id.display() == ":/W0/G"))
        .expect("group pass");
    // 主组直绘:W0 不再有离屏 pass,子组 pass 必须早于屏幕 pass。
    let w0_pass = f
        .passes
        .iter()
        .position(|p| matches!(p.target, PassTarget::Screen))
        .expect("screen pass");
    assert!(g_pass < w0_pass, "child group pass must precede screen");

    let w0 = &f.passes[w0_pass];
    let g = w0
        .ops
        .iter()
        .find_map(|o| match o {
            Op::Group(g) => Some(g),
            _ => None,
        })
        .expect("group op");
    assert_eq!(g.offscreen.display(), ":/W0/G");
    assert_eq!(g.layer.alpha, 128);
    assert_eq!(g.layer.group_size().unwrap().width, 400);
    assert_eq!(g.layer.group_size().unwrap().height, 300);

    let child_pass = &f.passes[g_pass];
    assert!(child_pass
        .ops
        .iter()
        .any(|o| matches!(o, Op::Layer(l) if l.id.display() == ":/W0/G/X")));
}

#[test]
fn hidden_group_renders_offscreen_but_not_composited() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/")),
                arg(&table, "CG", "E", i(0)),
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        cg(&table, "G/X", 1, 256, 1, Some("x")),
        frame_end(1),
    ]);
    let f = gfx.frame();
    // 组离屏仍然存在(COPY 可用),子层在组 pass 里。
    let g_pass = f
        .passes
        .iter()
        .find(|p| matches!(&p.target, PassTarget::Offscreen(id) if id.display() == ":/W0/G"))
        .expect("hidden group still renders offscreen");
    assert!(!g_pass.ops.is_empty());
    // 但父层/屏幕都不得把组合成进来。
    assert!(!f.passes.iter().any(|p| {
        p.ops
            .iter()
            .any(|o| matches!(o, Op::Group(g) if g.offscreen.display() == ":/W0/G"))
    }));
}

#[test]
fn cgend_removes_subtree() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/")),
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        cg(&table, "G/X", 1, 256, 1, Some("x")),
        call(1, "CGEND", vec![arg(&table, "CGEND", "ID", s("G/"))]),
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert!(!f
        .layers
        .iter()
        .any(|l| l.id.display().starts_with(":/W0/G")));
    let json = json::to_json(&f);
    assert!(json.contains(":/W0"));
}

#[test]
fn idno_appends_decimal_suffix() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("bg/title")),
                arg(&table, "CG", "IDNO", i(2)),
                arg(&table, "CG", "FILE", s("bg")),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert!(f.layers.iter().any(|l| l.id.display() == ":/W0/bg/title2"));
}

#[test]
fn file_replacement_preserves_attributes() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        cg(&table, "L", 7, 128, 1, Some("a")),
        // 只给 FILE:手册 cg/cg.html「画像ファイルが差し変わるだけで、
        // 不透明度やレイヤ位置などの属性値はそのまま保持」。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("b")),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let l = &f.layers[layer_index(&f, ":/W0/L")];
    assert_eq!(l.z, 7, "FILE 替换不重置 Z");
    assert_eq!(l.alpha, 128, "FILE 替换不重置 A");
    match &l.texture {
        yu_gfx::TextureRef::Asset(b) => assert_eq!(b, b"b"),
        other => panic!("expected asset b, got {other:?}"),
    }
}

#[test]
fn empty_file_is_one_by_one_black() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        cg(&table, "BLACK", 1, 256, 1, Some("")),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let l = &f.layers[layer_index(&f, ":/W0/BLACK")];
    assert_eq!(l.texture, yu_gfx::TextureRef::SolidBlack1x1);
}

#[test]
fn same_input_serializes_identically() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        cg(&table, "A", 10, 256, 1, Some("a")),
        cg(&table, "B", 20, 128, 1, Some("b")),
        frame_end(1),
    ]);
    let a = json::to_json(&gfx.frame());
    let b = json::to_json(&gfx.frame());
    assert_eq!(a, b);
}

#[test]
fn value_helpers_do_not_change_ids() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("ABC/CG1")),
                arg(&table, "CG", "FILE", s("cg1")),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let ids: Vec<LayerId> = f.layers.iter().map(|l| l.id.clone()).collect();
    assert!(ids.contains(&id(":/W0/ABC/CG1")));
    assert!(ids.contains(&id(":/W0/ABC")));
}

#[test]
fn text_model_keeps_style_and_bytes() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "COLOR", i(0xffff00)),
                arg(&table, "FONT", "SX", i(22)),
                arg(&table, "FONT", "BOLD", i(1)),
            ],
        ),
        yu_rt::VmEvent::Text {
            frame: 1,
            id: "MSG".to_string(),
            bytes: b"\x82\xa0".to_vec(),
        },
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert_eq!(f.text.len(), 1);
    assert_eq!(f.text[0].style.color, 0xffff00);
    assert_eq!(f.text[0].style.size_x, 22);
    assert!(f.text[0].style.bold);
    assert_eq!(f.text[0].bytes, b"\x82\xa0");
}
