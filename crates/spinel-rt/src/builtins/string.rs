//! `String` (CRuby string.c) -- the Tier A surface: case family, strip
//! family, split/chars/lines, sub/gsub (String AND Regexp patterns, with
//! block forms), indexing forms, tr/delete/squeeze/count, conversions,
//! succ, padding, `%` formatting. Strings are UTF-8 (`char`-indexed like
//! modern CRuby); the bytes/encoding surface is Tier C, documented in the
//! plan.

use crate::builtins::{arg_int, arg_str, arity, block_or_enum, builtin_methods, recv_str};
use crate::{RubyValue, Signal};

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
    let alnum_positions: Vec<usize> = (0..chars.len())
        .filter(|&i| chars[i].is_ascii_alphanumeric())
        .collect();
    if alnum_positions.is_empty() {
        let last = chars.len() - 1;
        chars[last] = char::from_u32(chars[last] as u32 + 1).unwrap_or(chars[last]);
        return chars.into_iter().collect();
    }
    let mut carry = true;
    let mut leftmost = *alnum_positions.first().expect("non-empty");
    for &i in alnum_positions.iter().rev() {
        if !carry {
            break;
        }
        leftmost = i;
        let (next, wrapped) = match chars[i] {
            'z' => ('a', true),
            'Z' => ('A', true),
            '9' => ('0', true),
            c => (char::from_u32(c as u32 + 1).expect("ascii alnum"), false),
        };
        chars[i] = next;
        carry = wrapped;
    }
    if carry {
        // Full wrap: prepend a new digit of the leftmost run's kind
        // ("zz" -> "aaa", "99" -> "100").
        let seed = match chars[leftmost] {
            'a'..='z' => 'a',
            'A'..='Z' => 'A',
            _ => '1',
        };
        chars.insert(leftmost, seed);
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

fn str_value(s: String) -> RubyValue {
    RubyValue::Str(crate::string_new(s))
}

/// `String#[]`/`slice` with a Regexp: the whole match or a named/numbered
/// capture group. `None` group arg means the whole match.
fn regexp_index(re: &crate::RRegexp, text: &str, group: Option<&RubyValue>) -> RubyValue {
    let RubyValue::MatchData(m) = crate::regexp_match(re, text) else {
        return RubyValue::Nil;
    };
    match group {
        None => crate::matchdata_group(&m, 0),
        Some(RubyValue::Int(n)) => crate::matchdata_group(&m, *n),
        Some(RubyValue::Str(name)) => {
            crate::matchdata_group_by_name(&m, &name.lock().to_utf8_lossy())
        }
        Some(RubyValue::Symbol(s)) => crate::matchdata_group_by_name(&m, &s.name()),
        _ => RubyValue::Nil,
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
                    if *exclusive {
                        v
                    } else {
                        v + 1
                    }
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

/// The first index of `needle` within `haystack` (both char slices), or None.
fn find_subslice(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}


builtin_methods! {
    pub(crate) fn lookup;

    "length" | "size" => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::string_len(recv_str!(recv))))
    }
    "empty?" => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(crate::string_len(recv_str!(recv)) == 0))
    }
    // Byte-level accessors, honoring the string's real encoding (`bytes`
    // yields the raw bytes; `bytesize` counts them, distinct from the
    // char-counting `length`).
    "bytesize" => fn bytesize(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_str!(recv).lock().bytesize() as i64))
    }
    // `String#-@` / `#dedup`: an already-frozen receiver is returned as-is
    // (CRuby #2630); otherwise the content is interned to its immortal
    // frozen twin, so two dedups of equal content are the same object.
    "-@" | "dedup" => fn dedup(recv, args, _block) {
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
    "bytes" => fn bytes(recv, args, _block) {
        arity!(args, 0);
        let out = recv_str!(recv)
            .lock()
            .bytes()
            .iter()
            .map(|b| RubyValue::Int(*b as i64))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "each_byte" => fn each_byte(recv, args, block) {
        arity!(args, 0);
        let Some(RubyValue::Proc(p)) = &block else {
            return Err(crate::dispatch::raise_error(
                "LocalJumpError",
                "no block given (yield)".to_string(),
            ));
        };
        let bytes: Vec<u8> = recv_str!(recv).lock().bytes().to_vec();
        for b in bytes {
            p.call(&[RubyValue::Int(b as i64)])?;
        }
        Ok(recv.clone())
    }
    "getbyte" => fn getbyte(recv, args, _block) {
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
    "setbyte" => fn setbyte(recv, args, _block) {
        arity!(args, 2);
        let (i, b) = (arg_int!(args, 0), arg_int!(args, 1));
        let s = recv_str!(recv);
        let len = s.lock().bytesize() as i64;
        let idx = if i < 0 { i + len } else { i };
        if idx < 0 || idx >= len {
            return Err(crate::dispatch::raise_error("IndexError", format!("index {i} out of string")));
        }
        s.lock().setbyte(idx as usize, (b & 0xff) as u8);
        Ok(args[1].clone())
    }
    // --- Encoding surface -------------------------------------------------
    "encoding" => fn encoding_m(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::builtins::encoding::encoding_value(recv_str!(recv).lock().encoding()))
    }
    // `force_encoding` re-TAGS the bytes without touching them; `b` COPIES
    // them under ASCII-8BIT. Both return a value the caller can chain.
    "force_encoding" => fn force_encoding(recv, args, _block) {
        arity!(args, 1);
        let id = crate::builtins::encoding::arg_encoding(&args[0])?;
        recv_str!(recv).lock().set_encoding(id);
        Ok(recv.clone())
    }
    "b" => fn to_binary(recv, args, _block) {
        arity!(args, 0);
        let bytes = recv_str!(recv).lock().bytes().to_vec();
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT)))
    }
    "ascii_only?" => fn ascii_only(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_str!(recv).lock().ascii_only()))
    }
    "valid_encoding?" => fn valid_encoding(recv, args, _block) {
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
    "unpack" => fn unpack(recv, args, _block) {
        arity!(args, 1);
        let template = unpack_template(&args[0])?;
        let bytes = recv_str!(recv).lock().bytes().to_vec();
        let vals = crate::builtins::pack::unpack(&bytes, &template)?;
        Ok(RubyValue::Array(crate::array_new(vals)))
    }
    "unpack1" => fn unpack1(recv, args, _block) {
        arity!(args, 1);
        let template = unpack_template(&args[0])?;
        let bytes = recv_str!(recv).lock().bytes().to_vec();
        let vals = crate::builtins::pack::unpack(&bytes, &template)?;
        Ok(vals.into_iter().next().unwrap_or(RubyValue::Nil))
    }
    "scrub" => fn scrub(recv, args, _block) {
        // Rewrite every invalid byte sequence to the replacement (an explicit
        // String argument, else U+FFFD for a Unicode encoding / "?" otherwise).
        arity!(args, 0..=1);
        let repl = match args.first() {
            Some(RubyValue::Str(r)) => Some(r.lock().to_utf8_lossy().into_owned()),
            _ => None,
        };
        let s = recv_str!(recv).lock();
        let mut opts = crate::encoding::TranscodeOptions { invalid_replace: true, ..Default::default() };
        opts.replace = repl;
        // Scrub = transcode to self's own encoding, replacing invalids.
        let out = crate::encoding::transcode(s.bytes(), s.encoding(), s.encoding(), &opts, None)
            .map_err(|e| e.into_signal())?;
        Ok(RubyValue::Str(crate::string_from_bytes(out, s.encoding())))
    }
    "include?" => fn include_p(recv, args, _block) {
        arity!(args, 1);
        let needle = arg_str!(args, 0);
        let found = recv_str!(recv)
            .lock()
            .to_utf8_lossy()
            .contains(&*needle.lock().to_utf8_lossy());
        Ok(RubyValue::Bool(found))
    }
    "+" => fn plus(recv, args, _block) {
        arity!(args, 1);
        let other = arg_str!(args, 0);
        let joined = format!("{}{}", recv_str!(recv).lock(), other.lock());
        Ok(str_value(joined))
    }
    // Mutating append -- returns the receiver (the same object).
    "<<" | "concat" => fn concat(recv, args, _block) {
        arity!(args, 1);
        let addition = match &args[0] {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            // `str << 65` appends the CODEPOINT's character.
            RubyValue::Int(i) => match u32::try_from(*i).ok().and_then(char::from_u32) {
                Some(c) => c.to_string(),
                None => {
                    return Err(crate::dispatch::raise_error(
                        "RangeError",
                        format!("{i} out of char range"),
                    ))
                }
            },
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        let s = recv_str!(recv);
        // Frozen check at the mutator (CRuby's `rb_str_modify`): `<<`/`concat`
        // dispatch through this one row for every receiver shape, so guarding
        // here covers them all -- including a `frozen_string_literal` literal.
        if s.is_frozen() {
            return Err(crate::dispatch::raise_error(
                "FrozenError",
                format!("can't modify frozen String: {}", recv.inspect_string()),
            ));
        }
        s.lock().push_str(&addition);
        Ok(recv.clone())
    }
    "*" => fn times(recv, args, _block) {
        arity!(args, 1);
        let n = arg_int!(args, 0);
        if n < 0 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "negative argument".to_string(),
            ));
        }
        Ok(str_value(recv_str!(recv).lock().to_utf8_lossy().repeat(n as usize)))
    }
    "to_s" | "to_str" => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "<=>" => fn spaceship(recv, args, _block) {
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
    "==" | "eql?" => fn eq(recv, args, _block) {
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
        Ok(str_value(recv_str!(recv).lock().to_utf8_lossy().trim().to_string()))
    }
    "lstrip" => fn lstrip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().to_utf8_lossy().trim_start().to_string()))
    }
    "rstrip" => fn rstrip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().to_utf8_lossy().trim_end().to_string()))
    }
    "chars" => fn chars(recv, args, _block) {
        arity!(args, 0);
        let out = recv_str!(recv)
            .lock()
            .chars()
            .map(|c| str_value(c.to_string()))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `lines` keeps each separator (`["a\n", "b\n", "c"]`).
    "lines" => fn lines(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(crate::array_new(split_lines(
            &recv_str!(recv).lock().to_utf8_lossy(),
        ))))
    }
    "each_char" => fn each_char(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_char", args, block);
        let cs: Vec<char> = recv_str!(recv).lock().chars().collect();
        for c in cs {
            p.call(&[str_value(c.to_string())])?;
        }
        Ok(recv.clone())
    }
    // `each_line` / `each_line(sep)`: a custom separator keeps its trailing
    // occurrence on each piece, exactly like the default `"\n"`.
    "each_line" => fn each_line(recv, args, block) {
        arity!(args, 0..=1);
        let p = block_or_enum!(recv, "each_line", args, block);
        let ls = match args.first() {
            Some(RubyValue::Str(sep)) => split_lines_sep(
                &recv_str!(recv).lock().to_utf8_lossy(),
                &sep.lock().to_utf8_lossy(),
            ),
            _ => split_lines(&recv_str!(recv).lock().to_utf8_lossy()),
        };
        for l in ls {
            p.call(&[l])?;
        }
        Ok(recv.clone())
    }
    // `split`: no-arg/nil = whitespace runs (leading skipped); a String
    // separator keeps interior empties; a Regexp separator delegates to the
    // shared regexp splitter. The optional `limit` caps the field count
    // (`> 0`, tail kept whole), keeps trailing empties (`< 0`), or drops
    // them (`0`/omitted).
    "split" => fn split(recv, args, _block) {
        arity!(args, 0..=2);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let limit = match args.get(1) {
            Some(RubyValue::Int(n)) => *n,
            Some(RubyValue::Nil) | None => 0,
            Some(other) => return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("no implicit conversion of {} into Integer", crate::builtins::class_name_of(other)),
            )),
        };
        match args.first() {
            None | Some(RubyValue::Nil) => Ok(str_array(awk_split(&text, limit))),
            Some(RubyValue::Str(sep)) => {
                let sep = sep.lock().to_utf8_lossy().into_owned();
                if sep == " " {
                    // The one magic separator: a single space means
                    // whitespace-run splitting, real Ruby's awk rule.
                    return Ok(str_array(awk_split(&text, limit)));
                }
                if sep.is_empty() {
                    // An empty separator splits into characters (Rust's own
                    // `split("")` would emit spurious leading/trailing empties).
                    let chars: Vec<char> = text.chars().collect();
                    let parts: Vec<String> = if limit > 0 && (limit as usize) < chars.len() {
                        let head = limit as usize - 1;
                        let mut v: Vec<String> = chars[..head].iter().map(|c| c.to_string()).collect();
                        v.push(chars[head..].iter().collect());
                        v
                    } else {
                        chars.iter().map(|c| c.to_string()).collect()
                    };
                    return Ok(str_array(parts));
                }
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
                Ok(str_array(parts))
            }
            Some(RubyValue::Regexp(re)) => Ok(crate::regexp_split(re, &text, limit)),
            Some(other) => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "wrong argument type {} (expected Regexp)",
                    crate::builtins::class_name_of(other)
                ),
            )),
        }
    }
    "chomp" => fn chomp(recv, args, _block) {
        arity!(args, 0..=1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let out = match args.first() {
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
    "chop" => fn chop(recv, args, _block) {
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
    "reverse" => fn reverse(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_wrap(recv_str!(recv).lock().reversed())))
    }
    "index" => fn index(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        match &args[0] {
            RubyValue::Str(needle) => {
                let needle = needle.lock().to_utf8_lossy().into_owned();
                Ok(match text.find(&needle) {
                    Some(byte_pos) => RubyValue::Int(text[..byte_pos].chars().count() as i64),
                    None => RubyValue::Nil,
                })
            }
            RubyValue::Regexp(re) => Ok(crate::regexp_match_index(re, &text)),
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into String",
                    crate::builtins::class_name_of(other)
                ),
            )),
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
                let p = int_arg(v)?;
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
            RubyValue::Str(needle) => {
                let needle = needle.lock().to_utf8_lossy().into_owned();
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
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into String",
                    crate::builtins::class_name_of(other)
                ),
            )),
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
            return Ok(regexp_index(re, &text, args.get(1)));
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
            RubyValue::Int(i) => Ok(wrap(s.char_at(*i))),
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
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            )),
        }
    }
    // `casecmp` is an ASCII case-insensitive `<=>`; `casecmp?` its boolean
    // (Unicode-aware) sibling. A non-String argument answers nil.
    "casecmp" => fn casecmp(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(o) = &args[0] else { return Ok(RubyValue::Nil) };
        let a = recv_str!(recv).lock().to_utf8_lossy().to_lowercase();
        let b = o.lock().to_utf8_lossy().to_lowercase();
        Ok(RubyValue::Int(a.cmp(&b) as i64))
    }
    "casecmp?" => fn casecmp_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(o) = &args[0] else { return Ok(RubyValue::Nil) };
        let a = recv_str!(recv).lock().to_utf8_lossy().to_lowercase();
        let b = o.lock().to_utf8_lossy().to_lowercase();
        Ok(RubyValue::Bool(a == b))
    }
    // `oct`/`hex` parse a leading integer in base 8/16, honoring an explicit
    // `0x`/`0b`/`0o`/`0d` radix prefix and stopping at the first invalid
    // digit (0 when there is none) -- CRuby's lenient rule.
    "oct" => fn oct(recv, args, _block) {
        arity!(args, 0);
        Ok(parse_int_lenient(&recv_str!(recv).lock().to_utf8_lossy(), 8))
    }
    "hex" => fn hex(recv, args, _block) {
        arity!(args, 0);
        Ok(parse_int_lenient(&recv_str!(recv).lock().to_utf8_lossy(), 16))
    }
    // `slice!(index[, len])` / `slice!(range)` / `slice!(substring)`: removes
    // the matched portion from the receiver IN PLACE and returns it (nil when
    // nothing matched).
    "slice!" => fn slice_bang(recv, args, _block) {
        arity!(args, 1..=2);
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
            let RubyValue::Str(prefix) = a else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(a)
                    ),
                ));
            };
            if text.starts_with(&*prefix.lock().to_utf8_lossy()) {
                return Ok(RubyValue::Bool(true));
            }
        }
        Ok(RubyValue::Bool(false))
    }
    "end_with?" => fn end_with_p(recv, args, _block) {
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        for a in args {
            let RubyValue::Str(suffix) = a else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(a)
                    ),
                ));
            };
            if text.ends_with(&*suffix.lock().to_utf8_lossy()) {
                return Ok(RubyValue::Bool(true));
            }
        }
        Ok(RubyValue::Bool(false))
    }
    // `tr(from, to)` with `a-z` range expansion; a short `to` repeats its
    // last character (CRuby's rule).
    "tr" => fn tr(recv, args, _block) {
        arity!(args, 2);
        let from = expand_charset(&arg_str!(args, 0).lock().to_utf8_lossy());
        let to = expand_charset(&arg_str!(args, 1).lock().to_utf8_lossy());
        let out = recv_str!(recv)
            .lock()
            .chars()
            .map(|c| match from.iter().position(|&f| f == c) {
                Some(i) => *to.get(i).or(to.last()).unwrap_or(&c),
                None => c,
            })
            .collect();
        Ok(str_value(out))
    }
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
                RubyValue::Str(s) => expand_charset(&s.lock().to_utf8_lossy()),
                _ => Vec::new(),
            });
        let mut out = String::new();
        let mut prev: Option<char> = None;
        for c in recv_str!(recv).lock().chars() {
            let squeezable = set.as_ref().is_none_or(|s| s.contains(&c));
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
            Some(RubyValue::Int(b)) if (2..=36).contains(b) => *b as u32,
            Some(RubyValue::Int(b)) => {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("invalid radix {b}"),
                ))
            }
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into Integer",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
            None => 10,
        };
        Ok(lenient_to_i(&recv_str!(recv).lock().to_utf8_lossy(), base))
    }
    // Lenient like `to_i`: the longest valid leading float, else 0.0.
    "to_f" => fn to_f(recv, args, _block) {
        arity!(args, 0);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        let t = text.trim_start();
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
    "to_sym" | "intern" => fn to_sym(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(crate::Symbol::intern(
            &recv_str!(recv).lock().to_utf8_lossy(),
        )))
    }
    "succ" | "next" => fn succ(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(succ_str(&recv_str!(recv).lock().to_utf8_lossy())))
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
    "insert" => fn insert(recv, args, _block) {
        arity!(args, 2);
        let at = arg_int!(args, 0);
        let addition = arg_str!(args, 1).lock().to_utf8_lossy().into_owned();
        let handle = recv_str!(recv);
        let mut guard = handle.lock();
        let n = guard.char_len() as i64;
        let at = if at < 0 { at + n + 1 } else { at };
        if at < 0 || at > n {
            return Err(crate::dispatch::raise_error(
                "IndexError",
                format!("index {} out of string", arg_int!(args, 0)),
            ));
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
    "prepend" => fn prepend(recv, args, _block) {
        arity!(args, 1);
        let addition = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        let handle = recv_str!(recv);
        let mut guard = handle.lock();
        let mut txt = guard.to_utf8_lossy().into_owned();
        txt.insert_str(0, &addition);
        guard.replace_utf8(txt);
        drop(guard);
        Ok(recv.clone())
    }
    "replace" => fn replace(recv, args, _block) {
        arity!(args, 1);
        let new_text = arg_str!(args, 0).lock().to_utf8_lossy().into_owned();
        recv_str!(recv).lock().replace_utf8(new_text);
        Ok(recv.clone())
    }
    // `Integer#chr`'s inverse -- the first character's codepoint.
    "ord" => fn ord(recv, args, _block) {
        arity!(args, 0);
        match recv_str!(recv).lock().chars().next() {
            Some(c) => Ok(RubyValue::Int(c as i64)),
            None => Err(crate::dispatch::raise_error(
                "ArgumentError",
                "empty string".to_string(),
            )),
        }
    }
    // `"%s..." % args` -- the shared sprintf engine (`builtins::format`).
    "%" => fn format_op(recv, args, _block) {
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
    "=~" => fn match_op(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Regexp(re) => {
                Ok(crate::regexp_match_index(re, &recv_str!(recv).lock().to_utf8_lossy()))
            }
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "wrong argument type {} (expected Regexp)",
                    crate::builtins::class_name_of(other)
                ),
            )),
        }
    }
    // Both accept an optional start position (char offset, end-relative when
    // negative); a position outside the string means "no match" without even
    // running the engine.
    "match" => fn match_m(recv, args, _block) {
        arity!(args, 1..=2);
        let re = to_regexp(&args[0])?;
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        match match_haystack(&text, args.get(1))? {
            Some(h) => Ok(crate::regexp_match(&re, &h)),
            None => Ok(RubyValue::Nil),
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
    "scan" => fn scan(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().to_utf8_lossy().into_owned();
        match &args[0] {
            RubyValue::Regexp(re) => Ok(crate::regexp_scan(re, &text)),
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "wrong argument type {} (expected Regexp)",
                    crate::builtins::class_name_of(other)
                ),
            )),
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
            let method = if matches!(fv, RubyValue::Hash(_)) { "[]" } else { "call" };
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
        fb.as_mut().map(|f| f as &mut dyn FnMut(&str) -> Option<String>),
    )
    .map_err(|e| e.into_signal())?;

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

