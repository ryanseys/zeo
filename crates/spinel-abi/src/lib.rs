//! The compiler/runtime ABI, single-sourced (Phase 15.1; hierarchy made
//! declarative in Phase 17.1).
//!
//! `spinelc` (the compiler) and `spinel-rt` (the runtime every generated
//! program links) deliberately never link each other -- but they must agree
//! on the numeric identity of every built-in class: the compiler bakes
//! `ClassId`s into generated code as literals, and the runtime's dispatch/
//! `is_a?`/registry machinery interprets them. Before this crate existed,
//! that agreement was TWO parallel hand-maintained const lists
//! (`spinelc::compiler` and `spinel_rt::dispatch`) synced by a
//! `debug_assert` -- a growing burden as the ABI gains class names,
//! module-ness, and (Phase 18) per-box method-table keys. This crate is the
//! one source of truth both sides re-export.
//!
//! Since Phase 17.1 the table also carries each builtin's SUPERCLASS and
//! INCLUDES -- the CRuby-exact hierarchy (oracle-verified against ruby
//! 4.0.5) that both the compiler's ancestor linearization and the runtime's
//! registry-free fallback chains are derived from. Ids are APPEND-ONLY:
//! renumbering is technically safe (nothing persists across builds), but
//! appending keeps generated-code diffs reviewable and eliminates any
//! stale-incremental-artifact risk.
//!
//! Zero dependencies, on purpose: the earlier decision against a shared
//! crate (Phase 14.4 rev.2) was about dragging the runtime's heavy deps
//! (`may`/`corosensei`) into every compiler build -- a dependency-free leaf
//! has no such cost.

/// Identifies a Ruby class at runtime AND at compile time -- the compiler
/// mirrors of this id are baked into generated code as literals, so the two
/// sides genuinely share one numbering (this type), not two synced copies.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(pub u32);

/// One reserved built-in class/module -- see [`BUILTINS`].
pub struct BuiltinClass {
    pub id: ClassId,
    /// The Ruby-visible name (`"Integer"`, `"Enumerable"`, ...).
    pub name: &'static str,
    /// `true` for a built-in MODULE (`Enumerable`, `Kernel`, `Math`): no
    /// superclass, never instantiated, participates in `ancestors` via
    /// `include` only.
    pub is_module: bool,
    /// CRuby's real superclass edge (oracle-verified). `None` for modules
    /// and for `BasicObject`, the true root.
    pub superclass: Option<ClassId>,
    /// CRuby's own mixins, in source order (oracle-verified): e.g.
    /// `Numeric` includes `Comparable`, `Array` includes `Enumerable`.
    pub includes: &'static [ClassId],
}

/// `ClassId(0)`, always present: the root every class ultimately chains up
/// to (via `BasicObject` since Phase 17.1). Not part of [`BUILTINS`] --
/// both sides construct/register `Object` specially (the compiler seeds it
/// as class index 0; the runtime's `Object` unit struct carries it as
/// `CLASS_ID`). Its own place in the chain is [`OBJECT_SUPERCLASS`] +
/// [`OBJECT_INCLUDES`]: `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_CLASS: ClassId = ClassId(0);

