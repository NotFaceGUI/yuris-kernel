//! Style composition: outline, shadow, color/gradient, rotation, composite.
//!
//! Fixed pipeline (see `docs/font-contract.md`):
//!
//! ```text
//! base coverage -> bold/italic (in rasterizer)
//!   -> outline coverage (round dilation)
//!   -> shadow coverage (integer offset of the silhouette; no blur)
//!   -> color/gradient fill
//!   -> compositing back-to-front: shadow, outline, fill
//!   -> rotation of the whole glyph layer (unverified)
//!   -> caller composites with DRAWMODE
//! ```
//!
//! The engine itself only evidences coverage + solid color + DRAWMODE; the
//! outline/shadow/rotation stages are reference-policy extensions and are
//! rejected by [`crate::style::StylePolicy::Strict`] when requested.

use std::sync::Arc;

use yu_log::{Boundary, Diag};

use crate::cache::{CacheLimits, CacheStats, GlyphCache, GlyphKey};
use crate::code;
use crate::color::{Rgb, Rgba};
use crate::image::{GlyphBitmap, GlyphImage, GlyphRun};
use crate::raster::{RasterParams, Rasterizer};
use crate::source::{FaceId, FontSource};
use crate::style::{DrawMode, Gradation, GradationScope, Outline, Shadow, StylePolicy, TextStyle};

/// Owns a font source, a reusable rasterizer and a bounded cache.
pub struct FontRenderer {
    source: FontSource,
    rasterizer: Rasterizer,
    cache: GlyphCache,
}

impl FontRenderer {
    #[must_use]
    pub fn new(source: FontSource, limits: CacheLimits) -> Self {
        Self {
            source,
            rasterizer: Rasterizer::new(),
            cache: GlyphCache::new(limits),
        }
    }

    #[must_use]
    pub fn source(&self) -> &FontSource {
        &self.source
    }

    #[must_use]
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Render one character with the full style pipeline.
    pub fn render_glyph(
        &mut self,
        ch: char,
        style: &TextStyle,
        policy: StylePolicy,
    ) -> Result<Arc<GlyphImage>, Diag> {
        style.validate(policy)?;
        if style.gradation_scope == GradationScope::WholeRun {
            let base = self.rasterize_base(ch, style)?;
            return Ok(Arc::new(compose_glyph(base, style, None)));
        }
        let key = GlyphKey::from_style(ch, style);
        if let Some(hit) = self.cache.get(&key) {
            return Ok(hit);
        }
        let base = self.rasterize_base(ch, style)?;
        let image = Arc::new(compose_glyph(base, style, None));
        Ok(self.cache.insert(key, image))
    }

    fn rasterize_base(&mut self, ch: char, style: &TextStyle) -> Result<GlyphBitmap, Diag> {
        self.rasterizer.rasterize(
            &self.source,
            style.face,
            ch,
            RasterParams {
                sx: style.sx,
                sy: style.sy,
                bold: style.bold,
                italic: style.italic,
                aa: style.aa,
            },
        )
    }

