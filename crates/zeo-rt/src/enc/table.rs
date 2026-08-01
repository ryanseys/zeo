//! The encoding registry's TYPES and lookups. The rows themselves live in
//! [`crate::enc::registry`], generated from the ruby 4.0.6 oracle, and are
//! re-exported here so callers keep reading `table::UTF_8` /
//! `table::ENCODINGS`.

use crate::enc::single_byte::SingleByteTable;

pub use crate::enc::registry::*;

/// An index into [`ENCODINGS`]. `Copy` and one byte wide, so every string
/// carries its encoding for free.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EncodingId(pub u8);

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
    /// One byte per character through a per-encoding mapping table
    /// (`EncodingSpec::table`): every byte is a valid CHARACTER, but a byte
    /// whose table slot is unmapped has no Unicode meaning and refuses to
    /// transcode OUT (the windows-125x vendor pages' unassigned slots).
    SingleByte,
    /// A multibyte CJK encoding: 1-3 bytes per character, structural walk
    /// per family, Unicode mapping via encoding_rs -- see `mb`.
    MultiByte(MbFamily),
    /// UTF-16 (2-byte code units, surrogate pairs) -- NOT ASCII-compatible.
    Utf16 { be: bool },
    /// UTF-32 (4-byte scalars) -- NOT ASCII-compatible.
    Utf32 { be: bool },
    /// Registered by NAME only: the row answers every reflection question
    /// (`#name`, `#names`, `#dummy?`, `#ascii_compatible?`, the constant, a
    /// place in `Encoding.list`) but this runtime has no byte<->character
    /// mapping for it. Bytes read one per character, as for `Binary`, and a
    /// conversion of anything but ASCII raises
    /// `Encoding::ConverterNotFoundError` rather than guessing.
    Registered,
}

use crate::enc::mb::MbFamily;

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
    /// The mapping table for `EncKind::SingleByte` rows; `None` for every
    /// kind that needs no table.
    pub table: Option<&'static SingleByteTable>,
    /// CRuby's `Encoding#dummy?`: the encoding exists as a NAME (strings can
    /// be tagged with it, `#inspect` shows ` (dummy)`) but has no per-
    /// character structure -- a "character" is one byte and nothing is ever
    /// invalid, which is why the dummy rows reuse `EncKind::Binary`. What a
    /// dummy CAN still do is transcode: `transcode` special-cases three ids
    /// (stateful ISO-2022-JP escapes, BOM-carrying UTF-16/32).
    pub dummy: bool,
}

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
        } else if self.is_dummy() {
            format!("{} (dummy)", self.name())
        } else {
            self.name().to_string()
        }
    }
    /// `Encoding#dummy?` -- see [`EncodingSpec::dummy`].
    pub fn is_dummy(self) -> bool {
        self.spec().dummy
    }
    pub fn kind(self) -> EncKind {
        self.spec().kind
    }
    /// Whether this runtime carries a byte<->character mapping for the
    /// encoding, or only its name -- see [`EncKind::Registered`].
    pub fn is_registered_only(self) -> bool {
        self.kind() == EncKind::Registered
    }
    /// The mapping table of an `EncKind::SingleByte` row. Panics for other
    /// kinds -- every caller has already matched on the kind.
    pub(crate) fn single_byte_table(self) -> &'static SingleByteTable {
        self.spec()
            .table
            .expect("SingleByte rows always carry a table")
    }
    pub fn ascii_compatible(self) -> bool {
        self.spec().ascii_compatible
    }
    /// The canonical name plus every TABLE alias -- the static half of
    /// `#names`, and the half that spells the `Encoding::*` constants.
    pub fn spec_names(self) -> Vec<&'static str> {
        let spec = self.spec();
        std::iter::once(spec.name)
            .chain(spec.aliases.iter().copied())
            .collect()
    }
    /// `Encoding#names`: the canonical name, every alias, and the runtime
    /// SELECTOR names this encoding currently answers to. CRuby moves
    /// `locale`/`external`/`filesystem` and `internal` onto whichever row
    /// `Encoding.default_external`/`.default_internal` names, so they are
    /// computed here rather than being table data.
    pub fn names(self) -> Vec<&'static str> {
        let mut names = self.spec_names();
        if self == crate::enc::defaults::default_external() {
            names.extend(["locale", "external", "filesystem"]);
        }
        if Some(self) == crate::enc::defaults::default_internal() {
            names.push("internal");
        }
        names
    }
}

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
