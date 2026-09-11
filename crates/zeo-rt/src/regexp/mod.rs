//! `Regexp`/`MatchData` over Oniguruma -- the engine CRuby's own Onigmo
//! forked from, speaking Ruby's regex dialect through `Syntax::ruby()`.
//!
//! ONE engine. Every pattern compiles and matches here, so what it refuses
//! ruby refuses and what it accepts ruby accepts, and there is no second
//! dialect to translate into (a translation is a second implementation of
//! the syntax, with its own drift). What stays on this side of the engine is
//! ruby's own `re.c` preprocessing -- `\u` escapes and the `\M-`/`\C-`/`\c`
//! byte escapes -- and the CRuby-shaped compile-error text; see
//! `translate.rs`. The handful of rows where Oniguruma and Onigmo answer
//! differently are ledgered in `docs/reference/compatibility.md`.
//!
//! `RegexpData`/`MatchDataInner` need no `Mutex` at all (unlike
//! `RArray`/`RHash`/`RStr`): both are immutable after construction, and
//! `onig::Regex` is `Send + Sync` and read-only during a search (it fills a
//! per-call `Region`) -- an `Arc` alone gives the same cheap-clone
//! shared-identity value semantics every other `RubyValue` payload uses.

mod charrange;
mod lint;
mod translate;

use crate::builtins::index_error;
use crate::collections::{array_new, hash_new, string_new};
use crate::{RProc, RubyValue, Signal};
use std::sync::Arc;

pub(crate) use translate::named_group_positions;
pub use translate::{RegexpSite, regexp_new, regexp_new_enc};
pub(crate) use translate::warn_pattern;

pub struct RegexpData {
    pub engine: Engine,
    pub source: String,
    /// True only for the blank `Regexp.allocate` answers. Ruby keeps an
    /// uninitialized pattern distinct from an EMPTY one: every reading row
    /// raises `TypeError: uninitialized Regexp` where `//` answers happily,
    /// and a source of `""` cannot tell the two apart.
    pub uninitialized: bool,
    pub ignore_case: bool,
    pub extended: bool,
    pub multiline: bool,
    /// The encoding a `/n`/`/e`/`/s`/`/u` literal FORCED, reported by
    /// `#options`, `#encoding` and `#fixed_encoding?`. `Source` for every
    /// runtime-built regexp (`Regexp.new` has no spelling for these) and for a
    /// plain literal, whose encoding follows its own bytes.
    pub encoding: zeo_abi::RegexpEncoding,
    /// A `/n` or binary-String pattern holding a byte past 0x7f: pinned to
    /// ASCII-8BIT, so `#encoding`, `#options` and every match follow it.
    pub fixed_binary: bool,
    /// The encoding a `/n` pattern was last prepared for (`u8::MAX`: its
    /// own). Ruby recompiles such a pattern for each new subject encoding and
    /// keeps the result, and warns only when that changes to a non-binary one.
    pub prepared_enc: std::sync::atomic::AtomicU8,
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

    /// The same pattern with a `timeout:` of its own.
    pub fn with_timeout(self: &Arc<Self>, timeout: Option<f64>) -> RRegexp {
        if timeout.is_none() {
            return self.clone();
        }
        let copy = self.dup_data(false);
        Arc::new(RegexpData {
            engine: self.engine.clone().with_timeout(timeout),
            ..Arc::try_unwrap(copy).unwrap_or_else(|_| unreachable!("the copy has one owner"))
        })
    }

    /// `Regexp#dup`/`#clone`'s payload copy: a fresh allocation (fresh
    /// object identity, `frozen` per the caller's dup-vs-clone rule) over
    /// clones of the compiled engine and flags.
    pub fn dup_data(&self, frozen: bool) -> RRegexp {
        Arc::new(RegexpData {
            engine: self.engine.clone(),
            source: self.source.clone(),
            uninitialized: self.uninitialized,
            ignore_case: self.ignore_case,
            extended: self.extended,
            multiline: self.multiline,
            encoding: self.encoding,
            fixed_binary: self.fixed_binary,
            prepared_enc: std::sync::atomic::AtomicU8::new(u8::MAX),
            frozen: std::sync::atomic::AtomicBool::new(frozen),
        })
    }

    /// `#encoding`: a forced one, ASCII-8BIT for a pattern pinned there,
    /// else what the source's own characters compute to.
    pub fn encoding_id(&self) -> crate::encoding::EncodingId {
        use zeo_abi::RegexpEncoding as E;
        if self.uninitialized || self.fixed_binary {
            return crate::encoding::ASCII_8BIT;
        }
        match self.encoding {
            E::EucJp => crate::encoding::EUC_JP,
            E::Windows31j => crate::encoding::WINDOWS_31J,
            E::Utf8 => crate::encoding::UTF_8,
            E::None | E::Source | E::Binary => {
                crate::builtins::encoding::computed_encoding_of(&self.source)
            }
        }
    }

    /// `#fixed_encoding?`: a forced encoding, or a source whose characters
    /// pin one.
    pub fn is_fixed_encoding(&self) -> bool {
        self.fixed_binary
            || self.encoding.is_fixed()
            || self.encoding_id() != crate::encoding::US_ASCII
    }
}

/// Link-path proof for the vendored Oniguruma C archive: compiles and runs an
/// onig pattern so a generated program demonstrably resolves the bundled C
/// archive. A cheap, dependency-free link smoke test exercised by an e2e.
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

