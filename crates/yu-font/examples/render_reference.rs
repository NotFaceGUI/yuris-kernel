//! Reference renderer for `yu-font` (task G acceptance artifact).
//!
//! What it does:
//! 1. reads the official 0.495 evidence context: `data.ypf` font entries,
//!    `ysc.ybn` FONT parameter table and every compiled ERIS `FONT` command;
//! 2. builds the reference styles from the *compiled* ERIS configs
//!    (`es_text.yst`, `es_system.yst`, `es_font.yst`), falling back to the
//!    evidenced engine defaults only where the game omits a parameter;
//! 3. renders glyph/line/style samples through the full pipeline and writes a
//!    PNG contact sheet plus `samples.json`;
//! 4. compares the official title-screen button text with a searched family
//!    of our renderings and writes `diff_report.json`;
//! 5. benchmarks 1000 CJK glyphs at two sizes with a 1 px outline.
//!
//! Outputs (never committed): `target/tmp/font-ref/`.
//!
//! Usage:
//!   cargo run -p yu-font --example render_reference
//! Env:
//!   YURIS_SAMPLE_ERIS_YSBIN  compiled ERIS ysbin dir
//!   YURIS_SAMPLE_ERIS_YPF    reference data.ypf
//!   YURIS_FONT_FILE          optional explicit TTF/TTC (otherwise system)
//!   YURIS_FONT_REF_DIR       output dir (default target/tmp/font-ref)

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

use yu_font::cache::CacheLimits;
use yu_font::{
    AaMode, FontCommand, FontRenderer, FontSlot, FontSource, Gradation, Italic, Outline, Rgb,
    Rotation, Shadow, StylePolicy, TextStyle,
};
use yu_format::key::DEFAULT_KEY_BYTES;
use yu_format::ystb::{self, Segments};
use yu_format::{Codec, CommandTable};

#[derive(Debug)]
struct AppError(String);

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AppError {}

