//! Gem discovery: the packages/bundled-gems directory scan, gemspec/manifest
//! parsing, lockfile precedence, and the native-gem/feature classifiers.

use super::{Gem, GemProvenance, PResult};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The directories holding the libraries the compiler ships, highest
/// precedence first. An absent dir contributes nothing.
///
/// The dev tree has TWO. Each library zeo owns is a gem-shaped directory
/// under `crates/zeo-rt/ext/`, its Ruby half in `lib/` beside the Rust that
/// implements it; the vendored upstream copies stay in `gems/`. A zeo
/// library must win its name, so it is searched first. Every other home has
/// one directory, because `dist`/`stage-publish` stage both tiers into it.
pub(super) fn bundled_gems_dirs() -> Vec<PathBuf> {
    let dirs = match crate::home::zeo_home() {
        crate::home::ZeoHome::DevTree { root } => {
            vec![root.join("crates/zeo-rt/ext"), root.join("gems")]
        }
        crate::home::ZeoHome::Installed { payload, .. } => vec![payload.join("gems")],
        // Embedded in the binary at publish time; extracted once per version.
        crate::home::ZeoHome::Registry { cache } => {
            crate::home::registry_gems_dir(cache).into_iter().collect()
        }
    };
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

/// Whether `ZEO_DEBUG=strict-ambiguous-require` is set: a feature found in
/// multiple gems becomes the old hard compile error instead of resolving to
/// the precedence-first provider with a warning. Real Ruby never errors here
/// (see `resolve_require_uncached`'s multi-hit arm), so strictness is opt-in.
pub(super) fn strict_ambiguous_require() -> bool {
    crate::debug_flags::debug(crate::debug_flags::DebugFlag::StrictAmbiguousRequire)
}

/// The lockfile's require-precedence ranks: gem name -> position, roots
/// first, then breadth-first down the dependency edges with each gem's deps
/// visited in name order. This is Bundler's activation order REVERSED --
/// Bundler activates dependencies before dependents (`spec_set.rb`'s tsort)
/// and a later activation inserts its load path ahead, so the DEPENDENT wins
/// an ambiguous feature; ranking dependents first expresses that as plain
/// first-match. Locked gems no edge reaches (a lockfile always connects, but
/// a hand-edited one may not) follow in name order, still ahead of anything
/// unlocked.
pub(super) fn lockfile_precedence(
    lock: &crate::parse::lockfile::Lockfile,
) -> HashMap<String, usize> {
    let by_name: HashMap<&str, &crate::parse::lockfile::LockedGem> =
        lock.gems.iter().map(|g| (g.name.as_str(), g)).collect();
    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut queue: std::collections::VecDeque<&str> =
        lock.roots.iter().map(String::as_str).collect();
    while let Some(name) = queue.pop_front() {
        if !seen.insert(name) {
            continue;
        }
        order.push(name);
        if let Some(gem) = by_name.get(name) {
            let mut deps: Vec<&str> = gem.deps.iter().map(String::as_str).collect();
            deps.sort_unstable();
            queue.extend(deps);
        }
    }
    // `lock.gems` is already name-sorted (the parser's BTreeMap).
    for gem in &lock.gems {
        if !seen.contains(gem.name.as_str()) {
            order.push(&gem.name);
        }
    }
    order
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), i))
        .collect()
}

