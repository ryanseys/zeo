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

use crate::builtins::{arg_error, name_error, runtime_error, type_error};
use crate::dispatch::{
    ConstructorFn, MethodImpl, RObj, RubyObject, ancestors_of_value, raise_error,
    registry_lookup_cloned, send_super_from,
};
use crate::{ClassId, RProc, RubyValue, Signal, Symbol};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
    methods: HashMap<Symbol, MethodImpl>,
    /// Explicit RUNTIME visibility marks (`Foo.class_eval { private :m }`,
    /// an alias inheriting its source's visibility): nearest ancestor's mark
    /// wins in `instance_method_visibility`'s walk. A name absent here but
    /// present in `methods` is public (a runtime `define_method` is). Keyed
    /// separately from `methods` because a mark can target a FROZEN-registry
    /// or builtin method the overlay never carries a body for.
    methods_vis: HashMap<Symbol, crate::dispatch::MethodVisibility>,
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
            methods_vis: HashMap::new(),
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
    /// `obj.singleton_class`'s cache: object identity -> the runtime class id
    /// minted for its singleton class (so a second call answers the same id,
    /// matching Ruby's identity).
    singleton_classes: RwLock<HashMap<usize, ClassId>>,
    /// The inverse plus the owner value: a singleton-class id -> the object (or
    /// class) it belongs to. A `define_method` on that id installs a per-object
    /// singleton (or, for a class owner, a class method) rather than an ordinary
    /// instance method -- which is exactly what `class << obj` semantics mean.
    singleton_owner: RwLock<HashMap<u32, RubyValue>>,
    next_id: AtomicU32,
}

static OVERLAY_LIVE: AtomicBool = AtomicBool::new(false);
static OVERLAY: OnceLock<OverlayMaps> = OnceLock::new();

