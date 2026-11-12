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
        // upem 与字符无关:原来每字符取一次(每次走 with_face_data),
        // 这里只在需要 legacy kern 时取一次;取不到就整行不套 kern
        // (与原来"该字符取不到就跳过该字符 kern"同效,因为失败与字符无关)。
        let upem = if self.kerning == Kerning::Font {
            source
                .units_per_em(style.face)
                .ok()
                .filter(|&units| units != 0)
        } else {
            None
        };
        for (i, ch) in text.chars().enumerate() {
            // 每字符只取一次 gid:原来 kern 查询、advance、下一字符的
            // prev 各调一次 `glyph_index`(每次走 with_face_data)。
            let gid = source.glyph_index(style.face, ch).ok().flatten();
            if i > 0 && self.kerning == Kerning::Font {
                if let (Some(left), Some(right), Some(upem)) = (prev, gid, upem) {
                    if let Ok(k) = source.kerning_units(style.face, left, right) {
                        pen += f32::from(k) * f32::from(style.sy) / f32::from(upem);
                    }
                }
            }
            out.push(PlacedGlyph { ch, x: pen, y: 0.0 });
            if let Some(gid) = gid {
                if let Ok(adv) = source.advance_px(style.face, gid, style.sx, style.sy) {
                    pen += adv;
                }
            }
            prev = gid;
        }
        out
    }
}
