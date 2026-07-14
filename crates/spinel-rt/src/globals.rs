//! `$foo`-family global variables -- a flat, genuinely process-wide store
//! (the same `LazyLock<Mutex<_>>` pattern `cvars`/`constants`/the Symbol
//! interner already use), needing no ancestor search at all: unlike `@@x`,
//! there's exactly ONE global namespace, shared by every class and every
//! thread.

use crate::RubyValue;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static GLOBALS: LazyLock<Mutex<HashMap<String, RubyValue>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// `nil` for a `$foo` never yet written -- matches real Ruby's own behavior
/// for reading a global variable before any assignment ever ran (no
/// `NameError`, unlike an unset constant -- see `constants::const_get`'s
/// docs for that distinction).
pub fn global_get(name: &str) -> RubyValue {
    GLOBALS.lock().get(name).cloned().unwrap_or(RubyValue::Nil)
}

pub fn global_set(name: &str, value: RubyValue) {
    GLOBALS.lock().insert(name.to_string(), value);
}
