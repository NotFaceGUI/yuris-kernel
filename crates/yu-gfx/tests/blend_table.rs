//! `MODE=0..18` 的固定管线状态表与取证等级。

use yu_gfx::json;
use yu_gfx::{BlendConfidence, BlendMode, TextureFactorRule};

#[test]
fn all_nineteen_modes_round_trip() {
    for (code, mode) in BlendMode::ALL.iter().enumerate() {
        assert_eq!(mode.code() as usize, code);
        assert_eq!(BlendMode::from_code(code as i64), Some(*mode));
    }
    assert_eq!(BlendMode::from_code(-1), None);
    assert_eq!(BlendMode::from_code(19), None);
}

#[test]
fn fixed_pipeline_modes_are_exactly_the_eight_plus_one() {
    for mode in BlendMode::ALL {
        let fixed = mode.fixed_state().is_some();
        let expected = matches!(mode.code(), 0 | 1 | 2 | 3 | 5 | 6 | 7 | 9);
        assert_eq!(fixed, expected, "mode {}", mode.code());
        assert_eq!(!mode.needs_shader(), expected, "mode {}", mode.code());
    }
}

#[test]
fn mode2_subtract_state_matches_engine() {
    let s = BlendMode::Subtract.fixed_state().unwrap();
    // FUN_00414ba4 行 510-560
    assert_eq!(s.stages[0].color_op.d3d_code(), 4); // MODULATE
    assert_eq!(s.stages[0].color_arg1.unwrap().d3d_code(), 0); // DIFFUSE
    assert_eq!(s.stages[0].color_arg2.unwrap().d3d_code(), 2); // TEXTURE
    assert_eq!(s.stages[0].alpha_op.d3d_code(), 4);
    assert_eq!(s.stages[0].alpha_arg1.unwrap().d3d_code(), 0);
    assert_eq!(s.stages[0].alpha_arg2.unwrap().d3d_code(), 2);
    assert_eq!(s.stages[1].color_op.d3d_code(), 1); // DISABLE
    assert_eq!(s.src_blend.d3d_code(), 5); // SRCALPHA
    assert_eq!(s.dst_blend.d3d_code(), 2); // ONE
    assert_eq!(s.blend_op.d3d_code(), 3); // REVSUBTRACT
}

#[test]
fn mode1_add_state_matches_engine() {
    let s = BlendMode::Add.fixed_state().unwrap();
    assert_eq!(s.stages[0].alpha_op.d3d_code(), 4);
    // FUN_00414ba4 行 484:TSS(0,6,0) = ALPHAARG2 DIFFUSE(不是 TFACTOR)
    assert_eq!(s.stages[0].alpha_arg1.unwrap().d3d_code(), 2); // TEXTURE
    assert_eq!(s.stages[0].alpha_arg2.unwrap().d3d_code(), 0); // DIFFUSE
    assert_eq!(s.stages[1].color_op.d3d_code(), 1);
    assert_eq!(s.src_blend.d3d_code(), 5);
    assert_eq!(s.dst_blend.d3d_code(), 2);
    assert_eq!(s.blend_op.d3d_code(), 1);
}

#[test]
fn mode3_multiply_uses_opaque_white_factor_and_srccolor_dst() {
    let s = BlendMode::Multiply.fixed_state().unwrap();
    assert_eq!(s.texture_factor, TextureFactorRule::OpaqueWhite);
    assert_eq!(s.src_blend.d3d_code(), 1); // ZERO
    assert_eq!(s.dst_blend.d3d_code(), 3); // SRCCOLOR
                                           // 行 572-607:TSS(0,2,0)/TSS(0,5,0) = DIFFUSE
    assert_eq!(s.stages[0].color_arg1.unwrap().d3d_code(), 0);
    assert_eq!(s.stages[0].alpha_arg1.unwrap().d3d_code(), 0);
    assert_eq!(s.stages[1].color_op.d3d_code(), 16); // MODULATEALPHA_ADDCOLOR
    assert_eq!(s.stages[1].color_arg2.unwrap().d3d_code(), 2); // TEXTURE
}

#[test]
fn mode5_screen_and_mode7_darken_match_engine() {
    let screen = BlendMode::Screen.fixed_state().unwrap();
    assert_eq!(screen.src_blend.d3d_code(), 10); // INVDESTCOLOR
    assert_eq!(screen.dst_blend.d3d_code(), 2); // ONE
    assert_eq!(screen.stages[1].color_op.d3d_code(), 16);
    assert_eq!(screen.stages[1].color_arg2.unwrap().d3d_code(), 2); // TEXTURE

    let darken = BlendMode::Darken.fixed_state().unwrap();
    assert_eq!(darken.texture_factor, TextureFactorRule::OpaqueWhite);
    assert_eq!(darken.blend_op.d3d_code(), 4); // MIN
                                               // 行 779:TSS(1,3,3) = COLORARG2 TFACTOR
    assert_eq!(darken.stages[1].color_arg2.unwrap().d3d_code(), 3);
}

#[test]
fn mode6_and_mode9_match_engine() {
    let lighten = BlendMode::Lighten.fixed_state().unwrap();
    assert_eq!(lighten.blend_op.d3d_code(), 5); // MAX
    assert_eq!(lighten.src_blend.d3d_code(), 2); // ONE
    assert_eq!(lighten.dst_blend.d3d_code(), 2);

    let exclusion = BlendMode::Exclusion.fixed_state().unwrap();
    assert_eq!(exclusion.src_blend.d3d_code(), 10); // INVDESTCOLOR
    assert_eq!(exclusion.dst_blend.d3d_code(), 4); // INVSRCCOLOR
}

#[test]
fn confidence_reflects_evidence_level() {
    assert_eq!(BlendMode::Normal.confidence(), BlendConfidence::Likely);
    assert_eq!(BlendMode::Add.confidence(), BlendConfidence::Confirmed);
    assert_eq!(BlendMode::Subtract.confidence(), BlendConfidence::Confirmed);
    for mode in [
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::Exclusion,
    ] {
        assert_eq!(mode.confidence(), BlendConfidence::Unverified, "{mode:?}");
    }
    for code in [4, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18] {
        let mode = BlendMode::from_code(code).unwrap();
        assert_eq!(mode.confidence(), BlendConfidence::Unimplemented, "{code}");
        assert!(mode.needs_shader());
    }
}

#[test]
fn blend_table_json_is_stable_and_complete() {
    let a = json::blend_table_json();
    let b = json::blend_table_json();
    assert_eq!(a, b);
    for code in 0..=18 {
        assert!(a.contains(&format!("\"code\":{code},")), "missing {code}");
    }
    assert_eq!(a.matches("\"fixed\":{").count(), 8);
    assert_eq!(a.matches("\"fixed\":null").count(), 11);
}
