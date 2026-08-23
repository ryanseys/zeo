//! Instance variables on a bare heap value -- `[].instance_variable_set(:@x, 1)`.
//!
//! Ruby lets you hang an ivar on almost anything. Only the permanently-frozen
//! tier refuses (an Integer, a Symbol, `nil`, a Range); an Array, a String, a
//! Hash, a Proc, a Regexp, a MatchData all accept one and report it through
//! `instance_variables`. zeo's other two ivar stores cannot serve them: an
//! `Object`'s ivars are FIELDS of its generated struct, and a class object's
//! live in the id-keyed `civars`. A bare `RubyValue::Array` has neither a
//! generated struct nor a class id of its own, so it needs a third store,
//! keyed the only way such a value can be named -- by identity.
//!
//! This is `runtime_meta`'s `value_singletons` shape, and it inherits that
//! table's soundness rule: an `Arc` address identifies a value only while the
//! value lives, so every write PINS its owner. Without the pin a dead value's
//! address gets handed to an unrelated later one, which would silently
//! inherit its ivars.
//!
//! The pin is WEAK, and that matters twice. A `Weak` holds the allocation
//! open, so the address stays unique -- which is the whole job. A strong one
//! would also keep the VALUE alive forever, which made this table a permanent
//! leak in its own right and made every owner permanently reachable from
//! outside the cycle collector's registry. [`sweep`] drops the rows whose
//! owner is gone, amortized against the table's own growth.
//!
//! `ANY` keeps the cost off every other program: until something actually
//! writes one of these, every read is a single relaxed load.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use crate::{FMap, RubyValue};

/// One value's ivars, newest last. A `Vec` rather than a map because Ruby
/// reports `instance_variables` in ASSIGNMENT order and no real value carries
/// enough of them for the linear scan to matter.
type Slots = Vec<(String, RubyValue)>;

use crate::value::WeakOwner;

struct Store {
    ivars: RwLock<FMap<usize, Slots>>,
    /// A weak reference to every value that has ever been given an ivar, so
    /// its address can never be reused. See the module docs.
    owners: RwLock<FMap<usize, WeakOwner>>,
}

static ANY: AtomicBool = AtomicBool::new(false);
static STORE: OnceLock<Store> = OnceLock::new();

fn store() -> &'static Store {
    STORE.get_or_init(|| Store {
        ivars: RwLock::new(FMap::default()),
        owners: RwLock::new(FMap::default()),
    })
}

/// Whether anything in the process has ever set one of these. Every read path
/// checks this first, so a program that never touches an ivar on a bare value
/// pays one relaxed load per `instance_variables`-family call.
#[inline]
fn any() -> bool {
    ANY.load(Ordering::Relaxed)
}

/// The identity key for a value that can carry these ivars, or `None` for one
/// that cannot.
///
/// `Class` is absent deliberately -- it has its own store (`civars`) and every
/// caller here checks it first. So is the permanently-frozen tier (immediates,
/// and `Range`, which zeo reports frozen exactly as Ruby does): those raise
/// `FrozenError` before reaching this, which is the right answer rather than a
/// missing feature.
///
/// `Object` answers only for the DECLINE path. Every caller consults the
/// `RObj`'s own store first, and a generated class always takes the write
/// (typed field, else `__overflow`). The runtime's hand-written objects --
/// `StringScanner`, FFI's `VariadicInvoker`, `TCPSocket` -- declare no ivars
/// and answer `false`, and reopening one in Ruby to keep an `@x` is ordinary
/// enough that dropping the write is not an option. They land here instead.
fn key(v: &RubyValue) -> Option<usize> {
    let addr = match v {
        // A fat `*const dyn RubyObject`; the data half is the identity, which
        // is the same cast `container_identity` takes.
        RubyValue::Object(o) => Arc::as_ptr(o).cast::<()>(),
        RubyValue::Str(s) => Arc::as_ptr(s).cast::<()>(),
        RubyValue::Array(a) => Arc::as_ptr(a).cast::<()>(),
        RubyValue::Hash(h) => Arc::as_ptr(h).cast::<()>(),
        RubyValue::Regexp(r) => Arc::as_ptr(r).cast::<()>(),
        RubyValue::MatchData(m) => Arc::as_ptr(m).cast::<()>(),
        RubyValue::Enumerator(e) => Arc::as_ptr(e).cast::<()>(),
        RubyValue::Fiber(f) => Arc::as_ptr(f).cast::<()>(),
        RubyValue::Thread(t) => Arc::as_ptr(t).cast::<()>(),
        RubyValue::Mutex(m) => Arc::as_ptr(m).cast::<()>(),
        RubyValue::Queue(q) => Arc::as_ptr(q).cast::<()>(),
        RubyValue::Ractor(r) => Arc::as_ptr(r).cast::<()>(),
        // A Yielder is a thin handle over the driving block, so it shares that
        // block's identity -- the same rule `is_frozen` applies to it.
        RubyValue::Proc(p) | RubyValue::Yielder(p) => return Some(p.identity()),
        _ => return None,
    };
    Some(addr as usize)
}

