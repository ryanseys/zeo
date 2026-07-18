//! The runtime's startup surface: assembling the class registry every generated
//! program installs, and seeding the core constants.
//!
//! A generated `main` does just two things here: build a registry pre-populated
//! with the CORE classes (`ClassRegistry::with_core`), then -- after layering
//! its own user classes on top and installing it -- seed the CORE constants
//! (`install_core_constants`). The alternative was ~540 identical `register(...)`
//! calls plus eight `seed_*` calls emitted into every program.

use spinel_abi::{declared_ancestors, BUILTINS, OBJECT_CLASS};

use crate::builtins::exception::register_exceptions;
use crate::dispatch::ClassRegistry;

/// Install the always-on built-in classes/modules (`Integer`, `Array`,
/// `Kernel`, ... and `Object`) into `registry` with their DECLARED ancestors --
/// the fixed hierarchy every program shares, which used to be ~540 lines of
/// identical `__registry.register(...)` calls in every generated `main()`.
/// Require-gated extensions (`Base64`, `StringIO`, ...) are NOT here: they stay
/// per-program in codegen so an un-`require`d one contributes nothing (its
/// constant must stay invisible). A program that reopens a builtin to change its
/// ancestors (`class Array; include M; end`) still works: codegen emits a
/// targeted override that lands after this, replacing the entry.
pub fn register_builtins(registry: &mut ClassRegistry) {
    let object = std::iter::once((OBJECT_CLASS, "Object", false));
    let always_on = BUILTINS
        .iter()
        .filter(|b| b.feature.is_none())
        .map(|b| (b.id, b.name, b.is_module));
    for (id, name, is_module) in object.chain(always_on) {
        registry.register(id, name, is_module, declared_ancestors(id), None);
    }
}

impl ClassRegistry {
    /// A fresh registry pre-populated with the whole CORE world -- the built-in
    /// classes/modules (`register_builtins`) and the built-in exception
    /// hierarchy (`register_exceptions`) -- at the ids `spinel-abi` reserves and
    /// the compiler asserts it assigned identically. Generated `main` layers its
    /// own user classes on top and reopens builtins as needed.
    pub fn with_core() -> Self {
        let mut registry = ClassRegistry::new();
        register_builtins(&mut registry);
        register_exceptions(&mut registry);
        registry
    }
}

/// Seed the CORE constants a generated program reads (`Float::INFINITY`,
/// `Encoding::UTF_8`, `Regexp::IGNORECASE`, `ARGV`, `STDOUT`/`$stdout`, `ENV`,
/// `Process::CLOCK_*`). Their owners resolve at compile time; only the values
/// need installing at startup, once the registry is in place. The order matches
/// the eight `seed_*` calls generated `main()` used to make itself.
pub fn install_core_constants() {
    crate::builtins::numeric::seed_numeric_constants();
    crate::builtins::encoding::seed_encoding_constants();
    crate::builtins::regexp::seed_regexp_constants();
    crate::constants::seed_argv();
    crate::builtins::io::seed_stdio();
    crate::builtins::io::seed_io_constants();
    crate::builtins::env::seed_env();
    crate::builtins::process::seed_process();
}
