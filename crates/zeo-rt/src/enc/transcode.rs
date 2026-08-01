//! Transcoding (`String#encode`): decode the source encoding into Unicode
//! units, re-encode into the target, applying the `:invalid`/`:undef`/
//! `:replace`/`:xml`/`:newline` options.

use crate::enc::table::{EncKind, EncodingId};

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
    InvalidByteSequence(String, Box<TranscodeDetail>),
    /// A valid source character with no representation in the TARGET.
    UndefinedConversion(String, Box<TranscodeDetail>),
}

/// What the raised exception exposes beyond its message: the encoding pair the
/// conversion ran between, and the offending input. CRuby's
/// `Encoding::InvalidByteSequenceError#error_bytes` and
/// `UndefinedConversionError#error_char` read exactly this, which is why the
/// raise site has to keep it rather than formatting it all into the message.
#[derive(Debug)]
pub struct TranscodeDetail {
    pub source: EncodingId,
    pub destination: EncodingId,
    /// The offending bytes AS THEY APPEAR in the source encoding.
    pub error_bytes: Vec<u8>,
    /// The character with no target representation; `None` for an invalid
    /// sequence, which never decoded to one.
    pub error_char: Option<char>,
    /// Whether the sequence was cut short rather than simply wrong -- what
    /// `#incomplete_input?` reports.
    pub incomplete: bool,
}

impl TranscodeDetail {
    fn new(source: EncodingId, destination: EncodingId) -> Box<Self> {
        Box::new(TranscodeDetail {
            source,
            destination,
            error_bytes: Vec::new(),
            error_char: None,
            incomplete: false,
        })
    }
}

/// One decoded source unit on the way from `from` to `to`.
pub(crate) enum Unit {
    /// A decoded Unicode scalar.
    Char(char),
    /// Bytes the SOURCE encoding couldn't decode (`:invalid` territory),
    /// with the CRuby message form they warrant.
    Invalid(Vec<u8>, crate::enc::mb::InvalidStyle),
    /// A VALID source character with no Unicode mapping (an unassigned
    /// windows-125x vendor-page slot, an unassigned CJK pair): `:undef`
    /// territory, but reported by its bytes -- there is no scalar to name.
    Unmapped(Vec<u8>),
}

const UTF_16_BOMS: &[(&[u8], EncodingId)] = &[
    (b"\xFE\xFF", crate::enc::table::UTF_16BE),
    (b"\xFF\xFE", crate::enc::table::UTF_16LE),
];

const UTF_32_BOMS: &[(&[u8], EncodingId)] = &[
    (b"\x00\x00\xFE\xFF", crate::enc::table::UTF_32BE),
    (b"\xFF\xFE\x00\x00", crate::enc::table::UTF_32LE),
];

/// The encoding a leading byte-order mark names, and how many bytes it takes.
/// UTF-32's marks are tested FIRST: `FF FE 00 00` opens with UTF-16LE's own
/// mark, so the shorter one would otherwise always win. UTF-8's mark is
/// included, which `IO#set_encoding_by_bom` needs and the UTF-16/32 decoders
/// above never see.
pub fn self_describing_bom(bytes: &[u8]) -> Option<(EncodingId, usize)> {
    let utf8: &[(&[u8], EncodingId)] = &[(b"\xEF\xBB\xBF", crate::enc::table::UTF_8)];
    UTF_32_BOMS
        .iter()
        .chain(utf8)
        .chain(UTF_16_BOMS)
        .find(|(mark, _)| bytes.starts_with(mark))
        .map(|(mark, id)| (*id, mark.len()))
}