    /// Layout-free horizontal run used by the reference renderer.
    ///
    /// This is deliberately **not** engine layout: no wrapping, alignment,
    /// ruby or per-character rhythm. `\n` is rejected and belongs to
    /// `yu-gfx/text`.
    pub fn render_run(
        &mut self,
        text: &str,
        style: &TextStyle,
        policy: StylePolicy,
    ) -> Result<GlyphRun, Diag> {
        style.validate(policy)?;
        if text.contains('\n') || text.contains('\r') {
            return Err(
                Diag::unimplemented(code::font::UNIMPLEMENTED, Boundary::Domain)
                    .with_detail("line breaking is outside yu-font; render each line separately"),
            );
        }
        if text.is_empty() {
            return Ok(GlyphRun {
                width: 1,
                height: 1,
                baseline_x: 0,
                baseline_y: 1,
                rgba: vec![0; 4],
                advance: 0,
            });
        }

        let images: Vec<Arc<GlyphImage>> = if style.gradation_scope == GradationScope::WholeRun {
            // First pass: geometry only (per-glyph gradient placeholder).
            let mut geo = Vec::with_capacity(text.chars().count());
            for ch in text.chars() {
                let base = self.rasterize_base(ch, style)?;
                geo.push(compose_glyph(base, style, None));
            }
            let (min_top, max_bottom) = geo.iter().fold((0i32, 0i32), |(lo, hi), g| {
                (lo.min(g.top), hi.max(g.top + g.height as i32))
            });
            // Second pass: color with the run-wide vertical range.
            let mut out = Vec::with_capacity(geo.len());
            for ch in text.chars() {
                let base = self.rasterize_base(ch, style)?;
                out.push(Arc::new(compose_glyph(
                    base,
                    style,
                    Some((min_top as f32, max_bottom as f32)),
                )));
            }
            out
        } else {
            let mut out = Vec::with_capacity(text.chars().count());
            for ch in text.chars() {
                out.push(self.render_glyph(ch, style, policy)?);
            }
            out
        };

        // Layout: pen in 1/256 px; optional legacy kern.
        let mut pen: i64 = 0;
        let mut placed: Vec<(i64, &Arc<GlyphImage>)> = Vec::with_capacity(images.len());
        let mut prev_gid: Option<u16> = None;
        for (i, (ch, img)) in text.chars().zip(images.iter()).enumerate() {
            if i > 0 && style.kerning == crate::style::Kerning::Font {
                if let (Some(left), Ok(Some(right))) =
                    (prev_gid, self.source.glyph_index(style.face, ch))
                {
                    let k = self.source.kerning_units(style.face, left, right)?;
                    if k != 0 {
                        let upem = self.source.units_per_em(style.face)?;
                        if upem != 0 {
                            pen += i64::from(k) * i64::from(style.sy) * 256 / i64::from(upem);
                        }
                    }
                }
            }
            placed.push((pen, img));
            pen += i64::from(img.advance);
            prev_gid = self.source.glyph_index(style.face, ch).ok().flatten();
        }

        let mut min_left = 0i32;
        let mut max_right = 1i32;
        let mut min_top = 0i32;
        let mut max_bottom = 1i32;
        for (pen, img) in &placed {
            let x = (*pen / 256) as i32 + img.left;
            min_left = min_left.min(x);
            max_right = max_right.max(x + img.width as i32);
            min_top = min_top.min(img.top);
            max_bottom = max_bottom.max(img.top + img.height as i32);
        }
        let width = (max_right - min_left).max(1) as u32;
        let height = (max_bottom - min_top).max(1) as u32;
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        let blend = style.draw_mode == DrawMode::Blend;
        for (pen, img) in placed {
            let x = (pen / 256) as i32 + img.left - min_left;
            let y = img.top - min_top;
            img.draw_onto(&mut rgba, width, height, x, y, blend);
        }
        Ok(GlyphRun {
            width,
            height,
            baseline_x: -min_left,
            baseline_y: -min_top,
            rgba,
            advance: (pen.clamp(i64::from(i32::MIN), i64::from(i32::MAX))) as i32,
        })
    }
}

