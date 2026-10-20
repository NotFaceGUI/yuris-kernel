//! Font discovery and face selection.
//!
//! Evidence: the 0.495 engine bundles no font (`data.ypf` has zero
//! `.ttf/.otf/.ttc/.fnt` entries), enumerates installed fonts with
//! `EnumFontFamiliesExA(SHIFTJIS_CHARSET)` and tries a fixed fallback list:
//! "ＭＳ ゴシック", "標準ゴシック", "ＭＳ Ｐゴシック", "ＭＳ 明朝",
//! "標準明朝" (REA `FUN_00403afc` @ 0x403afc, strings @ 0x807008..0x807044).
//!
//! `FaceId` is source-local: it indexes the face table of one [`FontSource`].
//! A cache must therefore never mix faces from two sources.

use std::collections::HashMap;
use yu_log::{Boundary, Diag};

use crate::code;
use crate::style::Italic;

/// Engine fallback list, in the exact order `FUN_00403afc` tries it.
pub const ENGINE_DEFAULT_FAMILIES: &[&str] = &[
    "ＭＳ ゴシック",
    "標準ゴシック",
    "ＭＳ Ｐゴシック",
    "ＭＳ 明朝",
    "標準明朝",
];

/// Where a face came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceOrigin {
    /// Bytes supplied by the caller (self-contained game fonts).
    Embedded,
    /// Enumerated from the host system.
    System,
}

/// Basic style of a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceStyle {
    Normal,
    Italic,
    Oblique,
}

/// Public, source-local face identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceId(pub(crate) u32);

impl FaceId {
    #[cfg(test)]
    pub(crate) const fn synthetic_for_tests(id: u32) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Description of one face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceInfo {
    pub id: FaceId,
    /// Every localized family name, in font order.
    pub families: Vec<String>,
    pub post_script_name: String,
    pub weight: u16,
    pub style: FaceStyle,
    /// Face index inside its font file (TTC index).
    pub index: u32,
    pub origin: FaceOrigin,
}

#[derive(Clone, Debug)]
struct FaceRecord {
    db_id: fontdb::ID,
    families: Vec<String>,
    family_key: String,
    post_script_name: String,
    weight: u16,
    style: FaceStyle,
    index: u32,
    origin: FaceOrigin,
}

/// Result of resolving bold/italic for a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedFace {
    pub face: FaceId,
    /// No bold member existed; the rasterizer must synthesize weight.
    pub synthetic_bold: bool,
    /// No italic member existed; the rasterizer must shear.
    pub synthetic_italic: bool,
}

/// A source-local face table.
pub struct FontSource {
    db: fontdb::Database,
    faces: Vec<FaceRecord>,
    by_db: HashMap<fontdb::ID, FaceId>,
    origin: FaceOrigin,
}

impl std::fmt::Debug for FontSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontSource")
            .field("faces", &self.faces.len())
            .field("origin", &self.origin)
            .finish()
    }
}

impl FontSource {
    /// Load font bytes (TrueType/OpenType/TTC). A self-contained game font is
    /// preferred over system fonts by the caller.
    pub fn from_bytes(bytes: Vec<u8>, origin: FaceOrigin) -> Result<Self, Diag> {
        let mut db = fontdb::Database::new();
        let before = db.len();
        db.load_font_data(bytes);
        if db.len() == before {
            return Err(Diag::fatal(code::font::BAD_FACE, Boundary::Data)
                .with_detail("font bytes contain no sfnt/TTC face"));
        }
        let ids: Vec<fontdb::ID> = db.faces().map(|f| f.id).collect();
        let mut faces = Vec::new();
        let mut by_db = HashMap::new();
        for id in ids {
            let Some(info) = db.face(id) else { continue };
            let families: Vec<String> = info.families.iter().map(|(n, _)| n.clone()).collect();
            let family_key = families
                .first()
                .map(|n| normalize_name(n))
                .unwrap_or_default();
            let record = FaceRecord {
                db_id: id,
                families,
                family_key,
                post_script_name: info.post_script_name.clone(),
                weight: info.weight.0,
                style: match info.style {
                    fontdb::Style::Normal => FaceStyle::Normal,
                    fontdb::Style::Italic => FaceStyle::Italic,
                    fontdb::Style::Oblique => FaceStyle::Oblique,
                },
                index: info.index,
                origin,
            };
            by_db.insert(id, FaceId(faces.len() as u32));
            faces.push(record);
        }
        if faces.is_empty() {
            return Err(Diag::fatal(code::font::BAD_FACE, Boundary::Data)
                .with_detail("fontdb exposed no usable face"));
        }
        Ok(Self {
            db,
            faces,
            by_db,
            origin,
        })
    }

