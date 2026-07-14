//! `RubyValue` -- the boxed/poly representation every ivar, method argument,
//! and method return uses in the spike (see the plan's stated scope-cut:
//! native-unboxed `i64` is used only for literal `Int` arithmetic in codegen,
//! everything else is `RubyValue` uniformly for now). Mirrors spinel's boxed
//! `sp_RbVal` tagged union (`lib/sp_gc.h:42`), but as a real Rust `enum`
//! instead of a hand-written `{ tag; cls_id; union { ... } }` struct.

use crate::collections::{RArray, RHash, RStr};
use crate::{RObj, RProc, Symbol};

#[derive(Clone)]
pub enum RubyValue {
    Nil,
    Bool(bool),
    Int(i64),
    Symbol(Symbol),
    Str(RStr),
    Array(RArray),
    Hash(RHash),
    /// `a..b` / `a...b` -- either endpoint may be absent (a beginless/endless
    /// range). Unlike `Array`/`Hash`/`Str`, a `Range` is immutable in Ruby
    /// (no in-place mutation methods exist), so plain `Box` value semantics
    /// are enough -- no `Rc<RefCell<_>>` sharing needed.
    Range(Option<Box<RubyValue>>, Option<Box<RubyValue>>, bool),
    Object(RObj),
    /// A real, escaping block/`Proc` (Phase 6) -- see `rproc`'s module docs.
    Proc(RProc),
}

// Hand-written rather than `#[derive(Debug)]`: `Object`'s payload is
// `Rc<dyn RubyObject>`, which isn't `Debug` (that would require
// `RubyObject: Debug` as a supertrait). Only needed so `Signal` -- which
// carries a `RubyValue` in most variants -- can itself derive `Debug`, which
// `Result::unwrap`'s `E: Debug` bound requires in tests.
impl std::fmt::Debug for RubyValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_display_string())
    }
}

impl RubyValue {
    /// Mirrors `sp_*_to_s`/CRuby's `Kernel#puts` argument stringification.
    pub fn to_display_string(&self) -> String {
        match self {
            RubyValue::Nil => String::new(),
            RubyValue::Bool(b) => b.to_string(),
            RubyValue::Int(i) => i.to_string(),
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => s.borrow().clone(),
            // `puts` on an `Array` recursively flattens and prints each
            // element on its own line (not `[1, 2, 3]`, which is `inspect`'s
            // job, not `to_s`'s) -- real, verified CRuby behavior, not a
            // simplification.
            RubyValue::Array(a) => a
                .borrow()
                .iter()
                .map(RubyValue::to_display_string)
                .collect::<Vec<_>>()
                .join("\n"),
            // An approximation of `Hash#inspect` (symbol keys as `key:
            // value`, everything else as `key => value`) -- good enough for
            // the `Int`/`Symbol`-keyed hashes the spike's examples use, but
            // NOT a faithful `inspect` for nested `String`s (no quoting).
            // Same posture as `Object`'s "#<Object>" placeholder above: a
            // documented simplification, not silent wrongness.
            RubyValue::Hash(h) => {
                let body = h
                    .borrow()
                    .iter()
                    .map(|(k, v)| match k {
                        RubyValue::Symbol(s) => format!("{}: {}", s.name(), v.to_display_string()),
                        _ => format!("{} => {}", k.to_display_string(), v.to_display_string()),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{body}}}")
            }
            RubyValue::Range(start, end, exclusive) => {
                let s = start.as_ref().map(|b| b.to_display_string()).unwrap_or_default();
                let e = end.as_ref().map(|b| b.to_display_string()).unwrap_or_default();
                let op = if *exclusive { "..." } else { ".." };
                format!("{s}{op}{e}")
            }
            RubyValue::Object(_) => "#<Object>".to_string(),
            RubyValue::Proc(_) => "#<Proc>".to_string(),
        }
    }

    /// Unwraps a `Symbol` payload -- used by codegen wherever a Ruby
    /// expression that's *typed* as producing a Symbol (e.g. the argument to
    /// `send`) needs to become the plain `Symbol` `send`'s own signature
    /// expects. Panics (not silently-wrong) if the value isn't actually a
    /// Symbol at runtime -- the spike has no static type-checker to catch
    /// this earlier (see the plan's scope-cut).
    pub fn as_symbol_unchecked(&self) -> Symbol {
        match self {
            RubyValue::Symbol(s) => *s,
            other => panic!("expected a Symbol, got {}", other.to_display_string()),
        }
    }