/// Compose the full glyph layer from base coverage.
///
/// `gradient_range` overrides the vertical gradient span (used for
/// `GradationScope::WholeRun`), in y-down raster coordinates.
#[must_use]
pub fn compose_glyph(
    base: GlyphBitmap,
    style: &TextStyle,
    gradient_range: Option<(f32, f32)>,
) -> GlyphImage {
    if base.is_empty() {
        return GlyphImage::empty(base.advance);
    }
    let base_left = base.bearing_x;
    let base_top = -base.bearing_y;
    let base_w = base.width as i32;
    let base_h = base.height as i32;

    let outline_r = style.outline.map_or(0, |o| o.width as i32);
    let outline_rect = if outline_r > 0 {
        Some((
            base_left - outline_r,
            base_top - outline_r,
            base_w + outline_r * 2,
            base_h + outline_r * 2,
        ))
    } else {
        None
    };
    let silhouette_rect = style
        .shadow
        .map_or((base_left, base_top, base_w, base_h), |s| {
            if s.include_outline {
                outline_rect.unwrap_or((base_left, base_top, base_w, base_h))
            } else {
                (base_left, base_top, base_w, base_h)
            }
        });
    let shadow_rect = style.shadow.map(|s| {
        (
            silhouette_rect.0 + i32::from(s.dx),
            silhouette_rect.1 + i32::from(s.dy),
            silhouette_rect.2,
            silhouette_rect.3,
        )
    });

    // Union of all layer rectangles.
    let mut x0 = base_left;
    let mut y0 = base_top;
    let mut x1 = base_left + base_w;
    let mut y1 = base_top + base_h;
    if let Some((x, y, w, h)) = outline_rect {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + w);
        y1 = y1.max(y + h);
    }
    if let Some((x, y, w, h)) = shadow_rect {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + w);
        y1 = y1.max(y + h);
    }
    let width = (x1 - x0).max(1) as u32;
    let height = (y1 - y0).max(1) as u32;

    // Base coverage placed in union space.
    let mut fill = vec![0u8; (width as usize) * (height as usize)];
    Blit {
        dst: &mut fill,
        dw: width,
        dh: height,
        src: &base.coverage,
        sw: base_w,
        sh: base_h,
    }
    .at(base_left - x0, base_top - y0);

    // Outline coverage (dilated base), projected into union space.
    let mut outline_cov: Option<Vec<u8>> = None;
    if let Some(o) = style.outline {
        if o.width > 0 {
            let (cov, _, _) = dilate(&base.coverage, base_w, base_h, o.width as i32);
            let ow = base_w + outline_r * 2;
            let oh = base_h + outline_r * 2;
            let mut placed = vec![0u8; (width as usize) * (height as usize)];
            Blit {
                dst: &mut placed,
                dw: width,
                dh: height,
                src: &cov,
                sw: ow,
                sh: oh,
            }
            .at(base_left - outline_r - x0, base_top - outline_r - y0);
            outline_cov = Some(placed);
        }
    }

    // Shadow source (outline or base silhouette), already in union space.
    let shadow_cov = style.shadow.and_then(|s| {
        if s.blur != 0 {
            return None;
        }
        let mut cov = vec![0u8; (width as usize) * (height as usize)];
        match (&outline_cov, s.include_outline) {
            (Some(o), true) => {
                Blit {
                    dst: &mut cov,
                    dw: width,
                    dh: height,
                    src: o,
                    sw: width as i32,
                    sh: height as i32,
                }
                .at(i32::from(s.dx), i32::from(s.dy));
            }
            _ => Blit {
                dst: &mut cov,
                dw: width,
                dh: height,
                src: &base.coverage,
                sw: base_w,
                sh: base_h,
            }
            .at(
                i32::from(s.dx) + (base_left - x0),
                i32::from(s.dy) + (base_top - y0),
            ),
        }
        Some(cov)
    });

    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];

    // Back-to-front: shadow, outline, fill.
    if let (Some(s), Some(cov)) = (style.shadow, shadow_cov.as_ref()) {
        paint_solid(&mut rgba, width, height, cov, s.color);
    }
    if let (Some(o), Some(cov)) = (style.outline, outline_cov.as_ref()) {
        paint_solid(&mut rgba, width, height, cov, o.color);
    }
    paint_gradient(&mut rgba, width, height, &fill, style, y0, gradient_range);

    let mut image = GlyphImage {
        width,
        height,
        left: x0,
        top: y0,
        advance: base.advance,
        rgba,
    };
    if let crate::style::Rotation::Octant(n) = style.rotation {
        image = rotate_image(&image, f32::from(n) * 45.0);
    }
    image
}

struct Blit<'a> {
    dst: &'a mut [u8],
    dw: u32,
    dh: u32,
    src: &'a [u8],
    sw: i32,
    sh: i32,
}

impl Blit<'_> {
    fn at(self, ox: i32, oy: i32) {
        for y in 0..self.sh {
            let dy = oy + y;
            if dy < 0 || dy >= self.dh as i32 {
                continue;
            }
            for x in 0..self.sw {
                let dx = ox + x;
                if dx < 0 || dx >= self.dw as i32 {
                    continue;
                }
                let s = self.src[(y * self.sw + x) as usize];
                let d = &mut self.dst[(dy as u32 * self.dw + dx as u32) as usize];
                *d = (*d).max(s);
            }
        }
    }
}

