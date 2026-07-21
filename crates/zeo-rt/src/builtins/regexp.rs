//! `Regexp` (CRuby re.c) -- stage B carries only `===` (pattern-match
//! case equality; Kernel's equality default would silently never match).
//! The dynamic-path breadth (`match`/`=~`/`source`/...) rides stage D with
//! String's, sharing `crate::regexp`'s helpers with the static paths.

use crate::RubyValue;
use crate::builtins::{arity, builtin_methods, regexp_error, type_error};

builtin_methods! {
    pub(crate) fn lookup;

    "==="[1] => fn case_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_case_eq(&args[0])))
    }
    "encoding"[0] => fn encoding_m(recv, args, _block) {
        arity!(args, 0);
        let RubyValue::Regexp(re) = recv else {
            unreachable!("the Regexp table only dispatches on Regexp receivers")
        };
        let id = crate::builtins::encoding::computed_encoding_of(&re.source);
        Ok(crate::builtins::encoding::encoding_value(id))
    }
    "source"[0] => fn source_m(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp_source(re_of(recv)))
    }
    // `#match?` tests for a match without building a `MatchData` or touching
    // `$~`; `#match` and `#=~` do build one (and set `$~`) via the runtime
    // helpers String's own rows share.
    "match?" => fn match_p(recv, args, _block) {
        arity!(args, 1..=2);
        let Some(h) = subject_arg(&args[0])? else { return Ok(RubyValue::Bool(false)) };
        // An optional start position (char offset, end-relative when negative)
        // anchors the search; a position past the end is simply no match.
        let Some(sub) = crate::builtins::string::match_haystack(&h, args.get(1))? else {
            return Ok(RubyValue::Bool(false));
        };
        Ok(RubyValue::Bool(crate::regexp_is_match(re_of(recv), &sub)))
    }
    "match" => fn match_m(recv, args, _block) {
        arity!(args, 1..=2);
        let Some(h) = subject_arg(&args[0])? else { return Ok(RubyValue::Nil) };
        Ok(crate::regexp_match(re_of(recv), &h))
    }
    "=~"[1] => fn match_op(recv, args, _block) {
        arity!(args, 1);
        let Some(h) = subject_arg(&args[0])? else { return Ok(RubyValue::Nil) };
        Ok(crate::regexp_match_index(re_of(recv), &h))
    }
    // `casefold?` reports the `/i` flag; `fixed_encoding?` is always false
    // (zeo regexps are encoding-agnostic over the supported set).
    "casefold?"[0] => fn casefold_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(re_of(recv).ignore_case))
    }
    // A regexp is fixed-encoding when it is tied to a specific encoding rather
    // than the ASCII-agnostic default -- here, when its source carries a
    // non-ASCII (multibyte) character, so `computed_encoding_of` resolves to
    // something other than US-ASCII (`/café/` -> UTF-8 -> true; `/abc/` ->
    // US-ASCII -> false). The flag-forced cases (`/u`, `/n`) are a documented
    // gap: no encoding flag is threaded onto the compiled regexp yet.
    "fixed_encoding?"[0] => fn fixed_encoding_p(recv, args, _block) {
        arity!(args, 0);
        let enc = crate::builtins::encoding::computed_encoding_of(&re_of(recv).source);
        Ok(RubyValue::Bool(enc != crate::encoding::US_ASCII))
    }
    // `names` lists the named capture groups in order; `named_captures` maps
    // each name to its 1-based capture position(s).
    "names"[0] => fn names_m(recv, args, _block) {
        arity!(args, 0);
        // Each distinct name once, in first-appearance order (a name reused by
        // several groups -- `/(?<a>x)(?<a>z)/` -- lists once, as CRuby does).
        // Parsed from the source, since the engine collapses repeated names.
        let mut seen: Vec<String> = Vec::new();
        for (n, _) in crate::regexp::named_group_positions(&re_of(recv).source) {
            if !seen.contains(&n) {
                seen.push(n);
            }
        }
        let out = seen.into_iter().map(|n| RubyValue::Str(crate::string_new(n))).collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "named_captures"[0] => fn named_captures_m(recv, args, _block) {
        arity!(args, 0);
        // Map each name to the LIST of its 1-based group indices, in
        // first-appearance order: a name shared by several groups
        // (`/(?<a>x)(?<a>z)/`) collects all of them (`{"a" => [1, 2]}`), not
        // just the last -- CRuby's `named_captures`.
        let mut order: Vec<String> = Vec::new();
        let mut indices: std::collections::HashMap<String, Vec<RubyValue>> = std::collections::HashMap::new();
        for (n, i) in crate::regexp::named_group_positions(&re_of(recv).source) {
            indices
                .entry(n.clone())
                .or_insert_with(|| {
                    order.push(n.clone());
                    Vec::new()
                })
                .push(RubyValue::Int(i as i64));
        }
        let pairs = order
            .into_iter()
            .map(|n| {
                let idxs = indices.remove(&n).expect("every ordered name has indices");
                (RubyValue::Str(crate::string_new(n)), RubyValue::Array(crate::array_new(idxs)))
            })
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    // `#timeout` -- this pattern's per-match timeout; zeo sets none, so
    // it reports the global default (`nil`, "no timeout").
    "timeout"[0] => fn timeout_m(recv, args, _block) {
        arity!(args, 0);
        let _ = re_of(recv);
        Ok(RubyValue::Nil)
    }
    // `#options` -- the `Regexp::` flag bitmask this pattern was built with.
    "options"[0] => fn options_m(recv, args, _block) {
        arity!(args, 0);
        let re = re_of(recv);
        let bits = (re.ignore_case as i64) * IGNORECASE
            + (re.extended as i64) * EXTENDED
            + (re.multiline as i64) * MULTILINE;
        Ok(RubyValue::Int(bits))
    }
    // `#linear_time?` -- whether matching is guaranteed linear-time. True
    // unless the pattern uses a backreference (lookaround and nested
    // quantifiers stay linear); oracle-verified.
    "linear_time?" => fn linear_time_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(!has_backreference(&re_of(recv).source)))
    }
}

