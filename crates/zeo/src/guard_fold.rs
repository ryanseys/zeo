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
//! * **Constant probes** -- `if defined? ::Psych::Visitors`. Which constants
//!   exist is likewise fixed for a whole-program target; see
//!   [`defined_const_fold`] for the (deliberately narrow) rule.
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

use crate::compiler::{ClassId, Compiler, ScopeId};
use crate::hir::{ArrayElem, HirNode, NodeId, Params, StrPart, Visibility};
use std::cmp::Ordering;

/// How far a fold may chase one guard through the definitions behind it -- a
/// constant's initializer, a predicate method's body, another constant inside
/// that. Deep enough that no real guard reaches it, and the reason it exists at
/// all is that the chase can CYCLE: `A = B` beside `B = A`, or a predicate that
/// calls itself, would otherwise recur until the stack ran out.
const MAX_FOLD_DEPTH: u32 = 32;

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
    // The top level, where every bare lookup ends. A `CONST = ...` written
    // outside any class is a statement of a `Program` rather than of a class
    // body, so it is not in the tables walked above. Only a name written ONCE
    // answers: two writes (log4r's `HAVE_REXML = true` / `= false`, one per
    // branch of a rescue) have no single initializer to read.
    if scope.is_some() {
        return None;
    }
    let mut found = None;
    for node in compiler.hir.iter() {
        let HirNode::Program(stmts) = node else {
            continue;
        };
        for &s in stmts {
            if let HirNode::ConstWrite {
                name: n,
                value,
                scope: None,
            } = &compiler.hir[s]
                && n == name
            {
                if found.is_some() {
                    return None;
                }
                found = Some((crate::compiler::OBJECT_CLASS, *value));
            }
        }
    }
    found
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
        // Nothing is registered under either name. Ruby preloads rubygems into
        // every program, so a gem may reach for `Gem::Version` without ever
        // requiring it -- unparser gates its two `Builder` definitions on one --
        // while zeo compiles rubygems in only when a program asks for it. With
        // no class to compare, the name AS WRITTEN is the only evidence, and it
        // is enough: a `Version` naming anything else would have resolved, and
        // answered above.
        (None, None) => matches!(name, "Gem::Version" | "::Gem::Version"),
        _ => false,
    }
}

/// The value of a string constant ruby seeds into every program. Each is as
/// fixed for a whole-program target as a literal written in the source.
fn seeded_string_const(name: &str) -> Option<String> {
    match name {
        "RUBY_VERSION" | "RUBY_ENGINE_VERSION" => Some(zeo_abi::RUBY_VERSION.to_string()),
        // `if RUBY_ENGINE == "truffleruby"` is the other compat gate a gem
        // writes around a whole `class`/`def`, and it decides the same way the
        // runtime seeds it (see `bootstrap`'s `ENGINE`: zeo reports MRI's
        // identity, so a gem takes its CRuby path).
        "RUBY_ENGINE" => Some("ruby".to_string()),
        // `if RUBY_PLATFORM == 'java'` guards a JRuby-only branch. Baked from
        // the build target (`build.rs`), the same way the runtime seeds the
        // program's own copy -- so the two always agree.
        "RUBY_PLATFORM" => Some(env!("ZEO_RUBY_PLATFORM").to_string()),
        _ => None,
    }
}

/// A constant's initializer, read as a string in the lexical scope where the
/// constant was WRITTEN rather than where it is read. The two differ the moment
/// an initializer names another constant, and the writing scope is the one that
/// resolved it.
fn through_const(
    compiler: &Compiler,
    box_id: u32,
    owner: ClassId,
    value: NodeId,
    depth: u32,
) -> Option<String> {
    static_string(
        compiler,
        &compiler.cref_of(Some(owner)),
        box_id,
        value,
        depth,
    )
}

/// Reduce a node to a compile-time PLAIN STRING (a `String` object's value),
/// for a lexicographic `String#<=>` comparison: a string literal,
/// `RUBY_VERSION`/`RUBY_ENGINE_VERSION`, a `Scope::NAME = "..."` constant, or
/// `Gem.rubygems_version`'s underlying `Gem::VERSION` string.
fn static_string(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
    depth: u32,
) -> Option<String> {
    if depth >= MAX_FOLD_DEPTH {
        return None;
    }
    let depth = depth + 1;
    match &compiler.hir[node] {
        HirNode::StringLit(_) => string_lit(compiler, node),
        // `X = defined?(::RUBY_ENGINE) ? ::RUBY_ENGINE : "ruby"` -- a constant
        // whose value is picked at load time by a question this module already
        // answers. sass writes exactly that, and its `ironruby?` reads the
        // result.
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let taken = if static_bool(compiler, cref, box_id, *cond, depth)? {
                then_body
            } else {
                else_body
            };
            let [only] = taken[..] else {
                return None;
            };
            static_string(compiler, cref, box_id, only, depth)
        }
        HirNode::ClassRef(name) if name.contains("::") => {
            const_string(compiler, cref, box_id, name)
        }
        // A constant the program writes itself SHADOWS the one ruby seeds, for
        // every read whose lexical scope reaches it -- sass defines its own
        // `Sass::Util::RUBY_VERSION` (the segments, as integers) and reads it
        // unqualified all through the module.
        HirNode::ClassRef(name) => match const_init(compiler, cref, box_id, None, name) {
            Some((owner, v)) => through_const(compiler, box_id, owner, v, depth),
            None => seeded_string_const(name),
        },
        HirNode::QualifiedConstRead(scope, name) => {
            match const_init(compiler, cref, box_id, Some(scope), name) {
                Some((owner, v)) => through_const(compiler, box_id, owner, v, depth),
                // `::RUBY_VERSION` -- the top-anchored spelling, which is how a
                // gem reaches the seeded constant from inside the namespace
                // shadowing its name. A top-level constant lives on `Object`.
                None if scope == "Object" => seeded_string_const(name),
                None => None,
            }
        }
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "rubygems_version" && args.is_empty() => match &compiler.hir[*r] {
            HirNode::ClassRef(rn) if rn == "Gem" => {
                const_string(compiler, cref, box_id, "Gem::VERSION")
            }
            _ => None,
        },
        // `RUBY_PLATFORM.to_s` -- `String#to_s` returns self, and gems write it
        // where the value might have been a symbol under some other engine.
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "to_s" && args.is_empty() => static_string(compiler, cref, box_id, *r, depth),
        // `RUBY_VERSION[0, 3]` -- the 1.8/1.9-era prefix probe
        // (rspec-expectations gates its whole 1.9 `append_features` on it).
        // Only the two-integer-argument slice; a Range or regexp index stays
        // undecided. Char-counted like `String#[]`, though every fed string
        // here is ASCII.
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "[]" && args.len() == 2 => {
            let [crate::hir::ArrayElem::Single(a0), crate::hir::ArrayElem::Single(a1)] = args[..]
            else {
                return None;
            };
            let start = static_integer(compiler, cref, box_id, a0, depth)?;
            let len = static_integer(compiler, cref, box_id, a1, depth)?;
            let s = static_string(compiler, cref, box_id, *r, depth)?;
            let chars: Vec<char> = s.chars().collect();
            let start = usize::try_from(start).ok()?;
            let len = usize::try_from(len).ok()?;
            if start > chars.len() {
                return None; // nil in ruby -- not a string
            }
            Some(chars[start..(start + len).min(chars.len())].iter().collect())
        }
        _ => None,
    }
}

