//! `FONT` command parameter model, mirroring the engine's per-slot state.
//!
//! Evidence: FONT handler @ 0x444428-0x444b1c (REA) clamps and stores:
//!
//! | slot | param | storage | clamp |
//! |---|---|---|---|
//! | 0 | BNO | special (not resolved) | — |
//! | 1 | NO | font-list index | `< installed count` |
//! | 2 | NAME | font name → index | — |
//! | 3 | ANTIALIASING | obj+0x34 / 0x5d1738 | 0..=1 |
//! | 4 | COLOR | obj+0x28 / 0x5d16e8 | 0..=0xffffff |
//! | 5 | COLOR2 | obj+0x2c / 0x5d1708 | 0..=0xffffff |
//! | 6 | GRADATION | obj+0x30 / 0x5d1728 | 0..=8 |
//! | 7 | ROTATION | obj+0x31 / 0x5d1730 | 0..=8 |
//! | 8 | DRAWMODE | obj+0x33 / 0x5d1740 | 0..=1 |
//! | 9 | SX | obj+0x20 / 0x5d1688 | 0..=0x100 |
//! | 10 | SY | obj+0x24 / 0x5d16a8 | 0..=0x100 |
//! | 11 | BOLD | obj+0x32 / 0x5d16c8 | 0..=1 |
//!
//! Defaults @ `FUN_00464fac`: SX=SY=20, AA=1, COLOR=COLOR2=0xffffff,
//! GRADATION=1, ROTATION=0, DRAWMODE=0, BOLD=0.

use yu_log::{Boundary, Diag};

use crate::code;
use crate::color::Rgb;
use crate::source::FontSource;
use crate::style::{AaMode, DrawMode, Gradation, Rotation, StylePolicy, TextStyle};

/// One FONT command as scanned from a compiled script. Every parameter is
/// optional; omitted parameters preserve the slot's previous value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontCommand {
    pub bno: Option<i64>,
    pub no: Option<i64>,
    pub name: Option<String>,
    pub antialiasing: Option<i64>,
    pub color: Option<i64>,
    pub color2: Option<i64>,
    pub gradation: Option<i64>,
    pub rotation: Option<i64>,
    pub drawmode: Option<i64>,
    pub sx: Option<i64>,
    pub sy: Option<i64>,
    pub bold: Option<i64>,
}

/// Engine-per-slot mutable font state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontSlot {
    pub bno: Option<i64>,
    pub no: usize,
    pub name: String,
    pub antialiasing: bool,
    pub color: Rgb,
    pub color2: Rgb,
    pub gradation: u8,
    pub rotation: u8,
    pub drawmode: bool,
    pub sx: u16,
    pub sy: u16,
    pub bold: bool,
}

impl FontSlot {
    /// Defaults from `FUN_00464fac` for a resolved face.
    #[must_use]
    pub fn engine_default(name: impl Into<String>) -> Self {
        Self {
            bno: None,
            no: 0,
            name: name.into(),
            antialiasing: true,
            color: Rgb::WHITE,
            color2: Rgb::WHITE,
            gradation: 1,
            rotation: 0,
            drawmode: false,
            sx: 20,
            sy: 20,
            bold: false,
        }
    }

    /// Apply one FONT command. Engine clamping is reproduced; out-of-range
    /// values are clamped rather than rejected, because that is what the
    /// engine does (REA clamps at 0 / max).
    pub fn apply(&mut self, cmd: &FontCommand, source: &FontSource) -> Result<(), Diag> {
        if let Some(v) = cmd.no {
            if v < 0 {
                return Err(Diag::recoverable(code::font::STYLE_RANGE, Boundary::Script)
                    .with_detail(format!("FONT[NO={v}] is negative")));
            }
            let no = v as usize;
            let Some(face) = source.face_by_no(no) else {
                return Err(Diag::recoverable(code::font::MISSING, Boundary::Script)
                    .with_detail(format!("FONT[NO={no}] is outside the font list")));
            };
            self.no = no;
            if let Ok(info) = source.face_info(face) {
                self.name = info
                    .families
                    .first()
                    .cloned()
                    .unwrap_or(info.post_script_name);
            }
        }
        if let Some(name) = &cmd.name {
            let Some(face) = source.find_family(name) else {
                return Err(Diag::recoverable(code::font::MISSING, Boundary::Script)
                    .with_detail(format!("FONT[NAME=\"{name}\"] is not installed")));
            };
            if let Ok(info) = source.face_info(face) {
                self.name = info
                    .families
                    .first()
                    .cloned()
                    .unwrap_or(info.post_script_name);
            }
        }
        if let Some(v) = cmd.bno {
            // BNO has no manual page and no resolved font list in this crate.
            self.bno = Some(v);
        }
        if let Some(v) = cmd.antialiasing {
            self.antialiasing = v.clamp(0, 1) != 0;
        }
        if let Some(v) = cmd.color {
            self.color = Rgb::from_rgb_u32(v.clamp(0, 0xff_ffff) as u32);
        }
        if let Some(v) = cmd.color2 {
            self.color2 = Rgb::from_rgb_u32(v.clamp(0, 0xff_ffff) as u32);
        }
        if let Some(v) = cmd.gradation {
            self.gradation = v.clamp(0, 8) as u8;
        }
        if let Some(v) = cmd.rotation {
            self.rotation = v.clamp(0, 8) as u8;
        }
        if let Some(v) = cmd.drawmode {
            self.drawmode = v.clamp(0, 1) != 0;
        }
        if let Some(v) = cmd.sx {
            self.sx = v.clamp(0, 0x100) as u16;
        }
        if let Some(v) = cmd.sy {
            self.sy = v.clamp(0, 0x100) as u16;
        }
        if let Some(v) = cmd.bold {
            self.bold = v.clamp(0, 1) != 0;
        }
        Ok(())
    }

    /// Convert to a render style. `face` must already be resolved from
    /// `name`/`no` by the caller (the slot stores the display name).
    #[must_use]
    pub fn to_style(&self, face: crate::source::FaceId) -> TextStyle {
        let mut s = TextStyle::engine_default(face);
        s.sx = self.sx;
        s.sy = self.sy;
        s.bold = self.bold;
        s.aa = if self.antialiasing {
            AaMode::Grayscale
        } else {
            AaMode::None
        };
        s.color = self.color;
        s.color2 = self.color2;
        s.gradation = if self.gradation == 0 {
            Gradation::None
        } else {
            Gradation::Direction {
                octant: self.gradation,
            }
        };
        s.rotation = Rotation::from_script(i64::from(self.rotation)).unwrap_or(Rotation::None);
        s.draw_mode = if self.drawmode {
            DrawMode::Blend
        } else {
            DrawMode::Replace
        };
        s
    }

    /// Strict/reference check for this slot.
    pub fn validate(&self, face: crate::source::FaceId, policy: StylePolicy) -> Result<(), Diag> {
        self.to_style(face).validate(policy)
    }
}
