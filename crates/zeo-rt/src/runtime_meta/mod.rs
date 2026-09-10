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

mod api;
mod dyn_object;
mod frames;
mod lock;
mod resolver;
mod watermark;
pub use api::*;
pub(crate) use dyn_object::*;
pub use frames::*;
pub(crate) use lock::OverlayLock;
pub use resolver::*;
pub use watermark::*;

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
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
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
    /// The struct allocator a `Class#dup` copy inherits from its SOURCE. The
    /// copy's ancestry deliberately omits the source (ruby's `K.dup.ancestors`
    /// does not list `K`), yet the copied method bodies are the source's
    /// compiled ones and downcast to the source's struct -- so the allocator
    /// has to travel with the copy rather than be found by walking.
    allocator: Option<crate::dispatch::AllocatorFn>,
    /// Which of `methods` a `Class#dup` copy took from a compiled ANCESTOR
    /// rather than from the source's own table. They have to sit in `methods`
    /// (the ancestor's own copy expects the ancestor's struct and would abort
    /// on this copy's instances) but must stay out of every "own methods"
    /// answer -- `K.dup.instance_methods(false)` is `K`'s own list, not its
    /// whole chain's.
    inherited_names: FSet<Symbol>,
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
    /// Names `remove_method` retired, and the reason it is not `undefs`:
    /// `undef` TERMINATES the MRO walk, `remove` only empties THIS class's own
    /// tables and lets the walk carry on. `class C < P; def m; end; end` then
    /// `C.remove_method(:m)` leaves `C.new.m` answering `P#m`, where an
    /// `undef` would raise.
    ///
    /// An entry rather than a deletion because a class's own definition can
    /// live in two layers -- the overlay and the COMPILED registry row -- and
    /// deleting the overlay one merely uncovered the compiled one underneath.
    /// Every write that defines the name again clears it.
    removed: FSet<Symbol>,
    /// [`OverlayEntry::removed`]'s class-method twin, keyed on the OWNER for
    /// the same reason [`OverlayEntry::class_undefs`] is.
    class_removed: FSet<Symbol>,
    /// Class-method names this class carries a MATERIALIZED copy of that no
    /// `extend` has seated yet -- see `api::defer_extended_class_method`.
    ///
    /// Read exactly like [`OverlayEntry::class_removed`] (both empty the
    /// class's own position without ending the walk), kept apart from it
    /// because they are cleared by different things: an `extend` seating the
    /// module clears only what it supplies, and must not resurrect a name
    /// `remove_method` took out of `class << self`.
    class_deferred: FSet<Symbol>,
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
            allocator: None,
            inherited_names: FSet::default(),
            undefs: FSet::default(),
            removed: FSet::default(),
            class_removed: FSet::default(),
            class_deferred: FSet::default(),
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
    classes: OverlayLock<FMap<u32, OverlayEntry>>,
    /// Per-object singleton methods, keyed by the receiver's `Arc` DATA address
    /// (object identity). Not carried across `dup` -- a fresh `Arc` is a fresh
    /// address -- matching Ruby (`dup` drops singletons); `clone` re-keys the
    /// tables onto the copy via [`copy_value_singletons`].
    singletons: OverlayLock<FMap<usize, FMap<Symbol, MethodImpl>>>,
    /// Singleton methods on a NON-object heap value (`def SOME_ARRAY.[](i)`),
    /// keyed the same way. Separate from `singletons` because there is no
    /// `RObj` to bind: the body stays an `RProc` and runs with the value itself
    /// as `self`. See `value_identity`.
    value_singletons: OverlayLock<FMap<usize, FMap<Symbol, RProc>>>,
    /// The VISIBILITY of a per-object singleton method, keyed by the same
    /// identity the two tables above use.
    ///
    /// A per-object singleton row is a name and a body and nothing else, so
    /// there was nowhere to record whether it is public, protected or private.
    /// Three things write one: a `private`/`protected` cursor running over a
    /// `class << obj` body, a `private :name` sent to the object's singleton
    /// class, and `obj.extend(M)`, which copies M's own marks along with its
    /// bodies -- a `module_function` name is PRIVATE on the instance side, and
    /// CRuby refuses `obj.extend(MF).public_send(:helper)` for exactly that
    /// reason.
    ///
    /// Keyed by identity rather than by the singleton class id, because a
    /// singleton class is minted only when something NAMES it: `def obj.x`
    /// alone mints nothing, and recording a mark must not be what forces one.
    /// An absent entry is public, which is what `def obj.x` writes.
    singleton_vis: OverlayLock<FMap<usize, FMap<Symbol, crate::dispatch::MethodVisibility>>>,
    /// Names `obj.singleton_class.undef_method(:name)` retired for ONE object,
    /// keyed by the same identity the two tables above use. A tombstone, not an
    /// absence: the class still defines the name, and the point of the undef is
    /// that this object no longer answers it. `OverlayEntry::undefs` is the
    /// per-CLASS twin; there is no per-object `OverlayEntry` to put this in.
    singleton_undefs: OverlayLock<FMap<usize, FSet<Symbol>>>,
    /// A weak reference to every value that has ever received a singleton
    /// method, keyed by the same identity the tables above use.
    ///
    /// Identity here IS a heap address, so it is only unique while the object
    /// lives. Without this, a value that received a singleton and then died let
    /// the allocator hand its address to an unrelated later object, which
    /// silently inherited the dead one's methods -- `def s.shout` on a string
    /// built in a loop made every later same-sized string answer `shout`.
    /// Holding the ALLOCATION makes the address un-reusable, which is the only
    /// thing that makes the key sound.
    ///
    /// A `Weak` holds exactly that and no more. A strong reference held the
    /// VALUE too, which made every receiver of a singleton method immortal --
    /// a leak of its own, and one the cycle collector correctly read as "this
    /// node is referenced from outside the registry". [`sweep_pinned`] drops
    /// the rows whose owner is gone.
    pinned: OverlayLock<FMap<usize, crate::value::WeakOwner>>,
    /// The modules `recv.extend(M)` mixed into one receiver, newest LAST,
    /// keyed by [`extend_key`]. Separate from the method tables above because
    /// `extend` changes what the receiver IS, not only what it answers:
    /// `o.is_a?(M)` and `o.singleton_class.ancestors` both read this, and
    /// neither can be recovered from a copied method table.
    extended: OverlayLock<FMap<usize, Vec<ClassId>>>,
    /// WHICH module a per-object `extend` copied each singleton-table name
    /// from -- `obj.method(:x).owner`'s record, since the copy itself can't
    /// say. Cleared per name by every own definition (`def obj.x` shadows
    /// the module's copy and owns the name from then on), and by an undef.
    /// The record, not a guess: "an extended module that defines the name
    /// owns it" is wrong exactly when a later own def shadows one.
    extended_names: OverlayLock<FMap<usize, FMap<Symbol, ClassId>>>,
    /// `obj.singleton_class`'s cache: object identity -> the runtime class id
    /// minted for its singleton class (so a second call answers the same id,
    /// matching Ruby's identity).
    singleton_classes: OverlayLock<FMap<usize, ClassId>>,
    /// The inverse plus the owner value: a singleton-class id -> the object (or
    /// class) it belongs to. A `define_method` on that id installs a per-object
    /// singleton (or, for a class owner, a class method) rather than an ordinary
    /// instance method -- which is exactly what `class << obj` semantics mean.
    singleton_owner: OverlayLock<FMap<u32, RubyValue>>,
    next_id: AtomicU32,
}