/// Reduce a node to a compile-time INTEGER. A version gate is as often written
/// against the number as against the string -- `Rails::VERSION::MAJOR == 8`,
/// `ActiveRecord::VERSION::MINOR >= 2`, `Chef::VERSION.to_i >= 12` -- and a
/// gem's `VERSION` module is as fixed for a whole-program target as the strings
/// beside it.
fn static_integer(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
    depth: u32,
) -> Option<i64> {
    if depth >= MAX_FOLD_DEPTH {
        return None;
    }
    let depth = depth + 1;
    let through = |owner: ClassId, value: NodeId| {
        static_integer(
            compiler,
            &compiler.cref_of(Some(owner)),
            box_id,
            value,
            depth,
        )
    };
    match &compiler.hir[node] {
        HirNode::IntegerLit(n) => Some(*n),
        HirNode::ClassRef(name) => {
            let (scope, base) = match name.rsplit_once("::") {
                Some((s, b)) => (Some(s), b),
                None => (None, name.as_str()),
            };
            let (owner, value) = const_init(compiler, cref, box_id, scope, base)?;
            through(owner, value)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let (owner, value) = const_init(compiler, cref, box_id, Some(scope), name)?;
            through(owner, value)
        }
        // `Chef::VERSION.to_i` is 12: ruby reads the leading integer and stops.
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "to_i" && args.is_empty() => {
            leading_integer(&static_string(compiler, cref, box_id, *r, depth)?)
        }
        // `Sass::Util::RUBY_VERSION[0]` -- a version kept as segments is read
        // one segment at a time. An index past the end answers `nil` in ruby,
        // which is not an integer, so the guard above stays undecided rather
        // than take a shorter list's missing segment for a zero.
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            block: None,
            block_arg: None,
            ..
        } if name == "[]" => {
            let [ArrayElem::Single(i)] = args[..] else {
                return None;
            };
            let list = static_integer_list(compiler, cref, box_id, *r, depth)?;
            let index = static_integer(compiler, cref, box_id, i, depth)?;
            let index = if index < 0 {
                index.checked_add(list.len() as i64)?
            } else {
                index
            };
            list.get(usize::try_from(index).ok()?).copied()
        }
        _ => None,
    }
}

/// Ruby's `String#split` for a separator that is a plain string.
///
/// A single space is the AWK split: runs of ASCII whitespace separate, and
/// leading whitespace starts no empty field. An empty separator splits into
/// characters. Everything else splits on the literal. With no limit ruby drops
/// trailing empty fields -- but not leading ones, so `".a.".split(".")` is
/// `["", "a"]`.
fn split_string(s: &str, sep: &str) -> Vec<String> {
    let mut parts: Vec<String> = if sep == " " {
        s.split_ascii_whitespace().map(str::to_string).collect()
    } else if sep.is_empty() {
        s.chars().map(String::from).collect()
    } else {
        s.split(sep).map(str::to_string).collect()
    };
    while parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    parts
}

/// Reduce a node to a compile-time list of STRINGS: an array literal of them,
/// or a static string cut up by `split`.
fn static_string_list(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
    depth: u32,
) -> Option<Vec<String>> {
    if depth >= MAX_FOLD_DEPTH {
        return None;
    }
    let depth = depth + 1;
    match &compiler.hir[node] {
        HirNode::ArrayLit(elems) => elems
            .iter()
            .map(|e| match e {
                ArrayElem::Single(v) => static_string(compiler, cref, box_id, *v, depth),
                // A splat is another list, spread here. Nothing measured needs
                // it, so the whole list declines rather than guess.
                ArrayElem::Splat(_) => None,
            })
            .collect(),
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            block: None,
            block_arg: None,
            ..
        } if name == "split" => {
            let [ArrayElem::Single(sep)] = args[..] else {
                return None;
            };
            let subject = static_string(compiler, cref, box_id, *r, depth)?;
            // A Regexp separator reduces to no string, so it declines here.
            let sep = static_string(compiler, cref, box_id, sep, depth)?;
            Some(split_string(&subject, &sep))
        }
        _ => None,
    }
}

/// Reduce a node to a compile-time list of INTEGERS. A gem that gates on the
/// ruby version by SEGMENT keeps it as one -- sass writes
/// `RUBY_VERSION = ::RUBY_VERSION.split(".").map {|s| s.to_i}` once and indexes
/// it at every guard beneath, so the list is what has to fold for any of them
/// to.
fn static_integer_list(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
    depth: u32,
) -> Option<Vec<i64>> {
    if depth >= MAX_FOLD_DEPTH {
        return None;
    }
    let depth = depth + 1;
    let through = |owner: ClassId, value: NodeId| {
        static_integer_list(
            compiler,
            &compiler.cref_of(Some(owner)),
            box_id,
            value,
            depth,
        )
    };
    match &compiler.hir[node] {
        HirNode::ArrayLit(elems) => elems
            .iter()
            .map(|e| match e {
                ArrayElem::Single(v) => static_integer(compiler, cref, box_id, *v, depth),
                ArrayElem::Splat(_) => None,
            })
            .collect(),
        HirNode::ClassRef(name) => {
            let (scope, base) = match name.rsplit_once("::") {
                Some((s, b)) => (Some(s), b),
                None => (None, name.as_str()),
            };
            let (owner, value) = const_init(compiler, cref, box_id, scope, base)?;
            through(owner, value)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let (owner, value) = const_init(compiler, cref, box_id, Some(scope), name)?;
            through(owner, value)
        }
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            block,
            block_arg,
            ..
        } if matches!(name.as_str(), "map" | "collect")
            && args.is_empty()
            && maps_each_to_i(compiler, *block, *block_arg) =>
        {
            Some(
                static_string_list(compiler, cref, box_id, *r, depth)?
                    .iter()
                    .map(|s| leading_integer(s))
                    .collect::<Option<Vec<_>>>()?,
            )
        }
        _ => None,
    }
}

