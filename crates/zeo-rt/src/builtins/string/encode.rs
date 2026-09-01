//! `String`'s encoding half: validity guards, encoding-carrying
//! construction, `dump`/`undump`, the byte-offset family, and the
//! `encode`/`unpack` argument parsing. The `ruby_class!` rows stay in
//! `mod.rs` and call these by bare name.

use super::*;

/// Ruby REFUSES a String operation that has to READ characters when the
/// receiver's bytes are not valid in its own encoding -- `rb_enc_check` and
/// the `str_enc_get` callers raise before doing any work. zeo's `StrBuf`
/// renders a bad byte as U+FFFD, which would silently answer instead.
///
/// The gate is selective, exactly as CRuby's is: `#length`, `#chars`,
/// `#reverse`, `#lstrip`, `#index` and friends all count or move by bytes and
/// answer fine on a broken string, so they take no guard. Only a row that
/// interprets a character gets one.
///
/// Three shapes, all oracle-verified against ruby 4.0.6, because CRuby
/// reaches this from three different places.
pub(super) fn guard_valid(recv: &RubyValue) -> Result<(), crate::Signal> {
    match broken_encoding(recv) {
        None => Ok(()),
        Some(enc) => Err(arg_error!("invalid byte sequence in {enc}")),
    }
}

/// The receiver's encoding name when its bytes are NOT valid in it, `None`
/// when they are. Takes the lock and RELEASES it before the caller raises --
/// a message that inspects the receiver would take it again, and these are
/// not reentrant.
pub(super) fn broken_encoding(recv: &RubyValue) -> Option<&'static str> {
    let s = recv_str!(recv).lock();
    (!s.valid_encoding()).then(|| s.encoding().name())
}

/// [`guard_valid`] for the case-mapping family, which raises from Onigmo's
/// case-fold pass and so words it differently.
pub(super) fn guard_valid_case(recv: &RubyValue) -> Result<(), crate::Signal> {
    match recv_str!(recv).lock().valid_encoding() {
        true => Ok(()),
        false => Err(arg_error!("input string invalid")),
    }
}

/// [`guard_valid`] for `#strip`/`#rstrip`, which scan BACKWARD from the end
/// and so reach the check through `rb_enc_check` -- a different class, same
/// message. (`#lstrip` scans forward and does not raise at all.)
pub(super) fn guard_valid_compat(recv: &RubyValue) -> Result<(), crate::Signal> {
    match broken_encoding(recv) {
        None => Ok(()),
        Some(enc) => Err(crate::dispatch::raise_error(
            "Encoding::CompatibilityError",
            format!("invalid byte sequence in {enc}"),
        )),
    }
}

/// The `Encoding::CompatibilityError` a concatenation of two
/// differently-encoded non-7-bit strings raises, message shaped exactly
/// like CRuby's (receiver's encoding first, `inspect_name` forms --
/// "incompatible character encodings: BINARY (ASCII-8BIT) and UTF-8",
/// oracle-verified). See `StrBuf::push_buf`.
pub(crate) fn concat_incompat(left: &StrBuf, right: &StrBuf) -> crate::Signal {
    crate::dispatch::raise_error(
        "Encoding::CompatibilityError",
        format!(
            "incompatible character encodings: {} and {}",
            left.encoding().inspect_name(),
            right.encoding().inspect_name()
        ),
    )
}

/// The encoding two strings COMBINE into, CRuby's `rb_enc_check`.
///
/// An ASCII-only side never moves the answer -- every encoding zeo carries is
/// ASCII-compatible -- so the other side's encoding wins. Two non-ASCII sides
/// in different encodings do not combine at all, and that is a
/// `CompatibilityError`, raised whether or not any byte of the second string
/// is actually used: `"café".ljust(1, latin1)` raises in ruby even though it
/// pads nothing.
pub(crate) fn combined_encoding(
    left: &StrBuf,
    right: &StrBuf,
) -> Result<crate::encoding::EncodingId, crate::Signal> {
    if right.ascii_only() || left.encoding() == right.encoding() {
        return Ok(left.encoding());
    }
    if left.ascii_only() {
        return Ok(right.encoding());
    }
    Err(concat_incompat(left, right))
}

