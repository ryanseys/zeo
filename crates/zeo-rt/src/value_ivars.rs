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
//! value lives, so every write PINS its owner (see [`OWNERS`]). Without the pin
//! a dead value's address gets handed to an unrelated later one, which would
//! silently inherit its ivars.
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

struct Store {
    ivars: RwLock<FMap<usize, Slots>>,
    /// A strong reference to every value that has ever been given an ivar, so
    /// its address can never be reused. See the module docs.
    owners: RwLock<FMap<usize, RubyValue>>,
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
/// `Object` and `Class` are absent deliberately -- they have their own stores
/// and every caller here checks them first. So is the permanently-frozen tier
/// (immediates, and `Range`, which zeo reports frozen exactly as Ruby does):
/// those raise `FrozenError` before reaching this, which is the right answer
/// rather than a missing feature.
fn key(v: &RubyValue) -> Option<usize> {
    let addr = match v {
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
    store()
        .owners
        .write()
        .unwrap()
        .entry(k)
        .or_insert_with(|| v.clone());
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
    store()
        .owners
        .write()
        .unwrap()
        .entry(dst)
        .or_insert_with(|| to.clone());
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