fn maps() -> &'static OverlayMaps {
    OVERLAY.get_or_init(|| OverlayMaps {
        classes: RwLock::new(HashMap::new()),
        singletons: RwLock::new(HashMap::new()),
        singleton_classes: RwLock::new(HashMap::new()),
        singleton_owner: RwLock::new(HashMap::new()),
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
    // redirect to the owner. For a class owner it becomes a class method.
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner {
        return runtime_define_singleton_method(&owner, name, body);
    }
    let m = dynamic_from_proc(id, name, body);
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(name, m);
        // A runtime redefinition is PUBLIC (CRuby's `define_method` at
        // runtime scope) -- drop any earlier `private :name` mark.
        e.methods_vis.remove(&name);
    }
    mark_live();
    Ok(RubyValue::Symbol(name))
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
    mark_live();
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
        [RubyValue::Array(a)] => a.lock().clone(),
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
    mark_live();
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

/// The `MethodImpl` that instance method `name` resolves to for instances of
/// `id`, in dispatch order per ancestor: overlay delta, registry method,
/// builtin-reopen value method, builtin table row -- the latter two wrapped
/// into the `MethodImpl` ABI (they run against a boxed receiver, so the
/// resulting alias serves `RObj` dispatch, which is where the overlay is
/// probed). A compile-time builtin-alias row resolves through its target
/// (rows are terminal, so the single recursion can't loop).
fn snapshot_instance_method(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    let n = name.name();
    let n = n.as_str();
    for &anc in ancestors_of_value(id) {
        {
            let c = maps().classes.read().unwrap();
            if let Some(m) = c.get(&anc.0).and_then(|e| e.methods.get(&name).cloned()) {
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
    crate::dispatch::alias_target(id, name).and_then(|old| snapshot_instance_method(id, old))
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
    if !ancestors_of_value(id).contains(&owner) {
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
    let m = snapshot_instance_method(id, src_name).ok_or_else(|| {
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
    mark_live();
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
    if !ancestors_of_value(recv_class).contains(&owner) {
        return Err(type_error!(
            "bind argument must be a subclass of {}",
            crate::dispatch::class_name(owner).unwrap_or_default()
        ));
    }
    // Snapshot as resolved for the RECEIVER's class (layout-correct copy) --
    // see the note in `runtime_define_method_from_method`.
    let m = snapshot_instance_method(recv_class, src_name).ok_or_else(|| {
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
            w.entry(cid.0)
                .or_insert_with(OverlayEntry::delta)
                .class_methods
                .insert(name, wrapped);
            mark_live();
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
            mark_live();
            Ok(RubyValue::Symbol(name))
        }
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
            // CRuby's rb_check_frozen on the singleton's attachee: a frozen
            // object refuses new singleton methods.
            crate::builtins::check_frozen(recv)?;
            let key = obj_identity(o);
            let m = dynamic_from_proc(SINGLETON_DEFINING, name, body);
            {
                let mut w = maps().singletons.write().unwrap();
                w.entry(key).or_default().insert(name, m);
            }
            mark_live();
            Ok(RubyValue::Symbol(name))
        }
        other => Err(type_error!(
            "can't define singleton method for {}",
            immediate_kind(other)
        )),
    }
}

/// `obj.extend(Mod)` -- mix a module's instance methods into the receiver's
/// singleton, so they resolve on `obj` (and only `obj`). Implemented by
/// copying the module's own public/protected methods into the identity-keyed
/// singleton table `resolve_dynamic` already consults first -- no new dispatch
/// path. An existing singleton method (`def obj.x`) is not clobbered.
pub fn runtime_extend(recv: &RubyValue, module_val: &RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::class_name_of(module_val)
        ));
    };
    let RubyValue::Object(o) = recv else {
        // Immediates have no singleton storage in this runtime (same posture as
        // `define_singleton_method`).
        return Err(type_error!("can't extend {}", immediate_kind(recv)));
    };
    // CRuby's rb_check_frozen: a frozen object refuses `extend` (its
    // singleton table is what would change).
    crate::builtins::check_frozen(recv)?;
    let mut names =
        crate::dispatch::instance_method_names(*mid, crate::dispatch::VisFilter::NotPrivate, false);
    // A runtime module (`Module.new` + `define_method`) keeps its methods in the
    // overlay, which the registry-based enumeration above can't see -- add them.
    for n in overlay_own_method_names(*mid) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let key = obj_identity(o);
    {
        let mut w = maps().singletons.write().unwrap();
        let table = w.entry(key).or_default();
        for name in names {
            if let Some(m) = module_own_method_impl(*mid, name) {
                table.insert(name, m);
            }
        }
    }
    mark_live();
    Ok(recv.clone())
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

fn module_own_method_impl(mid: ClassId, name: Symbol) -> Option<MethodImpl> {
    overlay_own_method(mid, name)
        .or_else(|| crate::dispatch::registry_lookup_cloned(mid, name))
        .or_else(|| crate::dispatch::registry_value_method_impl(mid, name))
        .or_else(|| builtin_module_method_impl(mid, name))
}

/// The `MethodImpl` a BUILTIN module (`Comparable`/`Enumerable`/`Math`, or any
/// module with a hardcoded `class_table`) defines for `name`. These don't live
/// in the registry -- their bodies are static method-table rows (or, for
/// `Math`, the pre-table `math_call` dispatcher) -- so each is wrapped in a
/// `Dynamic` closure that re-dispatches by name. This is what lets
/// `obj.extend(Comparable)` install `clamp`/`between?` etc.
fn builtin_module_method_impl(mid: ClassId, name: Symbol) -> Option<MethodImpl> {
    use zeo_abi::MATH_CLASS;
    let miss = move || {
        raise_error(
            "NoMethodError",
            format!("undefined method '{}'", name.name()),
        )
    };
    match mid {
        MATH_CLASS => Some(MethodImpl::Dynamic(std::sync::Arc::new(
            move |_recv: &RObj, args: &[RubyValue], _b| {
                crate::builtins::math::math_call(&name.name(), args).unwrap_or_else(|| Err(miss()))
            },
        ))),
        _ => {
            let f = crate::builtins::class_table(mid)?(&name.name())?;
            Some(MethodImpl::Dynamic(std::sync::Arc::new(
                move |recv: &RObj, args: &[RubyValue], b| {
                    f(&RubyValue::Object(recv.clone()), args, b)
                },
            )))
        }
    }
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
    let cache_key: Option<usize> = match recv {
        RubyValue::Object(o) => Some(obj_identity(o)),
        RubyValue::Class(cid) => Some(cid.0 as usize | (1usize << 48)),
        RubyValue::Nil
        | RubyValue::Bool(_)
        | RubyValue::Int(_)
        | RubyValue::BigInt(_)
        | RubyValue::Float(_)
        | RubyValue::Rational(_)
        | RubyValue::Complex(_)
        | RubyValue::Symbol(_) => {
            return Err(type_error!("can't define singleton"));
        }
        _ => None,
    };
    let real = recv.class_id();
    let owner = recv.clone();
    if let Some(k) = cache_key {
        if let Some(&sid) = maps().singleton_classes.read().unwrap().get(&k) {
            return Ok(RubyValue::Class(sid));
        }
    }
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let super_chain = ancestors_of_value(real);
    let mut anc = Vec::with_capacity(super_chain.len() + 1);
    anc.push(new_id);
    anc.extend_from_slice(super_chain);
    let leaked: &'static [ClassId] = Box::leak(anc.into_boxed_slice());
    let real_name = crate::dispatch::class_name(real).unwrap_or_else(|| "Object".to_string());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                name: RwLock::new(Some(format!("#<Class:{real_name}>"))),
                is_module: false,
                ancestors: leaked,
                methods: HashMap::new(),
                methods_vis: HashMap::new(),
                class_methods: HashMap::new(),
                constructor: None,
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
    mark_live();
    Ok(RubyValue::Class(new_id))
}

/// `Module.new { body }` -- allocate a runtime MODULE id (ancestors = just
/// itself, no superclass, no constructor -- `Module.new.new` is a NoMethodError)
/// and run the optional body block with `self` bound to it. The result composes
/// with `obj.extend`/`include`: its `define_method`-installed methods are
/// retrievable by id from the overlay.
pub fn runtime_module_new(body: Option<RProc>) -> Result<RubyValue, Signal> {
    let id_num = maps().next_id.fetch_add(1, Ordering::Relaxed);
    let new_id = ClassId(id_num);
    let leaked: &'static [ClassId] = Box::leak(vec![new_id].into_boxed_slice());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                name: RwLock::new(None),
                is_module: true,
                ancestors: leaked,
                methods: HashMap::new(),
                methods_vis: HashMap::new(),
                class_methods: HashMap::new(),
                constructor: None,
            },
        );
    }
    mark_live();
    let val = RubyValue::Class(new_id);
    if let Some(b) = body {
        b.call_with_self(&val, &[])?;
    }
    Ok(val)
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

    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                name: RwLock::new(None),
                is_module: false,
                ancestors: leaked,
                methods: HashMap::new(),
                methods_vis: HashMap::new(),
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
                methods_vis: HashMap::new(),
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
        // 2b. Frozen class: check this class AND every frozen ancestor for a
        // runtime method delta, so a `class_eval`/`define_method`/`class_exec`
        // that reopened a SUPERCLASS or an included MODULE is inherited (a
        // subclass instance / an includer sees it). The frozen registry's own
        // materialized methods are consulted separately by `send_in`'s MRO
        // walk; here we only add the overlay deltas, self-first.
        let chain = ancestors_of_value(id);
        let c = maps().classes.read().unwrap();
        chain
            .iter()
            .find_map(|anc| c.get(&anc.0).and_then(|e| e.methods.get(&name).cloned()))
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
    let RubyValue::Object(o) = recv else {
        return false;
    };
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
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .is_some_and(|e| e.methods.contains_key(&name))
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
    let name = entry.name.read().unwrap().clone();
    Some(name.unwrap_or_else(|| format!("#<Class:0x{:08x}>", id.0)))
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
    known.then(|| RubyValue::Object(Arc::new(DynObject::new(id))))
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
    args: &[RubyValue],
    block: &Option<RubyValue>,
) -> Result<RProc, Signal> {
    if let Some(RubyValue::Proc(p)) = block {
        return Ok(p.clone());
    }
    if let Some(RubyValue::Proc(p)) = args.get(1) {
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
        self.ivars.lock().remove(name)
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
}