/// True if `source` contains a backreference (`\1`..`\9` or `\k<name>`/
/// `\k'name'`) -- the only construct that forces non-linear matching in
/// `Regexp.linear_time?`. A backslash always consumes the next character, so
/// an escaped backslash (`\\1`) is a literal, not a backref.
fn has_backreference(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    // Inside a character class, `\1`/`\k` are octal/literal, NOT backreferences
    // (`/[\1]/` is linear); only a real backref OUTSIDE any class counts. `]`
    // closes the class unless it is the first member, and `\]` is an escaped
    // literal that does not close it.
    let mut in_class = false;
    let mut class_start = false;
    while i < bytes.len() {
        let c = bytes[i];
        if !in_class {
            match c {
                b'[' => {
                    in_class = true;
                    class_start = true;
                    i += 1;
                }
                b'\\' if i + 1 < bytes.len() => {
                    let n = bytes[i + 1];
                    if n.is_ascii_digit() && n != b'0' {
                        return true;
                    }
                    if n == b'k' && matches!(bytes.get(i + 2), Some(b'<' | b'\'')) {
                        return true;
                    }
                    i += 2;
                }
                _ => i += 1,
            }
        } else {
            match c {
                b']' if !class_start => {
                    in_class = false;
                    i += 1;
                }
                b'^' if class_start => i += 1,
                b'\\' if i + 1 < bytes.len() => {
                    class_start = false;
                    i += 2;
                }
                _ => {
                    class_start = false;
                    i += 1;
                }
            }
        }
    }
    false
}

/// A `Regexp`'s `#to_s` (`(?-mix:src)`) as a plain `String` -- `Regexp.union`
/// embeds each member regexp this way, preserving its own flags.
fn regexp_to_s_string(re: &crate::RRegexp) -> String {
    match crate::regexp::regexp_to_s(re) {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => unreachable!("regexp_to_s always returns a Str"),
    }
}

