//! Object model & dynamic dispatch. See the plan's "Object model & dynamic
//! dispatch" section for the full design rationale: two dispatch paths
//! (static direct/match-on-class_id calls, generated entirely by `spinelc`;
//! and this module's runtime `ClassRegistry`/`send`, reached only when
//! `spinelc` cannot resolve a call statically). Spinel itself never needs
//! this module at all -- it's the one deliberate architectural addition.

use crate::{RubyValue, Signal, Symbol};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// Identifies a Ruby class at runtime. Mirrors spinel's struct-embedded
/// `cls_id` field (`emit_class_struct`, codegen.c:2496).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(pub u32);

/// Implemented (via `ruby_class!`) by every generated Ruby class, and by the
/// built-in `Object` root below. `Send + Sync` supertrait bounds (Part 9):
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
}

/// A handle to any live Ruby object, used wherever the concrete class isn't
/// statically known. Mirrors spinel's boxed `sp_RbVal { tag: SP_TAG_OBJ,
/// cls_id, v: { p } }` (`lib/sp_gc.h:42`) for the object case -- except the
/// Rust trait object's vtable *is* the tag. Mutability lives on individual
/// ivar fields (see `ruby_class!`), not on this handle, so no lock layer is
/// needed here. `Arc` (not `Rc`, Part 9): every concrete `RubyObject` impl is
/// `Send + Sync` (via the trait's own supertrait bounds above), so this type
/// itself is genuinely `Send + Sync` -- no `unsafe impl` needed.
pub type RObj = Arc<dyn RubyObject>;

/// The root of every class hierarchy. `ClassId(0)`, no ivars, no
/// superclass -- the base case `ruby_class!`'s `$super` bottoms out at.
pub struct Object;

impl Object {
    pub const CLASS_ID: ClassId = ClassId(0);
}

