//! The multi-encoding string engine.
//!
//! A Ruby `String` is *bytes plus an interpretation*: the same three bytes
//! `[0xE2, 0x82, 0xAC]` are the single character `€` under UTF-8 but three
//! separate bytes under ASCII-8BIT. So a [`StrBuf`] stores the raw `bytes`
//! and an [`EncodingId`] naming how to read them -- exactly CRuby's own
//! `RString` + `rb_encoding` split.
//!
//! Everything an encoding needs to know lives in one [`EncodingSpec`] row of
//! the static [`ENCODINGS`] table; adding a new encoding (Windows-1252,
//! Shift_JIS, ...) is a new row, never a structural change. The four rows
//! here -- UTF-8, US-ASCII, ASCII-8BIT (a.k.a. BINARY), and ISO-8859-1
//! (Latin-1) -- already exercise every distinct behavior the engine models:
//! a variable-width encoding, a strict 7-bit one, a raw binary one, and a
//! fixed 1-byte one whose high bytes ARE valid characters.

use std::borrow::Cow;
use std::cell::Cell;

use crate::Signal;

/// An index into [`ENCODINGS`]. `Copy` and one byte wide, so every string
/// carries its encoding for free.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EncodingId(pub u8);

pub const UTF_8: EncodingId = EncodingId(0);
pub const US_ASCII: EncodingId = EncodingId(1);
pub const ASCII_8BIT: EncodingId = EncodingId(2);
pub const ISO_8859_1: EncodingId = EncodingId(3);

/// How an encoding maps bytes to characters -- the single knob that drives
/// character iteration, validation, and transcoding. A new encoding picks
/// the family it belongs to (or, for something genuinely new like a
/// stateful multibyte encoding, a new variant is added here).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncKind {
    /// UTF-8: 1..4 bytes per character, self-synchronizing, validated.
    Utf8,
    /// Strict 7-bit ASCII: one byte per character, every byte `>= 0x80` is
    /// an INVALID sequence (US-ASCII).
    Ascii,
    /// One byte per character, byte value == Unicode codepoint (`0..=255`),
    /// so every byte sequence is valid (ISO-8859-1 / Latin-1).
    Latin1,
    /// Raw bytes: one "character" per byte, never invalid, but bytes
    /// `>= 0x80` have no character meaning to convert FROM (ASCII-8BIT).
    Binary,
}

/// One encoding's declarative description -- the whole per-encoding surface.
pub struct EncodingSpec {
    /// The canonical name (`Encoding#name`): `"UTF-8"`, `"ASCII-8BIT"`.
    pub name: &'static str,
    /// Alternate names (`Encoding#names` is `[name, *aliases]`); `find`
    /// matches these case-insensitively too.
    pub aliases: &'static [&'static str],
    /// Whether the first 128 codepoints coincide with ASCII (true for all
    /// but genuinely non-ASCII encodings) -- governs `Encoding.compatible?`.
    pub ascii_compatible: bool,
    pub kind: EncKind,
}

/// The encoding registry. Extend by appending a row -- ids are the row
/// index, so existing ids never shift.
pub static ENCODINGS: &[EncodingSpec] = &[
    EncodingSpec {
        name: "UTF-8",
        aliases: &["CP65001", "locale", "external", "filesystem"],
        ascii_compatible: true,
        kind: EncKind::Utf8,
    },
    EncodingSpec {
        name: "US-ASCII",
        aliases: &["ASCII", "ANSI_X3.4-1968", "646"],
        ascii_compatible: true,
        kind: EncKind::Ascii,
    },
    EncodingSpec {
        name: "ASCII-8BIT",
        aliases: &["BINARY"],
        ascii_compatible: true,
        kind: EncKind::Binary,
    },
    EncodingSpec {
        name: "ISO-8859-1",
        aliases: &["ISO8859-1", "Latin-1"],
        ascii_compatible: true,
        kind: EncKind::Latin1,
    },
];