/// The gates the dispatch fast path reads, in ONE atomic word.
///
/// The ten bits:
///
/// * [`GATE_OVERLAY`] (1) -- something was defined at runtime, so the overlay
///   may answer where the frozen tables would not. Armed by `mark_live`,
///   which every runtime definition calls.
/// * [`GATE_PENDING`] (2) -- a definition hook is running with names still
///   ahead of it, so a resolved entry may not exist yet (see
///   [`with_pending_defs`]).
/// * [`GATE_MOVED`] (4) -- a `Ractor` move gutted some object, so a dispatch
///   receiver may be a husk that must raise `Ractor::MovedError`.
///   Deliberately NOT part of [`is_live`]'s mask: a move must not deopt the
///   inline caches or the overlay shortcuts, only arm the husk probes.
/// * [`GATE_ARITY_DEBUG`] (8) -- `ZEO_ARITY_DEBUG` was set at startup
///   ([`arm_arity_debug`]).
/// * [`GATE_PATCHED_ANY`] (16) -- some FROZEN class id sits in the `PATCHED`
///   set ([`patch_class`]); gates the set probe in [`class_maybe_patched`].
/// * [`GATE_ANY_SINGLETONS`] (32) -- some object carries a per-object
///   singleton method. Identity-keyed, so no class-id set can express it
///   (INV-2 below).
/// * [`GATE_ANCESTRY_MUTATED`] (64) -- a runtime `include`/`prepend` spliced
///   some ancestry chain, so `PATCHED`'s downward closure (INV-1) can no
///   longer be trusted per class.
/// * [`GATE_ANY_EXTENDED`] (128) -- some receiver was `extend`ed at runtime;
///   gates the identity-keyed extend maps ([`value_extends`]).
/// * [`GATE_MRO_DUPLICATES`] (256) -- some chain holds a class TWICE. Part of
///   [`GATE_LIVE_MASK`] on purpose: a cache hit skips the walk that publishes
///   WHICH copy is running, and a `super` from the body would then resume
///   past the wrong one.
/// * [`GATE_FRAMES_INDIRECT`] (512) -- emitted frame prologues must call
///   instead of storing inline: `TracePoint` armed, or a pending frame
///   label handed over ([`crate::frames::set_pending_frame_label`]).
///
/// The two masks: [`GATE_LIVE_MASK`] (OVERLAY | PENDING | MRO_DUPLICATES) is
/// what turns the inline caches and the overlay shortcuts off;
/// [`GATE_ITER_BLOCKED`] (ANY_SINGLETONS | ANCESTRY_MUTATED | MOVED) is what
/// forbids a fused-iterator splice, apart from the receiver's own patched
/// state.
///
/// NINE of the ten bits are monotone latches: a `fetch_or` sets one, and
/// nothing ever clears it. [`GATE_PENDING`] is the one exception --
/// `watermark.rs` depth-counts the live hook frames (`PENDING_DEPTH`) and
/// clears the bit when the last hook returns, with a documented benign
/// cross-thread race (see `PENDING_DEPTH`'s docs).
///
/// The gates are process-global and (PENDING aside) never reset, so every
/// unit test that arms one depends on nextest's process-per-test isolation.
/// A shared-process runner would make a stale flag look like a pass. Tests
/// assert a gate false BEFORE arming it, so a stale flag fails loudly.
///
/// One word rather than separate `AtomicBool`s because the fast-path readers
/// must stay a single load (and, for [`is_live`], a single masked compare).
/// As a second flag beside the first, the pending gate measured +2.6% on a
/// loop whose body is nothing but a cached dynamic send; folded in here it
/// is free -- the moved gate rides the same byte for the same reason.
/// Exported as a DATA symbol: an emitter fast path loads the whole word
/// inline, and only ZERO may take the guard-free arm -- zero means no
/// gate has ever armed, which implies every masked question below
/// answers "quiet". Nonzero falls back to asking the real question
/// through capi. Rust-side readers keep their named accessors.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static zeo_rt_gates: AtomicU16 = AtomicU16::new(0);
use self::zeo_rt_gates as GATES;
const GATE_OVERLAY: u16 = 1;
const GATE_PENDING: u16 = 2;
const GATE_MOVED: u16 = 4;
/// `ZEO_ARITY_DEBUG` is armed -- folded into the gate byte the dispatch path
/// already loads, per the standing rule: a second flag word beside the gates
/// measured 2.6% on dispatch.
const GATE_ARITY_DEBUG: u16 = 8;
/// The four latches below ride the gate word rather than being separate
/// `AtomicBool`s, which would put FOUR acquire loads in
/// `iter_inline_ok_for` -- a fused loop's entry test, i.e. the check every
/// inlined `each`/`map` pays before it may splice. Same reason the pending
/// and moved gates ride it: the word is loaded once and masked. A new gate
/// widens the word (`GATES` is a `u16`), never adds a second one.
///
/// Some class body's `extend` has not been seated yet, so a class method
/// materialized from it must not answer and must not be CACHED as the answer.
///
/// The second clearable bit, and for the same reason [`GATE_PENDING`] is one:
/// it marks a WINDOW, not a fact about the whole run. The window is class
/// definition time -- from the first deferred `extend` row at boot to the
/// last `extend` statement -- after which every site caches exactly as it did
/// before. A monotone latch here cost 77x on a loop calling a class method an
/// extended module supplies, because `GATE_LIVE_MASK` turns off every inline
/// cache in the process.
///
/// [`PENDING_EXTEND_NAMES`] counts the retired names; the bit clears when the
/// last one is seated. A class body that never RUNS (`if false; class C;
/// extend M; end; end`) never seats its edge, so the window stays open for
/// that program -- correct, if slow: the extend genuinely has not happened.
const GATE_PENDING_EXTENDS: u16 = 1024;
/// Some compiled-in UNIT's method rows are still concealed -- see
/// [`crate::dispatch::concealed`]. The third clearable bit, and a WINDOW
/// like the two above it: it opens at boot and closes when the last
/// concealing unit's file has run.
///
/// Deliberately NOT in [`GATE_LIVE_MASK`] or [`GATE_CACHE_WIDE`]. A unit's
/// names already join the compiler's `runtime_patches`, so their call sites
/// dispatch dynamically with no cache to spoil; and a program that never
/// requires a swept file keeps this bit for its whole run, which through the
/// live mask would turn every inline cache in the process off.
const GATE_CONCEALED: u16 = 2048;
const GATE_PATCHED_ANY: u16 = 16;
const GATE_ANY_SINGLETONS: u16 = 32;
const GATE_ANCESTRY_MUTATED: u16 = 64;
const GATE_ANY_EXTENDED: u16 = 128;
/// A chain in this process holds a class TWICE -- a module both `include`d
/// and `prepend`ed into one class. Part of [`GATE_LIVE_MASK`] on purpose:
/// the inline caches must deopt, because a cache hit skips the walk that
/// publishes WHICH copy is running and a `super` from the body would then
/// resume past the wrong one.
const GATE_MRO_DUPLICATES: u16 = 256;
/// Emitted frame prologues must take the CALL path instead of their
/// inline stores: a `TracePoint` is (or was) armed, or a runtime method
/// install handed over a pending frame label -- both are work the inline
/// path deliberately skips. Monotone like the others; a program that
/// traces once keeps callee-framed prologues, which is the price of
/// tracing, not of the fast path. NOT in [`GATE_LIVE_MASK`]: frames are
/// orthogonal to dispatch caching. The VALUE is ABI -- the emitter bakes
/// the same bit into every prologue's test.
const GATE_FRAMES_INDIRECT: u16 = zeo_abi::abi::GATE_FRAMES_INDIRECT_BIT;
const GATE_LIVE_MASK: u16 =
    GATE_OVERLAY | GATE_PENDING | GATE_MRO_DUPLICATES | GATE_PENDING_EXTENDS;
