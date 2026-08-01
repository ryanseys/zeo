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

/// The ways a transcode can refuse, matching CRuby's error classes.
#[derive(Debug)]
pub enum TranscodeError {
    /// A malformed byte sequence in the SOURCE encoding.
    InvalidByteSequence(String, Box<TranscodeDetail>),
    /// A valid source character with no representation in the TARGET.
    UndefinedConversion(String, Box<TranscodeDetail>),
    /// One side is registered by name only ([`EncKind::Registered`]) and the
    /// content is not pure ASCII, so this runtime has no mapping to apply.
    /// CRuby raises this class when its own registry holds no converter for
    /// a pair; it never reaches it for these encodings, which is the
    /// divergence `docs/COMPATIBILITY.md` records.
    NoConverter(String),
}

/// CRuby's `Encoding::ConverterNotFoundError` message for a pair.
pub fn converter_not_found(from: EncodingId, to: EncodingId) -> String {
    format!(
        "code converter not found ({} to {})",
        from.name(),
        to.name()
    )
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
    decode_spans(bytes, from)
        .into_iter()
        .map(|(u, _)| u)
        .collect()
}

/// Decodes `bytes` under `from` into units, each paired with the count of
/// SOURCE bytes it consumed. A byte-order mark and an ISO-2022-JP escape
/// carry no unit of their own, so their bytes fold into the unit that
/// follows -- which keeps the spans a partition of the input, and that is
/// what an incremental converter needs to say how far it got.
pub(crate) fn decode_spans(bytes: &[u8], from: EncodingId) -> Vec<(Unit, usize)> {
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
            Some((mark, real)) => {
                let mut units = decode_spans(&bytes[mark.len()..], *real);
                if let Some((_, len)) = units.first_mut() {
                    *len += mark.len();
                }
                units
            }
            None => vec![(
                Unit::Invalid(bytes.to_vec(), crate::enc::mb::InvalidStyle::Plain),
                bytes.len(),
            )],
        };
    }
    match from.kind() {
        EncKind::Latin1 => bytes.iter().map(|b| (Unit::Char(*b as char), 1)).collect(),
        EncKind::Ascii | EncKind::Binary | EncKind::Registered => bytes
            .iter()
            .map(|b| {
                let unit = if *b < 0x80 {
                    Unit::Char(*b as char)
                } else {
                    Unit::Invalid(vec![*b], crate::enc::mb::InvalidStyle::Plain)
                };
                (unit, 1)
            })
            .collect(),
        EncKind::SingleByte => {
            let table = from.single_byte_table();
            bytes
                .iter()
                .map(|b| {
                    let unit = match table.decode(*b) {
                        Some(c) => Unit::Char(c),
                        None => Unit::Unmapped(vec![*b]),
                    };
                    (unit, 1)
                })
                .collect()
        }
        EncKind::MultiByte(family) => {
            let mut units = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                let unit = crate::enc::mb::mb_unit(family, &bytes[i..]);
                let seq = &bytes[i..i + unit.len];
                let decoded = if !unit.valid {
                    Unit::Invalid(seq.to_vec(), unit.style)
                } else {
                    match crate::enc::mb::mb_decode_seq(family, seq) {
                        Some(c) => Unit::Char(c),
                        None => Unit::Unmapped(seq.to_vec()),
                    }
                };
                units.push((decoded, unit.len));
                i += unit.len;
            }
            units
        }
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::enc::wide::wide_of(from.kind()).expect("wide kind");
            crate::enc::wide::wide_ranges(w, bytes)
                .into_iter()
                .map(|(r, scalar, style)| {
                    let len = r.len();
                    let unit = match scalar {
                        Some(c) => Unit::Char(c),
                        None => Unit::Invalid(bytes[r].to_vec(), style),
                    };
                    (unit, len)
                })
                .collect()
        }
        EncKind::Utf8 => decode_utf8_spans(bytes),
    }
}

pub(crate) fn decode_utf8(bytes: &[u8]) -> Vec<Unit> {
    decode_utf8_spans(bytes)
        .into_iter()
        .map(|(u, _)| u)
        .collect()
}

