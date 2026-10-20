//! Colors in the engine's `0xRRGGBB` order.
//!
//! Manual evidence (`font/font.html`): `COLOR = 値(0～0xffffff)`, example
//! `0xffff00` = yellow and `0x0000ff` = blue, so the script value is RGB, not
//! the GDI `COLORREF` BGR order.

/// Opaque sRGB color with 8-bit channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const WHITE: Self = Self::new(255, 255, 255);
    pub const BLACK: Self = Self::new(0, 0, 0);

    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Decode the script value `0xRRGGBB`. Values above `0xffffff` are
    /// rejected by [`Rgb::from_script`] so no silent truncation happens.
    #[must_use]
    pub const fn from_rgb_u32(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        }
    }

    /// Decode a script value, rejecting the out-of-range values the engine
    /// clamps (`> 0xffffff`).
    pub fn from_script(value: i64) -> Option<Self> {
        if !(0..=0xff_ffff).contains(&value) {
            return None;
        }
        Some(Self::from_rgb_u32(value as u32))
    }

    #[must_use]
    pub const fn to_rgb_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    /// Linear interpolation in sRGB byte space.
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| {
            let v = a as f32 + (b as f32 - a as f32) * t;
            v.round().clamp(0.0, 255.0) as u8
        };
        Self {
            r: f(self.r, other.r),
            g: f(self.g, other.g),
            b: f(self.b, other.b),
        }
    }

    /// Convert to `0xRRGGBB`.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.to_rgb_u32()
    }
}

/// Straight-alpha RGBA pixel used by the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[must_use]
    pub const fn from_rgb(c: Rgb) -> Self {
        Self::new(c.r, c.g, c.b, 255)
    }

    /// Source-over compositing (`DRAWMODE=1`).
    #[must_use]
    pub fn over(self, dst: Self) -> Self {
        if self.a == 0 {
            return dst;
        }
        if self.a == 255 {
            return self;
        }
        let sa = self.a as u32;
        let da = dst.a as u32;
        let out_a = sa + da * (255 - sa) / 255;
        if out_a == 0 {
            return Self::TRANSPARENT;
        }
        let blend = |s: u8, d: u8| {
            let s = s as u32 * sa;
            let d = d as u32 * da * (255 - sa) / 255;
            ((s + d) / out_a).min(255) as u8
        };
        Self {
            r: blend(self.r, dst.r),
            g: blend(self.g, dst.g),
            b: blend(self.b, dst.b),
            a: out_a.min(255) as u8,
        }
    }

    /// Replace compositing (`DRAWMODE=0`): the new pixel wins, including its
    /// alpha. Engine manual: transparent parts are also overwritten.
    #[must_use]
    pub const fn replace_with(self, _dst: Self) -> Self {
        self
    }

    #[must_use]
    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_rgb_order_matches_manual() {
        assert_eq!(Rgb::from_rgb_u32(0x00ff00).to_rgb_u32(), 0x00ff00);
        assert_eq!(Rgb::from_script(0x0000ff), Some(Rgb::new(0, 0, 255)));
        assert_eq!(Rgb::from_script(0x100_0000), None);
    }

    #[test]
    fn source_over_is_identity_for_opaque() {
        let a = Rgba::new(10, 20, 30, 255);
        assert_eq!(a.over(Rgba::new(1, 2, 3, 128)), a);
    }
}
