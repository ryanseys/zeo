//! Runtime metaprogramming: the seam that fills
//! `MethodImpl::Dynamic`. Everything the AOT compiler bakes into the frozen,
//! write-once `REGISTRY` (dispatch.rs) is reached lock-free through `&'static`
//! reads. Anything DEFINED WHILE THE PROGRAM RUNS -- `define_method` with a
//! computed name, a per-object singleton (`def obj.foo`, `class << obj`), an
//! anonymous `Class.new` -- can't live there: the registry hands out no
//! `&mut` after install. This module is the parallel, lock-guarded overlay
//! those definitions land in.
//!
//! ## The `live` gate is the whole performance story
//!
//! A program that never defines at runtime must pay nothing. `OVERLAY_LIVE`
//! is a plain `AtomicBool` that starts `false` and flips `true`, forever, on
//! the first runtime definition -- and only then are the overlay's maps even
//! allocated (`OVERLAY` is a `OnceLock`). Every dispatch resolver reads that
//! one relaxed-ish atomic before touching anything here, so the frozen
//! lock-free hot path is unchanged for all existing code.
//!
//! ## No lock is ever held across a dispatched call
//!
//! `MethodImpl` is `Clone` (a `fn` copy or an `Arc` bump). Every resolver
//! read-locks, clones the entry out, drops the guard, and only then returns it
//! to the caller to `.call()`. A runtime method whose body defines ANOTHER
//! method therefore takes the write lock without deadlocking against a read
//! lock held across its own execution.

use crate::builtins::{arg_error, name_error, runtime_error, type_error};
use crate::dispatch::{
    ConstructorFn, MethodImpl, RObj, RubyObject, ancestors_of_value, registry_lookup_cloned,
    send_super_from,
};
use crate::{ClassId, RProc, RubyValue, Signal, Symbol};
use crate::{FMap, FSet};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use zeo_abi::RUNTIME_CLASS_ID_BASE;

/// One runtime-defined class, OR a set of runtime method deltas over a frozen
/// class (a `define_method` that overrides/adds to an existing class). The two
/// are the same shape: a delta entry simply leaves `ancestors` empty and
/// `constructor` `None`, so the frozen registry (which wins for a frozen id's
/// ancestors/constructor/name) is untouched, and only `methods`/`class_methods`
/// are consulted as overrides.
struct OverlayEntry {
    /// Anonymous (`None`) until a constant assignment names it
    /// (`Foo = Class.new`, CRuby's rule) -- interior-mutable so naming doesn't
    /// need a write lock on the whole `classes` map.
    name: RwLock<Option<String>>,
    is_module: bool,
    /// A fully runtime class's linearized MRO, LEAKED to `&'static` at creation
    /// so `ancestors_of_value` can hand it out like a frozen chain. Empty for a
    /// pure method-delta over a frozen class (that id's real ancestors live in
    /// the frozen registry). Leaking matches this project's documented
    /// no-runtime-GC policy: a runtime class lives for the process.
    ancestors: &'static [ClassId],
    /// Instance methods, self = the receiver object (`MethodImpl::Dynamic`).
    methods: FMap<Symbol, MethodImpl>,
    /// Explicit RUNTIME visibility marks (`Foo.class_eval { private :m }`,
    /// an alias inheriting its source's visibility): nearest ancestor's mark
    /// wins in `instance_method_visibility`'s walk. A name absent here but
    /// present in `methods` is public (a runtime `define_method` is). Keyed
    /// separately from `methods` because a mark can target a FROZEN-registry
    /// or builtin method the overlay never carries a body for.
    methods_vis: FMap<Symbol, crate::dispatch::MethodVisibility>,
    /// The class-method half: which of this class's CLASS methods a runtime
    /// The running default visibility a bare `private`/`public` set in a
    /// COMPILED `class << self` body -- reached as a rebound send on the
    /// surrogate, which has no thread-local body frame to record into. Read
    /// by `runtime_define_method`'s singleton redirect so a `define_method`
    /// after the directive installs with it; the lowering appends a `public`
    /// reset at body end, so it dies with the body like a cref cursor.
    singleton_default_vis: Option<crate::dispatch::MethodVisibility>,
    /// `private_class_method`/`public_class_method` marked. `true` is private,
    /// `false` a `public_class_method` promotion -- both recorded, because
    /// either has to beat whatever the frozen registry baked in.
    class_methods_vis: FMap<Symbol, bool>,
    /// Class/singleton-on-class methods (`def self.x`, `define_singleton_method`
    /// on a `Class` value): self = the `RubyValue::Class`, which the RObj-shaped
    /// `MethodImpl` can't carry -- so these are the raw `RProc`, invoked via
    /// `call_with_self(class_value, args)`.
    class_methods: FMap<Symbol, RProc>,
    /// The raw `RProc` behind whichever of `methods` above a `define_method`
    /// installed. `MethodImpl::call` takes an `RObj`, and a BUILTIN receiver
    /// (`5`, `[1, 2]`, a Range) has none -- so value dispatch calls the proc
    /// with the value itself as self. Only `define_method` fills this: an
    /// `attr_*`, an alias, or a copy from a `Method` object has no proc behind
    /// it and stays object-only.
    value_bodies: FMap<Symbol, RProc>,
    /// Which of `class_methods` above an `extend` copied in, rather than a
    /// `def self.x`/`define_singleton_method`/`module_function` writing here.
    /// The copies are indistinguishable once installed, and only
    /// `singleton_methods(false)` needs them apart: an extended module sits in
    /// the singleton class's SUPER chain in CRuby, so the narrow list skips it
    /// while the wide one reports it. Every own-definition write clears the
    /// name, so re-defining over a mixin makes it own again.
    extended_class_methods: FSet<Symbol>,
    /// Class methods a `singleton_class.prepend(M)` copied in -- the
    /// class-method twin of `prepended` below, and apart from `class_methods`
    /// for the same reason: ruby puts a prepended module in its own layer
    /// ahead of the singleton's table, so these must outrank an own
    /// `def self.x` and survive one defined later. Probed first.
    prepended_class_methods: FMap<Symbol, RProc>,
    /// The modules behind `prepended_class_methods`, in mix-in order.
    /// `send_super_class_from` reads this to resume a prepended method's
    /// `super` AT this class's own table rather than past it.
    singleton_prepends: Vec<ClassId>,
    constructor: Option<ConstructorFn>,
    /// Methods a PREPENDED module supplies, kept apart from `methods` so a
    /// later definition on the target cannot displace them -- ruby puts a
    /// prepended module in its own layer ahead of the class, and `prepend M`
    /// followed by `def m` leaves M's `m` winning. Probed before `methods`.
    prepended: FMap<Symbol, MethodImpl>,
    /// Names `undef_method` removed. An ENTRY, not a deletion: it TERMINATES
    /// the MRO walk here, so an ancestor's still-live definition can't answer
    /// for a descendant that undef'd the name. Mirrors the frozen registry's
    /// `ClassEntry::undefined_methods`.
    undefs: FSet<Symbol>,
    /// [`OverlayEntry::undefs`]'s class-method twin: names retired by an
    /// `undef` written inside `class << self`.
    ///
    /// A singleton class's instance methods ARE its owner's class methods, so
    /// `Sub.singleton_class.undef_method(:x)` has to land in the OWNER's
    /// class-method space. Tombstoning it under the singleton's own id instead
    /// left `Sub.x` answering as if nothing had happened -- nothing on the
    /// class-method path ever looks at a singleton id.
    class_undefs: FSet<Symbol>,
    /// The address the anonymous `#<Class:0x...>` rendering reports -- a real
    /// leaked allocation, so it is unique, stable, and 16 hex digits wide like
    /// every other object's. Not `ancestors.as_ptr()`: `include`/`prepend`
    /// re-leak that slice.
    addr: usize,
    /// `Class.allocate` handed this one out and no `initialize` has run, so it
    /// has no superclass AT ALL -- distinct from "its superclass is
    /// `BasicObject`", and the difference is what `#superclass` reports.
    uninitialized: bool,
    /// `(refining module, refined target)` when this entry is a holder a
    /// RUNTIME `refine` minted -- the overlay twin of
    /// `ClassEntry::refinement_of`. Definition only: nothing installs these
    /// methods anywhere until a `using` activates them.
    refinement_of: Option<(ClassId, ClassId)>,
    /// The user class this minted module is an INSTANCE of -- what
    /// `class DeprecatedConstantProxy < Module; end` produces when its `new`
    /// runs. Rails' whole deprecation layer is this shape, and 22 of the
    /// corpus's `Module` rows come from that one file.
    ///
    /// The value stays a real module id, so `include`, `Module#===`,
    /// `ancestors` and constant lookup keep working unchanged; only the
    /// question "what is your class?" changes answer. `None` for every
    /// ordinary `Module.new`, whose class is `Module` itself.
    owner_class: Option<ClassId>,
}

impl Default for OverlayEntry {
    fn default() -> Self {
        OverlayEntry {
            name: RwLock::new(None),
            is_module: false,
            ancestors: &[],
            methods: FMap::default(),
            prepended: FMap::default(),
            methods_vis: FMap::default(),
            singleton_default_vis: None,
            class_methods_vis: FMap::default(),
            class_methods: FMap::default(),
            value_bodies: FMap::default(),
            extended_class_methods: FSet::default(),
            prepended_class_methods: FMap::default(),
            singleton_prepends: Vec::new(),
            constructor: None,
            undefs: FSet::default(),
            class_undefs: FSet::default(),
            addr: Box::leak(Box::new(0u8)) as *const u8 as usize,
            uninitialized: false,
            refinement_of: None,
            owner_class: None,
        }
    }
}

impl OverlayEntry {
    /// A pure method-delta over an existing frozen class -- no ancestors, no
    /// constructor of its own.
    fn delta() -> OverlayEntry {
        OverlayEntry::default()
    }
}

struct OverlayMaps {
    classes: RwLock<FMap<u32, OverlayEntry>>,
    /// Per-object singleton methods, keyed by the receiver's `Arc` DATA address
    /// (object identity). Not carried across `dup` -- a fresh `Arc` is a fresh
    /// address -- matching Ruby (`dup` drops singletons); `clone` re-keys the
    /// tables onto the copy via [`copy_value_singletons`].
    singletons: RwLock<FMap<usize, FMap<Symbol, MethodImpl>>>,
    /// Singleton methods on a NON-object heap value (`def SOME_ARRAY.[](i)`),
    /// keyed the same way. Separate from `singletons` because there is no
    /// `RObj` to bind: the body stays an `RProc` and runs with the value itself
    /// as `self`. See `value_identity`.
    value_singletons: RwLock<FMap<usize, FMap<Symbol, RProc>>>,
    /// Names `obj.singleton_class.undef_method(:name)` retired for ONE object,
    /// keyed by the same identity the two tables above use. A tombstone, not an
    /// absence: the class still defines the name, and the point of the undef is
    /// that this object no longer answers it. `OverlayEntry::undefs` is the
    /// per-CLASS twin; there is no per-object `OverlayEntry` to put this in.
    singleton_undefs: RwLock<FMap<usize, FSet<Symbol>>>,
    /// A strong reference to every value that has ever received a singleton
    /// method, keyed by the same identity the tables above use.
    ///
    /// Identity here IS a heap address, so it is only unique while the object
    /// lives. Without this, a value that received a singleton and then died let
    /// the allocator hand its address to an unrelated later object, which
    /// silently inherited the dead one's methods -- `def s.shout` on a string
    /// built in a loop made every later same-sized string answer `shout`.
    /// Holding the owner makes the address un-reusable, which is the only
    /// thing that makes the key sound.
    pinned: RwLock<FMap<usize, RubyValue>>,
    /// The modules `recv.extend(M)` mixed into one receiver, newest LAST,
    /// keyed by [`extend_key`]. Separate from the method tables above because
    /// `extend` changes what the receiver IS, not only what it answers:
    /// `o.is_a?(M)` and `o.singleton_class.ancestors` both read this, and
    /// neither can be recovered from a copied method table.
    extended: RwLock<FMap<usize, Vec<ClassId>>>,
    /// WHICH module a per-object `extend` copied each singleton-table name
    /// from -- `obj.method(:x).owner`'s record, since the copy itself can't
    /// say. Cleared per name by every own definition (`def obj.x` shadows
    /// the module's copy and owns the name from then on), and by an undef.
    /// The record, not a guess: "an extended module that defines the name
    /// owns it" is wrong exactly when a later own def shadows one.
    extended_names: RwLock<FMap<usize, FMap<Symbol, ClassId>>>,
    /// `obj.singleton_class`'s cache: object identity -> the runtime class id
    /// minted for its singleton class (so a second call answers the same id,
    /// matching Ruby's identity).
    singleton_classes: RwLock<FMap<usize, ClassId>>,
    /// The inverse plus the owner value: a singleton-class id -> the object (or
    /// class) it belongs to. A `define_method` on that id installs a per-object
    /// singleton (or, for a class owner, a class method) rather than an ordinary
    /// instance method -- which is exactly what `class << obj` semantics mean.
    singleton_owner: RwLock<FMap<u32, RubyValue>>,
    next_id: AtomicU32,
}

/// The gates the dispatch fast path reads, in ONE atomic:
///
/// * [`GATE_OVERLAY`] -- something has been defined at runtime, so the overlay
///   may answer where the frozen tables would not.
/// * [`GATE_PENDING`] -- a definition hook is running with names still ahead of
///   it, so a resolved entry may not exist yet (see [`with_pending_defs`]).
/// * [`GATE_MOVED`] -- some object was gutted by a `Ractor` move, so a
///   dispatch receiver may be a husk that must raise `Ractor::MovedError`.
///   Deliberately NOT part of [`is_live`]'s mask: a move must not deopt the
///   inline caches or the overlay shortcuts, only arm the husk probes.
///
/// One word rather than separate `AtomicBool`s because the fast-path readers
/// must stay a single load (and, for [`is_live`], a single masked compare).
/// As a second flag beside the first, the pending gate measured +2.6% on a
/// loop whose body is nothing but a cached dynamic send; folded in here it
/// is free -- the moved gate rides the same byte for the same reason.
static GATES: AtomicU8 = AtomicU8::new(0);
const GATE_OVERLAY: u8 = 1;
const GATE_PENDING: u8 = 2;
const GATE_MOVED: u8 = 4;
/// `ZEO_ARITY_DEBUG` is armed -- folded into the gate byte the dispatch path
/// already loads, per the standing rule: a second flag word beside the gates
/// measured 2.6% on dispatch.
const GATE_ARITY_DEBUG: u8 = 8;
/// The four latches below used to be separate `AtomicBool`s, which put FOUR
/// acquire loads in `iter_inline_ok_for` -- a fused loop's entry test, i.e.
/// the check every inlined `each`/`map` pays before it may splice. They ride
/// the gate byte for exactly the reason the pending and moved gates do: the
/// byte is loaded once and masked. That fills the `u8`; a ninth gate needs a
/// `u16`, not a second word.
const GATE_PATCHED_ANY: u8 = 16;
const GATE_ANY_SINGLETONS: u8 = 32;
const GATE_ANCESTRY_MUTATED: u8 = 64;
const GATE_ANY_EXTENDED: u8 = 128;
const GATE_LIVE_MASK: u8 = GATE_OVERLAY | GATE_PENDING;
/// What forbids a fused-iterator splice, apart from the receiver's own
/// patched state.
const GATE_ITER_BLOCKED: u8 = GATE_ANY_SINGLETONS | GATE_ANCESTRY_MUTATED | GATE_MOVED;
static OVERLAY: OnceLock<OverlayMaps> = OnceLock::new();

fn maps() -> &'static OverlayMaps {
    OVERLAY.get_or_init(|| OverlayMaps {
        classes: RwLock::new(FMap::default()),
        singletons: RwLock::new(FMap::default()),
        value_singletons: RwLock::new(FMap::default()),
        singleton_undefs: RwLock::new(FMap::default()),
        extended: RwLock::new(FMap::default()),
        extended_names: RwLock::new(FMap::default()),
        pinned: RwLock::new(FMap::default()),
        singleton_classes: RwLock::new(FMap::default()),
        singleton_owner: RwLock::new(FMap::default()),
        next_id: AtomicU32::new(RUNTIME_CLASS_ID_BASE),
    })
}

/// The dispatch hot-path gate: `true` while the frozen tables may not be the
/// whole answer -- either something has been defined at runtime, or a
/// definition hook is running and part of its class is not installed yet.
///
/// Every reader wants that union, not one gate or the other: each of them
/// guards a shortcut that only a completely settled program may take. So this
/// asks whether ANY bit is set, which keeps it the one load and one
/// compare-against-zero it was when the overlay was the only gate.
#[inline(always)]
pub fn is_live() -> bool {
    GATES.load(Ordering::Acquire) & GATE_LIVE_MASK != 0
}

/// The whole gate byte in one load, for the callers that ask more than one
/// gate question per dispatch ([`crate::dispatch::send_value_cached`]) --
/// same single atomic load `is_live` costs, split by [`gates_live`]/
/// [`gates_moved`] with plain register tests.
#[inline(always)]
pub(crate) fn gates() -> u8 {
    GATES.load(Ordering::Acquire)
}

#[inline(always)]
pub(crate) fn gates_live(g: u8) -> bool {
    g & GATE_LIVE_MASK != 0
}

#[inline(always)]
pub(crate) fn gates_moved(g: u8) -> bool {
    g & GATE_MOVED != 0
}

#[inline(always)]
pub(crate) fn gates_arity_debug(g: u8) -> bool {
    g & GATE_ARITY_DEBUG != 0
}

/// Arm the `ZEO_ARITY_DEBUG` breadcrumb bit -- called once from program
/// startup ([`crate::exec::run_main`]) when the variable is set.
pub(crate) fn arm_arity_debug() {
    GATES.fetch_or(GATE_ARITY_DEBUG, Ordering::Release);
}

/// Whether any `Ractor` move has ever poisoned an object -- the cheap gate
/// in front of every husk probe off the dispatch fast path.
#[inline(always)]
pub fn any_moved() -> bool {
    GATES.load(Ordering::Acquire) & GATE_MOVED != 0
}

/// Arm the moved gate -- once per `move: true` send that actually poisoned
/// something (`ractor::cross_graph`'s commit), never cleared.
pub(crate) fn mark_moved() {
    GATES.fetch_or(GATE_MOVED, Ordering::Release);
}

/// The typed-iterator fusion gate (`codegen`'s `emit_typed_iter_inline`): a
/// fused native loop bypasses dispatch entirely, which is only sound while
/// nothing could have overridden the builtin iterator -- no runtime-defined
/// method anywhere (a reopen or a per-object singleton would win the lookup)
/// and not inside a box (a box may carry its own override). The same rule the
/// flat dispatch maps apply, asked once per loop entry.
///
/// The `_for` form asks the narrow question -- could anything have overridden
/// the iterator on THIS receiver's class? -- which is the condition the
/// paragraph above actually describes. `iter_inline_ok` is the whole-process
/// approximation of it, still used where the receiver class is not a
/// compile-time constant. One `Struct.new` measured **+55% on fused-iterator
/// work** through the wide form (0.53s -> 0.81s), and it is on the load path
/// of every rubygems and bundler program.
// Both forms also refuse once any Ractor move happened: a fused loop reads
// its receiver's payload directly and would iterate a husk's gutted storage
// instead of raising `Ractor::MovedError` -- after the first move, fused
// loops fall back to dispatch (whose probes answer correctly). The wide form
// tests the WHOLE gate byte (the same one load it always did); the `_for`
// form pays one extra load only because its other gates live outside GATES.
#[inline(always)]
pub fn iter_inline_ok(box_id: u32) -> bool {
    box_id == 0 && GATES.load(Ordering::Acquire) == 0
}

#[inline(always)]
pub fn iter_inline_ok_for(box_id: u32, recv: ClassId) -> bool {
    if box_id != 0 {
        return false;
    }
    // ONE load, then masks -- see `GATE_PATCHED_ANY`.
    let g = GATES.load(Ordering::Acquire);
    g & GATE_ITER_BLOCKED == 0 && !class_maybe_patched_gated(g, recv)
}

// ---------------------------------------------------------------------------
// The narrowed latch
// ---------------------------------------------------------------------------
//
// `OVERLAY_LIVE` answers "has ANYTHING been defined at runtime", which is a
// far stronger fact than any single dispatch decision needs. The flags below
// split it by WHAT changed, so one `Struct.new` -- which only mints a class id
// nothing else can reach -- stops deoptimizing the whole process. `is_live()`
// keeps its exact old meaning, and every setter still calls `mark_live`, so a
// site is narrowed only by being rewritten to ask one of these instead.
//
// **INV-1 (downward closure).** If a runtime operation can change what `send`
// on an instance of `C` resolves a name to, then `C` is in `PATCHED`.
// Resolution reads `C`'s entry and each `A` in `ancestors(C)`, so touching `A`
// must mark every `C` below it -- that is what `patch_class` computes --
// PROVIDED `ancestors(C)` is itself fixed, which is exactly what
// `GATE_ANCESTRY_MUTATED` guards.
//
// **INV-2 (identity resolution).** Per-object singletons are keyed by heap
// address; no class-id set can express them, so `GATE_ANY_SINGLETONS` is a global
// boolean deliberately.
//
// **INV-3 (fresh ids).** A runtime-minted id (>= `RUNTIME_CLASS_ID_BASE`) has
// no frozen registry entry at all, so it needs no flag to be found -- it reads
// as patched by construction, and nothing else is affected by its existence.

/// Frozen class ids whose method resolution may now differ from the registry,
/// downward-closed over ancestry. Behind [`GATE_PATCHED_ANY`] so the common
/// answer costs no lock -- and no load of its own, for a caller that already
/// holds the gate byte.
static PATCHED: OnceLock<RwLock<FSet<u32>>> = OnceLock::new();

#[inline(always)]
pub fn class_maybe_patched(id: ClassId) -> bool {
    class_maybe_patched_gated(GATES.load(Ordering::Acquire), id)
}

