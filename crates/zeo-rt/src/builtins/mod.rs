//! The builtin method tables -- one Rust module per Ruby core
//! class/module, mirroring CRuby's file-per-class source layout (string.c,
//! array.c, compar.c, ...). `struct`/`class`/`module` are Rust keywords, so
//! Struct follows the crate's existing `rproc.rs` precedent (`rstruct`) and
//! Class+Module share `class_module`.
//!
//! Architecture (the module-faithful placement the phase is named for):
//! every method is implemented ONCE, in the module/class that OWNS it in
//! CRuby -- `Comparable`'s 7 methods drive the receiver's own `<=>`,
//! `Enumerable`'s drive `each`, `Kernel` carries the universals `Object`
//! never owned -- and `send_value`/`send` find them by walking the
//! receiver's REAL ancestor chain (`dispatch::ancestors_of_value`), user
//! reopens first PER ANCESTOR. A table row therefore never needs to know
//! which concrete class it's serving.
//!
//! Every table function validates its own arity and argument types, raising
//! real rescuable `ArgumentError`/`TypeError` through
//! `dispatch::raise_error` -- CRuby's own behavior (`"a" + 1` is a
//! TypeError, not NoMethodError).

use crate::{ClassId, RubyValue, Signal};
use std::sync::LazyLock;

pub(crate) mod argf;
pub(crate) mod array;
pub(crate) mod backtrace_location;
pub(crate) mod basic_object;
pub(crate) mod binding;
pub(crate) mod comparable;
pub(crate) mod complex;
pub(crate) mod condition_variable;
pub(crate) mod convert;
pub(crate) mod converter;
pub(crate) mod data;
pub(crate) mod dir;
pub(crate) mod encoding;
pub(crate) mod enumerable;
pub(crate) mod enumerator;
pub(crate) mod env;
pub(crate) mod exception;
pub(crate) mod false_class;
pub(crate) mod fiber;
pub(crate) mod file;
pub(crate) mod file_test;
pub(crate) mod float;
pub(crate) mod format;
pub(crate) mod formatter;
pub(crate) mod gc;
pub(crate) mod hash;
pub(crate) mod integer;
pub(crate) mod io;
pub(crate) mod io_buffer;
pub(crate) mod io_console;
pub(crate) mod kernel;
pub(crate) mod lazy;
pub(crate) mod marshal;
pub(crate) mod matchdata;
pub(crate) mod math;
pub(crate) mod method;
pub(crate) mod mutex;
pub(crate) mod nil_class;
pub(crate) mod numeric;
pub(crate) mod objspace;
pub(crate) mod pack;
pub(crate) mod pathname;
pub(crate) mod process;
pub(crate) mod queue;
pub(crate) mod random;
pub(crate) mod range;
pub(crate) mod rational;
pub(crate) mod rclass;
pub(crate) mod refinement;
pub(crate) mod regexp;
pub(crate) mod rmodule;
pub(crate) mod rproc;
pub(crate) mod rstruct;
pub(crate) mod ruby_box;
pub(crate) mod rubyvm;
pub(crate) mod rubyvm_ast;
pub(crate) mod set;
pub(crate) mod signal;
pub(crate) mod sized_queue;
pub(crate) mod stat;
pub(crate) mod string;
pub(crate) mod symbol;
pub(crate) mod thread;
pub(crate) mod thread_group;
pub(crate) mod time;
pub(crate) mod true_class;
pub(crate) mod unbound_method;
pub(crate) mod value_subclass;
pub(crate) mod waiter;
pub(crate) mod warning;
pub(crate) mod weak;
pub(crate) mod yielder;

/// One builtin method: receiver (guaranteed by the table's ClassId keying
/// to be the right variant), positional args, optional block. Deliberately
/// the SAME shape as `ValueMethodFn` (the reopen trampolines) --
/// one trampoline ABI for everything the MRO walk can find.
pub type BuiltinMethodFn =
    fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

