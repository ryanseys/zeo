//! Symbol names for Cranelift-emitted objects. The method-name mangling
//! core (crate-level `names.rs`) plugs in when methods lower (M0-11); the
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
/// Startup initialization: interns the symbol table (and, later, the
/// `.bss` site slots).
pub const UNIT_INIT: &str = "zeo_unit_init";

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
