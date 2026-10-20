//! 图层组 / `CGACT[COPY/COPY2/SWAP]` 状态。

mod support;

use support::*;
use yu_gfx::{
    CopyKind, CopyRegion, CopySource, CopyTiming, LayerStatus, Op, OpStatus, PassTarget, TextureRef,
};

fn create(
    table: &yu_gfx::ParamTable,
    id: &str,
    file: &str,
    z: i64,
    a: i64,
) -> yu_rt::event::VmEvent {
    call(
        1,
        "CG",
        vec![
            arg(table, "CG", "ID", s(id)),
            arg(table, "CG", "FILE", s(file)),
            arg(table, "CG", "Z", i(z)),
            arg(table, "CG", "A", i(a)),
        ],
    )
}

#[test]
fn copy2_records_snapshot_and_texture_pass() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
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
    let f = gfx.frame();
    let dst = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/DST")
        .unwrap();
    assert!(matches!(dst.texture, TextureRef::Snapshot(_)));
    assert!(matches!(
        dst.status,
        LayerStatus::Unimplemented("gfx.copy.unsupported")
    ));
    assert_eq!(dst.alpha, 256, "COPY2 只复制图像,不复制属性");

    let copy_pass = f
        .passes
        .iter()
        .find(|p| matches!(&p.target, PassTarget::Texture(id) if id.display() == ":/W0/DST"))
        .expect("copy texture pass");
    assert_eq!(copy_pass.ops.len(), 1);
    match &copy_pass.ops[0] {
        Op::Copy(c) => {
            assert_eq!(c.kind, CopyKind::Copy2);
            assert_eq!(c.source, CopySource::Layer(id(":/W0/SRC")));
            assert_eq!(c.region, CopyRegion::Full);
            assert_eq!(c.timing, CopyTiming::LastRenderedFrame);
            assert_eq!(c.status, OpStatus::Unimplemented("gfx.copy.unsupported"));
        }
        other => panic!("expected copy, got {other:?}"),
    }
}

#[test]
fn copy_with_region_keeps_raw_bounds() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(10)),
                arg(&table, "CGACT", "Y", i(20)),
                arg(&table, "CGACT", "X2", i(-30)),
                arg(&table, "CGACT", "Y2", i(-40)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let copy = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Copy(c) => Some(c),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        copy.region,
        CopyRegion::Partial {
            x: 10,
            y: 20,
            x2: -30,
            y2: -40
        }
    );
}

#[test]
fn copy_copies_attributes_and_copy2_does_not() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 7, 128),
        create(&table, "DST", "dst", 1, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let dst = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/DST")
        .unwrap();
    assert_eq!(dst.z, 7);
    assert_eq!(dst.alpha, 128);
    assert!(matches!(dst.texture, TextureRef::Snapshot(_)));
}

#[test]
fn swap2_swaps_textures_only() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "A", "a", 1, 128),
        create(&table, "B", "b", 2, 64),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("A")),
                arg(&table, "CGACT", "ID2", s("B")),
                arg(&table, "CGACT", "SWAP2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let a = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/A")
        .unwrap();
    let b = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/B")
        .unwrap();
    assert_eq!(a.texture, TextureRef::Asset(b"b".to_vec()));
    assert_eq!(b.texture, TextureRef::Asset(b"a".to_vec()));
    assert_eq!(a.alpha, 128);
    assert_eq!(b.alpha, 64);
}

#[test]
fn rectpaint_is_recorded_as_unimplemented_image_op() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "L", "l", 1, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("L")),
                arg(&table, "CGACT", "RECTPAINT2", i(1)),
                arg(&table, "CGACT", "SET", i(255)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let image_pass = f
        .passes
        .iter()
        .find(|p| matches!(&p.target, PassTarget::Texture(id) if id.display() == ":/W0/L"))
        .expect("image pass");
    match &image_pass.ops[0] {
        Op::Image(i) => {
            assert_eq!(i.source.display(), ":/W0/L");
            assert_eq!(i.set.as_ref().and_then(|v| v.as_int()), Some(255));
            assert_eq!(i.status, OpStatus::Unimplemented("gfx.imageop.unsupported"));
        }
        other => panic!("expected image op, got {other:?}"),
    }
    let l = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L")
        .unwrap();
    assert!(matches!(
        l.status,
        LayerStatus::Unimplemented("gfx.imageop.unsupported")
    ));
}

#[test]
fn group_copy_source_is_group_and_destination_created() {
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
        create(&table, "G/X", "x", 1, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("G/")),
                arg(&table, "CGACT", "ID2", s("SHOT")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let copy = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Copy(c) => Some(c),
            _ => None,
        })
        .unwrap();
    assert_eq!(copy.source, CopySource::Group(id(":/W0/G")));
    assert!(f.layers.iter().any(|l| l.id.display() == ":/W0/SHOT"));
}