/// Discovers packages under each dir, in dir order: every subdirectory
/// containing a `.gemspec` is a gem (subdirectories without
/// one are silently ignored -- not gems). Within one dir, discovery is
/// name-sorted (deterministic); across dirs, the FIRST occurrence of a
/// package NAME wins entirely (a project-local package shadows a
/// same-named compiler-bundled one -- the "one version per name, nearest
/// wins" rule, the same shape as Bundler's lockfile picking exactly one
/// version). A missing/unreadable packages dir contributes nothing (the
/// CLI passes default candidate locations that often don't exist).
pub(super) fn discover_packages(
    package_dirs: &[PathBuf],
    bundled_dirs: &[PathBuf],
) -> PResult<Vec<Gem>> {
    let mut packages: Vec<Gem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in package_dirs {
        let bundled = bundled_dirs.contains(dir);
        let provenance = if bundled {
            GemProvenance::Bundled
        } else {
            GemProvenance::PackageDir
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut pkg_dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && (gemspec_path(p).is_some() || is_bundled_library(p, bundled)))
            .collect();
        pkg_dirs.sort();
        for pkg_dir in pkg_dirs {
            let pkg = parse_manifest(&pkg_dir, provenance)?;
            if seen.insert(pkg.name.clone()) {
                packages.push(pkg);
                continue;
            }
            // First-name-wins, EXCEPT for the curated set where zeo's own
            // native half IS the gem (`gem_report::substitution_note` -- ffi,
            // json, psych, openssl, ...). There the upstream Ruby half is
            // dead code: it opens by requiring a C extension zeo does not
            // have, and every class it then defines is one zeo's native half
            // already owns. Letting a caller-supplied copy shadow zeo's
            // compiled a program that died at load -- the real ffi gem's
            // `ffi/types.rb` raising `uninitialized constant FFI::TypeDefs`
            // -- and, once that file's computed `require RUBY_VERSION... +
            // "/ffi_c"` demanded the gem's whole load path as units, dragged
            // in `ffi/struct_layout.rb`'s `class Enum < Field` and failed the
            // compile outright. The `zeo-gems.json` record has always
            // CLAIMED zeo's implementation is the one in use; this is what
            // makes the claim true.
            if provenance == GemProvenance::Bundled
                && crate::gem_report::substitution_note(&pkg.name).is_some()
                && let Some(slot) = packages.iter_mut().find(|g| g.name == pkg.name)
            {
                *slot = pkg;
            }
        }
    }
    Ok(packages)
}

/// Finds and parses a gem directory's `.gemspec`.
///
/// The manifest is a real gemspec, not an invented format -- see
/// `parse::gemspec` for why a static parse is safe (RubyGems' serialized form
/// has a closed grammar, enforced by a corpus test over the installed store).
/// That is what lets one code path read a bundled zeo library, a vendored
/// gem, and a gem out of a real installed store.
///
/// The gemspec's `name` must match its directory, mirroring RubyGems' own
/// `<name>-<version>/` convention: a mismatch means a `require` would resolve
/// out of a directory that doesn't name the gem it provides.
///
/// A declared `require_paths` entry that doesn't exist contributes no search
/// root, and is not an error. RubyGems puts the directory on `$LOAD_PATH`
/// whether or not it is there, and `require` simply never matches inside it --
/// oracle-verified against ruby 4.0.6. Metagems rely on this: `rails` itself
/// declares `require_paths: [lib]` and ships only a README and a licence, so
/// rejecting the gem would refuse to compile every program that depends on it.
/// An unsatisfiable `require` still fails, which is where the real error is.
pub(super) fn parse_manifest(pkg_dir: &Path, provenance: GemProvenance) -> PResult<Gem> {
    let Some(manifest_path) = gemspec_path(pkg_dir) else {
        return bundled_library(pkg_dir, provenance);
    };
    let spec = crate::parse::gemspec::parse_file(&manifest_path)?;
    let dir_name = pkg_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if spec.name != dir_name {
        return Err(format!(
            "{}: gem name \"{}\" doesn't match its directory name \"{dir_name}\"",
            manifest_path.display(),
            spec.name
        )
        .into());
    }
    let roots: Vec<PathBuf> = spec
        .require_paths
        .iter()
        .map(|rp| pkg_dir.join(rp))
        .filter(|root| root.is_dir())
        .collect();
    Ok(Gem {
        name: spec.name,
        roots,
        version: spec.version,
        provenance,
    })
}

/// A `lib/` with no `.gemspec` in the tier zeo itself ships: `socket`, `pty`
/// and `monitor`, which ruby installs on rubylibdir rather than as gems.
fn is_bundled_library(pkg_dir: &Path, bundled: bool) -> bool {
    bundled && pkg_dir.join("lib").is_dir()
}

/// One of those, as a package with no version -- there is no gemspec to state
/// one, and inventing a number nothing can check is what this replaces.
fn bundled_library(pkg_dir: &Path, provenance: GemProvenance) -> PResult<Gem> {
    let lib = pkg_dir.join("lib");
    if !lib.is_dir() {
        return Err(format!("{}: no `.gemspec`", pkg_dir.display()).into());
    }
    Ok(Gem {
        name: pkg_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        roots: vec![lib],
        version: None,
        provenance,
    })
}