/// The receiver of a Regexp instance row, already known to be a Regexp.
fn re_of(recv: &RubyValue) -> &crate::RRegexp {
    let RubyValue::Regexp(re) = recv else {
        unreachable!("the Regexp table only dispatches on Regexp receivers")
    };
    re
}

/// The subject of `Regexp#=~`/`#match`/`#match?`: a String matches, `nil`
/// answers "no match" (never raises), and any other type raises TypeError --
/// CRuby's rule (`/p/ =~ 5` -> TypeError, not a silent non-match).
fn subject_arg(v: &RubyValue) -> Result<Option<String>, crate::Signal> {
    match v {
        RubyValue::Str(s) => Ok(Some(s.lock().to_utf8_lossy().into_owned())),
        RubyValue::Nil => Ok(None),
        other => Err(type_error!(
            "no implicit conversion of {} into String",
            crate::builtins::convert_name_of(other)
        )),
    }
}

/// Ruby's `Regexp::` flag bits, the second argument to `Regexp.new`.
const IGNORECASE: i64 = 1;
const EXTENDED: i64 = 2;
const MULTILINE: i64 = 4;

/// Seeds `Regexp::IGNORECASE`/`EXTENDED`/`MULTILINE` -- called once from
/// generated `main()`, alongside the other builtin-constant seeders.
pub fn seed_regexp_constants() {
    let re = zeo_abi::REGEXP_CLASS.0;
    crate::const_set(re, "IGNORECASE", RubyValue::Int(IGNORECASE));
    crate::const_set(re, "EXTENDED", RubyValue::Int(EXTENDED));
    crate::const_set(re, "MULTILINE", RubyValue::Int(MULTILINE));
}

