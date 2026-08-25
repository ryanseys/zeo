//! The version family: rubygems' `Gem::Version` ordering and the
//! version/number reductions a comparison guard folds through.

use super::*;
use std::cmp::Ordering;

/// One `Gem::Version` segment. rubygems splits a version string into maximal
/// digit / letter runs (`/[0-9]+|[a-z]+/i`); an all-digit run compares
/// numerically, a letter run (a prerelease tag like `dev`/`rc`) as a string,
/// and a string segment always orders BEFORE a numeric one at the same index.
#[derive(PartialEq, Eq)]
enum Seg {
    Num(u64),
    Str(String),
}

/// Split a version string into rubygems' canonical segments: maximal digit /
/// letter runs, then trailing zero segments trimmed off each of the numeric
/// prefix and the string-and-after tail (rubygems' `canonical_segments`, so
/// `"1.0.0"` and `"1"` compare equal). Non-alphanumeric bytes (`.`, `-`) are
/// separators.
fn canonical_segments(v: &str) -> Vec<Seg> {
    let bytes = v.as_bytes();
    let mut raw: Vec<Seg> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // A version segment wider than u64 never appears in practice; a
            // parse failure degrades the whole guard to undecidable rather
            // than mis-ordering, so `.ok()`-then-bail via a sentinel-free path.
            match v[start..i].parse::<u64>() {
                Ok(n) => raw.push(Seg::Num(n)),
                Err(_) => raw.push(Seg::Str(v[start..i].to_string())),
            }
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            raw.push(Seg::Str(v[start..i].to_string()));
        } else {
            i += 1;
        }
    }
    // Split at the first string segment: numeric prefix vs. the rest.
    let split = raw
        .iter()
        .position(|s| matches!(s, Seg::Str(_)))
        .unwrap_or(raw.len());
    let tail = raw.split_off(split);
    let mut out = raw;
    trim_trailing_zeros(&mut out);
    let mut tail = tail;
    trim_trailing_zeros(&mut tail);
    out.extend(tail);
    out
}

fn trim_trailing_zeros(segs: &mut Vec<Seg>) {
    while matches!(segs.last(), Some(Seg::Num(0))) {
        segs.pop();
    }
}

