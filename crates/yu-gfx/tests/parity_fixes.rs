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
    // 区域:FX/FY/FX2/FY2 ÷100,**保留亚像素**(不取整)。
    // 精灵系统把位置拆成整数部分 + 小数部分(`FX = @X1*100 + @FLX`),
    // 取整会把慢速移动量化成阶梯抖动;亚像素值直接进 UV 采样。
    // `FX=1234 ⇒ 12.34`:小数部分必须保留(取整会让慢速移动抖成阶梯)。
    assert_eq!((img.x, img.y), (Some(12.34), Some(56.78)));
    assert_eq!((img.x2, img.y2), (Some(411.34), Some(355.78)));
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

/// `CGACT[TRANSITION=1 SET=f ID=from ID2=to ID3=dst]`:
/// 两源按 `f`(0..256)插值写成**第三层**。
///
/// 语料唯一用法 `es_Sprite.yst:7978`
/// `CGACT[TRANSITION=1 SET=@L.F2(@LNO) ID=$LB1 ID2=$LB2 ID3=$LB3]`:
/// `$LB1`=上一帧采样结果、`$LB2`=目标图、`$LB3`=本帧采样源;产物随后被
/// `es.CGACT_SIZE` 缩放到可见层。缺这条 ⇒ 动画分支的输出层永远全透明
/// ⇒「有动画的立绘/背景不显示,静态 CG 正常」。
///
/// 注意参数方向:这里 `ID2` 是**第二输入**(不是目标),目标在 `ID3`。
#[test]
fn cgact_transition_blends_two_sources_into_id3() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("FROM")),
                arg(&table, "CG", "FILE", s("from")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("TO")),
                arg(&table, "CG", "FILE", s("to")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("FROM")),
                arg(&table, "CGACT", "ID2", s("TO")),
                arg(&table, "CGACT", "ID3", s("DST")),
                arg(&table, "CGACT", "TRANSITION", i(1)),
                arg(&table, "CGACT", "SET", i(128)),
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
            Op::Image(i) if matches!(&i.action, ImageAction::Other("transition")) => Some(i),
            _ => None,
        })
        .expect("transition op 必须被登记(而不是判未实现后丢弃)");
    assert_eq!(img.source.display(), ":/W0/FROM");
    assert_eq!(
        img.source2.as_ref().map(|l| l.display()),
        Some(":/W0/TO".to_string()),
        "ID2 是第二输入"
    );
    assert_eq!(
        img.target.as_ref().map(|l| l.display()),
        Some(":/W0/DST".to_string()),
        "ID3 才是输出层"
    );
    assert_eq!(img.set.as_ref().and_then(|v| v.as_int()), Some(128));
    assert_eq!(img.status, OpStatus::Executable);
    // 输出层必须存在且未被毒化(否则后续 SIZE 拿不到图)。
    assert_eq!(layer(&f, ":/W0/DST").status, LayerStatus::Executable);
}

/// `VERTICAL` 与 `HORIZONTAL` 同类(整层几何翻转),必须判可执行而不是毒化。
/// 语料 `es_Sprite.yst:8233/8240` 成对出现,各自由 `IF[@POSTF_*]` 门控。
#[test]
fn cgact_vertical_executes_like_horizontal() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("V")),
                arg(&table, "CG", "FILE", s("v")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("V")),
                arg(&table, "CGACT", "VERTICAL", i(1)),
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
        .expect("VERTICAL op");
    assert_eq!(img.action, ImageAction::Vertical);
    assert_eq!(img.status, OpStatus::Executable);
    assert_eq!(layer(&f, ":/W0/V").status, LayerStatus::Executable);
}

