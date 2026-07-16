//! `Regexp`/`MatchData` (Phase 12.7) -- backed by the `regex` crate, not
//! Ruby's own Onigmo engine. A real, documented semantic gap versus real
//! Ruby, not silent wrongness: no backreferences INSIDE a pattern (`\1` as
//! part of what's being matched -- as opposed to a `gsub`/`sub` REPLACEMENT
//! string, where numbered/whole-match/pre-match/post-match backreferences
//! ARE supported, see `expand_replacement` below) and no lookaround
//! (`(?=...)`/`(?!...)`/`(?<=...)`/`(?<!...)`), since `regex` is a
//! guaranteed-linear-time engine that deliberately doesn't support either.
//! Unicode property syntax also differs from Onigmo's. Same "approximation,
//! not silent wrongness, documented" posture this codebase already applies
//! to `defined?`/`Hash#inspect`.
//!
//! `RegexpData`/`MatchDataInner` need no `Mutex` at all (unlike
//! `RArray`/`RHash`/`RStr`): both are immutable after construction, and
//! `regex::Regex` is already `Send + Sync` on its own -- an `Arc` alone gives
//! the same cheap-clone-shared-identity value semantics every other
//! `RubyValue` payload uses, with no interior mutability to guard.

use crate::collections::{array_new, hash_new, string_new};
use crate::{RProc, RubyValue, Signal};
use std::sync::Arc;

pub struct RegexpData {
    pub compiled: regex::Regex,
    pub source: String,
    pub ignore_case: bool,
    pub extended: bool,
    pub multiline: bool,
}

pub type RRegexp = Arc<RegexpData>;

/// A successful `Regexp#match`/`String#match` result. `groups[0]` is always
/// the whole match (real Ruby: `MatchData#[0]` == the whole matched
/// substring, `#[1..]` are the captures) -- byte offsets into `haystack`,
/// converted to Ruby's char-indexed convention only where a caller actually
/// needs an index (`pre_match`/`post_match`/`[]` all just slice `haystack`
/// directly, needing no conversion).
pub struct MatchDataInner {
    pub haystack: String,
    pub groups: Vec<Option<(usize, usize)>>,
    pub names: Vec<(String, usize)>,
}

pub type RMatchData = Arc<MatchDataInner>;

/// Builds a real `regex::Regex`, translating Ruby's flag semantics --
/// crucially, Ruby's `^`/`$` ALWAYS match at line boundaries (there is no
/// separate "multi-line mode" opt-in the way most other regex flavors work),
/// so `multi_line(true)` is unconditional here, independent of any of the
/// three `RegexpFlags`. Ruby's `/m` flag instead makes `.` match a newline
/// too -- `regex`'s `dot_matches_new_line`, NOT its own `multi_line` (a
/// same-named-but-different-meaning trap between the two flag vocabularies).
/// `/x` maps directly to `ignore_whitespace`. Returns a plain `String` error
/// message (not a `Signal`/`RubyValue`) -- constructing the catchable
/// `RegexpError` VALUE needs the class registry, which only generated
/// `spinelc` codegen has access to (see `codegen::collections::emit_regexp_lit`).
pub fn regexp_new(source: &str, ignore_case: bool, extended: bool, multiline: bool) -> Result<RRegexp, String> {
    let compiled = regex::RegexBuilder::new(source)
        .case_insensitive(ignore_case)
        .ignore_whitespace(extended)
        .dot_matches_new_line(multiline)
        .multi_line(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(RegexpData {
        compiled,
        source: source.to_string(),
        ignore_case,
        extended,
        multiline,
    }))
}

fn build_match_data(re: &RRegexp, haystack: &str, caps: &regex::Captures) -> RMatchData {
    let groups = (0..caps.len()).map(|i| caps.get(i).map(|m| (m.start(), m.end()))).collect();
    let names = re
        .compiled
        .capture_names()
        .enumerate()
        .filter_map(|(i, n)| n.map(|n| (n.to_string(), i)))
        .collect();
    Arc::new(MatchDataInner {
        haystack: haystack.to_string(),
        groups,
        names,
    })
}

