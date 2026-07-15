//! `$foo`-family global variables -- keyed `(box_id, name)` since Phase 18:
//! every `Ruby::Box` gets a fully SEPARATE global table with no fallback
//! layer at all, which is empirically faithful to CRuby's box model (a box
//! reads `nil` for a `$g` main set: its clone-on-first-read pulls from the
//! ROOT entry, which user code never writes -- variable.c:1050, verified in
//! the plan's research contract). Box 0 is the root/main program. Same
//! `LazyLock<Mutex<_>>` pattern as `cvars`/`constants`/the Symbol interner.

use crate::RubyValue;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static GLOBALS: LazyLock<Mutex<HashMap<(u32, String), RubyValue>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `nil` for a `$foo` never yet written IN THIS BOX -- matches real Ruby's
/// own behavior for reading a global before any assignment ran (no
/// `NameError`, unlike an unset constant -- see `constants::const_get`'s
/// docs for that distinction).
pub fn global_get(box_id: u32, name: &str) -> RubyValue {
    GLOBALS
        .lock()
        .get(&(box_id, name.to_string()))
        .cloned()
        .unwrap_or(RubyValue::Nil)
}

pub fn global_set(box_id: u32, name: &str, value: RubyValue) {
    GLOBALS.lock().insert((box_id, name.to_string()), value);
}
