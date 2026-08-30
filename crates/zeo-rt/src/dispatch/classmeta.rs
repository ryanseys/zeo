//! Per-class metadata: the frozen-class registry, the box surrogates,
//! name<->id resolution, nested constant names, and the refinement/
//! module-kind queries.

use super::*;

/// The ids of classes/modules an explicit `Foo.freeze` has frozen -- a
/// side registry keyed by class id (NOT a pointer-keyed table: ids are
/// minted once and never reused, so there is no ABA hazard), covering both
/// AOT-registered classes and `runtime_meta`'s overlay classes with one
/// mechanism. Empty in the overwhelmingly common no-`freeze` program: the
/// guard paths (cvar/civar writes, runtime method definition) pay one
/// short-held lock + hash probe only when actually reached.
static FROZEN_CLASSES: std::sync::LazyLock<parking_lot::Mutex<FSet<u32>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(FSet::default()));

/// `Foo.frozen?`'s storage half -- see `FROZEN_CLASSES`.
pub fn class_frozen(id: ClassId) -> bool {
    FROZEN_CLASSES.lock().contains(&id.0)
}

/// The `FrozenError` every mutation of a frozen class/module raises --
/// CRuby's exact shape (`can't modify frozen Class: Foo` /
/// `can't modify frozen Module: Bar`, oracle-verified): the receiver's own
/// class kind, then the frozen class's name (its inspect).
pub fn frozen_class_error(id: ClassId) -> Signal {
    let kind = if class_is_module(id).unwrap_or(false) {
        "Module"
    } else {
        "Class"
    };
    let name = class_name(id).unwrap_or_else(|| format!("#<Class:{}>", id.0));
    frozen_error!("can't modify frozen {kind}: {name}")
}

/// A REOPEN of a class the program has since frozen -- `Fz.freeze; class Fz;
/// def x; end; end` is `FrozenError` in ruby, and the body never runs.
///
/// zeo's compile-time tables already carry that body's definitions, so the
/// raise is not enough: `names` -- the ones ONLY this body would have
/// installed -- are retired first, which is what makes `Fz.instance_methods
/// (false)` answer `[]` and a call to one raise `NoMethodError`, exactly as
/// they do when the definition never happened.
pub fn guard_class_reopen(id: ClassId, names: &[&str]) -> Result<(), Signal> {
    if !class_frozen(id) {
        return Ok(());
    }
    crate::runtime_meta::retire_names(id, names);
    Err(frozen_class_error(id))
}

/// `Foo.freeze`'s storage half -- see `FROZEN_CLASSES`. Repeat calls are
/// harmless no-ops (CRuby's already-frozen guard).
pub fn class_set_frozen(id: ClassId) {
    FROZEN_CLASSES.lock().insert(id.0);
}

/// The bidirectional `box_id <-> surrogate ClassId` pair: forward map first,
/// reverse second.
type SurrogateMaps = (crate::FMap<u32, u32>, crate::FMap<u32, u32>);

/// The compile-time box surrogates, derived ONCE from the installed
/// registry's name table: `#<Ruby::Box:N>` is the exact name the analyze
/// pass mints per box. One parse at first ask replaces a `format!` + name
/// hash (or a name parse-back) at every ask; Track 7's `BoxTable` replaces
/// this derivation with real registration. Registry-less (unit tests) asks
/// answer `None` WITHOUT caching, so a later install still populates.
fn box_surrogates() -> Option<&'static SurrogateMaps> {
    static MAPS: OnceLock<SurrogateMaps> = OnceLock::new();
    if let Some(m) = MAPS.get() {
        return Some(m);
    }
    let reg = REGISTRY.get()?;
    Some(MAPS.get_or_init(|| {
        let mut by_box = crate::FMap::default();
        let mut by_class = crate::FMap::default();
        for (name, id) in &reg.by_name {
            if let Some(n) = name
                .strip_prefix("#<Ruby::Box:")
                .and_then(|r| r.strip_suffix('>'))
                .and_then(|d| d.parse::<u32>().ok())
            {
                by_box.insert(n, *id);
                by_class.insert(*id, n);
            }
        }
        (by_box, by_class)
    }))
}

/// The surrogate class owning box `box_id`'s top-level constants, if one was
/// compiled in.
pub(crate) fn box_surrogate_class(box_id: u32) -> Option<ClassId> {
    box_surrogates()?.0.get(&box_id).map(|&c| ClassId(c))
}

/// The highest COMPILE-TIME box id this program declared -- where a
/// run-time `Ruby::Box.new` starts numbering. 0 when it declared none.
pub(crate) fn highest_compile_time_box() -> u32 {
    box_surrogates().map_or(0, |m| m.0.keys().copied().max().unwrap_or(0))
}

/// The box a surrogate class belongs to -- [`box_surrogate_class`]'s reverse.
pub(crate) fn box_of_surrogate_class(cid: ClassId) -> Option<u32> {
    box_surrogates()?.1.get(&cid.0).copied()
}

/// Reverse of `class_name`: the id a fully-qualified class/module NAME is
/// registered under (`"Integer"`, `"Math"`, a user `"Widget"`), or `None`.
/// A run-time `eval` uses this to resolve a bare class-name constant like
/// `eval("Integer")` -- codegen resolves those statically and so never
/// `const_set`s them, leaving the runtime constants table without them.
pub fn class_id_by_name(name: &str) -> Option<ClassId> {
    REGISTRY
        .get()
        .and_then(|r| r.by_name.get(name))
        .map(|&id| ClassId(id))
        // A registered-but-CONCEALED class (a definition under a runtime
        // guard that has not run yet) is no constant of anyone's.
        .filter(|&ClassId(id)| !crate::constants::class_concealed(id))
        // Runtime-defined classes (e.g. `Struct.new` assigned to a constant)
        // live in the overlay, not the frozen registry -- fall back so
        // `Marshal.load` resolves them by name too.
        .or_else(|| crate::runtime_meta::runtime_class_id_by_name(name))
}