/// One method surface (instance OR class): the same drift-free set every
/// `ruby_class!`/`ruby_module!` table derives from a single row set.
///
/// Four parallel `match`es on `&str` rather than one array of rows. That looks
/// like the wrong shape and reads like it too, but it measures smaller: rustc
/// buckets a string match by length and compares against inline literals, where
/// an array of rows costs two relocations an entry and cannot be stripped per
/// row. Collapsing these into one sorted `&[MethodRow]` was tried and measured
/// +59,744 bytes on a `hello` binary, so it stayed as it is.
pub struct MethodTable {
    pub lookup: fn(&str) -> Option<BuiltinMethodFn>,
    pub names: fn() -> &'static [&'static str],
    pub arity: fn(&str) -> Option<i64>,
    /// CRuby-private: reachable through implicit self/`send`/`super`, but
    /// invisible to `respond_to?` and to reflection. `false` for all but the
    /// `module_function` instance copies today.
    pub is_private: fn(&str) -> bool,
    /// CRuby-protected: reachable only from a receiver the CALLER is a kind
    /// of. A separate fn rather than a widened `is_private`, because the two
    /// readers ask the two questions separately.
    pub is_protected: fn(&str) -> bool,
    /// CLASS-method rows only: this one allocates through the RECEIVER class,
    /// so a subclass receiver gets an instance of itself. CRuby keeps the same
    /// knowledge inside each class-method C function -- see
    /// `zeo_dsl::MethodDef::allocs`. `false` for every instance table and for
    /// every class-method row that answers the base class.
    pub allocs: fn(&str) -> bool,
}

/// One builtin class/module's tables, registered by the `ruby_class!`/
/// `ruby_module!` macro and collected at link time into [`BUILTIN_TABLES`].
///
/// The routing fns (`class_table` etc.) consult the registered table FIRST and
/// fall back to the hand-written `ClassId`->lookup `match` arms below only for
/// the few classes still served by hand -- so a class registered here is fully
/// described by its own file.
pub struct BuiltinClassTable {
    pub id: ClassId,
    /// Instance methods (`class_table`/`class_arity_table`/`class_table_names`).
    pub instance: Option<MethodTable>,
    /// Class/singleton methods (`class_method_table`/`class_method_table_names`).
    pub class: Option<MethodTable>,
    /// Seeds this class's constants at startup; `None` when it declares none.
    pub install_constants: Option<fn()>,
}

/// Every `ruby_class!`/`ruby_module!` class self-registers here; linkme
/// gathers them into one slice at final link (whether the runtime is linked as
/// an rlib or a dylib -- all elements live inside `zeo-rt`, so the slice is
/// self-contained either way).
#[linkme::distributed_slice]
pub static BUILTIN_TABLES: [BuiltinClassTable] = [..];

/// The registered tables indexed by `ClassId` for O(1) routing, built once
/// from the link-time-collected slice.
///
/// A DIRECT index, not a hash probe: `ClassId`s are dense small integers, and
/// the ABI already indexes `BUILTINS` by `id.0 - 1` on the same grounds. This
/// is asked per ANCESTOR by `instance_method_visibility`, `responds_to` and
/// `scan_owner`, so the hash was paid once per step of every MRO walk.
pub(crate) fn registered_table(id: ClassId) -> Option<&'static BuiltinClassTable> {
    static BY_ID: LazyLock<Vec<Option<&'static BuiltinClassTable>>> = LazyLock::new(|| {
        let len = BUILTIN_TABLES
            .iter()
            .map(|t| t.id.0 as usize)
            .max()
            .map_or(0, |m| m + 1);
        let mut v = vec![None; len];
        for t in BUILTIN_TABLES {
            v[t.id.0 as usize] = Some(t);
        }
        v
    });
    BY_ID.get(id.0 as usize).copied().flatten()
}

/// The static ClassId -> method-table map. A plain match (rustc compiles it
/// to a jump table); `None` for user classes and for builtins with no table
/// yet. `Enumerable`/`Comparable` are ordinary rows here too -- the MRO
/// walk reaches them as ancestors of Array/Hash/Range and of any user class
/// that `include`s them, exactly like every other builtin module.
pub(crate) fn class_table(id: ClassId) -> Option<fn(&str) -> Option<BuiltinMethodFn>> {
    // A macro-registered class is fully described by its own table and has no
    // match arm below, so consult the registry first.
    if let Some(t) = registered_table(id) {
        return t.instance.as_ref().map(|m| m.lookup);
    }
    Some(match id {
        #[cfg(feature = "ext-date")]
        zeo_abi::DATETIME_CLASS => crate::ext::date::lookup,
        _ => return None,
    })
}