/// The compiled pattern: Oniguruma over the source as written. `Send + Sync`
/// and immutable after construction -- a search fills a per-call `Region`,
/// never the regex -- so an `Arc` gives it the cheap-clone value semantics
/// every other `RubyValue` payload has.
#[derive(Clone)]
pub struct Engine {
    re: Arc<onig::Regex>,
    /// The same pattern held to ASCII ([`ascii_only`]), for a subject whose
    /// high bytes are bytes: a binary String reaches the engine as Latin-1
    /// text, and `\xB5` must not read as the letter `µ`. Built on first use
    /// from `text` and `opts`; empty `text` means `re` already is one.
    ascii: Arc<std::sync::OnceLock<Option<onig::Regex>>>,
    text: Arc<str>,
    opts: onig::RegexOptions,
    /// `Regexp.new(src, timeout:)`'s per-pattern limit, in seconds; `None`
    /// defers to [`global_timeout`].
    timeout: Option<f64>,
    /// `(engine name, name as written)` for each group name the translate
    /// layer renamed because Oniguruma refuses it (`translate::rename_groups`).
    renames: Option<Arc<[(String, String)]>>,
}

/// Oniguruma's `IGNORECASE_IS_ASCII` and the four `*_IS_ASCII` switches after
/// it: case folds, `\w`/`\b`, `\d`, `\s` and the POSIX brackets all stay within
/// ASCII, which is how ruby reads a binary subject's high bytes.
pub(crate) fn ascii_only(opts: onig::RegexOptions) -> onig::RegexOptions {
    opts | onig::RegexOptions::from_bits_retain(0b1_1111 << 15)
}

/// `Regexp.timeout`, the process-wide default match limit in seconds.
static TIMEOUT: parking_lot::Mutex<Option<f64>> = parking_lot::Mutex::new(None);

pub fn global_timeout() -> Option<f64> {
    *TIMEOUT.lock()
}

pub fn set_global_timeout(seconds: Option<f64>) {
    *TIMEOUT.lock() = seconds;
}

/// A `timeout:` value as ruby reads it (`rb_reg_match_time_limit`): nil is
/// "no limit", anything else converts to a Float and must be positive.
pub fn timeout_seconds(v: &RubyValue) -> Result<Option<f64>, Signal> {
    let seconds = match v {
        RubyValue::Nil => return Ok(None),
        RubyValue::Int(_) | RubyValue::Float(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
            crate::builtins::numeric::num_to_f64_unchecked(v)
        }
        RubyValue::Str(_) => {
            return Err(crate::builtins::type_error!(
                "no implicit conversion to float from string"
            ));
        }
        other => {
            return Err(crate::builtins::type_error!(
                "can't convert {} into Float",
                crate::builtins::convert_name_of(other)
            ));
        }
    };
    // NaN spelled out: it is neither `> 0.0` nor `<= 0.0`, and a plain
    // `seconds <= 0.0` would accept it as a timeout.
    if seconds.is_nan() || seconds <= 0.0 {
        return Err(crate::builtins::arg_error!(
            "invalid timeout: {}",
            v.inspect_string()
        ));
    }
    Ok(Some(seconds))
}

/// One match normalized to byte-offset group spans (index 0 = whole match;
/// `None` = a non-participating optional group). Every downstream consumer
/// (`build_match_data`, `scan`, `split`, `gsub`/`sub`) reads this shape.
pub struct Caps {
    spans: Vec<Option<(usize, usize)>>,
    /// Where the search that produced this match BEGAN, which `\K` divorces
    /// from `spans[0]`: `/a\Kb/` against `"ab"` starts at 0 and reports a
    /// match of `"b"` at 1. This is what `onig_search` answers and what
    /// `Regexp#=~` returns; every other reader wants `spans[0]`.
    start: usize,
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

/// Onig's retry budget for a search with no timeout: its own default. A
/// pattern that backtracks past it answers "no match", ruby's answer for
/// `/(a*)*b/` against a long run of `a` before its memoization made that
/// linear.
const UNTIMED_RETRY_LIMIT: u32 = 10_000_000;

/// The first retry budget under a timeout; each exhausted budget doubles
/// until the wall clock passes the deadline.
const TIMED_RETRY_STEP: u32 = 1 << 20;

/// One Oniguruma search.
///
/// Onig counts retries, not seconds, and cannot be interrupted. With no
/// timeout an exhausted budget reads as "no match" (the crate's own
/// `search_with_options` would PANIC there). With one, the budget is handed
/// out in doubling slices and the clock read between them: the raise is
/// `Regexp::TimeoutError`, as ruby's is, once the deadline has passed.
fn onig_search(
    e: &Engine,
    haystack: &str,
    start: usize,
    mut region: Option<&mut onig::Region>,
    bytes: bool,
) -> Result<Option<usize>, Signal> {
    let re = e.pick(bytes);
    let timeout = e.timeout.or_else(global_timeout);
    let deadline =
        timeout.map(|t| std::time::Instant::now() + std::time::Duration::from_secs_f64(t));
    let mut budget = if deadline.is_some() {
        TIMED_RETRY_STEP
    } else {
        UNTIMED_RETRY_LIMIT
    };
    loop {
        let mut param = onig::MatchParam::default();
        param.set_retry_limit_in_match(budget);
        let found = re.search_with_param(
            haystack,
            start,
            haystack.len(),
            onig::SearchOptions::SEARCH_OPTION_NONE,
            region.as_deref_mut(),
            param,
        );
        let Some(deadline) = deadline else {
            return Ok(found.ok().flatten());
        };
        if std::time::Instant::now() >= deadline {
            return Err(crate::dispatch::raise_error_id(
                REGEXP_TIMEOUT_ERROR_CLASS,
                "regexp match timeout".to_string(),
            ));
        }
        match found {
            Ok(at) => return Ok(at),
            Err(err) if BUDGET_ERRORS.contains(&err.code()) => {
                budget = budget.saturating_mul(2);
            }
            Err(_) => return Ok(None),
        }
    }
}

/// `Regexp::TimeoutError`'s class id -- `exc_id(43)` in the exception table.
const REGEXP_TIMEOUT_ERROR_CLASS: zeo_abi::ClassId = zeo_abi::exc_id(43);

/// Onig's "budget exhausted" codes: `ONIGERR_MATCH_STACK_LIMIT_OVER`,
/// `ONIGERR_RETRY_LIMIT_IN_MATCH_OVER`, `ONIGERR_RETRY_LIMIT_IN_SEARCH_OVER`.
const BUDGET_ERRORS: [i32; 3] = [-15, -17, -18];

impl Engine {
    pub fn new(re: onig::Regex) -> Engine {
        Engine {
            re: Arc::new(re),
            ascii: Arc::default(),
            text: Arc::from(""),
            opts: onig::RegexOptions::REGEX_OPTION_NONE,
            timeout: None,
            renames: None,
        }
    }