/// `RESIZE` / `TRIM` / `MEIDO` 都是**单层原地**动作(手册例 7/21/22 均无 ID2),
/// 必须判可执行而不是毒化整层。
///
/// 语料 live 用量(`es_Sprite.yst`):`RESIZE` 12 次(含 `:5877` 立绘加载路径的
/// 无条件调用)、`SAIDO` 5、`MEIDO` 5。未实现时它们会把源层标成
/// `Unimplemented` ⇒ 该层永不绘制。
#[test]
fn cgact_resize_trim_meido_execute_in_place() {
    let table = table();
    // 各动作按手册例子的**真实形态**构造:
    // - 例 21 RESIZE:`RESIZE=1 X=10 Y=20 X2=-30 Y2=-40`
    // - 例 22 TRIM:只有 `SET=15`
    // - 例 7  MEIDO:只有 `SET=0xffff00`
    for (action, extra, with_edges) in [
        ("RESIZE", None, true),
        ("TRIM", Some(15i64), false),
        ("MEIDO", Some(0x808080i64), false),
    ] {
        let mut gfx = state();
        let mut args = vec![
            arg(&table, "CGACT", "ID", s("L")),
            arg(&table, "CGACT", action, i(1)),
        ];
        if with_edges {
            args.push(arg(&table, "CGACT", "X", i(10)));
            args.push(arg(&table, "CGACT", "Y", i(20)));
            args.push(arg(&table, "CGACT", "X2", i(-30)));
            args.push(arg(&table, "CGACT", "Y2", i(-40)));
        }
        if let Some(v) = extra {
            args.push(arg(&table, "CGACT", "SET", i(v)));
        }
        gfx.consume_events(&[
            frame_start(1),
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", s("L")),
                    arg(&table, "CG", "FILE", s("l")),
                ],
            ),
            call(1, "CGACT", args),
            frame_end(1),
        ]);
        let f = gfx.frame();
        let img = f
            .passes
            .iter()
            .flat_map(|p| p.ops.iter())
            .find_map(|o| match o {
                Op::Image(img) => Some(img),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{action} op 必须被登记"));
        assert_eq!(img.status, OpStatus::Executable, "{action} 应可执行");
        assert_eq!(
            layer(&f, ":/W0/L").status,
            LayerStatus::Executable,
            "{action} 不得毒化源层"
        );
    }
}

/// 手册例 5..17 与 yscm 表内全部逐层滤镜动作,都必须判**可执行**,不得毒化。
///
/// 「手册无页」的键(`SEPIA`/`XOR`/`AND`/`RASTER2..3`/`BLUR2..6`/`ROTATION2`)
/// 按同族约定实现(见 `DIV-GFX-062`),等级是 `partial(no-manual)`,
/// 但**不允许残留 Unimplemented**。
#[test]
fn all_layer_filter_actions_are_executable() {
    let table = table();
    // 按手册例子给参数;无页键用同族默认值。
    let cases: &[(&str, &[(&str, i64)])] = &[
        ("ROTATION", &[("SET", 30)]),
        ("SAIDO", &[("SET", 64)]),
        ("MEIDO", &[("SET", 0x00ff_ff00)]),
        ("AKARUSA", &[("SET", 192)]),
        ("MONO", &[("SET", 160)]),
        ("MOSAIC", &[("SET", 4)]),
        ("BLUR", &[("SET", 8)]),
        ("DIFFUSION", &[("SET", 8)]),
        ("REVERSE", &[]),
        ("WHIRLPOOL", &[("SET", 3000)]),
        ("RASTER", &[("SET", 5), ("SET2", 1)]),
        ("RASTER2", &[("SET", 5), ("SET2", 1)]),
        ("RASTER3", &[("SET", 5), ("SET2", 1)]),
        ("BLUR2", &[("SET", 4)]),
        ("BLUR3", &[("SET", 4)]),
        ("BLUR4", &[("SET", 4)]),
        ("BLUR5", &[("SET", 4)]),
        ("BLUR6", &[("SET", 4)]),
        ("ROTATION2", &[("SET", 45)]),
        ("SEPIA", &[]),
        ("VERTICAL", &[]),
        ("HORIZONTAL", &[]),
        ("TRIM", &[("SET", 15)]),
    ];
    for (action, params) in cases {
        let mut gfx = state();
        let mut args = vec![
            arg(&table, "CGACT", "ID", s("F")),
            arg(&table, "CGACT", action, i(1)),
        ];
        for (name, v) in *params {
            args.push(arg(&table, "CGACT", name, i(*v)));
        }
        gfx.consume_events(&[
            frame_start(1),
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", s("F")),
                    arg(&table, "CG", "FILE", s("f")),
                ],
            ),
            call(1, "CGACT", args),
            frame_end(1),
        ]);
        let f = gfx.frame();
        let img = f
            .passes
            .iter()
            .flat_map(|p| p.ops.iter())
            .find_map(|o| match o {
                Op::Image(img) => Some(img),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{action} op 必须被登记"));
        assert_eq!(img.status, OpStatus::Executable, "{action} 应可执行");
        assert_eq!(
            layer(&f, ":/W0/F").status,
            LayerStatus::Executable,
            "{action} 不得毒化图层"
        );
    }
}

