//! Transcoding (`String#encode`): decode the source encoding into Unicode
//! units, re-encode into the target, applying the `:invalid`/`:undef`/
//! `:replace`/`:xml`/`:newline` options.

use crate::table::{EncKind, EncodingId};

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

/// One decoded source unit on the way from `from` to `to`.
pub(crate) enum Unit {
    /// A decoded Unicode scalar.
    Char(char),
    /// Bytes the SOURCE encoding couldn't decode (`:invalid` territory),
    /// with the CRuby message form they warrant.
    Invalid(Vec<u8>, crate::mb::InvalidStyle),
    /// A VALID source character with no Unicode mapping (an unassigned
    /// windows-125x vendor-page slot, an unassigned CJK pair): `:undef`
    /// territory, but reported by its bytes -- there is no scalar to name.
    Unmapped(Vec<u8>),
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
                    Unit::Invalid(vec![*b], crate::mb::InvalidStyle::Plain)
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
                let unit = crate::mb::mb_unit(family, &bytes[i..]);
                let seq = &bytes[i..i + unit.len];
                if !unit.valid {
                    units.push(Unit::Invalid(seq.to_vec(), unit.style));
                } else {
                    match crate::mb::mb_decode_seq(family, seq) {
                        Some(c) => units.push(Unit::Char(c)),
                        None => units.push(Unit::Unmapped(seq.to_vec())),
                    }
                }
                i += unit.len;
            }
            units
        }
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::wide::wide_of(from.kind()).expect("wide kind");
            crate::wide::wide_ranges(w, bytes)
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
                let bad_len = e.error_len().unwrap_or(rest.len() - good).max(1);
                units.push(Unit::Invalid(
                    rest[good..good + bad_len].to_vec(),
                    crate::mb::InvalidStyle::Plain,
                ));
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
        EncKind::MultiByte(family) => crate::mb::mb_encode_char(family, c),
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::wide::wide_of(to.kind()).expect("wide kind");
            Some(crate::wide::wide_encode_char(w, c))
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
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    for unit in decode(bytes, from) {
        match unit {
            Unit::Invalid(raw, style) => {
                if opts.invalid_replace {
                    out.extend_from_slice(&replacement(opts, to));
                } else {
                    let esc: String = raw.iter().map(|b| quote_byte(*b)).collect();
                    // CRuby's three forms, oracle-verified: `incomplete
                    // "\x8F" on EUC-JP` (lead truncated by end-of-string),
                    // `"\x82" followed by "\x00" on Shift_JIS` (valid lead,
                    // wrong next byte -- printable trails quoted verbatim,
                    // `"\xA1" followed by "a"`), plain `"\xFF" on EUC-JP`.
                    let msg = match style {
                        crate::mb::InvalidStyle::Plain => {
                            format!("\"{esc}\" on {}", from.name())
                        }
                        crate::mb::InvalidStyle::Incomplete => {
                            format!("incomplete \"{esc}\" on {}", from.name())
                        }
                        crate::mb::InvalidStyle::FollowedBy(t) => {
                            format!(
                                "\"{esc}\" followed by \"{}\" on {}",
                                quote_byte(t),
                                from.name()
                            )
                        }
                    };
                    return Err(TranscodeError::InvalidByteSequence(msg));
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
                    out.extend_from_slice(&replacement(opts, to));
                } else {
                    let escaped: String = raw.iter().map(|b| quote_byte(*b)).collect();
                    let msg = if to == crate::table::UTF_8 && transcoder_name(from) == from.name() {
                        format!("\"{escaped}\" from {} to UTF-8", from.name())
                    } else {
                        let tail = if to == crate::table::UTF_8 {
                            String::new()
                        } else {
                            format!(" to {}", transcoder_name(to))
                        };
                        format!(
                            "\"{escaped}\" to UTF-8 in conversion from {} to UTF-8{tail}",
                            from.name()
                        )
                    };
                    return Err(TranscodeError::UndefinedConversion(msg));
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
                    return Err(TranscodeError::UndefinedConversion(undef_message(
                        c, from, to,
                    )));
                }
            }
        }
    }
    Ok(out)
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
    if from == crate::table::UTF_8 {
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
