//! Synthetic and vendored shims: stdlib features zeo synthesizes instead of
//! loading from disk (`rbconfig` and friends -- static stand-ins describing
//! the target zeo emulates), paired with the `parse/shims/*.rb` sources.

use std::path::Path;

/// The embedded source for a stdlib feature zeo synthesizes instead of loading
/// from disk. Real Ruby generates these at build time (`rbconfig`); zeo ships a
/// static stand-in describing the target it emulates. `None` for any other
/// feature.
pub(super) fn synthetic_shim_source(feature: &str) -> Option<&'static str> {
    match feature {
        // Rendered by build.rs from shims/rbconfig.rb.in with the build
        // target's platform facts (arch, darwin major, dlext, ...).
        "rbconfig" => Some(include_str!(concat!(env!("OUT_DIR"), "/rbconfig.rb"))),
        // `lib/mkmf.rb`, vendored VERBATIM from the same `ruby/ruby` pin the
        // C API headers ride (`crates/zeo-rt/cext/`). An `extconf.rb` runs
        // under zeo and writes a real Makefile, so mkmf is Ruby zeo RUNS
        // rather than a thing it reimplements -- 3,061 lines of probing and
        // Makefile generation that no summary of would stay true.
        "mkmf" => Some(include_str!("../../../tools-lib/mkmf.rb")),
        "securerandom" => Some(include_str!("../shims/securerandom.rb")),
        "gem-securerandom" => Some(include_str!("../shims/gem_securerandom.rb")),
        // CRuby's C `erb/escape` extension -- defined as a pure-Ruby shim over
        // the native `CGI.escapeHTML` (see `shims/erb_escape.rb`).
        "erb/escape" => Some(include_str!("../shims/erb_escape.rb")),
        _ => None,
    }
}

/// A rubygems-/bundler-vendored file that duplicates a library zeo already
/// provides natively, mapped to the synthetic shim that stands in for it (see
/// `synthetic_shim_source`). Currently just the vendored `securerandom` copy,
/// whose load-time `class << self` entropy probe zeo can't lower; matched by
/// path suffix so both the rubygems and bundler copies (identical files under
/// different `vendor/` roots) redirect to the same native-backed shim.
pub(super) fn vendored_shim_feature(canonical: &Path) -> Option<&'static str> {
    let path = canonical.to_string_lossy().replace('\\', "/");
    path.ends_with("vendor/securerandom/lib/securerandom.rb")
        .then_some("gem-securerandom")
}
