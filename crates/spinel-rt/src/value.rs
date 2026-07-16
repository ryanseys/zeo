//! `RubyValue` -- the boxed/poly representation every ivar, method argument,
//! and method return uses in the spike (see the plan's stated scope-cut:
//! native-unboxed `i64` is used only for literal `Int` arithmetic in codegen,
//! everything else is `RubyValue` uniformly for now). Mirrors spinel's boxed
//! `sp_RbVal` tagged union (`lib/sp_gc.h:42`), but as a real Rust `enum`
//! instead of a hand-written `{ tag; cls_id; union { ... } }` struct.

use crate::collections::{RArray, RHash, RStr};
use crate::dispatch::{
    ClassId, ARRAY_CLASS, CLASS_CLASS, FALSE_CLASS, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS,
    INTEGER_CLASS, MATCH_DATA_CLASS, MODULE_CLASS, MUTEX_CLASS, NIL_CLASS, PROC_CLASS,
    QUEUE_CLASS, RACTOR_CLASS, RANGE_CLASS, REGEXP_CLASS, STRING_CLASS, SYMBOL_CLASS,
    THREAD_CLASS, TRUE_CLASS,
};
use crate::fiber::RFiber;
use crate::ractor::RRactor;
use crate::thread::{RMutex, RQueue, RThread};
use crate::regexp::{RMatchData, RRegexp};
use crate::{RObj, RProc, Symbol};

#[derive(Clone)]
pub enum RubyValue {
    Nil,
    Bool(bool),
    Int(i64),
    /// An `Integer` beyond `i64` (Phase 17.1's full-bignum decision).
    /// INVARIANT: never holds an i64-range value -- every construction
    /// funnels through `builtins::integer::int_value`, which demotes to
    /// `Int` whenever the value fits, keeping equality/hashing/matching
    /// canonical (a `BigInt(5)` can never exist alongside `Int(5)`).
    /// `class_id()` is `INTEGER_CLASS` -- one Ruby class, two payloads.
    /// Always-frozen immediate tier, like `Int`.
    BigInt(std::sync::Arc<num_bigint::BigInt>),
    Float(f64),
    /// A `Rational` (Phase 17.1) -- always reduced, `den > 0`, bignum
    /// components; see `builtins::rational`. Always-frozen immediate tier.
    Rational(crate::builtins::rational::RRational),
    /// A `Complex` (Phase 17.1) -- two components that keep their own
    /// numeric class (Integer|Float|Rational); see `builtins::complex`.
    /// Always-frozen immediate tier.
    Complex(crate::builtins::complex::RComplex),
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
    /// An `Enumerator` (Phase 17.2) -- captures `(receiver, method, args)`
    /// or an `Enumerator.new` generator block; external iteration state is
    /// a thread-pinned fiber, same split as `Fiber` (see
    /// `builtins::enumerator`'s module docs).
    Enumerator(crate::builtins::enumerator::REnumerator),
    /// An `Enumerator::Yielder` (Phase 17.2) -- the `y` in
    /// `Enumerator.new { |y| y << 1 }`, wrapping the each-block currently
    /// being driven (`y << v` / `y.yield v` forward to it).
    Yielder(RProc),
    /// A `Thread` (Phase 13.5) -- a `may` green coroutine; see
    /// `thread`'s module docs for the cooperative-scheduling divergence.
    Thread(RThread),
    /// A Ruby `Mutex` (Phase 13.5) -- non-reentrant, per-execution-context
    /// owned, like CRuby's.
    Mutex(RMutex),
    /// A `Queue` (Phase 13.5) -- blocking pop, closable.
    Queue(RQueue),
    /// A `Ractor` (Phase 13.8) -- a real OS thread with a frozen-or-copy
    /// message boundary; see `ractor`'s module docs.
    Ractor(RRactor),
    /// A first-class class/module VALUE (Phase 16.1) -- `x = Widget`,
    /// `w.class`, a rescue binding's `.class`, classes stored in
    /// collections. `Copy` payload, always frozen (like the immediates);
    /// its Ruby-visible name/module-ness live in the `ClassRegistry`
    /// (`dispatch::class_name`/`class_is_module`), installed before any
    /// generated statement runs. `Class.new`-style runtime class CREATION
    /// is a permanent AOT exclusion; this value is a handle to a
    /// compile-time-known class, never a way to mint one.
    Class(ClassId),
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
    // Ruby switches to scientific notation when the decimal exponent
    // leaves [-4, 16) (`1e16.to_s == "1.0e+16"`, `Float::EPSILON ==
    // "2.220446049250313e-16"`); Rust's positional `{}` never does, so the
    // threshold is applied here. Mantissas keep at least one fractional
    // digit and positive exponents an explicit `+`, both Ruby's shapes.
    let abs = f.abs();
    if abs != 0.0 && !(1e-4..1e16).contains(&abs) {
        let sci = format!("{f:e}");
        let (mantissa, exp) = sci.split_once('e').expect("{:e} always has an exponent");
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        return if let Some(neg) = exp.strip_prefix('-') {
            format!("{mantissa}e-{neg}")
        } else {
            format!("{mantissa}e+{exp}")
        };
    }
    let s = f.to_string();
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}

/// The pointer identity of a shared, potentially SELF-REFERENTIAL container
/// -- the one visited-set key every recursive `RubyValue` traversal in this
/// runtime uses (`to_display_string`/`inspect_string` here,
/// `ractor::shareable`/`make_shareable` -- Phase 15.2's cycle guards). `Arc`
/// identity is exactly Ruby's object identity for these types (aliasing a
/// collection clones the `Arc`, never the payload -- see `collections`'s
/// module docs), so "this address is already on the traversal stack" is
/// precisely "this OBJECT is its own ancestor", CRuby's own
/// `rb_exec_recursive` test. `Str` never contains values and the immediates
/// have no identity, so only the three genuinely recursive kinds answer.
pub(crate) fn container_identity(v: &RubyValue) -> Option<usize> {
    match v {
        RubyValue::Array(a) => Some(std::sync::Arc::as_ptr(a) as usize),
        RubyValue::Hash(h) => Some(std::sync::Arc::as_ptr(h) as usize),
        // A fat `*const dyn RubyObject` -- casting to `*const ()` keeps the
        // data half, which is the identity (the vtable half only varies by
        // concrete type, never between two handles to the same object).
        RubyValue::Object(o) => Some(std::sync::Arc::as_ptr(o) as *const () as usize),
        _ => None,
    }
}

