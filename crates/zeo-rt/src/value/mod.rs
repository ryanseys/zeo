//! `RubyValue` -- the boxed representation every ivar, method argument, and
//! method return uses in this runtime: a real Rust `enum`, so the variant
//! itself is the tag (no hand-written `{ tag; cls_id; union { ... } }`).
//! Native-unboxed `i64` is used only for literal `Int` arithmetic in codegen;
//! everything else is `RubyValue` uniformly.

//!
//! # The value/ boundary
//!
//! * `mod.rs` -- the `RubyValue` tag enum itself: variants, clone/eq/`<=>`
//!   semantics, class-of, case-equality. The Cranelift ABI work adds
//!   `#[repr(C, u8)]`, explicit discriminants, and the layout asserts HERE.
//! * [`collections`] -- the container STORES behind the heap variants
//!   (`Freezable`, `ArrayStore`, hash storage).
//! * [`ivars`] -- per-class `IvarCell<N>` slot storage for generated
//!   object structs.
//! * [`value_ivars`] -- ivars ON VALUES (side tables for value-subclass
//!   instances and immediates).
//!
//! Everything outside `value/` reaches these through the crate-root
//! aliases (`crate::collections`, ...), so the grouping changed no paths.

pub(crate) mod collections;
pub(crate) mod ivars;
pub(crate) mod value_ivars;
use crate::builtins::{arg_error, type_error};
use crate::collections::{RArray, RHash, RStr};
use crate::dispatch::{
    ARRAY_CLASS, CLASS_CLASS, ClassId, FALSE_CLASS, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS,
    INTEGER_CLASS, MATCH_DATA_CLASS, MODULE_CLASS, MUTEX_CLASS, NIL_CLASS, QUEUE_CLASS,
    RACTOR_CLASS, RANGE_CLASS, REGEXP_CLASS, SIZED_QUEUE_CLASS, STRING_CLASS, SYMBOL_CLASS,
    TRUE_CLASS,
};
use crate::fiber::RFiber;
use crate::ractor::RRactor;
use crate::regexp::{RMatchData, RRegexp};
use crate::thread::{RMutex, RQueue, RThread};
use crate::{RObj, RProc, Symbol};

// `repr(C, u8)` = tag byte + payload union: size 24, align 8, every payload
// at offset 8, `Option`/`Result` niches preserved -- asserted by the
// `abi_layout` test below. Discriminants are `zeo_abi::abi::ValueTag`'s
// numbers: immediates (no `Arc` payload -- a plain 24-byte copy) below
// `FIRST_HEAP_TAG` (16), heap variants from 16, source order within each
// band.
#[derive(Clone)]
#[repr(C, u8)]
pub enum RubyValue {
    Nil = 0,
    Bool(bool) = 1,
    Int(i64) = 2,
    /// An `Integer` beyond `i64` (the full-bignum decision).
    /// INVARIANT: never holds an i64-range value -- every construction
    /// funnels through `builtins::integer::int_value`, which demotes to
    /// `Int` whenever the value fits, keeping equality/hashing/matching
    /// canonical (a `BigInt(5)` can never exist alongside `Int(5)`).
    /// `class_id()` is `INTEGER_CLASS` -- one Ruby class, two payloads.
    /// Always-frozen immediate tier, like `Int`.
    BigInt(std::sync::Arc<num_bigint::BigInt>) = 16,
    Float(f64) = 3,
    /// A `Rational` -- always reduced, `den > 0`, bignum
    /// components; see `builtins::rational`. Always-frozen immediate tier.
    Rational(crate::builtins::rational::RRational) = 17,
    /// A `Complex` -- two components that keep their own
    /// numeric class (Integer|Float|Rational); see `builtins::complex`.
    /// Always-frozen immediate tier.
    Complex(crate::builtins::complex::RComplex) = 18,
    Symbol(Symbol) = 4,
    Str(RStr) = 19,
    Array(RArray) = 20,
    Hash(RHash) = 21,
    /// `a..b` / `a...b` -- see `builtins::range::RangeData`.
    Range(crate::builtins::range::RRange) = 22,
    Object(RObj) = 23,
    /// A real, escaping block/`Proc` -- see `rproc`'s module docs.
    Proc(RProc) = 24,
    /// A real, `regex`-crate-backed `Regexp` -- see
    /// `regexp`'s module docs.
    Regexp(RRegexp) = 25,
    /// A successful `Regexp#match`/`String#match` result.
    MatchData(RMatchData) = 26,
    /// A `Fiber` -- the Send+Sync HANDLE only; the actual
    /// coroutine is thread-pinned in `fiber::FIBERS` (see that module's
    /// docs for why it can't live here).
    Fiber(RFiber) = 27,
    /// An `Enumerator` -- captures `(receiver, method, args)`
    /// or an `Enumerator.new` generator block; external iteration state is
    /// a thread-pinned fiber, same split as `Fiber` (see
    /// `builtins::enumerator`'s module docs).
    Enumerator(crate::builtins::enumerator::REnumerator) = 28,
    /// An `Enumerator::Yielder` -- the `y` in
    /// `Enumerator.new { |y| y << 1 }`, wrapping the each-block currently
    /// being driven (`y << v` / `y.yield v` forward to it).
    Yielder(RProc) = 29,
    /// A `Thread` -- a real OS thread, truly parallel by default; see
    /// `thread`'s module docs.
    Thread(RThread) = 30,
    /// A Ruby `Mutex` -- non-reentrant, per-execution-context
    /// owned, like CRuby's.
    Mutex(RMutex) = 31,
    /// A `Queue` -- blocking pop, closable.
    Queue(RQueue) = 32,
    /// A `Ractor` -- a real OS thread with a frozen-or-copy
    /// message boundary; see `ractor`'s module docs.
    Ractor(RRactor) = 33,
    /// A first-class class/module VALUE -- `x = Widget`,
    /// `w.class`, a rescue binding's `.class`, classes stored in
    /// collections. `Copy` payload, always frozen (like the immediates);
    /// its Ruby-visible name/module-ness live in the `ClassRegistry`
    /// (`dispatch::class_name`/`class_is_module`), installed before any
    /// generated statement runs -- OR, for a class minted at runtime by
    /// `Class.new`, in the `runtime_meta` overlay (an id at/above
    /// `zeo_abi::RUNTIME_CLASS_ID_BASE`), which those same accessors
    /// consult. A runtime class's instances are `runtime_meta::DynObject`s.
    Class(ClassId) = 5,
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
        return if f > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    // Ruby switches to scientific notation when the shortest decimal's point
    // position `decpt` leaves `-3..=15` -- i.e. `|x| >= 1e15` or `|x| < 1e-4`
    // (`999999999999999.0` stays fixed but `1e15.to_s == "1.0e+15"`;
    // `Float::EPSILON == "2.220446049250313e-16"`). Rust's positional `{}`
    // never does, so the threshold is applied here. Mantissas keep at least one
    // fractional digit and positive exponents an explicit `+`, both Ruby's shapes.
    //
    // Exception (Ruby #2593): a value in `[1e15, 1e16)` that has a fractional
    // part -- a 16-digit integer part plus fraction, only possible below
    // 2**52 -- prints in fixed notation (`4503599627370495.5`), while an
    // integer-valued 16-digit double stays scientific (`1e15 -> "1.0e+15"`).
    let abs = f.abs();
    let fixed_16digit_fraction = (1e15..1e16).contains(&abs) && f.fract() != 0.0;
    if abs != 0.0 && !(1e-4..1e15).contains(&abs) && !fixed_16digit_fraction {
        let sci = format!("{f:e}");
        let (mantissa, exp) = sci.split_once('e').expect("{:e} always has an exponent");
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        // Ruby zero-pads the exponent to at least two digits (`5.0e-07`,
        // `1.0e+20`); Rust's `{:e}` does not.
        return if let Some(neg) = exp.strip_prefix('-') {
            format!("{mantissa}e-{neg:0>2}")
        } else {
            format!("{mantissa}e+{exp:0>2}")
        };
    }
    let s = f.to_string();
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// The pointer identity of a shared, potentially SELF-REFERENTIAL container
/// -- the one visited-set key every recursive `RubyValue` traversal in this
/// runtime uses (`to_display_string`/`inspect_string` here,
/// `ractor::shareable`/`make_shareable`'s cycle guards). `Arc`
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

/// [`RubyValue::class_id`] as dispatch OBSERVES it: a container husk left by
/// a `Ractor` move (whose variant class id cannot change) answers
/// `Ractor::MovedObject`, everything else its ordinary class. Gated on the
/// process-wide moved gate, so a program that never moves pays one
/// shared-byte load.
pub fn observed_class_id(v: &RubyValue) -> zeo_abi::ClassId {
    if crate::runtime_meta::any_moved() && crate::dispatch::value_moved(v) {
        return zeo_abi::RACTOR_MOVED_OBJECT_CLASS;
    }
    v.class_id()
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
pub(crate) fn default_object_repr(
    o: &crate::RObj,
    with_ivars: bool,
    seen: &mut Vec<usize>,
) -> Result<String, crate::Signal> {
    // The top-level `self` renders as `main`, never as an address -- see
    // `dispatch::is_main_object`. Both spellings, since ruby's singletons
    // cover `to_s` and `inspect` alike.
    if crate::dispatch::is_main_object(o) {
        return Ok("main".to_string());
    }
    let name = crate::dispatch::class_name(o.class_id()).unwrap_or_else(|| "Object".to_string());
    let addr = std::sync::Arc::as_ptr(o) as *const () as usize;
    if !with_ivars {
        return Ok(format!("#<{name}:0x{addr:016x}>"));
    }
    if seen.contains(&addr) {
        return Ok(format!("#<{name}:0x{addr:016x} ...>"));
    }
    let mut pairs = o.ivar_pairs();
    if let Some(keep) = ivars_to_inspect_filter(o)? {
        pairs.retain(|(n, _)| keep.contains(n.as_str()));
    }
    if pairs.is_empty() {
        return Ok(format!("#<{name}:0x{addr:016x}>"));
    }
    seen.push(addr);
    let body = pairs
        .iter()
        .map(|(n, v)| Ok(format!("{n}={}", v.inspect_with(seen)?)))
        .collect::<Result<Vec<_>, crate::Signal>>();
    seen.pop();
    Ok(format!("#<{name}:0x{addr:016x} {}>", body?.join(", ")))
}

/// The `#instance_variables_to_inspect` hook: `None` = show every ivar
/// (Kernel's default answered nil, or nothing overrode it); `Some(set)` = a
/// members-only filter. The set keeps the object's own ivar ORDER (the hook's
/// array is membership, not ordering -- oracle-pinned), Symbol entries only
/// (a String never matches), and a non-Array/nil answer is CRuby's TypeError.
fn ivars_to_inspect_filter(
    o: &crate::RObj,
) -> Result<Option<std::collections::HashSet<String>>, crate::Signal> {
    let name = crate::Symbol::intern("instance_variables_to_inspect");
    // Kernel's default row answers nil; only an OVERRIDE is worth a dispatch.
    match crate::dispatch::method_owner(o.class_id(), name) {
        Some(owner) if owner != zeo_abi::KERNEL_CLASS => {}
        _ => return Ok(None),
    }
    let recv = RubyValue::Object(o.clone());
    match crate::dispatch::send_value(&recv, name, &[], None)? {
        RubyValue::Nil => Ok(None),
        RubyValue::Array(a) => Ok(Some(
            a.lock()
                .iter()
                .filter_map(|v| match v {
                    RubyValue::Symbol(s) => Some(s.name()),
                    _ => None,
                })
                .collect(),
        )),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "Expected #instance_variables_to_inspect to return an Array or nil, but it returned {}",
                crate::builtins::class_name_of(&other)
            ),
        )),
    }
}

