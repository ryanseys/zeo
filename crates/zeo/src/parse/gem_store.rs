//! The external gem store provider: turn a `Gemfile.lock` + an installed
//! RubyGems store (`gem env gemdir`) into extra require-path roots for the
//! gems zeo CAN compile, plus a disclosure entry for every gem it can't.
//!
//! zeo consumes Bundler's/RubyGems' NORMALIZED output; it never resolves,
//! fetches, or builds extensions. The one policy choice, TruffleRuby's
//! `force_ruby_platform`: a precompiled platform gem ships a `.bundle` zeo
//! can never load, so the SOURCE (ruby-platform) gemspec is required -- a
//! store that has only the precompiled variant means the gem is unusable, and
//! that is a recorded exclusion, not a silent miss.
//!
//! Three ways a gem turns out native, all detected (the plan's "three native
//! layouts"): `s.extensions` is set (a locally-built ext, layout 1); the
//! gemspec carries a non-ruby `s.platform` (a precompiled gem, layout 2); or a
//! `.bundle`/`.so` sits under a require path (belt-and-suspenders). A default
//! gem (layout 3) is compiled into the interpreter and is zeo's own to
//! provide, so it is caught earlier by the builtin-feature check.

use std::path::{Path, PathBuf};

use crate::gem_report::{GemRecord, SatisfiedBy};
use crate::lower::PResult;
use crate::parse::lockfile::{GemSource, LockedGem, Lockfile};

/// What an installed store yields for a lockfile: the pure-Ruby gems zeo can
/// compile (as `(name, roots)`), and disclosure entries for the rest.
pub(super) struct StoreResolution {
    /// `(gem name, require-path roots)` for each pure-Ruby gem -- fed to the
    /// loader's package set.
    pub roots: Vec<(String, Vec<PathBuf>)>,
    /// Divergence (a gem zeo satisfies natively) and exclusion (a native
    /// gem zeo can't provide) records for the disclosure report.
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

        // zeo already provides this under its own name -- a static ext
        // (`json`) or a default gem it reimplements. Its own implementation
        // wins; record the divergence and add no store root.
        if crate::lower::features::is_builtin_feature(&name) {
            disclosures.push(GemRecord {
                name: name.clone(),
                by: SatisfiedBy::BuiltinExt { feature: name },
            });
            continue;
        }

        // Force the ruby (source) platform: the suffix-less
        // `<name>-<version>.gemspec`.
        let gemspec_path = match locate_gemspec(&specs, &locked.name, &locked.version) {
            Located::Source(path) => path,
            // Only the precompiled `<name>-<version>-<platform>.gemspec` is
            // installed -- its `.bundle` is unloadable, so it is excluded.
            Located::PrecompiledOnly => {
                disclosures.push(excluded(
                    &name,
                    "precompiled-platform-gem",
                    format!(
                        "only a precompiled binary of `{name}` is installed; zeo forces the \
                         ruby platform and cannot load a `.bundle`. Reinstall with \
                         `--platform ruby`, or see the FFI path in docs/EXTENSIONS.md."
                    ),
                ));
                continue;
            }
            // Locked but no gemspec at all -- a store/`bundle install` state
            // issue, not something zeo owns. Skipped without a disclosure.
            Located::Absent => continue,
        };

        let spec = super::gemspec::parse_file(&gemspec_path)?;
        let version = spec.version.as_deref().unwrap_or(&locked.version);
        let gem_dir = gems.join(format!("{}-{version}", spec.name));