/// Whether a `map` block reads each entry as a number -- `{ |s| s.to_i }`, or
/// the `&:to_i` shorthand.
fn maps_each_to_i(compiler: &Compiler, block: Option<NodeId>, block_arg: Option<NodeId>) -> bool {
    if let Some(arg) = block_arg {
        return matches!(&compiler.hir[arg], HirNode::SymbolLit(s) if s == "to_i");
    }
    let Some(block) = block else {
        return false;
    };
    let HirNode::Block { params, body } = &compiler.hir[block] else {
        return false;
    };
    let ([param], [stmt]) = (params.required.as_slice(), body.as_slice()) else {
        return false;
    };
    matches!(
        &compiler.hir[*stmt],
        HirNode::Call { receiver: Some(entry), name, args, block: None, block_arg: None, .. }
            if name == "to_i"
                && args.is_empty()
                && matches!(&compiler.hir[*entry], HirNode::LocalRead(n) if n == param)
    )
}

/// `String#to_i`: optional leading whitespace, an optional sign, then digits,
/// stopping at the first character that is not one. A single `_` BETWEEN digits
/// is part of the number; anything else ends it, and a string with no leading
/// digits at all is `0`.
///
/// `None` only where the value would not fit -- a bignum ruby would still
/// compare exactly, so the guard stays undecided rather than wrap.
fn leading_integer(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let mut chars = digits.chars().peekable();
    let mut value: i64 = 0;
    let mut any = false;
    while let Some(&c) = chars.peek() {
        if let Some(d) = c.to_digit(10) {
            value = value.checked_mul(10)?.checked_add(d as i64)?;
            any = true;
        } else if !(c == '_' && any && chars.clone().nth(1).is_some_and(|n| n.is_ascii_digit())) {
            break;
        }
        chars.next();
    }
    Some(if negative { -value } else { value })
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
fn cmp_fold(
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
    None
}

/// One position of an alternative.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Elem {
    /// A character that means itself -- including `\.`, the escape a version
    /// test spells the dot with (`/^1\.8/`).
    Lit(char),
    /// `.` -- exactly one character, and never a newline. The one regexp
    /// METAcharacter this reduction admits, admitted because it is exact:
    /// nothing here approximates it. A quantifier is still refused, so a `.`
    /// can never stand for more or less than one character.
    AnyChar,
}

/// One `|`-alternative, position by position.
struct Alternative(Vec<Elem>);

impl Alternative {
    /// How many BYTES of `subject` this alternative consumes from its start, or
    /// `None` if it does not match there.
    fn match_at(&self, subject: &str, ignore_case: bool) -> Option<usize> {
        let mut chars = subject.chars();
        let mut consumed = 0;
        for elem in &self.0 {
            let c = chars.next()?;
            let ok = match elem {
                Elem::Lit(l) => {
                    c == *l || (ignore_case && c.eq_ignore_ascii_case(l) && c.is_ascii())
                }
                Elem::AnyChar => c != '\n',
            };
            if !ok {
                return None;
            }
            consumed += c.len_utf8();
        }
        Some(consumed)
    }
}

/// A regexp whose whole meaning is a plain character test -- so folding it here
/// cannot disagree with a real regexp engine. Either `|`-separated alternatives
/// matched anywhere in the subject, or ONE alternative anchored at the start
/// and/or end. Half the corpus writes its platform gate this way
/// (`/mswin|mingw|windows/`), and a version gate its prefix
/// (`/^1.8/`).
///
/// `None` for anything carrying regexp syntax beyond `.` (classes, quantifiers,
/// groups, other escapes), an interpolated pattern, or a flag that changes what
/// the pattern MEANS (`/x`, `/m`). Those stay undecided rather than being
/// answered by an approximation.
struct LiteralPattern {
    anchored_start: bool,
    anchored_end: bool,
    /// `/i`. ASCII-folded here, so [`LiteralPattern::matches`] declines a
    /// subject that isn't ASCII -- ruby folds the full Unicode case table, and
    /// a narrower rule must not answer where the two could part.
    ignore_case: bool,
    alts: Vec<Alternative>,
}

impl LiteralPattern {
    fn matches(&self, subject: &str) -> Option<bool> {
        if self.ignore_case && !subject.is_ascii() {
            return None;
        }
        // `^`/`$` are LINE anchors, and this treats them as string anchors. The
        // two agree on a subject with no newline in it, which every build-time
        // string here is; anything else declines rather than pick a reading.
        if (self.anchored_start || self.anchored_end) && subject.contains('\n') {
            return None;
        }
        Some(self.alts.iter().any(|alt| self.matches_alt(alt, subject)))
    }

    fn matches_alt(&self, alt: &Alternative, subject: &str) -> bool {
        let starts: Box<dyn Iterator<Item = usize>> = if self.anchored_start {
            Box::new(std::iter::once(0))
        } else {
            Box::new((0..=subject.len()).filter(|&i| subject.is_char_boundary(i)))
        };
        starts
            .into_iter()
            .any(|i| match alt.match_at(&subject[i..], self.ignore_case) {
                Some(n) if self.anchored_end => i + n == subject.len(),
                Some(_) => true,
                None => false,
            })
    }
}

/// One alternative parsed into its positions, or `None` if it uses regexp
/// syntax this reduction does not admit.
fn literal_alternative(src: &str) -> Option<Alternative> {
    // `.` is deliberately absent: it is handled below, exactly. Every
    // quantifier stays here, which is what keeps a `.` bound to one character.
    const SYNTAX: &[char] = &['^', '$', '[', ']', '(', ')', '*', '+', '?', '{', '}', '|'];
    let mut out = Vec::new();
    let mut chars = src.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '\\' => match chars.next() {
                Some('.') => Elem::Lit('.'),
                _ => return None,
            },
            '.' => Elem::AnyChar,
            c if SYNTAX.contains(&c) => return None,
            c => Elem::Lit(c),
        });
    }
    (!out.is_empty()).then_some(Alternative(out))
}