#[inline(always)]
fn class_maybe_patched_gated(gates: u8, id: ClassId) -> bool {
    if id.0 >= RUNTIME_CLASS_ID_BASE {
        return true;
    }
    gates & GATE_PATCHED_ANY != 0
        && PATCHED
            .get()
            .is_some_and(|p| p.read().unwrap().contains(&id.0))
}

/// Mark `id` and everything that inherits from it. O(#classes), and only ever
/// reached from a runtime definition -- never from a loop.
fn patch_class(id: ClassId) {
    let set = PATCHED.get_or_init(|| RwLock::new(FSet::default()));
    {
        let mut w = set.write().unwrap();
        w.insert(id.0);
        w.extend(crate::dispatch::classes_with_ancestor(id));
    }
    GATES.fetch_or(GATE_PATCHED_ANY, Ordering::Release);
}

fn mark_singletons() {
    GATES.fetch_or(GATE_ANY_SINGLETONS, Ordering::Release);
}

fn mark_ancestry_mutated() {
    GATES.fetch_or(GATE_ANCESTRY_MUTATED, Ordering::Release);
}

fn mark_live() {
    GATES.fetch_or(GATE_OVERLAY, Ordering::Release);
}

/// The object identity a per-object singleton table is keyed by: the `Arc`'s
/// data address (drops the vtable half of the fat pointer).
fn obj_identity(o: &RObj) -> usize {
    Arc::as_ptr(o).cast::<()>() as usize
}

/// The singleton-table key for any value that can CARRY singleton methods --
/// its heap identity. `None` for an immediate (Integer/Symbol/nil/true/false),
/// which Ruby refuses a singleton on.
///
/// Not just `RubyValue::Object`: Ruby lets a singleton method be defined on any
/// heap object, and `def SOME_ARRAY.[](i)` is a real idiom (csv's
/// `NO_QUOTED_FIELDS`). Every arm here is an `Arc`, so the pointer is a stable
/// per-object identity for as long as the value lives.
pub(crate) fn value_identity(v: &RubyValue) -> Option<usize> {
    let addr = match v {
        RubyValue::Object(o) => return Some(obj_identity(o)),
        RubyValue::Array(a) => Arc::as_ptr(a).cast::<()>(),
        RubyValue::Str(s) => Arc::as_ptr(s).cast::<()>(),
        RubyValue::Hash(h) => Arc::as_ptr(h).cast::<()>(),
        RubyValue::Regexp(r) => Arc::as_ptr(r).cast::<()>(),
        // `nil`/`true`/`false` have exactly ONE instance each, so a fixed key
        // per value IS a per-object identity -- and CRuby accepts a singleton
        // on them for that reason. 0/1/2 can never collide with a real `Arc`
        // pointer, which is non-null and word-aligned.
        RubyValue::Nil => return Some(0),
        RubyValue::Bool(true) => return Some(1),
        RubyValue::Bool(false) => return Some(2),
        _ => return None,
    };
    Some(addr as usize)
}

/// [`value_identity`], plus a strong reference held for as long as the process
/// runs. Every site that DEFINES a singleton goes through this; the lookup
/// sites do not, since a live receiver is already keeping its own address.
///
/// `nil`/`true`/`false` are skipped: their keys are fixed constants, not
/// addresses, so nothing can collide with them.
pub(crate) fn pin_identity(v: &RubyValue) -> Option<usize> {
    let key = value_identity(v)?;
    if !matches!(v, RubyValue::Nil | RubyValue::Bool(_)) {
        maps()
            .pinned
            .write()
            .unwrap()
            .entry(key)
            .or_insert_with(|| v.clone());
    }
    Some(key)
}

/// A Class's key in the identity-keyed maps. A class has no `Arc` address to
/// stand for it, so its id is lifted past the address space -- `1 << 48` is
/// above every user-space pointer on the platforms this runs on, so a class
/// and an object can never collide.
fn class_identity(cid: ClassId) -> usize {
    cid.0 as usize | (1usize << 48)
}

/// The key a receiver's extended-module list is filed under. `extend` accepts
/// more receiver kinds than [`value_identity`] answers for, so a Class takes
/// [`class_identity`] instead.
fn extend_key(recv: &RubyValue) -> Option<usize> {
    match recv {
        RubyValue::Class(cid) => Some(class_identity(*cid)),
        other => value_identity(other),
    }
}

/// The identity `singleton_classes` caches a minted singleton class under, or
/// `None` for a receiver that gets a fresh one each time (see
/// [`runtime_singleton_class`]).
fn singleton_class_key(recv: &RubyValue) -> Option<usize> {
    match recv {
        RubyValue::Object(o) => Some(obj_identity(o)),
        RubyValue::Class(cid) => Some(class_identity(*cid)),
        _ => None,
    }
}

/// File `module_id` as mixed into `recv`'s singleton.
fn record_extended(recv: &RubyValue, module_id: ClassId) {
    let Some(key) = extend_key(recv) else { return };
    {
        let mut w = maps().extended.write().unwrap();
        let list = w.entry(key).or_default();
        // Extending twice is a no-op in CRuby -- the module keeps the rank its
        // FIRST `extend` gave it -- so an already-present id is left alone.
        if !list.contains(&module_id) {
            list.push(module_id);
        }
    }
    GATES.fetch_or(GATE_ANY_EXTENDED, Ordering::Release);
}

/// The modules `extend` mixed into `recv`, in the order they were mixed in.
/// A CLASS carries two lists: the one its own body wrote, baked into the
/// registry at compile time, and any `SomeClass.extend(M)` a later runtime
/// call added.
pub(crate) fn extended_modules(recv: &RubyValue) -> Vec<ClassId> {
    let mut mods: Vec<ClassId> = match recv {
        RubyValue::Class(cid) => crate::dispatch::class_extends(*cid).to_vec(),
        _ => Vec::new(),
    };
    if GATES.load(Ordering::Acquire) & GATE_ANY_EXTENDED == 0 {
        return mods;
    }
    let Some(key) = extend_key(recv) else {
        return mods;
    };
    if let Some(dynamic) = maps().extended.read().unwrap().get(&key) {
        for &m in dynamic {
            if !mods.contains(&m) {
                mods.push(m);
            }
        }
    }
    mods
}

/// Was `recv` `extend`ed with `target`, or with a module that itself includes
/// it? This is the half of `is_a?` that no class id can answer, since `extend`
/// changes ONE object's ancestry and leaves its class alone.
///
/// The [`GATE_ANY_EXTENDED`] gate is load-bearing, not an optimization: codegen
/// folds a statically-false `x.is_a?(SomeModule)` down to this call, so a
/// program that never extends anything answers from a single relaxed load.
pub fn value_extends(recv: &RubyValue, target: ClassId) -> bool {
    let reaches =
        |m: ClassId| m == target || crate::dispatch::ancestors_of_value(m).contains(&target);
    // A class body's own `extend M` is compile-time known and lives in the
    // registry, so it answers without the gate -- the gate only covers the
    // runtime map below.
    if let RubyValue::Class(cid) = recv
        && crate::dispatch::class_extends(*cid)
            .iter()
            .copied()
            .any(reaches)
    {
        return true;
    }
    if GATES.load(Ordering::Acquire) & GATE_ANY_EXTENDED == 0 {
        return false;
    }
    let Some(key) = extend_key(recv) else {
        return false;
    };
    let r = maps().extended.read().unwrap();
    let Some(list) = r.get(&key) else {
        return false;
    };
    list.iter().copied().any(reaches)
}

/// The real parent class of `cid` -- the first non-module entry after it in
/// the linearization, which is what `Class#superclass` answers.
fn superclass_of(cid: ClassId) -> Option<ClassId> {
    crate::dispatch::ancestors_of_value(cid)
        .iter()
        .skip_while(|&&a| a != cid)
        .skip(1)
        .copied()
        .find(|&a| !crate::dispatch::class_is_module(a).unwrap_or(false))
}

/// The chain BELOW a receiver's singleton class: each extended module (newest
/// first, carrying its own ancestors) ahead of the receiver class's own chain.
/// That is CRuby's `obj.singleton_class.ancestors` without the singleton head.
///
/// A CLASS receiver takes one more step first. Ruby runs a whole parallel
/// hierarchy of singleton classes (`class.c`'s `make_metaclass`):
/// `#<Class:Sub>`'s superclass is `#<Class:Base>`, not `Class`, and it is that
/// chain -- minted the rest of the way here, as `ENSURE_EIGENCLASS` does --
/// which makes a class method inherit. It ends at `BasicObject`, whose
/// singleton's superclass is `Class` itself, where the ordinary chain resumes.
fn singleton_super_chain(recv: &RubyValue) -> Vec<ClassId> {
    let mut chain: Vec<ClassId> = Vec::new();
    fn push(id: ClassId, chain: &mut Vec<ClassId>) {
        if !chain.contains(&id) {
            chain.push(id);
        }
    }
    // Reversed: the LAST `extend` sits closest to the singleton, so it wins a
    // name collision -- the same order `runtime_extend` gives the method table.
    for m in extended_modules(recv).into_iter().rev() {
        for &a in crate::dispatch::ancestors_of_value(m) {
            push(a, &mut chain);
        }
    }
    // A MODULE has no superclass, so it has no parallel chain either: its
    // singleton sits straight on `Module`.
    if let RubyValue::Class(cid) = recv
        && !crate::dispatch::class_is_module(*cid).unwrap_or(false)
    {
        let mut parent = superclass_of(*cid);
        while let Some(p) = parent {
            push(singleton_class_id_of(p), &mut chain);
            parent = superclass_of(p);
        }
    }
    for &a in crate::dispatch::ancestors_of_value(recv.class_id()) {
        push(a, &mut chain);
    }
    chain
}

/// `cid`'s singleton class, minting it if this is the first ask. Holds NO
/// lock, so the recursion up the superclass chain cannot deadlock.
fn singleton_class_id_of(cid: ClassId) -> ClassId {
    match runtime_singleton_class(&RubyValue::Class(cid)) {
        Ok(RubyValue::Class(sid)) => sid,
        _ => unreachable!("a Class always has a singleton class"),
    }
}

/// Rebuild an ALREADY-MINTED singleton class's ancestry after an `extend`.
/// Without this, `o.singleton_class` before the extend and after it would
/// answer the same cached id with two different chains.
fn refresh_singleton_ancestors(recv: &RubyValue) {
    let Some(key) = singleton_class_key(recv) else {
        return;
    };
    let Some(sid) = maps().singleton_classes.read().unwrap().get(&key).copied() else {
        return;
    };
    let mut anc = vec![sid];
    anc.extend(singleton_super_chain(recv));
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());
    if let Some(entry) = maps().classes.write().unwrap().get_mut(&sid.0) {
        entry.ancestors = leaked;
    }
}

/// Every method installed directly on `recv` -- `Object#singleton_methods` for
/// a non-Class receiver. Both tables, since an ordinary object's singletons and
/// a bare value's live in different maps (see `value_singletons`).
pub fn singleton_method_names(recv: &RubyValue) -> Vec<Symbol> {
    let Some(key) = value_identity(recv) else {
        return Vec::new();
    };
    let mut names: Vec<Symbol> = maps()
        .singletons
        .read()
        .unwrap()
        .get(&key)
        .map(|t| t.keys().copied().collect())
        .unwrap_or_default();
    if let Some(t) = maps().value_singletons.read().unwrap().get(&key) {
        names.extend(t.keys().copied());
    }
    names
}

/// The singleton method `name` installed directly on `recv`, or `None`. Always
/// behind `is_live()`; the identity lookup is what a class-id walk cannot do.
pub fn value_singleton_method(recv: &RubyValue, name: Symbol) -> Option<RProc> {
    let key = value_identity(recv)?;
    let s = maps().value_singletons.read().unwrap();
    s.get(&key).and_then(|t| t.get(&name)).cloned()
}

/// `Object#clone`'s singleton-class carry: every method installed directly
/// on `from`, and every module `extend`ed onto it, is re-keyed under `to`'s
/// identity (`dup` deliberately never calls this -- it drops the singleton
/// class, Ruby's rule). The copy is pinned exactly as the original was, so
/// its identity key stays sound for its lifetime.
pub fn copy_value_singletons(from: &RubyValue, to: &RubyValue) {
    if !is_live() {
        return;
    }
    let (Some(fk), Some(tk)) = (value_identity(from), value_identity(to)) else {
        return;
    };
    let mut copied = false;
    // Each read guard must be DEAD before the matching write lock: a let-chain
    // scrutinee's guard temporary lives through the body, and a same-thread
    // read->write on these RwLocks deadlocks.
    let singles = maps().singletons.read().unwrap().get(&fk).cloned();
    if let Some(t) = singles
        && !t.is_empty()
    {
        maps().singletons.write().unwrap().insert(tk, t);
        copied = true;
    }
    let value_singles = maps().value_singletons.read().unwrap().get(&fk).cloned();
    if let Some(t) = value_singles
        && !t.is_empty()
    {
        maps().value_singletons.write().unwrap().insert(tk, t);
        copied = true;
    }
    // The per-name provenance travels with the copied tables, so the
    // clone's `method(:x).owner` answers as the original's did.
    let owners = maps().extended_names.read().unwrap().get(&fk).cloned();
    if let Some(t) = owners
        && !t.is_empty()
    {
        maps().extended_names.write().unwrap().insert(tk, t);
    }
    // `extended` keys through `extend_key`, which for these (non-Class)
    // receivers IS `value_identity` -- a Class never comes through `clone`'s
    // object arm.
    let mods = maps().extended.read().unwrap().get(&fk).cloned();
    if let Some(mods) = mods
        && !mods.is_empty()
    {
        maps().extended.write().unwrap().insert(tk, mods);
        copied = true;
    }
    if copied {
        maps().pinned.write().unwrap().insert(tk, to.clone());
    }
}

// ---------------------------------------------------------------------------
// The Proc -> Dynamic bridge
// ---------------------------------------------------------------------------

/// Wrap a compiled block as an instance method: the method's receiver becomes
/// the block's `self` (`instance_exec`-style rebinding, which `RProc` supports
/// by taking self as a parameter), and the block the METHOD is called with is
/// forwarded into the body, so `yield`/`&blk` inside a `define_method` body
/// see the method's caller's block -- CRuby's `invoke_bmethod` specval, not
/// the closure env (see `ProcData::f`).
///
/// `defining` is the class this method is installed on and `name` its name; the
/// wrapper pushes them as the current method frame for the duration of the call
/// so a `super` in the body (which has no compile-time defining class -- the
/// class was minted at runtime) can resume the receiver's MRO walk after
/// `defining`. See [`send_super_dynamic`].
pub fn dynamic_from_proc(defining: ClassId, name: Symbol, body: RProc) -> MethodImpl {
    MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], block| {
        let self_val = RubyValue::Object(recv.clone());
        push_method_frame(defining, name);
        // Control flow here is `Result<_, Signal>`, never an unwinding panic, so
        // this pop runs on every exit (value OR signal) without a guard type.
        let out = body.call_with_self_and_block(&self_val, args, block);
        pop_method_frame();
        out
    }))
}

// ---------------------------------------------------------------------------
// Runtime method frames -- what a `super` in a runtime-defined method resolves
// against
// ---------------------------------------------------------------------------

thread_local! {
    /// The stack of runtime-defined methods currently executing on this thread,
    /// each `(defining class, method name)`. `dynamic_from_proc`'s wrapper
    /// pushes on entry and pops on exit, so the top frame is always the
    /// innermost runtime method -- exactly what a bare `super` there needs.
    static METHOD_FRAMES: RefCell<Vec<(ClassId, Symbol)>> = const { RefCell::new(Vec::new()) };

    /// The stack of runtime class BODIES currently executing -- a
    /// `Class.new`/`Module.new` block or a `class_eval`. Carries the running
    /// default visibility and `module_function` mode a bare directive sets,
    /// as CRuby's cref does, and dies with the body like a cref too.
    static BODY_FRAMES: RefCell<Vec<BodyFrame>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy)]
struct BodyFrame {
    class: ClassId,
    vis: crate::dispatch::MethodVisibility,
    module_function: bool,
}

/// Runs `f` with a fresh body frame for `id`, so bare directives inside it
/// have somewhere to record themselves.
pub(crate) fn with_body_frame<T>(
    id: ClassId,
    f: impl FnOnce() -> Result<T, Signal>,
) -> Result<T, Signal> {
    BODY_FRAMES.with(|s| {
        s.borrow_mut().push(BodyFrame {
            class: id,
            vis: crate::dispatch::MethodVisibility::Public,
            module_function: false,
        })
    });
    // A class body's definee is the class, even inside an enclosing
    // `instance_eval` -- so suspend those frames for this body.
    let outer = SINGLETON_DEFINEE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let out = f();
    SINGLETON_DEFINEE.with(|s| *s.borrow_mut() = outer);
    BODY_FRAMES.with(|s| {
        s.borrow_mut().pop();
    });
    out
}

// The receivers of the `instance_eval`/`instance_exec` frames open on this
// thread. Inside one, a `def` whose self IS that receiver installs on the
// singleton class -- Ruby's rule, and what `SingleForwardable` relies on.
// The receiver is recorded rather than a bare depth so that a `def` reached
// through some OTHER object's method, from inside the block, still follows
// the ordinary rule.
std::thread_local!(static SINGLETON_DEFINEE: std::cell::RefCell<Vec<RubyValue>> =
    const { std::cell::RefCell::new(Vec::new()) });

/// Run `f` with `recv`'s singleton class as the default definee --
/// `instance_eval`/`instance_exec`'s rule.
pub(crate) fn with_singleton_definee<T>(
    recv: &RubyValue,
    f: impl FnOnce() -> Result<T, Signal>,
) -> Result<T, Signal> {
    SINGLETON_DEFINEE.with(|s| s.borrow_mut().push(recv.clone()));
    let out = f();
    SINGLETON_DEFINEE.with(|s| {
        s.borrow_mut().pop();
    });
    out
}

/// The class an enclosing `class_eval`/`class_exec`/`Class.new`/`Module.new`
/// block made the default definee -- [`singleton_definee`]'s module twin, and
/// what makes a `def` in such a block land on the new class rather than on
/// the cref where the block was written.
///
/// `slf` has to still BE that class: a `def` reached through some other
/// object's method, from inside the block, follows the ordinary rule.
pub(crate) fn module_definee(slf: &RubyValue) -> Option<ClassId> {
    let RubyValue::Class(cid) = slf else {
        return None;
    };
    BODY_FRAMES.with(|s| s.borrow().last().map(|f| f.class).filter(|c| c == cid))
}

/// Whether a `def` running with `recv` as its self installs on the singleton
/// class -- true exactly inside an `instance_eval`/`instance_exec` of `recv`.
pub(crate) fn singleton_definee(recv: &RubyValue) -> bool {
    SINGLETON_DEFINEE.with(|s| {
        s.borrow().last().is_some_and(|open| match (open, recv) {
            (RubyValue::Class(a), RubyValue::Class(b)) => a == b,
            _ => crate::builtins::basic_object::value_identity(open, recv),
        })
    })
}

fn current_frame_for(id: ClassId) -> Option<BodyFrame> {
    BODY_FRAMES.with(|s| s.borrow().iter().rev().find(|f| f.class == id).copied())
}

fn update_frame_for(id: ClassId, f: impl FnOnce(&mut BodyFrame)) {
    BODY_FRAMES.with(|s| {
        if let Some(frame) = s.borrow_mut().iter_mut().rev().find(|fr| fr.class == id) {
            f(frame);
        }
    });
}

/// A sentinel "defining class" for a per-object singleton method: it is never a
/// real class id (`u32::MAX` is above every runtime id), so `send_super_from`'s
/// ancestor lookup misses it and resumes from the TOP of the receiver's own
/// ancestry -- which is exactly where a singleton method's `super` belongs (the
/// conceptual singleton class sits ahead of the object's real class).
const SINGLETON_DEFINING: ClassId = ClassId(u32::MAX);

fn push_method_frame(defining: ClassId, name: Symbol) {
    METHOD_FRAMES.with(|f| f.borrow_mut().push((defining, name)));
}

fn pop_method_frame() {
    METHOD_FRAMES.with(|f| {
        f.borrow_mut().pop();
    });
}

/// `super` from inside a RUNTIME-defined method (a `def` in a `Class.new` /
/// `Struct.new` / `Data.define` body, or a `define_method`) whose defining class
/// is not known at compile time. Reads this thread's current method frame --
/// pushed by [`dynamic_from_proc`] on entry -- and resumes the receiver's MRO
/// walk after that class, exactly like the compile-time `send_super_from`.
/// Raises `RuntimeError` when there is no active runtime frame (a `super`
/// written outside any method), matching CRuby's runtime error rather than a
/// compile-time rejection.
pub fn send_super_dynamic(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match METHOD_FRAMES.with(|f| f.borrow().last().copied()) {
        Some((defining, name)) => send_super_from(recv, defining, name, args, block),
        None => Err(runtime_error!("super called outside of method")),
    }
}

// ---------------------------------------------------------------------------
// Public runtime API (what F2/F3/F4's builtins + codegen call)
// ---------------------------------------------------------------------------

