//! Gem discovery: the packages/bundled-gems directory scan, gemspec/manifest
//! parsing, lockfile precedence, and the native-gem/feature classifiers.

use super::{Gem, GemProvenance, PResult};
use crate::gems::bundled::Library;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The libraries the compiler ships, highest precedence first -- each entry
/// is one library's own directory, not a directory of them.
///
/// The dev tree draws on three tiers and an installed zeo on one, because
/// `dist`/`stage-publish` flatten all of them into `share/zeo/lib/ruby/`.
/// [`crate::gems::bundled`] is where that list is decided and why.
///
/// Memoized: the tiers are read once per process, and a run-time `eval` is a
/// whole compile that would otherwise walk the store again.
pub(super) fn bundled_libraries() -> &'static [Library] {
    static LIBS: std::sync::OnceLock<Vec<Library>> = std::sync::OnceLock::new();
    LIBS.get_or_init(|| match crate::home::zeo_home() {
        crate::home::ZeoHome::DevTree { root } => crate::gems::bundled::dev_tree_libraries(root),
        crate::home::ZeoHome::Installed { payload, .. } => {
            crate::gems::bundled::libraries_in(&payload.join(crate::gems::bundled::BOOTSTRAP_TIER))
        }
        // Embedded in the binary at publish time; extracted once per version.
        crate::home::ZeoHome::Registry { cache } => crate::home::registry_libraries_dir(cache)
            .as_deref()
            .map(crate::gems::bundled::libraries_in)
            .unwrap_or_default(),
    })
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
pub(super) fn lockfile_precedence(lock: &zeo_gem::Lockfile) -> HashMap<String, usize> {
    let gems = lock.resolved();
    let by_name: HashMap<&str, &zeo_gem::LockedGem> =
        gems.iter().map(|g| (g.name.as_str(), g)).collect();
    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let roots = lock.roots();
    let mut queue: std::collections::VecDeque<&str> = roots.iter().map(String::as_str).collect();
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
    // The resolved view is already name-sorted.
    for gem in &gems {
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

/// Discovers packages under each `package_dirs` entry, in dir order: every
/// subdirectory containing a `.gemspec` is a gem (subdirectories without
/// one are silently ignored -- not gems). Within one dir, discovery is
/// name-sorted (deterministic); across dirs, the FIRST occurrence of a
/// package NAME wins entirely (a project-local package shadows a
/// same-named compiler-bundled one -- the "one version per name, nearest
/// wins" rule, the same shape as Bundler's lockfile picking exactly one
/// version). A missing/unreadable packages dir contributes nothing (the
/// CLI passes default candidate locations that often don't exist).
///
/// `bundled_libs` names each shipped library's OWN directory and is appended
/// last, so a caller-supplied dir shadows the compiler's stdlib tier. It is
/// a list of libraries rather than of directories holding them because its
/// third tier is a RubyGems store, where the directory is `<name>-<version>`
/// and only the gemspec states the name.
///
/// MEMOIZED for the life of the process, and that is not an optimization
/// detail: a run-time `eval` is a whole compile, so it discovers packages
/// too. RubyGems evals 66 default gemspecs at boot, and each one re-read
/// every bundled gem directory and re-parsed every gemspec -- 11.5 ms a
/// time, which WAS essentially the entire cost of a run-time eval.
///
/// The key is the directory list plus each directory's mtime, so a gem
/// added or removed inside one process is still seen. A gemspec EDITED in
/// place is not: the directory does not change, and nothing in zeo rewrites
/// a gemspec while a compile is running. A fresh process reads it.
pub(super) fn discover_packages(
    package_dirs: &[PathBuf],
    bundled_libs: &[Library],
) -> PResult<Vec<Gem>> {
    type Key = (
        Vec<PathBuf>,
        Vec<PathBuf>,
        Vec<Option<std::time::SystemTime>>,
    );
    static MEMO: std::sync::Mutex<Option<HashMap<Key, Vec<Gem>>>> = std::sync::Mutex::new(None);

    let stamps: Vec<Option<std::time::SystemTime>> = package_dirs
        .iter()
        .map(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok())
        .collect();
    // Each library's directory identifies it; its gemspec path is a function
    // of that directory and the tier it came from.
    let libs: Vec<PathBuf> = bundled_libs.iter().map(|l| l.dir.clone()).collect();
    let key: Key = (package_dirs.to_vec(), libs, stamps);
    if let Some(hit) = MEMO
        .lock()
        .expect("the package memo is never poisoned")
        .get_or_insert_with(HashMap::default)
        .get(&key)
    {
        return Ok(hit.clone());
    }
    let packages = discover_packages_uncached(package_dirs, bundled_libs)?;
    MEMO.lock()
        .expect("the package memo is never poisoned")
        .get_or_insert_with(HashMap::default)
        .insert(key, packages.clone());
    Ok(packages)
}

fn discover_packages_uncached(
    package_dirs: &[PathBuf],
    bundled_libs: &[Library],
) -> PResult<Vec<Gem>> {
    let mut packages: Vec<Gem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut admit = |pkg: Gem, packages: &mut Vec<Gem>| {
        if seen.insert(pkg.name.clone()) {
            packages.push(pkg);
            return;
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
        //
        // Only a CALLER's copy is displaced. Within the bundled tiers the
        // list is already precedence-ordered, so zeo's own half is the
        // incumbent there and displacing it would hand the name back to the
        // upstream Ruby this rule exists to keep out.
        if pkg.provenance == GemProvenance::Bundled
            && crate::gems::report::substitution_note(&pkg.name).is_some()
            && let Some(slot) = packages
                .iter_mut()
                .find(|g| g.name == pkg.name && g.provenance == GemProvenance::PackageDir)
        {
            *slot = pkg;
        }
    };

    for dir in package_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut pkg_dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && gemspec_path(p).is_some())
            .collect();
        pkg_dirs.sort();
        for pkg_dir in pkg_dirs {
            admit(
                parse_manifest(&pkg_dir, None, GemProvenance::PackageDir)?,
                &mut packages,
            );
        }
    }
    for lib in bundled_libs {
        admit(
            parse_manifest(&lib.dir, lib.gemspec.as_deref(), GemProvenance::Bundled)?,
            &mut packages,
        );
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
/// The gemspec's `name` must match its directory -- either verbatim, or as
/// RubyGems' own `<name>-<version>/`, which is how the resolved tier's
/// `vendor/bundle` store names an unpacked gem. A mismatch means a `require`
/// would resolve out of a directory that doesn't name the gem it provides.
///
/// `manifest` overrides where the gemspec is read from, which a store gem
/// needs: see [`Library::gemspec`].
///
/// A declared `require_paths` entry that doesn't exist contributes no search
/// root, and is not an error. RubyGems puts the directory on `$LOAD_PATH`
/// whether or not it is there, and `require` simply never matches inside it --
/// oracle-verified against ruby 4.0.6. Metagems rely on this: `rails` itself
/// declares `require_paths: [lib]` and ships only a README and a licence, so
/// rejecting the gem would refuse to compile every program that depends on it.
/// An unsatisfiable `require` still fails, which is where the real error is.
pub(super) fn parse_manifest(
    pkg_dir: &Path,
    manifest: Option<&Path>,
    provenance: GemProvenance,
) -> PResult<Gem> {
    let manifest_path = match manifest {
        Some(path) => path.to_path_buf(),
        None => match gemspec_path(pkg_dir) {
            Some(path) => path,
            None => return bundled_library(pkg_dir, provenance),
        },
    };
    let spec = crate::parse::read_gemspec(&manifest_path)?;
    let dir_name = pkg_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let versioned = spec
        .version
        .as_deref()
        .map(|v| format!("{}-{v}", spec.name))
        .unwrap_or_default();
    if spec.name != dir_name && versioned != dir_name {
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
        gemspec: Some(manifest_path),
    })
}

/// A `lib/` with no `.gemspec` in the tier zeo itself ships -- `socket`,
/// `pty` and `monitor`, which ruby installs on rubylibdir rather than as gems
/// -- as a package with no version. There is no gemspec to state one, and
/// inventing a number nothing can check is what this replaces.
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
        gemspec: None,
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
/// when it sits under it (so a bundled gem reads
/// `vendor/bundle/ruby/4.0.0/gems/optparse-0.8.1/lib/optparse.rb` rather than
/// an absolute machine path), else left absolute.
pub(super) fn display_path(path: &Path) -> String {
    // Canonicalize first so a bundled path baked with `../..` collapses
    // before the CWD strip, yielding a clean relative path rather than
    // `crates/zeo/../..`.
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
             zeo does not provide a built-in for. See docs/how-to/add-an-extension.md; the FFI \
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

/// The suffixes a compiled extension wears on this platform, DLEXT first.
/// Must stay the runtime's list (`zeo_rt::features::NATIVE_SUFFIXES`): the
/// compile decides whether to publish the C API and the run answers the
/// require, so a suffix only one of them tries is a require that resolves in
/// one build mode and not the other.
#[cfg(target_vendor = "apple")]
pub(super) const NATIVE_SUFFIXES: &[&str] = &["bundle", "so"];
#[cfg(not(target_vendor = "apple"))]
pub(super) const NATIVE_SUFFIXES: &[&str] = &["so"];

#[cfg(test)]
mod tests {
    use super::*;

    fn gem_dir(root: &Path, name: &str, version: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("lib")).expect("a lib dir");
        std::fs::write(
            dir.join(format!("{name}.gemspec")),
            format!(
                "Gem::Specification.new do |s|\n  \
                 s.name = {name:?}\n  s.version = {version:?}\n  \
                 s.require_paths = [\"lib\"]\nend\n"
            ),
        )
        .expect("a gemspec");
        dir
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zeo-discover-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        dir
    }

    /// The memo answers the same thing the walk does. A stale or mis-keyed
    /// memo would show here as a package list that lost a name.
    #[test]
    fn a_second_discovery_answers_the_same_packages() {
        let root = scratch("same");
        gem_dir(&root, "alpha", "1.0.0");
        gem_dir(&root, "beta", "2.0.0");
        let dirs = vec![root.clone()];

        let first = discover_packages(&dirs, &[]).expect("discovery succeeds");
        let second = discover_packages(&dirs, &[]).expect("discovery succeeds");
        let names = |gems: &[Gem]| gems.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&first), vec!["alpha", "beta"]);
        assert_eq!(names(&first), names(&second));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A gem that appears BETWEEN two compiles in one process is still
    /// found. This is what the directory mtime is in the key for: `zeo gem
    /// install` writes a gem and then compiles, in one process.
    #[test]
    fn a_gem_added_after_the_first_discovery_is_still_found() {
        let root = scratch("added");
        gem_dir(&root, "alpha", "1.0.0");
        let dirs = vec![root.clone()];

        let before = discover_packages(&dirs, &[]).expect("discovery succeeds");
        assert_eq!(before.len(), 1, "one gem to start");

        gem_dir(&root, "beta", "2.0.0");
        let after = discover_packages(&dirs, &[]).expect("discovery succeeds");
        assert_eq!(
            after.iter().map(|g| g.name.clone()).collect::<Vec<_>>(),
            vec!["alpha", "beta"],
            "the memo served a list that predates the new gem"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
