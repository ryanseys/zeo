//! Top-level/class-lexically-scoped constant storage -- keyed the same way
//! `cvars.rs` keys `@@x` storage: `(owner_class_id, name)`, where the OWNER
//! is resolved entirely at spinelc compile time (`analyze::mro::resolve_consts`,
//! mirroring `resolve_cvars` exactly -- nearest ancestor, including self,
//! that ever claimed the name first; a bare top-level constant is owned by
//! `Object` itself, matching real Ruby's own representation). Unlike a
//! cvar's "never assigned -> `nil`" convention, an unset constant is a
//! genuine, distinguishable "never set" state (`None`) -- real Ruby raises
//! `NameError` for this, not `nil`; see `codegen::expr`'s `ClassRef`/
//! `ConstWrite`/`QualifiedConstRead` handling for where that raise is
//! constructed. Process-wide-shared (Part 9 style), same reasoning as
//! `cvars`/the Symbol interner: two threads referencing the same top-level
//! constant must see the same value.

use crate::RubyValue;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static CONSTANTS: LazyLock<Mutex<HashMap<(u32, String), RubyValue>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn const_get(owner_class_id: u32, name: &str) -> Option<RubyValue> {
    CONSTANTS.lock().get(&(owner_class_id, name.to_string())).cloned()
}

pub fn const_set(owner_class_id: u32, name: &str, value: RubyValue) {
    CONSTANTS.lock().insert((owner_class_id, name.to_string()), value);
}
