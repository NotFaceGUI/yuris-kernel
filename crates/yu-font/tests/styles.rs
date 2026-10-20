//! Style golden/behavior tests.

mod support;

use yu_font::cache::CacheLimits;
use yu_font::{
    code, AaMode, FontRenderer, Gradation, Italic, Outline, Rgb, Rotation, Shadow, StylePolicy,
    TextStyle,
};

fn setup() -> Option<(support::TestFont, FontRenderer)> {
    let font = support::load_test_font()?;
    let renderer = FontRenderer::new(
        // Re-load so the test owns the source independently of `font`.
        if let Some(p) = std::env::var_os("YURIS_FONT_TEST_FILE") {
            yu_font::FontSource::from_file(p).ok()?
        } else {
            yu_font::FontSource::load_system_fonts().ok()?
        },
        CacheLimits::default(),
    );
    Some((font, renderer))
}

#[test]
fn bold_increases_ink() {
    let Some((font, mut r)) = setup() else { return };
    let normal = support::style(&font, 24, 24);
    let mut bold = normal;
    bold.bold = true;
    let a = r.render_glyph('漢', &normal, StylePolicy::Strict).unwrap();
    let b = r.render_glyph('漢', &bold, StylePolicy::Strict).unwrap();
    assert!(
        b.alpha_mass() > a.alpha_mass(),
        "bold must add ink ({} -> {})",
        a.alpha_mass(),
        b.alpha_mass()
    );
}

#[test]
fn italic_face_or_shear_changes_shape() {
    let Some((font, mut r)) = setup() else { return };
    let normal = support::style(&font, 24, 24);
    let mut italic = normal;
    italic.italic = Italic::Synthetic { degrees: 12.0 };
    let a = r
        .render_glyph('A', &normal, StylePolicy::Reference)
        .unwrap();
    let b = r
        .render_glyph('A', &italic, StylePolicy::Reference)
        .unwrap();
    assert_ne!(a.hash64(), b.hash64());
    assert!(b.width >= a.width, "shear should not shrink the box");
}

#[test]
fn outline_expands_with_width() {
    let Some((font, mut r)) = setup() else { return };
    let base = support::style(&font, 24, 24);
    let mut o1 = base;
    o1.outline = Some(Outline::new(1, Rgb::new(255, 0, 0)));
    let mut o3 = base;
    o3.outline = Some(Outline::new(3, Rgb::new(255, 0, 0)));
    let a = r.render_glyph('あ', &base, StylePolicy::Strict).unwrap();
    let b = r.render_glyph('あ', &o1, StylePolicy::Reference).unwrap();
    let c = r.render_glyph('あ', &o3, StylePolicy::Reference).unwrap();
    assert!(b.width > a.width && b.height > a.height);
    assert!(c.width > b.width && c.height > b.height);
    assert!(c.alpha_mass() > b.alpha_mass());
    assert!(
        c.rgba
            .chunks_exact(4)
            .any(|p| p[3] > 0 && p[0] == 255 && p[1] == 0 && p[2] == 0),
        "outline color must appear at the silhouette edge"
    );
}

#[test]
fn shadow_offsets_negative_expand_left_and_up() {
    let Some((font, mut r)) = setup() else { return };
    let base = support::style(&font, 24, 24);
    let mut sh = base;
    sh.shadow = Some(Shadow::new(-3, -4, Rgb::new(0, 0, 0)));
    let a = r.render_glyph('A', &base, StylePolicy::Strict).unwrap();
    let b = r.render_glyph('A', &sh, StylePolicy::Reference).unwrap();
    assert_eq!(b.left, a.left - 3);
    assert_eq!(b.top, a.top - 4);
    assert!(b.width >= a.width + 3);
    assert!(b.height >= a.height + 4);
}