/// Rebuild a DERIVED string in the receiver's encoding.
///
/// The strip/pad/split/sub family computes over `to_utf8_lossy` text; handing
/// that text to `str_value` tags the answer UTF-8 and re-encodes every
/// non-ASCII character, so `"caf\xE9".force_encoding("ISO-8859-1").strip`
/// answered UTF-8 with `é` as two bytes where CRuby keeps ISO-8859-1 and one.
/// Mapping each character back through the receiver's own table restores both
/// the tag and the bytes.
///
/// `Binary` inverts `to_utf8_lossy`'s `b as char` arm directly: `encode_scalar`
/// refuses a non-ASCII scalar for a binary target, which is right for a
/// TRANSCODE (there is no such byte to convert into) and wrong here, where the
/// character came from this very buffer's own bytes.
///
/// A character the receiver's encoding cannot represent keeps today's UTF-8
/// answer rather than inventing a byte -- that only happens when the text did
/// not come from `src` to begin with.
pub(super) fn str_value_like(src: &StrBuf, text: &str) -> RubyValue {
    str_value_in(src.encoding(), text)
}

/// [`str_value_like`] for a row that answers a BYTE slice of its receiver --
/// what a trim has to do, since decoding would rewrite an invalid byte.
pub(super) fn bytes_value_like(src: &StrBuf, bytes: &[u8]) -> RubyValue {
    RubyValue::Str(crate::string_wrap(StrBuf::from_bytes(
        bytes.to_vec(),
        src.encoding(),
    )))
}

/// [`str_value_in`] for callers outside this module -- `regexp.rs` builds
/// match groups by slicing a decoded haystack and has to put them back into
/// the encoding the haystack came from.
pub fn str_value_in_enc(enc: crate::encoding::EncodingId, text: &str) -> RubyValue {
    str_value_in(enc, text)
}

/// [`str_value_like`] for a caller that has already released the receiver's
/// lock and kept only its encoding.
pub(super) fn str_value_in(enc: crate::encoding::EncodingId, text: &str) -> RubyValue {
    if enc == crate::encoding::UTF_8 {
        return str_value(text.to_string());
    }
    if text.is_ascii() {
        return RubyValue::Str(crate::string_from_bytes(text.as_bytes().to_vec(), enc));
    }
    let binary = matches!(enc.kind(), crate::encoding::EncKind::Binary);
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        let one = if binary {
            ((c as u32) < 0x100).then(|| vec![c as u32 as u8])
        } else {
            crate::encoding::encode_scalar(enc, c)
        };
        match one {
            Some(b) => bytes.extend_from_slice(&b),
            None => return str_value(text.to_string()),
        }
    }
    RubyValue::Str(crate::string_from_bytes(bytes, enc))
}

/// Rebuild every String inside a freshly-built result in `enc`.
///
/// The shared exit for `split`/`scan`, whose fields all come from the
/// receiver's own text but are built as UTF-8 by the splitter and by the
/// regexp engine (which takes a `&str` and so cannot know the encoding). The
/// nesting handles `scan` with groups, which answers an Array of Arrays.
///
/// Snapshots each Array before mapping rather than holding its guard across
/// the recursion -- the value is freshly built and cannot be self-referential
/// today, and this keeps that from being load-bearing.
pub(crate) fn reencode_strs(v: &RubyValue, enc: crate::encoding::EncodingId) -> RubyValue {
    if enc == crate::encoding::UTF_8 {
        return v.clone();
    }
    match v {
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            str_value_in(enc, &text)
        }
        RubyValue::Array(a) => {
            let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
            RubyValue::Array(crate::array_new(
                items.iter().map(|e| reencode_strs(e, enc)).collect(),
            ))
        }
        other => other.clone(),
    }
}

