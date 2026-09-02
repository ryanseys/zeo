//! Symbol names for Cranelift-emitted objects. The method-name mangling
//! core (crate-level `names.rs`) plugs in when methods lower; the
//! fixed program-level symbols live here.

/// The compiled `<main>` body -- `ProgramDesc.toplevel` points at it.
pub const TOPLEVEL: &str = "zeo_toplevel";
/// The one table of tables `zeo_rt_main` walks.
pub const PROGRAM_DESC: &str = "zeo_program_desc";
/// The program's read-only string bytes (file names, literals, feature
/// paths), one blob the `Str` tables point into.
pub const RODATA: &str = "zeo_rodata";
/// The `Str` (ptr, len) tables -- `loaded_features`, `parse_warnings`.
pub const STR_TABLES: &str = "zeo_str_tables";
/// The interned-symbol id array (`.bss`), filled by `zeo_unit_init`.
pub const SYMS: &str = "zeo_syms";

/// The `.bss` array of monomorphic inline caches -- one `CallSite` slot per
/// dynamic send the emitter could key by a COMPILE-TIME caller class.
/// `zeo_unit_init` initialises each with its caller (the runtime's
/// `OnceLock` is not valid zero bytes, so the slots cannot be left as they
/// come out of `.bss`).
pub const CALLSITES: &str = "zeo_callsites";

/// The `.bss` array of class-method caches -- one `ClassMethodSite` slot per
/// send whose RECEIVER is a Class immediate the emitter resolved.
/// `send_value_cached` rules a Class receiver out on purpose (its methods
/// resolve through a singleton-chain arm of its own), so `Foo.new` needs
/// this second array or it walks the chain on every call.
pub const CM_SITES: &str = "zeo_cm_sites";
/// The `.bss` array of `attach_function` sites -- one `FFISYM_SITE_SIZE`
/// slot per call site, holding the resolved C address at
/// `FFISYM_SITE_ADDR` once the first call has asked the runtime for it.
/// Zero is "unresolved", so `.bss` needs no init row.
pub const FFI_SITES: &str = "zeo_ffi_sites";
/// The rodata `Str` table `zeo_rt_syms_init` interns from -- one row per
/// `zeo_syms` slot, in the same order.
pub const SYM_ROWS: &str = "zeo_sym_rows";
/// The rodata caller-class blob `zeo_rt_callsites_init` reads -- one `u32`
/// per `zeo_callsites` slot.
pub const CALLSITE_CALLERS: &str = "zeo_callsite_callers";
/// The `.bss` array of constant-read caches -- one `ConstSite` slot per
/// emitted bare-constant read, epoch-validated against the runtime's
/// constant table. Same zero-bytes-are-not-a-slot rule as the call sites:
/// `zeo_unit_init` constructs the whole array in one bulk call.
pub const CONST_SITES: &str = "zeo_const_sites";
/// The `.bss` array of compiled-construction caches -- one `ClassNewSite`
/// slot per statically-constructed `Foo.new` (`construct_compiled`).
/// Bulk-initialized like the const sites (no per-slot constant).
pub const NEW_SITES: &str = "zeo_new_sites";
/// The `.bss` array of dynamic-caller caches -- one `DynCallerSite` slot
/// per send whose CALLER class is only a run-time fact (a dynamic-self
/// body). Bulk-initialized like the const sites.
pub const DYN_SITES: &str = "zeo_dyn_sites";
/// The `.rodata` table of proc shapes -- one `ProcShapeC` (plus its
/// `ParamC` rows) per block literal, so a proc creation hands over ONE
/// pointer instead of rebuilding the constants per call.
pub const PROC_SHAPES: &str = "zeo_proc_shapes";
/// The `.bss` byte array of builtin-reopen flags -- one byte per
/// `(builtin class, method name)` a program reopens at compile time.
///
/// A reopen's row registers at STARTUP, so without this every call written
/// ABOVE the `class Foo ... end` answers with the reopened body. The class
/// body stores 1 at its own document position; until then the reopened body
/// defers to the row it replaced. Both ends are emitted here, so there is no
/// table to register and nothing for the runtime to look up.
pub const REOPEN_FLAGS: &str = "zeo_reopen_flags";

/// Startup initialization: interns the symbol table (and, later, the
/// `.bss` site slots).
pub const UNIT_INIT: &str = "zeo_unit_init";

/// The `zeo` library's own installer for the run-time `eval` compiler,
/// named by `ProgramDesc.eval_install` in a program that can eval.
pub const EVAL_INSTALL: &str = "zeo_eval_install";

/// A fully-qualified owner (`Outer::Inner`) as a symbol fragment.
fn owner_fragment(owner: &str) -> String {
    owner.replace("::", "__")
}

/// The compiled body symbol for `Owner#name`.
pub fn method_symbol(owner: &str, name: &str) -> String {
    format!(
        "zeo_m_{}_{}",
        owner_fragment(owner),
        crate::names::ident_fragment(name)
    )
}

/// The `ValueFn` trampoline symbol for `Owner#name` (the dispatch-row twin
/// of the body above).
pub fn trampoline_symbol(owner: &str, name: &str) -> String {
    format!(
        "zeo_t_{}_{}",
        owner_fragment(owner),
        crate::names::ident_fragment(name)
    )
}

/// The compiled body symbol for class method `Owner.name`.
pub fn class_method_symbol(owner: &str, name: &str) -> String {
    format!(
        "zeo_cm_{}_{}",
        owner_fragment(owner),
        crate::names::ident_fragment(name)
    )
}

/// The `ValueFn` trampoline symbol for class method `Owner.name`.
pub fn class_trampoline_symbol(owner: &str, name: &str) -> String {
    format!(
        "zeo_ct_{}_{}",
        owner_fragment(owner),
        crate::names::ident_fragment(name)
    )
}

/// The owner spelling a SYMBOL uses: a per-box class shares its ruby name
/// with the main-box one, so the box rides in the symbol while the frame
/// label keeps the ruby name.
pub fn boxed_owner(name: &str, box_id: u32) -> String {
    match box_id {
        0 => name.to_string(),
        b => format!("b{b}_{name}"),
    }
}
