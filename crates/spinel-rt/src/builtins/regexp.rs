//! `Regexp` (CRuby re.c) -- stage B carries only `===` (pattern-match
//! case equality; Kernel's equality default would silently never match).
//! The dynamic-path breadth (`match`/`=~`/`source`/...) rides stage D with
//! String's, sharing `crate::regexp`'s helpers with the static paths.

use crate::builtins::{arity, builtin_methods};
use crate::RubyValue;

builtin_methods! {
    pub(crate) fn lookup;

    "===" => fn case_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_case_eq(&args[0])))
    }
    "encoding" => fn encoding_m(recv, args, _block) {
        arity!(args, 0);
        let RubyValue::Regexp(re) = recv else {
            unreachable!("the Regexp table only dispatches on Regexp receivers")
        };
        let id = crate::builtins::encoding::computed_encoding_of(&re.source);
        Ok(crate::builtins::encoding::encoding_value(id))
    }
    "source" => fn source_m(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp_source(re_of(recv)))
    }
    // `#match?` tests for a match without building a `MatchData` or touching
    // `$~`; `#match` and `#=~` do build one (and set `$~`) via the runtime
    // helpers String's own rows share.
    "match?" => fn match_p(recv, args, _block) {
        arity!(args, 1..=2);
        let Some(h) = str_arg(&args[0]) else { return Ok(RubyValue::Bool(false)) };
        Ok(RubyValue::Bool(crate::regexp_is_match(re_of(recv), &h)))
    }
    "match" => fn match_m(recv, args, _block) {
        arity!(args, 1..=2);
        let Some(h) = str_arg(&args[0]) else { return Ok(RubyValue::Nil) };
        Ok(crate::regexp_match(re_of(recv), &h))
    }
    "=~" => fn match_op(recv, args, _block) {
        arity!(args, 1);
        let Some(h) = str_arg(&args[0]) else { return Ok(RubyValue::Nil) };
        Ok(crate::regexp_match_index(re_of(recv), &h))
    }
    // `casefold?` reports the `/i` flag; `fixed_encoding?` is always false
    // (spinel regexps are encoding-agnostic over the supported set).
    "casefold?" => fn casefold_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(re_of(recv).ignore_case))
    }
    "fixed_encoding?" => fn fixed_encoding_p(recv, args, _block) {
        arity!(args, 0);
        let _ = re_of(recv);
        Ok(RubyValue::Bool(false))
    }
    // `names` lists the named capture groups in order; `named_captures` maps
    // each name to its 1-based capture position(s).
    "names" => fn names_m(recv, args, _block) {
        arity!(args, 0);
        let out = re_of(recv)
            .engine
            .capture_names()
            .into_iter()
            .map(|(n, _)| RubyValue::Str(crate::string_new(n)))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "named_captures" => fn named_captures_m(recv, args, _block) {
        arity!(args, 0);
        let pairs = re_of(recv)
            .engine
            .capture_names()
            .into_iter()
            .map(|(n, i)| {
                (
                    RubyValue::Str(crate::string_new(n)),
                    RubyValue::Array(crate::array_new(vec![RubyValue::Int(i as i64)])),
                )
            })
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    // `#timeout` -- this pattern's per-match timeout; spinel sets none, so
    // it reports the global default (`nil`, "no timeout").
    "timeout" => fn timeout_m(recv, args, _block) {
        arity!(args, 0);
        let _ = re_of(recv);
        Ok(RubyValue::Nil)
    }
    // `#options` -- the `Regexp::` flag bitmask this pattern was built with.
    "options" => fn options_m(recv, args, _block) {
        arity!(args, 0);
        let re = re_of(recv);
        let bits = (re.ignore_case as i64) * IGNORECASE
            + (re.extended as i64) * EXTENDED
            + (re.multiline as i64) * MULTILINE;
        Ok(RubyValue::Int(bits))
    }
}

/// The receiver of a Regexp instance row, already known to be a Regexp.
fn re_of(recv: &RubyValue) -> &crate::RRegexp {
    let RubyValue::Regexp(re) = recv else {
        unreachable!("the Regexp table only dispatches on Regexp receivers")
    };
    re
}

/// A match subject as a `String` -- `nil` and other non-strings simply don't
/// match (`re.match?(nil)` is `false` in Ruby, not a TypeError).
fn str_arg(v: &RubyValue) -> Option<String> {
    match v {
        RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
        _ => None,
    }
}

/// Ruby's `Regexp::` flag bits, the second argument to `Regexp.new`.
const IGNORECASE: i64 = 1;
const EXTENDED: i64 = 2;
const MULTILINE: i64 = 4;

/// Seeds `Regexp::IGNORECASE`/`EXTENDED`/`MULTILINE` -- called once from
/// generated `main()`, alongside the other builtin-constant seeders.
pub fn seed_regexp_constants() {
    let re = spinel_abi::REGEXP_CLASS.0;
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

    // `Regexp.timeout` -- the process-wide default match timeout; spinel
    // enforces none, so it is always `nil`.
    "timeout" => fn timeout_c(_recv, args, _block) {
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
            Some(other) => Err(crate::dispatch::raise_error(
                "TypeError",
                format!("no implicit conversion of {} into Integer", crate::builtins::class_name_of(other)),
            )),
        }
    }

    // `Regexp.escape(str)` / `.quote(str)`: a source-safe literal of `str`.
    "escape" | "quote" => fn escape_m(_recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(s) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("no implicit conversion of {} into String", crate::builtins::class_name_of(&args[0])),
            ));
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
                .map_err(|e| crate::dispatch::raise_error("RegexpError", e));
        }
        let RubyValue::Str(s) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into String",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
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
            .map_err(|e| crate::dispatch::raise_error("RegexpError", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_eq_matches_a_string_subject() {
        let re = RubyValue::Regexp(crate::regexp_new("ab", false, false, false).expect("valid pattern"));
        let s = RubyValue::Str(crate::string_new("cabs".to_string()));
        assert!(matches!(case_eq(&re, &[s], None).unwrap(), RubyValue::Bool(true)));
        let miss = RubyValue::Str(crate::string_new("xyz".to_string()));
        assert!(matches!(case_eq(&re, &[miss], None).unwrap(), RubyValue::Bool(false)));
    }
}