impl EncodingId {
    pub fn spec(self) -> &'static EncodingSpec {
        &ENCODINGS[self.0 as usize]
    }
    pub fn name(self) -> &'static str {
        self.spec().name
    }
    /// The `Encoding#inspect` display name. Normally the canonical name, but
    /// ASCII-8BIT renders as `BINARY (ASCII-8BIT)` -- CRuby's own quirk, the
    /// encoding having been half-renamed to BINARY. (CRuby also tags
    /// not-yet-used encodings `(autoload)`; this engine loads every encoding
    /// eagerly, so it never shows that lazy-loading artifact.)
    pub fn inspect_name(self) -> String {
        if self == ASCII_8BIT {
            "BINARY (ASCII-8BIT)".to_string()
        } else {
            self.name().to_string()
        }
    }
    pub fn kind(self) -> EncKind {
        self.spec().kind
    }
    pub fn ascii_compatible(self) -> bool {
        self.spec().ascii_compatible
    }
    /// `Encoding#names`: the canonical name followed by every alias, minus
    /// the special `default_*` selector aliases (which name no real
    /// encoding of their own).
    pub fn names(self) -> Vec<&'static str> {
        let spec = self.spec();
        std::iter::once(spec.name)
            .chain(
                spec.aliases
                    .iter()
                    .copied()
                    .filter(|a| !SELECTOR_ALIASES.contains(a)),
            )
            .collect()
    }
}

/// Aliases that are runtime SELECTORS (`Encoding.find("external")`), not
/// real alternate names -- excluded from `Encoding#names`.
const SELECTOR_ALIASES: &[&str] = &["locale", "external", "filesystem", "internal"];

/// Every real encoding id, in table order -- backs `Encoding.list`.
pub fn all() -> impl Iterator<Item = EncodingId> {
    (0..ENCODINGS.len() as u8).map(EncodingId)
}

/// Resolves a name or alias to its encoding, case-insensitively (CRuby
/// matches `"utf-8"`, `"UTF-8"`, `"Utf_8"` alike -- `-` and `_` are
/// equivalent). `None` for an unknown name.
pub fn find(name: &str) -> Option<EncodingId> {
    let want = normalize_name(name);
    all().find(|id| {
        let s = id.spec();
        std::iter::once(s.name)
            .chain(s.aliases.iter().copied())
            .any(|n| normalize_name(n) == want)
    })
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

// ---------------------------------------------------------------------------
// CodeRange: the cached "how ASCII/valid is this string" classification.
// ---------------------------------------------------------------------------

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
fn compute_coderange(bytes: &[u8], enc: EncodingId) -> CodeRange {
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
        // Binary and Latin-1 accept every byte.
        EncKind::Latin1 | EncKind::Binary => CodeRange::Valid,
    }
}

