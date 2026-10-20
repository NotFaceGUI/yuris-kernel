//! Bounded, deterministic glyph cache.
//!
//! Key = `(face, char, px, style, aa, outline, shadow, rotation, colors,
//! gradation, draw mode)`; the face is source-local. Eviction is strict LRU
//! over a monotonically increasing tick, with two independent limits
//! (entries and pixels) so a single huge glyph cannot blow the budget.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::color::Rgb;
use crate::image::GlyphImage;
use crate::source::FaceId;
use crate::style::{AaMode, DrawMode, Gradation, GradationScope, Italic, Rotation, TextStyle};

/// Cache key for a fully composited glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub face: FaceId,
    pub ch: char,
    pub sx: u16,
    pub sy: u16,
    pub bold: bool,
    pub italic_q: i16,
    pub aa: AaMode,
    pub outline_width: u8,
    pub outline_color: Rgb,
    pub shadow_dx: i16,
    pub shadow_dy: i16,
    pub shadow_color: Rgb,
    pub shadow_blur: u8,
    pub shadow_outline: bool,
    pub rotation_octant: u8,
    pub color: Rgb,
    pub color2: Rgb,
    pub gradation_octant: u8,
    pub draw_mode: DrawMode,
}

impl GlyphKey {
    #[must_use]
    pub fn from_style(ch: char, style: &TextStyle) -> Self {
        let outline = style.outline;
        let shadow = style.shadow;
        Self {
            face: style.face,
            ch,
            sx: style.sx,
            sy: style.sy,
            bold: style.bold,
            italic_q: style.italic.quantized(),
            aa: style.aa,
            outline_width: outline.map_or(0, |o| o.width),
            outline_color: outline.map_or(Rgb::BLACK, |o| o.color),
            shadow_dx: shadow.map_or(0, |s| s.dx),
            shadow_dy: shadow.map_or(0, |s| s.dy),
            shadow_color: shadow.map_or(Rgb::BLACK, |s| s.color),
            shadow_blur: shadow.map_or(0, |s| s.blur),
            shadow_outline: shadow.is_some_and(|s| s.include_outline),
            rotation_octant: match style.rotation {
                Rotation::None => 0,
                Rotation::Octant(n) => n,
            },
            color: style.color,
            color2: style.color2,
            gradation_octant: match style.gradation {
                Gradation::None => 0,
                Gradation::Direction { octant } => octant,
            },
            draw_mode: style.draw_mode,
        }
    }

    #[must_use]
    pub fn uses_per_glyph_gradient(style: &TextStyle) -> bool {
        style.gradation_scope == GradationScope::PerGlyph
    }

    /// Quantized italic degrees (for diagnostics/tests).
    #[must_use]
    pub fn italic_degrees(self) -> f32 {
        match self.italic_q {
            0 => 0.0,
            i16::MIN => 12.0,
            q => f32::from(q) / 100.0,
        }
    }

    /// Whether this style requests a synthetic shear.
    #[must_use]
    pub fn italic_kind(self) -> Italic {
        match self.italic_q {
            0 => Italic::None,
            i16::MIN => Italic::Face,
            q => Italic::Synthetic {
                degrees: f32::from(q) / 100.0,
            },
        }
    }
}

/// Cache limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheLimits {
    pub max_entries: usize,
    pub max_pixels: u64,
}

impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 4096,
            max_pixels: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub insertions: u64,
    pub evictions: u64,
    pub entries: usize,
    pub pixels: u64,
    pub memory_bytes: u64,
}

struct Entry {
    image: Arc<GlyphImage>,
    tick: u64,
    pixels: u64,
    bytes: u64,
}

/// Bounded glyph cache.
pub struct GlyphCache {
    limits: CacheLimits,
    entries: HashMap<GlyphKey, Entry>,
    lru: BTreeMap<u64, GlyphKey>,
    tick: u64,
    pixels: u64,
    stats: CacheStats,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new(CacheLimits::default())
    }
}