impl RubyValue {
    /// Whether this value is another object, and so a link a release can chain
    /// through -- see [`crate::IvarCell`]'s `Drop`.
    #[inline]
    pub(crate) fn links_to_object(&self) -> bool {
        matches!(self, RubyValue::Object(_))
    }

    /// Mirrors `sp_*_to_s`/CRuby's `Kernel#puts` argument stringification.
    ///
    /// INFALLIBLE wrapper over [`Self::try_display_string`] for the paths
    /// that have no exception channel (the `Debug` impl, error-message
    /// construction inside builtins): a user `to_s`/`inspect` that RAISES
    /// mid-render panics here. Every user-reachable display consumer
    /// (`puts`/`p`/`print`/interpolation/`format`/exception reporting)
    /// uses the fallible form, so the raise is catchable where Ruby says
    /// it is.
    pub fn to_display_string(&self) -> String {
        self.try_display_string().unwrap_or_else(|_| {
            panic!("a user-defined `to_s` raised inside an infallible display path")
        })
    }

    /// `to_display_string`'s fallible form: a user-defined `to_s` (or a
    /// nested element's `inspect`) that raises propagates as its `Signal`,
    /// exactly like CRuby's own `rb_obj_as_string` call chain.
    pub fn try_display_string(&self) -> Result<String, crate::Signal> {
        self.display_with(&mut Vec::new())
    }