/// What forbids a fused-iterator splice, apart from the receiver's own
/// patched state.
///
/// `GATE_MOVED` alone: a fused body reads its receiver's payload directly
/// and would iterate a husk's gutted storage instead of raising
/// `Ractor::MovedError`. [`GATE_ANY_SINGLETONS`] and
/// [`GATE_ANCESTRY_MUTATED`] are deliberately absent: those are answered per
/// class -- `def some_array.each` marks `Array`, an `include` marks the
/// class it splices into and its descendants -- so a program that puts a
/// singleton on one unrelated object keeps its fused loops.
const GATE_ITER_BLOCKED: u16 = GATE_MOVED;
/// What turns an inline cache off no matter WHICH class the site is keyed on
/// -- [`gates_cache_off`]'s wide half.
///
/// [`GATE_OVERLAY`] is deliberately absent. "Something, somewhere, was defined
/// at run time" is a fact about the PROCESS, and a cache asks a fact about ONE
/// class, which `PATCHED` records (INV-1). Reading the wide latch cost every
/// program that ever calls `define_method`, `attr_accessor`, `alias_method` or
/// `remove_method` -- which is every program that loads a gem -- **14x on a
/// loop whose body is one cached send**, for a definition on a class the loop
/// never touches.
///
/// What remains is what no class-id set can express: a hook part-way through
/// installing its class ([`GATE_PENDING`]), a chain holding a class twice,
/// whose `super` resume a hit would skip ([`GATE_MRO_DUPLICATES`]), and an
/// `extend` the program has not reached yet ([`GATE_PENDING_EXTENDS`]).
///
/// The other two narrowed latches are NOT here, and that is the load-bearing
/// half of this: [`GATE_ANY_SINGLETONS`] and [`GATE_ANCESTRY_MUTATED`] are
/// answered per class instead, which every writer of either now pays for --
/// `mark_singletons_for` patches the receiver's class scope, and every
/// ancestry splice patches the class it splices into BEFORE it splices.
const GATE_CACHE_WIDE: u16 = GATE_PENDING | GATE_MRO_DUPLICATES | GATE_PENDING_EXTENDS;
static OVERLAY: OnceLock<OverlayMaps> = OnceLock::new();

