//! Shared test helpers.
#![allow(dead_code)]
#![allow(clippy::result_large_err)]

use std::path::PathBuf;

use yu_font::{FaceId, FontSource, TextStyle};

pub struct TestFont {
    pub source: FontSource,
    pub face: FaceId,
}

/// Load the test font: `YURIS_FONT_TEST_FILE` if set, otherwise the engine
/// default family on the host. Missing samples fail hard when
/// `YURIS_REQUIRE_SAMPLES=1`, otherwise the test is visibly skipped.
pub fn load_test_font() -> Option<TestFont> {
    if let Some(path) = std::env::var_os("YURIS_FONT_TEST_FILE").map(PathBuf::from) {
        let source = FontSource::from_file(&path).expect("YURIS_FONT_TEST_FILE is not a font");
        let face = source.default_face().expect("default face");
        return Some(TestFont { source, face });
    }
    match FontSource::load_system_fonts().and_then(|s| {
        let face = s.default_face()?;
        Ok(TestFont { source: s, face })
    }) {
        Ok(f) => Some(f),
        Err(d) => {
            if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty())
            {
                panic!("no system font for tests: {d}");
            }
            println!("SKIP[font:test] {d}");
            None
        }
    }
}

pub fn style(font: &TestFont, sx: u16, sy: u16) -> TextStyle {
    let mut s = TextStyle::engine_default(font.face);
    s.sx = sx;
    s.sy = sy;
    s
}

/// Keep the compiler honest about unused imports in skip-only runs.
#[allow(dead_code)]
pub fn require(font: Option<TestFont>) -> Option<TestFont> {
    font
}
