//! 图层组 / `CGACT[COPY/COPY2/SWAP]` 状态。

mod support;

use support::*;
use yu_gfx::{
    CopyKind, CopyRegion, CopySource, CopyTiming, GfxState, LayerStatus, Op, OpStatus, PassTarget,
    TextDraw, TextureRef,
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
    assert!(
        matches!(dst.status, LayerStatus::Executable),
        "COPY2 目标层应可执行(EVID-T-11/T-12)"
    );
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
            assert_eq!(c.timing, CopyTiming::CommandCurrent);
            assert_eq!(c.status, OpStatus::Executable);
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
            x: Some(10),
            y: Some(20),
            x2: Some(-30),
            y2: Some(-40)
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
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("a")),
                arg(&table, "CG", "Z", i(1)),
                arg(&table, "CG", "A", i(128)),
                arg(&table, "CG", "X", i(11)),
                arg(&table, "CG", "Y", i(22)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("B")),
                arg(&table, "CG", "FILE", s("b")),
                arg(&table, "CG", "Z", i(2)),
                arg(&table, "CG", "A", i(64)),
                arg(&table, "CG", "X", i(33)),
                arg(&table, "CG", "Y", i(44)),
            ],
        ),
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
    // FUN_0042f57d:SWAP/SWAP2 都交换 +0x68/+0x70(X/Y)。
    assert_eq!((a.transform.x, a.transform.y), (33, 44));
    assert_eq!((b.transform.x, b.transform.y), (11, 22));
    // SWAP2 只换图像(+坐标),不换 A。
    assert_eq!(a.alpha, 128);
    assert_eq!(b.alpha, 64);
}

#[test]
fn rectpaint_family_is_executable() {
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
                arg(&table, "CGACT", "RECTPAINT", i(1)),
                arg(&table, "CGACT", "SET", i(0x112233)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("L")),
                arg(&table, "CGACT", "RECTPAINT2", i(1)),
                arg(&table, "CGACT", "SET", i(128)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("L")),
                arg(&table, "CGACT", "RECTPAINT3", i(1)),
                arg(&table, "CGACT", "SET", i(0x80AABBCC)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let image_passes: Vec<_> = f
        .passes
        .iter()
        .filter(|p| matches!(&p.target, PassTarget::Texture(id) if id.display() == ":/W0/L"))
        .collect();
    assert_eq!(image_passes.len(), 3, "three RECTPAINT passes");
    for pass in image_passes {
        match &pass.ops[0] {
            Op::Image(i) => {
                assert_eq!(i.source.display(), ":/W0/L");
                assert_eq!(i.status, OpStatus::Executable);
            }
            other => panic!("expected image op, got {other:?}"),
        }
    }
    let l = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L")
        .unwrap();
    // 可执行加工不改写图层状态;基图仍可绘制。
    assert_eq!(l.status, LayerStatus::Executable);
}

#[test]
fn rectpaint_with_id2_or_partial_region_stays_unimplemented() {
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
                arg(&table, "CGACT", "ID2", s("L2")),
                arg(&table, "CGACT", "RECTPAINT", i(1)),
                arg(&table, "CGACT", "SET", i(0x112233)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("L")),
                arg(&table, "CGACT", "RECTPAINT", i(1)),
                arg(&table, "CGACT", "SET", i(0x112233)),
                arg(&table, "CGACT", "X", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let statuses: Vec<_> = f
        .passes
        .iter()
        .filter(|p| matches!(&p.target, PassTarget::Texture(_)))
        .filter_map(|p| match &p.ops[0] {
            Op::Image(i) => Some(i.status),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            OpStatus::Unimplemented("gfx.imageop.id2.unsupported"),
            OpStatus::Unimplemented("gfx.imageop.region.partial"),
        ]
    );
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

// ---------------------------------------------------------------- DIV-GFX-058
// `CGACT[TEXT]` 持久化与 COPY/加工/交换的文本语义。

fn text_on(table: &yu_gfx::ParamTable, id: &str, text: &str, x: i64) -> yu_rt::event::VmEvent {
    call(
        1,
        "CGACT",
        vec![
            arg(table, "CGACT", "ID", s(id)),
            arg(table, "CGACT", "TEXT", i(1)),
            arg(table, "CGACT", "SET", i(0)),
            arg(table, "CGACT", "SETSTR", s(text)),
            arg(table, "CGACT", "X", i(x)),
            arg(table, "CGACT", "Y", i(0)),
        ],
    )
}

fn frame_copy_texts(gfx: &mut GfxState) -> Vec<TextDraw> {
    gfx.frame()
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Copy(c) => Some(c.texts.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn copy_freezes_source_texts_at_command_time() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
        text_on(&table, "SRC", "AB", 0),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        // COPY 之后再画:不得进入已冻结的快照(DIV-GFX-053 同门)。
        text_on(&table, "SRC", "C", 20),
        frame_end(1),
    ]);
    let texts = frame_copy_texts(&mut gfx);
    assert_eq!(texts.len(), 1, "快照只携带命令时刻的文本");
    assert_eq!(texts[0].bytes, b"AB");
    // 源层自身的绘制项是持久的全集(先 AB 后 C)。
    let src_draws = gfx
        .frame()
        .text_draws
        .iter()
        .filter(|d| d.target.display() == ":/W0/SRC")
        .count();
    assert_eq!(src_draws, 2);
}

#[test]
fn executable_copy_clears_dest_texts() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
        // 目标层先存在并已有文本(例如上一次刷新的旧标签)。
        create(&table, "DST", "", 2, 256),
        text_on(&table, "DST", "OLD", 0),
        frame_end(1),
        frame_start(2),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(2),
    ]);
    assert!(
        gfx.frame().text_draws.is_empty(),
        "COPY 覆写目标面 ⇒ 目标层旧文本作废"
    );
}

#[test]
fn executable_image_op_clears_target_texts() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "LBL", "", 1, 256),
        text_on(&table, "LBL", "LABEL", 0),
        // BT.CGCLEAR 的真实路径:RECTPAINT3 SET=0 清标签面(es_Button.yst:4547)。
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("LBL")),
                arg(&table, "CGACT", "RECTPAINT3", i(1)),
                arg(&table, "CGACT", "SET", i(0)),
            ],
        ),
        frame_end(1),
    ]);
    assert!(
        gfx.frame().text_draws.is_empty(),
        "已执行的像素加工重写表面 ⇒ 烘焙文本作废"
    );
}

#[test]
fn swap_exchanges_layer_texts() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "A", "a", 1, 256),
        create(&table, "B", "b", 1, 256),
        text_on(&table, "A", "ON-A", 0),
        text_on(&table, "B", "ON-B", 0),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("A")),
                arg(&table, "CGACT", "ID2", s("B")),
                arg(&table, "CGACT", "SWAP", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let moved: Vec<(String, Vec<u8>)> = gfx
        .frame()
        .text_draws
        .iter()
        .map(|d| (d.target.display(), d.bytes.clone()))
        .collect();
    assert!(
        moved.contains(&(":/W0/A".to_string(), b"ON-B".to_vec())),
        "{moved:?}"
    );
    assert!(
        moved.contains(&(":/W0/B".to_string(), b"ON-A".to_vec())),
        "{moved:?}"
    );
}