fn maps() -> &'static OverlayMaps {
    OVERLAY.get_or_init(|| OverlayMaps {
        classes: OverlayLock::new("classes", FMap::default()),
        singletons: OverlayLock::new("singletons", FMap::default()),
        value_singletons: OverlayLock::new("value_singletons", FMap::default()),
        singleton_vis: OverlayLock::new("singleton_vis", FMap::default()),
        singleton_undefs: OverlayLock::new("singleton_undefs", FMap::default()),
        extended: OverlayLock::new("extended", FMap::default()),
        extended_names: OverlayLock::new("extended_names", FMap::default()),
        pinned: OverlayLock::new("pinned", FMap::default()),
        singleton_classes: OverlayLock::new("singleton_classes", FMap::default()),
        singleton_owner: OverlayLock::new("singleton_owner", FMap::default()),
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
    GATES.load(Ordering::Relaxed) & GATE_LIVE_MASK != 0
}

/// The whole gate byte in one load, for the callers that ask more than one
/// gate question per dispatch ([`crate::dispatch::send_value_cached`]) --
/// same single atomic load `is_live` costs, split by [`gates_live`]/
/// [`gates_moved`] with plain register tests.
///
/// Every GATES read in this module is `Relaxed`, and that is an argument,
/// not an oversight. The bits are monotone latches (PENDING excepted, whose
/// benign race watermark.rs documents), and no reader dereferences data
/// through the bit alone: a set bit only licenses a PROBE of the overlay
/// maps, whose own `RwLock` supplies the happens-before edge to whatever the
/// arming thread published. A reader that observes a stale zero behaves as
/// if it ran before the arm -- the same outcome `Acquire` permits, since
/// acquire adds ordering, never freshness. GATE_MOVED's husk flag protocol
/// is already relaxed-plus-container-mutex on its own side. Writes keep
/// `Release` so the latch itself is cheap insurance, not load-bearing.
#[inline(always)]
pub(crate) fn gates() -> u16 {
    GATES.load(Ordering::Relaxed)
}

#[inline(always)]
pub(crate) fn gates_live(g: u16) -> bool {
    g & GATE_LIVE_MASK != 0
}

#[inline(always)]
pub(crate) fn gates_moved(g: u16) -> bool {
    g & GATE_MOVED != 0
}

#[inline(always)]
pub(crate) fn gates_arity_debug(g: u16) -> bool {
    g & GATE_ARITY_DEBUG != 0
}

/// Arm the `ZEO_ARITY_DEBUG` breadcrumb bit -- called once from program
/// startup ([`crate::exec::run_main`]) when the variable is set.
pub(crate) fn arm_arity_debug() {
    GATES.fetch_or(GATE_ARITY_DEBUG, Ordering::Release);
}

/// Arm the indirect-frames bit ([`GATE_FRAMES_INDIRECT`]) -- a
/// `TracePoint` enable, or a pending frame label handover. Emitted
/// prologues that see it take the call path, which fires the events and
/// consumes the label; the inline stores do neither.
pub(crate) fn arm_frames_indirect() {
    GATES.fetch_or(GATE_FRAMES_INDIRECT, Ordering::Release);
}

/// Whether some compiled-in unit still holds method rows back
/// ([`GATE_CONCEALED`]) -- the gate in front of every concealment probe.
#[inline(always)]
pub fn any_concealed() -> bool {
    GATES.load(Ordering::Relaxed) & GATE_CONCEALED != 0
}

/// Open the concealment window -- once, from `register_program`, and only
/// when the program really carries concealed rows.
pub(crate) fn arm_concealed() {
    GATES.fetch_or(GATE_CONCEALED, Ordering::Release);
}

/// Close it, when the last concealing unit's file has run.
pub(crate) fn disarm_concealed() {
    GATES.fetch_and(!GATE_CONCEALED, Ordering::Release);
}

/// Whether any `Ractor` move has ever poisoned an object -- the cheap gate
/// in front of every husk probe off the dispatch fast path.
#[inline(always)]
pub fn any_moved() -> bool {
    GATES.load(Ordering::Relaxed) & GATE_MOVED != 0
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
    box_id == 0 && GATES.load(Ordering::Relaxed) == 0
}

#[inline(always)]
pub fn iter_inline_ok_for(box_id: u32, recv: ClassId) -> bool {
    if box_id != 0 {
        return false;
    }
    // ONE load, then masks -- see `GATE_PATCHED_ANY`.
    let g = GATES.load(Ordering::Relaxed);
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
static PATCHED: OnceLock<RwLock<PatchedSets>> = OnceLock::new();

#[derive(Default)]
struct PatchedSets {
    /// The membership every reader asks about.
    ids: FSet<u32>,
    /// Ids whose whole descendant closure is already in `ids`, so a second
    /// [`patch_class`] on one costs nothing. Kept apart from `ids` because
    /// [`patch_class_own`] inserts a class WITHOUT its descendants, and a
    /// later closing mark on the same id must still do the walk.
    closed: FSet<u32>,
}

fn patched() -> &'static RwLock<PatchedSets> {
    PATCHED.get_or_init(|| RwLock::new(PatchedSets::default()))
}

/// How many class ids [`PATCHED_BITS`] covers. 8 KiB of BSS against a 29 MB
/// binary, and every program the corpus compiles fits inside it; anything
/// past it is correct but slow, which is the right way round.
const PATCHED_BITS_IDS: u32 = zeo_abi::abi::PATCHED_BITS_IDS;

/// The emitted typed-direct-call guard tests exactly the bits this module
/// does NOT answer per class. Asserted rather than restated, so renumbering a
/// gate breaks the build instead of the guard.
/// The emitted array-index guard tests [`iter_inline_ok_for`]'s wide half
/// inline, for the same reason and with the same protection.
const _: () = assert!(zeo_abi::abi::GATE_ITER_INLINE_SLOW == GATE_ITER_BLOCKED);

const _: () = assert!(
    zeo_abi::abi::GATE_TYPED_DIRECT_SLOW
        == GATE_PENDING
            | GATE_MOVED
            | GATE_ARITY_DEBUG
            | GATE_MRO_DUPLICATES
            | GATE_FRAMES_INDIRECT
            | GATE_PENDING_EXTENDS
);

/// One bit per frozen class id -- [`class_maybe_patched_gated`]'s read path,
/// and the answer for every id below [`PATCHED_BITS_IDS`].
///
/// [`PATCHED`] is the authority and this is its mirror. An `RwLock<FSet>`
/// read was the first shape and it is the wrong one for a question asked once
/// per cached send: a reader-lock acquire is a read-modify-write on one shared
/// word, so every thread sending any message serializes on it. Measured at 3M
/// sends, the mirror is worth ~13ns a call.
///
/// A fixed static array rather than a sized allocation, because the read is
/// then ONE load at a link-time-known address: an `OnceLock<Box<[..]>>` put
/// four dependent loads (the once state, its value, the box pointer, its
/// length) in front of the same bit test.
///
/// `Relaxed`, on the module's standing argument: a reader that misses a bit
/// behaves as if it ran before the patch, which is what `Acquire` permits too
/// -- acquire adds ordering, never freshness.
/// Exported, because the emitted typed-direct-call guard reads its word
/// inline: both the address and the mask are compile-time constants there.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static zeo_rt_patched_bits: [AtomicU64; (PATCHED_BITS_IDS / 64) as usize] =
    [const { AtomicU64::new(0) }; (PATCHED_BITS_IDS / 64) as usize];
use self::zeo_rt_patched_bits as PATCHED_BITS;

/// Set `added`'s bits. Ids past the array's span stay set-only; the reader
/// falls back to [`PATCHED`] for those.
fn mirror_patched_bits(added: &[u32]) {
    for &id in added {
        if id < PATCHED_BITS_IDS {
            PATCHED_BITS[(id / 64) as usize].fetch_or(1u64 << (id % 64), Ordering::Release);
        }
    }
}

#[inline(always)]
pub fn class_maybe_patched(id: ClassId) -> bool {
    class_maybe_patched_gated(GATES.load(Ordering::Relaxed), id)
}

#[inline(always)]
pub(crate) fn class_maybe_patched_gated(gates: u16, id: ClassId) -> bool {
    if id.0 >= RUNTIME_CLASS_ID_BASE {
        return true;
    }
    if gates & GATE_PATCHED_ANY == 0 {
        return false;
    }
    if id.0 < PATCHED_BITS_IDS {
        return PATCHED_BITS[(id.0 / 64) as usize].load(Ordering::Relaxed) & (1u64 << (id.0 % 64))
            != 0;
    }
    PATCHED
        .get()
        .is_some_and(|p| p.read().unwrap().ids.contains(&id.0))
}

/// Whether an inline cache keyed on `id` must stand down: the wide gates
/// ([`GATE_CACHE_WIDE`]), or this one class's own patched state.
///
/// The narrow half is INV-1 read forwards -- `PATCHED` holds every class whose
/// resolution a runtime operation could have changed, downward-closed -- so a
/// site keyed on a class nobody has touched keeps its answer while the rest of
/// the program redefines whatever it likes.
#[inline(always)]
pub(crate) fn gates_cache_off(g: u16, id: ClassId) -> bool {
    gates_cache_wide(g) || class_maybe_patched_gated(g, id)
}

/// [`gates_cache_off`]'s class-independent half, for the sites that answer
/// the per-class question some other way.
#[inline(always)]
pub(crate) fn gates_cache_wide(g: u16) -> bool {
    g & GATE_CACHE_WIDE != 0
}

/// Mark `id` and everything that inherits from it.
///
/// O(#classes) on the first mark of a class and O(1) after: an id already in
/// the set brought its whole descendant closure in with it, and
/// `classes_with_ancestor` is transitive, so a descendant re-marked later adds
/// nothing. That matters because `obj.extend(M)` in a loop reaches here once
/// per call.
fn patch_class(id: ClassId) {
    // The bump is UNCONDITIONAL, ahead of the early return: a second mark on
    // a class already in the set adds no member and still changes what it
    // resolves. `private_class_method` after a `define_singleton_method` is
    // the shape -- same class, same set, a different verdict.
    PATCH_GEN.fetch_add(1, Ordering::Release);
    if patched().read().unwrap().closed.contains(&id.0) {
        return;
    }
    let added: Vec<u32> = {
        let below = crate::dispatch::classes_with_ancestor(id);
        let mut w = patched().write().unwrap();
        w.closed.insert(id.0);
        std::iter::once(id.0)
            .chain(below)
            .filter(|&c| w.ids.insert(c))
            .collect()
    };
    mirror_patched_bits(&added);
    GATES.fetch_or(GATE_PATCHED_ANY, Ordering::Release);
}

/// How many times a class has been marked. Anything derived from "what does
/// this class resolve `name` to, and may I call it" is valid only while this
/// stands still -- see [`crate::dispatch::ClassMethodSite`]'s remembered
/// barrier verdict.
///
/// One counter for the whole process rather than one per class, because the
/// thing it guards is not always a fact about ONE class: the visibility
/// barrier falls through to `Class`'s own instance methods when the receiver
/// class does not supply the name. A program settles once its definitions
/// have run, and the counter stops moving with them.
static PATCH_GEN: AtomicU32 = AtomicU32::new(0);

#[inline(always)]
pub(crate) fn patch_gen() -> u32 {
    PATCH_GEN.load(Ordering::Relaxed)
}

/// Mark `id` ALONE -- no descendant closure.
///
/// For a change one class's instances can see and its subclasses' cannot,
/// which is exactly what a per-object singleton row is: it lives on ONE
/// object, whose class is `id` and nothing else. The closure matters here:
/// `def o.x` on a plain `Object.new` would otherwise mark
/// `classes_with_ancestor(Object)` -- every class in the program -- and
/// deoptimize the whole thing to record a row on one object.
fn patch_class_own(id: ClassId) {
    PATCH_GEN.fetch_add(1, Ordering::Release);
    if patched().read().unwrap().ids.contains(&id.0) {
        return;
    }
    patched().write().unwrap().ids.insert(id.0);
    mirror_patched_bits(&[id.0]);
    GATES.fetch_or(GATE_PATCHED_ANY, Ordering::Release);
}

/// Arm [`GATE_ANY_SINGLETONS`] for a singleton installed on `recv`, and patch
/// the class scope that singleton can be reached through.
///
/// The gate stays a process-wide boolean because INV-2 is true: an
/// identity-keyed row is not a fact about a class. The PATCH is the weaker
/// fact that is still worth recording -- a row on one object can only change
/// what a receiver OF THAT OBJECT'S CLASS resolves, so marking the class is
/// sound, and it is what keeps `def obj.x` on one object from deoptimizing
/// every call site in the program.
///
/// A CLASS receiver takes its own id rather than `Class`: a singleton on the
/// class value IS one of that class's class methods, which is what a
/// `ClassMethodSite` is keyed by.
fn mark_singletons_for(recv: &RubyValue) {
    mark_singletons();
    match recv {
        // A class method IS inherited, so a singleton on a Class value takes
        // the descendant closure that every other class-method write takes.
        RubyValue::Class(cid) => patch_class(*cid),
        // An ordinary object's row reaches one object, so its class alone.
        other => patch_class_own(other.class_id()),
    }
}

/// Arm [`GATE_ANY_SINGLETONS`] and nothing else -- for a seating whose rows
/// the COMPILER already materialized, so no class's resolution changes.
/// [`crate::runtime_meta::class_extend_at`] is the only caller.
fn mark_singletons() {
    GATES.fetch_or(GATE_ANY_SINGLETONS, Ordering::Release);
}

fn mark_ancestry_mutated() {
    GATES.fetch_or(GATE_ANCESTRY_MUTATED, Ordering::Release);
}

/// Arms [`GATE_MRO_DUPLICATES`] -- see it for what it costs and why.
pub fn mark_mro_duplicates() {
    GATES.fetch_or(GATE_MRO_DUPLICATES, Ordering::Release);
}

/// Whether any chain in this process holds a class twice.
#[inline]
pub fn mro_duplicates() -> bool {
    GATES.load(Ordering::Relaxed) & GATE_MRO_DUPLICATES != 0
}

fn mark_live() {
    GATES.fetch_or(GATE_OVERLAY, Ordering::Release);
}

/// Class-method names still retired waiting for their `extend` -- see
/// [`GATE_PENDING_EXTENDS`], whose window this counts.
static PENDING_EXTEND_NAMES: AtomicU32 = AtomicU32::new(0);

/// One more retired name; opens the window if it was shut.
pub(super) fn mark_extend_pending() {
    PENDING_EXTEND_NAMES.fetch_add(1, Ordering::Relaxed);
    GATES.fetch_or(GATE_PENDING_EXTENDS, Ordering::Release);
}

/// `seated` names are live again; shuts the window when the last one is.
///
/// The subtraction saturates rather than wrapping: an `extend` that seats a
/// name no boot row retired (a module whose row this class also defines
/// itself) must not take the count negative and leave the window open for
/// the rest of the run.
pub(super) fn mark_extends_seated(seated: u32) {
    if seated == 0 {
        return;
    }
    let left = PENDING_EXTEND_NAMES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_sub(seated))
    });
    if left.is_ok_and(|before| before.saturating_sub(seated) == 0) {
        GATES.fetch_and(!GATE_PENDING_EXTENDS, Ordering::Release);
    }
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
        RubyValue::Range(r) => Arc::as_ptr(r).cast::<()>(),
        RubyValue::MatchData(m) => Arc::as_ptr(m).cast::<()>(),
        RubyValue::Enumerator(e) => Arc::as_ptr(e).cast::<()>(),
        RubyValue::Fiber(f) => Arc::as_ptr(f).cast::<()>(),
        RubyValue::Thread(t) => Arc::as_ptr(t).cast::<()>(),
        RubyValue::Mutex(m) => Arc::as_ptr(m).cast::<()>(),
        RubyValue::Queue(q) => Arc::as_ptr(q).cast::<()>(),
        RubyValue::Ractor(r) => Arc::as_ptr(r).cast::<()>(),
        // A Proc's payload address, which is what its own `identity` spells.
        RubyValue::Proc(p) | RubyValue::Yielder(p) => return Some(p.identity()),
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
    if let Some(weak) = crate::value::weak_owner(v) {
        let len = {
            let mut pinned = maps().pinned.write().unwrap();
            pinned.insert(key, weak);
            pinned.len()
        };
        if len >= PIN_SWEEP_AT.load(Ordering::Relaxed) {
            sweep_pinned();
        }
    }
    Some(key)
}

