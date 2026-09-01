//! A locked project's precompiled-package tier.
//!
//! One module answers every verb-level question about the project in the
//! current directory: which gems its lockfile resolves, which of them can
//! precompile, where each artifact lives in the store, and which artifacts a
//! compile can link RIGHT NOW. `zeo install` builds and places artifacts
//! through it, and `zeo flags` prints what it answers -- one producer, so a
//! Makefile handed the flags cannot drift from what the verbs would do.
//!
//! Store artifacts are a cache tier, never intent: an artifact whose
//! compiler or target does not match this binary is passed over silently
//! (the contract is tag match or recompile), and one the merge later
//! refuses drops to a source compile with a warning. Neither can change
//! what a program answers.

use std::path::{Path, PathBuf};

/// Where a project keeps its inputs: the Gemfile's lockfile and the gem
/// stores its gems are installed in.
pub struct Project {
    pub gemfile: PathBuf,
    pub lockfile: PathBuf,
    /// Stores in probe order (`gem env gemdir` shape, first hit wins).
    pub stores: Vec<PathBuf>,
}

/// One lockfile gem through the package tier's eyes.
pub struct GemRow {
    pub name: String,
    pub version: String,
    /// The require spelling (`io-console` -> `io/console`).
    pub feature: String,
    /// The file that require reaches -- the package build's entry.
    pub entry: Option<PathBuf>,
    /// The artifact's store home
    /// (`<store>/zeo/<target>/zeo-<abi>/<name>-<version>/pkg.zeopkg`).
    pub home: Option<PathBuf>,
    /// An artifact the gem itself SHIPPED (`zeo/pkg.zeopkg` inside its
    /// tree -- what `zeo gem precompile` puts in a platform gem). Trusted
    /// only on an exact identity match, exactly like a store artifact.
    pub shipped: Option<PathBuf>,
    /// Why this gem cannot precompile, when it cannot.
    pub skip: Option<String>,
}

/// Locate the project: an explicit Gemfile wins, then `./Gemfile`. Stores
/// are the explicit ones, then the `vendor/bundle` tree `bundle install`
/// writes beside the Gemfile, then `GEM_PATH`. Every error names the verb
/// that fixes it.
pub fn locate(
    gemfile: Option<PathBuf>,
    stores: Vec<PathBuf>,
    gem_path_env: Option<&std::ffi::OsStr>,
) -> Result<Project, String> {
    let gemfile = match gemfile {
        Some(g) => g,
        None => {
            let g = PathBuf::from("Gemfile");
            if !g.is_file() {
                return Err(
                    "no Gemfile here; run from the project, or name one with --bundle-gemfile"
                        .to_string(),
                );
            }
            g
        }
    };
    let lockfile = derive_lockfile(gemfile.clone());
    if !lockfile.is_file() {
        return Err(format!(
            "{} has no lockfile at {}; run `zeo bundle install` first",
            gemfile.display(),
            lockfile.display()
        ));
    }
    let mut stores = stores;
    if stores.is_empty() {
        let root = gemfile.parent().unwrap_or_else(|| Path::new("."));
        if let Some(vendored) = crate::bundled::store_dir(root) {
            stores.push(vendored);
        } else if let Some(gp) = gem_path_env {
            stores.extend(std::env::split_paths(gp));
        }
    }
    if stores.is_empty() {
        return Err(
            "no gem store: run `zeo bundle install` (vendor/bundle), or name one with \
             --gem-path / GEM_PATH"
                .to_string(),
        );
    }
    Ok(Project {
        gemfile,
        lockfile,
        stores,
    })
}

/// The lockfile a Gemfile path names, by bundler's own rules: `Gemfile` ->
/// `Gemfile.lock`, `gems.rb` -> `gems.locked`, anything else gets `.lock`
/// appended; a path that already IS a lockfile is taken as given.
pub fn derive_lockfile(gemfile: PathBuf) -> PathBuf {
    let name = gemfile
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.ends_with(".lock") || name.ends_with(".locked") {
        gemfile
    } else if name == "gems.rb" {
        gemfile.with_file_name("gems.locked")
    } else {
        let mut n = name;
        n.push_str(".lock");
        gemfile.with_file_name(n)
    }
}

/// Every RubyGems-store gem the lockfile resolves, with its entry and its
/// artifact home. Rows with `skip` say why they cannot precompile.
pub fn survey(project: &Project) -> Result<Vec<GemRow>, String> {
    survey_lock(&project.lockfile, &project.stores)
}