/// `Regexp.escape`/`.quote`: backslash-escapes every regex metacharacter (and
/// renders control characters as their `\t`/`\n`/... escapes) so the result
/// matches the input literally -- CRuby's `rb_reg_quote` character set exactly.
fn escape_regexp_source(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '[' | ']' | '(' | ')' | '{' | '}' | '.' | '?' | '+' | '*' | '^' | '$' | '|' | '#'
            | '-' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' ' => out.push_str("\\ "),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{0b}' => out.push_str("\\v"),
            _ => out.push(c),
        }
    }
    out
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Regexp.timeout` -- the process-wide default match timeout; zeo
    // enforces none, so it is always `nil`.
    "timeout"[0] => fn timeout_c(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Nil)
    }

    // `Regexp.last_match` / `Regexp.last_match(n)` -- the thread-local `$~`
    // (whole MatchData), or its nth capture group when given an index.
    "last_match" => fn last_match_c(_recv, args, _block) {
        arity!(args, 0..=1);
        match args.first() {
            None => Ok(crate::lastmatch::last_match()),
            Some(RubyValue::Int(n)) => Ok(crate::lastmatch::last_match_group((*n).max(0) as usize)),
            Some(other) => Err(type_error!("no implicit conversion of {} into Integer", crate::builtins::convert_name_of(other))),
        }
    }

    // `Regexp.escape(str)` / `.quote(str)`: a source-safe literal of `str`.
    "escape" | "quote" => fn escape_m(_recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(s) = &args[0] else {
            return Err(type_error!("no implicit conversion of {} into String", crate::builtins::convert_name_of(&args[0])));
        };
        let escaped = escape_regexp_source(&s.lock().to_utf8_lossy());
        Ok(RubyValue::Str(crate::string_new(escaped)))
    }

    // `Regexp.new(str_or_regexp, flags = nil)` / `Regexp.compile(...)`. A
    // Regexp source is copied with its own flags; a string source takes its
    // flags from the second argument -- an Integer bitmask of the three
    // `Regexp::` constants, or `true` (case-insensitive) / `false`/`nil`
    // (none), matching CRuby's historical boolean shorthand.
    "new" | "compile" => fn regexp_new_m(_recv, args, _block) {
        arity!(args, 1..=3);
        // A Regexp source: clone it verbatim (flags and all), ignoring any
        // extra options -- CRuby warns but reuses the original.
        if let RubyValue::Regexp(re) = &args[0] {
            return crate::regexp_new(&re.source, re.ignore_case, re.extended, re.multiline)
                .map(RubyValue::Regexp)
                .map_err(|e| regexp_error!("{e}"));
        }
        let RubyValue::Str(s) = &args[0] else {
            return Err(type_error!("no implicit conversion of {} into String",
                    crate::builtins::convert_name_of(&args[0])));
        };
        let source = s.lock().to_utf8_lossy().into_owned();
        let (ignore_case, extended, multiline) = match args.get(1) {
            None | Some(RubyValue::Nil) | Some(RubyValue::Bool(false)) => (false, false, false),
            Some(RubyValue::Bool(true)) => (true, false, false),
            Some(RubyValue::Int(f)) => {
                (f & IGNORECASE != 0, f & EXTENDED != 0, f & MULTILINE != 0)
            }
            Some(other) => (other.truthy(), false, false),
        };
        crate::regexp_new(&source, ignore_case, extended, multiline)
            .map(RubyValue::Regexp)
            .map_err(|e| regexp_error!("{e}"))
    }

    // `Regexp.union(pat, ...)` / `Regexp.union([pat, ...])`: an alternation of
    // the patterns. A String member is escaped; a Regexp member keeps its own
    // flags via its `(?-mix:src)` form. Empty -> the never-matching `(?!)`.
    "union" => fn union_c(_recv, args, _block) {
        let items: Vec<RubyValue> = match args {
            [RubyValue::Array(a)] => a.lock().clone(),
            _ => args.to_vec(),
        };
        let source = if items.is_empty() {
            "(?!)".to_string()
        } else {
            let mut parts = Vec::with_capacity(items.len());
            for item in &items {
                match item {
                    RubyValue::Regexp(re) => parts.push(regexp_to_s_string(re)),
                    RubyValue::Str(s) => {
                        parts.push(escape_regexp_source(&s.lock().to_utf8_lossy()))
                    }
                    other => {
                        return Err(type_error!("no implicit conversion of {} into String", crate::builtins::convert_name_of(other)))
                    }
                }
            }
            parts.join("|")
        };
        crate::regexp_new(&source, false, false, false)
            .map(RubyValue::Regexp)
            .map_err(|e| regexp_error!("{e}"))
    }

    // `Regexp.try_convert(obj)` -- `obj` if it is already a Regexp, else `nil`
    // (never raises, unlike a coercion).
    "try_convert" => fn try_convert_c(_recv, args, _block) {
        arity!(args, 1);
        Ok(match &args[0] {
            RubyValue::Regexp(_) => args[0].clone(),
            _ => RubyValue::Nil,
        })
    }

    // `Regexp.linear_time?(re_or_str, flags = nil)` -- see the instance method.
    "linear_time?" => fn linear_time_c(_recv, args, _block) {
        arity!(args, 1..=2);
        let source = match &args[0] {
            RubyValue::Regexp(re) => re.source.clone(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => {
                return Err(type_error!("no implicit conversion of {} into String", crate::builtins::convert_name_of(other)))
            }
        };
        Ok(RubyValue::Bool(!has_backreference(&source)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_eq_matches_a_string_subject() {
        let re =
            RubyValue::Regexp(crate::regexp_new("ab", false, false, false).expect("valid pattern"));
        let s = RubyValue::Str(crate::string_new("cabs".to_string()));
        assert!(matches!(
            case_eq(&re, &[s], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let miss = RubyValue::Str(crate::string_new("xyz".to_string()));
        assert!(matches!(
            case_eq(&re, &[miss], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }
}