fn literal_pattern(compiler: &Compiler, node: NodeId) -> Option<LiteralPattern> {
    let HirNode::RegexpLit(parts, flags) = &compiler.hir[node] else {
        return None;
    };
    if flags.extended || flags.multiline {
        return None;
    }
    let [StrPart::Lit(src)] = parts.as_slice() else {
        return None;
    };
    let mut src = src.as_str();
    let anchored_start = ["\\A", "^"].iter().any(|p| match src.strip_prefix(p) {
        Some(rest) => {
            src = rest;
            true
        }
        None => false,
    });
    let anchored_end = ["\\z", "$"].iter().any(|s| match src.strip_suffix(s) {
        Some(rest) => {
            src = rest;
            true
        }
        None => false,
    });
    // An anchor binds only ONE alternative in ruby -- `/^a|b/` is `(^a)|b`, not
    // `^(a|b)` -- so an anchored pattern that alternates is not this simple.
    if (anchored_start || anchored_end) && src.contains('|') {
        return None;
    }
    let alts: Vec<Alternative> = src
        .split('|')
        .map(literal_alternative)
        .collect::<Option<_>>()?;
    // ASCII case folding is only ruby's answer for an ASCII pattern.
    if flags.ignore_case
        && !alts.iter().all(|a| {
            a.0.iter()
                .all(|e| !matches!(e, Elem::Lit(c) if !c.is_ascii()))
        })
    {
        return None;
    }
    Some(LiteralPattern {
        anchored_start,
        anchored_end,
        ignore_case: flags.ignore_case,
        alts,
    })
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
        // A LITERAL is an instance of its own class, and it is how the
        // capability probes are actually written: `''.respond_to?(:bytesize)`,
        // `[].respond_to?(:sum)`. Same fact as `String.new.respond_to?`, spelled
        // the way anyone would spell it.
        HirNode::StringLit(_) => Some(zeo_abi::STRING_CLASS),
        HirNode::SymbolLit(_) => Some(zeo_abi::SYMBOL_CLASS),
        HirNode::IntegerLit(_) => Some(zeo_abi::INTEGER_CLASS),
        HirNode::FloatLit(_) => Some(zeo_abi::FLOAT_CLASS),
        HirNode::ArrayLit(_) => Some(zeo_abi::ARRAY_CLASS),
        HirNode::HashLit(_) => Some(zeo_abi::HASH_CLASS),
        // The three standard streams are IO instances, and highline probes one
        // (`unless STDIN.respond_to? :getbyte`). They are constants rather than
        // classes, so nothing else here would reach them.
        HirNode::ClassRef(n)
            if matches!(n.trim_start_matches("::"), "STDIN" | "STDOUT" | "STDERR") =>
        {
            Some(zeo_abi::IO_CLASS)
        }
        _ => None,
    }
}

/// Whether a BUILTIN class or one of its ancestors declares instance method
/// `name` natively. `builtin_surface` answers per class, without inheritance,
/// so the chain is walked here -- `''.respond_to?(:each_char)` has to see
/// `String`'s own row, `''.respond_to?(:tap)` `Kernel`'s.
///
/// Walks the DECLARED `parent`/`includes` edges rather than `ClassInfo::
/// ancestors`, which `mro::materialize` fills in only after this whole walk has
/// finished -- reading it here would silently see an empty chain and report
/// every inherited method missing.
///
/// `Some(false)` only when EVERY class on the chain has a projected surface: a
/// class not yet migrated to the macro has no list to be absent from, and
/// "missing" would then be a fact about zeo's build rather than about Ruby.
/// When they all do, the absence is real -- `"".respond_to?(:parameterize)` is
/// how test-prof asks whether ActiveSupport has been loaded, and the honest
/// answer is no.
fn builtin_provides_instance_method(
    compiler: &Compiler,
    class: ClassId,
    name: &str,
) -> Option<bool> {
    let mut seen = Vec::new();
    let mut queue = vec![class];
    let mut all_projected = true;
    while let Some(c) = queue.pop() {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        match crate::builtin_surface::surface_for(c) {
            Some(s) if s.instance_methods.contains(&name) => return Some(true),
            Some(_) => {}
            // `Object` and compiled user classes have no projected surface and
            // need none: their methods are COMPILED `def`s, and
            // `method_in_chain` -- which the caller already asked -- is the
            // table that holds them.
            None if c == zeo_abi::OBJECT_CLASS || !compiler.class(c).is_builtin => {}
            None => all_projected = false,
        }
        let info = compiler.class(c);
        queue.extend(info.includes.iter().copied());
        queue.extend(info.parent);
    }
    all_projected.then_some(false)
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
        if let Some((_, sid)) = compiler.method_in_chain(cls, &m) {
            return Some(compiler.scope(sid).visibility == Visibility::Public);
        }
        // NATIVE rows are not in `method_in_chain` -- that table holds
        // compiled Ruby methods. Ask the projected surface for the rest,
        // whether `cls` is itself a builtin or a user class inheriting the
        // method from Object/Kernel.
        return builtin_provides_instance_method(compiler, cls, &m);
    }
    if let Some(cls) = const_receiver_class(compiler, cref, box_id, receiver) {
        // A user-defined class method (compiled `class_methods`) OR a native
        // builtin class method the class declares in its `ruby_class!`/
        // `ruby_module!` (projected into `CLASS_SURFACE` -- e.g.
        // `Process.respond_to?(:_fork)`, which connection_pool's ForkTracker
        // gates on). The projection is why this needs no hardcoded
        // allowlist.
        if compiler.class_method_in_chain(cls, &m).is_some()
            || crate::builtin_surface::provides_class_method(cls, &m)
        {
            return Some(true);
        }
    }
    None
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
    match compiler.method_in_chain(cls, &m) {
        Some((_, sid)) => Some(compiler.scope(sid).visibility != Visibility::Private),
        // `method_in_chain` holds COMPILED ruby methods, so native rows are not
        // in it -- neither a builtin's own nor the ones a USER class inherits
        // from Object/Kernel. Without the ancestor walk,
        // `Regexp.method_defined?(:match?)` -- and a user class asked about
        // `:singleton_class` (rspec's 1.8.7 shim guard) -- answered a
        // confident false about methods the class has.
        None => builtin_provides_instance_method(compiler, cls, &m),
    }
}