/// A character's code point AS THE RECEIVER'S ENCODING NUMBERS IT -- the raw
/// encoded bytes read big-endian, which is what CRuby's `String#codepoints`
/// and `#ord` answer for a non-Unicode encoding: KOI8-R `\xC1` is 193, not
/// U+0430's 1072, and EUC-JP `\xA4\xA2` is 42146, not U+3042's 12354. For
/// UTF-8 the two agree, so the Unicode scalar is used there.
pub(super) fn char_codepoint(buf: &StrBuf, r: std::ops::Range<usize>) -> i64 {
    let seq = &buf.bytes()[r];
    if buf.encoding() == crate::encoding::UTF_8
        && let Ok(s) = std::str::from_utf8(seq)
        && let Some(c) = s.chars().next()
    {
        return c as i64;
    }
    seq.iter().fold(0i64, |acc, b| (acc << 8) | *b as i64)
}

/// `<=>` over the RAW BYTES, which is what CRuby compares.
///
/// Comparing `to_utf8_lossy` renderings collapsed every undecodable byte to
/// U+FFFD, so `"\xC3" <=> "\xC4"` answered 0 while `==` -- which has always
/// compared bytes -- answered false. `sort`, `min`, `max` and `Comparable`
/// all read this.
///
/// Equal bytes are not always equal strings. CRuby breaks that tie on the
/// ENCODING when the two are not comparable, so `"café".b <=> "café"` is -1
/// rather than 0 -- and `sort` and `uniq` read the answer.
///
/// Locks in address order behind a pointer-equality short-circuit, the same
/// way `rb_eq`'s String arm does: `sort` calls this on the same pair from
/// both directions, and the receiver may BE the argument.
pub(crate) fn str_byte_cmp(a: &crate::collections::RStr, b: &crate::collections::RStr) -> i64 {
    if std::sync::Arc::ptr_eq(a, b) {
        return 0;
    }
    let forward = std::sync::Arc::as_ptr(a) < std::sync::Arc::as_ptr(b);
    let (x, y) = if forward { (a, b) } else { (b, a) };
    let gx = x.lock();
    let gy = y.lock();
    let ord = match gx.bytes().cmp(gy.bytes()) as i64 {
        0 => encoding_tiebreak(&gx, &gy),
        other => other,
    };
    if forward { ord } else { -ord }
}

/// CRuby's `rb_str_comparable` tail. Two strings with the same bytes differ
/// only when their encodings differ AND neither side is ASCII-only -- an
/// ASCII-only string compares equal against any encoding, because every
/// encoding zeo carries is ASCII-compatible. The order is the encoding's own
/// registry index, which is CRuby's too.
fn encoding_tiebreak(x: &StrBuf, y: &StrBuf) -> i64 {
    let (ex, ey) = (x.encoding(), y.encoding());
    if ex == ey || x.ascii_only() || y.ascii_only() {
        return 0;
    }
    (ex.0 as i64 - ey.0 as i64).signum()
}

use crate::encoding::StrBuf;

