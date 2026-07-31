//! `Regexp` (CRuby re.c) -- stage B carries only `===` (pattern-match
//! case equality; Kernel's equality default would silently never match).
//! The dynamic-path breadth (`match`/`=~`/`source`/...) rides stage D with
//! String's, sharing `crate::regexp`'s helpers with the static paths.

use crate::RubyValue;
use crate::builtins::{arity, regexp_error};
use zeo_macros::ruby_class;

ruby_class! {
    Regexp = zeo_abi::REGEXP_CLASS < zeo_abi::OBJECT_CLASS;

    // `Regexp.timeout` -- the process-wide default match timeout; zeo
    // enforces none, so it is always `nil`.
    def self."timeout" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Nil)
    }

    // `Regexp.last_match` / `Regexp.last_match(n)` -- the thread-local `$~`
    // (whole MatchData), or its nth capture group when given an index.
    def self."last_match"(_recv, *args, &_block) {
        arity!(args, 0..=1);
        match args.first() {
            None => Ok(crate::lastmatch::last_match()),
            Some(v) => Ok(crate::lastmatch::last_match_group(
                crate::builtins::convert::to_index(v)?.max(0) as usize,
            )),
        }
    }

    // `Regexp.escape(str)` / `.quote(str)`: a source-safe literal of `str`.
    def self."escape" | "quote"(_recv, *args, &_block) {
        arity!(args, 1);
        let s = &crate::builtins::convert::to_rstr(&args[0])?;
        let escaped = escape_regexp_source(&s.lock().to_utf8_lossy());
        Ok(RubyValue::Str(crate::string_new(escaped)))
    }

    // `Regexp.new(str_or_regexp, flags = nil)` / `Regexp.compile(...)`. A
    // Regexp source is copied with its own flags; a string source takes its
    // flags from the second argument -- an Integer bitmask of the three
    // `Regexp::` constants, or `true` (case-insensitive) / `false`/`nil`
    // (none), matching CRuby's historical boolean shorthand.
    def self."new" | "compile"(_recv, *args, &_block) {
        arity!(args, 1..=3);
        // A Regexp source: clone it verbatim (flags and all), ignoring any
        // extra options -- CRuby warns but reuses the original.
        if let RubyValue::Regexp(re) = &args[0] {
            return crate::regexp_new(&re.source, re.ignore_case, re.extended, re.multiline)
                .map(RubyValue::Regexp)
                .map_err(|e| regexp_error!("{e}"));
        }
        let s = &crate::builtins::convert::to_rstr(&args[0])?;
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
    def self."union"(_recv, *args, &_block) {
        let items: Vec<RubyValue> = match args {
            [RubyValue::Array(a)] => a.lock().to_vec(),
            _ => args.to_vec(),
        };
        let source = if items.is_empty() {
            "(?!)".to_string()
        } else {
            let mut parts = Vec::with_capacity(items.len());
            for item in &items {
                match item {
                    RubyValue::Regexp(re) => parts.push(regexp_to_s_string(re)),
                    other => {
                        let s = crate::builtins::convert::to_rstr(other)?;
                        parts.push(escape_regexp_source(&s.lock().to_utf8_lossy()))
                    }
                }
            }
            parts.join("|")
        };
        crate::regexp_new(&source, false, false, false)
            .map(RubyValue::Regexp)
            .map_err(|e| regexp_error!("{e}"))
    }

    // `Regexp.try_convert(obj)` -- `obj` if it is already a Regexp, its
    // `to_regexp` if it defines one, else `nil`. Only a present-and-lying
    // `to_regexp` raises.
    def self."try_convert" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        Ok(crate::builtins::convert::try_convert_value(&args[0], "Regexp", "to_regexp")?
            .unwrap_or(RubyValue::Nil))
    }

    // `Regexp.linear_time?(re_or_str, flags = nil)` -- see the instance method.
    def self."linear_time?"(_recv, *args, &_block) {
        arity!(args, 1..=2);
        let source = match &args[0] {
            RubyValue::Regexp(re) => re.source.clone(),
            other => crate::builtins::convert::to_rstr(other)?
                .lock()
                .to_utf8_lossy()
                .into_owned(),
        };
        Ok(RubyValue::Bool(!has_backreference(&source)))
    }

    def "===" arity 1 (recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_case_eq(&args[0])))
    }
    def "encoding" arity 0 (recv, *args, &_block) {
        arity!(args, 0);
        let RubyValue::Regexp(re) = recv else {
            unreachable!("the Regexp table only dispatches on Regexp receivers")
        };
        let id = crate::builtins::encoding::computed_encoding_of(&re.source);
        Ok(crate::builtins::encoding::encoding_value(id))
    }
    def "source" arity 0 (recv, *args, &_block) {
        arity!(args, 0);
        Ok(crate::regexp_source(re_of(recv)))
    }
    // `#match?` tests for a match without building a `MatchData` or touching
    // `$~`; `#match` and `#=~` do build one (and set `$~`) via the runtime
    // helpers String's own rows share.
    def "match?"(recv, *args, &_block) {
        arity!(args, 1..=2);
        let Some(h) = subject_arg(&args[0])? else { return Ok(RubyValue::Bool(false)) };
        // An optional start position (char offset, end-relative when negative)
        // anchors the search; a position past the end is simply no match.
        let Some(sub) = crate::builtins::string::match_haystack(&h, args.get(1))? else {
            return Ok(RubyValue::Bool(false));
        };
        Ok(RubyValue::Bool(crate::regexp_is_match(re_of(recv), &sub)))
    }
    def "match"(recv, *args, &_block) {
        arity!(args, 1..=2);
        let Some(h) = subject_arg(&args[0])? else { return Ok(RubyValue::Nil) };
        Ok(crate::regexp_match(re_of(recv), &h))
    }
    def "=~" arity 1 (recv, *args, &_block) {
        arity!(args, 1);
        let Some(h) = subject_arg(&args[0])? else { return Ok(RubyValue::Nil) };
        Ok(crate::regexp_match_index(re_of(recv), &h))
    }
    // `casefold?` reports the `/i` flag; `fixed_encoding?` is always false
    // (zeo regexps are encoding-agnostic over the supported set).
    def "casefold?" arity 0 (recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(re_of(recv).ignore_case))
    }
    // A regexp is fixed-encoding when it is tied to a specific encoding rather
    // than the ASCII-agnostic default -- here, when its source carries a
    // non-ASCII (multibyte) character, so `computed_encoding_of` resolves to
    // something other than US-ASCII (`/café/` -> UTF-8 -> true; `/abc/` ->
    // US-ASCII -> false). The flag-forced cases (`/u`, `/n`) are a documented
    // gap: no encoding flag is threaded onto the compiled regexp yet.
    def "fixed_encoding?" arity 0 (recv, *args, &_block) {
        arity!(args, 0);
        let enc = crate::builtins::encoding::computed_encoding_of(&re_of(recv).source);
        Ok(RubyValue::Bool(enc != crate::encoding::US_ASCII))
    }
    // `names` lists the named capture groups in order; `named_captures` maps
    // each name to its 1-based capture position(s).
    def "names" arity 0 (recv, *args, &_block) {
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
    def "named_captures" arity 0 (recv, *args, &_block) {
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
    def "timeout" arity 0 (recv, *args, &_block) {
        arity!(args, 0);
        let _ = re_of(recv);
        Ok(RubyValue::Nil)
    }
    // `#options` -- the `Regexp::` flag bitmask this pattern was built with.
    def "options" arity 0 (recv, *args, &_block) {
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
    def "linear_time?"(recv, *args, &_block) {
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
        RubyValue::Nil => Ok(None),
        // A Symbol matches as its name, which is not an implicit String
        // conversion but a case CRuby's regexp entry points special-case --
        // `delegate.rb` filters `private_instance_methods` with `/…/ =~ m`.
        RubyValue::Symbol(s) => Ok(Some(s.name())),
        other => Ok(Some(
            crate::builtins::convert::to_rstr(other)?
                .lock()
                .to_utf8_lossy()
                .into_owned(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::REGEXP_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    #[test]
    fn case_eq_matches_a_string_subject() {
        let re =
            RubyValue::Regexp(crate::regexp_new("ab", false, false, false).expect("valid pattern"));
        let s = RubyValue::Str(crate::string_new("cabs".to_string()));
        assert!(matches!(
            imethod("===")(&re, &[s], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let miss = RubyValue::Str(crate::string_new("xyz".to_string()));
        assert!(matches!(
            imethod("===")(&re, &[miss], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }
}
