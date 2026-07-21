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
    pub engine: Engine,
    pub source: String,
    pub ignore_case: bool,
    pub extended: bool,
    pub multiline: bool,
}

pub type RRegexp = Arc<RegexpData>;

/// The two backing engines. `Fast` is the linear-time `regex` crate (the
/// overwhelmingly common case); `Fancy` is the backtracking `fancy-regex`,
/// selected only when a pattern uses a construct `regex` structurally can't do
/// (in-pattern backreferences, look-around, atomic/possessive groups,
/// `(?#comment)`). Both are `Send + Sync` and immutable after construction.
pub enum Engine {
    Fast(regex::Regex),
    Fancy(fancy_regex::Regex),
    /// A pattern Onigmo (CRuby) accepts but neither Rust engine can compile --
    /// specifically a FORWARD numbered backreference (`/[\]]\1(a)/`, where `\1`
    /// precedes the group it names). It constructs so introspection
    /// (`#source`/`#encoding`/`Regexp.linear_time?`) works, but never matches:
    /// a narrow, documented divergence from Onigmo's match semantics for these
    /// rare patterns. Only reached when the referenced group actually exists
    /// (`regexp_new`); a genuine invalid backref number still raises RegexpError.
    Unmatchable,
}

/// One match normalized to byte-offset group spans (index 0 = whole match;
/// `None` = a non-participating optional group). This is the single shape both
/// engines' `Captures` collapse to, so every downstream consumer
/// (`build_match_data`, `scan`, `split`, `gsub`/`sub`) is engine-agnostic.
pub struct Caps {
    spans: Vec<Option<(usize, usize)>>,
}

impl Caps {
    /// The byte span of group `i`, or `None` if absent/non-participating.
    pub fn get(&self, i: usize) -> Option<(usize, usize)> {
        self.spans.get(i).copied().flatten()
    }
    pub fn len(&self) -> usize {
        self.spans.len()
    }
    /// The matched substring of group `i` (empty for an absent group).
    fn str<'h>(&self, i: usize, haystack: &'h str) -> Option<&'h str> {
        self.get(i).map(|(s, e)| &haystack[s..e])
    }
}

impl Engine {
    pub fn is_match(&self, haystack: &str) -> bool {
        match self {
            Engine::Fast(r) => r.is_match(haystack),
            // A runtime error (e.g. backtrack-limit) counts as "no match" --
            // rare, documented; CRuby would raise on catastrophic backtracking.
            Engine::Fancy(r) => r.is_match(haystack).unwrap_or(false),
            Engine::Unmatchable => false,
        }
    }

    /// The first match's group spans, or `None` when the pattern doesn't match.
    fn captures_first(&self, haystack: &str) -> Option<Caps> {
        match self {
            Engine::Fast(r) => r.captures(haystack).map(|c| Caps {
                spans: (0..c.len()).map(|i| c.get(i).map(|m| (m.start(), m.end()))).collect(),
            }),
            Engine::Fancy(r) => r.captures(haystack).ok().flatten().map(|c| Caps {
                spans: (0..c.len()).map(|i| c.get(i).map(|m| (m.start(), m.end()))).collect(),
            }),
            Engine::Unmatchable => None,
        }
    }

    /// Every non-overlapping match's group spans, left to right.
    fn captures_all(&self, haystack: &str) -> Vec<Caps> {
        match self {
            Engine::Fast(r) => r
                .captures_iter(haystack)
                .map(|c| Caps {
                    spans: (0..c.len()).map(|i| c.get(i).map(|m| (m.start(), m.end()))).collect(),
                })
                .collect(),
            Engine::Fancy(r) => r
                .captures_iter(haystack)
                .filter_map(|c| c.ok())
                .map(|c| Caps {
                    spans: (0..c.len()).map(|i| c.get(i).map(|m| (m.start(), m.end()))).collect(),
                })
                .collect(),
            Engine::Unmatchable => Vec::new(),
        }
    }

    fn captures_len(&self) -> usize {
        match self {
            Engine::Fast(r) => r.captures_len(),
            Engine::Fancy(r) => r.captures_len(),
            // Only the whole-match slot: an unmatchable regex never produces
            // captures, so this is consulted only for shape decisions.
            Engine::Unmatchable => 1,
        }
    }