/// `class_table`'s arity twin: ClassId -> the instance-method arity table
/// (`<lookup>_arity`, generated beside every `lookup`).
/// `Method#arity` consults this for a builtin-receiver method object, walking
/// the receiver's ancestry so an inherited builtin resolves against its owner.
pub(crate) fn class_arity_table(id: ClassId) -> Option<fn(&str) -> Option<i64>> {
    if let Some(t) = registered_table(id) {
        return t.instance.as_ref().map(|m| m.arity);
    }
    Some(match id {
        #[cfg(feature = "ext-date")]
        zeo_abi::DATETIME_CLASS => crate::ext::date::lookup_arity,
        _ => return None,
    })
}

/// The static ClassId -> CLASS-METHOD table map -- `class_table`'s
/// counterpart for methods invoked on the class/module VALUE itself
/// (`File.read`, `Time.now`, `Dir.pwd`, `Math.sqrt`), as opposed to on an
/// instance.
///
/// Needed because this runtime has no singleton-method tables: an instance
/// method table is keyed by the receiver's class, but the receiver of
/// `File.read` is `RubyValue::Class(FILE_CLASS)`, whose own class is
/// `Class` -- so the ordinary MRO walk looks at Class/Module and never at
/// File. `send_value_in` probes this table first for a `RubyValue::Class`
/// receiver, which is how `Math.sqrt` and `GC` resolve.
///
/// The `&RubyValue` a row receives is the CLASS VALUE itself, not an
/// instance -- rows generally ignore it (`Time.now` needs no receiver), but
/// it keeps the `BuiltinMethodFn` ABI uniform with `class_table`'s.
pub(crate) fn class_method_table(id: ClassId) -> Option<fn(&str) -> Option<BuiltinMethodFn>> {
    if let Some(t) = registered_table(id) {
        return t.class.as_ref().map(|m| m.lookup);
    }
    Some(match id {
        // In-tree `ext/` extensions, each behind its `ext-<name>` cargo feature.
        #[cfg(feature = "ext-date")]
        zeo_abi::DATETIME_CLASS => crate::ext::date::lookup_class,
        // The `YAML` alias id has no table of its own, so it routes to psych.
        #[cfg(feature = "ext-psych")]
        zeo_abi::YAML_MODULE => crate::ext::psych::lookup_class,
        _ => return None,
    })
}

/// `class_method_table`'s arity twin -- what `Foo.method(:bar).arity` reads
/// for a builtin class method, the singleton mirror of [`class_arity_table`].
pub(crate) fn class_method_arity_table(id: ClassId) -> Option<fn(&str) -> Option<i64>> {
    if let Some(t) = registered_table(id) {
        return t.class.as_ref().map(|m| m.arity);
    }
    // Only the hand-written tables that declare arities appear here; the rest
    // of `class_method_table`'s arms are hand-rolled `lookup_class` fns with
    // no arity twin, so their class methods report the `-1` catch-all.
    Some(match id {
        zeo_abi::FILE_TEST_MODULE => file::lookup_class_arity,
        #[cfg(feature = "ext-psych")]
        zeo_abi::YAML_MODULE => crate::ext::psych::lookup_class_arity,
        _ => return None,
    })
}

/// Whether the builtin instance method `name` on `id` is CRuby-private --
/// reachable through implicit self/`send`/`super`, invisible to reflection.
///
/// `module_function` is the whole story today: CRuby defines the instance copy
/// private, so `Math.instance_methods(false)` is empty while
/// `Math.private_instance_methods(false)` lists 28.
pub(crate) fn class_method_is_private(id: ClassId, name: &str) -> bool {
    registered_table(id)
        .and_then(|t| t.instance.as_ref())
        .is_some_and(|m| (m.is_private)(name))
}

/// Whether the builtin instance method `name` on `id` is CRuby-PROTECTED --
/// `Pathname#path` is the whole population, made protected so `#to_s` is the
/// way to spell a path out loud.
pub(crate) fn class_method_is_protected(id: ClassId, name: &str) -> bool {
    registered_table(id)
        .and_then(|t| t.instance.as_ref())
        .is_some_and(|m| (m.is_protected)(name))
}

