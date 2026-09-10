//! The builtin method tables -- one Rust module per Ruby core
//! class/module, mirroring CRuby's file-per-class source layout (string.c,
//! array.c, compar.c, ...).
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
//!
//! # Two files per class, and what each holds
//!
//! A class with real machinery has a file at the crate root as well as one
//! here, and the split is always the same: `crate::thread` is Thread's
//! machinery -- the OS threads, the join handles, the state a Ruby program
//! never sees -- and `builtins/thread.rs` is `Thread`'s method table, the
//! rows that call into it. Reading the table tells you what Ruby can ask
//! for; reading the root file tells you how it is answered.
//!
//! # The `r` prefix
//!
//! `struct`, `proc` and `module` are Rust keywords, so those three tables
//! are `rstruct.rs`, `rproc.rs` and `rmodule.rs`. `rclass.rs` follows them
//! for symmetry rather than necessity. The prefix means nothing else.
//!
//! # `support/`
//!
//! One file per class is what makes this directory navigable, so the eleven
//! helpers that belong to no class -- conversion, formatting, sorting,
//! waiting -- sit in [`support`] instead of diluting it. They keep their
//! names: `builtins::convert` still resolves.

use crate::{ClassId, RubyValue, Signal};
use std::sync::LazyLock;

pub mod support;
// The eleven helpers `support/` holds keep the names they always had: they
// moved to stop diluting the one-file-per-class rule, not to be respelled.
pub use support::{
    convert, format, formatter, lazy, pack, sort, value_subclass, waiter, weak, yielder, zeo_eval,
};
pub(crate) mod argf;
pub mod array;
pub(crate) mod backtrace_location;
pub(crate) mod basic_object;
pub(crate) mod binding;
pub(crate) mod comparable;
pub(crate) mod complex;
pub(crate) mod condition_variable;
pub(crate) mod converter;
pub(crate) mod data;
pub(crate) mod dir;
pub mod encoding;
pub(crate) mod enumerable;
pub(crate) mod enumerator;
pub(crate) mod env;
pub mod exception;
pub(crate) mod false_class;
pub(crate) mod fiber;
pub mod file;
pub(crate) mod file_test;
pub(crate) mod float;
pub(crate) mod gc;
pub(crate) mod hash;
pub mod integer;
pub mod io;
pub(crate) mod io_buffer;
pub(crate) mod kernel;
pub(crate) mod marshal;
pub(crate) mod matchdata;
pub(crate) mod math;
pub(crate) mod method;
pub(crate) mod mutex;
pub(crate) mod nil_class;
pub(crate) mod numeric;
pub(crate) mod objspace;
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
pub mod stat;
pub mod string;
pub(crate) mod symbol;
pub(crate) mod thread;
pub(crate) mod thread_group;
pub(crate) mod time;
pub(crate) mod true_class;
pub(crate) mod unbound_method;
pub mod warning;

/// One builtin method: receiver (guaranteed by the table's ClassId keying
/// to be the right variant), positional args, optional block. Deliberately
/// the SAME shape as `ValueMethodFn` (the reopen trampolines) --
/// one trampoline ABI for everything the MRO walk can find.
pub type BuiltinMethodFn =
    fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

/// One native row's `Method#parameters` answer, as the `params "..."` DSL
/// spelling was parsed into it. `&'static` throughout: the whole descriptor is
/// baked by the proc-macro, so reporting one allocates nothing but the Ruby
/// Array the caller sees.
pub type ParamRows = &'static [(crate::method_meta::ParamKind, Option<&'static str>)];

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
    /// The spelled signature, for a row that carries one. `None` -- the
    /// default -- leaves `Method#parameters` with the anonymous descriptor
    /// derived from `arity`, which names no parameter.
    pub params: fn(&str) -> Option<ParamRows>,
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
    /// Whether ruby names an ANCESTOR as this row's owner, so reflection must
    /// look past this class -- see `zeo_dsl::MethodDef::inherits`.
    pub inherits: fn(&str) -> bool,
    /// The arming key of the require/env gate covering this name
    /// (`"io/console"`, `"env:boxes"`), `None` for an always-on row -- see
    /// `zeo_dsl::MethodDef::gate` and [`gate`], which maps each key to the
    /// switch that opens it.
    pub gate: fn(&str) -> Option<&'static str>,
    /// Whether ANY row of this table is gated -- the cheap pre-question the
    /// table projections ask before taking [`gate`]'s filtering view.
    pub has_gated: bool,
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
    /// A blank instance of this class -- ruby's `allocate`, and the first half
    /// of ruby's `new`. `None` for the great majority: a class whose
    /// constructor carries no state needs none, and `Class#allocate` keeps
    /// raising `TypeError` for it exactly as CRuby does for `Time`.
    pub allocate: Option<fn() -> crate::RubyValue>,
}

/// The link-time table slice, populated ONLY in a `cfg(test)` build or
/// under the `unit-tables` feature.
///
/// A shipped program does not use it: `ruby_class!` registers a table through
/// an exported `zeo_ctable_<ID>` symbol the program names, and this slice is
/// empty. It stays for unit tests, which have no program desc to install one
/// from -- the runtime's own under `cfg(test)`, the C-API crate's through
/// the feature -- see `all_tables`.
#[linkme::distributed_slice]
pub static BUILTIN_TABLES: [BuiltinClassTable] = [..];

