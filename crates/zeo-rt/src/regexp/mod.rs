//! `Regexp`/`MatchData` -- backed by the `regex` crate, not
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

mod translate;

use crate::builtins::index_error;
use crate::collections::{array_new, hash_new, string_new};
use crate::{RProc, RubyValue, Signal};
use std::sync::Arc;

pub(crate) use translate::named_group_positions;
pub use translate::{RegexpSite, regexp_new, regexp_new_enc};

pub struct RegexpData {
    pub engine: Engine,
    pub source: String,
    pub ignore_case: bool,
    pub extended: bool,
    pub multiline: bool,
    /// The encoding a `/n`/`/e`/`/s`/`/u` literal FORCED, reported by
    /// `#options`, `#encoding` and `#fixed_encoding?`. `Source` for every
    /// runtime-built regexp (`Regexp.new` has no spelling for these) and for a
    /// plain literal, whose encoding follows its own bytes.
    pub encoding: zeo_abi::RegexpEncoding,
    /// `.frozen?` state. A regexp LITERAL is frozen at birth (real Ruby
    /// since 3.0 -- `/a/.frozen?` is true; codegen's literal emission sets
    /// this), `Regexp.new` starts unfrozen. Freezing changes nothing beyond
    /// the flag: no mutating methods exist on Regexp.
    pub frozen: std::sync::atomic::AtomicBool,
}

pub type RRegexp = Arc<RegexpData>;

/// The pattern behind a value -- through a SUBCLASS's payload as well as
/// directly.
///
/// `class VerEx < Regexp` builds a `ValueSubclass` husk whose payload is the
/// real `RubyValue::Regexp`, and a bare `RubyValue::Regexp(re)` match sees only
/// the husk. CRuby needs no such step: a Regexp subclass IS an RRegexp to every
/// C entry point.
///
/// String and Array subclasses reach the same places through the CONVERSION
/// protocol (`to_str`/`to_ary`, which `builtins::convert` unwraps for the
/// caller). Ruby has no `to_regexp`, so this is the only door -- which is why
/// every site that takes a pattern reads it here rather than matching.
pub fn as_regexp(v: &RubyValue) -> Option<RRegexp> {
    match v {
        RubyValue::Regexp(re) => Some(re.clone()),
        _ => match husk_payload(v) {
            Some(RubyValue::Regexp(re)) => Some(re),
            _ => None,
        },
    }
}

/// The `RubyValue::Regexp` a subclass husk stands in for, or `None` when `v` is
/// not one -- [`as_regexp`] for a site that keeps matching on the VALUE.
///
/// Written as a prelude so the site's own arms are untouched:
///
/// ```ignore
/// let husk = regexp::husk_payload(arg);
/// let arg = husk.as_ref().unwrap_or(arg);
/// ```
///
/// Every argument that is not an `Object` costs one discriminant test, so a
/// String pattern pays nothing for this.
pub fn husk_payload(v: &RubyValue) -> Option<RubyValue> {
    match v {
        RubyValue::Object(o) => match o.builtin_payload() {
            payload @ Some(RubyValue::Regexp(_)) => payload,
            _ => None,
        },
        _ => None,
    }
}

