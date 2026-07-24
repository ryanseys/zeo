//! Compile-time folding of the target-constant guards that gate compat/version
//! shims throughout rubygems, bundler, and the stdlib -- the home for every
//! predicate that is fixed for a whole-program AOT target yet written as a
//! runtime `if`/`unless` wrapping a whole definition.
//!
//! Two families, both decidable because a whole-program target has a fixed
//! Ruby/RubyGems version AND fixed compiled method tables:
//!
//! * **Version gates** -- `if Gem.rubygems_version < Gem::Version.new("3.5.22")`,
//!   `if RUBY_VERSION < "3.0"`, `return unless version <= Gem::Version.create(
//!   "2.6.9")`. `RUBY_VERSION`/`RUBY_ENGINE_VERSION`/`Gem::VERSION` are
//!   build-time constants (the same strings the runtime seeds), so a comparison
//!   whose operands reduce to them folds -- version-segment ordering for
//!   `Gem::Version` operands, lexicographic `String#<=>` for the rest.
//! * **Feature probes** -- `unless VALIDATES_FOR_RESOLUTION` (a `respond_to?`
//!   -derived boolean constant), `unless new.respond_to?(:installable_on_platform?)`,
//!   `unless method_defined?(:encode_with)`. Whether a class has a method is
//!   answered by the compiled method tables.
//!
//! Folding these is exactly CRuby's own load-time reachability, and the only
//! tractable answer under zeo's static MRO (a runtime-conditional
//! `prepend`/`include` can't be expressed against a fixed ancestry).
//! [`static_cond`] is the one entry point, shared by both branch-folders:
//! `analyze`'s `static_top_cond` (what a top-level `if` REGISTERS) and
//! `codegen::constfold::static_cond` (what an `if` EMITS), so registration and
//! emission always pick the same branch. Callers pass the lexical `cref` at the
//! guard site (empty at the top level) so a bare `Specification`/const resolves
//! in its enclosing namespace, exactly as it would at that source position.

use crate::compiler::{ClassId, Compiler};
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart, Visibility};
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

/// The `value` node and owning class of `NAME` (`scope::NAME`, or a bare `NAME`
/// found by walking the enclosing `cref` innermost-first) when it's a
/// `NAME = <expr>` written in that class's own body. The owner is returned so a
/// caller can re-fold the initializer in the owner's OWN lexical scope, exactly
/// where the constant's value was computed.
fn const_init(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    scope: Option<&str>,
    name: &str,
) -> Option<(ClassId, NodeId)> {
    let owners: Vec<ClassId> = match scope {
        Some(s) => vec![compiler.resolve_class(s, cref, box_id)?],
        // A bare const resolves against the enclosing namespaces, closest first.
        None => cref.iter().rev().copied().collect(),
    };
    for owner in owners {
        for &s in &compiler.class(owner).class_body_stmts {
            if let HirNode::ConstWrite {
                name: n,
                value,
                scope: cw_scope,
            } = &compiler.hir[s]
            {
                // A bare reference only matches a bare `NAME = ...` definition;
                // a `Scope::NAME` match is exact by construction.
                if n == name && (scope.is_some() || cw_scope.is_none()) {
                    return Some((owner, *value));
                }
            }
        }
    }
    None
}

/// The literal string of `scope::name` when it's a `NAME = "..."` written in
/// `scope`'s own body (e.g. `Gem::VERSION`) -- read straight from the registered
/// class-body statement, so it's known even before value constants are fully
/// resolved.
fn const_string(compiler: &Compiler, cref: &[ClassId], box_id: u32, name: &str) -> Option<String> {
    let (scope, base) = name.rsplit_once("::")?;
    let (_, value) = const_init(compiler, cref, box_id, Some(scope), base)?;
    string_lit(compiler, value)
}

