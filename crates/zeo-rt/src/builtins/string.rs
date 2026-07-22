//! `String` (CRuby string.c) -- the Tier A surface: case family, strip
//! family, split/chars/lines, sub/gsub (String AND Regexp patterns, with
//! block forms), indexing forms, tr/delete/squeeze/count, conversions,
//! succ, padding, `%` formatting. Strings are UTF-8 (`char`-indexed like
//! modern CRuby); the bytes/encoding surface is Tier C, documented in the
//! plan.

use crate::builtins::{
    arg_error, arg_int, arg_str, arity, block_or_enum, builtin_methods, convert, index_error,
    range_error, recv_str, regexp_error, type_error,
};
use crate::{RubyValue, Signal};

/// The largest string this runtime will attempt to allocate (1 GiB).
///
/// A request past this is answered with `ArgumentError: string size too big`
/// rather than being passed to the allocator, where an over-large request
/// ABORTS the process instead of raising something a program can rescue.
/// CRuby has no equivalent flat cap -- it guards only the length
/// multiplication and lets the allocator raise `NoMemoryError` -- so this is a
/// deliberate divergence toward a deterministic, catchable failure. See the
/// `String#*` guard for the full rationale.
const MAX_STRING_SIZE: usize = 1 << 30;

/// `capitalize`'s rule: first char upcased, the REST downcased.
pub(crate) fn capitalize_str(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

pub(crate) fn swapcase_str(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_uppercase() {
                c.to_lowercase().collect::<Vec<_>>()
            } else {
                c.to_uppercase().collect::<Vec<_>>()
            }
        })
        .collect()
}

/// `String#succ`: increment the rightmost alphanumeric run with carry
/// (`"az" -> "ba"`, `"zz" -> "aaa"`, `"a9" -> "b0"` -- CRuby's rule); with
/// no alphanumerics, bump the last char's codepoint.
pub(crate) fn succ_str(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    // CRuby's `str_succ`: walk right-to-left incrementing the rightmost
    // alphanumeric with carry. The carry skips non-alnums, but STOPS (inserting
    // a fresh char) when it crosses a non-alnum into an alnum of the OTHER kind
    // -- so "1.9" carries across the dot to "2.0", while "a-9" (letter then
    // digit) inserts to "a-10". No alnum at all -> plain byte increment.
    let mut found_alnum = false;
    let mut done = false;
    let mut prev_was_nonchar = false;
    let mut last_wrapped: Option<char> = None; // the char after the most recent wrap
    let mut carry_pos = 0usize; // where a leftover carry inserts its char
    let mut carry_char = '1';
    for i in (0..n).rev() {
        let c = chars[i];
        if prev_was_nonchar {
            if let Some(w) = last_wrapped {
                let flip = (w.is_ascii_alphabetic() && c.is_ascii_digit())
                    || (w.is_ascii_digit() && c.is_ascii_alphabetic());
                if flip {
                    break;
                }
            }
        }
        if !c.is_ascii_alphanumeric() {
            prev_was_nonchar = true;
            continue;
        }
        prev_was_nonchar = false;
        found_alnum = true;
        carry_pos = i;
        carry_char = match c {
            'a'..='z' => 'a',
            'A'..='Z' => 'A',
            _ => '1',
        };
        let (next, wrapped) = match c {
            'z' => ('a', true),
            'Z' => ('A', true),
            '9' => ('0', true),
            _ => (char::from_u32(c as u32 + 1).expect("ascii alnum"), false),
        };
        chars[i] = next;
        if !wrapped {
            done = true;
            break;
        }
        last_wrapped = Some(next);
    }
    if !found_alnum {
        let last = n - 1;
        chars[last] = char::from_u32(chars[last] as u32 + 1).unwrap_or(chars[last]);
        return chars.into_iter().collect();
    }
    if !done {
        // The carry overflowed the leftmost alnum (or broke at a kind flip):
        // insert a fresh char of that alnum's kind ("zz" -> "aaa", "a-9" ->
        // "a-10").
        chars.insert(carry_pos, carry_char);
    }
    chars.into_iter().collect()
}

