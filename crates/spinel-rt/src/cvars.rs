//! `@@x` class-variable storage. Ownership (which class/module's storage a
//! given `@@x` reference actually refers to) is resolved entirely at
//! spinelc COMPILE time (`analyze::mro::resolve_cvars`, searching the real
//! linearized `ancestors` for an existing owner before allocating fresh
//! storage -- the exact search spinel's own C implementation skips,
//! causing a real bug there: a subclass writing a superclass-only cvar
//! silently gets its own, wrong, per-class storage instead of sharing the
//! existing one). By the time generated code calls into this module, the
//! owner is already a plain compile-time-literal class id -- this is just
//! the runtime-mutable `HashMap` backing that already-resolved slot, keyed
//! by `(owner_class_id, name)` since one class can own several distinct
//! `@@` names.

use crate::RubyValue;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static CVARS: RefCell<HashMap<(u32, String), RubyValue>> = RefCell::new(HashMap::new());
}

/// `nil` for a `@@x` never yet written -- matches real Ruby's own behavior
/// for reading a class variable before any assignment ever ran (a
/// `NameError` in REAL CRuby, technically, but this spike's established
/// posture elsewhere treats "never assigned" as `nil` rather than adding a
/// distinct raise for it -- see `IvarRead`'s identical convention).
pub fn cvar_get(owner_class_id: u32, name: &str) -> RubyValue {
    CVARS.with(|c| {
        c.borrow()
            .get(&(owner_class_id, name.to_string()))
            .cloned()
            .unwrap_or(RubyValue::Nil)
    })
}

pub fn cvar_set(owner_class_id: u32, name: &str, value: RubyValue) {
    CVARS.with(|c| {
        c.borrow_mut()
            .insert((owner_class_id, name.to_string()), value);
    });
}
