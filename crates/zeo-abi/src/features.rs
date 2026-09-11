//! Which features this build answers `require` for, and why a declined
//! one is declined.

use super::*;

/// Whether `name` is an in-tree `ext/` feature whose `require` activates a
/// gated builtin (`"base64"` -> `Base64`). The ABI table is the single
/// source of truth for the feature -> class mapping, so the compiler's
/// require loader and its constant resolver stay in lockstep automatically
/// as extensions are added. Distinct from always-on core no-op requires
/// (`"set"`, `"tmpdir"`), which name no gated class and are handled
/// separately by the loader.
pub fn is_ext_feature(name: &str) -> bool {
    BUILTINS.iter().any(|b| b.feature == Some(name))
}

/// The require gate on a class, if it has one: the `feature` of `id`'s
/// BUILTINS row. The dual-build switch asks it to tell a feature-gated
/// extension class (absent from a build whose cargo feature is off) from an
/// always-on one.
pub fn feature_of_gated_class(id: ClassId) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|b| b.id.0 == id.0)
        .and_then(|b| b.feature)
}

/// The stdlib features the runtime compiles in that name no GATED class, so
/// requiring one is a pure no-op. `zeo::lower::features::is_builtin_feature`
/// carries the per-name reasoning; this is the list itself.
///
/// It lives here rather than in the compiler because the RUNTIME needs it
/// too: a `require` the compiler could not fold -- `f = "date"; require f` --
/// reaches `Kernel#require` at run time, and answering it needs the same
/// list. Two copies would drift, and the drift reads as a `LoadError` for a
/// library the binary is carrying.
pub const NATIVE_FEATURES: &[&str] = &[
    "tmpdir",
    "set",
    "time",
    // A file of its own in io-console, and ruby does NOT define
    // `IO.console_size` without it -- so it carries its own gate key rather
    // than riding on `io/console`'s.
    "io/console/size",
    "io/wait",
    "io/nonblock",
    "objspace",
    "fiber",
    "thread",
    "rational",
    "complex",
    "random/formatter",
    "pathname",
    // Synthesized by the compiler (real Ruby generates rbconfig.rb at build
    // time) and spliced into every program ahead of line 1, so the runtime
    // always carries `RbConfig` and a require loads nothing.
    "rbconfig",
];

/// Features CRuby has ALREADY loaded before the program's first line, so
/// requiring one answers `false` even the first time. Verified by running
/// `p require "<f>"` under ruby 4.0.6 for every feature
/// [`is_builtin_feature`] accepts.
/// `rbconfig` is on the list because rubygems requires it before the program
/// starts, so its answer is `false` there too.
pub const PRELOADED_AT_BOOT: &[&str] = &[
    "set", "monitor", "rational", "complex", "thread", "rbconfig",
];

/// Features ruby folded into CORE, keeping the name only so old code still
/// loads: it has NO FILE for them anywhere, so
/// `$LOAD_PATH.resolve_feature_path` answers nil rather than naming one.
///
/// NOT the same set as [`PRELOADED_AT_BOOT`], which is why it is written out.
/// MEASURED against ruby 4.0.6 over every [`NATIVE_FEATURES`] name: `monitor`
/// is preloaded and still HAS `monitor.rb`, and `fiber` is not preloaded and
/// has no file. Every other native feature names a real file there.
/// `rbconfig` sits here for zeo's own reason: ruby generates rbconfig.rb at
/// build time, but zeo synthesizes `RbConfig` in the compiler and ships no
/// file, so `nil` is the honest answer.
pub const CORE_WITH_NO_FILE: &[&str] =
    &["set", "fiber", "thread", "rational", "complex", "rbconfig"];

/// A `require` spelling mapped to its canonical in-tree `ext/` feature name.
/// Sub-path and alias spellings of one extension collapse to a single
/// feature: `cgi`/`cgi/util` -> `cgi/escape`, `digest/sha2` -> `digest`, and
/// `yaml` -> `psych` (Ruby's `yaml.rb` is just `YAML = Psych`).
#[must_use]
pub fn canonical_ext_feature(feature: &str) -> &str {
    match feature {
        "cgi" | "cgi/util" | "cgi/escape" => "cgi/escape",
        "yaml" => "psych",
        // Only the algorithm spellings ruby itself ships. A blanket
        // `digest/*` arm made `require "digest/nope"` answer false, which
        // starved `Digest.const_missing` of the LoadError it re-raises --
        // and swallowed `digest/version`, a real file in the gem's lib tree.
        "digest" | "digest/md5" | "digest/sha1" | "digest/sha2" | "digest/bubblebabble" => "digest",
        other => other,
    }
}

/// Whether `feature` names a stdlib feature the runtime compiles in, so
/// requiring it loads no file. Either a gated `ext/` module from the table
/// above, or one of [`NATIVE_FEATURES`].
#[must_use]
pub fn is_builtin_feature(feature: &str) -> bool {
    NATIVE_FEATURES.contains(&feature) || is_ext_feature(canonical_ext_feature(feature))
}

/// Every distinct extension feature name, for a caller that needs the LIST
/// rather than the predicate -- the loader probes each once to find which
/// are also backed by a vendored gem.
pub fn ext_feature_names() -> impl Iterator<Item = &'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for b in BUILTINS {
        if let Some(f) = b.feature
            && !seen.contains(&f)
        {
            seen.push(f);
        }
    }
    seen.into_iter()
}

/// Why a stdlib feature zeo DECLINES is missing -- appended to its
/// `LoadError` so a caller can tell a settled decision from a typo or an
/// unfinished feature. `None` for everything else, which keeps CRuby's bare
/// `cannot load such file -- <name>`.
///
/// Lives here because BOTH sides raise it and they never link each other: the
/// compiler's loader when it rejects the require outright, and the runtime's
/// `Kernel#require` when an unresolvable one was deferred. One source, so the
/// two wordings cannot drift.
///
/// See "Declined (a CRuby internal, not a missing binding)" in
/// `docs/reference/compatibility.md`.
pub fn declined_feature_reason(name: &str) -> Option<&'static str> {
    match name {
        "continuation" => Some(
            "declined. callcc captures and restores the machine stack, and a \
             native-compiled program has no stack-copying runtime; an escape-only \
             callcc would silently break re-entering callers. Use Fiber instead. \
             See docs/reference/compatibility.md.",
        ),
        _ => None,
    }
}
