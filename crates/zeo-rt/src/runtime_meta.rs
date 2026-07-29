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
    ConstructorFn, MethodImpl, RObj, RubyObject, ancestors_of_value,
    registry_lookup_cloned, send_super_from,
};
use crate::{ClassId, RProc, RubyValue, Signal, Symbol};
use std::any::Any;
use std::cell::RefCell;
use crate::{FMap, FSet};
use std::collections::{HashMap, HashSet};
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
    methods: FMap<Symbol, MethodImpl>,
    /// Explicit RUNTIME visibility marks (`Foo.class_eval { private :m }`,
    /// an alias inheriting its source's visibility): nearest ancestor's mark
    /// wins in `instance_method_visibility`'s walk. A name absent here but
    /// present in `methods` is public (a runtime `define_method` is). Keyed
    /// separately from `methods` because a mark can target a FROZEN-registry
    /// or builtin method the overlay never carries a body for.
    methods_vis: FMap<Symbol, crate::dispatch::MethodVisibility>,
    /// Class/singleton-on-class methods (`def self.x`, `define_singleton_method`
    /// on a `Class` value): self = the `RubyValue::Class`, which the RObj-shaped
    /// `MethodImpl` can't carry -- so these are the raw `RProc`, invoked via
    /// `call_with_self(class_value, args)`.
    class_methods: FMap<Symbol, RProc>,
    constructor: Option<ConstructorFn>,
    /// Names `undef_method` removed. An ENTRY, not a deletion: it TERMINATES
    /// the MRO walk here, so an ancestor's still-live definition can't answer
    /// for a descendant that undef'd the name. Mirrors the frozen registry's
    /// `ClassEntry::undefined_methods`.
    undefs: FSet<Symbol>,
    /// The address the anonymous `#<Class:0x...>` rendering reports -- a real
    /// leaked allocation, so it is unique, stable, and 16 hex digits wide like
    /// every other object's. Not `ancestors.as_ptr()`: `include`/`prepend`
    /// re-leak that slice.
    addr: usize,
}