/// Decodes `bytes` under `from` into a sequence of units.
pub(crate) fn decode(bytes: &[u8], from: EncodingId) -> Vec<Unit> {
    // The dummy rows have no per-character structure (kind `Binary`), but
    // they DO have converters -- CRuby's exact split. ISO-2022-JP is the
    // stateful escape codec; dummy UTF-16/32 read an endianness off their
    // BOM, and without one the whole string is one invalid unit
    // (`"a\x00" on UTF-16`, oracle-verified).
    if from == crate::enc::table::ISO_2022_JP {
        return crate::enc::iso2022jp::decode_units(bytes);
    }
    if from == crate::enc::table::UTF_16 || from == crate::enc::table::UTF_32 {
        let bom: &[(&[u8], EncodingId)] = if from == crate::enc::table::UTF_16 {
            UTF_16_BOMS
        } else {
            UTF_32_BOMS
        };
        if bytes.is_empty() {
            return Vec::new();
        }
        return match bom.iter().find(|(mark, _)| bytes.starts_with(mark)) {
            Some((mark, real)) => decode(&bytes[mark.len()..], *real),
            None => vec![Unit::Invalid(
                bytes.to_vec(),
                crate::enc::mb::InvalidStyle::Plain,
            )],
        };
    }
    match from.kind() {
        EncKind::Latin1 => bytes.iter().map(|b| Unit::Char(*b as char)).collect(),
        EncKind::Ascii | EncKind::Binary => bytes
            .iter()
            .map(|b| {
                if *b < 0x80 {
                    Unit::Char(*b as char)
                } else {
                    Unit::Invalid(vec![*b], crate::enc::mb::InvalidStyle::Plain)
                }
            })
            .collect(),
        EncKind::SingleByte => {
            let table = from.single_byte_table();
            bytes
                .iter()
                .map(|b| match table.decode(*b) {
                    Some(c) => Unit::Char(c),
                    None => Unit::Unmapped(vec![*b]),
                })
                .collect()
        }
        EncKind::MultiByte(family) => {
            let mut units = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                let unit = crate::enc::mb::mb_unit(family, &bytes[i..]);
                let seq = &bytes[i..i + unit.len];
                if !unit.valid {
                    units.push(Unit::Invalid(seq.to_vec(), unit.style));
                } else {
                    match crate::enc::mb::mb_decode_seq(family, seq) {
                        Some(c) => units.push(Unit::Char(c)),
                        None => units.push(Unit::Unmapped(seq.to_vec())),
                    }
                }
                i += unit.len;
            }
            units
        }
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::enc::wide::wide_of(from.kind()).expect("wide kind");
            crate::enc::wide::wide_ranges(w, bytes)
                .into_iter()
                .map(|(r, scalar, style)| match scalar {
                    Some(c) => Unit::Char(c),
                    None => Unit::Invalid(bytes[r].to_vec(), style),
                })
                .collect()
        }
        EncKind::Utf8 => decode_utf8(bytes),
    }
}

pub(crate) fn decode_utf8(bytes: &[u8]) -> Vec<Unit> {
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
                // `error_len() == None` means the input ENDED mid-sequence,
                // which is CRuby's `incomplete "..."` form (and what
                // `#incomplete_input?` reports) rather than a plainly wrong
                // byte. The multibyte decoders already draw this distinction;
                // UTF-8 discarded it.
                let style = match e.error_len() {
                    Some(_) => crate::enc::mb::InvalidStyle::Plain,
                    None => crate::enc::mb::InvalidStyle::Incomplete,
                };
                let bad_len = e.error_len().unwrap_or(rest.len() - good).max(1);
                units.push(Unit::Invalid(rest[good..good + bad_len].to_vec(), style));
                rest = &rest[good + bad_len..];
            }
        }
    }
    units
}

/// Encodes one scalar into `to`'s bytes, or `None` if `to` can't represent
/// it (an undefined conversion). Public as `encode_scalar` for the
/// runtime's `Integer#chr`/`String#<<` codepoint paths.
pub fn encode_scalar(enc: EncodingId, c: char) -> Option<Vec<u8>> {
    encode_char(c, enc)
}

