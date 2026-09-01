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
//! `clif::stmt::static_cond` (what an `if` EMITS), so registration and
//! emission always pick the same branch. Callers pass the lexical `cref` at the
//! guard site (empty at the top level) so a bare `Specification`/const resolves
//! in its enclosing namespace, exactly as it would at that source position.

mod method_fold;
mod pattern;
mod platform;
mod version;

pub(crate) use pattern::LiteralPattern;
pub(crate) use platform::{
    ffi_platform_integer, ffi_platform_predicate, ffi_platform_string, rbconfig_string,
    win_platform,
};

use crate::compiler::{ClassId, Compiler};
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use method_fold::{call_fold, probe_name};
use platform::{ffi_platform_leaf, ffi_platform_leaf_integer};

/// How far a fold may chase one guard through the definitions behind it -- a
/// constant's initializer, a predicate method's body, another constant inside
/// that. Deep enough that no real guard reaches it, and the reason it exists at
/// all is that the chase can CYCLE: `A = B` beside `B = A`, or a predicate that
/// calls itself, would otherwise recur until the stack ran out.
const MAX_FOLD_DEPTH: u32 = 32;

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
    // branch of a rescue) have no single initializer to read -- which is
    // exactly what `unique_top_const_inits` records, collected in the single
    // arena sweep `analyze` already makes.
    if scope.is_some() {
        return None;
    }
    compiler
        .unique_top_const_inits
        .get(name)
        .map(|&value| (crate::compiler::OBJECT_CLASS, value))
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
/// Shared with the LOWER-stage class-body guard (`lower::defs::static_guard`),
/// which answers the same questions over prism nodes before any HIR exists.
pub(crate) fn seeded_string_const(name: &str) -> Option<String> {
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
            const_string(compiler, cref, box_id, name).or_else(|| ffi_platform_leaf(name))
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
                None => ffi_platform_leaf(&format!("{scope}::{name}")),
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
        // `RbConfig::CONFIG['host_os']` -- the older spelling of the platform
        // question, read off the same values `build.rs` renders into the
        // program's own `RbConfig` shim.
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if matches!(name.as_str(), "[]" | "fetch")
            && matches!(&compiler.hir[*r],
                HirNode::QualifiedConstRead(scope, n) if scope == "RbConfig" && n == "CONFIG") =>
        {
            let [ArrayElem::Single(key)] = args[..] else {
                return None;
            };
            rbconfig_string(&string_lit(compiler, key)?).map(str::to_string)
        }
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
            let [
                crate::hir::ArrayElem::Single(a0),
                crate::hir::ArrayElem::Single(a1),
            ] = args[..]
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
            Some(
                chars[start..(start + len).min(chars.len())]
                    .iter()
                    .collect(),
            )
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
            const_init(compiler, cref, box_id, scope, base)
                .and_then(|(owner, value)| through(owner, value))
                .or_else(|| ffi_platform_leaf_integer(name))
        }
        HirNode::QualifiedConstRead(scope, name) => {
            const_init(compiler, cref, box_id, Some(scope), name)
                .and_then(|(owner, value)| through(owner, value))
                .or_else(|| ffi_platform_leaf_integer(&format!("{scope}::{name}")))
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

/// `String#to_f`'s reading: the longest leading float, `0.0` when there is
/// none at all. Underscore separators follow `to_i`'s rule (single, and
/// flanked by digits). A real exponent (`"1e3"`) would change the value, so
/// the fold declines it rather than mirror one more grammar corner; a bare
/// trailing `e` is ignored exactly as ruby ignores it.
fn leading_float(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.trim_start().chars().collect();
    let mut i = 0;
    let mut out = String::new();
    if matches!(chars.first(), Some('+' | '-')) {
        if chars[0] == '-' {
            out.push('-');
        }
        i += 1;
    }
    let digits = |out: &mut String, i: &mut usize| {
        let mut any = false;
        while let Some(&c) = chars.get(*i) {
            if c.is_ascii_digit() {
                out.push(c);
                any = true;
                *i += 1;
            } else if c == '_' && any && chars.get(*i + 1).is_some_and(char::is_ascii_digit) {
                *i += 1;
            } else {
                break;
            }
        }
        any
    };
    let int_part = digits(&mut out, &mut i);
    let mut frac = false;
    if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
        out.push('.');
        i += 1;
        frac = digits(&mut out, &mut i);
    }
    if !int_part && !frac {
        return Some(0.0);
    }
    if matches!(chars.get(i), Some('e' | 'E')) {
        let at_digit = i + 1 + usize::from(matches!(chars.get(i + 1), Some('+' | '-')));
        if chars.get(at_digit).is_some_and(char::is_ascii_digit) {
            return None;
        }
    }
    out.parse().ok()
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
    // A top-anchored CLASS spelling (`::Mutex`, `Object::Mutex`): the joined
    // path "Object::Mutex" is not a registered name, but the stripped one is
    // -- resolve it top-anchored (empty cref). rspec-support gates its whole
    // Mutex strategy on `defined? ::Mutex`, and the confident false here
    // spliced the 1.8.7 fallback branch instead.
    if top_level != joined
        && !top_level.contains("::")
        && compiler.resolve_class(top_level, &[], box_id).is_some()
    {
        return Some(true);
    }
    const_name_fold(compiler, cref, box_id, &joined, node)
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
    // The name ARGUMENT node stands in for the probe's document position:
    // any node inside the asking statement carries the same ordering.
    let at = match args {
        [ArrayElem::Single(a), ..] => *a,
        _ => return None,
    };
    const_name_fold(compiler, scope, box_id, &name, at)
}