/// `Regexp#match`/`String#match` -- a real `MatchData`, or `nil` if the
/// pattern doesn't match at all.
pub fn regexp_match(re: &RRegexp, haystack: &str) -> RubyValue {
    match re.compiled.captures(haystack) {
        Some(caps) => {
            let m = build_match_data(re, haystack, &caps);
            crate::lastmatch::set_last_match(Some(m.clone()));
            RubyValue::MatchData(m)
        }
        None => {
            // A failed match CLEARS `$~` and everything derived from it --
            // it does not leave the previous match in place (oracle-verified).
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    }
}

/// `Regexp#match?`/`String#match?` -- a plain bool, no `MatchData`
/// allocated (mirrors real Ruby: `match?` is specifically the
/// no-side-effect, no-allocation probe).
pub fn regexp_is_match(re: &RRegexp, haystack: &str) -> bool {
    re.compiled.is_match(haystack)
}

/// `Regexp#===` (case/when dispatch) -- same underlying check as
/// `match?`, exposed separately so `codegen`'s `case/when`/pattern-matching
/// desugar (`RubyValue::rb_case_eq`) has a name that reads as "the `===`
/// protocol", not just "another way to spell match?".
pub fn regexp_case_eq(re: &RRegexp, haystack: &str) -> bool {
    regexp_is_match(re, haystack)
}

fn char_index(haystack: &str, byte_idx: usize) -> i64 {
    haystack[..byte_idx].chars().count() as i64
}

/// `Regexp#=~`/`String#=~` -- the CHAR index (not byte index, matching
/// every other char-indexed string operation in this runtime -- see
/// `collections::string_get`'s docs) of the match start, or `nil`.
///
/// Runs `captures`, not the cheaper `find`, because `=~` must ALSO record
/// `$~`/`$1`/... -- the whole point of `if s =~ /(\d+)/ then $1 end`, and
/// the groups don't exist without capturing them.
pub fn regexp_match_index(re: &RRegexp, haystack: &str) -> RubyValue {
    match re.compiled.captures(haystack) {
        Some(caps) => {
            let start = caps.get(0).expect("group 0 always exists on a match").start();
            crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps)));
            RubyValue::Int(char_index(haystack, start))
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    }
}

/// `String#rindex(regexp[, pos])` -- the CHAR index of the RIGHTMOST match
/// whose start is at or before `before` (a char index; `None` searches the
/// whole string), or `nil`. Records `$~` like the leftward probes.
pub fn regexp_rindex(re: &RRegexp, haystack: &str, before: Option<usize>) -> RubyValue {
    let mut last = None;
    for caps in re.compiled.captures_iter(haystack) {
        let start_char = char_index(haystack, caps.get(0).expect("group 0 exists").start());
        if before.is_some_and(|lim| start_char as usize > lim) {
            break;
        }
        last = Some(caps);
    }
    match last {
        Some(caps) => {
            let start = caps.get(0).expect("group 0 exists").start();
            crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps)));
            RubyValue::Int(char_index(haystack, start))
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    }
}

pub fn regexp_source(re: &RRegexp) -> RubyValue {
    RubyValue::Str(string_new(re.source.clone()))
}

/// Canonical `m`/`i`/`x` ordering, split into "set" vs. "unset" -- shared by
/// `regexp_to_s`'s CRuby-matching `(?mi-x:body)`-style rendering (verified
/// against real `ruby`: `/abc/x` -> `"(?x-mi:abc)"`, `/a.c/im` ->
/// `"(?mi-x:a.c)"` -- flags that are SET print first, in `m,i,x` order,
/// filtered to just the set ones; unset ones print after the `-`, same
/// filter).
fn flags_split(ignore_case: bool, extended: bool, multiline: bool) -> (String, String) {
    let mut set = String::new();
    let mut unset = String::new();
    for (ch, is_set) in [('m', multiline), ('i', ignore_case), ('x', extended)] {
        if is_set {
            set.push(ch);
        } else {
            unset.push(ch);
        }
    }
    (set, unset)
}

