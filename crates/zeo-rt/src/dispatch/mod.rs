//! Object model & dynamic dispatch. See the plan's "Object model & dynamic
//! dispatch" section for the full design rationale: two dispatch paths
//! (static direct/match-on-class_id calls, generated entirely by `zeo`;
//! and this module's runtime `ClassRegistry`/`send`, reached only when
//! `zeo` cannot resolve a call statically). Zeo itself never needs
//! this module at all -- it's the one deliberate architectural addition.

use crate::builtins::{arg_error, frozen_error, name_error, type_error};
use crate::{FMap, FSet};
use crate::{RubyValue, Signal, Symbol};
use std::any::Any;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

mod alloc;
mod caches;
mod classmeta;
pub(crate) mod concealed;
mod define;
mod errors;
mod impls;
mod invoke;
pub(crate) mod ivars;
mod kwargs;
pub(crate) mod lookup;
pub(crate) mod names;
mod object;
mod reflect;
mod registry;
mod relations;
mod send_obj;
mod send_value;
mod singleton;
#[cfg(test)]
mod tests;
mod walk;
pub use alloc::*;
pub use caches::*;
pub use classmeta::*;
pub use define::*;
pub use errors::*;
pub use impls::*;
pub use invoke::*;
pub use ivars::*;
pub use kwargs::*;
pub use lookup::*;
pub use object::*;
pub use reflect::*;
pub use registry::*;
pub use relations::*;
pub use send_obj::*;
pub use send_value::*;
pub use singleton::*;
pub use walk::*;

/// Identifies a Ruby class at runtime. This is the SHARED `zeo-abi` type --
/// the compiler bakes the same numbering into generated code from the same
/// source of truth, so there is nothing to keep in sync by hand.
pub use zeo_abi::ClassId;

/// Implemented (via `ruby_class!`) by every generated Ruby class, and by the
/// built-in `Object` root below. `Send + Sync` supertrait bounds:
/// satisfied automatically for every generated class once its own fields are
/// `parking_lot::Mutex`-wrapped, needing no manual `unsafe impl` anywhere.
pub trait RubyObject: Any + Send + Sync {
    fn class_id(&self) -> ClassId;

    /// Lets `send`'s dispatcher downcast an erased `RObj` back to its
    /// concrete type. No default body: a default `{ self }` here would be
    /// type-checked generically over an unsized `Self` (rustc checks default
    /// bodies once, not per-impl) and fail to coerce to `&dyn Any`. Each
    /// concrete impl provides the one-line body instead (`ruby_class!`
    /// generates it), where `Self` is always a concrete, sized type.
    fn as_any(&self) -> &dyn Any;

    /// The owned-handle counterpart to `as_any` -- needed because every
    /// generated method now takes `self: Arc<Self>` (not `&self`, see
    /// `ruby_class!`'s docs), so Path 2's dynamic trampolines need an
    /// `Arc<Concrete>`, not a `&Concrete`, to actually call one. `Arc<dyn Any
    /// + Send + Sync>` supports a real consuming `downcast::<T>()`; `Arc<dyn
    /// RubyObject>` doesn't (there's no such inherent method on an arbitrary
    /// trait object), so this is the bridge -- same "no default body"
    /// reasoning as `as_any` above.
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;

    /// `.frozen?` state -- backed by the `__frozen: AtomicBool`
    /// field `ruby_class!` generates on every class struct (the per-object
    /// counterpart of `collections::Freezable`'s flag). On the trait (not
    /// just inherent) so `freeze_value`/`is_frozen_value` can reach it
    /// through an erased `RObj` without downcasting. The frozen CHECK before
    /// an ivar write is emitted by codegen (`emit_ivar_write_stmt`'s guard),
    /// which alone can construct the `FrozenError` to raise.
    fn is_frozen(&self) -> bool;

    /// Marks this object frozen -- `.freeze`'s storage half; a repeat call
    /// is a harmless no-op, matching CRuby's own already-frozen guard.
    fn set_frozen(&self);

    /// The cycle collector's enumerator: every strong reference this object
    /// owns, each EXACTLY once. `take` = false clones them (the reference
    /// walk); `take` = true moves them out and leaves the slots `Nil` (the
    /// sweep). One method serves both so the list the walk counted and the
    /// list the sweep releases cannot disagree.
    ///
    /// The two failure directions are not symmetric, which is what makes the
    /// empty default safe and the rollout incremental: an OMITTED edge only
    /// leaks (the target keeps an unexplained reference, so the collector
    /// reads it as live), while a duplicated or invented one over-subtracts
    /// and could clear an object something still points at. So a type that
    /// cannot release a reference must not report it either -- an immutable
    /// payload (`Range`, `Proc`) reports nothing and is simply never
    /// reclaimed.
    ///
    /// `out` is where values GO rather than being dropped here: a drop can
    /// cascade into another container's lock, and the caller may hold one.
    fn gc_visit(&self, _out: &mut Vec<RubyValue>, _take: bool) {}