/// Retire every identity whose owner is gone.
///
/// **The order is the whole correctness argument.** Dropping a pin is what
/// frees the allocation, and freeing the allocation is what lets the allocator
/// hand that address to an unrelated later value. So every row keyed by a dead
/// address is removed FIRST, and the pins go LAST. Reversed, there is a window
/// in which a new value can be born at a reused address and inherit a dead
/// object's singleton methods -- which is the exact bug the pin exists to
/// prevent, and which `test/lang/singleton/singleton_address_reuse.rb`
/// catches.
///
/// Only keys the pin table names are touched. A Class receiver is keyed by
/// [`class_identity`] and never pinned, so its rows are never candidates.
///
/// Amortized against the table's own growth, the same bargain a `Vec` makes:
/// the next sweep waits until the table has grown by half again, so the scan
/// costs O(1) per pin taken.
fn sweep_pinned() {
    let dead: Vec<usize> = maps()
        .pinned
        .read()
        .unwrap()
        .iter()
        .filter(|(_, w)| w.strong_count() == 0)
        .map(|(k, _)| *k)
        .collect();
    if dead.is_empty() {
        let len = maps().pinned.read().unwrap().len();
        PIN_SWEEP_AT.store(len + (len / 2).max(64), Ordering::Relaxed);
        return;
    }

    // A singleton class outlives its owner in one more table: the inverse map
    // holds the object STRONGLY, so it has to go with the rest.
    let mut classes = maps().singleton_classes.write().unwrap();
    let mut owners = maps().singleton_owner.write().unwrap();
    for k in &dead {
        if let Some(cid) = classes.remove(k) {
            owners.remove(&cid.0);
        }
    }
    drop(owners);
    drop(classes);

    maps()
        .singletons
        .write()
        .unwrap()
        .retain(|k, _| !dead.contains(k));
    maps()
        .value_singletons
        .write()
        .unwrap()
        .retain(|k, _| !dead.contains(k));
    maps()
        .singleton_vis
        .write()
        .unwrap()
        .retain(|k, _| !dead.contains(k));
    maps()
        .singleton_undefs
        .write()
        .unwrap()
        .retain(|k, _| !dead.contains(k));
    maps()
        .extended
        .write()
        .unwrap()
        .retain(|k, _| !dead.contains(k));
    maps()
        .extended_names
        .write()
        .unwrap()
        .retain(|k, _| !dead.contains(k));

    let mut pinned = maps().pinned.write().unwrap();
    for k in &dead {
        pinned.remove(k);
    }
    PIN_SWEEP_AT.store(pinned.len() + (pinned.len() / 2).max(64), Ordering::Relaxed);
}

