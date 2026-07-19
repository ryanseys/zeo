//! Runtime metaprogramming (#97, stage 1): the seam that fills
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

use crate::dispatch::{
    ancestors_of_value, raise_error, registry_lookup_cloned, ConstructorFn, MethodImpl, RObj,
    RubyObject,
};
use crate::{ClassId, RProc, RubyValue, Signal, Symbol};
use spinel_abi::RUNTIME_CLASS_ID_BASE;
use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

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
    methods: HashMap<Symbol, MethodImpl>,
    /// Class/singleton-on-class methods (`def self.x`, `define_singleton_method`
    /// on a `Class` value): self = the `RubyValue::Class`, which the RObj-shaped
    /// `MethodImpl` can't carry -- so these are the raw `RProc`, invoked via
    /// `call_with_self(class_value, args)`.
    class_methods: HashMap<Symbol, RProc>,
    constructor: Option<ConstructorFn>,
}

impl OverlayEntry {
    /// A pure method-delta over an existing frozen class -- no ancestors, no
    /// constructor of its own.
    fn delta() -> OverlayEntry {
        OverlayEntry {
            name: RwLock::new(None),
            is_module: false,
            ancestors: &[],
            methods: HashMap::new(),
            class_methods: HashMap::new(),
            constructor: None,
        }
    }
}

struct OverlayMaps {
    classes: RwLock<HashMap<u32, OverlayEntry>>,
    /// Per-object singleton methods, keyed by the receiver's `Arc` DATA address
    /// (object identity). Not carried across `dup` -- a fresh `Arc` is a fresh
    /// address -- matching Ruby (`dup` drops singletons). `clone`'s
    /// singleton-carry is a documented fast-follow.
    singletons: RwLock<HashMap<usize, HashMap<Symbol, MethodImpl>>>,
    next_id: AtomicU32,
}

static OVERLAY_LIVE: AtomicBool = AtomicBool::new(false);
static OVERLAY: OnceLock<OverlayMaps> = OnceLock::new();

fn maps() -> &'static OverlayMaps {
    OVERLAY.get_or_init(|| OverlayMaps {
        classes: RwLock::new(HashMap::new()),
        singletons: RwLock::new(HashMap::new()),
        next_id: AtomicU32::new(RUNTIME_CLASS_ID_BASE),
    })
}

/// The dispatch hot-path gate: `true` once anything has been defined at
/// runtime. Read before any overlay probe.
#[inline(always)]
pub fn is_live() -> bool {
    OVERLAY_LIVE.load(Ordering::Acquire)
}

fn mark_live() {
    OVERLAY_LIVE.store(true, Ordering::Release);
}

/// The object identity a per-object singleton table is keyed by: the `Arc`'s
/// data address (drops the vtable half of the fat pointer).
fn obj_identity(o: &RObj) -> usize {
    Arc::as_ptr(o).cast::<()>() as usize
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
pub fn dynamic_from_proc(body: RProc) -> MethodImpl {
    MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], block| {
        let self_val = RubyValue::Object(recv.clone());
        body.call_with_self_and_block(&self_val, args, block)
    }))
}

// ---------------------------------------------------------------------------
// Public runtime API (what F2/F3/F4's builtins + codegen call)
// ---------------------------------------------------------------------------

/// `some_class.define_method(name) { body }` -- install/override an instance
/// method on the class with id `id` (frozen or runtime). Returns the name.
pub fn runtime_define_method(id: ClassId, name: Symbol, body: RProc) -> RubyValue {
    let m = dynamic_from_proc(body);
    {
        let mut w = maps().classes.write().unwrap();
        w.entry(id.0).or_insert_with(OverlayEntry::delta).methods.insert(name, m);
    }
    mark_live();
    RubyValue::Symbol(name)
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
            {
                let mut w = maps().classes.write().unwrap();
                w.entry(cid.0)
                    .or_insert_with(OverlayEntry::delta)
                    .class_methods
                    .insert(name, body);
            }
            mark_live();
            Ok(RubyValue::Symbol(name))
        }
        RubyValue::Object(o) => {
            let key = obj_identity(o);
            let m = dynamic_from_proc(body);
            {
                let mut w = maps().singletons.write().unwrap();
                w.entry(key).or_default().insert(name, m);
            }
            mark_live();
            Ok(RubyValue::Symbol(name))
        }
        other => Err(raise_error(
            "TypeError",
            format!("can't define singleton method for {}", immediate_kind(other)),
        )),
    }
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
            return Err(raise_error("TypeError", "superclass must be a Class".to_string()))
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

    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                name: RwLock::new(None),
                is_module: false,
                ancestors: leaked,
                methods: HashMap::new(),
                class_methods: HashMap::new(),
                constructor: Some(dyn_object_construct),
            },
        );
    }
    mark_live();

    let class_val = RubyValue::Class(new_id);
    if let Some(b) = body {
        b.call_with_self(&class_val, &[])?;
    }
    Ok(class_val)
}