fn encode_char(c: char, to: EncodingId) -> Option<Vec<u8>> {
    let cp = c as u32;
    match to.kind() {
        EncKind::Utf8 => Some(c.to_string().into_bytes()),
        EncKind::Ascii => (cp < 0x80).then(|| vec![cp as u8]),
        EncKind::Latin1 => (cp < 0x100).then(|| vec![cp as u8]),
        // Binary accepts any ASCII char verbatim; a non-ASCII scalar has no
        // binary byte (it isn't a byte).
        EncKind::Binary => (cp < 0x80).then(|| vec![cp as u8]),
        EncKind::SingleByte => to.single_byte_table().encode(c).map(|b| vec![b]),
        EncKind::MultiByte(family) => crate::enc::mb::mb_encode_char(family, c),
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::enc::wide::wide_of(to.kind()).expect("wide kind");
            Some(crate::enc::wide::wide_encode_char(w, c))
        }
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
    // Dummy targets, mirroring the decode side: UTF-16/32 write a BOM then
    // big-endian code units; ISO-2022-JP threads a stateful escape encoder
    // through the loop (`jis`), finished after the last unit.
    let (to, bom): (EncodingId, &[u8]) = if to == crate::enc::table::UTF_16 {
        (crate::enc::table::UTF_16BE, b"\xFE\xFF")
    } else if to == crate::enc::table::UTF_32 {
        (crate::enc::table::UTF_32BE, b"\x00\x00\xFE\xFF")
    } else {
        (to, &[])
    };
    let mut jis =
        (to == crate::enc::table::ISO_2022_JP).then(|| crate::enc::iso2022jp::Encoder::new(false));
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    out.extend_from_slice(bom);
    for unit in decode(bytes, from) {
        match unit {
            Unit::Invalid(raw, style) => {
                if opts.invalid_replace {
                    match jis.as_mut() {
                        Some(enc) => {
                            emit_via(enc, opts.replace.as_deref().unwrap_or("?"), &mut out)
                        }
                        None => out.extend_from_slice(&replacement(opts, to)),
                    }
                } else {
                    let esc: String = raw.iter().map(|b| quote_byte(*b)).collect();
                    // CRuby's three forms, oracle-verified: `incomplete
                    // "\x8F" on EUC-JP` (lead truncated by end-of-string),
                    // `"\x82" followed by "\x00" on Shift_JIS` (valid lead,
                    // wrong next byte -- printable trails quoted verbatim,
                    // `"\xA1" followed by "a"`), plain `"\xFF" on EUC-JP`.
                    let msg = match style {
                        crate::enc::mb::InvalidStyle::Plain => {
                            format!("\"{esc}\" on {}", from.name())
                        }
                        crate::enc::mb::InvalidStyle::Incomplete => {
                            format!("incomplete \"{esc}\" on {}", from.name())
                        }
                        crate::enc::mb::InvalidStyle::FollowedBy(t) => {
                            format!(
                                "\"{esc}\" followed by \"{}\" on {}",
                                quote_byte(t),
                                from.name()
                            )
                        }
                    };
                    let mut detail = TranscodeDetail::new(from, to);
                    detail.error_bytes = raw;
                    detail.incomplete = matches!(style, crate::enc::mb::InvalidStyle::Incomplete);
                    return Err(TranscodeError::InvalidByteSequence(msg, detail));
                }
            }
            // A valid character with no Unicode mapping: `:undef` territory,
            // byte-quoted (there is no scalar to name). Follows the same
            // short/long message split as `undef_message`: the short form
            // only for a direct-to-UTF-8 conversion from a plainly-named
            // source (`"\x82z" from Shift_JIS to UTF-8`); a windows-named
            // source or any pivot gets the long path form -- oracle-verified
            // all three ways.
            Unit::Unmapped(raw) => {
                if opts.undef_replace {
                    match jis.as_mut() {
                        Some(enc) => {
                            emit_via(enc, opts.replace.as_deref().unwrap_or("?"), &mut out)
                        }
                        None => out.extend_from_slice(&replacement(opts, to)),
                    }
                } else {
                    let escaped: String = raw.iter().map(|b| quote_byte(*b)).collect();
                    let msg =
                        if to == crate::enc::table::UTF_8 && transcoder_name(from) == from.name() {
                            format!("\"{escaped}\" from {} to UTF-8", from.name())
                        } else {
                            let tail = if to == crate::enc::table::UTF_8 {
                                String::new()
                            } else {
                                format!(" to {}", transcoder_name(to))
                            };
                            format!(
                                "\"{escaped}\" to UTF-8 in conversion from {} to UTF-8{tail}",
                                from.name()
                            )
                        };
                    let mut detail = TranscodeDetail::new(from, to);
                    detail.error_bytes = raw;
                    return Err(TranscodeError::UndefinedConversion(msg, detail));
                }
            }
            Unit::Char(c) => {
                match jis.as_mut() {
                    Some(enc) => {
                        if enc.push(c, &mut out).is_ok() {
                            continue;
                        }
                    }
                    None => {
                        if let Some(bytes) = encode_char(c, to) {
                            apply_xml(&mut out, c, &bytes, opts, to);
                            if opts.xml.is_none() {
                                maybe_newline(&mut out, c, &bytes, opts);
                            }
                            continue;
                        }
                    }
                }
                // Undefined in the target: fallback, then :undef, then error.
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                if let Some(f) = fallback.as_deref_mut() {
                    if let Some(rep) = f(s) {
                        match jis.as_mut() {
                            Some(enc) => emit_via(enc, &rep, &mut out),
                            None => out.extend(
                                rep.chars()
                                    .flat_map(|c| encode_char(c, to).unwrap_or_default()),
                            ),
                        }
                        continue;
                    }
                }
                if let Some(xml) = opts.xml {
                    push_xml_ref(&mut out, c, xml);
                } else if opts.undef_replace {
                    match jis.as_mut() {
                        Some(enc) => {
                            emit_via(enc, opts.replace.as_deref().unwrap_or("?"), &mut out)
                        }
                        None => out.extend_from_slice(&replacement(opts, to)),
                    }
                } else {
                    let mut detail = TranscodeDetail::new(from, to);
                    detail.error_bytes = encode_char(c, from).unwrap_or_default();
                    detail.error_char = Some(c);
                    return Err(TranscodeError::UndefinedConversion(
                        undef_message(c, from, to),
                        detail,
                    ));
                }
            }
        }
    }
    if let Some(enc) = jis.as_mut() {
        enc.finish(&mut out);
    }
    Ok(out)
}

