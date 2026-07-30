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
use crate::FMap;
use std::sync::LazyLock;

/// Two-level (owner -> name -> value): the inner map's `Box<str>` keys let
/// every read probe with its borrowed `&str` -- the pre-split
/// `(u32, String)` key allocated a fresh `String` per read, once per
/// ancestor on the fallback walk.
static CONSTANTS: LazyLock<Mutex<crate::ScopedMap<RubyValue>>> =
    LazyLock::new(|| Mutex::new(FMap::default()));

/// The `Object`-owned constant names that existed before the program's own top
/// level ran -- `RUBY_VERSION`, `ARGV`, the seeded encodings, everything
/// `bootstrap::install_core_constants` puts in place. See
/// [`seal_master_constants`].
static MASTER: LazyLock<Mutex<Option<std::collections::HashSet<String>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Freeze the master set: called once from `install_core_constants`, at the
/// seam between startup seeding and `run_main`.
///
/// A `Ruby::Box` is a copy of the MASTER namespace, so it sees the constants
/// the runtime installed but NOT the ones the main program went on to define
/// -- the same line `globals.rs` draws for `$foo`. Both live in `Object`'s
/// table here, and the only thing distinguishing them is when they arrived, so
/// this is where the mark goes.
pub fn seal_master_constants() {
    let names = CONSTANTS
        .lock()
        .get(&0)
        .map(|m| m.keys().map(|name| name.to_string()).collect())
        .unwrap_or_default();
    *MASTER.lock() = Some(names);
}

/// A top-level constant as a BOX sees it: `Object`'s binding, but only for a
/// name that was already there when [`seal_master_constants`] ran. Before the
/// seal (or in a program that never calls it) this is plain [`const_get`].
pub fn const_get_master(name: &str) -> Option<RubyValue> {
    if let Some(master) = MASTER.lock().as_ref() {
        if !master.contains(name) {
            return None;
        }
    }
    const_get(0, name)
}

/// `owner`'s OWN binding for `name`, with no ancestor walk -- what
/// `const_get(name, false)`/`const_defined?(name, false)` ask for. A nested
/// class is a constant of its namespace too, and lives in the class registry
/// rather than this table, so it is checked alongside.
pub fn const_get_own(owner_class_id: u32, name: &str) -> Option<RubyValue> {
    if let Some(v) = CONSTANTS
        .lock()
        .get(&owner_class_id)
        .and_then(|m| m.get(name))
    {
        return Some(v.clone());
    }
    nested_class_of(crate::ClassId(owner_class_id), name).map(RubyValue::Class)
}

pub fn const_get(owner_class_id: u32, name: &str) -> Option<RubyValue> {
    let map = CONSTANTS.lock();
    if let Some(v) = map.get(&owner_class_id).and_then(|m| m.get(name)) {
        return Some(v.clone());
    }
    // Ruby constant lookup continues into the owner's ancestry: a bare
    // constant in a class/module that includes another (e.g. `include Math`
    // then a bare `PI`) resolves against the included module's constants.
    // The compile-time owner resolution can't see a builtin module's
    // constants, so this runtime walk covers them.
    for &anc in crate::dispatch::ancestors_of_value(crate::ClassId(owner_class_id)) {
        if anc.0 != owner_class_id {
            if let Some(v) = map.get(&anc.0).and_then(|m| m.get(name)) {
                return Some(v.clone());
            }
            if let Some(cid) = nested_class_of(anc, name) {
                return Some(RubyValue::Class(cid));
            }
        }
    }
    None
}

/// A class/module nested inside `owner` by unqualified `name`, as a constant
/// -- `(OpenSSL, "SSL") -> OpenSSL::SSL`. A nested BUILTIN lives in the class
/// registry rather than the constants table, so an ancestor walk that only
/// consulted the table missed it: `include OpenSSL` then a bare `SSL` (which
/// is how net/ftp reaches `OpenSSL::SSL`) found nothing.
fn nested_class_of(owner: crate::ClassId, name: &str) -> Option<crate::ClassId> {
    // A TOP-LEVEL class is a constant of `Object` under its bare name -- there
    // is no `Object::` prefix on it in the registry.
    if owner.0 == 0 {
        return crate::dispatch::class_id_by_name(name);
    }
    let owner_name = crate::dispatch::class_name(owner)?;
    crate::dispatch::class_id_by_name(&format!("{owner_name}::{name}"))
}

/// Explicit-scope constant lookup (`Scope::NAME`): the scope class and its
/// ancestors, but NOT a bare top-level (`Object`-owned) constant unless the
/// scope IS `Object`. CRuby's `Foo::BAR` raises `NameError` rather than
/// resolving a top-level `BAR` through `Object` merely being an ancestor of
/// `Foo` (a `Struct.new` block's constant lands at top level, so `Line::FLAGS`
/// must not find it).
pub fn const_get_scoped(owner_class_id: u32, name: &str) -> Option<RubyValue> {
    let map = CONSTANTS.lock();
    if let Some(v) = map.get(&owner_class_id).and_then(|m| m.get(name)) {
        return Some(v.clone());
    }
    for &anc in crate::dispatch::ancestors_of_value(crate::ClassId(owner_class_id)) {
        if anc.0 == owner_class_id || (anc.0 == 0 && owner_class_id != 0) {
            continue;
        }
        if let Some(v) = map.get(&anc.0).and_then(|m| m.get(name)) {
            return Some(v.clone());
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
        .get(&owner_class_id)
        .map(|m| m.keys().map(|name| name.to_string()).collect())
        .unwrap_or_default()
}

/// Drops `owner`'s OWN binding for `name`, returning it. An inherited
/// constant is left alone -- `Module#remove_const` only removes its own.
pub fn const_remove(owner_class_id: u32, name: &str) -> Option<RubyValue> {
    CONSTANTS
        .lock()
        .get_mut(&owner_class_id)
        .and_then(|m| m.remove(name))
}

pub fn const_set(owner_class_id: u32, name: &str, value: RubyValue) {
    // Naming an anonymous runtime class (`Foo = Class.new`): the FIRST
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
    CONSTANTS
        .lock()
        .entry(owner_class_id)
        .or_default()
        .insert(Box::from(name), value);
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
    const_set(
        0,
        "ARGV",
        RubyValue::Array(crate::collections::array_new(args)),
    );
}
