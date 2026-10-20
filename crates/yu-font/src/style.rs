//! Text style model and evidence gating.
//!
//! The engine's `FONT` command parameters are evidenced in
//! `docs/font-provenance.md`. Every effect that the 0.495 engine does not
//! exercise is still modeled here (so the API is complete), but is marked
//! [`Support::Unverified`] and rejected by [`StylePolicy::Strict`] with a
//! `font.unverified` diagnostic. [`StylePolicy::Reference`] is the explicit
//! opt-in used by the reference renderer.

use yu_log::{Boundary, Diag};

use crate::code;
use crate::color::Rgb;
use crate::source::FaceId;

/// How strongly a capability is backed by evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Directly observed in the manual, compiled ERIS data or the engine.
    Confirmed,
    /// Backed by a manual statement plus a reasonable, documented inference.
    Likely,
    /// Implemented for API completeness but not evidenced for 0.495 text.
    Unverified,
    /// Known semantics with no implementation (returns a diagnostic).
    Unimplemented,
}

/// One row of the capability matrix.
#[derive(Clone, Copy, Debug)]
pub struct Capability {
    pub name: &'static str,
    pub support: Support,
    pub note: &'static str,
}

/// Capability matrix rendered into `docs/font-contract.md`.
pub const CAPABILITIES: &[Capability] = &[
    Capability {
        name: "SX/SY",
        support: Support::Confirmed,
        note: "engine stores 0..=0x100 px per axis; default 20; ERIS uses 12",
    },
    Capability {
        name: "BOLD",
        support: Support::Confirmed,
        note: "lfWeight 400/700 in LOGFONTA; bold face first, GDI-style synthetic bold otherwise",
    },
    Capability {
        name: "ANTIALIASING",
        support: Support::Confirmed,
        note: "boolean selects AA/non-AA compositor; default 1; ERIS dialogue sets 0",
    },
    Capability {
        name: "COLOR",
        support: Support::Confirmed,
        note: "0xRRGGBB, default 0xffffff",
    },
    Capability {
        name: "COLOR2",
        support: Support::Confirmed,
        note: "second stop of the documented gradient; default 0xffffff",
    },
    Capability {
        name: "GRADATION",
        support: Support::Likely,
        note: "engine stores 0..=8 (default 1); direction mapping is inferred, not documented",
    },
    Capability {
        name: "DRAWMODE",
        support: Support::Confirmed,
        note: "0=replace incl. transparent, 1=alpha blend (manual)",
    },
    Capability {
        name: "ROTATION",
        support: Support::Unverified,
        note: "engine stores 0..=8 (default 0) but 0.495 rendering use is not observed",
    },
    Capability {
        name: "italic",
        support: Support::Unverified,
        note: "no FONT parameter in 0.495; API extension, italic face preferred, then fixed shear",
    },
    Capability {
        name: "outline",
        support: Support::Unverified,
        note: "no FONT parameter in 0.495; coverage dilation, round join, fixed AA",
    },
    Capability {
        name: "shadow",
        support: Support::Unverified,
        note: "no FONT parameter in 0.495; integer offset only, blur unimplemented",
    },
    Capability {
        name: "kerning",
        support: Support::Unverified,
        note: "engine draws one character per TextOutA; font kern is opt-in only",
    },
    Capability {
        name: "ClearType",
        support: Support::Unimplemented,
        note: "no ClearType import/quality in the 0.495 engine",
    },
];

/// Antialiasing mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AaMode {
    /// 8-bit grayscale coverage (engine `ANTIALIASING=1`, the default).
    #[default]
    Grayscale,
    /// 1-bit coverage, threshold at 128 (engine `ANTIALIASING=0`).
    None,
    /// Subpixel RGB coverage. Never used by the 0.495 engine.
    ClearType,
}

/// Pixel composition for the finished glyph layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrawMode {
    /// `DRAWMODE=0`: new pixels replace the destination, including alpha.
    #[default]
    Replace,
    /// `DRAWMODE=1`: source-over alpha compositing.
    Blend,
}

/// `ROTATION` parameter. The engine stores and clamps `0..=8`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Rotation {
    #[default]
    None,
    /// `n` × 45° clockwise around the pen origin. Mapping is inferred.
    Octant(u8),
}

impl Rotation {
    #[must_use]
    pub fn from_script(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1..=8 => Some(Self::Octant(value as u8)),
            _ => None,
        }
    }

    #[must_use]
    pub fn degrees(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Octant(n) => f32::from(n) * 45.0,
        }
    }
}

/// `GRADATION` parameter. The engine stores and clamps `0..=8`, default `1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gradation {
    /// Solid [`TextStyle::color`].
    None,
    /// Direction selector `1..=8`; `1` is the engine default (vertical).
    Direction { octant: u8 },
}