/// [`class_id_by_name`] without the concealment filter -- the id a compiled
/// `class Foo::Bar` registered, whether or not its body has run.
///
/// A C extension's `rb_define_class_under` wants this one. Concealment models
/// "ruby has no such constant YET", which is the right answer to a LOOKUP; a
/// C `define` is not a lookup but a definition, and CRuby's own
/// `rb_define_class_under` creates the class if it is absent and REOPENS it
/// otherwise. Answering `None` here minted a second class, so the extension's
/// rows and the program's own body ended up on different ids.
pub fn registered_class_id_by_name(name: &str) -> Option<ClassId> {
    REGISTRY
        .get()
        .and_then(|r| r.by_name.get(name))
        .map(|&id| ClassId(id))
        .or_else(|| crate::runtime_meta::runtime_class_id_by_name(name))
}

/// The classes and modules nested DIRECTLY inside `id`, by their unqualified
/// names -- `["Error", "ZStream", ...]` for `Zlib`.
///
/// A nested class IS a constant of the module it sits in, but zeo registers it
/// by qualified NAME rather than through the constant table: codegen resolves
/// `Zlib::Error` statically, so nothing ever `const_set`s it. `Module#constants`
/// is where the difference becomes visible, and this is what it consults --
/// the same shape as `class_id_by_name`, read the other way round.
pub fn nested_class_names(id: ClassId) -> Vec<String> {
    let Some(registry) = REGISTRY.get() else {
        return Vec::new();
    };
    // `Object` is the lexical parent of every TOP-LEVEL class, which the
    // registry files under a bare name rather than a `Prefix::` one -- so
    // `Object.constants` has to match the names with no separator at all.
    // Without this it listed `RUBY_VERSION` and the other seeded constants
    // while omitting `Array`, `String` and ~104 more.
    // A BOX's top level lists the box's own top-level classes, and main's
    // lists everything else: one name table, separated by the box a class
    // belongs to.
    let box_of = crate::boxes::box_of_surrogate_class(id);
    if id == crate::ClassId(0) || box_of.is_some() {
        let want = box_of.unwrap_or(0);
        return registry
            .by_name
            .iter()
            .filter(|&(_, &cid)| !crate::constants::class_concealed(cid))
            .filter(|&(_, &cid)| crate::boxes::class_box(crate::ClassId(cid)) == want)
            .map(|(name, _)| name)
            .filter(|name| !name.contains("::") && !name.contains('#') && !name.contains('.'))
            .map(String::clone)
            .collect();
    }
    let Some(prefix) = class_name(id) else {
        return Vec::new();
    };
    let prefix = format!("{prefix}::");
    registry
        .by_name
        .iter()
        .filter(|&(_, &cid)| !crate::constants::class_concealed(cid))
        .filter_map(|(name, _)| name.strip_prefix(&prefix))
        // DIRECTLY nested only: `Zlib::GzipFile::Error` belongs to
        // `Zlib::GzipFile`'s list, not to `Zlib`'s.
        .filter(|rest| !rest.contains("::"))
        // A `#` marks a name the source could never have written -- a
        // `refine` holder -- which is therefore no constant of anyone's.
        .filter(|rest| !rest.contains('#'))
        .map(str::to_string)
        .collect()
}

/// Whether `id` is a `refine` holder, whose `.class` is `Refinement`
/// rather than `Module`.
pub fn class_is_refinement(id: ClassId) -> bool {
    refinement_of(id).is_some()
}

/// `(refining module, refined target)` for a `refine` holder -- `None` for
/// every ordinary module. A holder can be compiled (the registry's mark) or
/// minted by a RUNTIME `refine` (the overlay's).
pub fn refinement_of(id: ClassId) -> Option<(ClassId, ClassId)> {
    if let Some(pair) = REGISTRY
        .get()
        .and_then(|r| r.entries.get(&id.0))
        .and_then(|e| e.refinement_of)
    {
        return Some(pair);
    }
    if crate::runtime_meta::is_live() {
        return crate::runtime_meta::overlay_refinement_of(id);
    }
    None
}

/// The holders `module`'s own `refine` blocks minted, in the order the source
/// wrote them -- ids are handed out as the compiler walks the body, so id
/// order IS declaration order. What `Module#refinements` answers.
pub fn refinements_of(module: ClassId) -> Vec<ClassId> {
    let Some(r) = REGISTRY.get() else {
        return Vec::new();
    };
    let mut holders: Vec<ClassId> = r
        .entries
        .iter()
        .filter(|(_, e)| e.refinement_of.is_some_and(|(m, _)| m == module))
        .map(|(id, _)| ClassId(id))
        .collect();
    holders.sort_by_key(|c| c.0);
    // Runtime-minted holders come after every compiled one -- their ids are
    // allocated above the frozen range, so a plain append keeps the order.
    if crate::runtime_meta::is_live() {
        holders.extend(crate::runtime_meta::overlay_refinements_of(module));
    }
    holders
}

/// Whether `id` names a MODULE (drives `Widget.class` -> `Class` vs
/// `Enumerable.class` -> `Module`) -- same graceful `None` as `class_name`.
pub fn class_is_module(id: ClassId) -> Option<bool> {
    if let Some(m) = REGISTRY
        .get()
        .and_then(|r| r.entries.get(&id.0))
        .map(|e| e.is_module)
    {
        return Some(m);
    }
    if crate::runtime_meta::is_live() {
        return crate::runtime_meta::overlay_is_module(id);
    }
    None
}
