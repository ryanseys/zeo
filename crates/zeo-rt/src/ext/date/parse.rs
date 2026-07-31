//! `Date._parse` -- the heuristic scanner behind `Date.parse`, `DateTime.parse`
//! and, through the vendored `time` gem, `Time.parse`.
//!
//! CRuby spells this as a cascade in `date_parse.c`: a weekday name, then a
//! clock time with an optional zone, then the FIRST date shape that matches out
//! of an ordered list. This is the same cascade over the shapes the corpus and
//! the `time` gem reach -- ISO 8601, RFC 2822/3339, HTTP-date, the month-name
//! forms, and bare digit runs -- each differentially checked against ruby
//! 4.0.6. A string nothing recognises answers an EMPTY hash rather than
//! raising, which is what lets `Time.parse` report its own error.
//!
//! Every parser consumes what it matched, so the ones after it see only the
//! residue. That is why the order is the whole design: `10:30` has to be a time
//! before it can be read as a date, and `15 Jan 2024` has to reach the
//! month-name parser before the bare-digits one claims `15`.

use crate::builtins::rational::rational_new;
use crate::{RubyValue, string_new};
use num_bigint::BigInt;
use regex::Regex;
use std::sync::LazyLock;

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// The extracted fields, in the order CRuby's own parsers fill them. A `Vec`
/// rather than a map because the hash `Date._parse` answers is printed as-is,
/// and its key order IS the order the cascade ran in.
#[derive(Default)]
struct Fields {
    pairs: Vec<(&'static str, RubyValue)>,
    zone: Option<String>,
}

impl Fields {
    fn int(&mut self, key: &'static str, value: i64) {
        self.pairs.push((key, RubyValue::Int(value)));
    }

    fn year(&mut self, digits: &str, comp: bool) {
        if let Some(v) = widen_year(digits, comp) {
            self.int("year", v);
        }
    }

    fn set_zone(&mut self, text: &str) {
        self.zone = Some(text.to_string());
        self.pairs
            .push(("zone", RubyValue::Str(string_new(text.to_string()))));
    }

    fn into_pairs(self) -> Vec<(RubyValue, RubyValue)> {
        self.pairs
            .into_iter()
            .map(|(k, v)| (RubyValue::Symbol(crate::Symbol::intern(k)), v))
            .collect()
    }
}

/// A year as written, with a two-digit one widened when the caller asked for a
/// COMPLETE date: 0..=68 is this century, 69..=99 the last. `Date._parse`'s
/// second argument turns this off, which is how `Time.parse`'s block form gets
/// to decide for itself.
fn widen_year(digits: &str, comp: bool) -> Option<i64> {
    let value = digits.parse::<i64>().ok()?;
    Some(match comp && digits.len() <= 2 && value >= 0 {
        true if value <= 68 => value + 2000,
        true => value + 1900,
        false => value,
    })
}

/// The fields of `text`, as `Date._parse`'s hash pairs.
pub(super) fn date_parse(text: &str, comp: bool) -> Vec<(RubyValue, RubyValue)> {
    let mut rest = strip_comments(text);
    let mut f = Fields::default();
    parse_day(&mut rest, &mut f);
    parse_time(&mut rest, &mut f);
    let _ = parse_named_month(&mut rest, &mut f, comp)
        || parse_iso_week(&mut rest, &mut f, comp)
        || parse_iso(&mut rest, &mut f, comp)
        || parse_iso_ordinal(&mut rest, &mut f, comp)
        || parse_slash(&mut rest, &mut f, comp)
        || parse_dot(&mut rest, &mut f, comp)
        || parse_digits(&mut rest, &mut f, comp)
        || parse_ordinal_day(&mut rest, &mut f);
    if let Some(offset) = f.zone.as_deref().and_then(zone_offset) {
        f.int("offset", offset);
    }
    f.into_pairs()
}

/// One integer field out of [`date_parse`]'s pairs, for a caller building a
/// calendar date rather than answering the hash.
pub(super) fn field(pairs: &[(RubyValue, RubyValue)], key: &str) -> Option<i64> {
    let wanted = crate::Symbol::intern(key);
    pairs.iter().find_map(|(k, v)| match (k, v) {
        (RubyValue::Symbol(s), RubyValue::Int(n)) if *s == wanted => Some(*n),
        _ => None,
    })
}

/// The seconds `zone` is ahead of UT, or `None` for one nothing recognises --
/// `Date.zone_to_diff`, and the same answer `Date._parse` puts under `:offset`.
pub(super) fn zone_offset(zone: &str) -> Option<i64> {
    let lower = zone.trim().to_ascii_lowercase();
    // `gmt-3`/`utc+05:30` -- a Universal-Time name carrying its own adjustment.
    let numeric = ["gmt", "utc", "ut"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    if numeric.starts_with(['+', '-']) {
        return numeric_offset(numeric);
    }
    named_offset(&lower)
}

/// `+HH`, `+HHMM`, `+HH:MM`, `+HH:MM:SS` -- and the single-digit hour `-3`,
/// which is what a `gmt-3` leaves behind.
fn numeric_offset(text: &str) -> Option<i64> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let digits = &text[1..];
    let parts: Vec<&str> = digits.split([':', ',', '.']).collect();
    let (h, m, s): (i64, i64, i64) = if parts.len() > 1 {
        (
            parts[0].parse().ok()?,
            parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0),
            parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0),
        )
    } else {
        // Unseparated: the rightmost two digits are minutes, the pair before
        // them seconds, whatever is left the hours.
        let d = parts[0];
        if !d.chars().all(|c| c.is_ascii_digit()) || d.is_empty() {
            return None;
        }
        match d.len() {
            1 | 2 => (d.parse().ok()?, 0, 0),
            3 | 4 => (
                d[..d.len() - 2].parse().ok()?,
                d[d.len() - 2..].parse().ok()?,
                0,
            ),
            5 | 6 => (
                d[..d.len() - 4].parse().ok()?,
                d[d.len() - 4..d.len() - 2].parse().ok()?,
                d[d.len() - 2..].parse().ok()?,
            ),
            _ => return None,
        }
    };
    Some(sign * ((h * 60 + m) * 60 + s))
}

