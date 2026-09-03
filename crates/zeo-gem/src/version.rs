//! `Gem::Version` and `Gem::Requirement`, with RubyGems' own ordering.
//!
//! The rules are not semver's, and the differences bite: a version is scanned
//! into runs of digits and runs of letters (`1.0.0-rc1` is
//! `[1, 0, 0, "rc", 1]`), trailing zeros are dropped before comparison
//! (`1.0` equals `1.0.0`), and a letter run sorts BELOW a digit run, which is
//! what makes `1.0.0.rc1` older than `1.0.0`. `version.rb` is the reference.

use std::cmp::Ordering;
use std::fmt;

use crate::error::{Error, Result};

/// One run of a version string: digits, or letters.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Num(u64),
    Text(String),
}

impl Ord for Segment {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Segment::Num(a), Segment::Num(b)) => a.cmp(b),
            (Segment::Text(a), Segment::Text(b)) => a.cmp(b),
            // A letter run is older than any number: `1.0.a` precedes `1.0.0`.
            (Segment::Text(_), Segment::Num(_)) => Ordering::Less,
            (Segment::Num(_), Segment::Text(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for Segment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Segment {
    fn is_zero(&self) -> bool {
        *self == Segment::Num(0)
    }
}

/// A RubyGems version. Holds the text it was written with, so a lockfile
/// round-trips byte-exact, and the segments every comparison uses.
#[derive(Debug, Clone)]
pub struct Version {
    text: String,
    segments: Vec<Segment>,
    canonical: Vec<Segment>,
}

impl Version {
    /// Parse a version string. Leading and trailing space is dropped, exactly
    /// as `Gem::Version.new` does; an empty string is version `0`.
    pub fn parse(text: &str) -> Result<Version> {
        let text = text.trim();
        if !is_correct(text) {
            return Err(Error::format(format!("malformed version: {text:?}")));
        }
        let text = if text.is_empty() { "0" } else { text };
        let segments = scan(text);
        Ok(Version {
            text: text.to_string(),
            canonical: canonical(&segments),
            segments,
        })
    }

    /// The version as written.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether any letter appears -- RubyGems' whole definition of a
    /// pre-release (`1.0.0.rc1`, `2.0.0-beta`).
    pub fn is_prerelease(&self) -> bool {
        self.text.bytes().any(|b| b.is_ascii_alphabetic())
    }

    /// The version with every pre-release part removed: `1.0.0.rc1` -> `1.0.0`.
    pub fn release(&self) -> Version {
        if !self.is_prerelease() {
            return self.clone();
        }
        Version::from_segments(&trim_letters(&self.segments))
    }

    /// The next version at the level `~>` allows: `1.4.2` -> `1.5`, `1.4` ->
    /// `2`. This is what makes `~> 1.4.2` mean `>= 1.4.2, < 1.5`.
    pub fn bump(&self) -> Version {
        let mut segments = trim_letters(&self.segments);
        if segments.len() > 1 {
            segments.pop();
        }
        match segments.last_mut() {
            Some(Segment::Num(n)) => *n += 1,
            _ => segments = vec![Segment::Num(1)],
        }
        Version::from_segments(&segments)
    }

    fn from_segments(segments: &[Segment]) -> Version {
        let text = segments
            .iter()
            .map(|s| match s {
                Segment::Num(n) => n.to_string(),
                Segment::Text(t) => t.clone(),
            })
            .collect::<Vec<_>>()
            .join(".");
        let text = if text.is_empty() { "0".into() } else { text };
        let segments = scan(&text);
        Version {
            canonical: canonical(&segments),
            segments,
            text,
        }
    }
}

/// `Gem::Version::ANCHORED_VERSION_PATTERN`, spelled out: digit-led, dotted
/// alphanumeric runs, with an optional `-`-joined tail.
fn is_correct(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let (head, tail) = match text.split_once('-') {
        Some((head, tail)) => (head, Some(tail)),
        None => (text, None),
    };
    let mut parts = head.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !parts.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric())) {
        return false;
    }
    match tail {
        None => true,
        Some(tail) => tail
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')),
    }
}

