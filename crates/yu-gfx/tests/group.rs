//! 图层组 / `CGACT[COPY/COPY2/SWAP]` 状态。

mod support;

use std::collections::BTreeSet;

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

/// `TEXCOPY` 是官方 PASTESPRITE(`0x42e67f → FUN_0043ccd8 → FUN_0040af1c`),
/// 不是纹理 blit。像素语义未取证时**不得**把目标层换成空快照并毒化:
/// 目标层保留原纹理继续绘制,只出 recoverable 诊断(DIV-GFX-007)。
#[test]
fn texcopy_keeps_destination_texture_and_layer_drawable() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
        create(&table, "DST", "dst", 2, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "TEXCOPY", i(1)),
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
    assert!(
        matches!(&dst.texture, TextureRef::Asset(key) if key == b"dst"),
        "TEXCOPY 未取证时目标层必须保留原纹理,实际 {:?}",
        dst.texture
    );
    assert!(
        matches!(dst.status, LayerStatus::Executable),
        "TEXCOPY 不得毒化目标层(DIV-GFX-007)"
    );
    assert!(
        !f.passes
            .iter()
            .any(|p| matches!(&p.target, PassTarget::Texture(id) if id.display() == ":/W0/DST")),
        "TEXCOPY 不产生空快照 pass"
    );
    assert!(
        f.diagnostics
            .iter()
            .any(|d| { d.code == "gfx.copy.unsupported" && d.detail.contains("TEXCOPY") }),
        "必须显式诊断 PASTESPRITE 未取证,实际 {:?}",
        f.diagnostics
    );
}

/// `TEXSSCOPY` 是官方 GETSS(`0x42e68a → FUN_0043cd6c`),同样不得整层毒化。
#[test]
fn texsscopy_keeps_destination_texture_and_layer_drawable() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
        create(&table, "DST", "dst", 2, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "TEXSSCOPY", i(1)),
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
    assert!(matches!(&dst.texture, TextureRef::Asset(key) if key == b"dst"));
    assert!(matches!(dst.status, LayerStatus::Executable));
    assert!(
        f.diagnostics
            .iter()
            .any(|d| { d.code == "gfx.copy.unsupported" && d.detail.contains("TEXSSCOPY") }),
        "必须显式诊断 GETSS 未取证,实际 {:?}",
        f.diagnostics
    );
}