/// `Regexp#to_s` -- what `puts`/string interpolation display for a Regexp
/// value (real Ruby: `Kernel#puts`/`#{}` both call `to_s`, not `inspect`).
pub fn regexp_to_s(re: &RRegexp) -> RubyValue {
    let (set, unset) = flags_split(re.ignore_case, re.extended, re.multiline);
    RubyValue::Str(string_new(format!("(?{set}-{unset}:{})", re.source)))
}

/// `Regexp#inspect` -- the `/pattern/flags` literal form, flags in `m,i,x`
/// order (verified against real `ruby`).
pub fn regexp_inspect(re: &RRegexp) -> RubyValue {
    let mut flags = String::new();
    if re.multiline {
        flags.push('m');
    }
    if re.ignore_case {
        flags.push('i');
    }
    if re.extended {
        flags.push('x');
    }
    RubyValue::Str(string_new(format!("/{}/{flags}", re.source)))
}

/// `Regexp#scan`... no -- `String#scan`: every match, as a plain `String`
/// (the whole match) if the pattern has no capture groups, or as an `Array`
/// of the captured groups (nil for a non-participating optional group) if it
/// does -- matches real Ruby's own shape-switching behavior exactly.
pub fn regexp_scan(re: &RRegexp, haystack: &str) -> RubyValue {
    let has_groups = re.compiled.captures_len() > 1;
    let mut results = Vec::new();
    for caps in re.compiled.captures_iter(haystack) {
        if has_groups {
            let group_vals: Vec<RubyValue> = (1..caps.len())
                .map(|i| match caps.get(i) {
                    Some(m) => RubyValue::Str(string_new(m.as_str().to_string())),
                    None => RubyValue::Nil,
                })
                .collect();
            results.push(RubyValue::Array(array_new(group_vals)));
        } else {
            let whole = caps.get(0).expect("group 0 is always the whole match").as_str();
            results.push(RubyValue::Str(string_new(whole.to_string())));
        }
    }
    RubyValue::Array(array_new(results))
}

/// `String#split(regexp[, limit])` -- a capture group inside the pattern
/// gets its captured text spliced into the output alongside the split
/// segments (verified against real `ruby`); an embedded or LEADING empty
/// string is kept (`",a".split(",") == ["", "a"]`). `limit == 0` (the
/// default) drops trailing empties; `limit > 0` caps the field count with
/// the tail kept whole; `limit < 0` keeps every field including trailing
/// empties.
pub fn regexp_split(re: &RRegexp, haystack: &str, limit: i64) -> RubyValue {
    let mut segments: Vec<String> = Vec::new();
    let mut last_end = 0usize;
    let mut fields = 0i64;
    for caps in re.compiled.captures_iter(haystack) {
        if limit > 0 && fields + 1 >= limit {
            break;
        }
        let m = caps.get(0).expect("group 0 is always the whole match");
        segments.push(haystack[last_end..m.start()].to_string());
        fields += 1;
        for i in 1..caps.len() {
            if let Some(g) = caps.get(i) {
                segments.push(g.as_str().to_string());
            }
        }
        last_end = m.end();
    }
    segments.push(haystack[last_end..].to_string());
    if limit == 0 {
        while segments.last().is_some_and(|s| s.is_empty()) {
            segments.pop();
        }
    }
    let items = segments.into_iter().map(|s| RubyValue::Str(string_new(s))).collect();
    RubyValue::Array(array_new(items))
}

