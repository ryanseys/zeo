//! Which `require` features the compiled runtime provides NATIVELY -- pure
//! predicates over static tables (and the ABI's `ext/` feature list), shared
//! by the loader (which short-circuits the filesystem search for them) and
//! `lower_node`'s require-fold (a `require` of a native feature is legal in
//! ANY position, because its whole effect is compile-time activation).

/// Map a `require` spelling to its canonical in-tree `ext/` feature name (the
/// ABI `feature` string). Sub-path and alias spellings of one extension
/// collapse to a single feature: `cgi`/`cgi/util` -> `cgi/escape`,
/// `digest/sha2` -> `digest`, and `yaml` -> `psych` (Ruby's `yaml.rb` is just
/// `YAML = Psych`). Everything else maps to itself.
pub fn canonical_ext_feature(feature: &str) -> &str {
    zeo_abi::canonical_ext_feature(feature)
}

/// Features CRuby has ALREADY loaded before the program's first line, so
/// `require`ing one answers `false` even the first time. Verified by running
/// `p require "<f>"` under ruby 4.0.6 for every feature `is_builtin_feature`
/// accepts. `rational`, `complex` and `thread` are the names CRuby folded
/// into core and keeps only so old code still loads.
///
/// Deliberately NOT folded into `Hir::activated_features`: that set also
/// gates CONSTANT visibility, so pre-seeding `monitor` there would make
/// `Monitor` resolve without its require.
pub fn is_preloaded_at_boot(feature: &str) -> bool {
    PRELOADED_AT_BOOT.contains(&feature)
}

/// [`is_preloaded_at_boot`]'s list, as data: the `$LOADED_FEATURES` seed
/// needs to ENUMERATE it, not just ask about one name.
pub const PRELOADED_AT_BOOT: &[&str] = zeo_abi::PRELOADED_AT_BOOT;

/// Whether zeo's OWN implementation answers `require "feature"` -- which is
/// [`is_builtin_feature`] unless `ZEO_DISABLE_BUILTIN` retired it.
///
/// Every site that chooses between zeo's implementation and a file asks this
/// rather than `is_builtin_feature` directly, so the dial reaches all of
/// them: the store resolver, the loader's three resolution sites, and the
/// fold in `lower::calls` that replaces the require with an activation.
/// `is_builtin_feature` stays the question "does zeo HAVE a row for this",
/// which the gated-constant resolver still needs whatever the dial says.
pub fn zeo_provides(feature: &str) -> bool {
    is_builtin_feature(feature)
        && !crate::debug_flags::builtin_disabled(feature)
        && build_carries_ext(feature)
}

/// Whether THIS build compiled the native half behind a gated ext feature --
/// the dual-build switch. The answer lives in zeo-rt, where the cargo
/// features actually are; the compiler links that same build, so its loader
/// and the runtime's `Kernel#require` agree by construction. A feature-off
/// extension's `require` must resolve to a file (the `pure/` tree, or a
/// runtime LoadError) rather than activate a class the runtime does not
/// carry.
pub fn build_carries_ext(feature: &str) -> bool {
    zeo_rt::features::build_carries_ext(feature)
}

/// [`build_carries_ext`] by class id, for the emitted table-symbol list: a
/// feature-off extension's `zeo_ctable_*` symbol is absent from `libzeo.a`,
/// so an emitted program must not reference it.
pub fn build_carries_class(id: zeo_abi::ClassId) -> bool {
    match zeo_abi::feature_of_gated_class(id) {
        Some(feature) => build_carries_ext(feature),
        None => true,
    }
}

/// Whether `feature` names a stdlib feature the runtime compiles in, so
/// `require`ing it is a no-op (nothing to splice). `tmpdir` (`Dir.mktmpdir`)
/// and `set` (the `Set` core class) are both compiled in -- `Set` is an
/// autoloaded core class in real Ruby, so `require "set"` is a no-op there too.
/// In-tree `ext/` modules (`base64`, ...) are recognized straight from the
/// ABI table (`zeo_abi::is_ext_feature`) so the loader and the constant
/// resolver never drift; `require`ing one both short-circuits the filesystem
/// search AND activates its gated constant (see `lower_require_statement`).
pub fn is_builtin_feature(feature: &str) -> bool {
    // `time` names no gated class: `Time` is an always-on builtin here, so
    // the require is a pure no-op. CRuby's real gate is finer -- `Time` is
    // core but `Time#iso8601` only exists after `require "time"` -- and
    // zeo has no per-method activation precedent to hang that on, so the
    // extra methods are unconditionally present. A program that calls
    // `#iso8601` WITHOUT the require works here and raises in CRuby.
    // `io/console` names no gated constant either -- `IO` is core and its
    // `#winsize` is an unconditional row on the IO table, so the require is
    // pure ceremony. Same shape of divergence as `time` above. `io/wait` is
    // identical: `IO#wait_readable`/`#wait_writable` are unconditional rows on
    // the IO table (real `poll(2)`), so its require is pure ceremony too.
    // `ffi` is an `is_ext_feature` (the `FFI` module + `Pointer`/
    // `MemoryPointer`/`Struct` rows carry `feature: Some("ffi")`), so
    // `require "ffi"` activates that feature and those constants resolve. The
    // compile-time frontend (`extend FFI::Library` / `attach_function`, the
    // `lower_class_body` FFI pre-scan) is orthogonal -- it emits `extern "C"` +
    // `#[link]` inline and never needs a constant. See [[ffi-real-gem-api]].
    // `objspace` (the whole `ObjectSpace` module, both
    // the always-on half CRuby defines in gc.c and the introspection half it
    // gates behind this require) and `fiber` (Fiber) name always-on builtins
    // here, so those requires are pure no-ops -- their classes resolve
    // unconditionally. CRuby answers `false` for `require "fiber"` too, Fiber
    // being core there as well. `thread` is the same story one step further
    // on: CRuby folded it into core long ago and keeps the name only so old
    // code still loads, answering `false` for the require -- which is what
    // zeo does too (minitest/parallel.rb opens with it).
    // `pathname` is the same shape once more: ruby 4.0 loads `pathname.so`
    // before the first line, so `Pathname` and 96 of its methods are there
    // whatever the program does, and the require only reopens the class to
    // add `#find` and `#rmtree`. zeo carries those two from the start.
    // `random/formatter` is the `time` shape again: ruby 4.0 keeps
    // `Random::Formatter` in CORE with `#rand`/`#random_number`, and this
    // require only REOPENS it to add the hex/uuid/base64 family. zeo gates
    // whole classes rather than methods, so it carries all of them from the
    // start and the require is ceremony -- it answers where ruby would raise
    // NoMethodError, never the reverse.
    zeo_abi::is_builtin_feature(feature)
}

/// Every feature `is_builtin_feature` answers true for that also NAMES a
/// file -- the candidates for being DUAL-HOMED.
///
/// The predicate above answers a question, not a list, and the loader needs
/// the list: it probes each name once at startup and records the ones that
/// really resolve, because only those need their `require` call kept.
pub fn builtin_feature_names() -> impl Iterator<Item = &'static str> {
    zeo_abi::NATIVE_FEATURES
        .iter()
        .copied()
        .chain(zeo_abi::ext_feature_names())
}
