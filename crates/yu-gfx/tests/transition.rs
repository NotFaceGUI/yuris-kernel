//! 过渡双纹理与 FadeVal。

mod support;

use support::*;
use yu_gfx::{LayerStatus, Op, TextureRef};

#[test]
fn transition_op_carries_both_textures_and_fade() {
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
        frame_end(1),
    ]);
    let f = gfx.frame();
    let op = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Transition(t) => Some(t),
            _ => None,
        })
        .expect("transition op");
    assert_eq!(op.from, TextureRef::Asset(b"a".to_vec()));
    assert_eq!(op.to, Some(TextureRef::Asset(b"b".to_vec())));
    assert_eq!(op.tid.as_ref().unwrap().display(), ":/W0/B");
    // 128 / (256/(256/256+128)) / 256 = 129/512
    assert!((op.fade - 129.0 / 512.0).abs() < 1e-15);
    assert_eq!(op.layer.alpha, 256);
}

#[test]
fn missing_tid_yields_unimplemented_transition() {
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
                arg(&table, "CG", "TA", i(128)),
                arg(&table, "CG", "TID", s("MISSING")),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let op = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Transition(t) => Some(t),
            _ => None,
        })
        .expect("transition op");
    assert_eq!(op.to, None);
    assert!(matches!(
        op.layer.status,
        LayerStatus::Unimplemented("gfx.transition.unsupported")
    ));
}

#[test]
fn ta_zero_falls_back_to_single_texture_layer() {
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
                arg(&table, "CG", "TA", i(0)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert!(!f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .any(|o| matches!(o, Op::Transition(_))));
    assert!(f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .any(|o| matches!(o, Op::Layer(l) if l.id.display() == ":/W0/A")));
}

#[test]
fn transition_to_group_uses_offscreen_texture() {
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
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/X")),
                arg(&table, "CG", "FILE", s("x")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("a")),
                arg(&table, "CG", "T", i(64)),
                arg(&table, "CG", "TA", i(64)),
                arg(&table, "CG", "TID", s("G/")),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let op = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Transition(t) => Some(t),
            _ => None,
        })
        .expect("transition");
    assert_eq!(op.to, Some(TextureRef::Offscreen(id(":/W0/G"))));
}
