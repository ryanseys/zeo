//! `ObjectSpace::WeakMap`/`WeakKeyMap` (CRuby weakmap.c) and the finalizer
//! registry behind `ObjectSpace.define_finalizer`. `WeakRef` itself is the
//! vendored `gems/weakref` -- CRuby's own pure-Ruby file, which stands on
//! `Delegator` and on the `WeakMap` here. The `ObjectSpace` module
//! itself is next door in `objspace.rs`, which stands on the registry here.
//!
//! zeo's memory model is `Arc` refcounting, not a tracing collector (the
//! plan's accepted trade -- cycles leak). That shapes what this module can
//! honestly offer: a `WeakMap` holds `Arc::downgrade`d handles to its keys and
//! values, so an entry vanishes once the last STRONG reference elsewhere
//! drops. Dead entries are pruned lazily on access and eagerly at `GC.start`.
//! Only the common heap kinds (`Object`/`String`/`Array`/`Hash`) are tracked
//! weakly; every other value -- immediates, and the rarer Arc-backed kinds
//! (`Proc`/`Regexp`/...) -- is held STRONGLY, a bounded, documented
//! best-effort divergence (weak-referencing those is rare, and a strongly-held
//! entry simply never expires rather than expiring wrongly).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::collections::{Freezable, RHashData, array_new, string_new};
use crate::dispatch::{ClassRegistry, RObj, RubyObject};
use crate::encoding::StrBuf;
use crate::{RubyValue, Signal, Symbol};
use zeo_abi::{ClassId, WEAKMAP_CLASS};
use zeo_macros::ruby_class;

use super::{arg_error, local_jump_error, type_error};

/// A weak handle to a Ruby value's liveness. The heap kinds carry a real
/// `Weak` to their backing `Arc`; everything else is kept alive strongly
/// (immediates never die anyway, and weak-referencing the rarer heap kinds is
/// uncommon enough that "never expires" is the accepted best-effort answer).
pub(super) enum WeakTarget {
    /// `true`/`false`/`nil` -- CRuby's weakref.rb cannot put these in its
    /// `WeakMap` at all and stashes them in a `@delegate_sd_obj` IVAR
    /// instead, which is why they alone survive a `#dup` and why
    /// `weakref_alive?` answers them with `defined?`'s String rather than
    /// `true`.
    Sd(RubyValue),
    Strong(RubyValue),
    Object(Weak<dyn RubyObject>),
    Str(Weak<Freezable<StrBuf>>),
    Array(Weak<Freezable<crate::collections::ArrayStore>>),
    Hash(Weak<Freezable<RHashData>>),
}

impl WeakTarget {
    pub(super) fn downgrade(v: &RubyValue) -> WeakTarget {
        match v {
            RubyValue::Object(o) => WeakTarget::Object(Arc::downgrade(o)),
            RubyValue::Str(s) => WeakTarget::Str(Arc::downgrade(s)),
            RubyValue::Array(a) => WeakTarget::Array(Arc::downgrade(a)),
            RubyValue::Hash(h) => WeakTarget::Hash(Arc::downgrade(h)),
            v @ (RubyValue::Nil | RubyValue::Bool(_)) => WeakTarget::Sd(v.clone()),
            other => WeakTarget::Strong(other.clone()),
        }
    }

    /// The referent if it's still alive, else `None`.
    pub(super) fn upgrade(&self) -> Option<RubyValue> {
        match self {
            WeakTarget::Sd(v) | WeakTarget::Strong(v) => Some(v.clone()),
            WeakTarget::Object(w) => w.upgrade().map(RubyValue::Object),
            WeakTarget::Str(w) => w.upgrade().map(RubyValue::Str),
            WeakTarget::Array(w) => w.upgrade().map(RubyValue::Array),
            WeakTarget::Hash(w) => w.upgrade().map(RubyValue::Hash),
        }
    }
}

