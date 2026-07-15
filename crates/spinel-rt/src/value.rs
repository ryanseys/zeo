//! `RubyValue` -- the boxed/poly representation every ivar, method argument,
//! and method return uses in the spike (see the plan's stated scope-cut:
//! native-unboxed `i64` is used only for literal `Int` arithmetic in codegen,
//! everything else is `RubyValue` uniformly for now). Mirrors spinel's boxed
//! `sp_RbVal` tagged union (`lib/sp_gc.h:42`), but as a real Rust `enum`
//! instead of a hand-written `{ tag; cls_id; union { ... } }` struct.

use crate::collections::{RArray, RHash, RStr};
use crate::dispatch::{
    ClassId, ARRAY_CLASS, FALSE_CLASS, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS,
    MATCH_DATA_CLASS, NIL_CLASS, PROC_CLASS, RANGE_CLASS, REGEXP_CLASS, STRING_CLASS,
    SYMBOL_CLASS, TRUE_CLASS,
};
use crate::fiber::RFiber;
use crate::regexp::{RMatchData, RRegexp};
use crate::{RObj, RProc, Symbol};

#[derive(Clone)]
pub enum RubyValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
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
    /// A real, `regex`-crate-backed `Regexp` (Phase 12.7) -- see
    /// `regexp`'s module docs.
    Regexp(RRegexp),
    /// A successful `Regexp#match`/`String#match` result (Phase 12.7).
    MatchData(RMatchData),
    /// A `Fiber` (Phase 13.3) -- the Send+Sync HANDLE only; the actual
    /// coroutine is thread-pinned in `fiber::FIBERS` (see that module's
    /// docs for why it can't live here).
    Fiber(RFiber),
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

/// Mirrors real Ruby's `Float#to_s`: unlike Rust's own `f64::to_string()`
/// (which omits the decimal point entirely for a whole number, e.g. `"1"`
/// for `1.0`), Ruby always shows at least one digit after the point
/// (`"1.0"`). A documented approximation, not a byte-for-byte match of
/// Ruby's own shortest-round-trip-vs-scientific-notation threshold rules --
/// same posture as this module's other narrower-than-real-Ruby formatting.
fn float_to_display_string(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() };
    }
    let s = f.to_string();
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

impl RubyValue {
    /// Mirrors `sp_*_to_s`/CRuby's `Kernel#puts` argument stringification.
    pub fn to_display_string(&self) -> String {
        match self {
            RubyValue::Nil => String::new(),
            RubyValue::Bool(b) => b.to_string(),
            RubyValue::Int(i) => i.to_string(),
            RubyValue::Float(f) => float_to_display_string(*f),
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => s.lock().clone(),
            // `puts` on an `Array` recursively flattens and prints each
            // element on its own line (not `[1, 2, 3]`, which is `inspect`'s
            // job, not `to_s`'s) -- real, verified CRuby behavior, not a
            // simplification.
            RubyValue::Array(a) => a
                .lock()
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
                    .lock()
                    .values()
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
            // `Regexp#to_s` -- real Ruby's `(?opts-negopts:body)` form (NOT
            // the `/pattern/flags` literal form, that's `#inspect`'s job --
            // see `regexp::regexp_to_s`'s docs).
            RubyValue::Regexp(re) => crate::regexp::regexp_to_s(re).to_display_string(),
            // `MatchData#to_s` -- the whole matched substring.
            RubyValue::MatchData(m) => crate::regexp::matchdata_to_s(m).to_display_string(),
            // Same placeholder posture as `Object`/`Proc` above.
            RubyValue::Fiber(_) => "#<Fiber>".to_string(),
        }
    }