/// A class whose table is a copy of another class's, registered under its own
/// id -- the `Digest::SHA1`-style aliases, which `ruby_class!` cannot spell.
///
/// This exists so an alias is rooted the same way a `ruby_class!` table is: an
/// exported `zeo_ctable_<ID>` symbol the emitted program names, never a
/// `distributed_slice` entry. A linkme entry is a `no_dead_strip` root in its
/// own right, so seven OpenSSL aliases kept ~3 MB of libcrypto in a program
/// that cannot reach them.
///
/// The `NAME = zeo_abi::ID` spelling is load-bearing: build.rs scans the
/// runtime sources for exactly that shape to build `CLASS_TABLE_SYMBOLS`.
/// `alias_table` resolves in the calling module.
#[macro_export]
macro_rules! alias_class_tables {
    ($($name:ident = zeo_abi::$id:ident),* $(,)?) => {
        $(
            #[cfg_attr(any(test, feature = "unit-tables"), linkme::distributed_slice($crate::builtins::BUILTIN_TABLES))]
            #[unsafe(export_name = concat!("zeo_ctable_", stringify!($id)))]
            pub static $name: $crate::builtins::BuiltinClassTable = alias_table(zeo_abi::$id);
        )*
    };
}

/// The registered tables indexed by `ClassId` for O(1) routing, built once
/// from the link-time-collected slice.
///
/// A DIRECT index, not a hash probe: `ClassId`s are dense small integers, and
/// the ABI already indexes `BUILTINS` by `id.0 - 1` on the same grounds. This
/// is asked per ANCESTOR by `instance_method_visibility`, `responds_to` and
/// `scan_owner`, so the hash was paid once per step of every MRO walk.
///
/// "Dense small integers" is true of every table key but one:
/// `ENV_SINGLETON_CLASS` is `u32::MAX`, a reserved key rather than a class
/// (see its doc in zeo-abi). Sizing the array by the largest key therefore
/// asked for `u32::MAX + 1` slots -- **32 GiB** -- on the first routing
/// question of every program. macOS hid it completely: `vec![None; n]` of a
/// nullable reference allocates ZEROED, so the pages were never touched and
/// the reservation cost nothing real. Linux refuses the allocation outright
/// under a memory cap and the process aborts, which is how it surfaced.
///
/// So the array covers the dense range only, and the sparse keys -- one
/// today -- ride in a list scanned linearly. The scan is off the hot path by
/// construction: a real class id never reaches it.
/// The tables the running program named, installed by `register_program`
/// before any Ruby runs. See [`BUILTIN_TABLES`] for why they are not
/// collected at link time.
static PROGRAM_TABLES: std::sync::OnceLock<&'static [&'static BuiltinClassTable]> =
    std::sync::OnceLock::new();

/// Hand the program's tables over. Called once, from `register_program`.
pub fn install_program_tables(tables: &'static [&'static BuiltinClassTable]) {
    let _ = PROGRAM_TABLES.set(tables);
}

/// Every table this process has: the program's own, then the link-time slice
/// (empty in a release build). THE source for both readers -- the id map and
/// `bootstrap`'s constant installers -- because a table missing from either
/// is a class with no methods or a class with no constants.
pub(crate) fn all_tables() -> impl Iterator<Item = &'static BuiltinClassTable> {
    PROGRAM_TABLES
        .get()
        .copied()
        .unwrap_or(&[])
        .iter()
        .copied()
        .chain(BUILTIN_TABLES.iter())
}

pub(crate) fn registered_table(id: ClassId) -> Option<&'static BuiltinClassTable> {
    let found = lookup_table(id);
    if found.is_none() && table_was_dropped(id) {
        table_is_missing(id);
    }
    found
}

/// Whether `id` names an always-on builtin whose table this program did NOT
/// carry -- the one shape [`registered_table`] aborts on, asked safely.
///
/// A sweep over every class that mixes a module in (`runtime_meta`'s
/// `mixin_hosts`) reaches builtins the program never names, and such a class
/// can never be a receiver here, so it is not a host. Asking the guarded
/// probe about it would abort a correct program.
///
/// Note this is NOT "has no table": `Object` legitimately has none (its rows
/// live on `Kernel`), and it is very much reachable.
pub(crate) fn table_was_dropped(id: ClassId) -> bool {
    if lookup_table(id).is_some() {
        return false;
    }
    zeo_abi::BUILTINS
        .iter()
        .find(|b| b.id.0 == id.0)
        .is_some_and(|b| b.feature.is_none() && !NO_TABLE.contains(&b.name))
}

fn lookup_table(id: ClassId) -> Option<&'static BuiltinClassTable> {
    /// Keys at or above this are reserved markers, not class ids. The
    /// runtime block itself starts here, and no `ruby_class!` table is
    /// keyed inside it.
    const DENSE_LIMIT: u32 = zeo_abi::RUNTIME_CLASS_ID_BASE;

    struct Map {
        dense: Vec<Option<&'static BuiltinClassTable>>,
        sparse: Vec<(u32, &'static BuiltinClassTable)>,
    }
    static BY_ID: LazyLock<Map> = LazyLock::new(|| {
        // The program's own list first (`ProgramDesc::class_tables`); the
        // link-time slice is the fallback, and in a release build it is
        // EMPTY -- see `install_program_tables`.
        let len = all_tables()
            .map(|t| t.id.0)
            .filter(|id| *id < DENSE_LIMIT)
            .max()
            .map_or(0, |m| m as usize + 1);
        let mut dense = vec![None; len];
        let mut sparse = Vec::new();
        for t in all_tables() {
            if t.id.0 < DENSE_LIMIT {
                dense[t.id.0 as usize] = Some(t);
            } else {
                sparse.push((t.id.0, t));
            }
        }
        Map { dense, sparse }
    });

    if id.0 < DENSE_LIMIT {
        BY_ID.dense.get(id.0 as usize).copied().flatten()
    } else {
        BY_ID
            .sparse
            .iter()
            .find(|(key, _)| *key == id.0)
            .map(|(_, t)| *t)
    }
}

/// The four always-on builtins that declare no `ruby_class!` table at all --
/// they are registered another way, so `None` is their right answer.
///
/// Written out rather than inferred because the runtime cannot see the
/// compiler's table list. `every_tableless_builtin_is_listed` asserts this IS
/// the set, so a fifth one cannot join it silently.
const NO_TABLE: &[&str] = &[
    "Ruby::Box::Entry",
    "Set::CoreSet",
    "UnicodeNormalize",
    "WeakRef",
    // A namespace only: its methods live on `Zeo::Eval`.
    "Zeo",
];