impl Default for Gradation {
    fn default() -> Self {
        Self::Direction { octant: 1 }
    }
}

impl Gradation {
    #[must_use]
    pub fn from_script(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1..=8 => Some(Self::Direction {
                octant: value as u8,
            }),
            _ => None,
        }
    }

    #[must_use]
    pub fn octant(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Direction { octant } => octant,
        }
    }
}

/// Gradient coordinate scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GradationScope {
    /// Recompute the gradient for every glyph box.
    #[default]
    PerGlyph,
    /// One gradient over the whole run (reference helper; not cached).
    WholeRun,
}

/// Italic request. A real italic face is preferred; `Synthetic` shears.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Italic {
    #[default]
    None,
    /// Ask for an italic face; if absent, fall back to [`Italic::Synthetic`].
    Face,
    /// Shear by `degrees` (positive leans right).
    Synthetic { degrees: f32 },
}

impl Italic {
    #[must_use]
    pub fn shear_tan(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Face => 12.0_f32.to_radians().tan(),
            Self::Synthetic { degrees } => {
                let d = degrees.clamp(-45.0, 45.0);
                d.to_radians().tan()
            }
        }
    }

    #[must_use]
    pub fn quantized(self) -> i16 {
        match self {
            Self::None => 0,
            Self::Face => i16::MIN,
            Self::Synthetic { degrees } => (degrees.clamp(-45.0, 45.0) * 100.0).round() as i16,
        }
    }
}

/// Stroke join mode. Only round joins are implemented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OutlineJoin {
    #[default]
    Round,
    Miter,
}

/// Outline stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Outline {
    /// Stroke radius in pixels, `1..=16` (a 1 px radius grows the silhouette
    /// by roughly one pixel in every direction).
    pub width: u8,
    pub color: Rgb,
    pub join: OutlineJoin,
}

impl Outline {
    #[must_use]
    pub const fn new(width: u8, color: Rgb) -> Self {
        Self {
            width,
            color,
            join: OutlineJoin::Round,
        }
    }
}

/// Drop shadow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shadow {
    /// Horizontal offset in pixels; positive moves right.
    pub dx: i16,
    /// Vertical offset in pixels; positive moves down.
    pub dy: i16,
    pub color: Rgb,
    /// Gaussian-like blur radius. Only `0` is implemented.
    pub blur: u8,
    /// Shadow the outline silhouette instead of the bare glyph.
    pub include_outline: bool,
}

impl Shadow {
    #[must_use]
    pub const fn new(dx: i16, dy: i16, color: Rgb) -> Self {
        Self {
            dx,
            dy,
            color,
            blur: 0,
            include_outline: false,
        }
    }
}

/// Kerning source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Kerning {
    /// The engine draws one character per `TextOutA`; no kerning.
    #[default]
    None,
    /// Legacy TrueType `kern` table, format 0. Not used by the engine.
    Font,
}

/// Whether unverified effects are allowed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StylePolicy {
    /// Production: reject every unverified effect with `font.unverified`.
    #[default]
    Strict,
    /// Reference rendering: allow implemented, unverified effects.
    Reference,
}

/// Complete per-glyph text style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub face: FaceId,
    /// Horizontal size in pixels (`SX`), engine range `0..=256`.
    pub sx: u16,
    /// Vertical size in pixels (`SY`), engine range `0..=256`.
    pub sy: u16,
    pub bold: bool,
    pub italic: Italic,
    pub aa: AaMode,
    pub color: Rgb,
    pub color2: Rgb,
    pub gradation: Gradation,
    pub gradation_scope: GradationScope,
    pub draw_mode: DrawMode,
    pub rotation: Rotation,
    pub outline: Option<Outline>,
    pub shadow: Option<Shadow>,
    pub kerning: Kerning,
}

impl TextStyle {
    /// Engine defaults for one of the eight font slots: SX/SY=20, AA=1,
    /// COLOR=COLOR2=0xffffff, GRADATION=1, ROTATION=0, DRAWMODE=0, BOLD=0.
    #[must_use]
    pub const fn engine_default(face: FaceId) -> Self {
        Self {
            face,
            sx: 20,
            sy: 20,
            bold: false,
            italic: Italic::None,
            aa: AaMode::Grayscale,
            color: Rgb::WHITE,
            color2: Rgb::WHITE,
            gradation: Gradation::Direction { octant: 1 },
            gradation_scope: GradationScope::PerGlyph,
            draw_mode: DrawMode::Replace,
            rotation: Rotation::None,
            outline: None,
            shadow: None,
            kerning: Kerning::None,
        }
    }