/// Mint a fresh runtime class rooted at `root` (e.g. `STRUCT_CLASS`/
/// `DATA_CLASS`), pre-populated with `methods` and a native `constructor` --
/// the counterpart of `runtime_class_new` for a native-backed class the user
/// never wrote a `class` body for (Batch E's `Struct.new`/`Data.define`). The
/// ancestry is `[new_id, *root.ancestors]`, leaked to `'static` like every
/// other runtime class's.
pub fn intern_native_class(
    root: ClassId,
    methods: HashMap<Symbol, MethodImpl>,
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
                name: RwLock::new(None),
                is_module: false,
                ancestors: leaked,
                methods,
                class_methods: HashMap::new(),
                constructor: Some(constructor),
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
        }
    }
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
        if !s.is_empty() {
            if let Some(m) = s.get(&obj_identity(recv)).and_then(|t| t.get(&name)) {
                return Some(m.clone());
            }
        }
    }
    if id.0 >= RUNTIME_CLASS_ID_BASE {
        // 2a. Runtime class: walk its ancestors (overlay methods, then frozen
        // materialized methods on a frozen ancestor).
        walk_runtime_class(id, name)
    } else {
        // 2b. Frozen class: a runtime override delta only.
        let c = maps().classes.read().unwrap();
        c.get(&id.0).and_then(|e| e.methods.get(&name).cloned())
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
            if let Some(m) = c.get(&anc.0).and_then(|e| e.methods.get(&name).cloned()) {
                return Some(m);
            }
        }
        if let Some(m) = registry_lookup_cloned(anc, name) {
            return Some(m);
        }
    }
    None
}

/// A class-level method (`def self.x` / `define_singleton_method` on a class)
/// for a `RubyValue::Class` receiver -- the raw block, invoked under the class.
pub fn overlay_class_method(id: ClassId, name: Symbol) -> Option<RProc> {
    let c = maps().classes.read().unwrap();
    c.get(&id.0)?.class_methods.get(&name).cloned()
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

/// Whether `recv` (an object) has a per-object singleton method `name` --
/// `respond_to?`'s identity-keyed probe, since the class-id walk can't see a
/// singleton installed on one specific object.
pub fn object_has_singleton_method(recv: &RubyValue, name: Symbol) -> bool {
    let RubyValue::Object(o) = recv else { return false };
    maps()
        .singletons
        .read()
        .unwrap()
        .get(&obj_identity(o))
        .is_some_and(|t| t.contains_key(&name))
}

/// Whether class `id`'s OWN overlay table defines instance method `name` -- a
/// runtime `define_method` delta on a frozen class, or a runtime class's own
/// method. `respond_to?`'s ancestor walk consults this per ancestor.
pub fn overlay_has_instance_method(id: ClassId, name: Symbol) -> bool {
    maps().classes.read().unwrap().get(&id.0).is_some_and(|e| e.methods.contains_key(&name))
}

/// A runtime class's leaked ancestor chain -- `None` for a frozen id or a pure
/// method-delta (empty ancestors). Feeds `ancestors_of_value`.
pub fn overlay_ancestors(id: ClassId) -> Option<&'static [ClassId]> {
    let c = maps().classes.read().unwrap();
    let anc = c.get(&id.0)?.ancestors;
    if anc.is_empty() {
        None
    } else {
        Some(anc)
    }
}

