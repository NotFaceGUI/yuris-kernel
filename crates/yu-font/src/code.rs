//! Stable diagnostic codes for the font boundary.
//!
//! Codes are data: countable, golden-testable and linked from
//! `docs/font-contract.md`. Every code is registered in [`ALL_CODES`].

/// Font source boundary.
pub mod font {
    /// A face, family or font file could not be found.
    pub const MISSING: &str = "font.missing";
    /// A requested style (bold/italic) has no matching face and no verified
    /// synthesis rule.
    pub const MISSING_STYLE: &str = "font.missing_style";
    /// The character has no glyph in the selected face.
    pub const GLYPH_MISSING: &str = "font.glyph_missing";
    /// Font bytes are not a usable sfnt/TTC font, or a face index is invalid.
    pub const BAD_FACE: &str = "font.bad_face";
    /// A font file could not be read (desktop only).
    pub const IO: &str = "font.io";
    /// System font enumeration is unavailable on this target.
    pub const SYSTEM_UNAVAILABLE: &str = "font.system_unavailable";
    /// The requested feature exists in the API but its engine semantics are
    /// not evidenced; callers must opt into the reference policy explicitly.
    pub const UNVERIFIED: &str = "font.unverified";
    /// Known but not implemented semantics (ClearType, miter joins, blur).
    pub const UNIMPLEMENTED: &str = "font.unimplemented";
    /// Style parameters are outside the engine's accepted range.
    pub const STYLE_RANGE: &str = "font.style_range";
}

/// All registered codes, in stable order. Uniqueness is tested.
pub const ALL_CODES: &[&str] = &[
    font::MISSING,
    font::MISSING_STYLE,
    font::GLYPH_MISSING,
    font::BAD_FACE,
    font::IO,
    font::SYSTEM_UNAVAILABLE,
    font::UNVERIFIED,
    font::UNIMPLEMENTED,
    font::STYLE_RANGE,
];

/// Whether `code` is a registered font code.
#[must_use]
pub fn is_registered(code: &str) -> bool {
    ALL_CODES.contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique_and_namespaced() {
        let mut seen = BTreeSet::new();
        for code in ALL_CODES {
            assert!(seen.insert(*code), "duplicate code: {code}");
            assert!(code.starts_with("font."), "bad namespace: {code}");
        }
    }
}
