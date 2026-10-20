//! Glyph bitmaps and composited images.

use crate::color::Rgba;

/// Rasterized coverage bitmap for one glyph at one size.
///
/// Coordinate convention: `bearing_x` is the left edge relative to the pen
/// origin (positive right); `bearing_y` is the top edge relative to the
/// baseline in a y-up font coordinate system (positive up). Raster pixels are
/// stored top-down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    pub bearing_x: i32,
    pub bearing_y: i32,
    /// Horizontal advance in pixels (fractional part kept as 1/256).
    pub advance: i32,
    /// 8-bit coverage, row-major, top-down. `len == width * height`.
    pub coverage: Vec<u8>,
}

impl GlyphBitmap {
    #[must_use]
    pub fn empty(advance: i32) -> Self {
        Self {
            width: 0,
            height: 0,
            bearing_x: 0,
            bearing_y: 0,
            advance,
            coverage: Vec::new(),
        }
    }

    /// Sum of coverage values (a stable shape/weight proxy).
    #[must_use]
    pub fn mass(&self) -> u64 {
        self.coverage.iter().map(|&v| u64::from(v)).sum()
    }

    /// Deterministic FNV-1a 64 hash over dimensions, metrics and coverage.
    #[must_use]
    pub fn hash64(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u32(self.width);
        h.write_u32(self.height);
        h.write_i32(self.bearing_x);
        h.write_i32(self.bearing_y);
        h.write_i32(self.advance);
        h.write(&self.coverage);
        h.finish()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Fully composited glyph layer in straight-alpha RGBA, top-down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphImage {
    pub width: u32,
    pub height: u32,
    /// Left edge relative to the pen origin (positive right).
    pub left: i32,
    /// Top edge relative to the baseline in y-down raster coordinates
    /// (negative above the baseline).
    pub top: i32,
    /// Horizontal advance in 1/256 px.
    pub advance: i32,
    /// Row-major RGBA.
    pub rgba: Vec<u8>,
}

impl GlyphImage {
    #[must_use]
    pub fn empty(advance: i32) -> Self {
        Self {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            advance,
            rgba: Vec::new(),
        }
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        Some(Rgba::new(
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ))
    }

    /// Deterministic FNV-1a 64 hash over geometry and pixels.
    #[must_use]
    pub fn hash64(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u32(self.width);
        h.write_u32(self.height);
        h.write_i32(self.left);
        h.write_i32(self.top);
        h.write_i32(self.advance);
        h.write(&self.rgba);
        h.finish()
    }

    /// Alpha mass, useful for tests and benchmarks.
    #[must_use]
    pub fn alpha_mass(&self) -> u64 {
        self.rgba.chunks_exact(4).map(|p| u64::from(p[3])).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Composite this image into an RGBA canvas at `(x, y)` with the chosen
    /// composition mode.
    pub fn draw_onto(&self, dst: &mut [u8], dst_w: u32, dst_h: u32, x: i32, y: i32, blend: bool) {
        for sy in 0..self.height as i32 {
            let dy = y + sy;
            if dy < 0 || dy >= dst_h as i32 {
                continue;
            }
            for sx in 0..self.width as i32 {
                let dx = x + sx;
                if dx < 0 || dx >= dst_w as i32 {
                    continue;
                }
                let si = ((sy as u32 * self.width + sx as u32) * 4) as usize;
                let di = ((dy as u32 * dst_w + dx as u32) * 4) as usize;
                let s = Rgba::new(
                    self.rgba[si],
                    self.rgba[si + 1],
                    self.rgba[si + 2],
                    self.rgba[si + 3],
                );
                let d = Rgba::new(dst[di], dst[di + 1], dst[di + 2], dst[di + 3]);
                let o = if blend { s.over(d) } else { s.replace_with(d) };
                dst[di..di + 4].copy_from_slice(&o.to_array());
            }
        }
    }
}

/// A horizontal run of composited glyphs (reference helper, not engine
/// layout).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphRun {
    pub width: u32,
    pub height: u32,
    /// Baseline origin inside the image; `x` is the left pen start, `y` is the
    /// baseline row in y-down coordinates.
    pub baseline_x: i32,
    pub baseline_y: i32,
    pub rgba: Vec<u8>,
    /// Advance sum in 1/256 px.
    pub advance: i32,
}

impl GlyphRun {
    #[must_use]
    pub fn hash64(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u32(self.width);
        h.write_u32(self.height);
        h.write_i32(self.baseline_x);
        h.write_i32(self.baseline_y);
        h.write_i32(self.advance);
        h.write(&self.rgba);
        h.finish()
    }
}

/// FNV-1a 64, used for golden hashes (stable, dependency-free).
#[derive(Clone, Copy, Debug)]
pub struct Fnv64(u64);

impl Fnv64 {
    pub const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    pub const PRIME: u64 = 0x0000_0100_0000_01b3;

    #[must_use]
    pub const fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    pub fn write_u32(&mut self, v: u32) {
        self.write(&v.to_le_bytes());
    }

    pub fn write_i32(&mut self, v: i32) {
        self.write(&v.to_le_bytes());
    }

    #[must_use]
    pub const fn finish(self) -> u64 {
        self.0
    }
}

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_across_calls() {
        let b = GlyphBitmap {
            width: 2,
            height: 1,
            bearing_x: 0,
            bearing_y: 0,
            advance: 512,
            coverage: vec![0, 255],
        };
        assert_eq!(b.hash64(), b.hash64());
        assert_eq!(b.mass(), 255);
    }

    #[test]
    fn draw_replace_writes_transparent_pixels() {
        let img = GlyphImage {
            width: 1,
            height: 1,
            left: 0,
            top: 0,
            advance: 0,
            rgba: vec![0, 0, 0, 0],
        };
        let mut dst = vec![9, 9, 9, 9];
        img.draw_onto(&mut dst, 1, 1, 0, 0, false);
        assert_eq!(dst, vec![0, 0, 0, 0]);
    }
}