/// `String#dump` (CRuby `rb_str_dump`): a re-parseable double-quoted literal.
/// Control bytes become named `\t`/`\n`/... escapes or `\xNN`; a UTF-8
/// string's non-ASCII characters become `\uXXXX`/`\u{...}`. `#` is escaped
/// only before an interpolation sigil (`{`, `$`, `@`). The result keeps the
/// receiver's (ASCII-compatible) encoding so it re-`undump`s exactly; all
/// zeo encodings are ASCII-compatible, so the `.force_encoding(...)`
/// suffix CRuby appends for other encodings never applies here.
pub(super) fn dump_str(buf: &StrBuf) -> crate::collections::RStr {
    let bytes = buf.bytes();
    let u8enc = buf.encoding() == crate::encoding::UTF_8;
    let mut out = String::from("\"");
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'"' | b'\\' => {
                out.push('\\');
                out.push(c as char);
                i += 1;
            }
            b'#' => {
                if matches!(bytes.get(i + 1), Some(b'{' | b'$' | b'@')) {
                    out.push('\\');
                }
                out.push('#');
                i += 1;
            }
            b'\n' => {
                out.push_str("\\n");
                i += 1;
            }
            b'\r' => {
                out.push_str("\\r");
                i += 1;
            }
            b'\t' => {
                out.push_str("\\t");
                i += 1;
            }
            0x0C => {
                out.push_str("\\f");
                i += 1;
            }
            0x0B => {
                out.push_str("\\v");
                i += 1;
            }
            0x08 => {
                out.push_str("\\b");
                i += 1;
            }
            0x07 => {
                out.push_str("\\a");
                i += 1;
            }
            0x1B => {
                out.push_str("\\e");
                i += 1;
            }
            0x20..=0x7E => {
                out.push(c as char);
                i += 1;
            }
            _ => {
                if u8enc
                    && c > 0x7F
                    && let Some((cp, len)) = decode_utf8_char(&bytes[i..])
                {
                    if cp <= 0xFFFF {
                        out.push_str(&format!("\\u{cp:04X}"));
                    } else {
                        out.push_str(&format!("\\u{{{cp:X}}}"));
                    }
                    i += len;
                    continue;
                }
                out.push_str(&format!("\\x{c:02X}"));
                i += 1;
            }
        }
    }
    out.push('"');
    crate::string_from_bytes(out.into_bytes(), buf.encoding())
}

/// Decodes one UTF-8 character at the front of `b`, returning `(codepoint,
/// byte_len)`, or `None` for an invalid/truncated sequence (dumped as `\xNN`).
pub(super) fn decode_utf8_char(b: &[u8]) -> Option<(u32, usize)> {
    let len = match b[0] {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => return None,
    };
    let slice = b.get(..len)?;
    let ch = std::str::from_utf8(slice).ok()?.chars().next()?;
    Some((ch as u32, len))
}

pub(super) fn runtime_err(msg: &str) -> Signal {
    crate::builtins::runtime_error!("{}", msg.to_string())
}