/// The default `#<Class:0xADDR ...>` rendering for a user object that defines
/// no `to_s`/`inspect` override -- CRuby's `rb_any_to_s`/`rb_obj_inspect`.
/// `to_s` (`with_ivars=false`) is just `#<Class:0xADDR>`; `inspect` lists the
/// ivars as `@name=<inspected value>` in field-declaration order. The address
/// is the object's identity (its `Arc` data pointer), address-normalized
/// (`0x[0-9a-f]+` -> `0xADDR`) by the conformance harness. A self-referential
/// ivar renders `...` (CRuby's recursion guard). Divergence: CRuby omits a
/// never-assigned ivar, but our generated structs pre-declare every `@x` the
/// class body mentions, so an unassigned one shows as `@x=nil` -- same root as
/// `ivar_get_named`'s invented-ivar TODO.
pub(crate) fn default_object_repr(o: &crate::RObj, with_ivars: bool, seen: &mut Vec<usize>) -> String {
    let name = crate::dispatch::class_name(o.class_id()).unwrap_or_else(|| "Object".to_string());
    let addr = std::sync::Arc::as_ptr(o) as *const () as usize;
    if !with_ivars {
        return format!("#<{name}:0x{addr:016x}>");
    }
    if seen.contains(&addr) {
        return format!("#<{name}:0x{addr:016x} ...>");
    }
    let pairs = o.ivar_pairs();
    if pairs.is_empty() {
        return format!("#<{name}:0x{addr:016x}>");
    }
    seen.push(addr);
    let body = pairs
        .iter()
        .map(|(n, v)| format!("{n}={}", v.inspect_with(seen)))
        .collect::<Vec<_>>()
        .join(", ");
    seen.pop();
    format!("#<{name}:0x{addr:016x} {body}>")
}

impl RubyValue {
    /// Mirrors `sp_*_to_s`/CRuby's `Kernel#puts` argument stringification.
    pub fn to_display_string(&self) -> String {
        self.display_with(&mut Vec::new())
    }