/// Dilate a coverage mask by `radius` px with a round kernel and a 1 px AA
/// band. Returns `(coverage, origin_x, origin_y)` where the origin is
/// `(-radius, -radius)` relative to the source.
fn dilate(src: &[u8], w: i32, h: i32, radius: i32) -> (Vec<u8>, i32, i32) {
    let r = radius.max(0);
    let dw = w + r * 2;
    let dh = h + r * 2;
    let mut out = vec![0u8; (dw * dh) as usize];
    let full = (r as f32 - 0.5).max(0.0);
    let zero = r as f32 + 0.5;
    let search = r + 1;
    for y in 0..dh {
        for x in 0..dw {
            let sx = x - r;
            let sy = y - r;
            if sx >= 0 && sx < w && sy >= 0 && sy < h {
                let inside = src[(sy * w + sx) as usize];
                if inside > 0 {
                    out[(y * dw + x) as usize] = inside;
                    continue;
                }
            }
            let mut best = i32::MAX;
            'search: for dy in -search..=search {
                for dx in -search..=search {
                    let px = sx + dx;
                    let py = sy + dy;
                    if px < 0 || px >= w || py < 0 || py >= h {
                        continue;
                    }
                    if src[(py * w + px) as usize] >= 128 {
                        let d2 = dx * dx + dy * dy;
                        if d2 < best {
                            best = d2;
                        }
                        if best == 0 {
                            break 'search;
                        }
                    }
                }
            }
            if best == i32::MAX {
                continue;
            }
            let d = (best as f32).sqrt();
            let a = if d <= full {
                255.0
            } else if d >= zero {
                0.0
            } else {
                255.0 * (zero - d)
            };
            out[(y * dw + x) as usize] = a.round().clamp(0.0, 255.0) as u8;
        }
    }
    (out, -r, -r)
}

fn paint_solid(rgba: &mut [u8], w: u32, h: u32, cov: &[u8], color: Rgb) {
    for (i, &a) in cov.iter().enumerate().take((w as usize) * (h as usize)) {
        if a == 0 {
            continue;
        }
        let src = Rgba::new(color.r, color.g, color.b, a);
        let di = i * 4;
        let dst = Rgba::new(rgba[di], rgba[di + 1], rgba[di + 2], rgba[di + 3]);
        let o = src.over(dst);
        rgba[di..di + 4].copy_from_slice(&o.to_array());
    }
}

fn paint_gradient(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    cov: &[u8],
    style: &TextStyle,
    origin_y: i32,
    range: Option<(f32, f32)>,
) {
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let a = cov[i];
            if a == 0 {
                continue;
            }
            let (u, v) = match range {
                Some((top, bottom)) if bottom > top => {
                    let gy = origin_y as f32 + y as f32;
                    (
                        (x as f32) / (w.max(2) - 1) as f32,
                        (gy - top) / (bottom - top),
                    )
                }
                _ => (
                    x as f32 / (w.max(2) - 1) as f32,
                    y as f32 / (h.max(2) - 1) as f32,
                ),
            };
            let t = gradation_t(style.gradation, u, v);
            let color = style.color.lerp(style.color2, t);
            let src = Rgba::new(color.r, color.g, color.b, a);
            let di = i * 4;
            let dst = Rgba::new(rgba[di], rgba[di + 1], rgba[di + 2], rgba[di + 3]);
            let o = src.over(dst);
            rgba[di..di + 4].copy_from_slice(&o.to_array());
        }
    }
}

/// Direction mapping for `GRADATION` 0..=8. `1` is the engine default
/// (vertical, top to bottom). The mapping itself is inferred.
#[must_use]
pub fn gradation_t(gradation: Gradation, u: f32, v: f32) -> f32 {
    let octant = match gradation {
        Gradation::None => 0,
        Gradation::Direction { octant } => octant,
    };
    match octant {
        1 => v,
        2 => 1.0 - v,
        3 => u,
        4 => 1.0 - u,
        5 => (u + v) * 0.5,
        6 => (u + 1.0 - v) * 0.5,
        7 => (1.0 - u + v) * 0.5,
        8 => (2.0 - u - v) * 0.5,
        _ => 0.0,
    }
}