/// Abort if `id` is a class this program was supposed to carry a table for.
///
/// Without this the miss is silent: `side_of` uses `?`, so a dropped table
/// becomes `NoMethodError` for every row that class has -- and its CONSTANTS
/// vanish too, because `install_core_constants` iterates `all_tables()`. That
/// reads as a dispatch bug anywhere but here.
///
/// A GATED builtin legitimately has no table in a program that never required
/// it; that is the whole point of `needed_class_tables`. An always-on one has
/// no such excuse.
#[cold]
#[inline(never)]
fn table_is_missing(id: ClassId) -> ! {
    let b = zeo_abi::BUILTINS
        .iter()
        .find(|b| b.id.0 == id.0)
        .expect("table_was_dropped already found this builtin");
    panic!(
        "zeo: internal error -- this program carries no method table for the \
         always-on builtin `{}` (id {}), so every method and constant it has \
         is missing. The emitter's `needed_class_tables` dropped a table the \
         program can reach.",
        b.name, id.0
    );
}

/// Which side of a builtin's tables a question addresses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Instance,
    Class,
}

/// THE registered-table projection: the one place a `(class, side)` becomes a
/// method-table view. Every routing question below reads through this, so a
/// runtime row store consulted in policy order slots in here without touching
/// a single reader.
pub(crate) fn side_of(id: ClassId, side: Side) -> Option<&'static MethodTable> {
    let t = registered_table(id)?;
    match side {
        Side::Instance => t.instance.as_ref(),
        Side::Class => t.class.as_ref(),
    }
}

/// This class's blank-instance fn, or `None` if it declares no `allocate`.
///
/// Read through the same registered table the methods come from, so an
/// allocator costs nothing in a program that does not ship the class.
pub(crate) fn allocator_of(id: ClassId) -> Option<fn() -> crate::RubyValue> {
    registered_table(id)?.allocate
}

/// ClassId -> instance-method table, off the program's own table list.
/// `None` for a user class and for a builtin this program did not name.
/// `Enumerable`/`Comparable` are ordinary rows here too -- the MRO
/// walk reaches them as ancestors of Array/Hash/Range and of any user class
/// that `include`s them, exactly like every other builtin module.
pub(crate) fn class_table(id: ClassId) -> Option<fn(&str) -> Option<BuiltinMethodFn>> {
    let t = registered_table(id)?;
    if table_gated(t) {
        return Some(gate::views(id).lookup);
    }
    t.instance.as_ref().map(|m| m.lookup)
}

/// Whether this entry carries gate-hidden rows on either side -- read off an
/// already-probed entry, so a projection costs ONE table-map probe instead
/// of the three `gate::gated` + `side_of` paid before.
fn table_gated(t: &BuiltinClassTable) -> bool {
    t.instance.as_ref().is_some_and(|m| m.has_gated)
        || t.class.as_ref().is_some_and(|m| m.has_gated)
}

/// `class_table`'s arity twin: ClassId -> the instance-method arity table
/// (`<lookup>_arity`, generated beside every `lookup`).
/// `Method#arity` consults this for a builtin-receiver method object, walking
/// the receiver's ancestry so an inherited builtin resolves against its owner.
pub(crate) fn class_arity_table(id: ClassId) -> Option<fn(&str) -> Option<i64>> {
    let t = registered_table(id)?;
    if table_gated(t) {
        return Some(gate::views(id).arity);
    }
    t.instance.as_ref().map(|m| m.arity)
}

/// `class_arity_table`'s parameter twin: ClassId -> the instance-method
/// signature table (`<lookup>_params`). Answers `None` for a class with no
/// registered table AND for a name in one that spells no signature -- both mean
/// "fall back to the anonymous descriptor", which is what nearly every row
/// still does.
pub(crate) fn class_params_table(id: ClassId) -> Option<fn(&str) -> Option<ParamRows>> {
    let t = registered_table(id)?;
    if table_gated(t) {
        return Some(gate::views(id).params);
    }
    Some(t.instance.as_ref()?.params)
}

/// `class_params_table`'s CLASS-METHOD counterpart.
pub(crate) fn class_method_params_table(id: ClassId) -> Option<fn(&str) -> Option<ParamRows>> {
    let t = registered_table(id)?;
    if table_gated(t) {
        return Some(gate::views(id).class_params);
    }
    Some(t.class.as_ref()?.params)
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
    let t = registered_table(id)?;
    if table_gated(t) {
        return Some(gate::views(id).class_lookup);
    }
    t.class.as_ref().map(|m| m.lookup)
}

/// `class_method_table`'s arity twin -- what `Foo.method(:bar).arity` reads
/// for a builtin class method, the singleton mirror of [`class_arity_table`].
pub(crate) fn class_method_arity_table(id: ClassId) -> Option<fn(&str) -> Option<i64>> {
    let t = registered_table(id)?;
    if table_gated(t) {
        return Some(gate::views(id).class_arity);
    }
    t.class.as_ref().map(|m| m.arity)
}

/// Whether the builtin instance method `name` on `id` is CRuby-private --
/// reachable through implicit self/`send`/`super`, invisible to reflection.
///
/// `module_function` is the whole story today: CRuby defines the instance copy
/// private, so `Math.instance_methods(false)` is empty while
/// `Math.private_instance_methods(false)` lists 28.
pub(crate) fn class_method_is_private(id: ClassId, name: &str) -> bool {
    side_of(id, Side::Instance).is_some_and(|m| (m.is_private)(name))
}

