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
//! The end-to-end behavioural proof is the gem probe's (the compile-side
//! inputs `tests/bench/rubygems.rb` and `tests/bench/bundler.rb` are the same
//! whole-graph programs; their golden runs cost minutes each and were retired
//! from the suite). What lives here is everything cheaper than that: the
//! vendoring is intact,
//! the whole require graph still reaches codegen, the classes that matter
//! survive it, and the disclosure record tells the truth about all of it. Those
//! are the failures that would otherwise show up only as a long whole-gem
//! compile ending in a diff.

use std::path::PathBuf;

fn repo(rel: &str) -> PathBuf {
    crate::paths::workspace_root().join(rel)
}

/// The one lock entry both committed trees are measured against.
const LOCKED_AS: &str = "rubygems-update";

/// `rubygems-update`'s version in `Gemfile.lock`.
fn locked_version() -> String {
    let text =
        std::fs::read_to_string(repo("Gemfile.lock")).expect("Gemfile.lock is committed");
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
    assert_eq!(version_in("lib/ruby/bundler/lib/bundler/version.rb"), locked);
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

/// One compile of the whole graph, shared by the tests below: `require
/// "rubygems"` alone pulls hundreds of files through parse, analyze and
/// codegen, so the suite pays for it once and asserts several things about
/// the one result.
/// `Analyzed` owns the whole `Compiler`, which is full of `RefCell`/`Cell`
/// and deliberately not `Sync`, so the memo holds the ANSWERS rather than the
/// analysis: every registered class's fully-qualified name, whether
/// `Gem::Package::TarWriter` kept its own `def self.new`, and the disclosure
/// record.
struct GraphFacts {
    classes: Vec<String>,
    tar_writer_has_own_new: bool,
}

fn compiled_graph() -> &'static (GraphFacts, String) {
    use std::sync::OnceLock;
    static ONCE: OnceLock<(GraphFacts, String)> = OnceLock::new();
    ONCE.get_or_init(|| {
        let report =
            std::env::temp_dir().join(format!("zeo-vendored-gems-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&report);
        let opts = zeo::CompileOptions {
            gem_report: Some(report.clone()),
            ..Default::default()
        };
        let out = zeo::analyze_program(
            "require \"rubygems\"\nrequire \"bundler\"\np Gem::Version.new(\"1.0\").to_s\n",
            &opts,
        )
        .expect("rubygems + bundler reach codegen");
        let json = std::fs::read_to_string(&report).expect("report was written");
        let _ = std::fs::remove_file(&report);
        let ids = (0..out.compiler.classes.len()).map(|i| zeo::compiler::ClassId(i as u32));
        let classes: Vec<String> = ids.clone().map(|c| out.compiler.fq_name(c)).collect();
        let tar_writer_has_own_new = ids
            .clone()
            .find(|&c| out.compiler.fq_name(c) == "Gem::Package::TarWriter")
            .is_some_and(|c| out.compiler.lookup_class_method(c, "new").is_some());
        (
            GraphFacts {
                classes,
                tar_writer_has_own_new,
            },
            json,
        )
    })
}

#[test]
fn the_whole_require_graph_reaches_codegen() {
    let (facts, _) = compiled_graph();
    // Reaching the emitter is not enough on its own: a class the analyze walk
    // never registered is dropped silently, and the program still "compiles".
    // Each name below is one the goldens then exercise. Asked of the CLASS
    // TABLE, which is the thing that would be missing -- a substring search of
    // emitted text answers the same question far less precisely.

    for class in [
        "Gem::Version",
        "Gem::Requirement",
        "Gem::Dependency",
        "Gem::Specification",
        "Gem::Platform",
        "Gem::Timeout::Error",
        "Gem::Resolver::InstallerSet",
        "Gem::Package::TarWriter",
        "Bundler::LockfileParser",
        "Bundler::Dependency",
        "Bundler::SpecSet",
        "Bundler::FeatureFlag",
        "Bundler::ConnectionPool::TimeoutError",
    ] {
        assert!(
            facts.classes.iter().any(|n| n == class),
            "the compiler registered no class named {class}"
        );
    }
}

#[test]
fn a_class_with_its_own_self_new_keeps_the_wrapper() {
    // `Gem::Package::TarWriter.new(io) { |tar| ... }` is a `def self.new` that
    // yields and closes. Routing `.new` past it would drop the block on the
    // floor, which compiles fine and writes an empty gem -- so assert the
    // class-method channel is what the generated program calls.
    let (facts, _) = compiled_graph();
    assert!(
        facts.tar_writer_has_own_new,
        "no user `def self.new` is dispatched as a class method"
    );
}

#[test]
fn the_disclosure_record_names_both_as_faithful_bundled_gems() {
    // Neither is a zeo reimplementation -- they are the upstream trees,
    // verbatim -- so the record must NOT flag them as diverging. (`--gem-report`
    // is the honesty anchor; a wrong entry here is a wrong claim to the user.)
    let (_, json) = compiled_graph();
    for gem in ["rubygems", "bundler"] {
        let line = json
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{gem:?}:")))
            .unwrap_or_else(|| panic!("{gem} is missing from the disclosure record:\n{json}"));
        assert!(line.contains(r#""by": "bundled-gem""#), "{line}");
        assert!(
            !line.contains("diverges"),
            "{gem} must not be flagged divergent: {line}"
        );
    }
}

/// racc's C extension is a pure ACCELERATOR -- its own parser.rb carries a
/// complete Ruby runtime behind `rescue LoadError`. The store must serve the
/// gem WITHOUT building cparse (which would arm the GVL process-wide): no
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
    zeo::check_program_with("require \"racc/parser\"\nputs Racc::Parser.racc_runtime_type\n", &opts)
        .expect("racc resolves from the store without its C extension");
    let json = std::fs::read_to_string(&report).unwrap();
    let _ = std::fs::remove_file(&report);
    assert!(
        !json.lines().any(|l| l.contains("racc") && l.contains("compiled-extension")),
        "racc must not build cparse: {json}"
    );
    assert!(
        json.contains(r#""racc/parser": {"by": "bundled-gem""#),
        "racc/parser should serve from the store as plain Ruby: {json}"
    );
}