pub const INTEGER_CLASS: ClassId = ClassId(1);
pub const FLOAT_CLASS: ClassId = ClassId(2);
pub const STRING_CLASS: ClassId = ClassId(3);
pub const SYMBOL_CLASS: ClassId = ClassId(4);
pub const ARRAY_CLASS: ClassId = ClassId(5);
pub const HASH_CLASS: ClassId = ClassId(6);
pub const RANGE_CLASS: ClassId = ClassId(7);
pub const NIL_CLASS: ClassId = ClassId(8);
pub const TRUE_CLASS: ClassId = ClassId(9);
pub const FALSE_CLASS: ClassId = ClassId(10);
pub const PROC_CLASS: ClassId = ClassId(11);
pub const REGEXP_CLASS: ClassId = ClassId(12);
pub const MATCH_DATA_CLASS: ClassId = ClassId(13);
pub const FIBER_CLASS: ClassId = ClassId(14);
pub const THREAD_CLASS: ClassId = ClassId(15);
pub const MUTEX_CLASS: ClassId = ClassId(16);
pub const QUEUE_CLASS: ClassId = ClassId(17);
pub const RACTOR_CLASS: ClassId = ClassId(18);
/// The builtin `Enumerable` MODULE -- implemented in Rust in
/// `spinel_rt::builtins::enumerable` (the enum.c architecture); `include
/// Enumerable` linearizes this id into a class's `ancestors` exactly like
/// a user module.
pub const ENUMERABLE_CLASS: ClassId = ClassId(19);
/// `Class` and `Module` (Phase 16.1) -- the classes a first-class
/// class/module VALUE (`RubyValue::Class`) answers `.class` with:
/// `Widget.class == Class`, `Enumerable.class == Module`. Both are
/// themselves CLASSES (`Class.class == Class` in real Ruby); `Class`'s
/// superclass is `Module` (`Widget.is_a?(Module)` is true).
pub const CLASS_CLASS: ClassId = ClassId(20);
pub const MODULE_CLASS: ClassId = ClassId(21);
/// The builtin `Comparable` MODULE (Phase 16.2) -- every method drives the
/// includer's own `<=>` (the compar.c architecture).
pub const COMPARABLE_CLASS: ClassId = ClassId(22);
/// Reserved for Phase 17.2's fiber-backed Enumerator; registered with the
/// CRuby-correct ancestry now so ids stay append-only.
pub const ENUMERATOR_CLASS: ClassId = ClassId(23);
/// The true root (Phase 17.1): `BasicObject.superclass` is nil in Ruby;
/// every chain ends `..., Object, Kernel, BasicObject`.
pub const BASIC_OBJECT_CLASS: ClassId = ClassId(24);
/// The `Kernel` MODULE -- Object owns ZERO instance methods in CRuby;
/// every "universal" method (`class`, `dup`, `inspect`, `is_a?`, ...) is
/// Kernel's, mixed into Object.
pub const KERNEL_CLASS: ClassId = ClassId(25);
/// The numeric tower root: `Integer`/`Float`/`Rational`/`Complex` <
/// `Numeric`, which includes `Comparable`.
pub const NUMERIC_CLASS: ClassId = ClassId(26);
pub const RATIONAL_CLASS: ClassId = ClassId(27);
pub const COMPLEX_CLASS: ClassId = ClassId(28);
/// The `Math` MODULE (module functions `Math.sqrt` etc. + `PI`/`E`).
pub const MATH_CLASS: ClassId = ClassId(29);
/// `Struct` -- superclass of every compile-time-synthesized
/// `Name = Struct.new(...)` class; includes `Enumerable` (CRuby).
pub const STRUCT_CLASS: ClassId = ClassId(30);
/// `Enumerator::Yielder` (Phase 17.2) -- the `y` in
/// `Enumerator.new { |y| y << 1 }`. Registered under its FLAT
/// fully-qualified name (this table has no nesting edges); user code
/// resolving the `Enumerator::Yielder` path lexically gets a loud
/// NameError -- documented, since yielders are only ever OBTAINED, never
/// named.
pub const YIELDER_CLASS: ClassId = ClassId(31);
/// The `GC` MODULE (G0) -- spinel-rs uses `Arc` refcounting, so
/// `GC.start`/`stat`/`enable`/`disable`/`compact` are honest no-ops (see
/// `spinel_rt::dispatch`'s GC probe); the id exists so `GC` resolves as a
/// constant and `GC.start` dispatches cleanly instead of NameError-ing.
pub const GC_CLASS: ClassId = ClassId(32);
/// `IO` (G0, minimal) -- backs the `STDOUT`/`STDERR` singletons and the
/// `$stdout`/`$stderr` globals; the print family routes through whichever
/// value those globals hold. Full file-backed IO is a later phase (plan
/// P-B).
pub const IO_CLASS: ClassId = ClassId(33);
/// `Method` (G0/P4) -- the object `Kernel#method(:name)` answers; wraps a
/// bound receiver + method name and dispatches `#call` through `send`.
pub const METHOD_CLASS: ClassId = ClassId(34);

