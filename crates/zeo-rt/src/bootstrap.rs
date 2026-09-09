//! The runtime's startup surface: assembling the class registry every generated
//! program installs, and seeding the core constants.
//!
//! A generated `main` does just two things here: build a registry pre-populated
//! with the CORE classes (`ClassRegistry::with_core`), then -- after layering
//! its own user classes on top and installing it -- seed the CORE constants
//! (`install_core_constants`). The alternative was ~540 identical `register(...)`
//! calls plus eight `seed_*` calls emitted into every program.

use zeo_abi::{BUILTINS, ClassId, OBJECT_CLASS, declared_ancestors};

use crate::builtins::exception::register_exceptions;
use crate::dispatch::{ClassRegistry, ConstructorFn};
use crate::{RubyValue, Signal, Symbol};

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
        let ctor = (id == OBJECT_CLASS).then_some(object_construct as ConstructorFn);
        registry.register(id, name, is_module, declared_ancestors(id), ctor);
    }
    undefine_builtin_methods(registry);
}

/// The builtin classes CRuby defines with `rb_struct_define`. They are real
/// `Struct` subclasses there, so the whole struct protocol -- `to_a`, `to_h`,
/// `==`, `each`, `[]`, `dig`, `size`, `deconstruct`, `Marshal` -- is
/// inherited rather than written per class. All each one has to supply is a
/// member list here and `hidden_ivar_get`/`_set` on its payload, which is
/// what the protocol reads a member by.
fn register_native_structs() {
    let mut rows: Vec<(zeo_abi::ClassId, &[&str])> = vec![(
        zeo_abi::PROCESS_TMS_CLASS,
        crate::builtins::process::TMS_MEMBERS.as_slice(),
    )];
    #[cfg(feature = "ext-etc")]
    rows.extend([
        (zeo_abi::ETC_PASSWD_CLASS, crate::ext::etc::PASSWD_MEMBERS),
        (zeo_abi::ETC_GROUP_CLASS, crate::ext::etc::GROUP_MEMBERS),
    ]);
    for (id, members) in rows {
        crate::register_compiled_struct(id, members, false, None);
    }
}

/// `Object.new` -- the bare sentinel instance of the runtime root. Both
/// backends FOLD a literal `Object.new` (no ivar layout exists to allocate
/// from: `Object`'s container holds top-level defs as free functions), but
/// a class reached only at run time (`k = Object; k.new`) dispatches here.
/// A user-defined `initialize` runs through the ordinary send.
fn object_construct(
    _id: ClassId,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let obj = crate::Object::new_value();
    if crate::dispatch::responds_to_value(&obj, Symbol::intern("initialize"), true) {
        crate::dispatch::send_value(&obj, Symbol::intern("initialize"), args, blk)?;
    } else if !args.is_empty() {
        // CRuby's zero-arity `Object#initialize` rejects them.
        return Err(crate::dispatch::wrong_arity(args.len(), "0"));
    }
    Ok(obj)
}

/// The built-in `undef`s: a name an ancestor defines that the class REFUSES,
/// so the call raises NoMethodError rather than inheriting an answer that
/// makes no sense.
///
/// `Complex` is the only core class that does this, and it is why
/// `Complex(1, 2).positive?` is a NoMethodError while `Rational(1, 2).positive?`
/// is false: an ordering on the complex plane does not exist, so every method
/// `Numeric` and `Comparable` build out of `<=>` has to go, and with them the
/// rounding family, which would need one. Without this the generic `Numeric`
/// rows leak straight through the ancestor walk.
fn undefine_builtin_methods(registry: &mut ClassRegistry) {
    const COMPLEX_UNDEF: &[&str] = &[
        // Ordering, and everything Comparable derives from it.
        "<",
        "<=",
        ">",
        ">=",
        "between?",
        "clamp",
        "negative?",
        "positive?", //
        // Division with a remainder, which needs an ordering to floor.
        "%",
        "div",
        "divmod",
        "modulo",
        "remainder",
        "step", //
        // Rounding, likewise.
        "ceil",
        "floor",
        "round",
        "truncate", //
        // `i` would build a Complex out of a Complex.
        "i",
    ];
    for name in COMPLEX_UNDEF {
        registry.mark_undefined(zeo_abi::COMPLEX_CLASS, crate::Symbol::intern(name));
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
        crate::builtins::pathname::register_pathname(&mut registry);
        crate::builtins::converter::register_converter(&mut registry);
        crate::builtins::random::register_random(&mut registry);
        crate::builtins::stat::register_stat(&mut registry);
        crate::builtins::io_buffer::register_io_buffer(&mut registry);
        crate::builtins::ruby_box::register_ruby_box(&mut registry);
        register_native_structs();
        registry
    }
}