/// The same question for a builtin CLASS-method row (`private def self."x"`)
/// -- prism's `serialize_parse` and friends, the backend seam the gem's own
/// Ruby calls with implicit self and nothing outside should see.
pub(crate) fn builtin_class_method_is_private(id: ClassId, name: &str) -> bool {
    registered_table(id)
        .and_then(|t| t.class.as_ref())
        .is_some_and(|m| (m.is_private)(name))
}

/// Whether a builtin CLASS-method row allocates through the RECEIVER class --
/// what decides if a value subclass re-tags the result as itself. See
/// `zeo_dsl::MethodDef::allocs`; the row itself carries the answer, as the
/// equivalent C function does in CRuby.
pub(crate) fn builtin_class_method_allocs(id: ClassId, name: &str) -> bool {
    registered_table(id)
        .and_then(|t| t.class.as_ref())
        .is_some_and(|m| (m.allocs)(name))
}

/// `class_table`'s reflection companion: the instance-method NAMES a builtin
/// class exposes (for `instance_methods`/`methods`). Mirrors `class_table`'s
/// arms exactly -- each `<mod>::lookup` has a paste-generated `<mod>::lookup_names`.
pub(crate) fn class_table_names(id: ClassId) -> &'static [&'static str] {
    if let Some(t) = registered_table(id) {
        return t.instance.as_ref().map(|m| (m.names)()).unwrap_or(&[]);
    }
    match id {
        #[cfg(feature = "ext-date")]
        zeo_abi::DATETIME_CLASS => crate::ext::date::lookup_names(),
        _ => &[],
    }
}

/// `class_method_table`'s reflection companion: the CLASS-method NAMES a
/// builtin exposes (for `SomeClass.singleton_methods` / `.methods`).
pub(crate) fn class_method_table_names(id: ClassId) -> &'static [&'static str] {
    if let Some(t) = registered_table(id) {
        return t.class.as_ref().map(|m| (m.names)()).unwrap_or(&[]);
    }
    match id {
        zeo_abi::FILE_TEST_MODULE => file::lookup_class_names(),
        _ => &[],
    }
}

/// Registry-FREE ancestor chains for the builtin classes, computed once
/// from the ABI's own declarative `superclass`/`includes` edges with the
/// same linearization rule as `analyze::mro::compute_ancestors` (self, then
/// includes reversed -- each recursively expanded -- then the parent's
/// chain; first occurrence wins). What lets the MRO walk (and this crate's
/// own unit tests) run without an installed `ClassRegistry`; a real
/// generated program's registry carries identical chains for builtins
/// (both derive from the one ABI table) plus richer ones for user classes.
pub(crate) fn fallback_ancestors(id: ClassId) -> &'static [ClassId] {
    static CHAINS: LazyLock<crate::FMap<u32, Vec<ClassId>>> = LazyLock::new(|| {
        fn edges(id: ClassId) -> (&'static [ClassId], Option<ClassId>) {
            if id == zeo_abi::OBJECT_CLASS {
                return (zeo_abi::OBJECT_INCLUDES, Some(zeo_abi::OBJECT_SUPERCLASS));
            }
            let b = &zeo_abi::BUILTINS[(id.0 as usize) - 1];
            (b.includes, b.superclass)
        }
        fn linearize(id: ClassId, out: &mut Vec<ClassId>) {
            if out.contains(&id) {
                return;
            }
            // A prepended module sits AHEAD of the class's own methods, so it
            // is linearized before the class rather than after it.
            for &(owner, modules) in zeo_abi::BUILTIN_PREPENDS {
                if owner == id {
                    for &m in modules.iter().rev() {
                        linearize(m, out);
                    }
                }
            }
            if out.contains(&id) {
                return;
            }
            out.push(id);
            let (includes, parent) = edges(id);
            for &inc in includes.iter().rev() {
                linearize(inc, out);
            }
            if let Some(p) = parent {
                linearize(p, out);
            }
        }
        let mut chains = crate::FMap::default();
        for id in
            std::iter::once(zeo_abi::OBJECT_CLASS).chain(zeo_abi::BUILTINS.iter().map(|b| b.id))
        {
            let mut chain = Vec::new();
            linearize(id, &mut chain);
            chains.insert(id.0, chain);
        }
        chains
    });
    CHAINS.get(&id.0).map_or(&[], |c| c.as_slice())
}

