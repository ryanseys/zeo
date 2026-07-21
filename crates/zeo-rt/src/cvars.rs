//! `@@x` class-variable storage. Ownership (which class/module's storage a
//! given `@@x` reference actually refers to) is resolved entirely at
//! zeo COMPILE time (`analyze::mro::resolve_cvars`, searching the real
//! linearized `ancestors` for an existing owner before allocating fresh
//! storage -- the exact search zeo's own C implementation skips,
//! causing a real bug there: a subclass writing a superclass-only cvar
//! silently gets its own, wrong, per-class storage instead of sharing the
//! existing one). By the time generated code calls into this module, the
//! owner is already a plain compile-time-literal class id -- this is just
//! the runtime-mutable `HashMap` backing that already-resolved slot, keyed
//! by `(owner_class_id, name)` since one class can own several distinct
//! `@@` names.
//!
//! Genuinely process-wide-shared storage (Part 9), not per-thread: a real
//! CRuby `@@counter` incremented by one `Thread` must be visible to another,
//! so this migrated from a `thread_local!` to a real `LazyLock<Mutex<_>>`
//! static rather than just swapping `Rc`/`RefCell` for `Arc`/`Mutex` in
//! place -- leaving it thread-local would have silently made class
//! variables per-thread, a real semantic bug, not just a representation
//! change.

use crate::RubyValue;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static CVARS: LazyLock<Mutex<HashMap<(u32, String), RubyValue>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `nil` for a `@@x` never yet written -- matches real Ruby's own behavior
/// for reading a class variable before any assignment ever ran (a
/// `NameError` in REAL CRuby, technically, but this spike's established
/// posture elsewhere treats "never assigned" as `nil` rather than adding a
/// distinct raise for it -- see `IvarRead`'s identical convention).
pub fn cvar_get(owner_class_id: u32, name: &str) -> RubyValue {
    CVARS
        .lock()
        .get(&(owner_class_id, name.to_string()))
        .cloned()
        .unwrap_or(RubyValue::Nil)
}

pub fn cvar_set(owner_class_id: u32, name: &str, value: RubyValue) {
    CVARS
        .lock()
        .insert((owner_class_id, name.to_string()), value);
}

/// The class-variable names (`@@x`) owned DIRECTLY by `owner_class_id` -- the
/// per-class half of `Module#class_variables`. Order is unspecified (a HashMap
/// iteration); callers asserting a stable result sort it.
pub fn cvar_names_of(owner_class_id: u32) -> Vec<String> {
    CVARS
        .lock()
        .keys()
        .filter(|(owner, _)| *owner == owner_class_id)
        .map(|(_, name)| name.clone())
        .collect()
}

/// Whether `@@name` has EVER been assigned on `owner_class_id` -- the
/// genuine "defined" distinction `cvar_get`'s nil-on-miss convention can't
/// express, needed by `Module#class_variable_defined?`/`class_variable_get`
/// (the latter raises `NameError` on a never-set name, unlike a plain
/// `@@x` read).
pub fn cvar_defined(owner_class_id: u32, name: &str) -> bool {
    CVARS
        .lock()
        .contains_key(&(owner_class_id, name.to_string()))
}
