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
//! Three ways a gem turns out native, and they no longer share an answer.
//!
//! | Signal | Means | zeo |
//! |---|---|---|
//! | `s.extensions` is set | the C is SHIPPED, and builds from source | builds it |
//! | `s.platform` is not `ruby` | a precompiled binary gem | excluded |
//! | a `.bundle`/`.so` under a require path | a prebuilt object, no `s.extensions` | excluded |
//!
//! The first row is what changed: zeo compiles a gem's `ext/**/*.c` from
//! source against MRI's own headers, so a gem that ships its C is buildable.
//! The other two ship a `.so` built against MRI's ABI, which zeo can never
//! load -- and the reason it cannot is worth saying precisely, because
//! "no built-in" invited the wrong fix.
//!
//! A default gem is compiled into the interpreter and is zeo's own to
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
    /// Gems whose C is shipped as source. The loader builds one when a
    /// `require` reaches its feature -- never eagerly, because an AOT
    /// compiler only builds what a require reaches.
    pub native_exts: Vec<NativeExt>,
}

/// A gem zeo can compile from source.
pub(super) struct NativeExt {
    pub name: String,
    /// The unpacked gem's own directory, which every `s.extensions` path is
    /// relative to.
    pub gem_dir: PathBuf,
    /// `s.extensions`: one `extconf.rb` per extension, and a gem may ship
    /// more than one.
    pub extconfs: Vec<String>,
}

impl NativeExt {
    /// Does this gem's extension provide `feature`?
    ///
    /// The answer is the argument to `create_makefile`, which is what mkmf
    /// turns into `TARGET` and `target_prefix` and therefore what a
    /// `require` has to spell. Reading it out of the `extconf.rb` is a
    /// LITERAL scan: a computed argument is not seen, and the require then
    /// falls through to the ordinary miss rather than to a wrong gem.
    pub fn provides(&self, feature: &str) -> bool {
        self.extconfs.iter().any(|extconf| {
            let path = self.gem_dir.join(extconf);
            std::fs::read_to_string(&path)
                .ok()
                .is_some_and(|text| create_makefile_names(&text).iter().any(|n| n == feature))
        })
    }
}

/// Every literal `create_makefile("...")` argument in an `extconf.rb`.
///
/// A gem may call it more than once (one Makefile per extension in a
/// multi-extension `ext/` tree), so this collects rather than answering the
/// first.
fn create_makefile_names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices("create_makefile") {
        let rest = &text[i + "create_makefile".len()..];
        let rest = rest.trim_start();
        let rest = rest.strip_prefix('(').unwrap_or(rest).trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let body = &rest[1..];
        // A name with an interpolation or an escape is not a literal, and
        // guessing at one is how a require lands on the wrong gem.
        let Some(end) = body.find(quote) else {
            continue;
        };
        let name = &body[..end];
        if !name.is_empty() && !name.contains(['#', '\\']) {
            out.push(name.to_string());
        }
    }
    out
}

/// Resolve `lockfile`'s gems against the `stores` directories (each a `gem
/// env gemdir` -- `GEM_PATH` is a list, probed in order, first hit per gem
/// wins). Never fails on an individual gem -- an unusable one becomes a
/// disclosure, because the program may never `require` it (an AOT compiler
/// only compiles what a require reaches).
pub(super) fn resolve(stores: &[PathBuf], lockfile: &Lockfile) -> PResult<StoreResolution> {
    let mut roots = Vec::new();
    let mut disclosures = Vec::new();
    let mut native_exts = Vec::new();

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
        // `<name>-<version>.gemspec`. The first store with a source-platform
        // spec supplies the gem; a precompiled-only store keeps the later
        // ones in play.
        let mut found = None;
        let mut saw_precompiled = false;
        for store in stores {
            match locate_gemspec(&store.join("specifications"), &locked.name, &locked.version) {
                Located::Source(path) => {
                    found = Some((path, store));
                    break;
                }
                Located::PrecompiledOnly => saw_precompiled = true,
                Located::Absent => {}
            }
        }
        let (gemspec_path, store) = match found {
            Some(hit) => hit,
            // Only the precompiled `<name>-<version>-<platform>.gemspec` is
            // installed -- its `.bundle` is unloadable, so it is excluded.
            None if saw_precompiled => {
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
            None => continue,
        };

        let spec = super::gemspec::parse_file(&gemspec_path)?;
        let version = spec.version.as_deref().unwrap_or(&locked.version);
        let gem_dir = store.join("gems").join(format!("{}-{version}", spec.name));

        match native_kind(&spec, &gem_dir) {
            // The C is shipped: zeo compiles it from source. The extension's
            // own require path is added like any other, and the build happens
            // when a `require` reaches the feature.
            NativeKind::Buildable => {
                native_exts.push(NativeExt {
                    name: name.clone(),
                    gem_dir: gem_dir.clone(),
                    extconfs: spec.extensions.clone(),
                });
            }
            NativeKind::PrecompiledAbi => {
                disclosures.push(excluded(
                    &name,
                    "precompiled-extension",
                    format!(
                        "`{name}` ships a PRECOMPILED extension built against CRuby's ABI, \
                         which zeo cannot load. zeo builds an extension from source; install \
                         the ruby-platform variant of this gem (`bundle config set \
                         force_ruby_platform true`) and it will compile."
                    ),
                ));
                continue;
            }
            NativeKind::No => {}
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

    Ok(StoreResolution {
        roots,
        disclosures,
        native_exts,
    })
}

/// A synthetic `Lockfile` naming every gem installed in the store, one per
/// name (ruby-platform spec preferred) -- the input to the no-lockfile
/// store-sweep mode, which classifies the whole installed store. A gemspec
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
                deps: Vec::new(),
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
        roots: Vec::new(),
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

/// What kind of native a gem is, which decides what happens to it.
#[derive(Debug, PartialEq)]
pub(super) enum NativeKind {
    /// Pure Ruby.
    No,
    /// The C is shipped as source and zeo can compile it.
    Buildable,
    /// A `.so` built against CRuby's ABI. zeo can never load one.
    PrecompiledAbi,
}

/// Which of the three signals a resolved gemspec carries.
///
/// `s.extensions` is checked FIRST and wins: a gem that ships its C source
/// also ships the `.so` from an earlier build in the same directory, and
/// reading that as "precompiled" would refuse a gem zeo can build.
fn native_kind(spec: &super::gemspec::GemSpec, gem_dir: &Path) -> NativeKind {
    if !spec.extensions.is_empty() {
        return NativeKind::Buildable;
    }
    if spec
        .platform
        .as_deref()
        .is_some_and(|p| p != "ruby" && !p.is_empty())
    {
        return NativeKind::PrecompiledAbi;
    }
    // A `.bundle`/`.so` under a require path with no `s.extensions` at all:
    // the object is all there is, so there is no source to build.
    if spec
        .require_paths
        .iter()
        .any(|rp| dir_has_native_object(&gem_dir.join(rp)))
    {
        return NativeKind::PrecompiledAbi;
    }
    NativeKind::No
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
            && matches!(ext, "bundle" | "so" | "dylib")
        {
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
