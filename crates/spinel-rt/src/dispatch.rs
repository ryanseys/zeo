//! Object model & dynamic dispatch. See the plan's "Object model & dynamic
//! dispatch" section for the full design rationale: two dispatch paths
//! (static direct/match-on-class_id calls, generated entirely by `spinelc`;
//! and this module's runtime `ClassRegistry`/`send`, reached only when
//! `spinelc` cannot resolve a call statically). Spinel itself never needs
//! this module at all -- it's the one deliberate architectural addition.

use crate::{RubyValue, Signal, Symbol};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Identifies a Ruby class at runtime. Mirrors spinel's struct-embedded
/// `cls_id` field (`emit_class_struct`, codegen.c:2496).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(pub u32);

/// Implemented (via `ruby_class!`) by every generated Ruby class, and by the
/// built-in `Object` root below.
pub trait RubyObject: Any {
    fn class_id(&self) -> ClassId;

    /// Lets `send`'s dispatcher downcast an erased `RObj` back to its
    /// concrete type. No default body: a default `{ self }` here would be
    /// type-checked generically over an unsized `Self` (rustc checks default
    /// bodies once, not per-impl) and fail to coerce to `&dyn Any`. Each
    /// concrete impl provides the one-line body instead (`ruby_class!`
    /// generates it), where `Self` is always a concrete, sized type.
    fn as_any(&self) -> &dyn Any;

    /// The owned-handle counterpart to `as_any` -- needed because every
    /// generated method now takes `self: Rc<Self>` (not `&self`, see
    /// `ruby_class!`'s docs), so Path 2's dynamic trampolines need an
    /// `Rc<Concrete>`, not a `&Concrete`, to actually call one. `Rc<dyn Any>`
    /// supports a real consuming `downcast::<T>()`; `Rc<dyn RubyObject>`
    /// doesn't (there's no such inherent method on an arbitrary trait
    /// object), so this is the bridge -- same "no default body" reasoning as
    /// `as_any` above.
    fn as_any_rc(self: Rc<Self>) -> Rc<dyn Any>;
}

/// A handle to any live Ruby object, used wherever the concrete class isn't
/// statically known. Mirrors spinel's boxed `sp_RbVal { tag: SP_TAG_OBJ,
/// cls_id, v: { p } }` (`lib/sp_gc.h:42`) for the object case -- except the
/// Rust trait object's vtable *is* the tag. Mutability lives on individual
/// ivar fields (see `ruby_class!`), not on this handle, so no `RefCell` layer
/// is needed here.
pub type RObj = Rc<dyn RubyObject>;

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
    fn as_any_rc(self: Rc<Self>) -> Rc<dyn Any> {
        self
    }
}

/// Downcasts an erased `RObj` to an owned `Rc<T>` -- the Path 2 trampoline
/// counterpart to `as_any().downcast_ref()`, needed because every generated
/// method takes `self: Rc<Self>` now (see `ruby_class!`'s docs). Cloning
/// `recv` first is a cheap `Rc` refcount bump, not a deep copy.
pub fn downcast_robj<T: RubyObject>(recv: &RObj) -> Option<Rc<T>> {
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
    superclass: Option<ClassId>,
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
    /// class from that class's generated `__register`.
    pub fn register(&mut self, id: ClassId, superclass: Option<ClassId>) {
        self.entries.insert(
            id.0,
            ClassEntry {
                superclass,
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

    fn superclass_of(&self, id: ClassId) -> Option<ClassId> {
        self.entries.get(&id.0)?.superclass
    }
}

thread_local! {
    static REGISTRY: RefCell<ClassRegistry> = RefCell::new(ClassRegistry::new());
}

/// Called once from generated `main()`, after every class's `__register` has
/// populated the registry passed in.
pub fn install_class_registry(registry: ClassRegistry) {
    REGISTRY.with(|r| *r.borrow_mut() = registry);
}

/// The general dispatcher -- reached only on Path 2 (see module docs). Mirrors
/// the *shape* of `comp_method_in_chain` (compiler.c:404): walk the class,
/// then its superclass, etc. -- but this walk happens at runtime, over a
/// table generated code populated and can still mutate. This is the question
/// spinel's whole architecture is built to never have to ask.
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
    let mut class = Some(recv.class_id());
    while let Some(id) = class {
        let found = REGISTRY.with(|r| r.borrow().lookup(id, name));
        if let Some(f) = found {
            return f(recv, args, block);
        }
        class = REGISTRY.with(|r| r.borrow().superclass_of(id));
    }

    // method_missing fallback: same chain walk, looking for `method_missing`
    // instead, with `name` prepended to args (mirrors CRuby's own protocol).
    let mm = Symbol::intern("method_missing");
    let mut class = Some(recv.class_id());
    while let Some(id) = class {
        let found = REGISTRY.with(|r| r.borrow().lookup(id, mm));
        if let Some(f) = found {
            let mut full_args = Vec::with_capacity(args.len() + 1);
            full_args.push(RubyValue::Symbol(name));
            full_args.extend_from_slice(args);
            return f(recv, &full_args, block);
        }
        class = REGISTRY.with(|r| r.borrow().superclass_of(id));
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