/// `MERGE` / `MERGE2` / `XOR` / `AND`:两层合成,`ID` 上层、`ID2` 下层、结果写 `ID2`。
/// 手册例 23/24 原文明示「結合した結果はレイヤ「BG2」(=ID2)のほうに格納」。
#[test]
fn layer_composite_actions_write_to_id2() {
    let table = table();
    for action in ["MERGE", "MERGE2", "XOR", "AND"] {
        let mut gfx = state();
        gfx.consume_events(&[
            frame_start(1),
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", s("UP")),
                    arg(&table, "CG", "FILE", s("up")),
                ],
            ),
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", s("LOW")),
                    arg(&table, "CG", "FILE", s("low")),
                ],
            ),
            call(
                1,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", s("UP")),
                    arg(&table, "CGACT", "ID2", s("LOW")),
                    arg(&table, "CGACT", action, i(1)),
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
                Op::Image(img) => Some(img),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{action} op"));
        assert_eq!(img.status, OpStatus::Executable, "{action} 应可执行");
        assert_eq!(
            img.target.as_ref().map(|t| t.display()),
            Some(":/W0/LOW".to_string()),
            "{action} 结果必须写 ID2"
        );
        assert_eq!(layer(&f, ":/W0/LOW").status, LayerStatus::Executable);
    }
}

/// `CGEND` 删除本就不存在的层 = 幂等空操作(`Verbose`,默认不显示)。
///
/// 证据:ERIS 精灵清理助手 `es.SP.DEL2.SET`(`yst00066` pc2008..)对每个
/// 精灵槽**无条件**发 `CGEND`,不先查 `EXIST`;演出期实测刷出 1301 条
/// WARN。后置条件在删除前已成立 ⇒ 与成功等价。
///
/// 级别选 `Verbose` 而非 `Info`:这不是运维信息,更不是告警 —— 默认输出里
/// 出现 1300+ 条只会训练使用者忽略日志。只有 `--log-level verbose` 才看得到。
#[test]
fn cgend_on_absent_layer_is_idempotent_verbose() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", s("NEVER_CREATED"))],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let has_warn = f
        .diagnostics
        .iter()
        .any(|d| d.severity == "Recoverable");
    assert!(!has_warn, "删除不存在的层不应产生 Recoverable 告警");
    let entry = f
        .diagnostics
        .iter()
        .find(|d| d.code == "gfx.layer.already_absent")
        .expect("应有幂等删除的留痕记录");
    assert_eq!(entry.severity, "Verbose", "幂等删除属最低档,默认不显示");
}