/// Compile-time truth of a membership test -- the same questions the arms above
/// answer, asked through a list.
///
/// Two receivers answer. A LITERAL array of strings tested against a build-time
/// string is just that comparison (`['opal', 'rubymotion'].include?(RUBY_ENGINE)`,
/// which array.rb gates a whole reopen on). And `X.instance_methods.include?(:m)`
/// is `X.method_defined?(:m)` spelled the long way -- both ask for the public
/// and protected instance methods of `X` and its ancestors.
fn include_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: NodeId,
    args: &[ArrayElem],
    depth: u32,
) -> Option<bool> {
    let [ArrayElem::Single(wanted)] = args else {
        return None;
    };
    match &compiler.hir[receiver] {
        HirNode::ArrayLit(elems) => {
            let wanted = static_string(compiler, cref, box_id, *wanted, depth)?;
            let mut hit = false;
            for elem in elems {
                // A splat could hold anything, so it takes the whole list with
                // it rather than being skipped.
                let ArrayElem::Single(e) = elem else {
                    return None;
                };
                hit |= static_string(compiler, cref, box_id, *e, depth)? == wanted;
            }
            Some(hit)
        }
        HirNode::Call {
            receiver: Some(cls),
            name,
            args: inner,
            block: None,
            ..
        } if inner.is_empty()
            && matches!(
                name.as_str(),
                "instance_methods" | "public_instance_methods"
            ) =>
        {
            method_defined_fold(compiler, cref, box_id, Some(*cls), args)
        }
        _ => None,
    }
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
    depth: u32,
) -> Option<bool> {
    match name {
        "<" | "<=" | ">" | ">=" | "==" | "!=" => {
            let l = receiver?;
            let [ArrayElem::Single(r)] = args else {
                return None;
            };
            cmp_fold(compiler, cref, box_id, name, l, *r, depth)
        }
        // `unless !defined?(X::VERSION)` -- the pervasive reload guard. Only a
        // condition that folds on its own negates; anything else stays `None`.
        "!" if args.is_empty() => Some(!static_bool(compiler, cref, box_id, receiver?, depth)?),
        "freeze" if args.is_empty() => static_bool(compiler, cref, box_id, receiver?, depth),
        // `if RUBY_PLATFORM =~ /mswin|mingw|windows/` -- the platform gate half
        // the corpus writes, and the reason a windows-only file gets compiled
        // at all. Only a pattern that is literal alternatives folds (see
        // `literal_alternatives`); anything with real regexp syntax in it stays
        // undecided rather than being matched by an approximation.
        "=~" | "match?" => {
            let [ArrayElem::Single(arg)] = args else {
                return None;
            };
            let recv = receiver?;
            // Either side may hold the pattern: `RUBY_PLATFORM =~ /x/` and
            // `/x/ =~ RUBY_PLATFORM` are the same question.
            let (subject, pattern) = match literal_pattern(compiler, *arg) {
                Some(p) => (recv, p),
                None => (*arg, literal_pattern(compiler, recv)?),
            };
            let subject = static_string(compiler, cref, box_id, subject, depth)?;
            pattern.matches(&subject)
        }
        // `if RUBY_VERSION.start_with?('1.9')` -- the same build-time question
        // the comparison operators above answer, asked by prefix. Ruby takes any
        // number of candidates and is true if ANY matches; a Regexp candidate
        // (also legal) reduces to no string, so the whole probe stays `None`.
        "start_with?" | "end_with?" => {
            let s = static_string(compiler, cref, box_id, receiver?, depth)?;
            let mut hit = false;
            for arg in args {
                let ArrayElem::Single(a) = arg else {
                    return None;
                };
                let candidate = static_string(compiler, cref, box_id, *a, depth)?;
                hit |= if name == "start_with?" {
                    s.starts_with(&candidate)
                } else {
                    s.ends_with(&candidate)
                };
            }
            Some(hit)
        }
        // `if RUBY_PLATFORM['linux']` -- `String#[]` hands back the match or
        // nil, so AS A CONDITION it is a containment test. Only reached when
        // both sides reduce to strings, so an Array or Hash `[]` never lands
        // here.
        "[]" => {
            let [ArrayElem::Single(a)] = args else {
                return None;
            };
            let s = static_string(compiler, cref, box_id, receiver?, depth)?;
            let part = static_string(compiler, cref, box_id, *a, depth)?;
            Some(s.contains(&part))
        }
        "include?" => include_fold(compiler, cref, box_id, receiver?, args, depth),
        "respond_to?" => respond_to_fold(compiler, cref, box_id, receiver, args),
        "const_defined?" => const_defined_fold(compiler, cref, box_id, receiver, args),
        "method_defined?" | "public_method_defined?" => {
            method_defined_fold(compiler, cref, box_id, receiver, args)
        }
        _ => predicate_fold(compiler, cref, box_id, receiver, name, args, depth),
    }
}

/// The class or module a CONSTANT-PATH receiver names: a bare `Process`
/// (`ClassRef`) or a top-anchored `::Process`, which reads as
/// `QualifiedConstRead("Object", "Process")` because a top-level constant lives
/// on `Object`. Both resolve at the root scope; everything else resolves at the
/// guard's own cref. `None` for any receiver that isn't a constant.
fn const_receiver_class(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: NodeId,
) -> Option<ClassId> {
    match &compiler.hir[receiver] {
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
    }
}

/// Whether a method takes nothing at all, so a bare `Recv.name` runs its body
/// with no argument to have changed the answer.
fn takes_no_arguments(params: &Params) -> bool {
    params.required.is_empty()
        && params.destructures.is_empty()
        && params.optional.is_empty()
        && params.rest.is_none()
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && params.block.is_none()
}

/// Every definition of `name` that a `Recv.name` on this module could reach and
/// that this file can read: the module's own class methods (`def self.name`),
/// its own instance methods, and those of the modules it extends.
///
/// Instance methods belong here because `extend self` and `module_function` are
/// the usual way a module makes its predicates callable on itself, and both
/// stay runtime calls in zeo -- the method lands in `own_methods` and nothing
/// static ever moves it. Rather than model the singleton ancestry to work out
/// which definition wins, [`predicate_fold`] reads them all and insists they
/// agree, which answers the question without needing to know.
fn predicate_definitions(compiler: &Compiler, owner: ClassId, name: &str) -> Vec<ScopeId> {
    let info = compiler.class(owner);
    let mut out: Vec<ScopeId> = Vec::new();
    let tables = info
        .own_class_methods
        .iter()
        .chain(&info.own_methods)
        .chain(
            info.extends
                .iter()
                .filter(|m| **m != owner)
                .flat_map(|m| &compiler.class(*m).own_methods),
        );
    for &sid in tables {
        if compiler.scope(sid).name == name && !out.contains(&sid) {
            out.push(sid);
        }
    }
    out
}

/// Whether `owner` is the only class in the program that defines `name`.
///
/// A receiverless call dispatches on the live `self`, which may be an instance
/// of a subclass or an includer rather than of `owner` itself. Reading only
/// `owner`'s definition would then answer for the wrong body. With no other
/// definition of the name anywhere, there is no other body to reach.
///
/// Deliberately not gated on [`Compiler::may_be_patched_at_runtime`]: that
/// answers a different question (might this name be REWRITTEN at run time),
/// which no predicate fold models -- one computed `define_method` anywhere sets
/// it program-wide, and every real gem has one. The one runtime-installed shape
/// that WOULD change the answer here is a conditional `def`, and
/// [`predicate_fold`] declines on that directly.
fn defines_name_alone(compiler: &Compiler, owner: ClassId, name: &str) -> bool {
    compiler.classes.iter().enumerate().all(|(i, info)| {
        i == owner.0 as usize
            || !info
                .own_methods
                .iter()
                .chain(&info.own_class_methods)
                .any(|&sid| compiler.scope(sid).name == name)
    })
}

