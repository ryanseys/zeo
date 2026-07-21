//! Top-level/class-lexically-scoped constant storage -- keyed the same way
//! `cvars.rs` keys `@@x` storage: `(owner_class_id, name)`, where the OWNER
//! is resolved entirely at zeo compile time (`analyze::mro::resolve_consts`,
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
    let map = CONSTANTS.lock();
    if let Some(v) = map.get(&(owner_class_id, name.to_string())) {
        return Some(v.clone());
    }
    // Ruby constant lookup continues into the owner's ancestry: a bare
    // constant in a class/module that includes another (e.g. `include Math`
    // then a bare `PI`) resolves against the included module's constants.
    // The compile-time owner resolution can't see a builtin module's
    // constants, so this runtime walk covers them.
    for &anc in crate::dispatch::ancestors_of_value(crate::ClassId(owner_class_id)) {
        if anc.0 != owner_class_id {
            if let Some(v) = map.get(&(anc.0, name.to_string())) {
                return Some(v.clone());
            }
        }
    }
    None
}

/// The constant names owned DIRECTLY by `owner_class_id` (not its ancestors)
/// -- the per-class half of `Module#constants`. Order is unspecified (a
/// `HashMap` iteration), matching CRuby's own id-table nondeterminism; callers
/// asserting a stable result sort it.
pub fn const_names_of(owner_class_id: u32) -> Vec<String> {
    CONSTANTS
        .lock()
        .keys()
        .filter(|(owner, _)| *owner == owner_class_id)
        .map(|(_, name)| name.clone())
        .collect()
}

pub fn const_set(owner_class_id: u32, name: &str, value: RubyValue) {
    // Naming an anonymous runtime class (`Foo = Class.new`, #97 F4): the FIRST
    // constant it's bound to becomes its name, matching CRuby -- so `Foo.name`
    // / `puts Foo` report `"Foo"` rather than `#<Class:...>`. A no-op for a
    // frozen class id or an already-named one.
    // Bound inside a namespace (`NS::Item = Class.new`), the name CRuby gives
    // it is the qualified path, not the bare constant.
    if let RubyValue::Class(cid) = &value {
        let qualified = match crate::dispatch::class_name(crate::ClassId(owner_class_id)) {
            Some(owner) if owner_class_id != 0 => format!("{owner}::{name}"),
            _ => name.to_string(),
        };
        crate::runtime_meta::name_runtime_class_if_anonymous(*cid, &qualified);
    }
    CONSTANTS.lock().insert((owner_class_id, name.to_string()), value);
}

/// Installs `ARGV` (the program's arguments, minus the binary name, as an
/// Array of Strings) as a top-level constant -- called once from generated
/// `main()`, mirroring CRuby's own startup. Reads resolve through the
/// ordinary runtime `const_get` fallback, so the compiler needs no
/// special-casing.
pub fn seed_argv() {
    let args: Vec<RubyValue> = std::env::args()
        .skip(1)
        .map(|a| RubyValue::Str(crate::collections::string_new(a)))
        .collect();
    const_set(0, "ARGV", RubyValue::Array(crate::collections::array_new(args)));
}