/// A runtime class's Ruby-visible name (or the anonymous `#<Class:ID>` form).
pub fn overlay_class_name(id: ClassId) -> Option<String> {
    let c = maps().classes.read().unwrap();
    let entry = c.get(&id.0)?;
    if entry.ancestors.is_empty() {
        return None; // a pure delta over a frozen class carries no name of its own
    }
    let name = entry.name.read().unwrap().clone();
    Some(name.unwrap_or_else(|| format!("#<Class:0x{:08x}>", id.0)))
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
    known.then(|| RubyValue::Object(Arc::new(DynObject::new(id))))
}

/// Coerce a `define_method`/`define_singleton_method` NAME argument (a Symbol
/// or String) to a `Symbol` -- CRuby's `rb_to_id`.
pub(crate) fn coerce_method_name(arg: Option<&RubyValue>) -> Result<Symbol, Signal> {
    match arg {
        Some(RubyValue::Symbol(s)) => Ok(*s),
        Some(RubyValue::Str(s)) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        _ => Err(raise_error(
            "TypeError",
            "expected a Symbol or String for the method name".to_string(),
        )),
    }
}

/// The block that becomes the method body: the passed block, or a `Proc` given
/// as the second positional argument (`define_method(:x, some_proc)`). A
/// `Method`/`UnboundMethod` second argument is a documented fast-follow.
pub(crate) fn coerce_method_body(
    args: &[RubyValue],
    block: &Option<RubyValue>,
) -> Result<RProc, Signal> {
    if let Some(RubyValue::Proc(p)) = block {
        return Ok(p.clone());
    }
    if let Some(RubyValue::Proc(p)) = args.get(1) {
        return Ok(p.clone());
    }
    Err(raise_error(
        "ArgumentError",
        "tried to create Proc object without a block".to_string(),
    ))
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
        return Err(raise_error(
            "ArgumentError",
            format!("wrong number of arguments (given {}, expected 0)", args.len()),
        ));
    }
    Ok(RubyValue::Object(obj))
}

/// An instance of a runtime-created class. Like the root `Object`, its ivars
/// are name-keyed (a runtime class has no compile-time-materialized field
/// list), plus a stored `class_id` (unlike `Object`'s hardcoded `0`) and a
/// per-object frozen flag.
struct DynObject {
    class_id: ClassId,
    frozen: AtomicBool,
    ivars: parking_lot::Mutex<HashMap<String, RubyValue>>,
}

impl DynObject {
    fn new(class_id: ClassId) -> DynObject {
        DynObject {
            class_id,
            frozen: AtomicBool::new(false),
            ivars: parking_lot::Mutex::new(HashMap::new()),
        }
    }
}

impl RubyObject for DynObject {
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
        self.ivars.lock().iter().map(|(k, v)| (format!("@{k}"), v.clone())).collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        // A never-assigned ivar reads nil (Ruby's rule), so `Some(Nil)` not
        // `None` -- a name-keyed object always "has" the slot.
        Some(self.ivars.lock().get(name).cloned().unwrap_or(RubyValue::Nil))
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        self.ivars.lock().insert(name.to_string(), v);
        true
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = DynObject::new(self.class_id);
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
        runtime_define_method(id, name, nullary(42));
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
        let RubyValue::Class(parent_id) = parent else { panic!() };
        let greet = Symbol::intern("greet");
        runtime_define_method(parent_id, greet, nullary(99));

        let child = runtime_class_new(Some(parent), None).unwrap();
        let RubyValue::Class(child_id) = child else { panic!() };
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
                    runtime_define_method(*cid, Symbol::intern("x"), nullary(5));
                }
                Ok(RubyValue::Nil)
            },
            RubyValue::Nil,
            0,
            false,
        );
        let klass = runtime_class_new(None, Some(body)).unwrap();
        let RubyValue::Class(cid) = klass else { panic!() };
        let inst = dyn_object_construct(cid, &[], None).unwrap();
        let RubyValue::Object(o) = inst else { panic!() };
        let m = resolve_dynamic(&o, cid, Symbol::intern("x")).unwrap();
        assert_eq!(int_of(m.call(&o, &[], None).unwrap()), 5);
    }
}
