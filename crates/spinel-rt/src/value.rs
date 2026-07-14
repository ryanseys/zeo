//! `RubyValue` -- the boxed/poly representation every ivar, method argument,
//! and method return uses in the spike (see the plan's stated scope-cut:
//! native-unboxed `i64` is used only for literal `Int` arithmetic in codegen,
//! everything else is `RubyValue` uniformly for now). Mirrors spinel's boxed
//! `sp_RbVal` tagged union (`lib/sp_gc.h:42`), but as a real Rust `enum`
//! instead of a hand-written `{ tag; cls_id; union { ... } }` struct.

use crate::{RObj, Symbol};

#[derive(Clone)]
pub enum RubyValue {
    Nil,
    Bool(bool),
    Int(i64),
    Symbol(Symbol),
    Str(String),
    Object(RObj),
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
            RubyValue::Str(s) => s.clone(),
            RubyValue::Object(_) => "#<Object>".to_string(),
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
}