/// The abbreviation table, in minutes. CRuby carries every zone name it has
/// ever been asked about; this is the RFC 822 set (which `time.rb` re-derives
/// for itself anyway), the military letters, and the abbreviations a real
/// timestamp is likely to arrive with.
fn named_offset(zone: &str) -> Option<i64> {
    const NAMED: &[(&str, i64)] = &[
        ("ut", 0),
        ("utc", 0),
        ("gmt", 0),
        ("z", 0),
        ("est", -300),
        ("edt", -240),
        ("cst", -360),
        ("cdt", -300),
        ("mst", -420),
        ("mdt", -360),
        ("pst", -480),
        ("pdt", -420),
        ("akst", -540),
        ("akdt", -480),
        ("hst", -600),
        ("hdt", -540),
        ("ast", -240),
        ("adt", -180),
        ("nst", -210),
        ("ndt", -150),
        ("bst", 60),
        ("wet", 0),
        ("west", 60),
        ("cet", 60),
        ("cest", 120),
        ("eet", 120),
        ("eest", 180),
        ("msk", 180),
        ("wat", 60),
        ("cat", 120),
        ("sast", 120),
        ("eat", 180),
        ("gst", 240),
        ("pkt", 300),
        ("ist", 330),
        ("npt", 345),
        ("ict", 420),
        ("wib", 420),
        ("hkt", 480),
        ("sgt", 480),
        ("myt", 480),
        ("pht", 480),
        ("jst", 540),
        ("kst", 540),
        ("chst", 600),
        ("aest", 600),
        ("aedt", 660),
        ("nzst", 720),
        ("nzdt", 780),
        ("eastern standard time", -300),
        ("eastern daylight time", -240),
        ("central standard time", -360),
        ("central daylight time", -300),
        ("mountain standard time", -420),
        ("mountain daylight time", -360),
        ("pacific standard time", -480),
        ("pacific daylight time", -420),
        ("alaska standard time", -540),
        ("alaska daylight time", -480),
        ("hawaii standard time", -600),
    ];
    if let Some((_, minutes)) = NAMED.iter().find(|(name, _)| *name == zone) {
        return Some(minutes * 60);
    }
    // The military single letters: `a`..`i` and `k`..`m` run +1..+12 (`j` is
    // skipped), `n`..`y` run -1..-12, and `z` is UT -- already in the table.
    let [c] = zone.as_bytes() else { return None };
    let hours = match c {
        b'a'..=b'i' => (c - b'a' + 1) as i64,
        b'k'..=b'm' => (c - b'a') as i64,
        b'n'..=b'y' => -((c - b'n' + 1) as i64),
        _ => return None,
    };
    Some(hours * 3600)
}