pub(crate) fn decode_utf8_spans(bytes: &[u8]) -> Vec<(Unit, usize)> {
    let mut units = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                units.extend(s.chars().map(|c| (Unit::Char(c), c.len_utf8())));
                break;
            }
            Err(e) => {
                let good = e.valid_up_to();
                // SAFETY: `good` is a validated UTF-8 boundary.
                let valid = unsafe { std::str::from_utf8_unchecked(&rest[..good]) };
                units.extend(valid.chars().map(|c| (Unit::Char(c), c.len_utf8())));
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
                units.push((
                    Unit::Invalid(rest[good..good + bad_len].to_vec(), style),
                    bad_len,
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
        EncKind::Binary | EncKind::Registered => (cp < 0x80).then(|| vec![cp as u8]),
        EncKind::SingleByte => to.single_byte_table().encode(c).map(|b| vec![b]),
        EncKind::MultiByte(family) => crate::enc::mb::mb_encode_char(family, c),
        EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            let w = crate::enc::wide::wide_of(to.kind()).expect("wide kind");
            Some(crate::enc::wide::wide_encode_char(w, c))
        }
    }
}

/// The replacement string for undefined/invalid units: the caller's
/// `:replace`, else U+FFFD for a Unicode target and `"?"` otherwise. Always
/// rendered INTO the target, so a wide target gets whole code units rather
/// than a stray ASCII byte.
fn replacement(opts: &TranscodeOptions, to: EncodingId) -> Vec<u8> {
    let text = opts.replace.as_deref().unwrap_or(match to.kind() {
        EncKind::Utf8 | EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => "\u{FFFD}",
        _ => "?",
    });
    text.chars()
        .flat_map(|c| encode_char(c, to).unwrap_or_default())
        .collect()
}

/// A `:fallback` handler: given an undefined character's UTF-8 text, answers
/// replacement text, or `None` to fall through to the error/replace path.
/// `&mut dyn FnMut` because the handler may invoke arbitrary Ruby (a Proc).
pub type TranscodeFallback<'a> = &'a mut dyn FnMut(&str) -> Option<String>;

/// Why a run stopped. Every variant but `Finished` leaves source behind.
pub enum Stop {
    /// Every source byte converted.
    Finished,
    /// The source ENDED mid-sequence. The tail is neither converted nor
    /// counted as consumed, because the caller decides what it means: more
    /// input to come (`Encoding::Converter#convert`) or a truncated string
    /// (`String#encode`, `Encoding::Converter#finish`).
    Incomplete {
        bytes: Vec<u8>,
        msg: String,
        detail: Box<TranscodeDetail>,
    },
    /// A malformed source sequence, already counted as consumed.
    Invalid {
        msg: String,
        detail: Box<TranscodeDetail>,
    },
    /// A source character the target cannot hold, already consumed.
    Undefined {
        msg: String,
        detail: Box<TranscodeDetail>,
    },
    /// One side is registered by name only, so there is no mapping to apply.
    NoConverter(String),
    /// `limit` was reached with source left over. The unit that would not
    /// fit is untouched -- neither converted nor consumed.
    DestinationFull,
}

/// One incremental run's result.
pub struct TranscodeRun {
    pub out: Vec<u8>,
    /// Source bytes consumed, which INCLUDES an offending sequence.
    pub consumed: usize,
    pub stop: Stop,
}

/// The part of a conversion that outlives one call, so a stateful converter
/// can carry it: the ISO-2022-JP escape mode and the leading byte-order
/// mark, which goes out once rather than once per chunk.
pub struct TranscodeState {
    jis: Option<crate::enc::iso2022jp::Encoder>,
    bom: &'static [u8],
    bom_written: bool,
    /// Whether the last character was a CR -- what `universal_newline` needs
    /// to fold a CRLF into one LF across a chunk boundary.
    saw_cr: bool,
    /// Whether `xml: :attr` opened a quote that `finish` must close.
    close_quote: bool,
    /// The REAL target. The dummy UTF-16/32 rows convert as their big-endian
    /// sibling behind a BOM, so every later step reads this rather than the
    /// requested encoding.
    pub to: EncodingId,
}

impl TranscodeState {
    pub fn new(to: EncodingId) -> Self {
        let (to, bom): (EncodingId, &'static [u8]) = if to == crate::enc::table::UTF_16 {
            (crate::enc::table::UTF_16BE, b"\xFE\xFF")
        } else if to == crate::enc::table::UTF_32 {
            (crate::enc::table::UTF_32BE, b"\x00\x00\xFE\xFF")
        } else {
            (to, b"")
        };
        TranscodeState {
            jis: (to == crate::enc::table::ISO_2022_JP)
                .then(|| crate::enc::iso2022jp::Encoder::new(false)),
            bom,
            bom_written: false,
            saw_cr: false,
            close_quote: false,
            to,
        }
    }

