//! [`StrBuf`]: a Ruby string's payload -- raw bytes plus the encoding that
//! says how to read them, with the lazily-computed coderange cache.

use std::borrow::Cow;
use std::cell::Cell;

use crate::case::{CaseMode, ascii_case_byte, case_bytes, case_unicode, latin1_case_byte};
use crate::coderange::{CodeRange, compute_coderange};
use crate::table::{EncKind, EncodingId, UTF_8};
use crate::transcode::utf8_seq_len;

/// A Ruby string's contents: raw `bytes` interpreted through `enc`, with a
/// lazily-computed `coderange` cache. The `Cell` is sound because every
/// `StrBuf` lives behind the `Freezable` mutex (see `collections::RStr`), so
/// the interior mutation is never concurrent.
pub struct StrBuf {
    bytes: Vec<u8>,
    enc: EncodingId,
    coderange: Cell<CodeRange>,
}

impl StrBuf {
    /// The everyday constructor: a UTF-8 `String` (Ruby's default script
    /// encoding). Keeps `collections::string_new`'s signature unchanged so
    /// no codegen emission site has to move.
    pub fn from_utf8(s: String) -> StrBuf {
        StrBuf {
            bytes: s.into_bytes(),
            enc: UTF_8,
            coderange: Cell::new(CodeRange::Unknown),
        }
    }

    /// Raw bytes tagged with an explicit encoding -- how `String#b`,
    /// `force_encoding`, IO reads, and `\xNN`-bearing literals build strings.
    pub fn from_bytes(bytes: Vec<u8>, enc: EncodingId) -> StrBuf {
        StrBuf {
            bytes,
            enc,
            coderange: Cell::new(CodeRange::Unknown),
        }
    }