/// The Ruby class name of `v` for error messages -- registry name for user
/// objects, ABI name for everything else.
pub(crate) fn class_name_of(v: &RubyValue) -> String {
    crate::dispatch::class_name(v.class_id())
        .or_else(|| zeo_abi::builtin_name(v.class_id()).map(str::to_string))
        .unwrap_or_else(|| format!("#<Class:{}>", v.class_id().0))
}

/// How a numeric-coercion TypeError names the offending operand -- CRuby's
/// `coerce_failed` rule: special constants (nil/true/false, a fixnum
/// Integer, a Symbol) and Floats read as their `inspect` form (`nil can't
/// be coerced into Float`), every other value as its class name (`String
/// can't be coerced into Float`). A Bignum is NOT a special constant, so
/// it keeps the class-name form, matching zeo's `Int`/`BigInt` payload
/// split exactly.
pub(crate) fn coerce_operand_name(v: &RubyValue) -> String {
    match v {
        RubyValue::Nil
        | RubyValue::Bool(_)
        | RubyValue::Int(_)
        | RubyValue::Float(_)
        | RubyValue::Symbol(_) => v.inspect_string(),
        _ => class_name_of(v),
    }
}

/// CRuby's `rb_check_frozen` over any VALUE receiver: the standard
/// `can't modify frozen <Class>: <inspect>` FrozenError (with the receiver
/// detail attached, backing `FrozenError#receiver`), raised BEFORE the
/// caller mutates anything. The value-level counterpart to the per-file
/// collection guards (`guard_str_frozen`/`guard_hash_frozen`/array's
/// `check_frozen`), which keep their handle-shaped signatures.
pub(crate) fn check_frozen(recv: &RubyValue) -> Result<(), crate::Signal> {
    if recv.is_frozen() {
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!(
                "can't modify frozen {}: {}",
                class_name_of(recv),
                recv.inspect_string()
            ),
            &[("receiver", recv.clone())],
        ));
    }
    Ok(())
}

/// The name CRuby uses for `v` in a coercion `TypeError` -- "no implicit
/// conversion of X into Y" / "can't convert X into Y". CRuby renders `nil`,
/// `true`, and `false` as those literals rather than their class names
/// (`NilClass`/`TrueClass`/`FalseClass`); every other object uses its class
/// name. Use this, not `class_name_of`, when building those messages.
pub(crate) fn convert_name_of(v: &RubyValue) -> String {
    match v {
        RubyValue::Nil => "nil".to_string(),
        RubyValue::Bool(true) => "true".to_string(),
        RubyValue::Bool(false) => "false".to_string(),
        _ => class_name_of(v),
    }
}

/// The typed error constructors: `type_error!("no implicit conversion...")`
/// over `raise_error("TypeError", format!(...))`, so the class name is spelled
/// once here (never typo-able per site) and call sites read as what they
/// raise. Each takes `format!` arguments and yields a `Signal` -- wrap in
/// `Err(...)` exactly as with `raise_error`. Classes raised from only one
/// site (Errno::*, ext-specific classes) stay on `raise_error` directly.
/// (Written flat rather than macro-generated: `$$` meta-variable escaping is
/// still unstable.)
macro_rules! type_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("TypeError", format!($($fmt)*)) };
}
macro_rules! arg_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("ArgumentError", format!($($fmt)*)) };
}
macro_rules! name_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("NameError", format!($($fmt)*)) };
}
macro_rules! index_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("IndexError", format!($($fmt)*)) };
}
macro_rules! range_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("RangeError", format!($($fmt)*)) };
}
macro_rules! runtime_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("RuntimeError", format!($($fmt)*)) };
}
macro_rules! frozen_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("FrozenError", format!($($fmt)*)) };
}
macro_rules! io_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("IOError", format!($($fmt)*)) };
}
macro_rules! eof_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("EOFError", format!($($fmt)*)) };
}
macro_rules! thread_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("ThreadError", format!($($fmt)*)) };
}
macro_rules! regexp_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("RegexpError", format!($($fmt)*)) };
}
macro_rules! local_jump_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("LocalJumpError", format!($($fmt)*)) };
}
macro_rules! float_domain_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("FloatDomainError", format!($($fmt)*)) };
}
macro_rules! not_impl_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("NotImplementedError", format!($($fmt)*)) };
}
pub(crate) use {
    arg_error, eof_error, float_domain_error, frozen_error, index_error, io_error,
    local_jump_error, name_error, not_impl_error, range_error, regexp_error, runtime_error,
    thread_error, type_error,
};

