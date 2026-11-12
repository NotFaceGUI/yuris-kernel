//! Glyph rasterization determinism, golden hashes and diagnostics.

mod support;

use yu_font::cache::CacheLimits;
use yu_font::{code, FontRenderer, StylePolicy};

#[test]
fn determinism_across_fresh_renderers() {
    let Some(f1) = support::load_test_font() else {
        return;
    };
    let Some(f2) = support::load_test_font() else {
        return;
    };
    let face = f1.face;
    let style = yu_font::TextStyle::engine_default(face);
    let mut a = FontRenderer::new(f1.source, CacheLimits::default());
    let mut b = FontRenderer::new(f2.source, CacheLimits::default());
    let ia = a.render_glyph('A', &style, StylePolicy::Strict).unwrap();
    let ib = b.render_glyph('A', &style, StylePolicy::Strict).unwrap();
    assert_eq!(ia.hash64(), ib.hash64());
    let ia2 = a.render_glyph('A', &style, StylePolicy::Strict).unwrap();
    assert_eq!(ia.hash64(), ia2.hash64());
}

#[test]
fn glyph_metrics_and_mass() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let face = font.face;
    let mut r = FontRenderer::new(font.source, CacheLimits::default());
    let style = yu_font::TextStyle::engine_default(face);
    let a = r.render_glyph('A', &style, StylePolicy::Strict).unwrap();
    assert!(a.width > 0 && a.height > 0, "A must have ink at 20px");
    assert!(a.advance > 0);
    assert!(a.alpha_mass() > 0);
    let space = r.render_glyph(' ', &style, StylePolicy::Strict).unwrap();
    assert_eq!(space.alpha_mass(), 0, "space has no ink");
    assert!(space.advance > 0, "space still advances");
}

#[test]
fn golden_hashes_ascii_kana_kanji_punct() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let face = font.face;
    let mut r = FontRenderer::new(font.source, CacheLimits::default());
    let cases = [('A', 20u16), ('あ', 20), ('漢', 20), ('。', 20), ('、', 12)];
    let mut lines = Vec::new();
    for (ch, px) in cases {
        let mut style = yu_font::TextStyle::engine_default(face);
        style.sx = px;
        style.sy = px;
        let img = r.render_glyph(ch, &style, StylePolicy::Strict).unwrap();
        lines.push(format!(
            "U+{:04X} px={px} size={}x{} left={} top={} adv={} hash={:016x} mass={}",
            ch as u32,
            img.width,
            img.height,
            img.left,
            img.top,
            img.advance,
            img.hash64(),
            img.alpha_mass()
        ));
    }
    let text = lines.join("\n");
    println!("{text}");
    let golden = include_str!("golden/glyphs.txt");
    assert_eq!(
        text.trim(),
        golden.trim(),
        "glyph golden changed; inspect and update tests/golden/glyphs.txt only with evidence"
    );
}

#[test]
fn missing_glyph_is_a_diag_not_a_panic() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let face = font.face;
    let mut r = FontRenderer::new(font.source, CacheLimits::default());
    let style = yu_font::TextStyle::engine_default(face);
    let err = r
        .render_glyph('\u{10FFFF}', &style, StylePolicy::Strict)
        .unwrap_err();
    assert_eq!(err.code, code::font::GLYPH_MISSING);
    // 第二次起走缺字负缓存:诊断内容必须与首次逐字段一致(文案/边界/code)。
    let again = r
        .render_glyph('\u{10FFFF}', &style, StylePolicy::Strict)
        .unwrap_err();
    assert_eq!(again, err, "negative glyph cache must return the same diag");
    // 同一字符换字号也命中同一负缓存(字体数据未变,缺失与字号无关)。
    let bigger = yu_font::TextStyle {
        sx: 40,
        sy: 40,
        ..style
    };
    let third = r
        .render_glyph('\u{10FFFF}', &bigger, StylePolicy::Strict)
        .unwrap_err();
    assert_eq!(third, err);
}

#[test]
fn bad_font_is_a_diag_not_a_panic() {
    let err =
        yu_font::FontSource::from_bytes(vec![0u8; 64], yu_font::FaceOrigin::Embedded).unwrap_err();
    assert_eq!(err.code, code::font::BAD_FACE);
}
