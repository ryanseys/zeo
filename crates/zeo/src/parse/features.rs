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
pub(super) fn canonical_ext_feature(feature: &str) -> &str {
    match feature {
        "cgi" | "cgi/util" | "cgi/escape" => "cgi/escape",
        "yaml" => "psych",
        f if f == "digest" || f.starts_with("digest/") => "digest",
        other => other,
    }
}

/// Features CRuby has ALREADY loaded before the program's first line, so
/// `require`ing one answers `false` even the first time. Verified by running
/// `p require "<f>"` under ruby 4.0.5 for every feature `is_builtin_feature`
/// accepts; only these two came back false.
///
/// Deliberately NOT folded into `Hir::activated_features`: that set also
/// gates CONSTANT visibility, so pre-seeding `monitor` there would make
/// `Monitor` resolve without its require.
pub(super) fn is_preloaded_at_boot(feature: &str) -> bool {
    matches!(feature, "set" | "monitor")
}

/// Whether `feature` names a stdlib feature the runtime compiles in, so
/// `require`ing it is a no-op (nothing to splice). `tmpdir` (`Dir.mktmpdir`)
/// and `set` (the `Set` core class) are both compiled in -- `Set` is now an
/// autoloaded core class in real Ruby, so `require "set"` is a no-op there too.
/// In-tree `ext/` modules (`base64`, ...) are recognized straight from the
/// ABI table (`zeo_abi::is_ext_feature`) so the loader and the constant
/// resolver never drift; `require`ing one both short-circuits the filesystem
/// search AND activates its gated constant (see `lower_require_statement`).
pub(super) fn is_builtin_feature(feature: &str) -> bool {
    // `time` names no gated class: `Time` is an always-on builtin here, so
    // the require is a pure no-op. CRuby's real gate is finer -- `Time` is
    // core but `Time#iso8601` only exists after `require "time"` -- and
    // zeo has no per-method activation precedent to hang that on, so the
    // extra methods are unconditionally present. A program that calls
    // `#iso8601` WITHOUT the require works here and raises in CRuby.
    // `io/console` names no gated constant either -- `IO` is core and its
    // `#winsize` is an unconditional row on the IO table, so the require is
    // pure ceremony. Same shape of divergence as `time` above.
    // `ffi` is now an `is_ext_feature` (the `FFI` module + `Pointer`/
    // `MemoryPointer`/`Struct` rows carry `feature: Some("ffi")`), so
    // `require "ffi"` activates that feature and those constants resolve. The
    // compile-time frontend (`extend FFI::Library` / `attach_function`, the
    // `lower_class_body` FFI pre-scan) is orthogonal -- it emits `extern "C"` +
    // `#[link]` inline and never needs a constant. See [[ffi-real-gem-api]].
    matches!(feature, "tmpdir" | "set" | "time" | "io/console")
        || zeo_abi::is_ext_feature(canonical_ext_feature(feature))
}