/// Expands `a-c` ranges in a `tr`/`delete`/`squeeze`/`count` charset.
fn expand_charset(set: &str) -> Vec<char> {
    let chars: Vec<char> = set.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i + 1] == '-' {
            let (lo, hi) = (chars[i] as u32, chars[i + 2] as u32);
            for c in lo..=hi {
                if let Some(c) = char::from_u32(c) {
                    out.push(c);
                }
            }
            i += 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// A `tr`/`squeeze` character-set spec expanded to `(chars, negated)`: a
/// leading `^` (with something after it) complements the set, matching CRuby.
/// A bare `"^"` is the literal caret.
fn tr_charset(spec: &str) -> (Vec<char>, bool) {
    match spec.strip_prefix('^') {
        Some(rest) if !rest.is_empty() => (expand_charset(rest), true),
        _ => (expand_charset(spec), false),
    }
}

/// Lenient `String#to_i(base)`: optional sign + leading digits (with
/// single underscores), anything else terminates the parse; no valid
/// digits at all is `0`. (Contrast `Kernel#Integer`'s strict parse.)
fn lenient_to_i(text: &str, base: u32) -> RubyValue {
    let t = text.trim_start();
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let mut digits = String::new();
    let mut prev_underscore = true;
    for c in t.chars() {
        if c == '_' && !prev_underscore {
            prev_underscore = true;
            continue;
        }
        if c.is_digit(base) {
            digits.push(c);
            prev_underscore = false;
        } else {
            break;
        }
    }
    match num_bigint::BigInt::parse_bytes(digits.as_bytes(), base) {
        Some(n) => crate::builtins::integer::int_value(if negative { -n } else { n }),
        None => RubyValue::Int(0),
    }
}

/// The `Encoding::CompatibilityError` a concatenation of two
/// differently-encoded non-7-bit strings raises, message shaped exactly
/// like CRuby's (receiver's encoding first, `inspect_name` forms --
/// "incompatible character encodings: BINARY (ASCII-8BIT) and UTF-8",
/// oracle-verified). See `StrBuf::push_buf`.
fn concat_incompat(left: &StrBuf, right: &StrBuf) -> crate::Signal {
    crate::dispatch::raise_error(
        "Encoding::CompatibilityError",
        format!(
            "incompatible character encodings: {} and {}",
            left.encoding().inspect_name(),
            right.encoding().inspect_name()
        ),
    )
}

fn str_value(s: String) -> RubyValue {
    RubyValue::Str(crate::string_new(s))
}

use crate::encoding::StrBuf;

/// `String#dump` (CRuby `rb_str_dump`): a re-parseable double-quoted literal.
/// Control bytes become named `\t`/`\n`/... escapes or `\xNN`; a UTF-8
/// string's non-ASCII characters become `\uXXXX`/`\u{...}`. `#` is escaped
/// only before an interpolation sigil (`{`, `$`, `@`). The result keeps the
/// receiver's (ASCII-compatible) encoding so it re-`undump`s exactly; all
/// zeo encodings are ASCII-compatible, so the `.force_encoding(...)`
/// suffix CRuby appends for other encodings never applies here.
fn dump_str(buf: &StrBuf) -> crate::collections::RStr {
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
                if u8enc && c > 0x7F {
                    if let Some((cp, len)) = decode_utf8_char(&bytes[i..]) {
                        if cp <= 0xFFFF {
                            out.push_str(&format!("\\u{cp:04X}"));
                        } else {
                            out.push_str(&format!("\\u{{{cp:X}}}"));
                        }
                        i += len;
                        continue;
                    }
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
fn decode_utf8_char(b: &[u8]) -> Option<(u32, usize)> {
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

fn runtime_err(msg: &str) -> Signal {
    crate::dispatch::raise_error("RuntimeError", msg.to_string())
}

/// `String#undump` (CRuby `str_undump`): the inverse of `dump`. Rejects a
/// non-ASCII or NUL-containing receiver, requires the `"..."` wrapper, and
/// decodes each escape. A trailing `[.dup].force_encoding("ENC")` retags the
/// result. Raises RuntimeError on any malformed input.
fn undump_str(buf: &StrBuf) -> Result<RubyValue, Signal> {
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
fn scan_hex(b: &[u8], max: usize) -> (u32, usize) {
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
fn push_codepoint(out: &mut Vec<u8>, cp: u32) -> Result<(), Signal> {
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
fn undump_backslash(
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
fn byte_offset_arg(arg: Option<&RubyValue>, len: usize) -> Result<Option<usize>, Signal> {
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
fn byte_find(hay: &[u8], needle: &[u8], start: usize) -> Option<usize> {
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
fn byte_rfind(hay: &[u8], needle: &[u8], before: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(before.min(hay.len()));
    }
    let last_start = before.min(hay.len().saturating_sub(needle.len()));
    (0..=last_start)
        .rev()
        .find(|&i| hay[i..].starts_with(needle))
}

/// CRuby's `rb_str_modify` guard: a frozen receiver can't be mutated in place.
/// Shared by the mutators that don't route through `str_bang_replace`
/// (`insert`/`prepend`/`replace`), so a `frozen_string_literal` literal raises
/// on every one.
fn guard_str_frozen(recv: &RubyValue) -> Result<(), Signal> {
    if recv_str!(recv).is_frozen() {
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!("can't modify frozen String: {}", recv.inspect_string()),
            &[("receiver", recv.clone())],
        ));
    }
    Ok(())
}

/// The shared body of the in-place `!` mutators (`chomp!`/`chop!`/
/// `delete_prefix!`/`delete_suffix!`): a frozen receiver is a FrozenError
/// (CRuby raises on ANY bang method, modification or not), then `new_text`
/// replaces the content and the receiver is returned -- unless the content
/// was already `new_text`, which answers `nil` ("no modification was made").
fn str_bang_replace(recv: &RubyValue, new_text: String) -> Result<RubyValue, Signal> {
    let s = recv_str!(recv);
    if s.is_frozen() {
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!("can't modify frozen String: {}", recv.inspect_string()),
            &[("receiver", recv.clone())],
        ));
    }
    let unchanged = s.lock().to_utf8_lossy() == new_text;
    if unchanged {
        return Ok(RubyValue::Nil);
    }
    s.lock().replace_utf8(new_text);
    Ok(recv.clone())
}

/// The shared body of the transform `!` mutators (`upcase!`, `gsub!`, ...):
/// runs the non-bang `base` method (reusing all its logic, including blocks
/// and args), then writes the result back in place -- answering `nil` when
/// nothing changed, the receiver otherwise, per CRuby.
fn str_bang_via(
    recv: &RubyValue,
    base: &str,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, crate::Signal> {
    let produced = crate::dispatch::send_value(recv, crate::Symbol::intern(base), args, block)?;
    match produced {
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            str_bang_replace(recv, text)
        }
        // A non-String result (e.g. a `succ` edge) can't be spliced back;
        // fall back to leaving the receiver untouched.
        _ => Ok(RubyValue::Nil),
    }
}

/// `partition`/`rpartition`'s three-part split around a String or Regexp
/// separator: `[before, match, after]`, and (`whole`, `""`, `""`) with the
/// empty parts on `partition`'s tail / `rpartition`'s head when there's no
/// match (`from_end` picks which). Char-index based so multibyte input keeps
/// its boundaries.
fn str_partition(text: &str, sep: &RubyValue, from_end: bool) -> Result<[RubyValue; 3], Signal> {
    let span = match sep {
        RubyValue::Str(needle) => {
            let needle = needle.lock().to_utf8_lossy().into_owned();
            if from_end {
                text.rfind(&needle).map(|b| (b, b + needle.len()))
            } else {
                text.find(&needle).map(|b| (b, b + needle.len()))
            }
        }
        RubyValue::Regexp(re) => {
            let idx = if from_end {
                crate::regexp_rindex(re, text, None)
            } else {
                crate::regexp_match_index(re, text)
            };
            match idx {
                RubyValue::Int(ci) => {
                    // `regexp_*index` answers a CHAR index; recover the match's
                    // byte span by re-matching the whole string.
                    let byte_start = text
                        .char_indices()
                        .nth(ci as usize)
                        .map_or(text.len(), |(b, _)| b);
                    match crate::regexp_match(re, &text[byte_start..]) {
                        RubyValue::MatchData(m) => {
                            let matched = crate::matchdata_group(&m, 0);
                            let len = match &matched {
                                RubyValue::Str(s) => s.lock().to_utf8_lossy().len(),
                                _ => 0,
                            };
                            Some((byte_start, byte_start + len))
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        other => {
            return Err(type_error!(
                "type mismatch: {} given",
                crate::builtins::class_name_of(other)
            ));
        }
    };
    Ok(match span {
        Some((start, end)) => [
            str_value(text[..start].to_string()),
            str_value(text[start..end].to_string()),
            str_value(text[end..].to_string()),
        ],
        None if from_end => [
            str_value(String::new()),
            str_value(String::new()),
            str_value(text.to_string()),
        ],
        None => [
            str_value(text.to_string()),
            str_value(String::new()),
            str_value(String::new()),
        ],
    })
}

/// `String#[]`/`slice` with a Regexp: the whole match or a named/numbered
/// capture group. `None` group arg means the whole match.
fn regexp_index(
    re: &crate::RRegexp,
    text: &str,
    group: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let RubyValue::MatchData(m) = crate::regexp_match(re, text) else {
        return Ok(RubyValue::Nil);
    };
    match group {
        None => Ok(crate::matchdata_group(&m, 0)),
        Some(RubyValue::Int(n)) => Ok(crate::matchdata_group(&m, *n)),
        Some(RubyValue::Str(name)) => {
            crate::matchdata_group_by_name(&m, &name.lock().to_utf8_lossy())
        }
        Some(RubyValue::Symbol(s)) => crate::matchdata_group_by_name(&m, &s.name()),
        _ => Ok(RubyValue::Nil),
    }
}

/// `String#oct`/`#hex`: a leading integer in `default_base`, honoring an
/// explicit `0x`/`0b`/`0o`/`0d` prefix, underscores between digits, and a
/// leading sign; stops at the first invalid digit (0 when none), never
/// raising -- CRuby's lenient parse. BigInt-accumulated, so large inputs stay
/// exact.
fn parse_int_lenient(text: &str, default_base: u32) -> RubyValue {
    use num_bigint::BigInt;
    let s = text.trim_start();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (base, s) = if let Some(r) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (16, r)
    } else if let Some(r) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        (2, r)
    } else if let Some(r) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        (8, r)
    } else if let Some(r) = s.strip_prefix("0d").or_else(|| s.strip_prefix("0D")) {
        (10, r)
    } else if default_base == 0 {
        // Base 0 (auto-detect): a bare leading `0` with more digits is octal,
        // C-style; anything else is decimal.
        match s.strip_prefix('0') {
            Some(rest) if !rest.is_empty() => (8, rest),
            _ => (10, s),
        }
    } else {
        (default_base, s)
    };
    let mut val = BigInt::from(0);
    let big_base = BigInt::from(base);
    for c in s.chars() {
        if c == '_' {
            continue;
        }
        match c.to_digit(base) {
            Some(d) => val = val * &big_base + BigInt::from(d),
            None => break,
        }
    }
    crate::builtins::integer::int_value(if neg { -val } else { val })
}

/// `String#slice!`: removes the matched span from `recv` in place and returns
/// it. Supports the `(index[, len])` / `(range)` / `(substring)` forms
/// (Regexp/`slice!` is a documented gap).
fn slice_bang_impl(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let handle = recv_str!(recv);
    let mut chars: Vec<char> = handle.lock().char_vec();
    let n = chars.len() as i64;
    let norm = |i: i64| if i < 0 { i + n } else { i };
    // Resolve the [start, end) character span to remove.
    let (start, end) = match (&args[0], args.get(1)) {
        (RubyValue::Int(i), Some(RubyValue::Int(len))) => {
            let start = norm(*i);
            if start < 0 || start > n || *len < 0 {
                return Ok(RubyValue::Nil);
            }
            (start as usize, (start + *len).min(n) as usize)
        }
        (RubyValue::Int(i), None) => {
            let start = norm(*i);
            if start < 0 || start >= n {
                return Ok(RubyValue::Nil);
            }
            (start as usize, (start + 1) as usize)
        }
        (RubyValue::Range(s, e, exclusive), None) => {
            let start = match s.as_deref() {
                Some(RubyValue::Int(v)) => norm(*v),
                None => 0,
                _ => return Ok(RubyValue::Nil),
            };
            let end = match e.as_deref() {
                Some(RubyValue::Int(v)) => {
                    let v = norm(*v);
                    if *exclusive { v } else { v + 1 }
                }
                None => n,
                _ => return Ok(RubyValue::Nil),
            };
            if start < 0 || start > n {
                return Ok(RubyValue::Nil);
            }
            (start as usize, end.clamp(start, n) as usize)
        }
        (RubyValue::Str(sub), None) => {
            let needle: Vec<char> = sub.lock().to_utf8_lossy().chars().collect();
            match find_subslice(&chars, &needle) {
                Some(pos) => (pos, pos + needle.len()),
                None => return Ok(RubyValue::Nil),
            }
        }
        _ => return Ok(RubyValue::Nil),
    };
    let removed: String = chars[start..end].iter().collect();
    chars.drain(start..end);
    handle.lock().replace_utf8(chars.into_iter().collect());
    Ok(str_value(removed))
}

/// The `String#[]=` engine (CRuby `rb_str_aset_m`): resolves the target
/// character span for every index shape, then splices in the replacement.
/// Returns the assigned value, matching Ruby's index-assignment expression.
fn index_set_impl(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let handle = recv_str!(recv);
    if handle.is_frozen() {
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!("can't modify frozen String: {}", recv.inspect_string()),
            &[("receiver", recv.clone())],
        ));
    }
    let val = args.last().unwrap();
    // The replacement's `to_str` conversion happens at CRuby's exact point in
    // each index shape: AFTER the index converts but BEFORE its bounds check
    // (Int/start-length/Range forms), yet only after a Regexp/substring index
    // has actually matched. `None` here means "not yet converted".
    let mut repl: Option<crate::collections::RStr> = None;

    let mut chars: Vec<char> = handle.lock().char_vec();
    let n = chars.len() as i64;
    let norm = |i: i64| if i < 0 { i + n } else { i };
    let index_err = |msg: String| crate::dispatch::raise_error("IndexError", msg);

    // Resolve the [start, end) character span to overwrite.
    let (start, end) = if let RubyValue::Regexp(re) = &args[0] {
        let text: String = handle.lock().to_utf8_lossy().into_owned();
        let RubyValue::MatchData(m) = crate::regexp_match(re, &text) else {
            return Err(index_err("regexp not matched".to_string()));
        };
        let span = if args.len() == 3 {
            match &args[1] {
                RubyValue::Int(k) => m.groups.get(*k as usize).copied().flatten(),
                RubyValue::Str(name) => group_span_by_name(&m, &name.lock().to_utf8_lossy()),
                RubyValue::Symbol(s) => group_span_by_name(&m, &s.name()),
                _ => None,
            }
        } else {
            m.groups.first().copied().flatten()
        };
        let Some((bstart, bend)) = span else {
            return Err(index_err("regexp not matched".to_string()));
        };
        (text[..bstart].chars().count(), text[..bend].chars().count())
    } else if args.len() == 3 {
        let (i, len) = (arg_int!(args, 0), arg_int!(args, 1));
        repl = Some(convert::to_rstr(val)?);
        let start = norm(i);
        if start < 0 || start > n {
            return Err(index_err(format!("index {i} out of string")));
        }
        if len < 0 {
            return Err(index_err(format!("negative length {len}")));
        }
        (start as usize, (start + len).min(n) as usize)
    } else {
        match &args[0] {
            RubyValue::Range(s, e, exclusive) => {
                let start = match s.as_deref() {
                    Some(v) => norm(convert::to_index(v)?),
                    None => 0,
                };
                if start < 0 || start > n {
                    return Err(range_error!("{} out of range", args[0].to_display_string()));
                }
                let end = match e.as_deref() {
                    Some(v) => {
                        let v = norm(convert::to_index(v)?);
                        if *exclusive { v } else { v + 1 }
                    }
                    None => n,
                };
                repl = Some(convert::to_rstr(val)?);
                (start as usize, end.clamp(start, n) as usize)
            }
            RubyValue::Str(sub) => {
                let needle: Vec<char> = sub.lock().to_utf8_lossy().chars().collect();
                match find_subslice(&chars, &needle) {
                    Some(pos) => (pos, pos + needle.len()),
                    None => return Err(index_err("string not matched".to_string())),
                }
            }
            other => {
                let i = convert::to_index(other)?;
                repl = Some(convert::to_rstr(val)?);
                let start = norm(i);
                if start < 0 || start > n {
                    return Err(index_err(format!("index {i} out of string")));
                }
                (start as usize, (start + 1).min(n) as usize)
            }
        }
    };

    let repl = match repl {
        Some(r) => r,
        None => convert::to_rstr(val)?,
    };
    let repl_chars: Vec<char> = repl.lock().to_utf8_lossy().chars().collect();
    chars.splice(start..end, repl_chars);
    handle.lock().replace_utf8(chars.into_iter().collect());
    Ok(val.clone())
}

/// `String#crypt` (CRuby `rb_str_crypt`): validates the receiver and salt,
/// then calls the host `crypt(3)`. The libc buffer is process-static, so the
/// call is serialized under a mutex.
fn crypt_impl(recv: &RubyValue, salt_arg: &RubyValue) -> Result<RubyValue, Signal> {
    use std::ffi::{CStr, CString};
    static CRYPT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let key = recv_str!(recv).lock().bytes().to_vec();
    if key.contains(&0) {
        return Err(arg_error!("string contains null byte"));
    }
    let salt = &convert::to_rstr(salt_arg)?;
    let salt = salt.lock().bytes().to_vec();
    if salt.len() < 2 || salt[0] == 0 || salt[1] == 0 {
        return Err(arg_error!("salt too short (need >=2 bytes)"));
    }
    // `crypt` reads both arguments as C strings; the key has no NUL and the
    // salt is truncated at its first NUL (its leading two bytes are non-zero).
    let key_c = CString::new(key).expect("key has no interior NUL");
    let salt_trunc: Vec<u8> = salt.into_iter().take_while(|&b| b != 0).collect();
    let salt_c = CString::new(salt_trunc).expect("salt truncated at first NUL");

    // `crypt(3)` is an XSI extension the `libc` crate doesn't declare on every
    // target, so bind it directly (it resolves from libSystem/libcrypt).
    unsafe extern "C" {
        fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
    }
    let _guard = CRYPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let res = unsafe { crypt(key_c.as_ptr(), salt_c.as_ptr()) };
    if res.is_null() {
        return Err(crate::dispatch::raise_error(
            "SystemCallError",
            "crypt failed".to_string(),
        ));
    }
    let bytes = unsafe { CStr::from_ptr(res) }.to_bytes().to_vec();
    Ok(RubyValue::Str(crate::string_from_bytes(
        bytes,
        crate::encoding::ASCII_8BIT,
    )))
}

/// Applies a Unicode normalization form named by a `:nfc`/`:nfd`/`:nfkc`/
/// `:nfkd` symbol (or string), defaulting to NFC. An unknown form raises
/// ArgumentError, matching CRuby's `lib/unicode_normalize`.
fn normalize_form(text: &str, form: Option<&RubyValue>) -> Result<String, Signal> {
    use unicode_normalization::UnicodeNormalization;
    let name = match form {
        None => "nfc".to_string(),
        Some(RubyValue::Symbol(s)) => s.name().to_string(),
        Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
        // CRuby's lib/unicode_normalize rejects any other argument as an
        // unknown form (`"a".unicode_normalize(1)` is ArgumentError, not
        // TypeError -- oracle-verified).
        Some(other) => {
            return Err(arg_error!(
                "Invalid normalization form {}.",
                other.to_display_string()
            ));
        }
    };
    Ok(match name.as_str() {
        "nfc" => text.nfc().collect(),
        "nfd" => text.nfd().collect(),
        "nfkc" => text.nfkc().collect(),
        "nfkd" => text.nfkd().collect(),
        _ => {
            return Err(arg_error!("Invalid normalization form {name}."));
        }
    })
}

/// The byte span of a named capture group, or `None` when the name is absent
/// or the group didn't participate in the match.
fn group_span_by_name(m: &crate::RMatchData, name: &str) -> Option<(usize, usize)> {
    let idx = m.names.iter().find(|(nm, _)| nm == name)?.1;
    m.groups.get(idx).copied().flatten()
}

/// The first index of `needle` within `haystack` (both char slices), or None.
fn find_subslice(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

builtin_methods! {
    pub(crate) fn lookup;

    "length"[0] | "size"[0] => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::string_len(recv_str!(recv))))
    }
    "empty?"[0] => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(crate::string_len(recv_str!(recv)) == 0))
    }
    // Byte-level accessors, honoring the string's real encoding (`bytes`
    // yields the raw bytes; `bytesize` counts them, distinct from the
    // char-counting `length`).
    "bytesize"[0] => fn bytesize(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_str!(recv).lock().bytesize() as i64))
    }
    // `String#-@` / `#dedup`: an already-frozen receiver is returned as-is
    // (CRuby #2630); otherwise the content is interned to its immortal
    // frozen twin, so two dedups of equal content are the same object.
    "-@"[0] | "dedup"[0] => fn dedup(recv, args, _block) {
        arity!(args, 0);
        let s = recv_str!(recv);
        if s.is_frozen() {
            return Ok(recv.clone());
        }
        let (bytes, enc) = {
            let buf = s.lock();
            (buf.bytes().to_vec(), buf.encoding())
        };
        Ok(RubyValue::Str(crate::intern_frozen(
            crate::encoding::StrBuf::from_bytes(bytes, enc),
        )))
    }
    "bytes"[0] => fn bytes(recv, args, _block) {
        arity!(args, 0);
        let out = recv_str!(recv)
            .lock()
            .bytes()
            .iter()
            .map(|b| RubyValue::Int(*b as i64))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "each_byte"[0] => fn each_byte(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_byte", args, block);
        let bytes: Vec<u8> = recv_str!(recv).lock().bytes().to_vec();
        for b in bytes {
            p.call(&[RubyValue::Int(b as i64)])?;
        }
        Ok(recv.clone())
    }
    "getbyte"[1] => fn getbyte(recv, args, _block) {
        arity!(args, 1);
        let i = arg_int!(args, 0);
        let s = recv_str!(recv).lock();
        let idx = if i < 0 { i + s.bytesize() as i64 } else { i };
        Ok(if idx >= 0 && (idx as usize) < s.bytesize() {
            RubyValue::Int(s.getbyte(idx as usize).unwrap() as i64)
        } else {
            RubyValue::Nil
        })
    }
    "setbyte"[2] => fn setbyte(recv, args, _block) {
        arity!(args, 2);
        let (i, b) = (arg_int!(args, 0), arg_int!(args, 1));
        let s = recv_str!(recv);
        let len = s.lock().bytesize() as i64;
        let idx = if i < 0 { i + len } else { i };
        if idx < 0 || idx >= len {
            return Err(index_error!("index {i} out of string"));
        }
        // AFTER the index conversion and range check -- CRuby's
        // `rb_str_setbyte` order (a frozen receiver still reports
        // TypeError/IndexError for bad arguments first, oracle-verified).
        guard_str_frozen(recv)?;
        s.lock().setbyte(idx as usize, (b & 0xff) as u8);
        Ok(args[1].clone())
    }
    // `byteslice(offset[, len])` -- a substring cut on BYTE boundaries (one
    // byte when `len` is omitted), tagged with the receiver's encoding; nil
    // when `offset` is out of range. Negative offsets count from the end.
    "byteslice" => fn byteslice(recv, args, _block) {
        arity!(args, 1..=2);
        let (bytes, enc) = {
            let s = recv_str!(recv).lock();
            (s.bytes().to_vec(), s.encoding())
        };
        let n = bytes.len() as i64;
        // `byteslice(start..end)` -- a single Range argument cuts on byte
        // boundaries; negative endpoints count from the end, an out-of-range
        // start is nil.
        if let RubyValue::Range(s, e, exclusive) = &args[0] {
            let start = match s.as_deref() {
                Some(RubyValue::Int(v)) => if *v < 0 { *v + n } else { *v },
                None => 0,
                _ => return Ok(RubyValue::Nil),
            };
            if start < 0 || start > n {
                return Ok(RubyValue::Nil);
            }
            let end = match e.as_deref() {
                Some(RubyValue::Int(v)) => {
                    let v = if *v < 0 { *v + n } else { *v };
                    if *exclusive { v } else { v + 1 }
                }
                None => n,
                _ => return Ok(RubyValue::Nil),
            };
            let end = end.clamp(start, n) as usize;
            return Ok(RubyValue::Str(crate::string_from_bytes(
                bytes[start as usize..end].to_vec(),
                enc,
            )));
        }
        let off = arg_int!(args, 0);
        let off = if off < 0 { off + n } else { off };
        // With a length, `offset == bytesize` yields "" (an empty cut); the
        // single-argument form needs an actual byte to read, so the boundary
        // is nil.
        let has_len = args.get(1).is_some();
        if off < 0 || off > n || (off == n && !has_len) {
            return Ok(RubyValue::Nil);
        }
        let len = match args.get(1) {
            Some(v) => {
                let l = convert::to_index(v)?;
                if l < 0 {
                    return Ok(RubyValue::Nil);
                }
                l
            }
            None => 1,
        };
        let end = (off + len).min(n) as usize;
        Ok(RubyValue::Str(crate::string_from_bytes(
            bytes[off as usize..end].to_vec(),
            enc,
        )))
    }
    // `byteindex`/`byterindex(str[, offset])` -- the BYTE offset of the first
    // (respectively last) occurrence of a String needle, or nil.
    "byteindex" => fn byteindex(recv, args, _block) {
        arity!(args, 1..=2);
        let hay = recv_str!(recv).lock().bytes().to_vec();
        // `byteindex(regexp[, offset])` -- the BYTE offset of the first match.
        if let RubyValue::Regexp(re) = &args[0] {
            let Some(start) = byte_offset_arg(args.get(1), hay.len())? else {
                return Ok(RubyValue::Nil);
            };
            let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
            if start > text.len() || !text.is_char_boundary(start) {
                return Ok(RubyValue::Nil);
            }
            return Ok(match crate::regexp::regexp_find(re, &text[start..]) {
                Some((b, _)) => RubyValue::Int((start + b) as i64),
                None => RubyValue::Nil,
            });
        }
        let needle = arg_str!(args, 0).lock().bytes().to_vec();
        let start = byte_offset_arg(args.get(1), hay.len())?;
        let Some(start) = start else { return Ok(RubyValue::Nil) };
        Ok(match byte_find(&hay, &needle, start) {
            Some(i) => RubyValue::Int(i as i64),
            None => RubyValue::Nil,
        })
    }
    "byterindex" => fn byterindex(recv, args, _block) {
        arity!(args, 1..=2);
        let hay = recv_str!(recv).lock().bytes().to_vec();
        // Omitted position searches the whole string (from the end); an
        // explicit one bounds the match start (negative counts from the end).
        let before = match args.get(1) {
            None => hay.len(),
            Some(v) => match byte_offset_arg(Some(v), hay.len())? {
                Some(p) => p,
                None => return Ok(RubyValue::Nil),
            },
        };
        // `byterindex(regexp[, pos])` -- the BYTE offset of the last match.
        if let RubyValue::Regexp(re) = &args[0] {
            let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
            return Ok(match crate::regexp::regexp_byterindex(re, &text, before) {
                Some(i) => RubyValue::Int(i as i64),
                None => RubyValue::Nil,
            });
        }
        let needle = arg_str!(args, 0).lock().bytes().to_vec();
        Ok(match byte_rfind(&hay, &needle, before) {
            Some(i) => RubyValue::Int(i as i64),
            None => RubyValue::Nil,
        })
    }
    // --- Encoding surface -------------------------------------------------
    "encoding"[0] => fn encoding_m(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::builtins::encoding::encoding_value(recv_str!(recv).lock().encoding()))
    }
    // `force_encoding` re-TAGS the bytes without touching them; `b` COPIES
    // them under ASCII-8BIT. Both return a value the caller can chain.
    "force_encoding"[1] => fn force_encoding(recv, args, _block) {
        arity!(args, 1);
        let id = crate::builtins::encoding::arg_encoding(&args[0])?;
        recv_str!(recv).lock().set_encoding(id);
        Ok(recv.clone())
    }
    "b"[0] => fn to_binary(recv, args, _block) {
        arity!(args, 0);
        let bytes = recv_str!(recv).lock().bytes().to_vec();
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT)))
    }
    // `append_as_bytes(*args)`: appends each argument's raw bytes to the
    // receiver in place -- an Integer contributes its low byte (`n & 0xFF`), a
    // String its bytes verbatim -- keeping the receiver's encoding. Answers
    // self.
    "append_as_bytes" => fn append_as_bytes(recv, args, _block) {
        let handle = recv_str!(recv);
        if handle.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen String: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        let mut extra: Vec<u8> = Vec::new();
        for a in args {
            match a {
                RubyValue::Int(i) => extra.push((*i & 0xFF) as u8),
                RubyValue::BigInt(b) => {
                    use num_traits::ToPrimitive;
                    extra.push((&**b & num_bigint::BigInt::from(0xFF)).to_u8().unwrap_or(0));
                }
                RubyValue::Str(s) => extra.extend_from_slice(s.lock().bytes()),
                other => {
                    return Err(type_error!("wrong argument type {} (expected String or Integer)",
                            crate::builtins::class_name_of(other)))
                }
            }
        }
        let mut lock = handle.lock();
        let enc = lock.encoding();
        let mut bytes = lock.bytes().to_vec();
        bytes.extend_from_slice(&extra);
        lock.replace_bytes(bytes, enc);
        Ok(recv.clone())
    }
    // `crypt(salt)`: the platform `crypt(3)` one-way hash (DES/MD5/... per the
    // salt), delegated to libc so the output matches the host Ruby exactly.
    // The result is ASCII-8BIT.
    "crypt"[1] => fn crypt(recv, args, _block) {
        arity!(args, 1);
        crypt_impl(recv, &args[0])
    }
    "ascii_only?"[0] => fn ascii_only(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_str!(recv).lock().ascii_only()))
    }
    "valid_encoding?"[0] => fn valid_encoding(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_str!(recv).lock().valid_encoding()))
    }
    "encode" => fn encode(recv, args, _block) {
        encode_impl(recv, args, false)
    }
    "encode!" => fn encode_bang(recv, args, _block) {
        encode_impl(recv, args, true)
    }
    // `unpack`/`unpack1`: deserialize the bytes per a template (see
    // `builtins::pack`). `unpack` answers the whole Array; `unpack1` the
    // first element (nil when empty).
    "unpack"[-2] => fn unpack(recv, args, _block) {
        let positional = kw_strip(args);
        arity!(positional, 1);
        let template = unpack_template(&positional[0])?;
        let bytes = recv_str!(recv).lock().bytes().to_vec();
        let start = kw_unpack_offset(args, bytes.len())?;
        let vals = crate::builtins::pack::unpack(&bytes[start..], &template)?;
        Ok(RubyValue::Array(crate::array_new(vals)))
    }
    "unpack1"[-2] => fn unpack1(recv, args, _block) {
        let positional = kw_strip(args);
        arity!(positional, 1);
        let template = unpack_template(&positional[0])?;
        let bytes = recv_str!(recv).lock().bytes().to_vec();
        let start = kw_unpack_offset(args, bytes.len())?;
        let vals = crate::builtins::pack::unpack(&bytes[start..], &template)?;
        Ok(vals.into_iter().next().unwrap_or(RubyValue::Nil))
    }
    "scrub" => fn scrub(recv, args, _block) {
        // Rewrite every invalid byte sequence to the replacement (an explicit
        // String argument, else U+FFFD for a Unicode encoding / "?" otherwise).
        arity!(args, 0..=1);
        let s = recv_str!(recv).lock();
        let repl = match args.first() {
            Some(RubyValue::Str(r)) => Some(r.lock().to_utf8_lossy().into_owned()),
            // Default: U+FFFD for a Unicode encoding (the replacement
            // character, 3 bytes in UTF-8), "?" otherwise -- CRuby's rule.
            _ => matches!(
                s.encoding().kind(),
                crate::encoding::EncKind::Utf8
                    | crate::encoding::EncKind::Utf16 { .. }
                    | crate::encoding::EncKind::Utf32 { .. }
            )
            .then(|| "\u{FFFD}".to_string()),
        };
        let mut opts = crate::encoding::TranscodeOptions { invalid_replace: true, ..Default::default() };
        opts.replace = repl;
        // Scrub = transcode to self's own encoding, replacing invalids.
        let out = crate::encoding::transcode(s.bytes(), s.encoding(), s.encoding(), &opts, None)
            .map_err(crate::encoding::transcode_signal)?;
        Ok(RubyValue::Str(crate::string_from_bytes(out, s.encoding())))
    }
    // `scrub!` scrubs in place and ALWAYS answers the receiver (unlike the
    // other bang mutators, which answer nil when nothing changed).
    "scrub!" => fn scrub_bang(recv, args, _block) {
        arity!(args, 0..=1);
        let scrubbed = crate::dispatch::send_value(recv, crate::Symbol::intern("scrub"), args, None)?;
        let s = recv_str!(recv);
        if s.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen String: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        if let RubyValue::Str(new) = &scrubbed {
            let g = new.lock();
            let (bytes, enc) = (g.bytes().to_vec(), g.encoding());
            drop(g);
            s.lock().replace_bytes(bytes, enc);
        }
        Ok(recv.clone())
    }
    "include?"[1] => fn include_p(recv, args, _block) {
        arity!(args, 1);
        let needle = arg_str!(args, 0);
        let found = recv_str!(recv)
            .lock()
            .to_utf8_lossy()
            .contains(&*needle.lock().to_utf8_lossy());
        Ok(RubyValue::Bool(found))
    }
    "+"[1] => fn plus(recv, args, _block) {
        arity!(args, 1);
        let other = arg_str!(args, 0);
        // BYTE concatenation under the encoding-compatibility rule
        // (`compat_concat_enc`) -- never through the lossy display text,
        // which promoted a BINARY `0xB5` to UTF-8 `0xC2 0xB5` (the bug that
        // corrupted digest/pack bytes and every `chr`-built binary string).
        //
        // Clone the receiver's buffer out and RELEASE its guard before
        // locking `other`. `s + s` hands the same `Arc<Mutex<..>>` in twice,
        // and parking_lot's Mutex is not reentrant, so taking both guards at
        // once deadlocks the process -- a hang, with no output and no error.
        let mut joined = recv_str!(recv).lock().clone();
        let compatible = joined.push_buf(&other.lock());
        if compatible.is_err() {
            return Err(concat_incompat(&joined, &other.lock()));
        }
        Ok(RubyValue::Str(crate::collections::string_wrap(joined)))
    }
    // Mutating append -- returns the receiver (the same object).
    "<<"[1] | "concat" => fn concat(recv, args, _block) {
        // `<<` is syntactically a single-arg operator; `concat` accepts any
        // number of arguments and appends them left-to-right.
        let s = recv_str!(recv);
        // Frozen check at the mutator (CRuby's `rb_str_modify`): `<<`/`concat`
        // dispatch through this one row for every receiver shape, so guarding
        // here covers them all -- including a `frozen_string_literal` literal.
        // CRuby checks modifiability before appending any argument.
        if s.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen String: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        for arg in args {
            match arg {
                // `str << 65` appends the CODEPOINT's character IN THE
                // RECEIVER'S ENCODING: one raw byte for the byte encodings
                // (`"".b << 181` is the single byte 0xB5, and anything past
                // 255 is out of range there), the UTF-8 character otherwise.
                // A Float does NOT truncate here -- it goes through `to_str`
                // and fails, CRuby's rule. All oracle-verified.
                RubyValue::Int(i) => {
                    let mut g = s.lock();
                    match g.encoding().kind() {
                        crate::encoding::EncKind::Latin1
                        | crate::encoding::EncKind::Binary
                        | crate::encoding::EncKind::SingleByte => {
                            let Ok(b) = u8::try_from(*i) else {
                                return Err(range_error!("{i} out of char range"));
                            };
                            g.push_bytes(&[b]);
                        }
                        // The codepoint IS the byte sequence read big-endian
                        // (`sjis << 0x82A0` appends bytes 82 A0); CRuby's two
                        // RangeError messages, oracle-verified.
                        crate::encoding::EncKind::MultiByte(family) => {
                            let enc = g.encoding();
                            let bytes = u32::try_from(*i)
                                .map_err(|_| crate::encoding::MbCodepointError::OutOfRange)
                                .and_then(|cp| crate::encoding::mb_codepoint_bytes(family, cp))
                                .map_err(|e| match e {
                                    crate::encoding::MbCodepointError::OutOfRange => {
                                        range_error!("{i} out of char range")
                                    }
                                    crate::encoding::MbCodepointError::InvalidCodepoint => {
                                        range_error!(
                                            "invalid codepoint 0x{i:X} in {}",
                                            enc.name()
                                        )
                                    }
                                })?;
                            g.push_bytes(&bytes);
                        }
                        crate::encoding::EncKind::Utf8 | crate::encoding::EncKind::Ascii => {
                            let Some(c) = u32::try_from(*i).ok().and_then(char::from_u32) else {
                                return Err(range_error!("{i} out of char range"));
                            };
                            g.push_str(&c.to_string());
                        }
                        // Unicode scalars, encoded in the receiver's own
                        // wide layout (`utf16le << 0x20AC` appends AC 20).
                        crate::encoding::EncKind::Utf16 { .. }
                        | crate::encoding::EncKind::Utf32 { .. } => {
                            let enc = g.encoding();
                            let Some(c) = u32::try_from(*i).ok().and_then(char::from_u32) else {
                                return Err(range_error!("{i} out of char range"));
                            };
                            let bytes = crate::encoding::encode_scalar(enc, c)
                                .expect("wide encodings represent every scalar");
                            g.push_bytes(&bytes);
                        }
                    }
                }
                RubyValue::BigInt(_) => {
                    return Err(range_error!("bignum out of char range"))
                }
                // A String (or `to_str` duck): raw-byte append under the
                // encoding-compatibility rule -- see `"+"` just above.
                other => {
                    let addition = convert::to_rstr(other)?;
                    let addition = addition.lock().clone();
                    let mut g = s.lock();
                    if g.push_buf(&addition).is_err() {
                        let signal = concat_incompat(&g, &addition);
                        return Err(signal);
                    }
                }
            }
        }
        Ok(recv.clone())
    }
    "*"[1] => fn times(recv, args, _block) {
        arity!(args, 1);
        let n = arg_int!(args, 0);
        if n < 0 {
            return Err(arg_error!("negative argument"));
        }
        // RAW bytes, keeping the receiver's encoding -- `180.chr * 3` is
        // three 0xB4 bytes, not three UTF-8 promotions (oracle-verified).
        let (src, enc) = {
            let g = recv_str!(recv).lock();
            (g.bytes().to_vec(), g.encoding())
        };
        // Guard the RESULT size before allocating. Without this, `"x" * (1 <<
        // 60)` hands the allocator a 2^60-byte request and the process ABORTS
        // -- an uncatchable failure, strictly worse than any exception.
        //
        // Divergence from CRuby, deliberate: `rb_str_times` (string.c:2591)
        // only guards the multiplication itself (`LONG_MAX/len <
        // RSTRING_LEN(str)` -> ArgumentError "argument too big"), which does
        // NOT trip for a 1-byte string times 2^60 -- so real Ruby reaches the
        // allocator here and raises NoMemoryError, a memory-dependent outcome.
        // The corpus pins the deterministic ArgumentError instead (its
        // expectation is checked in rather than oracle-generated, and its
        // header cites the segfault this replaced), so cap on total size.
        match src.len().checked_mul(n as usize) {
            Some(total) if total <= MAX_STRING_SIZE => Ok(RubyValue::Str(
                crate::collections::string_from_bytes(src.repeat(n as usize), enc),
            )),
            _ => Err(arg_error!("string size too big")),
        }
    }
    "to_s"[0] | "to_str"[0] => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "<=>"[1] => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(other) = &args[0] else {
            return Ok(RubyValue::Nil);
        };
        let ord = {
            let a = recv_str!(recv).lock();
            let b = other.lock();
            a.to_utf8_lossy().cmp(&b.to_utf8_lossy())
        };
        Ok(RubyValue::Int(ord as i64))
    }
    "=="[1] | "eql?"[1] => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    // Casing is encoding-aware (`StrBuf::*cased`): full Unicode for UTF-8
    // (unchanged), ASCII-only for BINARY/US-ASCII, Latin-1's own case map for
    // ISO-8859-1 -- and the result keeps the receiver's encoding.
    "upcase" => fn upcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_wrap(recv_str!(recv).lock().upcased())))
    }
    "downcase" => fn downcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_wrap(recv_str!(recv).lock().downcased())))
    }
    "capitalize" => fn capitalize(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_wrap(recv_str!(recv).lock().capitalized())))
    }
    "swapcase" => fn swapcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_wrap(recv_str!(recv).lock().swapcased())))
    }
    "strip" => fn strip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().to_utf8_lossy().trim_matches(is_rb_strip).to_string()))
    }
    "lstrip" => fn lstrip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().to_utf8_lossy().trim_start_matches(is_rb_strip).to_string()))
    }
    "rstrip" => fn rstrip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().to_utf8_lossy().trim_end_matches(is_rb_strip).to_string()))
    }
    "chars"[0] => fn chars(recv, args, _block) {
        arity!(args, 0);
        let out = recv_str!(recv)
            .lock()
            .chars()
            .map(|c| str_value(c.to_string()))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `lines` keeps each separator (`["a\n", "b\n", "c"]`).
    // `lines(sep = "\n", chomp: false)` -- split into lines, keeping the
    // separator unless `chomp:` strips it.
    "lines" => fn lines(recv, args, _block) {
        arity!(args, 0..=2);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Array(crate::array_new(lines_from_args(&text, args))))
    }
    "each_char"[0] => fn each_char(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_char", args, block);
        let cs: Vec<char> = recv_str!(recv).lock().chars().collect();
        for c in cs {
            p.call(&[str_value(c.to_string())])?;
        }
        Ok(recv.clone())
    }
    // `grapheme_clusters`: the string split into extended grapheme clusters
    // (UAX #29) -- a base char plus its combining marks, a regional-indicator
    // flag pair, or a ZWJ emoji sequence each count as one.
    "grapheme_clusters"[0] => fn grapheme_clusters(recv, args, _block) {
        arity!(args, 0);
        use unicode_segmentation::UnicodeSegmentation;
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let out = text.graphemes(true).map(|g| str_value(g.to_string())).collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "each_grapheme_cluster"[0] => fn each_grapheme_cluster(recv, args, block) {
        arity!(args, 0);
        use unicode_segmentation::UnicodeSegmentation;
        let p = block_or_enum!(recv, "each_grapheme_cluster", args, block);
        let clusters: Vec<String> = recv_str!(recv)
            .lock()
            .to_utf8_lossy()
            .graphemes(true)
            .map(str::to_string)
            .collect();
        for g in clusters {
            p.call(&[str_value(g)])?;
        }
        Ok(recv.clone())
    }
    // `unicode_normalize(form = :nfc)`: NFC/NFD/NFKC/NFKD normalization;
    // `unicode_normalized?(form = :nfc)` tests whether the receiver already is
    // in that form. An unknown form raises ArgumentError.
    "unicode_normalize" => fn unicode_normalize(recv, args, _block) {
        arity!(args, 0..=1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        Ok(str_value(normalize_form(&text, args.first())?))
    }
    "unicode_normalized?" => fn unicode_normalized_p(recv, args, _block) {
        arity!(args, 0..=1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Bool(text == normalize_form(&text, args.first())?))
    }
    // `each_line` / `each_line(sep)`: a custom separator keeps its trailing
    // occurrence on each piece, exactly like the default `"\n"`.
    "each_line" => fn each_line(recv, args, block) {
        arity!(args, 0..=2);
        let p = block_or_enum!(recv, "each_line", args, block);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        for l in lines_from_args(&text, args) {
            p.call(&[l])?;
        }
        Ok(recv.clone())
    }
    // `split`: no-arg/nil = whitespace runs (leading skipped); a String
    // separator keeps interior empties; a Regexp separator delegates to the
    // shared regexp splitter. The optional `limit` caps the field count
    // (`> 0`, tail kept whole), keeps trailing empties (`< 0`), or drops
    // them (`0`/omitted).
    "split" => fn split(recv, args, block) {
        arity!(args, 0..=2);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        // An explicit nil limit RAISES in CRuby (`NUM2LONG(nil)`), unlike the
        // nil separator, which selects awk mode.
        let limit = match args.get(1) {
            None => 0,
            Some(v) => convert::to_index(v)?,
        };
        // Separator: nil/absent = awk mode; Regexp as-is; anything else
        // through the `to_str` probe (CRuby's get_pat_quoted), a
        // non-convertible one raising the expected-Regexp shape.
        let sep_arg = match args.first() {
            None | Some(RubyValue::Nil) => None,
            Some(re @ RubyValue::Regexp(_)) => Some(re.clone()),
            Some(other) => match convert::check_to_str(other)? {
                Some(s) => Some(s),
                None => return Err(type_error!("wrong argument type {} (expected Regexp)",
                        crate::builtins::class_name_of(other))),
            },
        };
        let result = match &sep_arg {
            None => str_array(awk_split(&text, limit)),
            Some(RubyValue::Str(sep)) => {
                let sep = sep.lock().to_utf8_lossy().into_owned();
                let parts: Vec<String> = if sep == " " {
                    // The one magic separator: a single space means
                    // whitespace-run splitting, real Ruby's awk rule.
                    awk_split(&text, limit)
                } else if sep.is_empty() {
                    // An empty separator splits into characters (Rust's own
                    // `split("")` would emit spurious leading/trailing empties).
                    let chars: Vec<char> = text.chars().collect();
                    if limit > 0 && (limit as usize) < chars.len() {
                        let head = limit as usize - 1;
                        let mut v: Vec<String> = chars[..head].iter().map(|c| c.to_string()).collect();
                        v.push(chars[head..].iter().collect());
                        v
                    } else {
                        chars.iter().map(|c| c.to_string()).collect()
                    }
                } else {
                    let mut parts: Vec<String> = if limit > 0 {
                        text.splitn(limit as usize, sep.as_str()).map(str::to_string).collect()
                    } else {
                        text.split(sep.as_str()).map(str::to_string).collect()
                    };
                    if limit == 0 {
                        while parts.last().is_some_and(|s| s.is_empty()) {
                            parts.pop();
                        }
                    }
                    parts
                };
                str_array(parts)
            }
            Some(RubyValue::Regexp(re)) => crate::regexp_split(re, &text, limit),
            Some(_) => unreachable!("split separator normalized to Str/Regexp above"),
        };
        // The block form yields each field and answers the RECEIVER, not the
        // array (CRuby's `rb_str_split_m`).
        if let Some(blk) = &block {
            if let RubyValue::Array(a) = &result {
                let pieces: Vec<RubyValue> = a.lock().iter().cloned().collect();
                for piece in pieces {
                    crate::dispatch::send_value(blk, crate::Symbol::intern("call"), &[piece], None)?;
                }
            }
            return Ok(recv.clone());
        }
        Ok(result)
    }
    "chomp" => fn chomp(recv, args, _block) {
        arity!(args, 0..=1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let out = match args.first() {
            // `chomp("")` is paragraph mode: strip EVERY trailing newline record
            // (`\n`/`\r\n`), but keep a lone trailing `\r` (CRuby's rb_str_chomp).
            Some(RubyValue::Str(suffix)) if suffix.lock().to_utf8_lossy().is_empty() => {
                let mut t = text.as_str();
                while let Some(rest) = t.strip_suffix('\n') {
                    t = rest.strip_suffix('\r').unwrap_or(rest);
                }
                t.to_string()
            }
            Some(RubyValue::Str(suffix)) => {
                let suffix = suffix.lock().to_utf8_lossy().into_owned();
                text.strip_suffix(&suffix).unwrap_or(&text).to_string()
            }
            _ => text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'))
                .or_else(|| text.strip_suffix('\r'))
                .unwrap_or(&text)
                .to_string(),
        };
        Ok(str_value(out))
    }
    "chop"[0] => fn chop(recv, args, _block) {
        arity!(args, 0);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let mut cs: Vec<char> = text.chars().collect();
        if text.ends_with("\r\n") {
            cs.truncate(cs.len() - 2);
        } else {
            cs.pop();
        }
        Ok(str_value(cs.into_iter().collect()))
    }
    "reverse"[0] => fn reverse(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_wrap(recv_str!(recv).lock().reversed())))
    }
    // In-place `chomp`/`chop`: reuse the same trailing-separator logic, then
    // route through the shared bang mutator (frozen guard, nil when nothing
    // changed).
    "chomp!" => fn chomp_bang(recv, args, _block) {
        arity!(args, 0..=1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let out = match args.first() {
            // `chomp("")` is paragraph mode: strip EVERY trailing newline record
            // (`\n`/`\r\n`), but keep a lone trailing `\r` (CRuby's rb_str_chomp).
            Some(RubyValue::Str(suffix)) if suffix.lock().to_utf8_lossy().is_empty() => {
                let mut t = text.as_str();
                while let Some(rest) = t.strip_suffix('\n') {
                    t = rest.strip_suffix('\r').unwrap_or(rest);
                }
                t.to_string()
            }
            Some(RubyValue::Str(suffix)) => {
                let suffix = suffix.lock().to_utf8_lossy().into_owned();
                text.strip_suffix(&suffix).unwrap_or(&text).to_string()
            }
            _ => text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'))
                .or_else(|| text.strip_suffix('\r'))
                .unwrap_or(&text)
                .to_string(),
        };
        str_bang_replace(recv, out)
    }
    "chop!"[0] => fn chop_bang(recv, args, _block) {
        arity!(args, 0);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let mut cs: Vec<char> = text.chars().collect();
        if text.ends_with("\r\n") {
            cs.truncate(cs.len().saturating_sub(2));
        } else {
            cs.pop();
        }
        str_bang_replace(recv, cs.into_iter().collect())
    }
    // The transform `!` mutators: each reuses its non-bang sibling and writes
    // the result back (nil when unchanged). Grouped here so the whole family
    // stays one delegation path.
    "upcase!" => fn upcase_bang(recv, args, block) { str_bang_via(recv, "upcase", args, block) }
    "downcase!" => fn downcase_bang(recv, args, block) { str_bang_via(recv, "downcase", args, block) }
    "capitalize!" => fn capitalize_bang(recv, args, block) { str_bang_via(recv, "capitalize", args, block) }
    "swapcase!" => fn swapcase_bang(recv, args, block) { str_bang_via(recv, "swapcase", args, block) }
    "reverse!"[0] => fn reverse_bang(recv, args, block) { str_bang_via(recv, "reverse", args, block) }
    "strip!" => fn strip_bang(recv, args, block) { str_bang_via(recv, "strip", args, block) }
    "lstrip!" => fn lstrip_bang(recv, args, block) { str_bang_via(recv, "lstrip", args, block) }
    "rstrip!" => fn rstrip_bang(recv, args, block) { str_bang_via(recv, "rstrip", args, block) }
    "sub!" => fn sub_bang(recv, args, block) { str_bang_via(recv, "sub", args, block) }
    "gsub!" => fn gsub_bang(recv, args, block) { str_bang_via(recv, "gsub", args, block) }
    "tr!"[2] => fn tr_bang(recv, args, block) { str_bang_via(recv, "tr", args, block) }
    "delete!" => fn delete_bang(recv, args, block) { str_bang_via(recv, "delete", args, block) }
    "squeeze!" => fn squeeze_bang(recv, args, block) { str_bang_via(recv, "squeeze", args, block) }
    "succ!"[0] | "next!"[0] => fn succ_bang(recv, args, block) { str_bang_via(recv, "succ", args, block) }
    // `sum` -- the CRuby checksum: the sum of the byte values, masked to `bits`
    // (default 16) bits. `chr` is the first character as a one-char String.
    "sum" => fn sum(recv, args, _block) {
        arity!(args, 0..=1);
        let bits = match args.first() {
            None => 16,
            Some(_) => arg_int!(args, 0),
        };
        let total: i64 = recv_str!(recv).lock().bytes().iter().map(|&b| b as i64).sum();
        let masked = if (1..64).contains(&bits) {
            total & ((1i64 << bits) - 1)
        } else {
            total
        };
        Ok(RubyValue::Int(masked))
    }
    "chr"[0] => fn chr(recv, args, _block) {
        arity!(args, 0);
        let first: String = recv_str!(recv).lock().to_utf8_lossy().chars().take(1).collect();
        Ok(str_value(first))
    }
    // Empties the string in place (frozen guard); always answers the receiver.
    "clear"[0] => fn clear(recv, args, _block) {
        arity!(args, 0);
        let s = recv_str!(recv);
        if s.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen String: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        s.lock().replace_utf8(String::new());
        Ok(recv.clone())
    }
    // The integer codepoints of each character (`each_codepoint` is the
    // block/enumerator form over the same values).
    "codepoints"[0] => fn codepoints(recv, args, _block) {
        arity!(args, 0);
        let out = recv_str!(recv)
            .lock()
            .to_utf8_lossy()
            .chars()
            .map(|c| RubyValue::Int(c as i64))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "each_codepoint"[0] => fn each_codepoint(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_codepoint", args, block);
        for c in recv_str!(recv).lock().to_utf8_lossy().chars() {
            p.call(&[RubyValue::Int(c as i64)])?;
        }
        Ok(recv.clone())
    }
    // `[before, sep, after]` around the first (`partition`) / last
    // (`rpartition`) occurrence of a String or Regexp separator.
    "partition"[1] => fn partition(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Array(crate::array_new(str_partition(&text, &args[0], false)?.to_vec())))
    }
    "rpartition"[1] => fn rpartition(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Array(crate::array_new(str_partition(&text, &args[0], true)?.to_vec())))
    }
    // Prefix/suffix removal -- the non-bang form always returns a new String
    // (a copy when the affix is absent); the bang form mutates and answers
    // `nil` when there was nothing to remove.
    "delete_prefix"[1] => fn delete_prefix(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let prefix = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        Ok(str_value(text.strip_prefix(&prefix).unwrap_or(&text).to_string()))
    }
    "delete_prefix!"[1] => fn delete_prefix_bang(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let prefix = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        str_bang_replace(recv, text.strip_prefix(&prefix).unwrap_or(&text).to_string())
    }
    "delete_suffix"[1] => fn delete_suffix(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let suffix = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        Ok(str_value(text.strip_suffix(&suffix).unwrap_or(&text).to_string()))
    }
    "delete_suffix!"[1] => fn delete_suffix_bang(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let suffix = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        str_bang_replace(recv, text.strip_suffix(&suffix).unwrap_or(&text).to_string())
    }
    // `+@`: an unfrozen receiver is returned as-is; a frozen one yields a
    // fresh mutable copy (the mirror of `-@`'s "freeze/dedup").
    "+@"[0] => fn plus_at(recv, args, _block) {
        arity!(args, 0);
        let s = recv_str!(recv);
        if !s.is_frozen() {
            return Ok(recv.clone());
        }
        let (bytes, enc) = {
            let buf = s.lock();
            (buf.bytes().to_vec(), buf.encoding())
        };
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, enc)))
    }
    // `bytesplice(index, length, str)` / `bytesplice(range, str)`: replaces
    // the byte span in place with `str`'s bytes and answers `str`. A frozen
    // receiver raises. (The 5-arg `str`-sub-span form is a separate gap.)
    "bytesplice" => fn bytesplice(recv, args, _block) {
        arity!(args, 2..=3);
        let s = recv_str!(recv);
        if s.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen String: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        let total = s.lock().bytesize() as i64;
        // CRuby's discrimination: 2 args -- or 3 with a non-Integer first --
        // is the Range form (a non-Range first argument raises the
        // expected-Range shape); otherwise (index, length, str), both
        // NUM2LONG. The replacement converts via `to_str` in every form.
        let is_range_form = args.len() == 2
            || !matches!(&args[0], RubyValue::Int(_) | RubyValue::BigInt(_));
        let (start, len, repl) = if is_range_form {
            let RubyValue::Range(begin, end, exclusive) = &args[0] else {
                return Err(type_error!("wrong argument type {} (expected Range)",
                        crate::builtins::class_name_of(&args[0])));
            };
            let start = match begin.as_deref() {
                Some(v) => {
                    let v = convert::to_index(v)?;
                    if v < 0 { v + total } else { v }
                }
                None => 0,
            };
            let end_i = match end.as_deref() {
                Some(v) => {
                    let v = convert::to_index(v)?;
                    let v = if v < 0 { v + total } else { v };
                    if *exclusive { v } else { v + 1 }
                }
                None => total,
            };
            (start, (end_i - start).max(0), convert::to_rstr(&args[1])?)
        } else {
            let start = convert::to_index(&args[0])?;
            let start = if start < 0 { start + total } else { start };
            (start, convert::to_index(&args[1])?, convert::to_rstr(&args[2])?)
        };
        if start < 0 || start > total || len < 0 {
            return Err(index_error!("index {start} out of string"));
        }
        let start = start as usize;
        let end = (start + len as usize).min(total as usize);
        let (mut bytes, enc) = { let buf = s.lock(); (buf.bytes().to_vec(), buf.encoding()) };
        let repl_bytes = repl.lock().bytes().to_vec();
        bytes.splice(start..end, repl_bytes);
        s.lock().replace_bytes(bytes, enc);
        Ok(RubyValue::Str(repl))
    }
    "index" => fn index(recv, args, _block) {
        // `index(substr_or_regexp[, start])` -- the optional start is a CHAR
        // offset (from the end when negative) to begin searching at.
        arity!(args, 1..=2);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let clen = text.chars().count() as i64;
        let start_char = match args.get(1) {
            Some(v) => {
                let mut p = convert::to_index(v)?;
                if p < 0 {
                    p += clen;
                }
                if p < 0 || p > clen {
                    return Ok(RubyValue::Nil);
                }
                p as usize
            }
            None => 0,
        };
        let byte_start = text
            .char_indices()
            .nth(start_char)
            .map(|(b, _)| b)
            .unwrap_or(text.len());
        match &args[0] {
            RubyValue::Regexp(re) => match crate::regexp_match_index(re, &text[byte_start..]) {
                RubyValue::Int(i) => Ok(RubyValue::Int(i + start_char as i64)),
                other => Ok(other),
            },
            other => {
                let needle = convert::to_rstr(other)?.lock().to_utf8_lossy().into_owned();
                Ok(match text[byte_start..].find(&needle) {
                    Some(byte_pos) => {
                        RubyValue::Int(text[..byte_start + byte_pos].chars().count() as i64)
                    }
                    None => RubyValue::Nil,
                })
            }
        }
    }
    // `rindex(str_or_regexp[, pos])`: the CHAR index of the LAST match whose
    // start is at or before `pos` (end-relative when negative; the whole
    // string when omitted), or nil.
    "rindex" => fn rindex(recv, args, _block) {
        arity!(args, 1..=2);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let clen = text.chars().count() as i64;
        let before = match args.get(1) {
            Some(v) => {
                let p = convert::to_index(v)?;
                let p = if p < 0 { p + clen } else { p };
                if p < 0 {
                    return Ok(RubyValue::Nil);
                }
                Some((p.min(clen)) as usize)
            }
            None => None,
        };
        match &args[0] {
            RubyValue::Regexp(re) => Ok(crate::regexp_rindex(re, &text, before)),
            other => {
                let needle = convert::to_rstr(other)?.lock().to_utf8_lossy().into_owned();
                // Search only within the prefix up to (and including a needle
                // starting at) `before`.
                let cutoff = before.map_or(text.len(), |p| {
                    byte_at_char(&text, p) + needle.len()
                });
                Ok(match text[..cutoff.min(text.len())].rfind(&needle) {
                    Some(byte_pos) => RubyValue::Int(text[..byte_pos].chars().count() as i64),
                    None => RubyValue::Nil,
                })
            }
        }
    }
    // The `[]`/`slice` forms: Int, (Int, Int), Range, String -- all
    // char-indexed and encoding-preserving (a substring of a BINARY string
    // stays BINARY; a UTF-8 multibyte char is one index).
    "[]" | "slice" => fn index_op(recv, args, _block) {
        arity!(args, 1..=2);
        // Regexp indexing: `s[/re/]` is the whole match; `s[/re/, n]`/`s[/re/,
        // :name]` is that capture group (nil when the pattern doesn't match).
        if let RubyValue::Regexp(re) = &args[0] {
            let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
            return regexp_index(re, &text, args.get(1));
        }
        let s = recv_str!(recv).lock();
        let n = s.char_len() as i64;
        let wrap = |buf: Option<crate::encoding::StrBuf>| match buf {
            Some(b) => RubyValue::Str(crate::string_wrap(b)),
            None => RubyValue::Nil,
        };
        if args.len() == 2 {
            let (start, len) = (arg_int!(args, 0), arg_int!(args, 1));
            return Ok(wrap(s.char_substr(start, len)));
        }
        match &args[0] {
            RubyValue::Range(start, end, exclusive) => {
                let start_i = match start.as_deref() {
                    Some(RubyValue::Int(v)) => if *v < 0 { v + n } else { *v },
                    None => 0,
                    _ => return Ok(RubyValue::Nil),
                };
                let end_i = match end.as_deref() {
                    Some(RubyValue::Int(v)) => {
                        let v = if *v < 0 { v + n } else { *v };
                        if *exclusive { v - 1 } else { v }
                    }
                    None => n - 1,
                    _ => return Ok(RubyValue::Nil),
                };
                Ok(wrap(s.char_substr(start_i, (end_i - start_i + 1).max(0))))
            }
            RubyValue::Str(sub) => {
                let needle = sub.lock().to_utf8_lossy().into_owned();
                Ok(if s.to_utf8_lossy().contains(&needle) {
                    str_value(needle)
                } else {
                    RubyValue::Nil
                })
            }
            other => Ok(wrap(s.char_at(convert::to_index(other)?))),
        }
    }
    // `[]=`: index assignment across the same shapes as `[]`/`slice` --
    // `s[i] = v`, `s[i, len] = v`, `s[range] = v`, `s[substr] = v`,
    // `s[/re/[, group]] = v`. Replaces the matched span with `v` and answers
    // `v` (Ruby's index-assignment expression value). Out-of-range integers/
    // substrings raise IndexError; an out-of-range range begin raises
    // RangeError.
    "[]=" => fn index_set(recv, args, _block) {
        arity!(args, 2..=3);
        index_set_impl(recv, args)
    }
    // `casecmp` is an ASCII case-insensitive `<=>`; `casecmp?` its boolean
    // (Unicode-aware) sibling. A non-String argument answers nil.
    "casecmp"[1] => fn casecmp(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(o) = &args[0] else { return Ok(RubyValue::Nil) };
        let a = recv_str!(recv).lock().to_utf8_lossy().to_lowercase();
        let b = o.lock().to_utf8_lossy().to_lowercase();
        Ok(RubyValue::Int(a.cmp(&b) as i64))
    }
    "casecmp?"[1] => fn casecmp_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(o) = &args[0] else { return Ok(RubyValue::Nil) };
        let a = recv_str!(recv).lock().to_utf8_lossy().to_lowercase();
        let b = o.lock().to_utf8_lossy().to_lowercase();
        Ok(RubyValue::Bool(a == b))
    }
    // `oct`/`hex` parse a leading integer in base 8/16, honoring an explicit
    // `0x`/`0b`/`0o`/`0d` radix prefix and stopping at the first invalid
    // digit (0 when there is none) -- CRuby's lenient rule.
    "oct"[0] => fn oct(recv, args, _block) {
        arity!(args, 0);
        Ok(parse_int_lenient(&recv_str!(recv).lock().to_utf8_lossy(), 8))
    }
    "hex"[0] => fn hex(recv, args, _block) {
        arity!(args, 0);
        Ok(parse_int_lenient(&recv_str!(recv).lock().to_utf8_lossy(), 16))
    }
    // `slice!(index[, len])` / `slice!(range)` / `slice!(substring)`: removes
    // the matched portion from the receiver IN PLACE and returns it (nil when
    // nothing matched).
    "slice!" => fn slice_bang(recv, args, _block) {
        arity!(args, 1..=2);
        guard_str_frozen(recv)?;
        slice_bang_impl(recv, args)
    }
    // `sub`/`gsub`: String or Regexp pattern; String replacement or block.
    "sub" => fn sub(recv, args, block) {
        sub_gsub(recv, args, block, false)
    }
    "gsub" => fn gsub(recv, args, block) {
        sub_gsub(recv, args, block, true)
    }
    "start_with?" => fn start_with_p(recv, args, _block) {
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        for a in args {
            match a {
                // A Regexp prefix matches only when it matches anchored at the
                // start; CRuby sets `$~` to the match (nil on no start-match).
                RubyValue::Regexp(re) => {
                    if crate::regexp::regexp_anchored_len(re, &text).is_some() {
                        crate::regexp_match(re, &text);
                        return Ok(RubyValue::Bool(true));
                    }
                    crate::lastmatch::set_last_match(None);
                }
                other => {
                    let prefix = convert::to_rstr(other)?;
                    if text.starts_with(&*prefix.lock().to_utf8_lossy()) {
                        return Ok(RubyValue::Bool(true));
                    }
                }
            }
        }
        Ok(RubyValue::Bool(false))
    }
    "end_with?" => fn end_with_p(recv, args, _block) {
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        for a in args {
            let suffix = &convert::to_rstr(a)?;
            if text.ends_with(&*suffix.lock().to_utf8_lossy()) {
                return Ok(RubyValue::Bool(true));
            }
        }
        Ok(RubyValue::Bool(false))
    }
    // `tr(from, to)` with `a-z` range expansion; a short `to` repeats its
    // last character (CRuby's rule).
    "tr"[2] => fn tr(recv, args, _block) {
        arity!(args, 2);
        let (from, from_neg) = tr_charset(&arg_str!(args, 0).lock().to_utf8_lossy());
        let to = expand_charset(&arg_str!(args, 1).lock().to_utf8_lossy());
        let out = recv_str!(recv)
            .lock()
            .chars()
            // A negated `from` (`tr("^a", "x")`) maps every char OUTSIDE the set
            // to the last `to` char. Otherwise a char repeated in `from` takes
            // its LAST mapping (`rposition`), matching CRuby (`"_".tr("___",
            // ".+-") == "-"`), and a `to` shorter than `from` repeats its final
            // char.
            .map(|c| {
                if from_neg {
                    if from.contains(&c) { c } else { to.last().copied().unwrap_or(c) }
                } else {
                    match from.iter().rposition(|&f| f == c) {
                        Some(i) => *to.get(i).or(to.last()).unwrap_or(&c),
                        None => c,
                    }
                }
            })
            .collect();
        Ok(str_value(out))
    }
    // `tr_s(from, to)` -- like `tr`, but each RUN of a translated character
    // collapses to one (`"hello".tr_s("l","r") == "hero"`). Delegating to
    // `tr` then `squeeze(to)` reproduces this: only the `to` characters are
    // squeezed, and an empty `to` (a delete) squeezes nothing.
    "tr_s"[2] => fn tr_s(recv, args, _block) {
        arity!(args, 2);
        let translated = crate::dispatch::send_value(recv, crate::Symbol::intern("tr"), args, None)?;
        crate::dispatch::send_value(
            &translated,
            crate::Symbol::intern("squeeze"),
            std::slice::from_ref(&args[1]),
            None,
        )
    }
    "tr_s!"[2] => fn tr_s_bang(recv, args, block) { str_bang_via(recv, "tr_s", args, block) }
    // `delete`/`count` take ONE OR MORE char-set specs; a char is selected
    // only when it satisfies EVERY spec (CRuby's intersection rule), each of
    // which may itself be negated with a leading `^` or use `a-z` ranges.
    "delete" => fn delete(recv, args, _block) {
        let sets = charset_specs(args)?;
        let out = recv_str!(recv)
            .lock()
            .chars()
            .filter(|c| !in_all_charsets(*c, &sets))
            .collect();
        Ok(str_value(out))
    }
    "squeeze" => fn squeeze(recv, args, _block) {
        arity!(args, 0..=1);
        let set = args
            .first()
            .map(|a| match a {
                RubyValue::Str(s) => tr_charset(&s.lock().to_utf8_lossy()),
                _ => (Vec::new(), false),
            });
        let mut out = String::new();
        let mut prev: Option<char> = None;
        for c in recv_str!(recv).lock().chars() {
            let squeezable = set
                .as_ref()
                .is_none_or(|(s, neg)| s.contains(&c) != *neg);
            if prev == Some(c) && squeezable {
                continue;
            }
            out.push(c);
            prev = Some(c);
        }
        Ok(str_value(out))
    }
    "count" => fn count(recv, args, _block) {
        let sets = charset_specs(args)?;
        let n = recv_str!(recv)
            .lock()
            .chars()
            .filter(|c| in_all_charsets(*c, &sets))
            .count();
        Ok(RubyValue::Int(n as i64))
    }
    "to_i" => fn to_i(recv, args, _block) {
        arity!(args, 0..=1);
        let base = match args.first() {
            None => 10,
            Some(v) => match convert::to_index(v)? {
                // Base 0 auto-detects from the literal's prefix (`0x`/`0b`/
                // `0o`/a bare leading `0` = octal), via the prefix-aware
                // lenient parser.
                0 => {
                    return Ok(parse_int_lenient(&recv_str!(recv).lock().to_utf8_lossy(), 0));
                }
                b @ 2..=36 => b as u32,
                b => return Err(arg_error!("invalid radix {b}")),
            },
        };
        Ok(lenient_to_i(&recv_str!(recv).lock().to_utf8_lossy(), base))
    }
    // Lenient like `to_i`: the longest valid leading float, else 0.0.
    "to_f"[0] => fn to_f(recv, args, _block) {
        arity!(args, 0);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        // CRuby ignores a single underscore between two digits (`"1_000.5"` ->
        // 1000.5); a leading, trailing, or doubled underscore stops the parse.
        let cleaned: String = {
            let chars: Vec<char> = text.trim_start().chars().collect();
            chars
                .iter()
                .enumerate()
                .filter(|&(ref i, &c)| {
                    !(c == '_'
                        && *i > 0
                        && chars[i - 1].is_ascii_digit()
                        && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()))
                })
                .map(|(_, &c)| c)
                .collect()
        };
        let t = cleaned.as_str();
        let mut end = 0;
        for (i, _) in t.char_indices() {
            let candidate = &t[..=i + t[i..].chars().next().map_or(0, |c| c.len_utf8() - 1)];
            if candidate.parse::<f64>().is_ok()
                || candidate == "-"
                || candidate == "+"
                || candidate.ends_with(['e', 'E'])
                || candidate.ends_with("e-")
                || candidate.ends_with("e+")
            {
                end = candidate.len();
            } else {
                break;
            }
        }
        Ok(RubyValue::Float(
            t[..end].trim_end_matches(['e', 'E', '-', '+']).parse().unwrap_or(0.0),
        ))
    }
    // `to_r` -- the leading rational (`"3/4"`, `"1.5"`, `"12"`); junk with no
    // leading digits is `(0/1)`. Shares the parser with the `Rational` code.
    "to_r"[0] => fn to_r(recv, args, _block) {
        arity!(args, 0);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let (num, den) = crate::builtins::rational::parse_str_to_r(&text);
        crate::builtins::rational::rational_new(num, den)
    }
    "to_c"[0] => fn to_c(recv, args, _block) {
        arity!(args, 0);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        crate::builtins::complex::parse_str_to_c(&text)
    }
    "dump"[0] => fn dump(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(dump_str(&recv_str!(recv).lock())))
    }
    "undump"[0] => fn undump(recv, args, _block) {
        arity!(args, 0);
        undump_str(&recv_str!(recv).lock())
    }
    "to_sym"[0] | "intern"[0] => fn to_sym(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(crate::Symbol::intern(
            &recv_str!(recv).lock().to_utf8_lossy(),
        )))
    }
    "succ"[0] | "next"[0] => fn succ(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(succ_str(&recv_str!(recv).lock().to_utf8_lossy())))
    }
    // `upto(other[, exclusive])` -- yields successive `succ` values from self
    // through `other` (excluding `other` when `exclusive`); a blockless call
    // answers an Enumerator. Stops once a value grows past `other`.
    "upto" => fn upto(recv, args, block) {
        arity!(args, 1..=2);
        let exclusive = args.get(1).is_some_and(|v| v.truthy());
        let limit = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        let p = block_or_enum!(recv, "upto", args, block);
        let mut cur = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        loop {
            if cur.as_str() > limit.as_str() {
                break;
            }
            if exclusive && cur == limit {
                break;
            }
            p.call(&[str_value(cur.clone())])?;
            if !exclusive && cur == limit {
                break;
            }
            cur = succ_str(&cur);
            if cur.len() > limit.len() {
                break;
            }
        }
        Ok(recv.clone())
    }
    "center" => fn center(recv, args, _block) {
        arity!(args, 1..=2)
        ;
        pad(recv, args, Pad::Center)
    }
    "ljust" => fn ljust(recv, args, _block) {
        arity!(args, 1..=2);
        pad(recv, args, Pad::Left)
    }
    "rjust" => fn rjust(recv, args, _block) {
        arity!(args, 1..=2);
        pad(recv, args, Pad::Right)
    }
    "insert"[2] => fn insert(recv, args, _block) {
        arity!(args, 2);
        guard_str_frozen(recv)?;
        let at = arg_int!(args, 0);
        let addition = arg_str!(args, 1).lock().to_utf8_lossy().into_owned();
        let handle = recv_str!(recv);
        let mut guard = handle.lock();
        let n = guard.char_len() as i64;
        let at = if at < 0 { at + n + 1 } else { at };
        if at < 0 || at > n {
            return Err(index_error!("index {} out of string", arg_int!(args, 0)));
        }
        let mut txt = guard.to_utf8_lossy().into_owned();
        let byte_pos = txt
            .char_indices()
            .nth(at as usize)
            .map_or(txt.len(), |(b, _)| b);
        txt.insert_str(byte_pos, &addition);
        guard.replace_utf8(txt);
        drop(guard);
        Ok(recv.clone())
    }
    // `prepend(*strs)` -- insert every argument, in order, at the front.
    "prepend" => fn prepend(recv, args, _block) {
        guard_str_frozen(recv)?;
        let mut prefix = String::new();
        for a in args {
            prefix.push_str(&convert::to_rstr(a)?.lock().to_utf8_lossy());
        }
        let handle = recv_str!(recv);
        let mut guard = handle.lock();
        let mut txt = guard.to_utf8_lossy().into_owned();
        txt.insert_str(0, &prefix);
        guard.replace_utf8(txt);
        drop(guard);
        Ok(recv.clone())
    }
    "replace"[1] => fn replace(recv, args, _block) {
        arity!(args, 1);
        guard_str_frozen(recv)?;
        let new_text = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        recv_str!(recv).lock().replace_utf8(new_text);
        Ok(recv.clone())
    }
    // `Integer#chr`'s inverse -- the first character's codepoint.
    "ord"[0] => fn ord(recv, args, _block) {
        arity!(args, 0);
        match recv_str!(recv).lock().chars().next() {
            Some(c) => Ok(RubyValue::Int(c as i64)),
            None => Err(arg_error!("empty string")),
        }
    }
    // `"%s..." % args` -- the shared sprintf engine (`builtins::format`).
    "%"[1] => fn format_op(recv, args, _block) {
        arity!(args, 1);
        let format_args = match &args[0] {
            RubyValue::Array(a) => a.lock().clone(),
            other => vec![other.clone()],
        };
        Ok(str_value(crate::builtins::format::sprintf(
            &recv_str!(recv).lock().to_utf8_lossy(),
            &format_args,
        )?))
    }
    "=~"[1] => fn match_op(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Regexp(re) => {
                Ok(crate::regexp_match_index(re, &recv_str!(recv).lock().to_utf8_lossy()))
            }
            other => Err(type_error!("wrong argument type {} (expected Regexp)",
                    crate::builtins::class_name_of(other))),
        }
    }
    // Both accept an optional start position (char offset, end-relative when
    // negative); a position outside the string means "no match" without even
    // running the engine.
    "match" => fn match_m(recv, args, block) {
        arity!(args, 1..=2);
        let re = to_regexp(&args[0])?;
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let md = match match_haystack(&text, args.get(1))? {
            Some(h) => crate::regexp_match(&re, &h),
            None => RubyValue::Nil,
        };
        // The block form runs on a match, answering the block's value; a miss
        // answers nil without yielding (CRuby's rb_str_match_m).
        match (&block, &md) {
            (Some(blk), md) if !md.is_nil() => {
                crate::dispatch::send_value(blk, crate::Symbol::intern("call"), std::slice::from_ref(md), None)
            }
            _ => Ok(md),
        }
    }
    "match?" => fn match_p(recv, args, _block) {
        arity!(args, 1..=2);
        let re = to_regexp(&args[0])?;
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Bool(match match_haystack(&text, args.get(1))? {
            Some(h) => crate::regexp_is_match(&re, &h),
            None => false,
        }))
    }
    "scan"[1] => fn scan(recv, args, block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        // The matched substrings, in order. A Regexp defers to the engine; a
        // String pattern matches LITERALLY (its characters are never
        // metacharacters), non-overlapping and left to right -- and an empty
        // pattern matches at every character boundary, both ends included.
        let matches: Vec<RubyValue> = match &args[0] {
            RubyValue::Regexp(re) => match crate::regexp_scan(re, &text) {
                RubyValue::Array(a) => a.lock().iter().cloned().collect(),
                _ => Vec::new(),
            },
            RubyValue::Str(pat) => {
                let pat = pat.lock().to_utf8_lossy().into_owned();
                let mut out = Vec::new();
                if pat.is_empty() {
                    for _ in 0..=text.chars().count() {
                        out.push(str_value(String::new()));
                    }
                } else {
                    let mut start = 0;
                    while let Some(pos) = text[start..].find(&pat) {
                        out.push(str_value(pat.clone()));
                        start += pos + pat.len();
                    }
                }
                out
            }
            other => {
                return Err(type_error!("wrong argument type {} (expected Regexp)",
                        crate::builtins::class_name_of(other)));
            }
        };
        // With a block, yield each match and return the receiver; without one,
        // return the array of matches (never an Enumerator -- CRuby's `scan`
        // has no block-less lazy form).
        match &block {
            Some(RubyValue::Proc(p)) => {
                for m in matches {
                    p.call(&[m])?;
                }
                Ok(recv.clone())
            }
            _ => Ok(RubyValue::Array(crate::array_new(matches))),
        }
    }
}