/// Compile-time value of a zero-argument predicate on a module -- the shape a
/// compat gate takes once a gem gives its build-time question a name:
/// `if Sass::Util.rbx?`, `if Lutaml::Model::RuntimeCompatibility.opal?`. What
/// those predicates test is what the folders above already decide
/// (`RUBY_ENGINE == "rbx"`); naming it is the only thing that hid it.
///
/// EVERY definition the module carries has to fold, and to the SAME value --
/// see [`predicate_definitions`] for why reading all of them is what makes it
/// safe not to know which one a call reaches.
///
/// Only the VALUE folds. The method is still compiled and still callable, so
/// what a folded guard drops is one call whose whole effect was to compute a
/// constant -- including, for the memoized spelling, the caching of it (see
/// [`method_value_bool`]).
fn predicate_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    depth: u32,
) -> Option<bool> {
    if !args.is_empty() {
        return None;
    }
    let owner = match receiver {
        Some(r) => const_receiver_class(compiler, cref, box_id, r)?,
        // No receiver: `self`, which inside a method body is an instance of the
        // enclosing class -- or of something below it that may have overridden
        // the name. `defines_name_alone` is what rules that out, and sass needs
        // it: `ruby1_8?` opens with a bare `ironruby?`.
        None => {
            let owner = *cref.last()?;
            if !defines_name_alone(compiler, owner, name) {
                return None;
            }
            owner
        }
    };
    let definitions = predicate_definitions(compiler, owner, name);
    if definitions.is_empty() {
        return None;
    }
    let mut answer: Option<bool> = None;
    for sid in definitions {
        let scope = compiler.scope(sid);
        // A `def` under a guard zeo could not decide is registered but not
        // promised (`analyze::register_conditional_defs`), so its body is not
        // the answer -- whether it is installed at all is the same undecided
        // question that put it there.
        if !takes_no_arguments(&scope.params) || scope.runtime_conditional {
            return None;
        }
        // Folded where the method was WRITTEN: a bare constant in its body
        // resolves in its own lexical scope, not at the guard's.
        let home = compiler.cref_of(Some(scope.defining_class));
        let value = method_value_bool(compiler, &home, box_id, &scope.body, depth)?;
        if answer.is_some_and(|a| a != value) {
            return None;
        }
        answer = Some(value);
    }
    answer
}

/// The value a zero-argument method body produces on EVERY call, when that is
/// one of the decidable forms.
///
/// Either the body is a single expression that folds, or it is the memoized
/// spelling gems write a build-time predicate in:
///
/// ```ruby
/// def rbx?
///   return @rbx if defined?(@rbx)
///   @rbx = RUBY_ENGINE == "rbx"
/// end
/// ```
///
/// A memo over a constant expression answers that constant on every call, first
/// or later, so the guard reads the same either way -- and the cached ivar is
/// unobservable outside the memo that wrote it, which is why a guard that folds
/// may skip writing it.
fn method_value_bool(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    body: &[NodeId],
    depth: u32,
) -> Option<bool> {
    let (&last, leading) = body.split_last()?;
    let memo = match leading {
        [] => None,
        [guard] => Some(memo_guard_ivar(compiler, *guard)?),
        _ => return None,
    };
    let value = match (&compiler.hir[last], memo) {
        // An assignment's value is the value assigned. Allowed only for the
        // ivar the memo guard read -- that pairing is what makes the write the
        // cache rather than an effect.
        (HirNode::IvarWrite(n, v), Some(memo)) if n == memo => *v,
        (_, None) => last,
        _ => return None,
    };
    static_bool(compiler, cref, box_id, value, depth).or_else(|| literal_truth(compiler, value))
}

/// The ivar a `return @x if defined?(@x)` memo guard reads, or `None` for any
/// other statement.
fn memo_guard_ivar(compiler: &Compiler, stmt: NodeId) -> Option<&str> {
    let HirNode::If {
        cond,
        then_body,
        else_body,
    } = &compiler.hir[stmt]
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let HirNode::Defined(probe) = &compiler.hir[*cond] else {
        return None;
    };
    let HirNode::IvarRead(probed) = &compiler.hir[*probe] else {
        return None;
    };
    let [returned] = then_body.as_slice() else {
        return None;
    };
    let HirNode::Return(Some(value)) = &compiler.hir[*returned] else {
        return None;
    };
    let HirNode::IvarRead(read) = &compiler.hir[*value] else {
        return None;
    };
    (read == probed).then_some(probed.as_str())
}

/// Compile-time truth of `defined?(Const)` -- the OTHER whole-definition gate
/// rubygems and bundler are written in (`if defined? ::Psych::Visitors`,
/// `unless defined?(Gem::Timeout)`). A name that resolves to a compiled
/// class/module is `Some(true)`; one the whole program never defines -- not a
/// `class`/`module` anywhere (`shell_kinds`), never the target of a `NAME = ...`
/// (`assigned_const_names`) -- is `Some(false)`, which is what lets a branch
/// depending on a capability zeo doesn't have drop out before it reaches Rust.
///
/// Anything in between stays `None`. Both tables are whole-program sweeps taken
/// BEFORE registration, so the answer doesn't depend on how far the walk has
/// got -- a class defined later in the flattened require graph is still seen.
fn defined_const_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
) -> Option<bool> {
    let joined = match &compiler.hir[node] {
        HirNode::ClassRef(name) => name.clone(),
        HirNode::QualifiedConstRead(scope, name) => format!("{scope}::{name}"),
        _ => return None,
    };
    // A constant startup installs on `Object` is defined before the program's
    // first line, and `defined?` reaches `Object` from every lexical scope. The
    // top-anchored spellings answer the same way; a real scope
    // (`defined?(K::ENV)`) does not, because the scope operator excludes
    // `Object`'s constants.
    let top_level = joined
        .strip_prefix("Object::")
        .or_else(|| joined.strip_prefix("::"))
        .unwrap_or(&joined);
    if zeo_abi::SEEDED_OBJECT_CONSTANTS.contains(&top_level) && !top_level.contains("::") {
        return Some(true);
    }
    const_name_fold(compiler, cref, box_id, &joined)
}

/// `Module#const_defined?(:X)` -- `defined?(X)`'s reflective twin, and the shape
/// guard-compat's `unless Object.const_defined?('Guard')` is written in. The
/// name arrives as a literal argument rather than as a constant read, so it
/// shares [`defined_const_fold`]'s decision but not its node matching.
///
/// A receiver narrows the scope: `Object.const_defined?` asks at the root, a
/// bare call asks in the enclosing lexical scope. Any other receiver stays
/// `None` -- resolving `Foo.const_defined?` needs Foo's own namespace, which is
/// a different question from the one this answers.
fn const_defined_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    args: &[ArrayElem],
) -> Option<bool> {
    let name = probe_name(compiler, args)?;
    let scope: &[ClassId] = match receiver {
        None => cref,
        Some(r) => match &compiler.hir[r] {
            HirNode::ClassRef(n) if n == "Object" || n == "::Object" => &[],
            _ => return None,
        },
    };
    const_name_fold(compiler, scope, box_id, &name)
}