    /// Closes the stream: the `ESC ( B` every ISO-2022-JP generator emits at
    /// end of text, and the closing quote `xml: :attr` opened.
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        if let Some(enc) = self.jis.as_mut() {
            enc.finish(out);
        }
        if std::mem::take(&mut self.close_quote) {
            out.extend(encode_char('"', self.to).unwrap_or_default());
        }
    }
}

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
    let mut state = TranscodeState::new(to);
    let mut run = transcode_run(bytes, from, opts, fallback, &mut state, None);
    match run.stop {
        Stop::Finished => {}
        // A truncated tail is a whole-string error here: nothing more is
        // coming. `:invalid => :replace` swallows it like any other bad
        // sequence.
        Stop::Incomplete { msg, detail, .. } => {
            if !opts.invalid_replace {
                return Err(TranscodeError::InvalidByteSequence(msg, detail));
            }
            push_replacement(&mut run.out, opts, &mut state);
        }
        Stop::Invalid { msg, detail } => {
            return Err(TranscodeError::InvalidByteSequence(msg, detail));
        }
        Stop::Undefined { msg, detail } => {
            return Err(TranscodeError::UndefinedConversion(msg, detail));
        }
        Stop::NoConverter(msg) => return Err(TranscodeError::NoConverter(msg)),
        Stop::DestinationFull => unreachable!("no limit was set"),
    }
    state.finish(&mut run.out);
    Ok(run.out)
}

/// Converts as much of `bytes` as it can, stopping at the first refusal or
/// once `limit` output bytes exist. The engine both `String#encode` and
/// `Encoding::Converter` run on -- the difference between them is only what
/// they do with a [`Stop`].
pub fn transcode_run(
    bytes: &[u8],
    from: EncodingId,
    opts: &TranscodeOptions,
    fallback: Option<TranscodeFallback<'_>>,
    state: &mut TranscodeState,
    limit: Option<usize>,
) -> TranscodeRun {
    let mut fallback = fallback;
    let to = state.to;
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    if !state.bom_written {
        out.extend_from_slice(state.bom);
        // `xml: :attr` writes an ATTRIBUTE, quotes and all -- the opening one
        // here, the closing one in `finish`.
        if opts.xml == Some(XmlMode::Attr) {
            out.extend(encode_char('"', to).unwrap_or_default());
            state.close_quote = true;
        }
        state.bom_written = true;
    }
    // A registered-only source reads its ASCII half exactly and nothing else,
    // so a high byte is the point where this runtime runs out of mapping.
    if from.is_registered_only() && bytes.iter().any(|b| *b >= 0x80) {
        return TranscodeRun {
            out,
            consumed: 0,
            stop: Stop::NoConverter(converter_not_found(from, to)),
        };
    }
    let mut consumed = 0usize;
    let mut stop = Stop::Finished;
    for (unit, span) in decode_spans(bytes, from) {
        // Each unit's bytes are built aside so the destination limit can
        // refuse the WHOLE unit -- a half-written character is not an
        // answer. The escape-mode encoder is restored with them.
        let mut chunk: Vec<u8> = Vec::new();
        let saved = state.jis.clone();
        match unit {
            Unit::Invalid(raw, style) => {
                let incomplete = matches!(style, crate::enc::mb::InvalidStyle::Incomplete);
                if opts.invalid_replace && !incomplete {
                    push_replacement(&mut chunk, opts, state);
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
                    detail.error_bytes = raw.clone();
                    detail.incomplete = incomplete;
                    stop = if incomplete {
                        Stop::Incomplete {
                            bytes: raw,
                            msg,
                            detail,
                        }
                    } else {
                        consumed += span;
                        Stop::Invalid { msg, detail }
                    };
                    break;
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
                    push_replacement(&mut chunk, opts, state);
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
                    consumed += span;
                    stop = Stop::Undefined { msg, detail };
                    break;
                }
            }
            Unit::Char(c) => {
                // A registered-only TARGET takes ASCII and stops there, the
                // mirror of the source-side check above.
                if to.is_registered_only() && !c.is_ascii() {
                    stop = Stop::NoConverter(converter_not_found(from, to));
                    break;
                }
                if !push_char(&mut chunk, c, opts, state) {
                    // Undefined in the target: fallback, then :undef, then error.
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    let replaced = fallback.as_deref_mut().and_then(|f| f(s));
                    if let Some(rep) = replaced {
                        push_text(&mut chunk, &rep, state);
                    } else if opts.xml.is_some() {
                        push_ascii(&mut chunk, &format!("&#x{:X};", c as u32), to);
                    } else if opts.undef_replace {
                        push_replacement(&mut chunk, opts, state);
                    } else {
                        let mut detail = TranscodeDetail::new(from, to);
                        detail.error_bytes = encode_char(c, from).unwrap_or_default();
                        detail.error_char = Some(c);
                        consumed += span;
                        stop = Stop::Undefined {
                            msg: undef_message(c, from, to),
                            detail,
                        };
                        break;
                    }
                }
            }
        }
        if limit.is_some_and(|l| out.len() + chunk.len() > l) {
            state.jis = saved;
            stop = Stop::DestinationFull;
            break;
        }
        out.extend_from_slice(&chunk);
        consumed += span;
    }
    TranscodeRun {
        out,
        consumed,
        stop,
    }
}