/// Runs of digits and runs of letters, in order. Every separator (`.`, `-`)
/// is dropped, which is why `1.0-rc1` and `1.0.rc.1` compare equal.
fn scan(text: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i].is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // A version segment past u64 is not a version anyone ships; treat
            // it as text so the comparison stays total instead of panicking.
            match text[start..i].parse::<u64>() {
                Ok(n) => out.push(Segment::Num(n)),
                Err(_) => out.push(Segment::Text(text[start..i].to_string())),
            }
        } else if bytes[i].is_ascii_alphabetic() {
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push(Segment::Text(text[start..i].to_string()));
        } else {
            i += 1;
        }
    }
    out
}

/// Trailing zeros carry no meaning, so `1.0.0` and `1` are the same version.
/// The numeric head and the letter tail are trimmed SEPARATELY, which keeps
/// `1.0.0.a` from collapsing into `1.a`.
fn canonical(segments: &[Segment]) -> Vec<Segment> {
    let split = segments
        .iter()
        .position(|s| matches!(s, Segment::Text(_)))
        .unwrap_or(segments.len());
    let (nums, rest) = segments.split_at(split);
    let mut out = drop_trailing_zeros(nums);
    out.extend(drop_trailing_zeros(rest));
    out
}

fn drop_trailing_zeros(segments: &[Segment]) -> Vec<Segment> {
    let end = segments
        .iter()
        .rposition(|s| !s.is_zero())
        .map_or(0, |i| i + 1);
    segments[..end].to_vec()
}