/// `String#undump` (CRuby `str_undump`): the inverse of `dump`. Rejects a
/// non-ASCII or NUL-containing receiver, requires the `"..."` wrapper, and
/// decodes each escape. A trailing `[.dup].force_encoding("ENC")` retags the
/// result. Raises RuntimeError on any malformed input.
pub(super) fn undump_str(buf: &StrBuf) -> Result<RubyValue, Signal> {
    if !buf.ascii_only() {
        return Err(runtime_err("non-ASCII character detected"));
    }
    let bytes = buf.bytes();
    if bytes.contains(&0) {
        return Err(runtime_err("string contains null byte"));
    }
    let invalid = || {
        runtime_err(
            "invalid dumped string; not wrapped with '\"' nor \
             '\"...\".force_encoding(\"...\")' form",
        )
    };
    if bytes.len() < 2 || bytes[0] != b'"' {
        return Err(invalid());
    }

    let mut out: Vec<u8> = Vec::new();
    let mut enc = buf.encoding();
    let mut utf8 = false;
    let mut binary = false;
    let mut i = 1;
    loop {
        if i >= bytes.len() {
            return Err(runtime_err("unterminated dumped string"));
        }
        match bytes[i] {
            b'"' => {
                i += 1;
                if i == bytes.len() {
                    break;
                }
                // A `[.dup].force_encoding("ENC")` epilogue.
                if bytes[i..].starts_with(b".dup") {
                    i += 4;
                }
                let prefix = b".force_encoding(\"";
                if bytes.len() - i <= prefix.len() || !bytes[i..].starts_with(prefix) {
                    return Err(invalid());
                }
                i += prefix.len();
                if utf8 {
                    return Err(runtime_err(
                        "dumped string contained Unicode escape but used force_encoding",
                    ));
                }
                let close = bytes[i..]
                    .iter()
                    .position(|&b| b == b'"')
                    .map(|p| i + p)
                    .ok_or_else(invalid)?;
                if bytes.len() - close != 2 || bytes[close + 1] != b')' {
                    return Err(invalid());
                }
                let name = std::str::from_utf8(&bytes[i..close]).unwrap_or("");
                enc = crate::encoding::find(name)
                    .ok_or_else(|| runtime_err("dumped string has unknown encoding name"))?;
                break;
            }
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    return Err(runtime_err("invalid escape"));
                }
                undump_backslash(bytes, &mut i, &mut out, &mut enc, &mut utf8, &mut binary)?;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(RubyValue::Str(crate::string_from_bytes(out, enc)))
}

/// Reads up to `max` hex digits from the front of `b`, returning `(value,
/// digits_consumed)`.
pub(super) fn scan_hex(b: &[u8], max: usize) -> (u32, usize) {
    let mut value = 0u32;
    let mut n = 0;
    while n < max {
        match b.get(n).and_then(|c| (*c as char).to_digit(16)) {
            Some(d) => {
                value = value * 16 + d;
                n += 1;
            }
            None => break,
        }
    }
    (value, n)
}

/// Appends the UTF-8 encoding of a `\u` codepoint, rejecting out-of-range and
/// surrogate values.
pub(super) fn push_codepoint(out: &mut Vec<u8>, cp: u32) -> Result<(), Signal> {
    if cp > 0x10FFFF {
        return Err(runtime_err("invalid Unicode codepoint (too large)"));
    }
    match char::from_u32(cp) {
        Some(ch) => {
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            Ok(())
        }
        None => Err(runtime_err("invalid Unicode codepoint")),
    }
}

/// Decodes the escape following a `\` in `undump` (CRuby
/// `undump_after_backslash`), advancing `*i` past it.
pub(super) fn undump_backslash(
    bytes: &[u8],
    i: &mut usize,
    out: &mut Vec<u8>,
    enc: &mut crate::encoding::EncodingId,
    utf8: &mut bool,
    binary: &mut bool,
) -> Result<(), Signal> {
    match bytes[*i] {
        c @ (b'\\' | b'"' | b'#') => {
            out.push(c);
            *i += 1;
        }
        b'n' => {
            out.push(b'\n');
            *i += 1;
        }
        b'r' => {
            out.push(b'\r');
            *i += 1;
        }
        b't' => {
            out.push(b'\t');
            *i += 1;
        }
        b'f' => {
            out.push(0x0C);
            *i += 1;
        }
        b'v' => {
            out.push(0x0B);
            *i += 1;
        }
        b'b' => {
            out.push(0x08);
            *i += 1;
        }
        b'a' => {
            out.push(0x07);
            *i += 1;
        }
        b'e' => {
            out.push(0x1B);
            *i += 1;
        }
        b'u' => {
            if *binary {
                return Err(runtime_err("hex escape and Unicode escape are mixed"));
            }
            *utf8 = true;
            *enc = crate::encoding::UTF_8;
            *i += 1;
            if *i >= bytes.len() {
                return Err(runtime_err("invalid Unicode escape"));
            }
            if bytes[*i] == b'{' {
                *i += 1;
                loop {
                    match bytes.get(*i) {
                        None => return Err(runtime_err("unterminated Unicode escape")),
                        Some(b'}') => {
                            *i += 1;
                            break;
                        }
                        Some(c) if c.is_ascii_whitespace() => {
                            *i += 1;
                        }
                        _ => {
                            let (cp, hexlen) = scan_hex(&bytes[*i..], 7);
                            if hexlen == 0 || hexlen > 6 {
                                return Err(runtime_err("invalid Unicode escape"));
                            }
                            push_codepoint(out, cp)?;
                            *i += hexlen;
                        }
                    }
                }
            } else {
                let (cp, hexlen) = scan_hex(&bytes[*i..], 4);
                if hexlen != 4 {
                    return Err(runtime_err("invalid Unicode escape"));
                }
                push_codepoint(out, cp)?;
                *i += 4;
            }
        }
        b'x' => {
            *i += 1;
            let (v, hexlen) = scan_hex(&bytes[*i..], 2);
            if hexlen != 2 {
                return Err(runtime_err("invalid hex escape"));
            }
            let byte = v as u8;
            if byte > 0x7F {
                if *utf8 {
                    return Err(runtime_err("hex escape and Unicode escape are mixed"));
                }
                *binary = true;
            }
            out.push(byte);
            *i += 2;
        }
        c => {
            out.push(b'\\');
            out.push(c);
            *i += 1;
        }
    }
    Ok(())
}

/// Normalizes an optional byte-offset argument (`byteindex`/`byterindex`'s
/// second parameter): negative counts from the end, and an out-of-range
/// offset answers `None` (the caller returns nil).
pub(super) fn byte_offset_arg(
    arg: Option<&RubyValue>,
    len: usize,
) -> Result<Option<usize>, Signal> {
    match arg {
        None => Ok(Some(0)),
        Some(v) => {
            let p = convert::to_index(v)?;
            let p = if p < 0 { p + len as i64 } else { p };
            Ok(if p < 0 || p > len as i64 {
                None
            } else {
                Some(p as usize)
            })
        }
    }
}

/// First byte offset `>= start` where `needle` occurs in `hay` (an empty
/// needle matches at `start`).
pub(super) fn byte_find(hay: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(hay.len()));
    }
    if start > hay.len() {
        return None;
    }
    hay[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + start)
}