/// `lines`' separator-keeping splitter.
/// Split into lines, KEEPING each terminating newline (a trailing fragment
/// with no newline is still a line) -- `String#each_line`/`#lines`, and
/// `File.readlines`, which is the same rule applied to a whole file.
/// `String#encode`/`encode!`: transcode to a target encoding (default
/// `Encoding.default_internal`, else self's own), reading the CRuby option
/// matrix from a trailing options Hash. `:fallback` may be a Hash, Method, or
/// Proc -- consulted (via ordinary dispatch) for characters the target can't
/// represent, before the `:undef`/error path.
fn encode_impl(recv: &RubyValue, args: &[RubyValue], in_place: bool) -> Result<RubyValue, Signal> {
    use crate::encoding;
    // A trailing Hash carries the keyword options; the rest are positional.
    let (positional, opts_hash) = match args.last() {
        Some(RubyValue::Hash(h)) => (&args[..args.len() - 1], Some(h.clone())),
        _ => (args, None),
    };
    let src = recv_str!(recv);
    let from_enc = match positional.get(1) {
        Some(v) => crate::builtins::encoding::arg_encoding(v)?,
        None => src.lock().encoding(),
    };
    let to_enc = match positional.first() {
        Some(v) => crate::builtins::encoding::arg_encoding(v)?,
        None => encoding::default_internal().unwrap_or_else(|| src.lock().encoding()),
    };
    let opts = parse_encode_opts(opts_hash.as_ref())?;
    let bytes = src.lock().bytes().to_vec();

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
fn parse_encode_opts(
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
    opts.newline = if get("cr_newline").truthy() {
        Some(NewlineMode::Cr)
    } else if get("crlf_newline").truthy() {
        Some(NewlineMode::Crlf)
    } else if get("universal_newline").truthy() {
        Some(NewlineMode::Universal)
    } else {
        None
    };
    Ok(opts)
}

/// The template argument of `unpack`/`unpack1` as a `String` (through the
/// `to_str` protocol).
fn unpack_template(v: &RubyValue) -> Result<String, Signal> {
    Ok(convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned())
}

/// The byte offset of the `char_idx`-th character (the string's byte length
/// when past the end) -- bridges this runtime's char-indexed string API to
/// Rust's byte-indexed slicing.
fn byte_at_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(b, _)| b)
}