/// Replacement text through the stateful ISO-2022-JP encoder -- raw bytes
/// would land inside a kanji run and corrupt the mode. Characters the
/// encoder can't take (a non-JIS replacement string) are dropped, matching
/// `encode_char`'s `unwrap_or_default` on the stateless path.
fn emit_via(enc: &mut crate::enc::iso2022jp::Encoder, text: &str, out: &mut Vec<u8>) {
    for c in text.chars() {
        let _ = enc.push(c, out);
    }
}

/// How CRuby quotes one byte inside an error message's `"..."`: printable
/// ASCII verbatim (`incomplete "\xD84"` -- the 0x34 is a literal `4`),
/// everything else as `\xNN`. Oracle-verified.
fn quote_byte(b: u8) -> String {
    if (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\' {
        (b as char).to_string()
    } else {
        format!("\\x{b:02X}")
    }
}

/// How CRuby's transcoder registry spells `enc` in error messages: the
/// windows-125x pages register under UPCASED entry names (`WINDOWS-1252`),
/// everything else under its canonical name. The name mismatch is also what
/// selects the long message form below -- both oracle-verified.
fn transcoder_name(enc: EncodingId) -> String {
    let name = enc.name();
    if name.starts_with("Windows-125") {
        name.to_ascii_uppercase()
    } else {
        name.to_string()
    }
}

/// The `Encoding::UndefinedConversionError` message, matching CRuby's three
/// oracle-verified shapes:
/// - direct from UTF-8, plainly-named target: `U+3042 from UTF-8 to KOI8-R`
/// - direct from UTF-8, windows target: `U+044B to WINDOWS-1250 in
///   conversion from UTF-8 to WINDOWS-1250`
/// - any pivoted pair: `U+0430 to ISO-8859-2 in conversion from KOI8-R to
///   UTF-8 to ISO-8859-2` (the FROM side keeps its requested spelling).
fn undef_message(c: char, from: EncodingId, to: EncodingId) -> String {
    let cp = c as u32;
    let to_disp = transcoder_name(to);
    if from == crate::enc::table::UTF_8 {
        if to_disp == to.name() {
            format!("U+{cp:04X} from UTF-8 to {to_disp}")
        } else {
            format!("U+{cp:04X} to {to_disp} in conversion from UTF-8 to {to_disp}")
        }
    } else {
        format!(
            "U+{cp:04X} to {to_disp} in conversion from {} to UTF-8 to {to_disp}",
            from.name()
        )
    }
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
pub(crate) fn utf8_seq_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}