/// 语料形态:`CGACT[ID=$L1 ID2=$L1 TEXCOPY=1]` 自复制
/// (`es_Sprite.yst:5604/8884/8956`)。至少不能因命令让该层消失。
#[test]
fn texcopy_self_copy_keeps_layer_drawable() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "L1", "sprite", 1, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("L1")),
                arg(&table, "CGACT", "ID2", s("L1")),
                arg(&table, "CGACT", "TEXCOPY", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let layer = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L1")
        .unwrap();
    assert!(matches!(&layer.texture, TextureRef::Asset(key) if key == b"sprite"));
    assert!(matches!(layer.status, LayerStatus::Executable));
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

/// CG 缩略图链:`_TMP_OF`/`_TMP_OV` 在 MERGE2 之后被 `CGEND` 删除。
///
/// 官方命令立即执行,AKARUSA/COPY2 的结果已经进入 MERGE2 的输入;帧末重放
/// 必须保留这些中间产物。槽位 2 复用同名 `_TMP_*` 时,槽位 1 的 COPY2 快照
/// 仍被槽位 1 的 MERGE2 引用,不能被 `prune_copy_log` 删掉 —— 否则 hover
/// 缩略图只剩底板(2026-11-01 实测)。
#[test]
fn thumbnail_merge_chain_survives_cg_end_and_reused_tmp_layers() {
    let table = table();
    let mut gfx = state();
    let mut events = vec![
        frame_start(1),
        create(&table, "BT.OFF1", "plate", 10, 256),
        create(&table, "BT.OVER1", "plate", 11, 256),
        create(&table, "BT.OFF2", "plate", 12, 256),
        create(&table, "BT.OVER2", "plate", 13, 256),
    ];
    for (thumb, off, over) in [
        ("yuu_01", "BT.OFF1", "BT.OVER1"),
        ("yuu_02", "BT.OFF2", "BT.OVER2"),
    ] {
        events.push(create(&table, "_TMP_OF", thumb, 1, 256));
        events.push(create(&table, "_TMP_OV", "", 1, 256));
        events.push(call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("_TMP_OF")),
                arg(&table, "CGACT", "ID2", s("_TMP_OV")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ));
        events.push(call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("_TMP_OF")),
                arg(&table, "CGACT", "AKARUSA", i(1)),
                arg(&table, "CGACT", "SET", i(96)),
            ],
        ));
        events.push(call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("_TMP_OF")),
                arg(&table, "CGACT", "ID2", s(off)),
                arg(&table, "CGACT", "MERGE2", i(1)),
            ],
        ));
        events.push(call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("_TMP_OV")),
                arg(&table, "CGACT", "ID2", s(over)),
                arg(&table, "CGACT", "MERGE2", i(1)),
            ],
        ));
        events.push(call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", s("_TMP_OF"))],
        ));
        events.push(call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", s("_TMP_OV"))],
        ));
    }
    events.push(frame_end(1));
    gfx.consume_events(&events);
    let f = gfx.frame();

    let copies: BTreeSet<u64> = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter_map(|o| match o {
            Op::Copy(c) => Some(c.id.0),
            _ => None,
        })
        .collect();
    let images: Vec<&yu_gfx::ImageOp> = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter_map(|o| match o {
            Op::Image(i) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(
        images
            .iter()
            .filter(|i| i.action.name() == "akarusa")
            .count(),
        2,
        "两个槽位的 AKARUSA 都必须保留({images:?})"
    );
    let merges: Vec<&yu_gfx::ImageOp> = images
        .iter()
        .copied()
        .filter(|i| i.action.name() == "merge2")
        .collect();
    assert_eq!(merges.len(), 4, "每槽 OFF/OVER 各一条 MERGE2");
    for merge in merges {
        let Some(TextureRef::Snapshot(snap)) = &merge.source_texture else {
            continue;
        };
        assert!(
            copies.contains(&snap.0),
            "MERGE2 引用的快照 {} 必须由帧内 COPY2 生产(实际 {copies:?})",
            snap.0
        );
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
fn swap_swaps_textures_only() {
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
                arg(&table, "CGACT", "SWAP", i(1)),
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
    // REA:SWAP(slot 66)→ FUN_0042f57d 模式 1 = 纹理 + X/Y。
    assert_eq!((a.transform.x, a.transform.y), (33.0, 44.0));
    assert_eq!((b.transform.x, b.transform.y), (11.0, 22.0));
    // 模式 1 不换可见属性。
    assert_eq!(a.alpha, 128);
    assert_eq!(b.alpha, 64);
}

#[test]
fn swap2_also_swaps_visibility_attributes() {
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
                arg(&table, "CG", "A", i(128)),
                arg(&table, "CG", "MODE", i(1)),
                arg(&table, "CG", "T", i(100)),
                arg(&table, "CG", "TA", i(10)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("B")),
                arg(&table, "CG", "FILE", s("b")),
                arg(&table, "CG", "A", i(64)),
                arg(&table, "CG", "MODE", i(5)),
                arg(&table, "CG", "T", i(200)),
                arg(&table, "CG", "TA", i(20)),
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
    // REA:SWAP2(slot 67)→ 模式 2 = 纹理/X/Y + E/A/T/TA/MODE。
    assert_eq!(a.alpha, 64);
    assert_eq!(b.alpha, 128);
    assert_eq!(a.blend.code(), 5);
    assert_eq!(b.blend.code(), 1);
    assert_eq!(a.transition.as_ref().map(|t| (t.t, t.ta)), Some((200, 20)));
    assert_eq!(b.transition.as_ref().map(|t| (t.t, t.ta)), Some((100, 10)));
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

/// `COPY2 ID=:/W0/` 的快照必须取**命令时刻**的层状态:同一帧内先截屏
/// (SRC 可见)、隐藏 SRC、再截屏(CGS1 里 SRC 不可见)。ERIS 的旧屏 →
/// 隐藏 UI → 新屏交叉叠化依赖这个差异;取上一帧会让两张完全相同。
#[test]
fn main_group_screen_snapshot_uses_command_time_layer_state() {
    let table = table();
    let mut gfx = state();
    let copy_from_screen = |table: &yu_gfx::ParamTable, to: &str| {
        call(
            1,
            "CGACT",
            vec![
                arg(table, "CGACT", "ID", s(":/W0/")),
                arg(table, "CGACT", "ID2", s(to)),
                arg(table, "CGACT", "COPY2", i(1)),
            ],
        )
    };
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
        copy_from_screen(&table, "CGS100"),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "E", i(0)),
            ],
        ),
        copy_from_screen(&table, "CGS1"),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let copy_id = |dest: &str| {
        f.passes
            .iter()
            .flat_map(|pass| pass.ops.iter())
            .find_map(|op| match op {
                Op::Copy(copy) if copy.dest.display() == dest => Some(copy.id),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{dest} copy"))
    };
    let visible_in = |dest: &str| {
        let id = copy_id(dest);
        let snapshot = f
            .screen_snapshots
            .get(&id)
            .unwrap_or_else(|| panic!("{dest} screen snapshot"));
        snapshot
            .layers
            .iter()
            .find(|layer| layer.id.display() == ":/W0/SRC")
            .unwrap_or_else(|| panic!("SRC in {dest}"))
            .visible
    };
    assert!(visible_in(":/W0/CGS100"), "第一张截屏应在隐藏之前");
    assert!(!visible_in(":/W0/CGS1"), "第二张截屏应在隐藏之后");
}

/// 屏幕快照在**采集当帧**必须物化,即使目标层同一 tick 稍后被 `CGEND`:
/// CGS 栈把快照 id 存进脚本变量,若等到栈恢复才首次发射 copy,后端只能
/// 用当前场景重算旧 id(2026-11-03 真机:淡出幕布闪出当前不存在的立绘)。
#[test]
fn screen_snapshot_materializes_on_capture_frame_even_if_target_dies() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        create(&table, "SRC", "src", 1, 256),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s(":/W0/")),
                arg(&table, "CGACT", "ID2", s("CGS100")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        call(1, "CGEND", vec![arg(&table, "CGEND", "ID", s("CGS100"))]),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let synthetic = f.passes.iter().find_map(|pass| {
        let PassTarget::Texture(target) = &pass.target else {
            return None;
        };
        if !target.display().starts_with("__copy_screen_snapshot_") {
            return None;
        }
        pass.ops.iter().find_map(|op| match op {
            Op::Copy(copy) => Some(copy),
            _ => None,
        })
    });
    let copy = synthetic.expect("采集帧应有一条内部合成 copy");
    assert!(
        f.screen_snapshots.contains_key(&copy.id),
        "内部合成 copy 必须对应命令时刻屏幕快照"
    );
}

/// `frame()` 是事件批次的纯投影:同一批事件重复取帧必须逐位一致。
/// 旧实现会在第一次取帧时写 `baked_images`,第二次把刚发射的加工项跳过,
/// 导致 soft/wgpu 两个消费者拿到不同 pass 集(立绘 `COPY→SIZE2` 丢层)。
#[test]
fn frame_projection_is_idempotent_within_event_batch() {
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
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(32)),
                arg(&table, "CGACT", "SY", i(32)),
            ],
        ),
        frame_end(1),
    ]);
    let first = gfx.frame();
    let second = gfx.frame();
    assert_eq!(first, second, "同一批事件的 frame() 必须相同");
    assert!(
        first
            .passes
            .iter()
            .any(|pass| matches!(pass.target, PassTarget::Texture(_))),
        "加工项必须在两次投影里都保留"
    );
}