/// Wraps a `Regexp` or `String` pattern argument as a compiled Regexp --
/// `match`/`match?`'s shared coercion (a String pattern compiles literally).
fn to_regexp(v: &RubyValue) -> Result<crate::regexp::RRegexp, Signal> {
    match v {
        RubyValue::Regexp(re) => Ok(re.clone()),
        RubyValue::Str(pat) => crate::regexp_new(&pat.lock().to_utf8_lossy(), false, false, false)
            .map_err(|e| regexp_error!("{e}")),
        other => Err(type_error!(
            "wrong argument type {} (expected Regexp)",
            crate::builtins::class_name_of(other)
        )),
    }
}

/// The haystack a `match`/`match?` engine should run over given an optional
/// start position (char offset, end-relative when negative). `None` means
/// the position lands outside the string -- the caller reports "no match"
/// without running the engine.
pub(crate) fn match_haystack(
    text: &str,
    pos: Option<&RubyValue>,
) -> Result<Option<String>, Signal> {
    let Some(v) = pos else {
        return Ok(Some(text.to_string()));
    };
    let clen = text.chars().count() as i64;
    let start = match convert::to_index(v)? {
        p if p < 0 => p + clen,
        p => p,
    };
    if start < 0 || start > clen {
        return Ok(None);
    }
    Ok(Some(text.chars().skip(start as usize).collect()))
}

