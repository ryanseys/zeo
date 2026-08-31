//! `Gemfile.lock` and the committed library trees describe the same gems and
//! must agree about their versions. Two formats for one fact, and a fact
//! written twice drifts.
//!
//! Most of that drift is gone by construction now: 52 vendored trees left the
//! repo and are resolved out of `vendor/bundle` at the version the lock
//! states, so there is no second number to disagree with. What is left is the
//! committed tiers -- rubygems/bundler under `lib/ruby/`, and zeo's own halves
//! under `crates/zeo-rt/ext/` -- plus the store itself, which is a directory
//! that can quietly fall behind the lock that named it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the workspace root")
}

/// Names ruby 4.0.6 carries as plain ext/lib with no gemspec anywhere, and
/// that no repository publishes. zeo ships them the same way -- a `lib/` and
/// nothing else -- so they carry no version to compare.
const NO_SOURCE: &[&str] = &["monitor", "pty", "socket"];

/// Libraries zeo REIMPLEMENTS in Rust rather than vendors.
///
/// The equality this file checks rests on a premise -- that the tree and the
/// lock describe the same code -- and for these that premise is false. Their
/// directory holds zeo's own implementation, so its gemspec version is a
/// claim about how much of that release zeo MATCHES, which only moves when
/// the Rust does. The lock meanwhile names the latest release, which is what
/// a project resolving these from its own store gets (`Loader::builtin_wins`)
/// and what `ZEO_DISABLE_BUILTIN` makes it possible to run.
///
/// Exempt from the equality, NOT from the lock: each still has to appear
/// there, so a name that stops being released is still caught. Which of these
/// zeo keeps is task #116; every one that goes drops a row from here.
const REIMPLEMENTED: &[&str] = &["psych", "strscan"];

/// Every `name (version)` in the lock's `specs:` block. A gem resolved for
/// several platforms appears once per platform with the platform appended to
/// the version, so the value is a list.
fn locked_versions() -> BTreeMap<String, Vec<String>> {
    let text = std::fs::read_to_string(repo_root().join("Gemfile.lock"))
        .expect("Gemfile.lock is committed");
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut in_specs = false;
    for line in text.lines() {
        if line.trim_end() == "  specs:" {
            in_specs = true;
            continue;
        }
        if in_specs && !line.starts_with("    ") {
            in_specs = false;
        }
        // Six spaces is a dependency OF the entry above it, not an entry.
        if !in_specs || line.starts_with("      ") {
            continue;
        }
        let entry = line.trim();
        let Some((name, rest)) = entry.split_once(" (") else {
            continue;
        };
        let version = rest.trim_end_matches(')');
        out.entry(name.to_string())
            .or_default()
            .push(version.to_string());
    }
    assert!(!out.is_empty(), "no specs parsed out of Gemfile.lock");
    out
}

/// Every library the repo COMMITS and the version its gemspec claims, or
/// `None` where it has no gemspec. Two tiers: zeo's own Ruby halves beside
/// the Rust that implements them, and the rubygems/bundler bootstrap.
///
/// The resolved tier is deliberately absent -- its version comes from the
/// lock and is written nowhere else, which is the whole point.
fn committed_versions() -> BTreeMap<String, Option<String>> {
    let root = repo_root();
    let mut out = BTreeMap::new();
    for tier in [zeo::bundled::EXT_TIER, zeo::bundled::BOOTSTRAP_TIER] {
        for lib in zeo::bundled::libraries_in(&root.join(tier)) {
            let spec = lib.dir.join(format!("{}.gemspec", lib.name));
            let version = spec.is_file().then(|| {
                let text = std::fs::read_to_string(&spec)
                    .unwrap_or_else(|e| panic!("{} is readable: {e}", spec.display()));
                text.lines()
                    .find_map(|l| l.split_once("s.version").and_then(|(_, r)| r.split('"').nth(1)))
                    .unwrap_or_else(|| panic!("{} states no s.version", spec.display()))
                    .to_string()
            });
            out.entry(lib.name).or_insert(version);
        }
    }
    out
}

