//! CP932 (Shift_JIS) byte → text decoding for text drawing.
//!
//! Engine text buffers are CP932 bytes (`$_TEXT`, `WORD`, `CGACT[SETSTR]`).
//! `yu-gfx` keeps the raw bytes; glyph rasterization needs `char`s. This
//! module is the single decoder used by both render backends so soft and wgpu
//! always see the same character stream.
//!
//! Undecodable bytes are reported (not silently dropped): callers can count
//! them in their render report.

/// Result of decoding a CP932 byte slice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecodedText {
    /// Decoded characters in order.
    pub chars: Vec<char>,
    /// Number of invalid byte sequences (mapped to U+FFFD).
    pub invalid: usize,
}

/// Decode a CP932 byte slice.
///
/// Uses the WHATWG `shift_jis` mapping through `encoding_rs` (the same mapping
/// Windows CP932 uses for the characters YU-RIS scripts contain). Invalid
/// sequences become U+FFFD and are counted in [`DecodedText::invalid`].
#[must_use]
pub fn decode(bytes: &[u8]) -> DecodedText {
    let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(bytes);
    let chars: Vec<char> = text.chars().collect();
    let invalid = if had_errors {
        chars.iter().filter(|&&c| c == '\u{FFFD}').count().max(1)
    } else {
        0
    };
    DecodedText { chars, invalid }
}

/// Decode one CP932 character (1 or 2 bytes) exactly.
///
/// Returns `None` for empty input or a byte sequence that is not exactly one
/// decodable character (trailing garbage included).
#[must_use]
pub fn decode_char(bytes: &[u8]) -> Option<char> {
    let decoded = decode(bytes);
    if decoded.invalid > 0 || decoded.chars.len() != 1 {
        return None;
    }
    Some(decoded.chars[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_full_and_half_width() {
        let d = decode(b"A\x82\xB1B");
        assert_eq!(d.chars, vec!['A', 'こ', 'B']);
        assert_eq!(d.invalid, 0);
        assert_eq!(decode_char(b"\x82\xB1"), Some('こ'));
        assert_eq!(decode_char(b"A"), Some('A'));
    }

    #[test]
    fn ascii_punctuation_follows_cp932_not_shift_jis_strict() {
        // Windows CP932: 0x5C/0x7E 保留 ASCII(不是 ¥/‾)。
        assert_eq!(decode_char(&[0x5C]), Some('\\'));
        assert_eq!(decode_char(&[0x7E]), Some('~'));
    }

    #[test]
    fn rejects_non_single_char_input() {
        assert_eq!(decode_char(b""), None);
        assert_eq!(decode_char(b"AB"), None);
    }

    #[test]
    fn epilogue_text_decodes() {
        // 「エピローグ。」(CP932)
        let bytes = [
            0x83, 0x47, 0x83, 0x73, 0x83, 0x8D, 0x81, 0x5B, 0x83, 0x4F, 0x81, 0x42,
        ];
        let d = decode(&bytes);
        assert_eq!(d.chars, vec!['エ', 'ピ', 'ロ', 'ー', 'グ', '。']);
        assert_eq!(d.invalid, 0);
    }
}