/// Whether the builtin instance method `name` on `id` is CRuby-PROTECTED --
/// `Pathname#path` is the whole population, made protected so `#to_s` is the
/// way to spell a path out loud.
pub(crate) fn class_method_is_protected(id: ClassId, name: &str) -> bool {
    side_of(id, Side::Instance).is_some_and(|m| (m.is_protected)(name))
}

/// The same question for a builtin CLASS-method row (`private def self."x"`)
/// -- prism's `serialize_parse` and friends, the backend seam the gem's own
/// Ruby calls with implicit self and nothing outside should see.
pub(crate) fn builtin_class_method_is_private(id: ClassId, name: &str) -> bool {
    side_of(id, Side::Class).is_some_and(|m| (m.is_private)(name))
}

/// Whether a builtin CLASS-method row allocates through the RECEIVER class --
/// what decides if a value subclass re-tags the result as itself. See
/// `zeo_dsl::MethodDef::allocs`; the row itself carries the answer, as the
/// equivalent C function does in CRuby.
pub(crate) fn builtin_class_method_allocs(id: ClassId, name: &str) -> bool {
    side_of(id, Side::Class).is_some_and(|m| (m.allocs)(name))
}

/// Whether `id`'s row for `name` is one ruby owns further up the ancestry, so
/// `instance_methods(false)` must skip it and the owner scan must walk past
/// it. `kind` picks the instance or the class table. See
/// `zeo_dsl::MethodDef::inherits`.
pub(crate) fn builtin_row_inherits(id: ClassId, name: &str, class_side: bool) -> bool {
    let side = if class_side {
        Side::Class
    } else {
        Side::Instance
    };
    side_of(id, side).is_some_and(|m| (m.inherits)(name))
}

/// `class_table`'s reflection companion: the instance-method NAMES a builtin
/// class exposes (for `instance_methods`/`methods`).
pub(crate) fn class_table_names(id: ClassId) -> &'static [&'static str] {
    gate::names(id, Side::Instance)
}

/// `class_method_table`'s reflection companion: the CLASS-method NAMES a
/// builtin exposes (for `SomeClass.singleton_methods` / `.methods`).
pub(crate) fn class_method_table_names(id: ClassId) -> &'static [&'static str] {
    gate::names(id, Side::Class)
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

/// How a `Check_Type` TypeError (`wrong argument type X (expected Y)`)
/// names the offending value -- CRuby's `builtin_class_name`: `nil`,
/// `true` and `false` read as the VALUE, everything else as its class
/// name. Distinct from `coerce_operand_name`, which also reads a Symbol,
/// a fixnum and a Float as their inspect form.
pub(crate) fn check_type_name(v: &RubyValue) -> String {
    match v {
        RubyValue::Nil => "nil".to_string(),
        RubyValue::Bool(b) => b.to_string(),
        other => class_name_of(other),
    }
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

/// CRuby's coercion TypeError, spelled once: `no implicit conversion of X
/// into Y`, with X rendered by [`convert_name_of`] (nil/true/false as the
/// literals, everything else as its class name). Only for sites producing
/// EXACTLY this text -- the close variants ("no implicit conversion from nil
/// to integer", "... into Integer for {who}") keep their own strings.
pub(crate) fn no_implicit(v: &RubyValue, want: &str) -> Signal {
    crate::builtins::type_error!(
        "no implicit conversion of {} into {want}",
        convert_name_of(v)
    )
}

/// The class-check TypeError, spelled once: `wrong argument type X (expected
/// Y)`, with X the value's CLASS name ([`class_name_of`] -- nil reads as
/// `NilClass` here). The sites rendering nil as the literal go through
/// [`check_type_name`] and keep their own strings.
pub fn wrong_arg_type(v: &RubyValue, want: &str) -> Signal {
    crate::builtins::type_error!("wrong argument type {} (expected {want})", class_name_of(v))
}

/// The typed error constructors: `type_error!("no implicit conversion...")`
/// over `raise_error("TypeError", format!(...))`, so the class name is spelled
/// once here (never typo-able per site) and call sites read as what they
/// raise. Each takes `format!` arguments and yields a `Signal` -- wrap in
/// `Err(...)` exactly as with `raise_error`. Each expands to `raise_error_id`
/// with the class's fixed `zeo_abi` id, skipping the registry's by-name
/// String probe. Classes raised from only a few sites (Errno::*,
/// ext-specific classes) stay on `raise_error` directly.
/// (Written flat rather than macro-generated: `$$` meta-variable escaping is
/// still unstable.)
#[macro_export]
macro_rules! type_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::TYPE_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! arg_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::ARGUMENT_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! name_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::NAME_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! index_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::INDEX_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! range_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::RANGE_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! runtime_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::RUNTIME_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! frozen_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::FROZEN_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! io_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::IO_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! eof_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::EOF_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! thread_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::THREAD_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! regexp_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::REGEXP_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! local_jump_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::LOCAL_JUMP_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! float_domain_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::FLOAT_DOMAIN_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! not_impl_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::NOT_IMPLEMENTED_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! zero_division_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::ZERO_DIVISION_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! load_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::LOAD_ERROR_CLASS, format!($($fmt)*)) };
}
// `SystemCallError` composes its printed line at the raise site, so the
// registry stamps the message VERBATIM onto the object -- the id channel
// keys that off the entry's registered name, exactly as by-name does.
#[macro_export]
macro_rules! system_call_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::SYSTEM_CALL_ERROR_CLASS, format!($($fmt)*)) };
}
// The plain-message channel. A site that populates `NoMethodError`'s
// `#name`/`#args`/`#receiver` goes through `raise_method_missing` or
// `raise_error_details` instead.
#[macro_export]
macro_rules! no_method_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::NO_METHOD_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! no_memory_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::NO_MEMORY_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! fiber_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::FIBER_ERROR_CLASS, format!($($fmt)*)) };
}
#[macro_export]
macro_rules! syntax_error {
    ($($fmt:tt)*) => { $crate::dispatch::raise_error_id(zeo_abi::SYNTAX_ERROR_CLASS, format!($($fmt)*)) };
}
pub use {
    arg_error, eof_error, fiber_error, float_domain_error, frozen_error, index_error, io_error,
    load_error, local_jump_error, name_error, no_memory_error, no_method_error, not_impl_error,
    range_error, regexp_error, runtime_error, syntax_error, system_call_error, thread_error,
    type_error, zero_division_error,
};