/// The argument-count guard the `ruby_class!` macro emits from a def's
/// parameter list. `max: None` means a `*rest` accepts any number.
///
/// One shared helper rather than an inlined `format!` per call site: the
/// message is only ever built on the failing path, and LLVM otherwise lays that
/// path ahead of the hot body, so a four-line builtin spends its first cache
/// line on error construction.
#[inline(always)]
pub(crate) fn check_arity(given: usize, min: usize, max: Option<usize>) -> Result<(), Signal> {
    if given < min || max.is_some_and(|hi| given > hi) {
        return Err(arity_err(given, min, max));
    }
    Ok(())
}

/// CRuby's exact wording, oracle-verified against 4.0.6: `expected 2` for a
/// fixed count, `expected 1..2` for a range, `expected 2+` when a splat leaves
/// the maximum open.
#[cold]
#[inline(never)]
pub(crate) fn arity_err(given: usize, min: usize, max: Option<usize>) -> Signal {
    let expected = match max {
        Some(hi) if hi == min => format!("{min}"),
        Some(hi) => format!("{min}..{hi}"),
        None => format!("{min}+"),
    };
    crate::dispatch::raise_error(
        "ArgumentError",
        format!("wrong number of arguments (given {given}, expected {expected})"),
    )
}

/// Receiver unwrappers for the HELPER fns that take a raw `&RubyValue` and so
/// have no class header to unwrap it for them. A `ruby_class!` def names the
/// unwrapped receiver directly instead -- see the `receiver` header line, which
/// replaced 234 of these calls.
///
/// The table's ClassId keying guarantees the variant either way, so a mismatch
/// is a dispatch bug, not a user error.
macro_rules! recv_str {
    ($recv:expr_2021) => {
        match $recv {
            crate::RubyValue::Str(s) => s,
            _ => unreachable!("String table row dispatched on a non-String receiver"),
        }
    };
}
pub(crate) use recv_str;

macro_rules! recv_hash {
    ($recv:expr_2021) => {
        match $recv {
            crate::RubyValue::Hash(h) => h,
            _ => unreachable!("Hash table row dispatched on a non-Hash receiver"),
        }
    };
}
pub(crate) use recv_hash;

/// Argument coercion guards -- CRuby's exact TypeError shape.
/// Argument `$i` as an `i64` through the full implicit-conversion protocol
/// (`convert::to_index`): `Int` fast path, `to_int` duck types accepted,
/// CRuby's TypeError for the rest and RangeError for bignum-range answers.
macro_rules! arg_int {
    ($args:expr_2021, $i:literal) => {
        match &$args[$i] {
            crate::RubyValue::Int(v) => *v,
            other => crate::builtins::convert::to_index(other)?,
        }
    };
    // The same, for a parameter the def named rather than an index into the
    // raw slice.
    ($v:expr_2021) => {
        match $v {
            crate::RubyValue::Int(v) => *v,
            other => crate::builtins::convert::to_index(other)?,
        }
    };
}
pub(crate) use arg_int;

/// Argument `$i` as a string handle through the protocol (`convert::to_rstr`):
/// `Str` fast path (an `Arc` bump), `to_str` duck types accepted, CRuby's
/// TypeError for the rest.
macro_rules! arg_str {
    ($v:expr_2021) => {
        match $v {
            crate::RubyValue::Str(s) => s.clone(),
            other => crate::builtins::convert::to_rstr(other)?,
        }
    };
}
pub(crate) use arg_str;