/// Last byte offset `<= before` where `needle` starts in `hay` (an empty
/// needle matches at `before`).
pub(super) fn byte_rfind(hay: &[u8], needle: &[u8], before: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(before.min(hay.len()));
    }
    let last_start = before.min(hay.len().saturating_sub(needle.len()));
    (0..=last_start)
        .rev()
        .find(|&i| hay[i..].starts_with(needle))
}

/// `String#encode`/`encode!`: transcode to a target encoding (default
/// `Encoding.default_internal`, else self's own), reading the CRuby option
/// matrix from a trailing options Hash. `:fallback` may be a Hash, Method, or
/// Proc -- consulted (via ordinary dispatch) for characters the target can't
/// represent, before the `:undef`/error path.
pub(super) fn encode_impl(
    recv: &RubyValue,
    args: &[RubyValue],
    in_place: bool,
) -> Result<RubyValue, Signal> {
    use crate::encoding;
    // A trailing Hash carries the keyword options; the rest are positional.
    let (positional, opts_hash) = match args.last() {
        Some(RubyValue::Hash(h)) => (&args[..args.len() - 1], Some(h.clone())),
        _ => (args, None),
    };
    let src = recv_str!(recv);
    // `encode`'s refusal is about the CONVERTER, not the name: an unknown
    // encoding here is `Encoding::ConverterNotFoundError` naming the PAIR,
    // with both sides printed RAW as given and the source first. Resolution
    // is therefore deferred until both are known -- raising on the first
    // unknown name reported an ArgumentError about it alone.
    //
    // `Encoding.find` keeps its own ArgumentError, and a Symbol argument its
    // TypeError, so the change is scoped to this row.
    let name_of = |v: &RubyValue| -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => other.to_display_string(),
        }
    };
    let from_raw = positional.get(1).map(&name_of);
    let to_raw = positional.first().map(name_of);
    let from_enc = match positional.get(1) {
        Some(v) => crate::builtins::encoding::arg_encoding(v)
            .map_err(|e| converter_not_found(&e, from_raw.as_deref(), to_raw.as_deref(), src))?,
        None => src.lock().encoding(),
    };
    let to_enc = match positional.first() {
        Some(v) => crate::builtins::encoding::arg_encoding(v)
            .map_err(|e| converter_not_found(&e, from_raw.as_deref(), to_raw.as_deref(), src))?,
        None => encoding::default_internal().unwrap_or_else(|| src.lock().encoding()),
    };
    let opts = parse_encode_opts(opts_hash.as_ref())?;
    let bytes = src.lock().bytes().to_vec();

    // Same source and destination encoding with no options is a NO-OP in
    // CRuby -- no converter runs, so invalid bytes ride through untouched
    // rather than raising. Options (`invalid:`, newline modes, ...) still
    // force the converter even for identical encodings.
    if from_enc == to_enc && opts_hash.is_none() {
        return if in_place {
            Ok(recv.clone())
        } else {
            Ok(RubyValue::Str(crate::string_from_bytes(bytes, to_enc)))
        };
    }

    let fallback_val = opts_hash.as_ref().and_then(|h| {
        let v = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("fallback")));
        (!v.is_nil()).then_some(v)
    });
    let mut fb = fallback_val.as_ref().map(|fv| {
        move |c: &str| -> Option<String> {
            let key = RubyValue::Str(crate::string_new(c.to_string()));
            let method = if matches!(fv, RubyValue::Hash(_)) {
                "[]"
            } else {
                "call"
            };
            match crate::dispatch::send_value(fv, crate::Symbol::intern(method), &[key], None) {
                Ok(v) if !v.is_nil() => Some(v.to_display_string()),
                _ => None,
            }
        }
    });
    let out = encoding::transcode(
        &bytes,
        from_enc,
        to_enc,
        &opts,
        fb.as_mut()
            .map(|f| f as &mut dyn FnMut(&str) -> Option<String>),
    )
    .map_err(crate::encoding::transcode_signal)?;

    if in_place {
        src.lock().replace_bytes(out, to_enc);
        Ok(recv.clone())
    } else {
        Ok(RubyValue::Str(crate::string_from_bytes(out, to_enc)))
    }
}

