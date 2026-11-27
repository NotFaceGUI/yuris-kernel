mod support;

use support::*;
use yu_gfx::{CopyKind, GfxState, TextureRef};
use yu_rt::event::VmEvent;

fn layer(id: &str, file: &str) -> VmEvent {
    let t = table();
    call(
        1,
        "CG",
        vec![arg(&t, "CG", "ID", s(id)), arg(&t, "CG", "FILE", s(file))],
    )
}

fn clear(id: &str) -> VmEvent {
    let t = table();
    call(
        1,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s(id)),
            arg(&t, "CGACT", "RECTPAINT3", i(1)),
            arg(&t, "CGACT", "SET", i(0)),
        ],
    )
}

fn copy(source: &str, dest: &str, action: &str) -> VmEvent {
    let t = table();
    call(
        1,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s(source)),
            arg(&t, "CGACT", "ID2", s(dest)),
            arg(&t, "CGACT", action, i(1)),
        ],
    )
}

fn exists(gfx: &GfxState, id: &str) -> i64 {
    let t = table();
    gfx.query_info(
        "CGINFO",
        &[
            arg(&t, "CGINFO", "ID", s(id)),
            arg(&t, "CGINFO", "EXIST", i(1)),
        ],
    )
    .expect("exist query")
    .as_int()
    .expect("integer")
}

#[test]
fn missing_copy_source_does_not_create_nodes_or_parents() {
    let mut gfx = state();
    gfx.consume_events(&[frame_start(1), layer("TMP", ""), clear("TMP")]);
    let count = gfx.frame().layers.len();
    for n in 0..32 {
        let source = format!("ABSENT{n}/SOURCE");
        gfx.consume_events(&[clear("TMP"), copy(&source, "TMP", "COPY2")]);
        assert_eq!(
            exists(&gfx, &source),
            0,
            "a read must not create its source"
        );
        assert_eq!(exists(&gfx, &format!("ABSENT{n}/")), 0);
        assert_eq!(
            gfx.frame().layers.len(),
            count,
            "missing names must not accumulate nodes"
        );
        let op = gfx.copies().last().expect("recorded blank copy");
        assert_eq!(op.kind, CopyKind::Copy2);
        assert!(op.blank_if_source_missing);
        assert_eq!(op.source_texture, Some(TextureRef::None));
    }
}

#[test]
fn uncleared_missing_copy_is_rejected_without_mutating_layers() {
    for kind in ["COPY", "COPY2", "TEXCOPY", "TEXSSCOPY"] {
        let mut gfx = state();
        gfx.consume_events(&[frame_start(1), layer("TMP", "image")]);
        let before = gfx.frame().layers;
        gfx.consume_events(&[copy("ABSENT/SOURCE", "TMP", kind)]);
        assert_eq!(
            gfx.frame().layers,
            before,
            "{kind} must not invent source attributes"
        );
        assert!(
            gfx.copies().is_empty(),
            "{kind} must not publish an unreadable replacement"
        );
        gfx.consume_events(&[copy("ABSENT/SOURCE", "ABSENT/DEST", kind)]);
        assert_eq!(
            exists(&gfx, "ABSENT/DEST"),
            0,
            "invalid reads must not create their destination"
        );
    }
}

#[test]
fn text_after_clear_disqualifies_blank_copy() {
    let t = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        layer("TMP", ""),
        layer("SRC", "missing_asset"),
        clear("TMP"),
        call(
            1,
            "CGACT",
            vec![
                arg(&t, "CGACT", "ID", s("TMP")),
                arg(&t, "CGACT", "TEXT", i(1)),
                arg(&t, "CGACT", "SETSTR", s("A")),
            ],
        ),
        copy("SRC", "TMP", "COPY2"),
    ]);
    assert!(
        !gfx.copies().last().expect("copy").blank_if_source_missing,
        "the most recent image op is a clear, but the surface now contains text"
    );
}
