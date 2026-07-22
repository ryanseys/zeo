//! `ObjectSpace` (CRuby gc.c / weakmap.c) and its `WeakMap` primitive.
//!
//! zeo's memory model is `Arc` refcounting, not a tracing collector (the
//! plan's accepted trade -- cycles leak). That shapes what this module can
//! honestly offer:
//!
//! - `ObjectSpace::WeakMap` holds `Arc::downgrade`d handles to its keys and
//!   values, so an entry vanishes once the last STRONG reference elsewhere
//!   drops. Dead entries are pruned lazily on access and eagerly at
//!   `GC.start`. Only the common heap kinds (`Object`/`String`/`Array`/`Hash`)
//!   are tracked weakly; every other value -- immediates, and the rarer
//!   Arc-backed kinds (`Proc`/`Regexp`/...) -- is held STRONGLY, a bounded,
//!   documented best-effort divergence (weak-referencing those is rare, and a
//!   strongly-held entry simply never expires rather than expiring wrongly).
//! - `each_object`/`_id2ref` can't be served without heap enumeration / an
//!   id-to-object table, so they raise `NotImplementedError` (decision #4).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::collections::{Freezable, RHashData, array_new, string_new};
use crate::dispatch::{ClassRegistry, RObj, RubyObject};
use crate::encoding::StrBuf;
use crate::{RubyValue, Signal, Symbol};
use zeo_abi::{ClassId, WEAKMAP_CLASS};

use super::{arg_error, arity, builtin_methods, local_jump_error, not_impl_error, type_error};

/// A weak handle to a Ruby value's liveness. The heap kinds carry a real
/// `Weak` to their backing `Arc`; everything else is kept alive strongly
/// (immediates never die anyway, and weak-referencing the rarer heap kinds is
/// uncommon enough that "never expires" is the accepted best-effort answer).
enum WeakTarget {
    Strong(RubyValue),
    Object(Weak<dyn RubyObject>),
    Str(Weak<Freezable<StrBuf>>),
    Array(Weak<Freezable<Vec<RubyValue>>>),
    Hash(Weak<Freezable<RHashData>>),
}

impl WeakTarget {
    fn downgrade(v: &RubyValue) -> WeakTarget {
        match v {
            RubyValue::Object(o) => WeakTarget::Object(Arc::downgrade(o)),
            RubyValue::Str(s) => WeakTarget::Str(Arc::downgrade(s)),
            RubyValue::Array(a) => WeakTarget::Array(Arc::downgrade(a)),
            RubyValue::Hash(h) => WeakTarget::Hash(Arc::downgrade(h)),
            other => WeakTarget::Strong(other.clone()),
        }
    }

    /// The referent if it's still alive, else `None`.
    fn upgrade(&self) -> Option<RubyValue> {
        match self {
            WeakTarget::Strong(v) => Some(v.clone()),
            WeakTarget::Object(w) => w.upgrade().map(RubyValue::Object),
            WeakTarget::Str(w) => w.upgrade().map(RubyValue::Str),
            WeakTarget::Array(w) => w.upgrade().map(RubyValue::Array),
            WeakTarget::Hash(w) => w.upgrade().map(RubyValue::Hash),
        }
    }
}