/// The shared decision behind `defined?(X)` and `const_defined?(:X)`.
fn const_name_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    joined: &str,
    at: NodeId,
) -> Option<bool> {
    if let Some(cid) = compiler.resolve_class(joined, cref, box_id) {
        // Registered but not PROMISED: whether a runtime-conditional class's
        // constant exists is settled by its guarded body having run.
        if compiler.constant_is_positional(cid) {
            return None;
        }
        // Registered, but ruby defines the constant at the marker's own
        // line: once `index_document_order` has placed the definitions, a
        // guard that runs before the leaf's first one answers nil -- the
        // rule the VALUE fold already applies. During the registration
        // walk the order table is empty and this narrows nothing; that
        // walk consults in execution order, so a later class is simply
        // not resolved yet.
        let ci = compiler.class(cid);
        let leaf = ci.name.rsplit("::").next().unwrap_or(&ci.name).to_string();
        let owner = ci
            .lexical_parent
            .unwrap_or(crate::compiler::OBJECT_CLASS);
        if compiler.const_defined_before(owner, &leaf, at) == Some(false) {
            return Some(false);
        }
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
    let defined_somewhere = compiler.class_shaped_anywhere(box_id, path.unanchored());
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
                if !compiler.hir.loader.unresolvable_requires.contains(&feature) {
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

/// A BUILTIN constant whose TRUTH is a property of the build --
/// `File::ALT_SEPARATOR` is `"\\"` on windows and `nil` everywhere else,
/// the oldest "am I on windows" test there is. The loader's cond fold
/// answers the same question at the prism level; this is the HIR-level
/// half, which is what lets a user predicate BODY (`puppet`'s
/// `Platform.windows?` is `!!File::ALT_SEPARATOR`) fold through
/// `predicate_fold`, and the whole windows-only require graph behind it
/// fold away.
fn seeded_bool_const(scope: &str, name: &str) -> Option<bool> {
    match (scope.trim_start_matches("::"), name) {
        ("File", "ALT_SEPARATOR") => win_platform(),
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
        // `File::ALT_SEPARATOR` written as one path -- same seeded truth as
        // the `QualifiedConstRead` spelling below.
        HirNode::ClassRef(name) => {
            let (scope, leaf) = name.rsplit_once("::")?;
            seeded_bool_const(scope, leaf)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            if let Some(b) = seeded_bool_const(scope, name) {
                return Some(b);
            }
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
    fn leading_float_matches_string_to_f() {
        for (s, want) in [
            ("4.0.6", 4.0),
            ("2.4", 2.4),
            ("8", 8.0),
            ("  -42.5abc", -42.5),
            ("+7", 7.0),
            ("1_2.3_4", 12.34),
            ("1__2", 1.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("x9", 0.0),
            ("", 0.0),
            ("-", 0.0),
            // A bare trailing `e` is not an exponent; ruby ignores it too.
            ("3.9e", 3.9),
        ] {
            assert_eq!(leading_float(s), Some(want), "{s:?}.to_f");
        }
        // A real exponent would change the value ("1e3" is 1000.0); the fold
        // declines it rather than mirror one more grammar corner.
        assert_eq!(leading_float("1e3"), None);
        assert_eq!(leading_float("1.5E-2"), None);
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
}
