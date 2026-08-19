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
/// Startup initialization: interns the symbol table (and, later, the
/// `.bss` site slots).
pub const UNIT_INIT: &str = "zeo_unit_init";

/// The compiled body symbol for `Owner#name`.
pub fn method_symbol(owner: &str, name: &str) -> String {
    format!("zeo_m_{owner}_{}", crate::names::ident_fragment(name))
}

/// The `ValueFn` trampoline symbol for `Owner#name` (the dispatch-row twin
/// of the body above).
pub fn trampoline_symbol(owner: &str, name: &str) -> String {
    format!("zeo_t_{owner}_{}", crate::names::ident_fragment(name))
}