/// Drops `(...)` commentary, which RFC 2822 allows anywhere a space is legal.
fn strip_comments(text: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^()]*\)").expect("valid"));
    let mut out = text.to_string();
    while let Some(m) = RE.find(&out) {
        out.replace_range(m.range(), " ");
    }
    out
}

/// A weekday name anywhere in the string -- RFC 2822 leads with one, and it
/// carries no information the rest of the date does not, so it is recorded and
/// removed before anything else looks at the digits.
fn parse_day(rest: &mut String, f: &mut Fields) {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(sun|mon|tue|wed|thu|fri|sat)[a-z]*\.?").expect("valid")
    });
    let Some(m) = RE.captures(rest) else { return };
    let name = m[1].to_ascii_lowercase();
    let wday = DAYS.iter().position(|d| *d == name).expect("matched above");
    let span = m.get(0).expect("group 0 always matches").range();
    f.int("wday", wday as i64);
    rest.replace_range(span, " ");
}

/// `hh:mm`, `hh:mm:ss`, a fractional second, an am/pm marker, and a trailing
/// zone. The zone belongs to the TIME rather than the date: it is what follows
/// the clock, and CRuby records it before the hour for that reason.
fn parse_time(rest: &mut String, f: &mut Fields) {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)(?P<h>\d+)\s*:\s*(?P<mi>\d+)",
            r"(?:\s*:\s*(?P<s>\d+)(?:[.,](?P<f>\d+))?)?",
            r"(?:\s*(?P<mer>[ap])\.?\s?m\.?)?",
            r"(?:\s*(?P<zone>",
            r"(?:gmt|utc?)?[-+]\d+(?:[:,.]?\d+(?::\d+)?)?",
            r"|(?:[[:alpha:].]+\s+)*(?:standard|daylight)\s+time\b",
            r"|[[:alpha:]]+(?:\s+dst)?\b",
            r"))?",
        ))
        .expect("valid")
    });
    let Some(m) = RE.captures(rest) else { return };
    let mut hour: i64 = m["h"].parse().unwrap_or(0);
    match m.name("mer").map(|v| v.as_str().to_ascii_lowercase()) {
        Some(mer) if mer == "p" && hour < 12 => hour += 12,
        Some(mer) if mer == "a" && hour == 12 => hour = 0,
        _ => {}
    }
    if let Some(z) = m.name("zone") {
        f.set_zone(z.as_str().trim());
    }
    f.int("hour", hour);
    f.int("min", m["mi"].parse().unwrap_or(0));
    if let Some(s) = m.name("s") {
        f.int("sec", s.as_str().parse().unwrap_or(0));
    }
    if let Some(frac) = m.name("f").map(|v| v.as_str()) {
        // A fraction of a second is exact in CRuby: a Rational over a power of
        // ten, not the Float that reading it back as one would give.
        if let (Ok(num), Ok(den)) = (
            frac.parse::<BigInt>(),
            format!("1{}", "0".repeat(frac.len())).parse::<BigInt>(),
        ) {
            if let Ok(v) = rational_new(num, den) {
                f.pairs.push(("sec_fraction", v));
            }
        }
    }
    let span = m.get(0).expect("group 0 always matches").range();
    rest.replace_range(span, " ");
}

/// A number as one of these parsers sees it: its digits, and whether an
/// apostrophe marked it a year (`'99`).
struct Num {
    digits: String,
    quoted: bool,
}

/// Every number left around a month name. A `-` here is a SEPARATOR, never a
/// sign (`15-Jan-2024`), which is the one place this differs from the parsers
/// that read a bare `-2024` as a year before the common era.
fn numbers(text: &str) -> Vec<Num> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)('?)(\d+)(?:st|nd|rd|th)?").expect("valid"));
    RE.captures_iter(text)
        .map(|c| Num {
            digits: c[2].to_string(),
            quoted: &c[1] == "'",
        })
        .collect()
}