    /// Keeps the engine text and options, so a binary subject can have the
    /// ASCII-held twin built from them.
    pub(crate) fn with_text(mut self, text: String, opts: onig::RegexOptions) -> Engine {
        self.text = text.into();
        self.opts = opts;
        self
    }

    /// The compiled pattern a search runs: the ASCII-held twin for a subject
    /// that reads bytes.
    fn pick(&self, bytes: bool) -> &onig::Regex {
        if bytes && !self.text.is_empty() {
            let twin = self.ascii.get_or_init(|| {
                onig::Regex::with_options(&self.text, ascii_only(self.opts), onig::Syntax::ruby()).ok()
            });
            if let Some(twin) = twin {
                return twin;
            }
        }
        &self.re
    }

    pub(crate) fn with_renames(mut self, renames: Vec<(String, String)>) -> Engine {
        if !renames.is_empty() {
            self.renames = Some(renames.into());
        }
        self
    }

    pub fn with_timeout(mut self, timeout: Option<f64>) -> Engine {
        self.timeout = timeout;
        self
    }

    /// `Regexp#timeout`: the per-pattern limit, never the global default.
    pub fn timeout(&self) -> Option<f64> {
        self.timeout
    }

    /// `bytes` here and below: the subject's high bytes are bytes, not
    /// Latin-1 letters -- see [`Engine::pick`].
    pub fn is_match(&self, haystack: &str, bytes: bool) -> Result<bool, Signal> {
        Ok(onig_search(self, haystack, 0, None, bytes)?.is_some())
    }

    /// The first match's group spans, or `None` when the pattern doesn't match.
    fn captures_first(&self, haystack: &str, bytes: bool) -> Result<Option<Caps>, Signal> {
        self.captures_at(haystack, 0, bytes)
    }

    /// The leftmost match whose start is at or after `start`, with group
    /// spans. Searches the WHOLE `haystack` from byte `start` (not a
    /// `haystack[start..]` slice): onig reads the real character before
    /// `start` from the full buffer, so `^`/`$`/`\A`/`\Z`/`\G` anchor against
    /// the true string, and a fresh `Region` holds the byte spans of every
    /// group (`None` for a non-participating group).
    fn captures_at(&self, haystack: &str, start: usize, bytes: bool) -> Result<Option<Caps>, Signal> {
        let mut region = onig::Region::new();
        let Some(at) = onig_search(self, haystack, start, Some(&mut region), bytes)? else {
            return Ok(None);
        };
        Ok(Some(Caps {
            spans: (0..region.len()).map(|i| region.pos(i)).collect(),
            start: at,
        }))
    }