/// The lock entry a committed directory is measured against, where the two
/// are spelled differently.
fn locked_name(dir: &str) -> Option<&'static str> {
    zeo::bundled::BOOTSTRAP_LOCK_NAMES
        .iter()
        .find(|(d, _)| *d == dir)
        .map(|(_, locked)| *locked)
}

#[test]
fn every_committed_gem_matches_the_locked_version() {
    let locked = locked_versions();
    let mismatched: Vec<String> = committed_versions()
        .into_iter()
        .filter_map(|(dir, v)| v.map(|v| (dir, v)))
        .filter_map(|(dir, committed)| {
            let name = locked_name(&dir).unwrap_or(&dir);
            let Some(versions) = locked.get(name) else {
                return Some(format!("{dir}: no `{name}` in Gemfile.lock"));
            };
            // Present in the lock is all a reimplemented library owes: its
            // version is a claim about zeo's Rust, not about a vendored copy.
            if REIMPLEMENTED.contains(&name) {
                return None;
            }
            // A platform-specific row appends its platform to the version, so
            // compare against the plain one and the prefixed ones alike.
            let agrees = versions
                .iter()
                .any(|v| *v == committed || v.starts_with(&format!("{committed}-")));
            (!agrees).then(|| {
                format!(
                    "{dir}: gemspec says {committed}, Gemfile.lock says {}",
                    versions.join(", ")
                )
            })
        })
        .collect();
    assert!(
        mismatched.is_empty(),
        "the lock and the committed trees disagree; re-run `bundle lock` or \
         re-vendor:\n{}",
        mismatched.join("\n")
    );
}

/// The store `bundle install` wrote holds every library the lock says zeo
/// ships. A stale `vendor/bundle` is otherwise silent: the loader skips a
/// library whose directory is missing, so a compile just fails to find a
/// feature it should have, and a golden with a committed `.expected` never
/// even asks the oracle.
#[test]
fn every_resolved_gem_is_unpacked_in_the_store() {
    let root = repo_root();
    let resolved = zeo::bundled::resolved_libraries(root);
    let missing: Vec<String> = zeo::bundled::vendored_names(root)
        .into_iter()
        .filter(|(name, _)| !resolved.iter().any(|lib| &lib.name == name))
        .map(|(name, version)| format!("{name} {version}"))
        .collect();
    assert!(
        missing.is_empty(),
        "vendor/bundle is behind Gemfile.lock -- run `make deps`. \
         Missing: {missing:?}"
    );
}

/// A withheld library is still PINNED, so the oracle resolves the same
/// release and a name that stops being published is still caught. An entry
/// naming nothing is an exemption that outlived its reason.
#[test]
fn every_withheld_library_is_still_locked() {
    let locked = locked_versions();
    let stale: Vec<&str> = zeo::bundled::NOT_SHIPPED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !locked.contains_key(*name))
        .collect();
    assert!(
        stale.is_empty(),
        "withheld but no longer in Gemfile.lock, so drop them from \
         `zeo::bundled::NOT_SHIPPED`: {stale:?}"
    );
    let shipped = zeo::bundled::vendored_names(repo_root());
    for (name, _) in zeo::bundled::NOT_SHIPPED {
        assert!(
            !shipped.iter().any(|(n, _)| n == name),
            "{name} is withheld and must not be part of the payload"
        );
    }
}

#[test]
fn every_exemption_still_names_a_committed_library() {
    // An exemption that outlives the directory it excuses is how a real
    // mismatch goes unnoticed, so each table entry has to still apply.
    let committed = committed_versions();
    let stale: Vec<&str> = NO_SOURCE
        .iter()
        .chain(REIMPLEMENTED.iter())
        .chain(
            zeo::bundled::BOOTSTRAP_LOCK_NAMES
                .iter()
                .map(|(dir, _)| dir),
        )
        .copied()
        .filter(|d| !committed.contains_key(*d))
        .collect();
    assert!(
        stale.is_empty(),
        "these names have no library directory any more, so drop them from \
         the tables in this file: {stale:?}"
    );
}