    /// Load one font file from disk (desktop/Android only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, Diag> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| {
            Diag::fatal(code::font::IO, Boundary::Platform)
                .with_detail(format!("cannot read font {}: {e}", path.display()))
        })?;
        Self::from_bytes(bytes, FaceOrigin::Embedded)
    }

    /// Enumerate system fonts. Explicitly unavailable on wasm; never silently
    /// falls back to a bundled face.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_system_fonts() -> Result<Self, Diag> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        if db.is_empty() {
            return Err(Diag::fatal(code::font::MISSING, Boundary::Platform)
                .with_detail("system font enumeration returned zero faces"));
        }
        let mut faces = Vec::new();
        let mut by_db = HashMap::new();
        for info in db.faces() {
            let families: Vec<String> = info.families.iter().map(|(n, _)| n.clone()).collect();
            let family_key = families
                .first()
                .map(|n| normalize_name(n))
                .unwrap_or_default();
            by_db.insert(info.id, FaceId(faces.len() as u32));
            faces.push(FaceRecord {
                db_id: info.id,
                families,
                family_key,
                post_script_name: info.post_script_name.clone(),
                weight: info.weight.0,
                style: match info.style {
                    fontdb::Style::Normal => FaceStyle::Normal,
                    fontdb::Style::Italic => FaceStyle::Italic,
                    fontdb::Style::Oblique => FaceStyle::Oblique,
                },
                index: info.index,
                origin: FaceOrigin::System,
            });
        }
        Ok(Self {
            db,
            faces,
            by_db,
            origin: FaceOrigin::System,
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub fn load_system_fonts() -> Result<Self, Diag> {
        Err(
            Diag::fatal(code::font::SYSTEM_UNAVAILABLE, Boundary::Platform)
                .with_detail("system font enumeration is unavailable on wasm32; supply font bytes"),
        )
    }

    /// Origin of all faces in this source.
    #[must_use]
    pub fn origin(&self) -> FaceOrigin {
        self.origin
    }

    /// Number of faces.
    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Description of one face.
    pub fn face_info(&self, face: FaceId) -> Result<FaceInfo, Diag> {
        let r = self.record(face)?;
        Ok(FaceInfo {
            id: face,
            families: r.families.clone(),
            post_script_name: r.post_script_name.clone(),
            weight: r.weight,
            style: r.style,
            index: r.index,
            origin: r.origin,
        })
    }

    /// Deterministic family list: primary family name, sorted, deduplicated.
    /// This is **not** the Windows `EnumFontFamiliesExA` order; `NO` indices
    /// documented as a divergence.
    #[must_use]
    pub fn family_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .faces
            .iter()
            .map(|f| {
                f.families
                    .first()
                    .cloned()
                    .unwrap_or_else(|| f.post_script_name.clone())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Resolve a family by name, localized name or PostScript name.
    /// Fullwidth ASCII, spaces and hyphens are folded, so "ＭＳ ゴシック"
    /// matches "MS Gothic".
    #[must_use]
    pub fn find_family(&self, name: &str) -> Option<FaceId> {
        let want = normalize_name(name);
        let mut best: Option<FaceId> = None;
        for (idx, f) in self.faces.iter().enumerate() {
            let matched = f.family_key == want
                || f.families.iter().any(|n| normalize_name(n) == want)
                || normalize_name(&f.post_script_name) == want;
            if !matched {
                continue;
            }
            let id = FaceId(idx as u32);
            let score = (
                (f.style == FaceStyle::Normal) as u8,
                (f.weight == 400) as u8,
                u32::MAX - idx as u32,
            );
            let best_score = best.map(|b| {
                let r = &self.faces[b.0 as usize];
                (
                    (r.style == FaceStyle::Normal) as u8,
                    (r.weight == 400) as u8,
                    u32::MAX - b.0,
                )
            });
            if best_score.is_none_or(|s| score > s) {
                best = Some(id);
            }
        }
        best
    }

    /// Engine default: try [`ENGINE_DEFAULT_FAMILIES`] in order. No silent
    /// fallback to a generic family.
    pub fn default_face(&self) -> Result<FaceId, Diag> {
        for name in ENGINE_DEFAULT_FAMILIES {
            if let Some(id) = self.find_family(name) {
                return Ok(id);
            }
        }
        Err(
            Diag::fatal(code::font::MISSING, Boundary::Platform).with_detail(format!(
                "none of the engine default families is installed: {}",
                ENGINE_DEFAULT_FAMILIES.join(", ")
            )),
        )
    }

    /// `FONT[NO=n]` lookup using this crate's deterministic family order.
    ///
    /// Divergence: the engine's `NO` indexes the GDI enumeration order, which
    /// is not portable. Callers that need parity should use `NAME`.
    #[must_use]
    pub fn face_by_no(&self, no: usize) -> Option<FaceId> {
        let mut sorted: Vec<(String, u32, FaceId)> = self
            .faces
            .iter()
            .enumerate()
            .map(|(i, f)| {
                (
                    f.families
                        .first()
                        .cloned()
                        .unwrap_or_else(|| f.post_script_name.clone()),
                    f.index,
                    FaceId(i as u32),
                )
            })
            .collect();
        sorted.sort_by(|a, b| {
            a.0.to_lowercase()
                .cmp(&b.0.to_lowercase())
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        sorted.get(no).map(|x| x.2)
    }

    /// Resolve bold/italic to an actual face, reporting synthetic fallbacks.
    pub fn resolve_style(
        &self,
        face: FaceId,
        bold: bool,
        italic: Italic,
    ) -> Result<ResolvedFace, Diag> {
        let base = self.record(face)?;
        let want_italic = matches!(italic, Italic::Face);
        let want_style = if want_italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        };
        let want_weight = if bold { 700 } else { 400 };
        let mut chosen: Option<&FaceRecord> = None;
        'outer: for family in &base.families {
            let query = fontdb::Query {
                families: &[fontdb::Family::Name(family)],
                weight: fontdb::Weight(want_weight),
                stretch: fontdb::Stretch::Normal,
                style: want_style,
            };
            if let Some(id) = self.db.query(&query) {
                if let Some(&fid) = self.by_db.get(&id) {
                    let rec = &self.faces[fid.0 as usize];
                    if rec.family_key == base.family_key {
                        chosen = Some(rec);
                        break 'outer;
                    }
                }
            }
        }
        let rec = chosen.unwrap_or(base);
        let synthetic_bold = bold && rec.weight < 600;
        let synthetic_italic = match italic {
            Italic::None => false,
            Italic::Synthetic { .. } => true,
            Italic::Face => rec.style == FaceStyle::Normal,
        };
        let index = self
            .faces
            .iter()
            .position(|f| f.db_id == rec.db_id)
            .ok_or_else(|| {
                Diag::fatal(code::font::BAD_FACE, Boundary::Data)
                    .with_detail("resolved face is missing from the source table")
            })?;
        Ok(ResolvedFace {
            face: FaceId(index as u32),
            synthetic_bold,
            synthetic_italic,
        })
    }

    /// Run `f` with the raw bytes and TTC face index of `face`.
    pub fn with_face_data<T>(
        &self,
        face: FaceId,
        f: impl FnOnce(&[u8], u32) -> T,
    ) -> Result<T, Diag> {
        let r = self.record(face)?;
        self.db.with_face_data(r.db_id, f).ok_or_else(|| {
            Diag::fatal(code::font::BAD_FACE, Boundary::Data)
                .with_detail("fontdb could not expose face bytes")
        })
    }

    /// Glyph id for `ch`, or `None` when the face has no glyph.
    pub fn glyph_index(&self, face: FaceId, ch: char) -> Result<Option<u16>, Diag> {
        self.with_face_data(face, |bytes, index| {
            let font = swash::FontRef::from_index(bytes, index as usize)?;
            let gid = font.charmap().map(ch);
            if gid == 0 && ch != '\0' {
                None
            } else {
                Some(gid)
            }
        })
    }

    /// Horizontal advance in pixels for an explicit glyph id.
    pub fn advance_px(&self, face: FaceId, gid: u16, sx: u16, sy: u16) -> Result<f32, Diag> {
        if sy == 0 {
            return Ok(0.0);
        }
        self.with_face_data(face, |bytes, index| {
            let Some(font) = swash::FontRef::from_index(bytes, index as usize) else {
                return 0.0;
            };
            let scale = f32::from(sy);
            let x_scale = f32::from(sx) / scale;
            font.glyph_metrics(&[]).scale(scale).advance_width(gid) * x_scale
        })
    }

    /// Font design units per em (0 when the face is unusable).
    pub fn units_per_em(&self, face: FaceId) -> Result<u16, Diag> {
        self.with_face_data(face, |bytes, index| {
            swash::FontRef::from_index(bytes, index as usize)
                .map(|f| f.metrics(&[]).units_per_em)
                .unwrap_or(0)
        })
    }

    /// Legacy TrueType `kern` table, format 0, horizontal pairs.
    /// Returns the kerning value in font units.
    pub fn kerning_units(&self, face: FaceId, left: u16, right: u16) -> Result<i16, Diag> {
        self.with_face_data(face, |bytes, index| {
            let Some(font) = swash::FontRef::from_index(bytes, index as usize) else {
                return 0;
            };
            let Some(table) = font.table(swash::tag_from_bytes(b"kern")) else {
                return 0;
            };
            parse_kern_format0(table, left, right).unwrap_or(0)
        })
    }

    fn record(&self, face: FaceId) -> Result<&FaceRecord, Diag> {
        self.faces.get(face.0 as usize).ok_or_else(|| {
            Diag::fatal(code::font::MISSING, Boundary::Data)
                .with_detail(format!("face index {} is not in this source", face.0))
        })
    }
}

/// Parse the first matching pair in a format-0 `kern` subtable.
fn parse_kern_format0(table: &[u8], left: u16, right: u16) -> Option<i16> {
    let u16_at = |at: usize| -> Option<u16> {
        table
            .get(at..at + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let n_tables = u16_at(2)?;
    let mut at = 4usize;
    for _ in 0..n_tables {
        let length = u16_at(at + 2)? as usize;
        let coverage = u16_at(at + 4)?;
        let format = (coverage >> 8) as u8;
        if format == 0 && length >= 14 {
            let n_pairs = u16_at(at + 6)? as usize;
            let mut p = at + 14;
            for _ in 0..n_pairs {
                let l = u16_at(p)?;
                let r = u16_at(p + 2)?;
                let v = u16_at(p + 4)? as i16;
                if l == left && r == right {
                    return Some(v);
                }
                p += 6;
            }
        }
        at = at.checked_add(length)?;
        if at > table.len() {
            return None;
        }
    }
    None
}

/// Fold fullwidth ASCII, case, spaces and hyphens for name matching.
#[must_use]
pub fn normalize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        let c = match ch {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
            _ => ch,
        };
        if c.is_whitespace() || c == '-' || c == '_' {
            continue;
        }
        for l in c.to_lowercase() {
            out.push(l);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_font_name_folds_fullwidth_ascii() {
        // The engine fallback string matches fontdb's Japanese family name for
        // MS Gothic; ASCII "MS Gothic" is a separate localized alias.
        assert_eq!(
            normalize_name("ＭＳ ゴシック"),
            normalize_name("ＭＳ　ゴシック")
        );
        assert_eq!(normalize_name("ＭＳ Ｐゴシック"), "mspゴシック");
        assert_ne!(normalize_name("ＭＳ 明朝"), normalize_name("ＭＳ ゴシック"));
    }

    #[test]
    fn kern_format0_parser_reads_a_pair() {
        // header(version=0, nTables=1) + subtable(format 0, 1 pair)
        let mut t = Vec::new();
        t.extend_from_slice(&0u16.to_be_bytes()); // version
        t.extend_from_slice(&1u16.to_be_bytes()); // nTables
        t.extend_from_slice(&0u16.to_be_bytes()); // subtable version
        t.extend_from_slice(&20u16.to_be_bytes()); // length
        t.extend_from_slice(&0x0001u16.to_be_bytes()); // coverage: format 0, horizontal
        t.extend_from_slice(&1u16.to_be_bytes()); // nPairs
        t.extend_from_slice(&0u16.to_be_bytes()); // searchRange
        t.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
        t.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        t.extend_from_slice(&1u16.to_be_bytes()); // left
        t.extend_from_slice(&2u16.to_be_bytes()); // right
        t.extend_from_slice(&(-50i16).to_be_bytes()); // value
        assert_eq!(parse_kern_format0(&t, 1, 2), Some(-50));
        assert_eq!(parse_kern_format0(&t, 1, 3), None);
    }

    #[test]
    fn bad_font_bytes_are_a_diag_not_a_panic() {
        let err = FontSource::from_bytes(vec![0u8; 32], FaceOrigin::Embedded).unwrap_err();
        assert_eq!(err.code, code::font::BAD_FACE);
    }
}