#[test]
fn vertical_gradient_goes_color_to_color2() {
    let Some((font, mut r)) = setup() else { return };
    let mut s = support::style(&font, 28, 28);
    s.color = Rgb::new(255, 0, 0);
    s.color2 = Rgb::new(0, 0, 255);
    s.gradation = Gradation::Direction { octant: 1 };
    let img = r.render_glyph('口', &s, StylePolicy::Reference).unwrap();
    let mut top = None;
    let mut bottom = None;
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel(x, y).unwrap();
            if p.a > 200 {
                top.get_or_insert(p);
                bottom = Some(p);
            }
        }
    }
    let top = top.expect("top ink");
    let bottom = bottom.expect("bottom ink");
    assert!(top.r > top.b, "top must be red-dominant: {top:?}");
    assert!(
        bottom.b > bottom.r,
        "bottom must be blue-dominant: {bottom:?}"
    );
}

#[test]
fn rotation_changes_geometry_and_hash() {
    let Some((font, mut r)) = setup() else { return };
    let base = support::style(&font, 24, 24);
    let mut rot = base;
    rot.rotation = Rotation::Octant(1);
    let a = r.render_glyph('A', &base, StylePolicy::Strict).unwrap();
    let b = r.render_glyph('A', &rot, StylePolicy::Reference).unwrap();
    assert_ne!(a.hash64(), b.hash64());
    assert!(b.width > a.width || b.height > a.height);
}

#[test]
fn strict_policy_rejects_unverified_effects() {
    let Some((font, mut r)) = setup() else { return };
    let mut s = support::style(&font, 20, 20);
    s.outline = Some(Outline::new(1, Rgb::BLACK));
    let err = r.render_glyph('A', &s, StylePolicy::Strict).unwrap_err();
    assert_eq!(err.code, code::font::UNVERIFIED);
    assert!(r.render_glyph('A', &s, StylePolicy::Reference).is_ok());
}

#[test]
fn cache_hits_are_counted_and_deterministic() {
    let Some((font, mut r)) = setup() else { return };
    let s = support::style(&font, 20, 20);
    let a = r.render_glyph('あ', &s, StylePolicy::Strict).unwrap();
    let b = r.render_glyph('あ', &s, StylePolicy::Strict).unwrap();
    assert_eq!(a.hash64(), b.hash64());
    let stats = r.cache_stats();
    assert_eq!(stats.entries, 1);
    assert!(stats.hits >= 1);
    assert!(stats.misses >= 1);
}

#[test]
fn antialias_off_is_binary() {
    let Some((font, mut r)) = setup() else { return };
    let mut s = support::style(&font, 24, 24);
    s.aa = AaMode::None;
    let img = r.render_glyph('A', &s, StylePolicy::Strict).unwrap();
    assert!(
        img.rgba.chunks_exact(4).all(|p| p[3] == 0 || p[3] == 255),
        "ANTIALIASING=0 must produce binary alpha"
    );
}

#[test]
fn anisotropic_sx_sy_changes_box() {
    let Some((font, mut r)) = setup() else { return };
    let wide = support::style(&font, 32, 16);
    let tall = support::style(&font, 16, 32);
    let a = r.render_glyph('A', &wide, StylePolicy::Strict).unwrap();
    let b = r.render_glyph('A', &tall, StylePolicy::Strict).unwrap();
    assert!(
        a.width > b.width,
        "32x16 must be wider: {} vs {}",
        a.width,
        b.width
    );
    assert!(
        b.height > a.height,
        "16x32 must be taller: {} vs {}",
        b.height,
        a.height
    );
}

#[test]
fn run_layout_rejects_line_breaks() {
    let Some((font, mut r)) = setup() else { return };
    let s = support::style(&font, 12, 12);
    let err = r
        .render_run(
            "a
b",
            &s,
            StylePolicy::Strict,
        )
        .unwrap_err();
    assert_eq!(err.code, code::font::UNIMPLEMENTED);
}

#[test]
fn default_style_is_strict_clean() {
    let Some((font, _)) = setup() else { return };
    let s = TextStyle::engine_default(font.face);
    assert!(s.validate(StylePolicy::Strict).is_ok());
}
