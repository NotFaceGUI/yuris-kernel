//! 过渡双纹理与 FadeVal。

mod support;

use support::*;
use yu_gfx::{LayerId, LayerStatus, Op, TextureRef};

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
fn missing_tid_falls_back_to_single_texture_layer() {
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
    // EVID-T-03:配对缺失 = 单纹理普通层(asm 0x415558 → 0x416f8d;
    // 官方微场景 j2-micro-trans1 TR7 输出正常图层)。
    assert!(!f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .any(|o| matches!(o, Op::Transition(_))));
    let layer = f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|o| match o {
            Op::Layer(l) if l.id.display() == ":/W0/A" => Some(l),
            _ => None,
        })
        .expect("normal layer");
    assert_eq!(layer.texture, TextureRef::Asset(b"a".to_vec()));
    assert!(matches!(layer.status, LayerStatus::Executable));
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

/// 转场取样只看**图层状态**,不看图层名字(EVID-T-10)。
///
/// ERIS 精灵系统把 `SPLnnn` 当可见占位层、把真实像素做进 `SPLBUFnnn`,
/// 但那是脚本用 `CGACT[SIZE]` 把缓冲层内容写进可见层实现的;引擎本身
/// 只认 `YS_Texture1 = 本层纹理` / `YS_Texture2 = TID 纹理`。
/// 因此这里验证:可见层自身纹理就是 from,TID 层纹理就是 to,
/// 二者换名后行为完全相同(没有任何 `SPL*` 特判)。
#[test]
fn transition_uses_layer_state_not_layer_names() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("FROM")),
                arg(&table, "CG", "FILE", s("logo")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("MASK")),
                arg(&table, "CG", "FILE", s("mask")),
                arg(&table, "CG", "E", i(0)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("VIS")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(128)),
                arg(&table, "CG", "TID", s("MASK")),
            ],
        ),
        // 占位层由脚本自己写入真实像素(`CGACT[SIZE]`)后才是 from。
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("FROM")),
                arg(&table, "CGACT", "ID2", s("VIS")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(800)),
                arg(&table, "CGACT", "SY", i(600)),
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
            Op::Transition(t) if t.layer.id.display() == ":/W0/VIS" => Some(t),
            _ => None,
        })
        .expect("VIS transition");
    // from_layer 必须指向**可见层自己**(后端据此优先取它的图像加工结果),
    // TID 解析出的 mask 层纹理作为 to。
    assert_eq!(
        op.from_layer.as_ref().map(|l| l.display()),
        Some(":/W0/VIS".to_string())
    );
    assert_eq!(op.to, Some(TextureRef::Asset(b"mask".to_vec())));
    // 该 SIZE 也把可见层的绘制尺寸更新为 800x600(手册 cg/cgact.html)。
    let vis = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/VIS")
        .expect("VIS layer");
    assert_eq!(
        vis.image_size().map(|s| (s.width, s.height)),
        Some((800, 600))
    );
}

#[test]
fn group_tid_emits_transition_from_offscreen() {
    // 引擎按层表统一解析 TID(组也是层);组离屏纹理作为 YS_Texture1。
    // 未取证到官方微场景前标 Likely,但不得静默当普通组画掉。
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(":/W0/G/")),
                arg(&table, "CG", "SX", i(120)),
                arg(&table, "CG", "SY", i(80)),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(256)),
                arg(&table, "CG", "TID", s("B")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(":/W0/G/C")),
                arg(&table, "CG", "FILE", s("c")),
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
            Op::Transition(t) if t.layer.id.display() == ":/W0/G" => Some(t),
            _ => None,
        })
        .expect("group transition");
    assert_eq!(
        op.from,
        TextureRef::Offscreen(LayerId::new(b":/W0/G".to_vec()))
    );
    assert_eq!(op.to, Some(TextureRef::Asset(b"b".to_vec())));
    assert!(!f
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .any(|o| matches!(o, Op::Group(g) if g.layer.id.display() == ":/W0/G")));
}

/// `TIDNO`:REA CG 处理体 `0x42a8e3..0x42a936` 把 TID 拷入缓冲后追加
/// 常量 `"."`(`0x808354`)再 `__ultoa` 基数 10 ⇒ 查表键 `TID.N`。
#[test]
fn tidno_appends_decimal_suffix_for_pairing() {
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
                arg(&table, "CG", "TIDNO", i(2)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("B.2")),
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
        .expect("TID.N 配对应产出过渡 op");
    assert_eq!(op.to, Some(TextureRef::Asset(b"b".to_vec())));
    assert_eq!(
        op.to_layer.as_ref().map(LayerId::display),
        Some(":/W0/B.2".into())
    );
    // CGINFO[TID] 返回脚本原文(不含 TIDNO 后缀)。
    let raw = gfx
        .query_info(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s("A")),
                arg(&table, "CGINFO", "TID", i(1)),
            ],
        )
        .expect("TID 查询");
    assert_eq!(raw, yu_rt::value::Value::Str(b"B".to_vec()));
}

