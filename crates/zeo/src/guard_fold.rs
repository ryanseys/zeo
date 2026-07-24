//! Compile-time folding of the target-constant guards that gate compat/version
//! shims throughout rubygems, bundler, and the stdlib -- the home for every
//! predicate that is fixed for a whole-program AOT target yet written as a
//! runtime `if`/`unless` wrapping a whole definition.
//!
//! Today that's **version gates**: `if Gem.rubygems_version <
//! Gem::Version.new("3.5.22")`, `if RUBY_VERSION < "3.0"`, `return unless
//! version <= Gem::Version.create("2.6.9")`. `RUBY_VERSION`,
//! `RUBY_ENGINE_VERSION`, and `Gem::VERSION` are build-time constants (the same
//! strings the runtime seeds), so a comparison whose operands all reduce to
//! those constants is decidable at compile time -- exactly CRuby's own
//! reachability, and the only tractable answer under zeo's static MRO (a
//! runtime-conditional `prepend`/`include` can't be expressed against a fixed
//! ancestry).
//!
//! [`static_cmp`] is the one entry point, shared by both branch-folders:
//! `analyze`'s `static_top_cond` (which decides what a top-level `if` REGISTERS)
//! and `codegen::constfold::static_cond` (which decides what an `if` EMITS), so
//! registration and emission always pick the same branch.

use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
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

/// The literal string of `scope::name` when it's a `NAME = "..."` written in
/// `scope`'s own body (e.g. `Gem::VERSION`) -- read straight from the
/// registered class-body statement, so it's known even before value constants
/// are fully resolved.
fn const_string(compiler: &Compiler, box_id: u32, scope: &str, name: &str) -> Option<String> {
    let sid = compiler.resolve_class(scope, &[], box_id)?;
    for &s in &compiler.class(sid).class_body_stmts {
        if let HirNode::ConstWrite { name: n, value, .. } = &compiler.hir[s] {
            if n == name {
                if let Some(v) = string_lit(compiler, *value) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// A bare single-part string literal's value (`"3.5.22"`), or `None` for an
/// interpolated / multi-part string that isn't a compile-time constant.
fn string_lit(compiler: &Compiler, node: NodeId) -> Option<String> {
    let HirNode::StringLit(parts) = &compiler.hir[node] else {
        return None;
    };
    match parts.as_slice() {
        [StrPart::Lit(s)] => Some(s.clone()),
        [] => Some(String::new()),
        _ => None,
    }
}

/// Whether `name` (as written at a call site) resolves to the `Gem::Version`
/// class -- so `Gem::Version.new`, or a bare `Version` whose cref reaches it.
fn is_gem_version(compiler: &Compiler, box_id: u32, name: &str) -> bool {
    match (
        compiler.resolve_class(name, &[], box_id),
        compiler.resolve_class("Gem::Version", &[], box_id),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Reduce a node to a compile-time PLAIN STRING (a `String` object's value),
/// for a lexicographic `String#<=>` comparison: a string literal,
/// `RUBY_VERSION`/`RUBY_ENGINE_VERSION`, a `Scope::NAME = "..."` constant, or
/// `Gem.rubygems_version`'s underlying `Gem::VERSION` string.
fn static_string(compiler: &Compiler, box_id: u32, node: NodeId) -> Option<String> {
    match &compiler.hir[node] {
        HirNode::StringLit(_) => string_lit(compiler, node),
        HirNode::ClassRef(name) => match name.as_str() {
            "RUBY_VERSION" | "RUBY_ENGINE_VERSION" => Some(zeo_abi::RUBY_VERSION.to_string()),
            _ => name
                .rsplit_once("::")
                .and_then(|(scope, base)| const_string(compiler, box_id, scope, base)),
        },
        HirNode::QualifiedConstRead(scope, name) => const_string(compiler, box_id, scope, name),
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "rubygems_version" && args.is_empty() => match &compiler.hir[*r] {
            HirNode::ClassRef(rn) if rn == "Gem" => const_string(compiler, box_id, "Gem", "VERSION"),
            _ => None,
        },
        _ => None,
    }
}

/// Reduce a node to a compile-time `Gem::Version` string (for version-segment
/// comparison, not lexicographic): `Gem::Version.new("x")` /
/// `Gem::Version.create("x")`, or `Gem.rubygems_version`.
fn static_gem_version(compiler: &Compiler, box_id: u32, node: NodeId) -> Option<String> {
    match &compiler.hir[node] {
        HirNode::New {
            class_name, args, ..
        } if is_gem_version(compiler, box_id, class_name) => match args.as_slice() {
            [a] => static_string(compiler, box_id, *a),
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
                if !is_gem_version(compiler, box_id, rn) {
                    return None;
                }
                match args.as_slice() {
                    [ArrayElem::Single(a)] => static_string(compiler, box_id, *a),
                    _ => None,
                }
            }
            // `Gem.rubygems_version` IS a `Gem::Version`, built from `Gem::VERSION`.
            "rubygems_version" if args.is_empty() => match &compiler.hir[*r] {
                HirNode::ClassRef(rn) if rn == "Gem" => {
                    const_string(compiler, box_id, "Gem", "VERSION")
                }
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// Compile-time truth of a comparison guard whose operands all reduce to
/// build-time version constants; `None` when it isn't such a guard (leave the
/// condition to run normally). Tries `Gem::Version` (version-segment) semantics
/// first, then plain `String` (lexicographic) semantics -- a mixed
/// Version/String comparison, which Ruby itself raises on, stays `None`.
pub(crate) fn static_cmp(compiler: &Compiler, box_id: u32, cond: NodeId) -> Option<bool> {
    let HirNode::Call {
        receiver: Some(l),
        name,
        args,
        kwargs,
        block,
        ..
    } = &compiler.hir[cond]
    else {
        return None;
    };
    if !kwargs.is_empty() || block.is_some() {
        return None;
    }
    let op = name.as_str();
    if !matches!(op, "<" | "<=" | ">" | ">=" | "==" | "!=") {
        return None;
    }
    let [ArrayElem::Single(r)] = args.as_slice() else {
        return None;
    };
    let (l, r) = (*l, *r);
    if let (Some(lv), Some(rv)) = (
        static_gem_version(compiler, box_id, l),
        static_gem_version(compiler, box_id, r),
    ) {
        return apply(op, cmp_versions(&lv, &rv));
    }
    if let (Some(ls), Some(rs)) = (
        static_string(compiler, box_id, l),
        static_string(compiler, box_id, r),
    ) {
        return apply(op, ls.as_bytes().cmp(rs.as_bytes()));
    }
    None
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

    #[test]
    fn apply_covers_each_operator() {
        assert_eq!(apply("<", Ordering::Greater), Some(false));
        assert_eq!(apply(">=", Ordering::Greater), Some(true));
        assert_eq!(apply("==", Ordering::Equal), Some(true));
        assert_eq!(apply("!=", Ordering::Equal), Some(false));
    }
}