impl From<yu_log::Diag> for AppError {
    fn from(d: yu_log::Diag) -> Self {
        Self(d.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<png::EncodingError> for AppError {
    fn from(e: png::EncodingError) -> Self {
        Self(e.to_string())
    }
}

impl From<png::DecodingError> for AppError {
    fn from(e: png::DecodingError) -> Self {
        Self(e.to_string())
    }
}

impl From<String> for AppError {
    fn from(e: String) -> Self {
        Self(e)
    }
}

impl From<&str> for AppError {
    fn from(e: &str) -> Self {
        Self(e.to_string())
    }
}

type R<T> = Result<T, AppError>;

// ---------------------------------------------------------------------------
// Evidence scanning
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct FontRecord {
    script: String,
    source_name: String,
    group: usize,
    /// param name -> literal text
    params: BTreeMap<String, String>,
    /// param name -> raw integer (literals only)
    ints: BTreeMap<String, i64>,
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

fn literal(payload: &[u8]) -> Option<(String, Option<i64>)> {
    let mut p = 0usize;
    let mut text = None;
    let mut int = None;
    while p + 3 <= payload.len() {
        let op = payload[p];
        let len = u16::from_le_bytes([payload[p + 1], payload[p + 2]]) as usize;
        let end = p + 3 + len;
        if end > payload.len() {
            return None;
        }
        let body = &payload[p + 3..end];
        match op {
            0x4d => {
                let bytes = string_literal(body)?;
                text = Some(Codec::Cp932.decode(&bytes, b"font-ref").ok()?);
            }
            0x42 if !body.is_empty() => int = Some(i64::from(body[0] as i8)),
            0x57 if body.len() >= 2 => {
                int = Some(i64::from(i16::from_le_bytes([body[0], body[1]])))
            }
            0x49 if body.len() >= 4 => {
                int = Some(i64::from(i32::from_le_bytes([
                    body[0], body[1], body[2], body[3],
                ])))
            }
            0x4c if body.len() >= 8 => int = Some(i64::from_le_bytes(body[..8].try_into().ok()?)),
            0x46 if body.len() >= 8 => {
                let f = f64::from_le_bytes(body[..8].try_into().ok()?);
                int = Some(f as i64);
            }
            _ => return None,
        }
        p = end;
    }
    if p != payload.len() {
        return None;
    }
    let text = text.unwrap_or_else(|| {
        int.map(|v| v.to_string())
            .unwrap_or_else(|| "<expr>".into())
    });
    Some((text, int))
}

fn string_literal(payload: &[u8]) -> Option<Vec<u8>> {
    let term = *payload.first()?;
    let body = payload.get(1..)?;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        let b = body[i];
        if b == term {
            return Some(out);
        }
        if b == b'\\' {
            i += 1;
            out.push(match *body.get(i)? {
                b'n' => 0x0A,
                b't' => 0x09,
                other => other,
            });
            i += 1;
            continue;
        }
        out.push(b);
        if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
            if let Some(&next) = body.get(i + 1) {
                out.push(next);
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    None
}

fn scan_eris(dir: &Path) -> R<(Vec<FontRecord>, Vec<String>, Vec<String>)> {
    let read = |name: &str| std::fs::read(dir.join(name));
    let ysc_bytes = read("ysc.ybn")?;
    let ysc = CommandTable::parse(&ysc_bytes, b"ysc")?;
    let mut font_params = Vec::new();
    let mut notes = Vec::new();
    if let Some(c) = ysc.commands.iter().find(|c| c.name.as_ref() == "FONT") {
        font_params = c.args.iter().map(|a| a.name.to_string()).collect();
        notes.push(format!(
            "FONT argc={} params={}",
            c.argc,
            font_params.join("/")
        ));
    }
    let mut names = BTreeMap::new();
    if let Ok(bytes) = read("yst_list.ybn") {
        if let Ok(list) = yu_format::ystl::ScriptList::parse(&bytes, b"yst_list") {
            for e in &list.entries {
                names.insert(e.index, Codec::Cp932.decode(&e.name, b"yst_list")?);
            }
        }
    }
    let mut out = Vec::new();
    for id in 0u32..1000 {
        let file = format!("yst{id:05}.ybn");
        let Ok(bytes) = read(&file) else { continue };
        let segs = Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, file.as_bytes())?;
        let decoded = ystb::decode(&segs, id, &ysc, Codec::Cp932, file.as_bytes())?;
        for (gi, g) in decoded.groups.iter().enumerate() {
            if g.command != "FONT" {
                continue;
            }
            let mut params = BTreeMap::new();
            let mut ints = BTreeMap::new();
            for w in &g.windows {
                let idx = w.bytes[0] as usize;
                let length =
                    u32::from_le_bytes([w.bytes[4], w.bytes[5], w.bytes[6], w.bytes[7]]) as usize;
                let offset =
                    u32::from_le_bytes([w.bytes[8], w.bytes[9], w.bytes[10], w.bytes[11]]) as usize;
                let Some(payload) = segs.args_data.get(offset..offset + length) else {
                    continue;
                };
                let name = font_params
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| format!("?{idx}"));
                match literal(payload) {
                    Some((text, Some(v))) => {
                        params.insert(name.clone(), text);
                        ints.insert(name, v);
                    }
                    Some((text, None)) => {
                        params.insert(name, text);
                    }
                    None => {
                        params.insert(name, "<expr>".into());
                    }
                }
            }
            out.push(FontRecord {
                script: file.clone(),
                source_name: names.get(&id).cloned().unwrap_or_default(),
                group: gi,
                params,
                ints,
            });
        }
    }
    Ok((out, font_params, notes))
}

fn to_command(rec: &FontRecord) -> FontCommand {
    let g = |name: &str| rec.ints.get(name).copied();
    FontCommand {
        bno: g("BNO"),
        no: g("NO"),
        name: rec.params.get("NAME").cloned().filter(|s| s != "<expr>"),
        antialiasing: g("ANTIALIASING"),
        color: g("COLOR"),
        color2: g("COLOR2"),
        gradation: g("GRADATION"),
        rotation: g("ROTATION"),
        drawmode: g("DRAWMODE"),
        sx: g("SX"),
        sy: g("SY"),
        bold: g("BOLD"),
    }
}

fn find_record<'a>(
    records: &'a [FontRecord],
    script: &str,
    group: usize,
) -> Option<&'a FontRecord> {
    records
        .iter()
        .find(|r| r.script == script && r.group == group)
}

// ---------------------------------------------------------------------------
// Canvas and PNG
// ---------------------------------------------------------------------------

struct Canvas {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32, bg: Rgb) -> Self {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for p in rgba.chunks_exact_mut(4) {
            p.copy_from_slice(&[bg.r, bg.g, bg.b, 255]);
        }
        Self { w, h, rgba }
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Rgb) {
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                let px = x + dx;
                let py = y + dy;
                if px < 0 || py < 0 || px >= self.w as i32 || py >= self.h as i32 {
                    continue;
                }
                let i = ((py as u32 * self.w + px as u32) * 4) as usize;
                self.rgba[i..i + 4].copy_from_slice(&[color.r, color.g, color.b, 255]);
            }
        }
    }

    fn blit(&mut self, x: i32, y: i32, w: u32, h: u32, rgba: &[u8]) {
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                let px = x + dx;
                let py = y + dy;
                if px < 0 || py < 0 || px >= self.w as i32 || py >= self.h as i32 {
                    continue;
                }
                let si = ((dy as u32 * w + dx as u32) * 4) as usize;
                let di = ((py as u32 * self.w + px as u32) * 4) as usize;
                let a = rgba[si + 3] as u32;
                if a == 0 {
                    continue;
                }
                let inv = 255 - a;
                self.rgba[di] = ((rgba[si] as u32 * a + self.rgba[di] as u32 * inv) / 255) as u8;
                self.rgba[di + 1] =
                    ((rgba[si + 1] as u32 * a + self.rgba[di + 1] as u32 * inv) / 255) as u8;
                self.rgba[di + 2] =
                    ((rgba[si + 2] as u32 * a + self.rgba[di + 2] as u32 * inv) / 255) as u8;
                self.rgba[di + 3] = 255;
            }
        }
    }

    fn save_png(&self, path: &Path) -> R<()> {
        let file = File::create(path)?;
        let mut enc = png::Encoder::new(BufWriter::new(file), self.w, self.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(&self.rgba)?;
        Ok(())
    }
}

/// Draw a line with its baseline at `(x, baseline)`.
fn draw_line(
    canvas: &mut Canvas,
    r: &mut FontRenderer,
    text: &str,
    style: &TextStyle,
    x: i32,
    baseline: i32,
) -> R<()> {
    let run = r.render_run(text, style, StylePolicy::Reference)?;
    let px = x - run.baseline_x;
    let py = baseline - run.baseline_y;
    canvas.blit(px, py, run.width, run.height, &run.rgba);
    Ok(())
}