/// The argument-count guard the `ruby_class!` macro emits from a def's
/// parameter list. `max: None` means a `*rest` accepts any number.
///
/// One shared helper rather than an inlined `format!` per call site: the
/// message is only ever built on the failing path, and LLVM otherwise lays that
/// path ahead of the hot body, so a four-line builtin spends its first cache
/// line on error construction.
///
/// WHERE RAW `args` HANDLING REMAINS ON PURPOSE -- the survivor classes of
/// the macro-kit sweep, so the next sweep does not re-litigate them:
/// (1) arity-DISPATCH bodies, where `args.len()` selects a behavior no one
///     parameter list expresses (`Enumerable#first`/`#inject`/`#count`/`#sum`,
///     `Hash.[]`, `String#[]=`, `Array#fill`/`#insert`, `SystemExit.new`);
/// (2) shared free helpers serving several defs or a raw constructor
///     boundary, which take the slice itself (enumerable's cores, Kernel's
///     `*_impl` family, `io_buffer::init_in_place`, time's civil-field
///     parsers, process's command builders, exception.rs's hand-registered
///     rows, io_console's forwarded rows);
/// (3) rows whose hand-rolled message `check_arity` cannot produce
///     (`find_index` and `Range#first`/`#last` say "expected 1" while also
///     accepting zero arguments);
/// (4) the sanctioned `__args` channel: a body forwarding its whole slice to
///     a helper or into an enumerator capture;
/// (5) `RProc` closure bodies, whose `args` are the yielded values, not a
///     method's argument list.
/// tramp.rs and the capi/runtime_meta extern boundaries stay raw by design;
/// their module docs say so. Unguarded probes that IGNORE surplus arguments
/// (`Enumerable#tally`/`#cycle`, `Queue#initialize`) also survive: giving
/// them a header would add the raise CRuby has, which is a behavior change,
/// not a refactor.
#[inline(always)]
pub(crate) fn check_arity(given: usize, min: usize, max: Option<usize>) -> Result<(), Signal> {
    if given < min || max.is_some_and(|hi| given > hi) {
        return Err(arity_err(given, min, max));
    }
    Ok(())
}

/// Split the trailing keyword Hash off an argument slice -- what a `ruby def`
/// with keywords, or any `**kwrest`, binds from.
///
/// The `given > min` condition is CRuby's own (`rb_scan_args`' `:`, whose test
/// is `n_mand < argc`) and it is load-bearing: without it a Hash passed as a
/// real ARGUMENT is swallowed as keywords, which is how
/// `Ractor.make_shareable(a_hash)` came to report `given 0, expected 1`.
#[inline(always)]
pub(crate) fn peel_kwargs(args: &[RubyValue], min: usize) -> (Option<&RubyValue>, &[RubyValue]) {
    match args.last() {
        Some(h @ RubyValue::Hash(_)) if args.len() > min => (Some(h), &args[..args.len() - 1]),
        _ => (None, args),
    }
}

/// The same split, but only for a Hash the CALLER marked as keywords.
///
/// What tells `f(h)` from `f(**h)`, which ruby answers three different ways:
/// `[1,2].sample(h)` is a TypeError (the Hash is the positional `n`),
/// `[1,2].shuffle(h)` is an arity error, and `[1,2].shuffle(**h)` is
/// `unknown keyword: :x`. [`peel_kwargs`] cannot tell them apart and never
/// could -- harmless while an unmatched key was silently ignored, and a wrong
/// RAISE once a def declares its keywords by name.
///
/// `peel_kwargs` keeps its looser rule so a `**kwrest`-only def behaves exactly
/// as it did; only a def naming its keywords takes this one.
#[inline(always)]
pub(crate) fn peel_keywords(args: &[RubyValue], min: usize) -> (Option<&RubyValue>, &[RubyValue]) {
    match args.last() {
        Some(h @ RubyValue::Hash(inner))
            if args.len() > min && crate::collections::hash_is_kwargs(inner) =>
        {
            (Some(h), &args[..args.len() - 1])
        }
        _ => (None, args),
    }
}

/// One named keyword, CLONED out of the peeled Hash.
///
/// Owned rather than borrowed, and the asymmetry with a positional is forced:
/// a positional is an element of the caller's argv, alive for the whole call,
/// while a keyword lives inside an `Arc<Mutex<..>>` whose guard must not be
/// held across body code. The clone is an `Arc` bump for a heap value and
/// nothing for an immediate -- and it is exactly what every hand-written peel
/// this replaces already returned.
#[inline(always)]
pub(crate) fn kw_take(src: Option<&RubyValue>, name: &str) -> Option<RubyValue> {
    let RubyValue::Hash(h) = src? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    crate::collections::hash_has_key(h, &key).then(|| crate::collections::hash_get(h, &key))
}

/// A REQUIRED keyword: ruby's `missing keyword: :k` when it is absent.
///
/// No builtin row references this, but it is NOT an orphan: `ruby_class!`
/// (zeo-macros/src/lib.rs, the required-keyword arm) emits a call to it for
/// any def that declares a required keyword. No such row exists today --
/// measured, not assumed: a live diff of `#parameters` over 3,526 of ruby's
/// own rows finds zero `keyreq` -- hence the allow. Deleting this breaks
/// that macro arm.
#[allow(
    dead_code,
    reason = "used only from a `ruby_class!` arm the unit-test build takes"
)]
#[inline(always)]
pub(crate) fn kw_required(src: Option<&RubyValue>, name: &str) -> Result<RubyValue, Signal> {
    kw_take(src, name).ok_or_else(|| crate::builtins::arg_error!("missing keyword: :{name}"))
}

