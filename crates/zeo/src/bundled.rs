//! Which libraries the compiler ships, and where each one's tree is.
//!
//! One answer, three tiers, highest precedence first:
//!
//! | Tier | Dev tree | What it holds |
//! |---|---|---|
//! | zeo's own | `crates/zeo-rt/ext/<name>/` | a Ruby half beside the Rust that implements it |
//! | bootstrap | `vendor/ruby/<name>/` | rubygems and bundler, at the pinned tag |
//! | resolved | `vendor/gems/gems/<name>-<version>/` | every other library, from `Gemfile.lock` |
//!
//! The third tier is why this module exists. zeo used to commit 52 vendored
//! upstream trees under `gems/`, which meant a gem's version was written in
//! two places -- the tree's gemspec and the lock the ruby oracle resolves --
//! and a fact written twice drifts. Now the lock is the only writer, so a
//! golden can no longer record a difference between two library versions and
//! call it a zeo bug.
//!
//! Neither of the two lower tiers is committed. `cargo xtask deps` writes
//! both, and it needs no ruby to do it: the bootstrap pair comes from the
//! rubygems repo at the tag `crates/xtask/rubygems.lock` pins, and every
//! other library from the `.gem` the lock names, verified against the lock's
//! own checksum. That is what lets a machine with no ruby on it still reach a
//! working `zeo bundle install` -- the pair cannot come out of the store that
//! zeo's own bundler fills.
//!
//! An installed or `cargo install`ed zeo has ONE directory, because `dist`
//! and `stage-publish` flatten all three into `share/zeo/lib/ruby/`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// One shipped library: the name a `require` resolves through, and the
/// directory holding its `lib/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub name: String,
    pub dir: PathBuf,
    /// Where the gemspec is, when it is not inside [`Self::dir`].
    ///
    /// A RubyGems store keeps the two apart, and the difference is not
    /// cosmetic: the unpacked `gems/<name>-<version>/<name>.gemspec` is the
    /// gem's SOURCE gemspec, which computes its own name and reads its
    /// version out of a file, while `specifications/<name>-<version>.gemspec`
    /// is RubyGems' serialized form -- the closed grammar zeo parses
    /// statically. Reading the wrong one refuses the gem.
    pub gemspec: Option<PathBuf>,
}

/// zeo's own libraries, relative to the repo root.
pub const EXT_TIER: &str = "crates/zeo-rt/ext";
/// The zeo-AUTHORED pure-Ruby gem tier: one STANDARD gem directory per name
/// (`<name>.gemspec` + `lib/`), served when the matching Rust ext is absent
/// from the build -- the dual-build switch's other half. Only for ports
/// whose content matches NO published release (a StringScanner port over
/// zeo's regex engine); a verbatim official pure gem (`base64`) rides the
/// lock instead and resolves from the store like any bundled gem.
pub const PURE_TIER: &str = "crates/zeo-rt/gems";
/// rubygems and bundler, written by `cargo xtask deps`.
pub const BOOTSTRAP_TIER: &str = "vendor/ruby";
/// The RubyGems store `cargo xtask deps` unpacks the lock into. Flat: one
/// store for one lock, with no ruby ABI level, because nothing here is
/// installed for a ruby.
pub const RESOLVED_TIER: &str = "vendor/gems";

/// A bootstrap directory and the `Gemfile.lock` entry that states its
/// version. One release ships both: `rubygems-update` is the only published
/// gem carrying the `lib/rubygems/**` tree, and it carries `bundler/`
/// complete at the same version.
///
/// Read in both directions -- it keeps the resolved tier from ALSO supplying
/// `rubygems-update` (whose `require_paths` is deliberately not `lib`, so it
/// would contribute nothing but a name collision), and it tells the version
/// check which lock entry a committed tree is measured against.
pub const BOOTSTRAP_LOCK_NAMES: &[(&str, &str)] = &[
    ("bundler", "rubygems-update"),
    ("rubygems", "rubygems-update"),
];

/// Locked libraries the compiler resolves but does NOT ship, each with the
/// reason. Pinned in the lock all the same, so the oracle resolves the same
/// release and a name that stops being published is still caught.
///
/// `every_withheld_library_is_still_locked` holds the list to real entries.
pub const NOT_SHIPPED: &[(&str, &str)] = &[(
    "rbs",
    "rbs is a C-extension gem: `require \"rbs\"` opens by loading \
     `rbs_extension`, which the bundled tier has no way to build, so the \
     Ruby half cannot run. Shipping it is pure cost -- it also carries a \
     computed require, so every program that touches rdoc drags all 850 of \
     its files in for nothing. (Those files no longer LEAK: a unit's `def`s \
     are concealed until the unit runs. The extension is what still \
     stands.)",
)];