/// The plan P-B core classes. All are `RubyValue::Object(RObj)` over a
/// spinel-rt-resident struct -- `RubyValue` stays frozen at its 23 variants
/// (a new variant only pays for itself for structural Hash-key equality, a
/// codegen fast path, or an immediate; none of these qualify).
///
/// `File < IO` is CRuby's real edge, so a `File` instance answers every IO
/// instance method through the ordinary MRO walk with no duplication.
pub const FILE_CLASS: ClassId = ClassId(35);
pub const DIR_CLASS: ClassId = ClassId(36);
/// `Time` includes `Comparable` (CRuby), so `t1 < t2`/`between?`/`clamp`
/// all fall out of the existing `comparable_send` driver once `Time#<=>`
/// exists.
pub const TIME_CLASS: ClassId = ClassId(37);
/// `Process` is a MODULE (`Process.pid`, `Process::CLOCK_MONOTONIC`).
pub const PROCESS_CLASS: ClassId = ClassId(38);
/// `File::Stat` -- what `File.stat`/`File#stat` answer; the predicates
/// (`File.file?`, `.directory?`, `.size`) read through it.
pub const FILE_STAT_CLASS: ClassId = ClassId(39);
/// `Encoding` -- what `String#encoding` answers and `Encoding::UTF_8` names;
/// wraps an `encoding::EncodingId` in the runtime.
pub const ENCODING_CLASS: ClassId = ClassId(40);
/// `Data` -- superclass of every compile-time-synthesized
/// `Name = Data.define(...)` class. Unlike `Struct`, `Data` is
/// immutable and NOT `Enumerable` (no `each`). The one other subclassable
/// builtin besides `Struct`.
pub const DATA_CLASS: ClassId = ClassId(41);
pub const SET_CLASS: ClassId = ClassId(42);
pub const LAZY_CLASS: ClassId = ClassId(43);

/// Every reserved built-in class/module except `Object` (see
/// [`OBJECT_CLASS`]), in id order -- ids are contiguous from 1 by
/// construction (asserted by the unit test below), which is what lets the
/// compiler seed its class arena by pushing these in order. Superclass
/// edges may point FORWARD in the table (`Integer(1)` -> `Numeric(26)`);
/// consumers store the edge and linearize later.
pub const BUILTINS: &[BuiltinClass] = &[
    BuiltinClass { id: INTEGER_CLASS, name: "Integer", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[] },
    BuiltinClass { id: FLOAT_CLASS, name: "Float", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[] },
    BuiltinClass { id: STRING_CLASS, name: "String", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS] },
    BuiltinClass { id: SYMBOL_CLASS, name: "Symbol", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS] },
    BuiltinClass { id: ARRAY_CLASS, name: "Array", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: HASH_CLASS, name: "Hash", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: RANGE_CLASS, name: "Range", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: NIL_CLASS, name: "NilClass", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: TRUE_CLASS, name: "TrueClass", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: FALSE_CLASS, name: "FalseClass", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: PROC_CLASS, name: "Proc", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: REGEXP_CLASS, name: "Regexp", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: MATCH_DATA_CLASS, name: "MatchData", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: FIBER_CLASS, name: "Fiber", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: THREAD_CLASS, name: "Thread", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: MUTEX_CLASS, name: "Mutex", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: QUEUE_CLASS, name: "Queue", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: RACTOR_CLASS, name: "Ractor", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: ENUMERABLE_CLASS, name: "Enumerable", is_module: true, superclass: None, includes: &[] },
    BuiltinClass { id: CLASS_CLASS, name: "Class", is_module: false, superclass: Some(MODULE_CLASS), includes: &[] },
    BuiltinClass { id: MODULE_CLASS, name: "Module", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: COMPARABLE_CLASS, name: "Comparable", is_module: true, superclass: None, includes: &[] },
    BuiltinClass { id: ENUMERATOR_CLASS, name: "Enumerator", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: BASIC_OBJECT_CLASS, name: "BasicObject", is_module: false, superclass: None, includes: &[] },
    BuiltinClass { id: KERNEL_CLASS, name: "Kernel", is_module: true, superclass: None, includes: &[] },
    BuiltinClass { id: NUMERIC_CLASS, name: "Numeric", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS] },
    BuiltinClass { id: RATIONAL_CLASS, name: "Rational", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[] },
    BuiltinClass { id: COMPLEX_CLASS, name: "Complex", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[] },
    BuiltinClass { id: MATH_CLASS, name: "Math", is_module: true, superclass: None, includes: &[] },
    BuiltinClass { id: STRUCT_CLASS, name: "Struct", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: YIELDER_CLASS, name: "Enumerator::Yielder", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: GC_CLASS, name: "GC", is_module: true, superclass: None, includes: &[] },
    BuiltinClass { id: IO_CLASS, name: "IO", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: METHOD_CLASS, name: "Method", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: FILE_CLASS, name: "File", is_module: false, superclass: Some(IO_CLASS), includes: &[] },
    BuiltinClass { id: DIR_CLASS, name: "Dir", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: TIME_CLASS, name: "Time", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS] },
    BuiltinClass { id: PROCESS_CLASS, name: "Process", is_module: true, superclass: None, includes: &[] },
    BuiltinClass { id: FILE_STAT_CLASS, name: "File::Stat", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS] },
    BuiltinClass { id: ENCODING_CLASS, name: "Encoding", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: DATA_CLASS, name: "Data", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[] },
    BuiltinClass { id: SET_CLASS, name: "Set", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
    BuiltinClass { id: LAZY_CLASS, name: "Enumerator::Lazy", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS] },
];