/// `some_class.define_method(name) { body }` -- install/override an instance
/// method on the class with id `id` (frozen or runtime). Returns the name.
/// A `Foo.freeze`d class refuses (`can't modify frozen Class: Foo`,
/// CRuby's guard on every method-table mutation).
pub fn runtime_define_method(id: ClassId, name: Symbol, body: RProc) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // A method defined on an object's singleton class (`obj.singleton_class`)
    // is a per-object singleton, not an instance method of a shared class --
    // redirect to the owner. For a class owner it becomes a class method,
    // carrying the body's visibility cursor: a live `class_eval` frame, or
    // the persistent default a bare `private` in a compiled singleton body
    // stored (see `runtime_set_visibility`).
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner {
        let vis = current_frame_for(id).map(|f| f.vis).or_else(|| {
            maps()
                .classes
                .read()
                .unwrap()
                .get(&id.0)
                .and_then(|e| e.singleton_default_vis)
        });
        let out = runtime_define_singleton_method(&owner, name, body)?;
        if let (Some(v), RubyValue::Class(cid)) = (vis, &owner) {
            runtime_class_method_visibility(
                *cid,
                &[RubyValue::Symbol(name)],
                v != crate::dispatch::MethodVisibility::Public,
            )?;
        }
        return Ok(out);
    }
    crate::method_meta::record_runtime_params(id, crate::MethodKind::Instance, name, &body);
    let m = dynamic_from_proc(id, name, body.clone());
    let frame = current_frame_for(id);
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(name, m);
        e.value_bodies.insert(name, body);
        e.undefs.remove(&name);
        // `initialize` and its copy/clone/dup family are private wherever
        // they are defined -- CRuby stamps them so in `rb_method_entry_make`
        // regardless of the visibility cursor.
        let always_private = matches!(
            name.name_str(),
            "initialize" | "initialize_copy" | "initialize_clone" | "initialize_dup"
        );
        match frame {
            _ if always_private => {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Private);
            }
            // Outside a class body a runtime definition is public -- drop any
            // earlier `private :name` mark.
            None => {
                e.methods_vis.remove(&name);
            }
            // `module_function` makes the instance copy private and adds a
            // public module method; a bare `private`/`protected` just marks.
            Some(f) if f.module_function => {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Private);
            }
            Some(f) if f.vis != crate::dispatch::MethodVisibility::Public => {
                e.methods_vis.insert(name, f.vis);
            }
            Some(_) => {
                e.methods_vis.remove(&name);
            }
        }
    }
    // The module-method half is built with the overlay lock DROPPED:
    // `extended_class_method` reads the overlay itself.
    if frame.is_some_and(|f| f.module_function)
        && let Some(wrapper) = extended_class_method(id, name)
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.class_methods.insert(name, wrapper);
        e.extended_class_methods.remove(&name);
    }
    patch_class(id);
    mark_live();
    // A hook name defined on Module/Class/BasicObject itself applies to every
    // class; nothing downstream could infer that from the owner id.
    if global_def_hook_owner(id, name) {
        mark_global_def_hook(name.name_str());
    }
    fire_def_hook(DefTarget::Class(id), DefEvent::Added, name)?;
    // `module_function` adds a module method too, and ruby reports BOTH: the
    // instance copy through `method_added`, then the module copy through
    // `singleton_method_added`.
    if frame.is_some_and(|f| f.module_function) {
        fire_def_hook(
            DefTarget::Singleton(&RubyValue::Class(id)),
            DefEvent::Added,
            name,
        )?;
    }
    Ok(RubyValue::Symbol(name))
}

/// Seeds the singleton mint with a COMPILE-registered singleton class: the
/// surrogate a constant-bearing `class << self` body registered for `owner`
/// (its constants live on the surrogate's id in the frozen tables). After
/// this, `owner.singleton_class` answers the surrogate, and a runtime
/// `def owner.x` through it redirects to `owner` exactly as a minted
/// singleton would (`singleton_owner`). Called from generated `main()`
/// before the first statement runs -- no gates need flipping, because
/// nothing here adds an overlay method table.
pub fn register_singleton_surrogate(owner: ClassId, surrogate: ClassId) {
    let owner_val = RubyValue::Class(owner);
    let Some(key) = singleton_class_key(&owner_val) else {
        return;
    };
    maps()
        .singleton_classes
        .write()
        .unwrap()
        .insert(key, surrogate);
    maps()
        .singleton_owner
        .write()
        .unwrap()
        .insert(surrogate.0, owner_val);
}

/// Installs a COMPILED trampoline as the current body of `id`'s `name`.
///
/// This is the runtime half of a positional method redefinition: a reopen's
/// `def` over an existing method, or a `def x; def x` pair that a
/// `method_added` hook observes. The static tables keep the final body.
/// Codegen calls this at boot with the FIRST body, and again at each
/// redefinition's document position, so dynamic dispatch tracks Ruby's
/// install-where-it-stands timeline.
///
/// No hook fires here. The spliced `DefHook` at the same position is the
/// report, exactly as for a statically-registered `def`.
pub fn runtime_replace_method(id: ClassId, name: Symbol, f: crate::dispatch::MethodFn) {
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods
            .insert(name, crate::dispatch::MethodImpl::Static(f));
        e.undefs.remove(&name);
    }
    patch_class(id);
    mark_live();
}

#[derive(Clone, Copy, PartialEq)]
pub enum AttrKind {
    Reader,
    Writer,
    Accessor,
}

/// `attr_reader`/`attr_writer`/`attr_accessor` reached AT RUNTIME
/// (`Class.new { attr_reader :v }`, `Foo.class_eval { attr_accessor :y }`).
/// The literal class-body form expands to real `def`s at compile time.
///
/// Each accessor closes over the RECEIVER's name-keyed ivar storage, which is
/// total across every receiver kind -- a `DynObject`, a generated struct's
/// typed field or `__overflow` map, a `ValueSubclass` -- so one implementation
/// serves a runtime class and a `class_eval` over a compiled one alike.
pub fn runtime_attr(id: ClassId, args: &[RubyValue], kind: AttrKind) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // An attr defined on a SINGLETON class is a singleton attr on its owner,
    // reading that owner's own ivars -- not an instance method of a shared
    // class. `runtime_define_method` redirects the same way, and for the same
    // reason. minitest's `cattr_accessor` is written exactly this way:
    // `(class << self; self; end).attr_accessor name`.
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner {
        return singleton_attr(&owner, args, kind);
    }
    let mut defined = Vec::new();
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if kind != AttrKind::Writer {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let getter =
                MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], _| {
                    if !args.is_empty() {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 0)",
                            args.len()
                        ));
                    }
                    Ok(recv.ivar_get_named(&key).unwrap_or(RubyValue::Nil))
                }));
            install_attr(id, name, getter);
            defined.push(RubyValue::Symbol(name));
        }
        if kind != AttrKind::Reader {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let setter_name = Symbol::intern(&format!("{}=", name.name()));
            let setter =
                MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], _| {
                    let [v] = args else {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 1)",
                            args.len()
                        ));
                    };
                    recv.ivar_set_named(&key, v.clone());
                    Ok(v.clone())
                }));
            install_attr(id, setter_name, setter);
            defined.push(RubyValue::Symbol(setter_name));
        }
    }
    patch_class(id);
    mark_live();
    // One hook per generated name, reader before writer -- ruby reports
    // `attr_accessor :c` as `method_added(:c)` then `method_added(:c=)`.
    for sym in &defined {
        let RubyValue::Symbol(sym) = sym else {
            continue;
        };
        fire_def_hook(DefTarget::Class(id), DefEvent::Added, *sym)?;
    }
    Ok(RubyValue::Array(crate::array_new(defined)))
}

/// [`runtime_attr`] for a singleton class: each accessor becomes a SINGLETON
/// method on the owner, over the owner's own ivars (`ivar_get_dyn` reaches a
/// class's ivar table and an object's alike). The bodies are `RProc`s rather
/// than the `MethodImpl::Dynamic` the instance-method path builds, because a
/// class-method receiver is a `RubyValue::Class` and has no `RObj` to bind.
fn singleton_attr(
    owner: &RubyValue,
    args: &[RubyValue],
    kind: AttrKind,
) -> Result<RubyValue, Signal> {
    let mut defined = Vec::new();
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if kind != AttrKind::Writer {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let getter = RProc::with_self(
                move |slf: &RubyValue, args: &[RubyValue]| {
                    if !args.is_empty() {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 0)",
                            args.len()
                        ));
                    }
                    Ok(crate::dispatch::ivar_get_dyn(slf, &key))
                },
                owner.clone(),
                0,
                true,
            );
            runtime_define_singleton_method(owner, name, getter)?;
            defined.push(RubyValue::Symbol(name));
        }
        if kind != AttrKind::Reader {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let setter_name = Symbol::intern(&format!("{}=", name.name()));
            let setter = RProc::with_self(
                move |slf: &RubyValue, args: &[RubyValue]| {
                    let [v] = args else {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 1)",
                            args.len()
                        ));
                    };
                    crate::dispatch::ivar_set_dyn(slf, &key, v.clone())
                },
                owner.clone(),
                1,
                true,
            );
            runtime_define_singleton_method(owner, setter_name, setter)?;
            defined.push(RubyValue::Symbol(setter_name));
        }
    }
    mark_live();
    Ok(RubyValue::Array(crate::array_new(defined)))
}

fn install_attr(id: ClassId, name: Symbol, m: MethodImpl) {
    let mut w = maps().classes.write().unwrap();
    let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
    e.methods.insert(name, m);
    e.methods_vis.remove(&name);
    e.undefs.remove(&name);
}

/// `Module#undef_method` -- CRuby's `rb_undef`. The name must currently
/// RESOLVE for instances of `id`; the entry then terminates the MRO walk here,
/// so an ancestor's definition can no longer answer for `id`.
pub fn runtime_undef_method(id: ClassId, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // `class << self; undef x; end` reaches here with the SINGLETON's id. Its
    // instance methods are the owner's class methods, so the retirement belongs
    // in the owner's class-method space -- see `OverlayEntry::class_undefs`.
    if let Some(owner) = singleton_class_owner(id) {
        return runtime_undef_class_method(owner, id, args);
    }
    // The same redirect for an ORDINARY object's singleton class, which
    // `singleton_class_owner` cannot answer for (it names a class). Its
    // instance methods are that one object's singleton methods, so the
    // retirement is that one object's too -- and it has to be recorded, not
    // just applied: the class still defines the name, and the tombstone is the
    // only thing that says this object no longer answers it.
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner
        && !matches!(owner, RubyValue::Class(_))
    {
        return runtime_undef_singleton_method(&owner, id, args);
    }
    let mut undefined = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if !crate::dispatch::responds_to(id, name, true) {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(id).unwrap_or_else(|| "?".to_string())
            ));
        }
        {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
            e.undefs.insert(name);
            e.methods.remove(&name);
        }
        undefined.push(name);
    }
    patch_class(id);
    mark_live();
    for name in undefined {
        fire_def_hook(DefTarget::Class(id), DefEvent::Undefined, name)?;
    }
    Ok(RubyValue::Class(id))
}

/// [`runtime_undef_method`] for ONE OBJECT, reached through that object's
/// singleton class (`g.singleton_class.undef_method(:close)`, and the
/// `class << g; undef :close; end` that spells the same thing).
///
/// Writes the tombstone `singleton_undefs` holds and takes any singleton method
/// of that name with it. `singleton` is the id the call came in on, and it
/// decides what EXISTS: undefining a name the object cannot answer at all is
/// ruby's NameError, and it names the singleton class the caller reached
/// through.
fn runtime_undef_singleton_method(
    owner: &RubyValue,
    singleton: ClassId,
    args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let Some(key) = pin_identity(owner) else {
        return Err(type_error!("can't define singleton"));
    };
    let mut undefined = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if !crate::dispatch::responds_to_value(owner, name, true) {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(singleton).unwrap_or_else(|| "?".to_string())
            ));
        }
        {
            let mut w = maps().singletons.write().unwrap();
            if let Some(t) = w.get_mut(&key) {
                t.remove(&name);
            }
        }
        {
            let mut w = maps().value_singletons.write().unwrap();
            if let Some(t) = w.get_mut(&key) {
                t.remove(&name);
            }
        }
        clear_extended_name(key, name);
        maps()
            .singleton_undefs
            .write()
            .unwrap()
            .entry(key)
            .or_default()
            .insert(name);
        undefined.push(name);
    }
    mark_singletons();
    mark_live();
    for name in undefined {
        fire_def_hook(DefTarget::Singleton(owner), DefEvent::Undefined, name)?;
    }
    Ok(RubyValue::Class(singleton))
}

/// [`runtime_alias_method`] for ONE OBJECT, reached through that object's
/// singleton class (`class << obj; alias shut close; end`).
///
/// `old` is resolved the way the OBJECT answers it -- its own singleton table
/// first, then its class's chain -- because that is what ruby copies: an alias
/// takes the definition the receiver would have run.
fn runtime_alias_singleton_method(
    owner: &RubyValue,
    singleton: ClassId,
    new: Symbol,
    old: Symbol,
) -> Result<RubyValue, Signal> {
    let Some(key) = pin_identity(owner) else {
        return Err(type_error!("can't define singleton"));
    };
    let own = maps()
        .singletons
        .read()
        .unwrap()
        .get(&key)
        .and_then(|t| t.get(&old).cloned());
    let Some(m) = own.or_else(|| snapshot_instance_method(owner.class_id(), old)) else {
        return Err(name_error!(
            "undefined method '{}' for class '{}'",
            old.name(),
            crate::dispatch::class_name(singleton).unwrap_or_default()
        ));
    };
    maps()
        .singletons
        .write()
        .unwrap()
        .entry(key)
        .or_default()
        .insert(new, m);
    // An alias DEFINES the new name, so it lifts any tombstone standing over it.
    if let Some(t) = maps().singleton_undefs.write().unwrap().get_mut(&key) {
        t.remove(&new);
    }
    mark_singletons();
    mark_live();
    Ok(RubyValue::Symbol(new))
}

/// Whether `recv` retired `name` for itself -- see `singleton_undefs`. Always
/// behind `is_live()`, like every other identity-keyed probe: an ordinary
/// program never takes the hash lookup.
pub fn object_method_undefined(recv: &RubyValue, name: Symbol) -> bool {
    let u = maps().singleton_undefs.read().unwrap();
    if u.is_empty() {
        return false;
    }
    value_identity(recv).is_some_and(|k| u.get(&k).is_some_and(|t| t.contains(&name)))
}

/// [`runtime_undef_method`] for a CLASS method, reached through the owner's
/// singleton class. Retires the name for `owner` and every subclass, exactly as
/// the instance-method form does.
///
/// `singleton` is the id the call came in on, and it decides what EXISTS here.
/// A singleton class inherits `Module`'s instance methods -- `#<Class:M>`'s
/// ancestors are `[#<Class:M>, Module, Object, Kernel, BasicObject]` -- and
/// ruby's `undef` retires an inherited method as readily as an own one
/// (`rb_undef` resolves the name with `rb_method_entry`, which walks the whole
/// chain). Checking only the owner's CLASS-method space missed that half: the
/// singleton gem's `undef_method :extend_object`, written inside an `extended`
/// hook, raised NameError for a method `private_method_defined?` reported on
/// the very same receiver.
fn runtime_undef_class_method(
    owner: ClassId,
    singleton: ClassId,
    args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let mut undefined = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if crate::dispatch::class_method_owner(owner, name).is_none()
            && overlay_class_method(owner, name).is_none()
            && crate::dispatch::instance_method_visibility(singleton, name).is_none()
        {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(owner).unwrap_or_else(|| "?".to_string())
            ));
        }
        {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(owner.0).or_insert_with(OverlayEntry::delta);
            e.class_undefs.insert(name);
            e.class_methods.remove(&name);
            // And a tombstone on the SINGLETON class itself, which is where
            // ruby puts it -- `rb_undef` writes the undef entry into the
            // singleton's own method table, so it shadows the rest of that
            // chain (`Module`, `Class`, `Object`, `Kernel`). `class_undefs`
            // above gates class-method DISPATCH, which is the owner's space;
            // this one is what every ancestor-walking reader already
            // consults, so `method_defined?`, `private_method_defined?`,
            // `instance_method` and `respond_to?` all agree with dispatch
            // instead of finding `Module`'s definition again behind it.
            let s = w.entry(singleton.0).or_insert_with(OverlayEntry::delta);
            s.undefs.insert(name);
            s.methods.remove(&name);
        }
        undefined.push(name);
    }
    patch_class(owner);
    patch_class(singleton);
    mark_live();
    for name in undefined {
        fire_def_hook(DefTarget::Class(owner), DefEvent::Undefined, name)?;
    }
    Ok(RubyValue::Class(owner))
}

/// Whether an `undef` inside `class << self` retired `name` as a CLASS method
/// of `id` -- the gate every class-method resolution owes
/// [`OverlayEntry::class_undefs`].
pub(crate) fn class_method_undefined(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.class_undefs.contains(&name))
}

/// `Module#remove_method` -- drops this class's OWN definition, leaving an
/// inherited one reachable. Unlike `undef_method` it plants no tombstone.
pub fn runtime_remove_method(id: ClassId, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    let mut removed_names = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        let removed = maps()
            .classes
            .write()
            .unwrap()
            .get_mut(&id.0)
            .and_then(|e| e.methods.remove(&name))
            .is_some();
        if !removed {
            return Err(name_error!(
                "method '{}' not defined in {}",
                name.name(),
                crate::dispatch::class_name(id).unwrap_or_else(|| "?".to_string())
            ));
        }
        removed_names.push(name);
    }
    patch_class(id);
    mark_live();
    for name in removed_names {
        fire_def_hook(DefTarget::Class(id), DefEvent::Removed, name)?;
    }
    Ok(RubyValue::Class(id))
}

/// `Module#alias_method(new, old)` reached AT RUNTIME (computed names --
/// e.g. ostruct's `instance_methods.each { |m| alias_method "#{m}!", m }`
/// bulk loop; the literal-symbol class-body form resolves at compile time).
/// The method `old` resolves to for instances of `id` is SNAPSHOTTED and
/// installed under `new` in the overlay -- CRuby's copy-the-method-entry
/// semantics (`rb_alias`), so a later runtime redefinition of `old` does
/// not change the alias. Returns the new name's Symbol. The alias inherits
/// `old`'s visibility (CRuby: the copied method entry keeps its flags).
pub fn runtime_alias_method(id: ClassId, new: Symbol, old: Symbol) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // A SINGLETON class's instance methods are its owner's CLASS methods, so an
    // alias written there copies one of those -- securerandom's `class << self;
    // begin; Random.urandom(1); alias gen_random gen_random_urandom; rescue`.
    // Resolved and installed on the owner's class-method side, since the
    // singleton id has no instance table of its own.
    if let Some(owner) = singleton_class_owner(id) {
        let existing = maps()
            .classes
            .read()
            .unwrap()
            .get(&owner.0)
            .and_then(|e| e.class_methods.get(&old).cloned());
        let source = existing
            .or_else(|| {
                crate::dispatch::class_method_fn(owner, old)
                    .map(|f| RProc::with_self_and_block(f, RubyValue::Nil, -1, true))
            })
            .or_else(|| extended_class_method(owner, old));
        let Some(source) = source else {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                old.name(),
                crate::dispatch::class_name(id).unwrap_or_default()
            ));
        };
        {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(owner.0).or_insert_with(OverlayEntry::delta);
            e.class_methods.insert(new, source);
            e.extended_class_methods.remove(&new);
        }
        patch_class(owner);
        mark_live();
        return Ok(RubyValue::Symbol(new));
    }
    // The same redirect for an ORDINARY object's singleton class, which
    // `singleton_class_owner` cannot answer for (it names a class). `class <<
    // obj; alias shut close; end` aliases a method for THAT ONE OBJECT, so the
    // copy belongs in its singleton table -- the walk below would instead write
    // it into an instance table the singleton id never had, where no send for
    // that object ever looks, and `obj.shut` raised NoMethodError.
    let value_owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = value_owner
        && !matches!(owner, RubyValue::Class(_))
    {
        return runtime_alias_singleton_method(&owner, id, new, old);
    }
    let snapshot = snapshot_instance_method(id, old).or_else(|| {
        // A parse-special Kernel source (`alias_method :block_given!,
        // :block_given?` reached at runtime): statically-resolved call sites
        // compile these directly, so there is no dispatch row to snapshot --
        // but the alias itself is valid, exactly as `validate_aliases`
        // accepts the static form. Accept it with a stub that only raises if
        // a call actually arrives dynamically (which zeo cannot serve: the
        // answer lives in the CALLER's compiled frame).
        crate::dispatch::PARSE_SPECIAL_KERNEL
            .contains(&old.name().as_str())
            .then(|| {
                MethodImpl::Dynamic(Arc::new(
                    move |_recv: &RObj, _args: &[RubyValue], _block: Option<RubyValue>| {
                        Err(crate::builtins::not_impl_error!(
                            "'{}' cannot be called through a runtime alias (zeo limitation: \
                             it resolves in the caller's compiled frame)",
                            old.name()
                        ))
                    },
                ))
            })
    });
    let Some(m) = snapshot else {
        let kind = if crate::dispatch::class_is_module(id).unwrap_or(false) {
            "module"
        } else {
            "class"
        };
        let cls = crate::dispatch::class_name(id).unwrap_or_default();
        return Err(name_error!(
            "undefined method '{}' for {kind} '{cls}'",
            old.name()
        ));
    };
    // The alias inherits its source's CURRENT visibility (CRuby: the copied
    // method entry keeps its flags) -- resolved before install so a stale
    // mark under `new` can't shadow it.
    let vis = crate::dispatch::instance_method_visibility(id, old);
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(new, m);
        match vis {
            Some(v) => e.methods_vis.insert(new, v),
            None => e.methods_vis.remove(&new),
        };
    }
    // Reflection follows the copy: the alias reports the source's birth name
    // (so a chain -- `alias b a; alias c b` -- still answers `:a`) and the
    // source's own signature and definition site.
    let origin = crate::method_meta::original_name(id, crate::MethodKind::Instance, old);
    let mut meta = crate::MethodMeta::instance(id.0, &new.name()).aliased_from(&origin.name());
    if let Some(source) = crate::method_meta::lookup(id, crate::MethodKind::Instance, old) {
        meta = meta.with_params(source.params().clone());
        if let Some((file, line)) = source.source() {
            meta = meta.defined_at(file, line);
        }
    }
    meta.register();
    patch_class(id);
    mark_live();
    // An alias is a definition: ruby reports the NEW name, once.
    fire_def_hook(DefTarget::Class(id), DefEvent::Added, new)?;
    Ok(RubyValue::Symbol(new))
}