/// How many pins the table may hold before the next [`sweep_pinned`].
static PIN_SWEEP_AT: AtomicUsize = AtomicUsize::new(64);

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
///
/// Every heap value, not only an `Object`: `v.singleton_class` has to answer
/// the SAME object every time, for a `String`/`Array`/`Hash`/`Range` too --
/// a fresh one per call would make `equal?` false and a `class << str`
/// body's ivar read back nil. Identical to [`extend_key`], because the two
/// answer the same question about the same receiver.
fn singleton_class_key(recv: &RubyValue) -> Option<usize> {
    extend_key(recv)
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
    if GATES.load(Ordering::Relaxed) & GATE_ANY_EXTENDED == 0 {
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
    if GATES.load(Ordering::Relaxed) & GATE_ANY_EXTENDED == 0 {
        return false;
    }
    // A module PREPENDED into a singleton class is in that chain too, and it
    // is not in the extended list: the two verbs are recorded apart, because
    // a module reached BOTH ways holds two positions and one list cannot say
    // so. See `singleton_chain`.
    if let RubyValue::Class(cid) = recv
        && resolver::singleton_prepends_of(*cid)
            .into_iter()
            .any(reaches)
    {
        return true;
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
pub(crate) fn singleton_super_chain(recv: &RubyValue) -> Vec<ClassId> {
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
            for id in singleton_layer(p) {
                push(id, &mut chain);
            }
            parent = superclass_of(p);
        }
    }
    for &a in crate::dispatch::ancestors_of_value(recv.class_id()) {
        push(a, &mut chain);
    }
    chain
}

/// The module prepended into `cid`'s singleton class that supplies class
/// method `name`, latest prepend first -- what `Method#owner` reports where
/// zeo's flattened rows name the host class.
///
/// A singleton prepend arrives two ways and both are read here: a run-time
/// `K.singleton_class.prepend(M)` records the module in the overlay, and a
/// compiled `class << self; prepend M; end` seats it ahead of the surrogate
/// in the surrogate's own chain. The surrogate is read WITHOUT minting -- it
/// was registered at boot, and minting here would arm the overlay gate for a
/// reflection question.
pub fn singleton_prepend_owner(cid: ClassId, name: Symbol) -> Option<ClassId> {
    let defines = |m: ClassId| crate::dispatch::method_owner(m, name).is_some();
    if let Some(m) = resolver::singleton_prepends_of(cid)
        .into_iter()
        .find(|&m| defines(m))
    {
        return Some(m);
    }
    let key = singleton_class_key(&RubyValue::Class(cid))?;
    let sid = maps()
        .singleton_classes
        .read()
        .unwrap()
        .get(&key)
        .copied()?;
    crate::dispatch::ancestors_of_value(sid)
        .iter()
        .take_while(|&&a| a != sid)
        .copied()
        .find(|&m| defines(m))
}

/// What ruby files AT one class's singleton: the modules prepended into it,
/// the singleton class itself, then the modules extended onto the class. What
/// sits ABOVE the layer -- the parent's own layer, then `Class`'s ancestry --
/// is [`singleton_super_chain`]'s job.
///
/// A subclass's chain carries its parent's whole layer, not just the parent's
/// singleton class: `class Child < Inc` where `Inc` mixes a module into its
/// singleton lists that module between `#<Class:Inc>` and `#<Class:Object>`.
///
/// The prepends come from two places because a singleton prepend does: a
/// run-time one lands in the overlay, and a compiled one lives in the
/// singleton class's own registered chain, ahead of the singleton itself.
fn singleton_layer(cid: ClassId) -> Vec<ClassId> {
    let sid = singleton_class_id_of(cid);
    let mut layer: Vec<ClassId> = resolver::singleton_prepends_of(cid);
    for &a in crate::dispatch::ancestors_of_value(sid) {
        if a == sid {
            break;
        }
        if !layer.contains(&a) {
            layer.push(a);
        }
    }
    layer.push(sid);
    for m in extended_modules(&RubyValue::Class(cid)).into_iter().rev() {
        for &a in crate::dispatch::ancestors_of_value(m) {
            if !layer.contains(&a) {
                layer.push(a);
            }
        }
    }
    layer
}

/// `cid`'s singleton class, minting it if this is the first ask. Holds NO
/// lock, so the recursion up the superclass chain cannot deadlock.
fn singleton_class_id_of(cid: ClassId) -> ClassId {
    match runtime_singleton_class(&RubyValue::Class(cid)) {
        Ok(RubyValue::Class(sid)) => sid,
        _ => unreachable!("a Class always has a singleton class"),
    }
}

/// Rebase a COMPILED singleton surrogate onto ruby's parallel metaclass
/// chain, the first time a program asks for the singleton class.
///
/// A `class << self` body that carries a mixin makes the compiler register a
/// surrogate class with an ancestry of its own, and
/// `analyze::mro::compute_ancestors` roots every class at `Object`. So the
/// surrogate arrives as `[prepends.., surrogate, includes.., Object, Kernel,
/// BasicObject]` where ruby continues through `#<Class:Object>`,
/// `#<Class:BasicObject>`, `Class` and `Module` first. Everything from the
/// first `Object` on is that ordinary tail, and [`singleton_super_chain`]
/// builds the real one.
///
/// The chain it builds also carries the owner's `extend`s, which is how an
/// `include` written inside `class << self` reaches the ancestry at all: it
/// lowers to `extend`, ruby's own meaning for it.
///
/// A MINTED singleton already has the right chain and an overlay entry, so
/// the entry's absence is what tells the two apart -- and it also makes this
/// idempotent, since the rebase writes one.
fn rebase_compiled_surrogate(sid: ClassId, recv: &RubyValue) {
    if maps().classes.read().unwrap().contains_key(&sid.0) {
        return;
    }
    let compiled: Vec<ClassId> = crate::dispatch::ancestors_of_value(sid)
        .iter()
        .copied()
        .take_while(|&a| a != zeo_abi::OBJECT_CLASS)
        .collect();
    // Everything ahead of the surrogate in its compiled chain IS its prepend
    // area; the surrogate and what follows are the include side.
    let at = compiled.iter().position(|&a| a == sid);
    let (area, rest) = match at {
        Some(i) => (compiled[..i].to_vec(), compiled[i..].to_vec()),
        None => (Vec::new(), {
            let mut v = compiled;
            v.push(sid);
            v
        }),
    };
    let mut tail = rest;
    tail.extend(singleton_super_chain(recv));
    let anc = singleton_chain(area, tail);
    let name = crate::dispatch::class_name(sid);
    maps().classes.write().unwrap().insert(
        sid.0,
        OverlayEntry {
            name: RwLock::new(name),
            ancestors: Box::leak(anc.into_boxed_slice()),
            ..Default::default()
        },
    );
    // The surrogate's chain is what its owner's CLASS methods resolve through,
    // so rewriting it is a change to `sid` -- and the only caller reaches here
    // with the owner in hand and marks that side itself.
    patch_class(sid);
    mark_live();
}

/// The row a module seated in a singleton chain supplies for `name`, wrapped
/// as a class method -- a value-receiver `RProc` invoked with the Class as
/// `self`, which is what an `extend`ed instance method IS when it serves as a
/// class method.
///
/// Public because the class-method walk visits each mixed-in module as a
/// POSITION of its own now, rather than reading the host's flattened copy: a
/// module reached both ways has two positions and one copy.
pub fn singleton_mixin_method(module: ClassId, name: Symbol) -> Option<RProc> {
    watermark::extended_class_method(module, name)
}

/// A singleton class's ancestry, built the way CRuby's two verbs build one.
///
/// A `prepend` searches only the PREPEND AREA and an `include`/`extend`
/// searches the whole chain -- `rb_prepend_module`'s `search_super = FALSE`
/// against `rb_include_module`'s `TRUE`, the same rule the instance side
/// replays in `splice_into_chain`. So a module reached BOTH ways holds two
/// positions: `extend M; singleton_class.prepend M` is
/// `[M, #<Class:K>, M, ..]` in ruby.
///
/// One dedup over the concatenation collapsed them into one, and that was two
/// bugs at once: `K.singleton_class.ancestors` was a row short, and a `super`
/// from the prepended copy re-entered the same copy until the stack died,
/// because the walk could not tell the two positions apart.
///
/// Each side dedups against ITSELF, which is all either verb's scope allows.
fn singleton_chain(prepend_area: Vec<ClassId>, tail: Vec<ClassId>) -> Vec<ClassId> {
    let dedup = |mut v: Vec<ClassId>| {
        let mut seen = crate::FSet::default();
        v.retain(|&a| seen.insert(a));
        v
    };
    let mut chain = dedup(prepend_area);
    chain.extend(dedup(tail));
    // Arms the duplicate gate, which is what lets a class-method `super`
    // resume past the copy that is RUNNING rather than past the first one.
    // Every other linearized chain is noted where it is installed; this one
    // is built here and leaked straight into the overlay entry.
    crate::dispatch::note_chain(&chain);
    chain
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
    // A singleton PREPEND sits AHEAD of the singleton class -- that is what
    // prepend means, and it is where CRuby puts it. `extend` sits behind.
    // Both arrive through the extended list (the method copies need it), so
    // the prepend area is built from the prepends alone and the include side
    // from everything else.
    let area: Vec<ClassId> = match recv {
        RubyValue::Class(cid) => resolver::singleton_prepends_of(*cid),
        _ => Vec::new(),
    };
    let mut tail = vec![sid];
    tail.extend(singleton_super_chain(recv));
    let leaked: &'static [ClassId] = Box::leak(singleton_chain(area, tail).into_boxed_slice());
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

/// The visibility of the singleton method `name` installed directly on
/// `recv` -- see [`OverlayMaps::singleton_vis`]. `None` means no mark, which
/// reads as public: `def obj.x` and `define_singleton_method` both write one.
pub fn singleton_visibility(
    recv: &RubyValue,
    name: Symbol,
) -> Option<crate::dispatch::MethodVisibility> {
    let key = value_identity(recv)?;
    maps()
        .singleton_vis
        .read()
        .unwrap()
        .get(&key)
        .and_then(|t| t.get(&name).copied())
}

/// Record the visibility of a per-object singleton method. A `Public` mark is
/// stored rather than dropped: it has to be able to CLEAR an earlier private
/// one, which `obj.singleton_class.public :x` does.
pub(crate) fn set_singleton_visibility(
    recv: &RubyValue,
    name: Symbol,
    vis: crate::dispatch::MethodVisibility,
) {
    let Some(key) = pin_identity(recv) else {
        return;
    };
    maps()
        .singleton_vis
        .write()
        .unwrap()
        .entry(key)
        .or_default()
        .insert(name, vis);
    mark_singletons_for(recv);
    mark_live();
}

/// Drop any visibility mark for `name` on `recv` -- a re-`def` resets the name
/// to the body's running default, exactly as it does on a class.
pub(crate) fn clear_singleton_visibility(recv: &RubyValue, name: Symbol) {
    let Some(key) = value_identity(recv) else {
        return;
    };
    if let Some(t) = maps().singleton_vis.write().unwrap().get_mut(&key) {
        t.remove(&name);
    }
    mark_singletons_for(recv);
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
    let vis = maps().singleton_vis.read().unwrap().get(&fk).cloned();
    if let Some(t) = vis
        && !t.is_empty()
    {
        maps().singleton_vis.write().unwrap().insert(tk, t);
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
    if copied && let Some(weak) = crate::value::weak_owner(to) {
        maps().pinned.write().unwrap().insert(tk, weak);
        // The copy's class already carries the source's patch (a clone keeps
        // its class), but the rule is "every writer of a singleton table
        // patches its class scope" and an unstated exception is how one gets
        // broken.
        mark_singletons_for(to);
    }
}

/// The CLASS half of the carry above: `Class#dup`/`#clone` both clone the
/// source's singleton class (CRuby's `rb_mod_init_copy`), so the extend
/// table and its per-name provenance travel to the copy's identity.
pub(crate) fn copy_class_extensions(from: ClassId, to: ClassId) {
    let fk = class_identity(from);
    let tk = class_identity(to);
    let mods = maps().extended.read().unwrap().get(&fk).cloned();
    if let Some(mods) = mods
        && !mods.is_empty()
    {
        maps().extended.write().unwrap().insert(tk, mods);
    }
    let owners = maps().extended_names.read().unwrap().get(&fk).cloned();
    if let Some(t) = owners
        && !t.is_empty()
    {
        maps().extended_names.write().unwrap().insert(tk, t);
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
    fn a_singleton_stops_fusion_for_its_owners_class_alone() {
        // The ROW is identity-keyed and no class-id set can express it
        // (INV-2). What a class-id set CAN express is the weaker fact that
        // makes it reachable: the row answers only for a receiver of the
        // owner's class, so that class alone stands down.
        let a: RObj = Arc::new(DynObject::new(ClassId(0)));
        assert!(iter_inline_ok_for(0, ClassId(0)));
        assert!(iter_inline_ok_for(0, ClassId(3)));
        runtime_define_singleton_method(
            &RubyValue::Object(a.clone()),
            Symbol::intern("m"),
            nullary(1),
        )
        .unwrap();
        assert!(!iter_inline_ok_for(0, ClassId(0)));
        assert!(iter_inline_ok_for(0, ClassId(3)));
    }

    #[test]
    fn a_singleton_marks_its_class_without_the_descendant_closure() {
        // `def o.x` on a plain `Object.new` must not mark every class in the
        // program: the row lives on ONE object, and a subclass instance is
        // not it.
        let a: RObj = Arc::new(DynObject::new(zeo_abi::OBJECT_CLASS));
        runtime_define_singleton_method(
            &RubyValue::Object(a.clone()),
            Symbol::intern("m"),
            nullary(1),
        )
        .unwrap();
        assert!(class_maybe_patched(zeo_abi::OBJECT_CLASS));
        assert!(!class_maybe_patched(ClassId(3)));
    }

    #[test]
    fn a_box_never_fuses_however_narrow_the_latch() {
        assert!(!iter_inline_ok_for(1, ClassId(3)));
    }

    // -- the gate word's transitions (see the GATES contract above) --------
    //
    // Eight bits are monotone latches: the arming call flips one and nothing
    // clears it. GATE_PENDING is the exception, depth-counted by watermark.rs.
    // Same posture as the narrowed-latch tests above: every gate is asserted
    // false BEFORE its arming call, so a shared-process runner fails loudly.

    fn gate(bit: u16) -> bool {
        GATES.load(Ordering::Relaxed) & bit != 0
    }

    #[test]
    fn a_runtime_definition_arms_the_overlay_latch_for_good() {
        assert!(!is_live());
        runtime_define_method(ClassId(7), Symbol::intern("m"), nullary(1)).unwrap();
        assert!(is_live());
        assert!(gate(GATE_OVERLAY));
        // Set-only: a second definition re-arms, never clears.
        runtime_define_method(ClassId(8), Symbol::intern("m"), nullary(2)).unwrap();
        assert!(is_live());
    }

    #[test]
    fn a_runtime_definition_arms_the_patched_latch() {
        assert!(!gate(GATE_PATCHED_ANY));
        runtime_define_method(ClassId(7), Symbol::intern("m"), nullary(1)).unwrap();
        assert!(gate(GATE_PATCHED_ANY));
    }

    #[test]
    fn a_singleton_definition_arms_the_singletons_latch() {
        let a: RObj = Arc::new(DynObject::new(ClassId(0)));
        assert!(!gate(GATE_ANY_SINGLETONS));
        assert!(iter_inline_ok_for(0, ClassId(3)));
        runtime_define_singleton_method(
            &RubyValue::Object(a.clone()),
            Symbol::intern("m"),
            nullary(1),
        )
        .unwrap();
        assert!(gate(GATE_ANY_SINGLETONS));
        // The latch arms for the identity-keyed PROBES; fusion is decided
        // per class now, so an untouched one keeps it.
        assert!(iter_inline_ok_for(0, ClassId(3)));
        assert!(!iter_inline_ok_for(0, ClassId(0)));
    }

    #[test]
    fn a_ractor_move_arms_the_moved_latch_without_deopting_caches() {
        assert!(!any_moved());
        mark_moved();
        assert!(any_moved());
        // Not in the live mask: a move arms the husk probes and nothing else.
        assert!(!is_live());
        // Set-only; a second arm is a no-op.
        mark_moved();
        assert!(any_moved());
    }

    #[test]
    fn arity_debug_arms_its_bit_outside_the_live_mask() {
        assert!(!gates_arity_debug(gates()));
        arm_arity_debug();
        assert!(gates_arity_debug(gates()));
        assert!(!is_live());
    }

    #[test]
    fn a_runtime_extend_arms_the_extended_latch() {
        let obj = RubyValue::Object(Arc::new(DynObject::new(ClassId(0))) as RObj);
        let module = runtime_module_new(None).unwrap();
        let RubyValue::Class(mid) = module else {
            panic!()
        };
        assert!(!gate(GATE_ANY_EXTENDED));
        assert!(!value_extends(&obj, mid));
        runtime_extend(&obj, &module).unwrap();
        assert!(gate(GATE_ANY_EXTENDED));
        // The latch gates the identity-keyed map this records into.
        assert!(value_extends(&obj, mid));
    }

    #[test]
    fn a_runtime_include_arms_the_ancestry_latch() {
        let class = runtime_class_new(None, None).unwrap();
        let RubyValue::Class(cid) = class else {
            panic!()
        };
        let module = runtime_module_new(None).unwrap();
        let RubyValue::Class(mid) = module else {
            panic!()
        };
        assert!(!gate(GATE_ANCESTRY_MUTATED));
        runtime_include(&class, std::slice::from_ref(&module)).unwrap();
        assert!(gate(GATE_ANCESTRY_MUTATED));
        // The splice really landed: the overlay chain carries the module.
        assert!(overlay_ancestors(cid).unwrap().contains(&mid));
    }

    #[test]
    fn a_doubled_chain_arms_the_duplicates_latch() {
        let a = ClassId(900_001);
        let b = ClassId(900_002);
        assert!(!mro_duplicates());
        // A clean chain arms nothing.
        crate::dispatch::note_chain(&[a, b]);
        assert!(!mro_duplicates());
        crate::dispatch::note_chain(&[a, b, a]);
        assert!(mro_duplicates());
        // Set-only: a later clean chain does not clear it.
        crate::dispatch::note_chain(&[b]);
        assert!(mro_duplicates());
        // Part of the live mask: the inline caches must deopt.
        assert!(is_live());
    }

    #[test]
    fn pending_defs_begin_sets_and_a_balanced_end_clears() {
        let name = Symbol::intern("later");
        assert!(!any_pending());
        pending_defs_begin(ClassId(7), &[name]);
        assert!(any_pending());
        // Part of the live mask while a hook runs.
        assert!(is_live());
        pending_defs_end();
        assert!(!any_pending());
        assert!(!is_live());
    }

    #[test]
    fn nested_pending_defs_clear_only_at_depth_zero() {
        let name = Symbol::intern("later");
        assert!(!any_pending());
        pending_defs_begin(ClassId(7), &[name]);
        pending_defs_begin(ClassId(8), &[name]);
        pending_defs_end();
        // One frame is still on the stack: the gate must hold.
        assert!(any_pending());
        pending_defs_end();
        assert!(!any_pending());
    }
}