    /// Mirrors `#inspect` -- the receiver-rendering half of a `FrozenError`
    /// message (`can't modify frozen Array: [1, 2, 3]`) and, eventually, a
    /// real `Kernel#p`. Distinct from `to_display_string` (`#to_s`/`puts`
    /// semantics) where the two genuinely differ in Ruby: `nil` -> `"nil"`
    /// (not empty), `Symbol` -> `:name` (leading colon), `Str` -> quoted,
    /// `Array` -> `[a, b]` (not one-element-per-line). Documented
    /// approximations, same posture as `to_display_string`'s own: `Str`
    /// quoting uses Rust's `{:?}` escaping (matches Ruby for ordinary
    /// ASCII/UTF-8 content, diverges on exotic escapes); `Object` renders as
    /// `#<Object>` with no class name/ivars (the runtime has no class-name
    /// table); and a SELF-REFERENTIAL `Array`/`Hash` (`a = [1]; a[0] = a`)
    /// self-deadlocks on its own non-reentrant `Mutex` rather than printing
    /// CRuby's recursion-guarded `[...]` -- a narrow, documented gap.
    pub fn inspect_string(&self) -> String {
        match self {
            RubyValue::Nil => "nil".to_string(),
            RubyValue::Symbol(s) => format!(":{}", s.name()),
            RubyValue::Str(s) => format!("{:?}", &*s.lock()),
            RubyValue::Array(a) => {
                let body = a
                    .lock()
                    .iter()
                    .map(RubyValue::inspect_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{body}]")
            }
            // Ruby 3.4+ `Hash#inspect` format: `{a: 1, "k" => 2}` -- symbol
            // keys as `name: value` with no braces-padding spaces.
            RubyValue::Hash(h) => {
                let body = h
                    .lock()
                    .values()
                    .map(|(k, v)| match k {
                        RubyValue::Symbol(s) => format!("{}: {}", s.name(), v.inspect_string()),
                        _ => format!("{} => {}", k.inspect_string(), v.inspect_string()),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{body}}}")
            }
            RubyValue::Range(start, end, exclusive) => {
                let s = start.as_ref().map(|b| b.inspect_string()).unwrap_or_default();
                let e = end.as_ref().map(|b| b.inspect_string()).unwrap_or_default();
                let op = if *exclusive { "..." } else { ".." };
                format!("{s}{op}{e}")
            }
            RubyValue::Regexp(re) => crate::regexp::regexp_inspect(re).to_display_string(),
            RubyValue::MatchData(_) => "#<MatchData>".to_string(),
            // `Bool`/`Int`/`Float`/`Object`/`Proc`: `#inspect` and `#to_s`
            // agree (or share the same placeholder approximation).
            other => other.to_display_string(),
        }
    }