/// The `count`/`delete` char-set arguments as `(chars, negated)` specs: a
/// leading `^` negates (a bare `"^"` stays literal), `a-z` expands to a
/// range. Zero arguments is CRuby's `ArgumentError`.
#[allow(clippy::type_complexity)]
fn charset_specs(
    args: &[RubyValue],
) -> Result<Vec<(std::collections::HashSet<char>, bool)>, Signal> {
    if args.is_empty() {
        return Err(arg_error!(
            "wrong number of arguments (given 0, expected 1+)"
        ));
    }
    args.iter()
        .map(|a| {
            let spec = convert::to_rstr(a)?.lock().to_utf8_lossy().into_owned();
            let (negated, body) = match spec.strip_prefix('^') {
                Some(rest) if !rest.is_empty() => (true, rest.to_string()),
                _ => (false, spec),
            };
            Ok((expand_charset(&body).into_iter().collect(), negated))
        })
        .collect()
}

/// Whether `c` belongs to EVERY char-set spec (CRuby's intersection rule for
/// the multi-argument `count`/`delete` forms).
fn in_all_charsets(c: char, sets: &[(std::collections::HashSet<char>, bool)]) -> bool {
    sets.iter()
        .all(|(set, negated)| set.contains(&c) != *negated)
}

/// `split`'s whitespace (awk) mode: leading whitespace skipped, fields split
/// on whitespace runs. A positive `limit` keeps the tail (internal
/// whitespace and all) whole as the final field.
fn awk_split(text: &str, limit: i64) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    // `limit == 1`: no splitting at all, the whole string is the one field.
    if limit == 1 {
        return if n == 0 {
            Vec::new()
        } else {
            vec![text.to_string()]
        };
    }
    let mut fields: Vec<String> = Vec::new();
    let mut i = 0;
    // Leading whitespace is always skipped in awk mode.
    while i < n && chars[i].is_whitespace() {
        i += 1;
    }
    while i < n {
        // At the field cap the remainder (from here, verbatim -- the whitespace
        // before it was already skipped) is the final field.
        if limit > 0 && (fields.len() as i64) + 1 >= limit {
            fields.push(chars[i..].iter().collect());
            return fields;
        }
        let beg = i;
        while i < n && !chars[i].is_whitespace() {
            i += 1;
        }
        fields.push(chars[beg..i].iter().collect());
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
    }
    // A non-zero limit keeps ONE trailing empty field when the string ended with
    // whitespace (`limit == 0` drops trailing empties, CRuby's default).
    if limit != 0 && n > 0 && chars[n - 1].is_whitespace() {
        fields.push(String::new());
    }
    fields
}