impl RegexpData {
    /// `Regexp#frozen?` -- see the `frozen` field.
    pub fn is_frozen(&self) -> bool {
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `Regexp#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// `Regexp#dup`/`#clone`'s payload copy: a fresh allocation (fresh
    /// object identity, `frozen` per the caller's dup-vs-clone rule) over
    /// clones of the compiled engine and flags.
    pub fn dup_data(&self, frozen: bool) -> RRegexp {
        Arc::new(RegexpData {
            engine: self.engine.clone(),
            source: self.source.clone(),
            ignore_case: self.ignore_case,
            extended: self.extended,
            multiline: self.multiline,
            encoding: self.encoding,
            frozen: std::sync::atomic::AtomicBool::new(frozen),
        })
    }
}

/// Link-path proof for the vendored Oniguruma C archive: compiles and runs an
/// onig pattern so generated programs (linked by bare `rustc` against the
/// prebuilt rlib, `zeo::backend`) demonstrably resolve the bundled C archive.
/// Onig now backs the `Engine::Onig` matching path (see `Engine`); this stays
/// as a cheap, dependency-free link smoke test exercised by an e2e.
pub fn onig_linkcheck() -> bool {
    let re = onig::Regex::with_options(
        r"(a+)\1",
        onig::RegexOptions::REGEX_OPTION_NONE,
        onig::Syntax::ruby(),
    );
    match re {
        Ok(re) => re.find("xaaaay").is_some(),
        Err(_) => false,
    }
}

/// The backing engines, in preference order. `Fast` is the linear-time
/// `regex` crate (the overwhelmingly common case); `Fancy` is the backtracking
/// `fancy-regex`, selected when a pattern uses a construct `regex` structurally
/// can't do (in-pattern backreferences, look-around, atomic/possessive groups,
/// `(?#comment)`); `Onig` is real Oniguruma (CRuby's own engine, via the
/// vendored C library) reserved for patterns whose SEMANTICS the Rust engines
/// get wrong or can't express -- Ruby's line anchors `^`/`$` (which, unlike
/// `regex`'s `multi_line`, do not match at the phantom position after a
/// trailing newline), inline flag groups where Ruby's `/m` means DOTALL rather
/// than multi-line (`(?m:a.c)` spanning `\n`), the absence operator `(?~...)`,
/// and anything both Rust engines reject but Onig accepts (e.g. the redundant
/// `a***`). Onig receives the RAW Ruby source (`Syntax::ruby()` parses it
/// directly -- no escape translation). All variants are `Send + Sync` and
/// immutable after construction (`onig::Regex` is `Send + Sync` and read-only
/// during a search that fills a per-call `Region`, so an `Arc` gives it the
/// same cheap-clone value semantics the other engines have natively).
#[derive(Clone)]
pub enum Engine {
    Fast(regex::Regex),
    Fancy(fancy_regex::Regex),
    Onig(Arc<onig::Regex>),
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
    /// Never true for a real match (group 0, the whole match, always exists);
    /// present so `len` has its idiomatic partner.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
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
            Engine::Onig(r) => r
                .search_with_options(
                    haystack,
                    0,
                    haystack.len(),
                    onig::SearchOptions::SEARCH_OPTION_NONE,
                    None,
                )
                .is_some(),
            Engine::Unmatchable => false,
        }
    }

    /// The first match's group spans, or `None` when the pattern doesn't match.
    fn captures_first(&self, haystack: &str) -> Option<Caps> {
        self.captures_at(haystack, 0)
    }

    /// The leftmost match whose start is at or after `start`, with group spans.
    fn captures_at(&self, haystack: &str, start: usize) -> Option<Caps> {
        match self {
            Engine::Fast(r) => r.captures_at(haystack, start).map(|c| Caps {
                spans: (0..c.len())
                    .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                    .collect(),
            }),
            Engine::Fancy(r) => r
                .captures_from_pos(haystack, start)
                .ok()
                .flatten()
                .map(|c| Caps {
                    spans: (0..c.len())
                        .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                        .collect(),
                }),
            // Search the WHOLE `haystack` starting at byte `start` (not a
            // `haystack[start..]` slice): Onig reads the real character before
            // `start` from the full buffer, so `^`/`$`/`\A`/`\Z`/`\G` anchor
            // against the true string, and a fresh `Region` holds the byte
            // spans of every group (`None` for a non-participating group).
            Engine::Onig(r) => {
                let mut region = onig::Region::new();
                r.search_with_options(
                    haystack,
                    start,
                    haystack.len(),
                    onig::SearchOptions::SEARCH_OPTION_NONE,
                    Some(&mut region),
                )?;
                Some(Caps {
                    spans: (0..region.len()).map(|i| region.pos(i)).collect(),
                })
            }
            Engine::Unmatchable => None,
        }
    }

    /// Every non-overlapping match's group spans, left to right, reproducing
    /// CRuby/Onig's zero-width iteration: after an EMPTY match the search
    /// advances one character, but an empty match abutting the PREVIOUS match's
    /// end is still yielded (`"abc".gsub(/b*/, "X") == "XaXXcX"`,
    /// `"aaaa".scan(/a{0,2}/) == ["aa", "aa", ""]`) -- the rust/fancy-regex
    /// `captures_iter` drops that abutting empty, so it can't be used here.
    fn captures_all(&self, haystack: &str) -> Vec<Caps> {
        if matches!(self, Engine::Unmatchable) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut from = 0usize;
        while from <= haystack.len() {
            let Some(caps) = self.captures_at(haystack, from) else {
                break;
            };
            let (s, e) = caps.spans[0].expect("group 0 is always the whole match");
            out.push(caps);
            if e == s {
                // Zero-width match: step one character so the search makes
                // progress, but the next position may still match empty.
                if e >= haystack.len() {
                    break;
                }
                from = e + haystack[e..].chars().next().map_or(1, char::len_utf8);
            } else {
                from = e;
            }
        }
        out
    }

    fn captures_len(&self) -> usize {
        match self {
            Engine::Fast(r) => r.captures_len(),
            Engine::Fancy(r) => r.captures_len(),
            // Onig's `captures_len` counts capturing groups WITHOUT the
            // whole-match slot; `+1` matches the Rust engines' convention
            // (group 0 included).
            Engine::Onig(r) => r.captures_len() + 1,
            // Only the whole-match slot: an unmatchable regex never produces
            // captures, so this is consulted only for shape decisions.
            Engine::Unmatchable => 1,
        }
    }

    pub fn capture_names(&self) -> Vec<(String, usize)> {
        if let Engine::Onig(r) = self {
            // Onig reports each name with the group indices that carry it; a
            // duplicated name resolves to its LAST group (Ruby's rule for a
            // named backreference / `MatchData[name]`).
            let mut out = Vec::new();
            r.foreach_name(|name, groups| {
                if let Some(&last) = groups.iter().max() {
                    out.push((name.to_string(), last as usize));
                }
                true
            });
            out.sort_by_key(|(_, i)| *i);
            return out;
        }
        let names: Vec<Option<String>> = match self {
            Engine::Fast(r) => r.capture_names().map(|n| n.map(str::to_string)).collect(),
            Engine::Fancy(r) => r.capture_names().map(|n| n.map(str::to_string)).collect(),
            _ => Vec::new(),
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
    /// The encoding the haystack CAME FROM. The engine works on decoded
    /// UTF-8 text, so every group sliced out of `haystack` has to be put
    /// back into this to answer with the receiver's own bytes -- see
    /// `matchdata_group`. UTF-8 for a match whose source encoding is not
    /// threaded through yet, which is what every path did before.
    pub enc: crate::encoding::EncodingId,
    pub groups: Vec<Option<(usize, usize)>>,
    pub names: Vec<(String, usize)>,
    /// The `Regexp` that produced this match -- `MatchData#regexp`.
    pub regexp: RRegexp,
    /// `.frozen?` state -- flag-only, like `RegexpData::frozen` (MatchData
    /// has no mutating methods either).
    pub frozen: std::sync::atomic::AtomicBool,
}

pub type RMatchData = Arc<MatchDataInner>;

impl MatchDataInner {
    /// `MatchData#frozen?` -- see the `frozen` field.
    pub fn is_frozen(&self) -> bool {
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `MatchData#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// `MatchData#dup`/`#clone`'s payload copy -- fresh allocation, shared
    /// regexp handle, `frozen` per the caller's dup-vs-clone rule.
    pub fn dup_data(&self, frozen: bool) -> RMatchData {
        Arc::new(MatchDataInner {
            haystack: self.haystack.clone(),
            enc: self.enc,
            groups: self.groups.clone(),
            names: self.names.clone(),
            regexp: self.regexp.clone(),
            frozen: std::sync::atomic::AtomicBool::new(frozen),
        })
    }
}

/// Whether `re` matches at or after `byte_start` -- `match?`'s question,
/// which sets no `$~` and so builds no MatchData.
pub fn regexp_is_match_at(re: &RRegexp, haystack: &str, byte_start: usize) -> bool {
    re.engine.captures_at(haystack, byte_start).is_some()
}

/// [`regexp_match_in`] starting at a BYTE offset, over the whole haystack.
///
/// `String#match(pattern, pos)` cannot slice: the MatchData built from a
/// slice reports offsets relative to it, so `.begin(0)` answered 1 where
/// ruby says 4, and `pre_match` lost everything before `pos`. The engine
/// already takes a start offset -- this is CRuby's `rb_reg_search(str, re,
/// pos, 0)`, which is anchored the same way.
pub fn regexp_match_in_at(
    re: &RRegexp,
    haystack: &str,
    byte_start: usize,
    enc: crate::encoding::EncodingId,
) -> RubyValue {
    match re.engine.captures_at(haystack, byte_start) {
        Some(caps) => {
            let m = build_match_data(re, haystack, &caps, enc);
            crate::lastmatch::set_last_match(Some(m.clone()));
            RubyValue::MatchData(m)
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    }
}

fn build_match_data(
    re: &RRegexp,
    haystack: &str,
    caps: &Caps,
    enc: crate::encoding::EncodingId,
) -> RMatchData {
    Arc::new(MatchDataInner {
        haystack: haystack.to_string(),
        enc,
        groups: caps.spans.clone(),
        names: re.engine.capture_names(),
        regexp: re.clone(),
        frozen: std::sync::atomic::AtomicBool::new(false),
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
        RubyValue::Symbol(s) => name_group_index(md, &s.name())?,
        RubyValue::Str(s) => name_group_index(md, &s.lock().to_utf8_lossy())?,
        other => crate::builtins::convert::to_index(other)?,
    };
    let span = md
        .groups
        .get(usize::try_from(idx).unwrap_or(usize::MAX))
        .ok_or_else(|| index_error!("index {idx} out of matches"))?;
    let (lo, hi) = match span {
        Some((lo, hi)) => (*lo, *hi),
        None => return Ok(offset_pair(RubyValue::Nil, RubyValue::Nil)),
    };
    let (lo, hi) = if byte_mode {
        (lo as i64, hi as i64)
    } else {
        (char_index(&md.haystack, lo), char_index(&md.haystack, hi))
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
        .ok_or_else(|| index_error!("undefined group name reference: {name}"))
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
    regexp_match_in(re, haystack, crate::encoding::UTF_8)
}

/// [`regexp_match`] told what encoding the haystack was decoded FROM, so the
/// groups can be handed back in it. `String#match` knows this; a bare
/// `Regexp#match` against an already-decoded haystack does not.
pub fn regexp_match_in(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> RubyValue {
    match re.engine.captures_first(haystack) {
        Some(caps) => {
            let m = build_match_data(re, haystack, &caps, enc);
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

/// One `StringScanner` hit -- everything the scanner's match surface needs, and
/// nothing more. Deliberately NOT a `MatchData`: a scanner matches once per
/// token, and `MatchData` carries an owned copy of the subject, so building one
/// per `scan` would copy the whole input on every token.
pub struct ScannerMatch {
    /// Group byte spans in the SUBJECT's own coordinates (index 0 is the whole
    /// match) -- rebasing them here is what lets `#pre_match`/`#post_match`
    /// slice the string directly.
    pub groups: Vec<Option<(usize, usize)>>,
    /// `(name, group index)` per named group in the pattern, for `[]` by name
    /// and `#named_captures`.
    pub names: Vec<(String, usize)>,
}

/// Match `pattern` against `subject` at `at`, either `anchored` there or
/// searching forward from it -- the one entry point behind every
/// `StringScanner` method that takes a pattern. `pattern` is a `Regexp` or a
/// `String` matched literally, which CRuby's scanner allows in both positions.
///
/// (Documented divergence, inherited from [`regexp_anchored_len`]: `^`/`\A`
/// and look-behind see the scan position as the string start, since the engine
/// is handed the tail slice.)
pub fn scanner_match(
    pattern: &crate::RubyValue,
    subject: &str,
    at: usize,
    anchored: bool,
) -> Result<Option<ScannerMatch>, crate::Signal> {
    let tail = &subject[at..];
    let husk = husk_payload(pattern);
    let pattern = husk.as_ref().unwrap_or(pattern);
    let (spans, names) = match pattern {
        crate::RubyValue::Regexp(re) => {
            let caps = match re.engine.captures_first(tail) {
                Some(caps) if !anchored || caps.get(0).is_some_and(|(s, _)| s == 0) => caps,
                _ => return Ok(None),
            };
            (caps.spans, re.engine.capture_names())
        }
        crate::RubyValue::Str(s) => {
            let literal = s.lock().to_utf8_lossy().into_owned();
            let start = match anchored {
                true if tail.starts_with(&literal) => 0,
                true => return Ok(None),
                false => match tail.find(&literal) {
                    Some(i) => i,
                    None => return Ok(None),
                },
            };
            (vec![Some((start, start + literal.len()))], Vec::new())
        }
        // `StringScanner` reads a non-Regexp pattern with `StringValue`,
        // so its refusal is the STRING conversion's, not the
        // regexp-expected `Check_Type` one every other pattern slot gives.
        other => {
            return Err(crate::builtins::no_implicit(other, "String"));
        }
    };
    Ok(Some(ScannerMatch {
        groups: spans
            .into_iter()
            .map(|span| span.map(|(s, e)| (s + at, e + at)))
            .collect(),
        names,
    }))
}

/// `Regexp#===` (case/when dispatch) -- same underlying check as
/// `match?`, exposed separately so `codegen`'s `case/when`/pattern-matching
/// desugar (`RubyValue::rb_case_eq`) has a name that reads as "the `===`
/// protocol", not just "another way to spell match?".
/// `Regexp#===` -- what `case`/`when`, bare `===` and `Enumerable#grep` all
/// reach. Unlike `match?` it RECORDS the outcome in `$~`: a hit stores the
/// match data, a miss clears it, so `$1` after a matched `when` arm reads the
/// arm's own captures.
pub fn regexp_case_eq(re: &RRegexp, haystack: &str) -> bool {
    match re.engine.captures_first(haystack) {
        Some(caps) => {
            crate::lastmatch::set_last_match(Some(build_match_data(
                re,
                haystack,
                &caps,
                crate::encoding::UTF_8,
            )));
            true
        }
        None => {
            crate::lastmatch::set_last_match(None);
            false
        }
    }
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
            crate::lastmatch::set_last_match(Some(build_match_data(
                re,
                haystack,
                &caps,
                crate::encoding::UTF_8,
            )));
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
    // CRuby's `rindex(regexp)` is the LARGEST start position (char index, at or
    // before `before`) where the pattern matches ANCHORED -- it tries every
    // start from the end, so /\d+/ on "hello123world" answers 7 ("3"), not the
    // greedy left-most non-overlapping match at 5. `regexp_byterindex` already
    // implements that scan; this just maps the char limit in and the byte offset
    // (plus `$~`) back out.
    let clen = haystack.chars().count();
    let char_limit = before.unwrap_or(clen).min(clen);
    let byte_limit = haystack
        .char_indices()
        .nth(char_limit)
        .map_or(haystack.len(), |(b, _)| b);
    match regexp_byterindex(re, haystack, byte_limit) {
        Some(byte_start) => {
            if let Some(caps) = anchored_caps_at(re, haystack, byte_start) {
                crate::lastmatch::set_last_match(Some(build_match_data(
                    re,
                    haystack,
                    &caps,
                    crate::encoding::UTF_8,
                )));
            }
            RubyValue::Int(char_index(haystack, byte_start))
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    }
}

/// The capture spans of the match ANCHORED at `byte_start`, expressed as
/// full-haystack byte offsets (so `$~`/`MatchData` slice correctly). `None` if
/// nothing matches exactly there.
fn anchored_caps_at(re: &RRegexp, haystack: &str, byte_start: usize) -> Option<Caps> {
    let caps = re.engine.captures_first(&haystack[byte_start..])?;
    if caps.get(0)?.0 != 0 {
        return None; // not anchored at byte_start
    }
    Some(Caps {
        spans: caps
            .spans
            .iter()
            .map(|s| s.map(|(a, b)| (a + byte_start, b + byte_start)))
            .collect(),
    })
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

/// Escapes bare `/` characters in a regexp source for the `/.../ ` and
/// `(?...:...)` display forms (CRuby's `rb_reg_expr_str`): a backslash escape
/// is copied verbatim (so an already-escaped `\/` stays a single escape), and
/// any remaining `/` gets a `\` prepended.
fn escape_forward_slashes(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push('\\');
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '/' => out.push_str("\\/"),
            other => out.push(other),
        }
    }
    out
}

/// `Regexp#to_s` -- what `puts`/string interpolation display for a Regexp
/// value (real Ruby: `Kernel#puts`/`#{}` both call `to_s`, not `inspect`).
pub fn regexp_to_s(re: &RRegexp) -> RubyValue {
    let (set, unset) = flags_split(re.ignore_case, re.extended, re.multiline);
    RubyValue::Str(string_new(format!(
        "(?{set}-{unset}:{})",
        escape_forward_slashes(&re.source)
    )))
}

/// `Regexp#inspect` -- the `/pattern/flags` literal form, flags in `m,i,x`
/// order (verified against real `ruby`).
///
/// Of the four ENCODING letters only `/n` shows: it says the pattern is
/// encoding-agnostic, which re-reading the printed form has no other way to
/// learn. `/e`, `/s` and `/u` print bare (`/x/e.inspect` is `"/x/"`), since the
/// encoding rides on the object rather than on its source (oracle-verified).
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
    if re.encoding == zeo_abi::RegexpEncoding::None {
        flags.push('n');
    }
    RubyValue::Str(string_new(format!(
        "/{}/{flags}",
        escape_forward_slashes(&re.source)
    )))
}

/// `Regexp#scan`... no -- `String#scan`: every match, as a plain `String`
/// (the whole match) if the pattern has no capture groups, or as an `Array`
/// of the captured groups (nil for a non-participating optional group) if it
/// does -- matches real Ruby's own shape-switching behavior exactly.
pub fn regexp_scan(re: &RRegexp, haystack: &str) -> RubyValue {
    let has_groups = re.engine.captures_len() > 1;
    let mut results = Vec::new();
    // `$~` ends up on the LAST match -- CRuby's `scan` writes the backref per
    // iteration, so the final state is the last one (nil when nothing
    // matched, same as any failed match).
    let mut last_md = None;
    for caps in re.engine.captures_all(haystack) {
        last_md = Some(build_match_data(
            re,
            haystack,
            &caps,
            crate::encoding::UTF_8,
        ));
        if has_groups {
            let group_vals: Vec<RubyValue> = (1..caps.len())
                .map(|i| match caps.str(i, haystack) {
                    Some(s) => RubyValue::Str(string_new(s.to_string())),
                    None => RubyValue::Nil,
                })
                .collect();
            results.push(RubyValue::Array(array_new(group_vals)));
        } else {
            let whole = caps
                .str(0, haystack)
                .expect("group 0 is always the whole match");
            results.push(RubyValue::Str(string_new(whole.to_string())));
        }
    }
    crate::lastmatch::set_last_match(last_md);
    RubyValue::Array(array_new(results))
}

/// `String#scan(regexp) { |match| ... }` -- the block form: yields each match
/// (a String when the pattern has no capture groups, else an Array of the
/// groups, exactly like the array `scan` returns) and answers nothing here;
/// the caller returns the receiver (CRuby's `str_scan`). A user `break` in the
/// block propagates untouched.
pub fn regexp_scan_block(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
    blk: &RProc,
) -> Result<(), Signal> {
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
            let whole = caps
                .str(0, haystack)
                .expect("group 0 is always the whole match");
            RubyValue::Str(string_new(whole.to_string()))
        };
        // `$~` tracks the CURRENT match inside the block, as it does in
        // `sub`/`gsub`'s block form.
        crate::lastmatch::set_last_match(Some(build_match_data(
            re,
            haystack,
            &caps,
            crate::encoding::UTF_8,
        )));
        blk.call(&[crate::builtins::string::reencode_strs(&yielded, enc)])?;
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
        let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
        // A zero-width match at the current segment boundary produces no field
        // (CRuby's rb_str_split_m advances instead) -- this is what stops
        // `"abc".split(//)` from leading with an empty "".
        if m_start == m_end && m_start == last_end {
            continue;
        }
        if limit > 0 && fields + 1 >= limit {
            break;
        }
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
    let items = segments
        .into_iter()
        .map(|s| RubyValue::Str(string_new(s)))
        .collect();
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
fn expand_replacement(
    template: &str,
    caps: &Caps,
    names: &[(String, usize)],
    haystack: &str,
    match_start: usize,
    match_end: usize,
) -> Result<String, Signal> {
    let mut out = String::new();
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // A NAMED group turns the NUMBERED spellings off: once a pattern
            // names a group, ruby reads `\1` in a replacement as nothing at
            // all, and only `\k<name>` reaches a capture. `\0` is the whole
            // match, not a group, so it keeps working.
            Some(d) if d.is_ascii_digit() => {
                let idx = d.to_digit(10).expect("guarded by is_ascii_digit") as usize;
                if (idx == 0 || names.is_empty())
                    && let Some(g) = caps.str(idx, haystack)
                {
                    out.push_str(g);
                }
            }
            Some('&') => out.push_str(&haystack[match_start..match_end]),
            Some('`') => out.push_str(&haystack[..match_start]),
            Some('\'') => out.push_str(&haystack[match_end..]),
            Some('\\') => out.push('\\'),
            // `\+` -- the text of the HIGHEST-numbered group that participated
            // (`/(a)(b)?/` on `"a"` -> group 1; on `"ab"` -> group 2). Nothing
            // if only the whole match participated.
            Some('+') => {
                for idx in (1..caps.len()).rev() {
                    if let Some(g) = caps.str(idx, haystack) {
                        out.push_str(g);
                        break;
                    }
                }
            }
            // `\k<name>` -- a named backreference into the match. Only the
            // angle-bracket spelling is a replacement backref (CRuby leaves
            // `\k'name'` literal here); an UNKNOWN group name is an IndexError,
            // while a known-but-unmatched group inserts nothing.
            Some('k') if chars.clone().next() == Some('<') => {
                chars.next(); // consume '<'
                let mut name = String::new();
                for nc in chars.by_ref() {
                    if nc == '>' {
                        break;
                    }
                    name.push(nc);
                }
                match names.iter().find(|(n, _)| *n == name) {
                    Some((_, idx)) => {
                        if let Some(g) = caps.str(*idx, haystack) {
                            out.push_str(g);
                        }
                    }
                    None => {
                        return Err(index_error!("undefined group name reference: {name}"));
                    }
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Ok(out)
}

/// `String#gsub(regexp, replacement)` -- every match replaced.
pub fn regexp_gsub(re: &RRegexp, haystack: &str, replacement: &str) -> Result<RubyValue, Signal> {
    let names = re.engine.capture_names();
    let mut out = String::new();
    let mut last_end = 0usize;
    for caps in re.engine.captures_all(haystack) {
        let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
        out.push_str(&haystack[last_end..m_start]);
        out.push_str(&expand_replacement(
            replacement,
            &caps,
            &names,
            haystack,
            m_start,
            m_end,
        )?);
        last_end = m_end;
    }
    out.push_str(&haystack[last_end..]);
    Ok(RubyValue::Str(string_new(out)))
}

/// `String#sub(regexp, replacement)` -- only the FIRST match replaced.
pub fn regexp_sub(re: &RRegexp, haystack: &str, replacement: &str) -> Result<RubyValue, Signal> {
    let names = re.engine.capture_names();
    match re.engine.captures_first(haystack) {
        Some(caps) => {
            let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
            let mut out = String::new();
            out.push_str(&haystack[..m_start]);
            out.push_str(&expand_replacement(
                replacement,
                &caps,
                &names,
                haystack,
                m_start,
                m_end,
            )?);
            out.push_str(&haystack[m_end..]);
            Ok(RubyValue::Str(string_new(out)))
        }
        None => Ok(RubyValue::Str(string_new(haystack.to_string()))),
    }
}

/// `String#gsub(regexp) { |whole_match| ... }` -- the block form. Each
/// match's whole-matched substring is passed to `blk`; its result is
/// stringified via `to_display_string` (real Ruby: the block's return value
/// is converted with `to_s`) and spliced in place of the match. Propagates
/// whatever `Signal` the block itself raises (`break`/`return`/an uncaught
/// `raise`) via `?`, same as any other real-`Proc` invocation.
///
/// `enc` is the RECEIVER's encoding, and the matched substring is rebuilt in
/// it before the block sees it -- the engine works on decoded UTF-8, so
/// without this a byte the receiver holds raw (`0xE3` in a BINARY string)
/// reaches the block as the two-byte UTF-8 spelling of U+00E3. That is the
/// same rule `regexp_scan_block` keeps, and it is load-bearing for the
/// HASH-replacement form, whose table is keyed by single raw bytes: a miss
/// there substitutes the empty string, so every non-ASCII byte silently
/// VANISHED (`URI.encode_www_form_component` dropped every multibyte
/// character and every Latin-1 byte).
pub fn regexp_gsub_block(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
    blk: &RProc,
) -> Result<RubyValue, Signal> {
    let mut out = String::new();
    let mut last_end = 0usize;
    for caps in re.engine.captures_all(haystack) {
        let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
        out.push_str(&haystack[last_end..m_start]);
        // Each iteration sets `$~`/`$1..` so the block can read the capture
        // groups of the CURRENT match (CRuby updates the frame's backref).
        crate::lastmatch::set_last_match(Some(build_match_data(
            re,
            haystack,
            &caps,
            crate::encoding::UTF_8,
        )));
        let matched = RubyValue::Str(string_new(haystack[m_start..m_end].to_string()));
        let replaced = blk.call(&[crate::builtins::string::reencode_strs(&matched, enc)])?;
        out.push_str(&replaced.to_display_string());
        last_end = m_end;
    }
    out.push_str(&haystack[last_end..]);
    Ok(RubyValue::Str(string_new(out)))
}

/// `String#sub(regexp) { |whole_match| ... }` -- see `regexp_gsub_block`'s
/// docs; only the first match is replaced.
pub fn regexp_sub_block(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
    blk: &RProc,
) -> Result<RubyValue, Signal> {
    match re.engine.captures_first(haystack) {
        Some(caps) => {
            let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
            crate::lastmatch::set_last_match(Some(build_match_data(
                re,
                haystack,
                &caps,
                crate::encoding::UTF_8,
            )));
            let matched = RubyValue::Str(string_new(haystack[m_start..m_end].to_string()));
            let replaced = blk.call(&[crate::builtins::string::reencode_strs(&matched, enc)])?;
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
    let i = if index < 0 {
        // A negative index counts back through the CAPTURE GROUPS only and can
        // never land on group 0, the whole match: `rb_reg_nth_match` answers
        // nil once the wrap reaches `nth <= 0`. So `"hello".match(/(l)(o)/)[-3]`
        // is nil, not `"lo"`.
        let wrapped = index + len;
        if wrapped <= 0 {
            return RubyValue::Nil;
        }
        wrapped
    } else {
        index
    };
    if i >= len {
        return RubyValue::Nil;
    }
    match m.groups[i as usize] {
        Some((s, e)) => crate::builtins::string::str_value_in_enc(m.enc, &m.haystack[s..e]),
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
        None => Err(index_error!("undefined group name reference: {name}")),
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
        RubyValue::Symbol(s) => matchdata_group_by_name(m, &s.name()),
        RubyValue::Str(s) => matchdata_group_by_name(m, &s.lock().to_utf8_lossy()),
        // `md[range]` slices the group array, like `to_a[range]`.
        RubyValue::Range(..) => {
            let all = matchdata_to_a(m);
            Ok(crate::dispatch::send_value(
                &all,
                crate::Symbol::intern("[]"),
                std::slice::from_ref(key),
                None,
            )
            .unwrap_or(RubyValue::Nil))
        }
        other => Ok(matchdata_group(
            m,
            crate::builtins::convert::to_index(other)?,
        )),
    }
}

pub fn matchdata_pre_match(m: &RMatchData) -> RubyValue {
    let (start, _) = m.groups[0].expect("group 0 (the whole match) always participates");
    crate::builtins::string::str_value_in_enc(m.enc, &m.haystack[..start])
}

pub fn matchdata_post_match(m: &RMatchData) -> RubyValue {
    let (_, end) = m.groups[0].expect("group 0 (the whole match) always participates");
    crate::builtins::string::str_value_in_enc(m.enc, &m.haystack[end..])
}

/// `MatchData#to_a` -- the whole match (`[0]`) followed by every capture.
pub fn matchdata_to_a(m: &RMatchData) -> RubyValue {
    let items = (0..m.groups.len() as i64)
        .map(|i| matchdata_group(m, i))
        .collect();
    RubyValue::Array(array_new(items))
}

/// `MatchData#captures` -- every capture, EXCLUDING the whole match.
pub fn matchdata_captures(m: &RMatchData) -> RubyValue {
    let items = (1..m.groups.len() as i64)
        .map(|i| matchdata_group(m, i))
        .collect();
    RubyValue::Array(array_new(items))
}

/// `MatchData#named_captures` -- a `Hash` of `name => captured string`.
pub fn matchdata_named_captures(m: &RMatchData) -> RubyValue {
    let pairs = m
        .names
        .iter()
        .map(|(name, idx)| {
            (
                RubyValue::Str(string_new(name.clone())),
                matchdata_group(m, *idx as i64),
            )
        })
        .collect();
    RubyValue::Hash(hash_new(pairs))
}

pub fn matchdata_string(m: &RMatchData) -> RubyValue {
    let s = crate::builtins::string::str_value_in_enc(m.enc, &m.haystack);
    // FROZEN: `match_string` hands back `RMATCH(match)->str`, which
    // `rb_backref_set` froze when the match was recorded, so the haystack
    // cannot be mutated out from under the offsets the MatchData holds.
    if let RubyValue::Str(buf) = &s {
        buf.set_frozen();
    }
    s
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
            Some((s, e)) => {
                crate::builtins::string::str_value_in_enc(m.enc, &m.haystack[s..e]).inspect_string()
            }
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
        let RubyValue::Array(a) = v else {
            panic!("expected an Array")
        };
        a.lock().iter().map(RubyValue::to_display_string).collect()
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
        let RubyValue::Str(s) = regexp_gsub(&look, "$100 and $5", "N").unwrap() else {
            panic!("expected a Str")
        };
        assert_eq!(s.lock().to_utf8_lossy(), "$N and $N");
    }

    /// Oracle-verified against real `ruby` (see `crates/zeo/tests/e2e.rs`'s
    /// `split_*` tests for the e2e-visible half of this behavior) -- tested
    /// directly here too since `puts` on an EMPTY `Array` result can't
    /// currently distinguish "empty array" from "array of one empty string"
    /// (a separate, pre-existing, unrelated `Kernel#puts` gap), so this is
    /// the one place the empty-haystack case is actually verified.
    #[test]
    fn split_matches_real_ruby_leniency() {
        let comma = regexp_new(",", false, false, false).unwrap();
        assert_eq!(
            strs(&regexp_split(&comma, "a,b,,c", 0)),
            ["a", "b", "", "c"]
        );
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
        let RubyValue::Str(s) = regexp_gsub(&word_pair, "John Smith", r"\2 \1").unwrap() else {
            panic!("expected a Str")
        };
        assert_eq!(&*s.lock().to_utf8_lossy(), "Smith John");

        let o = regexp_new("o", false, false, false).unwrap();
        let RubyValue::Str(s) = regexp_gsub(&o, "hello world", "0").unwrap() else {
            panic!("expected a Str")
        };
        assert_eq!(&*s.lock().to_utf8_lossy(), "hell0 w0rld");
        let RubyValue::Str(s) = regexp_sub(&o, "hello world", "0").unwrap() else {
            panic!("expected a Str")
        };
        assert_eq!(&*s.lock().to_utf8_lossy(), "hell0 world");

        let l = regexp_new("l", false, false, false).unwrap();
        let RubyValue::Str(s) = regexp_gsub(&l, "hello", r"[\&]").unwrap() else {
            panic!("expected a Str")
        };
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
        let RubyValue::Array(a) = regexp_scan(&pair, "a1b2") else {
            panic!("expected an Array")
        };
        let groups = a.lock();
        assert_eq!(groups.len(), 2);
        assert_eq!(strs(&groups[0]), ["a", "1"]);
        assert_eq!(strs(&groups[1]), ["b", "2"]);
    }
}