// ---------------------------------------------------------------------------
// StrBuf: the string payload.
// ---------------------------------------------------------------------------

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

    /// The zero-copy UTF-8 view -- the migration bridge for the countless
    /// operations that want a `&str`. Cheap for the common case (UTF-8 or
    /// ASCII content); raises Ruby's `Encoding::CompatibilityError` when the
    /// bytes can't be read as UTF-8 (a high-byte Latin-1/BINARY string, a
    /// broken sequence), matching CRuby's "you can't do that char operation
    /// on those bytes" behavior.
    pub fn as_utf8(&self) -> Result<&str, Signal> {
        std::str::from_utf8(&self.bytes).map_err(|_| {
            crate::dispatch::raise_error(
                "Encoding::CompatibilityError",
                format!(
                    "incompatible character encodings: {} and UTF-8",
                    self.enc.name()
                ),
            )
        })
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

/// `String#inspect`: the double-quoted, escaped rendering. Printable
/// characters keep Rust's own debug escaping (which the runtime already
/// relied on); a byte that isn't a character in this string's encoding --
/// broken UTF-8, or a high byte of a non-Unicode encoding -- becomes
/// `\xNN`, exactly as CRuby shows it. For an all-UTF-8, all-valid string
/// this is byte-for-byte the old `format!("{:?}", s)`.
pub fn inspect(buf: &StrBuf) -> String {
    let mut out = String::from("\"");
    match buf.enc.kind() {
        EncKind::Utf8 => {
            let units = decode_utf8(&buf.bytes);
            for i in 0..units.len() {
                match &units[i] {
                    Unit::Char(c) => {
                        // `#` is escaped only before an interpolation sigil,
                        // so the next unit's character is needed.
                        let next = match units.get(i + 1) {
                            Some(Unit::Char(nc)) => Some(*nc),
                            _ => None,
                        };
                        push_inspect_char(&mut out, *c, next, true);
                    }
                    Unit::Invalid(bytes) => {
                        for b in bytes {
                            out.push_str(&format!("\\x{b:02X}"));
                        }
                    }
                }
            }
        }
        // A non-Unicode encoding: ASCII bytes escape as usual, high bytes as
        // `\xNN` (CRuby prints Latin-1 `0xE9` as `\xE9`, not as `é`).
        EncKind::Ascii | EncKind::Latin1 | EncKind::Binary => {
            let bytes = &buf.bytes;
            for i in 0..bytes.len() {
                let b = bytes[i];
                if b < 0x80 {
                    let next = bytes
                        .get(i + 1)
                        .and_then(|&nb| (nb < 0x80).then_some(nb as char));
                    push_inspect_char(&mut out, b as char, next, false);
                } else {
                    out.push_str(&format!("\\x{b:02X}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// Escapes one character for `String#inspect` (CRuby `rb_str_inspect`): the
/// named control escapes, `\uXXXX`/`\xXX` for other control bytes (Unicode vs
/// byte encoding), `\#` before an interpolation sigil, and every other
/// character verbatim. Non-ASCII characters print literally, matching CRuby
/// for the printable-character common case (a documented simplification: the
/// rare non-printable Unicode formats/separators CRuby would escape are not
/// distinguished here).
fn push_inspect_char(out: &mut String, c: char, next: Option<char>, is_utf8: bool) {
    let cp = c as u32;
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\u{07}' => out.push_str("\\a"),
        '\u{08}' => out.push_str("\\b"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\u{0B}' => out.push_str("\\v"),
        '\u{0C}' => out.push_str("\\f"),
        '\r' => out.push_str("\\r"),
        '\u{1B}' => out.push_str("\\e"),
        '#' => {
            if matches!(next, Some('{' | '$' | '@')) {
                out.push('\\');
            }
            out.push('#');
        }
        _ if cp < 0x20 || cp == 0x7F => {
            if is_utf8 {
                out.push_str(&format!("\\u{cp:04X}"));
            } else {
                out.push_str(&format!("\\x{cp:02X}"));
            }
        }
        _ => out.push(c),
    }
}

// ---------------------------------------------------------------------------
// Transcoding (String#encode).
// ---------------------------------------------------------------------------

/// The `encode` keyword options, already extracted from the Ruby hash.
/// `:fallback` is handled by the caller (it may invoke arbitrary Ruby), so
/// it arrives as a closure rather than living here.
#[derive(Default)]
pub struct TranscodeOptions {
    /// `:invalid => :replace` -- swallow malformed SOURCE bytes.
    pub invalid_replace: bool,
    /// `:undef => :replace` -- swallow characters absent from the TARGET.
    pub undef_replace: bool,
    /// `:replace => "..."` -- the replacement text (default per encoding).
    pub replace: Option<String>,
    pub xml: Option<XmlMode>,
    pub newline: Option<NewlineMode>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum XmlMode {
    Text,
    Attr,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NewlineMode {
    Cr,
    Crlf,
    Universal,
}

/// The two ways a transcode can refuse a unit, matching CRuby's two error
/// classes.
#[derive(Debug)]
pub enum TranscodeError {
    /// A malformed byte sequence in the SOURCE encoding.
    InvalidByteSequence(String),
    /// A valid source character with no representation in the TARGET.
    UndefinedConversion(String),
}

impl TranscodeError {
    pub fn into_signal(self) -> Signal {
        match self {
            TranscodeError::InvalidByteSequence(m) => {
                crate::dispatch::raise_error("Encoding::InvalidByteSequenceError", m)
            }
            TranscodeError::UndefinedConversion(m) => {
                crate::dispatch::raise_error("Encoding::UndefinedConversionError", m)
            }
        }
    }
}

/// One decoded source unit on the way from `from` to `to`.
enum Unit {
    /// A decoded Unicode scalar.
    Char(char),
    /// Bytes the SOURCE encoding couldn't decode (`:invalid` territory).
    Invalid(Vec<u8>),
}

/// Decodes `bytes` under `from` into a sequence of units.
fn decode(bytes: &[u8], from: EncodingId) -> Vec<Unit> {
    match from.kind() {
        EncKind::Latin1 => bytes.iter().map(|b| Unit::Char(*b as char)).collect(),
        EncKind::Ascii | EncKind::Binary => bytes
            .iter()
            .map(|b| {
                if *b < 0x80 {
                    Unit::Char(*b as char)
                } else {
                    Unit::Invalid(vec![*b])
                }
            })
            .collect(),
        EncKind::Utf8 => decode_utf8(bytes),
    }
}

fn decode_utf8(bytes: &[u8]) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                units.extend(s.chars().map(Unit::Char));
                break;
            }
            Err(e) => {
                let good = e.valid_up_to();
                // SAFETY: `good` is a validated UTF-8 boundary.
                let valid = unsafe { std::str::from_utf8_unchecked(&rest[..good]) };
                units.extend(valid.chars().map(Unit::Char));
                let bad_len = e.error_len().unwrap_or(rest.len() - good).max(1);
                units.push(Unit::Invalid(rest[good..good + bad_len].to_vec()));
                rest = &rest[good + bad_len..];
            }
        }
    }
    units
}

/// Encodes one scalar into `to`'s bytes, or `None` if `to` can't represent
/// it (an undefined conversion).
fn encode_char(c: char, to: EncodingId) -> Option<Vec<u8>> {
    let cp = c as u32;
    match to.kind() {
        EncKind::Utf8 => Some(c.to_string().into_bytes()),
        EncKind::Ascii => (cp < 0x80).then(|| vec![cp as u8]),
        EncKind::Latin1 => (cp < 0x100).then(|| vec![cp as u8]),
        // Binary accepts any ASCII char verbatim; a non-ASCII scalar has no
        // binary byte (it isn't a byte).
        EncKind::Binary => (cp < 0x80).then(|| vec![cp as u8]),
    }
}

/// The replacement string for undefined/invalid units: the caller's
/// `:replace`, else U+FFFD for a Unicode target and `"?"` otherwise.
fn replacement(opts: &TranscodeOptions, to: EncodingId) -> Vec<u8> {
    if let Some(r) = &opts.replace {
        // The replacement is given as text; render it into the target.
        return r
            .chars()
            .flat_map(|c| encode_char(c, to).unwrap_or_default())
            .collect();
    }
    match to.kind() {
        EncKind::Utf8 => "\u{FFFD}".as_bytes().to_vec(),
        _ => b"?".to_vec(),
    }
}

/// A `:fallback` handler: given an undefined character's UTF-8 text, answers
/// replacement text, or `None` to fall through to the error/replace path.
/// `&mut dyn FnMut` because the handler may invoke arbitrary Ruby (a Proc).
pub type TranscodeFallback<'a> = &'a mut dyn FnMut(&str) -> Option<String>;

/// Transcodes `bytes` from `from` to `to`, applying `opts`. `fallback` (if
/// any) is consulted for otherwise-undefined characters BEFORE the error/
/// replace path -- it returns replacement text or `None` to fall through.
pub fn transcode(
    bytes: &[u8],
    from: EncodingId,
    to: EncodingId,
    opts: &TranscodeOptions,
    fallback: Option<TranscodeFallback<'_>>,
) -> Result<Vec<u8>, TranscodeError> {
    let mut fallback = fallback;
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    for unit in decode(bytes, from) {
        match unit {
            Unit::Invalid(raw) => {
                if opts.invalid_replace {
                    out.extend_from_slice(&replacement(opts, to));
                } else {
                    return Err(TranscodeError::InvalidByteSequence(format!(
                        "\"{}\" on {}",
                        raw.iter()
                            .map(|b| format!("\\x{b:02X}"))
                            .collect::<String>(),
                        from.name()
                    )));
                }
            }
            Unit::Char(c) => {
                if let Some(bytes) = encode_char(c, to) {
                    apply_xml(&mut out, c, &bytes, opts, to);
                    if opts.xml.is_none() {
                        maybe_newline(&mut out, c, &bytes, opts);
                    }
                    continue;
                }
                // Undefined in the target: fallback, then :undef, then error.
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                if let Some(f) = fallback.as_deref_mut() {
                    if let Some(rep) = f(s) {
                        out.extend(
                            rep.chars()
                                .flat_map(|c| encode_char(c, to).unwrap_or_default()),
                        );
                        continue;
                    }
                }
                if let Some(xml) = opts.xml {
                    push_xml_ref(&mut out, c, xml);
                } else if opts.undef_replace {
                    out.extend_from_slice(&replacement(opts, to));
                } else {
                    return Err(TranscodeError::UndefinedConversion(format!(
                        "U+{:04X} from {} to {}",
                        c as u32,
                        from.name(),
                        to.name()
                    )));
                }
            }
        }
    }
    Ok(out)
}

/// Applies the `:xml` escaping to a REPRESENTABLE character, or emits its
/// bytes verbatim. Undefined characters are handled by `push_xml_ref`.
fn apply_xml(out: &mut Vec<u8>, c: char, bytes: &[u8], opts: &TranscodeOptions, _to: EncodingId) {
    match opts.xml {
        None => out.extend_from_slice(bytes),
        Some(mode) => {
            let escaped = match c {
                '&' => Some("&amp;"),
                '<' => Some("&lt;"),
                '>' => Some("&gt;"),
                '"' if mode == XmlMode::Attr => Some("&quot;"),
                _ => None,
            };
            match escaped {
                Some(e) => out.extend_from_slice(e.as_bytes()),
                None => out.extend_from_slice(bytes),
            }
        }
    }
}

/// A numeric character reference for an undefined char under `:xml`.
fn push_xml_ref(out: &mut Vec<u8>, c: char, _mode: XmlMode) {
    out.extend_from_slice(format!("&#x{:X};", c as u32).as_bytes());
}

/// Rewrites a just-emitted `\n` per the newline option.
fn maybe_newline(out: &mut Vec<u8>, c: char, bytes: &[u8], opts: &TranscodeOptions) {
    if c != '\n' {
        return;
    }
    if let Some(mode) = opts.newline {
        // Undo the `\n` just pushed by the caller and replace it.
        out.truncate(out.len() - bytes.len());
        match mode {
            NewlineMode::Cr => out.push(b'\r'),
            NewlineMode::Crlf => out.extend_from_slice(b"\r\n"),
            NewlineMode::Universal => out.push(b'\n'),
        }
    }
}

/// The byte length a UTF-8 sequence claims from its leading byte (1 for an
/// ASCII or continuation/invalid byte, else 2..4).
fn utf8_seq_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

#[derive(Clone, Copy)]
enum CaseMode {
    Up,
    Down,
    Swap,
    Cap,
}

/// ASCII-only case fold (US-ASCII / ASCII-8BIT): only `a-z`/`A-Z` flip.
fn ascii_case_byte(b: u8, up: bool) -> u8 {
    if up && b.is_ascii_lowercase() {
        b - 32
    } else if !up && b.is_ascii_uppercase() {
        b + 32
    } else {
        b
    }
}

/// Latin-1 case fold: ASCII plus the `À`-`Þ` / `à`-`þ` accented-letter ranges
/// (skipping the `×`/`÷` math signs at 0xD7/0xF7).
fn latin1_case_byte(b: u8, up: bool) -> u8 {
    if up {
        if b.is_ascii_lowercase() || ((0xE0..=0xFE).contains(&b) && b != 0xF7) {
            b - 32
        } else {
            b
        }
    } else if b.is_ascii_uppercase() || ((0xC0..=0xDE).contains(&b) && b != 0xD7) {
        b + 32
    } else {
        b
    }
}

fn case_bytes(bytes: &[u8], mode: CaseMode, fold: fn(u8, bool) -> u8) -> Vec<u8> {
    match mode {
        CaseMode::Up => bytes.iter().map(|b| fold(*b, true)).collect(),
        CaseMode::Down => bytes.iter().map(|b| fold(*b, false)).collect(),
        CaseMode::Swap => bytes
            .iter()
            .map(|b| {
                let up = fold(*b, true);
                if up != *b { up } else { fold(*b, false) }
            })
            .collect(),
        CaseMode::Cap => bytes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if i == 0 {
                    fold(*b, true)
                } else {
                    fold(*b, false)
                }
            })
            .collect(),
    }
}

fn case_unicode(s: &str, mode: CaseMode) -> String {
    match mode {
        CaseMode::Up => s.to_uppercase(),
        CaseMode::Down => s.to_lowercase(),
        CaseMode::Swap => s
            .chars()
            .flat_map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().collect::<Vec<_>>()
                } else {
                    c.to_uppercase().collect()
                }
            })
            .collect(),
        CaseMode::Cap => {
            let mut chars = s.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Process default encodings.
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicU8, Ordering};

// `u8::MAX` sentinel = "unset" for the optional default_internal.
static DEFAULT_EXTERNAL: AtomicU8 = AtomicU8::new(UTF_8.0);
static DEFAULT_INTERNAL: AtomicU8 = AtomicU8::new(u8::MAX);

pub fn default_external() -> EncodingId {
    EncodingId(DEFAULT_EXTERNAL.load(Ordering::Relaxed))
}
pub fn set_default_external(enc: EncodingId) {
    DEFAULT_EXTERNAL.store(enc.0, Ordering::Relaxed);
}
pub fn default_internal() -> Option<EncodingId> {
    match DEFAULT_INTERNAL.load(Ordering::Relaxed) {
        u8::MAX => None,
        v => Some(EncodingId(v)),
    }
}
pub fn set_default_internal(enc: Option<EncodingId>) {
    DEFAULT_INTERNAL.store(enc.map_or(u8::MAX, |e| e.0), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_is_case_and_separator_insensitive() {
        assert_eq!(find("UTF-8"), Some(UTF_8));
        assert_eq!(find("utf_8"), Some(UTF_8));
        assert_eq!(find("BINARY"), Some(ASCII_8BIT));
        assert_eq!(find("ascii-8bit"), Some(ASCII_8BIT));
        assert_eq!(find("Latin-1"), Some(ISO_8859_1));
        assert_eq!(find("nope"), None);
    }

    #[test]
    fn names_lists_canonical_then_real_aliases() {
        assert_eq!(ASCII_8BIT.names(), vec!["ASCII-8BIT", "BINARY"]);
        // UTF-8's selector aliases (external/locale/...) are filtered out.
        assert_eq!(UTF_8.names(), vec!["UTF-8", "CP65001"]);
    }

    #[test]
    fn coderange_classifies_per_encoding() {
        assert_eq!(
            StrBuf::from_utf8("abc".into()).coderange(),
            CodeRange::SevenBit
        );
        assert_eq!(
            StrBuf::from_utf8("caf\u{e9}".into()).coderange(),
            CodeRange::Valid
        );
        // A lone 0xE9 is broken UTF-8 but a valid Latin-1 character.
        assert_eq!(
            StrBuf::from_bytes(vec![0xE9], UTF_8).coderange(),
            CodeRange::Broken
        );
        assert_eq!(
            StrBuf::from_bytes(vec![0xE9], ISO_8859_1).coderange(),
            CodeRange::Valid
        );
        // A high byte is never valid US-ASCII.
        assert_eq!(
            StrBuf::from_bytes(vec![0xE9], US_ASCII).coderange(),
            CodeRange::Broken
        );
    }

    #[test]
    fn ascii_only_and_valid_encoding() {
        let s = StrBuf::from_utf8("abc".into());
        assert!(s.ascii_only() && s.valid_encoding());
        let s = StrBuf::from_utf8("caf\u{e9}".into());
        assert!(!s.ascii_only() && s.valid_encoding());
        let s = StrBuf::from_bytes(vec![0xC2], UTF_8); // truncated 2-byte seq
        assert!(!s.valid_encoding());
    }

    #[test]
    fn latin1_high_byte_renders_as_its_codepoint() {
        // 0xE9 in Latin-1 is 'é' (U+00E9).
        let s = StrBuf::from_bytes(vec![0x63, 0x61, 0x66, 0xE9], ISO_8859_1);
        assert_eq!(s.to_utf8_lossy(), "caf\u{e9}");
        assert_eq!(s.char_len(), 4);
    }

    #[test]
    fn force_encoding_keeps_bytes_reinterprets() {
        let mut s = StrBuf::from_bytes(vec![0xE9], ISO_8859_1);
        assert_eq!(s.to_utf8_lossy(), "\u{e9}");
        s.set_encoding(UTF_8);
        // Same byte, now broken UTF-8.
        assert!(!s.valid_encoding());
    }

    #[test]
    fn char_indexing_is_encoding_aware() {
        // UTF-8: a multibyte char is one index.
        let utf = StrBuf::from_utf8("caf\u{e9}".into());
        assert_eq!(utf.char_len(), 4);
        assert_eq!(utf.char_at(3).unwrap().bytes(), "\u{e9}".as_bytes());
        // BINARY: one character per byte, kept BINARY.
        let bin = StrBuf::from_bytes("caf\u{e9}".to_string().into_bytes(), ASCII_8BIT);
        assert_eq!(bin.char_len(), 5);
        assert_eq!(bin.char_at(3).unwrap().bytes(), &[0xC3]);
        assert_eq!(bin.char_at(3).unwrap().encoding(), ASCII_8BIT);
        assert_eq!(bin.reversed().bytes(), &[0xA9, 0xC3, 0x66, 0x61, 0x63]);
        assert_eq!(bin.char_substr(1, 2).unwrap().bytes(), &[0x61, 0x66]);
    }

    #[test]
    fn casing_is_encoding_aware() {
        assert_eq!(
            StrBuf::from_utf8("caf\u{e9}".into()).upcased().bytes(),
            "CAF\u{c9}".as_bytes()
        );
        // BINARY: only ASCII bytes fold; high bytes are untouched.
        let bin = StrBuf::from_bytes(vec![0x63, 0x61, 0x66, 0xC3, 0x89], ASCII_8BIT);
        assert_eq!(bin.upcased().bytes(), &[0x43, 0x41, 0x46, 0xC3, 0x89]);
        // Latin-1: é (0xE9) uppercases to É (0xC9).
        let latin = StrBuf::from_bytes(vec![0x63, 0x61, 0x66, 0xE9], ISO_8859_1);
        assert_eq!(latin.upcased().bytes(), &[0x43, 0x41, 0x46, 0xC9]);
        assert_eq!(latin.upcased().encoding(), ISO_8859_1);
    }

    #[test]
    fn transcode_latin1_to_utf8_and_back() {
        let opts = TranscodeOptions::default();
        // Latin-1 "café" -> UTF-8 bytes for é (0xC3 0xA9).
        let utf8 = transcode(&[0x63, 0x61, 0x66, 0xE9], ISO_8859_1, UTF_8, &opts, None).unwrap();
        assert_eq!(utf8, "caf\u{e9}".as_bytes());
        // Round trip back.
        let latin = transcode(&utf8, UTF_8, ISO_8859_1, &opts, None).unwrap();
        assert_eq!(latin, vec![0x63, 0x61, 0x66, 0xE9]);
    }

    #[test]
    fn transcode_undefined_raises_or_replaces() {
        // é has no US-ASCII byte.
        let strict = TranscodeOptions::default();
        assert!(matches!(
            transcode("caf\u{e9}".as_bytes(), UTF_8, US_ASCII, &strict, None),
            Err(TranscodeError::UndefinedConversion(_))
        ));
        let replace = TranscodeOptions {
            undef_replace: true,
            ..Default::default()
        };
        let out = transcode("caf\u{e9}".as_bytes(), UTF_8, US_ASCII, &replace, None).unwrap();
        assert_eq!(out, b"caf?");
    }

    #[test]
    fn transcode_xml_text_escapes() {
        let opts = TranscodeOptions {
            xml: Some(XmlMode::Text),
            ..Default::default()
        };
        let out = transcode(b"a<b>&c", UTF_8, US_ASCII, &opts, None).unwrap();
        assert_eq!(out, b"a&lt;b&gt;&amp;c");
    }

    #[test]
    fn inspect_escapes_invalid_and_high_bytes() {
        // Valid UTF-8: byte-identical to the old Debug quoting.
        assert_eq!(inspect(&StrBuf::from_utf8("a\nb".into())), r#""a\nb""#);
        assert_eq!(
            inspect(&StrBuf::from_utf8("caf\u{e9}".into())),
            "\"caf\u{e9}\""
        );
        // A broken UTF-8 byte renders as \xNN.
        assert_eq!(
            inspect(&StrBuf::from_bytes(vec![0x61, 0xE9, 0x62], UTF_8)),
            r#""a\xE9b""#
        );
        // A Latin-1 high byte renders as \xNN, not as its codepoint char.
        assert_eq!(
            inspect(&StrBuf::from_bytes(
                vec![0x63, 0x61, 0x66, 0xE9],
                ISO_8859_1
            )),
            r#""caf\xE9""#
        );
    }

    #[test]
    fn cross_encoding_equality_and_hash_tag() {
        // ASCII-only strings are equal (and one hash key) across encodings.
        let ascii_utf8 = StrBuf::from_utf8("abc".into());
        let ascii_latin = StrBuf::from_bytes(b"abc".to_vec(), ISO_8859_1);
        assert_eq!(ascii_utf8, ascii_latin);
        assert_eq!(ascii_utf8.hash_key_tag(), ascii_latin.hash_key_tag());
        // High-byte strings with the same bytes but different encodings are
        // distinct.
        let hi_utf8 = StrBuf::from_utf8("caf\u{e9}".into());
        let hi_latin = StrBuf::from_bytes("caf\u{e9}".to_string().into_bytes(), ISO_8859_1);
        assert_ne!(hi_utf8, hi_latin);
        assert_ne!(hi_utf8.hash_key_tag(), hi_latin.hash_key_tag());
    }

    #[test]
    fn transcode_fallback_consulted_first() {
        let opts = TranscodeOptions::default();
        let mut fb = |s: &str| (s == "\u{e9}").then(|| "e".to_string());
        let out = transcode(
            "caf\u{e9}".as_bytes(),
            UTF_8,
            US_ASCII,
            &opts,
            Some(&mut fb),
        )
        .unwrap();
        assert_eq!(out, b"cafe");
    }
}