/// Wraps a list of split fields as a Ruby `Array` of `String`s.
fn str_array(parts: Vec<String>) -> RubyValue {
    RubyValue::Array(crate::array_new(parts.into_iter().map(str_value).collect()))
}

/// `each_line(sep)`: like `split_lines` but on an arbitrary separator, each
/// piece keeping its trailing separator.
/// `lines`/`each_line`'s shared split: an optional `sep` positional and a
/// `chomp:` keyword (a trailing Hash). Split keeps the separator unless chomped;
/// the default separator also strips a preceding `\r` when chomping.
fn lines_from_args(text: &str, args: &[RubyValue]) -> Vec<RubyValue> {
    let mut chomp = false;
    let mut positional = args;
    if let Some(RubyValue::Hash(h)) = args.last() {
        if let RubyValue::Bool(b) =
            crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("chomp")))
        {
            chomp = b;
        }
        positional = &args[..args.len() - 1];
    }
    let sep = match positional.first() {
        Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
        _ => "\n".to_string(),
    };
    if sep.is_empty() {
        return vec![RubyValue::Str(crate::string_new(text.to_string()))];
    }
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(&sep) {
        let end = i + sep.len();
        pieces.push(&rest[..end]);
        rest = &rest[end..];
    }
    if !rest.is_empty() {
        pieces.push(rest);
    }
    pieces
        .into_iter()
        .map(|l| {
            let cut = if chomp {
                let l = l.strip_suffix(&sep).unwrap_or(l);
                if sep == "\n" {
                    l.strip_suffix('\r').unwrap_or(l)
                } else {
                    l
                }
            } else {
                l
            };
            RubyValue::Str(crate::string_new(cut.to_string()))
        })
        .collect()
}