/// Every library the dev tree ships, in precedence order: zeo's own first, so
/// a name both tiers carry resolves to zeo's implementation.
///
/// A resolved-tier gem whose directory is missing is SKIPPED rather than
/// fatal -- a fresh clone has no store until `cargo xtask deps` runs, and a
/// compile that needs none of those libraries should still work.
/// `checks::gem_versions` asserts the set is complete, so a stale store is
/// loud in the suite rather than silent in a compile.
pub fn dev_tree_libraries(root: &Path) -> Vec<Library> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    // An ext-tier library whose Rust half this BUILD did not compile is
    // dropped: its `lib/` is the Ruby half OF that Rust and requires the
    // native module beside it. The pure tier below answers the name
    // instead, when it holds the gem.
    for lib in libraries_in(&root.join(EXT_TIER)) {
        if ext_build_carried(&lib.name) && seen.insert(lib.name.clone()) {
            out.push(lib);
        }
    }
    // The pure tier serves ONLY names whose Rust half is absent. A carried
    // builtin must not see a file-backed twin: the dual-homed machinery
    // would read the pair as tmpdir's shape and splice the file over the
    // native implementation.
    for lib in libraries_in(&root.join(PURE_TIER)) {
        if !ext_build_carried(&lib.name) && seen.insert(lib.name.clone()) {
            out.push(lib);
        }
    }
    for lib in libraries_in(&root.join(BOOTSTRAP_TIER)) {
        if seen.insert(lib.name.clone()) {
            out.push(lib);
        }
    }
    for lib in resolved_libraries(root) {
        if seen.insert(lib.name.clone()) {
            out.push(lib);
        }
    }
    out
}

/// Whether this build compiled the Rust half behind the ext-tier library
/// `name`. A directory whose feature names no gated ABI class (`fiddle`,
/// pure Ruby over zeo's ffi) is always carried. The dir name maps to the
/// require spelling by the same rule `ZEO_DISABLE_BUILTIN` uses.
fn ext_build_carried(name: &str) -> bool {
    let feature = name.replace('-', "/");
    crate::lower::features::build_carries_ext(&feature)
}

/// The `<name>/` directories under one name-keyed tier, name-sorted. A
/// directory with no `lib/` is not a library -- under `ext/` that skips every
/// extension whose Rust needs no Ruby half.
pub fn libraries_in(dir: &Path) -> Vec<Library> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Library> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("lib").is_dir())
        .map(|dir| Library {
            name: dir
                .file_name()
                .expect("a directory name")
                .to_string_lossy()
                .into_owned(),
            dir,
            gemspec: None,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The resolved tier: every gem [`vendored_names`] names, at the version the
/// lock states, in the store `cargo xtask deps` unpacked. Missing directories
/// are dropped (see [`dev_tree_libraries`]).
pub fn resolved_libraries(root: &Path) -> Vec<Library> {
    let store = root.join(RESOLVED_TIER);
    let gems = store.join("gems");
    let specs = store.join("specifications");
    vendored_names(root)
        .into_iter()
        .map(|(name, version)| Library {
            dir: gems.join(format!("{name}-{version}")),
            gemspec: Some(specs.join(format!("{name}-{version}.gemspec"))),
            name,
        })
        .filter(|lib| {
            lib.dir.join("lib").is_dir() && lib.gemspec.as_ref().is_some_and(|spec| spec.is_file())
        })
        .collect()
}

/// `<root>/vendor/bundle/ruby/<abi>`: a BUNDLER-shaped store, which is what a
/// user's project has and what `cargo xtask deps --oracle` writes for the
/// ruby oracle. zeo's own resolved tier is [`RESOLVED_TIER`] and does not go
/// through here.
///
/// The ABI directory is named for the ruby that installed it, and there is
/// exactly one; it is discovered rather than spelled out so a ruby bump does
/// not need an edit here as well.
pub fn store_dir(root: &Path) -> Option<PathBuf> {
    let mut abis: Vec<PathBuf> = std::fs::read_dir(root.join("vendor/bundle/ruby"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("gems").is_dir())
        .collect();
    abis.sort();
    abis.pop()
}

/// Every gem the compiler ships from the store, `(name, version)`,
/// name-sorted: the transitive closure of the Gemfile's own requests, less
/// the `:oracle` group, less every name a higher tier already supplies, less
/// [`NOT_SHIPPED`].
///
/// The group is read from the Gemfile because Bundler writes no group into
/// the lock. Only the closure needs the lock, and only the lock states a
/// version, so the two files answer the halves each one owns.
pub fn vendored_names(root: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join("Gemfile.lock")) else {
        return Vec::new();
    };
    let Ok(lock) = zeo_gem::Lockfile::parse(&text) else {
        return Vec::new();
    };
    let oracle_only = oracle_group(root);
    let mut supplied: BTreeSet<String> = [EXT_TIER, BOOTSTRAP_TIER]
        .iter()
        .flat_map(|tier| libraries_in(&root.join(tier)))
        .map(|lib| lib.name)
        .collect();
    // A carried extension with NO Ruby half (`base64`) supplies its name
    // with no `lib/` for the scan above to see -- and the pure tier
    // supplies the flip side when the Rust half is absent. Without these
    // rows the resolved store's copy of a locked gem would read as
    // dual-homed and splice over the native implementation.
    if let Ok(entries) = std::fs::read_dir(root.join(EXT_TIER)) {
        supplied.extend(
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| ext_build_carried(n)),
        );
    }
    supplied.extend(
        libraries_in(&root.join(PURE_TIER))
            .into_iter()
            .map(|lib| lib.name)
            .filter(|n| !ext_build_carried(n)),
    );
    supplied.extend(
        BOOTSTRAP_LOCK_NAMES
            .iter()
            .map(|(_, locked)| (*locked).to_string()),
    );
    supplied.extend(NOT_SHIPPED.iter().map(|(name, _)| (*name).to_string()));

    let gems = lock.resolved();
    let by_name: BTreeMap<&str, &zeo_gem::LockedGem> =
        gems.iter().map(|g| (g.name.as_str(), g)).collect();
    let roots = lock.roots();
    let mut queue: VecDeque<&str> = roots
        .iter()
        .map(String::as_str)
        .filter(|n| !oracle_only.contains(*n))
        .collect();
    let mut reached: BTreeSet<&str> = BTreeSet::new();
    while let Some(name) = queue.pop_front() {
        if !reached.insert(name) {
            continue;
        }
        if let Some(gem) = by_name.get(name) {
            queue.extend(gem.deps.iter().map(String::as_str));
        }
    }
    reached
        .into_iter()
        .filter(|n| !supplied.contains(*n))
        .filter_map(|n| by_name.get(n).map(|g| (g.name.clone(), g.version.clone())))
        .collect()
}

/// The gem names inside the Gemfile's `group :oracle do ... end` block --
/// what only the ruby oracle resolves, and what zeo therefore does not ship.
///
/// A line scan, not a Ruby parse: this reads zeo's own Gemfile, whose shape
/// is one `gem "name", "version"` per line and one group block.
/// `every_oracle_only_gem_stays_out_of_the_payload` is what holds that shape.
fn oracle_group(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Ok(text) = std::fs::read_to_string(root.join("Gemfile")) else {
        return names;
    };
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("group ") && line.contains(":oracle") {
            inside = true;
        } else if inside && line == "end" {
            inside = false;
        } else if inside && let Some(name) = gem_line_name(line) {
            names.insert(name);
        }
    }
    names
}