/// `Module#private`/`public`/`protected` WITH NAME ARGUMENTS reached at
/// runtime (`Foo.class_eval { private :m }`, ostruct's guarded
/// `private :block_given!`): validate each name resolves as an instance
/// method of `id` (NameError otherwise, CRuby's timing) and record the mark
/// in the overlay, where `instance_method_visibility`'s walk finds it ahead
/// of the frozen registry's own flags. Returns the arguments as passed: a
/// lone name verbatim, several names as an Array (Ruby >= 3.1's shape).
///
/// The ARGUMENT-LESS form (set the default visibility for subsequent defs
/// in this scope) is accepted as a nil-returning no-op: the overlay has no
/// per-scope default, and compiled `def`s took their visibility at compile
/// time -- divergence limited to a bare `private` inside `class_eval`.
pub fn runtime_set_visibility(
    id: ClassId,
    args: &[RubyValue],
    vis: crate::dispatch::MethodVisibility,
) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        // Bare `private` switches the enclosing body's default for every
        // subsequent `def`.
        update_frame_for(id, |f| f.vis = vis);
        // A compiled `class << self` body has no frame to record into --
        // the rebound directive persists as the SURROGATE's body default
        // (read by `runtime_define_method`'s singleton redirect), and the
        // lowering's body-end reset clears it.
        if current_frame_for(id).is_none() && singleton_owner_value(id).is_some() {
            let mut w = maps().classes.write().unwrap();
            w.entry(id.0)
                .or_insert_with(OverlayEntry::delta)
                .singleton_default_vis = Some(vis);
            drop(w);
            mark_live();
        }
        return Ok(RubyValue::Nil);
    }
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // `private [:a, :b]` (one Array argument) marks the contents. The return
    // value is the ARGUMENT SHAPE as passed -- a lone name (Symbol or
    // String) comes back verbatim, several names come back as an Array --
    // per CRuby's `Module#private` docs.
    let names: Vec<RubyValue> = match args {
        [RubyValue::Array(a)] => a.lock().to_vec(),
        one_or_many => one_or_many.to_vec(),
    };
    let result = match args {
        [one] => one.clone(),
        many => RubyValue::Array(crate::array_new(many.to_vec())),
    };
    let mut syms = Vec::with_capacity(names.len());
    for name in &names {
        syms.push(coerce_method_name(Some(name))?);
    }
    // A SINGLETON class's instance methods are its owner's CLASS methods --
    // `class << self; public(*METHODS); end` (fileutils' Verbose/NoWrite/
    // DryRun) is `public_class_method(*METHODS)` on the owner, and that is the
    // table those methods and their visibility actually live in. Without this
    // the names resolve against an instance table the singleton id never had.
    if let Some(owner) = singleton_class_owner(id) {
        for &sym in &syms {
            // Either provenance counts: a compiled `def self.x` (the owner's
            // class-method table) or one the singleton itself gained at run
            // time -- fileutils reaches this line right after `extend self`.
            let resolves = crate::dispatch::class_method_owner(owner, sym).is_some()
                || crate::dispatch::instance_method_visibility(id, sym).is_some()
                || snapshot_instance_method(id, sym).is_some();
            if !resolves {
                return Err(name_error!(
                    "undefined method '{}' for class '{}'",
                    sym.name(),
                    crate::dispatch::class_name(id).unwrap_or_default()
                ));
            }
        }
        let marks: Vec<RubyValue> = syms.iter().map(|&s| RubyValue::Symbol(s)).collect();
        runtime_class_method_visibility(
            owner,
            &marks,
            vis == crate::dispatch::MethodVisibility::Private,
        )?;
        return Ok(result);
    }
    for &sym in &syms {
        let resolves = crate::dispatch::instance_method_visibility(id, sym).is_some()
            || snapshot_instance_method(id, sym).is_some();
        if !resolves {
            let kind = if crate::dispatch::class_is_module(id).unwrap_or(false) {
                "module"
            } else {
                "class"
            };
            let cls = crate::dispatch::class_name(id).unwrap_or_default();
            return Err(name_error!(
                "undefined method '{}' for {kind} '{cls}'",
                sym.name()
            ));
        }
    }
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        for &sym in &syms {
            e.methods_vis.insert(sym, vis);
        }
    }
    patch_class(id);
    mark_live();
    // `private :m` on an INHERITED method is a definition -- CRuby synthesizes
    // a `ZSUPER` entry through `rb_add_method`, which fires the hook
    // (`vm_method.c:2318-2336`). On the class's OWN method the visibility is
    // set in place and nothing fires. Oracle-verified both ways.
    for &sym in &syms {
        if crate::dispatch::method_owner(id, sym) != Some(id) {
            fire_def_hook(DefTarget::Class(id), DefEvent::Added, sym)?;
        }
    }
    Ok(result)
}

/// The explicit runtime visibility mark for `name` on class `id`'s OWN
/// overlay entry -- `instance_method_visibility` probes this per ancestor,
/// nearest mark winning, ahead of the frozen registry's flags.
pub(crate) fn overlay_method_visibility(
    id: ClassId,
    name: Symbol,
) -> Option<crate::dispatch::MethodVisibility> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.methods_vis.get(&name).copied())
}

/// The CLASS a singleton-class id belongs to (`Foo.singleton_class` -> `Foo`),
/// or `None` for an ordinary id or an object's singleton. What lets the
/// singleton class's instance-method reflection answer from the owner's
/// CLASS-method tables, which is where `def self.x` really lives.
pub fn singleton_class_owner(id: ClassId) -> Option<ClassId> {
    match singleton_owner_value(id) {
        Some(RubyValue::Class(cid)) => Some(cid),
        _ => None,
    }
}

/// [`singleton_class_owner`] without the class narrowing -- the VALUE a
/// singleton-class id was minted for, whatever its kind.
pub fn singleton_owner_value(id: ClassId) -> Option<RubyValue> {
    maps().singleton_owner.read().unwrap().get(&id.0).cloned()
}

/// The singleton class ALREADY minted for `recv`, if any -- without minting
/// one, which is what makes it safe to ask from a reflection row. `def obj.x`
/// alone mints nothing; only naming `obj.singleton_class` does.
pub fn minted_singleton_class(recv: &RubyValue) -> Option<ClassId> {
    let key = singleton_class_key(recv)?;
    maps().singleton_classes.read().unwrap().get(&key).copied()
}

/// Whether a runtime `private_class_method`/`public_class_method` marked class
/// method `name` on `id`, and which way. `None` when neither was called for it.
pub(crate) fn overlay_class_method_private(id: ClassId, name: Symbol) -> Option<bool> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.class_methods_vis.get(&name).copied())
}

/// `private_class_method :x` / `public_class_method :x` at runtime -- see
/// [`overlay_class_method_private`].
pub fn runtime_class_method_visibility(
    id: ClassId,
    args: &[RubyValue],
    private: bool,
) -> Result<(), Signal> {
    let mut w = maps().classes.write().unwrap();
    let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
    for a in args {
        e.class_methods_vis
            .insert(coerce_method_name(Some(a))?, private);
    }
    drop(w);
    patch_class(id);
    mark_live();
    Ok(())
}

/// The `MethodImpl` that instance method `name` resolves to for instances of
/// `id`, in dispatch order per ancestor: overlay delta, registry method,
/// builtin-reopen value method, builtin table row -- the latter two wrapped
/// into the `MethodImpl` ABI (they run against a boxed receiver, so the
/// resulting alias serves `RObj` dispatch, which is where the overlay is
/// probed). A compile-time builtin-alias row resolves through its target
/// (rows are terminal, so the single recursion can't loop).
pub(crate) fn snapshot_instance_method(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    snapshot_from(id, name, false)
}

/// [`snapshot_instance_method`] with the overlay layer skipped on EVERY
/// ancestor -- the body that answered before any runtime `define_method` did.
///
/// This is how a `Method`/`UnboundMethod` re-finds the entry it froze. It
/// cannot simply keep the `MethodImpl`: zeo materializes a LAYOUT-CORRECT copy
/// of a compiled method per class, so `Foo#b`'s body cannot run against a `Sub`
/// instance. Freezing which LAYER answered, and re-resolving from that layer
/// for the actual receiver's class, is both redefinition-proof and
/// layout-correct.
pub(crate) fn snapshot_below_overlay(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    snapshot_from(id, name, true)
}

fn snapshot_from(id: ClassId, name: Symbol, skip_overlay: bool) -> Option<MethodImpl> {
    let n = name.name();
    let n = n.as_str();
    for &anc in ancestors_of_value(id) {
        if !skip_overlay {
            let c = maps().classes.read().unwrap();
            if let Some(m) = c.get(&anc.0).and_then(|e| {
                e.prepended
                    .get(&name)
                    .or_else(|| e.methods.get(&name))
                    .cloned()
            }) {
                return Some(m);
            }
        }
        if let Some(m) = registry_lookup_cloned(anc, name) {
            return Some(m);
        }
        if let Some(m) = crate::dispatch::registry_value_method_impl(anc, name) {
            return Some(m);
        }
        if let Some(f) = crate::builtins::class_table(anc).and_then(|t| t(n)) {
            return Some(MethodImpl::Dynamic(Arc::new(
                move |recv: &RObj, args: &[RubyValue], block: Option<RubyValue>| {
                    f(&RubyValue::Object(recv.clone()), args, block)
                },
            )));
        }
    }
    crate::dispatch::alias_target(id, name).and_then(|old| snapshot_from(id, old, skip_overlay))
}

/// Whether `name` currently resolves for instances of `id` through the
/// OVERLAY -- a runtime `define_method` body. See [`snapshot_below_overlay`].
pub(crate) fn resolves_through_overlay(id: ClassId, name: Symbol) -> bool {
    if !is_live() {
        return false;
    }
    for &anc in ancestors_of_value(id) {
        {
            let c = maps().classes.read().unwrap();
            if c.get(&anc.0).is_some_and(|e| e.methods.contains_key(&name)) {
                return true;
            }
        }
        if registry_lookup_cloned(anc, name).is_some()
            || crate::dispatch::registry_value_method_impl(anc, name).is_some()
            || crate::builtins::class_table(anc).is_some_and(|t| t(name.name_str()).is_some())
        {
            return false;
        }
    }
    false
}

/// `Module#module_function(*names)` reached at RUNTIME -- fileutils calls
/// `module_function name` with a COMPUTED name inside its own
/// `private_module_function` helper (the plain literal form resolves at compile
/// time in `lower/defs.rs`). Promotes each named instance method to a
/// class/module method using the same wrapper `extend self` builds
/// (`extended_class_method`), so `Mod.name` and bare calls in class-method
/// context resolve. The instance copy stays -- that is the `include`-mixin half
/// of `module_function` -- but becomes PRIVATE, as in CRuby. The bare (no-arg)
/// mode form has no runtime spelling here and is a documented nil no-op.
pub fn runtime_module_function(id: ClassId, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        update_frame_for(id, |f| f.module_function = true);
        return Ok(RubyValue::Nil);
    }
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    let mut syms = Vec::with_capacity(args.len());
    for a in args {
        syms.push(coerce_method_name(Some(a))?);
    }
    // Build the wrappers BEFORE taking the overlay write lock:
    // `extended_class_method` itself reads the overlay (see `runtime_extend`).
    let mut installs = Vec::with_capacity(syms.len());
    for &sym in &syms {
        let proc_ = extended_class_method(id, sym).ok_or_else(|| {
            let cls = crate::dispatch::class_name(id).unwrap_or_default();
            name_error!("undefined method '{}' for module '{cls}'", sym.name())
        })?;
        installs.push((sym, proc_));
    }
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        for (sym, proc_) in installs {
            e.class_methods.insert(sym, proc_);
            e.extended_class_methods.remove(&sym);
            e.methods_vis
                .insert(sym, crate::dispatch::MethodVisibility::Private);
        }
    }
    patch_class(id);
    mark_live();
    // Only the module-method half is new here -- the instance copy already
    // existed and merely turned private, so ruby reports just the singleton.
    // (The BARE `module_function` mode reports both, from the `def` that
    // follows it; see `runtime_define_method`.)
    for &sym in &syms {
        fire_def_hook(
            DefTarget::Singleton(&RubyValue::Class(id)),
            DefEvent::Added,
            sym,
        )?;
    }
    Ok(match syms.as_slice() {
        [one] => RubyValue::Symbol(*one),
        many => RubyValue::Array(crate::array_new(
            many.iter().map(|s| RubyValue::Symbol(*s)).collect(),
        )),
    })
}

/// `define_method(name, method_obj)` -- install an instance method on `id`
/// whose body IS the `Method`/`UnboundMethod`'s source definition (`owner`
/// class, `src_name`). CRuby requires `id` be the source's owner or a
/// descendant (so `self` is a valid instance of the method's class),
/// TypeError otherwise. Snapshots the source impl (the same resolution
/// `alias_method` uses) and installs it under `name`. Returns the name.
pub fn runtime_define_method_from_method(
    id: ClassId,
    name: Symbol,
    owner: ClassId,
    src_name: Symbol,
) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // A MODULE-owned method binds anywhere (CRuby's rule -- rack installs
    // `ERB::Escape.instance_method(:html_escape)` into `Rack::Utils`, which
    // never includes it); only a CLASS-owned one requires the target to be
    // the owner or a descendant, so `self` is a valid instance.
    let owner_is_module = crate::dispatch::class_is_module(owner).unwrap_or(false);
    if !owner_is_module && !ancestors_of_value(id).contains(&owner) {
        return Err(type_error!(
            "bind argument must be a subclass of {}",
            crate::dispatch::class_name(owner).unwrap_or_default()
        ));
    }
    // Snapshot the method as resolved for the TARGET class's own instances,
    // not the owner's: zeo materializes each class's own layout-correct copy
    // of an inherited method, and the owner's compiled impl would panic on a
    // subclass-layout receiver. (For the common case where the target doesn't
    // override the name, this is the same behavior; a target that DOES
    // override it binds its own version -- a documented AOT divergence.)
    // A module owner outside the target's chain resolves nothing on the
    // target, so the module's own receiver-generic body answers instead.
    let m = snapshot_instance_method(id, src_name)
        .or_else(|| {
            owner_is_module
                .then(|| module_own_method_impl(owner, src_name))
                .flatten()
        })
        .ok_or_else(|| {
            name_error!(
                "undefined method '{}' for class '{}'",
                src_name.name(),
                crate::dispatch::class_name(owner).unwrap_or_default()
            )
        })?;
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(name, m);
        e.methods_vis.remove(&name);
    }
    patch_class(id);
    mark_live();
    fire_def_hook(DefTarget::Class(id), DefEvent::Added, name)?;
    Ok(RubyValue::Symbol(name))
}

/// `recv.define_singleton_method(name, method_obj)` -- the method-object body
/// form: install the source method's definition as a per-object singleton (or
/// a class method for a `Class` receiver). The compatibility check runs
/// against the RECEIVER's class, matching CRuby's singleton bind rule.
pub fn runtime_define_singleton_from_method(
    recv: &RubyValue,
    name: Symbol,
    owner: ClassId,
    src_name: Symbol,
) -> Result<RubyValue, Signal> {
    let recv_class = recv.class_id();
    // Module-owned sources bind anywhere -- see
    // `runtime_define_method_from_method`.
    let owner_is_module = crate::dispatch::class_is_module(owner).unwrap_or(false);
    if !owner_is_module && !ancestors_of_value(recv_class).contains(&owner) {
        return Err(type_error!(
            "bind argument must be a subclass of {}",
            crate::dispatch::class_name(owner).unwrap_or_default()
        ));
    }
    // Snapshot as resolved for the RECEIVER's class (layout-correct copy) --
    // see the note in `runtime_define_method_from_method`.
    let m = snapshot_instance_method(recv_class, src_name)
        .or_else(|| {
            owner_is_module
                .then(|| module_own_method_impl(owner, src_name))
                .flatten()
        })
        .ok_or_else(|| {
            name_error!(
                "undefined method '{}' for class '{}'",
                src_name.name(),
                crate::dispatch::class_name(owner).unwrap_or_default()
            )
        })?;
    match recv {
        RubyValue::Class(cid) => {
            if crate::dispatch::class_frozen(*cid) {
                return Err(crate::dispatch::frozen_class_error(*cid));
            }
            // A class method whose body is a snapshot of an instance method:
            // wrap it so the `RubyValue::Class` receiver reaches the impl.
            let wrapped = RProc::with_self_and_block(
                move |self_val: &RubyValue, args: &[RubyValue], block: Option<RubyValue>| {
                    if let RubyValue::Object(o) = self_val {
                        m.call(o, args, block)
                    } else {
                        Err(type_error!(
                            "singleton method body needs an object receiver"
                        ))
                    }
                },
                recv.clone(),
                -1,
                true,
            );
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
            e.class_methods.insert(name, wrapped);
            e.extended_class_methods.remove(&name);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        RubyValue::Object(o) => {
            crate::builtins::check_frozen(recv)?;
            let key = obj_identity(o);
            maps()
                .singletons
                .write()
                .unwrap()
                .entry(key)
                .or_default()
                .insert(name, m);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        // Only an ordinary object, unlike `runtime_define_singleton_method`
        // below: the body here is a snapshot of a compiled instance method,
        // whose signature takes an `RObj` receiver, so there is nothing to bind
        // a bare Array/String to.
        other => Err(type_error!(
            "can't define singleton method for {}",
            immediate_kind(other)
        )),
    }
}

/// `recv.define_singleton_method(name) { body }` -- a per-object singleton when
/// `recv` is an ordinary object, or a class/singleton method when `recv` is a
/// `Class`. A singleton on an immediate (Integer/Symbol/nil/...) is a
/// `TypeError`, matching CRuby.
pub fn runtime_define_singleton_method(
    recv: &RubyValue,
    name: Symbol,
    body: RProc,
) -> Result<RubyValue, Signal> {
    match recv {
        RubyValue::Class(cid) => {
            if crate::dispatch::class_frozen(*cid) {
                return Err(crate::dispatch::frozen_class_error(*cid));
            }
            crate::method_meta::record_runtime_params(
                *cid,
                crate::MethodKind::Singleton,
                name,
                &body,
            );
            {
                let mut w = maps().classes.write().unwrap();
                let e = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
                e.class_methods.insert(name, body);
                e.extended_class_methods.remove(&name);
            }
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        RubyValue::Object(o) => {
            // CRuby's rb_check_frozen on the singleton's attachee: a frozen
            // object refuses new singleton methods.
            crate::builtins::check_frozen(recv)?;
            let key = pin_identity(&RubyValue::Object(o.clone()))
                .expect("an Object always has an identity");
            crate::method_meta::record_singleton_params(key, name, &body);
            let m = dynamic_from_proc(SINGLETON_DEFINING, name, body);
            {
                let mut w = maps().singletons.write().unwrap();
                w.entry(key).or_default().insert(name, m);
            }
            clear_extended_name(key, name);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        other => {
            // Any other heap value (`def SOME_ARRAY.[](i)`) -- see
            // `value_identity`. The body stays an `RProc`, called with the value
            // itself as `self`, because a bare Array has no `RObj` to bind.
            let Some(key) = pin_identity(other) else {
                return Err(type_error!("can't define singleton"));
            };
            // `nil`/`true`/`false` report as frozen but still accept a
            // singleton -- CRuby's singleton class for them IS their class, and
            // takes no frozen check (oracle-verified). Every other value does.
            if !matches!(other, RubyValue::Nil | RubyValue::Bool(_)) {
                crate::builtins::check_frozen(recv)?;
            }
            crate::method_meta::record_singleton_params(key, name, &body);
            {
                let mut w = maps().value_singletons.write().unwrap();
                w.entry(key).or_default().insert(name, body);
            }
            clear_extended_name(key, name);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
    }
}

/// `recv.extend(Mod)` -- mix a module's instance methods into the receiver's
/// singleton, so they resolve on `recv`. Works for every receiver kind Ruby
/// allows:
///
/// * an ORDINARY object -- methods land in the identity-keyed singleton table
///   `resolve_dynamic` consults first (`obj.foo` only for this object).
/// * a CLASS or MODULE (`SecureRandom.extend(Random::Formatter)`) -- methods
///   become the receiver's CLASS/module methods (its singleton class), so
///   `SecureRandom.hex` resolves. A native module's method runs with the Class
///   as `self` verbatim (redispatching e.g. `gen_random` back to the receiver);
///   a user module's method, whose compiled body wants an object receiver, runs
///   against a fresh instance of the receiver class.
///
/// An existing singleton/class method (`def self.x`) is never clobbered -- own
/// singletons outrank an extended module, exactly as in CRuby's ancestry.
///
/// `Object#extend` is DEFINED IN TERMS of `Module#extend_object` and does no
/// mixing itself (`eval.c`'s `rb_obj_extend`), the same shape `include` has
/// around `append_features`. A module that overrides the primitive therefore
/// controls what `extend` does to the receiver -- and an override that omits
/// `super` skips the mixin entirely while `extended` still fires. The default
/// primitive lands back in [`extend_object_default`], which is this function's
/// body minus the routing and the hook.
pub fn runtime_extend(recv: &RubyValue, module_val: &RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::class_name_of(module_val)
        ));
    };
    match overrides_mixin_primitive(*mid, "extend_object") {
        true => {
            crate::dispatch::send_value(
                module_val,
                Symbol::intern("extend_object"),
                std::slice::from_ref(recv),
                None,
            )?;
        }
        false => extend_object_default(recv, module_val)?,
    }
    fire_mixin_hook(module_val, "extended", recv)?;
    Ok(recv.clone())
}

/// `Module#extend_object`'s default body -- the mixin itself, without the
/// `extended` notification its caller owns. See [`runtime_extend`].
pub fn extend_object_default(recv: &RubyValue, module_val: &RubyValue) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::class_name_of(module_val)
        ));
    };
    // A repeat `extend` re-ranks nothing: the module keeps the position its
    // FIRST one gave it, so re-copying its methods would wrongly promote it
    // over a module extended in between. The `extended` hook still fires each
    // time -- which it does, because the caller owns it (both oracle-verified).
    if extended_modules(recv).contains(mid) {
        return Ok(());
    }
    let names = module_extendable_method_names(*mid);
    match recv {
        RubyValue::Object(o) => {
            // CRuby's rb_check_frozen: a frozen object refuses `extend` (its
            // singleton table is what would change).
            crate::builtins::check_frozen(recv)?;
            let key = pin_identity(&RubyValue::Object(o.clone()))
                .expect("an Object always has an identity");
            let mut copied = Vec::new();
            {
                let mut w = maps().singletons.write().unwrap();
                let table = w.entry(key).or_default();
                for name in names {
                    if let Some(m) = module_own_method_impl(*mid, name) {
                        table.insert(name, m);
                        copied.push(name);
                    }
                }
            }
            record_extended_names(key, *mid, &copied);
        }
        RubyValue::Class(cid) => {
            if crate::dispatch::class_frozen(*cid) {
                return Err(crate::dispatch::frozen_class_error(*cid));
            }
            // Build every wrapper BEFORE taking the overlay write lock:
            // `extended_class_method` reads the overlay (module_own_method_impl),
            // so building under the lock would deadlock.
            let installs: Vec<(Symbol, RProc)> = names
                .into_iter()
                // Own `def self.x` (materialized into the registry) outranks the
                // module, so leave it be -- also what keeps SecureRandom's own
                // `gen_random` from being shadowed by the mixin's bridge copy.
                .filter(|&name| !crate::dispatch::class_defines_own_class_method(*cid, name))
                .filter_map(|name| extended_class_method(*mid, name).map(|p| (name, p)))
                .collect();
            let mut w = maps().classes.write().unwrap();
            let entry = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
            for (name, proc_) in installs {
                // A later `extend` layers ABOVE an earlier one (CRuby ancestry),
                // so it wins on a name collision between two mixins.
                entry.class_methods.insert(name, proc_);
                entry.extended_class_methods.insert(name);
            }
        }
        other => {
            // Any other HEAP value (`ARGV.extend(OptionParser::Arguable)`, the
            // last line of optparse) -- the same value-keyed singleton table
            // `def SOME_ARRAY.m` writes to, since a bare Array has no `RObj` to
            // bind a compiled body against. Immediates have no singleton
            // storage here at all, same posture as `define_singleton_method`.
            // An Integer/Float/Symbol has no singleton storage in CRuby either,
            // and reports it with the same wording `define_singleton_method`
            // does -- not a per-kind message (oracle-verified). `nil`/`true`/
            // `false` DO accept one, and take no frozen check.
            let Some(key) = pin_identity(other) else {
                return Err(type_error!("can't define singleton"));
            };
            if !matches!(other, RubyValue::Nil | RubyValue::Bool(_)) {
                crate::builtins::check_frozen(recv)?;
            }
            // Built before the write lock, like the Class arm above:
            // `extended_value_method` reads the overlay.
            let installs: Vec<(Symbol, RProc)> = names
                .into_iter()
                .filter_map(|name| extended_value_method(*mid, name).map(|p| (name, p)))
                .collect();
            let copied: Vec<Symbol> = installs.iter().map(|(n, _)| *n).collect();
            {
                let mut w = maps().value_singletons.write().unwrap();
                let table = w.entry(key).or_default();
                for (name, proc_) in installs {
                    table.insert(name, proc_);
                }
            }
            record_extended_names(key, *mid, &copied);
        }
    }
    // The method copies above make the module ANSWER on `recv`; this is what
    // makes `recv` BE one -- `is_a?`, `===` and `singleton_class.ancestors`
    // all read it, and none of them can see a copied method table.
    record_extended(recv, *mid);
    refresh_singleton_ancestors(recv);
    mark_singletons();
    mark_ancestry_mutated();
    mark_live();
    Ok(())
}