/// Every directory in either committed tier is a real library: it has a
/// `lib/`, a gemspec, or Rust at `ext/<name>/src/`. Anything else is a
/// leftover -- a half-finished move, or a directory holding nothing but a
/// `.DS_Store`, both of which the compiler would silently skip.
#[test]
fn no_library_tier_holds_a_leftover_directory() {
    let root = repo_root();
    let mut strays: Vec<String> = Vec::new();
    for tier in [zeo::bundled::EXT_TIER, zeo::bundled::BOOTSTRAP_TIER] {
        let dir = root.join(tier);
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{tier} is readable: {e}")) {
            let path: PathBuf = entry.expect("readable entry").path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .expect("a directory name")
                .to_string_lossy()
                .to_string();
            let real = path.join("lib").is_dir()
                || path.join(format!("{name}.gemspec")).is_file()
                || path.join("ext").join(&name).join("src").is_dir();
            if !real {
                strays.push(format!("{tier}/{name}"));
            }
        }
    }
    assert!(
        strays.is_empty(),
        "these directories are not libraries, so nothing will ever load them: \
         {strays:?}"
    );
}

/// The versionless set is closed. A gemspec appearing beside one of these
/// would put a number nothing can check back into the tree, which is what
/// `socket 0.7.1`, `pty 0.5.9` and `monitor 0.1.0` were.
#[test]
fn only_the_no_source_names_ship_without_a_gemspec() {
    let versionless: Vec<String> = committed_versions()
        .into_iter()
        .filter(|(_, v)| v.is_none())
        .map(|(name, _)| name)
        .collect();
    assert_eq!(versionless, NO_SOURCE);
}

#[test]
fn the_gemfile_pins_every_dependency_to_one_version() {
    // A requirement the resolver is free to move is a version nothing in the
    // tree records, which is the shape this whole migration removes.
    let text = std::fs::read_to_string(repo_root().join("Gemfile.lock"))
        .expect("Gemfile.lock is committed");
    let deps: Vec<&str> = text
        .lines()
        .skip_while(|l| l.trim_end() != "DEPENDENCIES")
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .map(str::trim)
        .collect();
    assert!(!deps.is_empty(), "no DEPENDENCIES parsed out of Gemfile.lock");
    let loose: Vec<&&str> = deps
        .iter()
        .filter(|d| !d.contains("(= ") || !d.ends_with(')'))
        .collect();
    assert!(loose.is_empty(), "unpinned Gemfile entries: {loose:?}");
}

/// The `:oracle` group is what keeps rspec out of the payload, and it is read
/// by a line scan. A group spelled some other way -- `group :oracle, :dev do`
/// on its own line is fine, but a one-line `group(:oracle) { ... }` is not --
/// would silently ship four gems zeo does not implement.
#[test]
fn the_oracle_group_is_still_a_block_the_scan_can_read() {
    let text =
        std::fs::read_to_string(repo_root().join("Gemfile")).expect("the Gemfile is committed");
    let opens = text
        .lines()
        .filter(|l| l.trim_start().starts_with("group ") && l.contains(":oracle"))
        .count();
    assert_eq!(opens, 1, "expected exactly one `group ... :oracle` line");
    assert!(
        text.lines().any(|l| l.trim() == "end"),
        "the group block has no closing `end` on a line of its own"
    );
    let root = repo_root();
    let shipped = zeo::bundled::vendored_names(root);
    for gem in ["rspec", "rspec-core", "diff-lcs"] {
        assert!(
            !shipped.iter().any(|(name, _)| name == gem),
            "{gem} is oracle-only and must not be part of the payload"
        );
    }
}