/// `gem "rspec", "3.13.2"` -> `rspec`.
fn gem_line_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("gem ")?.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = &rest[1..];
    Some(body[..body.find(quote)?].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crates/zeo sits two levels under the workspace root")
            .to_path_buf()
    }

    /// The three tiers answer one list, and zeo's own implementation wins a
    /// name the resolved tier also carries. `json` is the case that matters:
    /// the lock names json 2.21.2 and zeo IS json 2.21.2, in Rust.
    #[test]
    fn zeos_own_half_wins_a_name_the_lock_also_names() {
        let libs = dev_tree_libraries(&root());
        let json = libs
            .iter()
            .find(|l| l.name == "json")
            .expect("json is a shipped library");
        assert!(
            json.dir.ends_with("crates/zeo-rt/ext/json"),
            "json resolved to {} rather than zeo's own half",
            json.dir.display()
        );
        assert_eq!(
            libs.iter().filter(|l| l.name == "json").count(),
            1,
            "one directory per name"
        );
    }

    /// rubygems and bundler come from the committed tier, never from the
    /// store: the store is what they produce.
    #[test]
    fn the_bootstrap_pair_is_committed() {
        let libs = dev_tree_libraries(&root());
        for name in ["rubygems", "bundler"] {
            let lib = libs
                .iter()
                .find(|l| l.name == name)
                .unwrap_or_else(|| panic!("{name} is a shipped library"));
            assert!(
                lib.dir.starts_with(root().join(BOOTSTRAP_TIER)),
                "{name} resolved to {}",
                lib.dir.display()
            );
        }
        assert!(
            !libs.iter().any(|l| l.name == "rubygems-update"),
            "the store's rubygems-update duplicates the committed tree under \
             a require_paths that is deliberately not `lib`"
        );
    }

    /// The `:oracle` group is documentation to Bundler and a filter here.
    #[test]
    fn the_oracle_group_ships_nothing() {
        let oracle = oracle_group(&root());
        assert!(oracle.contains("rspec"), "the group was read: {oracle:?}");
        let libs = dev_tree_libraries(&root());
        for name in oracle {
            assert!(
                !libs.iter().any(|l| l.name == name),
                "{name} is oracle-only and must not ship"
            );
        }
        // rspec's own dependencies ride out with it.
        assert!(!libs.iter().any(|l| l.name == "diff-lcs"));
    }
}