    /// This value's runtime class -- the UNIVERSAL counterpart to
    /// `as_object_unchecked().class_id()` (which panics on anything but an
    /// `Object`): works for every variant, including the built-in primitive
    /// types, by returning their reserved, well-known `ClassId` (see
    /// `dispatch`'s `INTEGER_CLASS`/etc., mirrored on the `spinelc` side by
    /// `compiler::BUILTIN_CLASSES`). Backs `is_a?`/`kind_of?`/`respond_to?`
    /// whenever the receiver's static type isn't known at compile time
    /// (`TyKind::Poly`), so a builtin-tagged Poly value resolves correctly
    /// instead of panicking.
    pub fn class_id(&self) -> ClassId {
        match self {
            RubyValue::Nil => NIL_CLASS,
            RubyValue::Bool(true) => TRUE_CLASS,
            RubyValue::Bool(false) => FALSE_CLASS,
            RubyValue::Int(_) => INTEGER_CLASS,
            RubyValue::Float(_) => FLOAT_CLASS,
            RubyValue::Symbol(_) => SYMBOL_CLASS,
            RubyValue::Str(_) => STRING_CLASS,
            RubyValue::Array(_) => ARRAY_CLASS,
            RubyValue::Hash(_) => HASH_CLASS,
            RubyValue::Range(..) => RANGE_CLASS,
            RubyValue::Object(o) => o.class_id(),
            RubyValue::Proc(_) => PROC_CLASS,
            RubyValue::Regexp(_) => REGEXP_CLASS,
            RubyValue::MatchData(_) => MATCH_DATA_CLASS,
            RubyValue::Fiber(_) => FIBER_CLASS,
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

    /// Unwraps a `Float` payload -- same posture as `as_int_unchecked`.
    pub fn as_float_unchecked(&self) -> f64 {
        match self {
            RubyValue::Float(f) => *f,
            other => panic!("expected a Float, got {}", other.to_display_string()),
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

    /// Unwraps a `Regexp` payload -- see `as_array_unchecked`'s docs.
    pub fn as_regexp_unchecked(&self) -> RRegexp {
        match self {
            RubyValue::Regexp(r) => r.clone(),
            other => panic!("expected a Regexp, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `MatchData` payload -- see `as_array_unchecked`'s docs.
    pub fn as_matchdata_unchecked(&self) -> RMatchData {
        match self {
            RubyValue::MatchData(m) => m.clone(),
            other => panic!("expected a MatchData, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Fiber` payload -- see `as_array_unchecked`'s docs.
    pub fn as_fiber_unchecked(&self) -> RFiber {
        match self {
            RubyValue::Fiber(f) => f.clone(),
            other => panic!("expected a Fiber, got {}", other.to_display_string()),
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
            (RubyValue::Float(a), RubyValue::Float(b)) => a == b,
            // Real Ruby: `1 == 1.0` is `true` -- Int/Float compare
            // numerically across the mixed numeric tower, not just
            // same-variant pairs.
            (RubyValue::Int(a), RubyValue::Float(b)) | (RubyValue::Float(b), RubyValue::Int(a)) => {
                *a as f64 == *b
            }
            (RubyValue::Symbol(a), RubyValue::Symbol(b)) => a == b,
            (RubyValue::Str(a), RubyValue::Str(b)) => *a.lock() == *b.lock(),
            // Real Ruby `Regexp#==`: same source pattern AND same flags.
            (RubyValue::Regexp(a), RubyValue::Regexp(b)) => {
                a.source == b.source
                    && a.ignore_case == b.ignore_case
                    && a.extended == b.extended
                    && a.multiline == b.multiline
            }
            _ => false,
        }
    }

    /// `Kernel#frozen?`, universally over every variant (Phase 13.1),
    /// mirroring CRuby's own tiering: immediates (`Integer`/`Float`/
    /// `Symbol`/`nil`/`true`/`false`) are ALWAYS frozen (CRuby stores no
    /// flag for them at all -- `RB_FL_ABLE` is false, frozen is implied);
    /// `Range` is frozen-at-birth (real Ruby since 3.0, and this runtime's
    /// `Range` is a plain immutable value anyway); the shared-mutable types
    /// (`Str`/`Array`/`Hash`/`Object`) consult their real stored flag.
    /// `Proc`/`Regexp`/`MatchData` report `false` unconditionally -- they
    /// have nowhere to store the bit (`RProc` is a bare `Arc<dyn Fn>`) and
    /// no mutating methods exist on any of them, so `.freeze` on one is
    /// accepted as a no-op whose flag isn't remembered: a documented, narrow
    /// approximation (real Ruby would report `true` after an explicit
    /// freeze), not silent wrongness.
    pub fn is_frozen(&self) -> bool {
        match self {
            RubyValue::Nil
            | RubyValue::Bool(_)
            | RubyValue::Int(_)
            | RubyValue::Float(_)
            | RubyValue::Symbol(_)
            | RubyValue::Range(..) => true,
            RubyValue::Str(s) => s.is_frozen(),
            RubyValue::Array(a) => a.is_frozen(),
            RubyValue::Hash(h) => h.is_frozen(),
            RubyValue::Object(o) => o.is_frozen(),
            RubyValue::Proc(_)
            | RubyValue::Regexp(_)
            | RubyValue::MatchData(_)
            | RubyValue::Fiber(_) => false,
        }
    }

    /// `Kernel#freeze`: SHALLOW (sets only this value's own flag, never
    /// recursing into elements -- deep freeze is `Ractor.make_shareable`'s
    /// job, a later phase), returns self (a cheap handle clone), and is a
    /// silent no-op on anything already/always frozen -- all three verified
    /// against CRuby's `rb_obj_freeze` (`object.c:1360`).
    pub fn freeze_value(&self) -> RubyValue {
        match self {
            RubyValue::Str(s) => s.set_frozen(),
            RubyValue::Array(a) => a.set_frozen(),
            RubyValue::Hash(h) => h.set_frozen(),
            RubyValue::Object(o) => o.set_frozen(),
            // Always-frozen immediates/`Range`, and the flagless
            // `Proc`/`Regexp`/`MatchData` approximation -- see `is_frozen`.
            _ => {}
        }
        self.clone()
    }

    /// `case`/`when`'s and `case`/`in`'s value-pattern matching escape hatch
    /// -- a strict superset of `rb_eq` (falls back to it for every value
    /// shape that isn't a `Regexp` pattern against a `Str` subject), adding
    /// real Ruby's actual `Regexp#===` behavior (`when /foo/` matching
    /// against a `String` subject) instead of the plain structural-equality
    /// check `rb_eq` alone would give (which -- since a `Regexp` never
    /// structurally equals a `Str` -- would make `when /foo/` never match
    /// anything at all). A non-`String` subject against a `Regexp` pattern
    /// is `false` (real Ruby: `Regexp#===` returns `false`, not an error,
    /// for anything that doesn't respond to `to_str`), matching this
    /// function's own no-panic-on-mismatched-shape posture.
    pub fn rb_case_eq(&self, subject: &RubyValue) -> bool {
        if let (RubyValue::Regexp(re), RubyValue::Str(s)) = (self, subject) {
            return re.compiled.is_match(&s.lock());
        }
        self.rb_eq(subject)
    }
}