/// Reads `encode`'s keyword options out of the trailing Hash.
pub(super) fn parse_encode_opts(
    hash: Option<&crate::collections::RHash>,
) -> Result<crate::encoding::TranscodeOptions, Signal> {
    use crate::encoding::{NewlineMode, TranscodeOptions, XmlMode};
    let mut opts = TranscodeOptions::default();
    let Some(h) = hash else { return Ok(opts) };
    let get = |name: &str| crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(name)));
    let is_replace = |v: &RubyValue| matches!(v, RubyValue::Symbol(s) if s.name() == "replace");
    opts.invalid_replace = is_replace(&get("invalid"));
    opts.undef_replace = is_replace(&get("undef"));
    if let RubyValue::Str(r) = get("replace") {
        opts.replace = Some(r.lock().to_utf8_lossy().into_owned());
    }
    opts.xml = match get("xml") {
        RubyValue::Symbol(s) if s.name() == "text" => Some(XmlMode::Text),
        RubyValue::Symbol(s) if s.name() == "attr" => Some(XmlMode::Attr),
        _ => None,
    };
    // `newline:` names the mode as a SYMBOL; the three boolean spellings are
    // the older per-mode flags. The symbol form was simply absent, so
    // `encode(newline: :crlf)` converted nothing.
    //
    // Its errors are asymmetric, and copied that way: a bad SYMBOL is named
    // in the message, a non-symbol value is not.
    opts.newline = match get("newline") {
        RubyValue::Nil => None,
        RubyValue::Symbol(s) => match s.name_str() {
            "universal" => Some(NewlineMode::Universal),
            "crlf" => Some(NewlineMode::Crlf),
            "cr" => Some(NewlineMode::Cr),
            "lf" => None,
            other => {
                return Err(crate::builtins::arg_error!(
                    "unexpected value for newline option: {other}"
                ));
            }
        },
        _ => {
            return Err(crate::builtins::arg_error!(
                "unexpected value for newline option"
            ));
        }
    };
    if opts.newline.is_none() {
        opts.newline = if get("cr_newline").truthy() {
            Some(NewlineMode::Cr)
        } else if get("crlf_newline").truthy() {
            Some(NewlineMode::Crlf)
        } else if get("universal_newline").truthy() {
            Some(NewlineMode::Universal)
        } else {
            None
        };
    }
    Ok(opts)
}