    /// Validate engine ranges plus policy gating.
    ///
    /// Returns the first diagnostic; use [`TextStyle::validate_all`] to collect
    /// every issue.
    pub fn validate(&self, policy: StylePolicy) -> Result<(), Diag> {
        self.validate_all(policy)
            .into_iter()
            .next()
            .map_or(Ok(()), Err)
    }

    /// Validate engine ranges plus policy gating, returning every issue.
    #[must_use]
    pub fn validate_all(&self, policy: StylePolicy) -> Vec<Diag> {
        let mut out = Vec::new();
        if self.sx > 0x100 || self.sy > 0x100 {
            out.push(
                Diag::fatal(code::font::STYLE_RANGE, Boundary::Domain).with_detail(format!(
                    "SX/SY must be 0..=256 (engine clamps); got {}x{}",
                    self.sx, self.sy
                )),
            );
        }
        if self.aa == AaMode::ClearType {
            out.push(
                Diag::unimplemented(code::font::UNIMPLEMENTED, Boundary::Domain)
                    .with_detail("ClearType: 0.495 imports no ClearType quality/API; no evidence"),
            );
        }
        if let Some(o) = self.outline {
            if o.join == OutlineJoin::Miter {
                out.push(
                    Diag::unimplemented(code::font::UNIMPLEMENTED, Boundary::Domain)
                        .with_detail("miter outline join is not implemented; use Round"),
                );
            }
            if !(1..=16).contains(&o.width) {
                out.push(
                    Diag::fatal(code::font::STYLE_RANGE, Boundary::Domain)
                        .with_detail(format!("outline width must be 1..=16; got {}", o.width)),
                );
            }
        }
        if let Some(s) = self.shadow {
            if s.blur != 0 {
                out.push(
                    Diag::unimplemented(code::font::UNIMPLEMENTED, Boundary::Domain).with_detail(
                        "shadow blur has no engine evidence; only blur=0 is implemented",
                    ),
                );
            }
        }
        if policy == StylePolicy::Reference {
            return out;
        }

        // Strict policy: only evidenced effects.
        if self.italic != Italic::None {
            out.push(unverified(
                "italic",
                "FONT has no italic parameter in 0.495",
            ));
        }
        if self.outline.is_some() {
            out.push(unverified(
                "outline",
                "FONT has no outline parameter in 0.495",
            ));
        }
        if self.shadow.is_some() {
            out.push(unverified(
                "shadow",
                "FONT has no shadow parameter in 0.495",
            ));
        }
        if self.rotation != Rotation::None {
            out.push(unverified(
                "ROTATION",
                "parameter exists but no 0.495 text rendering use was observed",
            ));
        }
        let visible_gradient = self.gradation != Gradation::None && self.color != self.color2;
        if visible_gradient {
            out.push(unverified(
                "GRADATION/COLOR2",
                "gradient direction is not documented; only equal COLOR/COLOR2 is safe in strict mode",
            ));
        }
        if self.kerning != Kerning::None {
            out.push(unverified(
                "kerning",
                "engine draws per character with TextOutA; font kerning changes parity",
            ));
        }
        out
    }

    /// The same style with the face replaced.
    #[must_use]
    pub fn with_face(mut self, face: FaceId) -> Self {
        self.face = face;
        self
    }
}

fn unverified(feature: &str, why: &str) -> Diag {
    Diag::unimplemented(code::font::UNVERIFIED, Boundary::Domain).with_detail(format!(
        "{feature}: {why}; use StylePolicy::Reference to render it"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_defaults_match_re_facts() {
        let face = FaceId::synthetic_for_tests(0);
        let s = TextStyle::engine_default(face);
        assert_eq!((s.sx, s.sy), (20, 20));
        assert_eq!(s.aa, AaMode::Grayscale);
        assert_eq!(s.color, Rgb::WHITE);
        assert_eq!(s.color2, Rgb::WHITE);
        assert_eq!(s.gradation, Gradation::Direction { octant: 1 });
        assert_eq!(s.rotation, Rotation::None);
        assert_eq!(s.draw_mode, DrawMode::Replace);
        assert!(!s.bold);
    }

    #[test]
    fn strict_rejects_unverified_gradient() {
        let face = FaceId::synthetic_for_tests(0);
        let mut s = TextStyle::engine_default(face);
        s.color2 = Rgb::new(0, 0, 255);
        assert!(s.validate(StylePolicy::Strict).is_err());
        assert!(s.validate(StylePolicy::Reference).is_ok());
    }

    #[test]
    fn cleartype_is_unimplemented_even_in_reference() {
        let face = FaceId::synthetic_for_tests(0);
        let mut s = TextStyle::engine_default(face);
        s.aa = AaMode::ClearType;
        assert!(s.validate(StylePolicy::Reference).is_err());
    }
}