    /// `to_display_string`'s recursive worker: `seen` is the traversal
    /// STACK of container identities (pushed on entry, popped on exit --
    /// not a permanent "already printed" set: a DAG that shares one array
    /// twice still prints it twice, like CRuby; only a genuine cycle hits
    /// the guard). A self-referential `Array`/`Hash` prints CRuby's own
    /// recursion markers (`[...]`/`{...}`) instead of deadlocking on its
    /// own non-reentrant payload `Mutex` (the pre-15.2 behavior).
    fn display_with(&self, seen: &mut Vec<usize>) -> String {
        // A builtin-reopen `to_s` override wins (Phase 16.3) -- real Ruby's
        // behavior for `puts`/interpolation, oracle-verified (`class
        // Integer; def to_s; "int"; end` makes `puts 5`/`"v=#{5}"` print
        // "int"). Object receivers keep their own registry probe in the
        // match below; the result's payload is taken directly when it's a
        // `Str` (re-dispatching would re-probe the same override forever
        // for an identity-shaped `to_s`).
        if !matches!(self, RubyValue::Object(_)) {
            if let Some(f) =
                crate::dispatch::value_method(self.class_id(), 0, crate::Symbol::intern("to_s"))
            {
                return match f(self, &[], None) {
                    Ok(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
                    Ok(other) => other.display_with(seen),
                    Err(_) => panic!(
                        "a user-defined `to_s` raised inside stringification (spike scope: no exception channel here)"
                    ),
                };
            }
        }
        match self {
            RubyValue::Nil => String::new(),
            RubyValue::Bool(b) => b.to_string(),
            RubyValue::Int(i) => i.to_string(),
            RubyValue::BigInt(b) => b.to_string(),
            // `Rational#to_s` is "3/4"; `Complex#to_s` is "1+2i" -- the
            // parenthesized forms are inspect's (oracle-verified).
            RubyValue::Rational(r) => crate::builtins::rational::rat_to_s(r),
            RubyValue::Complex(c) => crate::builtins::complex::cpx_format(c, false),
            RubyValue::Float(f) => float_to_display_string(*f),
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            // `puts` on an `Array` recursively flattens and prints each
            // element on its own line (not `[1, 2, 3]`, which is `inspect`'s
            // job, not `to_s`'s) -- real, verified CRuby behavior, not a
            // simplification.
            RubyValue::Array(a) => {
                let ptr = container_identity(self).expect("Array is a container");
                if seen.contains(&ptr) {
                    return "[...]".to_string();
                }
                seen.push(ptr);
                let out = a
                    .lock()
                    .iter()
                    .map(|e| e.display_with(seen))
                    .collect::<Vec<_>>()
                    .join("\n");
                seen.pop();
                out
            }
            // An approximation of `Hash#inspect` (symbol keys as `key:
            // value`, everything else as `key => value`) -- good enough for
            // the `Int`/`Symbol`-keyed hashes the spike's examples use, but
            // NOT a faithful `inspect` for nested `String`s (no quoting).
            // Same posture as `Object`'s "#<Object>" placeholder above: a
            // documented simplification, not silent wrongness.
            RubyValue::Hash(h) => {
                let ptr = container_identity(self).expect("Hash is a container");
                if seen.contains(&ptr) {
                    return "{...}".to_string();
                }
                seen.push(ptr);
                let body = h
                    .lock()
                    .values()
                    .map(|(k, v)| match k {
                        RubyValue::Symbol(s) => format!("{}: {}", s.name(), v.display_with(seen)),
                        _ => format!("{} => {}", k.display_with(seen), v.display_with(seen)),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                seen.pop();
                format!("{{{body}}}")
            }
            RubyValue::Range(start, end, exclusive) => {
                let s = start.as_ref().map(|b| b.display_with(seen)).unwrap_or_default();
                let e = end.as_ref().map(|b| b.display_with(seen)).unwrap_or_default();
                let op = if *exclusive { "..." } else { ".." };
                format!("{s}{op}{e}")
            }
            // A user-defined `to_s` wins (Phase 16.2, dispatched through
            // the registry so inherited/mixed-in definitions resolve); the
            // default is CRuby's `#<Class:0xADDR>` (no ivars -- that's
            // `inspect`'s job). The address is normalized by the conformance
            // harness (see `default_object_repr`).
            RubyValue::Object(o) => {
                match crate::dispatch::call_user_method(o, "to_s", &[]) {
                    Some(Ok(v)) => v.display_with(seen),
                    Some(Err(_)) => panic!(
                        "a user-defined `to_s` raised inside stringification (spike scope: no exception channel here)"
                    ),
                    None => default_object_repr(o, false, seen),
                }
            }
            RubyValue::Proc(_) => "#<Proc>".to_string(),
            // `Regexp#to_s` -- real Ruby's `(?opts-negopts:body)` form (NOT
            // the `/pattern/flags` literal form, that's `#inspect`'s job --
            // see `regexp::regexp_to_s`'s docs).
            RubyValue::Regexp(re) => crate::regexp::regexp_to_s(re).to_display_string(),
            // `MatchData#to_s` -- the whole matched substring.
            RubyValue::MatchData(m) => crate::regexp::matchdata_to_s(m).to_display_string(),
            // Same placeholder posture as `Object`/`Proc` above.
            RubyValue::Fiber(_) => "#<Fiber>".to_string(),
            // The real CRuby shape (`#<Enumerator: [1, 2]:each>`) -- to_s
            // and inspect agree for enumerators.
            RubyValue::Enumerator(e) => crate::builtins::enumerator::enum_inspect(e),
            RubyValue::Yielder(_) => "#<Enumerator::Yielder>".to_string(),
            RubyValue::Thread(_) => "#<Thread>".to_string(),
            RubyValue::Mutex(_) => "#<Mutex>".to_string(),
            RubyValue::Queue(_) => "#<Thread::Queue>".to_string(),
            RubyValue::Ractor(_) => "#<Ractor>".to_string(),
            // The registered fully-qualified name (`puts Widget` ->
            // "Widget", `puts Store::Item` -> "Store::Item"); the id form
            // is only reachable registry-less (this crate's unit tests).
            RubyValue::Class(cid) => crate::dispatch::class_name(*cid)
                .unwrap_or_else(|| format!("#<Class:{}>", cid.0)),
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
    /// table). A SELF-REFERENTIAL `Array`/`Hash` prints CRuby's own
    /// recursion markers (`[1, [...]]` / `{k: {...}}`, oracle-verified) via
    /// the shared visited-stack guard -- see `container_identity`.
    pub fn inspect_string(&self) -> String {
        self.inspect_with(&mut Vec::new())
    }

    /// `inspect_string`'s recursive worker -- same visited-STACK discipline
    /// as `display_with` (its docs explain the push/pop shape). The marker
    /// is chosen by the RECURRING container's own kind, so a cycle that
    /// enters through a Hash back into an outer Array prints `[...]` at the
    /// Array's re-entry point (`[1, {x: [...]}]`, oracle-verified).
    fn inspect_with(&self, seen: &mut Vec<usize>) -> String {
        // A builtin-reopen `inspect` override wins (Phase 16.3) -- and it
        // propagates into CONTAINER rendering too (`[5].inspect` ->
        // `[I<5>]` with an `Integer#inspect` override -- real Ruby's
        // `rb_inspect` dispatches per element, oracle-verified), which this
        // probe's position inside the recursive worker reproduces. Same
        // `Str`-payload shortcut as `display_with`'s probe.
        if !matches!(self, RubyValue::Object(_)) {
            if let Some(f) =
                crate::dispatch::value_method(self.class_id(), 0, crate::Symbol::intern("inspect"))
            {
                return match f(self, &[], None) {
                    Ok(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
                    Ok(other) => other.display_with(seen),
                    Err(_) => panic!(
                        "a user-defined `inspect` raised inside inspection (spike scope: no exception channel here)"
                    ),
                };
            }
        }
        match self {
            RubyValue::Nil => "nil".to_string(),
            // The parenthesized inspect forms (`(3/4)` / `(1+2i)`) vs the
            // bare `to_s` ones -- oracle-verified.
            RubyValue::Rational(r) => {
                format!("({})", crate::builtins::rational::rat_to_s(r))
            }
            RubyValue::Complex(c) => crate::builtins::complex::cpx_format(c, true),
            RubyValue::Symbol(s) => format!(":{}", s.name()),
            RubyValue::Str(s) => crate::encoding::inspect(&s.lock()),
            RubyValue::Array(a) => {
                let ptr = container_identity(self).expect("Array is a container");
                if seen.contains(&ptr) {
                    return "[...]".to_string();
                }
                seen.push(ptr);
                let body = a
                    .lock()
                    .iter()
                    .map(|e| e.inspect_with(seen))
                    .collect::<Vec<_>>()
                    .join(", ");
                seen.pop();
                format!("[{body}]")
            }
            // Ruby 3.4+ `Hash#inspect` format: `{a: 1, "k" => 2}` -- symbol
            // keys as `name: value` with no braces-padding spaces.
            RubyValue::Hash(h) => {
                let ptr = container_identity(self).expect("Hash is a container");
                if seen.contains(&ptr) {
                    return "{...}".to_string();
                }
                seen.push(ptr);
                let body = h
                    .lock()
                    .values()
                    .map(|(k, v)| match k {
                        RubyValue::Symbol(s) => format!("{}: {}", s.name(), v.inspect_with(seen)),
                        _ => format!("{} => {}", k.inspect_with(seen), v.inspect_with(seen)),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                seen.pop();
                format!("{{{body}}}")
            }
            RubyValue::Range(start, end, exclusive) => {
                let s = start.as_ref().map(|b| b.inspect_with(seen)).unwrap_or_default();
                let e = end.as_ref().map(|b| b.inspect_with(seen)).unwrap_or_default();
                let op = if *exclusive { "..." } else { ".." };
                format!("{s}{op}{e}")
            }
            RubyValue::Regexp(re) => crate::regexp::regexp_inspect(re).to_display_string(),
            RubyValue::MatchData(_) => "#<MatchData>".to_string(),
            // A user-defined `inspect` wins (Phase 16.2); the default is
            // CRuby's `#<Class:0xADDR @iv=val, ...>` -- address plus the
            // object's ivars, each inspected, in field-declaration order (see
            // `default_object_repr`). NO fallback to a user `to_s` (real
            // Ruby's inspect is independent of to_s).
            RubyValue::Object(o) => {
                match crate::dispatch::call_user_method(o, "inspect", &[]) {
                    Some(Ok(v)) => v.display_with(seen),
                    Some(Err(_)) => panic!(
                        "a user-defined `inspect` raised inside inspection (spike scope: no exception channel here)"
                    ),
                    None => default_object_repr(o, true, seen),
                }
            }
            // `Bool`/`Int`/`Float`/`Proc`: `#inspect` and `#to_s` agree
            // (or share the same placeholder approximation).
            other => other.display_with(seen),
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
            RubyValue::Int(_) | RubyValue::BigInt(_) => INTEGER_CLASS,
            RubyValue::Rational(_) => crate::dispatch::RATIONAL_CLASS,
            RubyValue::Complex(_) => crate::dispatch::COMPLEX_CLASS,
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
            RubyValue::Enumerator(_) => crate::dispatch::ENUMERATOR_CLASS,
            RubyValue::Yielder(_) => crate::dispatch::YIELDER_CLASS,
            RubyValue::Thread(_) => THREAD_CLASS,
            RubyValue::Mutex(_) => MUTEX_CLASS,
            RubyValue::Queue(_) => QUEUE_CLASS,
            RubyValue::Ractor(_) => RACTOR_CLASS,
            // `Widget.class` -> `Class`, `Enumerable.class` -> `Module`
            // (real Ruby; `Class < Module` handled by the registered
            // ancestor chain). Outside a generated program (no registry --
            // this crate's own unit tests) the class case is assumed.
            RubyValue::Class(cid) => {
                if crate::dispatch::class_is_module(*cid).unwrap_or(false) {
                    MODULE_CLASS
                } else {
                    CLASS_CLASS
                }
            }
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

    /// Unwraps a `Thread` payload -- see `as_array_unchecked`'s docs.
    pub fn as_thread_unchecked(&self) -> RThread {
        match self {
            RubyValue::Thread(t) => t.clone(),
            other => panic!("expected a Thread, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Mutex` payload -- see `as_array_unchecked`'s docs.
    pub fn as_mutex_unchecked(&self) -> RMutex {
        match self {
            RubyValue::Mutex(m) => m.clone(),
            other => panic!("expected a Mutex, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Queue` payload -- see `as_array_unchecked`'s docs.
    pub fn as_queue_unchecked(&self) -> RQueue {
        match self {
            RubyValue::Queue(q) => q.clone(),
            other => panic!("expected a Queue, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Ractor` payload -- see `as_array_unchecked`'s docs.
    pub fn as_ractor_unchecked(&self) -> RRactor {
        match self {
            RubyValue::Ractor(r) => r.clone(),
            other => panic!("expected a Ractor, got {}", other.to_display_string()),
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

    /// `==` -- real Ruby's protocol (Phase 16.2, retiring the documented
    /// "conservatively compare unequal" approximation): scalars compare
    /// structurally (`1 == 1.0` across the numeric tower), `Array`/`Hash`
    /// compare ELEMENT-WISE (recursively, cycle-guarded -- see
    /// `rb_eq_guarded`), and an `Object` receiver dispatches its
    /// user-defined `==` when one exists, falling back to real Ruby's own
    /// `Object#==` default: reference identity. Backs `case`/`when`'s
    /// value-matching desugar, `Array#include?`, `send_value`'s universal
    /// `==`, and the codegen operator fallback.
    pub fn rb_eq(&self, other: &RubyValue) -> bool {
        self.rb_eq_guarded(other, &mut Vec::new())
    }

    /// `rb_eq`'s recursive worker: `seen` holds PAIRS of container
    /// identities currently being compared -- CRuby's
    /// `rb_exec_recursive_paired` rule, under which a recursive pair
    /// compares EQUAL (`a = [1]; a << a; b = [1]; b << b; a == b` is true
    /// in real Ruby). Collection payloads are cloned before recursing
    /// (element clones are cheap handle bumps) so no lock is held across a
    /// nested comparison.
    fn rb_eq_guarded(&self, other: &RubyValue, seen: &mut Vec<(usize, usize)>) -> bool {
        // Real Ruby: `1 == 1.0`, `Rational(2,1) == 2`, `Complex(2,0) == 2`
        // -- every numeric pair compares through the ONE tower matrix
        // (Phase 17.1), including the Bignum/Rational/Complex lanes. The
        // (Int, Int) arm below stays as the hot exact fast path.
        if let (RubyValue::Int(a), RubyValue::Int(b)) = (self, other) {
            return a == b;
        }
        if let Some(eq) = crate::builtins::numeric::num_eq(self, other) {
            return eq;
        }
        match (self, other) {
            (RubyValue::Nil, RubyValue::Nil) => true,
            (RubyValue::Bool(a), RubyValue::Bool(b)) => a == b,
            (RubyValue::Symbol(a), RubyValue::Symbol(b)) => a == b,
            // Class identity (Phase 16.1): `Widget == Widget`, and what
            // `Array#include?` on an array of classes consults.
            (RubyValue::Class(a), RubyValue::Class(b)) => a == b,
            (RubyValue::Str(a), RubyValue::Str(b)) => {
                std::sync::Arc::ptr_eq(a, b) || *a.lock() == *b.lock()
            }
            // Real Ruby `Regexp#==`: same source pattern AND same flags.
            (RubyValue::Regexp(a), RubyValue::Regexp(b)) => {
                a.source == b.source
                    && a.ignore_case == b.ignore_case
                    && a.extended == b.extended
                    && a.multiline == b.multiline
            }
            (RubyValue::Array(a), RubyValue::Array(b)) => {
                if std::sync::Arc::ptr_eq(a, b) {
                    return true;
                }
                let pair = (
                    container_identity(self).expect("Array is a container"),
                    container_identity(other).expect("Array is a container"),
                );
                if seen.contains(&pair) {
                    return true;
                }
                seen.push(pair);
                let av: Vec<RubyValue> = a.lock().clone();
                let bv: Vec<RubyValue> = b.lock().clone();
                let eq = av.len() == bv.len()
                    && av.iter().zip(bv.iter()).all(|(x, y)| x.rb_eq_guarded(y, seen));
                seen.pop();
                eq
            }
            // `Hash#==`: same size, same KEYS (by the key table's own
            // `eql?`-style projection -- see `collections::HashKey`), each
            // value `==`.
            (RubyValue::Hash(a), RubyValue::Hash(b)) => {
                if std::sync::Arc::ptr_eq(a, b) {
                    return true;
                }
                let pair = (
                    container_identity(self).expect("Hash is a container"),
                    container_identity(other).expect("Hash is a container"),
                );
                if seen.contains(&pair) {
                    return true;
                }
                seen.push(pair);
                let pairs: Vec<(RubyValue, RubyValue)> = a.lock().values().cloned().collect();
                let eq = crate::hash_len(a) == crate::hash_len(b)
                    && pairs.iter().all(|(k, va)| {
                        crate::hash_has_key(b, k)
                            && va.rb_eq_guarded(&crate::hash_get(b, k), seen)
                    });
                seen.pop();
                eq
            }
            // An `Object` receiver, real Ruby's resolution order: its
            // user-defined `==` (dispatched through the registry -- a
            // materialized method, so inherited/mixed-in definitions
            // resolve too), then `Comparable#==` derived from `<=>` for an
            // `include Comparable` class, else `Object#==`'s default:
            // reference identity. A user `==` that RAISES inside a
            // structural comparison is a loud panic (spike scope: `rb_eq`
            // has no exception channel).
            (RubyValue::Object(o), _) => {
                match crate::dispatch::call_user_method(o, "==", std::slice::from_ref(other)) {
                    Some(Ok(v)) => v.truthy(),
                    Some(Err(_)) => panic!(
                        "a user-defined `==` raised inside a structural comparison (spike scope: no exception channel here)"
                    ),
                    None if crate::dispatch::ancestors_contain(
                        o.class_id(),
                        crate::dispatch::COMPARABLE_CLASS,
                    ) =>
                    {
                        self.rb_cmp(other) == Some(0)
                    }
                    None => match other {
                        RubyValue::Object(b) => {
                            std::sync::Arc::ptr_eq(o, b)
                                || container_identity(self) == container_identity(other)
                        }
                        _ => false,
                    },
                }
            }
            // Reference identity -- CRuby's `Object#==` default (an
            // enumerator never equals a structurally-identical sibling).
            (RubyValue::Enumerator(a), RubyValue::Enumerator(b)) => std::sync::Arc::ptr_eq(a, b),
            (RubyValue::Yielder(a), RubyValue::Yielder(b)) => a.ptr_eq(b),
            _ => false,
        }
    }

    /// `a <=> b` as a signed ordering (Phase 16.2) -- `None` is Ruby's
    /// `nil` (incomparable). Native Int/Float/String fast paths (CRuby's
    /// OPTIMIZED_CMP), then a user-defined `<=>` on an `Object` receiver;
    /// anything else is incomparable. Consumed by `Enumerable#min`/`#max`
    /// and `comparable::comparable_send` (and Phase 17.1's `sort` family).
    pub fn rb_cmp(&self, other: &RubyValue) -> Option<i64> {
        // Every numeric pair orders through the ONE tower matrix (Phase
        // 17.1): exact Int/Bignum/Rational lanes, Float promotion,
        // NaN -> nil, Complex -> nil.
        if let Some(ord) = crate::builtins::numeric::num_cmp(self, other) {
            return ord;
        }
        match (self, other) {
            (RubyValue::Str(a), RubyValue::Str(b)) => {
                let a = a.lock().to_utf8_lossy().into_owned();
                let b = b.lock().to_utf8_lossy().into_owned();
                Some(a.cmp(&b) as i64)
            }
            (RubyValue::Object(o), _) => {
                match crate::dispatch::call_user_method(o, "<=>", std::slice::from_ref(other)) {
                    Some(Ok(RubyValue::Int(i))) => Some(i.signum()),
                    Some(Ok(_)) => None,
                    Some(Err(_)) => panic!(
                        "a user-defined `<=>` raised inside a comparison (spike scope: no exception channel here)"
                    ),
                    None => None,
                }
            }
            _ => None,
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
            | RubyValue::BigInt(_)
            | RubyValue::Rational(_)
            | RubyValue::Complex(_)
            | RubyValue::Float(_)
            | RubyValue::Symbol(_)
            | RubyValue::Range(..)
            // Real Ruby classes aren't frozen by default, but the only
            // mutations (reopening) happen at compile time in this AOT
            // model -- reporting frozen matches what the value can DO.
            | RubyValue::Class(_) => true,
            RubyValue::Str(s) => s.is_frozen(),
            RubyValue::Array(a) => a.is_frozen(),
            RubyValue::Hash(h) => h.is_frozen(),
            RubyValue::Object(o) => o.is_frozen(),
            RubyValue::Proc(_)
            | RubyValue::Regexp(_)
            | RubyValue::MatchData(_)
            | RubyValue::Fiber(_)
            | RubyValue::Enumerator(_)
            | RubyValue::Yielder(_)
            | RubyValue::Thread(_)
            | RubyValue::Mutex(_)
            | RubyValue::Queue(_)
            | RubyValue::Ractor(_) => false,
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

    /// `Kernel#dup`/`#clone` -- the per-kind SHALLOW copy (Phase 15.2), the
    /// one semantic difference between the two being the frozen flag:
    /// `clone` (`copy_frozen: true`) carries it over, `dup` never does
    /// (oracle-verified: `"abc".freeze.dup.frozen?` is `false`,
    /// `.clone.frozen?` is `true`). Copies are shallow exactly like CRuby's
    /// `rb_obj_dup`: a nested element/value/ivar is SHARED with the
    /// original (`arr.dup[1].equal?(arr[1])`, oracle-verified), only the
    /// top-level container is fresh.
    ///
    /// Per-kind rules (each oracle-verified against ruby 4.0.5):
    /// - Immediates/`Symbol`: `dup`/`clone` return self (real Ruby).
    /// - `Str`/`Array`/`Hash`: fresh payload, fresh (or copied) flag.
    /// - `Range`: immutable here and in Ruby -- self suffices (the fresh
    ///   object identity real Ruby mints is unobservable in this runtime).
    /// - `Object`: per-class `RubyObject::dup_object` (see `ruby_class!`).
    /// - `Proc`/`Regexp`/`MatchData`: real Ruby allocates a fresh object;
    ///   all three are immutable in this runtime with no exposed object
    ///   identity, so a reference copy is indistinguishable -- a documented
    ///   approximation, not silent wrongness.
    /// - `Mutex`: a fresh, unlocked mutex (real Ruby's `Mutex#dup` gives
    ///   exactly that -- allocate + no state ivars).
    /// - `Thread`/`Queue`/`Ractor`: raise in real Ruby (`TypeError:
    ///   allocator undefined` / `NoMethodError: initialize_copy`) -- a loud
    ///   panic here (spike scope: no exception-raising channel from this
    ///   method). `Fiber#dup` succeeds in real Ruby but would need a copied
    ///   coroutine we can't build -- rejected loudly rather than aliased
    ///   silently.
    pub fn dup_value(&self, copy_frozen: bool) -> RubyValue {
        let keep_frozen = copy_frozen && self.is_frozen();
        match self {
            RubyValue::Nil
            | RubyValue::Bool(_)
            | RubyValue::Int(_)
            | RubyValue::BigInt(_)
            | RubyValue::Rational(_)
            | RubyValue::Complex(_)
            | RubyValue::Float(_)
            | RubyValue::Symbol(_)
            | RubyValue::Range(..)
            | RubyValue::Proc(_)
            | RubyValue::Regexp(_)
            | RubyValue::MatchData(_)
            // Real `Class#dup` mints an anonymous class copy -- impossible
            // in this AOT model; the handle is returned instead (documented
            // divergence, same posture as Proc/Regexp above).
            | RubyValue::Class(_) => self.clone(),
            RubyValue::Str(s) => {
                let fresh = crate::string_new(s.lock().to_utf8_lossy().into_owned());
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Str(fresh)
            }
            RubyValue::Array(a) => {
                let fresh = crate::array_new(a.lock().clone());
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Array(fresh)
            }
            RubyValue::Hash(h) => {
                let pairs: Vec<(RubyValue, RubyValue)> = h.lock().values().cloned().collect();
                let fresh = crate::hash_new(pairs);
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Hash(fresh)
            }
            RubyValue::Object(o) => RubyValue::Object(o.dup_object(copy_frozen)),
            RubyValue::Mutex(_) => crate::mutex_new(),
            // A never-iterated (or finished) enumerator copies as a fresh
            // one over the same source; a LIVE iteration can't be copied
            // (CRuby raises "can't copy execution context").
            RubyValue::Enumerator(e) => {
                if e.iteration_live() {
                    panic!("can't copy execution context (an Enumerator mid external iteration; CRuby raises TypeError)");
                }
                RubyValue::Enumerator(e.fresh_copy())
            }
            // A yielder is just a handle onto the driving block -- the
            // reference copy is indistinguishable (the Proc posture above).
            RubyValue::Yielder(_) => self.clone(),
            RubyValue::Fiber(_) => panic!("Fiber#dup/clone isn't supported yet (spike scope: the backing coroutine can't be copied)"),
            RubyValue::Thread(_) => panic!("can't dup/clone a Thread (TypeError: allocator undefined for Thread; spike scope: raised as a panic)"),
            RubyValue::Queue(_) => panic!("can't dup/clone a Queue (NoMethodError: undefined method 'initialize_copy'; spike scope: raised as a panic)"),
            RubyValue::Ractor(_) => panic!("can't dup/clone a Ractor (TypeError: allocator undefined for Ractor; spike scope: raised as a panic)"),
        }
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
            return re.compiled.is_match(&s.lock().to_utf8_lossy());
        }
        // `Range#===` is `#cover?` (Phase 17.1, fixing `when 1..5` -- which
        // previously fell to `rb_eq` and silently never matched): each
        // present endpoint compares via `rb_cmp` (numeric tower, strings,
        // user `<=>`), and an incomparable subject is `false`, real Ruby's
        // rule (`(1..5) === "x"` is false, not an error; oracle-verified,
        // incl. `(1..6) === 5.5` true and `(1...5) === 5` false).
        if let RubyValue::Range(start, end, exclusive) = self {
            return range_covers(start.as_deref(), end.as_deref(), *exclusive, subject);
        }
        // `Module#===` (Phase 16.1): `case x when Integer` / `when Widget`
        // is an instance-of-ancestry check, NOT equality (`Widget ===
        // Widget` is false in real Ruby -- a class is not an instance of
        // itself). Registry-backed; only reachable in generated programs,
        // which always install one.
        if let RubyValue::Class(cid) = self {
            return crate::dispatch::is_a(subject.class_id(), *cid);
        }
        self.rb_eq(subject)
    }
}

/// `Range#cover?`'s core (also `Range#===`/`case when 1..5`): begin <=
/// subject (< or <=) end, via `rb_cmp` (numeric tower, strings, user
/// `<=>`); a `nil` comparison (incomparable) is `false`, real Ruby's rule.
/// Beginless/endless sides are unbounded.
pub(crate) fn range_covers(
    start: Option<&RubyValue>,
    end: Option<&RubyValue>,
    exclusive: bool,
    subject: &RubyValue,
) -> bool {
    let lower_ok = match start {
        Some(s) => matches!(subject.rb_cmp(s), Some(c) if c >= 0),
        None => true,
    };
    if !lower_ok {
        return false;
    }
    match end {
        Some(e) => match subject.rb_cmp(e) {
            Some(c) if exclusive => c < 0,
            Some(c) => c <= 0,
            None => false,
        },
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{array_new, array_push, hash_new, hash_set, string_new, Symbol};

    fn sym(name: &str) -> RubyValue {
        RubyValue::Symbol(Symbol::intern(name))
    }

    /// Phase 15.2: every expected string below is oracle-verified against
    /// real ruby 4.0.5 (`p`/`puts` on the same graphs).
    #[test]
    fn inspect_marks_a_self_referential_array() {
        let a = array_new(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        array_push(&a, RubyValue::Array(a.clone()));

        assert_eq!(RubyValue::Array(a).inspect_string(), "[1, 2, [...]]");
    }

    #[test]
    fn inspect_marks_a_self_referential_hash() {
        let h = hash_new(vec![(sym("k"), RubyValue::Int(1))]);
        hash_set(&h, sym("me"), RubyValue::Hash(h.clone()));

        assert_eq!(RubyValue::Hash(h).inspect_string(), "{k: 1, me: {...}}");
    }

    /// The marker follows the RECURRING container's kind: an array cycle
    /// re-entered through a nested hash still prints `[...]`.
    #[test]
    fn inspect_marks_a_cross_container_cycle_by_the_recurring_kind() {
        let b = array_new(vec![RubyValue::Int(1)]);
        let h = hash_new(vec![(sym("x"), RubyValue::Nil)]);
        array_push(&b, RubyValue::Hash(h.clone()));
        hash_set(&h, sym("x"), RubyValue::Array(b.clone()));

        assert_eq!(RubyValue::Array(b).inspect_string(), "[1, {x: [...]}]");
    }

    /// `puts`'s display path shares the guard: one line per element, the
    /// cycle rendered as its marker.
    #[test]
    fn display_marks_a_self_referential_array() {
        let a = array_new(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        array_push(&a, RubyValue::Array(a.clone()));

        assert_eq!(RubyValue::Array(a).to_display_string(), "1\n2\n[...]");
    }

    /// The visited set is a traversal STACK, not a permanent "seen" set: a
    /// DAG sharing one array twice (no cycle) prints it in full both times,
    /// like real Ruby.
    #[test]
    fn inspect_prints_a_shared_but_acyclic_child_twice() {
        let inner = array_new(vec![RubyValue::Int(7)]);
        let outer = array_new(vec![
            RubyValue::Array(inner.clone()),
            RubyValue::Array(inner),
        ]);

        assert_eq!(RubyValue::Array(outer).inspect_string(), "[[7], [7]]");
    }

    /// `dup` never copies the frozen flag; `clone` always does
    /// (oracle-verified for Str/Array/Hash and objects alike).
    #[test]
    fn dup_starts_unfrozen_and_clone_copies_the_flag() {
        let s = RubyValue::Str(string_new("abc".to_string()));
        s.freeze_value();

        assert!(!s.dup_value(false).is_frozen(), "dup of frozen is unfrozen");
        assert!(s.dup_value(true).is_frozen(), "clone of frozen stays frozen");

        let unfrozen = RubyValue::Str(string_new("abc".to_string()));
        assert!(!unfrozen.dup_value(true).is_frozen(), "clone of unfrozen stays unfrozen");
    }

    /// The copy's top-level payload is FRESH (mutating it leaves the
    /// original untouched)...
    #[test]
    fn dup_copies_the_top_level_payload() {
        let a = array_new(vec![RubyValue::Int(1)]);
        let copy = RubyValue::Array(a.clone()).dup_value(false);
        array_push(&copy.as_array_unchecked(), RubyValue::Int(2));

        assert_eq!(a.lock().len(), 1);
        assert_eq!(copy.as_array_unchecked().lock().len(), 2);

        let h = hash_new(vec![(sym("a"), RubyValue::Int(1))]);
        let hcopy = RubyValue::Hash(h.clone()).dup_value(false);
        hash_set(&hcopy.as_hash_unchecked(), sym("b"), RubyValue::Int(2));

        assert_eq!(h.lock().len(), 1);
        assert_eq!(hcopy.as_hash_unchecked().lock().len(), 2);
        assert_eq!(
            hcopy.inspect_string(),
            "{a: 1, b: 2}",
            "insertion order survives the copy"
        );
    }

    /// ...but nested elements are SHARED -- CRuby's shallow rule
    /// (`arr.dup[1].equal?(arr[1])` is true, oracle-verified).
    #[test]
    fn dup_is_shallow_nested_values_are_shared() {
        let inner = array_new(vec![RubyValue::Int(9)]);
        let outer = array_new(vec![RubyValue::Array(inner.clone())]);
        let copy = RubyValue::Array(outer).dup_value(false);

        let copied_inner = copy.as_array_unchecked().lock()[0].as_array_unchecked();
        assert!(std::sync::Arc::ptr_eq(&inner, &copied_inner));
    }

    /// Immediates and Symbols: `dup`/`clone` return self (real Ruby).
    #[test]
    fn dup_of_immediates_returns_the_same_value() {
        for v in [
            RubyValue::Nil,
            RubyValue::Bool(true),
            RubyValue::Int(5),
            RubyValue::Float(1.5),
            sym("s"),
        ] {
            assert!(v.dup_value(false).rb_eq(&v));
            assert!(v.dup_value(true).rb_eq(&v));
        }
        let r = RubyValue::Range(
            Some(Box::new(RubyValue::Int(1))),
            Some(Box::new(RubyValue::Int(3))),
            false,
        );
        assert_eq!(r.dup_value(false).inspect_string(), "1..3");
    }

    /// Phase 16.2: element-wise `==` (retiring "conservatively compare
    /// unequal"), including nesting and the recursive-pair rule.
    #[test]
    fn arrays_and_hashes_compare_element_wise() {
        let a = RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Array(array_new(vec![RubyValue::Int(2)])),
        ]));
        let b = RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Array(array_new(vec![RubyValue::Int(2)])),
        ]));
        assert!(a.rb_eq(&b));
        assert!(!a.rb_eq(&RubyValue::Array(array_new(vec![RubyValue::Int(1)]))));

        let h1 = hash_new(vec![(sym("a"), RubyValue::Int(1))]);
        let h2 = hash_new(vec![(sym("a"), RubyValue::Int(1))]);
        let h3 = hash_new(vec![(sym("a"), RubyValue::Int(2))]);
        assert!(RubyValue::Hash(h1.clone()).rb_eq(&RubyValue::Hash(h2)));
        assert!(!RubyValue::Hash(h1).rb_eq(&RubyValue::Hash(h3)));
    }

    /// CRuby's `rb_exec_recursive_paired` rule: two structurally-identical
    /// self-referential arrays compare EQUAL (`a = [1]; a << a` twice over
    /// -- oracle-verified `a == b` is true in real Ruby), and the
    /// comparison terminates.
    #[test]
    fn recursive_pairs_compare_equal_and_terminate() {
        let a = array_new(vec![RubyValue::Int(1)]);
        array_push(&a, RubyValue::Array(a.clone()));
        let b = array_new(vec![RubyValue::Int(1)]);
        array_push(&b, RubyValue::Array(b.clone()));

        assert!(RubyValue::Array(a).rb_eq(&RubyValue::Array(b)));
    }

    /// `rb_cmp`'s native tiers; `None` is Ruby's nil (incomparable).
    #[test]
    fn rb_cmp_orders_the_native_tiers() {
        assert_eq!(RubyValue::Int(1).rb_cmp(&RubyValue::Int(2)), Some(-1));
        assert_eq!(RubyValue::Int(2).rb_cmp(&RubyValue::Float(2.0)), Some(0));
        assert_eq!(RubyValue::Float(3.5).rb_cmp(&RubyValue::Int(3)), Some(1));
        assert_eq!(
            RubyValue::Str(string_new("a".into())).rb_cmp(&RubyValue::Str(string_new("b".into()))),
            Some(-1)
        );
        assert_eq!(
            RubyValue::Float(f64::NAN).rb_cmp(&RubyValue::Float(1.0)),
            None,
            "NaN is incomparable, like real Ruby's nil"
        );
        assert_eq!(RubyValue::Int(1).rb_cmp(&sym("x")), None);
    }

    /// `Object#hash`'s universal digest agrees exactly where the Hash
    /// table would treat two values as one key.
    #[test]
    fn value_hash_code_is_structural() {
        use crate::value_hash_code;
        assert_eq!(
            value_hash_code(&RubyValue::Str(string_new("a".into()))),
            value_hash_code(&RubyValue::Str(string_new("a".into())))
        );
        assert_eq!(
            value_hash_code(&RubyValue::Array(array_new(vec![RubyValue::Int(1)]))),
            value_hash_code(&RubyValue::Array(array_new(vec![RubyValue::Int(1)])))
        );
        assert_ne!(
            value_hash_code(&RubyValue::Int(1)),
            value_hash_code(&RubyValue::Float(1.0)),
            "eql?-style keying: 1 and 1.0 are distinct keys"
        );
    }

    /// One representative of the rejected tier (Thread/Queue/Ractor/Fiber
    /// all take the same loud-panic arm; Queue is the only one
    /// constructible without a running `may`/coroutine context).
    #[test]
    #[should_panic(expected = "can't dup/clone a Queue")]
    fn dup_of_a_queue_panics_loudly() {
        crate::queue_new().dup_value(false);
    }

    #[test]
    fn dup_of_a_mutex_makes_a_fresh_unlocked_mutex() {
        let m = crate::mutex_new();
        let copy = m.dup_value(false);
        let (RubyValue::Mutex(a), RubyValue::Mutex(b)) = (&m, &copy) else {
            panic!("expected two mutexes")
        };
        assert!(!std::sync::Arc::ptr_eq(a, b), "fresh mutex, not an alias");
    }
}
