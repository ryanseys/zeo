//! Synthetic and vendored shims: stdlib features zeo synthesizes instead of
//! loading from disk (`rbconfig` and friends -- static stand-ins describing
//! the target zeo emulates), paired with the `parse/shims/*.rb` sources.

use std::path::Path;

/// The embedded source for a stdlib feature zeo synthesizes instead of loading
/// from disk. Real Ruby generates these at build time (`rbconfig`); zeo ships a
/// static stand-in describing the target it emulates. `None` for any other
/// feature.
pub(super) fn synthetic_shim_source(feature: &str) -> Option<std::borrow::Cow<'static, str>> {
    // `rbconfig` is the one shim whose text is not fully known at build time.
    // Its `bindir`/`ruby_install_name` name THE RUNNING ZEO BINARY and its
    // `prefix` is the resolved home's writable root -- both run-time facts the
    // build cannot see. Filled here, where `cext::zeo_binary` and
    // `home::ruby_prefix` can answer. See the template's own comments.
    if feature == "rbconfig" {
        let raw: &'static str = include_str!("../shims/rbconfig.rb.in");
        let zeo = crate::cext::zeo_binary().ok();
        let dir = zeo
            .as_deref()
            .and_then(Path::parent)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/usr/local/bin".to_string());
        // `file_name`, not `file_stem`: the shim's `EXEEXT` is empty, so the
        // name has to carry any extension itself.
        let name = zeo
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "ruby".to_string());
        let prefix = crate::home::ruby_prefix();
        // Where the headers live once fetched -- a path, not a fetch, so a
        // program that only reads `RbConfig::CONFIG` never pays for one.
        // `cext::configure` fetches before it runs an extconf.
        let headers = header_dirs_for_rbconfig();
        return Some(std::borrow::Cow::Owned(
            raw.replace("@ZEO_BINDIR@", &dir)
                .replace("@ZEO_RUBY_INSTALL_NAME@", &name)
                .replace("@ZEO_PREFIX@", &prefix.to_string_lossy())
                .replace("@CEXT_HDRDIR@", &headers.0)
                .replace("@CEXT_ARCHHDRDIR@", &headers.1),
        ));
    }
    // A program that loads mkmf is about to build an extension, in whatever
    // process -- rubygems runs `extconf.rb` as a child of its own -- so this
    // is where the headers are fetched if this machine has none. A failure
    // is reported here and again, with the path, by mkmf's first probe.
    #[cfg(feature = "capi")]
    if feature == "mkmf"
        && let Err(why) = crate::cext::headers::ensure()
    {
        eprintln!("zeo: {why}");
    }
    synthetic_shim_static(feature).map(std::borrow::Cow::Borrowed)
}

/// `(hdrdir, archhdrdir)` for the rbconfig shim. Without the C API there is
/// no tree to name, and an `extconf.rb` fails at its first `#include` with
/// the path that says why.
fn header_dirs_for_rbconfig() -> (String, String) {
    #[cfg(feature = "capi")]
    {
        let dirs = crate::cext::headers::expected();
        (
            dirs.include.to_string_lossy().into_owned(),
            dirs.config.to_string_lossy().into_owned(),
        )
    }
    #[cfg(not(feature = "capi"))]
    {
        let none = "/zeo-built-without-c-extension-support".to_string();
        (none.clone(), none)
    }
}

/// The shims whose text IS fully known at build time.
fn synthetic_shim_static(feature: &str) -> Option<&'static str> {
    match feature {
        // `lib/mkmf.rb`, vendored VERBATIM from the same `ruby/ruby` pin the
        // C API headers ride (`zeo-capi/ruby-headers.lock`). An `extconf.rb` runs
        // under zeo and writes a real Makefile, so mkmf is Ruby zeo RUNS
        // rather than a thing it reimplements -- 3,061 lines of probing and
        // Makefile generation that no summary of would stay true.
        // ...followed by `shims/mkmf_zeo.rb`, the delta zeo maintains, which
        // is kept as its own file so the vendored copy above stays
        // byte-identical to ruby's. See that file for what it changes and why.
        "mkmf" => Some(concat!(
            include_str!("../../../tools-lib/mkmf.rb"),
            "\n",
            include_str!("../shims/mkmf_zeo.rb")
        )),
        "gem-readline" => Some(include_str!("../shims/gem_readline.rb")),
        _ => None,
    }
}

/// A vendored or installed file whose load-time probing has exactly one
/// possible outcome under zeo, mapped to the synthetic shim that states that
/// outcome (see `synthetic_shim_source`). Matched by path suffix so every
/// copy of the file redirects to the same shim.
pub(super) fn vendored_shim_feature(canonical: &Path) -> Option<&'static str> {
    let path = canonical.to_string_lossy().replace('\\', "/");
    // The readline gem's entry file, from any installed store copy: its
    // load-time probe for the C readline extension has one possible outcome
    // under zeo (see `shims/gem_readline.rb`).
    (path.contains("/gems/readline-") && path.ends_with("/lib/readline.rb"))
        .then_some("gem-readline")
}
