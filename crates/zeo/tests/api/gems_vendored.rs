//! rubygems and bundler: the two libraries zeo still COMMITS, and the only
//! pair that ships from one upstream release (`rubygems-update`, which
//! carries `bundler/` complete at the same version).
//!
//! They are committed because nothing else can supply them. Every other
//! library resolves out of `vendor/bundle`, and `vendor/bundle` is what
//! `bundle install` writes -- so the bundler that runs the install cannot
//! come from the thing the install produces. A machine with no ruby on it
//! must still reach a working `zeo bundle install`.
//!
//! The end-to-end behavioural proof is the gem probe's and `bundler_parity`'s
//! (the compile-side inputs `tests/bench/rubygems.rb` and `tests/bench/
//! bundler.rb` are the same whole-graph programs; their golden runs cost
//! minutes each and were retired from the suite, and so was a shared
//! whole-graph compile that asserted class names). What lives here is only
//! what costs milliseconds: the vendoring is intact and the two trees agree
//! with the lock.

use std::path::PathBuf;

fn repo(rel: &str) -> PathBuf {
    crate::paths::workspace_root().join(rel)
}

/// The one lock entry both committed trees are measured against.
const LOCKED_AS: &str = "rubygems-update";

/// `rubygems-update`'s version in `Gemfile.lock`.
fn locked_version() -> String {
    let text = std::fs::read_to_string(repo("Gemfile.lock")).expect("Gemfile.lock is committed");
    text.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(&format!("{LOCKED_AS} (")))
        .map(|rest| rest.trim_end_matches(')').to_string())
        .unwrap_or_else(|| panic!("Gemfile.lock names no {LOCKED_AS}"))
}

#[test]
fn one_release_states_the_version_of_both_trees() {
    // bundler is not separately released, and a `gem "bundler"` line would
    // additionally force every contributor onto exactly that bundler --
    // bundler refuses to resolve a version other than the one running it. So
    // the pair is pinned once, and drifting out of step is not expressible.
    assert!(
        zeo::bundled::BOOTSTRAP_LOCK_NAMES
            .iter()
            .all(|(_, locked)| *locked == LOCKED_AS),
        "the bootstrap tier is measured against more than one lock entry"
    );
    let names: Vec<&str> = zeo::bundled::BOOTSTRAP_LOCK_NAMES
        .iter()
        .map(|(dir, _)| *dir)
        .collect();
    assert_eq!(names, ["bundler", "rubygems"]);
}

#[test]
fn the_committed_trees_carry_the_files_their_requires_name() {
    // A half-finished vendoring still leaves `lib/ruby/<name>/` behind, so the
    // check has to name real entry points.
    for rel in [
        "lib/ruby/rubygems/lib/rubygems.rb",
        "lib/ruby/rubygems/lib/rubygems/version.rb",
        "lib/ruby/rubygems/lib/rubygems/requirement.rb",
        "lib/ruby/rubygems/lib/rubygems/specification.rb",
        "lib/ruby/rubygems/lib/rubygems/platform.rb",
        // The vendored-inside-the-vendored tree: rubygems carries its own
        // copies of timeout/uri/net-http under `Gem::`, and the `::Gem::
        // Timeout::Error` superclass in it is what the anchored-path
        // forward-shell fix exists for.
        "lib/ruby/rubygems/lib/rubygems/vendor/timeout/lib/timeout.rb",
        "lib/ruby/bundler/lib/bundler.rb",
        "lib/ruby/bundler/lib/bundler/lockfile_parser.rb",
        "lib/ruby/bundler/lib/bundler/dependency.rb",
        "lib/ruby/bundler/lib/bundler/version.rb",
    ] {
        assert!(repo(rel).is_file(), "missing committed file: {rel}");
    }
    // The stub gemspec each directory carries is what makes it a package.
    for rel in [
        "lib/ruby/rubygems/rubygems.gemspec",
        "lib/ruby/bundler/bundler.gemspec",
    ] {
        assert!(repo(rel).is_file(), "missing stub gemspec: {rel}");
    }
}

#[test]
fn the_committed_versions_agree_with_the_lock() {
    // `Gem::VERSION`/`Bundler::VERSION` are the goldens' own sanity anchors,
    // and they are what the trees themselves say -- a re-vendor that took a
    // different release than the lock names is worth catching here rather
    // than as a mystery diff.
    let version_in = |rel: &str| -> String {
        let text = std::fs::read_to_string(repo(rel)).expect("readable");
        let line = text
            .lines()
            .find(|l| l.trim_start().starts_with("VERSION = "))
            .expect("a VERSION assignment");
        line.split('"')
            .nth(1)
            .expect("a quoted version")
            .to_string()
    };
    let locked = locked_version();
    assert_eq!(version_in("lib/ruby/rubygems/lib/rubygems.rb"), locked);
    assert_eq!(
        version_in("lib/ruby/bundler/lib/bundler/version.rb"),
        locked
    );
}

#[test]
fn the_bootstrap_pair_never_comes_out_of_the_store() {
    // `rubygems-update`'s require_paths is deliberately NOT `lib` -- it exists
    // so installing it cannot shadow the running RubyGems -- so a store copy
    // would contribute a name and no files.
    let libs = zeo::bundled::dev_tree_libraries(&crate::paths::workspace_root());
    for name in ["rubygems", "bundler"] {
        let lib = libs
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("{name} is not a shipped library at all"));
        assert!(
            lib.dir.starts_with(repo(zeo::bundled::BOOTSTRAP_TIER)),
            "{name} resolved to {}",
            lib.dir.display()
        );
    }
}

/// racc's C extension is a pure ACCELERATOR -- its own parser.rb carries a
/// complete Ruby runtime behind `rescue LoadError`. The store must serve the
/// gem WITHOUT building cparse (a C extension in a threaded program arms the
/// GVL): no
/// compiled-extension row, and the runtime require of `racc/cparse` stays a
/// catchable LoadError so the gem's own rescue picks the Ruby runtime.
#[test]
fn the_racc_accelerator_is_declined_and_the_gem_serves_pure_ruby() {
    let report = std::env::temp_dir().join(format!("zeo-racc-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&report);
    let opts = zeo::CompileOptions {
        gem_paths: vec![repo("vendor/bundle/ruby/4.0.0")],
        lockfile: Some(repo("Gemfile.lock")),
        gem_report: Some(report.clone()),
        ..Default::default()
    };
    zeo::check_program_with(
        "require \"racc/parser\"\nputs Racc::Parser.racc_runtime_type\n",
        &opts,
    )
    .expect("racc resolves from the store without its C extension");
    let json = std::fs::read_to_string(&report).unwrap();
    let _ = std::fs::remove_file(&report);
    assert!(
        !json
            .lines()
            .any(|l| l.contains("racc") && l.contains("compiled-extension")),
        "racc must not build cparse: {json}"
    );
    assert!(
        json.contains(r#""racc/parser": {"by": "bundled-gem""#),
        "racc/parser should serve from the store as plain Ruby: {json}"
    );
}
