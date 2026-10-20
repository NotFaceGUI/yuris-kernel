//! `A` 0..256 与固定管线/着色器两条缩放路径。

mod support;

use support::*;
use yu_gfx::{fixed_pipeline_alpha, shader_alpha, BlendMode, TextureFactorRule};

#[test]
fn fixed_pipeline_alpha_clamps_256_to_255() {
    assert_eq!(fixed_pipeline_alpha(0), 0.0);
    assert!((fixed_pipeline_alpha(1) - 1.0 / 255.0).abs() < 1e-15);
    assert!((fixed_pipeline_alpha(128) - 128.0 / 255.0).abs() < 1e-15);
    assert_eq!(fixed_pipeline_alpha(255), 1.0);
    assert_eq!(fixed_pipeline_alpha(256), 1.0);
    assert_eq!(fixed_pipeline_alpha(-5), 0.0);
}

#[test]
fn shader_alpha_uses_256_denominator() {
    assert_eq!(shader_alpha(0), 0.0);
    assert_eq!(shader_alpha(1), 1.0 / 256.0);
    assert_eq!(shader_alpha(128), 0.5);
    assert_eq!(shader_alpha(255), 255.0 / 256.0);
    assert_eq!(shader_alpha(256), 1.0);
}

#[test]
fn layer_stores_raw_alpha_without_clamping() {
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
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("B")),
                arg(&table, "CG", "FILE", s("b")),
                arg(&table, "CG", "A", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("C")),
                arg(&table, "CG", "FILE", s("c")),
                arg(&table, "CG", "A", i(0)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let get = |id: &str| {
        f.layers
            .iter()
            .find(|l| l.id.display() == id)
            .unwrap()
            .alpha
    };
    assert_eq!(get(":/W0/A"), 256);
    assert_eq!(get(":/W0/B"), 1);
    assert_eq!(get(":/W0/C"), 0);
}

#[test]
fn normal_mode_fixed_state_uses_alpha_gray_texture_factor() {
    let s = BlendMode::Normal.fixed_state().expect("fixed");
    assert_eq!(s.texture_factor, TextureFactorRule::AlphaGray);
    assert_eq!(s.src_blend.d3d_code(), 5); // SRCALPHA
    assert_eq!(s.dst_blend.d3d_code(), 6); // INVSRCALPHA
    assert_eq!(s.blend_op.d3d_code(), 1); // ADD
}

#[test]
fn alpha_does_not_change_filter_or_probe_order() {
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
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    assert_eq!(f.filter.mag.as_str(), "point");
    assert_eq!(f.image_probe_order.len(), 4);
}