/// [`survey`] from the two inputs a compile already holds.
pub fn survey_lock(lockfile: &Path, stores: &[PathBuf]) -> Result<Vec<GemRow>, String> {
    let lock = crate::parse::lockfile::parse_file(lockfile)
        .map_err(|e| format!("reading {}: {e}", lockfile.display()))?;
    let gems = crate::parse::store_gems(stores, &lock)
        .map_err(|e| format!("reading the gem store: {e}"))?;
    let target = crate::backend::link::host_triple();
    Ok(gems
        .into_iter()
        .map(|g| {
            let home = g.entry.is_some().then(|| {
                crate::package::store_artifact_home(&g.store, target, &g.name, &g.version)
            });
            GemRow {
                name: g.name,
                version: g.version,
                feature: g.feature,
                entry: g.entry,
                home,
                shipped: g.shipped,
                skip: g.skip,
            }
        })
        .collect())
}

/// The artifacts a compile of this project can link right now: present at
/// their store homes, built by THIS compiler for THIS target. Anything
/// else is passed over without a word -- the artifact is a cache entry,
/// and a cache answers or it does not.
pub fn linkable(rows: &[GemRow]) -> Vec<PathBuf> {
    rows.iter()
        .filter_map(|row| {
            [row.home.as_ref(), row.shipped.as_ref()]
                .into_iter()
                .flatten()
                .find(|c| artifact_matches(c))
                .cloned()
        })
        .collect()
}

/// Whether the artifact at `path` was built by THIS compiler for THIS
/// target -- the whole acceptance contract (decision 7: exact match, no
/// stable tag). Unreadable or unparseable answers false.
pub fn artifact_matches(path: &Path) -> bool {
    let Ok((manifest_text, _)) = crate::package::read_zeopkg(path) else {
        return false;
    };
    let Ok(m) = crate::package::Manifest::parse(&manifest_text) else {
        return false;
    };
    m.compiler == crate::package::compiler_identity()
        && m.target == crate::backend::link::host_triple()
}

/// RubyGems' name for this build's platform (`Gem::Platform.local`).
pub fn gem_platform() -> &'static str {
    match crate::backend::link::host_triple() {
        "aarch64-apple-darwin" => "arm64-darwin",
        "x86_64-apple-darwin" => "x86_64-darwin",
        "x86_64-unknown-linux-gnu" => "x86_64-linux",
        "aarch64-unknown-linux-gnu" => "aarch64-linux",
        other => other,
    }
}

/// The gem an author is standing in: exactly one `*.gemspec` at `dir`'s
/// top level, pure Ruby, with the conventional entry file.
pub struct AuthorGem {
    pub gemspec: PathBuf,
    pub name: String,
    pub version: String,
    /// The require spelling (`io-console` -> `io/console`).
    pub feature: String,
    pub entry: PathBuf,
}

/// Read the gem at `dir` for `zeo gem precompile`. Every refusal says
/// what is missing or why the gem cannot carry an artifact.
pub fn author_gem(dir: &Path) -> Result<AuthorGem, String> {
    let mut specs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "gemspec"))
        .collect();
    specs.sort();
    let gemspec = match specs.len() {
        0 => return Err("no .gemspec here; run from the gem's own directory".to_string()),
        1 => specs.remove(0),
        _ => return Err(format!("{} gemspecs here; expected one", specs.len())),
    };
    let spec = crate::parse::gemspec::parse_file(&gemspec)
        .map_err(|e| format!("reading {}: {e}", gemspec.display()))?;
    if !spec.extensions.is_empty() {
        return Err(format!(
            "`{}` ships a native extension; the package tier cannot carry it",
            spec.name
        ));
    }
    let version = spec
        .version
        .clone()
        .ok_or_else(|| format!("{} sets no version", gemspec.display()))?;
    // Two entry spellings, the same probe `store_gems` runs: the slash
    // convention (net-http -> net/http.rb) and the name verbatim (open-uri
    // ships open-uri.rb).
    let (feature, entry) = [spec.name.replace('-', "/"), spec.name.clone()]
        .into_iter()
        .find_map(|feature| {
            spec.require_paths
                .iter()
                .map(|rp| dir.join(rp).join(format!("{feature}.rb")))
                .find(|p| p.is_file())
                .map(|p| (feature, p))
        })
        .ok_or_else(|| {
            format!(
                "`{}` has no {}.rb under its require paths",
                spec.name,
                spec.name.replace('-', "/")
            )
        })?;
    Ok(AuthorGem {
        gemspec,
        name: spec.name,
        version,
        feature,
        entry,
    })
}

/// The store tier a compile consults on its own: every linkable artifact
/// for `lockfile` against `stores`. Any error along the way -- a lockfile
/// that does not parse, an unreadable store -- answers the empty set, so a
/// broken store can slow a compile but never stop one.
pub fn store_linkable(lockfile: &Path, stores: &[PathBuf]) -> Vec<PathBuf> {
    match survey_lock(lockfile, stores) {
        Ok(rows) => linkable(&rows),
        Err(_) => Vec::new(),
    }
}