/// `15 Jan 2024`, `Jan 15, 2024`, `Aug 2000`, `Jan '99` -- any arrangement with
/// a month NAME in it. The name fixes the month, and the numbers around it are
/// then unambiguous enough to place by width alone: three digits or more (or an
/// apostrophe) means a year, and of two short numbers the first is the day.
fn parse_named_month(rest: &mut String, f: &mut Fields, comp: bool) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?")
            .expect("valid")
    });
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    let name = m[1].to_ascii_lowercase();
    let mon = MONTHS
        .iter()
        .position(|x| *x == name)
        .expect("matched above") as i64
        + 1;
    let mut residue = rest.clone();
    residue.replace_range(m.get(0).expect("group 0 always matches").range(), " ");

    let (mut year, mut mday) = (None, None);
    for n in numbers(&residue) {
        let long = n.quoted || n.digits.trim_start_matches('-').len() >= 3;
        match () {
            _ if year.is_none() && (long || mday.is_some()) => year = Some(n),
            _ if mday.is_none() && !long => mday = Some(n),
            _ => break,
        }
    }
    if let Some(y) = &year {
        f.year(&y.digits, comp);
    }
    f.int("mon", mon);
    if let Some(d) = &mday {
        if let Ok(v) = d.digits.parse::<i64>() {
            f.int("mday", v);
        }
    }
    rest.clear();
    true
}

/// `2024-W03-1` -- the ISO 8601 week date, which names a different calendar
/// (`cwyear`/`cweek`/`cwday`) rather than a year-month-day.
fn parse_iso_week(rest: &mut String, f: &mut Fields, comp: bool) -> bool {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(-?\d+)-w(\d+)(?:-(\d))?").expect("valid"));
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    let Some(cwyear) = widen_year(&m[1], comp) else {
        return false;
    };
    f.int("cwyear", cwyear);
    f.int("cweek", m[2].parse().unwrap_or(0));
    if let Some(d) = m.get(3) {
        f.int("cwday", d.as_str().parse().unwrap_or(0));
    }
    rest.clear();
    true
}

/// `2024-01-15`, and every two-digit-year variant of it (`01-10-31`).
fn parse_iso(rest: &mut String, f: &mut Fields, comp: bool) -> bool {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(-?\d+)-(\d+)-(\d+)").expect("valid"));
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    f.year(&m[1], comp);
    f.int("mon", m[2].parse().unwrap_or(0));
    f.int("mday", m[3].parse().unwrap_or(0));
    rest.clear();
    true
}

/// `2024-015` -- the ISO 8601 ordinal date, a year and a day OF that year.
fn parse_iso_ordinal(rest: &mut String, f: &mut Fields, comp: bool) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(-?\d+)-(\d{3})\b").expect("valid"));
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    f.year(&m[1], comp);
    f.int("yday", m[2].parse().unwrap_or(0));
    rest.clear();
    true
}

/// `2024/01/15` and `7/23`. With only two components there is no day: a
/// four-digit one is the year and the other the month, otherwise it reads as
/// month and day.
fn parse_slash(rest: &mut String, f: &mut Fields, comp: bool) -> bool {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(-?\d+)\s*/\s*(\d+)(?:\s*/\s*(-?\d+))?").expect("valid"));
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    match m.get(3) {
        Some(third) => {
            f.year(&m[1], comp);
            f.int("mon", m[2].parse().unwrap_or(0));
            f.int("mday", third.as_str().parse().unwrap_or(0));
        }
        None if m[1].len() >= 3 => {
            f.year(&m[1], comp);
            f.int("mon", m[2].parse().unwrap_or(0));
        }
        None if m[2].len() >= 3 => {
            f.year(&m[2], comp);
            f.int("mon", m[1].parse().unwrap_or(0));
        }
        None => {
            f.int("mon", m[1].parse().unwrap_or(0));
            f.int("mday", m[2].parse().unwrap_or(0));
        }
    }
    rest.clear();
    true
}

/// `2024.01.15` and its European mirror `15.01.2024`. Which one is decided by
/// the LAST component: a four-digit tail can only be the year.
fn parse_dot(rest: &mut String, f: &mut Fields, comp: bool) -> bool {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(-?\d+)\.(\d+)\.(-?\d+)").expect("valid"));
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    let (y, mon, d) = if m[3].len() >= 3 {
        (&m[3], &m[2], &m[1])
    } else {
        (&m[1], &m[2], &m[3])
    };
    f.year(y, comp);
    f.int("mon", mon.parse().unwrap_or(0));
    f.int("mday", d.parse().unwrap_or(0));
    rest.clear();
    true
}