/// Bind `DATA` to the bytes after the script's `__END__` marker: an open
/// `File` on the source, seeked past the marker, exactly as ruby leaves it.
/// Emitted into startup only for a program that HAS an `__END__`.
///
/// A real handle on the source, not the bytes copied into the binary, because
/// that is what `DATA` is: `rewind` seeks to offset 0 of the file and reads the
/// program's own text back. The cost is that the source must still be readable
/// at run time -- unlike everything else in an AOT binary. When it is not,
/// `DATA` stays undefined, so the program gets the same rescuable `NameError`
/// a program with no `__END__` gets rather than dying at startup.
pub fn install_data_section(path: &str, offset: u64) {
    use std::io::Seek;
    let Ok(mut f) = std::fs::File::open(path) else {
        return;
    };
    if f.seek(std::io::SeekFrom::Start(offset)).is_err() {
        return;
    }
    crate::constants::const_set(
        zeo_abi::OBJECT_CLASS.0,
        "DATA",
        crate::builtins::io::file_value(f, Some(path.to_string())),
    );
}

/// Seed the CORE constants a generated program reads (`Float::INFINITY`,
/// `Encoding::UTF_8`, `Regexp::IGNORECASE`, `ARGV`, `STDOUT`/`$stdout`, `ENV`,
/// `Process::CLOCK_*`). Their owners resolve at compile time; only the values
/// need installing at startup, once the registry is in place.
pub fn install_core_constants() {
    // `Float::*`, `Math::PI`/`E`, and `Complex::I` seed via their classes'
    // ruby_class! `const` rows (the BUILTIN_TABLES loop below).
    // `BasicObject::BasicObject` -- ruby defines the class as a constant on
    // ITSELF, so `BasicObject.constants(false)` is `[:BasicObject]` where
    // every other class's is empty. Nothing else names it that way.
    crate::constants::const_set(
        zeo_abi::BASIC_OBJECT_CLASS.0,
        "BasicObject",
        crate::RubyValue::Class(zeo_abi::BASIC_OBJECT_CLASS),
    );
    crate::constants::seed_argv();
    crate::builtins::env::seed_env();
    // `File`'s constants seed via its ruby_class! `const` rows (installed
    // by the BUILTIN_TABLES loop below).
    #[cfg(feature = "ext-etc")]
    crate::ext::etc::seed_etc();
    #[cfg(feature = "ext-socket")]
    crate::ext::socket::seed_socket();
    crate::builtins::argf::seed_argf();
    crate::builtins::exception::seed_errno_constants();
    crate::globals::seed_default_globals();
    // The main ractor exists before any program statement runs, so
    // `Ractor.count`/`.current`/`#inspect` never observe a world without it.
    crate::ractor::init_main_ractor();
    // `ThreadGroup::Default` seeds itself via thread_group's ruby_class!
    // `const Default` row (installed by the BUILTIN_TABLES loop below).
    seed_ruby_constants();
    // A `ruby_class!`/`ruby_module!` class seeds its own constants via its
    // `install_constants` thunk, collected in BUILTIN_TABLES. Independent
    // per class, so ordering after the hand-written seeders above is fine.
    for table in crate::builtins::all_tables() {
        if let Some(install) = table.install_constants {
            install();
        }
    }
    // Everything above is the MASTER namespace; everything after this call
    // belongs to the main program, and a `Ruby::Box` must not see it.
    crate::constants::seal_master_constants();
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

    // CRuby sets this at the VM level (`ruby.c`), not in any library, so it
    // is ALWAYS defined and `nil` for an ordinary build. `mkmf` reads it at
    // module-body level -- `if !CROSS_COMPILING` decides which `mkintpath` to
    // define -- so an undefined one is a NameError that stops mkmf loading,
    // and therefore stops every C extension from building.
    const_set(object, "CROSS_COMPILING", RubyValue::Nil);

    // CRuby copyright line, verbatim from `version.c`.
    const COPYRIGHT: &str = "ruby - Copyright (C) 1993-2026 Yukihiro Matsumoto";
    const_set(object, "RUBY_COPYRIGHT", rb_str(COPYRIGHT));

    // The `Ruby` module carries the SAME identity under the modern
    // spellings (`Ruby::VERSION == RUBY_VERSION`, one source for both).
    let ruby_ns = zeo_abi::RUBY_MODULE.0;
    const_set(ruby_ns, "VERSION", rb_str(VERSION));
    const_set(ruby_ns, "PATCHLEVEL", RubyValue::Int(0));
    const_set(ruby_ns, "ENGINE", rb_str(ENGINE));
    const_set(ruby_ns, "ENGINE_VERSION", rb_str(ENGINE_VERSION));
    const_set(ruby_ns, "PLATFORM", rb_str(PLATFORM));
    const_set(ruby_ns, "REVISION", rb_str(REVISION));
    const_set(ruby_ns, "RELEASE_DATE", rb_str(RELEASE_DATE));
    const_set(ruby_ns, "DESCRIPTION", rb_str(&description));
    const_set(ruby_ns, "COPYRIGHT", rb_str(COPYRIGHT));

    // `Mutex`, `Queue`, `SizedQueue` and `ConditionVariable` are TOP-LEVEL
    // spellings of the `Thread::*` classes -- the same class under two
    // constants, which is why `Queue.equal?(Thread::Queue)` holds.
    for (top, nested) in [
        ("Mutex", zeo_abi::MUTEX_CLASS),
        ("Queue", zeo_abi::QUEUE_CLASS),
        ("SizedQueue", zeo_abi::SIZED_QUEUE_CLASS),
        ("ConditionVariable", zeo_abi::CONDITION_VARIABLE_CLASS),
    ] {
        const_set(object, top, RubyValue::Class(nested));
    }

    let file = zeo_abi::FILE_CLASS.0;
    const_set(file, "SEPARATOR", rb_str("/"));
    // CRuby's own second spelling of `SEPARATOR`.
    const_set(file, "Separator", rb_str("/"));
    const_set(file, "ALT_SEPARATOR", RubyValue::Nil);
    const_set(file, "PATH_SEPARATOR", rb_str(":"));
}

fn rb_str(s: &str) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s.to_string()))
}

#[cfg(test)]
mod tests {
    /// Every `Object` constant this file seeds must be in
    /// `zeo_abi::SEEDED_OBJECT_CONSTANTS`, or the compiler folds
    /// `defined?(NAME)` to nil for a name the running program has
    /// (`CROSS_COMPILING` is the kind of name that slips). The compiler cannot see
    /// this crate, so the agreement is pinned here, over the source text
    /// the seeding actually is.
    #[test]
    fn every_seeded_object_constant_is_in_the_abi_list() {
        let src = include_str!("bootstrap.rs");
        let mut missing = Vec::new();
        for line in src.lines() {
            let Some(rest) = line.trim().strip_prefix("const_set(object, \"") else {
                continue;
            };
            let Some(name) = rest.split('"').next() else {
                continue;
            };
            if !zeo_abi::SEEDED_OBJECT_CONSTANTS.contains(&name) {
                missing.push(name.to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "seeded on Object but absent from zeo_abi::SEEDED_OBJECT_CONSTANTS \
             (the compiler will fold defined?(..) to nil for these): {missing:?}"
        );
    }
}
