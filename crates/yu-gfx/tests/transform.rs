//! 变换/过渡精度:保留引擎单位,不提前取整。

mod support;

use support::*;
use yu_gfx::{json, LayerStatus, Op};

#[test]
fn xy_are_stored_as_engine_integers() {
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
                arg(&table, "CG", "X", i(123456789)),
                arg(&table, "CG", "Y", i(-987654321)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let l = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L")
        .unwrap();
    assert_eq!(l.transform.x, 123456789.0);
    assert_eq!(l.transform.y, -987654321.0);
    // 不做 16.16/半像素换算:gfx 只保存引擎单位。
    assert!(l.transform.affine.is_default());
}

#[test]
fn fx_fy_are_the_same_field_as_x_y_in_hundredths() {
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
                arg(&table, "CG", "FX", i(12345)),
                arg(&table, "CG", "FY", i(-600)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let l = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L")
        .unwrap();
    // 引擎 `CG[FX=n]` 写 `+0x1b0=n`、`+0x68=n/100`(0x42ab4b);
    // 内核的 `transform.x` 是 `+0x68` 口径 ⇒ 百分位 ÷100。
    assert_eq!(l.transform.x, 123.45);
    assert_eq!(l.transform.y, -6.0);
    // 仿射字段本身仍是引擎原值。
    assert!(l.transform.affine.is_default());
}

#[test]
fn affine_params_land_on_engine_fields() {
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
                arg(&table, "CG", "FBX", i(5000)),
                arg(&table, "CG", "FBY", i(2500)),
                arg(&table, "CG", "FRZ", i(3000)),
                arg(&table, "CG", "FCX", i(6400)),
                arg(&table, "CG", "FEX", i(-100)),
                arg(&table, "CG", "FSD", i(200)),
                arg(&table, "CG", "FQU", i(-1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let l = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L")
        .unwrap();
    let a = &l.transform.affine;
    // 绑定见 `Affine` 文档表(REA 0x42b10f/0x42b16f/0x42b32d/0x42b394/
    // 0x42b454/0x42a798/0x42b5ac)。
    assert_eq!(a.bx, 5000.0);
    assert_eq!(a.by, 2500.0);
    assert_eq!(a.rz, 3000.0);
    assert_eq!(a.cx, 6400.0);
    assert_eq!(a.ex, -100.0);
    assert_eq!(a.sd, 200);
    assert_eq!(a.qu, -1);
    // 未给出的字段保持引擎初值(`FUN_00465730`)。
    assert_eq!(a.md, 0x0080_8080);
    assert_eq!(a.flags, 0);
    assert!(matches!(
        l.status,
        LayerStatus::Unimplemented("gfx.affine.unverified")
    ));
    assert!(f
        .diagnostics
        .iter()
        .any(|d| d.code == "gfx.affine.unverified"));
    let ops = f.passes.iter().flat_map(|p| p.ops.iter());
    assert!(ops
        .into_iter()
        .any(|o| matches!(o, Op::Layer(x) if x.id.display() == ":/W0/L")));
}

#[test]
fn affine_defaults_match_engine_layer_constructor() {
    let a = yu_gfx::Affine::engine_default();
    assert_eq!((a.bx, a.by), (10000.0, 10000.0));
    assert_eq!(a.sd, 0x80);
    assert_eq!(a.md, 0x0080_8080);
    assert_eq!((a.rx, a.ry, a.rz), (0.0, 0.0, 0.0));
    assert!(a.is_default());
}

#[test]
fn fqu_is_plain_assignment_like_engine() {
    // 引擎 `FQU` 是**直接赋值**(0x42b592..0x42b5ac),不走属性算子
    // `0x42bd3c`;这里钉住「重复给出时是覆盖,不是累加」。
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
                arg(&table, "CG", "FQU", i(7)),
            ],
        ),
        frame_end(1),
    ]);
    gfx.consume_events(&[
        frame_start(2),
        call(
            2,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FQU", i(3)),
            ],
        ),
        frame_end(2),
    ]);
    let f = gfx.frame();
    let l = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L")
        .unwrap();
    assert_eq!(l.transform.affine.qu, 3);
}

#[test]
fn fade_formula_matches_engine_for_ta_256() {
    let t = yu_gfx::Transition {
        t: 256,
        ta: 256,
        tid: None,
        tid_raw: None,
    };
    let fade = t.fade_value().unwrap();
    // 256 / (256/(256/256+128)) / 256 = 129/256
    assert!((fade - 129.0 / 256.0).abs() < 1e-15, "fade={fade}");
}

#[test]
fn fade_formula_matches_engine_for_ta_1() {
    let t = yu_gfx::Transition {
        t: 256,
        ta: 1,
        tid: None,
        tid_raw: None,
    };
    // 256 / (256/(256/1+128)) / 256 = 384/256 = 1.5(引擎不 clamp)
    assert!((t.fade_value().unwrap() - 1.5).abs() < 1e-15);
}

#[test]
fn ta_below_one_means_no_transition() {
    let t = yu_gfx::Transition {
        t: 128,
        ta: 0,
        tid: None,
        tid_raw: None,
    };
    assert_eq!(t.fade_value(), None);
}

#[test]
fn transform_json_keeps_raw_values() {
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
                arg(&table, "CG", "X", i(7)),
                arg(&table, "CG", "FX", i(9900)),
            ],
        ),
        frame_end(1),
    ]);
    let s = json::to_json(&gfx.frame());
    // X 与 FX 是同一字段的两种单位:X=7(像素)后 FX=9900(百分位)
    // 覆盖为 99.0 像素。
    assert!(s.contains("\"x\":99"));
    assert!(s.contains("\"affine\":{\"FBX\":10000,\"FBY\":10000"));
    assert!(s.contains("\"FSD\":128"));
}
