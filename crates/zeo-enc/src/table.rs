//! The encoding registry: every encoding's declarative description in one
//! static [`ENCODINGS`] table, addressed by the one-byte [`EncodingId`]
//! every string carries. Adding an encoding is a new row (plus, for a
//! genuinely new byte<->character mapping family, an [`EncKind`] variant).

use crate::mb::MbFamily;
use crate::single_byte::{self, SingleByteTable};

/// An index into [`ENCODINGS`]. `Copy` and one byte wide, so every string
/// carries its encoding for free.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EncodingId(pub u8);

pub const UTF_8: EncodingId = EncodingId(0);
pub const US_ASCII: EncodingId = EncodingId(1);
pub const ASCII_8BIT: EncodingId = EncodingId(2);
pub const ISO_8859_1: EncodingId = EncodingId(3);
pub const WINDOWS_1250: EncodingId = EncodingId(4);
pub const WINDOWS_1251: EncodingId = EncodingId(5);
pub const WINDOWS_1252: EncodingId = EncodingId(6);
pub const WINDOWS_1253: EncodingId = EncodingId(7);
pub const WINDOWS_1254: EncodingId = EncodingId(8);
pub const WINDOWS_1255: EncodingId = EncodingId(9);
pub const WINDOWS_1256: EncodingId = EncodingId(10);
pub const WINDOWS_1257: EncodingId = EncodingId(11);
pub const ISO_8859_2: EncodingId = EncodingId(12);
pub const ISO_8859_15: EncodingId = EncodingId(13);
pub const KOI8_R: EncodingId = EncodingId(14);
pub const SHIFT_JIS: EncodingId = EncodingId(15);
pub const WINDOWS_31J: EncodingId = EncodingId(16);
pub const EUC_JP: EncodingId = EncodingId(17);
pub const GBK: EncodingId = EncodingId(18);
pub const BIG5: EncodingId = EncodingId(19);

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
    /// whose table slot is `None` has no Unicode mapping and refuses to
    /// transcode OUT (the windows-125x vendor pages' unassigned slots).
    SingleByte,
    /// A multibyte CJK encoding: 1-3 bytes per character, structural walk
    /// per family, Unicode mapping via encoding_rs -- see `mb`.
    MultiByte(MbFamily),
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
    /// The mapping table for `EncKind::SingleByte` rows; `None` for every
    /// kind that needs no table.
    pub table: Option<&'static SingleByteTable>,
}

/// A `SingleByte` row -- name/aliases straight from `Encoding#names` under
/// the ruby 4.0.5 oracle, mapping table generated from the same oracle.
const fn single_byte(
    name: &'static str,
    aliases: &'static [&'static str],
    table: &'static SingleByteTable,
) -> EncodingSpec {
    EncodingSpec {
        name,
        aliases,
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(table),
    }
}

/// A `MultiByte` row -- name/aliases straight from `Encoding#names` under
/// the ruby 4.0.5 oracle. Both Shift_JIS and Windows-31J share the `Sjis`
/// family (CP932 mappings for both -- the documented divergence).
const fn multi_byte(
    name: &'static str,
    aliases: &'static [&'static str],
    family: MbFamily,
) -> EncodingSpec {
    EncodingSpec {
        name,
        aliases,
        ascii_compatible: true,
        kind: EncKind::MultiByte(family),
        table: None,
    }
}

/// The encoding registry. Extend by appending a row -- ids are the row
/// index, so existing ids never shift.
pub static ENCODINGS: &[EncodingSpec] = &[
    EncodingSpec {
        name: "UTF-8",
        aliases: &["CP65001", "locale", "external", "filesystem"],
        ascii_compatible: true,
        kind: EncKind::Utf8,
        table: None,
    },
    EncodingSpec {
        name: "US-ASCII",
        aliases: &["ASCII", "ANSI_X3.4-1968", "646"],
        ascii_compatible: true,
        kind: EncKind::Ascii,
        table: None,
    },
    EncodingSpec {
        name: "ASCII-8BIT",
        aliases: &["BINARY"],
        ascii_compatible: true,
        kind: EncKind::Binary,
        table: None,
    },
    EncodingSpec {
        name: "ISO-8859-1",
        aliases: &["ISO8859-1", "Latin-1"],
        ascii_compatible: true,
        kind: EncKind::Latin1,
        table: None,
    },
    single_byte("Windows-1250", &["CP1250"], &single_byte::WINDOWS_1250),
    single_byte("Windows-1251", &["CP1251"], &single_byte::WINDOWS_1251),
    single_byte("Windows-1252", &["CP1252"], &single_byte::WINDOWS_1252),
    single_byte("Windows-1253", &["CP1253"], &single_byte::WINDOWS_1253),
    single_byte("Windows-1254", &["CP1254"], &single_byte::WINDOWS_1254),
    single_byte("Windows-1255", &["CP1255"], &single_byte::WINDOWS_1255),
    single_byte("Windows-1256", &["CP1256"], &single_byte::WINDOWS_1256),
    single_byte("Windows-1257", &["CP1257"], &single_byte::WINDOWS_1257),
    single_byte("ISO-8859-2", &["ISO8859-2"], &single_byte::ISO_8859_2),
    single_byte("ISO-8859-15", &["ISO8859-15"], &single_byte::ISO_8859_15),
    single_byte("KOI8-R", &["CP878"], &single_byte::KOI8_R),
    multi_byte("Shift_JIS", &[], MbFamily::Sjis),
    multi_byte(
        "Windows-31J",
        &["CP932", "csWindows31J", "SJIS", "PCK"],
        MbFamily::Sjis,
    ),
    multi_byte("EUC-JP", &["eucJP"], MbFamily::EucJp),
    multi_byte("GBK", &["CP936"], MbFamily::Gbk),
    multi_byte("Big5", &[], MbFamily::Big5),
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
