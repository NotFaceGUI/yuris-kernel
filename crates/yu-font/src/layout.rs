//! Layout interfaces only.
//!
//! Engine line breaking, alignment, ruby and per-character rhythm belong to
//! `yu-gfx/text`; this crate deliberately implements no engine layout. The
//! reference renderer uses [`HorizontalLayout`] as a convenience helper for
//! contact sheets, not as a parity claim.

use crate::source::FontSource;
use crate::style::{Kerning, TextStyle};

/// One character placed relative to a pen origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub ch: char,
    /// Pen x in pixels (fractional allowed).
    pub x: f32,
    /// Pen y in pixels (baseline).
    pub y: f32,
}

/// Layout provider interface consumed by `yu-gfx/text`.
pub trait TextLayout {
    /// Place characters for one line. No wrapping is implied.
    fn layout(&mut self, source: &FontSource, style: &TextStyle, text: &str) -> Vec<PlacedGlyph>;
}

/// Reference-only horizontal placement using glyph advances and optional
/// legacy kerning. Not engine layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct HorizontalLayout {
    pub kerning: Kerning,
}

impl HorizontalLayout {
    #[must_use]
    pub fn new(kerning: Kerning) -> Self {
        Self { kerning }
    }
}

impl TextLayout for HorizontalLayout {
    fn layout(&mut self, source: &FontSource, style: &TextStyle, text: &str) -> Vec<PlacedGlyph> {
        let mut out = Vec::with_capacity(text.chars().count());
        let mut pen = 0.0f32;
        let mut prev: Option<u16> = None;
        for (i, ch) in text.chars().enumerate() {
            if i > 0 && self.kerning == Kerning::Font {
                if let (Some(left), Ok(Some(right))) = (prev, source.glyph_index(style.face, ch)) {
                    if let Ok(k) = source.kerning_units(style.face, left, right) {
                        if let Ok(upem) = source.units_per_em(style.face) {
                            if upem != 0 {
                                pen += f32::from(k) * f32::from(style.sy) / f32::from(upem);
                            }
                        }
                    }
                }
            }
            out.push(PlacedGlyph { ch, x: pen, y: 0.0 });
            if let Ok(Some(gid)) = source.glyph_index(style.face, ch) {
                if let Ok(adv) = source.advance_px(style.face, gid, style.sx, style.sy) {
                    pen += adv;
                }
            }
            prev = source.glyph_index(style.face, ch).ok().flatten();
        }
        out
    }
}