#[test]
fn invisible_buffer_reverse_is_display_only_for_transition() {
    // <v481-x> 形态:mask 载入 E=0 的缓冲层 → REVERSE=1 → SIZE 进空壳配对层。
    // 官方转场看的是配对层状态,源缓冲层的反色不参与采样。
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("TRBUF")),
                arg(&table, "CG", "FILE", s("mask")),
                arg(&table, "CG", "E", i(0)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("TRBUF")),
                arg(&table, "CGACT", "REVERSE", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("TR")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(8)),
                arg(&table, "CG", "SY", i(8)),
            ],
        ),
        // SIZE 的 `ID` 是源、`ID2` 是目标(见 state.rs `apply_image` 注释)。
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("TRBUF")),
                arg(&table, "CGACT", "ID2", s("TR")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(8)),
                arg(&table, "CGACT", "SY", i(8)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("a")),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(256)),
                arg(&table, "CG", "TID", s("TR")),
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
        .expect("mask 链应产出过渡 op");
    assert_eq!(op.to_base, Some(TextureRef::Asset(b"mask".to_vec())));
    assert!(!op.mask_transform.reverse, "源缓冲层 REVERSE 不参与采样");
    assert!(!op.mask_transform.flip_h);
    assert!(!op.mask_transform.flip_v);
}

#[test]
fn pair_layer_reverse_is_carried_to_transition() {
    // pieces `LOADTR.REV` 形态:配对层自身 REVERSE=1,psC 采样随之翻转。
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("M")),
                arg(&table, "CG", "FILE", s("mask")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("M")),
                arg(&table, "CGACT", "REVERSE", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("a")),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(256)),
                arg(&table, "CG", "TID", s("M")),
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
        .expect("配对层应产出过渡 op");
    assert_eq!(op.to_base, Some(TextureRef::Asset(b"mask".to_vec())));
    assert!(op.mask_transform.reverse, "配对层自身的 REVERSE 参与采样");
    assert!(!op.mask_transform.flip_h);
    assert!(!op.mask_transform.flip_v);
}

#[test]
fn pieces_pair_reverse_wins_over_buffer_double_reverse() {
    // 2026-11-24 实测形态(pieces SPLTR101 正向):
    // 缓冲层两次 REVERSE(净偶)+ 配对层一次 REVERSE → 采样翻。
    // 若把源层奇偶并进来会抵掉配对层的翻,方向反而错。
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BUF")),
                arg(&table, "CG", "FILE", s("mask")),
                arg(&table, "CG", "E", i(0)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "REVERSE", i(1)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "REVERSE", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("TR")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(8)),
                arg(&table, "CG", "SY", i(8)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "ID2", s("TR")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(8)),
                arg(&table, "CGACT", "SY", i(8)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("TR")),
                arg(&table, "CGACT", "REVERSE", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("a")),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(256)),
                arg(&table, "CG", "TID", s("TR")),
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
        .expect("mask 链应产出过渡 op");
    assert_eq!(op.to_base, Some(TextureRef::Asset(b"mask".to_vec())));
    assert!(op.mask_transform.reverse, "配对层 REVERSE 决定方向");
    assert!(!op.mask_transform.flip_h);
    assert!(!op.mask_transform.flip_v);
}

#[test]
fn visible_size_source_reverse_is_not_carried_to_transition() {
    // 可见缓冲层走 REVERSE → SIZE 时,加工对官方是可见显示;
    // 转场采基图,采样同样不翻。
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BUF")),
                arg(&table, "CG", "FILE", s("mask")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "REVERSE", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("TR")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(8)),
                arg(&table, "CG", "SY", i(8)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "ID2", s("TR")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(8)),
                arg(&table, "CGACT", "SY", i(8)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("a")),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(256)),
                arg(&table, "CG", "TID", s("TR")),
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
        .expect("mask 链应产出过渡 op");
    assert_eq!(op.to_base, Some(TextureRef::Asset(b"mask".to_vec())));
    assert!(!op.mask_transform.reverse, "可见源层的 REVERSE 不参与采样");
    assert!(!op.mask_transform.flip_h);
    assert!(!op.mask_transform.flip_v);
}

/// 对照:有像素的源仍按官方语义把目标改指向复制快照。
#[test]
fn copy2_from_asset_source_still_repoints_destination() {
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
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("A")),
                arg(&table, "CGACT", "ID2", s("B")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let b = f
        .layers
        .iter()
        .find(|layer| layer.id.display() == ":/W0/B")
        .expect("B 层");
    assert!(
        matches!(b.texture, TextureRef::Snapshot(_)),
        "有像素的源必须正常执行 COPY2"
    );
}
