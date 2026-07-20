//! The external gem store provider: turn a `Gemfile.lock` + an installed
//! RubyGems store (`gem env gemdir`) into extra require-path roots for the
//! gems spinel CAN compile, plus a disclosure entry for every gem it can't.
//!
//! spinel consumes Bundler's/RubyGems' NORMALIZED output; it never resolves,
//! fetches, or builds extensions. The one policy choice, TruffleRuby's
//! `force_ruby_platform`: a precompiled platform gem ships a `.bundle` spinel
//! can never load, so the SOURCE (ruby-platform) gemspec is required -- a
//! store that has only the precompiled variant means the gem is unusable, and
//! that is a recorded exclusion, not a silent miss.
//!
//! Three ways a gem turns out native, all detected (the plan's "three native
//! layouts"): `s.extensions` is set (a locally-built ext, layout 1); the
//! gemspec carries a non-ruby `s.platform` (a precompiled gem, layout 2); or a
//! `.bundle`/`.so` sits under a require path (belt-and-suspenders). A default
//! gem (layout 3) is compiled into the interpreter and is spinel's own to
//! provide, so it is caught earlier by the builtin-feature check.

use std::path::{Path, PathBuf};

use crate::gem_report::{GemRecord, SatisfiedBy};
use crate::parse::lockfile::{GemSource, Lockfile};
use crate::parse::PResult;

/// What an installed store yields for a lockfile: the pure-Ruby gems spinel can
/// compile (as `(name, roots)`), and disclosure entries for the rest.
pub(super) struct StoreResolution {
    /// `(gem name, require-path roots)` for each pure-Ruby gem -- fed to the
    /// loader's package set.
    pub roots: Vec<(String, Vec<PathBuf>)>,
    /// Divergence (a gem spinel satisfies natively) and exclusion (a native
    /// gem spinel can't provide) records for the disclosure report.
    pub disclosures: Vec<GemRecord>,
}

/// Resolve `lockfile`'s gems against the `store` directory (a `gem env
/// gemdir`). Never fails on an individual gem -- an unusable one becomes a
/// disclosure, because the program may never `require` it (an AOT compiler
/// only compiles what a require reaches).
pub(super) fn resolve(store: &Path, lockfile: &Lockfile) -> PResult<StoreResolution> {
    let specs = store.join("specifications");
    let gems = store.join("gems");
    let mut roots = Vec::new();
    let mut disclosures = Vec::new();

    for locked in &lockfile.gems {
        // Only RubyGems-store gems live in `specifications/`; a git checkout or
        // a local path gem is out of scope for the store provider.
        if locked.source != GemSource::Rubygems {
            continue;
        }
        let name = locked.name.clone();

        // spinel already provides this under its own name -- a static ext
        // (`json`) or a default gem it reimplements. Its own implementation
        // wins; record the divergence and add no store root.
        if super::loader::is_builtin_feature(&name) {
            disclosures.push(GemRecord {
                name: name.clone(),
                by: SatisfiedBy::BuiltinExt { feature: name },
            });
            continue;
        }

        // Force the ruby (source) platform: the suffix-less
        // `<name>-<version>.gemspec`. A store that only has the precompiled
        // `<name>-<version>-<platform>.gemspec` cannot be used -- its `.bundle`
        // is unloadable -- so that is a recorded exclusion.
        let Some(gemspec_path) = find_source_gemspec(&specs, &locked.name, &locked.version) else {
            disclosures.push(excluded(
                &name,
                "precompiled-platform-gem",
                format!(
                    "only a precompiled binary of `{name}` is installed; spinel forces the ruby \
                     platform and cannot load a `.bundle`. Reinstall with \
                     `--platform ruby`, or see the FFI path in docs/EXTENSIONS.md."
                ),
            ));
            continue;
        };

        let spec = super::gemspec::parse_file(&gemspec_path)?;
        let version = spec.version.as_deref().unwrap_or(&locked.version);
        let gem_dir = gems.join(format!("{}-{version}", spec.name));

        if is_native(&spec, &gem_dir) {
            disclosures.push(excluded(
                &name,
                "native-extension",
                format!(
                    "`{name}` has a native (C) extension spinel has no built-in for. \
                     See docs/EXTENSIONS.md; the FFI path is the intended escape hatch."
                ),
            ));
            continue;
        }

        // Pure Ruby: its `require_paths`, in order (`lib/` shadows an ext dir,
        // matching `full_require_paths`), become search roots.
        let gem_roots: Vec<PathBuf> = spec
            .require_paths
            .iter()
            .map(|rp| gem_dir.join(rp))
            .filter(|p| p.is_dir())
            .collect();
        // A default gem's placeholder dirs are empty (its files live in the
        // interpreter's rubylibdir); an empty root set means "nothing here",
        // so prefer spinel's own / the stdlib rather than an empty search root.
        if gem_roots.is_empty() {
            continue;
        }
        roots.push((name, gem_roots));
    }

    Ok(StoreResolution { roots, disclosures })
}

/// The suffix-less `<name>-<version>.gemspec` under `specifications/` (regular
/// gems) or `specifications/default/` (default gems). `None` when only a
/// platform-suffixed variant exists.
fn find_source_gemspec(specs: &Path, name: &str, version: &str) -> Option<PathBuf> {
    let file = format!("{name}-{version}.gemspec");
    for dir in [specs.to_path_buf(), specs.join("default")] {
        let p = dir.join(&file);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Whether a resolved gemspec is native -- any of the three detectable
/// signals (see the module docs).
fn is_native(spec: &super::gemspec::GemSpec, gem_dir: &Path) -> bool {
    if !spec.extensions.is_empty() {
        return true;
    }
    if spec.platform.as_deref().is_some_and(|p| p != "ruby" && !p.is_empty()) {
        return true;
    }
    // A `.bundle`/`.so` under any require path -- a precompiled gem that
    // declared no `s.extensions`.
    spec.require_paths.iter().any(|rp| dir_has_native_object(&gem_dir.join(rp)))
}

/// Recursively: does this directory tree contain a `.bundle`/`.so`/`.dylib`?
fn dir_has_native_object(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_native_object(&path) {
                return true;
            }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "bundle" | "so" | "dylib") {
                return true;
            }
        }
    }
    false
}

fn excluded(name: &str, kind: &str, reason: String) -> GemRecord {
    GemRecord {
        name: name.to_string(),
        by: SatisfiedBy::Excluded { kind: kind.to_string(), reason },
    }
}