/// Everything up to the first letter run -- the release part of a version.
fn trim_letters(segments: &[Segment]) -> Vec<Segment> {
    let end = segments
        .iter()
        .position(|s| matches!(s, Segment::Text(_)))
        .unwrap_or(segments.len());
    segments[..end].to_vec()
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let limit = self.canonical.len().max(other.canonical.len());
        for i in 0..limit {
            // A missing segment is zero: `1.0` against `1.0.1`.
            let zero = Segment::Num(0);
            let lhs = self.canonical.get(i).unwrap_or(&zero);
            let rhs = other.canonical.get(i).unwrap_or(&zero);
            match lhs.cmp(rhs) {
                Ordering::Equal => {}
                other => return other,
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for Version {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}

impl std::hash::Hash for Segment {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Segment::Num(n) => (0u8, n).hash(state),
            Segment::Text(t) => (1u8, t).hash(state),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::str::FromStr for Version {
    type Err = Error;

    fn from_str(s: &str) -> Result<Version> {
        Version::parse(s)
    }
}

/// One comparison in a requirement, such as `>= 2.0.2` or `~> 7.1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Lt,
    Gte,
    Lte,
    /// "pessimistic": at least this version, below the next one up.
    Twiddle,
}

impl Op {
    fn parse(text: &str) -> Option<Op> {
        Some(match text {
            "=" => Op::Eq,
            "!=" => Op::Ne,
            ">" => Op::Gt,
            "<" => Op::Lt,
            ">=" => Op::Gte,
            "<=" => Op::Lte,
            "~>" => Op::Twiddle,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Gt => ">",
            Op::Lt => "<",
            Op::Gte => ">=",
            Op::Lte => "<=",
            Op::Twiddle => "~>",
        }
    }
}

/// A `Gem::Requirement`: every clause must hold. An empty requirement is
/// `>= 0`, RubyGems' own default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Requirement {
    pub clauses: Vec<(Op, Version)>,
}

impl Requirement {
    /// Parse `">= 2.0.2, < 8.0"`, or one clause on its own. A bare version is
    /// `=` that version, as in a gemspec's `["1.2.3"]`.
    pub fn parse(text: &str) -> Result<Requirement> {
        let mut clauses = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            clauses.push(parse_clause(part)?);
        }
        Ok(Requirement { clauses })
    }

    /// Whether `version` meets every clause.
    pub fn satisfied_by(&self, version: &Version) -> bool {
        if self.clauses.is_empty() {
            return true;
        }
        self.clauses.iter().all(|(op, bound)| match op {
            Op::Eq => version == bound,
            Op::Ne => version != bound,
            Op::Gt => version > bound,
            Op::Lt => version < bound,
            Op::Gte => version >= bound,
            Op::Lte => version <= bound,
            // `~> 1.4.2` is `>= 1.4.2` and below `1.5`. The upper bound tests
            // the RELEASE, so `1.5.0.rc1` is out even though it sorts lower.
            Op::Twiddle => version >= bound && version.release() < bound.bump(),
        })
    }

    /// Whether this is RubyGems' default, `>= 0` -- written as `()` nowhere
    /// and omitted from a lockfile's dependency line.
    pub fn is_default(&self) -> bool {
        self.clauses.is_empty()
            || (self.clauses.len() == 1
                && self.clauses[0].0 == Op::Gte
                && self.clauses[0].1.as_str() == "0")
    }
}

fn parse_clause(part: &str) -> Result<(Op, Version)> {
    let split = part
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(part.len());
    let (op, version) = part.split_at(split);
    let op = op.trim();
    let op = if op.is_empty() {
        Op::Eq
    } else {
        Op::parse(op).ok_or_else(|| Error::format(format!("unknown version operator: {op:?}")))?
    };
    Ok((op, Version::parse(version)?))
}

impl fmt::Display for Requirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text: Vec<String> = self
            .clauses
            .iter()
            .map(|(op, v)| format!("{} {v}", op.as_str()))
            .collect();
        f.write_str(&text.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn trailing_zeros_do_not_make_a_different_version() {
        assert_eq!(v("1.0"), v("1.0.0"));
        assert_eq!(v("1"), v("1.0.0.0"));
        assert_ne!(v("1.0"), v("1.0.1"));
    }

    #[test]
    fn a_prerelease_sorts_below_its_own_release() {
        assert!(v("1.0.0.rc1") < v("1.0.0"));
        assert!(v("1.0.0-rc1") < v("1.0.0"));
        assert!(v("1.0.0.alpha") < v("1.0.0.beta"));
        assert!(v("1.0.0.rc1") < v("1.0.0.rc2"));
        assert!(v("1.0.0").is_prerelease().eq(&false));
        assert!(v("1.0.0.rc1").is_prerelease());
    }

    #[test]
    fn numbers_compare_as_numbers_not_as_text() {
        assert!(v("1.10") > v("1.9"));
        assert!(v("1.0.10") > v("1.0.9"));
    }

    #[test]
    fn bump_drops_the_last_segment_and_the_prerelease_tail() {
        assert_eq!(v("1.4.2").bump().as_str(), "1.5");
        assert_eq!(v("1.4").bump().as_str(), "2");
        assert_eq!(v("5").bump().as_str(), "6");
        assert_eq!(v("1.4.2.rc1").bump().as_str(), "1.5");
        assert_eq!(v("1.0.0.rc1").release().as_str(), "1.0.0");
    }

    #[test]
    fn a_twiddle_requirement_bounds_the_last_named_segment() {
        let r = Requirement::parse("~> 1.4.2").unwrap();
        assert!(r.satisfied_by(&v("1.4.2")));
        assert!(r.satisfied_by(&v("1.4.99")));
        assert!(!r.satisfied_by(&v("1.5.0")));
        assert!(!r.satisfied_by(&v("1.4.1")));

        let r = Requirement::parse("~> 7.1").unwrap();
        assert!(r.satisfied_by(&v("7.9")));
        assert!(!r.satisfied_by(&v("8.0")));
    }

    #[test]
    fn several_clauses_must_all_hold() {
        let r = Requirement::parse(">= 2.0.2, < 8.0").unwrap();
        assert!(r.satisfied_by(&v("2.0.2")));
        assert!(r.satisfied_by(&v("7.9")));
        assert!(!r.satisfied_by(&v("8.0")));
        assert!(!r.satisfied_by(&v("2.0.1")));
        assert_eq!(r.to_string(), ">= 2.0.2, < 8.0");
    }

    #[test]
    fn a_bare_version_means_equal_and_an_empty_requirement_accepts_anything() {
        assert!(
            Requirement::parse("1.2.3")
                .unwrap()
                .satisfied_by(&v("1.2.3"))
        );
        assert!(Requirement::parse("").unwrap().satisfied_by(&v("9")));
        assert!(Requirement::parse(">= 0").unwrap().is_default());
    }

    #[test]
    fn a_version_that_is_not_one_is_a_loud_error() {
        assert!(Version::parse("not.a.version").is_err());
        assert!(Version::parse("1.2.3").is_ok());
        assert!(Version::parse("1.2.3-arm64-darwin").is_ok());
        assert_eq!(v("").as_str(), "0");
    }
}
