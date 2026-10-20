//! 主窗口/子窗口:屏幕 pass 只合成主窗口图层组(`W0`)。

mod support;

use support::*;
use yu_gfx::{Op, PassTarget};

#[test]
fn subwindow_layer_set_is_offscreen_not_screen() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(":/W2/")),
                arg(&table, "CG", "SX", i(320)),
                arg(&table, "CG", "SY", i(240)),
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(":/W2/DBG")),
                arg(&table, "CG", "FILE", s("dbg")),
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    // 子窗口的离屏 pass 仍然产出,供 window/host 会话组合。
    assert!(f
        .passes
        .iter()
        .any(|p| matches!(&p.target, PassTarget::Offscreen(id) if id.display() == ":/W2")));
    // 屏幕 pass 只允许主窗口 W0,不允许 W2。
    let screen = f
        .passes
        .iter()
        .find(|p| matches!(p.target, PassTarget::Screen))
        .expect("screen pass");
    assert!(!screen.ops.iter().any(|o| match o {
        Op::Group(g) => g.offscreen.display() == ":/W2",
        _ => false,
    }));
    assert!(screen.ops.is_empty(), "W0 为空,屏幕 pass 不应有绘制 op");
}

#[test]
fn main_window_set_is_composited_as_group() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("MAIN")),
                arg(&table, "CG", "FILE", s("main")),
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let screen = f
        .passes
        .iter()
        .find(|p| matches!(p.target, PassTarget::Screen))
        .expect("screen pass");
    assert_eq!(screen.ops.len(), 1);
    match &screen.ops[0] {
        Op::Group(g) => assert_eq!(g.offscreen.display(), ":/W0"),
        other => panic!("expected W0 group, got {other:?}"),
    }
}