/// `K.singleton_class.prepend(M)` -- M's instance methods become K's CLASS
/// methods at HIGHER priority than K's own `def self.x`, with `super` from one
/// resuming at the shadowed definition (the ForkTracker / fork-hook shape,
/// reached at runtime when K itself is a runtime-minted class the compile-time
/// ancestry edit could not name). The copies live in their own layer
/// (`prepended_class_methods`), probed before everything by
/// [`overlay_class_method`] and skipped when a prepended method's own `super`
/// resumes the walk (`send_super_class_from`).
///
/// Reflection nuance, written down rather than papered over (the posture of
/// the singleton-include note in [`mix_in`]): the module is recorded via the
/// extended list, so `K.singleton_class.ancestors` reports it AFTER the
/// singleton head where CRuby puts it before.
fn prepend_into_class_singleton(owner: ClassId, module_val: &RubyValue) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::class_name_of(module_val)
        ));
    };
    if crate::dispatch::class_frozen(owner) {
        return Err(crate::dispatch::frozen_class_error(owner));
    }
    // Re-prepending a module already in the stack is a no-op, as CRuby's is
    // -- and overwriting the winner copies would silently hoist it above
    // later prepends.
    if has_singleton_prepend(owner, *mid) {
        return Ok(());
    }
    // Built before the write lock, like `extend_object_default`'s Class arm:
    // `extended_class_method` reads the overlay.
    let installs: Vec<(Symbol, RProc)> = module_extendable_method_names(*mid)
        .into_iter()
        .filter_map(|name| extended_class_method(*mid, name).map(|p| (name, p)))
        .collect();
    let owner_val = RubyValue::Class(owner);
    {
        let mut w = maps().classes.write().unwrap();
        let entry = w.entry(owner.0).or_insert_with(OverlayEntry::delta);
        for (name, proc_) in installs {
            // A later prepend layers ABOVE an earlier one (CRuby ancestry), so
            // it wins a name collision -- the extend table's rule. The shadowed
            // copy stays reachable: a prepended method's `super` resumes down
            // the recorded stack (`singleton_prepend_super_below`).
            entry.prepended_class_methods.insert(name, proc_);
        }
        entry.singleton_prepends.push(*mid);
    }
    // The method copies make the module ANSWER on the owner; the extended
    // record is what makes the owner BE one (`is_a?`, `===`,
    // `singleton_class.ancestors` -- see `extend_object_default`).
    record_extended(&owner_val, *mid);
    refresh_singleton_ancestors(&owner_val);
    mark_singletons();
    mark_ancestry_mutated();
    mark_live();
    Ok(())
}

/// `Module#include(M, ...)` reached AT RUNTIME on a Class/Module receiver --
/// e.g. `Class.new { include M }`. Splices each module (and its own ancestors
/// not already present) into the receiver's overlay ancestry right after the
/// receiver itself, matching CRuby's insertion point, so instances dispatch the
/// module's methods through `dispatch::send_in`'s ancestor walk (which finds a
/// compiled module's `emit_user_module_bridges` value-methods by id, and a
/// runtime `Module.new`'s overlay methods).
///
/// Only effective for a receiver whose ancestry lives in the overlay (a runtime
/// `Class.new`/`Module.new`); a FROZEN compiled class's registry ancestry is
/// immutable, so a runtime `include` on it is a documented no-op on dispatch
/// (rare -- static `include` is the compiled path).
pub fn runtime_include(recv: &RubyValue, modules: &[RubyValue]) -> Result<RubyValue, Signal> {
    // `include A, B` inserts each right after self, so the LAST argument ends
    // up closest to self -- process right-to-left to reproduce that order.
    mix_in(recv, modules, Placement::After, "include")
}

/// `Module#prepend(M, ...)` -- the mirror of `include`, splicing before the
/// receiver so the module's methods win over the receiver's own. `super` from
/// one then resumes at the receiver, because `send_super_from` walks by
/// POSITION and the module's id is what was pushed as the defining class.
pub fn runtime_prepend(recv: &RubyValue, modules: &[RubyValue]) -> Result<RubyValue, Signal> {
    mix_in(recv, modules, Placement::Before, "prepend")
}

#[derive(Clone, Copy, PartialEq)]
enum Placement {
    Before,
    After,
}

fn mix_in(
    recv: &RubyValue,
    modules: &[RubyValue],
    placement: Placement,
    verb: &str,
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = recv else {
        return Err(type_error!("can't {verb} into {}", immediate_kind(recv)));
    };
    // `obj.singleton_class.include(M)` IS `obj.extend(M)` -- CRuby DEFINES the
    // latter as the former (`rb_include_module(rb_singleton_class(obj), M)`),
    // so the primitive and the wrapper are one operation. zeo implements a
    // singleton as a copied method table (`extend_object_default`) rather than
    // spliced ancestry, so splicing this id would write somewhere the owner's
    // dispatch never reads: the mixin would vanish silently.
    //
    // rdoc's `class << self; prepend Git`, spreadsheet's
    // `class << self; include Compatibility` and treetop's are all this shape.
    //
    // `prepend` into a CLASS's singleton has machinery of its own: the module
    // must outrank the owner's `def self.x`, which the extend table (own
    // definitions win there) cannot express. On a plain OBJECT's singleton,
    // `prepend` still lands in the extend table, which is only observable
    // against the object's OWN `def obj.x` -- CRuby would let the module win
    // over that, and this does not. Written down rather than papered over; no
    // gem in the corpus depends on the difference.
    if let Some(owner) = singleton_owner_value(*cid) {
        for module_val in modules {
            match (&owner, placement) {
                (RubyValue::Class(owner_id), Placement::Before) => {
                    prepend_into_class_singleton(*owner_id, module_val)?;
                    fire_mixin_hook(module_val, "prepended", recv)?;
                }
                _ => {
                    runtime_extend(&owner, module_val)?;
                }
            }
        }
        return Ok(recv.clone());
    }
    if crate::dispatch::class_frozen(*cid) {
        return Err(crate::dispatch::frozen_class_error(*cid));
    }
    // `prepend A, B` puts B closest to self and `include A, B` puts B closest
    // too, so both walk the arguments in the order that lands them there.
    let ordered: Vec<&RubyValue> = match placement {
        Placement::After => modules.iter().rev().collect(),
        Placement::Before => modules.iter().collect(),
    };
    let (primitive, hook) = match placement {
        Placement::Before => ("prepend_features", "prepended"),
        Placement::After => ("append_features", "included"),
    };
    for module_val in ordered {
        let RubyValue::Class(mid) = module_val else {
            return Err(type_error!(
                "wrong argument type {} (expected Module)",
                crate::builtins::class_name_of(module_val)
            ));
        };
        // `Module#include` is DEFINED IN TERMS of `append_features` and does no
        // splicing itself (`eval.c`'s `rb_mod_include`), which is what lets a
        // module police how it is mixed in -- the `singleton` gem overrides
        // this very method to reject inclusion into a module. An override that
        // omits `super` therefore skips the mixin entirely, and `included`
        // still fires. Both oracle-verified.
        match overrides_mixin_primitive(*mid, primitive) {
            true => {
                crate::dispatch::send_value(
                    module_val,
                    Symbol::intern(primitive),
                    std::slice::from_ref(recv),
                    None,
                )?;
            }
            false => {
                splice_module_into(*cid, *mid, placement);
                // The same overlay copy `Module#prepend_features` makes: a
                // prepend has to outrank the target's OWN methods, and the
                // flattened per-class table wins over the spliced chain --
                // see `overlay_prepended_methods`.
                if placement == Placement::Before {
                    overlay_prepended_methods(*cid, *mid);
                }
            }
        }
        fire_mixin_hook(module_val, hook, recv)?;
    }
    mark_ancestry_mutated();
    patch_class(*cid);
    mark_live();
    Ok(recv.clone())
}

/// Whether `module` supplies its own body for one of the three mix-in
/// PRIMITIVES, as opposed to inheriting `Module`'s. The defaults are the splice
/// itself, so reaching them through a send would be an infinite regress --
/// `Module#append_features` calls the splice directly for exactly that reason.
pub(crate) fn overrides_mixin_primitive(module: ClassId, primitive: &str) -> bool {
    crate::dispatch::class_method_owner(module, Symbol::intern(primitive)).is_some()
}

/// The splice `Module#prepend_features`/`#append_features` performs -- the
/// primitive, with no notification hook of its own.
pub fn splice_mixin(target: &RubyValue, module: &RubyValue, before: bool) -> Result<(), Signal> {
    let (RubyValue::Class(cid), RubyValue::Class(mid)) = (target, module) else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::class_name_of(target)
        ));
    };
    if crate::dispatch::class_frozen(*cid) {
        return Err(crate::dispatch::frozen_class_error(*cid));
    }
    let placement = match before {
        true => Placement::Before,
        false => Placement::After,
    };
    splice_module_into(*cid, *mid, placement);
    if before {
        overlay_prepended_methods(*cid, *mid);
    }
    mark_ancestry_mutated();
    patch_class(*cid);
    mark_live();
    Ok(())
}

/// A PREPEND has to outrank the target's OWN methods, and `send_in` resolves an
/// object through a flattened per-class table before it walks any ancestry --
/// so splicing the chain alone leaves the class's own body still winning.
///
/// Copy the module's methods into the target's overlay, which IS probed first.
/// The copies come from the module's value-method container, whose bodies take
/// their receiver as an argument, so one copy serves every instance -- unlike a
/// compiled class method, which is laid out per class. Only the module's OWN
/// methods; one it inherited in turn stays on the ancestry walk.
fn overlay_prepended_methods(cid: ClassId, mid: ClassId) {
    let names = crate::dispatch::instance_method_names(mid, crate::dispatch::VisFilter::All, false);
    let installs: Vec<(Symbol, MethodImpl)> = names
        .into_iter()
        .filter_map(|n| crate::dispatch::registry_value_method_impl(mid, n).map(|m| (n, m)))
        .collect();
    if installs.is_empty() {
        return;
    }
    let mut w = maps().classes.write().unwrap();
    let e = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
    for (name, m) in installs {
        e.prepended.insert(name, m);
    }
}

/// Ruby's mixin hook, run right after the ancestry edit: `M.included(target)`,
/// `M.extended(target)`, `M.prepended(target)`. `Module`'s own default is a
/// no-op, so nothing is dispatched unless the module really defines one.
pub(crate) fn fire_mixin_hook(
    module: &RubyValue,
    hook: &str,
    target: &RubyValue,
) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module else {
        return Ok(());
    };
    let sym = Symbol::intern(hook);
    // A module minted by `class X < Module` reaches the hook as an INSTANCE
    // method of X -- CRuby looks it up through the module's singleton chain,
    // which runs into its class. `class_method_owner` only sees class methods
    // of the module itself, so it misses that one; Rails' `included` hooks on
    // `DeprecatedConstantProxy` are exactly this shape.
    let defined = crate::dispatch::class_method_owner(*mid, sym).is_some()
        || module_owner_class(*mid).is_some_and(|owner| module_subclass_defines(owner, sym));
    if !defined {
        return Ok(());
    }
    crate::dispatch::send_value(module, sym, std::slice::from_ref(target), None)?;
    Ok(())
}

/// Which of Ruby's three definition events happened. The hook's NAME is this
/// plus the target's shape: the same event is `method_added` on a class and
/// `singleton_method_added` on a singleton.
#[derive(Clone, Copy)]
pub(crate) enum DefEvent {
    Added,
    Removed,
    Undefined,
}

/// Where a definition landed. CRuby reads this off the target class's
/// `RCLASS_SINGLETON_P` bit and then rewrites the receiver to the attached
/// object (`vm_method.c`'s `CALL_METHOD_HOOK`). zeo's runtime writers already
/// know which of the two they are -- a singleton definition never reaches a
/// shared class's method table -- so they say so rather than making this
/// re-derive it from an id.
pub(crate) enum DefTarget<'a> {
    Class(ClassId),
    Singleton(&'a RubyValue),
}

/// Hook names reopened onto `Module`/`Class`/`BasicObject`, which apply to
/// EVERY class in the program. No per-class owner scan can see one: the reopen
/// registers an ordinary instance method whose owner id is the very id the
/// no-op default carries, so the two are indistinguishable by owner. Recorded
/// explicitly instead, by whoever installs it.
static ANY_GLOBAL_DEF_HOOK: AtomicBool = AtomicBool::new(false);
static GLOBAL_DEF_HOOKS: OnceLock<RwLock<FSet<Symbol>>> = OnceLock::new();

/// `hook` was defined on `Module`/`Class`/`BasicObject` itself. Called by
/// codegen for a compiled reopen and by [`runtime_define_method`] for a
/// runtime one.
pub fn mark_global_def_hook(hook: &str) {
    GLOBAL_DEF_HOOKS
        .get_or_init(|| RwLock::new(FSet::default()))
        .write()
        .unwrap()
        .insert(Symbol::intern(hook));
    ANY_GLOBAL_DEF_HOOK.store(true, Ordering::Release);
}

fn global_def_hook(hook: Symbol) -> bool {
    ANY_GLOBAL_DEF_HOOK.load(Ordering::Acquire)
        && GLOBAL_DEF_HOOKS
            .get()
            .is_some_and(|h| h.read().unwrap().contains(&hook))
}

/// Whether a body the USER wrote will answer `hook`, as opposed to the no-op
/// default. Every caller reaches this from a runtime definition, which already
/// pays a write lock and an O(#classes) `patch_class`, so an MRO scan here
/// costs nothing worth latching around.
fn def_hook_runs(target: &DefTarget<'_>, hook: Symbol) -> bool {
    if global_def_hook(hook) {
        return true;
    }
    match target {
        // `class_method_owner` finds a `def self.method_added` and an
        // `extend`ed module's copy of one, and deliberately does NOT find
        // `Module`'s own no-op row: that is an INSTANCE method of Module, and
        // this scan walks class-method tables. So the default costs nothing.
        DefTarget::Class(cid) | DefTarget::Singleton(RubyValue::Class(cid)) => {
            crate::dispatch::class_method_owner(*cid, hook).is_some()
        }
        // `singleton_method_added` IS an instance method of `BasicObject`, so
        // here the scan does reach the default and it has to be ruled out by
        // owner. `Numeric`'s override -- which refuses a singleton on a number
        // at all -- resolves to `Numeric` and passes.
        DefTarget::Singleton(v) => {
            singleton_method_names(v).contains(&hook)
                || crate::dispatch::method_owner(v.class_id(), hook)
                    .is_some_and(|owner| owner != zeo_abi::BASIC_OBJECT_CLASS)
        }
    }
}

