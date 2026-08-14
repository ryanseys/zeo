//! `Date._strptime` -- the format-DIRECTED scanner, next to `parse.rs`'s
//! heuristic one.
//!
//! Where `Date._parse` guesses at an unknown shape, this is told the shape and
//! either matches it or answers `nil`. `Date.strptime`, `DateTime.strptime`
//! and -- through the vendored `time` gem -- `Time.strptime` all read their
//! fields out of it, which is where aws-sigv4's `presign_url` reaches it.
//!
//! The answered hash's key order IS the order the directives matched, the same
//! contract `parse.rs` keeps, because the hash is printed as-is.
//!
//! Anything the format asks for that the input does not have answers `None`
//! for the whole call: a partial match is not a match. Input LEFT OVER after
//! the format is fine and is reported as `:leftover`, which is CRuby's rule
//! (`Date._strptime("2000-10-31 extra", "%Y-%m-%d")` keeps `" extra"`).

use super::parse::{named_offset, numeric_offset};
use crate::builtins::rational::rational_new;
use crate::{RubyValue, string_new};
use num_bigint::BigInt;

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const DAYS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

/// Accumulates in match order -- see the module docs.
#[derive(Default)]
struct Fields {
    pairs: Vec<(&'static str, RubyValue)>,
}

impl Fields {
    fn int(&mut self, key: &'static str, value: i64) {
        self.pairs.push((key, RubyValue::Int(value)));
    }

    fn str(&mut self, key: &'static str, value: &str) {
        self.pairs
            .push((key, RubyValue::Str(string_new(value.to_string()))));
    }