/// Rotate a glyph layer clockwise around the pen origin (local `(0,0)`).
/// Bilinear sampling in premultiplied alpha. Unverified engine semantics.
#[must_use]
pub fn rotate_image(img: &GlyphImage, degrees: f32) -> GlyphImage {
    if degrees == 0.0 || img.is_empty() {
        return img.clone();
    }
    let rad = degrees.to_radians();
    let (sin, cos) = rad.sin_cos();
    let corners = [
        (img.left as f32, img.top as f32),
        ((img.left + img.width as i32) as f32, img.top as f32),
        (img.left as f32, (img.top + img.height as i32) as f32),
        (
            (img.left + img.width as i32) as f32,
            (img.top + img.height as i32) as f32,
        ),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (x, y) in corners {
        let rx = x * cos - y * sin;
        let ry = x * sin + y * cos;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }
    let left = min_x.floor() as i32;
    let top = min_y.floor() as i32;
    let width = (max_x.ceil() as i32 - left).max(1) as u32;
    let height = (max_y.ceil() as i32 - top).max(1) as u32;
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    for dy in 0..height {
        for dx in 0..width {
            let rx = left as f32 + dx as f32 + 0.5;
            let ry = top as f32 + dy as f32 + 0.5;
            // Inverse rotation.
            let x = rx * cos + ry * sin;
            let y = -rx * sin + ry * cos;
            if let Some((p, _)) = sample_bilinear(img, x, y) {
                let di = ((dy * width + dx) * 4) as usize;
                rgba[di..di + 4].copy_from_slice(&p.to_array());
            }
        }
    }
    GlyphImage {
        width,
        height,
        left,
        top,
        advance: img.advance,
        rgba,
    }
}

fn sample_bilinear(img: &GlyphImage, x: f32, y: f32) -> Option<(Rgba, f32)> {
    let fx = x - img.left as f32;
    let fy = y - img.top as f32;
    if fx < 0.0 || fy < 0.0 || fx >= img.width as f32 || fy >= img.height as f32 {
        return None;
    }
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let mut acc = [0f32; 4];
    let mut wsum = 0f32;
    for (ox, oy, w) in [
        (0, 0, (1.0 - tx) * (1.0 - ty)),
        (1, 0, tx * (1.0 - ty)),
        (0, 1, (1.0 - tx) * ty),
        (1, 1, tx * ty),
    ] {
        let px = x0 + ox;
        let py = y0 + oy;
        if px < 0 || py < 0 || px >= img.width as i32 || py >= img.height as i32 {
            continue;
        }
        let i = ((py as u32 * img.width + px as u32) * 4) as usize;
        let a = img.rgba[i + 3] as f32 / 255.0;
        // Premultiplied accumulation.
        acc[0] += img.rgba[i] as f32 * a * w;
        acc[1] += img.rgba[i + 1] as f32 * a * w;
        acc[2] += img.rgba[i + 2] as f32 * a * w;
        acc[3] += a * w;
        wsum += w;
    }
    if wsum <= 0.0 || acc[3] <= 0.0 {
        return Some((Rgba::TRANSPARENT, wsum));
    }
    let a = acc[3] / wsum;
    let un = |v: f32| (v / wsum / a).round().clamp(0.0, 255.0) as u8;
    Some((
        Rgba::new(
            un(acc[0]),
            un(acc[1]),
            un(acc[2]),
            (a * 255.0).round() as u8,
        ),
        wsum,
    ))
}

/// Convenience: render one glyph with a source and style, without a
/// persistent renderer. Useful for tests; production paths should reuse
/// [`FontRenderer`] to keep the cache.
pub fn render_glyph_once(
    source: &FontSource,
    ch: char,
    style: &TextStyle,
    policy: StylePolicy,
) -> Result<GlyphImage, Diag> {
    style.validate(policy)?;
    let mut rasterizer = Rasterizer::new();
    let base = rasterizer.rasterize(
        source,
        style.face,
        ch,
        RasterParams {
            sx: style.sx,
            sy: style.sy,
            bold: style.bold,
            italic: style.italic,
            aa: style.aa,
        },
    )?;
    Ok(compose_glyph(base, style, None))
}

/// Exposed for tests and the reference renderer: dilate a coverage mask.
#[must_use]
pub fn dilate_coverage(src: &[u8], w: u32, h: u32, radius: u8) -> (Vec<u8>, u32, u32) {
    let (cov, _, _) = dilate(src, w as i32, h as i32, i32::from(radius));
    (cov, w + u32::from(radius) * 2, h + u32::from(radius) * 2)
}

/// Default outline style for the reference samples.
#[must_use]
pub fn sample_outline(width: u8, color: Rgb) -> Outline {
    Outline::new(width, color)
}

/// Default shadow style for the reference samples.
#[must_use]
pub fn sample_shadow(dx: i16, dy: i16, color: Rgb) -> Shadow {
    Shadow::new(dx, dy, color)
}

/// Check that a face can be used at all (diagnostics helper).
pub fn check_face(source: &FontSource, face: FaceId) -> Result<(), Diag> {
    source.face_info(face).map(|_| ())
}