/// Ruby's definition hook: `Klass.method_added(:name)`, or its singleton twin
/// on the attached object. Run right after the definition lands, which is
/// CRuby's order -- the method is already callable when the hook sees its name.
///
/// Call this with every overlay lock DROPPED. A hook body that defines another
/// method is the whole point of the hook, and it takes the write lock again.
pub(crate) fn fire_def_hook(
    target: DefTarget<'_>,
    event: DefEvent,
    name: Symbol,
) -> Result<(), Signal> {
    let hook = match (&target, event) {
        (DefTarget::Class(_), DefEvent::Added) => "method_added",
        (DefTarget::Class(_), DefEvent::Removed) => "method_removed",
        (DefTarget::Class(_), DefEvent::Undefined) => "method_undefined",
        (DefTarget::Singleton(_), DefEvent::Added) => "singleton_method_added",
        (DefTarget::Singleton(_), DefEvent::Removed) => "singleton_method_removed",
        (DefTarget::Singleton(_), DefEvent::Undefined) => "singleton_method_undefined",
    };
    let hook = Symbol::intern(hook);
    if !def_hook_runs(&target, hook) {
        return Ok(());
    }
    let recv = match target {
        DefTarget::Class(cid) => RubyValue::Class(cid),
        DefTarget::Singleton(v) => v.clone(),
    };
    // `send_value`, not a visibility-checked call: CRuby reaches its hooks
    // through `rb_funcallv`, an FCALL, so a `private def self.method_added`
    // still runs.
    crate::dispatch::send_value(&recv, hook, &[RubyValue::Symbol(name)], None)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The definition watermark
// ---------------------------------------------------------------------------
//
// Ruby's definition hook sees a HALF-BUILT class: at `method_added(:a)`,
// `instance_methods(false)` answers `[:a]` alone, and `method_defined?(:b)` is
// false for a `def b` written below. zeo installs every method table before the
// program's first statement runs, so there is no such moment to observe.
//
// Rather than defer registration -- which would cost every program -- the
// compiler works out which names are still in the future at each announcement
// (it knows them statically) and hands them over for the duration of the hook
// body. The reflection rows subtract the set; DISPATCH deliberately does not,
// so calling a not-yet-defined method from inside a hook succeeds here and
// raises `NoMethodError` in ruby. Truncating dispatch would put a thread-local
// check on the hot path for a case no real code exercises; it is recorded as
// `tests/gaps/method_added_calls_later_method.rb`.

thread_local! {
    /// One frame per definition hook currently on the stack. A hook body that
    /// defines another method nests, which is why this is a stack.
    static PENDING_DEFS: std::cell::RefCell<Vec<(ClassId, Vec<Symbol>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// How many hook frames are live across ALL threads. [`GATE_PENDING`] tracks
/// whether this is non-zero, so the gate lifts again once the last hook
/// returns rather than de-optimizing the rest of the program.
///
/// Two threads defining methods at the same time can interleave the decrement
/// and the increment such that the gate clears while the other thread is still
/// inside a hook. The consequence is exactly the behaviour zeo had before the
/// truncation existed -- that thread's hook body resolves a method it has not
/// been told about -- and no shape zeo compiles today fires definition hooks on
/// two threads at once. The thread-local stack stays the authority for WHICH
/// names are pending; this word only decides whether asking is worthwhile.
static PENDING_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Runs `f` -- a definition hook's send -- with `names` marked as not yet
/// defined on `class`.
pub fn with_pending_defs<R>(class: ClassId, names: &[Symbol], f: impl FnOnce() -> R) -> R {
    PENDING_DEPTH.fetch_add(1, Ordering::AcqRel);
    GATES.fetch_or(GATE_PENDING, Ordering::Release);
    PENDING_DEFS.with(|p| p.borrow_mut().push((class, names.to_vec())));
    let out = f();
    PENDING_DEFS.with(|p| {
        p.borrow_mut().pop();
    });
    if PENDING_DEPTH.fetch_sub(1, Ordering::AcqRel) == 1 {
        GATES.fetch_and(!GATE_PENDING, Ordering::Release);
    }
    out
}

/// Whether a definition hook is running with a non-empty pending set -- the
/// gate every truncation site reads before touching the thread-local.
#[inline(always)]
pub fn any_pending() -> bool {
    GATES.load(Ordering::Acquire) & GATE_PENDING != 0
}

/// Whether `name` is a method of `class` that the definition hook now running
/// has not seen defined yet.
#[inline]
pub fn not_yet_defined(class: ClassId, name: Symbol) -> bool {
    any_pending() && listed_pending(class, name) && !inherited(class, name)
}

/// Every name of `class` the running hook has not seen defined, for the callers
/// that enumerate rather than ask. Empty (and free) outside a hook body.
///
/// `inherit` picks which question is being asked, and the two differ. "Does
/// this name exist AT ALL?" -- `instance_methods(true)`, `method_defined?` --
/// is answered by an ancestor's copy, so a pending name an ancestor also
/// defines is not hidden. "Is it defined DIRECTLY here?" --
/// `instance_methods(false)` -- is not: `Sub#shared` written below the hook is
/// absent from Sub's OWN list even though `Base#shared` exists.
pub fn pending_defs_for(class: ClassId, inherit: bool) -> Vec<Symbol> {
    if !any_pending() {
        return Vec::new();
    }
    PENDING_DEFS.with(|p| {
        p.borrow()
            .iter()
            .filter(|(c, _)| *c == class)
            .flat_map(|(_, ns)| ns.iter().copied())
            .filter(|&n| !inherit || !inherited(class, n))
            .collect()
    })
}

/// Whether `class` itself has a `def name` still ahead of the running hook --
/// [`not_yet_defined`] without the ancestor rule, which is the question DISPATCH
/// asks. The two differ on an override: `Sub#shared` written below the hook is
/// not installed yet, so a call resolves to `Base#shared` rather than missing,
/// while `method_defined?(:shared)` was already true through `Base`.
#[inline]
pub fn pending_here(class: ClassId, name: Symbol) -> bool {
    any_pending() && listed_pending(class, name)
}

fn listed_pending(class: ClassId, name: Symbol) -> bool {
    PENDING_DEFS.with(|p| {
        p.borrow()
            .iter()
            .any(|(c, ns)| *c == class && ns.contains(&name))
    })
}

/// An ANCESTOR beyond `class` defines `name` too, so it exists no matter what
/// this class has reached: ruby hides only what does not exist ANYWHERE yet,
/// and a `Sub#to_s` written below the hook still leaves `Object#to_s`
/// reachable. Only ever asked inside a hook body.
fn inherited(class: ClassId, name: Symbol) -> bool {
    crate::dispatch::method_owner_after(class, class, name).is_some()
}

/// `Module#const_added` -- ruby announces a constant right after it becomes
/// readable, on the module it was set on. Unlike the `method_*` family this one
/// has no singleton rerouting: a constant set inside `class << K` announces on
/// `#<Class:K>` itself (oracle-verified).
pub fn fire_const_added(owner: ClassId, name: &str) -> Result<(), Signal> {
    let hook = Symbol::intern("const_added");
    if !global_def_hook(hook) && crate::dispatch::class_method_owner(owner, hook).is_none() {
        return Ok(());
    }
    crate::dispatch::send_value(
        &RubyValue::Class(owner),
        hook,
        &[RubyValue::Symbol(Symbol::intern(name))],
        None,
    )?;
    Ok(())
}

/// The seven hook names that become GLOBAL when defined on `Module`/`Class`
/// (the `method_*` trio) or on `BasicObject` (the `singleton_method_*` trio).
pub fn global_def_hook_owner(id: ClassId, name: Symbol) -> bool {
    match name.name_str() {
        "method_added" | "method_removed" | "method_undefined" | "const_added" => {
            id == zeo_abi::MODULE_CLASS || id == zeo_abi::CLASS_CLASS
        }
        "singleton_method_added" | "singleton_method_removed" | "singleton_method_undefined" => {
            id == zeo_abi::BASIC_OBJECT_CLASS
        }
        _ => false,
    }
}

/// One chain's version of the splice: `mid`'s ancestry inserted at `target`'s
/// own position in `current`, skipping ancestors this chain already has.
/// `None` when the chain doesn't pass through `target` or gains nothing.
fn splice_into_chain(
    current: &[ClassId],
    target: ClassId,
    fresh_src: &[ClassId],
    placement: Placement,
) -> Option<Vec<ClassId>> {
    // Not `current[0]`: after a prepend, self is no longer first.
    let at = current.iter().position(|&a| a == target)?;
    let present: HashSet<ClassId> = current.iter().copied().collect();
    let fresh: Vec<ClassId> = fresh_src
        .iter()
        .copied()
        .filter(|m| !present.contains(m))
        .collect();
    if fresh.is_empty() {
        return None;
    }
    let cut = if placement == Placement::Before {
        at
    } else {
        at + 1
    };
    let mut new_anc = Vec::with_capacity(current.len() + fresh.len());
    new_anc.extend_from_slice(&current[..cut]);
    new_anc.extend_from_slice(&fresh);
    new_anc.extend_from_slice(&current[cut..]);
    Some(new_anc)
}

/// Splices `mid`'s ancestry into `cid`'s -- and into EVERY chain that passes
/// through `cid`. CRuby's ancestry is a shared linked structure, so a later
/// `include` into a superclass is visible to subclasses minted earlier, and
/// (since Ruby 3.0) an `include` into a module already mixed in elsewhere
/// reaches its hosts too. zeo chains are flat leaked snapshots, so the splice
/// must visit each: the target's own chain, every overlay chain containing the
/// target (runtime-minted subclasses -- rspec's describe-groups gaining the
/// mock adapter their base class was given at configure time is the corpus
/// case), and every REGISTERED class whose frozen chain contains the target
/// (those mint an overlay chain here; the frozen row stays untouched).
///
/// Lock discipline: candidate ids are collected under the read lock reading
/// `ancestors` fields directly; chains are recomputed lock-free through
/// `ancestors_of_value` (overlay-first); one write installs them all. Old
/// slices leak, matching this runtime's no-GC policy for interned ancestries.
fn splice_module_into(cid: ClassId, mid: ClassId, placement: Placement) {
    let fresh_src: Vec<ClassId> = ancestors_of_value(mid).to_vec();
    let mut candidates: Vec<u32> = vec![cid.0];
    {
        let r = maps().classes.read().unwrap();
        for (&id, e) in r.iter() {
            if id != cid.0 && e.ancestors.contains(&cid) {
                candidates.push(id);
            }
        }
    }
    for id in crate::dispatch::classes_with_ancestor(cid) {
        // A registered class with a live overlay chain was already considered
        // above; only frozen-chain classes join here.
        if overlay_ancestors(ClassId(id)).is_none() && id != cid.0 {
            candidates.push(id);
        }
    }
    let mut updates: Vec<(u32, &'static [ClassId])> = Vec::new();
    for id in candidates {
        let current = ancestors_of_value(ClassId(id));
        if let Some(new_anc) = splice_into_chain(current, cid, &fresh_src, placement) {
            updates.push((id, Box::leak(new_anc.into_boxed_slice())));
        }
    }
    let mut w = maps().classes.write().unwrap();
    for (id, leaked) in updates {
        w.entry(id).or_insert_with(OverlayEntry::delta).ancestors = leaked;
    }
}

/// The public/protected instance-method names a module contributes to a host
/// via `extend`/`include` -- registry methods plus any runtime-overlay ones a
/// `Module.new` added.
fn module_extendable_method_names(mid: ClassId) -> Vec<Symbol> {
    // PRIVATE methods extend too -- ruby copies the module's whole instance
    // set onto the singleton, private ones staying private there, which is how
    // `singleton`'s own `set_mutex` writer travels. The filter said
    // `NotPrivate`, which went unnoticed only because a `private` inside a
    // module body never reached the registry to begin with.
    let mut names =
        crate::dispatch::instance_method_names(mid, crate::dispatch::VisFilter::All, false);
    for n in overlay_own_method_names(mid) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names
}

/// One `extend`ed module method for a BARE HEAP VALUE receiver
/// (`ARGV.extend(OptionParser::Arguable)`). Both the native module tables and
/// a compiled user module's own bridge already take a `&RubyValue` receiver --
/// which an Array/String/Hash IS -- so the value passes straight through, with
/// no surrogate instance anywhere. A module method that only exists as an
/// `&RObj` body has no way to run against a bare value and is skipped, the same
/// posture `extended_class_method` takes for its own unreachable cases.
fn extended_value_method(mid: ClassId, name: Symbol) -> Option<RProc> {
    let f = crate::builtins::class_table(mid)
        .and_then(|t| t(&name.name()))
        .or_else(|| crate::dispatch::value_method(mid, 0, name))?;
    Some(RProc::with_self_and_block(f, RubyValue::Nil, -1, true))
}

/// One `extend`ed module method, wrapped as a class method (a value-receiver
/// `RProc`, invoked with the Class as `self`).
fn extended_class_method(mid: ClassId, name: Symbol) -> Option<RProc> {
    let sname = name.name();
    // A NATIVE module method (`Random::Formatter`, `Comparable`, ...) takes a
    // value receiver, so it runs correctly with a Class `self` and can
    // redispatch to it. Pass `self` straight through.
    if let Some(f) = crate::builtins::class_table(mid).and_then(|t| t(&sname)) {
        return Some(RProc::with_self_and_block(f, RubyValue::Nil, -1, true));
    }
    // A compiled user module ALSO emits a value bridge per method, which takes
    // `self` as a plain `RubyValue` -- so a Class receiver passes straight
    // through and `self` really is the class. That is what makes an
    // implicit-self CLASS-method call resolve (the singleton gem's
    // `@singleton__instance__ ||= new`, where an object receiver would look
    // for an instance method `new` and find none) and what routes `@x` to the
    // class's own store. Preferred over the `&RObj` body below.
    if let Some(f) = crate::dispatch::value_method(mid, 0, name) {
        return Some(RProc::with_self_and_block(f, RubyValue::Nil, -1, true));
    }
    // A USER module method is a compiled `&RObj` body: it needs an object
    // receiver. When invoked with a Class `self`, run it against a
    // `ClassSurrogate` -- an `RObj` shell whose ivars ARE the class's own
    // class-level store, so `@x = 1` in the module's body lands where
    // `Klass.instance_variable_get(:@x)` and a `def self.x` reading `@x` both
    // look. (A blank throwaway instance stood here, and swallowed every such
    // write: the singleton gem's `klass.extend SingletonClassMethods` then
    // `klass.instance_eval { set_mutex(Thread::Mutex.new) }` left the mutex
    // nil, and `Singleton#instance` raised on it.)
    // An INCLUDED module's method counts: `module_function :greet` after
    // `include Greeting` promotes the mixed-in body, and `extend self` reaches
    // the same way. Own definitions win, so the ancestry walk is only the
    // fallback.
    let m = module_own_method_impl(mid, name).or_else(|| snapshot_instance_method(mid, name))?;
    Some(RProc::with_self_and_block(
        move |self_val, args, block| match self_val {
            RubyValue::Object(o) => m.call(o, args, block),
            RubyValue::Class(cid) => {
                let surrogate: crate::dispatch::RObj = Arc::new(ClassSurrogate { class_id: *cid });
                m.call(&surrogate, args, block)
            }
            other => Err(type_error!(
                "can't run an extended method with {} as self",
                immediate_kind(other)
            )),
        },
        RubyValue::Nil,
        -1,
        true,
    ))
}

/// The `MethodImpl` a module id defines for `name` directly -- overlay delta
/// first (a runtime `Module.new`/`define_method`), then the frozen registry (a
/// compile-time `module M; def hi; end`).
/// The overlay's own runtime-defined instance-method names for a class/module
/// id (empty if it has no overlay entry) -- the names the registry-based
/// `instance_method_names` can't see.
fn overlay_own_method_names(id: ClassId) -> Vec<Symbol> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .map(|e| e.methods.keys().copied().collect())
        .unwrap_or_default()
}

/// Every instance-method name `id`'s own overlay entry has an opinion about,
/// paired with its visibility -- `None` for an `undef_method` tombstone, which
/// carries no method but still CLAIMS the name so an ancestor's definition
/// can't answer for it.
///
/// This is the overlay half of `dispatch::instance_method_names`, whose other
/// half reads the frozen registry: a class minted by `Class.new` has all of its
/// methods here and none of them there.
pub fn overlay_instance_method_names(
    id: ClassId,
) -> Vec<(Symbol, Option<crate::dispatch::MethodVisibility>)> {
    let c = maps().classes.read().unwrap();
    let Some(e) = c.get(&id.0) else {
        return Vec::new();
    };
    // A visibility mark can name a method the overlay carries no body for
    // (`class_eval { private :compiled_method }`), so the marks contribute
    // names of their own rather than just annotating `methods`.
    let named: HashSet<Symbol> = e
        .methods
        .keys()
        .chain(e.prepended.keys())
        .chain(e.methods_vis.keys())
        .copied()
        .filter(|n| !e.undefs.contains(n))
        .collect();
    e.undefs
        .iter()
        .map(|&n| (n, None))
        .chain(named.into_iter().map(|n| {
            let vis = e.methods_vis.get(&n).copied();
            let vis = vis.unwrap_or(crate::dispatch::MethodVisibility::Public);
            (n, Some(vis))
        }))
        .collect()
}

/// The overlay's own CLASS-method names for `id` -- the class-level counterpart
/// of [`overlay_instance_method_names`], and likewise invisible to the frozen
/// registry that `dispatch::class_method_names` otherwise reads.
/// The runtime `define_method` body `id` ITSELF holds for `name`. Value-shaped:
/// see [`OverlayEntry::value_bodies`] for why an `RObj` receiver takes a
/// different route.
///
/// Own-only, because the caller interleaves it with the registry and builtin
/// probes ancestor by ancestor -- ruby's placement rule. A whole-ancestry walk
/// here would let `Enumerable.define_method(:map)` beat `Array`'s own `map`,
/// which it must not.
pub fn overlay_value_body(id: ClassId, name: Symbol) -> Option<RProc> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.value_bodies.get(&name).cloned())
}

/// Run an [`overlay_value_body`] with the method frame around it.
///
/// Not just `body.call_with_self_and_block`: `dynamic_from_proc`'s wrapper is
/// what pushes `METHOD_FRAMES`, and a bare `super` inside a runtime-defined
/// method reads exactly that. Calling the proc raw made prism's
/// `Module.new { def unpack1(..) ... super ... }` raise "super called outside
/// of method" the moment String dispatch started consulting the overlay.
pub fn call_value_body(
    defining: ClassId,
    name: Symbol,
    body: &RProc,
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    push_method_frame(defining, name);
    let out = body.call_with_self_and_block(recv, args, block);
    pop_method_frame();
    out
}

/// `own_only` drops the names an `extend` copied in, which is
/// `singleton_methods(false)`'s narrowing -- see
/// [`OverlayEntry::extended_class_methods`].
/// Whether the overlay's class method `name` on `id` was copied in by an
/// `extend` rather than written on the class itself -- see
/// [`OverlayEntry::extended_class_methods`]. What tells reflection to report
/// the MODULE as the owner.
pub fn overlay_class_method_is_extended(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.extended_class_methods.contains(&name))
}

pub fn overlay_class_method_names(id: ClassId, own_only: bool) -> Vec<Symbol> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .map(|e| {
            let own = e
                .class_methods
                .keys()
                .copied()
                .filter(|n| !own_only || !e.extended_class_methods.contains(n));
            // A singleton-prepend copy dispatches on the class, so the WIDE
            // list reports it; like an extend it lives in the singleton's
            // chain, not on the class itself, so the narrow list skips it.
            match own_only {
                true => own.collect(),
                false => {
                    let mut names: Vec<Symbol> = own.collect();
                    for n in e.prepended_class_methods.keys() {
                        if !names.contains(n) {
                            names.push(*n);
                        }
                    }
                    names
                }
            }
        })
        .unwrap_or_default()
}

fn module_own_method_impl(mid: ClassId, name: Symbol) -> Option<MethodImpl> {
    overlay_own_method(mid, name)
        .or_else(|| crate::dispatch::registry_lookup_cloned(mid, name))
        .or_else(|| crate::dispatch::registry_value_method_impl(mid, name))
        .or_else(|| builtin_module_method_impl(mid, name))
}

/// The `MethodImpl` a BUILTIN module (`Comparable`/`Enumerable`/`Math`, or any
/// module with a hardcoded `class_table`) defines for `name`. These don't live
/// in the registry -- their bodies are static method-table rows -- so each is
/// wrapped in a `Dynamic` closure that re-dispatches by name. This is what lets
/// `obj.extend(Comparable)` install `clamp`/`between?` etc. (`Math`'s module
/// functions are ordinary instance-table rows now, reached the same way.)
fn builtin_module_method_impl(mid: ClassId, name: Symbol) -> Option<MethodImpl> {
    let f = crate::builtins::class_table(mid)?(&name.name())?;
    Some(MethodImpl::Dynamic(std::sync::Arc::new(
        move |recv: &RObj, args: &[RubyValue], b| f(&RubyValue::Object(recv.clone()), args, b),
    )))
}

/// `obj.singleton_class` -- the per-object singleton class as a real `Class`
/// value (minted once per object identity, cached). Defining a method on it
/// routes back to `obj`'s singleton table via the `singleton_owner` hook in
/// `runtime_define_method`; its ancestry is `[singleton, *obj.class.ancestors]`.
/// For a `Class` receiver the owner is the class itself, so defs become class
/// methods (`class << Foo` semantics).
pub fn runtime_singleton_class(recv: &RubyValue) -> Result<RubyValue, Signal> {
    // An IMMEDIATE (Integer/Float/Symbol/nil/true/false and the numeric towers)
    // has no singleton class -- CRuby raises `TypeError: can't define singleton`.
    // Object/Class get a CACHED singleton class whose method defs redirect to the
    // owner (`singleton_owner`); other heap values (String/Array/...) get a fresh
    // singleton class good for `.class`/`.superclass`/reflection (defining on one
    // isn't supported, matching this runtime's singleton-storage limits).
    // `nil`/`true`/`false` are the exception CRuby carves out: each is the sole
    // instance of its class, so its singleton class IS that class, and
    // `nil.singleton_class.equal?(NilClass)` is true. Only the numeric and
    // Symbol immediates raise.
    match recv {
        RubyValue::Nil => return Ok(RubyValue::Class(zeo_abi::NIL_CLASS)),
        RubyValue::Bool(true) => return Ok(RubyValue::Class(zeo_abi::TRUE_CLASS)),
        RubyValue::Bool(false) => return Ok(RubyValue::Class(zeo_abi::FALSE_CLASS)),
        _ => {}
    }
    if matches!(
        recv,
        RubyValue::Int(_)
            | RubyValue::BigInt(_)
            | RubyValue::Float(_)
            | RubyValue::Rational(_)
            | RubyValue::Complex(_)
            | RubyValue::Symbol(_)
    ) {
        return Err(type_error!("can't define singleton"));
    }
    let cache_key = singleton_class_key(recv);
    let real = recv.class_id();
    let owner = recv.clone();
    if let Some(k) = cache_key
        && let Some(&sid) = maps().singleton_classes.read().unwrap().get(&k)
    {
        return Ok(RubyValue::Class(sid));
    }
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    // The chain carries every module `extend` mixed in, ahead of the receiver
    // class's own -- CRuby files an extended module between the singleton and
    // the class, which is what makes `o.extend(M)` show up here.
    let mut anc = vec![new_id];
    anc.extend(singleton_super_chain(recv));
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());
    // A CLASS receiver's singleton is named after the class itself --
    // CRuby's `#<Class:Melody>`. A plain object's is named after the OBJECT,
    // in its address form (`#<Class:#<Object:0xADDR>>`, CRuby's
    // `rb_any_to_s` of the attached object -- never its class, which would
    // collapse every instance's singleton to one name).
    let singleton_name = match recv {
        RubyValue::Class(cid) => {
            let n = crate::dispatch::class_name(*cid).unwrap_or_else(|| "Object".to_string());
            format!("#<Class:{n}>")
        }
        _ => {
            let cname = crate::dispatch::class_name(real).unwrap_or_else(|| "Object".to_string());
            match value_identity(recv) {
                Some(addr) => format!("#<Class:#<{cname}:0x{addr:016x}>>"),
                None => format!("#<Class:{cname}>"),
            }
        }
    };
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                name: RwLock::new(Some(singleton_name)),
                ancestors: leaked,
                ..Default::default()
            },
        );
    }
    if let Some(k) = cache_key {
        maps().singleton_classes.write().unwrap().insert(k, new_id);
        maps()
            .singleton_owner
            .write()
            .unwrap()
            .insert(id_num, owner);
    }
    mark_singletons();
    mark_live();
    Ok(RubyValue::Class(new_id))
}

