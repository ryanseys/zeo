//! `Gemfile.lock` and the committed library trees describe the same gems and
//! must agree about their versions. Two formats for one fact, and a fact
//! written twice drifts.

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
/// that no repository publishes. zeo ships them the same way -- a `lib/` and
/// nothing else -- so they carry no version to compare.
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

/// Every library the compiler ships and the version its gemspec claims, or
/// `None` where it has no gemspec. zeo's own Ruby halves sit beside the Rust
/// that implements them and the vendored copies in `gems/`, so both tiers are
/// walked -- the same two the loader reads (`bundled_gems_dirs`).
fn bundled_versions() -> BTreeMap<String, Option<String>> {
    let root = repo_root();
    let mut out = BTreeMap::new();
    for tier in ["crates/zeo-rt/ext", "gems"] {
        let dir = root.join(tier);
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{tier} is readable: {e}")) {
            let path: PathBuf = entry.expect("readable entry").path();
            if !path.join("lib").is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .expect("a directory name")
                .to_string_lossy()
                .to_string();
            let spec = path.join(format!("{name}.gemspec"));
            let version = spec.is_file().then(|| {
                let text = std::fs::read_to_string(&spec)
                    .unwrap_or_else(|e| panic!("{} is readable: {e}", spec.display()));
                text.lines()
                    .find_map(|l| l.split_once("s.version").and_then(|(_, r)| r.split('"').nth(1)))
                    .unwrap_or_else(|| panic!("{} states no s.version", spec.display()))
                    .to_string()
            });
            out.entry(name).or_insert(version);
        }
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
    let mismatched: Vec<String> = bundled_versions()
        .into_iter()
        .filter_map(|(dir, v)| v.map(|v| (dir, v)))
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
    let bundled = bundled_versions();
    let stale: Vec<&str> = NO_SOURCE
        .iter()
        .chain(RENAMED.iter().map(|(d, _)| d))
        .chain(COVERED_BY.iter().map(|(d, _)| d))
        .copied()
        .filter(|d| !bundled.contains_key(*d))
        .collect();
    assert!(
        stale.is_empty(),
        "these names have no library directory any more, so drop them from \
         the tables in this file: {stale:?}"
    );
}

/// Every directory in either tier is a real library: it has a `lib/`, a
/// gemspec, or Rust at `ext/<name>/src/`. Anything else is a leftover -- a
/// half-finished move, or a directory holding nothing but a `.DS_Store`,
/// both of which the compiler would silently skip.
#[test]
fn no_library_tier_holds_a_leftover_directory() {
    let root = repo_root();
    let mut strays: Vec<String> = Vec::new();
    for tier in ["crates/zeo-rt/ext", "gems"] {
        let dir = root.join(tier);
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{tier} is readable: {e}")) {
            let path = entry.expect("readable entry").path();
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
    let versionless: Vec<String> = bundled_versions()
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