/// `Object`'s own hierarchy slot (it isn't a [`BUILTINS`] row):
/// superclass `BasicObject`, includes `Kernel` -- oracle-verified
/// `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_SUPERCLASS: ClassId = BASIC_OBJECT_CLASS;
pub const OBJECT_INCLUDES: &[ClassId] = &[KERNEL_CLASS];

/// The Ruby-visible name of any builtin id, `Object` included. `None` for
/// user-class ids. Retires the runtime's hand-maintained variant->name
/// match (NoMethodError messages, registry-less display).
pub fn builtin_name(id: ClassId) -> Option<&'static str> {
    if id == OBJECT_CLASS {
        return Some("Object");
    }
    BUILTINS
        .get((id.0 as usize).wrapping_sub(1))
        .filter(|b| b.id == id)
        .map(|b| b.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contiguity both consumers rely on: the compiler pushes
    /// `BUILTINS` in order into its class arena (so each entry's index must
    /// equal its id), and the runtime treats these ids as stable literals
    /// baked into generated programs.
    #[test]
    fn builtin_ids_are_contiguous_from_one() {
        for (i, b) in BUILTINS.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i + 1, "{} out of order", b.name);
        }
    }

    /// Every hierarchy edge points at a real table entry (or Object), and
    /// modules never claim a superclass -- the shape `Compiler::new`'s
    /// table walk and the runtime's registry-free chains both assume.
    #[test]
    fn hierarchy_edges_are_well_formed() {
        let exists = |id: ClassId| id == OBJECT_CLASS || builtin_name(id).is_some();
        for b in BUILTINS {
            if b.is_module {
                assert!(b.superclass.is_none(), "module {} has a superclass", b.name);
            }
            if let Some(sup) = b.superclass {
                assert!(exists(sup), "{}'s superclass id is unknown", b.name);
                assert_ne!(sup, b.id, "{} is its own superclass", b.name);
            }
            for &inc in b.includes {
                assert!(exists(inc), "{}'s include id is unknown", b.name);
            }
        }
        assert!(exists(OBJECT_SUPERCLASS));
        for &inc in OBJECT_INCLUDES {
            assert!(exists(inc));
        }
    }

    #[test]
    fn builtin_name_answers_object_builtins_and_unknowns() {
        assert_eq!(builtin_name(OBJECT_CLASS), Some("Object"));
        assert_eq!(builtin_name(INTEGER_CLASS), Some("Integer"));
        assert_eq!(builtin_name(STRUCT_CLASS), Some("Struct"));
        assert_eq!(builtin_name(ClassId(999)), None);
    }
}
