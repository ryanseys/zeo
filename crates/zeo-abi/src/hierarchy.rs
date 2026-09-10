//! The built-in class graph: who inherits what, who includes what, and
//! the ancestor list each of those answers.

use super::*;

/// Builtins whose `allocate` singleton method CRuby `undef`s outright
/// (`rb_undef_method(CLASS_OF(x), "allocate")`), rather than merely leaving
/// them without an allocator. The two refusals differ in class and in
/// message: an undef'd name is `NoMethodError: undefined method 'allocate'
/// for class X`, where a missing allocator is `TypeError: allocator undefined
/// for X`. Inherited, so a subclass of one of these refuses the same way.
pub const ALLOCATE_UNDEFINED: &[ClassId] = &[
    RATIONAL_CLASS,
    COMPLEX_CLASS,
    MATCH_DATA_CLASS,
    MODULE_CLASS,
    REFINEMENT_CLASS,
];

/// Builtins whose SINGLETON class mixes a module in -- CRuby's `extend`, which
/// no [`BuiltinClass`] field can express (`includes` is the instance side, and
/// the two are independent: `CGI` does both with the same module).
/// `(class, modules)`, in source order, exactly as `includes` is.
pub const BUILTIN_EXTENDS: &[(ClassId, &[ClassId])] = &[(CGI_MODULE, &[CGI_ESCAPE_MODULE])];

/// Builtins that PREPEND a module -- ahead of their own methods, so the module
/// wins a name they both define. `CGI::Escape` prepends `CGI::EscapeExt`, which
/// is why `CGI.method(:escapeHTML).owner` is `EscapeExt` while
/// `CGI.method(:escapeElement).owner` is `Escape`.
pub const BUILTIN_PREPENDS: &[(ClassId, &[ClassId])] =
    &[(CGI_ESCAPE_MODULE, &[CGI_ESCAPE_EXT_MODULE])];

/// Top-level constant aliases for nested builtins Ruby ALSO exposes at the
/// top level: `::Queue = Thread::Queue`, `::Mutex = Thread::Mutex`, etc. The
/// class is defined `Thread::`-nested (so `.name`/`inspect` report the
/// qualified path, matching CRuby), while these aliases let bare `Queue`/
/// `Mutex`/`SizedQueue`/`ConditionVariable` still resolve at the top level.
/// `(alias_name, target_id)`; consulted last in name resolution so a user's
/// own top-level constant of the same name still wins.
pub const TOP_LEVEL_ALIASES: &[(&str, ClassId)] = &[
    ("Queue", QUEUE_CLASS),
    ("SizedQueue", SIZED_QUEUE_CLASS),
    ("Mutex", MUTEX_CLASS),
    ("ConditionVariable", CONDITION_VARIABLE_CLASS),
];

/// `Object`'s own hierarchy slot (it isn't a [`BUILTINS`] row):
/// superclass `BasicObject`, includes `Kernel` -- oracle-verified
/// `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_SUPERCLASS: ClassId = BASIC_OBJECT_CLASS;
pub const OBJECT_INCLUDES: &[ClassId] = &[KERNEL_CLASS];

/// Whether `id` names a class this ABI defines -- `Object`, a [`BUILTINS`]
/// row, or an [`EXCEPTION_CLASSES`] row. What separates a row the runtime
/// provides from one a program wrote.
#[must_use]
pub fn is_core_class(id: ClassId) -> bool {
    id == OBJECT_CLASS
        || BUILTINS.iter().any(|b| b.id == id)
        || EXCEPTION_CLASSES.iter().any(|e| e.id == id)
}

/// A core class's `(superclass, includes)` edges, covering `Object`, every
/// [`BUILTINS`] row, and every [`EXCEPTION_CLASSES`] row. The single source both
/// the compiler's seeding (`Compiler::new`) and the runtime's registry
/// (`ClassRegistry::with_core`) derive the hierarchy from. Almost no exception
/// includes a module; the few that do are in [`EXCEPTION_INCLUDES`].
fn core_class_edges(id: ClassId) -> (Option<ClassId>, &'static [ClassId]) {
    if id == OBJECT_CLASS {
        return (Some(OBJECT_SUPERCLASS), OBJECT_INCLUDES);
    }
    if let Some(b) = BUILTINS.iter().find(|b| b.id == id) {
        return (b.superclass, b.includes);
    }
    if let Some(e) = EXCEPTION_CLASSES.iter().find(|e| e.id == id) {
        let includes = EXCEPTION_INCLUDES
            .iter()
            .find(|(c, _)| *c == id)
            .map_or(&[][..], |(_, m)| *m);
        return (e.superclass, includes);
    }
    (None, &[])
}

fn expand_core(id: ClassId, out: &mut Vec<ClassId>) {
    if out.contains(&id) {
        return;
    }
    out.push(id);
    let (superclass, includes) = core_class_edges(id);
    // `includes` reversed, then the superclass -- the exact order (and the
    // dedup above) the compiler's `mro::expand_into` uses, so an UNMODIFIED
    // core class linearizes here identically to how the compiler linearizes it.
    for &m in includes.iter().rev() {
        expand_core(m, out);
    }
    if let Some(parent) = superclass {
        expand_core(parent, out);
    }
}

/// Whether this builtin needs a per-program registry entry: `register_builtins`
/// installs exactly the UNGATED ones, so a `require`-gated class must be
/// registered by the program that activates it.
///
/// Asked of the ABI rather than of the compiler's own `feature_gate`, which a
/// reopen deliberately clears to materialize the constant -- that changes name
/// resolution, never who registered the class.
pub fn is_gated_builtin(id: ClassId) -> bool {
    BUILTINS
        .iter()
        .any(|b| b.id.0 == id.0 && b.feature.is_some())
}

/// A core class's DECLARED linearized ancestors -- what the ABI declares for
/// it before any program reopens it.
///
/// One DFS covers builtins and exceptions alike. For example,
/// `declared_ancestors(StandardError)` walks its superclass chain up to
/// `Exception`, then `Object`'s own tail, giving
/// `[StandardError, Exception, Object, Kernel, BasicObject]`.
///
/// The runtime installs these once, in `ClassRegistry::with_core`. The
/// compiler emits a per-program OVERRIDE only when a program actually changes
/// a builtin's ancestors, so `class Array; include M; end` keeps full parity
/// without every program re-listing the unchanged hierarchy.
pub fn declared_ancestors(id: ClassId) -> Vec<ClassId> {
    let mut out = Vec::new();
    expand_core(id, &mut out);
    out
}

/// The Ruby-visible name of any builtin id, `Object` included. `None` for
/// user-class ids. Backs NoMethodError messages and registry-less display.
pub fn builtin_name(id: ClassId) -> Option<&'static str> {
    if id == OBJECT_CLASS {
        return Some("Object");
    }
    BUILTINS
        .get((id.0 as usize).wrapping_sub(1))
        .filter(|b| b.id == id)
        .map(|b| b.name)
}