    /// A snapshot of every ivar's current value -- the runtime
    /// ivar ENUMERATION `Ractor`'s recursive shareability check and
    /// `make_shareable`'s deep-freeze traversal need. (Ractor sends still
    /// reject an unfrozen Object rather than deep-copying it -- see
    /// `ractor`'s module docs for that documented divergence.) Generated by
    /// `ruby_class!` from its own ivar list.
    fn ivar_values(&self) -> Vec<RubyValue>;

    /// Every ivar as an ordered `("@name", value)` pair -- what the default
    /// `Object#inspect` rendering and `instance_variables` reflection need,
    /// with names and values paired in ONE pass (so a name-keyed root
    /// `Object`'s map can't desync the two). Generated by `ruby_class!` in
    /// field-declaration order; the default is empty for the runtime's
    /// hand-written objects, which expose no Ruby-visible ivars.
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        Vec::new()
    }

    /// Read one ivar BY NAME (without the `@`), for the paths where the
    /// receiver's concrete class isn't statically known and codegen
    /// therefore couldn't emit a direct field access: `instance_exec`'s
    /// rebound self, and the top-level `main` object. `None` means this
    /// class declares no such ivar -- distinct from `Some(Nil)`, which is a
    /// declared-but-unassigned one. Generated by `ruby_class!` from its own
    /// ivar list; the default keeps the trait total for the runtime's
    /// hand-written objects, which have no Ruby-visible ivars.
    ///
    /// An ivar the class never names statically (`obj.instance_exec {
    /// @brand_new = 1 }`, `instance_variable_set` with a novel name) lives in
    /// the generated struct's `__overflow` map -- `ruby_class!` routes both
    /// accessors through it after the typed fields miss, so the named path is
    /// total over declared AND invented ivars.
    fn ivar_get_named(&self, _name: &str) -> Option<RubyValue> {
        None
    }

    /// Write one ivar BY NAME -- `ivar_get_named`'s counterpart, same
    /// statically-unknown-receiver paths. Answers whether the write landed
    /// (generated classes always land it, via typed field or `__overflow`;
    /// only ivar-less hand-written runtime objects answer `false`).
    fn ivar_set_named(&self, _name: &str, _v: RubyValue) -> bool {
        false
    }

    /// Remove one ivar BY NAME (without the `@`) -- `Kernel#remove_instance_variable`'s
    /// storage half. `Some(old)` is the removed value; `None` means the object
    /// has no such ivar (the caller raises `NameError`). The default keeps the
    /// trait total for the runtime's hand-written objects. See the caveat in
    /// `ivar_get_named`'s TODO: a generated struct field always physically
    /// exists, so a declared-but-never-assigned field answers `Some(Nil)` where
    /// CRuby raises -- name-keyed objects (`Object`/`DynObject`) don't have this
    /// gap because their map genuinely lacks absent keys.
    fn ivar_remove_named(&self, _name: &str) -> Option<RubyValue> {
        None
    }

    /// A `Struct`/`Data` MEMBER by position, in declaration order.
    ///
    /// Members are real slots on the generated struct but are NOT instance
    /// variables: CRuby answers `nil` to `S.new(1).instance_variable_get(:@x)`
    /// and `[]` to `instance_variables`. Every by-name path above is therefore
    /// blind to them, and the native protocol (`to_a`, `[]`, `==`, `each`,
    /// `dig`, `Marshal`) reaches them here instead, by the index it already
    /// knows. `None`/`false` for an out-of-range index and for every ordinary
    /// class, which declares no members.
    fn hidden_ivar_get(&self, _i: usize) -> Option<RubyValue> {
        None
    }

    fn hidden_ivar_set(&self, _i: usize, _v: RubyValue) -> bool {
        false
    }

    /// Read one ivar BY SLOT, the index it occupies in this class's
    /// [`crate::IvarCell`].
    ///
    /// A body a base class and its descendants share
    /// has a `RubyValue` receiver, so it cannot name a concrete struct to
    /// index -- but the slot number is still a compile-time constant, because
    /// `analyze::mro` lays every class's slots out parent-first. This pair is
    /// what lets such a body keep indexing instead of falling back to the
    /// name scan. Out-of-range answers `nil` rather than panicking, matching
    /// the named accessors' totality.
    fn ivar_slot_get(&self, _slot: usize) -> RubyValue {
        RubyValue::Nil
    }

    /// Write one ivar BY SLOT -- `ivar_slot_get`'s counterpart. The caller has
    /// already checked frozenness, exactly as the by-name path's own guard
    /// does at its call site.
    fn ivar_slot_set(&self, _slot: usize, _value: RubyValue) {}

    /// Move every ivar holding another OBJECT into `out`, leaving `nil` behind.
    ///
    /// Only [`crate::IvarCell`]'s release path calls this, and only on an
    /// object it is about to drop as the last reference: taking the links
    /// first is what lets a long chain be released iteratively instead of one
    /// stack frame per link. The default keeps the trait total for the
    /// runtime's own hand-written objects, which hold no user ivars.
    fn take_linked_ivars(&self, _out: &mut Vec<RubyValue>) {}

    /// `Kernel#dup`/`#clone`'s per-class shallow copy: a fresh
    /// instance of the same concrete struct with every ivar's CURRENT value
    /// cloned into it (a `RubyValue` clone is a handle clone, so nested
    /// objects are SHARED -- CRuby's own shallow rule). `copy_frozen` is
    /// the one `dup`-vs-`clone` difference: `clone` carries the receiver's
    /// frozen flag over, `dup` starts unfrozen. Generated by `ruby_class!`
    /// from its own ivar list (only the concrete impl knows the fields);
    /// `initialize_copy`/singleton-state carryover are documented
    /// scope-cuts. See `RubyValue::dup_value` for the non-Object kinds.
    fn dup_object(&self, copy_frozen: bool) -> RObj;

    /// The wrapped builtin value of a value-builtin SUBCLASS instance (D3):
    /// `class Stack < Array` stores a `RubyValue::Array` here, so inherited
    /// `Array` methods run against it (the `send_in` payload bridge). `None`
    /// for every ordinary object -- only `ValueSubclass` overrides these three.
    /// Returns a HANDLE clone (cheap `Arc` bump), never holding the internal
    /// lock across the re-entrant dispatch that follows.
    fn builtin_payload(&self) -> Option<RubyValue> {
        None
    }
    /// The root builtin whose methods this subclass inherits (`Array` for
    /// `Stack < Array`) -- the ancestor the payload bridge substitutes at.
    fn builtin_root(&self) -> Option<ClassId> {
        None
    }
    /// Re-seat the whole payload (a `super`/`replace`/`initialize` that rebuilds
    /// the collection). `false` for a non-value-subclass. The single write path.
    fn set_builtin_payload(&self, _v: RubyValue) -> bool {
        false
    }

    /// A `Ractor` move's poison: re-tag this instance's class word to
    /// `Ractor::MovedObject`, so `class_id()` -- and with it dispatch,
    /// `method_missing`, `===`, inspect -- REALLY answers the husk class.
    /// `true` when the concrete type could retag (an atomic class word);
    /// the default `false` leaves a hand-written builtin `RObj` usable
    /// after a move -- the safe direction, documented on
    /// `ractor::cross_graph`.
    fn retag_moved(&self) -> bool {
        false
    }
}

