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
//! constructed. Process-wide-shared, for the same reason as
//! `cvars`/the Symbol interner: two threads referencing the same top-level
//! constant must see the same value.

use crate::FMap;
use crate::RubyValue;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, OnceLock};

/// Two-level (owner -> name -> value): the inner map's `Box<str>` keys let
/// every read probe with its borrowed `&str` -- the pre-split
/// `(u32, String)` key allocated a fresh `String` per read, once per
/// ancestor on the fallback walk.
static CONSTANTS: LazyLock<Mutex<crate::ScopedMap<RubyValue>>> =
    LazyLock::new(|| Mutex::new(FMap::default()));

/// Bumped by every operation that can change what any constant resolves to --
/// a write, a removal, and the naming of a runtime class (which is what makes
/// a nested `Owner::Name` newly resolvable). One counter for the whole table,
/// because a per-site cache only has to know that NOTHING changed.
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// `u64::MAX` marks a [`ConstSite`] that has not cached anything yet, so it can
/// never compare equal to a real epoch.
const UNFILLED: u64 = u64::MAX;

#[inline]
pub fn const_epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

pub(crate) fn bump_const_epoch() {
    EPOCH.fetch_add(1, Ordering::Release);
}

/// One emitted constant reference, holding the value it last resolved to and
/// the epoch it resolved at. The lookup behind it is a global lock, two hashes
/// and -- on a miss -- an ancestor walk, so a constant read in a loop is worth
/// caching; a hit costs two atomic loads and a `RubyValue` clone.
///
/// A site that fills its cache and then sees any constant written ANYWHERE
/// falls back to the full lookup permanently. That is deliberate: a `OnceLock`
/// cannot be refilled, and the alternative -- storage the site could keep
/// pointing at -- needs a lock per read, which is most of what this avoids.
/// Constants are written while a program loads and read while it runs, so the
/// sites that matter fill after the last write. A degraded site costs exactly
/// what every site cost before.
pub struct ConstSite {
    cached: OnceLock<RubyValue>,
    epoch: AtomicU64,
}

impl Default for ConstSite {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstSite {
    pub const fn new() -> Self {
        Self {
            cached: OnceLock::new(),
            epoch: AtomicU64::new(UNFILLED),
        }
    }