/// Whether two values are the SAME object (identity), the keying `WeakMap`
/// and `equal?` use: immediates compare by value, heap kinds by `Arc` pointer.
pub(super) fn same_object(a: &RubyValue, b: &RubyValue) -> bool {
    match (a, b) {
        (RubyValue::Nil, RubyValue::Nil) => true,
        (RubyValue::Bool(x), RubyValue::Bool(y)) => x == y,
        (RubyValue::Int(x), RubyValue::Int(y)) => x == y,
        (RubyValue::Symbol(x), RubyValue::Symbol(y)) => x == y,
        (RubyValue::Object(x), RubyValue::Object(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Str(x), RubyValue::Str(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Array(x), RubyValue::Array(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Hash(x), RubyValue::Hash(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

/// `ObjectSpace::WeakMap` instance -- an identity-keyed list of
/// weakly-held key/value pairs. A `Vec` (not a hash) keeps the identity
/// keying trivially correct; real WeakMaps stay small, so the linear scan is
/// not a concern in practice.
pub struct WeakMap {
    class_id: ClassId,
    frozen: AtomicBool,
    entries: Mutex<Vec<(WeakTarget, WeakTarget)>>,
}

impl WeakMap {
    fn new(class_id: ClassId) -> Arc<WeakMap> {
        Arc::new(WeakMap {
            class_id,
            frozen: AtomicBool::new(false),
            entries: Mutex::new(Vec::new()),
        })
    }

    /// Drop every entry whose KEY has been collected -- the entry-liveness the
    /// internal table (and `length`/`size`) reflects. An entry whose key still
    /// lives but whose VALUE was collected stays counted, matching CRuby's
    /// key-driven WeakMap table; the value-alive filter is applied separately
    /// by the read methods (`keys`/`values`/`each`/`[]`). Called before any
    /// read and eagerly at `GC.start`.
    fn prune(&self) {
        self.entries.lock().retain(|(k, _)| k.upgrade().is_some());
    }

    /// The pairs with BOTH key and value still alive -- the view `keys`,
    /// `values`, `each`, and `key?` expose (a key whose value died reads as
    /// absent, exactly like CRuby: `keys` drops it though `length` counts it).
    fn live_pairs(&self) -> Vec<(RubyValue, RubyValue)> {
        self.entries
            .lock()
            .iter()
            .filter_map(|(k, v)| Some((k.upgrade()?, v.upgrade()?)))
            .collect()
    }

    /// The value for `key`, or `None` if the key is absent OR its value has
    /// been collected (`m[key]` reads nil, `key?` reads false, in that case).
    fn get(&self, key: &RubyValue) -> Option<RubyValue> {
        self.entries.lock().iter().find_map(|(k, v)| {
            let kv = k.upgrade()?;
            same_object(&kv, key).then(|| v.upgrade()).flatten()
        })
    }

    fn set(&self, key: &RubyValue, val: &RubyValue) {
        let mut es = self.entries.lock();
        // Drop key-dead entries and any prior mapping for this key in one pass.
        es.retain(|(k, _)| match k.upgrade() {
            Some(kv) => !same_object(&kv, key),
            None => false,
        });
        es.push((WeakTarget::downgrade(key), WeakTarget::downgrade(val)));
    }

    /// Remove the entry for `key`, returning its (live) value.
    fn delete(&self, key: &RubyValue) -> Option<RubyValue> {
        let mut es = self.entries.lock();
        let idx = es.iter().position(|(k, _)| match k.upgrade() {
            Some(kv) => same_object(&kv, key),
            None => false,
        })?;
        let (_, v) = es.remove(idx);
        v.upgrade()
    }
}

impl RubyObject for WeakMap {
    fn class_id(&self) -> ClassId {
        self.class_id
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = WeakMap::new(self.class_id);
        *d.entries.lock() = self
            .entries
            .lock()
            .iter()
            .filter_map(|(k, v)| {
                let (kv, vv) = (k.upgrade()?, v.upgrade()?);
                Some((WeakTarget::downgrade(&kv), WeakTarget::downgrade(&vv)))
            })
            .collect();
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

/// Downcast a receiver to `&WeakMap`, or a `TypeError` if it isn't one (only
/// reachable via a deliberately mis-dispatched call). Takes the untyped value
/// because that is what a table row receives; `WeakMap` has no `RubyValue`
/// variant of its own, so the class header cannot unwrap it.
fn as_weakmap(recv: &RubyValue) -> Result<&WeakMap, Signal> {
    let RubyValue::Object(o) = recv else {
        return Err(type_error!("not an ObjectSpace::WeakMap"));
    };
    o.as_any()
        .downcast_ref::<WeakMap>()
        .ok_or_else(|| type_error!("not an ObjectSpace::WeakMap"))
}

/// `ObjectSpace::WeakKeyMap` -- weak on the KEY side ONLY, so a value may
/// safely reference its own key without pinning it. Its other difference from
/// `WeakMap` is that lookup compares by `==`, not by identity: a key equal to
/// the one stored finds the entry, and `#getkey` hands back the one stored.
pub struct WeakKeyMap {
    class_id: ClassId,
    frozen: AtomicBool,
    entries: Mutex<Vec<(WeakTarget, RubyValue)>>,
}

impl WeakKeyMap {
    fn new(class_id: ClassId) -> Arc<WeakKeyMap> {
        Arc::new(WeakKeyMap {
            class_id,
            frozen: AtomicBool::new(false),
            entries: Mutex::new(Vec::new()),
        })
    }

    /// The entries whose key still lives, each as `(stored key, value)`.
    fn live(&self) -> Vec<(RubyValue, RubyValue)> {
        self.entries
            .lock()
            .iter()
            .filter_map(|(k, v)| Some((k.upgrade()?, v.clone())))
            .collect()
    }

    fn find(&self, key: &RubyValue) -> Option<(RubyValue, RubyValue)> {
        self.live().into_iter().find(|(k, _)| k.rb_eq(key))
    }

    fn set(&self, key: &RubyValue, val: &RubyValue) {
        let mut es = self.entries.lock();
        es.retain(|(k, _)| match k.upgrade() {
            Some(kv) => !kv.rb_eq(key),
            None => false,
        });
        es.push((WeakTarget::downgrade(key), val.clone()));
    }

    fn delete(&self, key: &RubyValue) -> Option<RubyValue> {
        let mut es = self.entries.lock();
        let idx = es.iter().position(|(k, _)| match k.upgrade() {
            Some(kv) => kv.rb_eq(key),
            None => false,
        })?;
        Some(es.remove(idx).1)
    }
}

impl RubyObject for WeakKeyMap {
    fn class_id(&self) -> ClassId {
        self.class_id
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        self.entries.lock().iter().map(|(_, v)| v.clone()).collect()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = WeakKeyMap::new(self.class_id);
        *d.entries.lock() = self
            .live()
            .iter()
            .map(|(k, v)| (WeakTarget::downgrade(k), v.clone()))
            .collect();
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

fn as_weak_key_map(recv: &RubyValue) -> Result<&WeakKeyMap, Signal> {
    let RubyValue::Object(o) = recv else {
        return Err(type_error!("not an ObjectSpace::WeakKeyMap"));
    };
    o.as_any()
        .downcast_ref::<WeakKeyMap>()
        .ok_or_else(|| type_error!("not an ObjectSpace::WeakKeyMap"))
}

/// `ObjectSpace::WeakKeyMap.new` -- the registered constructor.
fn weak_key_map_construct(
    class: ClassId,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::check_arity(args.len(), 0, Some(0))?;
    Ok(RubyValue::Object(WeakKeyMap::new(class)))
}

// The DSL emits one method table per block, so this third class in the
// file lives in its own module too.
mod weak_key_map_rows {
    use super::*;

    ruby_class! {
        WeakKeyMap = zeo_abi::WEAK_KEY_MAP_CLASS < zeo_abi::OBJECT_CLASS;

        def "[]"(recv, key) {
            Ok(as_weak_key_map(recv)?.find(key).map(|(_, v)| v).unwrap_or(RubyValue::Nil))
        }
        // An immediate cannot be collected, so a map keyed on one would never
        // shed the entry -- CRuby refuses the key rather than leak it. Note
        // there is NO frozen check: CRuby lets a frozen WeakKeyMap be written.
        def "[]="(recv, key, value) {
            if !collectable(key) {
                return Err(arg_error!("WeakKeyMap keys must be garbage collectable"));
            }
            as_weak_key_map(recv)?.set(key, value);
            Ok(value.clone())
        }
        def "key?"(recv, key) {
            Ok(RubyValue::Bool(as_weak_key_map(recv)?.find(key).is_some()))
        }
        // The key AS STORED, which is the point: an equal key finds the entry,
        // and this hands back the object the map is actually holding.
        def "getkey"(recv, key) {
            Ok(as_weak_key_map(recv)?.find(key).map(|(k, _)| k).unwrap_or(RubyValue::Nil))
        }
        // `delete(key)` answers the removed value; a miss answers the block's
        // value if one was given, else nil.
        def "delete"(recv, key, &block) {
            if let Some(v) = as_weak_key_map(recv)?.delete(key) {
                return Ok(v);
            }
            match block {
                Some(RubyValue::Proc(p)) => p.call(std::slice::from_ref(key)),
                _ => Ok(RubyValue::Nil),
            }
        }
        def "clear"(recv) {
            as_weak_key_map(recv)?.entries.lock().clear();
            Ok(recv.clone())
        }
        // CRuby prints the live entry COUNT here, unlike `WeakMap`'s inspect.
        def "inspect"(recv) {
            let map = as_weak_key_map(recv)?;
            let RubyValue::Object(o) = recv else {
                return Err(type_error!("not an ObjectSpace::WeakKeyMap"));
            };
            let addr = Arc::as_ptr(o) as *const () as usize;
            Ok(RubyValue::Str(string_new(format!(
                "#<ObjectSpace::WeakKeyMap:0x{addr:016x} size={}>",
                map.live().len()
            ))))
        }
    }
}

/// Whether a value can be collected at all. An immediate (nil, true/false,
/// an Integer of either width, a Float, a Symbol) cannot, so it can never be
/// a `WeakKeyMap` key.
fn collectable(v: &RubyValue) -> bool {
    !matches!(
        v,
        RubyValue::Nil
            | RubyValue::Bool(_)
            | RubyValue::Int(_)
            | RubyValue::BigInt(_)
            | RubyValue::Float(_)
            | RubyValue::Symbol(_)
    )
}

/// Register a user `class WeakSet < ObjectSpace::WeakMap`: name + linearized
/// ancestors + the root's own constructor, unchanged. This is the cleanest of
/// the native shapes -- [`weakmap_construct`] already builds
/// `WeakMap::new(class)` from the receiver, so a subclass instance is the
/// native type tagged with the subclass id and needs no payload wrapper and no
/// re-tagging. Its own `def`s arrive as `RubyValue`-self deltas, like the
/// other struct-less shapes.
pub fn register_weakmap_subclass(
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
        Some(weakmap_subclass_construct as crate::dispatch::ConstructorFn),
    );
}

/// The `ConstructorFn` for a user WeakMap subclass: the native build (which
/// takes no arguments), then the standard `run_initialize` tail -- so a
/// subclass's own `initialize` RUNS, with `super()` bottoming out on
/// `BasicObject#initialize`. Registering the root's own constructor directly
/// skipped the user body, and the answer was silently wrong (an ivar the
/// `initialize` was supposed to write simply read back nil).
fn weakmap_subclass_construct(
    class: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let obj: crate::dispatch::RObj = WeakMap::new(class);
    crate::dispatch::run_initialize(class, &obj, args, block)?;
    Ok(RubyValue::Object(obj))
}

/// `ObjectSpace::WeakMap.new` -- the registered constructor.
fn weakmap_construct(
    class: ClassId,
    _args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Object(WeakMap::new(class)))
}

ruby_class! {
    WeakMap = zeo_abi::WEAKMAP_CLASS < zeo_abi::OBJECT_CLASS;

    def "[]"(recv, key) {
        Ok(as_weakmap(recv)?.get(key).unwrap_or(RubyValue::Nil))
    }
    def "[]="(recv, key, value) {
        as_weakmap(recv)?.set(key, value);
        Ok(value.clone())
    }
    def "delete"(recv, key) {
        Ok(as_weakmap(recv)?.delete(key).unwrap_or(RubyValue::Nil))
    }
    def "key?" | "include?" | "member?"(recv, key) {
        Ok(RubyValue::Bool(as_weakmap(recv)?.get(key).is_some()))
    }
    def "keys"(recv) {
        let keys = as_weakmap(recv)?
            .live_pairs()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        Ok(RubyValue::Array(array_new(keys)))
    }
    def "values"(recv) {
        let vals = as_weakmap(recv)?
            .live_pairs()
            .into_iter()
            .map(|(_, v)| v)
            .collect();
        Ok(RubyValue::Array(array_new(vals)))
    }
    def "length" | "size"(recv) {
        let wm = as_weakmap(recv)?;
        wm.prune();
        Ok(RubyValue::Int(wm.entries.lock().len() as i64))
    }
    // `each`/`each_pair` -- yields `[key, value]`; `each_key`/`each_value`
    // yield one side. Returns the map. A missing block would need an
    // Enumerator; that shape is uncommon on WeakMap and not modeled (a
    // `LocalJumpError` surfaces from the yield attempt otherwise).
    def "each" | "each_pair"(recv, &block) {
        for (k, v) in as_weakmap(recv)?.live_pairs() {
            // A pair yield (`{ |k, v| }` binds both; `{ |x| }` binds `[k, v]`),
            // like `Hash#each` -- yield_tuple's array-wrap is exactly that shape.
            yield_pair(&block, k, v)?;
        }
        Ok(recv.clone())
    }
    def "each_key"(recv, &block) {
        for (k, _) in as_weakmap(recv)?.live_pairs() {
            yield_one(&block, k)?;
        }
        Ok(recv.clone())
    }
    def "each_value"(recv, &block) {
        for (_, v) in as_weakmap(recv)?.live_pairs() {
            yield_one(&block, v)?;
        }
        Ok(recv.clone())
    }
    def "inspect"(recv) {
        // The empty form is byte-exact with CRuby
        // (`#<ObjectSpace::WeakMap:0xADDR>`). CRuby's NON-empty form appends
        // address-based entry info (`: #<Object:0x..> => #<String:0x..>`)
        // which is non-deterministic and deliberately never calls the
        // entries' own `inspect`; reproducing that is pointless (no test
        // could pin an address), so the address-only form stands in for
        // non-empty maps too -- a documented, cosmetic simplification.
        let RubyValue::Object(o) = recv else {
            return Err(type_error!("not an ObjectSpace::WeakMap"));
        };
        let addr = Arc::as_ptr(o) as *const () as usize;
        Ok(RubyValue::Str(string_new(format!(
            "#<ObjectSpace::WeakMap:0x{addr:016x}>"
        ))))
    }
}

/// Yield a `[key, value]` PAIR to the block -- `{ |k, v| }` binds both,
/// `{ |x| }` binds the `[k, v]` array (`Hash#each`'s shape). `None` block ->
/// LocalJumpError.
fn yield_pair(blk: &Option<RubyValue>, k: RubyValue, v: RubyValue) -> Result<RubyValue, Signal> {
    match blk {
        Some(RubyValue::Proc(p)) => crate::rproc::yield_pair(p, k, v),
        _ => Err(local_jump_error!("no block given (yield)")),
    }
}

/// Yield a SINGLE value to the block (`{ |v| }` binds it directly).
fn yield_one(blk: &Option<RubyValue>, v: RubyValue) -> Result<RubyValue, Signal> {
    match blk {
        Some(RubyValue::Proc(p)) => p.call(&[v]),
        _ => Err(local_jump_error!("no block given (yield)")),
    }
}

/// Install `ObjectSpace::WeakMap` and `WeakRef` with their native
/// constructors. Called from `ClassRegistry::with_core` AFTER
/// `register_builtins` has seeded their ancestor rows (this overrides those
/// rows to attach the constructor).
///
/// The ordinary instance methods are `ruby_class!` table rows above; only what
/// the tables CANNOT serve stays here -- the constructors (no table slot) and
/// `WeakRef`'s `method_missing`/`respond_to_missing?` (the send-miss fallback
/// consults only `registry().lookup`, never the builtin class table, so
/// delegation to the referent would never fire from a table row).
pub fn register_weak(registry: &mut ClassRegistry) {
    let wm_ancestors = zeo_abi::declared_ancestors(WEAKMAP_CLASS);
    registry.register(
        WEAKMAP_CLASS,
        "ObjectSpace::WeakMap",
        false,
        wm_ancestors,
        Some(weakmap_construct as crate::dispatch::ConstructorFn),
    );

    // `WeakRef` is a NAMESPACE slot whose class body is the vendored Ruby file
    // (`gems/weakref`), so it has no native rows and no native payload. Its
    // instances must be whatever its declared superclass allocates --
    // `Delegator`'s compiled struct, whose inherited bodies downcast to it --
    // which is exactly what `compiled_subclass_construct` resolves, falling
    // back to a plain name-keyed object when no ancestor has a struct.
    let wr_ancestors = zeo_abi::declared_ancestors(zeo_abi::WEAKREF_CLASS);
    registry.register(
        zeo_abi::WEAKREF_CLASS,
        "WeakRef",
        false,
        wr_ancestors,
        Some(crate::runtime_meta::compiled_subclass_construct as crate::dispatch::ConstructorFn),
    );

    let wkm_ancestors = zeo_abi::declared_ancestors(zeo_abi::WEAK_KEY_MAP_CLASS);
    registry.register(
        zeo_abi::WEAK_KEY_MAP_CLASS,
        "ObjectSpace::WeakKeyMap",
        false,
        wkm_ancestors,
        Some(weak_key_map_construct as crate::dispatch::ConstructorFn),
    );
}

/// One registered `ObjectSpace.define_finalizer` callback. `object_id` is
/// captured at registration (the referent may be gone by the time the
/// callback runs, so the id -- CRuby's finalizer argument -- must be kept
/// independently). `target` detects collection for the `GC.start` sweep.
pub(super) struct Finalizer {
    pub(super) target: WeakTarget,
    pub(super) object_id: i64,
    pub(super) callback: RubyValue,
}

pub(super) static FINALIZERS: Mutex<Vec<Finalizer>> = Mutex::new(Vec::new());

/// CRuby's object id for a value -- the Integer a finalizer callback receives.
/// Mirrors `Kernel#object_id` (the immediate shapes and heap-pointer ids).
pub(super) fn object_id_i64(v: &RubyValue) -> i64 {
    match v {
        RubyValue::Int(i) => i.wrapping_mul(2).wrapping_add(1),
        RubyValue::Nil => 4,
        RubyValue::Bool(true) => 20,
        RubyValue::Bool(false) => 0,
        RubyValue::Object(o) => Arc::as_ptr(o) as *const () as i64,
        RubyValue::Str(s) => Arc::as_ptr(s) as i64,
        RubyValue::Array(a) => Arc::as_ptr(a) as i64,
        RubyValue::Hash(h) => Arc::as_ptr(h) as i64,
        RubyValue::Symbol(s) => 0x1000_0000_0000 + i64::from(s.to_u32()),
        _ => v as *const _ as i64,
    }
}

/// Run `f`'s callback with the captured object id, swallowing any error --
/// CRuby warns on a raising finalizer but never aborts the sweep.
fn run_one(f: &Finalizer) {
    let _ = crate::dispatch::send_value(
        &f.callback,
        Symbol::intern("call"),
        &[RubyValue::Int(f.object_id)],
        None,
    );
}

/// Run and drop every finalizer whose referent has been collected -- the
/// `GC.start` sweep.
pub fn run_finalizers_for_dead() {
    let dead: Vec<Finalizer> = {
        let mut fs = FINALIZERS.lock();
        let mut i = 0;
        let mut out = Vec::new();
        while i < fs.len() {
            if fs[i].target.upgrade().is_none() {
                out.push(fs.remove(i));
            } else {
                i += 1;
            }
        }
        out
    };
    // Run OUTSIDE the lock: a callback may register/undefine finalizers.
    for f in &dead {
        run_one(f);
    }
}

/// Run every remaining finalizer, live or dead, and clear the registry -- the
/// program-exit sweep (CRuby finalizes all objects at exit). Called from
/// generated `main` after `at_exit`.
pub fn run_finalizers() {
    let all: Vec<Finalizer> = std::mem::take(&mut *FINALIZERS.lock());
    for f in &all {
        run_one(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_string(s: &str) -> RubyValue {
        RubyValue::Str(string_new(s.to_string()))
    }

    #[test]
    fn weak_target_upgrades_while_the_arc_lives_and_dies_after() {
        let s = a_string("hi");
        let wt = WeakTarget::downgrade(&s);
        assert!(wt.upgrade().is_some());
        drop(s);
        assert!(wt.upgrade().is_none(), "the last strong ref dropped");
    }

    #[test]
    fn immediates_are_held_strongly_and_never_expire() {
        let wt = WeakTarget::downgrade(&RubyValue::Int(7));
        assert!(matches!(wt.upgrade(), Some(RubyValue::Int(7))));
    }

    #[test]
    fn weakmap_set_get_delete_by_identity() {
        let m = WeakMap::new(WEAKMAP_CLASS);
        let k = a_string("k");
        let v = a_string("v");
        m.set(&k, &v);
        assert!(m.get(&k).is_some());
        // A DIFFERENT string with equal contents is a different object.
        assert!(m.get(&a_string("k")).is_none());
        assert!(same_object(&m.delete(&k).unwrap(), &v));
        assert!(m.get(&k).is_none());
    }

    #[test]
    fn weakmap_prunes_a_collected_key() {
        let m = WeakMap::new(WEAKMAP_CLASS);
        let v = a_string("v"); // kept alive so only the KEY's death prunes
        {
            let k = a_string("temp");
            m.set(&k, &v);
            assert_eq!(m.live_pairs().len(), 1);
        } // k dropped here
        m.prune();
        assert_eq!(m.live_pairs().len(), 0);
    }
}