/// rubygems' `Gem::Version#<=>`: pad the shorter with `0`, compare segment by
/// segment; a string segment orders before a numeric one at the same index.
fn cmp_versions(a: &str, b: &str) -> Ordering {
    let a = canonical_segments(a);
    let b = canonical_segments(b);
    let zero = Seg::Num(0);
    for i in 0..a.len().max(b.len()) {
        let l = a.get(i).unwrap_or(&zero);
        let r = b.get(i).unwrap_or(&zero);
        let ord = match (l, r) {
            (Seg::Num(x), Seg::Num(y)) => x.cmp(y),
            (Seg::Str(x), Seg::Str(y)) => x.cmp(y),
            (Seg::Str(_), Seg::Num(_)) => Ordering::Less,
            (Seg::Num(_), Seg::Str(_)) => Ordering::Greater,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

/// rubygems' `Gem::Version.correct?` -- whether `<=>` will COERCE this string
/// rather than raise `ArgumentError`. Its `ANCHORED_VERSION_PATTERN` is a
/// numeric first segment, then `.`-joined alphanumeric segments, then an
/// optional `-` prerelease of `.`-joined `[0-9A-Za-z-]` runs; surrounding
/// whitespace is allowed, and an empty string IS correct (it means version 0).
/// Rejecting something rubygems would have accepted only leaves a guard
/// undecided, so this errs strict.
fn correct_version(v: &str) -> bool {
    let v = v.trim_matches(|c: char| c.is_ascii_whitespace());
    if v.is_empty() {
        return true;
    }
    // The numeric core can hold no `-`, so the first one opens the prerelease.
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut segs = core.split('.');
    let Some(first) = segs.next() else {
        return false;
    };
    if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !segs.all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())) {
        return false;
    }
    match pre {
        None => true,
        Some(pre) => pre
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')),
    }
}

fn apply(op: &str, ord: Ordering) -> Option<bool> {
    Some(match op {
        "<" => ord == Ordering::Less,
        "<=" => ord != Ordering::Greater,
        ">" => ord == Ordering::Greater,
        ">=" => ord != Ordering::Less,
        "==" => ord == Ordering::Equal,
        "!=" => ord != Ordering::Equal,
        _ => return None,
    })
}

/// Reduce a node to a compile-time `Gem::Version` string (for version-segment
/// comparison, not lexicographic): `Gem::Version.new("x")` /
/// `Gem::Version.create("x")`, or `Gem.rubygems_version`.
fn static_gem_version(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
    depth: u32,
) -> Option<String> {
    match &compiler.hir[node] {
        HirNode::New {
            class_name, args, ..
        } if is_gem_version(compiler, cref, box_id, class_name) => match args.as_slice() {
            [a] => static_string(compiler, cref, box_id, *a, depth),
            _ => None,
        },
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } => match name.as_str() {
            "new" | "create" => {
                let HirNode::ClassRef(rn) = &compiler.hir[*r] else {
                    return None;
                };
                if !is_gem_version(compiler, cref, box_id, rn) {
                    return None;
                }
                match args.as_slice() {
                    [ArrayElem::Single(a)] => static_string(compiler, cref, box_id, *a, depth),
                    _ => None,
                }
            }
            // `Gem.rubygems_version` IS a `Gem::Version`, built from `Gem::VERSION`.
            "rubygems_version" if args.is_empty() => match &compiler.hir[*r] {
                HirNode::ClassRef(rn) if rn == "Gem" => {
                    const_string(compiler, cref, box_id, "Gem::VERSION")
                }
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// A comparison guard whose operands both reduce to build-time constants:
/// `Gem::Version` (version-segment) semantics when EITHER side is one, then
/// plain `String` (lexicographic) semantics, then `Integer`.
///
/// A `Gem::Version` compared against a plain string is version semantics, not
/// lexicographic: rubygems' `<=>` runs `Gem::Version.create` over a `String`
/// operand it can parse (and raises `ArgumentError` over one it can't, which
/// stays undecided here). `Gem::Version.new(RUBY_VERSION) <= "3.4"` is the
/// spelling gems reach for most often -- unparser gates its two `Builder`
/// definitions on it.
///
/// The three readings cannot collide: a node reduces to at most one of them,
/// because each starts from a literal of its own kind.
pub(super) fn cmp_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    op: &str,
    l: NodeId,
    r: NodeId,
    depth: u32,
) -> Option<bool> {
    let lv = static_gem_version(compiler, cref, box_id, l, depth);
    let rv = static_gem_version(compiler, cref, box_id, r, depth);
    match (&lv, &rv) {
        (Some(lv), Some(rv)) => return apply(op, cmp_versions(lv, rv)),
        (Some(lv), None) => {
            if let Some(rs) = static_string(compiler, cref, box_id, r, depth)
                && correct_version(&rs)
            {
                return apply(op, cmp_versions(lv, &rs));
            }
        }
        // The other way round. `String#<=>` hands an operand it can't compare
        // back to that operand's own `<=>` and negates the answer, so the
        // ORDERING operators still read as version comparisons. `==`/`!=` do
        // not: those are `String#==`, which is plain false against anything
        // that isn't a string, so they are left undecided rather than folded to
        // the answer the other direction would give.
        (None, Some(rv)) if matches!(op, "<" | "<=" | ">" | ">=") => {
            if let Some(ls) = static_string(compiler, cref, box_id, l, depth)
                && correct_version(&ls)
            {
                return apply(op, cmp_versions(&ls, rv));
            }
        }
        _ => {}
    }
    if let (Some(ls), Some(rs)) = (
        static_string(compiler, cref, box_id, l, depth),
        static_string(compiler, cref, box_id, r, depth),
    ) {
        return apply(op, ls.as_bytes().cmp(rs.as_bytes()));
    }
    if let (Some(li), Some(ri)) = (
        static_integer(compiler, cref, box_id, l, depth),
        static_integer(compiler, cref, box_id, r, depth),
    ) {
        return apply(op, li.cmp(&ri));
    }
    // The mixed-kind numeric compare ruby does across Integer/Float --
    // `RUBY_VERSION.to_f >= 2.4` (rampi), `RUBY_VERSION.to_i < 3.0`
    // (sanity-ruby). Every reduction is a finite value, so the partial
    // order is total here.
    if let (Some(lf), Some(rf)) = (
        static_number(compiler, cref, box_id, l, depth),
        static_number(compiler, cref, box_id, r, depth),
    ) {
        return apply(op, lf.partial_cmp(&rf)?);
    }
    None
}

/// Reduce a node to a compile-time NUMBER, for the mixed-kind comparisons the
/// integer reading above cannot take. Anything that reading answers rides
/// through as the same value.
fn static_number(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
    depth: u32,
) -> Option<f64> {
    if depth >= MAX_FOLD_DEPTH {
        return None;
    }
    match &compiler.hir[node] {
        HirNode::FloatLit(f) => Some(*f),
        // `"4.0.6".to_f` is 4.0: ruby reads the leading float and stops.
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "to_f" && args.is_empty() => {
            leading_float(&static_string(compiler, cref, box_id, *r, depth + 1)?)
        }
        _ => static_integer(compiler, cref, box_id, node, depth).map(|i| i as f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering_matches_rubygems() {
        assert_eq!(cmp_versions("4.1.0.dev", "3.5.22"), Ordering::Greater);
        assert_eq!(cmp_versions("3.5.22", "3.5.22"), Ordering::Equal);
        assert_eq!(cmp_versions("1.0.0", "1"), Ordering::Equal);
        // Numeric segments compare as numbers, not strings: 3.10 > 3.9.
        assert_eq!(cmp_versions("3.10", "3.9"), Ordering::Greater);
        // A prerelease (string segment) orders before the release.
        assert_eq!(cmp_versions("1.0.a", "1.0"), Ordering::Less);
        assert_eq!(cmp_versions("1.0", "1.0.a"), Ordering::Greater);
    }

    /// Every case checked against `Gem::Version.correct?` under ruby 4.0.6.
    #[test]
    fn correct_version_matches_rubygems() {
        for ok in [
            "3.4",
            "1",
            "4.0.6",
            "1.0.0.rc1",
            "1.0-beta.2",
            "10.20.30",
            "",
        ] {
            assert!(correct_version(ok), "{ok:?} is a version rubygems accepts");
        }
        // Leading whitespace is allowed; a leading `v`, an empty segment or a
        // non-alphanumeric one is not -- rubygems raises `ArgumentError` on
        // these, so a guard using one must stay undecided rather than fold.
        assert!(correct_version("  3.4  "));
        for bad in ["v3.4", "3..4", "3.4.", ".4", "3.4+build", "three"] {
            assert!(!correct_version(bad), "{bad:?} is one rubygems rejects");
        }
    }

    #[test]
    fn apply_covers_each_operator() {
        assert_eq!(apply("<", Ordering::Greater), Some(false));
        assert_eq!(apply(">=", Ordering::Greater), Some(true));
        assert_eq!(apply("==", Ordering::Equal), Some(true));
        assert_eq!(apply("!=", Ordering::Equal), Some(false));
    }
}
