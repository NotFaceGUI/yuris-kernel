//! `yu-font` — YU-RIS font sources, glyph rasterization and text style
//! composition.
//!
//! # Scope
//!
//! * font discovery/loading (`FontSource`, `FaceId`, engine default family
//!   fallback);
//! * glyph rasterization through swash into 8-bit coverage;
//! * the complete style pipeline (SX/SY, BOLD, ANTIALIASING, COLOR/COLOR2,
//!   GRADATION, DRAWMODE, outline, shadow, ROTATION, italic, kerning);
//! * a bounded deterministic glyph cache;
//! * layout **interfaces** only — engine line breaking/alignment/ruby lives
//!   in `yu-gfx/text`.
//!
//! # Evidence
//!
//! `docs/font-provenance.md` records the official 0.495 facts (REA addresses,
//! manual pages, compiled ERIS FONT commands, reference screenshot).
//! `docs/font-contract.md` is the interface/style contract, including every
//! `Unverified`/`Unimplemented` item.
//!
//! The production path is [`StylePolicy::Strict`]: it rejects style effects
//! without engine evidence. [`StylePolicy::Reference`] is the explicit opt-in
//! used by `examples/render_reference.rs`.
//!
//! # Example
//!
//! ```no_run
//! use yu_font::{AaMode, FontRenderer, FontSource, StylePolicy, TextStyle};
//! use yu_font::cache::CacheLimits;
//!
//! # fn main() -> Result<(), yu_log::Diag> {
//! let source = FontSource::load_system_fonts()?;
//! let face = source.default_face()?;
//! let mut renderer = FontRenderer::new(source, CacheLimits::default());
//! let style = TextStyle::engine_default(face);
//! let image = renderer.render_glyph('あ', &style, StylePolicy::Strict)?;
//! assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
//! # Ok(())
//! # }
//! ```
//!
//! `clippy::result_large_err` is allowed because diagnostics are structured
//! [`yu_log::Diag`] values and are part of the public contract.

#![allow(clippy::result_large_err)]

pub mod cache;
pub mod code;
pub mod color;
pub mod config;
pub mod image;
pub mod layout;
pub mod raster;
pub mod render;
pub mod source;
pub mod style;

pub use cache::{CacheLimits, CacheStats, GlyphCache, GlyphKey};
pub use color::{Rgb, Rgba};
pub use config::{FontCommand, FontSlot};
pub use image::{GlyphBitmap, GlyphImage, GlyphRun};
pub use layout::{HorizontalLayout, PlacedGlyph, TextLayout};
pub use raster::{RasterParams, Rasterizer};
pub use render::{compose_glyph, render_glyph_once, FontRenderer};
pub use source::{
    FaceId, FaceInfo, FaceOrigin, FaceStyle, FontSource, ResolvedFace, ENGINE_DEFAULT_FAMILIES,
};
pub use style::{
    AaMode, Capability, DrawMode, Gradation, GradationScope, Italic, Kerning, Outline, OutlineJoin,
    Rotation, Shadow, StylePolicy, Support, TextStyle, CAPABILITIES,
};