/// The template argument of `unpack`/`unpack1` as a `String`.
fn unpack_template(v: &RubyValue) -> Result<String, Signal> {
    match v {
        RubyValue::Str(t) => Ok(t.lock().to_utf8_lossy().into_owned()),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!("no implicit conversion of {} into String", crate::builtins::class_name_of(other)),
        )),
    }
}

/// An Integer argument (a position/limit), raising CRuby's exact TypeError
/// for a non-Integer.
fn int_arg(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Int(n) => Ok(*n),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// The byte offset of the `char_idx`-th character (the string's byte length
/// when past the end) -- bridges this runtime's char-indexed string API to
/// Rust's byte-indexed slicing.
fn byte_at_char(text: &str, char_idx: usize) -> usize {
    text.char_indices().nth(char_idx).map_or(text.len(), |(b, _)| b)
}

/// Wraps a `Regexp` or `String` pattern argument as a compiled Regexp --
/// `match`/`match?`'s shared coercion (a String pattern compiles literally).
fn to_regexp(v: &RubyValue) -> Result<crate::regexp::RRegexp, Signal> {
    match v {
        RubyValue::Regexp(re) => Ok(re.clone()),
        RubyValue::Str(pat) => crate::regexp_new(&pat.lock().to_utf8_lossy(), false, false, false)
            .map_err(|e| crate::dispatch::raise_error("RegexpError", e)),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "wrong argument type {} (expected Regexp)",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// The haystack a `match`/`match?` engine should run over given an optional
/// start position (char offset, end-relative when negative). `None` means
/// the position lands outside the string -- the caller reports "no match"
/// without running the engine.
fn match_haystack(text: &str, pos: Option<&RubyValue>) -> Result<Option<String>, Signal> {
    let Some(v) = pos else {
        return Ok(Some(text.to_string()));
    };
    let clen = text.chars().count() as i64;
    let start = match int_arg(v)? {
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
fn charset_specs(args: &[RubyValue]) -> Result<Vec<(std::collections::HashSet<char>, bool)>, Signal> {
    if args.is_empty() {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "wrong number of arguments (given 0, expected 1+)".to_string(),
        ));
    }
    args.iter()
        .map(|a| {
            let RubyValue::Str(s) = a else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("no implicit conversion of {} into String", crate::builtins::class_name_of(a)),
                ));
            };
            let spec = s.lock().to_utf8_lossy().into_owned();
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
    sets.iter().all(|(set, negated)| set.contains(&c) != *negated)
}

/// `split`'s whitespace (awk) mode: leading whitespace skipped, fields split
/// on whitespace runs. A positive `limit` keeps the tail (internal
/// whitespace and all) whole as the final field.
fn awk_split(text: &str, limit: i64) -> Vec<String> {
    if limit <= 0 {
        // Whitespace mode never yields empty fields, so trailing-empty
        // handling is moot for both the 0 and negative cases.
        return text.split_whitespace().map(str::to_string).collect();
    }
    let mut fields = Vec::new();
    let mut rest = text.trim_start();
    while (fields.len() as i64) + 1 < limit {
        match rest.find(char::is_whitespace) {
            Some(i) => {
                fields.push(rest[..i].to_string());
                rest = rest[i..].trim_start();
            }
            None => break,
        }
        if rest.is_empty() {
            break;
        }
    }
    if !rest.is_empty() {
        fields.push(rest.to_string());
    }
    fields
}

/// Wraps a list of split fields as a Ruby `Array` of `String`s.
fn str_array(parts: Vec<String>) -> RubyValue {
    RubyValue::Array(crate::array_new(parts.into_iter().map(str_value).collect()))
}

/// `each_line(sep)`: like `split_lines` but on an arbitrary separator, each
/// piece keeping its trailing separator.
fn split_lines_sep(text: &str, sep: &str) -> Vec<RubyValue> {
    if sep.is_empty() {
        return vec![RubyValue::Str(crate::string_new(text.to_string()))];
    }
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(sep) {
        let end = i + sep.len();
        out.push(RubyValue::Str(crate::string_new(rest[..end].to_string())));
        rest = &rest[end..];
    }
    if !rest.is_empty() {
        out.push(RubyValue::Str(crate::string_new(rest.to_string())));
    }
    out
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
    let label = if global { "gsub" } else { "sub" };
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
    } else {
        crate::builtins::arity!(args, 2);
    }
    match (&args[0], block_proc) {
        (RubyValue::Regexp(re), None) => match &args[1] {
            RubyValue::Str(replacement) => {
                let replacement = replacement.lock().to_utf8_lossy().into_owned();
                Ok(if global {
                    crate::regexp_gsub(re, &text, &replacement)
                } else {
                    crate::regexp_sub(re, &text, &replacement)
                })
            }
            // A Hash replacement maps each matched substring to `hash[match]`
            // (a missing key stringifies to ""), exactly a block that looks the
            // match up -- so it rides the existing block-substitution path.
            RubyValue::Hash(h) => {
                let table = h.clone();
                let p = crate::RProc::new(move |a: &[RubyValue]| {
                    Ok(crate::hash_get(&table, &a[0]))
                });
                if global {
                    crate::regexp_gsub_block(re, &text, &p)
                } else {
                    crate::regexp_sub_block(re, &text, &p)
                }
            }
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!("no implicit conversion of {} into String", crate::builtins::class_name_of(other)),
            )),
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
            let RubyValue::Str(replacement) = &args[1] else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(&args[1])
                    ),
                ));
            };
            let replacement = replacement.lock().to_utf8_lossy().into_owned();
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
        (other, _) => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "wrong argument type {} (expected Regexp) for String#{label}",
                crate::builtins::class_name_of(other)
            ),
        )),
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
    let RubyValue::Int(width) = &args[0] else {
        return Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::class_name_of(&args[0])
            ),
        ));
    };
    let fill = match args.get(1) {
        Some(RubyValue::Str(f)) => f.lock().to_utf8_lossy().into_owned(),
        _ => " ".to_string(),
    };
    if fill.is_empty() {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "zero width padding".to_string(),
        ));
    }
    let len = text.chars().count() as i64;
    let total = (*width - len).max(0) as usize;
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
            Some(RubyValue::Str(s)) => {
                let s = s.lock();
                (s.bytes().to_vec(), s.encoding())
            }
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(other)
                    ),
                ))
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
        assert_eq!(show(slice_bang(&str, &[RubyValue::Int(1), RubyValue::Int(2)], None)), "\"el\"");
        assert_eq!(str.to_display_string(), "hlo");
        let str2 = s("hello");
        assert_eq!(show(slice_bang(&str2, &[s("ll")], None)), "\"ll\"");
        assert_eq!(str2.to_display_string(), "heo");
    }

    #[test]
    fn split_empty_separator_yields_characters() {
        assert_eq!(show(split(&s("hello"), &[s("")], None)), "[\"h\", \"e\", \"l\", \"l\", \"o\"]");
        assert_eq!(show(split(&s("hello"), &[s(""), RubyValue::Int(2)], None)), "[\"h\", \"ello\"]");
    }

    #[test]
    fn index_with_regexp_and_group() {
        let re = RubyValue::Regexp(crate::regexp_new("(\\w+) (\\w+)", false, false, false).unwrap());
        assert_eq!(show(index_op(&s("hello world foo"), &[re.clone()], None)), "\"hello world\"");
        assert_eq!(show(index_op(&s("hello world"), &[re, RubyValue::Int(2)], None)), "\"world\"");
    }

    #[test]
    fn case_and_strip_families_match_the_oracle() {
        assert_eq!(show(capitalize(&s("hello world"), &[], None)), "\"Hello world\"");
        assert_eq!(show(swapcase(&s("HeLLo"), &[], None)), "\"hEllO\"");
        assert_eq!(show(strip(&s("  hi  "), &[], None)), "\"hi\"");
        assert_eq!(show(lstrip(&s("  hi"), &[], None)), "\"hi\"");
    }

    #[test]
    fn split_covers_the_three_separator_shapes() {
        assert_eq!(show(split(&s("a b  c"), &[], None)), "[\"a\", \"b\", \"c\"]");
        assert_eq!(
            show(split(&s("a,b,,c"), &[s(",")], None)),
            "[\"a\", \"b\", \"\", \"c\"]"
        );
        assert_eq!(show(split(&s("hello"), &[s("l")], None)), "[\"he\", \"\", \"o\"]");
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
        assert_eq!(show(tr(&s("hello"), &[s("el"), s("ip")], None)), "\"hippo\"");
        assert_eq!(show(tr(&s("hello"), &[s("a-y"), s("b-z")], None)), "\"ifmmp\"");
        assert_eq!(show(tr(&s("a-b_c"), &[s("-_"), s(" ")], None)), "\"a b c\"");
    }

    #[test]
    fn lenient_conversions_match_the_oracle() {
        assert!(matches!(to_i(&s("42abc"), &[], None).unwrap(), RubyValue::Int(42)));
        assert!(matches!(to_i(&s("abc"), &[], None).unwrap(), RubyValue::Int(0)));
        assert!(matches!(to_i(&s("0x1A"), &[], None).unwrap(), RubyValue::Int(0)));
        assert!(
            matches!(to_i(&s("ff"), &[RubyValue::Int(16)], None).unwrap(), RubyValue::Int(255))
        );
        assert!(
            matches!(to_f(&s("42.5xyz"), &[], None).unwrap(), RubyValue::Float(f) if f == 42.5)
        );
    }

    #[test]
    fn indexing_forms_match_the_oracle() {
        assert_eq!(show(index_op(&s("hello"), &[RubyValue::Int(1)], None)), "\"e\"");
        assert_eq!(
            show(index_op(&s("hello"), &[RubyValue::Int(1), RubyValue::Int(3)], None)),
            "\"ell\""
        );
        let range = RubyValue::Range(
            Some(Box::new(RubyValue::Int(1))),
            Some(Box::new(RubyValue::Int(3))),
            false,
        );
        assert_eq!(show(index_op(&s("hello"), &[range], None)), "\"ell\"");
        assert_eq!(show(index_op(&s("hello"), &[RubyValue::Int(99)], None)), "nil");
    }

    #[test]
    fn padding_and_charset_rows_match_the_oracle() {
        assert_eq!(show(center(&s("hi"), &[RubyValue::Int(7), s("*")], None)), "\"**hi***\"");
        assert_eq!(show(ljust(&s("hi"), &[RubyValue::Int(5), s(".")], None)), "\"hi...\"");
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
