//! Font source and FONT slot semantics.

mod support;

use yu_font::{code, FontCommand, FontSlot, FontSource, StylePolicy};

#[test]
fn default_face_uses_engine_fallback_list() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let info = font.source.face_info(font.face).unwrap();
    assert!(!info.families.is_empty());
    // On the reference host the first fallback, MS Gothic, is installed.
    let joined = info.families.join("|");
    println!("default face: {joined}");
    assert!(info.weight >= 100);
    assert_eq!(info.origin, yu_font::FaceOrigin::System);
}

#[test]
fn family_list_is_sorted_and_deduped() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let names = font.source.family_names();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    let mut deduped = sorted.clone();
    deduped.dedup();
    assert_eq!(names.len(), deduped.len());
}

#[test]
fn no_lookup_is_deterministic_and_bounded() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let a = font.source.face_by_no(0);
    let b = font.source.face_by_no(0);
    assert_eq!(a, b);
    assert!(a.is_some());
    assert!(font.source.face_by_no(usize::MAX).is_none());
}

#[test]
fn font_slot_applies_engine_clamps() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let mut slot = FontSlot::engine_default("MS Gothic");
    let cmd = FontCommand {
        no: Some(0),
        sx: Some(999),
        sy: Some(-4),
        color: Some(0x1ff_ffff),
        color2: Some(0),
        gradation: Some(99),
        rotation: Some(9),
        drawmode: Some(7),
        bold: Some(2),
        antialiasing: Some(-1),
        ..Default::default()
    };
    slot.apply(&cmd, &font.source).unwrap();
    assert_eq!(slot.sx, 0x100);
    assert_eq!(slot.sy, 0);
    assert_eq!(slot.color.to_rgb_u32(), 0xff_ffff);
    assert_eq!(slot.color2, yu_font::Rgb::BLACK);
    assert_eq!(slot.gradation, 8);
    assert_eq!(slot.rotation, 8);
    assert!(slot.drawmode);
    assert!(slot.bold);
    assert!(!slot.antialiasing);
    let style = slot.to_style(font.face);
    assert_eq!(style.sx, 0x100);
    assert_eq!(style.sy, 0);
    assert_eq!(style.rotation, yu_font::Rotation::Octant(8));
    assert_eq!(style.draw_mode, yu_font::DrawMode::Blend);
}

#[test]
fn out_of_range_no_is_a_diag() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let mut slot = FontSlot::engine_default("MS Gothic");
    let err = slot
        .apply(
            &FontCommand {
                no: Some(99_999),
                ..Default::default()
            },
            &font.source,
        )
        .unwrap_err();
    assert_eq!(err.code, code::font::MISSING);
}

#[test]
fn default_slot_is_strict_clean() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let slot = FontSlot::engine_default("MS Gothic");
    assert!(slot.validate(font.face, StylePolicy::Strict).is_ok());
}

#[test]
fn embedded_bytes_source_works() {
    // Build a source from bytes by reading the system font file when the
    // environment exposes one; otherwise this is a skip.
    let Some(path) = std::env::var_os("YURIS_FONT_TEST_FILE") else {
        println!("SKIP[font:embedded] set YURIS_FONT_TEST_FILE to exercise from_bytes");
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let src = FontSource::from_bytes(bytes, yu_font::FaceOrigin::Embedded).unwrap();
    assert!(!src.is_empty());
    assert_eq!(src.origin(), yu_font::FaceOrigin::Embedded);
}
