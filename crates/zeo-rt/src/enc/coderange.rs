//! CRuby's coderange cache -- the "how ASCII/valid is this string"
//! classification that unlocks byte == char fast paths.

use crate::enc::table::{EncKind, EncodingId};

/// CRuby's coderange cache -- how the bytes sit relative to their encoding.
/// `SevenBit` is the overwhelmingly common case and unlocks byte == char
/// fast paths.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodeRange {
    /// Not yet computed (a fresh or just-mutated buffer).
    Unknown,
    /// Every byte is `< 0x80` -- pure ASCII, valid under every ascii-compatible
    /// encoding, and byte-indexable as characters directly.
    SevenBit,
    /// A valid multi-byte / high-byte sequence in this encoding.
    Valid,
    /// An invalid byte sequence for this encoding.
    Broken,
}

/// Classifies `bytes` under `enc` -- the definition of `SevenBit`/`Valid`/
/// `Broken` per encoding family.
pub(crate) fn compute_coderange(bytes: &[u8], enc: EncodingId) -> CodeRange {
    // `SevenBit` is only meaningful for ASCII-compatible encodings: a
    // UTF-16 string whose bytes all happen to sit below 0x80 is NOT
    // ASCII-usable (`"abc".force_encoding("UTF-16LE").ascii_only?` is
    // false in CRuby, empty strings included).
    if enc.ascii_compatible() && bytes.iter().all(|b| *b < 0x80) {
        return CodeRange::SevenBit;
    }
    match enc.kind() {
        EncKind::Utf8 => {
            if std::str::from_utf8(bytes).is_ok() {
                CodeRange::Valid
            } else {
                CodeRange::Broken
            }
        }
        // A high byte is never a valid US-ASCII character.
        EncKind::Ascii => CodeRange::Broken,
        // Binary, Latin-1, and the table-driven single-byte encodings accept
        // every byte as a character (a table slot being unmapped to Unicode
        // refuses TRANSCODING, not validity -- oracle-verified:
        // `"\x81".force_encoding("Windows-1252").valid_encoding?` is true).
        EncKind::Latin1 | EncKind::Binary | EncKind::Registered | EncKind::SingleByte => {
            CodeRange::Valid
        }
        // Multibyte: STRUCTURALLY valid sequences only (an unmapped pair is
        // still a character; a bad lead/trail or truncated lead is Broken).
        EncKind::MultiByte(family) => {
            if crate::enc::mb::mb_ranges(family, bytes)
                .iter()
                .all(|(_, v)| *v)
            {
                CodeRange::Valid
            } else {
                CodeRange::Broken
            }
        }
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::enc::wide::wide_of(enc.kind()).expect("wide kind");
            if crate::enc::wide::wide_ranges(w, bytes)
                .iter()
                .all(|(_, scalar, _)| scalar.is_some())
            {
                CodeRange::Valid
            } else {
                CodeRange::Broken
            }
        }
    }
}