impl GlyphCache {
    #[must_use]
    pub fn new(limits: CacheLimits) -> Self {
        Self {
            limits,
            entries: HashMap::new(),
            lru: BTreeMap::new(),
            tick: 0,
            pixels: 0,
            stats: CacheStats::default(),
        }
    }

    #[must_use]
    pub fn get(&mut self, key: &GlyphKey) -> Option<Arc<GlyphImage>> {
        if !self.entries.contains_key(key) {
            self.stats.misses += 1;
            return None;
        }
        let tick = self.next_tick();
        let entry = self.entries.get_mut(key)?;
        let old_tick = entry.tick;
        entry.tick = tick;
        let image = Arc::clone(&entry.image);
        self.lru.remove(&old_tick);
        self.lru.insert(tick, *key);
        self.stats.hits += 1;
        Some(image)
    }

    /// Insert, replacing an existing key and evicting until both limits hold.
    pub fn insert(&mut self, key: GlyphKey, image: Arc<GlyphImage>) -> Arc<GlyphImage> {
        let pixels = u64::from(image.width) * u64::from(image.height);
        let bytes = pixels * 4 + 128;
        if let Some(old) = self.entries.remove(&key) {
            self.lru.remove(&old.tick);
            self.pixels = self.pixels.saturating_sub(old.pixels);
        }
        let tick = self.next_tick();
        self.entries.insert(
            key,
            Entry {
                image: Arc::clone(&image),
                tick,
                pixels,
                bytes,
            },
        );
        self.lru.insert(tick, key);
        self.pixels += pixels;
        self.stats.insertions += 1;
        self.evict();
        image
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    fn evict(&mut self) {
        while self.entries.len() > self.limits.max_entries || self.pixels > self.limits.max_pixels {
            let Some((&tick, &key)) = self.lru.iter().next() else {
                break;
            };
            self.lru.remove(&tick);
            if let Some(entry) = self.entries.remove(&key) {
                self.pixels = self.pixels.saturating_sub(entry.pixels);
                self.stats.evictions += 1;
            }
        }
    }

    /// Current counters plus entry/pixel totals.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        let memory_bytes: u64 = self.entries.values().map(|e| e.bytes).sum();
        CacheStats {
            entries: self.entries.len(),
            pixels: self.pixels,
            memory_bytes,
            ..self.stats.clone()
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.pixels = 0;
        self.tick = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> Arc<GlyphImage> {
        Arc::new(GlyphImage {
            width: w,
            height: h,
            left: 0,
            top: 0,
            advance: w as i32 * 256,
            rgba: vec![0; (w * h * 4) as usize],
        })
    }

    fn key(ch: char) -> GlyphKey {
        GlyphKey {
            face: FaceId(0),
            ch,
            sx: 12,
            sy: 12,
            bold: false,
            italic_q: 0,
            aa: AaMode::Grayscale,
            outline_width: 0,
            outline_color: Rgb::BLACK,
            shadow_dx: 0,
            shadow_dy: 0,
            shadow_color: Rgb::BLACK,
            shadow_blur: 0,
            shadow_outline: false,
            rotation_octant: 0,
            color: Rgb::WHITE,
            color2: Rgb::WHITE,
            gradation_octant: 1,
            draw_mode: DrawMode::Replace,
        }
    }

    #[test]
    fn eviction_is_lru_and_deterministic() {
        let mut c = GlyphCache::new(CacheLimits {
            max_entries: 2,
            max_pixels: 1_000_000,
        });
        c.insert(key('a'), img(4, 4));
        c.insert(key('b'), img(4, 4));
        assert!(c.get(&key('a')).is_some());
        c.insert(key('c'), img(4, 4));
        assert!(c.get(&key('a')).is_some());
        assert!(c.get(&key('b')).is_none());
        assert!(c.get(&key('c')).is_some());
        assert_eq!(c.stats().evictions, 1);
    }

    #[test]
    fn pixel_limit_is_enforced() {
        let mut c = GlyphCache::new(CacheLimits {
            max_entries: 100,
            max_pixels: 20,
        });
        c.insert(key('a'), img(4, 4));
        c.insert(key('b'), img(4, 4));
        assert!(c.stats().pixels <= 20);
    }
}