/// The block -- or, blockless, an early return with the ENUMERATOR every
/// iteration method answers in real Ruby (retiring the
/// "would return an Enumerator" panics): the enumerator
/// captures `(recv, method-name, args)` and re-invokes the method when
/// iterated (`rb_enumeratorize`'s rule).
macro_rules! block_or_enum {
    // The ordinary form: the method name comes from the def that owns the body
    // (`__RUBY_METHOD`, bound by `ruby_class!`), so it cannot drift from it.
    ($recv:expr_2021, $args:expr_2021, $block:expr_2021) => {
        crate::builtins::block_or_enum!($recv, __RUBY_METHOD, $args, $block)
    };
    // The explicit form, for a shared helper that is not itself a def body and
    // for the one def whose enumerator names an ALIAS rather than its primary.
    ($recv:expr_2021, $meth:expr_2021, $args:expr_2021, $block:expr_2021) => {
        match $block {
            Some(crate::RubyValue::Proc(p)) => p,
            _ => {
                return Ok(crate::builtins::enumerator::enumerator_for(
                    $recv, $meth, $args,
                ))
            }
        }
    };
}
pub(crate) use block_or_enum;

/// The row `name` exactly as the ANCESTOR module declares it.
///
/// Ruby owns a good many methods on a SUBCLASS while their bodies live
/// further up the chain -- `Array#map` is `Array`'s, not `Enumerable`'s, and
/// `Float#<` is `Float`'s, not `Comparable`'s. Declaring the row on the
/// subclass is what makes `.owner` and `instance_methods(false)` agree;
/// routing it straight back to the ancestor's row is what stops the two from
/// ever drifting apart, since there is only ever one body.
macro_rules! inherited_row {
    ($module:ident, $name:literal, $recv:expr_2021, $args:expr_2021, $block:expr_2021) => {
        crate::builtins::$module::lookup($name).expect(concat!("the ancestor declares ", $name))(
            $recv, $args, $block,
        )
    };
}
pub(crate) use inherited_row;

/// The block, or CRuby's `LocalJumpError` (what a bare `yield` with no
/// block raises -- `5.tap` reproduces it, oracle-verified).
macro_rules! need_block {
    ($block:expr_2021) => {
        match &$block {
            Some(crate::RubyValue::Proc(p)) => p.clone(),
            _ => return Err(crate::dispatch::raise_no_block_yield()),
        }
    };
}
pub(crate) use need_block;

#[cfg(test)]
mod tests {
    use super::*;
    use zeo_abi::*;

    #[test]
    fn fallback_chains_match_the_oracle_hierarchy() {
        assert_eq!(
            fallback_ancestors(INTEGER_CLASS),
            &[
                INTEGER_CLASS,
                NUMERIC_CLASS,
                COMPARABLE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        assert_eq!(
            fallback_ancestors(STRING_CLASS),
            &[
                STRING_CLASS,
                COMPARABLE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        assert_eq!(
            fallback_ancestors(ARRAY_CLASS),
            &[
                ARRAY_CLASS,
                ENUMERABLE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        assert_eq!(
            fallback_ancestors(OBJECT_CLASS),
            &[OBJECT_CLASS, KERNEL_CLASS, BASIC_OBJECT_CLASS]
        );
        assert_eq!(
            fallback_ancestors(BASIC_OBJECT_CLASS),
            &[BASIC_OBJECT_CLASS]
        );
        assert_eq!(fallback_ancestors(KERNEL_CLASS), &[KERNEL_CLASS]);
        assert_eq!(
            fallback_ancestors(CLASS_CLASS),
            &[
                CLASS_CLASS,
                MODULE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        // Unknown (user) ids: empty, the registry's job.
        assert_eq!(fallback_ancestors(ClassId(999)), &[] as &[ClassId]);
    }

    #[test]
    fn class_table_covers_the_expected_ids() {
        assert!(class_table(STRING_CLASS).is_some());
        assert!(class_table(KERNEL_CLASS).is_some());
        assert!(class_table(BASIC_OBJECT_CLASS).is_some());
        assert!(class_table(ENUMERABLE_CLASS).is_some());
        assert!(class_table(ClassId(999)).is_none());
    }
}
