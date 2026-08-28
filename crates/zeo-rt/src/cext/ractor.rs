//! Ractor-local storage.
//!
//! zeo has ONE ractor, so a ractor-local slot is a process-global one. That is
//! not an approximation: CRuby's main ractor holds exactly one copy of each
//! key too, and every thread in it reads that copy. A program that makes a
//! second ractor is refused long before it reaches here.
//!
//! openssl is why this exists. `ossl.c` keeps its per-ractor error queue in a
//! `ptr` key, so `Init_openssl` reaches the newkey entry on its first line.
//!
//! The `type` argument names a `mark` and a `free`. Neither runs: zeo's single
//! ractor ends with the process, exactly as CRuby's main ractor does, and
//! CRuby does not free the main ractor's storage at exit either.

use super::value::Value;
use std::ffi::c_void;

/// One slot per key. The key an extension holds is its INDEX plus one, cast to
/// a pointer, so a key is never null -- extensions test for that.
static SLOTS: parking_lot::Mutex<Vec<usize>> = parking_lot::Mutex::new(Vec::new());

/// A key's address is an index, not a pointer to dereference.
type Key = *mut c_void;

fn newkey() -> Key {
    let mut slots = SLOTS.lock();
    slots.push(0);
    slots.len() as Key
}

fn index(key: Key) -> Option<usize> {
    (key as usize).checked_sub(1)
}

fn get(key: Key) -> usize {
    index(key)
        .and_then(|i| SLOTS.lock().get(i).copied())
        .unwrap_or(0)
}

fn set(key: Key, word: usize) {
    if let Some(i) = index(key)
        && let Some(slot) = SLOTS.lock().get_mut(i)
    {
        *slot = word;
    }
}

crate::cext_fn! {
    /// A fresh `ptr` key. The `type` is recorded nowhere -- see the module
    /// docs for why its `free` never runs.
    fn rb_ractor_local_storage_ptr_newkey(_type: *const c_void) -> *mut c_void {
        Ok(newkey())
    }

    fn rb_ractor_local_storage_ptr(key: *mut c_void) -> *mut c_void {
        Ok(get(key) as *mut c_void)
    }

    fn rb_ractor_local_storage_ptr_set(key: *mut c_void, ptr: *mut c_void) -> () {
        set(key, ptr as usize);
        Ok(())
    }

    /// A fresh `value` key. Its slot is registered as a GC root at once, so a
    /// stored object stays alive without the extension holding it anywhere
    /// else -- which is the whole point of the value half.
    fn rb_ractor_local_storage_value_newkey() -> *mut c_void {
        let key = newkey();
        // `Q_UNDEF` is "never set", which `_lookup` reports as absent. A key
        // deliberately set to nil is a DIFFERENT answer, and both matter.
        let slot = Box::leak(Box::new(super::value::Q_UNDEF));
        set(key, std::ptr::from_mut(slot) as usize);
        super::call::register_root(slot);
        Ok(key)
    }

    fn rb_ractor_local_storage_value(key: *mut c_void) -> Value {
        let held = value_slot(key).map_or(super::value::Q_UNDEF, |s| unsafe { *s });
        Ok(match held == super::value::Q_UNDEF {
            true => super::value::Q_NIL,
            false => held,
        })
    }

    /// Answers whether the key was ever SET, which is not the same as its
    /// value being non-nil: a key set to nil looks up as present.
    fn rb_ractor_local_storage_value_lookup(key: *mut c_void, out: *mut Value) -> bool {
        let Some(slot) = value_slot(key) else {
            return Ok(false);
        };
        let held = unsafe { *slot };
        if held == super::value::Q_UNDEF {
            return Ok(false);
        }
        if !out.is_null() {
            unsafe { *out = held };
        }
        Ok(true)
    }

    fn rb_ractor_local_storage_value_set(key: *mut c_void, val: Value) -> () {
        if let Some(slot) = value_slot(key) {
            unsafe { *slot = val };
        }
        Ok(())
    }
}

fn value_slot(key: Key) -> Option<*mut Value> {
    match get(key) {
        0 => None,
        word => Some(word as *mut Value),
    }
}