    pub fn capture_names(&self) -> Vec<(String, usize)> {
        let names: Vec<Option<String>> = match self {
            Engine::Fast(r) => r.capture_names().map(|n| n.map(str::to_string)).collect(),
            Engine::Fancy(r) => r.capture_names().map(|n| n.map(str::to_string)).collect(),
            Engine::Unmatchable => Vec::new(),
        };
        names
            .into_iter()
            .enumerate()
            .filter_map(|(i, n)| n.map(|n| (n, i)))
            .collect()
    }
}

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
    /// The `Regexp` that produced this match -- `MatchData#regexp`.
    pub regexp: RRegexp,
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
/// Rewrites the Ruby-flavoured escapes the Rust `regex` crate doesn't
/// recognise into equivalents it does, leaving everything else byte-for-byte
/// untouched. Today that is just `\e` (Ruby's ESC, U+001B) -> `\x1b`; the
/// crate already accepts `\a \f \n \r \t \v` and escaped metacharacters. A
/// backslash always consumes the character after it, so `\\e` (an escaped
/// backslash followed by a literal `e`) is left alone, as is an `e` inside a
/// character class that isn't preceded by a backslash.
fn translate_ruby_escapes(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    // A character class changes escape meaning: `\1`..`\7` become OCTAL (not a
    // backreference), and `\k`/`\g` are literal letters (Onigmo) -- both of
    // which the linear `regex` crate rejects verbatim, so they are rewritten.
    // `]` closes the class unless it is the first member (`[]` / `[^]`), and
    // `\]` inside the class is an escaped literal.
    let mut in_class = false;
    let mut class_start = false;
    while let Some(c) = chars.next() {
        if !in_class {
            match c {
                '[' => {
                    in_class = true;
                    class_start = true;
                    out.push('[');
                }
                '\\' => translate_escape_outside(&mut out, &mut chars),
                _ => out.push(c),
            }
            continue;
        }
        match c {
            ']' if !class_start => {
                in_class = false;
                out.push(']');
            }
            // A leading `^` keeps the "next `]` is literal" rule alive (`[^]`).
            '^' if class_start => out.push('^'),
            '\\' => {
                class_start = false;
                translate_escape_in_class(&mut out, &mut chars);
            }
            _ => {
                class_start = false;
                out.push(c);
            }
        }
    }
    out
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// Escape translation OUTSIDE a character class: `\e` -> ESC, `\0...` octal
/// (a leading zero is never a backreference), everything else (`\1`..`\9`
/// backrefs, `\k`, `\d`, ...) passes through for the engine to interpret.
fn translate_escape_outside(out: &mut String, chars: &mut Chars) {
    match chars.next() {
        Some('e') => out.push_str("\\x1b"),
        Some('0') => push_octal(out, 0, chars),
        Some(next) => {
            out.push('\\');
            out.push(next);
        }
        None => out.push('\\'),
    }
}

/// Escape translation INSIDE a character class: `\0`..`\7` are octal, `\k`/`\g`
/// are literal letters (the `regex` crate rejects `\k` in a class), and valid
/// class escapes (`\d` `\w` `\]` `\-` ...) pass through.
fn translate_escape_in_class(out: &mut String, chars: &mut Chars) {
    match chars.next() {
        Some('e') => out.push_str("\\x1b"),
        Some(d @ '0'..='7') => push_octal(out, d.to_digit(8).unwrap(), chars),
        Some(c @ ('k' | 'g')) => out.push(c),
        Some(next) => {
            out.push('\\');
            out.push(next);
        }
        None => out.push('\\'),
    }
}

/// Reads up to two more octal digits after `first` (3 total, Ruby's max) and
/// emits the code point as `\xHH` (or `\x{...}` past 0xff), which both engines
/// understand -- and which keeps `needs_fancy` from mistaking a `\0`-style
/// octal for a backreference.
fn push_octal(out: &mut String, first: u32, chars: &mut Chars) {
    let mut val = first;
    for _ in 0..2 {
        match chars.peek() {
            Some(d @ '0'..='7') => {
                val = val * 8 + d.to_digit(8).unwrap();
                chars.next();
            }
            _ => break,
        }
    }
    if val > 0xff {
        out.push_str(&format!("\\x{{{val:x}}}"));
    } else {
        out.push_str(&format!("\\x{val:02x}"));
    }
}

/// Whether `pattern` uses a construct the linear-time `regex` crate cannot
/// compile, so the backtracking `fancy-regex` engine must back it: an
/// in-pattern backreference (`\1`..`\9`, `\k<name>`), any look-around
/// (`(?=` `(?!` `(?<=` `(?<!`), an atomic group `(?>`, an inline comment
/// `(?#`, or a possessive quantifier (`*+` `++` `?+` `}+`). Scanned on the
/// ALREADY-escape-translated pattern; a `\\` consumes its next char so an
/// escaped backslash before a digit isn't mistaken for a backreference.
fn needs_fancy(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if let Some(&n) = bytes.get(i + 1) {
                    // `\1`..`\9` (backref) or `\k` (named backref).
                    if n.is_ascii_digit() || n == b'k' {
                        return true;
                    }
                }
                i += 2; // skip the escaped char
                continue;
            }
            b'(' if bytes.get(i + 1) == Some(&b'?') => {
                match bytes.get(i + 2) {
                    Some(b'=') | Some(b'!') | Some(b'>') | Some(b'#') => return true,
                    Some(b'<') if matches!(bytes.get(i + 3), Some(b'=') | Some(b'!')) => {
                        return true
                    }
                    _ => {}
                }
            }
            // Possessive quantifiers: a quantifier immediately followed by `+`.
            b'+' if i > 0 && matches!(bytes[i - 1], b'*' | b'+' | b'?' | b'}') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Builds the `fancy-regex` engine, applying Ruby's flag semantics via inline
/// flag groups (fancy-regex's builder exposes only case-insensitivity): `(?m)`
/// is unconditional (Ruby's `^`/`$` are always line-anchored), `s` maps Ruby's
/// `/m` (dot matches newline), `x` maps `/x`.
fn build_fancy(translated: &str, ignore_case: bool, extended: bool, multiline: bool) -> Result<fancy_regex::Regex, String> {
    let mut flags = String::from("m");
    if multiline {
        flags.push('s');
    }
    if ignore_case {
        flags.push('i');
    }
    if extended {
        flags.push('x');
    }
    fancy_regex::Regex::new(&format!("(?{flags}){translated}")).map_err(|e| e.to_string())
}

pub fn regexp_new(source: &str, ignore_case: bool, extended: bool, multiline: bool) -> Result<RRegexp, String> {
    let translated = translate_ruby_escapes(source);
    let engine = if needs_fancy(&translated) {
        match build_fancy(&translated, ignore_case, extended, multiline) {
            Ok(r) => Engine::Fancy(r),
            // A forward numbered backreference (`/[\]]\1(a)/`) is valid in
            // Onigmo but rejected by fancy-regex. If every backref names a
            // group that actually exists, keep the regexp constructible (as
            // Unmatchable) instead of raising -- a genuine invalid backref
            // number still surfaces the error.
            Err(e) if forward_backref_only(&translated) => {
                let _ = e;
                Engine::Unmatchable
            }
            Err(e) => return Err(e),
        }
    } else {
        match regex::RegexBuilder::new(&translated)
            .case_insensitive(ignore_case)
            .ignore_whitespace(extended)
            .dot_matches_new_line(multiline)
            .multi_line(true)
            .build()
        {
            Ok(r) => Engine::Fast(r),
            // The pre-scan missed something the fast engine still rejects
            // (e.g. a construct only its parser flags): fall back to fancy.
            Err(fast_err) => Engine::Fancy(
                build_fancy(&translated, ignore_case, extended, multiline)
                    .map_err(|_| fast_err.to_string())?,
            ),
        }
    };
    Ok(Arc::new(RegexpData {
        engine,
        source: source.to_string(),
        ignore_case,
        extended,
        multiline,
    }))
}

/// Whether `pattern` (already escape-translated) fails to compile ONLY because
/// of a forward numbered backreference: there is at least one `\N` backref, and
/// every one names a capture group that exists somewhere in the pattern (`N <=`
/// the capture-group count). A backref past the group count is a genuine
/// invalid-backref error and returns false, so `regexp_new` still raises. Named
/// backrefs (`\k<...>`) are treated conservatively as NOT forward-only (they
/// compile in fancy-regex when the name is defined, so a failure is a real
/// error). Character-class contents are octal after translation and are skipped.
fn forward_backref_only(pattern: &str) -> bool {
    let groups = count_capture_groups(pattern);
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    let mut saw_backref = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_class {
            match c {
                b']' if !class_start => in_class = false,
                b'^' if class_start => {}
                b'\\' if i + 1 < bytes.len() => {
                    class_start = false;
                    i += 2;
                    continue;
                }
                _ => class_start = false,
            }
            i += 1;
            continue;
        }
        match c {
            b'[' => {
                in_class = true;
                class_start = true;
                i += 1;
            }
            b'\\' if i + 1 < bytes.len() => {
                let n = bytes[i + 1];
                if n == b'k' {
                    return false; // a named backref failing is a real error
                }
                if n.is_ascii_digit() && n != b'0' {
                    let mut j = i + 1;
                    let mut num = 0usize;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        num = num * 10 + (bytes[j] - b'0') as usize;
                        j += 1;
                    }
                    if num > groups {
                        return false; // references a group that doesn't exist
                    }
                    saw_backref = true;
                    i = j;
                    continue;
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    saw_backref
}

/// Counts capture groups in `pattern`: an unescaped `(` that is not a
/// non-capturing/assertion group (`(?:`/`(?=`/`(?!`/`(?<=`/`(?<!`/`(?>`/`(?#`/
/// `(?flags)`). A named group (`(?<name>`/`(?'name'`/`(?P<name>`) DOES count.
/// Skips character classes and escaped parens.
fn count_capture_groups(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut count = 0;
    let mut in_class = false;
    let mut class_start = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_class {
            match c {
                b']' if !class_start => in_class = false,
                b'^' if class_start => {}
                b'\\' if i + 1 < bytes.len() => {
                    class_start = false;
                    i += 2;
                    continue;
                }
                _ => class_start = false,
            }
            i += 1;
            continue;
        }
        match c {
            b'[' => {
                in_class = true;
                class_start = true;
            }
            b'\\' if i + 1 < bytes.len() => {
                i += 2;
                continue;
            }
            b'(' => {
                let capturing = if bytes.get(i + 1) == Some(&b'?') {
                    // `(?<name>` / `(?'name'` / `(?P<name>` capture; the rest
                    // (`(?:`, `(?=`, `(?<=`, flags, ...) do not.
                    matches!(bytes.get(i + 2), Some(b'\''))
                        || (bytes.get(i + 2) == Some(&b'<')
                            && !matches!(bytes.get(i + 3), Some(b'=') | Some(b'!')))
                        || (bytes.get(i + 2) == Some(&b'P') && bytes.get(i + 3) == Some(&b'<'))
                } else {
                    true
                };
                if capturing {
                    count += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    count
}

fn build_match_data(re: &RRegexp, haystack: &str, caps: &Caps) -> RMatchData {
    Arc::new(MatchDataInner {
        haystack: haystack.to_string(),
        groups: caps.spans.clone(),
        names: re.engine.capture_names(),
        regexp: re.clone(),
    })
}

/// `MatchData#offset(n)` / `#byteoffset(n)`: the `[start, end]` of group `n`
/// (an index, or a named-group Symbol/String) as char indices (`byte_mode`
/// false) or byte indices (true). `[nil, nil]` for a group that didn't
/// participate; `IndexError` for an out-of-range index or unknown name.
pub fn matchdata_offset(
    md: &RMatchData,
    key: &RubyValue,
    byte_mode: bool,
) -> Result<RubyValue, crate::Signal> {
    let idx = match key {
        RubyValue::Int(n) => *n,
        RubyValue::Symbol(s) => name_group_index(md, &s.name())?,
        RubyValue::Str(s) => name_group_index(md, &s.lock().to_utf8_lossy())?,
        other => {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
    };
    let span = md.groups.get(usize::try_from(idx).unwrap_or(usize::MAX)).ok_or_else(|| {
        crate::dispatch::raise_error("IndexError", format!("index {idx} out of matches"))
    })?;
    let (lo, hi) = match span {
        Some((lo, hi)) => (*lo, *hi),
        None => return Ok(offset_pair(RubyValue::Nil, RubyValue::Nil)),
    };
    let (lo, hi) = if byte_mode {
        (lo as i64, hi as i64)
    } else {
        (char_index(&md.haystack, lo) as i64, char_index(&md.haystack, hi) as i64)
    };
    Ok(offset_pair(RubyValue::Int(lo), RubyValue::Int(hi)))
}

fn offset_pair(a: RubyValue, b: RubyValue) -> RubyValue {
    RubyValue::Array(crate::array_new(vec![a, b]))
}

fn name_group_index(md: &RMatchData, name: &str) -> Result<i64, crate::Signal> {
    md.names
        .iter()
        .find(|(n, _)| n.as_str() == name)
        .map(|(_, i)| *i as i64)
        .ok_or_else(|| {
            crate::dispatch::raise_error(
                "IndexError",
                format!("undefined group name reference: {name}"),
            )
        })
}

/// `MatchData#names` -- the named capture groups, in group order.
pub fn matchdata_names(md: &RMatchData) -> RubyValue {
    let out = md
        .names
        .iter()
        .map(|(n, _)| RubyValue::Str(crate::string_new(n.clone())))
        .collect();
    RubyValue::Array(crate::array_new(out))
}

/// `MatchData#regexp` -- the `Regexp` that produced the match.
pub fn matchdata_regexp(md: &RMatchData) -> RubyValue {
    RubyValue::Regexp(md.regexp.clone())
}

/// `Regexp#match`/`String#match` -- a real `MatchData`, or `nil` if the
/// pattern doesn't match at all.
pub fn regexp_match(re: &RRegexp, haystack: &str) -> RubyValue {
    match re.engine.captures_first(haystack) {
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
    re.engine.is_match(haystack)
}

/// `StringScanner`'s anchored probe: the byte length of `re`'s leftmost match
/// when it begins exactly at the start of `haystack`, else `None`.
/// `StringScanner#scan`/`#skip` match anchored at the scanner's current
/// position, so the caller passes the not-yet-scanned tail and treats a
/// `Some(len)` as "consume `len` bytes". Reuses the same `Caps` normalization
/// every other engine consumer goes through. (Documented divergence: `^`/`\A`
/// and look-behind see `haystack`'s start as the string start, not the
/// original position -- acceptable for the scanner's tail-slice model.)
pub fn regexp_anchored_len(re: &RRegexp, haystack: &str) -> Option<usize> {
    let caps = re.engine.captures_first(haystack)?;
    let (start, end) = caps.get(0)?;
    (start == 0).then_some(end)
}

/// The byte span `(start, end)` of `re`'s leftmost match in `haystack`, or
/// `None`. `StringScanner#scan_until`/`#exist?` need where the *next* match
/// lands (not anchored), which is exactly this.
pub fn regexp_find(re: &RRegexp, haystack: &str) -> Option<(usize, usize)> {
    re.engine.captures_first(haystack).and_then(|c| c.get(0))
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
    match re.engine.captures_first(haystack) {
        Some(caps) => {
            let start = caps.get(0).expect("group 0 always exists on a match").0;
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
    for caps in re.engine.captures_all(haystack) {
        let start_char = char_index(haystack, caps.get(0).expect("group 0 exists").0);
        if before.is_some_and(|lim| start_char as usize > lim) {
            break;
        }
        last = Some(caps);
    }
    match last {
        Some(caps) => {
            let start = caps.get(0).expect("group 0 exists").0;
            crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps)));
            RubyValue::Int(char_index(haystack, start))
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    }
}

/// `String#byterindex(regexp[, pos])` -- the BYTE offset of the LAST (highest)
/// start position at or before `before` where `re` matches anchored, or
/// `None`. CRuby's `rindex` tries every start from the end, so `/l+/` against
/// `"hello"` finds the single `"l"` at 3, not the greedy `"ll"` leftmost at 2.
pub fn regexp_byterindex(re: &RRegexp, haystack: &str, before: usize) -> Option<usize> {
    let mut p = before.min(haystack.len());
    loop {
        if haystack.is_char_boundary(p) && regexp_anchored_len(re, &haystack[p..]).is_some() {
            return Some(p);
        }
        if p == 0 {
            return None;
        }
        p -= 1;
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
    let has_groups = re.engine.captures_len() > 1;
    let mut results = Vec::new();
    for caps in re.engine.captures_all(haystack) {
        if has_groups {
            let group_vals: Vec<RubyValue> = (1..caps.len())
                .map(|i| match caps.str(i, haystack) {
                    Some(s) => RubyValue::Str(string_new(s.to_string())),
                    None => RubyValue::Nil,
                })
                .collect();
            results.push(RubyValue::Array(array_new(group_vals)));
        } else {
            let whole = caps.str(0, haystack).expect("group 0 is always the whole match");
            results.push(RubyValue::Str(string_new(whole.to_string())));
        }
    }
    RubyValue::Array(array_new(results))
}

/// `String#scan(regexp) { |match| ... }` -- the block form: yields each match
/// (a String when the pattern has no capture groups, else an Array of the
/// groups, exactly like the array `scan` returns) and answers nothing here;
/// the caller returns the receiver (CRuby's `str_scan`). A user `break` in the
/// block propagates untouched.
pub fn regexp_scan_block(re: &RRegexp, haystack: &str, blk: &RProc) -> Result<(), Signal> {
    let has_groups = re.engine.captures_len() > 1;
    for caps in re.engine.captures_all(haystack) {
        let yielded = if has_groups {
            let group_vals: Vec<RubyValue> = (1..caps.len())
                .map(|i| match caps.str(i, haystack) {
                    Some(s) => RubyValue::Str(string_new(s.to_string())),
                    None => RubyValue::Nil,
                })
                .collect();
            RubyValue::Array(array_new(group_vals))
        } else {
            let whole = caps.str(0, haystack).expect("group 0 is always the whole match");
            RubyValue::Str(string_new(whole.to_string()))
        };
        blk.call(&[yielded])?;
    }
    Ok(())
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
    for caps in re.engine.captures_all(haystack) {
        if limit > 0 && fields + 1 >= limit {
            break;
        }
        let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
        segments.push(haystack[last_end..m_start].to_string());
        fields += 1;
        for i in 1..caps.len() {
            if let Some(g) = caps.str(i, haystack) {
                segments.push(g.to_string());
            }
        }
        last_end = m_end;
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
fn expand_replacement(template: &str, caps: &Caps, haystack: &str, match_start: usize, match_end: usize) -> String {
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
                if let Some(g) = caps.str(idx, haystack) {
                    out.push_str(g);
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
    for caps in re.engine.captures_all(haystack) {
        let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
        out.push_str(&haystack[last_end..m_start]);
        out.push_str(&expand_replacement(replacement, &caps, haystack, m_start, m_end));
        last_end = m_end;
    }
    out.push_str(&haystack[last_end..]);
    RubyValue::Str(string_new(out))
}

/// `String#sub(regexp, replacement)` -- only the FIRST match replaced.
pub fn regexp_sub(re: &RRegexp, haystack: &str, replacement: &str) -> RubyValue {
    match re.engine.captures_first(haystack) {
        Some(caps) => {
            let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
            let mut out = String::new();
            out.push_str(&haystack[..m_start]);
            out.push_str(&expand_replacement(replacement, &caps, haystack, m_start, m_end));
            out.push_str(&haystack[m_end..]);
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
    for caps in re.engine.captures_all(haystack) {
        let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
        out.push_str(&haystack[last_end..m_start]);
        let matched = RubyValue::Str(string_new(haystack[m_start..m_end].to_string()));
        let replaced = blk.call(&[matched])?;
        out.push_str(&replaced.to_display_string());
        last_end = m_end;
    }
    out.push_str(&haystack[last_end..]);
    Ok(RubyValue::Str(string_new(out)))
}

/// `String#sub(regexp) { |whole_match| ... }` -- see `regexp_gsub_block`'s
/// docs; only the first match is replaced.
pub fn regexp_sub_block(re: &RRegexp, haystack: &str, blk: &RProc) -> Result<RubyValue, Signal> {
    match re.engine.captures_first(haystack) {
        Some(caps) => {
            let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
            let matched = RubyValue::Str(string_new(haystack[m_start..m_end].to_string()));
            let replaced = blk.call(&[matched])?;
            let mut out = String::new();
            out.push_str(&haystack[..m_start]);
            out.push_str(&replaced.to_display_string());
            out.push_str(&haystack[m_end..]);
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
pub fn matchdata_group_by_name(m: &RMatchData, name: &str) -> Result<RubyValue, Signal> {
    match m.names.iter().find(|(n, _)| n == name) {
        Some((_, idx)) => Ok(matchdata_group(m, *idx as i64)),
        None => Err(crate::dispatch::raise_error(
            "IndexError",
            format!("undefined group name reference: {name}"),
        )),
    }
}

/// `MatchData#[]`, dispatched at RUNTIME on the key's own `RubyValue` tag
/// (`Int` -> positional, `Symbol`/`Str` -> named) -- used whenever the
/// key's static type isn't known at `codegen` time (a `TyKind::Poly` index
/// expression), so a single call site works uniformly regardless of whether
/// the index was statically provable. Panics on any other key shape (same
/// "loud, not silently wrong" posture as this runtime's `_unchecked`
/// accessors).
pub fn matchdata_get(m: &RMatchData, key: &RubyValue) -> Result<RubyValue, Signal> {
    match key {
        RubyValue::Int(i) => Ok(matchdata_group(m, *i)),
        RubyValue::Symbol(s) => matchdata_group_by_name(m, &s.name()),
        RubyValue::Str(s) => matchdata_group_by_name(m, &s.lock().to_utf8_lossy()),
        // `md[range]` slices the group array, like `to_a[range]`.
        RubyValue::Range(..) => {
            let all = matchdata_to_a(m);
            Ok(crate::dispatch::send_value(&all, crate::Symbol::intern("[]"), std::slice::from_ref(key), None)
                .unwrap_or(RubyValue::Nil))
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!("no implicit conversion of {} into Integer", crate::builtins::class_name_of(other)),
        )),
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

/// `MatchData#inspect` -- `#<MatchData "whole" 1:"cap" name:"cap">`. Each
/// capture past the whole match is labelled by its group name when it has one,
/// else by its 1-based index; a non-participating group renders as `nil`.
pub fn matchdata_inspect(m: &RMatchData) -> String {
    let whole = RubyValue::Str(string_new(match m.groups.first() {
        Some(Some((s, e))) => m.haystack[*s..*e].to_string(),
        _ => String::new(),
    }))
    .inspect_string();
    let mut out = format!("#<MatchData {whole}");
    for i in 1..m.groups.len() {
        let label = m
            .names
            .iter()
            .find(|(_, idx)| *idx == i)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| i.to_string());
        let value = match m.groups[i] {
            Some((s, e)) => RubyValue::Str(string_new(m.haystack[s..e].to_string())).inspect_string(),
            None => "nil".to_string(),
        };
        out.push_str(&format!(" {label}:{value}"));
    }
    out.push('>');
    out
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
    fn needs_fancy_detects_only_unsupported_constructs() {
        // Fancy-only constructs.
        for p in [
            r"(\w)\1",       // backreference
            r"foo(?=bar)",   // lookahead
            r"foo(?!bar)",   // negative lookahead
            r"(?<=\$)\d+",   // lookbehind
            r"(?<!x)y",      // negative lookbehind
            r"(?>ab)",       // atomic group
            r"a(?#note)b",   // inline comment
            r"a++",          // possessive
            r"(?<n>\w)\k<n>", // named backref
        ] {
            assert!(needs_fancy(p), "{p} should need fancy");
        }
        // Plain patterns stay on the fast engine.
        for p in [
            r"\d+",
            r"(?<year>\d{4})", // named GROUP is fine on regex
            r"[a-z]\\1",        // an escaped backslash then literal 1, not a backref
            r"a|b",
            r"(?i)abc",         // inline flag, supported by regex
        ] {
            assert!(!needs_fancy(p), "{p} should NOT need fancy");
        }
    }

    #[test]
    fn backreferences_and_lookaround_match_via_fancy_engine() {
        let dbl = regexp_new(r"(\w)\1", false, false, false).unwrap();
        assert!(matches!(dbl.engine, Engine::Fancy(_)));
        assert!(regexp_is_match(&dbl, "hello"));
        assert!(!regexp_is_match(&dbl, "abc"));

        let plain = regexp_new(r"\d+", false, false, false).unwrap();
        assert!(matches!(plain.engine, Engine::Fast(_)));

        let look = regexp_new(r"(?<=\$)\d+", false, false, false).unwrap();
        let RubyValue::Str(s) = regexp_gsub(&look, "$100 and $5", "N") else {
            panic!("expected a Str")
        };
        assert_eq!(s.lock().to_utf8_lossy(), "$N and $N");
    }

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