pub(crate) fn split_lines(text: &str) -> Vec<RubyValue> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if c == '\n' {
            out.push(RubyValue::Str(crate::string_new(std::mem::take(&mut cur))));
        }
    }
    if !cur.is_empty() {
        out.push(RubyValue::Str(crate::string_new(cur)));
    }
    out
}

/// `sub`/`gsub`'s shared core: String or Regexp pattern, String
/// replacement or block.
fn sub_gsub(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    global: bool,
) -> Result<RubyValue, Signal> {
    let text = match recv {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => unreachable!("String table row dispatched on a non-String receiver"),
    };
    let block_proc = match &block {
        Some(RubyValue::Proc(p)) => Some(p.clone()),
        _ => None,
    };
    if block_proc.is_some() {
        crate::builtins::arity!(args, 1);
    } else if global && args.len() == 1 {
        // Blockless `gsub(pattern)` is an Enumerator over the matched
        // substrings (iterating it with a block performs the substitution,
        // `rb_enumeratorize`'s re-invoke rule). `sub` has no such form --
        // it keeps the 2-arg ArgumentError below (oracle-verified).
        return Ok(crate::builtins::enumerator::enumerator_for(
            recv, "gsub", args,
        ));
    } else {
        crate::builtins::arity!(args, 2);
    }
    // Pattern: a Regexp as-is; anything else through the `to_str` probe (a
    // literal pattern); a non-convertible pattern is CRuby's
    // "wrong argument type X (expected Regexp)" (oracle-verified, no
    // method-name suffix).
    let pattern_arg = match &args[0] {
        re @ RubyValue::Regexp(_) => re.clone(),
        other => match convert::check_to_str(other)? {
            Some(s) => s,
            None => {
                return Err(type_error!(
                    "wrong argument type {} (expected Regexp)",
                    crate::builtins::class_name_of(other)
                ));
            }
        },
    };
    match (&pattern_arg, block_proc) {
        (RubyValue::Regexp(re), None) => match &args[1] {
            // A Hash replacement maps each matched substring to `hash[match]`
            // (a missing key stringifies to ""), exactly a block that looks the
            // match up -- so it rides the existing block-substitution path.
            RubyValue::Hash(h) => {
                let table = h.clone();
                let p =
                    crate::RProc::new(move |a: &[RubyValue]| Ok(crate::hash_get(&table, &a[0])));
                if global {
                    crate::regexp_gsub_block(re, &text, &p)
                } else {
                    crate::regexp_sub_block(re, &text, &p)
                }
            }
            other => {
                let replacement = convert::to_rstr(other)?.lock().to_utf8_lossy().into_owned();
                if global {
                    crate::regexp_gsub(re, &text, &replacement)
                } else {
                    crate::regexp_sub(re, &text, &replacement)
                }
            }
        },
        (RubyValue::Regexp(re), Some(p)) => {
            if global {
                crate::regexp_gsub_block(re, &text, &p)
            } else {
                crate::regexp_sub_block(re, &text, &p)
            }
        }
        (RubyValue::Str(pattern), None) => {
            let pattern = pattern.lock().to_utf8_lossy().into_owned();
            // A String pattern matches literally, so its "matched substring" is
            // always the pattern itself; a Hash replacement looks that up (a
            // missing key stringifies to ""), a String replacement is literal.
            let replacement = match &args[1] {
                RubyValue::Hash(h) => {
                    let key = RubyValue::Str(crate::string_new(pattern.clone()));
                    crate::hash_get(h, &key).to_display_string()
                }
                other => convert::to_rstr(other)?.lock().to_utf8_lossy().into_owned(),
            };
            Ok(RubyValue::Str(crate::string_new(if global {
                text.replace(&pattern, &replacement)
            } else {
                text.replacen(&pattern, &replacement, 1)
            })))
        }
        (RubyValue::Str(pattern), Some(p)) => {
            let pattern = pattern.lock().to_utf8_lossy().into_owned();
            let mut out = String::new();
            let mut rest = text.as_str();
            loop {
                match rest.find(&pattern) {
                    Some(pos) if !pattern.is_empty() => {
                        out.push_str(&rest[..pos]);
                        let replaced =
                            p.call(&[RubyValue::Str(crate::string_new(pattern.clone()))])?;
                        out.push_str(&replaced.to_display_string());
                        rest = &rest[pos + pattern.len()..];
                        if !global {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            out.push_str(rest);
            Ok(RubyValue::Str(crate::string_new(out)))
        }
        (_, _) => unreachable!("sub/gsub pattern normalized to Str/Regexp above"),
    }
}

enum Pad {
    Center,
    Left,
    Right,
}

fn pad(recv: &RubyValue, args: &[RubyValue], kind: Pad) -> Result<RubyValue, Signal> {
    let text = match recv {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => unreachable!("String table row dispatched on a non-String receiver"),
    };
    let width = convert::to_index(&args[0])?;
    let fill = match args.get(1) {
        None => " ".to_string(),
        Some(f) => convert::to_rstr(f)?.lock().to_utf8_lossy().into_owned(),
    };
    if fill.is_empty() {
        return Err(arg_error!("zero width padding"));
    }
    let len = text.chars().count() as i64;
    let total = (width - len).max(0) as usize;
    let fill_n = |n: usize| -> String { fill.chars().cycle().take(n).collect() };
    let out = match kind {
        Pad::Left => format!("{text}{}", fill_n(total)),
        Pad::Right => format!("{}{text}", fill_n(total)),
        Pad::Center => {
            let left = total / 2;
            format!("{}{text}{}", fill_n(left), fill_n(total - left))
        }
    };
    Ok(RubyValue::Str(crate::string_new(out)))
}

/// The `encoding:` keyword shared by `String.new` -- reads it from the
/// trailing options Hash (the G2 kwargs convention), resolving a name string
/// or an `Encoding` value; `None` when absent.
fn kw_encoding(args: &[RubyValue]) -> Result<Option<crate::encoding::EncodingId>, Signal> {
    let Some(RubyValue::Hash(h)) = args.last() else {
        return Ok(None);
    };
    let v = crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("encoding")));
    if v.is_nil() {
        return Ok(None);
    }
    Ok(Some(crate::builtins::encoding::arg_encoding(&v)?))
}

/// The positional args of a call that may carry a trailing keyword Hash --
/// strips that Hash so `arity!` counts only the real positionals.
fn kw_strip(args: &[RubyValue]) -> &[RubyValue] {
    match args.last() {
        Some(RubyValue::Hash(_)) => &args[..args.len() - 1],
        _ => args,
    }
}

/// The byte set `String#strip`/`#lstrip`/`#rstrip` remove: CRuby strips
/// `"\0\t\n\v\f\r "` -- ASCII whitespace PLUS the NUL byte (which Rust's
/// `char::is_whitespace` does not include), and only these ASCII bytes (no
/// Unicode spaces, unlike `str::trim`).
fn is_rb_strip(c: char) -> bool {
    matches!(c, '\0' | '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ')
}

/// `unpack`/`unpack1`'s `offset:` keyword: the byte index to start decoding
/// from (default 0). CRuby allows `offset == bytesize` (an empty remainder ->
/// nil), rejects a larger offset ("offset outside of string") and a negative
/// one ("offset can't be negative").
fn kw_unpack_offset(args: &[RubyValue], len: usize) -> Result<usize, Signal> {
    let Some(RubyValue::Hash(h)) = args.last() else {
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

builtin_methods! {
    pub(crate) fn lookup_class;

    // `String.new` / `String.new(str)` / `String.new(str, encoding:, capacity:)`.
    // A no-arg new is an empty ASCII-8BIT string (CRuby's default for a
    // fresh buffer); a source string is copied, keeping its own encoding
    // unless `encoding:` overrides it. `capacity:` only hints allocation, so
    // it is accepted and ignored.
    "new" => fn string_new_m(_recv, args, _block) {
        let enc_override = kw_encoding(args)?;
        // Strip a trailing options Hash before reading the positional source.
        let positional = match args.last() {
            Some(RubyValue::Hash(_)) => &args[..args.len() - 1],
            _ => args,
        };
        arity!(positional, 0..=1);
        let (bytes, enc) = match positional.first() {
            None => (Vec::new(), crate::encoding::ASCII_8BIT),
            Some(v) => {
                let s = convert::to_rstr(v)?;
                let s = s.lock();
                (s.bytes().to_vec(), s.encoding())
            }
        };
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, enc_override.unwrap_or(enc))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(v.to_string()))
    }

    fn show(r: Result<RubyValue, Signal>) -> String {
        r.unwrap().inspect_string()
    }

    #[test]
    fn oct_and_hex_honor_prefixes_and_stop_at_garbage() {
        assert_eq!(show(oct(&s("777"), &[], None)), "511");
        assert_eq!(show(oct(&s("0x1f"), &[], None)), "31"); // 0x prefix overrides base 8
        assert_eq!(show(hex(&s("ff"), &[], None)), "255");
        assert_eq!(show(hex(&s("0xff"), &[], None)), "255");
        assert_eq!(show(oct(&s("12 z9"), &[], None)), "10"); // stops at 'z'
        assert_eq!(show(hex(&s(""), &[], None)), "0");
    }

    #[test]
    fn casecmp_families() {
        assert_eq!(show(casecmp(&s("Hello"), &[s("hello")], None)), "0");
        assert_eq!(show(casecmp(&s("A"), &[s("b")], None)), "-1");
        assert_eq!(show(casecmp_p(&s("Hello"), &[s("HELLO")], None)), "true");
        assert_eq!(show(casecmp_p(&s("a"), &[s("b")], None)), "false");
    }

    #[test]
    fn slice_bang_removes_in_place_and_returns_the_slice() {
        let str = s("hello");
        assert_eq!(
            show(slice_bang(
                &str,
                &[RubyValue::Int(1), RubyValue::Int(2)],
                None
            )),
            "\"el\""
        );
        assert_eq!(str.to_display_string(), "hlo");
        let str2 = s("hello");
        assert_eq!(show(slice_bang(&str2, &[s("ll")], None)), "\"ll\"");
        assert_eq!(str2.to_display_string(), "heo");
    }

    #[test]
    fn split_empty_separator_yields_characters() {
        assert_eq!(
            show(split(&s("hello"), &[s("")], None)),
            "[\"h\", \"e\", \"l\", \"l\", \"o\"]"
        );
        assert_eq!(
            show(split(&s("hello"), &[s(""), RubyValue::Int(2)], None)),
            "[\"h\", \"ello\"]"
        );
    }

    #[test]
    fn index_with_regexp_and_group() {
        let re =
            RubyValue::Regexp(crate::regexp_new("(\\w+) (\\w+)", false, false, false).unwrap());
        assert_eq!(
            show(index_op(
                &s("hello world foo"),
                std::slice::from_ref(&re),
                None
            )),
            "\"hello world\""
        );
        assert_eq!(
            show(index_op(&s("hello world"), &[re, RubyValue::Int(2)], None)),
            "\"world\""
        );
    }

    #[test]
    fn case_and_strip_families_match_the_oracle() {
        assert_eq!(
            show(capitalize(&s("hello world"), &[], None)),
            "\"Hello world\""
        );
        assert_eq!(show(swapcase(&s("HeLLo"), &[], None)), "\"hEllO\"");
        assert_eq!(show(strip(&s("  hi  "), &[], None)), "\"hi\"");
        assert_eq!(show(lstrip(&s("  hi"), &[], None)), "\"hi\"");
    }

    #[test]
    fn split_covers_the_three_separator_shapes() {
        assert_eq!(
            show(split(&s("a b  c"), &[], None)),
            "[\"a\", \"b\", \"c\"]"
        );
        assert_eq!(
            show(split(&s("a,b,,c"), &[s(",")], None)),
            "[\"a\", \"b\", \"\", \"c\"]"
        );
        assert_eq!(
            show(split(&s("hello"), &[s("l")], None)),
            "[\"he\", \"\", \"o\"]"
        );
    }

    #[test]
    fn succ_carries_like_cruby() {
        assert_eq!(succ_str("az"), "ba");
        assert_eq!(succ_str("zz"), "aaa");
        assert_eq!(succ_str("a9"), "b0");
        assert_eq!(succ_str("Zz"), "AAa");
        assert_eq!(succ_str("99"), "100");
    }

    #[test]
    fn tr_expands_ranges_and_repeats_the_last_target() {
        assert_eq!(
            show(tr(&s("hello"), &[s("el"), s("ip")], None)),
            "\"hippo\""
        );
        assert_eq!(
            show(tr(&s("hello"), &[s("a-y"), s("b-z")], None)),
            "\"ifmmp\""
        );
        assert_eq!(show(tr(&s("a-b_c"), &[s("-_"), s(" ")], None)), "\"a b c\"");
    }

    #[test]
    fn tr_duplicate_from_char_uses_the_last_mapping() {
        // CRuby: a char repeated in `from` takes its LAST corresponding `to`.
        assert_eq!(
            show(tr(&s("a___b"), &[s("___"), s(".+-")], None)),
            "\"a---b\""
        );
        assert_eq!(
            show(tr(&s("abcaa"), &[s("aa"), s("xy")], None)),
            "\"ybcyy\""
        );
    }

    #[test]
    fn lenient_conversions_match_the_oracle() {
        assert!(matches!(
            to_i(&s("42abc"), &[], None).unwrap(),
            RubyValue::Int(42)
        ));
        assert!(matches!(
            to_i(&s("abc"), &[], None).unwrap(),
            RubyValue::Int(0)
        ));
        assert!(matches!(
            to_i(&s("0x1A"), &[], None).unwrap(),
            RubyValue::Int(0)
        ));
        assert!(matches!(
            to_i(&s("ff"), &[RubyValue::Int(16)], None).unwrap(),
            RubyValue::Int(255)
        ));
        assert!(
            matches!(to_f(&s("42.5xyz"), &[], None).unwrap(), RubyValue::Float(f) if f == 42.5)
        );
    }

    #[test]
    fn indexing_forms_match_the_oracle() {
        assert_eq!(
            show(index_op(&s("hello"), &[RubyValue::Int(1)], None)),
            "\"e\""
        );
        assert_eq!(
            show(index_op(
                &s("hello"),
                &[RubyValue::Int(1), RubyValue::Int(3)],
                None
            )),
            "\"ell\""
        );
        let range = RubyValue::Range(
            Some(Box::new(RubyValue::Int(1))),
            Some(Box::new(RubyValue::Int(3))),
            false,
        );
        assert_eq!(show(index_op(&s("hello"), &[range], None)), "\"ell\"");
        assert_eq!(
            show(index_op(&s("hello"), &[RubyValue::Int(99)], None)),
            "nil"
        );
    }

    #[test]
    fn padding_and_charset_rows_match_the_oracle() {
        assert_eq!(
            show(center(&s("hi"), &[RubyValue::Int(7), s("*")], None)),
            "\"**hi***\""
        );
        assert_eq!(
            show(ljust(&s("hi"), &[RubyValue::Int(5), s(".")], None)),
            "\"hi...\""
        );
        assert_eq!(show(delete(&s("hello"), &[s("l")], None)), "\"heo\"");
        assert_eq!(show(squeeze(&s("aabbcc"), &[], None)), "\"abc\"");
        assert_eq!(show(squeeze(&s("aabbcc"), &[s("a")], None)), "\"abbcc\"");
        assert_eq!(show(count(&s("hello world"), &[s("lo")], None)), "5");
    }

    #[test]
    fn mutating_rows_write_through_the_shared_payload() {
        let orig = s("orig");
        replace(&orig, &[s("xyz")], None).unwrap();
        assert_eq!(orig.to_display_string(), "xyz");
        concat(&orig, &[s("!")], None).unwrap();
        assert_eq!(orig.to_display_string(), "xyz!");
        prepend(&orig, &[s("ab")], None).unwrap();
        assert_eq!(orig.to_display_string(), "abxyz!");
    }
}