/// An unknown encoding name inside `encode` is a CONVERTER failure naming
/// both ends, source first -- both raw as written, since neither resolved.
/// A non-name TypeError (a Symbol argument) passes through untouched.
fn converter_not_found(
    original: &Signal,
    from_raw: Option<&str>,
    to_raw: Option<&str>,
    src: &crate::RStr,
) -> Signal {
    let Signal::Raise(exc) = original else {
        return original.clone();
    };
    if crate::builtins::class_name_of(exc) != "ArgumentError" {
        return original.clone();
    }
    let from = from_raw
        .map(str::to_string)
        .unwrap_or_else(|| src.lock().encoding().name().to_string());
    let to = to_raw
        .map(str::to_string)
        .unwrap_or_else(|| src.lock().encoding().name().to_string());
    crate::dispatch::raise_error(
        "Encoding::ConverterNotFoundError",
        format!("code converter not found ({from} to {to})"),
    )
}

/// The template argument of `unpack`/`unpack1` as a `String` (through the
/// `to_str` protocol).
pub(super) fn unpack_template(v: &RubyValue) -> Result<String, Signal> {
    Ok(convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned())
}

/// The strings a `p`/`P` pack hung off this one, for `unpack` to resolve a
/// pointer back to. `None` when the string never came out of such a pack --
/// which is what makes CRuby's "no associated pointer" reachable.
pub(super) fn pack_associated(v: &RubyValue) -> Option<Vec<RubyValue>> {
    match crate::value::value_ivars::get(v, crate::builtins::pack::ASSOCIATED) {
        Some(RubyValue::Array(a)) => Some(a.lock().to_vec()),
        _ => None,
    }
}

/// The byte offset of the `char_idx`-th character (the string's byte length
/// when past the end) -- bridges this runtime's char-indexed string API to
/// Rust's byte-indexed slicing.
pub(super) fn byte_at_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(b, _)| b)
}

/// The `encoding:` keyword shared by `String.new` -- reads it from the
/// trailing options Hash (the kwargs convention), resolving a name string
/// or an `Encoding` value; `None` when absent.
pub(super) fn kw_encoding(
    opts: Option<&RubyValue>,
) -> Result<Option<crate::encoding::EncodingId>, Signal> {
    let Some(RubyValue::Hash(h)) = opts else {
        return Ok(None);
    };
    let v = crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("encoding")));
    if v.is_nil() {
        return Ok(None);
    }
    Ok(Some(crate::builtins::encoding::arg_encoding(&v)?))
}

/// `unpack`/`unpack1`'s `offset:` keyword: the byte index to start decoding
/// from (default 0). CRuby allows `offset == bytesize` (an empty remainder ->
/// nil), rejects a larger offset ("offset outside of string") and a negative
/// one ("offset can't be negative").
pub(super) fn kw_unpack_offset(opts: Option<&RubyValue>, len: usize) -> Result<usize, Signal> {
    let Some(RubyValue::Hash(h)) = opts else {
        return Ok(0);
    };
    let v = crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("offset")));
    let off = match v {
        RubyValue::Nil => return Ok(0),
        other => convert::to_index(&other)?,
    };
    if off < 0 {
        return Err(arg_error!("offset can't be negative"));
    }
    let off = off as usize;
    if off > len {
        return Err(arg_error!("offset outside of string"));
    }
    Ok(off)
}