/// Reads `digits` as one field per key. A lone key takes the whole run (a
/// three-digit day-of-year), and otherwise each field is a fixed pair -- so a
/// short run simply fills fewer of them, which is how `T1030` gives an hour and
/// a minute but no second.
fn split_fields(digits: &str, keys: &[&'static str], f: &mut Fields) {
    if let [only] = keys {
        f.int(only, digits.parse().unwrap_or(0));
        return;
    }
    for (i, key) in keys.iter().enumerate() {
        let Some(pair) = digits.get(i * 2..i * 2 + 2) else {
            return;
        };
        f.int(key, pair.parse().unwrap_or(0));
    }
}

/// A bare digit run, read by its LENGTH -- `20240115` is a date, `123456` a
/// two-digit-year one, `24` a day of the month. An `T`-separated time and a
/// zone may follow, which is how a compact ISO 8601 stamp arrives.
fn parse_digits(rest: &mut String, f: &mut Fields, comp: bool) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(\d{2,14})(?:t(\d{2,6})(?:[.,](\d+))?)?(?:\s*(z|[-+]\d+(?::?\d+)?))?")
            .expect("valid")
    });
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    let d = &m[1];
    // How many leading digits are the year, and which fields the rest spells.
    // A length with no reading at all (9, 11, 13) leaves the string unparsed,
    // exactly as CRuby leaves it.
    let (ywidth, keys): (usize, &[&'static str]) = match d.len() {
        2 => (0, &["mday"]),
        3 => (0, &["yday"]),
        4 => (0, &["mon", "mday"]),
        5 => (2, &["yday"]),
        6 => (2, &["mon", "mday"]),
        7 => (4, &["yday"]),
        8 => (4, &["mon", "mday"]),
        10 => (2, &["mon", "mday", "hour", "min"]),
        12 => (2, &["mon", "mday", "hour", "min", "sec"]),
        14 => (4, &["mon", "mday", "hour", "min", "sec"]),
        _ => return false,
    };
    if ywidth > 0 {
        f.year(&d[..ywidth], comp);
    }
    split_fields(&d[ywidth..], keys, f);
    if let Some(t) = m.get(2) {
        split_fields(t.as_str(), &["hour", "min", "sec"], f);
    }
    if let Some(z) = m.get(4) {
        f.set_zone(z.as_str().trim());
    }
    rest.clear();
    true
}

/// `3rd` -- a single digit is too short for the digit-run reading above, so an
/// ordinal suffix is the only thing that makes it a day of the month.
fn parse_ordinal_day(rest: &mut String, f: &mut Fields) -> bool {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(\d)(?:st|nd|rd|th)\b").expect("valid"));
    let Some(m) = RE.captures(rest) else {
        return false;
    };
    f.int("mday", m[1].parse().unwrap_or(0));
    rest.clear();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Vec<(String, String)> {
        date_parse(text, true)
            .into_iter()
            .map(|(k, v)| (k.inspect_string(), v.inspect_string()))
            .collect()
    }
    fn keyed(text: &str, key: &str) -> Option<String> {
        parsed(text)
            .into_iter()
            .find(|(k, _)| k == &format!(":{key}"))
            .map(|(_, v)| v)
    }

    #[test]
    fn iso_date_and_time() {
        assert_eq!(keyed("2024-01-15", "year").as_deref(), Some("2024"));
        assert_eq!(keyed("2024-01-15", "mon").as_deref(), Some("1"));
        assert_eq!(keyed("2024-01-15", "mday").as_deref(), Some("15"));
        assert_eq!(keyed("2024-01-15T10:30:00Z", "hour").as_deref(), Some("10"));
        assert_eq!(
            keyed("2024-01-15T10:30:00Z", "offset").as_deref(),
            Some("0")
        );
    }

    #[test]
    fn two_digit_years_widen_only_when_complete() {
        assert_eq!(keyed("69-01-15", "year").as_deref(), Some("1969"));
        assert_eq!(keyed("68-01-15", "year").as_deref(), Some("2068"));
        let raw = date_parse("01-10-31", false);
        assert_eq!(raw[0].1.inspect_string(), "1");
    }

    #[test]
    fn zone_names_and_offsets() {
        assert_eq!(zone_offset("+09:00"), Some(32400));
        assert_eq!(zone_offset("-0530"), Some(-19800));
        assert_eq!(zone_offset("gmt-3"), Some(-10800));
        assert_eq!(zone_offset("JST"), Some(32400));
        assert_eq!(zone_offset("EST5EDT"), None);
    }

    #[test]
    fn unrecognised_text_answers_nothing() {
        assert!(parsed("garbage").is_empty());
        assert!(parsed("").is_empty());
    }
}