/// Whether `name` (as written at a call site) resolves to the `Gem::Version`
/// class -- so `Gem::Version.new`, or a bare `Version` whose cref reaches it.
fn is_gem_version(compiler: &Compiler, cref: &[ClassId], box_id: u32, name: &str) -> bool {
    match (
        compiler.resolve_class(name, cref, box_id),
        compiler.resolve_class("Gem::Version", cref, box_id),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Reduce a node to a compile-time PLAIN STRING (a `String` object's value),
/// for a lexicographic `String#<=>` comparison: a string literal,
/// `RUBY_VERSION`/`RUBY_ENGINE_VERSION`, a `Scope::NAME = "..."` constant, or
/// `Gem.rubygems_version`'s underlying `Gem::VERSION` string.
fn static_string(compiler: &Compiler, cref: &[ClassId], box_id: u32, node: NodeId) -> Option<String> {
    match &compiler.hir[node] {
        HirNode::StringLit(_) => string_lit(compiler, node),
        HirNode::ClassRef(name) => match name.as_str() {
            "RUBY_VERSION" | "RUBY_ENGINE_VERSION" => Some(zeo_abi::RUBY_VERSION.to_string()),
            _ if name.contains("::") => const_string(compiler, cref, box_id, name),
            _ => const_init(compiler, cref, box_id, None, name)
                .and_then(|(_, v)| string_lit(compiler, v)),
        },
        HirNode::QualifiedConstRead(scope, name) => const_init(compiler, cref, box_id, Some(scope), name)
            .and_then(|(_, v)| string_lit(compiler, v)),
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "rubygems_version" && args.is_empty() => match &compiler.hir[*r] {
            HirNode::ClassRef(rn) if rn == "Gem" => const_string(compiler, cref, box_id, "Gem::VERSION"),
            _ => None,
        },
        _ => None,
    }
}

/// Reduce a node to a compile-time `Gem::Version` string (for version-segment
/// comparison, not lexicographic): `Gem::Version.new("x")` /
/// `Gem::Version.create("x")`, or `Gem.rubygems_version`.
fn static_gem_version(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
) -> Option<String> {
    match &compiler.hir[node] {
        HirNode::New {
            class_name, args, ..
        } if is_gem_version(compiler, cref, box_id, class_name) => match args.as_slice() {
            [a] => static_string(compiler, cref, box_id, *a),
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
                    [ArrayElem::Single(a)] => static_string(compiler, cref, box_id, *a),
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

/// A comparison guard whose operands both reduce to build-time version
/// constants: `Gem::Version` (version-segment) semantics first, then plain
/// `String` (lexicographic) semantics -- a mixed Version/String comparison,
/// which Ruby itself raises on, stays `None`.
fn cmp_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    op: &str,
    l: NodeId,
    r: NodeId,
) -> Option<bool> {
    if let (Some(lv), Some(rv)) = (
        static_gem_version(compiler, cref, box_id, l),
        static_gem_version(compiler, cref, box_id, r),
    ) {
        return apply(op, cmp_versions(&lv, &rv));
    }
    if let (Some(ls), Some(rs)) = (
        static_string(compiler, cref, box_id, l),
        static_string(compiler, cref, box_id, r),
    ) {
        return apply(op, ls.as_bytes().cmp(rs.as_bytes()));
    }
    None
}

/// A literal method-name argument (`:validate_for_resolution` / its string
/// form), the first argument of a `respond_to?`/`method_defined?` probe.
fn probe_name(compiler: &Compiler, args: &[ArrayElem]) -> Option<String> {
    let first = match args {
        [ArrayElem::Single(a), ..] => *a,
        _ => return None,
    };
    match &compiler.hir[first] {
        HirNode::SymbolLit(s) => Some(s.clone()),
        HirNode::StringLit(parts) => match parts.as_slice() {
            [StrPart::Lit(s)] => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The class an `X.new` / bare `new` (implicit-self `new` in a class body)
/// receiver is an INSTANCE of, resolved in `cref`. `None` for any other receiver
/// shape (a class object itself, a local, ...), which the caller handles.
fn instance_class(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: NodeId,
) -> Option<ClassId> {
    match &compiler.hir[receiver] {
        HirNode::New { class_name, .. } => compiler.resolve_class(class_name, cref, box_id),
        // Bare `new` -- `self.new` in a class body, so an instance of the class
        // whose body the guard sits in (the innermost cref entry).
        HirNode::Call {
            receiver: None,
            name,
            args,
            ..
        } if name == "new" && args.is_empty() => cref.last().copied(),
        _ => None,
    }
}

/// Compile-time truth of `recv.respond_to?(:m)`. Answered against the compiled
/// method tables: for an INSTANCE receiver (`X.new`, bare `new`), whether the
/// class has a PUBLIC instance method `m` (respond_to?'s default excludes
/// non-public); for a CLASS receiver, its class methods. A found-public method
/// is `Some(true)`; a definitively-absent instance method is `Some(false)`; a
/// class receiver whose class method is absent stays `None` (a builtin from
/// `Module`/`Object` could still answer it -- don't guess `false`).
fn respond_to_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    args: &[ArrayElem],
) -> Option<bool> {
    let receiver = receiver?;
    let m = probe_name(compiler, args)?;
    if let Some(cls) = instance_class(compiler, cref, box_id, receiver) {
        return Some(match compiler.method_in_chain(cls, &m) {
            Some((_, sid)) => compiler.scope(sid).visibility == Visibility::Public,
            None => false,
        });
    }
    // A class receiver: a bare `Process` (`ClassRef`) or a top-anchored
    // `::Process` (which reads as `QualifiedConstRead("Object", "Process")` --
    // a top-level constant lives on `Object`), both resolved at the root scope.
    let resolved = match &compiler.hir[receiver] {
        HirNode::ClassRef(name) => match name.strip_prefix("::") {
            Some(rooted) => compiler.resolve_class(rooted, &[], box_id),
            None => compiler.resolve_class(name, cref, box_id),
        },
        HirNode::QualifiedConstRead(scope, name) if scope == "Object" => {
            compiler.resolve_class(name, &[], box_id)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            compiler.resolve_class(&format!("{scope}::{name}"), cref, box_id)
        }
        _ => None,
    };
    if let Some(cls) = resolved {
        if compiler.class_method_in_chain(cls, &m).is_some()
            || target_provides_class_method(compiler, cls, &m)
        {
            return Some(true);
        }
    }
    None
}

/// Builtin class methods the compile TARGET (ruby 4.0.5 on this platform)
/// provides NATIVELY, which the compiler's own `class_methods` tables don't list
/// (they live in zeo-rt, never linked into `zeo`). Consulted by `respond_to?`
/// folding so a platform/version gate written against one decides the way it
/// does on the target. Currently just `Process._fork` (MRI 3.1+, present on
/// every fork-capable platform zeo targets) -- gems gate a fork hook on it
/// (`if ::Process.respond_to?(:_fork)`, connection_pool). Keep in sync with what
/// zeo-rt actually implements; the class-method analogue of `ALWAYS_DEFINED_CONSTS`.
fn target_provides_class_method(compiler: &Compiler, cls: ClassId, name: &str) -> bool {
    matches!(
        (compiler.fq_name(cls).as_str(), name),
        ("Process", "fork") | ("Process", "_fork")
    )
}

/// Compile-time truth of `Recv.method_defined?(:m)` / bare `method_defined?(:m)`
/// (implicit self = the enclosing class): whether the class has a non-private
/// instance method `m` (`method_defined?`'s rule). `None` if the class doesn't
/// resolve.
fn method_defined_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    args: &[ArrayElem],
) -> Option<bool> {
    let m = probe_name(compiler, args)?;
    let cls = match receiver {
        Some(r) => match &compiler.hir[r] {
            HirNode::ClassRef(name) => compiler.resolve_class(name, cref, box_id)?,
            _ => return None,
        },
        None => *cref.last()?,
    };
    Some(match compiler.method_in_chain(cls, &m) {
        Some((_, sid)) => compiler.scope(sid).visibility != Visibility::Private,
        None => false,
    })
}

/// A call-shaped guard: a version/string comparison, a feature probe
/// (`respond_to?`/`method_defined?`), or `<bool>.freeze` (a no-op on a boolean,
/// which is how `VALIDATES_FOR_RESOLUTION` is written).
fn call_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
) -> Option<bool> {
    match name {
        "<" | "<=" | ">" | ">=" | "==" | "!=" => {
            let l = receiver?;
            let [ArrayElem::Single(r)] = args else {
                return None;
            };
            cmp_fold(compiler, cref, box_id, name, l, *r)
        }
        "freeze" if args.is_empty() => static_bool(compiler, cref, box_id, receiver?),
        "respond_to?" => respond_to_fold(compiler, cref, box_id, receiver, args),
        "method_defined?" | "public_method_defined?" => {
            method_defined_fold(compiler, cref, box_id, receiver, args)
        }
        _ => None,
    }
}

/// Compile-time truth of a guard expression, or `None` when it isn't one of the
/// decidable target-constant forms (leave the condition to run normally).
fn static_bool(compiler: &Compiler, cref: &[ClassId], box_id: u32, node: NodeId) -> Option<bool> {
    match &compiler.hir[node] {
        // A plain `true`/`false` literal is deliberately NOT folded here: literal
        // conditions have their own (if-expression-aware) codegen path, and
        // hijacking it drops leading side-effect statements from a folded branch.
        HirNode::And(l, r) => match static_bool(compiler, cref, box_id, *l) {
            Some(false) => Some(false),
            Some(true) => static_bool(compiler, cref, box_id, *r),
            None => None,
        },
        HirNode::Or(l, r) => match static_bool(compiler, cref, box_id, *l) {
            Some(true) => Some(true),
            Some(false) => static_bool(compiler, cref, box_id, *r),
            None => None,
        },
        // A boolean value constant (`VALIDATES_FOR_RESOLUTION`) folds through
        // its own initializer, evaluated in the owning class's lexical scope.
        HirNode::ClassRef(name) if !name.contains("::") => {
            let (owner, value) = const_init(compiler, cref, box_id, None, name)?;
            static_bool(compiler, &compiler.cref_of(Some(owner)), box_id, value)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let (owner, value) = const_init(compiler, cref, box_id, Some(scope), name)?;
            static_bool(compiler, &compiler.cref_of(Some(owner)), box_id, value)
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            ..
        } if kwargs.is_empty() && block.is_none() => {
            call_fold(compiler, cref, box_id, *receiver, name, args)
        }
        _ => None,
    }
}

/// The one entry point (see the module docs). `cref` is the lexical class chain
/// at the guard site (outermost-first, as `Compiler::cref_of` yields), empty at
/// the top level.
pub(crate) fn static_cond(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    cond: NodeId,
) -> Option<bool> {
    static_bool(compiler, cref, box_id, cond)
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
