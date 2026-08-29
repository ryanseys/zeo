//! `String`'s splitting half: `partition`, the three `split` separator
//! shapes, `lines`, and the charset-set helpers behind `count`/`delete`/
//! `squeeze`. The `ruby_class!` rows stay in `mod.rs` and call these by bare
//! name.

use super::*;

/// `partition`/`rpartition`'s three-part split around a String or Regexp
/// separator: `[before, match, after]`, and (`whole`, `""`, `""`) with the
/// empty parts on `partition`'s tail / `rpartition`'s head when there's no
/// match (`from_end` picks which). Char-index based so multibyte input keeps
/// its boundaries.
pub(super) fn str_partition(
    text: &str,
    sep: &RubyValue,
    from_end: bool,
) -> Result<[RubyValue; 3], Signal> {
    let husk = crate::regexp::husk_payload(sep);
    let sep = husk.as_ref().unwrap_or(sep);
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

/// The `count`/`delete` char-set arguments as `(chars, negated)` specs: a
/// leading `^` negates (a bare `"^"` stays literal), `a-z` expands to a
/// range. Zero arguments is CRuby's `ArgumentError`.
#[allow(clippy::type_complexity)]
pub(super) fn charset_specs(
    args: &[RubyValue],
) -> Result<Vec<(std::collections::HashSet<char>, bool)>, Signal> {
    crate::builtins::check_arity(args.len(), 1, None)?;
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
pub(super) fn in_all_charsets(c: char, sets: &[(std::collections::HashSet<char>, bool)]) -> bool {
    sets.iter()
        .all(|(set, negated)| set.contains(&c) != *negated)
}

/// `split`'s whitespace (awk) mode: leading whitespace skipped, fields split
/// on whitespace runs. A positive `limit` keeps the tail (internal
/// whitespace and all) whole as the final field.
pub(super) fn awk_split(text: &str, limit: i64) -> Vec<String> {
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
pub(super) fn str_array(parts: Vec<String>) -> RubyValue {
    RubyValue::Array(crate::array_new(parts.into_iter().map(str_value).collect()))
}

/// `each_line(sep)`: like `split_lines` but on an arbitrary separator, each
/// piece keeping its trailing separator.
/// `lines`/`each_line`'s shared split: an optional `sep` positional and a
/// `chomp:` keyword (a trailing Hash). Split keeps the separator unless chomped;
/// the default separator also strips a preceding `\r` when chomping.
pub(super) fn lines_from_args(
    text: &str,
    sep: Option<&RubyValue>,
    opts: Option<&RubyValue>,
) -> Vec<RubyValue> {
    // `chomp:` is a flag, so any truthy value arms it -- not `true` alone.
    let mut chomp = false;
    if let Some(RubyValue::Hash(h)) = opts {
        chomp = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("chomp"))).truthy();
    }
    let sep = match sep {
        Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
        _ => "\n".to_string(),
    };
    // PARAGRAPH MODE: an empty separator is not "no separator" -- ruby splits
    // on a blank line, keeping the "\n\n" that ended each paragraph and then
    // DISCARDING any further consecutive newlines, so "a\n\n\nb" is
    // ["a\n\n", "b"]. Answering the whole string was the old behaviour.
    if sep.is_empty() {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let (mut start, mut i) = (0usize, 0usize);
        while i < bytes.len() {
            if bytes[i] == b'\n' && bytes.get(i + 1) == Some(&b'\n') {
                let mut end = i + 2;
                // Every additional newline belongs to the SEPARATOR run and is
                // dropped, not carried into the next paragraph.
                while bytes.get(end) == Some(&b'\n') {
                    end += 1;
                }
                let piece = if chomp {
                    &text[start..i]
                } else {
                    &text[start..i + 2]
                };
                out.push(RubyValue::Str(crate::string_new(piece.to_string())));
                start = end;
                i = end;
            } else {
                i += 1;
            }
        }
        if start < text.len() {
            out.push(RubyValue::Str(crate::string_new(text[start..].to_string())));
        }
        return out;
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

/// `lines`' separator-keeping splitter.
/// Split into lines, KEEPING each terminating newline (a trailing fragment
/// with no newline is still a line) -- `String#each_line`/`#lines`, and
/// `File.readlines`, which is the same rule applied to a whole file.
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
