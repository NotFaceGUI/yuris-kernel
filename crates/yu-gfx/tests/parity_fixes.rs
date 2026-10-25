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
    // `SIZE` 的 SX/SY 是浮点(实测 `SY=825.5`);整数输入经 `as_f64` 后
    // 仍是 315.0/190.0。
    assert_eq!(img.sx, Some(315.0));
    assert_eq!(img.sy, Some(190.0));

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
                arg(&table, "CG", "MODE", i(4)),
            ],
        ),
        frame_end(1),
    ]);
    assert!(matches!(
        layer(&gfx.frame(), ":/W0/L").status,
        LayerStatus::Unimplemented("gfx.blend.unsupported")
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
        LayerStatus::Executable
    ));
    assert!(matches!(
        layer(&gfx.frame(), ":/W0/DST").texture,
        TextureRef::Snapshot(_)
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
    // 主组直绘(DIV-GFX-008/041):CGID 指定的主组不再包成 Group op,
    // 它的子层直接进屏幕 pass。
    assert!(
        screen(&f).ops.iter().all(|op| !matches!(op, Op::Group(_))),
        "main layer set must be drawn directly"
    );
    assert!(
        screen(&f)
            .ops
            .iter()
            .any(|op| matches!(op, Op::Layer(l) if l.id.display() == ":/W1/X")),
        "CGID 指定的组子层必须在屏幕 pass 里"
    );

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
    // 主组直绘:W0 的子层在屏幕 pass 里(不再有 W0 离屏 pass)。
    let w0 = f
        .passes
        .iter()
        .find(|p| matches!(p.target, PassTarget::Screen))
        .expect("screen pass");
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

/// `es_Sprite.yst` 的中画质分支(`quo != 0`)只发 `FX/FY/FX2/FY2/FSX/FSY`
/// 百分位定点键,没有整数 `X/SX` 键:
///
/// ```text
/// CGACT[ID=$src ID2=$dst SIZE2=1 FX=@X1*100+@FLX FY=... FX2=... FY2=...
///        FSX=@SIZE_X*100 FSY=@SIZE_Y*100]
/// ```
///
/// 脚本注释原文:「表示位��� @LEFT_X,@LEFT_Y の小数点値を掃い、それを
/// CGACT[SIZE2=1 ～] に伝える」⇒ ÷100 回退。若不回退,整条 SIZE2 被判定
/// 「尺寸缺失」(`gfx.imageop.size.missing`)⇒ 目标层标 Unimplemented ⇒
/// 立绘/CG 永久不上屏。
#[test]
fn cgact_size_accepts_fixed_point_fx_fsx_keys() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SPLBUF")),
                arg(&table, "CG", "FILE", s("spl")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SPL")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SPLBUF")),
                arg(&table, "CGACT", "ID2", s("SPL")),
                arg(&table, "CGACT", "SIZE2", i(1)),
                arg(&table, "CGACT", "FX", f(1234.0)),
                arg(&table, "CGACT", "FY", f(5678.0)),
                arg(&table, "CGACT", "FX2", f(1234.0 + 39900.0)),
                arg(&table, "CGACT", "FY2", f(5678.0 + 29900.0)),
                arg(&table, "CGACT", "FSX", f(39900.0)),
                arg(&table, "CGACT", "FSY", f(29900.0)),
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
        .expect("SIZE2 op");
    assert_eq!(img.action, ImageAction::SizeBilinear);
    // 尺寸:FSX/FSY ÷100(`@SIZE_X*100` ⇒ 原像素)。
    assert_eq!(img.sx, Some(399.0));
    assert_eq!(img.sy, Some(299.0));
    // 区域:FX/FY/FX2/FY2 ÷100 后取整(与 `find_px` 同口径)。
    assert_eq!((img.x, img.y), (Some(12), Some(57)));
    assert_eq!((img.x2, img.y2), (Some(411), Some(356)));
    assert_eq!(img.status, OpStatus::Executable);
}

/// `SIZE4` 在语料里与 `SIZE2` 同形(同为 `SX/SY` + 可选源区域):
/// `es_ERIS.yst:631` `CGACT[ID=$id SIZE4=1 SX=@sx/2 SY=@sy/2]`、
/// `es_System.yst:261` 二分降采样。手册 `cg/cgact.html` 无 SIZE4 页
/// ⇒ `partial(no-manual)`;几何语义按缩放执行,不再毒化整层。
#[test]
fn cgact_size4_executes_as_scale() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BIG")),
                arg(&table, "CG", "FILE", s("big")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BIG")),
                arg(&table, "CGACT", "SIZE4", i(1)),
                arg(&table, "CGACT", "SX", i(400)),
                arg(&table, "CGACT", "SY", i(300)),
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
        .expect("SIZE4 op");
    assert_eq!(img.action, ImageAction::Other("size4"));
    assert!(img.action.is_scale());
    assert_eq!((img.sx, img.sy), (Some(400.0), Some(300.0)));
    assert_eq!(img.status, OpStatus::Executable);
    assert_eq!(layer(&f, ":/W0/BIG").status, LayerStatus::Executable);
}

/// 同一 `(源, 目标)` 的重复缩放必须折叠:引擎的 `CGACT` 加工是立即执行的破坏性
/// 写入,旧版本像素不会再被看到;本内核逐帧重放若不折叠,`image_log` 线性增长
/// ⇒ `frame()` 每帧产出更多 pass ⇒ 渲染耗时随游玩时长上升(实测 passes
/// 103→139、skip 累计到 336)。
///
/// 但原地加工(`ID` 无 `ID2`)会叠加,必须保留历史。
#[test]
fn repeated_scale_ops_collapse_but_in_place_ops_stay() {
    let table = table();
    let mut gfx = state();
    let scale = |sx: i64| {
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(sx)),
                arg(&table, "CGACT", "SY", i(sx)),
            ],
        )
    };
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
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        scale(100),
        scale(200),
        scale(300),
        frame_end(1),
    ]);
    let imgs: Vec<_> = gfx
        .frame()
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter_map(|o| match o {
            Op::Image(i) => Some(i.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(imgs.len(), 1, "同源同目标的重复缩放应折叠为最后一条");
    assert_eq!(imgs[0].sx, Some(300.0));

    // 原地缩放(源 = 目标):每次都要在上一版像素上再缩放,不能折叠。
    gfx.consume_events(&[
        frame_start(2),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SELF")),
                arg(&table, "CG", "FILE", s("self")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SELF")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(50)),
                arg(&table, "CGACT", "SY", i(50)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SELF")),
                arg(&table, "CGACT", "SIZE", i(1)),
                arg(&table, "CGACT", "SX", i(25)),
                arg(&table, "CGACT", "SY", i(25)),
            ],
        ),
        frame_end(2),
    ]);
    let self_imgs = gfx
        .frame()
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter(|o| matches!(o, Op::Image(i) if i.source.display() == ":/W0/SELF"))
        .count();
    assert_eq!(self_imgs, 2, "原地缩放必须逐次保留");
}
