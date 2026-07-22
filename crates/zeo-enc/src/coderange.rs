//! CRuby's coderange cache -- the "how ASCII/valid is this string"
//! classification that unlocks byte == char fast paths.

use crate::table::{EncKind, EncodingId};

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
    let all_ascii = bytes.iter().all(|b| *b < 0x80);
    if all_ascii {
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
        EncKind::Latin1 | EncKind::Binary | EncKind::SingleByte => CodeRange::Valid,
    }
}