/// Drop every row whose owner is gone.
///
/// The ivars go FIRST and the pins LAST. Dropping a pin frees the allocation,
/// and freeing the allocation is what lets that address be handed to an
/// unrelated later value -- which must never find a dead one's ivars still
/// filed under it. Both locks are held across the whole sweep, so no reader
/// can interleave either way; the order is written down because the same
/// mistake in `runtime_meta`'s twin made a live String answer a dead one's
/// singleton method.
///
/// Amortized against the table's own growth, the same bargain a `Vec` makes:
/// the next sweep waits until the table has grown by half again, so the scan
/// costs O(1) per row written.
fn sweep(owners: &mut FMap<usize, WeakOwner>, ivars: &mut FMap<usize, Slots>) {
    ivars.retain(|k, _| owners.get(k).is_some_and(|w| w.strong_count() > 0));
    owners.retain(|_, w| w.strong_count() > 0);
    SWEEP_AT.store(owners.len() + (owners.len() / 2).max(64), Ordering::Relaxed);
}

/// How many rows `owners` may hold before the next [`sweep`].
static SWEEP_AT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(64);

/// Record `v` as the owner of key `k`, sweeping the table when it has grown
/// enough to be worth the scan. Both locks are taken here, in one order, and
/// this is the only place that holds them at once.
fn pin(k: usize, v: &RubyValue) {
    let Some(weak) = crate::value::weak_owner(v) else {
        return;
    };
    let mut owners = store().owners.write().unwrap();
    owners.insert(k, weak);
    if owners.len() >= SWEEP_AT.load(Ordering::Relaxed) {
        let mut ivars = store().ivars.write().unwrap();
        sweep(&mut owners, &mut ivars);
    }
}

/// `v`'s `@name` (bare, no `@`), or `None` if it has none.
pub(crate) fn get(v: &RubyValue, name: &str) -> Option<RubyValue> {
    if !any() {
        return None;
    }
    let k = key(v)?;
    let ivars = store().ivars.read().unwrap();
    let slots = ivars.get(&k)?;
    slots
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, val)| val.clone())
}

/// Set `v`'s `@name` to `value`, answering whether this store took it. The
/// caller has already applied the frozen check; a `false` answer means `v` is
/// not a kind that can carry an ivar at all.
pub(crate) fn set(v: &RubyValue, name: &str, value: RubyValue) -> bool {
    let Some(k) = key(v) else {
        return false;
    };
    pin(k, v);
    let mut ivars = store().ivars.write().unwrap();
    let slots = ivars.entry(k).or_default();
    match slots.iter_mut().find(|(n, _)| n == name) {
        Some((_, slot)) => *slot = value,
        None => slots.push((name.to_string(), value)),
    }
    // After the write, so a concurrent reader that sees the latch also sees
    // the entry it is about to look for.
    ANY.store(true, Ordering::Relaxed);
    true
}

