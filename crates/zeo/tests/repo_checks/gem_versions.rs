//! `Gemfile.lock` and the committed `gems/<name>/` trees describe the same
//! gems, and must agree about their versions.
//!
//! They are two formats for one fact while the migration to the lock is under
//! way, and a fact written twice is a fact that drifts: `gems/json` claimed
//! 2.18.0 in its gemspec and 2.21.2 in its `lib/`, which made a golden
//! unreadable because neither answer was obviously the wrong one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the workspace root")
}

/// The gem name on rubygems.org, where it differs from the directory zeo
/// vendors it under.
const RENAMED: &[(&str, &str)] = &[
    // ruby's own lib/English.gemspec names it `english`; zeo's stub says
    // `English`, which no index has ever carried.
    ("English", "english"),
    // The only published gem carrying the `lib/rubygems/**` tree. Its
    // require_paths deliberately is not `lib`, so it cannot shadow the
    // running RubyGems.
    ("rubygems", "rubygems-update"),
];

/// Vendored trees whose version comes from another entry's release rather
/// than one of their own.
const COVERED_BY: &[(&str, &str)] = &[
    // `rubygems-update` ships the whole `bundler/` subtree at its own
    // version -- the same one-tag-two-gems shape upstream.rb used.
    ("bundler", "rubygems-update"),
];

/// Names ruby 4.0.6 carries as plain ext/lib with no gemspec anywhere, and
/// that no repository publishes. Their Ruby halves are zeo-authored and zeo
/// versions them itself, so there is nothing to compare against.
const NO_SOURCE: &[&str] = &["monitor", "pty", "socket"];

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

/// Every `gems/<name>/` and the version its stub gemspec claims.
fn vendored_versions() -> BTreeMap<String, String> {
    let dir = repo_root().join("gems");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("gems/ is readable") {
        let path: PathBuf = entry.expect("readable entry").path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .expect("a directory name")
            .to_string_lossy()
            .to_string();
        let spec = path.join(format!("{name}.gemspec"));
        let text = std::fs::read_to_string(&spec)
            .unwrap_or_else(|e| panic!("{} is readable: {e}", spec.display()));
        let version = text
            .lines()
            .find_map(|l| l.split_once("s.version").and_then(|(_, r)| r.split('"').nth(1)))
            .unwrap_or_else(|| panic!("{} states no s.version", spec.display()))
            .to_string();
        out.insert(name, version);
    }
    out
}

/// The lock entry a vendored directory should be read against.
fn locked_name(dir: &str) -> Option<&'static str> {
    let renamed = RENAMED.iter().find(|(d, _)| *d == dir).map(|(_, n)| *n);
    renamed.or_else(|| COVERED_BY.iter().find(|(d, _)| *d == dir).map(|(_, n)| *n))
}

#[test]
fn every_vendored_gem_matches_the_locked_version() {
    let locked = locked_versions();
    let mismatched: Vec<String> = vendored_versions()
        .into_iter()
        .filter(|(dir, _)| !NO_SOURCE.contains(&dir.as_str()))
        .filter_map(|(dir, vendored)| {
            let name = locked_name(&dir).unwrap_or(&dir);
            let Some(versions) = locked.get(name) else {
                return Some(format!("gems/{dir}: no `{name}` in Gemfile.lock"));
            };
            // A platform-specific row appends its platform to the version, so
            // compare against the plain one and the prefixed ones alike.
            let agrees = versions
                .iter()
                .any(|v| *v == vendored || v.starts_with(&format!("{vendored}-")));
            (!agrees).then(|| {
                format!(
                    "gems/{dir}: gemspec says {vendored}, Gemfile.lock says {}",
                    versions.join(", ")
                )
            })
        })
        .collect();
    assert!(
        mismatched.is_empty(),
        "the lock and the vendored trees disagree; re-run `bundle lock` or \
         re-vendor:\n{}",
        mismatched.join("\n")
    );
}

#[test]
fn every_exemption_still_names_a_vendored_gem() {
    // An exemption that outlives the directory it excuses is how a real
    // mismatch goes unnoticed, so each table entry has to still apply.
    let vendored = vendored_versions();
    let stale: Vec<&str> = NO_SOURCE
        .iter()
        .chain(RENAMED.iter().map(|(d, _)| d))
        .chain(COVERED_BY.iter().map(|(d, _)| d))
        .copied()
        .filter(|d| !vendored.contains_key(*d))
        .collect();
    assert!(
        stale.is_empty(),
        "these names have no gems/ directory any more, so drop them from the \
         tables in this file: {stale:?}"
    );
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