/// `SomeModule.dup`/`.clone` -- a real, independent copy: a fresh runtime
/// module id carrying its own snapshot of the original's OWN instance
/// methods. Handing the original's handle back instead is silently
/// destructive, because the copy is made in order to be EDITED. delegate.rb
/// opens with `kernel = ::Kernel.dup` and then undefines
/// `to_s`/`inspect`/`!~`/`===`/`<=>`/`hash` on it -- which, sharing one id,
/// stripped them from the real `Kernel` and from every object in the program.
///
/// Only the method table is copied. Constants and class-level ivars stay with
/// the original, and the copy is anonymous until a constant names it.
pub fn runtime_module_dup(mid: ClassId) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    // Own names only (`inherit: false`), private included: delegate.rb's
    // second pass walks `private_instance_methods` on the copy.
    // Visibility comes from the same per-filter name queries
    // `private_instance_methods` and friends answer with, so the copy reports
    // exactly what the original does. Every name is marked, including the
    // public ones: an unmarked overlay entry reads as public anyway, but a
    // mark is what survives a later `private :m` lookup on the copy alone.
    let mut methods = crate::FMap::default();
    let mut methods_vis = crate::FMap::default();
    let record =
        |name: Symbol, vis, methods: &mut crate::FMap<_, _>, vism: &mut crate::FMap<_, _>| {
            if let Some(m) = module_own_method_impl(mid, name) {
                methods.insert(name, m);
                vism.insert(name, vis);
            }
        };
    for (filter, vis) in [
        (
            crate::dispatch::VisFilter::Public,
            crate::dispatch::MethodVisibility::Public,
        ),
        (
            crate::dispatch::VisFilter::Protected,
            crate::dispatch::MethodVisibility::Protected,
        ),
        (
            crate::dispatch::VisFilter::Private,
            crate::dispatch::MethodVisibility::Private,
        ),
    ] {
        for name in crate::dispatch::instance_method_names(mid, filter, false) {
            record(name, vis, &mut methods, &mut methods_vis);
        }
    }
    // A builtin module's rows are not in the name queries above (`Kernel`'s
    // table is the whole of it), and they are public unless the runtime says
    // otherwise.
    for n in crate::builtins::class_table_names(mid) {
        let sym = Symbol::intern(n);
        if !methods.contains_key(&sym) {
            let vis = crate::dispatch::instance_method_visibility(mid, sym)
                .unwrap_or(crate::dispatch::MethodVisibility::Public);
            record(sym, vis, &mut methods, &mut methods_vis);
        }
    }
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                is_module: true,
                ancestors: leaked,
                methods,
                methods_vis,
                ..Default::default()
            },
        );
    }
    mark_live();
    Ok(RubyValue::Class(new_id))
}

/// `Module.new { body }` -- allocate a runtime MODULE id and run the optional
/// body block with `self` bound to it.
///
/// A module's ancestors are just itself. It has no superclass and no
/// constructor, so `Module.new.new` raises NoMethodError. The result composes
/// with `obj.extend`/`include`: its `define_method`-installed methods are
/// retrievable by id from the overlay.
pub fn runtime_module_new(body: Option<RProc>) -> Result<RubyValue, Signal> {
    runtime_module_new_owned(body, None)
}

/// The class a minted module is an INSTANCE of -- `Some(X)` only for one
/// `X.new` where `class X < Module`. `None` for every ordinary `Module.new`
/// and for every frozen id, so the hot `.class` path pays one overlay probe
/// and only once anything has been minted at run time at all.
///
/// This is the whole of the "class-valued instance": the value is still a real
/// module id, so `include`, `Module#===`, `ancestors` and constant lookup are
/// untouched. Only its class differs.
pub fn module_owner_class(id: ClassId) -> Option<ClassId> {
    if !is_live() {
        return None;
    }
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.owner_class)
}

/// Register a user `class X < Module`: name + linearized ancestors + the
/// shared [`module_subclass_construct`]. Like the other struct-less shapes it
/// installs no methods -- `X`'s own `def`s register as `RubyValue`-self
/// `define_method` deltas (`Compiler::is_native_backed` covers it).
pub fn register_module_subclass(
    registry: &mut crate::dispatch::ClassRegistry,
    id: ClassId,
    name: &str,
    ancestors: Vec<ClassId>,
) {
    registry.register(
        id,
        name,
        false,
        ancestors,
        Some(module_subclass_construct as crate::dispatch::ConstructorFn),
    );
}

/// Whether the module subclass `owner` (or an ancestor of it BELOW `Module`)
/// defines `name` as one of its own instance methods.
///
/// `value_method`, not `has_instance_method`: a module subclass's methods take
/// a `RubyValue::Class` self, so they register into `value_methods`, which the
/// `methods` table `has_instance_method` reads never sees. Stopping at `Module`
/// is what keeps `Module`'s own defaults out -- they are not what the user
/// wrote, and running them here would be wrong for both callers.
pub(crate) fn module_subclass_defines(owner: ClassId, name: Symbol) -> bool {
    crate::dispatch::ancestors_of_value(owner)
        .iter()
        .take_while(|&&a| a != zeo_abi::MODULE_CLASS)
        .any(|&a| crate::dispatch::value_method(a, 0, name).is_some())
}

/// The `ConstructorFn` behind every `class X < Module`. Mints a real runtime
/// module tagged as an instance of `X`, then runs `X`'s own `initialize`
/// against it with the MODULE as `self` -- which is what Rails'
/// `DeprecatedConstantProxy#initialize` expects when it stores `@old_const`.
///
/// The result is a `RubyValue::Class`, so `include X.new(...)` reaches
/// `runtime_include` unchanged and the user's `included` hook fires from
/// `fire_mixin_hook` like any module's.
pub fn module_subclass_construct(
    class_id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let val = runtime_module_new_owned(None, Some(class_id))?;
    let init = Symbol::intern("initialize");
    // A USER `initialize` only: `Module`'s own takes no arguments and would
    // raise on `X.new(attrs)`.
    if module_subclass_defines(class_id, init) {
        crate::dispatch::send_value_in(0, &val, init, args, block)?;
    }
    Ok(val)
}

/// [`runtime_module_new`] with the minted module tagged as an instance of
/// `owner` -- what `X.new` runs for a `class X < Module`.
pub fn runtime_module_new_owned(
    body: Option<RProc>,
    owner: Option<ClassId>,
) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                is_module: true,
                ancestors: leaked,
                owner_class: owner,
                ..Default::default()
            },
        );
    }
    mark_live();
    let val = RubyValue::Class(new_id);
    if let Some(b) = body {
        // `Module.new` reaches its body through `Class#new` -> `Module#initialize`
        // -- the allocator is Class's, the initializer Module's. See
        // `runtime_class_new` for the pair it mirrors.
        let _new = crate::frames::synthetic_c_frame("Class#new");
        let _init = crate::frames::synthetic_c_frame("Module#initialize");
        with_body_frame(new_id, || b.call_with_self(&val, &[]))?;
    }
    Ok(val)
}

/// A RUNTIME `refine(target) { body }` (Module's private `refine`, reached
/// from a `Module.new` body or any other runtime module context): mints the
/// holder module, marks it a refinement of `(module, target)`, and runs the
/// body with the holder as self and definee -- so its `def`s land on the
/// holder, exactly like a module body. Definition only: `Module#refinements`
/// reports it and nothing changes dispatch until a `using`.
pub fn runtime_refine(
    module: ClassId,
    target: ClassId,
    body: &crate::RProc,
) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                is_module: true,
                ancestors: leaked,
                refinement_of: Some((module, target)),
                ..Default::default()
            },
        );
    }
    mark_live();
    let val = RubyValue::Class(new_id);
    with_body_frame(new_id, || body.call_with_self(&val, &[]))?;
    Ok(val)
}

/// `Refinement#import_methods(*modules)` -- CRuby copies each module's OWN
/// method entries into the refinement, re-compiled under the refinement's
/// cref (eval.c:1863), refusing methods not defined in Ruby. zeo's bodies
/// are Rust fns with no cref to re-bind, so the copy is the resolved body
/// itself and every method qualifies; the module doc records that copied
/// bodies do not see the refinement's own refinements. Ancestors are NOT
/// imported, with CRuby's warning.
pub fn refinement_import_methods(
    holder: &RubyValue,
    modules: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(hid) = holder else {
        return Err(runtime_error!("import_methods on a non-module refinement"));
    };
    // Every argument is validated BEFORE anything imports (CRuby's shape).
    for m in modules {
        let ok = matches!(m, RubyValue::Class(id)
            if crate::dispatch::class_is_module(*id) == Some(true));
        if !ok {
            return Err(crate::builtins::type_error!(
                "wrong argument type {} (expected Module)",
                crate::builtins::class_name_of(m)
            ));
        }
    }
    for m in modules {
        let RubyValue::Class(mid) = m else {
            unreachable!()
        };
        if ancestors_of_value(*mid).len() > 1
            && let Some((file, line)) = crate::frames::current_location()
        {
            eprintln!(
                "{file}:{line}: warning: {} has ancestors, but Refinement#import_methods doesn't import their methods",
                m.try_display_string()?
            );
        }
        let private: std::collections::HashSet<Symbol> = crate::dispatch::instance_method_names(
            *mid,
            crate::dispatch::VisFilter::Private,
            false,
        )
        .into_iter()
        .collect();
        let protected: std::collections::HashSet<Symbol> = crate::dispatch::instance_method_names(
            *mid,
            crate::dispatch::VisFilter::Protected,
            false,
        )
        .into_iter()
        .collect();
        for name in
            crate::dispatch::instance_method_names(*mid, crate::dispatch::VisFilter::All, false)
        {
            let Some(body) = snapshot_instance_method(*mid, name) else {
                continue;
            };
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(hid.0).or_insert_with(OverlayEntry::delta);
            e.methods.insert(name, body);
            e.undefs.remove(&name);
            if private.contains(&name) {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Private);
            } else if protected.contains(&name) {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Protected);
            } else {
                e.methods_vis.remove(&name);
            }
        }
        patch_class(*hid);
    }
    mark_live();
    Ok(holder.clone())
}

/// The overlay twin of `dispatch::refinement_of` -- `(refining module,
/// refined target)` for a holder a runtime `refine` minted.
pub fn overlay_refinement_of(id: ClassId) -> Option<(ClassId, ClassId)> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.refinement_of)
}

/// The holders a runtime `refine` minted for `module`, in declaration order
/// (ids are handed out sequentially) -- `Module#refinements`' overlay half.
pub fn overlay_refinements_of(module: ClassId) -> Vec<ClassId> {
    let mut holders: Vec<ClassId> = maps()
        .classes
        .read()
        .unwrap()
        .iter()
        .filter(|(_, e)| e.refinement_of.is_some_and(|(m, _)| m == module))
        .map(|(&id, _)| ClassId(id))
        .collect();
    holders.sort_by_key(|c| c.0);
    holders
}

/// `Class.new(superclass) { body }` -- allocate a runtime class id, register
/// its overlay entry (ancestors linearized from the superclass, a generic
/// name-keyed-object constructor), then run the body block with `self` bound to
/// the new class so `define_method`/`include`/const-assign inside populate it.
pub fn runtime_class_new(
    superclass: Option<RubyValue>,
    body: Option<RProc>,
) -> Result<RubyValue, Signal> {
    let super_id = match &superclass {
        None => ClassId(0), // default super is Object
        Some(RubyValue::Class(cid)) => *cid,
        Some(_) => {
            return Err(type_error!("superclass must be a Class"));
        }
    };

    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);

    // ancestors = [self, *superclass.ancestors], leaked to 'static.
    let super_chain = ancestors_of_value(super_id);
    let mut anc = Vec::with_capacity(super_chain.len() + 1);
    anc.push(new_id);
    anc.extend_from_slice(super_chain);
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());

    // `Class.new(String)` needs the payload-carrying instance the value bridge
    // dispatches against, not a name-keyed `DynObject` -- exactly as a compiled
    // `class Tag < String` registers. Read off `leaked` rather than through
    // `value_root_of`, which resolves the ancestry via an overlay this entry
    // isn't in yet.
    // An EXCEPTION subclass needs the native `RubyException` allocator for the
    // same reason: every `Exception` method it inherits reads that payload, so a
    // name-keyed `DynObject` would satisfy `is_a?` and then fail on `#message`.
    let constructor = if leaked
        .iter()
        .copied()
        .any(crate::builtins::value_subclass::is_payload_root)
    {
        crate::builtins::value_subclass::value_subclass_construct
    } else if leaked.contains(&zeo_abi::EXCEPTION_CLASS) {
        crate::builtins::exception::exception_construct
    } else if leaked
        .iter()
        .any(|&a| crate::dispatch::registry_allocator(a).is_some())
    {
        // `Class.new(CompiledBase)`: instances must be the compiled
        // ancestor's real struct (stamped with the runtime id) or every
        // inherited compiled method's downcast aborts.
        compiled_subclass_construct
    } else {
        dyn_object_construct
    };
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                ancestors: leaked,
                constructor: Some(constructor),
                ..Default::default()
            },
        );
    }
    mark_live();

    let class_val = RubyValue::Class(new_id);
    // CRuby fires `inherited` on the superclass at creation -- before the
    // body block runs and before any constant names the class (the hook sees
    // `name == nil`). minitest's whole Runnable registry IS this hook, fired
    // by every `describe` block's `Class.new(Minitest::Spec)`. The default
    // `Class#inherited` is a no-op row, so an unconditional send is safe.
    crate::dispatch::send_value(
        &RubyValue::Class(super_id),
        Symbol::intern("inherited"),
        std::slice::from_ref(&class_val),
        None,
    )?;
    if let Some(b) = body {
        // The body runs two C frames deep in CRuby (`Class.new` calls
        // `Class#initialize`, which yields), and a raise from inside it shows
        // both -- so a backtrace here has to as well.
        let _new = crate::frames::synthetic_c_frame("Class#new");
        let _init = crate::frames::synthetic_c_frame("Class#initialize");
        with_body_frame(new_id, || b.call_with_self(&class_val, &[]))?;
    }
    Ok(class_val)
}

/// `Class.allocate` -- a class with NO superclass, because `Class#initialize`
/// is what installs one and it has not run. Its ancestry is just itself, so it
/// inherits nothing, answers no instance method, and cannot instantiate.
///
/// Only `Marshal` and the reflection corners reach this. It exists so
/// `Class.allocate` answers the object ruby answers rather than the TypeError
/// the generic `Class#allocate` raises for a class with no allocator.
pub fn runtime_class_allocate() -> RubyValue {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                ancestors: leaked,
                uninitialized: true,
                ..Default::default()
            },
        );
    }
    mark_live();
    RubyValue::Class(new_id)
}

/// Whether `id` is a class `Class.allocate` handed out and nothing initialized.
pub fn class_is_uninitialized(id: ClassId) -> bool {
    is_live()
        && maps()
            .classes
            .read()
            .unwrap()
            .get(&id.0)
            .is_some_and(|e| e.uninitialized)
}

/// Mint a fresh runtime class rooted at `root` (e.g. `STRUCT_CLASS`/
/// `DATA_CLASS`), pre-populated with `methods` and a native `constructor` --
/// the counterpart of `runtime_class_new` for a native-backed class the user
/// never wrote a `class` body for (Batch E's `Struct.new`/`Data.define`). The
/// ancestry is `[new_id, *root.ancestors]`, leaked to `'static` like every
/// other runtime class's.
pub fn intern_native_class(
    root: ClassId,
    methods: FMap<Symbol, MethodImpl>,
    constructor: ConstructorFn,
) -> ClassId {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let super_chain = ancestors_of_value(root);
    let mut anc = Vec::with_capacity(super_chain.len() + 1);
    anc.push(new_id);
    anc.extend_from_slice(super_chain);
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                ancestors: leaked,
                methods,
                constructor: Some(constructor),
                ..Default::default()
            },
        );
    }
    mark_live();
    new_id
}

/// Name a runtime class the first time it's assigned to a constant
/// (`Foo = Class.new`). A no-op for a frozen id or an already-named runtime
/// class (CRuby names on FIRST binding only).
pub fn name_runtime_class_if_anonymous(id: ClassId, name: &str) {
    if id.0 < RUNTIME_CLASS_ID_BASE || !is_live() {
        return;
    }
    if let Some(entry) = maps().classes.read().unwrap().get(&id.0) {
        let mut slot = entry.name.write().unwrap();
        if slot.is_none() {
            *slot = Some(name.to_string());
            // A named class is reachable as a nested constant of its namespace
            // (`constants::nested_class_of` asks the registry, not the table),
            // so naming one changes what a constant lookup can find.
            crate::constants::bump_const_epoch();
        }
    }
}

/// The runtime classes whose current name came from `set_temporary_name`
/// rather than from a constant binding. CRuby's rule is that only a name
/// reachable by a CONSTANT PATH is permanent, and a temporary one may be
/// replaced or cleared; nothing else in this runtime needs to tell the two
/// apart, so the distinction lives in this side set rather than in the entry.
static TEMPORARY_NAMES: std::sync::LazyLock<parking_lot::Mutex<crate::FSet<u32>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(crate::FSet::default()));

/// `Module#set_temporary_name`'s storage half. `false` when the class already
/// carries a PERMANENT name (every compile-time class, and any runtime class
/// already bound to a constant), which the caller turns into CRuby's
/// `RuntimeError: can't change permanent name`. `None` clears the name, making
/// the class anonymous again.
pub fn set_temporary_class_name(id: ClassId, name: Option<String>) -> bool {
    if id.0 < RUNTIME_CLASS_ID_BASE || !is_live() {
        return false;
    }
    let classes = maps().classes.read().unwrap();
    let Some(entry) = classes.get(&id.0) else {
        return false;
    };
    let mut temporary = TEMPORARY_NAMES.lock();
    let mut slot = entry.name.write().unwrap();
    if slot.is_some() && !temporary.contains(&id.0) {
        return false;
    }
    match &name {
        Some(_) => temporary.insert(id.0),
        None => temporary.remove(&id.0),
    };
    *slot = name;
    drop(slot);
    drop(temporary);
    drop(classes);
    crate::constants::bump_const_epoch();
    true
}

// ---------------------------------------------------------------------------
// Resolver support (called from dispatch.rs, always behind `is_live()`)
// ---------------------------------------------------------------------------

/// Resolve an instance method for a `send_in` receiver through the overlay:
/// a per-object singleton first, then (for a runtime class) an ancestor walk
/// that consults both overlay entries and the frozen registry, else (for a
/// frozen class) just this class's runtime method-delta. `None` falls back to
/// `send_in`'s existing frozen fast path + builtin MRO walk.
pub fn resolve_dynamic(recv: &RObj, id: ClassId, name: Symbol) -> Option<MethodImpl> {
    // 1. Per-object singleton (identity-keyed).
    {
        let s = maps().singletons.read().unwrap();
        if !s.is_empty()
            && let Some(m) = s.get(&obj_identity(recv)).and_then(|t| t.get(&name))
        {
            return Some(m.clone());
        }
    }
    if id.0 >= RUNTIME_CLASS_ID_BASE {
        // 2a. Runtime class: walk its ancestors (overlay methods, then frozen
        // materialized methods on a frozen ancestor).
        walk_runtime_class(id, name)
    } else {
        // 2b. Frozen class: check this class AND every frozen ancestor for a
        // runtime method delta, so a `class_eval`/`define_method`/`class_exec`
        // that reopened a SUPERCLASS or an included MODULE is inherited (a
        // subclass instance / an includer sees it). The frozen registry's own
        // materialized methods are consulted separately by `send_in`'s MRO
        // walk; here we only add the overlay deltas, self-first.
        let chain = ancestors_of_value(id);
        let c = maps().classes.read().unwrap();
        chain.iter().find_map(|anc| {
            c.get(&anc.0)
                .and_then(|e| e.prepended.get(&name).or_else(|| e.methods.get(&name)))
                .cloned()
        })
    }
}

/// The instance-method resolution for a RUNTIME class id: overlay-defined
/// methods per ancestor, then that ancestor's frozen materialized method.
fn walk_runtime_class(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    let chain: &'static [ClassId] = {
        let c = maps().classes.read().unwrap();
        c.get(&id.0)?.ancestors
    };
    for &anc in chain {
        {
            let c = maps().classes.read().unwrap();
            let entry = c.get(&anc.0);
            // An undef here terminates the walk -- an ancestor's still-live
            // definition must not answer past it.
            if entry.is_some_and(|e| e.undefs.contains(&name)) {
                return None;
            }
            if let Some(m) = entry.and_then(|e| {
                e.prepended
                    .get(&name)
                    .or_else(|| e.methods.get(&name))
                    .cloned()
            }) {
                return Some(m);
            }
        }
        if let Some(m) = registry_lookup_cloned(anc, name) {
            return Some(m);
        }
        // ...then what this ancestor DEFINES, which is a different question for
        // a module. A module's bodies are never in the flat table above:
        // materialization copies them onto each compile-time includer, and a
        // class that included or prepended the module at RUNTIME has no copy.
        // They survive in two places, and `super`'s own walk probes both --
        // `own_impls` for a class's super-reachable bridge, and a VALUE METHOD
        // on the module's own id, which is where `emit_user_module_bridges`
        // puts every module instance method.
        //
        // Missing the second is what made a prepended module correctly first in
        // `ancestors` and yet invisible to dispatch: the walk reached its
        // position, found three empty tables, and carried on to the class.
        if let Some(m) = crate::dispatch::registry_own_impl_cloned(anc, name) {
            return Some(m);
        }
        if let Some(m) = crate::dispatch::registry_value_method_impl(anc, name) {
            return Some(m);
        }
    }
    None
}

/// [`walk_runtime_class`] for callers outside this module: the instance method a
/// RUNTIME class id answers `name` with. The frozen registry cannot resolve one
/// of these ids at all -- it holds no entry, so not even the ancestor walk finds
/// the chain -- which is why a caller that starts from a class rather than a
/// receiver has to ask here as well.
pub(crate) fn runtime_class_method(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    walk_runtime_class(id, name)
}

/// A class-level method (`def self.x` / `define_singleton_method` on a class)
/// for a `RubyValue::Class` receiver -- the raw block, invoked under the class.
/// A `singleton_class.prepend(M)` copy outranks everything else, ruby's own
/// layering (see [`OverlayEntry::prepended_class_methods`]).
pub fn overlay_class_method(id: ClassId, name: Symbol) -> Option<RProc> {
    let c = maps().classes.read().unwrap();
    let e = c.get(&id.0)?;
    e.prepended_class_methods
        .get(&name)
        .or_else(|| e.class_methods.get(&name))
        .cloned()
}