/// 烘焙前缀只能在后端确认成功后提交;失败目标下一帧必须重新发射。
#[test]
fn baked_prefix_waits_for_backend_ack_and_retries_failed_target() {
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
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(32)),
                arg(&table, "CGACT", "SY", i(32)),
            ],
        ),
        frame_end(1),
    ]);
    let has_image = |gfx: &GfxState| {
        gfx.frame()
            .passes
            .iter()
            .any(|pass| matches!(pass.target, PassTarget::Texture(_)))
    };
    assert!(has_image(&gfx));
    // 只投影、不 commit:下一批事件后仍须发射。
    gfx.consume_events(&[frame_start(2), frame_end(2)]);
    assert!(has_image(&gfx), "未 commit 的投影不能当作已烘焙");
    // 后端回传失败目标:该目标不提交,下一帧仍发射。
    gfx.commit_baked(&[b":/W0/SRC".to_vec()]);
    gfx.consume_events(&[frame_start(3), frame_end(3)]);
    assert!(has_image(&gfx), "失败目标必须保留重试");
    // 成功回执:提交后下一帧不再发射。
    gfx.commit_baked(&[]);
    gfx.consume_events(&[frame_start(4), frame_end(4)]);
    assert!(!has_image(&gfx), "成功执行后才可跳过历史烘焙前缀");
}

/// 已成功冻结、且目标层仍持有该快照的 COPY 不再逐帧重放;失败回执则重试。
#[test]
fn frozen_copy_is_not_replayed_every_frame() {
    let table = table();
    let mut gfx = state();
    let build = |gfx: &mut GfxState, frame: u64| {
        gfx.consume_events(&[
            frame_start(frame),
            create(&table, "SRC", "src", 1, 256),
            call(
                frame,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", s("SRC")),
                    arg(&table, "CGACT", "ID2", s("DST")),
                    arg(&table, "CGACT", "COPY2", i(1)),
                ],
            ),
            frame_end(frame),
        ]);
    };
    let copy_to_dst = |gfx: &GfxState| {
        gfx.frame().passes.iter().any(|pass| {
            pass.ops
                .iter()
                .any(|op| matches!(op, Op::Copy(copy) if copy.dest.display() == ":/W0/DST"))
        })
    };
    build(&mut gfx, 1);
    assert!(copy_to_dst(&gfx), "首帧必须发射 COPY");
    // 失败回执:不冻结,下一帧重试(后续帧不再重发 COPY 命令)。
    gfx.commit_baked(&[b":/W0/DST".to_vec()]);
    gfx.consume_events(&[frame_start(2), frame_end(2)]);
    assert!(copy_to_dst(&gfx), "失败目标必须重试");
    // 成功回执:目标层仍持有 Snapshot,命令已执行完,不再重放。
    gfx.commit_baked(&[]);
    gfx.consume_events(&[frame_start(3), frame_end(3)]);
    assert!(!copy_to_dst(&gfx), "已冻结 COPY 不应逐帧重放");
}