/// Remove `v`'s `@name`, answering its former value.
pub(crate) fn remove(v: &RubyValue, name: &str) -> Option<RubyValue> {
    if !any() {
        return None;
    }
    let k = key(v)?;
    let mut ivars = store().ivars.write().unwrap();
    let slots = ivars.get_mut(&k)?;
    let at = slots.iter().position(|(n, _)| n == name)?;
    Some(slots.remove(at).1)
}

/// `v`'s ivar names (bare, no `@`) in assignment order.
pub(crate) fn names(v: &RubyValue) -> Vec<String> {
    if !any() {
        return Vec::new();
    }
    let Some(k) = key(v) else {
        return Vec::new();
    };
    store()
        .ivars
        .read()
        .unwrap()
        .get(&k)
        .map(|slots| slots.iter().map(|(n, _)| n.clone()).collect())
        .unwrap_or_default()
}

/// Copy every ivar from `from` onto `to` -- what `dup` and `clone` owe a bare
/// value. Ruby carries ivars across both (only SINGLETONS are `clone`-only).
pub(crate) fn copy(from: &RubyValue, to: &RubyValue) {
    if !any() {
        return;
    }
    let (Some(src), Some(dst)) = (key(from), key(to)) else {
        return;
    };
    if src == dst {
        return;
    }
    let slots = store().ivars.read().unwrap().get(&src).cloned();
    let Some(slots) = slots.filter(|s| !s.is_empty()) else {
        return;
    };
    pin(dst, to);
    store().ivars.write().unwrap().insert(dst, slots);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{array_new, string_new};

    #[test]
    fn set_then_get_and_list_in_assignment_order() {
        let a = RubyValue::Array(array_new(Vec::new()));
        assert!(set(&a, "b", RubyValue::Int(2)));
        assert!(set(&a, "a", RubyValue::Int(1)));
        assert!(matches!(get(&a, "a"), Some(RubyValue::Int(1))));
        assert_eq!(names(&a), vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn a_second_write_replaces_in_place_and_keeps_its_position() {
        let s = RubyValue::Str(string_new("x".to_string()));
        set(&s, "x", RubyValue::Int(1));
        set(&s, "y", RubyValue::Int(2));
        set(&s, "x", RubyValue::Int(9));
        assert!(matches!(get(&s, "x"), Some(RubyValue::Int(9))));
        assert_eq!(names(&s), vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn two_values_of_the_same_kind_do_not_share() {
        let one = RubyValue::Array(array_new(Vec::new()));
        let two = RubyValue::Array(array_new(Vec::new()));
        set(&one, "k", RubyValue::Int(1));
        assert!(get(&two, "k").is_none());
    }

    #[test]
    fn remove_answers_the_former_value_then_misses() {
        let h = RubyValue::Hash(crate::hash_new(Vec::new()));
        set(&h, "k", RubyValue::Int(7));
        assert!(matches!(remove(&h, "k"), Some(RubyValue::Int(7))));
        assert!(remove(&h, "k").is_none());
        assert!(names(&h).is_empty());
    }

    #[test]
    fn copy_carries_the_ivars_to_a_fresh_value() {
        let from = RubyValue::Array(array_new(Vec::new()));
        let to = RubyValue::Array(array_new(Vec::new()));
        set(&from, "k", RubyValue::Int(4));
        copy(&from, &to);
        assert!(matches!(get(&to, "k"), Some(RubyValue::Int(4))));
        // The copy is independent: writing one must not touch the other.
        set(&to, "k", RubyValue::Int(5));
        assert!(matches!(get(&from, "k"), Some(RubyValue::Int(4))));
    }

    #[test]
    fn an_immediate_can_hold_nothing() {
        assert!(!set(&RubyValue::Int(1), "x", RubyValue::Nil));
        assert!(!set(&RubyValue::Nil, "x", RubyValue::Nil));
        assert!(get(&RubyValue::Int(1), "x").is_none());
    }
}