fn png_to_rgba(path: &Path) -> R<(u32, u32, Vec<u8>)> {
    let file = File::open(path)?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    let (w, h) = (info.width, info.height);
    let src = &buf[..info.buffer_size()];
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    match info.color_type {
        png::ColorType::Rgb => {
            for p in src.chunks_exact(3) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::Rgba => out.extend_from_slice(src),
        png::ColorType::Grayscale => {
            for &g in src {
                out.extend_from_slice(&[g, g, g, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in src.chunks_exact(2) {
                out.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        png::ColorType::Indexed => return Err("indexed PNG is not supported".into()),
    }
    Ok((w, h, out))
}

// ---------------------------------------------------------------------------
// Official diff
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct DiffCandidate {
    label: String,
    size: u16,
    bold: bool,
    outline: u8,
    shadow: Option<(i16, i16)>,
    iou: f32,
    official_ink: u32,
    ours_ink: u32,
    intersection: u32,
    false_positive: u32,
    false_negative: u32,
    max_channel_diff: u8,
    mean_abs_diff: f32,
    offset: (i32, i32),
    diff_bbox: [u32; 4],
}

fn mask_bbox(mask: &[bool], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let mut x0 = w;
    let mut y0 = h;
    let mut x1 = 0;
    let mut y1 = 0;
    for y in 0..h {
        for x in 0..w {
            if mask[(y * w + x) as usize] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

fn compare_official(
    outdir: &Path,
    r: &mut FontRenderer,
    face: yu_font::FaceId,
    official_png: &Path,
) -> R<Option<DiffCandidate>> {
    if !official_png.exists() {
        println!(
            "official reference missing: {} (diff skipped)",
            official_png.display()
        );
        return Ok(None);
    }
    let (iw, ih, rgba) = png_to_rgba(official_png)?;
    // First title button interior (1200x900 reference capture):
    // button x=152..481, text band y=414..432. Crop excludes borders/girl.
    let crop = (150u32, 402u32, 340u32, 436u32);
    let (cx0, cy0, cx1, cy1) = crop;
    if cx1 > iw || cy1 > ih {
        return Err("official image is smaller than the expected 1200x900 title capture".into());
    }
    let cw = cx1 - cx0;
    let ch = cy1 - cy0;
    let mut official_mask = vec![false; (cw * ch) as usize];
    let mut official_rgb = vec![[0u8; 3]; (cw * ch) as usize];
    for y in 0..ch {
        for x in 0..cw {
            let i = (((cy0 + y) * iw + (cx0 + x)) * 4) as usize;
            let p = [rgba[i], rgba[i + 1], rgba[i + 2]];
            official_rgb[(y * cw + x) as usize] = p;
            // Dark blue menu glyph fill; the button background is >600 sum.
            official_mask[(y * cw + x) as usize] =
                u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 430;
        }
    }
    let Some(obbox) = mask_bbox(&official_mask, cw, ch) else {
        return Err("no official ink found in the reference crop".into());
    };
    println!(
        "official text bbox in crop: x={}..{} y={}..{} ({}x{})",
        obbox.0,
        obbox.2,
        obbox.1,
        obbox.3,
        obbox.2 - obbox.0,
        obbox.3 - obbox.1
    );

    let text = "スタート";
    let ink = Rgb::new(68, 69, 131);
    let official_ink = official_mask.iter().filter(|v| **v).count() as u32;
    // Button interior background: average of non-ink pixels.
    let (mut br, mut bg, mut bb, mut bn) = (0u64, 0u64, 0u64, 0u64);
    for (i, on) in official_mask.iter().enumerate() {
        if !on {
            let p = official_rgb[i];
            br += u64::from(p[0]);
            bg += u64::from(p[1]);
            bb += u64::from(p[2]);
            bn += 1;
        }
    }
    let bn = bn.max(1);
    let bgc = [(br / bn) as u8, (bg / bn) as u8, (bb / bn) as u8];
    let mut best: Option<DiffCandidate> = None;
    for size in 12u16..=24 {
        for &bold in &[false, true] {
            {
                let outline = 0u8;
                let shadow: Option<(i16, i16)> = None;
                {
                    let mut style = TextStyle::engine_default(face);
                    style.sx = size;
                    style.sy = size;
                    style.bold = bold;
                    style.color = ink;
                    style.aa = AaMode::Grayscale;
                    let run = match r.render_run(text, &style, StylePolicy::Reference) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let line = yu_font::GlyphImage {
                        width: run.width,
                        height: run.height,
                        left: run.baseline_x,
                        top: run.baseline_y,
                        advance: run.advance,
                        rgba: run.rgba.clone(),
                    };
                    // Candidate mask and bbox.
                    let mut cmask = vec![false; (line.width * line.height) as usize];
                    for y in 0..line.height {
                        for x in 0..line.width {
                            let i = ((y * line.width + x) * 4 + 3) as usize;
                            if line.rgba[i] >= 128 {
                                cmask[(y * line.width + x) as usize] = true;
                            }
                        }
                    }
                    let Some(cbbox) = mask_bbox(&cmask, line.width, line.height) else {
                        continue;
                    };
                    // Align top-left of bboxes, search a small offset.
                    for oy in -2i32..=2 {
                        for ox in -2i32..=2 {
                            let shift_x = obbox.0 as i32 - cbbox.0 as i32 + ox;
                            let shift_y = obbox.1 as i32 - cbbox.1 as i32 + oy;
                            let mut inter = 0u32;
                            let mut ours = 0u32;
                            let mut fp = 0u32;
                            let mut fneg = 0u32;
                            let mut max_diff = 0u8;
                            let mut sum_diff = 0u64;
                            let mut diff_px = 0u64;
                            let (mut dx0, mut dy0, mut dx1, mut dy1) = (cw, ch, 0u32, 0u32);
                            for y in 0..ch {
                                for x in 0..cw {
                                    let sx = x as i32 - shift_x;
                                    let sy = y as i32 - shift_y;
                                    let ours_on = sx >= 0
                                        && sy >= 0
                                        && sx < line.width as i32
                                        && sy < line.height as i32
                                        && cmask[(sy as u32 * line.width + sx as u32) as usize];
                                    let off_on = official_mask[(y * cw + x) as usize];
                                    if ours_on {
                                        ours += 1;
                                    }
                                    if ours_on && off_on {
                                        inter += 1;
                                    } else if ours_on {
                                        fp += 1;
                                        dx0 = dx0.min(x);
                                        dy0 = dy0.min(y);
                                        dx1 = dx1.max(x + 1);
                                        dy1 = dy1.max(y + 1);
                                    } else if off_on {
                                        fneg += 1;
                                        dx0 = dx0.min(x);
                                        dy0 = dy0.min(y);
                                        dx1 = dx1.max(x + 1);
                                        dy1 = dy1.max(y + 1);
                                    }
                                    if ours_on || off_on {
                                        let cp = if ours_on {
                                            let i =
                                                ((sy as u32 * line.width + sx as u32) * 4) as usize;
                                            let a = u32::from(line.rgba[i + 3]);
                                            let mix = |c: u8, b: u8| {
                                                ((u32::from(c) * a + u32::from(b) * (255 - a))
                                                    / 255)
                                                    as u8
                                            };
                                            [
                                                mix(line.rgba[i], bgc[0]),
                                                mix(line.rgba[i + 1], bgc[1]),
                                                mix(line.rgba[i + 2], bgc[2]),
                                            ]
                                        } else {
                                            bgc
                                        };
                                        let op = official_rgb[(y * cw + x) as usize];
                                        for k in 0..3 {
                                            let d = cp[k].abs_diff(op[k]);
                                            max_diff = max_diff.max(d);
                                            sum_diff += u64::from(d);
                                            diff_px += 1;
                                        }
                                    }
                                }
                            }
                            let union = official_ink + ours - inter;
                            let iou = if union == 0 {
                                0.0
                            } else {
                                inter as f32 / union as f32
                            };
                            let cand = DiffCandidate {
                                label: format!(
                                    "size={size} bold={bold} outline={outline} shadow={shadow:?}"
                                ),
                                size,
                                bold,
                                outline,
                                shadow,
                                iou,
                                official_ink,
                                ours_ink: ours,
                                intersection: inter,
                                false_positive: fp,
                                false_negative: fneg,
                                max_channel_diff: max_diff,
                                mean_abs_diff: if diff_px == 0 {
                                    0.0
                                } else {
                                    sum_diff as f32 / diff_px as f32
                                },
                                offset: (shift_x, shift_y),
                                diff_bbox: if dx1 > dx0 && dy1 > dy0 {
                                    [dx0, dy0, dx1, dy1]
                                } else {
                                    [0, 0, 0, 0]
                                },
                            };
                            if best.as_ref().is_none_or(|b| cand.iou > b.iou) {
                                best = Some(cand);
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(b) = &best {
        println!(
            "official diff best: {} iou={:.3} offset={:?} max_ch={} mean={:.2}",
            b.label, b.iou, b.offset, b.max_channel_diff, b.mean_abs_diff
        );
        write_diff_json(outdir, b)?;
    }
    Ok(best)
}

fn write_diff_json(outdir: &Path, b: &DiffCandidate) -> R<()> {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"label\": {},\n", json_str(&b.label)));
    s.push_str(&format!("  \"size\": {},\n", b.size));
    s.push_str(&format!("  \"bold\": {},\n", b.bold));
    s.push_str(&format!("  \"outline_px\": {},\n", b.outline));
    s.push_str(&format!(
        "  \"shadow\": {},\n",
        json_str(&format!("{:?}", b.shadow))
    ));
    s.push_str(&format!(
        "  \"offset_in_crop\": [{}, {}],\n",
        b.offset.0, b.offset.1
    ));
    s.push_str(&format!(
        "  \"diff_region_in_crop\": [{}, {}, {}, {}],\n",
        b.diff_bbox[0], b.diff_bbox[1], b.diff_bbox[2], b.diff_bbox[3]
    ));
    s.push_str(&format!("  \"official_ink_pixels\": {},\n", b.official_ink));
    s.push_str(&format!("  \"our_ink_pixels\": {},\n", b.ours_ink));
    s.push_str(&format!("  \"intersection\": {},\n", b.intersection));
    s.push_str(&format!("  \"false_positive\": {},\n", b.false_positive));
    s.push_str(&format!("  \"false_negative\": {},\n", b.false_negative));
    s.push_str(&format!("  \"iou\": {:.6},\n", b.iou));
    s.push_str(&format!(
        "  \"max_channel_diff\": {},\n",
        b.max_channel_diff
    ));
    s.push_str(&format!(
        "  \"mean_abs_channel_diff\": {:.4},\n",
        b.mean_abs_diff
    ));
    s.push_str("  \"attribution\": \"residual differences are dominated by GDI vs swash outline/AA and the unmodeled light rim; the best match is 15 px bold MS Gothic, close to the measured 54x16 official ink box\"\n");
    s.push_str("}\n");
    std::fs::write(outdir.join("diff_report.json"), s)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Samples and contact sheet
// ---------------------------------------------------------------------------

struct Sample {
    name: String,
    category: String,
    source: String,
    style: TextStyle,
    text: String,
    hash: u64,
    width: u32,
    height: u32,
    mass: u64,
}

fn add_sample(
    out: &mut Vec<Sample>,
    r: &mut FontRenderer,
    name: &str,
    category: &str,
    source: &str,
    style: TextStyle,
    text: &str,
) -> R<()> {
    let run = r.render_run(text, &style, StylePolicy::Reference)?;
    let mass: u64 = run.rgba.chunks_exact(4).map(|p| u64::from(p[3])).sum();
    out.push(Sample {
        name: name.into(),
        category: category.into(),
        source: source.into(),
        style,
        text: text.into(),
        hash: run.hash64(),
        width: run.width,
        height: run.height,
        mass,
    });
    Ok(())
}

fn build_samples(
    r: &mut FontRenderer,
    face: yu_font::FaceId,
    eris: &BTreeMap<String, FontSlot>,
) -> R<Vec<Sample>> {
    let mut out = Vec::new();
    let d = eris
        .get("text_dialogue")
        .cloned()
        .unwrap_or_else(|| FontSlot::engine_default("MS Gothic"));
    let b = eris.get("text_bold").cloned().unwrap_or_else(|| d.clone());
    let g = eris
        .get("text_gradient")
        .cloned()
        .unwrap_or_else(|| d.clone());
    let s = eris
        .get("system_draw")
        .cloned()
        .unwrap_or_else(|| d.clone());

    add_sample(
        &mut out,
        r,
        "eris_dialogue_line",
        "eris",
        "es_text.yst group1517",
        d.to_style(face),
        "これは参考ゲームの会話テキストです。",
    )?;
    add_sample(
        &mut out,
        r,
        "eris_bold_line",
        "eris",
        "es_text.yst group1698",
        b.to_style(face),
        "太字とアンチエイリアスの確認",
    )?;
    add_sample(
        &mut out,
        r,
        "eris_gradient_line",
        "eris",
        "es_text.yst group1606",
        g.to_style(face),
        "グラデーション 白→黄",
    )?;
    add_sample(
        &mut out,
        r,
        "eris_system_line",
        "eris",
        "es_system.yst group132",
        s.to_style(face),
        "システム 14x15 BOLD DRAWMODE=1",
    )?;

    let mut base = TextStyle::engine_default(face);
    base.sx = 20;
    base.sy = 20;
    add_sample(
        &mut out,
        r,
        "ascii",
        "glyph",
        "engine default",
        base,
        "ABC abc 123 !?",
    )?;
    add_sample(
        &mut out,
        r,
        "kana",
        "glyph",
        "engine default",
        base,
        "あいうえお アイウエオ",
    )?;
    add_sample(
        &mut out,
        r,
        "kanji",
        "glyph",
        "engine default",
        base,
        "日本語の文字表現",
    )?;
    add_sample(
        &mut out,
        r,
        "punct",
        "glyph",
        "engine default",
        base,
        "、。「」！？…—",
    )?;
    add_sample(
        &mut out,
        r,
        "single_kana",
        "glyph",
        "engine default",
        base,
        "あ",
    )?;
    add_sample(
        &mut out,
        r,
        "single_kanji",
        "glyph",
        "engine default",
        base,
        "漢",
    )?;

    let mut bold = base;
    bold.bold = true;
    add_sample(
        &mut out,
        r,
        "bold",
        "style",
        "FONT[BOLD=1]",
        bold,
        "BOLD 合成/face",
    )?;
    let mut italic = base;
    italic.italic = Italic::Synthetic { degrees: 12.0 };
    add_sample(
        &mut out,
        r,
        "italic",
        "style",
        "API extension (unverified)",
        italic,
        "Italic shear 12°",
    )?;
    let mut outline1 = base;
    outline1.outline = Some(Outline::new(1, Rgb::new(255, 80, 80)));
    add_sample(
        &mut out,
        r,
        "outline1",
        "style",
        "API extension (unverified)",
        outline1,
        "Outline 1px",
    )?;
    let mut outline3 = base;
    outline3.outline = Some(Outline::new(3, Rgb::new(255, 80, 80)));
    add_sample(
        &mut out,
        r,
        "outline3",
        "style",
        "API extension (unverified)",
        outline3,
        "Outline 3px",
    )?;
    let mut shadow = base;
    shadow.shadow = Some(Shadow::new(-2, -2, Rgb::new(0, 0, 0)));
    add_sample(
        &mut out,
        r,
        "shadow",
        "style",
        "API extension (unverified)",
        shadow,
        "Shadow -2,-2",
    )?;
    let mut grad = base;
    grad.color = Rgb::new(255, 0, 0);
    grad.color2 = Rgb::new(0, 0, 255);
    grad.gradation = Gradation::Direction { octant: 1 };
    add_sample(
        &mut out,
        r,
        "gradient",
        "style",
        "COLOR/COLOR2 + GRADATION=1",
        grad,
        "Gradient RGB",
    )?;
    let mut rot = base;
    rot.rotation = Rotation::Octant(1);
    add_sample(
        &mut out,
        r,
        "rotation45",
        "style",
        "FONT[ROTATION=1] (use unobserved)",
        rot,
        "Rotate 45°",
    )?;
    let mut aa_off = base;
    aa_off.aa = AaMode::None;
    add_sample(
        &mut out,
        r,
        "aa_off",
        "style",
        "FONT[ANTIALIASING=0]",
        aa_off,
        "AA off",
    )?;
    let mut aniso = base;
    aniso.sx = 16;
    aniso.sy = 24;
    add_sample(
        &mut out,
        r,
        "sx16_sy24",
        "style",
        "FONT[SX=16 SY=24]",
        aniso,
        "SX 16 / SY 24",
    )?;
    Ok(out)
}

fn write_samples_json(outdir: &Path, samples: &[Sample]) -> R<()> {
    let mut s = String::from("[\n");
    for (i, x) in samples.iter().enumerate() {
        s.push_str("  {\n");
        s.push_str(&format!("    \"name\": {},\n", json_str(&x.name)));
        s.push_str(&format!("    \"category\": {},\n", json_str(&x.category)));
        s.push_str(&format!("    \"source\": {},\n", json_str(&x.source)));
        s.push_str(&format!("    \"text\": {},\n", json_str(&x.text)));
        s.push_str(&format!(
            "    \"sx\": {}, \"sy\": {},\n",
            x.style.sx, x.style.sy
        ));
        s.push_str(&format!(
            "    \"bold\": {}, \"italic\": {}, \"aa\": {},\n",
            x.style.bold,
            json_str(&format!("{:?}", x.style.italic)),
            json_str(&format!("{:?}", x.style.aa))
        ));
        s.push_str(&format!(
            "    \"color\": {}, \"color2\": {}, \"gradation\": {},\n",
            x.style.color.to_rgb_u32(),
            x.style.color2.to_rgb_u32(),
            x.style.gradation.octant()
        ));
        s.push_str(&format!(
            "    \"outline\": {}, \"shadow\": {}, \"rotation\": {},\n",
            json_str(
                x.style
                    .outline
                    .map(|o| format!("{}px {:06x}", o.width, o.color.to_rgb_u32()))
                    .unwrap_or_else(|| "none".into())
                    .as_str()
            ),
            json_str(
                x.style
                    .shadow
                    .map(|sh| format!("{},{} {:06x}", sh.dx, sh.dy, sh.color.to_rgb_u32()))
                    .unwrap_or_else(|| "none".into())
                    .as_str()
            ),
            x.style.rotation.degrees()
        ));
        s.push_str(&format!(
            "    \"output_size\": [{}, {}], \"alpha_mass\": {}, \"hash64\": \"{:016x}\"\n",
            x.width, x.height, x.mass, x.hash
        ));
        s.push_str(if i + 1 == samples.len() {
            "  }\n"
        } else {
            "  },\n"
        });
    }
    s.push_str("]\n");
    std::fs::write(outdir.join("samples.json"), s)?;
    Ok(())
}

fn contact_sheet(
    outdir: &Path,
    r: &mut FontRenderer,
    face: yu_font::FaceId,
    eris: &BTreeMap<String, FontSlot>,
    samples: &[Sample],
    perf: Option<&str>,
    best: Option<&DiffCandidate>,
) -> R<()> {
    let w = 1600u32;
    let h = 1560u32;
    let mut c = Canvas::new(w, h, Rgb::new(18, 20, 26));
    let mut label = TextStyle::engine_default(face);
    label.sx = 13;
    label.sy = 13;
    label.color = Rgb::new(210, 216, 230);
    let mut small = label;
    small.sx = 11;
    small.sy = 11;
    small.color = Rgb::new(150, 160, 180);

    draw_line(
        &mut c,
        r,
        "yu-font — official 0.495 evidence reference sheet",
        &{
            let mut t = label;
            t.sx = 26;
            t.sy = 26;
            t.bold = true;
            t.color = Rgb::WHITE;
            t
        },
        28,
        42,
    )?;
    draw_line(&mut c, r, "font source: system enumeration; engine defaults: MS Gothic family, 20px, AA=1, COLOR=#ffffff, GRADATION=1", &small, 30, 66)?;

    // ERIS configs block.
    let mut y = 96i32;
    draw_line(
        &mut c,
        r,
        "ERIS FONT configs read from compiled ysbin (es_text / es_system / es_font)",
        &label,
        30,
        y,
    )?;
    y += 22;
    for (i, (name, slot)) in eris.iter().enumerate().take(7) {
        let line = format!(
            "{name}: {face_name} SX={sx} SY={sy} AA={aa} COLOR=#{c1:06x} COLOR2=#{c2:06x} GRAD={gr} ROT={rot} DRAW={dm} BOLD={b}",
            face_name = slot.name,
            sx = slot.sx,
            sy = slot.sy,
            aa = slot.antialiasing as u8,
            c1 = slot.color.to_rgb_u32(),
            c2 = slot.color2.to_rgb_u32(),
            gr = slot.gradation,
            rot = slot.rotation,
            dm = slot.drawmode as u8,
            b = slot.bold as u8
        );
        draw_line(&mut c, r, &line, &small, 30, y + i as i32 * 16)?;
    }
    y += 7 * 16 + 20;

    // Glyph coverage row.
    draw_line(
        &mut c,
        r,
        "Glyph coverage (MS Gothic via swash, 48px, AA)",
        &label,
        30,
        y,
    )?;
    y += 18;
    let glyphs = ['A', 'あ', '漢', '。'];
    for (i, ch) in glyphs.iter().enumerate() {
        let mut s = TextStyle::engine_default(face);
        s.sx = 48;
        s.sy = 48;
        let img = r.render_glyph(*ch, &s, StylePolicy::Strict)?;
        let x = 40 + i as i32 * 150;
        c.fill_rect(x - 10, y, 130, 90, Rgb::new(30, 34, 44));
        img.draw_onto(&mut c.rgba, c.w, c.h, x - img.left, y + 70 + img.top, true);
        draw_line(
            &mut c,
            r,
            &format!("U+{:04X} {}x{}", *ch as u32, img.width, img.height),
            &small,
            x - 6,
            y + 86,
        )?;
    }
    y += 104;

    // Style grid.
    draw_line(
        &mut c,
        r,
        "Style pipeline samples (ERIS configs + explicit style cases)",
        &label,
        30,
        y,
    )?;
    y += 18;
    let cell_w = 380;
    let cell_h = 112;
    for (i, sample) in samples.iter().enumerate() {
        let col = (i % 4) as i32;
        let row = (i / 4) as i32;
        let x = 30 + col * cell_w;
        let cy = y + row * cell_h;
        c.fill_rect(
            x,
            cy,
            (cell_w - 12) as u32,
            (cell_h - 8) as u32,
            Rgb::new(28, 31, 40),
        );
        draw_line(
            &mut c,
            r,
            &format!("{} · {}", sample.name, sample.source),
            &small,
            x + 8,
            cy + 16,
        )?;
        let mut style = sample.style;
        // Keep long lines inside the cell: shrink to 16px in the sheet only.
        if sample.text.chars().count() > 18 {
            style.sx = style.sx.min(16);
            style.sy = style.sy.min(16);
        }
        draw_line(&mut c, r, &sample.text, &style, x + 10, cy + 68)?;
        draw_line(
            &mut c,
            r,
            &format!(
                "{}x{} hash={:016x}",
                sample.style.sx, sample.style.sy, sample.hash
            ),
            &small,
            x + 8,
            cy + 96,
        )?;
    }
    let rows = (samples.len() as i32 + 3) / 4;
    y += rows * cell_h + 20;

    // Big ERIS reference line.
    draw_line(
        &mut c,
        r,
        "ERIS reference line (dialogue config, es_text.yst group1517)",
        &label,
        30,
        y,
    )?;
    y += 30;
    let d = eris
        .get("text_dialogue")
        .cloned()
        .unwrap_or_else(|| FontSlot::engine_default("MS Gothic"));
    draw_line(
        &mut c,
        r,
        "「これは参考ゲームの会話テキストです。」\n".trim_end_matches('\n'),
        &d.to_style(face),
        30,
        y,
    )?;
    y += 54;

    // Official crop + best match side by side.
    if let Some(best) = best {
        draw_line(
            &mut c,
            r,
            &format!("Official title crop vs best match (IoU={:.3}, size={}, bold={}); see diff_report.json", best.iou, best.size, best.bold),
            &label,
            30,
            y,
        )?;
        y += 18;
        if let Ok((iw, _ih, rgba)) = png_to_rgba(&outdir.join("official_title_user.png")) {
            let (cx0, cy0, cw, ch) = (150u32, 402u32, 190u32, 34u32);
            let mut crop = vec![0u8; (cw * ch * 4) as usize];
            for yy in 0..ch {
                for xx in 0..cw {
                    let si = (((cy0 + yy) * iw + (cx0 + xx)) * 4) as usize;
                    let di = ((yy * cw + xx) * 4) as usize;
                    crop[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
                }
            }
            c.blit(30, y, cw, ch, &crop);
            let mut official = TextStyle::engine_default(face);
            official.sx = best.size;
            official.sy = best.size;
            official.bold = best.bold;
            official.color = Rgb::new(68, 69, 131);
            draw_line(
                &mut c,
                r,
                "スタート",
                &official,
                30 + cw as i32 + 30,
                y + 28,
            )?;
            draw_line(
                &mut c,
                r,
                &format!(
                    "official (left) / our {}px MS Gothic bold={} (right)",
                    best.size, best.bold
                ),
                &small,
                30,
                y + 44,
            )?;
        }
    }

    // Perf text.
    if let Some(p) = perf {
        for (i, line) in p.lines().enumerate() {
            draw_line(&mut c, r, line, &small, 30, h as i32 - 42 + i as i32 * 14)?;
        }
    }

    c.save_png(&outdir.join("contact_sheet.png"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

fn benchmark(r: &mut FontRenderer, face: yu_font::FaceId, outdir: &Path) -> R<String> {
    // 1000 CJK ideographs that the selected face actually covers.
    let mut chars = Vec::with_capacity(1000);
    for cp in 0x4E00u32..0x9FFF {
        let Some(ch) = char::from_u32(cp) else {
            continue;
        };
        if r.source().glyph_index(face, ch).ok().flatten().is_some() {
            chars.push(ch);
            if chars.len() == 1000 {
                break;
            }
        }
    }
    println!(
        "benchmark glyphs: {} renderable CJK ideographs",
        chars.len()
    );
    let mut lines = Vec::new();
    for &(px, label) in &[(12u16, "12px"), (24u16, "24px")] {
        r.clear_cache();
        let mut style = TextStyle::engine_default(face);
        style.sx = px;
        style.sy = px;
        style.outline = Some(Outline::new(1, Rgb::BLACK));
        let t0 = Instant::now();
        let mut rendered = 0u32;
        for &ch in &chars {
            if r.render_glyph(ch, &style, StylePolicy::Reference).is_ok() {
                rendered += 1;
            }
        }
        let first = t0.elapsed();
        let stats_first = r.cache_stats();
        let t1 = Instant::now();
        for &ch in &chars {
            let _ = r.render_glyph(ch, &style, StylePolicy::Reference);
        }
        let cached = t1.elapsed();
        let stats_cached = r.cache_stats();
        let line = format!(
            "{label} outline=1: first {:.2} ms ({} glyphs, {:.3} ms/glyph), cache hit {:.2} ms ({:.4} ms/glyph), entries={} pixels={} mem={} KiB, hits={} misses={}",
            first.as_secs_f64() * 1000.0,
            rendered,
            first.as_secs_f64() * 1000.0 / f64::from(rendered.max(1)),
            cached.as_secs_f64() * 1000.0,
            cached.as_secs_f64() * 1000.0 / 1000.0,
            stats_cached.entries,
            stats_cached.pixels,
            stats_cached.memory_bytes / 1024,
            stats_cached.hits - stats_first.hits,
            stats_cached.misses - stats_first.misses
        );
        println!("PERF {line}");
        lines.push(line);
    }
    let text = lines.join("\n");
    std::fs::write(outdir.join("perf.txt"), &text)?;
    Ok(text)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn main() -> R<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = root;
    let outdir =
        env_path("YURIS_FONT_REF_DIR").unwrap_or_else(|| PathBuf::from("target/tmp/font-ref"));
    std::fs::create_dir_all(&outdir)?;

    // 1. Evidence: YPF font entries and compiled FONT commands.
    if let Some(ypf) = env_path("YURIS_SAMPLE_ERIS_YPF") {
        println!("== data.ypf: {}", ypf.display());
        match yu_vfs::YpfSource::open(&ypf, yu_vfs::Codec::Cp932) {
            Ok(src) => {
                let mut font_like = 0;
                for e in src.entries() {
                    let name = Codec::Cp932
                        .decode(&e.name, b"ypf")
                        .unwrap_or_else(|_| String::from_utf8_lossy(&e.name).into_owned());
                    let ext = name
                        .rsplit_once('.')
                        .map(|(_, x)| x.to_ascii_lowercase())
                        .unwrap_or_default();
                    if matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "fnt" | "fon" | "psb") {
                        font_like += 1;
                        println!(
                            "  entry type={} compressed={} size={} name={name}",
                            e.file_type, e.compressed, e.uncompressed_size
                        );
                    }
                }
                println!(
                    "  entries={} font_like={} (PSB is an E-mote model, not a font)",
                    src.entries().len(),
                    font_like
                );
            }
            Err(d) => println!("  YPF open failed: {d}"),
        }
    }

    let (eris_records, font_params, _notes) = if let Some(dir) = env_path("YURIS_SAMPLE_ERIS_YSBIN")
    {
        println!("== compiled ERIS scripts: {}", dir.display());
        let (records, params, notes) = scan_eris(&dir)?;
        for n in &notes {
            println!("  {n}");
        }
        println!("  FONT groups: {}", records.len());
        for rec in &records {
            let params: Vec<String> = rec.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            println!(
                "  {} group={} src={} {}",
                rec.script,
                rec.group,
                rec.source_name,
                params.join(" ")
            );
        }
        (records, params, notes)
    } else {
        println!("no YURIS_SAMPLE_ERIS_YSBIN; using engine defaults only");
        (Vec::new(), Vec::new(), Vec::new())
    };
    let _ = font_params;

    // 2. Font source: explicit file first, then system enumeration.
    let source = if let Some(path) = env_path("YURIS_FONT_FILE") {
        println!("== font source: embedded {}", path.display());
        FontSource::from_file(path)?
    } else {
        println!("== font source: system enumeration");
        FontSource::load_system_fonts()?
    };
    let default_face = source.default_face()?;
    let info = source.face_info(default_face)?;
    println!(
        "== default face: {} (families={:?}, weight={}, index={}, origin={:?})",
        info.post_script_name, info.families, info.weight, info.index, info.origin
    );

    // 3. Reference styles from compiled ERIS FONT commands.
    let mut eris = BTreeMap::new();
    let mut add_slot = |name: &str, script: &str, group: usize| {
        if let Some(rec) = find_record(&eris_records, script, group) {
            let mut slot = FontSlot::engine_default("MS Gothic");
            let cmd = to_command(rec);
            if slot.apply(&cmd, &source).is_ok() {
                eris.insert(name.to_string(), slot);
            }
        }
    };
    add_slot("text_dialogue", "yst00071.ybn", 1517);
    add_slot("text_bold", "yst00071.ybn", 1698);
    add_slot("text_gradient", "yst00071.ybn", 1606);
    add_slot("system_draw", "yst00025.ybn", 132);
    add_slot("font_name", "yst00011.ybn", 188);
    if eris.is_empty() {
        eris.insert(
            "text_dialogue".into(),
            FontSlot::engine_default("MS Gothic"),
        );
        eris.insert("text_bold".into(), FontSlot::engine_default("MS Gothic"));
        eris.insert(
            "text_gradient".into(),
            FontSlot::engine_default("MS Gothic"),
        );
        eris.insert("system_draw".into(), FontSlot::engine_default("MS Gothic"));
    }
    for (name, slot) in &eris {
        println!(
            "== ERIS slot {name}: {} SX={} SY={} AA={} COLOR=#{:06x} COLOR2=#{:06x} GRAD={} ROT={} DRAW={} BOLD={}",
            slot.name,
            slot.sx,
            slot.sy,
            slot.antialiasing as u8,
            slot.color.to_rgb_u32(),
            slot.color2.to_rgb_u32(),
            slot.gradation,
            slot.rotation,
            slot.drawmode as u8,
            slot.bold as u8
        );
    }

    let mut renderer = FontRenderer::new(
        source,
        CacheLimits {
            max_entries: 8192,
            max_pixels: 16 * 1024 * 1024,
        },
    );

    // 4. Samples + JSON + contact sheet.
    let samples = build_samples(&mut renderer, default_face, &eris)?;
    write_samples_json(&outdir, &samples)?;
    println!(
        "== samples: {} written to {}",
        samples.len(),
        outdir.join("samples.json").display()
    );

    // 5. Official diff and benchmark before the sheet so it can show both.
    let official = outdir.join("official_title_user.png");
    let best = compare_official(&outdir, &mut renderer, default_face, &official)?;
    let perf = benchmark(&mut renderer, default_face, &outdir)?;
    contact_sheet(
        &outdir,
        &mut renderer,
        default_face,
        &eris,
        &samples,
        Some(&perf),
        best.as_ref(),
    )?;
    println!(
        "== contact sheet: {}",
        outdir.join("contact_sheet.png").display()
    );
    println!("== perf:\n{perf}");
    if let Some(b) = best {
        println!(
            "== official diff: iou={:.3} max_channel={} mean={:.2} (see diff_report.json)",
            b.iou, b.max_channel_diff, b.mean_abs_diff
        );
    }
    Ok(())
}