/// `CGINFO[TEX]` 读回 `CG[TEX=]` 写入的值;层不存在时回 0(类型探测语义)。
///
/// 证据:`es_Sprite.yst` 源码
/// - `CG[ID=... TEX=@_DXMODE]` 写入(`:4091/4144/4396/5280/...`);
/// - `CGINFO[ID=... TEX=1 LET=@texchk]` 读回(`:3454/4155/5848/7770`);
/// - `IF[@texchk==2]` 四处的分支注释为「E-moteレイヤなら」⇒ **2 = E-mote**。
///
/// 这四处 ID 由 `$LIDBUF+槽号` 拼出,未装载的槽自然不存在,且中间没有
/// `EXIST` 前置检查 ⇒ 缺层必须答 0 而不是报 `gfx.info.missing`。
#[test]
fn cginfo_tex_reads_back_and_absent_is_zero() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SP")),
                arg(&table, "CG", "FILE", s("cg/bg/x")),
                arg(&table, "CG", "TEX", i(2)),
            ],
        ),
        frame_end(1),
    ]);
    let q = |gfx: &yu_gfx::GfxState, id: &str| {
        gfx.query_info(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s(id)),
                arg(&table, "CGINFO", "TEX", i(1)),
            ],
        )
    };
    assert_eq!(q(&gfx, "SP").expect("TEX 查询"), yu_rt::value::Value::Int(2));
    assert_eq!(
        q(&gfx, "NOT_LOADED").expect("缺层 TEX 应回 0 而不是报错"),
        yu_rt::value::Value::Int(0)
    );
}

/// `LINT`/`REMALLOC` 等非视觉参数原样留档,不再报 Unimplemented。
///
/// 它们不进像素、不进 `CGINFO` 返回值,留档即完整语义(手册 `cg/cg.html`
/// 无页;语料里以固定常量出现)。
#[test]
fn nonvisual_cg_params_are_archived_without_unimplemented() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("cg/x")),
                arg(&table, "CG", "LINT", i(3)),
                arg(&table, "CG", "REMALLOC", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let diag = f
        .diagnostics
        .iter()
        .find(|d| d.code == "gfx.param.nonvisual");
    assert!(diag.is_none(), "非视觉参数不应报 Unimplemented");
    let layer = layer(&f, ":/W0/L");
    assert_eq!(layer.extra.get("LINT"), Some(&yu_gfx::EngineValue::Int(3)));
    assert_eq!(layer.extra.get("REMALLOC"), Some(&yu_gfx::EngineValue::Int(1)));
}

/// `CG[… X+=v]`(参数种类 2)是**增量**,不是赋值。
///
/// 证据:0.495 窗口 `+2` 的 u16 高字节 = 官方参数类型字节 ——
/// `CG[… X=…]` 的窗口 slot=1(种类 0),`CG[… X+=…]` 的窗口 slot=257
/// (0x0101,种类 2)。语料 `es_Sprite.yst:8173`
/// `CG[ID=$L1 X+=(@es.GSX/2)-(@WSX/2) Y+=…]` 每帧在
/// `es.CGACT_SIZE` 写入的绝对位置上做旋转画布补偿;若按赋值处理,
/// 补偿量 0 会把位置清成 0 —— 立绘被钉在屏幕左上(2026-10-26 实测)。
#[test]
fn cg_numeric_offset_params_accumulate() {
    let table = table();
    let mut gfx = state();
    let offset_arg = |cmd: &str, name: &str, value: yu_rt::value::Value| {
        let mut a = arg(&table, cmd, name, value);
        a.offset = true;
        a
    };
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("cg/x")),
                arg(&table, "CG", "X", f(240.5)),
                arg(&table, "CG", "Y", f(70.25)),
                arg(&table, "CG", "A", i(200)),
            ],
        ),
        // 零增量不得清零位置
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                offset_arg("CG", "X", f(0.0)),
                offset_arg("CG", "Y", f(0.0)),
            ],
        ),
        // 非零增量累加(含负数)
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                offset_arg("CG", "X", f(1.25)),
                offset_arg("CG", "Y", f(-2.5)),
                offset_arg("CG", "A", i(-50)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let layer = layer(&f, ":/W0/L");
    assert_eq!(layer.transform.x, 241.75);
    assert_eq!(layer.transform.y, 67.75);
    assert_eq!(layer.alpha, 150);
}