    pub fn encoding(&self) -> EncodingId {
        self.enc
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn bytesize(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn coderange(&self) -> CodeRange {
        if self.coderange.get() == CodeRange::Unknown {
            self.coderange.set(compute_coderange(&self.bytes, self.enc));
        }
        self.coderange.get()
    }

    pub fn ascii_only(&self) -> bool {
        self.coderange() == CodeRange::SevenBit
    }

    pub fn valid_encoding(&self) -> bool {
        self.coderange() != CodeRange::Broken
    }

    /// Retags the bytes without touching them (`String#force_encoding`) --
    /// only the interpretation changes, so the coderange is invalidated.
    pub fn set_encoding(&mut self, enc: EncodingId) {
        self.enc = enc;
        self.coderange.set(CodeRange::Unknown);
    }

    /// An always-succeeding UTF-8 rendering for display/`to_s`-style
    /// contexts. Honors the encoding: Latin-1 high bytes become their real
    /// codepoints (`0xE9` -> `é`); UTF-8/ASCII pass through; anything
    /// undecodable is replaced with U+FFFD.
    pub fn to_utf8_lossy(&self) -> Cow<'_, str> {
        match self.enc.kind() {
            EncKind::Utf8 | EncKind::Ascii => String::from_utf8_lossy(&self.bytes),
            EncKind::Latin1 | EncKind::Binary => {
                if self.ascii_only() {
                    // Reuse the borrow when it's plain ASCII.
                    String::from_utf8_lossy(&self.bytes)
                } else {
                    Cow::Owned(self.bytes.iter().map(|b| *b as char).collect())
                }
            }
        }
    }

    /// The characters of the string in order -- encoding-aware, mirroring
    /// `str::chars()` so `.chars().count()`/`.map()`/`.collect()` call sites
    /// keep working. Invalid sequences surface as U+FFFD so callers never
    /// panic.
    pub fn chars(&self) -> std::vec::IntoIter<char> {
        self.to_utf8_lossy().chars().collect::<Vec<_>>().into_iter()
    }

    /// The characters as an indexable `Vec` -- for random access and splice
    /// (`String#[]`/`[]=`), where an iterator won't do.
    pub fn char_vec(&self) -> Vec<char> {
        self.to_utf8_lossy().chars().collect()
    }

    /// The BYTE RANGE of each character, in order -- the encoding-correct
    /// basis for char indexing/slicing/reversing. UTF-8 characters span 1..4
    /// bytes (a malformed byte counts as a lone 1-byte character); every
    /// single-byte encoding is exactly one byte per character. Unlike
    /// `char_vec` (which loses a non-UTF-8 byte's identity by mapping it to a
    /// Unicode scalar), a range preserves the raw bytes so a substring stays
    /// in the SAME encoding.
    pub fn char_ranges(&self) -> Vec<std::ops::Range<usize>> {
        match self.enc.kind() {
            EncKind::Utf8 => {
                let mut ranges = Vec::new();
                let mut i = 0;
                while i < self.bytes.len() {
                    let want = utf8_seq_len(self.bytes[i]);
                    let end = (i + want).min(self.bytes.len());
                    if want > 1 && std::str::from_utf8(&self.bytes[i..end]).is_ok() {
                        ranges.push(i..end);
                        i = end;
                    } else {
                        // ASCII byte or a malformed sequence: one byte.
                        ranges.push(i..i + 1);
                        i += 1;
                    }
                }
                ranges
            }
            EncKind::Ascii | EncKind::Latin1 | EncKind::Binary => {
                (0..self.bytes.len()).map(|i| i..i + 1).collect()
            }
        }
    }

    /// The `i`-th character (negative counts from the end) as its OWN
    /// same-encoding string, or `None` when out of range.
    pub fn char_at(&self, i: i64) -> Option<StrBuf> {
        let ranges = self.char_ranges();
        let idx = if i < 0 { i + ranges.len() as i64 } else { i };
        let r = ranges.get(usize::try_from(idx).ok()?)?.clone();
        Some(StrBuf::from_bytes(self.bytes[r].to_vec(), self.enc))
    }

    /// The substring of `len` characters starting at char `start` (negative
    /// counts from the end), same encoding -- `None` when `start` is out of
    /// range. Clamps `len` to the end.
    pub fn char_substr(&self, start: i64, len: i64) -> Option<StrBuf> {
        let ranges = self.char_ranges();
        let n = ranges.len() as i64;
        let start = if start < 0 { start + n } else { start };
        if start < 0 || start > n || len < 0 {
            return None;
        }
        let end = (start + len).min(n);
        let bytes = if start == end {
            Vec::new()
        } else {
            self.bytes[ranges[start as usize].start..ranges[(end - 1) as usize].end].to_vec()
        };
        Some(StrBuf::from_bytes(bytes, self.enc))
    }

    /// The string reversed by CHARACTER (multibyte-safe), same encoding.
    pub fn reversed(&self) -> StrBuf {
        let mut bytes = Vec::with_capacity(self.bytes.len());
        for r in self.char_ranges().into_iter().rev() {
            bytes.extend_from_slice(&self.bytes[r]);
        }
        StrBuf::from_bytes(bytes, self.enc)
    }

    pub fn char_len(&self) -> usize {
        match self.enc.kind() {
            // One character per byte for every single-byte encoding.
            EncKind::Ascii | EncKind::Latin1 | EncKind::Binary => self.bytes.len(),
            EncKind::Utf8 if self.ascii_only() => self.bytes.len(),
            EncKind::Utf8 => self.char_ranges().len(),
        }
    }

    /// Uppercase / lowercase / swapcase / capitalize, per this string's
    /// encoding, returning a same-encoding string. UTF-8 gets full Unicode
    /// case mapping (identical to the pre-encoding behavior); the single-byte
    /// encodings fold ASCII always, plus Latin-1's own accented-letter range.
    pub fn upcased(&self) -> StrBuf {
        self.case_mapped(CaseMode::Up)
    }
    pub fn downcased(&self) -> StrBuf {
        self.case_mapped(CaseMode::Down)
    }
    pub fn swapcased(&self) -> StrBuf {
        self.case_mapped(CaseMode::Swap)
    }
    pub fn capitalized(&self) -> StrBuf {
        self.case_mapped(CaseMode::Cap)
    }

    fn case_mapped(&self, mode: CaseMode) -> StrBuf {
        match self.enc.kind() {
            EncKind::Utf8 => StrBuf::from_utf8(case_unicode(&self.to_utf8_lossy(), mode)),
            EncKind::Ascii | EncKind::Binary => {
                StrBuf::from_bytes(case_bytes(&self.bytes, mode, ascii_case_byte), self.enc)
            }
            EncKind::Latin1 => {
                StrBuf::from_bytes(case_bytes(&self.bytes, mode, latin1_case_byte), self.enc)
            }
        }
    }

    /// Appends `other`'s raw bytes under CRuby's concatenation-compatibility
    /// rule (see `compat_concat_enc`), adopting the negotiated encoding.
    /// `Err(IncompatibleEncodings)` is the incompatible case, receiver left
    /// untouched -- the caller raises `Encoding::CompatibilityError` (this
    /// crate keeps `Signal` out of the encoding layer).
    pub fn push_buf(&mut self, other: &StrBuf) -> Result<(), IncompatibleEncodings> {
        let enc = compat_concat_enc(self.enc, self.ascii_only(), other.enc, other.ascii_only())
            .ok_or(IncompatibleEncodings)?;
        self.bytes.extend_from_slice(&other.bytes);
        self.enc = enc;
        self.coderange.set(CodeRange::Unknown);
        Ok(())
    }

    /// Appends valid UTF-8 text; resets the coderange cache.
    pub fn push_str(&mut self, s: &str) {
        self.bytes.extend_from_slice(s.as_bytes());
        self.coderange.set(CodeRange::Unknown);
    }

    /// Appends raw bytes; resets the coderange cache.
    pub fn push_bytes(&mut self, b: &[u8]) {
        self.bytes.extend_from_slice(b);
        self.coderange.set(CodeRange::Unknown);
    }

    /// Replaces the contents with fresh UTF-8 text, keeping the current
    /// encoding tag (the common `*guard = new_string` mutation).
    pub fn replace_utf8(&mut self, s: String) {
        self.bytes = s.into_bytes();
        self.coderange.set(CodeRange::Unknown);
    }

    /// Replaces both the bytes and the encoding (`String#encode!`, IO
    /// re-tagging).
    pub fn replace_bytes(&mut self, bytes: Vec<u8>, enc: EncodingId) {
        self.bytes = bytes;
        self.enc = enc;
        self.coderange.set(CodeRange::Unknown);
    }

    /// `push_buf`'s decision as a dry run: the encoding a concatenation of
    /// `self` and `other` would produce, `None` for the incompatible pair.
    pub fn concat_enc_with(&self, other: &StrBuf) -> Option<EncodingId> {
        compat_concat_enc(self.enc, self.ascii_only(), other.enc, other.ascii_only())
    }

    /// The `(bytes, encoding-tag)` a Hash key / `eql?` comparison uses.
    /// Ruby's rule: two strings are the same key iff their bytes match AND
    /// they're encoding-compatible -- and an ASCII-only string is compatible
    /// with EVERY encoding. So an ascii-only string gets the shared tag `0`
    /// (encoding-agnostic), and any other string gets `enc + 1`, making
    /// `"abc"` one key across encodings while `"caf\xe9"` in UTF-8 vs
    /// Latin-1 are distinct keys.
    pub fn hash_key_tag(&self) -> u8 {
        if self.ascii_only() { 0 } else { self.enc.0 + 1 }
    }

    /// One byte, or `None` past the end (`String#getbyte`).
    pub fn getbyte(&self, i: usize) -> Option<u8> {
        self.bytes.get(i).copied()
    }

    /// Sets one byte in place (`String#setbyte`); resets the coderange.
    pub fn setbyte(&mut self, i: usize, b: u8) {
        self.bytes[i] = b;
        self.coderange.set(CodeRange::Unknown);
    }
}

impl Clone for StrBuf {
    fn clone(&self) -> StrBuf {
        StrBuf {
            bytes: self.bytes.clone(),
            enc: self.enc,
            coderange: Cell::new(self.coderange.get()),
        }
    }
}

/// CRuby's `rb_enc_compatible` for CONCATENATION, reduced to this engine's
/// encodings (every one is ASCII-compatible): equal encodings are trivially
/// compatible; a 7-bit (`ascii_only`) side adopts the OTHER side's encoding
/// -- checked right side first, so the LEFT side wins when both are 7-bit
/// (oracle-verified: `usascii + "y"` is US-ASCII, `"y" + usascii` is UTF-8,
/// `"abc" + binary_high` is BINARY); two differently-encoded non-7-bit
/// strings are incompatible -- `None`, the caller's
/// `Encoding::CompatibilityError`.
pub fn compat_concat_enc(
    left: EncodingId,
    left_ascii: bool,
    right: EncodingId,
    right_ascii: bool,
) -> Option<EncodingId> {
    if left == right || right_ascii {
        Some(left)
    } else if left_ascii {
        Some(right)
    } else {
        None
    }
}

/// `push_buf`'s refusal marker: the two buffers' encodings can't legally
/// concatenate (two differently-encoded non-7-bit strings). Carries no data
/// -- the caller re-reads both encodings for the error message.
#[derive(Debug)]
pub struct IncompatibleEncodings;

impl std::fmt::Debug for StrBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.to_utf8_lossy())
    }
}

/// `Display` renders the (lossy) text -- so `format!("{}", buf)` and the many
/// interpolation sites keep working through the swap.
impl std::fmt::Display for StrBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_utf8_lossy())
    }
}

/// String equality, CRuby's rule: same bytes AND encoding-compatible. Two
/// ASCII-only strings are compatible across encodings (both tag `0`), so
/// `"abc"` in UTF-8 equals `"abc"` in Latin-1; two high-byte strings match
/// only within the same encoding. For all-UTF-8 programs this reduces to
/// plain byte equality, so nothing observable changes from the pre-encoding
/// world.
impl PartialEq for StrBuf {
    fn eq(&self, other: &StrBuf) -> bool {
        self.bytes == other.bytes && self.hash_key_tag() == other.hash_key_tag()
    }
}
impl Eq for StrBuf {}