impl Default for OverlayEntry {
    fn default() -> Self {
        OverlayEntry {
            name: RwLock::new(None),
            is_module: false,
            ancestors: &[],
            methods: FMap::default(),
            methods_vis: FMap::default(),
            class_methods: FMap::default(),
            constructor: None,
            undefs: FSet::default(),
            addr: Box::leak(Box::new(0u8)) as *const u8 as usize,
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
    /// address -- matching Ruby (`dup` drops singletons). `clone`'s
    /// singleton-carry is a documented fast-follow.
    singletons: RwLock<FMap<usize, FMap<Symbol, MethodImpl>>>,
    /// Singleton methods on a NON-object heap value (`def SOME_ARRAY.[](i)`),
    /// keyed the same way. Separate from `singletons` because there is no
    /// `RObj` to bind: the body stays an `RProc` and runs with the value itself
    /// as `self`. See `value_identity`.
    value_singletons: RwLock<FMap<usize, FMap<Symbol, RProc>>>,
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

static OVERLAY_LIVE: AtomicBool = AtomicBool::new(false);
static OVERLAY: OnceLock<OverlayMaps> = OnceLock::new();

fn maps() -> &'static OverlayMaps {
    OVERLAY.get_or_init(|| OverlayMaps {
        classes: RwLock::new(FMap::default()),
        singletons: RwLock::new(FMap::default()),
        value_singletons: RwLock::new(FMap::default()),
        singleton_classes: RwLock::new(FMap::default()),
        singleton_owner: RwLock::new(FMap::default()),
        next_id: AtomicU32::new(RUNTIME_CLASS_ID_BASE),
    })
}

/// The dispatch hot-path gate: `true` once anything has been defined at
/// runtime. Read before any overlay probe.
#[inline(always)]
pub fn is_live() -> bool {
    OVERLAY_LIVE.load(Ordering::Acquire)
}

/// The typed-iterator fusion gate (`codegen`'s `emit_typed_iter_inline`): a
/// fused native loop bypasses dispatch entirely, which is only sound while
/// nothing could have overridden the builtin iterator -- no runtime-defined
/// method anywhere (a reopen or a per-object singleton would win the lookup)
/// and not inside a box (a box may carry its own override). The same rule the
/// flat dispatch maps apply, asked once per loop entry.
#[inline(always)]
pub fn iter_inline_ok(box_id: u32) -> bool {
    box_id == 0 && !is_live()
}

fn mark_live() {
    OVERLAY_LIVE.store(true, Ordering::Release);
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

/// Whether a `def` running with `recv` as its self installs on the singleton
/// class -- true exactly inside an `instance_eval`/`instance_exec` of `recv`.
pub(crate) fn singleton_definee(recv: &RubyValue) -> bool {
    SINGLETON_DEFINEE.with(|s| {
        s.borrow()
            .last()
            .is_some_and(|open| match (open, recv) {
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
    // redirect to the owner. For a class owner it becomes a class method.
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner {
        return runtime_define_singleton_method(&owner, name, body);
    }
    let m = dynamic_from_proc(id, name, body);
    let frame = current_frame_for(id);
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(name, m);
        e.undefs.remove(&name);
        match frame {
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
    if frame.is_some_and(|f| f.module_function) {
        if let Some(wrapper) = extended_class_method(id, name) {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
            e.class_methods.insert(name, wrapper);
        }
    }
    mark_live();
    Ok(RubyValue::Symbol(name))
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
    let mut defined = Vec::new();
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if kind != AttrKind::Writer {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let getter = MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], _| {
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
            let setter = MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], _| {
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
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if !crate::dispatch::responds_to(id, name, true) {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(id).unwrap_or_else(|| "?".to_string())
            ));
        }
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.undefs.insert(name);
        e.methods.remove(&name);
    }
    mark_live();
    Ok(RubyValue::Class(id))
}

/// `Module#remove_method` -- drops this class's OWN definition, leaving an
/// inherited one reachable. Unlike `undef_method` it plants no tombstone.
pub fn runtime_remove_method(id: ClassId, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        let mut w = maps().classes.write().unwrap();
        let removed = w
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
    }
    mark_live();
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
        // Bare `private` switches the enclosing body's default for every
        // subsequent `def`.
        update_frame_for(id, |f| f.vis = vis);
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
            e.methods_vis
                .insert(sym, crate::dispatch::MethodVisibility::Private);
        }
    }
    mark_live();
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
        other => {
            // Any other heap value (`def SOME_ARRAY.[](i)`) -- see
            // `value_identity`. The body stays an `RProc`, called with the value
            // itself as `self`, because a bare Array has no `RObj` to bind.
            let Some(key) = value_identity(other) else {
                return Err(type_error!("can't define singleton"));
            };
            // `nil`/`true`/`false` report as frozen but still accept a
            // singleton -- CRuby's singleton class for them IS their class, and
            // takes no frozen check (oracle-verified). Every other value does.
            if !matches!(other, RubyValue::Nil | RubyValue::Bool(_)) {
                crate::builtins::check_frozen(recv)?;
            }
            {
                let mut w = maps().value_singletons.write().unwrap();
                w.entry(key).or_default().insert(name, body);
            }
            mark_live();
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
pub fn runtime_extend(recv: &RubyValue, module_val: &RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::class_name_of(module_val)
        ));
    };
    let names = module_extendable_method_names(*mid);
    match recv {
        RubyValue::Object(o) => {
            // CRuby's rb_check_frozen: a frozen object refuses `extend` (its
            // singleton table is what would change).
            crate::builtins::check_frozen(recv)?;
            let key = obj_identity(o);
            let mut w = maps().singletons.write().unwrap();
            let table = w.entry(key).or_default();
            for name in names {
                if let Some(m) = module_own_method_impl(*mid, name) {
                    table.insert(name, m);
                }
            }
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
            let Some(key) = value_identity(other) else {
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
            let mut w = maps().value_singletons.write().unwrap();
            let table = w.entry(key).or_default();
            for (name, proc_) in installs {
                table.insert(name, proc_);
            }
        }
    }
    mark_live();
    fire_mixin_hook(module_val, "extended", recv)?;
    Ok(recv.clone())
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
    if crate::dispatch::class_frozen(*cid) {
        return Err(crate::dispatch::frozen_class_error(*cid));
    }
    // `prepend A, B` puts B closest to self and `include A, B` puts B closest
    // too, so both walk the arguments in the order that lands them there.
    let ordered: Vec<&RubyValue> = match placement {
        Placement::After => modules.iter().rev().collect(),
        Placement::Before => modules.iter().collect(),
    };
    for module_val in ordered {
        let RubyValue::Class(mid) = module_val else {
            return Err(type_error!(
                "wrong argument type {} (expected Module)",
                crate::builtins::class_name_of(module_val)
            ));
        };
        splice_module_into(*cid, *mid, placement);
        let hook = if placement == Placement::Before {
            "prepended"
        } else {
            "included"
        };
        fire_mixin_hook(module_val, hook, recv)?;
    }
    mark_live();
    Ok(recv.clone())
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
    if crate::dispatch::class_method_owner(*mid, sym).is_none() {
        return Ok(());
    }
    crate::dispatch::send_value(module, sym, std::slice::from_ref(target), None)?;
    Ok(())
}

/// Splices `mid`'s ancestry into `cid`'s, at `cid`'s own position, skipping any
/// ancestor already present. Computes the new chain BEFORE taking the write
/// lock (`ancestors_of_value` reads it); the old slice leaks, matching this
/// runtime's no-GC policy for interned ancestries.
fn splice_module_into(cid: ClassId, mid: ClassId, placement: Placement) {
    let current: Vec<ClassId> = ancestors_of_value(cid).to_vec();
    // Not `current[0]`: after a prepend, self is no longer first.
    let Some(at) = current.iter().position(|&a| a == cid) else {
        return;
    };
    let present: HashSet<ClassId> = current.iter().copied().collect();
    let fresh: Vec<ClassId> = ancestors_of_value(mid)
        .iter()
        .copied()
        .filter(|m| !present.contains(m))
        .collect();
    let cut = if placement == Placement::Before { at } else { at + 1 };
    let mut new_anc = Vec::with_capacity(current.len() + fresh.len());
    new_anc.extend_from_slice(&current[..cut]);
    new_anc.extend_from_slice(&fresh);
    new_anc.extend_from_slice(&current[cut..]);
    let leaked: &'static [ClassId] = Box::leak(new_anc.into_boxed_slice());
    let mut w = maps().classes.write().unwrap();
    w.entry(cid.0).or_insert_with(OverlayEntry::delta).ancestors = leaked;
}

/// The public/protected instance-method names a module contributes to a host
/// via `extend`/`include` -- registry methods plus any runtime-overlay ones a
/// `Module.new` added.
fn module_extendable_method_names(mid: ClassId) -> Vec<Symbol> {
    let mut names =
        crate::dispatch::instance_method_names(mid, crate::dispatch::VisFilter::NotPrivate, false);
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
    // A USER module method is a compiled `&RObj` body: it needs an object
    // receiver. When invoked with a Class `self` (the module value itself),
    // run it against a fresh blank instance of that class. Self-contained
    // methods work; a method that reaches a Class-level sibling or ivar is a
    // documented limitation (zeo represents modules as class values, not
    // objects, so there is no module instance to bind).
    // An INCLUDED module's method counts: `module_function :greet` after
    // `include Greeting` promotes the mixed-in body, and `extend self` reaches
    // the same way. Own definitions win, so the ancestry walk is only the
    // fallback.
    let m = module_own_method_impl(mid, name).or_else(|| snapshot_instance_method(mid, name))?;
    Some(RProc::with_self_and_block(
        move |self_val, args, block| match self_val {
            RubyValue::Object(o) => m.call(o, args, block),
            RubyValue::Class(cid) => {
                let surrogate: crate::dispatch::RObj = Arc::new(DynObject::new(*cid));
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
pub fn overlay_class_method_names(id: ClassId) -> Vec<Symbol> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .map(|e| e.class_methods.keys().copied().collect())
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
    // A CLASS receiver's singleton is named after the class itself --
    // CRuby's `#<Class:Melody>` -- not after `real` (the class of a Class
    // is `Class`, which would name every one `#<Class:Class>`).
    let named = match recv {
        RubyValue::Class(cid) => *cid,
        _ => real,
    };
    let real_name = crate::dispatch::class_name(named).unwrap_or_else(|| "Object".to_string());
    {
        let mut w = maps().classes.write().unwrap();
        w.insert(
            id_num,
            OverlayEntry {
                name: RwLock::new(Some(format!("#<Class:{real_name}>"))),
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
                is_module: true,
                ancestors: leaked,
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
    let constructor = if leaked
        .iter()
        .copied()
        .any(crate::builtins::value_subclass::is_payload_root)
    {
        crate::builtins::value_subclass::value_subclass_construct
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
            let entry = c.get(&anc.0);
            // An undef here terminates the walk -- an ancestor's still-live
            // definition must not answer past it.
            if entry.is_some_and(|e| e.undefs.contains(&name)) {
                return None;
            }
            if let Some(m) = entry.and_then(|e| e.methods.get(&name).cloned()) {
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
    if let Some(name) = entry.name.read().unwrap().clone() {
        return Some(name);
    }
    let kind = if entry.is_module { "Module" } else { "Class" };
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
