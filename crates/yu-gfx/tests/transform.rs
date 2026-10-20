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
    assert_eq!(l.transform.x, 123456789);
    assert_eq!(l.transform.y, -987654321);
    // 不做 16.16/半像素换算:gfx 只保存引擎单位。
    assert_eq!(l.transform.filter.values.len(), 0);
}

#[test]
fn filter_params_are_raw_and_mark_unimplemented() {
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
                arg(&table, "CG", "FRZ", i(3000)),
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
    assert_eq!(
        l.transform
            .filter
            .values
            .get(&yu_gfx::FilterParam::Fx)
            .and_then(|v| v.as_int()),
        Some(12345)
    );
    assert_eq!(
        l.transform
            .filter
            .values
            .get(&yu_gfx::FilterParam::Frz)
            .and_then(|v| v.as_int()),
        Some(3000)
    );
    assert!(matches!(
        l.status,
        LayerStatus::Unimplemented("gfx.filter.unsupported")
    ));
    assert!(f
        .diagnostics
        .iter()
        .any(|d| d.code == "gfx.filter.unsupported"));
    let ops = f.passes.iter().flat_map(|p| p.ops.iter());
    assert!(ops
        .into_iter()
        .any(|o| matches!(o, Op::Layer(x) if x.id.display() == ":/W0/L")));
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
    assert!(s.contains("\"x\":7"));
    assert!(s.contains("{\"name\":\"FX\",\"value\":{\"t\":\"int\",\"v\":9900}}"));
}