/// The shared decision behind `defined?(X)` and `const_defined?(:X)`.
fn const_name_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    joined: &str,
) -> Option<bool> {
    if compiler.resolve_class(joined, cref, box_id).is_some() {
        return Some(true);
    }
    // A name startup installs on `Object` is real, but whether THIS question
    // reaches `Object` depends on the asker: `defined?` always does (and
    // answers before this, in `defined_const_fold`), while
    // `Module#const_defined?` reaches it from a class and not from a bare
    // module. Undecided rather than false, which is what it would otherwise
    // read as.
    if zeo_abi::SEEDED_OBJECT_CONSTANTS.contains(&joined) {
        return None;
    }
    let path = crate::constpath::ConstPath::parse(joined);
    // A definition the walk hasn't reached yet, or one written under a scope
    // this reference spells differently (`Psych::Visitors` from inside
    // `module Psych`) -- undecidable, so let the guard run.
    let suffix = format!("::{}", path.unanchored());
    let defined_somewhere = compiler
        .shell_kinds
        .keys()
        .any(|(bx, k)| *bx == box_id && (*k == *path.unanchored() || k.ends_with(&suffix)));
    if defined_somewhere || compiler.assigned_const_names.contains(path.base()) {
        return None;
    }
    Some(false)
}

/// The value of a `begin; require "x"; <value>; rescue LoadError; <value>; end`
/// as a boolean, or `None` when the shape is anything else.
///
/// Every statement but the last must be a require -- either one the loader
/// resolved (already a `BoolLit`) or one it could not (still a call, and the
/// one that raises). Anything else in there could raise for its own reasons,
/// or have effects the fold would silently reorder, so the shape is checked
/// rather than assumed.
fn rescued_require_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    body: &[NodeId],
    rescues: &[crate::hir::RescueClause],
    depth: u32,
) -> Option<bool> {
    let (&value, leading) = body.split_last()?;
    let mut raises = false;
    for &stmt in leading {
        match &compiler.hir[stmt] {
            HirNode::BoolLit(_) => {}
            HirNode::Call { name, args, .. } if name == "require" => {
                let [ArrayElem::Single(arg)] = args.as_slice() else {
                    return None;
                };
                let feature = string_lit(compiler, *arg)?;
                if !compiler.hir.unresolvable_requires.contains(&feature) {
                    return None;
                }
                raises = true;
            }
            _ => return None,
        }
    }
    if !raises {
        return static_bool(compiler, cref, box_id, value, depth)
            .or_else(|| literal_truth(compiler, value));
    }
    // The raise happened, so the answer is the first clause that catches
    // `LoadError`. A splatted class list is a runtime question -- decline.
    let clause = rescues.iter().find(|r| {
        r.splats.is_empty()
            && (r.classes.is_empty()
                || r.classes
                    .iter()
                    .any(|c| c == "LoadError" || c == "::LoadError" || c == "StandardError"))
    })?;
    if rescues.iter().any(|r| !r.splats.is_empty()) {
        return None;
    }
    // An empty rescue body is `nil`, which is exactly what the idiom leans on.
    match clause.body.last() {
        Some(&v) => {
            static_bool(compiler, cref, box_id, v, depth).or_else(|| literal_truth(compiler, v))
        }
        None => Some(false),
    }
}

/// Ruby truthiness of a LITERAL -- everything but `false` and `nil` is true.
///
/// Deliberately not part of `static_bool`: a literal written directly as a
/// condition has its own if-expression-aware codegen path, and folding it there
/// drops the leading statements of the branch. Here the literal is a stored
/// VALUE one step removed from the condition, so reading it is just reading it.
fn literal_truth(compiler: &Compiler, node: NodeId) -> Option<bool> {
    match &compiler.hir[node] {
        HirNode::BoolLit(b) => Some(*b),
        HirNode::NilLit => Some(false),
        HirNode::IntegerLit(_)
        | HirNode::FloatLit(_)
        | HirNode::StringLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::ArrayLit(_)
        | HirNode::HashLit(_) => Some(true),
        _ => None,
    }
}

