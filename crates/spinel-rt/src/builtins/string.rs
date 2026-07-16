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

/// A `[]`-style char-index extraction shared by `[]`/`slice`/`index`
/// helpers: `chars` is the receiver's chars, negative starts count from
/// the end.
fn char_slice(chars: &[char], start: i64, len: i64) -> Option<String> {
    let n = chars.len() as i64;
    let start = if start < 0 { start + n } else { start };
    if start < 0 || start > n || len < 0 {
        return None;
    }
    let end = (start + len).min(n);
    Some(chars[start as usize..end as usize].iter().collect())
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
    // Byte accessors over the UTF-8 representation (the plan's encoding
    // engine gives these real per-encoding semantics later; UTF-8 bytes ARE
    // the bytes until then).
    "bytesize" => fn bytesize(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_str!(recv).lock().len() as i64))
    }
    "bytes" => fn bytes(recv, args, _block) {
        arity!(args, 0);
        let out = recv_str!(recv)
            .lock()
            .bytes()
            .map(|b| RubyValue::Int(b as i64))
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
        let bytes: Vec<u8> = recv_str!(recv).lock().bytes().collect();
        for b in bytes {
            p.call(&[RubyValue::Int(b as i64)])?;
        }
        Ok(recv.clone())
    }
    "getbyte" => fn getbyte(recv, args, _block) {
        arity!(args, 1);
        let i = arg_int!(args, 0);
        let s = recv_str!(recv).lock().clone();
        let idx = if i < 0 { i + s.len() as i64 } else { i };
        Ok(if idx >= 0 && (idx as usize) < s.len() {
            RubyValue::Int(s.as_bytes()[idx as usize] as i64)
        } else {
            RubyValue::Nil
        })
    }
    "include?" => fn include_p(recv, args, _block) {
        arity!(args, 1);
        let needle = arg_str!(args, 0);
        let found = recv_str!(recv).lock().contains(&*needle.lock());
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
            RubyValue::Str(s) => s.lock().clone(),
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
        recv_str!(recv).lock().push_str(&addition);
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
        Ok(str_value(recv_str!(recv).lock().repeat(n as usize)))
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
            a.cmp(&b)
        };
        Ok(RubyValue::Int(ord as i64))
    }
    "==" | "eql?" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "upcase" => fn upcase(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().to_uppercase()))
    }
    "downcase" => fn downcase(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().to_lowercase()))
    }
    "capitalize" => fn capitalize(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(capitalize_str(&recv_str!(recv).lock())))
    }
    "swapcase" => fn swapcase(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(swapcase_str(&recv_str!(recv).lock())))
    }
    "strip" => fn strip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().trim().to_string()))
    }
    "lstrip" => fn lstrip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().trim_start().to_string()))
    }
    "rstrip" => fn rstrip(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(recv_str!(recv).lock().trim_end().to_string()))
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
            &recv_str!(recv).lock(),
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
    "each_line" => fn each_line(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_line", args, block);
        let ls = split_lines(&recv_str!(recv).lock());
        for l in ls {
            p.call(&[l])?;
        }
        Ok(recv.clone())
    }
    // `split`: no-arg/nil = whitespace runs (leading skipped); a String
    // separator keeps interior empties, drops trailing ones; a Regexp
    // separator delegates to the shared regexp splitter.
    "split" => fn split(recv, args, _block) {
        arity!(args, 0..=1);
        let text = recv_str!(recv).lock().clone();
        match args.first() {
            None | Some(RubyValue::Nil) => {
                let out = text
                    .split_whitespace()
                    .map(|p| str_value(p.to_string()))
                    .collect();
                Ok(RubyValue::Array(crate::array_new(out)))
            }
            Some(RubyValue::Str(sep)) => {
                let sep = sep.lock().clone();
                if sep == " " {
                    // The one magic separator: a single space means
                    // whitespace-run splitting, real Ruby's awk rule.
                    let out = text
                        .split_whitespace()
                        .map(|p| str_value(p.to_string()))
                        .collect();
                    return Ok(RubyValue::Array(crate::array_new(out)));
                }
                let mut parts: Vec<&str> = text.split(sep.as_str()).collect();
                while parts.last() == Some(&"") {
                    parts.pop();
                }
                Ok(RubyValue::Array(crate::array_new(
                    parts.into_iter().map(|p| str_value(p.to_string())).collect(),
                )))
            }
            Some(RubyValue::Regexp(re)) => Ok(crate::regexp_split(re, &text)),
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
        let text = recv_str!(recv).lock().clone();
        let out = match args.first() {
            Some(RubyValue::Str(suffix)) => {
                let suffix = suffix.lock().clone();
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
        let text = recv_str!(recv).lock().clone();
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
        Ok(str_value(recv_str!(recv).lock().chars().rev().collect()))
    }
    "index" => fn index(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().clone();
        match &args[0] {
            RubyValue::Str(needle) => {
                let needle = needle.lock().clone();
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
    "rindex" => fn rindex(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().clone();
        let needle = arg_str!(args, 0).lock().clone();
        Ok(match text.rfind(&needle) {
            Some(byte_pos) => RubyValue::Int(text[..byte_pos].chars().count() as i64),
            None => RubyValue::Nil,
        })
    }
    // The `[]`/`slice` forms: Int, (Int, Int), Range, String.
    "[]" | "slice" => fn index_op(recv, args, _block) {
        arity!(args, 1..=2);
        let text = recv_str!(recv).lock().clone();
        let cs: Vec<char> = text.chars().collect();
        if args.len() == 2 {
            let (start, len) = (arg_int!(args, 0), arg_int!(args, 1));
            return Ok(match char_slice(&cs, start, len) {
                Some(s) => str_value(s),
                None => RubyValue::Nil,
            });
        }
        match &args[0] {
            RubyValue::Int(i) => {
                let n = cs.len() as i64;
                let i = if *i < 0 { i + n } else { *i };
                Ok(if (0..n).contains(&i) {
                    str_value(cs[i as usize].to_string())
                } else {
                    RubyValue::Nil
                })
            }
            RubyValue::Range(start, end, exclusive) => {
                let n = cs.len() as i64;
                let s = match start.as_deref() {
                    Some(RubyValue::Int(v)) => {
                        if *v < 0 { v + n } else { *v }
                    }
                    None => 0,
                    _ => return Ok(RubyValue::Nil),
                };
                let e = match end.as_deref() {
                    Some(RubyValue::Int(v)) => {
                        let v = if *v < 0 { v + n } else { *v };
                        if *exclusive { v - 1 } else { v }
                    }
                    None => n - 1,
                    _ => return Ok(RubyValue::Nil),
                };
                Ok(match char_slice(&cs, s, (e - s + 1).max(0)) {
                    Some(sub) => str_value(sub),
                    None => RubyValue::Nil,
                })
            }
            RubyValue::Str(sub) => {
                let sub = sub.lock().clone();
                Ok(if text.contains(&sub) {
                    str_value(sub)
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
    // `sub`/`gsub`: String or Regexp pattern; String replacement or block.
    "sub" => fn sub(recv, args, block) {
        sub_gsub(recv, args, block, false)
    }
    "gsub" => fn gsub(recv, args, block) {
        sub_gsub(recv, args, block, true)
    }
    "start_with?" => fn start_with_p(recv, args, _block) {
        let text = recv_str!(recv).lock().clone();
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
            if text.starts_with(&*prefix.lock()) {
                return Ok(RubyValue::Bool(true));
            }
        }
        Ok(RubyValue::Bool(false))
    }
    "end_with?" => fn end_with_p(recv, args, _block) {
        let text = recv_str!(recv).lock().clone();
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
            if text.ends_with(&*suffix.lock()) {
                return Ok(RubyValue::Bool(true));
            }
        }
        Ok(RubyValue::Bool(false))
    }
    // `tr(from, to)` with `a-z` range expansion; a short `to` repeats its
    // last character (CRuby's rule).
    "tr" => fn tr(recv, args, _block) {
        arity!(args, 2);
        let from = expand_charset(&arg_str!(args, 0).lock());
        let to = expand_charset(&arg_str!(args, 1).lock());
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
    "delete" => fn delete(recv, args, _block) {
        arity!(args, 1);
        let set = expand_charset(&arg_str!(args, 0).lock());
        let out = recv_str!(recv)
            .lock()
            .chars()
            .filter(|c| !set.contains(c))
            .collect();
        Ok(str_value(out))
    }
    "squeeze" => fn squeeze(recv, args, _block) {
        arity!(args, 0..=1);
        let set = args
            .first()
            .map(|a| match a {
                RubyValue::Str(s) => expand_charset(&s.lock()),
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
        arity!(args, 1);
        let set = expand_charset(&arg_str!(args, 0).lock());
        let n = recv_str!(recv)
            .lock()
            .chars()
            .filter(|c| set.contains(c))
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
        Ok(lenient_to_i(&recv_str!(recv).lock(), base))
    }
    // Lenient like `to_i`: the longest valid leading float, else 0.0.
    "to_f" => fn to_f(recv, args, _block) {
        arity!(args, 0);
        let text = recv_str!(recv).lock().clone();
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
            &recv_str!(recv).lock(),
        )))
    }
    "succ" | "next" => fn succ(recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(succ_str(&recv_str!(recv).lock())))
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
        let addition = arg_str!(args, 1).lock().clone();
        let handle = recv_str!(recv);
        let mut guard = handle.lock();
        let n = guard.chars().count() as i64;
        let at = if at < 0 { at + n + 1 } else { at };
        if at < 0 || at > n {
            return Err(crate::dispatch::raise_error(
                "IndexError",
                format!("index {} out of string", arg_int!(args, 0)),
            ));
        }
        let byte_pos = guard
            .char_indices()
            .nth(at as usize)
            .map_or(guard.len(), |(b, _)| b);
        guard.insert_str(byte_pos, &addition);
        drop(guard);
        Ok(recv.clone())
    }
    "prepend" => fn prepend(recv, args, _block) {
        arity!(args, 1);
        let addition = arg_str!(args, 0).lock().clone();
        let handle = recv_str!(recv);
        let mut guard = handle.lock();
        guard.insert_str(0, &addition);
        drop(guard);
        Ok(recv.clone())
    }
    "replace" => fn replace(recv, args, _block) {
        arity!(args, 1);
        let new_text = arg_str!(args, 0).lock().clone();
        *recv_str!(recv).lock() = new_text;
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
            &recv_str!(recv).lock(),
            &format_args,
        )?))
    }
    "=~" => fn match_op(recv, args, _block) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Regexp(re) => {
                Ok(crate::regexp_match_index(re, &recv_str!(recv).lock()))
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
    "match" => fn match_m(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().clone();
        match &args[0] {
            RubyValue::Regexp(re) => Ok(crate::regexp_match(re, &text)),
            RubyValue::Str(pat) => {
                let re = crate::regexp_new(&pat.lock(), false, false, false)
                    .map_err(|e| crate::dispatch::raise_error("RegexpError", e))?;
                Ok(crate::regexp_match(&re, &text))
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
    "match?" => fn match_p(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().clone();
        match &args[0] {
            RubyValue::Regexp(re) => Ok(RubyValue::Bool(crate::regexp_is_match(re, &text))),
            RubyValue::Str(pat) => {
                let re = crate::regexp_new(&pat.lock(), false, false, false)
                    .map_err(|e| crate::dispatch::raise_error("RegexpError", e))?;
                Ok(RubyValue::Bool(crate::regexp_is_match(&re, &text)))
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
    "scan" => fn scan(recv, args, _block) {
        arity!(args, 1);
        let text = recv_str!(recv).lock().clone();
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
fn split_lines(text: &str) -> Vec<RubyValue> {
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
        RubyValue::Str(s) => s.lock().clone(),
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
        (RubyValue::Regexp(re), None) => {
            let RubyValue::Str(replacement) = &args[1] else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(&args[1])
                    ),
                ));
            };
            let replacement = replacement.lock().clone();
            Ok(if global {
                crate::regexp_gsub(re, &text, &replacement)
            } else {
                crate::regexp_sub(re, &text, &replacement)
            })
        }
        (RubyValue::Regexp(re), Some(p)) => {
            if global {
                crate::regexp_gsub_block(re, &text, &p)
            } else {
                crate::regexp_sub_block(re, &text, &p)
            }
        }
        (RubyValue::Str(pattern), None) => {
            let pattern = pattern.lock().clone();
            let RubyValue::Str(replacement) = &args[1] else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(&args[1])
                    ),
                ));
            };
            let replacement = replacement.lock().clone();
            Ok(RubyValue::Str(crate::string_new(if global {
                text.replace(&pattern, &replacement)
            } else {
                text.replacen(&pattern, &replacement, 1)
            })))
        }
        (RubyValue::Str(pattern), Some(p)) => {
            let pattern = pattern.lock().clone();
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
        RubyValue::Str(s) => s.lock().clone(),
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
        Some(RubyValue::Str(f)) => f.lock().clone(),
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