    /// `resolve` runs the site's real lookup. It is a closure rather than a
    /// `(owner, name)` pair because the emitted lookup is a chain -- the owner,
    /// then the enclosing top level, then a box's master namespace -- and the
    /// cache should cover all of it, not just the first link.
    #[inline]
    pub fn get(&self, resolve: impl FnOnce() -> Option<RubyValue>) -> Option<RubyValue> {
        if let Some(value) = self.cached.get() {
            if self.epoch.load(Ordering::Acquire) == const_epoch() {
                return Some(value.clone());
            }
            crate::builtins::rubyvm::CONST_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
            return resolve();
        }
        // Read BEFORE resolving: a write landing between the lookup and the
        // store must leave the site stamped with the older epoch, so readers
        // reject the value rather than trusting a stale one.
        let epoch = const_epoch();
        // Cold branch only -- `RubyVM.stat[:constant_cache_misses]`.
        crate::builtins::rubyvm::CONST_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
        let value = resolve()?;
        if self.cached.set(value.clone()).is_ok() {
            self.epoch.store(epoch, Ordering::Release);
        }
        Some(value)
    }
}

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
    if let Some(master) = MASTER.lock().as_ref()
        && !master.contains(name)
    {
        return None;
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
    const_search(owner_class_id, name, false)
}

/// `owner` and its ancestry, with `skip_object` deciding whether a constant
/// `Object` itself owns may answer. That single flag is the whole difference
/// between ruby's two constant searches (`variable.c`'s `exclude`), so both
/// spellings share this walk and cannot drift.
fn const_search(owner_class_id: u32, name: &str, skip_object: bool) -> Option<RubyValue> {
    let map = CONSTANTS.lock();
    if let Some(v) = map.get(&owner_class_id).and_then(|m| m.get(name)) {
        return Some(v.clone());
    }
    // The owner's OWN nested class, before any ancestor is consulted: a
    // subclass that redefines a nested name (`class L < B; class H`) owns it,
    // and reaching `B::H` first answered the base's class for `L::H` /
    // `L.const_get(:H)` -- the shape faraday's `self.class::Handler` is built
    // on. The ancestry walk below checks the same table for each ancestor.
    if let Some(cid) = nested_class_of(crate::ClassId(owner_class_id), name) {
        return Some(RubyValue::Class(cid));
    }
    // Ruby constant lookup continues into the owner's ancestry: a bare
    // constant in a class/module that includes another (e.g. `include Math`
    // then a bare `PI`) resolves against the included module's constants.
    // The compile-time owner resolution can't see a builtin module's
    // constants, so this runtime walk covers them.
    for &anc in crate::dispatch::ancestors_of_value(crate::ClassId(owner_class_id)) {
        if anc.0 == owner_class_id || (skip_object && anc.0 == 0) {
            continue;
        }
        if let Some(v) = map.get(&anc.0).and_then(|m| m.get(name)) {
            return Some(v.clone());
        }
        if let Some(cid) = nested_class_of(anc, name) {
            return Some(RubyValue::Class(cid));
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
/// ancestors, but NOT a constant `Object` itself owns unless the scope IS
/// `Object`. `Object` is an ancestor of every class, so without that rule
/// every top-level constant would answer as every class's own -- `K::Errno`
/// would find the top-level `Errno`, and `Line::FLAGS` would find a `FLAGS`
/// that a `Struct.new` block left at top level. Ruby raises `NameError` for
/// both, and reserves the through-`Object` answer for `const_get`.
pub fn const_get_scoped(owner_class_id: u32, name: &str) -> Option<RubyValue> {
    const_search(owner_class_id, name, owner_class_id != 0)
}

/// `(owner, name)` pairs marked `private_constant`. Kept here rather than on
/// the frozen `ClassEntry` because `public_constant` can clear a mark at RUN
/// time and the registry is immutable after install.
///
/// Reflection is all this drives: `Module#constants` and `defined?` filter on
/// it, and a qualified `M::A` guards on it (codegen only emits that guard where
/// the compiler already saw a `private_constant`, so an ordinary read is
/// untouched). `const_get` deliberately ignores it -- CRuby lets `const_get`
/// through, and rejects only the scope operator.
static PRIVATE_CONSTANTS: LazyLock<Mutex<FMap<u32, std::collections::HashSet<Box<str>>>>> =
    LazyLock::new(|| Mutex::new(FMap::default()));

/// Mark `names` private on `owner`, or (with `private` false) restore them.
pub fn const_set_private(owner_class_id: u32, names: &[&str], private: bool) {
    let mut map = PRIVATE_CONSTANTS.lock();
    let set = map.entry(owner_class_id).or_default();
    for name in names {
        if private {
            set.insert((*name).into());
        } else {
            set.remove(*name);
        }
    }
}

/// Whether `owner` marks constant `name` private.
pub fn const_is_private(owner_class_id: u32, name: &str) -> bool {
    PRIVATE_CONSTANTS
        .lock()
        .get(&owner_class_id)
        .is_some_and(|s| s.contains(name))
}

/// Class ids REGISTERED but not yet REVEALED: a class/module defined under a
/// top-level guard zeo cannot decide registers its shape in the prologue (the
/// static MRO needs one), but CRuby has no such constant until the guarded
/// body actually runs. Until `reveal_class` fires -- emitted at the head of
/// the class's body site, which runs at document position inside the
/// still-live `if` -- every by-name path answers as if the class did not
/// exist. Kept beside `PRIVATE_CONSTANTS` for the same reason it is: the
/// registry is frozen after install, and this is a mutable visibility fact.
static CONCEALED_CLASSES: LazyLock<Mutex<std::collections::HashSet<u32>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// One relaxed load spares every by-name lookup the lock in the common
/// program, which conceals nothing. Never cleared: a program that concealed
/// anything keeps paying the lock, and reveal-all is not distinguishable
/// cheaply from reveal-most.
static ANY_CONCEALED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Registers `id` as concealed. Called from `main`'s registration prologue,
/// before any Ruby statement runs.
pub fn conceal_class(id: u32) {
    CONCEALED_CLASSES.lock().insert(id);
    ANY_CONCEALED.store(true, Ordering::Release);
}

/// The guarded definition ran: the constant exists from here on. Idempotent
/// (a reopened conditional class reveals at every site).
pub fn reveal_class(id: u32) {
    if ANY_CONCEALED.load(Ordering::Acquire) && CONCEALED_CLASSES.lock().remove(&id) {
        // Per-site caches may hold a miss-shaped answer for a name this
        // reveal just made resolvable.
        bump_const_epoch();
    }
}

/// Whether `id` is registered but concealed.
pub fn class_concealed(id: u32) -> bool {
    ANY_CONCEALED.load(Ordering::Acquire) && CONCEALED_CLASSES.lock().contains(&id)
}

/// A constant read of a runtime-conditional class: `NameError` while
/// concealed (CRuby never had the constant), the class value once revealed.
/// `owner` is the lexical scope the read names, `Object` for a top-level one
/// -- it becomes `NameError#receiver`.
pub fn conditional_class_ref(
    id: crate::ClassId,
    fq_name: &str,
    owner: crate::ClassId,
) -> Result<RubyValue, crate::Signal> {
    if class_concealed(id.0) {
        let leaf = fq_name.rsplit("::").next().unwrap_or(fq_name);
        Err(crate::Signal::Raise(crate::dispatch::stamp_backtrace(
            crate::dispatch::make_name_error(
                format!("uninitialized constant {fq_name}"),
                leaf,
                RubyValue::Class(owner),
            ),
        )))
    } else {
        Ok(RubyValue::Class(id))
    }
}

/// The `defined?`/`const_defined?` half of [`conditional_class_ref`].
pub fn class_revealed(id: crate::ClassId) -> bool {
    !class_concealed(id.0)
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
    bump_const_epoch();
    CONSTANTS
        .lock()
        .get_mut(&owner_class_id)
        .and_then(|m| m.remove(name))
}

pub fn const_set(owner_class_id: u32, name: &str, value: RubyValue) {
    bump_const_epoch();
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

/// Where each constant was assigned -- `(owner, name) -> (file, line)`, the
/// answer behind `Module#const_source_location`. CRuby keeps it on the
/// constant entry itself (`rb_const_entry_t`); a table beside the values keeps
/// the read path -- the hot one -- exactly as wide as it was.
///
/// A REASSIGNMENT overwrites the record, the way CRuby's `setup_const_entry`
/// does, so the location always names the write that is in force. A `class` or
/// `module` reopen does not: it creates nothing, so nothing restamps it.
static LOCATIONS: LazyLock<Mutex<crate::ScopedMap<SourceLine>>> =
    LazyLock::new(|| Mutex::new(FMap::default()));

/// Where one constant was written: the file, as the program names it, and the
/// line. Both come from a compiled literal or a live frame, so neither is owned.
type SourceLine = (&'static str, u32);

/// Records where a constant that lives OUTSIDE the value table came from -- a
/// `class`/`module` declaration, which the class registry holds.
pub fn record_const_location(owner_class_id: u32, name: &str, file: &'static str, line: u32) {
    LOCATIONS
        .lock()
        .entry(owner_class_id)
        .or_default()
        .insert(Box::from(name), (file, line));
}

/// [`const_set`] plus where the assignment was written -- and, when the name
/// is already bound, CRuby's two-line reassignment warning. The gate is
/// VALUE presence: `remove_const` deletes the value (so a re-set after it is
/// a fresh definition, silently), and a `class`/`module` reopen never comes
/// through the value table at all. Only located assignments warn -- written
/// ones and `Module#const_set` route here; bootstrap seeding uses the plain
/// [`const_set`] and stays quiet.
pub fn const_set_at(
    owner_class_id: u32,
    name: &str,
    value: RubyValue,
    file: &'static str,
    line: u32,
) {
    let already = CONSTANTS
        .lock()
        .get(&owner_class_id)
        .is_some_and(|m| m.contains_key(name));
    // A zeo-authored shim re-executing IS CRuby's `$LOADED_FEATURES` no-op:
    // the loader gives every requiring unit its own copy of the shim body
    // (rbconfig inside two lazily-loaded units), so the same virtual-path
    // assignment can run twice. CRuby prints nothing there -- the second
    // require never runs -- so neither does this. A redefinition from REAL
    // code (different location) still warns.
    let shim_rerun = file.starts_with("<zeo-shim>")
        && const_location(owner_class_id, name, true)
            .is_some_and(|(prev_file, prev_line)| prev_file == file && prev_line == line);
    if already && !shim_rerun {
        // Line 1 carries the qualified name, line 2 the bare one, exactly as
        // CRuby prints them; the OLD location is read before the new record
        // overwrites it. A binding with no recorded location (a builtin) gets
        // line 1 alone -- CRuby's shape there too.
        let qualified = match crate::dispatch::class_name(crate::ClassId(owner_class_id)) {
            Some(owner) if owner_class_id != 0 => format!("{owner}::{name}"),
            _ => name.to_string(),
        };
        eprintln!("{file}:{line}: warning: already initialized constant {qualified}");
        if let Some((prev_file, prev_line)) = const_location(owner_class_id, name, true) {
            eprintln!("{prev_file}:{prev_line}: warning: previous definition of {name} was here");
        }
    }
    const_set(owner_class_id, name, value);
    record_const_location(owner_class_id, name, file, line);
}

/// Where `owner` -- or, unless `own_only`, anything it inherits from -- says
/// `name` was assigned. `None` means nobody recorded one, which for a constant
/// that DOES exist is CRuby's "defined in C" answer rather than a miss.
pub fn const_location(owner_class_id: u32, name: &str, own_only: bool) -> Option<SourceLine> {
    let map = LOCATIONS.lock();
    let of = |id: u32| map.get(&id).and_then(|m| m.get(name)).copied();
    if let Some(loc) = of(owner_class_id) {
        return Some(loc);
    }
    if own_only {
        return None;
    }
    crate::dispatch::ancestors_of_value(crate::ClassId(owner_class_id))
        .iter()
        .find_map(|anc| of(anc.0))
        // A module's ancestry does not pass through `Object`, but the
        // inheriting search consults it anyway -- `const_lookup`'s own rule.
        .or_else(|| of(0))
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
    let argv = RubyValue::Array(crate::collections::array_new(args));
    // `$*` is an ALIAS of ARGV -- the same array object (`$*.equal?(ARGV)`),
    // not a copy.
    crate::globals::global_set(0, "$*", argv.clone());
    const_set(0, "ARGV", argv);
}