/// Expands a `gsub`/`sub` replacement STRING's backreferences against one
/// match: `\1`.."\9"` (a numbered capture group), `\&` (the whole match),
/// `` \` `` (everything before the match), `\'` (everything after), `\\`
/// (a literal backslash). An unrecognized `\x` escape is passed through
/// literally (`\` + `x`), matching real Ruby's own lenient behavior for an
/// unknown replacement escape. This is the one place backreferences ARE
/// supported despite the pattern-matching engine itself having no
/// backreference support -- a replacement string's `\1` just indexes into
/// the ALREADY-COMPUTED `Captures`, no re-matching involved.
fn expand_replacement(template: &str, caps: &regex::Captures, haystack: &str, match_start: usize, match_end: usize) -> String {
    let mut out = String::new();
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(d) if d.is_ascii_digit() => {
                let idx = d.to_digit(10).expect("guarded by is_ascii_digit") as usize;
                if let Some(g) = caps.get(idx) {
                    out.push_str(g.as_str());
                }
            }
            Some('&') => out.push_str(&haystack[match_start..match_end]),
            Some('`') => out.push_str(&haystack[..match_start]),
            Some('\'') => out.push_str(&haystack[match_end..]),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `String#gsub(regexp, replacement)` -- every match replaced.
pub fn regexp_gsub(re: &RRegexp, haystack: &str, replacement: &str) -> RubyValue {
    let mut out = String::new();
    let mut last_end = 0usize;
    for caps in re.compiled.captures_iter(haystack) {
        let m = caps.get(0).expect("group 0 is always the whole match");
        out.push_str(&haystack[last_end..m.start()]);
        out.push_str(&expand_replacement(replacement, &caps, haystack, m.start(), m.end()));
        last_end = m.end();
    }
    out.push_str(&haystack[last_end..]);
    RubyValue::Str(string_new(out))
}

/// `String#sub(regexp, replacement)` -- only the FIRST match replaced.
pub fn regexp_sub(re: &RRegexp, haystack: &str, replacement: &str) -> RubyValue {
    match re.compiled.captures(haystack) {
        Some(caps) => {
            let m = caps.get(0).expect("group 0 is always the whole match");
            let mut out = String::new();
            out.push_str(&haystack[..m.start()]);
            out.push_str(&expand_replacement(replacement, &caps, haystack, m.start(), m.end()));
            out.push_str(&haystack[m.end()..]);
            RubyValue::Str(string_new(out))
        }
        None => RubyValue::Str(string_new(haystack.to_string())),
    }
}

/// `String#gsub(regexp) { |whole_match| ... }` -- the block form. Each
/// match's whole-matched substring is passed to `blk`; its result is
/// stringified via `to_display_string` (real Ruby: the block's return value
/// is converted with `to_s`) and spliced in place of the match. Propagates
/// whatever `Signal` the block itself raises (`break`/`return`/an uncaught
/// `raise`) via `?`, same as any other real-`Proc` invocation.
pub fn regexp_gsub_block(re: &RRegexp, haystack: &str, blk: &RProc) -> Result<RubyValue, Signal> {
    let mut out = String::new();
    let mut last_end = 0usize;
    for caps in re.compiled.captures_iter(haystack) {
        let m = caps.get(0).expect("group 0 is always the whole match");
        out.push_str(&haystack[last_end..m.start()]);
        let matched = RubyValue::Str(string_new(m.as_str().to_string()));
        let replaced = blk.call(&[matched])?;
        out.push_str(&replaced.to_display_string());
        last_end = m.end();
    }
    out.push_str(&haystack[last_end..]);
    Ok(RubyValue::Str(string_new(out)))
}

/// `String#sub(regexp) { |whole_match| ... }` -- see `regexp_gsub_block`'s
/// docs; only the first match is replaced.
pub fn regexp_sub_block(re: &RRegexp, haystack: &str, blk: &RProc) -> Result<RubyValue, Signal> {
    match re.compiled.captures(haystack) {
        Some(caps) => {
            let m = caps.get(0).expect("group 0 is always the whole match");
            let matched = RubyValue::Str(string_new(m.as_str().to_string()));
            let replaced = blk.call(&[matched])?;
            let mut out = String::new();
            out.push_str(&haystack[..m.start()]);
            out.push_str(&replaced.to_display_string());
            out.push_str(&haystack[m.end()..]);
            Ok(RubyValue::Str(string_new(out)))
        }
        None => Ok(RubyValue::Str(string_new(haystack.to_string()))),
    }
}

/// `MatchData#[]` with an `Int` index -- negative indices count from the
/// end (mirrors `Array#[]`'s convention, real Ruby's own `MatchData#[]`
/// behavior), an out-of-range index (in either direction) is `nil`.
pub fn matchdata_group(m: &RMatchData, index: i64) -> RubyValue {
    let len = m.groups.len() as i64;
    let i = if index < 0 { index + len } else { index };
    if i < 0 || i >= len {
        return RubyValue::Nil;
    }
    match m.groups[i as usize] {
        Some((s, e)) => RubyValue::Str(string_new(m.haystack[s..e].to_string())),
        None => RubyValue::Nil,
    }
}

/// `MatchData#[]` with a `String`/`Symbol` name -- panics on an unknown
/// group name (mirrors real Ruby's `IndexError` for this case; a real,
/// catchable exception is a documented future refinement, same "loud, not
/// silently wrong" posture as this runtime's other `_unchecked` accessors).
pub fn matchdata_group_by_name(m: &RMatchData, name: &str) -> RubyValue {
    match m.names.iter().find(|(n, _)| n == name) {
        Some((_, idx)) => matchdata_group(m, *idx as i64),
        None => panic!("undefined group name reference: {name}"),
    }
}

/// `MatchData#[]`, dispatched at RUNTIME on the key's own `RubyValue` tag
/// (`Int` -> positional, `Symbol`/`Str` -> named) -- used whenever the
/// key's static type isn't known at `codegen` time (a `TyKind::Poly` index
/// expression), so a single call site works uniformly regardless of whether
/// the index was statically provable. Panics on any other key shape (same
/// "loud, not silently wrong" posture as this runtime's `_unchecked`
/// accessors).
pub fn matchdata_get(m: &RMatchData, key: &RubyValue) -> RubyValue {
    match key {
        RubyValue::Int(i) => matchdata_group(m, *i),
        RubyValue::Symbol(s) => matchdata_group_by_name(m, &s.name()),
        RubyValue::Str(s) => matchdata_group_by_name(m, &s.lock().to_utf8_lossy()),
        other => panic!("MatchData#[] expected an Int/Symbol/String key, got {}", other.to_display_string()),
    }
}

pub fn matchdata_pre_match(m: &RMatchData) -> RubyValue {
    let (start, _) = m.groups[0].expect("group 0 (the whole match) always participates");
    RubyValue::Str(string_new(m.haystack[..start].to_string()))
}

pub fn matchdata_post_match(m: &RMatchData) -> RubyValue {
    let (_, end) = m.groups[0].expect("group 0 (the whole match) always participates");
    RubyValue::Str(string_new(m.haystack[end..].to_string()))
}

/// `MatchData#to_a` -- the whole match (`[0]`) followed by every capture.
pub fn matchdata_to_a(m: &RMatchData) -> RubyValue {
    let items = (0..m.groups.len() as i64).map(|i| matchdata_group(m, i)).collect();
    RubyValue::Array(array_new(items))
}

/// `MatchData#captures` -- every capture, EXCLUDING the whole match.
pub fn matchdata_captures(m: &RMatchData) -> RubyValue {
    let items = (1..m.groups.len() as i64).map(|i| matchdata_group(m, i)).collect();
    RubyValue::Array(array_new(items))
}

/// `MatchData#named_captures` -- a `Hash` of `name => captured string`.
pub fn matchdata_named_captures(m: &RMatchData) -> RubyValue {
    let pairs = m
        .names
        .iter()
        .map(|(name, idx)| (RubyValue::Str(string_new(name.clone())), matchdata_group(m, *idx as i64)))
        .collect();
    RubyValue::Hash(hash_new(pairs))
}

pub fn matchdata_string(m: &RMatchData) -> RubyValue {
    RubyValue::Str(string_new(m.haystack.clone()))
}

/// `MatchData#to_s` -- the whole matched substring (distinct from
/// `#string`, which is the entire ORIGINAL haystack).
pub fn matchdata_to_s(m: &RMatchData) -> RubyValue {
    matchdata_group(m, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: &RubyValue) -> Vec<String> {
        let RubyValue::Array(a) = v else { panic!("expected an Array") };
        a.lock().iter().map(RubyValue::to_display_string).collect()
    }

    /// Oracle-verified against real `ruby` (see `crates/spinelc/tests/e2e.rs`'s
    /// `split_*` tests for the e2e-visible half of this behavior) -- tested
    /// directly here too since `puts` on an EMPTY `Array` result can't
    /// currently distinguish "empty array" from "array of one empty string"
    /// (a separate, pre-existing, unrelated `Kernel#puts` gap), so this is
    /// the one place the empty-haystack case is actually verified.
    #[test]
    fn split_matches_real_ruby_leniency() {
        let comma = regexp_new(",", false, false, false).unwrap();
        assert_eq!(strs(&regexp_split(&comma, "a,b,,c", 0)), ["a", "b", "", "c"]);
        assert_eq!(strs(&regexp_split(&comma, ",a,b", 0)), ["", "a", "b"]);
        assert_eq!(strs(&regexp_split(&comma, "a,b,", 0)), ["a", "b"]);
        assert!(strs(&regexp_split(&comma, "", 0)).is_empty());

        let digit = regexp_new(r"\d", false, false, false).unwrap();
        assert_eq!(strs(&regexp_split(&digit, "a1b2c3", 0)), ["a", "b", "c"]);

        let no_match = regexp_new("x", false, false, false).unwrap();
        assert_eq!(strs(&regexp_split(&no_match, "abc", 0)), ["abc"]);
    }

    #[test]
    fn gsub_and_sub_expand_numbered_and_whole_match_backreferences() {
        let word_pair = regexp_new(r"(\w+) (\w+)", false, false, false).unwrap();
        let RubyValue::Str(s) = regexp_gsub(&word_pair, "John Smith", r"\2 \1") else {
            panic!("expected a Str")
        };
        assert_eq!(&*s.lock().to_utf8_lossy(), "Smith John");

        let o = regexp_new("o", false, false, false).unwrap();
        let RubyValue::Str(s) = regexp_gsub(&o, "hello world", "0") else { panic!("expected a Str") };
        assert_eq!(&*s.lock().to_utf8_lossy(), "hell0 w0rld");
        let RubyValue::Str(s) = regexp_sub(&o, "hello world", "0") else { panic!("expected a Str") };
        assert_eq!(&*s.lock().to_utf8_lossy(), "hell0 world");

        let l = regexp_new("l", false, false, false).unwrap();
        let RubyValue::Str(s) = regexp_gsub(&l, "hello", r"[\&]") else { panic!("expected a Str") };
        assert_eq!(&*s.lock().to_utf8_lossy(), "he[l][l]o");
    }

    /// Ruby's `^`/`$` are always line-anchored (`multi_line` unconditional);
    /// `/m` maps to `dot_matches_new_line`, NOT `regex`'s own `multi_line` --
    /// see this module's docs for why those are two different concepts
    /// despite the same-ish name.
    #[test]
    fn flags_translate_to_the_correct_regex_crate_options() {
        let re = regexp_new("^line2", false, false, false).unwrap();
        assert!(regexp_is_match(&re, "line1\nline2"));

        let dot = regexp_new("c.d", false, false, false).unwrap();
        assert!(!regexp_is_match(&dot, "abc\ndef"));
        let dot_m = regexp_new("c.d", false, false, true).unwrap();
        assert!(regexp_is_match(&dot_m, "abc\ndef"));

        let ci = regexp_new("hello", true, false, false).unwrap();
        assert!(regexp_is_match(&ci, "HELLO"));
    }

    #[test]
    fn scan_switches_shape_based_on_capture_groups() {
        let word = regexp_new(r"\w+", false, false, false).unwrap();
        assert_eq!(strs(&regexp_scan(&word, "one two")), ["one", "two"]);

        let pair = regexp_new(r"([a-z])(\d)", false, false, false).unwrap();
        let RubyValue::Array(a) = regexp_scan(&pair, "a1b2") else { panic!("expected an Array") };
        let groups = a.lock();
        assert_eq!(groups.len(), 2);
        assert_eq!(strs(&groups[0]), ["a", "1"]);
        assert_eq!(strs(&groups[1]), ["b", "2"]);
    }

    #[test]
    fn invalid_pattern_is_a_plain_string_error_not_a_panic() {
        assert!(regexp_new("(", false, false, false).is_err());
    }
}