/// Compile-time truth of a guard expression, or `None` when it isn't one of the
/// decidable target-constant forms (leave the condition to run normally).
fn static_bool(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    node: NodeId,
    depth: u32,
) -> Option<bool> {
    if depth >= MAX_FOLD_DEPTH {
        return None;
    }
    let depth = depth + 1;
    match &compiler.hir[node] {
        HirNode::Defined(inner) => defined_const_fold(compiler, cref, box_id, *inner),
        // A plain `true`/`false` literal is deliberately NOT folded here: literal
        // conditions have their own (if-expression-aware) codegen path, and
        // hijacking it drops leading side-effect statements from a folded branch.
        HirNode::And(l, r) => match static_bool(compiler, cref, box_id, *l, depth) {
            Some(false) => Some(false),
            Some(true) => static_bool(compiler, cref, box_id, *r, depth),
            // `<undecidable> && false` is falsy whichever way the left goes --
            // `Ruby.mri? && RUBY_VERSION[0, 3] == '1.9'` (rspec-expectations'
            // 1.9-only append_features) folds on the version half alone. Same
            // pure-guard posture every fold here takes: the left's evaluation
            // is dropped with the branch.
            None => match static_bool(compiler, cref, box_id, *r, depth) {
                Some(false) => Some(false),
                _ => None,
            },
        },
        HirNode::Or(l, r) => match static_bool(compiler, cref, box_id, *l, depth) {
            Some(true) => Some(true),
            Some(false) => static_bool(compiler, cref, box_id, *r, depth),
            // The mirror of `And`'s undecidable-left arm: `<undecidable> ||
            // true` is truthy whichever way the left goes.
            None => match static_bool(compiler, cref, box_id, *r, depth) {
                Some(true) => Some(true),
                _ => None,
            },
        },
        // A boolean value constant (`VALIDATES_FOR_RESOLUTION`) folds through
        // its own initializer, evaluated in the owning class's lexical scope.
        HirNode::ClassRef(name) if !name.contains("::") => {
            let (owner, value) = const_init(compiler, cref, box_id, None, name)?;
            static_bool(
                compiler,
                &compiler.cref_of(Some(owner)),
                box_id,
                value,
                depth,
            )
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let (owner, value) = const_init(compiler, cref, box_id, Some(scope), name)?;
            static_bool(
                compiler,
                &compiler.cref_of(Some(owner)),
                box_id,
                value,
                depth,
            )
        }
        // `CONST = begin; require "x"; true; rescue LoadError; end` -- the
        // have-I-got-this-library idiom, and its value is decided at compile
        // time even though the constant reads like a runtime one. A `require`
        // the loader RESOLVED is already a `BoolLit` by now; one it could not
        // is still a call, and that call raises `LoadError`. hexapdf gates a
        // whole file on `HARFBUZZ_AVAILABLE` this way.
        HirNode::Begin {
            body,
            rescues,
            else_body: None,
            ensure_body: None,
        } => rescued_require_fold(compiler, cref, box_id, body, rescues, depth),
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            ..
        } if kwargs.is_empty() && block.is_none() => {
            call_fold(compiler, cref, box_id, *receiver, name, args, depth)
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
    static_bool(compiler, cref, box_id, cond, 0)
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

    /// The fold decides which branch of a program is COMPILED, so a pattern it
    /// reads wrongly picks the wrong half. Every expectation checked against
    /// ruby 4.0.6.
    #[test]
    fn an_alternative_is_the_positions_it_spells() {
        let elems = |src| literal_alternative(src).map(|a| a.0);
        // `\.` is an escape that means a real dot; a bare `.` is any character.
        assert_eq!(
            elems(r"1\.8"),
            Some(vec![Elem::Lit('1'), Elem::Lit('.'), Elem::Lit('8')])
        );
        assert_eq!(
            elems("1.8"),
            Some(vec![Elem::Lit('1'), Elem::AnyChar, Elem::Lit('8')])
        );
        assert_eq!(
            elems("mingw"),
            Some("mingw".chars().map(Elem::Lit).collect::<Vec<_>>())
        );
        // Anything with regexp meaning this cannot spell exactly must decline.
        for src in [
            "a+",    // repetition
            ".*",    // a quantified `.` is any NUMBER of characters
            "[0-9]", // a class
            "(a)",   // a group
            r"\d",   // an escape that is not `\.`
            r"a\",   // a dangling backslash
            "",      // nothing to match
        ] {
            assert!(elems(src).is_none(), "{src:?} is not spellable");
        }
    }

    #[test]
    fn an_anchored_pattern_matches_by_position() {
        let p = |anchored_start, anchored_end, alts: &[&str]| LiteralPattern {
            anchored_start,
            anchored_end,
            ignore_case: false,
            alts: alts
                .iter()
                .map(|s| literal_alternative(s).unwrap())
                .collect(),
        };
        // `RUBY_VERSION =~ /^1\.8/` on 4.0.6 -- the ipaddress guard.
        assert_eq!(p(true, false, &[r"1\.8"]).matches("4.0.6"), Some(false));
        assert_eq!(p(true, false, &[r"1\.8"]).matches("1.8.7"), Some(true));
        // Unanchored is a substring test, which is how the platform gates read.
        assert_eq!(
            p(false, false, &["mingw", "mswin"]).matches("x64-mingw32"),
            Some(true)
        );
        assert_eq!(
            p(false, false, &["mingw", "mswin"]).matches("arm64-darwin24"),
            Some(false)
        );
        // A `$` anchor is a suffix, and both anchors together are equality.
        assert_eq!(
            p(false, true, &["darwin24"]).matches("arm64-darwin24"),
            Some(true)
        );
        assert_eq!(p(true, true, &["ruby"]).matches("ruby3"), Some(false));
        assert_eq!(p(true, true, &["ruby"]).matches("ruby"), Some(true));
    }

    /// `.` is ONE character, wherever it sits. gmp's `unless RUBY_VERSION =~
    /// /^1.8/` is the shape, and it reads the same as `/^1\.8/` on every real
    /// version string -- but not on one where the dot is something else, which
    /// is why it is matched rather than assumed.
    #[test]
    fn any_char_consumes_exactly_one_character() {
        let p = |anchored_start, src: &str| LiteralPattern {
            anchored_start,
            anchored_end: false,
            ignore_case: false,
            alts: vec![literal_alternative(src).unwrap()],
        };
        assert_eq!(p(true, "1.8").matches("1.8.7"), Some(true));
        assert_eq!(p(true, "1.8").matches("108"), Some(true));
        assert_eq!(p(true, "1.8").matches("4.0.6"), Some(false));
        // One character, so a subject one short cannot match.
        assert_eq!(p(true, "1.8").matches("18"), Some(false));
        // `.` never matches a newline, even unanchored.
        assert_eq!(p(false, "a.b").matches("a\nb"), Some(false));
        assert_eq!(p(false, "a.b").matches("xaybz"), Some(true));
    }

    /// `/i` is ASCII folding here. ruby's is the full Unicode case table, so a
    /// subject that is not ASCII gets no answer rather than the narrower one.
    #[test]
    fn ignore_case_answers_only_for_ascii() {
        let p = |src: &str| LiteralPattern {
            anchored_start: false,
            anchored_end: false,
            ignore_case: true,
            alts: vec![literal_alternative(src).unwrap()],
        };
        // `RUBY_PLATFORM =~ /java/i`, the shape jruby gates on.
        assert_eq!(p("java").matches("Java-1.7"), Some(true));
        assert_eq!(p("JAVA").matches("x86_64-java"), Some(true));
        assert_eq!(p("java").matches("arm64-darwin24"), Some(false));
        assert_eq!(p("java").matches("\u{212a}elvin"), None);
    }

    /// Every case checked against `String#to_i` under ruby 4.0.6.
    #[test]
    fn leading_integer_matches_string_to_i() {
        for (s, want) in [
            ("12.0.0", 12),
            ("8", 8),
            ("  -42abc", -42),
            ("+7", 7),
            // A single `_` between digits is part of the number; a second one
            // ends it.
            ("1_2", 12),
            ("1__2", 1),
            ("007", 7),
            ("3.9", 3),
            // No leading digits at all is zero, which is what ruby answers.
            ("x9", 0),
            ("", 0),
        ] {
            assert_eq!(leading_integer(s), Some(want), "{s:?}.to_i");
        }
        // A value no `i64` holds stays undecided rather than wrapping: ruby
        // would compare it exactly.
        assert_eq!(leading_integer(&"9".repeat(30)), None);
    }

    /// Every case checked against ruby 4.0.6.
    #[test]
    fn split_string_matches_string_split() {
        for (subject, sep, want) in [
            ("4.0.6", ".", vec!["4", "0", "6"]),
            // Trailing empty fields go; a LEADING one stays.
            ("a,b,,c,,", ",", vec!["a", "b", "", "c"]),
            (".a.", ".", vec!["", "a"]),
            ("", ".", vec![]),
            ("a", ".", vec!["a"]),
            // An empty separator splits into characters.
            ("abc", "", vec!["a", "b", "c"]),
            // A single space is the awk split: leading whitespace starts no
            // empty field, and a run of it separates once.
            ("  a  b ", " ", vec!["a", "b"]),
            ("a\tb", " ", vec!["a", "b"]),
            // ASCII whitespace only -- a no-break space is an ordinary
            // character to it, which is why the field keeps both of these.
            ("\u{a0}a\u{a0}b", " ", vec!["\u{a0}a\u{a0}b"]),
        ] {
            assert_eq!(
                split_string(subject, sep),
                want,
                "{subject:?}.split({sep:?})"
            );
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