/// Whatever keys a `**kwrest` gets after the NAMED keywords have been taken.
///
/// The one allocation this design adds, and only for a def declaring both --
/// a `**kwrest` on its own keeps borrowing the peeled Hash untouched.
///
/// Like [`kw_required`], referenced only by `ruby_class!`'s emission
/// (zeo-macros/src/lib.rs, the keyrest arm), for a shape no row uses today:
/// no ruby builtin row declares named keywords AND a keyrest -- the shapes
/// that carry a keyrest all carry the ANONYMOUS forwarding trio (`*, **, &`)
/// instead. Deleting this breaks that macro arm.
#[allow(
    dead_code,
    reason = "used only from a `ruby_class!` arm the unit-test build takes"
)]
#[inline(always)]
pub(crate) fn kw_rest(src: Option<&RubyValue>, taken: &[&str]) -> Option<RubyValue> {
    let RubyValue::Hash(h) = src? else {
        return src.cloned();
    };
    Some(RubyValue::Hash(crate::collections::hash_except_keys(
        h, taken,
    )))
}

/// Ruby refuses a keyword a method does not declare, unless it takes a
/// `**kwrest`. Only emitted for a def that declares named keywords WITHOUT
/// one, since that is the only shape where an unknown key is an error.
pub(crate) fn kw_check_unknown(src: Option<&RubyValue>, known: &[&str]) -> Result<(), Signal> {
    let Some(RubyValue::Hash(h)) = src else {
        return Ok(());
    };
    let extra = crate::collections::hash_except_keys(h, known);
    let names = crate::collections::hash_keys(&extra);
    let RubyValue::Array(a) = names else {
        return Ok(());
    };
    let list: Vec<String> = a
        .lock()
        .iter()
        .map(|k| match k {
            RubyValue::Symbol(s) => format!(":{}", s.name()),
            other => other.inspect_string(),
        })
        .collect();
    if list.is_empty() {
        return Ok(());
    }
    let plural = if list.len() == 1 { "" } else { "s" };
    Err(crate::builtins::arg_error!(
        "unknown keyword{plural}: {}",
        list.join(", ")
    ))
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
    crate::builtins::arg_error!("wrong number of arguments (given {given}, expected {expected})")
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
    ($args:expr_2021, $i:literal) => {
        match &$args[$i] {
            crate::RubyValue::Str(s) => s.clone(),
            other => crate::builtins::convert::to_rstr(other)?,
        }
    };
    // The same, for a parameter the def named rather than an index into the
    // raw slice.
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

    fn kw_hash(pairs: &[(&str, i64)], marked: bool) -> RubyValue {
        let pairs: Vec<(RubyValue, RubyValue)> = pairs
            .iter()
            .map(|(k, v)| {
                (
                    RubyValue::Symbol(crate::Symbol::intern(k)),
                    RubyValue::Int(*v),
                )
            })
            .collect();
        let h = crate::value::collections::hash_new(pairs);
        if marked {
            crate::value::collections::hash_mark_kwargs(&h);
        }
        RubyValue::Hash(h)
    }

    /// The mark is what tells `f(h)` from `f(**h)`, and it is the whole reason
    /// a def naming its keywords peels differently from a `**kwrest`-only one.
    #[test]
    fn only_a_marked_hash_is_keywords() {
        let marked = [kw_hash(&[("a", 1)], true)];
        let plain = [kw_hash(&[("a", 1)], false)];

        assert!(peel_keywords(&marked, 0).0.is_some());
        assert!(
            peel_keywords(&plain, 0).0.is_none(),
            "an unmarked Hash is a positional argument, not keywords"
        );
        // The looser rule a `**kwrest`-only def keeps: any trailing Hash.
        assert!(peel_kwargs(&plain, 0).0.is_some());
        // ...but never one the mandatory positionals still need. This is
        // CRuby's `n_mand < argc`, and without it a Hash ARGUMENT vanishes.
        assert!(peel_kwargs(&plain, 1).0.is_none());
        assert!(peel_keywords(&marked, 1).0.is_none());
    }

    #[test]
    fn a_keyword_is_taken_by_name_and_a_required_one_must_be_there() {
        let args = [kw_hash(&[("a", 1), ("b", 2)], true)];
        let (src, pos) = peel_keywords(&args, 0);
        assert!(pos.is_empty());

        assert!(matches!(kw_take(src, "a"), Some(RubyValue::Int(1))));
        assert!(kw_take(src, "nope").is_none());
        assert!(matches!(kw_required(src, "b"), Ok(RubyValue::Int(2))));
        // The ABSENT case raises, so it is asserted where a raise is legal --
        // `tests/a_builtin_row_reports_its_parameter_names.rb`, against the
        // oracle. Building the exception here needs a booted class registry.
    }

    /// A `**kwrest` beside NAMED keywords gets only what they did not take --
    /// the one allocation this design adds, and only for a def declaring both.
    #[test]
    fn a_keyrest_gets_what_the_named_keywords_left() {
        let args = [kw_hash(&[("a", 1), ("b", 2), ("c", 3)], true)];
        let (src, _) = peel_keywords(&args, 0);
        let rest = kw_rest(src, &["a"]).expect("a Hash remains");
        let RubyValue::Hash(h) = &rest else {
            panic!("kw_rest answers a Hash")
        };
        let key = |n: &str| RubyValue::Symbol(crate::Symbol::intern(n));
        assert!(!crate::collections::hash_has_key(h, &key("a")), "taken");
        assert!(crate::collections::hash_has_key(h, &key("b")));
        assert!(crate::collections::hash_has_key(h, &key("c")));
    }

    /// Without a `**kwrest`, ruby refuses a keyword the method never declared.
    #[test]
    fn a_declared_keyword_set_accepts_its_own_names() {
        let args = [kw_hash(&[("a", 1), ("zz", 2)], true)];
        let (src, _) = peel_keywords(&args, 0);
        assert!(kw_check_unknown(src, &["a", "zz"]).is_ok());
        // Nothing to refuse when the call passed no keywords at all.
        assert!(kw_check_unknown(None, &["a"]).is_ok());
        // The refusal itself is a golden, for the same registry reason.
    }

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

/// Rows a builtin's table declares that ruby only grows at a `require` (or
/// under a startup switch).
///
/// CRuby ships `io/console` and `io/nonblock` as require-gated extensions;
/// zeo implements them natively, so their rows sit in `IO`'s own table --
/// one class owns one table -- and were answerable before the require. That
/// is observable beyond reflection: `respond_to?(:getch)` is how a library
/// decides whether the console extension is there at all.
///
/// WHICH rows are gated is the table's own knowledge: each row carries a
/// `gated "feature"` marker in its `ruby_class!` def (see
/// `zeo_dsl::MethodDef::gate`), and the generated table answers through its
/// `gate` fn. This module only maps each gate KEY to the switch that arms
/// it. A require-armed gate is flipped by the `require` itself, at its own
/// document position (`features::feature_loaded`), so a program that
/// requires `io/console` on its last line does not answer `getch` on its
/// first.
pub(crate) mod gate {
    use std::sync::atomic::{AtomicU8, Ordering};
    use zeo_abi::ClassId;

    /// 0 = not required yet, 1 = required. Flipped by [`activate`].
    ///
    /// Deliberately NOT read back out of `$LOADED_FEATURES`: that array is a
    /// mutable Ruby value a program may push anything into, and the question
    /// here is whether zeo's own loader activated the extension.
    static CONSOLE: AtomicU8 = AtomicU8::new(0);
    static CONSOLE_SIZE: AtomicU8 = AtomicU8::new(0);
    static NONBLOCK: AtomicU8 = AtomicU8::new(0);

    fn required(cell: &AtomicU8) -> bool {
        cell.load(Ordering::Acquire) == 1
    }

    /// A `require` of `feature` ran. Idempotent; a feature that gates no rows
    /// is a no-op here (its gating is the CONSTANT's, see
    /// `constants::reveal_feature_classes`).
    pub(crate) fn activate(feature: &str) {
        match feature {
            "io/console" => CONSOLE.store(1, Ordering::Release),
            // `size.rb` opens with `require 'io/console'`, so this one name
            // arms both gates.
            "io/console/size" => {
                CONSOLE_SIZE.store(1, Ordering::Release);
                CONSOLE.store(1, Ordering::Release);
            }
            "io/nonblock" => NONBLOCK.store(1, Ordering::Release),
            _ => return,
        }
        NAME_CACHE.write().unwrap().take();
    }

    /// Whether the gate named by `key` is open -- the one place each
    /// `gated` marker's key binds to the switch that arms it. The `env:`
    /// keys are startup switches (`Ruby::Box`'s six rows exist only under
    /// `RUBY_BOX=1`), the rest are requires.
    ///
    /// An UNKNOWN key stays closed: leaking the row would answer a require
    /// ruby never saw. `every_gate_key_is_wired_to_a_switch` pins the set.
    fn armed(key: &str) -> bool {
        match key {
            "io/console" => required(&CONSOLE),
            "io/console/size" => required(&CONSOLE_SIZE),
            "io/nonblock" => required(&NONBLOCK),
            "env:boxes" => crate::boxes::boxes_enabled(),
            _ => false,
        }
    }

    /// Whether a name in `id`'s instance table is answerable yet.
    pub(crate) fn instance_ok(id: ClassId, name: &str) -> bool {
        match super::side_of(id, super::Side::Instance).and_then(|m| (m.gate)(name)) {
            Some(key) => armed(key),
            None => true,
        }
    }

    /// `instance_ok`'s class-method twin.
    pub(crate) fn class_ok(id: ClassId, name: &str) -> bool {
        match super::side_of(id, super::Side::Class).and_then(|m| (m.gate)(name)) {
            Some(key) => armed(key),
            None => true,
        }
    }

    /// Whether `id` has gated rows at all. Reflection-path only (`names`,
    /// the wiring test); the hot table projections read `has_gated` off
    /// their single `registered_table` probe instead (`table_gated`).
    pub(crate) fn gated(id: ClassId) -> bool {
        super::side_of(id, super::Side::Instance).is_some_and(|m| m.has_gated)
            || super::side_of(id, super::Side::Class).is_some_and(|m| m.has_gated)
    }

    /// One gated class's six filtered fn-pointer views: the class's own
    /// table with the rows the gate hides taken out.
    pub(crate) struct Views {
        pub(crate) lookup: fn(&str) -> Option<super::BuiltinMethodFn>,
        pub(crate) arity: fn(&str) -> Option<i64>,
        pub(crate) params: fn(&str) -> Option<super::ParamRows>,
        pub(crate) class_lookup: fn(&str) -> Option<super::BuiltinMethodFn>,
        pub(crate) class_arity: fn(&str) -> Option<i64>,
        pub(crate) class_params: fn(&str) -> Option<super::ParamRows>,
    }

    fn v_lookup<const ID: u32>(name: &str) -> Option<super::BuiltinMethodFn> {
        instance_ok(ClassId(ID), name).then_some(())?;
        (super::side_of(ClassId(ID), super::Side::Instance)?.lookup)(name)
    }
    fn v_arity<const ID: u32>(name: &str) -> Option<i64> {
        instance_ok(ClassId(ID), name).then_some(())?;
        (super::side_of(ClassId(ID), super::Side::Instance)?.arity)(name)
    }
    fn v_params<const ID: u32>(name: &str) -> Option<super::ParamRows> {
        instance_ok(ClassId(ID), name).then_some(())?;
        (super::side_of(ClassId(ID), super::Side::Instance)?.params)(name)
    }
    fn v_class_lookup<const ID: u32>(name: &str) -> Option<super::BuiltinMethodFn> {
        class_ok(ClassId(ID), name).then_some(())?;
        (super::side_of(ClassId(ID), super::Side::Class)?.lookup)(name)
    }
    fn v_class_arity<const ID: u32>(name: &str) -> Option<i64> {
        class_ok(ClassId(ID), name).then_some(())?;
        (super::side_of(ClassId(ID), super::Side::Class)?.arity)(name)
    }
    fn v_class_params<const ID: u32>(name: &str) -> Option<super::ParamRows> {
        class_ok(ClassId(ID), name).then_some(())?;
        (super::side_of(ClassId(ID), super::Side::Class)?.params)(name)
    }

    const fn views_of<const ID: u32>() -> Views {
        Views {
            lookup: v_lookup::<ID>,
            arity: v_arity::<ID>,
            params: v_params::<ID>,
            class_lookup: v_class_lookup::<ID>,
            class_arity: v_class_arity::<ID>,
            class_params: v_class_params::<ID>,
        }
    }

    /// The views for one gated class. The projections hand back fn
    /// POINTERS, which cannot capture the id, so each gated class needs its
    /// own const-generic instantiations -- this match is the one per-class
    /// list left, and `every_gated_class_has_a_fn_pointer_view` keeps it
    /// honest.
    pub(crate) fn views(id: ClassId) -> &'static Views {
        static IO: Views = views_of::<{ zeo_abi::IO_CLASS.0 }>();
        static BOX: Views = views_of::<{ zeo_abi::RUBY_BOX_CLASS.0 }>();
        match id {
            zeo_abi::IO_CLASS => &IO,
            zeo_abi::RUBY_BOX_CLASS => &BOX,
            other => unreachable!(
                "class {} has gated rows but no fn-pointer view instantiation -- \
                 add one beside gate::views' existing pair",
                other.0
            ),
        }
    }

    /// The name lists, filtered once per `(class, side)` -- [`names`]'
    /// memo. Dropped by [`activate`]: the filtered list is an
    /// answer about a moment, and a `require` written below a reflection call
    /// changes it. The slices it holds are `Vec::leak`ed, so a dropped entry
    /// stays valid for any `&'static` a caller is still holding.
    /// Keyed by `(class id, is instance side)`.
    type NameMemo = crate::FMap<(u32, bool), &'static [&'static str]>;

    static NAME_CACHE: std::sync::RwLock<Option<NameMemo>> = std::sync::RwLock::new(None);

    pub(crate) fn names(id: ClassId, side: super::Side) -> &'static [&'static str] {
        let all = super::side_of(id, side).map(|m| (m.names)()).unwrap_or(&[]);
        // The static list IS the answer for the ungated majority -- no memo,
        // no leak.
        if !gated(id) {
            return all;
        }
        let key = (id.0, matches!(side, super::Side::Instance));
        if let Some(hit) = NAME_CACHE
            .read()
            .unwrap()
            .as_ref()
            .and_then(|m| m.get(&key))
        {
            return hit;
        }
        let kept: Vec<&'static str> = all
            .iter()
            .copied()
            .filter(|n| match side {
                super::Side::Instance => instance_ok(id, n),
                super::Side::Class => class_ok(id, n),
            })
            .collect();
        let leaked: &'static [&'static str] = Vec::leak(kept);
        NAME_CACHE
            .write()
            .unwrap()
            .get_or_insert_with(crate::FMap::default)
            .insert(key, leaked);
        leaked
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// `views` panics for a gated class nobody instantiated -- surface
        /// that here, not on a program's first `IO`-shaped question.
        #[test]
        fn every_gated_class_has_a_fn_pointer_view() {
            for t in crate::builtins::all_tables() {
                if gated(t.id) {
                    let _ = views(t.id);
                }
            }
        }

        /// Every gate key some table carries must be one [`armed`] knows --
        /// an unwired key would hide its rows FOREVER (the require could
        /// never reveal them). Set equality also catches a wired key no row
        /// uses any more.
        #[test]
        fn every_gate_key_is_wired_to_a_switch() {
            let mut keys: Vec<&str> = Vec::new();
            for t in crate::builtins::all_tables() {
                for m in [t.instance.as_ref(), t.class.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    keys.extend((m.names)().iter().filter_map(|n| (m.gate)(n)));
                }
            }
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(
                keys,
                ["env:boxes", "io/console", "io/console/size", "io/nonblock"]
            );
        }
    }
}

#[cfg(test)]
mod table_tests {
    /// `NO_TABLE` IS the set of always-on builtins with no `ruby_class!`
    /// table. If a fifth one appears, the ICE beside it would abort a working
    /// program; if one of these four grows a table, the list is dead weight
    /// hiding a real miss.
    #[test]
    fn every_tableless_builtin_is_listed() {
        let mut found: Vec<&str> = zeo_abi::BUILTINS
            .iter()
            .filter(|b| b.feature.is_none() && super::registered_table(b.id).is_none())
            .map(|b| b.name)
            .collect();
        found.sort_unstable();
        assert_eq!(
            found,
            super::NO_TABLE,
            "the always-on builtins with no table are not what `NO_TABLE` says"
        );
    }

    /// A GATED builtin with no table is the ordinary answer -- that is what
    /// `needed_class_tables` narrowing produces -- so it must not abort.
    #[test]
    fn a_gated_builtin_without_a_table_is_not_an_error() {
        let gated = zeo_abi::BUILTINS
            .iter()
            .find(|b| b.feature.is_some() && super::registered_table(b.id).is_none());
        // In a unit-test binary every table is linked in, so this may find
        // none; the assertion is that asking did not abort.
        let _ = gated;
    }
}