/// Emits one character into `out`, applying the newline decorator and
/// `:xml` escaping. `false` when the target cannot represent it.
fn push_char(
    out: &mut Vec<u8>,
    c: char,
    opts: &TranscodeOptions,
    state: &mut TranscodeState,
) -> bool {
    // `universal_newline` is an INPUT transformation -- a CR and a CRLF both
    // become one LF -- so it acts before the character is encoded, and it
    // needs the one bit of state saying whether a CR came just before.
    if opts.newline == Some(NewlineMode::Universal) {
        let after_cr = std::mem::replace(&mut state.saw_cr, c == '\r');
        if c == '\r' {
            return emit(out, '\n', opts, state);
        }
        if c == '\n' && after_cr {
            return true;
        }
    }
    // The other two rewrite a LF on the way OUT.
    match (opts.newline, c) {
        (Some(NewlineMode::Cr), '\n') => return emit(out, '\r', opts, state),
        (Some(NewlineMode::Crlf), '\n') => {
            return emit(out, '\r', opts, state) && emit(out, '\n', opts, state);
        }
        _ => {}
    }
    emit(out, c, opts, state)
}

/// One character straight into the target, with `:xml` escaping applied.
fn emit(out: &mut Vec<u8>, c: char, opts: &TranscodeOptions, state: &mut TranscodeState) -> bool {
    if let Some(enc) = state.jis.as_mut() {
        return enc.push(c, out).is_ok();
    }
    let Some(bytes) = encode_char(c, state.to) else {
        return false;
    };
    apply_xml(out, c, &bytes, opts, state.to);
    true
}

/// Emits replacement TEXT -- through the escape encoder when there is one,
/// so raw bytes never land inside a kanji run.
fn push_text(out: &mut Vec<u8>, text: &str, state: &mut TranscodeState) {
    match state.jis.as_mut() {
        Some(enc) => emit_via(enc, text, out),
        None => out.extend(
            text.chars()
                .flat_map(|c| encode_char(c, state.to).unwrap_or_default()),
        ),
    }
}

/// Emits the `:replace` text for a refused unit.
fn push_replacement(out: &mut Vec<u8>, opts: &TranscodeOptions, state: &mut TranscodeState) {
    match state.jis.is_some() {
        true => push_text(out, opts.replace.as_deref().unwrap_or("?"), state),
        false => out.extend_from_slice(&replacement(opts, state.to)),
    }
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
fn apply_xml(out: &mut Vec<u8>, c: char, bytes: &[u8], opts: &TranscodeOptions, to: EncodingId) {
    let escaped = match (opts.xml, c) {
        (Some(_), '&') => Some("&amp;"),
        (Some(_), '<') => Some("&lt;"),
        (Some(_), '>') => Some("&gt;"),
        (Some(XmlMode::Attr), '"') => Some("&quot;"),
        _ => None,
    };
    match escaped {
        Some(e) => push_ascii(out, e, to),
        None => out.extend_from_slice(bytes),
    }
}

/// ASCII markup rendered INTO the target -- a wide target spells `&amp;` in
/// whole code units, not in bare bytes.
fn push_ascii(out: &mut Vec<u8>, text: &str, to: EncodingId) {
    out.extend(
        text.chars()
            .flat_map(|c| encode_char(c, to).unwrap_or_default()),
    );
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
