//! The runtime's startup surface: assembling the class registry every generated
//! program installs, and seeding the core constants.
//!
//! A generated `main` does just two things here: build a registry pre-populated
//! with the CORE classes (`ClassRegistry::with_core`), then -- after layering
//! its own user classes on top and installing it -- seed the CORE constants
//! (`install_core_constants`). The alternative was ~540 identical `register(...)`
//! calls plus eight `seed_*` calls emitted into every program.

use zeo_abi::{BUILTINS, OBJECT_CLASS, declared_ancestors};

use crate::RubyValue;
use crate::builtins::exception::register_exceptions;
use crate::dispatch::ClassRegistry;

/// Install the always-on built-in classes/modules (`Integer`, `Array`,
/// `Kernel`, ... and `Object`) into `registry` with their DECLARED ancestors --
/// the fixed hierarchy every program shares.
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
    /// hierarchy (`register_exceptions`) -- at the ids `zeo-abi` reserves and
    /// the compiler asserts it assigned identically. Generated `main` layers its
    /// own user classes on top and reopens builtins as needed.
    pub fn with_core() -> Self {
        let mut registry = ClassRegistry::new();
        register_builtins(&mut registry);
        register_exceptions(&mut registry);
        crate::builtins::weak::register_weak(&mut registry);
        registry
    }
}

/// Seed the CORE constants a generated program reads (`Float::INFINITY`,
/// `Encoding::UTF_8`, `Regexp::IGNORECASE`, `ARGV`, `STDOUT`/`$stdout`, `ENV`,
/// `Process::CLOCK_*`). Their owners resolve at compile time; only the values
/// need installing at startup, once the registry is in place.
pub fn install_core_constants() {
    // `Float::*`, `Math::PI`/`E`, and `Complex::I` seed via their classes'
    // ruby_class! `const` rows (the BUILTIN_TABLES loop below).
    crate::builtins::encoding::seed_encoding_constants();
    crate::builtins::regexp::seed_regexp_constants();
    crate::constants::seed_argv();
    crate::builtins::io::seed_stdio();
    crate::builtins::io::seed_io_constants();
    crate::builtins::env::seed_env();
    // `File`'s constants now seed via its ruby_class! `const` rows (installed
    // by the BUILTIN_TABLES loop below).
    #[cfg(feature = "ext-etc")]
    crate::ext::etc::seed_etc();
    #[cfg(feature = "ext-socket")]
    crate::ext::socket::seed_socket();
    crate::builtins::argf::seed_argf();
    crate::globals::seed_default_globals();
    // `ThreadGroup::Default` now seeds itself via thread_group's ruby_class!
    // `const Default` row (installed by the BUILTIN_TABLES loop below).
    seed_ruby_constants();
    // Macro-migrated classes seed their own constants via the ruby_class!/
    // ruby_module! `install_constants` thunk, collected in BUILTIN_TABLES --
    // the data-driven replacement for the per-class `seed_*` calls above, run
    // as each class moves off them. Independent per class, so ordering after
    // the legacy seeders is fine.
    for table in crate::builtins::BUILTIN_TABLES {
        if let Some(install) = table.install_constants {
            install();
        }
    }
}

/// Top-level `RUBY_*` version/build constants (owner `Object`, id 0) plus the
/// `File::SEPARATOR` family.
///
/// The version, release date, and revision are the pinned oracle's own release
/// identity (`ruby 4.0.6`) -- fixed for the version exactly as CRuby bakes them
/// from `version.h`/`revision.h`. `RUBY_PLATFORM` is derived from the *build
/// target* (`build.rs` -> `ZEO_RUBY_PLATFORM`), and `RUBY_DESCRIPTION` is
/// *composed* from those parts the same way CRuby's `version.c` builds
/// `ruby_description`, rather than hardcoded.
fn seed_ruby_constants() {
    use crate::const_set;
    let object = 0;

    // `RUBY_VERSION` stays the pinned oracle's -- zeo targets its language
    // and library level. `RUBY_ENGINE`/`RUBY_ENGINE_VERSION` mirror CRuby's
    // exactly (engine `"ruby"`, engine version == `RUBY_VERSION`): the north
    // star is byte-for-byte MRI parity, so zeo reports the MRI identity
    // rather than a distinct engine name (a gem's `RUBY_ENGINE`-branch takes
    // its CRuby path, which is what zeo implements).
    const VERSION: &str = zeo_abi::RUBY_VERSION;
    const ENGINE: &str = "ruby";
    const ENGINE_VERSION: &str = VERSION;
    const RELEASE_DATE: &str = "2026-07-14";
    const REVISION: &str = "03b6d3f8898a28604fe6cb00eae3226b821168f4";
    // Build-target-derived, like CRuby's configure-time `RUBY_PLATFORM`.
    const PLATFORM: &str = env!("ZEO_RUBY_PLATFORM");

    const_set(object, "RUBY_VERSION", rb_str(VERSION));
    const_set(object, "RUBY_PATCHLEVEL", RubyValue::Int(0));
    const_set(object, "RUBY_ENGINE", rb_str(ENGINE));
    const_set(object, "RUBY_ENGINE_VERSION", rb_str(ENGINE_VERSION));
    const_set(object, "RUBY_PLATFORM", rb_str(PLATFORM));
    const_set(object, "RUBY_REVISION", rb_str(REVISION));
    const_set(object, "RUBY_RELEASE_DATE", rb_str(RELEASE_DATE));

    // CRuby's own banner shape (version.c): `ruby <ver> (<date> revision
    // <short-rev>) +PRISM [<platform>]`, where `<short-rev>` is the leading 10
    // chars of the git revision. `+PRISM` is this build's default parser (the
    // pinned oracle's).
    let short_rev = &REVISION[..10];
    let description =
        format!("ruby {VERSION} ({RELEASE_DATE} revision {short_rev}) +PRISM [{PLATFORM}]");
    const_set(object, "RUBY_DESCRIPTION", rb_str(&description));

    let file = zeo_abi::FILE_CLASS.0;
    const_set(file, "SEPARATOR", rb_str("/"));
    const_set(file, "ALT_SEPARATOR", RubyValue::Nil);
    const_set(file, "PATH_SEPARATOR", rb_str(":"));
}

fn rb_str(s: &str) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s.to_string()))
}