/// The `.gemspec` in a gem directory, if there is exactly one candidate.
///
/// Prefers `<dir>/<dir-name>.gemspec` (the convention every real gem follows)
/// and otherwise takes any single `.gemspec` present, so a directory whose
/// gemspec is named differently still loads and fails with the clearer
/// name-mismatch error above rather than a bare "no `.gemspec`".
pub(super) fn gemspec_path(pkg_dir: &Path) -> Option<PathBuf> {
    let dir_name = pkg_dir.file_name()?;
    let conventional = pkg_dir.join(dir_name).with_extension("gemspec");
    if conventional.is_file() {
        return Some(conventional);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(pkg_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "gemspec"))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// CRuby's exact missing-feature message (`load_failed` -> `rb_load_fail`).
/// A path for the disclosure record, made relative to the current directory
/// when it sits under it (so a bundled gem reads `gems/optparse/lib/optparse.rb`
/// rather than an absolute machine path), else left absolute.
pub(super) fn display_path(path: &Path) -> String {
    // Canonicalize first so a bundled path baked with `../..`
    // (`CARGO_MANIFEST_DIR/../../gems/...`) collapses before the CWD strip,
    // yielding a clean relative path rather than `crates/zeo/../..`.
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .and_then(|cwd| resolved.strip_prefix(&cwd).ok().map(Path::to_path_buf))
        .unwrap_or(resolved)
        .display()
        .to_string()
}

pub(super) fn cannot_load(name: &str) -> String {
    // mruby's unclaimed win: when the name is a well-known gem with a NATIVE
    // half zeo has no static ext for, say so -- otherwise it reads like an
    // unsupported language feature rather than "this gem isn't linked in".
    // FFI is the future escape hatch this points at.
    if is_known_native_gem(name) {
        return format!(
            "cannot load such file -- {name}: this gem has a native (C) extension \
             zeo does not provide a built-in for. See docs/EXTENSIONS.md; the FFI \
             path is the intended escape hatch."
        );
    }
    if let Some(reason) = zeo_abi::declined_feature_reason(name) {
        return format!("cannot load such file -- {name}: {reason}");
    }
    format!("cannot load such file -- {name}")
}

/// A small allowlist of popular gems whose real implementation is a C
/// extension -- named so a failed `require` explains itself (see `cannot_load`).
/// Not exhaustive and not load-bearing: an unrecognized native gem still fails,
/// just with the plainer message.
pub(super) fn is_known_native_gem(name: &str) -> bool {
    matches!(
        name,
        // `ffi` is NOT here: zeo provides it (the compile-time FFI frontend),
        // so `require "ffi"` succeeds via `is_builtin_feature`.
        "sqlite3"
            | "nokogiri"
            | "pg"
            | "mysql2"
            | "bcrypt"
            | "nio4r"
            | "puma"
            | "grpc"
            | "protobuf"
            | "oj"
            | "msgpack"
            | "eventmachine"
            | "sass"
            | "rmagick"
    )
}

/// Whether `feature` is a mspec/spec_helper require the rubyspec conformance
/// suite stubs out (see `splice_feature`). Gated on `ZEO_MSPEC_STUBS` so it
/// never affects a normal compile. Matched by basename, so `../spec_helper`,
/// `spec/spec_helper`, `mspec`, and `mspec/guards/version` all collapse.
pub(super) fn is_mspec_stub_feature(feature: &str) -> bool {
    if std::env::var_os("ZEO_MSPEC_STUBS").is_none() {
        return false;
    }
    let base = feature.rsplit('/').next().unwrap_or(feature);
    base == "spec_helper" || feature == "mspec" || feature.starts_with("mspec/")
}

pub(super) fn with_rb_ext(feature: &str) -> String {
    if feature.ends_with(".rb") {
        feature.to_string()
    } else {
        format!("{feature}.rb")
    }
}

pub(super) fn is_native_feature(feature: &str) -> bool {
    feature.ends_with(".so") || feature.ends_with(".o") || feature.ends_with(".bundle")
}