    /// Every non-overlapping match's group spans, left to right, with
    /// CRuby/Onig's zero-width iteration: after an EMPTY match the search
    /// advances one character, but an empty match abutting the PREVIOUS
    /// match's end is still yielded (`"abc".gsub(/b*/, "X") == "XaXXcX"`,
    /// `"aaaa".scan(/a{0,2}/) == ["aa", "aa", ""]`).
    fn captures_all(&self, haystack: &str, bytes: bool) -> Result<Vec<Caps>, Signal> {
        let mut out = Vec::new();
        let mut from = 0usize;
        while from <= haystack.len() {
            let Some(caps) = self.captures_at(haystack, from, bytes)? else {
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
        Ok(out)
    }

    /// Group count INCLUDING the whole-match slot. Onig's `captures_len`
    /// counts capturing groups alone, so `+1` puts group 0 back.
    fn captures_len(&self) -> usize {
        self.re.captures_len() + 1
    }

    /// Every named group as `(name, index)`, one pair per GROUP, in group
    /// order: a name written twice appears twice. [`group_of_name`] picks
    /// which of them a lookup by name answers.
    pub fn capture_names(&self) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        self.re.foreach_name(|name, groups| {
            let written = self
                .renames
                .as_deref()
                .and_then(|r| r.iter().find(|(engine, _)| engine == name))
                .map_or(name, |(_, written)| written.as_str());
            out.extend(groups.iter().map(|&g| (written.to_string(), g as usize)));
            true
        });
        out.sort_by_key(|(_, i)| *i);
        out
    }
}

/// The group a lookup by `name` answers: the LAST group of that name that
/// took part in the match, or the last group of that name when none did
/// (Onigmo's `onig_name_to_backref_number` with a region).
pub(crate) fn group_of_name(
    names: &[(String, usize)],
    groups: &[Option<(usize, usize)>],
    name: &str,
) -> Option<usize> {
    let (mut last, mut last_matched) = (None, None);
    for (_, idx) in names.iter().filter(|(n, _)| n == name) {
        last = Some(*idx);
        if groups.get(*idx).copied().flatten().is_some() {
            last_matched = Some(*idx);
        }
    }
    last_matched.or(last)
}

/// Each distinct group name once, in the order the pattern first writes it.
pub(crate) fn distinct_names(names: &[(String, usize)]) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for (n, _) in names {
        if !out.contains(&n.as_str()) {
            out.push(n);
        }
    }
    out
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

/// Every public search's first step, ruby's `rb_reg_prepare_enc`: refuses a
/// subject the pattern's encoding cannot read (or warns, for `/n`), and
/// answers whether the subject's high bytes are bytes -- a binary String
/// reaches here as Latin-1 text.
fn enter(re: &RRegexp, haystack: &str, enc: crate::encoding::EncodingId) -> Result<bool, Signal> {
    let ascii = haystack.is_ascii();
    crate::builtins::encoding::guard_regexp_haystack(re, enc, ascii)?;
    Ok(enc == crate::encoding::ASCII_8BIT && !ascii)
}

/// Whether `re` matches at or after `byte_start` -- `match?`'s question,
/// which sets no `$~` and so builds no MatchData.
pub fn regexp_is_match_at(
    re: &RRegexp,
    haystack: &str,
    byte_start: usize,
    enc: crate::encoding::EncodingId,
) -> Result<bool, Signal> {
    let bytes = enter(re, haystack, enc)?;
    Ok(re.engine.captures_at(haystack, byte_start, bytes)?.is_some())
}

/// [`regexp_match`] starting at a BYTE offset, over the whole haystack.
///
/// `String#match(pattern, pos)` cannot slice: the MatchData built from a
/// slice reports offsets relative to it, so `.begin(0)` answered 1 where
/// ruby says 4, and `pre_match` lost everything before `pos`. The engine
/// already takes a start offset -- this is CRuby's `rb_reg_search(str, re,
/// pos, 0)`, which is anchored the same way.
pub fn regexp_match_at(
    re: &RRegexp,
    haystack: &str,
    byte_start: usize,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    let bytes = enter(re, haystack, enc)?;
    Ok(match re.engine.captures_at(haystack, byte_start, bytes)? {
        Some(caps) => {
            let m = build_match_data(re, haystack, &caps, enc);
            crate::lastmatch::set_last_match(Some(m.clone()));
            RubyValue::MatchData(m)
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    })
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
        (
            subject_byte_offset(&md.haystack, lo, md.enc) as i64,
            subject_byte_offset(&md.haystack, hi, md.enc) as i64,
        )
    } else {
        (char_index(&md.haystack, lo), char_index(&md.haystack, hi))
    };
    Ok(offset_pair(RubyValue::Int(lo), RubyValue::Int(hi)))
}

/// The offset in the subject's own bytes of `off`, an offset into its
/// decoded text: a binary or Latin-1 character is one byte there.
pub(crate) fn subject_byte_offset(
    haystack: &str,
    off: usize,
    enc: crate::encoding::EncodingId,
) -> usize {
    let prefix = &haystack[..off];
    if enc == crate::encoding::UTF_8 || prefix.is_ascii() {
        return off;
    }
    match crate::builtins::string::str_value_in_enc(enc, prefix) {
        RubyValue::Str(s) => s.lock().bytes().len(),
        _ => off,
    }
}

/// [`subject_byte_offset`] backwards: the text offset of the character that
/// starts at byte `raw` of the subject, or `None` inside a character.
pub(crate) fn subject_text_offset(
    haystack: &str,
    raw: usize,
    enc: crate::encoding::EncodingId,
) -> Option<usize> {
    if enc == crate::encoding::UTF_8 || haystack.is_ascii() {
        return haystack.is_char_boundary(raw.min(haystack.len())).then_some(raw);
    }
    let mut seen = 0usize;
    for (at, c) in haystack.char_indices() {
        if seen == raw {
            return Some(at);
        }
        if seen > raw {
            return None;
        }
        seen += subject_byte_offset(c.encode_utf8(&mut [0; 4]), c.len_utf8(), enc);
    }
    (seen == raw).then_some(haystack.len())
}

fn offset_pair(a: RubyValue, b: RubyValue) -> RubyValue {
    RubyValue::Array(crate::array_new(vec![a, b]))
}

fn name_group_index(md: &RMatchData, name: &str) -> Result<i64, crate::Signal> {
    group_of_name(&md.names, &md.groups, name)
        .map(|i| i as i64)
        .ok_or_else(|| index_error!("undefined group name reference: {name}"))
}

/// `MatchData#names` -- each group name once, in the order first written.
pub fn matchdata_names(md: &RMatchData) -> RubyValue {
    let out = distinct_names(&md.names)
        .into_iter()
        .map(|n| RubyValue::Str(crate::string_new(n.to_string())))
        .collect();
    RubyValue::Array(crate::array_new(out))
}

/// `MatchData#regexp` -- the `Regexp` that produced the match.
pub fn matchdata_regexp(md: &RMatchData) -> RubyValue {
    RubyValue::Regexp(md.regexp.clone())
}

/// `Regexp#match`/`String#match` -- a real `MatchData`, or `nil` if the
/// pattern doesn't match at all.
///
/// `enc` is what the haystack was decoded FROM. It is a REQUIRED argument
/// rather than a UTF-8 default: every string sliced out of the match
/// (`[0]`, a group, `pre_match`, `post_match`, `#string`) has to come back
/// in the subject's own encoding, and a default silently answered UTF-8 for
/// a binary or Latin-1 subject. Every match builder here takes it for the
/// same reason.
pub fn regexp_match(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    let bytes = enter(re, haystack, enc)?;
    Ok(match re.engine.captures_first(haystack, bytes)? {
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
    })
}

/// `Regexp#match?`/`String#match?` -- a plain bool, no `MatchData`
/// allocated (mirrors real Ruby: `match?` is specifically the
/// no-side-effect, no-allocation probe).
pub fn regexp_is_match(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<bool, Signal> {
    let bytes = enter(re, haystack, enc)?;
    re.engine.is_match(haystack, bytes)
}

/// `StringScanner`'s anchored probe: the byte length of `re`'s leftmost match
/// when it begins exactly at the start of `haystack`, else `None`.
/// `StringScanner#scan`/`#skip` match anchored at the scanner's current
/// position, so the caller passes the not-yet-scanned tail and treats a
/// `Some(len)` as "consume `len` bytes". Reuses the same `Caps` normalization
/// every other engine consumer goes through. (Documented divergence: `^`/`\A`
/// and look-behind see `haystack`'s start as the string start, not the
/// original position -- acceptable for the scanner's tail-slice model.)
pub fn regexp_anchored_len(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<Option<usize>, Signal> {
    let bytes = enter(re, haystack, enc)?;
    anchored_len(re, haystack, bytes)
}

fn anchored_len(re: &RRegexp, haystack: &str, bytes: bool) -> Result<Option<usize>, Signal> {
    Ok(re
        .engine
        .captures_first(haystack, bytes)?
        .and_then(|caps| caps.get(0))
        .and_then(|(start, end)| (start == 0).then_some(end)))
}

/// The byte span `(start, end)` of `re`'s leftmost match in `haystack`, or
/// `None`. `StringScanner#scan_until`/`#exist?` need where the *next* match
/// lands (not anchored), which is exactly this.
pub fn regexp_find(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<Option<(usize, usize)>, Signal> {
    let bytes = enter(re, haystack, enc)?;
    Ok(re.engine.captures_first(haystack, bytes)?.and_then(|c| c.get(0)))
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
/// `fixed` is `StringScanner.new(str, fixed_anchor: true)`: the engine sees
/// the WHOLE subject and the scan position is only the search start, so `\A`
/// keeps meaning the string's own head and look-behind reads real context.
/// The default mode hands the engine the tail slice instead, which is CRuby's
/// documented behaviour there: the position acts as the string start.
///
/// (Documented divergence in the default mode only, inherited from
/// [`regexp_anchored_len`]: `\G` also sees the scan position as the string
/// start, which happens to agree with CRuby, where `\G` anchors at the
/// position.)
pub fn scanner_match(
    pattern: &crate::RubyValue,
    subject: &str,
    at: usize,
    anchored: bool,
    fixed: bool,
) -> Result<Option<ScannerMatch>, crate::Signal> {
    if fixed {
        return scanner_match_fixed(pattern, subject, at, anchored);
    }
    let tail = &subject[at..];
    let husk = husk_payload(pattern);
    let pattern = husk.as_ref().unwrap_or(pattern);
    let (spans, names) = match pattern {
        crate::RubyValue::Regexp(re) => {
            let caps = match re.engine.captures_first(tail, false)? {
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

/// The `fixed_anchor: true` half of [`scanner_match`]. The engine searches the
/// FULL subject from `at`, so every span is already absolute; `anchored` means
/// the match must START at `at` -- exactly what `\G(?:pattern)` would demand,
/// without recompiling the pattern.
fn scanner_match_fixed(
    pattern: &crate::RubyValue,
    subject: &str,
    at: usize,
    anchored: bool,
) -> Result<Option<ScannerMatch>, crate::Signal> {
    let husk = husk_payload(pattern);
    let pattern = husk.as_ref().unwrap_or(pattern);
    let (spans, names) = match pattern {
        crate::RubyValue::Regexp(re) => {
            let caps = match re.engine.captures_at(subject, at, false)? {
                Some(caps) if !anchored || caps.spans[0].is_some_and(|(s, _)| s == at) => caps,
                _ => return Ok(None),
            };
            (caps.spans, re.engine.capture_names())
        }
        crate::RubyValue::Str(s) => {
            let literal = s.lock().to_utf8_lossy().into_owned();
            let start = match anchored {
                true if subject.as_bytes()[at..].starts_with(literal.as_bytes()) => at,
                true => return Ok(None),
                false => match subject[at..].find(&literal) {
                    Some(i) => at + i,
                    None => return Ok(None),
                },
            };
            (vec![Some((start, start + literal.len()))], Vec::new())
        }
        other => {
            return Err(crate::builtins::no_implicit(other, "String"));
        }
    };
    Ok(Some(ScannerMatch {
        groups: spans,
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
pub fn regexp_case_eq(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<bool, Signal> {
    let bytes = enter(re, haystack, enc)?;
    Ok(match re.engine.captures_first(haystack, bytes)? {
        Some(caps) => {
            crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps, enc)));
            true
        }
        None => {
            crate::lastmatch::set_last_match(None);
            false
        }
    })
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
pub fn regexp_match_index(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    match_index(re, haystack, enc, |c| {
        c.get(0).expect("group 0 always exists on a match").0
    })
}

/// `Regexp#=~`/`String#=~`, which report the SEARCH start rather than the
/// match start. The two differ only under `\K`: ruby's `rb_reg_match` hands
/// back `rb_reg_search`'s own answer, so `/a\Kb/ =~ "ab"` is `0` while
/// `$~.begin(0)` is `1`. `String#index` and `String#split` keep the match
/// start, which is why this is a separate entry.
pub fn regexp_search_index(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    match_index(re, haystack, enc, |c| c.start)
}

fn match_index(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
    pick: fn(&Caps) -> usize,
) -> Result<RubyValue, Signal> {
    let bytes = enter(re, haystack, enc)?;
    Ok(match re.engine.captures_first(haystack, bytes)? {
        Some(caps) => {
            let start = pick(&caps);
            crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps, enc)));
            RubyValue::Int(char_index(haystack, start))
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    })
}

/// `String#rindex(regexp[, pos])` -- the CHAR index of the RIGHTMOST match
/// whose start is at or before `before` (a char index; `None` searches the
/// whole string), or `nil`. Records `$~` like the leftward probes.
pub fn regexp_rindex(
    re: &RRegexp,
    haystack: &str,
    before: Option<usize>,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    // CRuby's `rindex(regexp)` is the LARGEST start position (char index, at or
    // before `before`) where the pattern matches ANCHORED -- it tries every
    // start from the end, so /\d+/ on "hello123world" answers 7 ("3"), not the
    // greedy left-most non-overlapping match at 5. `regexp_byterindex` already
    // implements that scan; this just maps the char limit in and the byte offset
    // (plus `$~`) back out.
    let bytes = enter(re, haystack, enc)?;
    let clen = haystack.chars().count();
    let char_limit = before.unwrap_or(clen).min(clen);
    let byte_limit = haystack
        .char_indices()
        .nth(char_limit)
        .map_or(haystack.len(), |(b, _)| b);
    Ok(match byterindex(re, haystack, byte_limit, bytes)? {
        Some(byte_start) => {
            if let Some(caps) = anchored_caps_at(re, haystack, byte_start, bytes)? {
                crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps, enc)));
            }
            RubyValue::Int(char_index(haystack, byte_start))
        }
        None => {
            crate::lastmatch::set_last_match(None);
            RubyValue::Nil
        }
    })
}

/// The capture spans of the match ANCHORED at `byte_start`, expressed as
/// full-haystack byte offsets (so `$~`/`MatchData` slice correctly). `None` if
/// nothing matches exactly there.
fn anchored_caps_at(
    re: &RRegexp,
    haystack: &str,
    byte_start: usize,
    bytes: bool,
) -> Result<Option<Caps>, Signal> {
    let Some(caps) = re.engine.captures_first(&haystack[byte_start..], bytes)? else {
        return Ok(None);
    };
    if caps.get(0).is_none_or(|(s, _)| s != 0) {
        return Ok(None); // not anchored at byte_start
    }
    Ok(Some(Caps {
        spans: caps
            .spans
            .iter()
            .map(|s| s.map(|(a, b)| (a + byte_start, b + byte_start)))
            .collect(),
        start: caps.start + byte_start,
    }))
}

/// `String#byterindex(regexp[, pos])` -- the BYTE offset of the LAST (highest)
/// start position at or before `before` where `re` matches anchored, or
/// `None`. CRuby's `rindex` tries every start from the end, so `/l+/` against
/// `"hello"` finds the single `"l"` at 3, not the greedy `"ll"` leftmost at 2.
pub fn regexp_byterindex(
    re: &RRegexp,
    haystack: &str,
    before: usize,
    enc: crate::encoding::EncodingId,
) -> Result<Option<usize>, Signal> {
    let bytes = enter(re, haystack, enc)?;
    byterindex(re, haystack, before, bytes)
}

fn byterindex(re: &RRegexp, haystack: &str, before: usize, bytes: bool) -> Result<Option<usize>, Signal> {
    let mut p = before.min(haystack.len());
    loop {
        if haystack.is_char_boundary(p) && anchored_len(re, &haystack[p..], bytes)?.is_some() {
            return Ok(Some(p));
        }
        if p == 0 {
            return Ok(None);
        }
        p -= 1;
    }
}

/// `Regexp#source`: the pattern as written, in the regexp's own encoding --
/// US-ASCII for an ASCII pattern, the raw bytes for a binary one.
pub fn regexp_source(re: &RRegexp) -> RubyValue {
    crate::builtins::string::str_value_in_enc(re.encoding_id(), &re.source)
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
            // An unprintable ASCII byte prints as `\xHH`, so a NUL in a
            // pattern never reaches the terminal raw. The five ASCII
            // whitespace bytes are the exception ruby makes: `\t` and `\n`
            // print as themselves.
            other
                if (other.is_ascii() && !other.is_ascii_graphic() && other != ' ')
                    && !matches!(other, '\t' | '\n' | '\x0b' | '\x0c' | '\r') =>
            {
                out.push_str(&format!("\\x{:02X}", other as u32));
            }
            other => out.push(other),
        }
    }
    out
}

/// `Regexp#to_s` -- what `puts`/string interpolation display for a Regexp
/// value (real Ruby: `Kernel#puts`/`#{}` both call `to_s`, not `inspect`).
pub fn regexp_to_s(re: &RRegexp) -> RubyValue {
    let (set, unset) = flags_split(re.ignore_case, re.extended, re.multiline);
    let text = format!("(?{set}-{unset}:{})", escape_forward_slashes(&re.source));
    // A binary pattern's bytes go in raw.
    if re.fixed_binary {
        return crate::builtins::string::str_value_in_enc(crate::encoding::ASCII_8BIT, &text);
    }
    RubyValue::Str(string_new(text))
}

/// `Regexp#inspect` -- the `/pattern/flags` literal form, flags in `m,i,x`
/// order (verified against real `ruby`).
///
/// Of the four ENCODING letters only `/n` shows: it says the pattern is
/// encoding-agnostic, which re-reading the printed form has no other way to
/// learn. `/e`, `/s` and `/u` print bare (`/x/e.inspect` is `"/x/"`), since the
/// encoding rides on the object rather than on its source (oracle-verified).
pub fn regexp_inspect(re: &RRegexp) -> RubyValue {
    // A blank has no source to print between the slashes, so ruby names it by
    // address instead -- the one row on an uninitialized Regexp that answers.
    if re.uninitialized {
        return RubyValue::Str(string_new(format!(
            "#<Regexp:0x{:016x}>",
            Arc::as_ptr(re) as *const () as usize
        )));
    }
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
    let body = escape_forward_slashes(&re.source);
    // A binary pattern's high bytes print as `\xHH`, in an ASCII-8BIT String.
    if re.fixed_binary {
        let mut escaped = String::with_capacity(body.len());
        for c in body.chars() {
            match c as u32 {
                0x80..=0xff => escaped.push_str(&format!("\\x{:02X}", c as u32)),
                _ => escaped.push(c),
            }
        }
        return RubyValue::Str(crate::string_from_bytes(
            format!("/{escaped}/{flags}").into_bytes(),
            crate::encoding::ASCII_8BIT,
        ));
    }
    RubyValue::Str(string_new(format!("/{body}/{flags}")))
}

/// `Regexp#scan`... no -- `String#scan`: every match, as a plain `String`
/// (the whole match) if the pattern has no capture groups, or as an `Array`
/// of the captured groups (nil for a non-participating optional group) if it
/// does -- matches real Ruby's own shape-switching behavior exactly.
pub fn regexp_scan(
    re: &RRegexp,
    haystack: &str,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    let bytes = enter(re, haystack, enc)?;
    let has_groups = re.engine.captures_len() > 1;
    let mut results = Vec::new();
    // `$~` ends up on the LAST match -- CRuby's `scan` writes the backref per
    // iteration, so the final state is the last one (nil when nothing
    // matched, same as any failed match).
    let mut last_md = None;
    for caps in re.engine.captures_all(haystack, bytes)? {
        last_md = Some(build_match_data(re, haystack, &caps, enc));
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
    Ok(RubyValue::Array(array_new(results)))
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
    let bytes = enter(re, haystack, enc)?;
    let has_groups = re.engine.captures_len() > 1;
    for caps in re.engine.captures_all(haystack, bytes)? {
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
        crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps, enc)));
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
pub fn regexp_split(
    re: &RRegexp,
    haystack: &str,
    limit: i64,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    let bytes = enter(re, haystack, enc)?;
    let mut segments: Vec<String> = Vec::new();
    let mut last_end = 0usize;
    let mut fields = 0i64;
    for caps in re.engine.captures_all(haystack, bytes)? {
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
    Ok(RubyValue::Array(array_new(items)))
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
                match group_of_name(names, &caps.spans, &name) {
                    Some(idx) => {
                        if let Some(g) = caps.str(idx, haystack) {
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
pub fn regexp_gsub(
    re: &RRegexp,
    haystack: &str,
    replacement: &str,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    let bytes = enter(re, haystack, enc)?;
    let names = re.engine.capture_names();
    let mut out = String::new();
    let mut last_end = 0usize;
    for caps in re.engine.captures_all(haystack, bytes)? {
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
pub fn regexp_sub(
    re: &RRegexp,
    haystack: &str,
    replacement: &str,
    enc: crate::encoding::EncodingId,
) -> Result<RubyValue, Signal> {
    let bytes = enter(re, haystack, enc)?;
    let names = re.engine.capture_names();
    match re.engine.captures_first(haystack, bytes)? {
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
    let bytes = enter(re, haystack, enc)?;
    let mut out = String::new();
    let mut last_end = 0usize;
    for caps in re.engine.captures_all(haystack, bytes)? {
        let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
        out.push_str(&haystack[last_end..m_start]);
        // Each iteration sets `$~`/`$1..` so the block can read the capture
        // groups of the CURRENT match (CRuby updates the frame's backref).
        crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps, enc)));
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
    let bytes = enter(re, haystack, enc)?;
    match re.engine.captures_first(haystack, bytes)? {
        Some(caps) => {
            let (m_start, m_end) = caps.get(0).expect("group 0 is always the whole match");
            crate::lastmatch::set_last_match(Some(build_match_data(re, haystack, &caps, enc)));
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
    match group_of_name(&m.names, &m.groups, name) {
        Some(idx) => Ok(matchdata_group(m, idx as i64)),
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
    let pairs = distinct_names(&m.names)
        .into_iter()
        .map(|name| {
            let value = group_of_name(&m.names, &m.groups, name)
                .map_or(RubyValue::Nil, |idx| matchdata_group(m, idx as i64));
            (RubyValue::Str(string_new(name.to_string())), value)
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

    // The engine rows over a UTF-8 subject, which every test here uses.
    fn regexp_is_match(re: &RRegexp, haystack: &str) -> Result<bool, Signal> {
        super::regexp_is_match(re, haystack, crate::encoding::UTF_8)
    }

    fn regexp_gsub(re: &RRegexp, haystack: &str, with: &str) -> Result<RubyValue, Signal> {
        super::regexp_gsub(re, haystack, with, crate::encoding::UTF_8)
    }

    fn regexp_sub(re: &RRegexp, haystack: &str, with: &str) -> Result<RubyValue, Signal> {
        super::regexp_sub(re, haystack, with, crate::encoding::UTF_8)
    }

    fn regexp_split(re: &RRegexp, haystack: &str, limit: i64) -> Result<RubyValue, Signal> {
        super::regexp_split(re, haystack, limit, crate::encoding::UTF_8)
    }

    fn strs(v: &RubyValue) -> Vec<String> {
        let RubyValue::Array(a) = v else {
            panic!("expected an Array")
        };
        a.lock().iter().map(RubyValue::to_display_string).collect()
    }

    #[test]
    fn backreferences_and_lookaround_match() {
        let dbl = regexp_new(r"(\w)\1", false, false, false).unwrap();
        assert!(regexp_is_match(&dbl, "hello").unwrap());
        assert!(!regexp_is_match(&dbl, "abc").unwrap());

        let look = regexp_new(r"(?<=\$)\d+", false, false, false).unwrap();
        let RubyValue::Str(s) = regexp_gsub(&look, "$100 and $5", "N").unwrap() else {
            panic!("expected a Str")
        };
        assert_eq!(s.lock().to_utf8_lossy(), "$N and $N");
    }

    /// Oracle-verified against real `ruby` (see the corpus's recorded
    /// `split_*` tests for the e2e-visible half of this behavior) -- tested
    /// directly here too, because `puts` prints an empty Array and an Array
    /// of one empty string identically, so a golden cannot pin the
    /// empty-haystack case; this is the one place it is verified.
    #[test]
    fn split_matches_real_ruby_leniency() {
        let comma = regexp_new(",", false, false, false).unwrap();
        assert_eq!(
            strs(&regexp_split(&comma, "a,b,,c", 0).unwrap()),
            ["a", "b", "", "c"]
        );
        assert_eq!(
            strs(&regexp_split(&comma, ",a,b", 0).unwrap()),
            ["", "a", "b"]
        );
        assert_eq!(strs(&regexp_split(&comma, "a,b,", 0).unwrap()), ["a", "b"]);
        assert!(strs(&regexp_split(&comma, "", 0).unwrap()).is_empty());

        let digit = regexp_new(r"\d", false, false, false).unwrap();
        assert_eq!(
            strs(&regexp_split(&digit, "a1b2c3", 0).unwrap()),
            ["a", "b", "c"]
        );

        let no_match = regexp_new("x", false, false, false).unwrap();
        assert_eq!(strs(&regexp_split(&no_match, "abc", 0).unwrap()), ["abc"]);
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

    /// Ruby's `^`/`$` are always line-anchored, and `/m` makes `.` match a
    /// newline -- onig's `MULTILINE` option, not a line-anchor mode.
    #[test]
    fn flags_mean_what_ruby_means() {
        let re = regexp_new("^line2", false, false, false).unwrap();
        assert!(regexp_is_match(&re, "line1\nline2").unwrap());

        let dot = regexp_new("c.d", false, false, false).unwrap();
        assert!(!regexp_is_match(&dot, "abc\ndef").unwrap());
        let dot_m = regexp_new("c.d", false, false, true).unwrap();
        assert!(regexp_is_match(&dot_m, "abc\ndef").unwrap());

        let ci = regexp_new("hello", true, false, false).unwrap();
        assert!(regexp_is_match(&ci, "HELLO").unwrap());
    }

    #[test]
    fn scan_switches_shape_based_on_capture_groups() {
        let word = regexp_new(r"\w+", false, false, false).unwrap();
        assert_eq!(
            strs(&regexp_scan(&word, "one two", crate::encoding::UTF_8).unwrap()),
            ["one", "two"]
        );

        let pair = regexp_new(r"([a-z])(\d)", false, false, false).unwrap();
        let RubyValue::Array(a) = regexp_scan(&pair, "a1b2", crate::encoding::UTF_8).unwrap()
        else {
            panic!("expected an Array")
        };
        let groups = a.lock();
        assert_eq!(groups.len(), 2);
        assert_eq!(strs(&groups[0]), ["a", "1"]);
        assert_eq!(strs(&groups[1]), ["b", "2"]);
    }
}