    /// `to_display_string`'s recursive worker: `seen` is the traversal
    /// STACK of container identities (pushed on entry, popped on exit --
    /// not a permanent "already printed" set: a DAG that shares one array
    /// twice still prints it twice, like CRuby; only a genuine cycle hits
    /// the guard). A self-referential `Array`/`Hash` prints CRuby's own
    /// recursion markers (`[...]`/`{...}`) instead of deadlocking on its
    /// own non-reentrant payload `Mutex` (the pre-15.2 behavior).
    fn display_with(&self, seen: &mut Vec<usize>) -> Result<String, crate::Signal> {
        // A builtin-reopen `to_s` override wins -- real Ruby's
        // behavior for `puts`/interpolation, oracle-verified (`class
        // Integer; def to_s; "int"; end` makes `puts 5`/`"v=#{5}"` print
        // "int"). Object receivers keep their own registry probe in the
        // match below; the result's payload is taken directly when it's a
        // `Str` (re-dispatching would re-probe the same override forever
        // for an identity-shaped `to_s`). A RAISING override propagates --
        // `puts obj` with a raising `to_s` is a catchable exception in
        // Ruby, not a crash.
        if !matches!(self, RubyValue::Object(_))
            && crate::dispatch::has_display_reopen(self.class_id())
            && let Some(f) =
                crate::dispatch::value_method(self.class_id(), 0, crate::symbol::wk::to_s())
        {
            return match f.call(self, &[], None)? {
                RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
                other => other.display_with(seen),
            };
        }
        Ok(match self {
            RubyValue::Nil => String::new(),
            RubyValue::Bool(b) => b.to_string(),
            RubyValue::Int(i) => i.to_string(),
            RubyValue::BigInt(b) => b.to_string(),
            // `Rational#to_s` is "3/4"; `Complex#to_s` is "1+2i" -- the
            // parenthesized forms are inspect's (oracle-verified).
            RubyValue::Rational(r) => crate::builtins::rational::rat_to_s(r),
            RubyValue::Complex(c) => crate::builtins::complex::cpx_format(c, false)?,
            RubyValue::Float(f) => float_to_display_string(*f),
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            // `puts` on an `Array` recursively flattens and prints each
            // element on its own line (not `[1, 2, 3]`, which is `inspect`'s
            // job, not `to_s`'s) -- real, verified CRuby behavior, not a
            // simplification.
            // `Array#to_s` IS `#inspect` (`[1, 2]`), and so are `print`/
            // string interpolation of an Array. Only `puts` flattens onto
            // separate lines, and it does that in `io::render_puts`, never
            // through this display path.
            RubyValue::Array(a) => {
                let ptr = container_identity(self).expect("Array is a container");
                if seen.contains(&ptr) {
                    return Ok("[...]".to_string());
                }
                // Snapshot first: `seen` guards a container that contains
                // ITSELF, but not an element whose `inspect` reaches back into
                // this array. Holding the payload guard across that dispatch
                // deadlocks a non-reentrant Mutex.
                let items = crate::collections::array_snapshot(a);
                seen.push(ptr);
                let body = items
                    .iter()
                    .map(|e| e.inspect_with(seen))
                    .collect::<Result<Vec<_>, _>>();
                seen.pop();
                format!("[{}]", body?.join(", "))
            }
            // An approximation of `Hash#inspect` (symbol keys as `key:
            // value`, everything else as `key => value`) -- good enough for
            // the `Int`/`Symbol`-keyed hashes the common cases use, but
            // NOT a faithful `inspect` for nested `String`s (no quoting).
            // Same posture as `Object`'s "#<Object>" placeholder above: a
            // documented simplification, not silent wrongness.
            RubyValue::Hash(h) => {
                let ptr = container_identity(self).expect("Hash is a container");
                if seen.contains(&ptr) {
                    return Ok("{...}".to_string());
                }
                // Snapshot for the same reason as the Array arm above.
                let pairs = crate::collections::hash_pairs_snapshot(h);
                seen.push(ptr);
                // `Hash#to_s` IS `#inspect`, so keys and values render in
                // their inspect form (`{a: "x"}`, not `{a: x}`) here too.
                let body = pairs
                    .iter()
                    .map(|(k, v)| {
                        Ok(match k {
                            RubyValue::Symbol(s) => format!(
                                "{}: {}",
                                crate::builtins::symbol::hash_key(&s.name()),
                                v.inspect_with(seen)?
                            ),
                            _ => format!("{} => {}", k.inspect_with(seen)?, v.inspect_with(seen)?),
                        })
                    })
                    .collect::<Result<Vec<_>, crate::Signal>>();
                seen.pop();
                format!("{{{}}}", body?.join(", "))
            }
            RubyValue::Range(r) => {
                let (start, end, exclusive) = (&r.start, &r.end, r.exclusive);
                let s = match start {
                    Some(b) => b.display_with(seen)?,
                    None => String::new(),
                };
                let e = match end {
                    Some(b) => b.display_with(seen)?,
                    None => String::new(),
                };
                let op = if exclusive { "..." } else { ".." };
                format!("{s}{op}{e}")
            }
            // A user-defined `to_s` wins (dispatched through
            // the registry so inherited/mixed-in definitions resolve, and a
            // RAISING one propagates); the default is CRuby's
            // `#<Class:0xADDR>` (no ivars -- that's `inspect`'s job). The
            // address is normalized by the conformance harness (see
            // `default_object_repr`).
            RubyValue::Object(o) => {
                match crate::dispatch::call_user_method(o, "to_s", &[]) {
                    Some(v) => v?.display_with(seen)?,
                    // A value-builtin subclass (D3) with no `to_s` override
                    // renders as its payload (`Array#to_s` etc.).
                    None => match o.builtin_payload() {
                        Some(p) => p.display_with(seen)?,
                        None => default_object_repr(o, false, seen)?,
                    },
                }
            }
            // The real renderers, not placeholders: interpolation and `puts`
            // must agree with `#to_s` (`"#{pr}"` said "#<Proc>" while
            // `pr.to_s` said "#<Proc:0x...>").
            RubyValue::Proc(p) => crate::builtins::rproc::proc_inspect(p),
            // `Regexp#to_s` -- real Ruby's `(?opts-negopts:body)` form (NOT
            // the `/pattern/flags` literal form, that's `#inspect`'s job --
            // see `regexp::regexp_to_s`'s docs).
            RubyValue::Regexp(re) => crate::regexp::regexp_to_s(re).display_with(seen)?,
            // `MatchData#to_s` -- the whole matched substring.
            RubyValue::MatchData(m) => crate::regexp::matchdata_to_s(m).display_with(seen)?,
            RubyValue::Fiber(f) => crate::fiber::fiber_inspect(f),
            // `Object#to_s`'s address form -- inspect alone describes the
            // iteration.
            RubyValue::Enumerator(e) => crate::builtins::enumerator::enum_to_s(e),
            RubyValue::Yielder(_) => "#<Enumerator::Yielder>".to_string(),
            RubyValue::Thread(t) => {
                let status = if crate::thread::thread_alive(t) {
                    "run"
                } else {
                    "dead"
                };
                crate::thread::thread_inspect(t, status)
            }
            // `Object#to_s`'s address form (CRuby defines no `to_s`/`inspect`
            // on these, so the default applies to both -- inspect_with's
            // catch-all lands here too).
            RubyValue::Mutex(m) => {
                let addr = std::sync::Arc::as_ptr(m) as *const () as usize;
                format!("#<Thread::Mutex:0x{addr:016x}>")
            }
            RubyValue::Queue(q) => {
                let kind = if crate::thread::queue_is_sized(q) {
                    "Thread::SizedQueue"
                } else {
                    "Thread::Queue"
                };
                let addr = std::sync::Arc::as_ptr(q) as *const () as usize;
                format!("#<{kind}:0x{addr:016x}>")
            }
            RubyValue::Ractor(r) => crate::ractor::ractor_inspect(r),
            // The registered fully-qualified name (`puts Widget` ->
            // "Widget", `puts Store::Item` -> "Store::Item"); the id form
            // is only reachable registry-less (this crate's unit tests).
            RubyValue::Class(cid) => {
                crate::dispatch::class_name(*cid).unwrap_or_else(|| format!("#<Class:{}>", cid.0))
            }
        })
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
    /// INFALLIBLE wrapper -- same contract as [`Self::to_display_string`]
    /// (panics if a user `inspect` raises; the user-reachable consumers use
    /// [`Self::try_inspect_string`]).
    pub fn inspect_string(&self) -> String {
        self.try_inspect_string().unwrap_or_else(|_| {
            panic!("a user-defined `inspect` raised inside an infallible display path")
        })
    }

    /// `inspect_string`'s fallible form -- a raising user `inspect`
    /// propagates as its `Signal` (CRuby's `rb_inspect` behavior).
    pub fn try_inspect_string(&self) -> Result<String, crate::Signal> {
        self.inspect_with(&mut Vec::new())
    }

    /// `inspect_string`'s recursive worker -- same visited-STACK discipline
    /// as `display_with` (its docs explain the push/pop shape). The marker
    /// is chosen by the RECURRING container's own kind, so a cycle that
    /// enters through a Hash back into an outer Array prints `[...]` at the
    /// Array's re-entry point (`[1, {x: [...]}]`, oracle-verified).
    fn inspect_with(&self, seen: &mut Vec<usize>) -> Result<String, crate::Signal> {
        // A builtin-reopen `inspect` override wins -- and it
        // propagates into CONTAINER rendering too (`[5].inspect` ->
        // `[I<5>]` with an `Integer#inspect` override -- real Ruby's
        // `rb_inspect` dispatches per element, oracle-verified), which this
        // probe's position inside the recursive worker reproduces. Same
        // `Str`-payload shortcut as `display_with`'s probe; a raising
        // override propagates.
        if !matches!(self, RubyValue::Object(_))
            && crate::dispatch::has_display_reopen(self.class_id())
            && let Some(f) =
                crate::dispatch::value_method(self.class_id(), 0, crate::symbol::wk::inspect())
        {
            return match f.call(self, &[], None)? {
                RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
                other => other.display_with(seen),
            };
        }
        Ok(match self {
            RubyValue::Nil => "nil".to_string(),
            // A class renders by name -- plus the one suffix ruby's
            // `Module#inspect` adds where its `to_s` and `name` do not: a
            // `keyword_init: true` Struct class inspects as
            // `K(keyword_init: true)`. Without an arm of its own this fell
            // through to the `to_s` rendering and lost the suffix, so
            // `K.inspect` and `p K` disagreed.
            RubyValue::Class(cid) => {
                let mut n = crate::dispatch::class_name(*cid)
                    .unwrap_or_else(|| format!("#<Class:{}>", cid.0));
                if crate::builtins::rstruct::keyword_init_suffix(*cid) {
                    n.push_str("(keyword_init: true)");
                }
                n
            }
            // The parenthesized inspect forms (`(3/4)` / `(1+2i)`) vs the
            // bare `to_s` ones -- oracle-verified.
            RubyValue::Rational(r) => {
                format!("({})", crate::builtins::rational::rat_to_s(r))
            }
            RubyValue::Complex(c) => crate::builtins::complex::cpx_format(c, true)?,
            RubyValue::Symbol(s) => crate::builtins::symbol::inspect_name(&s.name()),
            RubyValue::Str(s) => crate::encoding::inspect(&s.lock()),
            RubyValue::Array(a) => {
                let ptr = container_identity(self).expect("Array is a container");
                if seen.contains(&ptr) {
                    return Ok("[...]".to_string());
                }
                // Snapshot before recursing -- see the `display_with` twin.
                let items = crate::collections::array_snapshot(a);
                seen.push(ptr);
                let body = items
                    .iter()
                    .map(|e| e.inspect_with(seen))
                    .collect::<Result<Vec<_>, _>>();
                seen.pop();
                format!("[{}]", body?.join(", "))
            }
            // Ruby 3.4+ `Hash#inspect` format: `{a: 1, "k" => 2}` -- symbol
            // keys as `name: value` with no braces-padding spaces.
            RubyValue::Hash(h) => {
                let ptr = container_identity(self).expect("Hash is a container");
                if seen.contains(&ptr) {
                    return Ok("{...}".to_string());
                }
                let pairs = crate::collections::hash_pairs_snapshot(h);
                seen.push(ptr);
                let body = pairs
                    .iter()
                    .map(|(k, v)| {
                        Ok(match k {
                            RubyValue::Symbol(s) => format!(
                                "{}: {}",
                                crate::builtins::symbol::hash_key(&s.name()),
                                v.inspect_with(seen)?
                            ),
                            _ => format!("{} => {}", k.inspect_with(seen)?, v.inspect_with(seen)?),
                        })
                    })
                    .collect::<Result<Vec<_>, crate::Signal>>();
                seen.pop();
                format!("{{{}}}", body?.join(", "))
            }
            RubyValue::Range(r) => {
                let (start, end, exclusive) = (&r.start, &r.end, r.exclusive);
                // An absent endpoint renders as nothing (`..5`, `1..`) -- EXCEPT
                // when both are absent, which ruby spells `nil..nil` so the
                // result is not the bare `..` that no literal can produce.
                let both_open = start.is_none() && end.is_none();
                let s = match start {
                    Some(b) => b.inspect_with(seen)?,
                    None if both_open => "nil".to_string(),
                    None => String::new(),
                };
                let e = match end {
                    Some(b) => b.inspect_with(seen)?,
                    None if both_open => "nil".to_string(),
                    None => String::new(),
                };
                let op = if exclusive { "..." } else { ".." };
                format!("{s}{op}{e}")
            }
            RubyValue::Regexp(re) => crate::regexp::regexp_inspect(re).display_with(seen)?,
            RubyValue::MatchData(m) => crate::regexp::matchdata_inspect(m),
            // A user-defined `inspect` wins (a raising one propagates); the
            // default is CRuby's `#<Class:0xADDR @iv=val, ...>` -- address
            // plus the object's ivars, each inspected, in field-declaration
            // order (see `default_object_repr`). NO fallback to a user
            // `to_s` (real Ruby's inspect is independent of to_s).
            RubyValue::Object(o) => {
                match crate::dispatch::call_user_method(o, "inspect", &[]) {
                    Some(v) => v?.display_with(seen)?,
                    // A value-builtin subclass (D3) with no `inspect` override
                    // inspects as its payload (`[1, 2, 3]`).
                    None => match o.builtin_payload() {
                        Some(p) => p.inspect_with(seen)?,
                        None => default_object_repr(o, true, seen)?,
                    },
                }
            }
            // Inspect DESCRIBES the iteration; `to_s` is the address form
            // (the `display_with` arm) -- ruby keeps the two apart here.
            RubyValue::Enumerator(e) => crate::builtins::enumerator::enum_inspect(e),
            // `Bool`/`Int`/`Float`/`Proc`: `#inspect` and `#to_s` agree
            // (or share the same placeholder approximation).
            other => other.display_with(seen)?,
        })
    }

    /// This value's runtime class -- the UNIVERSAL counterpart to
    /// `as_object_unchecked().class_id()` (which panics on anything but an
    /// `Object`): works for every variant, including the built-in primitive
    /// types, by returning their reserved, well-known `ClassId` (see
    /// `dispatch`'s `INTEGER_CLASS`/etc., mirrored on the `zeo` side by
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
            // A `class P < Proc` instance is still a `RubyValue::Proc`; the
            // class it answers rides in the proc itself.
            RubyValue::Proc(p) => p.class_id(),
            RubyValue::Regexp(_) => REGEXP_CLASS,
            RubyValue::MatchData(_) => MATCH_DATA_CLASS,
            RubyValue::Fiber(_) => FIBER_CLASS,
            RubyValue::Enumerator(e) => crate::builtins::enumerator::enumerator_class_id(e),
            RubyValue::Yielder(_) => crate::dispatch::YIELDER_CLASS,
            RubyValue::Thread(t) => crate::thread::thread_class_id(t),
            RubyValue::Mutex(_) => MUTEX_CLASS,
            RubyValue::Queue(q) => {
                if crate::thread::queue_is_sized(q) {
                    SIZED_QUEUE_CLASS
                } else {
                    QUEUE_CLASS
                }
            }
            RubyValue::Ractor(_) => RACTOR_CLASS,
            // `Widget.class` -> `Class`, `Enumerable.class` -> `Module`
            // (real Ruby; `Class < Module` handled by the registered
            // ancestor chain). Outside a generated program (no registry --
            // this crate's own unit tests) the class case is assumed.
            RubyValue::Class(cid) => {
                // A module a `class X < Module` minted answers X, not Module:
                // it IS an instance of X, and stays a real module besides. See
                // `runtime_meta::module_owner_class`.
                if let Some(owner) = crate::runtime_meta::module_owner_class(*cid) {
                    owner
                } else if crate::dispatch::class_is_refinement(*cid) {
                    zeo_abi::REFINEMENT_CLASS
                } else if crate::dispatch::class_is_module(*cid).unwrap_or(false) {
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
    /// Symbol at runtime -- this compiler has no static type-checker to catch
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
    /// the native `i64` to hand to `zeo_rt::int_*`. Panics (not
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
    #[inline]
    pub fn truthy(&self) -> bool {
        !matches!(self, RubyValue::Nil | RubyValue::Bool(false))
    }

    #[inline]
    pub fn is_nil(&self) -> bool {
        matches!(self, RubyValue::Nil)
    }

    /// Unwraps an `Array` payload -- the runtime counterpart to codegen's
    /// static `TyKind::Array` check, same posture as `as_int_unchecked`.
    pub fn as_array_unchecked(&self) -> RArray {
        self.as_array_ref().clone()
    }

    /// Borrowing form of [`Self::as_array_unchecked`]: most emission sites
    /// pass the handle straight to a `&RArray` parameter, and the owned form
    /// paid an atomic refcount round-trip per element access for a value
    /// that was never kept.
    #[inline]
    pub fn as_array_ref(&self) -> &RArray {
        match self {
            RubyValue::Array(a) => a,
            // The ruby frames name the site the wrong static type was
            // inferred FOR -- without them a violation in a large program is
            // undiagnosable (the rust backtrace only shows generated code).
            other => panic!(
                "expected an Array, got {}\nruby backtrace:\n{}",
                other.to_display_string(),
                crate::frames::capture_backtrace().join("\n")
            ),
        }
    }

    /// Unwraps a `Hash` payload -- see `as_array_unchecked`'s docs.
    pub fn as_hash_unchecked(&self) -> RHash {
        self.as_hash_ref().clone()
    }

    /// Borrowing form of [`Self::as_hash_unchecked`] -- see `as_array_ref`.
    #[inline]
    pub fn as_hash_ref(&self) -> &RHash {
        match self {
            RubyValue::Hash(h) => h,
            other => panic!("expected a Hash, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Str` payload -- see `as_array_unchecked`'s docs.
    pub fn as_str_unchecked(&self) -> RStr {
        self.as_str_ref().clone()
    }

    /// Borrowing form of [`Self::as_str_unchecked`] -- see `as_array_ref`.
    #[inline]
    pub fn as_str_ref(&self) -> &RStr {
        match self {
            RubyValue::Str(s) => s,
            other => panic!("expected a String, got {}", other.to_display_string()),
        }
    }

    /// Unwraps a `Proc` payload -- see `as_array_unchecked`'s docs.
    pub fn as_proc_unchecked(&self) -> RProc {
        self.as_proc_ref().clone()
    }

    /// Borrowing form of [`Self::as_proc_unchecked`] -- see `as_array_ref`.
    #[inline]
    pub fn as_proc_ref(&self) -> &RProc {
        match self {
            RubyValue::Proc(p) => p,
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

    /// A range's BEGIN endpoint, `nil` when beginless -- what `Range#begin`
    /// answers, and what a `for i in a..b` loop reads. NOT `Range#first`, which
    /// raises on a beginless range: see [`RubyValue::range_first_checked`].
    pub fn range_first(&self) -> RubyValue {
        match self {
            RubyValue::Range(r) => r.start.clone().unwrap_or(RubyValue::Nil),
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// A range's END endpoint, `nil` when endless -- the `Range#end` half of
    /// [`RubyValue::range_first`].
    pub fn range_last(&self) -> RubyValue {
        match self {
            RubyValue::Range(r) => r.end.clone().unwrap_or(RubyValue::Nil),
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// `Range#first` -- the endpoint, or the RangeError a beginless range
    /// raises. Codegen's typed fast path calls this rather than
    /// [`RubyValue::range_first`] so it cannot disagree with the table row.
    pub fn range_first_checked(&self) -> Result<RubyValue, crate::Signal> {
        match self {
            RubyValue::Range(r) => match r.start.as_ref() {
                Some(v) => Ok(v.clone()),
                None => Err(range_endpoint_error(true)),
            },
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// `Range#last` -- the `Range#first` half of
    /// [`RubyValue::range_first_checked`], raising on an ENDLESS range.
    pub fn range_last_checked(&self) -> Result<RubyValue, crate::Signal> {
        match self {
            RubyValue::Range(r) => match r.end.as_ref() {
                Some(v) => Ok(v.clone()),
                None => Err(range_endpoint_error(false)),
            },
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// `Range#exclude_end?`.
    pub fn range_exclude_end(&self) -> bool {
        match self {
            RubyValue::Range(r) => r.exclusive,
            other => panic!("expected a Range, got {}", other.to_display_string()),
        }
    }

    /// `==` -- real Ruby's protocol (retiring the documented
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
        // -- every numeric pair compares through the ONE tower matrix,
        // including the Bignum/Rational/Complex lanes. The
        // (Int, Int) arm below stays as the hot exact fast path.
        if let (RubyValue::Int(a), RubyValue::Int(b)) = (self, other) {
            return a == b;
        }
        if let Some(eq) = crate::builtins::numeric::num_eq(self, other) {
            return eq;
        }
        // A value-builtin subclass (D3) compares by its wrapped payload, so
        // `Tag.new("a")` equals `"a"` (symmetrically) and two subclass instances
        // with equal payloads are equal -- what structural `==` and Hash-key
        // `eql?` need. A `==` EXPRESSION dispatches a user override first (via
        // `send`); this is the low-level fallback comparison.
        if let RubyValue::Object(o) = self
            && let Some(p) = o.builtin_payload()
        {
            return p.rb_eq_guarded(other, seen);
        }
        if let RubyValue::Object(o) = other
            && let Some(p) = o.builtin_payload()
        {
            return self.rb_eq_guarded(&p, seen);
        }
        match (self, other) {
            (RubyValue::Nil, RubyValue::Nil) => true,
            (RubyValue::Bool(a), RubyValue::Bool(b)) => a == b,
            (RubyValue::Symbol(a), RubyValue::Symbol(b)) => a == b,
            // Class identity: `Widget == Widget`, and what
            // `Array#include?` on an array of classes consults.
            (RubyValue::Class(a), RubyValue::Class(b)) => a == b,
            (RubyValue::Str(a), RubyValue::Str(b)) => {
                // Address-ordered locking: two threads comparing `a == b` /
                // `b == a` must not take the locks in opposite orders
                // (parallel-mode deadlock); eq is symmetric, so which guard
                // comes first is free. Identity short-circuits (and keeps
                // the same-Arc case from self-deadlocking).
                if std::sync::Arc::ptr_eq(a, b) {
                    true
                } else {
                    let (x, y) = if std::sync::Arc::as_ptr(a) < std::sync::Arc::as_ptr(b) {
                        (a, b)
                    } else {
                        (b, a)
                    };
                    let gx = x.lock();
                    let gy = y.lock();
                    *gx == *gy
                }
            }
            // Real Ruby `Regexp#==`: same source pattern AND same flags.
            (RubyValue::Regexp(a), RubyValue::Regexp(b)) => {
                a.source == b.source
                    && a.ignore_case == b.ignore_case
                    && a.extended == b.extended
                    && a.multiline == b.multiline
            }
            // `Range#==`: equal (by `==`) endpoints and the same exclusivity.
            (RubyValue::Range(a), RubyValue::Range(b)) => {
                if std::sync::Arc::ptr_eq(a, b) {
                    return true;
                }
                let bounds_eq =
                    |x: &Option<RubyValue>,
                     y: &Option<RubyValue>,
                     seen: &mut Vec<(usize, usize)>| match (x, y) {
                        (None, None) => true,
                        (Some(x), Some(y)) => x.rb_eq_guarded(y, seen),
                        _ => false,
                    };
                a.exclusive == b.exclusive
                    && bounds_eq(&a.start, &b.start, seen)
                    && bounds_eq(&a.end, &b.end, seen)
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
                let av: Vec<RubyValue> = a.lock().to_vec();
                let bv: Vec<RubyValue> = b.lock().to_vec();
                let eq = av.len() == bv.len()
                    && av
                        .iter()
                        .zip(bv.iter())
                        .all(|(x, y)| x.rb_eq_guarded(y, seen));
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
                        crate::hash_has_key(b, k) && va.rb_eq_guarded(&crate::hash_get(b, k), seen)
                    });
                seen.pop();
                eq
            }
            // An `Object` receiver, real Ruby's resolution order: its
            // user-defined `==` (dispatched through the registry -- a
            // materialized method, so inherited/mixed-in definitions
            // resolve too), then `Comparable#==` derived from `<=>` for an
            // `include Comparable` class, else `Object#==`'s default:
            // reference identity. A user `==` that RAISES is stashed for
            // `rb_eq_checked` to surface as a real exception (see below).
            (RubyValue::Object(o), _) => {
                match crate::dispatch::call_user_method(o, "==", std::slice::from_ref(other)) {
                    Some(Ok(v)) => v.truthy(),
                    // A raising user `==` is stashed for the fallible
                    // `rb_eq_checked` wrapper to surface (`rb_eq` itself has
                    // no exception channel); a plain caller sees `false`.
                    Some(Err(sig)) => {
                        stash_cmp_signal(sig);
                        false
                    }
                    None if crate::dispatch::ancestors_contain(
                        o.class_id(),
                        crate::dispatch::COMPARABLE_CLASS,
                    ) =>
                    {
                        // `Comparable#==`: identity wins first (CRuby's
                        // `x == y` short-circuit), so a `<=>` that answers
                        // `nil` for self still reports an object equal to
                        // itself; a numeric result is equal iff its sign is 0;
                        // a non-numeric (or raising) `<=>` is an ArgumentError,
                        // stashed like the user-`==` case above.
                        match other {
                            RubyValue::Object(b) if std::sync::Arc::ptr_eq(o, b) => true,
                            _ => match crate::dispatch::call_user_method(
                                o,
                                "<=>",
                                std::slice::from_ref(other),
                            ) {
                                Some(Ok(v)) => match cmp_int(&v) {
                                    Ok(sign) => sign == Some(0),
                                    Err(sig) => {
                                        stash_cmp_signal(sig);
                                        false
                                    }
                                },
                                Some(Err(sig)) => {
                                    stash_cmp_signal(sig);
                                    false
                                }
                                None => false,
                            },
                        }
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
            // The concurrency handles compare by identity too -- what
            // `Thread.list.include?(Thread.current)` and Mutex/Queue membership
            // checks rely on.
            (RubyValue::Thread(a), RubyValue::Thread(b)) => std::sync::Arc::ptr_eq(a, b),
            (RubyValue::Mutex(a), RubyValue::Mutex(b)) => std::sync::Arc::ptr_eq(a, b),
            (RubyValue::Queue(a), RubyValue::Queue(b)) => std::sync::Arc::ptr_eq(a, b),
            // A numeric compared with a (non-numeric, non-builtin) object
            // reflects: CRuby's Integer#==/Float#== fall back to `other == self`
            // (rb_equal), so `5 == Wrapper.new(5)` reaches Wrapper#==. A raising
            // reflection is stashed like the Object-receiver arm above.
            (
                RubyValue::Int(_)
                | RubyValue::BigInt(_)
                | RubyValue::Float(_)
                | RubyValue::Rational(_)
                | RubyValue::Complex(_),
                RubyValue::Object(o),
            ) => match crate::dispatch::call_user_method(o, "==", std::slice::from_ref(self)) {
                Some(Ok(v)) => v.truthy(),
                Some(Err(sig)) => {
                    stash_cmp_signal(sig);
                    false
                }
                None => false,
            },
            _ => false,
        }
    }

    /// `a <=> b` as a signed ordering -- `None` is Ruby's
    /// `nil` (incomparable). Native Int/Float/String fast paths (CRuby's
    /// OPTIMIZED_CMP), then a user-defined `<=>` on an `Object` receiver;
    /// anything else is incomparable. Consumed by `Enumerable#min`/`#max`
    /// and `comparable`'s table rows (and the `sort` family).
    pub fn rb_cmp(&self, other: &RubyValue) -> Option<i64> {
        // Every numeric pair orders through the ONE tower matrix:
        // exact Int/Bignum/Rational lanes, Float promotion,
        // NaN -> nil, Complex -> nil.
        if let Some(ord) = crate::builtins::numeric::num_cmp(self, other) {
            return ord;
        }
        match (self, other) {
            // RAW BYTES, matching CRuby and matching `rb_eq` above. Comparing
            // `to_utf8_lossy` text collapsed every undecodable byte to U+FFFD,
            // so `"\xC3" <=> "\xC4"` answered 0 while `==` answered false.
            // Address-ordered locking behind a ptr-eq short-circuit, because
            // `sort` compares a pair from both directions.
            (RubyValue::Str(a), RubyValue::Str(b)) => {
                if std::sync::Arc::ptr_eq(a, b) {
                    return Some(0);
                }
                let forward = std::sync::Arc::as_ptr(a) < std::sync::Arc::as_ptr(b);
                let (x, y) = if forward { (a, b) } else { (b, a) };
                let gx = x.lock();
                let gy = y.lock();
                let ord = gx.bytes().cmp(gy.bytes()) as i64;
                Some(if forward { ord } else { -ord })
            }
            // Symbols order by their names, CRuby's `Symbol#<=>` -- what makes
            // `%i[b a].sort` and `{b: 1, a: 2}.sort` (which orders `[:key, v]`
            // pairs) work.
            (RubyValue::Symbol(a), RubyValue::Symbol(b)) => Some(a.name().cmp(&b.name()) as i64),
            // Arrays order lexicographically, element by element (CRuby's
            // `Array#<=>`) -- what lets `[[:b, 2], [:a, 1]].sort` and hence
            // `Hash#sort` work, since the generic comparison drivers use
            // `rb_cmp` rather than dispatching `Array#<=>`.
            (RubyValue::Array(a), RubyValue::Array(b)) => {
                // Sequential snapshots (the tuple form holds both guards to
                // statement end -- opposite-order deadlock under parallel
                // threads).
                let a = a.lock().clone();
                let b = b.lock().clone();
                for (x, y) in a.iter().zip(b.iter()) {
                    match x.rb_cmp(y) {
                        Some(0) => continue,
                        Some(c) => return Some(c),
                        None => return None,
                    }
                }
                Some((a.len() as i64 - b.len() as i64).signum())
            }
            (RubyValue::Object(o), _) => {
                match crate::dispatch::call_user_method(o, "<=>", std::slice::from_ref(other)) {
                    Some(Ok(v)) => cmp_sign(&v),
                    // `rb_cmp` is infallible, so a raising user `<=>` is
                    // stashed for the fallible `cmp_or_raise` to surface
                    // (the sort/min/max/Comparable drivers) rather than
                    // aborting; a plain infallible caller sees `None`.
                    Some(Err(e)) => {
                        stash_cmp_signal(e);
                        None
                    }
                    None => None,
                }
            }
            _ => None,
        }
    }

    /// `Kernel#frozen?`, universally over every variant,
    /// mirroring CRuby's own tiering: immediates (`Integer`/`Float`/
    /// `Symbol`/`nil`/`true`/`false`) are ALWAYS frozen (CRuby stores no
    /// flag for them at all -- `RB_FL_ABLE` is false, frozen is implied);
    /// `Range` is frozen-at-birth (real Ruby since 3.0, and this runtime's
    /// `Range` is a plain immutable value anyway); every other heap kind
    /// consults its payload's real stored flag. A `Queue`/`SizedQueue`
    /// cannot be frozen at all (`freeze` raises TypeError, oracle-verified),
    /// so it is the one heap kind that answers a constant `false`.
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
            | RubyValue::Range(..) => true,
            // A class/module object's flag lives in the id-keyed side
            // registry (`dispatch::class_frozen`) -- `Class` values are
            // `Copy` ids with no payload of their own to store it in.
            RubyValue::Class(cid) => crate::dispatch::class_frozen(*cid),
            RubyValue::Str(s) => s.is_frozen(),
            RubyValue::Array(a) => a.is_frozen(),
            RubyValue::Hash(h) => h.is_frozen(),
            RubyValue::Object(o) => o.is_frozen(),
            RubyValue::Proc(p) => p.is_frozen(),
            RubyValue::Regexp(re) => re.is_frozen(),
            RubyValue::MatchData(m) => m.is_frozen(),
            RubyValue::Fiber(f) => f.is_frozen(),
            RubyValue::Enumerator(e) => e.is_frozen(),
            // A yielder is a thin handle on the driving block; the flag
            // rides the underlying proc.
            RubyValue::Yielder(p) => p.is_frozen(),
            RubyValue::Thread(t) => t.is_frozen(),
            RubyValue::Mutex(m) => m.is_frozen(),
            RubyValue::Queue(_) => false,
            RubyValue::Ractor(r) => r.is_frozen(),
        }
    }

    /// `Kernel#freeze`: SHALLOW (sets only this value's own flag, never
    /// recursing into elements -- deep freeze is `Ractor.make_shareable`'s
    /// job, a later phase), returns self (a cheap handle clone), and is a
    /// silent no-op on anything already/always frozen -- all three verified
    /// against CRuby's `rb_obj_freeze` (`object.c:1360`). Fallible because
    /// `Queue`/`SizedQueue` REFUSE to freeze (`cannot freeze #<Thread::Queue:
    /// 0x...>`, a TypeError -- CRuby's rb_obj_freeze override for queues).
    pub fn freeze_value(&self) -> Result<RubyValue, crate::Signal> {
        match self {
            RubyValue::Str(s) => s.set_frozen(),
            RubyValue::Array(a) => a.set_frozen(),
            RubyValue::Hash(h) => h.set_frozen(),
            RubyValue::Object(o) => o.set_frozen(),
            RubyValue::Proc(p) | RubyValue::Yielder(p) => p.set_frozen(),
            RubyValue::Regexp(re) => re.set_frozen(),
            RubyValue::MatchData(m) => m.set_frozen(),
            RubyValue::Fiber(f) => f.set_frozen(),
            RubyValue::Enumerator(e) => e.set_frozen(),
            RubyValue::Thread(t) => t.set_frozen(),
            RubyValue::Mutex(m) => m.set_frozen(),
            RubyValue::Queue(q) => {
                // CRuby's message renders the receiver's default inspect,
                // address included (the same form the display arm builds).
                let kind = if crate::thread::queue_is_sized(q) {
                    "Thread::SizedQueue"
                } else {
                    "Thread::Queue"
                };
                let addr = std::sync::Arc::as_ptr(q) as *const () as usize;
                return Err(type_error!("cannot freeze #<{kind}:0x{addr:016x}>"));
            }
            RubyValue::Ractor(r) => r.set_frozen(),
            // A class/module: the id-keyed side registry (see `is_frozen`).
            RubyValue::Class(cid) => crate::dispatch::class_set_frozen(*cid),
            // Always-frozen immediates/`Range` -- the silent no-op tier.
            RubyValue::Nil
            | RubyValue::Bool(_)
            | RubyValue::Int(_)
            | RubyValue::BigInt(_)
            | RubyValue::Rational(_)
            | RubyValue::Complex(_)
            | RubyValue::Float(_)
            | RubyValue::Symbol(_)
            | RubyValue::Range(..) => {}
        }
        Ok(self.clone())
    }

    /// `Kernel#dup`/`#clone` -- the per-kind SHALLOW copy, the
    /// one semantic difference between the two being the frozen flag:
    /// `clone` (`copy_frozen: true`) carries it over, `dup` never does
    /// (oracle-verified: `"abc".freeze.dup.frozen?` is `false`,
    /// `.clone.frozen?` is `true`). Copies are shallow exactly like CRuby's
    /// `rb_obj_dup`: a nested element/value/ivar is SHARED with the
    /// original (`arr.dup[1].equal?(arr[1])`, oracle-verified), only the
    /// top-level container is fresh.
    ///
    /// Per-kind rules (each oracle-verified against ruby 4.0.6):
    /// - Immediates/`Symbol`: `dup`/`clone` return self (real Ruby).
    /// - `Str`/`Array`/`Hash`: fresh payload, fresh (or copied) flag.
    /// - `Range`: immutable here and in Ruby -- self suffices (the fresh
    ///   object identity real Ruby mints is unobservable in this runtime).
    /// - `Object`: per-class `RubyObject::dup_object` (see `ruby_class!`).
    /// - `Proc`/`Regexp`/`MatchData`: a fresh payload sharing the immutable
    ///   innards (closure/engine/haystack), so the copy's frozen flag is its
    ///   own -- freezing the original never freezes an earlier `dup`,
    ///   CRuby's rule.
    /// - `Mutex`: a fresh, unlocked mutex (real Ruby's `Mutex#dup` gives
    ///   exactly that -- allocate + no state ivars).
    /// - `Thread`/`Ractor`: `TypeError: allocator undefined`;
    ///   `Queue`/`SizedQueue`: `NoMethodError: undefined method
    ///   'initialize_copy'`; a mid-iteration `Enumerator`: `TypeError:
    ///   can't copy execution context` -- all real, rescuable raises
    ///   (messages oracle-verified against ruby 4.0.6).
    /// - `Fiber`: succeeds, yielding an UNINITIALIZED fiber -- CRuby's
    ///   shallow copy skips the machine stack, so resuming the copy raises
    ///   `FiberError: uninitialized fiber` while the original still works
    ///   (oracle-verified; see `fiber::dup_uninitialized`).
    pub fn dup_value(&self, copy_frozen: bool) -> Result<RubyValue, crate::Signal> {
        let keep_frozen = copy_frozen && self.is_frozen();
        let copy = match self {
            RubyValue::Nil
            | RubyValue::Bool(_)
            | RubyValue::Int(_)
            | RubyValue::BigInt(_)
            | RubyValue::Rational(_)
            | RubyValue::Complex(_)
            | RubyValue::Float(_)
            | RubyValue::Symbol(_)
            | RubyValue::Range(..) => self.clone(),
            // A MODULE or CLASS really is copied: the reason to dup one is to
            // edit the copy, and sharing the original's id makes every such
            // edit hit the original (delegate.rb's `::Kernel.dup` then
            // undefines `inspect`/`to_s`/... off the real Kernel). The class
            // copy is ANONYMOUS, so naming it does not rename the source.
            RubyValue::Class(cid) => {
                if crate::dispatch::class_is_module(*cid).unwrap_or(false) {
                    crate::runtime_meta::runtime_module_dup(*cid)?
                } else {
                    crate::runtime_meta::runtime_class_dup(*cid, copy_frozen)?
                }
            }
            RubyValue::Proc(p) => RubyValue::Proc(p.dup_data(keep_frozen)),
            RubyValue::Regexp(re) => RubyValue::Regexp(re.dup_data(keep_frozen)),
            RubyValue::MatchData(m) => RubyValue::MatchData(m.dup_data(keep_frozen)),
            RubyValue::Str(s) => {
                // Copy the BUFFER -- bytes and encoding both. Rebuilding the
                // copy from `to_utf8_lossy` text made `dup` re-encode: a
                // BINARY 0x8B came back as UTF-8 0xC2 0x8B, so a duplicated
                // compressed stream or packed record was silently a different
                // string. Same rule as `String#+` (see `builtins::string`).
                let fresh = crate::collections::string_wrap(s.lock().clone());
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Str(fresh)
            }
            RubyValue::Array(a) => {
                let fresh = crate::array_new(a.lock().to_vec());
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Array(fresh)
            }
            RubyValue::Hash(h) => {
                let pairs: Vec<(RubyValue, RubyValue)> = h.lock().values().cloned().collect();
                let fresh = crate::hash_new(pairs);
                crate::collections::copy_hash_meta(h, &fresh);
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Hash(fresh)
            }
            RubyValue::Object(o) => {
                let c = RubyValue::Object(o.dup_object(copy_frozen));
                // Data copies stay frozen through every copy (#2716) -- the
                // hook-free fast path applies it here; the `Kernel` rows do
                // the same after their hooks.
                crate::builtins::rstruct::refreeze_data_copy(&c);
                c
            }
            RubyValue::Mutex(_) => {
                let fresh = crate::mutex_new();
                if keep_frozen && let RubyValue::Mutex(m) = &fresh {
                    m.set_frozen();
                }
                fresh
            }
            // A never-iterated (or finished) enumerator copies as a fresh
            // one over the same source; a LIVE iteration can't be copied.
            RubyValue::Enumerator(e) => {
                if e.iteration_live() {
                    return Err(type_error!("can't copy execution context"));
                }
                let fresh = e.fresh_copy();
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Enumerator(fresh)
            }
            // A yielder is just a handle onto the driving block -- the
            // reference copy is indistinguishable (the Proc posture above).
            RubyValue::Yielder(_) => self.clone(),
            RubyValue::Fiber(f) => {
                let fresh = crate::fiber::dup_uninitialized(f);
                if keep_frozen {
                    fresh.set_frozen();
                }
                RubyValue::Fiber(fresh)
            }
            RubyValue::Thread(_) => {
                return Err(type_error!("allocator undefined for Thread"));
            }
            RubyValue::Queue(q) => {
                let kind = if crate::thread::queue_is_sized(q) {
                    "Thread::SizedQueue"
                } else {
                    "Thread::Queue"
                };
                return Err(crate::dispatch::raise_error(
                    "NoMethodError",
                    format!("undefined method 'initialize_copy' for an instance of {kind}"),
                ));
            }
            RubyValue::Ractor(_) => {
                return Err(type_error!("allocator undefined for Ractor"));
            }
        };
        // A bare heap value's ivars live BESIDE it (see `value_ivars`), so the
        // copy has to be told about them -- Ruby carries ivars across both
        // `dup` and `clone`; only singletons are `clone`-only. Here rather than
        // in `Kernel#dup` because codegen folds a `dup` with no user override
        // straight to this call and never reaches the row.
        crate::value_ivars::copy(self, &copy);
        Ok(copy)
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
        // A Symbol is a legal subject too (CRuby coerces it via `rb_sym2str`),
        // and anything else CLEARS `$~` rather than leaving a stale match.
        if let RubyValue::Regexp(re) = self {
            return match subject {
                RubyValue::Str(s) => crate::regexp::regexp_case_eq(re, &s.lock().to_utf8_lossy()),
                RubyValue::Symbol(sym) => crate::regexp::regexp_case_eq(re, &sym.name()),
                _ => {
                    crate::lastmatch::set_last_match(None);
                    false
                }
            };
        }
        // `Range#===` is `#cover?` (`when 1..5`): each
        // present endpoint compares via `rb_cmp` (numeric tower, strings,
        // user `<=>`), and an incomparable subject is `false`, real Ruby's
        // rule (`(1..5) === "x"` is false, not an error; oracle-verified,
        // incl. `(1..6) === 5.5` true and `(1...5) === 5` false).
        if let RubyValue::Range(r) = self {
            return range_covers(r.start.as_ref(), r.end.as_ref(), r.exclusive, subject);
        }
        // `Module#===`: `case x when Integer` / `when Widget`
        // is an instance-of-ancestry check, NOT equality (`Widget ===
        // Widget` is false in real Ruby -- a class is not an instance of
        // itself). Registry-backed; only reachable in generated programs,
        // which always install one.
        if let RubyValue::Class(cid) = self {
            return crate::dispatch::is_a_value(subject, *cid);
        }
        self.rb_eq(subject)
    }
}

/// `Range#cover?`'s core (also `Range#===`/`case when 1..5`): begin <=
/// subject (< or <=) end, via `rb_cmp` (numeric tower, strings, user
/// `<=>`); a `nil` comparison (incomparable) is `false`, real Ruby's rule.
/// Beginless/endless sides are unbounded.
/// Reduce any `<=>`/comparison-block result to its sign, CRuby's
/// `rb_cmpint`: a numeric answer (`Int`/`BigInt`/`Float`/`Rational`) gives
/// its sign relative to zero, anything else (nil, a non-numeric, NaN) is
/// incomparable (`None`). The one place the Int-only cut lives,
/// shared by `rb_cmp`, `comparable::cmp`, and the sort/min/max drivers.
pub(crate) fn cmp_sign(result: &RubyValue) -> Option<i64> {
    crate::builtins::numeric::num_cmp(result, &RubyValue::Int(0)).flatten()
}

/// CRuby's `rb_cmpint`, distinguishing the two incomparable outcomes a bare
/// sign can't: a `nil` result is `Ok(None)` (the caller decides -- `==`
/// yields `false`, ordering raises `comparison of <recv> with <arg> failed`),
/// while any other non-numeric result is itself the `comparison of <class>
/// with 0 failed` ArgumentError (`rb_cmpint` reaching for `result > 0`).
pub(crate) fn cmp_int(result: &RubyValue) -> Result<Option<i64>, crate::Signal> {
    if result.is_nil() {
        return Ok(None);
    }
    match cmp_sign(result) {
        Some(sign) => Ok(Some(sign)),
        None => Err(cmp_error(result, &RubyValue::Int(0))),
    }
}

/// CRuby's `rb_cmperr` message: the receiver always shows its class; the
/// argument shows its `inspect` when it's an immediate (Integer/Float/
/// Symbol/…) and its class otherwise (`comparison of String with 1 failed`
/// vs `comparison of Integer with String failed`).
pub(crate) fn cmp_error(a: &RubyValue, b: &RubyValue) -> crate::Signal {
    let shown = match b {
        RubyValue::Nil
        | RubyValue::Bool(_)
        | RubyValue::Int(_)
        | RubyValue::BigInt(_)
        | RubyValue::Float(_)
        | RubyValue::Rational(_)
        | RubyValue::Complex(_)
        | RubyValue::Symbol(_) => b.inspect_string(),
        _ => crate::builtins::class_name_of(b),
    };
    arg_error!(
        "comparison of {} with {} failed",
        crate::builtins::class_name_of(a),
        shown
    )
}

/// `a <=> b` reduced to a sign, raising `ArgumentError` when the pair is
/// incomparable. An `Object` receiver dispatches its own `<=>` so a raising
/// user `<=>` propagates as its `Signal` (unlike the infallible `rb_cmp`,
/// which aborts); every other receiver takes the fast structural path.
pub(crate) fn cmp_or_raise(a: &RubyValue, b: &RubyValue) -> Result<i64, crate::Signal> {
    if let RubyValue::Object(o) = a {
        // Call the receiver's own `<=>` (not `send_value`, whose missing-
        // method NoMethodError would mask CRuby's `Object#<=>` default of
        // "incomparable"): a raising `<=>` propagates, a value is validated
        // through `rb_cmpint`, and a missing one is incomparable -- except
        // an object always equals itself (`Object#<=>` identity -> 0).
        return match crate::dispatch::call_user_method(o, "<=>", std::slice::from_ref(b)) {
            Some(Ok(v)) => cmp_int(&v)?.ok_or_else(|| cmp_error(a, b)),
            Some(Err(sig)) => Err(sig),
            None => match b {
                RubyValue::Object(bb) if std::sync::Arc::ptr_eq(o, bb) => Ok(0),
                _ => Err(cmp_error(a, b)),
            },
        };
    }
    // Clear before comparing, then surface any raising `<=>` that `rb_cmp`
    // stashed -- including one nested inside an `Array#<=>` element compare.
    clear_cmp_signal();
    let ord = a.rb_cmp(b);
    if let Some(sig) = take_cmp_signal() {
        return Err(sig);
    }
    ord.ok_or_else(|| cmp_error(a, b))
}

/// `a == b` with a fallible channel: an infallible `rb_eq` stashes a raising
/// user `==`/`<=>` (or a non-numeric `Comparable#==` result); this clears
/// before comparing and turns any stash back into the `Signal` it was.
/// `case`/`when`, an `in` value/pin pattern, and `grep`: the case-equality
/// test, DISPATCHING the pattern's own `===`.
///
/// `rb_case_eq` answers this natively for the builtin patterns and stays the
/// fast path for them, but it cannot be the whole story: `===` is the one
/// operator `case` exists to let a user define, and a native ladder ending in
/// `rb_eq` silently ignores it. A user class whose `===` differs from its `==`
/// (`class Even; def ===(n) = n.even?; end`) took the WRONG branch with no
/// error, and a singleton `===` on a class (`def Trip.===`) was ignored the
/// same way -- so both an `Object` pattern and a `Class` one go through
/// dispatch. A `Class` still resolves to `Module#===`'s ancestry check when
/// nothing overrides it, so the common `when Integer` keeps its meaning.
/// A range endpoint as ruby stores it. An explicit `nil` IS the open end, so
/// `nil..5` and `..5` are the SAME range -- equal, identically inspected, and
/// alike beginless. Normalizing once at construction is what keeps `first`,
/// `min`, `size`, `==` and `inspect` from each having to ask twice; without it
/// `(1..nil).min` reads a bounded range and iterates forever.
#[inline]
pub fn range_endpoint(v: RubyValue) -> Option<RubyValue> {
    match v {
        RubyValue::Nil => None,
        other => Some(other),
    }
}

/// Range construction with CRuby's endpoint check (`range_init`): two present
/// endpoints must answer `begin <=> end`, else `ArgumentError: bad value for
/// range` -- what makes a runtime `(true..false)` raise. The `rb_eq` fallback
/// is `Kernel#<=>`'s own rule (0 when `==`), which is how `(true..true)`
/// stays legal. Codegen emits this only where comparability isn't a static
/// fact; the literal `1..10` keeps the unchecked constructor.
pub fn range_checked(
    b: Option<RubyValue>,
    e: Option<RubyValue>,
    exclusive: bool,
) -> Result<RubyValue, crate::Signal> {
    if let (Some(x), Some(y)) = (&b, &e)
        && x.rb_cmp(y).is_none()
        && !x.rb_eq(y)
    {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "bad value for range".to_string(),
        ));
    }
    Ok(crate::builtins::range::range_value(b, e, exclusive))
}

/// The RangeError `Range#first`/`#last` raise when the endpoint they want is
/// open -- one function so the two messages stay a matched pair.
pub fn range_endpoint_error(first: bool) -> crate::Signal {
    let (which, side) = if first {
        ("first", "beginless")
    } else {
        ("last", "endless")
    };
    crate::dispatch::raise_error(
        "RangeError",
        format!("cannot get the {which} element of {side} range"),
    )
}

pub fn case_eq(pattern: &RubyValue, subject: &RubyValue) -> Result<bool, crate::Signal> {
    match pattern {
        RubyValue::Object(_) | RubyValue::Class(_) => {
            let matched = crate::dispatch::send_value(
                pattern,
                crate::symbol::wk::case_eq(),
                std::slice::from_ref(subject),
                None,
            )?;
            Ok(matched.truthy())
        }
        // `Proc#===` calls the proc/lambda with the subject (`case x when
        // ->(v) { ... }`) -- the subject can be any value (e.g. a Class), so
        // this rides the normal `call`, not the integer fast-path ABI.
        RubyValue::Proc(p) => Ok(p.call(std::slice::from_ref(subject))?.truthy()),
        _ => Ok(pattern.rb_case_eq(subject)),
    }
}

/// `when *candidates` -- `case_eq` against each element, short-circuiting on
/// the first hit exactly as the listed `when a, b, c` form's `||` chain does.
/// A free function rather than an `.any(..)` closure at the call site because
/// each test is now fallible.
pub fn case_eq_any(candidates: &RubyValue, subject: &RubyValue) -> Result<bool, crate::Signal> {
    let elems = candidates.as_array_unchecked().lock().clone();
    for candidate in elems.iter() {
        if case_eq(candidate, subject)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn rb_eq_checked(a: &RubyValue, b: &RubyValue) -> Result<bool, crate::Signal> {
    clear_cmp_signal();
    let eq = a.rb_eq(b);
    match take_cmp_signal() {
        Some(sig) => Err(sig),
        None => Ok(eq),
    }
}

thread_local! {
    /// A raising user `<=>`/`==` caught by the infallible `rb_cmp`/`rb_eq`,
    /// awaiting a fallible driver (`cmp_or_raise`/`rb_eq_checked`) to turn it
    /// back into a `Signal`.
    static PENDING_CMP: std::cell::RefCell<Option<crate::Signal>> =
        const { std::cell::RefCell::new(None) };
}

fn stash_cmp_signal(sig: crate::Signal) {
    PENDING_CMP.with(|c| *c.borrow_mut() = Some(sig));
}
fn clear_cmp_signal() {
    PENDING_CMP.with(|c| *c.borrow_mut() = None);
}
fn take_cmp_signal() -> Option<crate::Signal> {
    PENDING_CMP.with(|c| c.borrow_mut().take())
}

pub(crate) fn range_covers(
    start: Option<&RubyValue>,
    end: Option<&RubyValue>,
    exclusive: bool,
    subject: &RubyValue,
) -> bool {
    // A `nil` endpoint (`(nil.."m")`, or an unset ivar range bound) is an OPEN
    // side, exactly like a missing one -- never compared, always widens.
    let lower_ok = match start {
        Some(s) if !s.is_nil() => matches!(subject.rb_cmp(s), Some(c) if c >= 0),
        _ => true,
    };
    if !lower_ok {
        return false;
    }
    match end {
        Some(e) if !e.is_nil() => match subject.rb_cmp(e) {
            Some(c) if exclusive => c < 0,
            Some(c) => c <= 0,
            None => false,
        },
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Symbol, array_new, array_push, hash_new, hash_set, string_new};

    fn sym(name: &str) -> RubyValue {
        RubyValue::Symbol(Symbol::intern(name))
    }

    #[test]
    fn rb_cmp_orders_symbols_and_arrays() {
        assert_eq!(sym("a").rb_cmp(&sym("b")), Some(-1));
        assert_eq!(sym("b").rb_cmp(&sym("a")), Some(1));
        assert_eq!(sym("x").rb_cmp(&sym("x")), Some(0));
        // Arrays order lexicographically -- the pair shape `Hash#sort` needs.
        let p1 = RubyValue::Array(array_new(vec![sym("a"), RubyValue::Int(1)]));
        let p2 = RubyValue::Array(array_new(vec![sym("b"), RubyValue::Int(2)]));
        assert_eq!(p1.rb_cmp(&p2), Some(-1));
        // A shorter prefix sorts before its extension.
        let short = RubyValue::Array(array_new(vec![RubyValue::Int(1)]));
        let long = RubyValue::Array(array_new(vec![RubyValue::Int(1), RubyValue::Int(0)]));
        assert_eq!(short.rb_cmp(&long), Some(-1));
    }

    #[test]
    fn cmp_sign_reduces_any_numeric_result_to_its_sign() {
        assert_eq!(cmp_sign(&RubyValue::Int(7)), Some(1));
        assert_eq!(cmp_sign(&RubyValue::Int(-3)), Some(-1));
        assert_eq!(cmp_sign(&RubyValue::Int(0)), Some(0));
        assert_eq!(cmp_sign(&RubyValue::Float(2.5)), Some(1));
        assert_eq!(cmp_sign(&RubyValue::Float(-0.1)), Some(-1));
        // Non-numeric and nil are not signs.
        assert_eq!(cmp_sign(&RubyValue::Nil), None);
        assert_eq!(cmp_sign(&sym("x")), None);
        assert_eq!(cmp_sign(&RubyValue::Float(f64::NAN)), None);
    }

    #[test]
    fn cmp_int_distinguishes_nil_from_a_numeric_result() {
        // A numeric result is its sign; nil is `Ok(None)` (incomparable, the
        // caller decides). The non-numeric-raises path is covered by the
        // corpus tests, where the exception factory is installed (registry-
        // less here, `cmp_error` would panic).
        assert_eq!(cmp_int(&RubyValue::Float(-4.0)).unwrap(), Some(-1));
        assert_eq!(cmp_int(&RubyValue::Int(0)).unwrap(), Some(0));
        assert_eq!(cmp_int(&RubyValue::Nil).unwrap(), None);
    }

    /// Every expected string below is oracle-verified against
    /// real ruby 4.0.6 (`p`/`puts` on the same graphs).
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

    /// `Array#to_s`/`print`/interpolation share `inspect`'s bracketed form
    /// (only `puts` flattens, in `io::render_puts`); the cycle guard renders
    /// the self-reference as its marker.
    #[test]
    fn display_marks_a_self_referential_array() {
        let a = array_new(vec![RubyValue::Int(1), RubyValue::Int(2)]);
        array_push(&a, RubyValue::Array(a.clone()));

        assert_eq!(RubyValue::Array(a).to_display_string(), "[1, 2, [...]]");
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
        s.freeze_value().unwrap();

        assert!(
            !s.dup_value(false).unwrap().is_frozen(),
            "dup of frozen is unfrozen"
        );
        assert!(
            s.dup_value(true).unwrap().is_frozen(),
            "clone of frozen stays frozen"
        );

        let unfrozen = RubyValue::Str(string_new("abc".to_string()));
        assert!(
            !unfrozen.dup_value(true).unwrap().is_frozen(),
            "clone of unfrozen stays unfrozen"
        );
    }

    /// The newly-flagged heap kinds follow the same tiering: unfrozen at
    /// birth, latch on `freeze`, fresh flag through `dup` (freezing the
    /// original never freezes an earlier copy) -- and `Queue` refuses to
    /// freeze at all (TypeError, CRuby's rule). All oracle-verified.
    #[test]
    fn freeze_covers_the_handle_kinds_and_queue_refuses() {
        let p = RubyValue::Proc(crate::RProc::new(|_| Ok(RubyValue::Nil)));
        assert!(!p.is_frozen());
        let d = p.dup_value(false).unwrap();
        p.freeze_value().unwrap();
        assert!(p.is_frozen(), "proc latches");
        assert!(!d.is_frozen(), "earlier dup keeps its own flag");
        assert!(
            p.dup_value(true).unwrap().is_frozen(),
            "clone copies the flag"
        );
        assert!(
            !p.dup_value(false).unwrap().is_frozen(),
            "dup drops the flag"
        );

        let m = crate::mutex_new();
        m.freeze_value().unwrap();
        assert!(m.is_frozen());
    }

    /// `Queue#freeze` refuses (TypeError in CRuby); registry-less unit
    /// tests see `raise_error`'s loud-panic fallback. The exact rescuable
    /// message shape is pinned by the e2e freeze suite.
    #[test]
    #[should_panic(expected = "TypeError: cannot freeze #<Thread::Queue:0x")]
    fn queue_refuses_to_freeze() {
        let q = crate::queue_new();
        let _ = q.freeze_value();
    }

    /// The copy's top-level payload is FRESH (mutating it leaves the
    /// original untouched)...
    #[test]
    fn dup_copies_the_top_level_payload() {
        let a = array_new(vec![RubyValue::Int(1)]);
        let copy = RubyValue::Array(a.clone()).dup_value(false).unwrap();
        array_push(&copy.as_array_unchecked(), RubyValue::Int(2));

        assert_eq!(a.lock().len(), 1);
        assert_eq!(copy.as_array_unchecked().lock().len(), 2);

        let h = hash_new(vec![(sym("a"), RubyValue::Int(1))]);
        let hcopy = RubyValue::Hash(h.clone()).dup_value(false).unwrap();
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
        let copy = RubyValue::Array(outer).dup_value(false).unwrap();

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
            assert!(v.dup_value(false).unwrap().rb_eq(&v));
            assert!(v.dup_value(true).unwrap().rb_eq(&v));
        }
        let r = crate::builtins::range::range_value(
            Some(RubyValue::Int(1)),
            Some(RubyValue::Int(3)),
            false,
        );
        assert_eq!(r.dup_value(false).unwrap().inspect_string(), "1..3");
    }

    /// Element-wise `==` (retiring "conservatively compare
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

    /// One representative of the raising tier (Thread/Ractor raise
    /// `allocator undefined`, Queue/SizedQueue `initialize_copy`; Queue is
    /// the only one constructible on its own). Registry-less unit tests see
    /// `raise_error`'s loud-panic
    /// fallback; the rescuable shape is pinned by e2e.
    #[test]
    #[should_panic(
        expected = "NoMethodError: undefined method 'initialize_copy' for an instance of Thread::Queue"
    )]
    fn dup_of_a_queue_raises() {
        let _ = crate::queue_new().dup_value(false);
    }

    #[test]
    fn dup_of_a_mutex_makes_a_fresh_unlocked_mutex() {
        let m = crate::mutex_new();
        let copy = m.dup_value(false).unwrap();
        let (RubyValue::Mutex(a), RubyValue::Mutex(b)) = (&m, &copy) else {
            panic!("expected two mutexes")
        };
        assert!(!std::sync::Arc::ptr_eq(a, b), "fresh mutex, not an alias");
    }
}

/// The `zeo_abi::abi` layout contract, asserted against the real types --
/// the Cranelift backend reads/writes values through these numbers, so a
/// drift here is generated code corrupting memory.
#[cfg(test)]
mod abi_layout {
    use super::*;
    use crate::{array_new, hash_new, string_new};
    use std::mem::{align_of, size_of};
    use zeo_abi::abi::{self, ValueTag};

    fn tag(v: &RubyValue) -> u8 {
        unsafe { *(v as *const RubyValue as *const u8).add(abi::TAG_OFFSET) }
    }

    fn payload<T: Copy>(v: &RubyValue) -> T {
        unsafe {
            (v as *const RubyValue as *const u8)
                .add(abi::PAYLOAD_OFFSET)
                .cast::<T>()
                .read()
        }
    }

    #[test]
    fn value_size_and_align() {
        assert_eq!(size_of::<RubyValue>(), abi::VALUE_SIZE);
        assert_eq!(align_of::<RubyValue>(), abi::VALUE_ALIGN);
        // The niches `repr(C, u8)` must not cost: `Option<RubyValue>` stays
        // one value wide, and the `Result` every body returns today stays
        // 32 bytes.
        assert_eq!(size_of::<Option<RubyValue>>(), abi::VALUE_SIZE);
        assert_eq!(size_of::<Result<RubyValue, crate::Signal>>(), 32);
        // `abi::Value` (the opaque slot in the C fn-pointer mirrors) must be
        // interchangeable with `RubyValue` -- what makes `register_program`'s
        // fn-pointer transmute sound.
        assert_eq!(size_of::<abi::Value>(), size_of::<RubyValue>());
        assert_eq!(align_of::<abi::Value>(), align_of::<RubyValue>());
    }

    #[test]
    fn immediate_tags_and_payload_offsets() {
        // The six inline-known payloads: tag byte at 0, payload at 8.
        assert_eq!(tag(&RubyValue::Nil), ValueTag::Nil as u8);
        let b = RubyValue::Bool(true);
        assert_eq!(tag(&b), ValueTag::Bool as u8);
        assert_eq!(payload::<u8>(&b), 1);
        let i = RubyValue::Int(0x1122_3344_5566_7788);
        assert_eq!(tag(&i), ValueTag::Int as u8);
        assert_eq!(payload::<i64>(&i), 0x1122_3344_5566_7788);
        let f = RubyValue::Float(1.5);
        assert_eq!(tag(&f), ValueTag::Float as u8);
        assert_eq!(payload::<f64>(&f), 1.5);
        let sym = Symbol::intern("abi_layout_probe");
        let s = RubyValue::Symbol(sym);
        assert_eq!(tag(&s), ValueTag::Symbol as u8);
        assert_eq!(payload::<u32>(&s), sym.to_u32());
        let c = RubyValue::Class(ClassId(7));
        assert_eq!(tag(&c), ValueTag::Class as u8);
        assert_eq!(payload::<u32>(&c), 7);
        // All six sit in the memcpy band.
        for v in [&RubyValue::Nil, &b, &i, &f, &s, &c] {
            assert!(tag(v) < abi::FIRST_HEAP_TAG);
        }
    }

    #[test]
    fn heap_tags() {
        // Representative heap variants (the rest are pinned by their
        // explicit source discriminants, which mirror `ValueTag` verbatim;
        // Fiber/Thread/Ractor values need live runtime machinery a unit
        // test shouldn't spin up).
        let cases = [
            (
                RubyValue::BigInt(std::sync::Arc::new(num_bigint::BigInt::from(1) << 80)),
                ValueTag::BigInt,
            ),
            (RubyValue::Str(string_new("x".to_string())), ValueTag::Str),
            (RubyValue::Array(array_new(vec![])), ValueTag::Array),
            (RubyValue::Hash(hash_new(vec![])), ValueTag::Hash),
        ];
        for (v, want) in &cases {
            assert_eq!(tag(v), *want as u8);
            assert!(tag(v) >= abi::FIRST_HEAP_TAG);
        }
    }

    #[test]
    fn site_struct_sizes() {
        assert_eq!(size_of::<crate::CallSite>(), abi::CALLSITE_SIZE);
        assert_eq!(size_of::<crate::DynCallerSite>(), abi::DYNCALLER_SITE_SIZE);
        assert_eq!(
            size_of::<crate::ClassMethodSite>(),
            abi::CLASSMETHOD_SITE_SIZE
        );
        assert_eq!(size_of::<crate::ConstSite>(), abi::CONST_SITE_SIZE);
        assert_eq!(size_of::<crate::CivarSite>(), abi::CIVAR_SITE_SIZE);
        assert_eq!(size_of::<crate::RegexpSite>(), abi::REGEXP_SITE_SIZE);
        assert_eq!(size_of::<crate::ffi::FfiSymSite>(), abi::FFISYM_SITE_SIZE);
        for a in [
            align_of::<crate::CallSite>(),
            align_of::<crate::DynCallerSite>(),
            align_of::<crate::ClassMethodSite>(),
            align_of::<crate::ConstSite>(),
            align_of::<crate::CivarSite>(),
            align_of::<crate::RegexpSite>(),
            align_of::<crate::ffi::FfiSymSite>(),
        ] {
            assert_eq!(a, abi::SITE_ALIGN);
        }
    }
}
