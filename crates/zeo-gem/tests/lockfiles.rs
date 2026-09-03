//! Every lockfile in the repo, read and written back.
//!
//! A round-trip is the strongest statement a reader can make: nothing was
//! dropped, nothing was invented, and the section order and indentation the
//! writer used survived. The fixtures beside this file cover the shapes the
//! repo's own lock does not have -- git and path sources, a plugin source, a
//! `RUBY VERSION` section, pre-release versions, and precompiled platform rows.

use std::path::{Path, PathBuf};

use zeo_gem::lockfile::{GemSource, SectionKind};
use zeo_gem::{Lockfile, Platform};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root is above this crate")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lockfiles")
        .join(name)
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lockfiles");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the fixture directory is committed")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "lock"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {}", dir.display());
    paths
}

/// Every lockfile the repo tracks, fixtures and real ones alike.
fn every_lockfile() -> Vec<PathBuf> {
    let root = repo_root();
    let mut paths = fixtures();
    for rel in [
        "Gemfile.lock",
        "crates/zeo/tests/fixtures/gem_store/Gemfile.lock",
        "crates/zeo/tests/fixtures/gem_store/store/Gemfile.lock",
    ] {
        let path = root.join(rel);
        if path.is_file() {
            paths.push(path);
        }
    }
    paths
}

#[test]
fn every_lockfile_round_trips_byte_for_byte() {
    for path in every_lockfile() {
        let text = std::fs::read_to_string(&path).expect("a readable lockfile");
        let lock = Lockfile::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(
            lock.to_string(),
            text,
            "{} did not round-trip",
            path.display()
        );
    }
}

/// Parsing twice through the writer must reach a fixed point too -- a reader
/// that loses something on the SECOND pass is still a reader that loses
/// something.
#[test]
fn a_rewritten_lockfile_parses_to_the_same_thing() {
    for path in every_lockfile() {
        let text = std::fs::read_to_string(&path).expect("a readable lockfile");
        let once = Lockfile::parse(&text).expect("parses");
        let twice = Lockfile::parse(&once.to_string()).expect("the rewrite parses");
        assert_eq!(once, twice, "{}", path.display());
    }
}

#[test]
fn a_git_source_keeps_its_revision_and_its_gems_are_tagged() {
    let lock = Lockfile::parse_file(&fixture("git_and_path.lock")).expect("parses");
    let git: Vec<_> = lock
        .sources
        .iter()
        .filter(|s| s.kind == SectionKind::Git)
        .collect();
    assert_eq!(git.len(), 2);
    assert_eq!(
        git[0].remote(),
        Some("https://github.com/rubocop/rubocop.git")
    );
    assert!(git[0].revision().is_some_and(|r| r.len() == 40));
    assert_eq!(git[1].get("tag"), Some("v0.17"));
    assert_eq!(git[1].get("submodules"), Some("true"));

    let source = |name: &str| {
        lock.resolved()
            .into_iter()
            .find(|g| g.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .source
    };
    assert_eq!(source("rubocop"), GemSource::Git);
    assert_eq!(source("cuprite"), GemSource::Git);
    assert_eq!(source("local_gem"), GemSource::Path);
    assert_eq!(source("rake"), GemSource::Rubygems);

    // The `!` marker rides the dependency row, whatever else it carries.
    let pinned: Vec<&str> = lock
        .dependencies
        .iter()
        .filter(|d| d.pinned)
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(pinned, ["cuprite", "local_gem", "rubocop"]);
}

#[test]
fn the_row_for_this_machine_is_the_one_chosen() {
    let lock = Lockfile::parse_file(&fixture("platforms.lock")).expect("parses");
    for (host, want) in [
        ("arm64-darwin-23", "arm64-darwin"),
        ("x86_64-linux-gnu", "x86_64-linux-gnu"),
        ("aarch64-linux-gnu", "aarch64-linux"),
        // No row was built for this one, so the source row is what remains.
        ("sparc-solaris-2.11", "ruby"),
    ] {
        let spec = lock
            .spec_for("google-protobuf", &Platform::parse(host))
            .expect("a row for google-protobuf");
        assert_eq!(spec.platform.to_string(), want, "host {host}");
    }
    // The compiler's view takes the source row every time: a precompiled gem
    // ships a binary zeo cannot load.
    let resolved = lock.resolved();
    let protobuf = resolved
        .iter()
        .find(|g| g.name == "google-protobuf")
        .expect("google-protobuf is locked");
    assert_eq!(protobuf.platform, None);
    assert_eq!(protobuf.deps, ["bigdecimal", "rake"]);
    // nokogiri has no source row at all, so its only row stands.
    let nokogiri = resolved
        .iter()
        .find(|g| g.name == "nokogiri")
        .expect("nokogiri is locked");
    assert_eq!(nokogiri.platform.as_deref(), Some("arm64-darwin"));
}

#[test]
fn checksums_are_addressable_by_full_name() {
    let lock = Lockfile::parse_file(&fixture("checksums.lock")).expect("parses");
    assert_eq!(lock.checksums.len(), 3);
    assert_eq!(
        lock.checksum("ast-2.4.3").unwrap().sha256().unwrap().len(),
        32
    );
    assert!(
        lock.checksum("nokogiri-1.18.2-arm64-darwin")
            .expect("the platform row is addressed with its platform")
            .sha256()
            .is_some()
    );
    // Bundler writes a row with no digest when it has none to record.
    assert!(lock.checksum("racc-1.8.1").unwrap().sha256().is_none());
    assert!(lock.checksum("absent-1.0").is_none());
}

#[test]
fn a_prerelease_version_survives_the_read() {
    let lock = Lockfile::parse_file(&fixture("prerelease.lock")).expect("parses");
    let spec = |name: &str| {
        lock.specs()
            .map(|(_, s)| s)
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("{name} is not locked"))
    };
    let rails = spec("rails");
    assert_eq!(rails.version.as_str(), "8.1.0.beta1");
    assert!(rails.version.is_prerelease());
    assert_eq!(rails.full_name(), "rails-8.1.0.beta1");
    // A version with four numeric parts and a platform suffix splits cleanly.
    let sorbet = spec("sorbet");
    assert_eq!(sorbet.version.as_str(), "0.5.11708");
    assert_eq!(sorbet.platform.to_string(), "universal-darwin");
}

#[test]
fn a_plugin_source_is_read_and_written_like_any_other() {
    let lock = Lockfile::parse_file(&fixture("plugin_source.lock")).expect("parses");
    let plugin = lock
        .sources
        .iter()
        .find(|s| s.kind == SectionKind::Plugin)
        .expect("the plugin source is kept");
    assert_eq!(plugin.get("type"), Some("git"));
    assert_eq!(plugin.specs.len(), 1);
}

/// The repo's own lock is what `cargo xtask deps` reads. Every `GEM` spec
/// needs a checksum row, because the download is verified before anything
/// unpacks it.
#[test]
fn the_repo_lock_states_a_checksum_for_every_gem_it_resolves() {
    let lock = Lockfile::parse_file(&repo_root().join("Gemfile.lock")).expect("parses");
    let missing: Vec<String> = lock
        .specs()
        .filter(|(source, _)| source.kind == SectionKind::Gem)
        .filter(|(_, spec)| {
            lock.checksum(&spec.full_name())
                .and_then(|c| c.sha256())
                .is_none()
        })
        .map(|(_, spec)| spec.full_name())
        .collect();
    assert!(
        missing.is_empty(),
        "no sha256 in CHECKSUMS for: {}",
        missing.join(", ")
    );
    assert!(
        lock.gem_remote().is_some(),
        "no GEM remote to download from"
    );
}