impl RubyObject for Object {
    fn class_id(&self) -> ClassId {
        Self::CLASS_ID
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Downcasts an erased `RObj` to an owned `Arc<T>` -- the Path 2 trampoline
/// counterpart to `as_any().downcast_ref()`, needed because every generated
/// method takes `self: Arc<Self>` now (see `ruby_class!`'s docs). Cloning
/// `recv` first is a cheap `Arc` refcount bump, not a deep copy.
pub fn downcast_robj<T: RubyObject>(recv: &RObj) -> Option<Arc<T>> {
    recv.clone().as_any_rc().downcast::<T>().ok()
}

/// Every generated method/trampoline returns `Result<RubyValue, Signal>`, not
/// a bare `RubyValue` -- see `Signal`'s docs for why this is fixed from the
/// start rather than retrofitted once `break`/`raise`/non-local `return`
/// exist. The third parameter is the call's block, if any (`None` when no
/// block was given) -- see `signal.rs`'s `catch_break` and `codegen::params`'s
/// docs for how a block crosses the Path 2 boundary.
pub type MethodFn = fn(&RObj, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

struct ClassEntry {
    /// The full, already-linearized MRO (this class first, then prepends/
    /// includes/superclass in resolution order) -- computed at COMPILE time
    /// by `spinelc::analyze::mro::compute_ancestors` and baked in as a
    /// literal list by `ruby_class!`'s generated `__register`. Method
    /// dispatch itself never needs to walk this (every reachable method is
    /// already MATERIALIZED directly onto this class -- see the plan's Part
    /// 6), but `is_a`/rescue-by-class matching does.
    ancestors: Vec<ClassId>,
    methods: HashMap<Symbol, MethodFn>,
}

#[derive(Default)]
pub struct ClassRegistry {
    entries: HashMap<u32, ClassEntry>,
}

impl ClassRegistry {
    pub fn new() -> ClassRegistry {
        ClassRegistry::default()
    }

    /// Mirrors declaring a class's place in the hierarchy -- called once per
    /// class from that class's generated `__register`, with `ancestors`
    /// already fully linearized at spinelc compile time.
    pub fn register(&mut self, id: ClassId, ancestors: Vec<ClassId>) {
        self.entries.insert(
            id.0,
            ClassEntry {
                ancestors,
                methods: HashMap::new(),
            },
        );
    }

    /// The runtime-mutable path `define_method`/`define_singleton_method`
    /// actually go through: a real `HashMap` insert, no compile-time
    /// literal-name restriction (contrast spinel's `walk_scope`, which can
    /// only register a `define_method` as a scope if the name is a literal).
    pub fn define_method(&mut self, id: ClassId, name: Symbol, f: MethodFn) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before defining methods on it")
            .methods
            .insert(name, f);
    }

    fn lookup(&self, id: ClassId, name: Symbol) -> Option<MethodFn> {
        self.entries.get(&id.0)?.methods.get(&name).copied()
    }

    fn ancestors_of(&self, id: ClassId) -> &[ClassId] {
        self.entries.get(&id.0).map_or(&[], |e| &e.ancestors)
    }
}

/// `recv_class.is_a?(target)` -- a real ancestry check against the SAME
/// linearized `ancestors` list `super`/reflection uses at compile time (see
/// `analyze::mro::compute_ancestors`'s docs), not spinel's own two-tier
/// "transplant for dispatch, shallower list for reflection" split (confirmed
/// to diverge on a module-of-module diamond -- see the plan). The one
/// runtime surface this spike needs for `Signal::Raise`/`rescue` matching:
/// there's no first-class `Class`/`Module` runtime VALUE (can't be stored in
/// a variable or reflected on generally), just this narrow "is this concrete
/// class id ancestor-compatible with that one" check.
pub fn is_a(recv_class: ClassId, target: ClassId) -> bool {
    registry().ancestors_of(recv_class).contains(&target)
}

/// `recv.respond_to?(:name)` -- a flat lookup on the receiver's own
/// already-materialized method table (every reachable method -- own,
/// inherited, or mixed-in -- is already present there, so no ancestor walk
/// is needed, mirroring `send`'s own dispatch below). Matches real Ruby's
/// default behavior (doesn't consult `method_missing`/`respond_to_missing?`,
/// which this spike doesn't model).
pub fn responds_to(recv_class: ClassId, name: Symbol) -> bool {
    registry().lookup(recv_class, name).is_some()
}

/// The class registry is installed exactly once, from generated `main()`,
/// before any `Thread`/`Ractor` spawns anything (Part 9) -- a `OnceLock`
/// (not a `thread_local!`, unlike before the Send+Sync migration) gives
/// lock-free reads forever after that single write, and is itself the
/// correct semantic choice regardless of concurrency: classes/methods are
/// genuinely process-wide-shared in real Ruby, not per-thread state.
static REGISTRY: OnceLock<ClassRegistry> = OnceLock::new();

fn registry() -> &'static ClassRegistry {
    REGISTRY
        .get()
        .expect("class registry not installed -- install_class_registry must run first")
}

/// Called once from generated `main()`, after every class's `__register` has
/// populated the registry passed in.
pub fn install_class_registry(registry: ClassRegistry) {
    REGISTRY
        .set(registry)
        .unwrap_or_else(|_| panic!("class registry installed twice"));
}

/// The general dispatcher -- reached only on Path 2 (see module docs).
/// Since every reachable method (own, inherited, or mixed-in) is already
/// MATERIALIZED directly onto its receiver's own class at spinelc compile
/// time (see the plan's Part 6 -- no cloning-with-shadow-names,
/// monomorphization instead), this is now a FLAT lookup on the receiver's
/// own class -- no ancestor walk needed here at all, only for `is_a`
/// (above), which real dispatch doesn't need.
///
/// `block` is threaded through to whichever `MethodFn` is actually found --
/// including the `method_missing` fallback, matching real Ruby's own
/// `method_missing(name, *args, &block)` protocol.
pub fn send(
    recv: &RObj,
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let id = recv.class_id();
    if let Some(f) = registry().lookup(id, name) {
        return f(recv, args, block);
    }

    // method_missing fallback, with `name` prepended to args (mirrors
    // CRuby's own protocol).
    let mm = Symbol::intern("method_missing");
    if let Some(f) = registry().lookup(id, mm) {
        let mut full_args = Vec::with_capacity(args.len() + 1);
        full_args.push(RubyValue::Symbol(name));
        full_args.extend_from_slice(args);
        return f(recv, &full_args, block);
    }

    // Minimal error handling (see plan Context): match Ruby's real default --
    // an uncaught NoMethodError prints and exits 1. Full `raise`/`rescue`
    // propagation is a later phase; this one message is exercised directly by
    // method_missing.rb.
    eprintln!(
        "undefined method '{name}' for an instance of class {}",
        recv.class_id().0
    );
    std::process::exit(1);
}