    /// Unwraps an `Int` payload -- the runtime counterpart to codegen's
    /// static `TyKind::Int` check (`analyze::locals`/`types::infer_type_with_locals`):
    /// once codegen has proven an operand is statically `Int`, this recovers
    /// the native `i64` to hand to `spinel_rt::int_*`. Panics (not
    /// silently-wrong) if that proof was ever unsound -- it shouldn't be
    /// reachable, but this isn't a real type-checker (see the plan's
    /// scope-cut), so a loud failure beats a silent one.
    pub fn as_int_unchecked(&self) -> i64 {
        match self {
            RubyValue::Int(i) => *i,
            other => panic!("expected an Int, got {}", other.to_display_string()),
        }
    }

    /// Ruby truthiness: everything is truthy except `nil` and `false` --
    /// unlike Rust's `bool`, arbitrary `RubyValue`s (including `Int(0)` and
    /// `Str("")`) are truthy. Backs `&&`/`||`/`!`/`if` (see `codegen::expr`).
    pub fn truthy(&self) -> bool {
        !matches!(self, RubyValue::Nil | RubyValue::Bool(false))
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, RubyValue::Nil)
    }

    /// Unwraps an `Array` payload -- the runtime counterpart to codegen's
    /// static `TyKind::Array` check, same posture as `as_int_unchecked`.
    pub fn as_array_unchecked(&self) -> RArray {
        match self {
            RubyValue::Array(a) => a.clone(),
            other => panic!("expected an Array, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Hash` payload -- see `as_array_unchecked`'s docs.
    pub fn as_hash_unchecked(&self) -> RHash {
        match self {
            RubyValue::Hash(h) => h.clone(),
            other => panic!("expected a Hash, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Str` payload -- see `as_array_unchecked`'s docs.
    pub fn as_str_unchecked(&self) -> RStr {
        match self {
            RubyValue::Str(s) => s.clone(),
            other => panic!("expected a String, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Proc` payload -- see `as_array_unchecked`'s docs.
    pub fn as_proc_unchecked(&self) -> RProc {
        match self {
            RubyValue::Proc(p) => p.clone(),
            other => panic!("expected a Proc, got {}", other.to_display_string()),
        }
    }

    /// Unwraps an `Object` payload -- see `as_array_unchecked`'s docs. Used
    /// wherever a runtime `class_id()` is needed off a POLY-typed value
    /// (a dynamic `is_a?`/`kind_of?` check, or -- once `raise`/`rescue`
    /// exist -- matching a raised exception's class against a `rescue`
    /// clause) rather than the statically-known-class fast path, which
    /// constant-folds instead of calling this at all.
    pub fn as_object_unchecked(&self) -> RObj {
        match self {
            RubyValue::Object(o) => o.clone(),
            other => panic!("expected an Object, got {}", other.to_display_string()),
        }
    }

    /// `Range#first` -- `nil` for a beginless range (`..5`).
    pub fn range_first(&self) -> RubyValue {
        match self {
            RubyValue::Range(start, ..) => start.as_deref().cloned().unwrap_or(RubyValue::Nil),
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// `Range#last` -- `nil` for an endless range (`1..`).
    pub fn range_last(&self) -> RubyValue {
        match self {
            RubyValue::Range(_, end, _) => end.as_deref().cloned().unwrap_or(RubyValue::Nil),
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// `Range#exclude_end?`.
    pub fn range_exclude_end(&self) -> bool {
        match self {
            RubyValue::Range(_, _, exclusive) => *exclusive,
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// Structural value equality for the primitive variants -- backs
    /// `case`/`when`'s value-matching desugar (`val === subject`, which for
    /// every value shape `case/when` currently supports -- `Int`/`Symbol`
    /// literals -- means the same thing as `==`) and `Hash`'s key lookup
    /// (`collections::hash_get`/`hash_set`). Deliberately NOT wired into the
    /// general `==`/`!=` operator table (`codegen::call`, still scoped to
    /// statically-known `Int` operands): this is a narrower escape hatch,
    /// not a general `Object#==`. `Array`/`Hash`/`Range`/`Object` values (or
    /// a mismatched-variant pair) conservatively compare unequal rather than
    /// panicking or recursing -- element-wise `Array`/`Hash` equality and
    /// identity/`==` dispatch on arbitrary objects are documented gaps, not
    /// silent wrongness, until there's a real use for them.
    pub fn rb_eq(&self, other: &RubyValue) -> bool {
        match (self, other) {
            (RubyValue::Nil, RubyValue::Nil) => true,
            (RubyValue::Bool(a), RubyValue::Bool(b)) => a == b,
            (RubyValue::Int(a), RubyValue::Int(b)) => a == b,
            (RubyValue::Symbol(a), RubyValue::Symbol(b)) => a == b,
            (RubyValue::Str(a), RubyValue::Str(b)) => *a.borrow() == *b.borrow(),
            _ => false,
        }
    }
}
