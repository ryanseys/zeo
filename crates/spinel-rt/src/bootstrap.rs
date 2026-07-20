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
use crate::RubyValue;

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
    crate::builtins::file::seed_file();
    seed_ruby_constants();
}

/// Top-level `RUBY_*` version/build constants (owner `Object`, id 0) plus the
/// `File::SEPARATOR` family.
///
/// The version, release date, and revision are the pinned oracle's own release
/// identity (`ruby 4.0.5`) -- fixed for the version exactly as CRuby bakes them
/// from `version.h`/`revision.h`. `RUBY_PLATFORM` is derived from the *build
/// target* (`build.rs` -> `SPINEL_RUBY_PLATFORM`), and `RUBY_DESCRIPTION` is
/// *composed* from those parts the same way CRuby's `version.c` builds
/// `ruby_description`, rather than hardcoded.
fn seed_ruby_constants() {
    use crate::const_set;
    let object = 0;

    // `RUBY_VERSION` stays the pinned oracle's -- spinel targets its language
    // and library level. `RUBY_ENGINE`/`RUBY_ENGINE_VERSION` are spinel's OWN
    // identity: spinel is a distinct Ruby engine (like TruffleRuby/JRuby), and
    // `RUBY_ENGINE == "spinel"` is exactly what lets a gem branch to its
    // non-MRI code path. The engine version is this build's own crate version.
    const VERSION: &str = "4.0.5";
    const ENGINE: &str = "spinel";
    const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
    const RELEASE_DATE: &str = "2026-05-20";
    const REVISION: &str = "64336ffd0ee9e1f4c05891695a3d7b49cb709721";
    // Build-target-derived, like CRuby's configure-time `RUBY_PLATFORM`.
    const PLATFORM: &str = env!("SPINEL_RUBY_PLATFORM");

    const_set(object, "RUBY_VERSION", rb_str(VERSION));
    const_set(object, "RUBY_PATCHLEVEL", RubyValue::Int(0));
    const_set(object, "RUBY_ENGINE", rb_str(ENGINE));
    const_set(object, "RUBY_ENGINE_VERSION", rb_str(ENGINE_VERSION));
    const_set(object, "RUBY_PLATFORM", rb_str(PLATFORM));
    const_set(object, "RUBY_REVISION", rb_str(REVISION));
    const_set(object, "RUBY_RELEASE_DATE", rb_str(RELEASE_DATE));

    // TruffleRuby's banner shape -- `<engine> <engine_ver> (<date>) +PRISM
    // [<platform>] like ruby <ruby_ver>` -- so a tool that greps the engine
    // name still finds the MRI-compat level in the `like ruby` tail. `+PRISM`
    // is this build's default parser (the pinned oracle's).
    let description = format!(
        "{ENGINE} {ENGINE_VERSION} ({RELEASE_DATE}) +PRISM [{PLATFORM}] like ruby {VERSION}"
    );
    const_set(object, "RUBY_DESCRIPTION", rb_str(&description));

    let file = spinel_abi::FILE_CLASS.0;
    const_set(file, "SEPARATOR", rb_str("/"));
    const_set(file, "ALT_SEPARATOR", RubyValue::Nil);
    const_set(file, "PATH_SEPARATOR", rb_str(":"));
}

fn rb_str(s: &str) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s.to_string()))
}