    fn into_pairs(self) -> Vec<(RubyValue, RubyValue)> {
        self.pairs
            .into_iter()
            .map(|(k, v)| (RubyValue::Symbol(crate::Symbol::intern(k)), v))
            .collect()
    }
}

/// A cursor over the input, in CHARS -- a `%Z` may be any zone name and the
/// corpus has non-ASCII ones.
struct Cursor {
    chars: Vec<char>,
    at: usize,
}

impl Cursor {
    fn new(s: &str) -> Cursor {
        Cursor {
            chars: s.chars().collect(),
            at: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn rest(&self) -> String {
        self.chars[self.at.min(self.chars.len())..].iter().collect()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    /// Up to `max` digits, with an optional leading sign when `signed`.
    /// CRuby's own scanner skips leading blanks first, which is what lets
    /// `%d` read `" 4"` (the `%e` spelling) with no separate directive.
    fn number(&mut self, max: usize, signed: bool) -> Option<(i64, usize)> {
        self.skip_spaces();
        let start = self.at;
        let mut neg = false;
        if signed && matches!(self.peek(), Some('+') | Some('-')) {
            neg = self.peek() == Some('-');
            self.at += 1;
        }
        let digits_at = self.at;
        let mut value: i64 = 0;
        while self.at - digits_at < max
            && let Some(c) = self.peek()
            && c.is_ascii_digit()
        {
            value = value.checked_mul(10)?.checked_add(c as i64 - '0' as i64)?;
            self.at += 1;
        }
        if self.at == digits_at {
            self.at = start;
            return None;
        }
        Some((if neg { -value } else { value }, self.at - digits_at))
    }

    /// Matches the longest name in `names` case-insensitively, answering its
    /// index. Longest-first so `"june"` is not claimed by a `"jun"` prefix.
    fn name(&mut self, names: &[&str]) -> Option<usize> {
        let rest = self.rest().to_lowercase();
        let mut best: Option<(usize, usize)> = None;
        for (i, full) in names.iter().enumerate() {
            for cand in [*full, &full[..3.min(full.len())]] {
                if rest.starts_with(cand) && best.is_none_or(|(len, _)| cand.chars().count() > len)
                {
                    best = Some((cand.chars().count(), i));
                }
            }
        }
        let (len, idx) = best?;
        self.at += len;
        Some(idx)
    }
}

/// `Date._strptime(input, format)` -- `None` where CRuby answers `nil`.
pub(super) fn date_strptime(input: &str, format: &str) -> Option<Vec<(RubyValue, RubyValue)>> {
    let mut f = Fields::default();
    let mut cur = Cursor::new(input);
    let mut merid: Option<bool> = None;
    if !scan(&mut cur, format, &mut f, &mut merid) {
        return None;
    }
    // `%p` is applied at the END: it may be written before or after `%I`, and
    // either way it adjusts the hour that directive read.
    if let Some(pm) = merid
        && let Some(slot) = f.pairs.iter_mut().find(|(k, _)| *k == "hour")
        && let RubyValue::Int(h) = slot.1
    {
        let h12 = h % 12;
        slot.1 = RubyValue::Int(if pm { h12 + 12 } else { h12 });
    }
    let leftover = cur.rest();
    if !leftover.is_empty() {
        f.str("leftover", &leftover);
    }
    Some(f.into_pairs())
}

/// Walks `format`, consuming `cur`. `false` the moment anything disagrees.
fn scan(cur: &mut Cursor, format: &str, f: &mut Fields, merid: &mut Option<bool>) -> bool {
    let fmt: Vec<char> = format.chars().collect();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c != '%' {
            if c.is_whitespace() {
                // Whitespace in the format matches any run of it, including
                // none -- CRuby is deliberately lax here.
                cur.skip_spaces();
                i += 1;
                continue;
            }
            if cur.peek() != Some(c) {
                return false;
            }
            cur.at += 1;
            i += 1;
            continue;
        }
        i += 1;
        // `%-d`/`%_d`/`%0d`/`%10d` -- flags and a width, which only affect
        // FORMATTING. Scanning ignores them, but it still has to step past.
        while i < fmt.len() && matches!(fmt[i], '-' | '_' | '0' | '^' | '#') {
            i += 1;
        }
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            i += 1;
        }
        let Some(&d) = fmt.get(i) else {
            // A trailing bare `%` matches a literal one.
            return cur.peek() == Some('%') && {
                cur.at += 1;
                true
            };
        };
        i += 1;
        // CRuby's `NUM_PATTERN_P`: an unbounded numeric directive reads only
        // its natural width when DIGITS follow it in the format, because
        // nothing else could tell where it ends. `%Y%m%d` against "20130524"
        // is the whole reason -- greedy, `%Y` swallows all eight and the rest
        // of the format has nothing left to match.
        let numeric_next = fmt.get(i).is_some_and(char::is_ascii_digit)
            || (fmt.get(i) == Some(&'%') && fmt.get(i + 1).is_some_and(|c| reads_digits(*c)));
        if !directive(cur, d, f, merid, numeric_next) {
            return false;
        }
    }
    true
}

/// One directive. Composites (`%F`, `%T`, ...) recurse through `scan` on their
/// expansion, so each shape is spelled once.
fn directive(
    cur: &mut Cursor,
    d: char,
    f: &mut Fields,
    merid: &mut Option<bool>,
    numeric_next: bool,
) -> bool {
    let expand = |cur: &mut Cursor, f: &mut Fields, merid: &mut Option<bool>, s: &str| {
        scan(cur, s, f, merid)
    };
    match d {
        '%' => {
            if cur.peek() != Some('%') {
                return false;
            }
            cur.at += 1;
            true
        }
        'n' | 't' => {
            cur.skip_spaces();
            true
        }
        'Y' => match cur.number(if numeric_next { 4 } else { 10 }, true) {
            Some((v, _)) => {
                f.int("year", v);
                true
            }
            None => false,
        },
        'C' => match cur.number(2, true) {
            Some((v, _)) => {
                f.int("_cent", v);
                true
            }
            None => false,
        },
        // Two digits, widened the way `Date._parse` widens: 0..=68 this
        // century, 69..=99 the last.
        'y' => match cur.number(2, false) {
            Some((v, _)) => {
                f.int("year", if v <= 68 { 2000 + v } else { 1900 + v });
                true
            }
            None => false,
        },
        'm' => bounded(cur, f, "mon", 2, 1, 12),
        'd' | 'e' => bounded(cur, f, "mday", 2, 1, 31),
        'j' => bounded(cur, f, "yday", 3, 1, 366),
        'H' | 'k' => bounded(cur, f, "hour", 2, 0, 24),
        'I' | 'l' => bounded(cur, f, "hour", 2, 1, 12),
        'M' => bounded(cur, f, "min", 2, 0, 59),
        'S' => bounded(cur, f, "sec", 2, 0, 60),
        'u' => bounded(cur, f, "cwday", 1, 1, 7),
        'w' => bounded(cur, f, "wday", 1, 0, 6),
        'L' => fraction(cur, f, 3),
        'N' => fraction(cur, f, 9),
        's' => match cur.number(19, true) {
            Some((v, _)) => {
                f.int("seconds", v);
                true
            }
            None => false,
        },
        'B' | 'b' | 'h' => match cur.name(&MONTHS) {
            Some(idx) => {
                f.int("mon", idx as i64 + 1);
                true
            }
            None => false,
        },
        'A' | 'a' => match cur.name(&DAYS) {
            Some(idx) => {
                f.int("wday", idx as i64);
                true
            }
            None => false,
        },
        'p' | 'P' => {
            let rest = cur.rest().to_lowercase();
            for (tag, pm) in [("am", false), ("pm", true), ("a.m.", false), ("p.m.", true)] {
                if rest.starts_with(tag) {
                    cur.at += tag.chars().count();
                    *merid = Some(pm);
                    return true;
                }
            }
            false
        }
        'z' | 'Z' => zone(cur, f),
        'F' => expand(cur, f, merid, "%Y-%m-%d"),
        'T' | 'X' => expand(cur, f, merid, "%H:%M:%S"),
        'D' | 'x' => expand(cur, f, merid, "%m/%d/%y"),
        'R' => expand(cur, f, merid, "%H:%M"),
        'r' => expand(cur, f, merid, "%I:%M:%S %p"),
        'v' => expand(cur, f, merid, "%e-%b-%Y"),
        'c' => expand(cur, f, merid, "%a %b %e %H:%M:%S %Y"),
        // An unknown directive is not a licence to guess: refuse, so the
        // caller's `nil` says the format was not understood.
        _ => false,
    }
}

/// Whether a directive consumes DIGITS from the input -- the other half of
/// `NUM_PATTERN_P`, so `%Y%m` caps the year but `%Y%b` does not.
fn reads_digits(d: char) -> bool {
    matches!(
        d,
        'Y' | 'C'
            | 'y'
            | 'm'
            | 'd'
            | 'e'
            | 'j'
            | 'H'
            | 'k'
            | 'I'
            | 'l'
            | 'M'
            | 'S'
            | 'L'
            | 'N'
            | 's'
            | 'u'
            | 'w'
    )
}

/// A fixed-width number that must land inside `lo..=hi` -- CRuby rejects
/// `"13"` for `%m` rather than carrying it.
fn bounded(
    cur: &mut Cursor,
    f: &mut Fields,
    key: &'static str,
    width: usize,
    lo: i64,
    hi: i64,
) -> bool {
    match cur.number(width, false) {
        Some((v, _)) if v >= lo && v <= hi => {
            f.int(key, v);
            true
        }
        _ => false,
    }
}

/// `%L`/`%N` -- a fraction of a second, answered as the exact Rational CRuby
/// answers (`.678` is `(339/500)`, not a float).
fn fraction(cur: &mut Cursor, f: &mut Fields, width: usize) -> bool {
    let start = cur.at;
    let Some((_, digits)) = cur.number(width, false) else {
        return false;
    };
    let text: String = cur.chars[start..cur.at].iter().collect();
    let text = text.trim();
    let Ok(num) = text.parse::<BigInt>() else {
        return false;
    };
    let den = BigInt::from(10u8).pow(digits as u32);
    match rational_new(num, den) {
        Ok(v) => {
            f.pairs.push(("sec_fraction", v));
            true
        }
        Err(_) => false,
    }
}

/// `%z`/`%Z` -- the same scan for both, because CRuby accepts a numeric offset
/// for `%Z` and a name for `%z`. `zone` keeps the text AS WRITTEN; `offset` is
/// added only when the text resolves to one.
fn zone(cur: &mut Cursor, f: &mut Fields) -> bool {
    cur.skip_spaces();
    let start = cur.at;
    while let Some(c) = cur.peek() {
        if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | ':') {
            cur.at += 1;
        } else {
            break;
        }
    }
    if cur.at == start {
        return false;
    }
    let text: String = cur.chars[start..cur.at].iter().collect();
    f.str("zone", &text);
    if let Some(off) = numeric_offset(&text).or_else(|| named_offset(&text.to_lowercase())) {
        f.int("offset", off);
    }
    true
}

/// The `(year, month, day)` a `Date.strptime` fragment hash denotes, or `None`
/// where CRuby raises `Date::Error: invalid date`.
///
/// CRuby calls this completing the fragments, and the defaults are not
/// uniform: an absent MONTH or DAY is 1, but an absent YEAR is the CURRENT
/// one, so `Date.strptime("10-31", "%m-%d")` moves with the calendar. A
/// `%j` (day of the year) or a `%s` (epoch seconds) names the date outright
/// and outranks the civil fields.
pub(super) fn civil_from_fragments(
    pairs: &[(RubyValue, RubyValue)],
    this_year: i64,
) -> Option<(i64, i64, i64)> {
    let get = |key: &str| -> Option<i64> {
        pairs.iter().find_map(|(k, v)| match (k, v) {
            (RubyValue::Symbol(s), RubyValue::Int(n)) if &*s.name_str() == key => Some(*n),
            _ => None,
        })
    };
    if let Some(secs) = get("seconds") {
        // Epoch day 0 is 1970-01-01, JDN 2440588. `div_euclid` so a negative
        // epoch floors into the previous day rather than toward zero.
        return Some(super::jdn_to_civil(2_440_588 + secs.div_euclid(86_400)));
    }
    let year = get("year").unwrap_or(this_year);
    if let Some(yday) = get("yday") {
        if !(1..=366).contains(&yday) {
            return None;
        }
        return Some(super::jdn_to_civil(
            super::civil_to_jdn(year, 1, 1) + yday - 1,
        ));
    }
    let mon = get("mon").unwrap_or(1);
    let mday = get("mday").unwrap_or(1);
    // Round-tripping through the JDN is the validity check: a day the calendar
    // does not have comes back as a different date.
    let jdn = super::civil_to_jdn(year, mon, mday);
    if super::jdn_to_civil(jdn) != (year, mon, mday) {
        return None;
    }
    Some((year, mon, mday))
}