        if is_native(&spec, &gem_dir) {
            disclosures.push(excluded(
                &name,
                "native-extension",
                format!(
                    "`{name}` has a native (C) extension zeo has no built-in for. \
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
        // so prefer zeo's own / the stdlib rather than an empty search root.
        if gem_roots.is_empty() {
            continue;
        }
        roots.push((name, gem_roots));
    }

    Ok(StoreResolution { roots, disclosures })
}

/// A synthetic `Lockfile` naming every gem installed in the store, one per
/// name (ruby-platform spec preferred) -- the input to the no-lockfile
/// `gem-compat` mode, which classifies the whole installed store. A gemspec
/// that fails the static parse is skipped rather than aborting the sweep.
pub(super) fn installed_as_lockfile(store: &Path) -> PResult<Lockfile> {
    use std::collections::BTreeMap;
    let specs = store.join("specifications");
    let mut gems: BTreeMap<String, LockedGem> = BTreeMap::new();
    for dir in [specs.clone(), specs.join("default")] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("gemspec") {
                continue;
            }
            let Ok(spec) = super::gemspec::parse_file(&path) else {
                continue; // an unparseable gemspec is skipped, not fatal
            };
            let platform = spec.platform.filter(|p| p != "ruby" && !p.is_empty());
            let gem = LockedGem {
                name: spec.name.clone(),
                version: spec.version.unwrap_or_default(),
                platform,
                source: GemSource::Rubygems,
            };
            // One row per name; a ruby-platform (suffix-less) spec wins over a
            // precompiled variant, matching the lockfile's own preference.
            match gems.get(&spec.name) {
                Some(existing) if existing.platform.is_none() && gem.platform.is_some() => {}
                _ => {
                    gems.insert(spec.name, gem);
                }
            }
        }
    }
    if gems.is_empty() {
        return Err(format!("no gemspecs found under {}", specs.display()).into());
    }
    Ok(Lockfile {
        gems: gems.into_values().collect(),
        platforms: Vec::new(),
        bundler_version: None,
    })
}

/// The result of locating a locked gem's gemspec, forcing the ruby platform.
enum Located {
    /// The suffix-less `<name>-<version>.gemspec` -- a source-platform gem.
    Source(PathBuf),
    /// Only a platform-suffixed `<name>-<version>-<platform>.gemspec` exists.
    PrecompiledOnly,
    /// No gemspec for this name+version at all (not installed).
    Absent,
}

/// Locate `<name>-<version>.gemspec` under `specifications/` (regular gems) or
/// `specifications/default/` (default gems), distinguishing "only a precompiled
/// variant is installed" from "not installed at all".
fn locate_gemspec(specs: &Path, name: &str, version: &str) -> Located {
    let source = format!("{name}-{version}.gemspec");
    let prefix = format!("{name}-{version}-");
    let mut saw_precompiled = false;
    for dir in [specs.to_path_buf(), specs.join("default")] {
        let p = dir.join(&source);
        if p.is_file() {
            return Located::Source(p);
        }
        // Any `<name>-<version>-<platform>.gemspec` -> a precompiled install.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name();
                let fname = fname.to_string_lossy();
                if fname.starts_with(&prefix) && fname.ends_with(".gemspec") {
                    saw_precompiled = true;
                }
            }
        }
    }
    if saw_precompiled {
        Located::PrecompiledOnly
    } else {
        Located::Absent
    }
}

/// Whether a resolved gemspec is native -- any of the three detectable
/// signals (see the module docs).
fn is_native(spec: &super::gemspec::GemSpec, gem_dir: &Path) -> bool {
    if !spec.extensions.is_empty() {
        return true;
    }
    if spec
        .platform
        .as_deref()
        .is_some_and(|p| p != "ruby" && !p.is_empty())
    {
        return true;
    }
    // A `.bundle`/`.so` under any require path -- a precompiled gem that
    // declared no `s.extensions`.
    spec.require_paths
        .iter()
        .any(|rp| dir_has_native_object(&gem_dir.join(rp)))
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
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && matches!(ext, "bundle" | "so" | "dylib") {
                return true;
            }
    }
    false
}

fn excluded(name: &str, kind: &str, reason: String) -> GemRecord {
    GemRecord {
        name: name.to_string(),
        by: SatisfiedBy::Excluded {
            kind: kind.to_string(),
            reason,
        },
    }
}