/// A handle to any live Ruby object, used wherever the concrete class isn't
/// statically known. The Rust trait object's vtable *is* the tag. Mutability
/// lives on individual ivar fields (see `ruby_class!`), not on this handle, so
/// no lock layer is needed here. `Arc` (not `Rc`): every concrete `RubyObject` impl is
/// `Send + Sync` (via the trait's own supertrait bounds above), so this type
/// itself is genuinely `Send + Sync` -- no `unsafe impl` needed.
pub type RObj = Arc<dyn RubyObject>;

/// The builtin `Enumerable` MODULE -- an ordinary
/// `class_table` row (`builtins::enumerable`'s generated `lookup`), reached
/// by the MRO walk for any receiver whose ancestors contain this id. Like
/// every id above, re-exported from the shared `zeo-abi` numbering.
pub use zeo_abi::ENUMERABLE_CLASS;
/// Fixed, well-known `ClassId`s for every built-in Ruby type this runtime
/// models as a `RubyValue` variant rather than a generated `ruby_class!`
/// struct -- numerically mirrored by `zeo::compiler::BUILTIN_CLASSES`
/// (same "two `ClassId` types, on purpose" convention as `Object::CLASS_ID`/
/// `compiler::OBJECT_CLASS`). Registered into the `ClassRegistry` once, from
/// generated `main()` (`clif::classes`' `register_builtin` rows), with the
/// SAME linearized `ancestors` every user class gets -- what makes
/// `5.is_a?(Integer)`/`"x".is_a?(Object)`-style checks against a built-in-
/// typed receiver work uniformly through the one general `is_a`/`send`
/// mechanism, and the prerequisite for eventually `include`ing a plain-Ruby
/// `Enumerable`/`Comparable` into these types via ordinary materialization.
pub use zeo_abi::{
    ARRAY_CLASS, BASIC_OBJECT_CLASS, CLASS_CLASS, COMPARABLE_CLASS, COMPLEX_CLASS,
    ENUMERATOR_CLASS, FALSE_CLASS, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS,
    KERNEL_CLASS, MATCH_DATA_CLASS, MATH_CLASS, MODULE_CLASS, MUTEX_CLASS, NIL_CLASS,
    NUMERIC_CLASS, PROC_CLASS, QUEUE_CLASS, RACTOR_CLASS, RANGE_CLASS, RATIONAL_CLASS,
    REGEXP_CLASS, SIZED_QUEUE_CLASS, STRING_CLASS, STRUCT_CLASS, SYMBOL_CLASS, THREAD_CLASS,
    TRUE_CLASS, YIELDER_CLASS,
};