/// [`overlay_class_method`] with the singleton-PREPEND layer skipped -- the
/// resume point for a prepended module method's own `super`, which must reach
/// the shadowed `def self.x` rather than the copy of itself.
pub fn overlay_class_method_below_prepends(id: ClassId, name: Symbol) -> Option<RProc> {
    let c = maps().classes.read().unwrap();
    c.get(&id.0)?.class_methods.get(&name).cloned()
}

/// The next `name` in `id`'s singleton-PREPEND stack STRICTLY BELOW `mid` --
/// an EARLIER prepend, which later ones outrank -- or `None` when only the
/// host's own `def self.x` is left. The resume point for a prepended module
/// method's `super` when more than one module prepends the same name.
pub fn singleton_prepend_super_below(id: ClassId, mid: ClassId, name: Symbol) -> Option<RProc> {
    let below: Vec<ClassId> = {
        let c = maps().classes.read().unwrap();
        let stack = &c.get(&id.0)?.singleton_prepends;
        let pos = stack.iter().position(|&m| m == mid)?;
        stack[..pos].iter().rev().copied().collect()
    };
    below
        .into_iter()
        .find_map(|m| extended_class_method(m, name))
}

/// Whether `mid` was prepended into `id`'s singleton class
/// (`id.singleton_class.prepend(mid)`) -- how `send_super_class_from` learns
/// that a `defining_class` missing from the ancestry sits in the prepend
/// layer, whose `super` resumes AT `id` rather than past it.
pub fn has_singleton_prepend(id: ClassId, mid: ClassId) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.singleton_prepends.contains(&mid))
}

/// The instance method THIS class's OWN overlay delta defines -- no ancestor
/// walk (unlike `resolve_dynamic`), so it is safe to call from the
/// reentrancy-sensitive `call_user_method` (which needs only "does THIS class
/// define the method itself"). Lets a runtime-defined `inspect`/`to_s`/`hash`
/// -- e.g. a native `Struct`/`Data` class's -- be honoured by `p`/string
/// interpolation, not just by `send`.
pub fn overlay_own_method(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    let c = maps().classes.read().unwrap();
    c.get(&id.0)?.methods.get(&name).cloned()
}

/// Record which module supplied each name a per-object `extend` just copied
/// -- see `OverlayMaps::extended_names`.
fn record_extended_names(key: usize, mid: ClassId, names: &[Symbol]) {
    if names.is_empty() {
        return;
    }
    let mut w = maps().extended_names.write().unwrap();
    let table = w.entry(key).or_default();
    for &name in names {
        table.insert(name, mid);
    }
}

/// Drop the extended-module record for one name -- every OWN definition (and
/// undef) of a per-object singleton owns the name from then on.
fn clear_extended_name(key: usize, name: Symbol) {
    if let Some(t) = maps().extended_names.write().unwrap().get_mut(&key) {
        t.remove(&name);
    }
}

/// The class a PER-OBJECT singleton method is rooted at for reflection --
/// `obj.method(:x)`'s home: the module a per-object `extend` copied it from,
/// or the object's own singleton class for an own `def obj.x` (minted on
/// demand, as CRuby's `.owner` observably does).
pub fn per_object_method_home(recv: &RubyValue, name: Symbol) -> Option<ClassId> {
    let key = value_identity(recv)?;
    let recorded = maps()
        .extended_names
        .read()
        .unwrap()
        .get(&key)
        .and_then(|t| t.get(&name).copied());
    if let Some(mid) = recorded {
        return Some(mid);
    }
    match runtime_singleton_class(recv) {
        Ok(RubyValue::Class(sid)) => Some(sid),
        _ => None,
    }
}

/// Whether `recv` (an object) has a per-object singleton method `name` --
/// `respond_to?`'s identity-keyed probe, since the class-id walk can't see a
/// singleton installed on one specific object.
pub fn object_has_singleton_method(recv: &RubyValue, name: Symbol) -> bool {
    let Some(key) = value_identity(recv) else {
        return false;
    };
    let by_object = maps()
        .singletons
        .read()
        .unwrap()
        .get(&key)
        .is_some_and(|t| t.contains_key(&name));
    by_object
        || maps()
            .value_singletons
            .read()
            .unwrap()
            .get(&key)
            .is_some_and(|t| t.contains_key(&name))
}

/// Whether class `id`'s OWN overlay table defines instance method `name` -- a
/// runtime `define_method` delta on a frozen class, or a runtime class's own
/// method. `respond_to?`'s ancestor walk consults this per ancestor.
pub fn overlay_has_instance_method(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.methods.contains_key(&name) || e.prepended.contains_key(&name))
}

/// A runtime class's leaked ancestor chain -- `None` for a frozen id or a pure
/// method-delta (empty ancestors). Feeds `ancestors_of_value`.
pub fn overlay_ancestors(id: ClassId) -> Option<&'static [ClassId]> {
    let c = maps().classes.read().unwrap();
    let anc = c.get(&id.0)?.ancestors;
    if anc.is_empty() { None } else { Some(anc) }
}

/// A runtime class's Ruby-visible name (or the anonymous `#<Class:ID>` form).
pub fn overlay_class_name(id: ClassId) -> Option<String> {
    let c = maps().classes.read().unwrap();
    let entry = c.get(&id.0)?;
    if entry.ancestors.is_empty() {
        return None; // a pure delta over a frozen class carries no name of its own
    }
    if let Some(name) = entry.name.read().unwrap().clone() {
        return Some(name);
    }
    // An anonymous value renders as `#<ITS CLASS:0xADDR>`, so a module a
    // `class X < Module` minted reads `#<X:0x...>` -- CRuby's rule, and the
    // reason the owner is consulted before the plain Class/Module split.
    let kind = match entry.owner_class.and_then(crate::dispatch::class_name) {
        Some(owner) => owner,
        None if entry.is_module => "Module".to_string(),
        None => "Class".to_string(),
    };
    Some(format!("#<{kind}:0x{:016x}>", entry.addr))
}

/// Whether class `id`'s OWN entry undef'd `name` -- the terminator every MRO
/// walk probes per ancestor, alongside `ClassRegistry::is_undefined`.
pub fn overlay_is_undefined(id: ClassId, name: Symbol) -> bool {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.undefs.contains(&name))
}

/// Reverse of [`overlay_class_name`]: the runtime class id whose Ruby-visible
/// name is `name` (so `Marshal.load` can resolve a `Struct.new`-minted or
/// otherwise runtime-defined class back to its id). Only real, named classes
/// match -- anonymous ids and pure frozen-class deltas never do.
pub fn runtime_class_id_by_name(name: &str) -> Option<ClassId> {
    if !is_live() {
        return None;
    }
    let c = maps().classes.read().unwrap();
    c.iter().find_map(|(&id, entry)| {
        if entry.ancestors.is_empty() {
            return None;
        }
        (entry.name.read().unwrap().as_deref() == Some(name)).then_some(ClassId(id))
    })
}

/// Whether a runtime id names a module (always `false` -- `Class.new` makes a
/// class). `None` for a frozen id / pure delta.
pub fn overlay_is_module(id: ClassId) -> Option<bool> {
    let c = maps().classes.read().unwrap();
    let entry = c.get(&id.0)?;
    if entry.ancestors.is_empty() {
        None
    } else {
        Some(entry.is_module)
    }
}

/// A runtime class's constructor -- feeds `constructor_of` so `RuntimeClass.new`
/// works through the same `construct_by_class_id` path frozen classes use.
pub fn overlay_constructor(id: ClassId) -> Option<ConstructorFn> {
    let c = maps().classes.read().unwrap();
    c.get(&id.0)?.constructor
}

/// A fresh uninitialized instance of a runtime (`Class.new`) class, backing
/// `Class#allocate` -- a name-keyed `DynObject` with no `initialize` run.
/// `None` if `id` is not a known runtime class.
pub fn runtime_allocate(id: ClassId) -> Option<RubyValue> {
    let known = maps().classes.read().unwrap().contains_key(&id.0);
    known.then(|| {
        // Mirror of `runtime_class_new`'s constructor pick: a runtime
        // subclass of a compiled class allocates the ancestor's struct under
        // its own id.
        match crate::dispatch::ancestor_allocator_of(id) {
            Some(alloc) => RubyValue::Object(alloc(id)),
            None => RubyValue::Object(Arc::new(DynObject::new(id))),
        }
    })
}

/// Coerce a `define_method`/`define_singleton_method` NAME argument (a Symbol
/// or String) to a `Symbol` -- CRuby's `rb_to_id`.
pub(crate) fn coerce_method_name(arg: Option<&RubyValue>) -> Result<Symbol, Signal> {
    match arg {
        Some(RubyValue::Symbol(s)) => Ok(*s),
        Some(RubyValue::Str(s)) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        _ => Err(type_error!(
            "expected a Symbol or String for the method name"
        )),
    }
}

/// The block that becomes the method body: the passed block, or a `Proc` given
/// as the second positional argument (`define_method(:x, some_proc)`). A
/// `Method`/`UnboundMethod` second argument is a documented fast-follow.
pub(crate) fn coerce_method_body(
    body: Option<&RubyValue>,
    block: &Option<RubyValue>,
) -> Result<RProc, Signal> {
    if let Some(RubyValue::Proc(p)) = block {
        return Ok(p.clone());
    }
    if let Some(RubyValue::Proc(p)) = body {
        return Ok(p.clone());
    }
    Err(arg_error!("tried to create Proc object without a block"))
}

fn immediate_kind(v: &RubyValue) -> &'static str {
    match v {
        RubyValue::Nil => "nil",
        RubyValue::Bool(true) => "true",
        RubyValue::Bool(false) => "false",
        RubyValue::Int(_) | RubyValue::BigInt(_) => "Integer",
        RubyValue::Float(_) => "Float",
        RubyValue::Symbol(_) => "Symbol",
        _ => "this value",
    }
}

// ---------------------------------------------------------------------------
// DynObject: the instance type of a runtime (Class.new) class
// ---------------------------------------------------------------------------

/// The generic constructor every runtime class registers: allocate a
/// name-keyed object tagged with the class id, then run the class's own
/// `initialize` if it defines one (mirroring `Class#new`). No user
/// `initialize` + extra args is CRuby's `ArgumentError`.
/// `Class.new(CompiledBase)`'s constructor: allocate through the nearest
/// compiled ancestor's `AllocatorFn`, stamped with the RUNTIME class id, so
/// every inherited compiled method's trampoline downcasts to the real
/// ancestor struct instead of aborting on a `DynObject`. Named ivars a
/// runtime body invents spill into `IvarCell`'s invented storage.
/// `initialize` resolves through full dispatch, so a runtime-defined body
/// wins over the inherited compiled one.
fn compiled_subclass_construct(
    id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(alloc) = crate::dispatch::ancestor_allocator_of(id) else {
        return dyn_object_construct(id, args, block);
    };
    let handle = alloc(id);
    crate::dispatch::run_initialize(id, &handle, args, block)?;
    Ok(RubyValue::Object(handle))
}

fn dyn_object_construct(
    id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let obj: RObj = Arc::new(DynObject::new(id));
    let init = Symbol::intern("initialize");
    if walk_runtime_class(id, init).is_some() {
        crate::dispatch::send_in(0, &obj, init, args, block)?;
    } else if !args.is_empty() {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 0)",
            args.len()
        ));
    }
    Ok(RubyValue::Object(obj))
}

/// A blank name-keyed instance tagged with `id` -- what `BasicObject.new`
/// answers (the root class is instantiable in real Ruby, and its instance is
/// exactly the blank object `Object.new` builds, under its own id).
pub(crate) fn blank_instance(id: ClassId) -> RubyValue {
    RubyValue::Object(Arc::new(DynObject::new(id)))
}

/// An instance of a runtime-created class. Like the root `Object`, its ivars
/// are name-keyed (a runtime class has no compile-time-materialized field
/// list), plus a stored `class_id` (unlike `Object`'s hardcoded `0`) and a
/// per-object frozen flag.
/// An `RObj` shell standing in for a CLASS as the receiver of an `extend`ed
/// user module's compiled method (see `extended_class_method`). It owns no
/// storage of its own: every ivar operation reads and writes the class's own
/// class-level store, so `@x` means the same slot inside the module's body as
/// it does in a `def self.x` or an `instance_variable_get` on the class.
struct ClassSurrogate {
    class_id: ClassId,
}

impl RubyObject for ClassSurrogate {
    fn class_id(&self) -> ClassId {
        self.class_id
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        crate::dispatch::class_frozen(self.class_id)
    }
    // A class freezes through `Module#freeze`, which the registry records --
    // there is nothing for this shell to mark.
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        crate::civars::class_ivar_names(self.class_id.0)
            .iter()
            .map(|n| crate::civars::class_ivar_get(self.class_id.0, n))
            .collect()
    }
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        crate::civars::class_ivar_names(self.class_id.0)
            .iter()
            .map(|n| {
                (
                    format!("@{n}"),
                    crate::civars::class_ivar_get(self.class_id.0, n),
                )
            })
            .collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        Some(crate::civars::class_ivar_get(self.class_id.0, name))
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        crate::civars::class_ivar_set(self.class_id.0, name, v).is_ok()
    }
    fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
        // `civars` has no remove; answering the current value and clearing it
        // to nil is the closest this store offers.
        let old = crate::civars::class_ivar_get(self.class_id.0, name);
        crate::civars::class_ivar_set(self.class_id.0, name, RubyValue::Nil).ok()?;
        Some(old)
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(ClassSurrogate {
            class_id: self.class_id,
        })
    }
}

struct DynObject {
    /// Atomic so a `Ractor` move can RETAG the instance to
    /// `Ractor::MovedObject` in place (`retag_moved`); plain relaxed loads
    /// everywhere else.
    class_id: AtomicU32,
    frozen: AtomicBool,
    /// Insertion-ordered: ruby reports instance variables in FIRST-ASSIGNMENT
    /// order, and that order is a property of the object rather than of its
    /// class -- an ivar added after construction appears last. A compiled
    /// class gets a generated struct whose fields are already in source order;
    /// this is the same guarantee for a class minted at run time. It is what
    /// makes `p obj` stable enough to diff, which is most of what `inspect` is
    /// for.
    ivars: parking_lot::Mutex<indexmap::IndexMap<String, RubyValue>>,
}

impl DynObject {
    fn new(class_id: ClassId) -> DynObject {
        DynObject {
            class_id: AtomicU32::new(class_id.0),
            frozen: AtomicBool::new(false),
            ivars: parking_lot::Mutex::new(indexmap::IndexMap::new()),
        }
    }
}

impl RubyObject for DynObject {
    fn class_id(&self) -> ClassId {
        ClassId(self.class_id.load(Ordering::Relaxed))
    }
    fn retag_moved(&self) -> bool {
        self.class_id
            .store(zeo_abi::RACTOR_MOVED_OBJECT_CLASS.0, Ordering::Relaxed);
        true
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        self.ivars.lock().values().cloned().collect()
    }
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        // Keys are stored without the leading `@` (see `ivar_set_named`); pair
        // in one lock so name/value order can't diverge.
        self.ivars
            .lock()
            .iter()
            .map(|(k, v)| (format!("@{k}"), v.clone()))
            .collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        // A never-assigned ivar reads nil (Ruby's rule), so `Some(Nil)` not
        // `None` -- a name-keyed object always "has" the slot.
        Some(
            self.ivars
                .lock()
                .get(name)
                .cloned()
                .unwrap_or(RubyValue::Nil),
        )
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        self.ivars.lock().insert(name.to_string(), v);
        true
    }
    fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
        // Name-keyed: a missing key is genuinely absent (CRuby's `NameError`).
        // `shift_remove`, not `remove`: the latter swaps the last entry into
        // the hole, which would reorder the survivors.
        self.ivars.lock().shift_remove(name)
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = DynObject::new(RubyObject::class_id(self));
        *d.ivars.lock() = self.ivars.lock().clone();
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nullary(ret: i64) -> RProc {
        RProc::with_meta(move |_args| Ok(RubyValue::Int(ret)), 0, false)
    }

    fn int_of(v: RubyValue) -> i64 {
        match v {
            RubyValue::Int(n) => n,
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn define_method_over_frozen_id_is_found_and_overrides() {
        // A runtime delta on a (pretend) frozen id resolves via resolve_dynamic.
        let id = ClassId(7); // a low, "frozen-range" id
        let name = Symbol::intern("answer");
        runtime_define_method(id, name, nullary(42)).unwrap();
        assert!(is_live());

        // A DynObject standing in as a receiver of that class.
        let recv: RObj = Arc::new(DynObject::new(id));
        let m = resolve_dynamic(&recv, id, name).expect("overlay delta resolves");
        assert_eq!(int_of(m.call(&recv, &[], None).unwrap()), 42);
    }

    #[test]
    fn per_object_singleton_is_identity_keyed() {
        let a: RObj = Arc::new(DynObject::new(ClassId(0)));
        let b: RObj = Arc::new(DynObject::new(ClassId(0)));
        let name = Symbol::intern("only_a");
        runtime_define_singleton_method(&RubyValue::Object(a.clone()), name, nullary(1)).unwrap();

        assert!(resolve_dynamic(&a, ClassId(0), name).is_some());
        // b is a different allocation -> no singleton.
        assert!(resolve_dynamic(&b, ClassId(0), name).is_none());
    }

    #[test]
    #[should_panic(expected = "TypeError")]
    fn singleton_on_immediate_is_type_error() {
        // `raise_error` returns a rescuable `Signal::Raise` in a real program,
        // but panics loudly registry-less (this crate's unit context) -- so the
        // TypeError manifests here as that panic. e2e (F3) checks the rescue.
        let name = Symbol::intern("nope");
        let _ = runtime_define_singleton_method(&RubyValue::Int(3), name, nullary(0));
    }

    #[test]
    fn runtime_class_inherits_from_another_runtime_class() {
        // parent defines #greet; child < parent inherits it.
        let parent = runtime_class_new(None, None).unwrap();
        let RubyValue::Class(parent_id) = parent else {
            panic!()
        };
        let greet = Symbol::intern("greet");
        runtime_define_method(parent_id, greet, nullary(99)).unwrap();

        let child = runtime_class_new(Some(parent), None).unwrap();
        let RubyValue::Class(child_id) = child else {
            panic!()
        };
        assert!(child_id.0 >= RUNTIME_CLASS_ID_BASE);
        // child's ancestors include parent.
        let anc = overlay_ancestors(child_id).unwrap();
        assert!(anc.contains(&parent_id));

        // An instance of child resolves the inherited method.
        let inst = dyn_object_construct(child_id, &[], None).unwrap();
        let RubyValue::Object(o) = inst else { panic!() };
        let m = resolve_dynamic(&o, child_id, greet).expect("inherited method resolves");
        assert_eq!(int_of(m.call(&o, &[], None).unwrap()), 99);
    }

    #[test]
    fn class_new_body_populates_via_class_self() {
        // Class.new { define_method(:x){ 5 } } -- the body runs under the new
        // class; here the body calls runtime_define_method on its `self` class.
        let body = RProc::with_self(
            move |self_val, _args| {
                if let RubyValue::Class(cid) = self_val {
                    runtime_define_method(*cid, Symbol::intern("x"), nullary(5)).unwrap();
                }
                Ok(RubyValue::Nil)
            },
            RubyValue::Nil,
            0,
            false,
        );
        let klass = runtime_class_new(None, Some(body)).unwrap();
        let RubyValue::Class(cid) = klass else {
            panic!()
        };
        let inst = dyn_object_construct(cid, &[], None).unwrap();
        let RubyValue::Object(o) = inst else { panic!() };
        let m = resolve_dynamic(&o, cid, Symbol::intern("x")).unwrap();
        assert_eq!(int_of(m.call(&o, &[], None).unwrap()), 5);
    }

    // -- the narrowed latch (see the flags' own docs for INV-1..3) --------
    //
    // Each of these runs in its own process under nextest, so the global
    // flags start clean; they are asserted false first regardless, since a
    // shared-process runner would make a stale flag look like a pass.

    #[test]
    fn minting_a_native_class_patches_nothing_frozen() {
        let probe = ClassId(3);
        assert!(!class_maybe_patched(probe));
        let minted = intern_native_class(ClassId(2), FMap::default(), dyn_object_construct);
        // The wide latch gives up -- which is the whole cost this phase is
        // about, since `Struct.new` and `Data.define` land here.
        assert!(is_live());
        assert!(!iter_inline_ok(0));
        // The narrow one does not: a fresh id has no frozen registry entry, so
        // nothing that resolves against `probe` can have changed (INV-3).
        assert!(!class_maybe_patched(probe));
        assert!(iter_inline_ok_for(0, probe));
        // The minted class itself always reads as patched -- it can only be
        // found through the overlay.
        assert!(class_maybe_patched(minted));
    }

    #[test]
    fn defining_a_method_patches_only_that_class() {
        let touched = ClassId(7);
        let other = ClassId(8);
        runtime_define_method(touched, Symbol::intern("q"), nullary(1)).unwrap();
        assert!(class_maybe_patched(touched));
        assert!(!iter_inline_ok_for(0, touched));
        assert!(!class_maybe_patched(other));
        assert!(iter_inline_ok_for(0, other));
    }

    #[test]
    fn a_singleton_anywhere_stops_fusion_for_every_class() {
        // Identity-keyed, so no class-id set can express it (INV-2).
        let a: RObj = Arc::new(DynObject::new(ClassId(0)));
        assert!(iter_inline_ok_for(0, ClassId(3)));
        runtime_define_singleton_method(
            &RubyValue::Object(a.clone()),
            Symbol::intern("m"),
            nullary(1),
        )
        .unwrap();
        assert!(!iter_inline_ok_for(0, ClassId(3)));
    }

    #[test]
    fn a_box_never_fuses_however_narrow_the_latch() {
        assert!(!iter_inline_ok_for(1, ClassId(3)));
    }
}