/// Whether two values are the SAME object (identity), the keying `WeakMap`
/// and `equal?` use: immediates compare by value, heap kinds by `Arc` pointer.
fn same_object(a: &RubyValue, b: &RubyValue) -> bool {
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

    /// Drop every entry whose key OR value has been collected. Called before
    /// any read so a dead entry is never observed, and eagerly at `GC.start`.
    fn prune(&self) {
        self.entries
            .lock()
            .retain(|(k, v)| k.upgrade().is_some() && v.upgrade().is_some());
    }

    /// The live `(key, value)` pairs, both upgraded.
    fn live_pairs(&self) -> Vec<(RubyValue, RubyValue)> {
        self.entries
            .lock()
            .iter()
            .filter_map(|(k, v)| Some((k.upgrade()?, v.upgrade()?)))
            .collect()
    }

    fn get(&self, key: &RubyValue) -> Option<RubyValue> {
        self.entries.lock().iter().find_map(|(k, v)| {
            let kv = k.upgrade()?;
            same_object(&kv, key).then(|| v.upgrade()).flatten()
        })
    }

    fn set(&self, key: &RubyValue, val: &RubyValue) {
        let mut es = self.entries.lock();
        // Prune dead entries and any prior mapping for this key in one pass.
        es.retain(|(k, v)| {
            let Some(kv) = k.upgrade() else { return false };
            v.upgrade().is_some() && !same_object(&kv, key)
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

/// Downcast a receiver handle to `&WeakMap`, or a `TypeError` if it isn't one
/// (only reachable via a deliberately mis-dispatched call).
fn as_weakmap(recv: &RObj) -> Result<&WeakMap, Signal> {
    recv.as_any()
        .downcast_ref::<WeakMap>()
        .ok_or_else(|| type_error!("not an ObjectSpace::WeakMap"))
}

/// `ObjectSpace::WeakMap.new` -- the registered constructor.
fn weakmap_construct(
    class: ClassId,
    _args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Object(WeakMap::new(class)))
}

fn wm_aref(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    Ok(as_weakmap(recv)?.get(&args[0]).unwrap_or(RubyValue::Nil))
}

fn wm_aset(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    arity!(args, 2);
    as_weakmap(recv)?.set(&args[0], &args[1]);
    Ok(args[1].clone())
}

fn wm_delete(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    Ok(as_weakmap(recv)?.delete(&args[0]).unwrap_or(RubyValue::Nil))
}

fn wm_key_p(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    Ok(RubyValue::Bool(as_weakmap(recv)?.get(&args[0]).is_some()))
}

fn wm_keys(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let keys = as_weakmap(recv)?
        .live_pairs()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    Ok(RubyValue::Array(array_new(keys)))
}

fn wm_values(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let vals = as_weakmap(recv)?
        .live_pairs()
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    Ok(RubyValue::Array(array_new(vals)))
}

fn wm_length(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let wm = as_weakmap(recv)?;
    wm.prune();
    Ok(RubyValue::Int(wm.entries.lock().len() as i64))
}

/// `each`/`each_pair` -- yields `[key, value]`; `each_key`/`each_value` yield
/// one side. Returns the map. A missing block would need an Enumerator; that
/// shape is uncommon on WeakMap and not modeled (a `LocalJumpError` surfaces
/// from the yield attempt otherwise).
fn wm_each_pair(
    recv: &RObj,
    _args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    for (k, v) in as_weakmap(recv)?.live_pairs() {
        yield_block(&blk, vec![k, v])?;
    }
    Ok(RubyValue::Object(recv.clone()))
}

fn wm_each_key(
    recv: &RObj,
    _args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    for (k, _) in as_weakmap(recv)?.live_pairs() {
        yield_block(&blk, vec![k])?;
    }
    Ok(RubyValue::Object(recv.clone()))
}

fn wm_each_value(
    recv: &RObj,
    _args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    for (_, v) in as_weakmap(recv)?.live_pairs() {
        yield_block(&blk, vec![v])?;
    }
    Ok(RubyValue::Object(recv.clone()))
}

fn wm_inspect(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // The empty form is byte-exact with CRuby (`#<ObjectSpace::WeakMap:0xADDR>`).
    // CRuby's NON-empty form appends address-based entry info
    // (`: #<Object:0x..> => #<String:0x..>`) which is non-deterministic and
    // deliberately never calls the entries' own `inspect`; reproducing that is
    // pointless (no test could pin an address), so the address-only form stands
    // in for non-empty maps too -- a documented, cosmetic simplification.
    let addr = Arc::as_ptr(recv) as *const () as usize;
    Ok(RubyValue::Str(string_new(format!(
        "#<ObjectSpace::WeakMap:0x{addr:016x}>"
    ))))
}

/// Yield `elems` to the block (a `Proc`), auto-splatting a lone Array across
/// multi-param blocks like every other iterator. `None` block -> LocalJumpError.
fn yield_block(blk: &Option<RubyValue>, elems: Vec<RubyValue>) -> Result<RubyValue, Signal> {
    match blk {
        Some(RubyValue::Proc(p)) => crate::rproc::yield_tuple(p, elems),
        _ => Err(local_jump_error!("no block given (yield)")),
    }
}

/// Install `ObjectSpace::WeakMap` and `WeakRef` with their native
/// constructors and instance methods. Called from `ClassRegistry::with_core`
/// AFTER `register_builtins` has seeded their ancestor rows (this overrides
/// those rows to attach the constructor + methods).
pub fn register_weak(registry: &mut ClassRegistry) {
    let wm_ancestors = zeo_abi::declared_ancestors(WEAKMAP_CLASS);
    registry.register(
        WEAKMAP_CLASS,
        "ObjectSpace::WeakMap",
        false,
        wm_ancestors,
        Some(weakmap_construct as crate::dispatch::ConstructorFn),
    );
    let m = |registry: &mut ClassRegistry, name: &str, f: crate::dispatch::MethodFn| {
        registry.define_method_own(WEAKMAP_CLASS, Symbol::intern(name), f);
    };
    m(registry, "[]", wm_aref);
    m(registry, "[]=", wm_aset);
    m(registry, "delete", wm_delete);
    m(registry, "key?", wm_key_p);
    m(registry, "include?", wm_key_p);
    m(registry, "member?", wm_key_p);
    m(registry, "keys", wm_keys);
    m(registry, "values", wm_values);
    m(registry, "length", wm_length);
    m(registry, "size", wm_length);
    m(registry, "each", wm_each_pair);
    m(registry, "each_pair", wm_each_pair);
    m(registry, "each_key", wm_each_key);
    m(registry, "each_value", wm_each_value);
    m(registry, "inspect", wm_inspect);
}

/// One registered `ObjectSpace.define_finalizer` callback. `object_id` is
/// captured at registration (the referent may be gone by the time the
/// callback runs, so the id -- CRuby's finalizer argument -- must be kept
/// independently). `target` detects collection for the `GC.start` sweep.
struct Finalizer {
    target: WeakTarget,
    object_id: i64,
    callback: RubyValue,
}

static FINALIZERS: Mutex<Vec<Finalizer>> = Mutex::new(Vec::new());

/// CRuby's object id for a value -- the Integer a finalizer callback receives.
/// Mirrors `Kernel#object_id` (the immediate shapes and heap-pointer ids).
fn object_id_i64(v: &RubyValue) -> i64 {
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

builtin_methods! {
    pub(crate) fn lookup_class;

    // `define_finalizer(obj, callable)` or `define_finalizer(obj) { |id| }` --
    // best-effort: the callback runs when `obj` is seen collected (`GC.start`)
    // and unconditionally at program exit, receiving obj's id.
    "define_finalizer" => fn os_define_finalizer(_recv, args, block) {
        arity!(args, 1..=2);
        let obj = &args[0];
        let callback = match (args.get(1), block) {
            (Some(cb), _) => {
                if !crate::dispatch::responds_to(cb.class_id(), Symbol::intern("call"), false) {
                    return Err(arg_error!(
                        "wrong type argument {} (should be callable)",
                        crate::builtins::class_name_of(cb)
                    ));
                }
                cb.clone()
            }
            (None, Some(b)) => b,
            (None, None) => {
                return Err(arg_error!("tried to create Proc object without a block"));
            }
        };
        FINALIZERS.lock().push(Finalizer {
            target: WeakTarget::downgrade(obj),
            object_id: object_id_i64(obj),
            callback,
        });
        // CRuby returns `[0, callable]` (arity + the finalizer); the arity slot
        // is an internal detail callers don't read.
        Ok(RubyValue::Array(array_new(vec![RubyValue::Int(0), obj.clone()])))
    }
    // Remove every finalizer registered for `obj` (by identity). Returns obj.
    "undefine_finalizer" => fn os_undefine_finalizer(_recv, args, _block) {
        arity!(args, 1);
        let obj = &args[0];
        FINALIZERS.lock().retain(|f| match f.target.upgrade() {
            Some(t) => !same_object(&t, obj),
            None => true,
        });
        Ok(obj.clone())
    }
    // A `WeakMap` of every live object of a class -- zeo has no heap
    // enumeration, so this is an honest NotImplementedError (decision #4).
    "each_object" => fn os_each_object(_recv, _args, _block) {
        Err(not_impl_error!("ObjectSpace.each_object is not available (zeo has no heap enumeration)"))
    }
    // No id->object table exists under Arc refcounting.
    "_id2ref" => fn os_id2ref(_recv, _args, _block) {
        Err(not_impl_error!("ObjectSpace._id2ref is not available (zeo has no id-to-object table)"))
    }
    // `garbage_collect` is `GC.start` by another name -- a no-op sweep (plus
    // the finalizer/weakmap sweep once those land).
    "garbage_collect" => fn os_garbage_collect(_recv, _args, _block) {
        Ok(RubyValue::Nil)
    }
    // An empty per-class census: no fabricated counts, matching `GC.stat`'s
    // empty-Hash posture.
    "count_objects" => fn os_count_objects(_recv, _args, _block) {
        Ok(RubyValue::Hash(crate::collections::hash_new(Vec::new())))
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
