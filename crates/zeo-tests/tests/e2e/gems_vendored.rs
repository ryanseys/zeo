//! rubygems and bundler: the two largest vendored gems, and the only pair that
//! ships from ONE upstream repo (`rubygems/rubygems`, bundler under its
//! `bundler/` subdirectory).
//!
//! The behavioural proof is the ruby-oracle golden pair -- `tests/gems/rubygems.rb`
//! and `tests/gems/bundler.rb`, which build real binaries and diff their output.
//! What lives here is everything cheaper than that: the vendoring is intact,
//! the whole require graph still reaches codegen, the classes that matter
//! survive it, and the disclosure record tells the truth about all of it. Those
//! are the failures that would otherwise show up only as a 30-minute `rustc`
//! run ending in a diff.

use std::path::{Path, PathBuf};

fn repo(rel: &str) -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join(rel)
}

/// The `[gems.<name>]` block's key/value pairs, in file order.
fn manifest_block(name: &str) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(repo("gems.toml")).expect("gems.toml is readable");
    text.lines()
        .skip_while(|l| l.trim() != format!("[gems.{name}]"))
        .skip(1)
        .take_while(|l| !l.trim().starts_with('['))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect()
}

fn manifest_value(name: &str, key: &str) -> String {
    manifest_block(name)
        .into_iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("gems.toml [gems.{name}] has no `{key}`"))
        .1
}

#[test]
fn both_gems_are_pinned_to_the_one_upstream_repo() {
    // Two manifest entries, one repo: bundler is not separately released, and
    // pinning it to a different rev than the rubygems it rides with is how the
    // pair silently drifts out of step.
    assert_eq!(manifest_value("rubygems", "github"), "rubygems/rubygems");
    assert_eq!(manifest_value("bundler", "github"), "rubygems/rubygems");
    // `subdir` is what lets one repo ship two gems -- without it `gem sync`
    // vendors rubygems' `lib/` twice and bundler is simply absent.
    assert_eq!(manifest_value("bundler", "subdir"), "bundler");
    assert!(
        !manifest_block("rubygems")
            .iter()
            .any(|(k, _)| k == "subdir"),
        "rubygems is the repo root; a subdir there would vendor the wrong tree"
    );
    for gem in ["rubygems", "bundler"] {
        assert_eq!(
            manifest_value(gem, "rev").len(),
            40,
            "{gem} rev is a full SHA"
        );
    }
}

#[test]
fn the_vendored_trees_carry_the_files_their_requires_name() {
    // A `gem sync` that silently vendored an empty or wrong tree still leaves
    // `gems/<name>/` behind, so the check has to name real entry points.
    for rel in [
        "gems/rubygems/lib/rubygems.rb",
        "gems/rubygems/lib/rubygems/version.rb",
        "gems/rubygems/lib/rubygems/requirement.rb",
        "gems/rubygems/lib/rubygems/specification.rb",
        "gems/rubygems/lib/rubygems/platform.rb",
        // The vendored-inside-the-vendored tree: rubygems carries its own
        // copies of timeout/uri/net-http under `Gem::`, and the `::Gem::
        // Timeout::Error` superclass in it is what the anchored-path
        // forward-shell fix exists for.
        "gems/rubygems/lib/rubygems/vendor/timeout/lib/timeout.rb",
        "gems/bundler/lib/bundler.rb",
        "gems/bundler/lib/bundler/lockfile_parser.rb",
        "gems/bundler/lib/bundler/dependency.rb",
        "gems/bundler/lib/bundler/version.rb",
    ] {
        assert!(repo(rel).is_file(), "missing vendored file: {rel}");
    }
    // The gemspec `xtask gem` writes is what makes the directory a package.
    for rel in [
        "gems/rubygems/rubygems.gemspec",
        "gems/bundler/bundler.gemspec",
    ] {
        assert!(repo(rel).is_file(), "missing stub gemspec: {rel}");
    }
}

#[test]
fn the_vendored_versions_agree_with_their_manifest_tags() {
    // `Gem::VERSION`/`Bundler::VERSION` are the goldens' own sanity anchors, so
    // a tag that no longer matches the tree it vendored is worth catching here
    // rather than as a mystery diff.
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
    let tag_version = |gem: &str| {
        let tag = manifest_value(gem, "tag");
        tag.rsplit_once('v')
            .expect("a v-prefixed tag")
            .1
            .to_string()
    };
    assert_eq!(
        version_in("gems/rubygems/lib/rubygems.rb"),
        tag_version("rubygems")
    );
    assert_eq!(
        version_in("gems/bundler/lib/bundler/version.rb"),
        tag_version("bundler")
    );
}

/// One compile of the whole graph, shared by the tests below: `require
/// "rubygems"` alone pulls hundreds of files through parse, analyze and
/// codegen, so the suite pays for it once and asserts several things about
/// the one result.
fn compiled_graph() -> &'static (String, String) {
    use std::sync::OnceLock;
    static ONCE: OnceLock<(String, String)> = OnceLock::new();
    ONCE.get_or_init(|| {
        let report =
            std::env::temp_dir().join(format!("zeo-vendored-gems-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&report);
        let opts = zeo::CompileOptions {
            gem_report: Some(report.clone()),
            ..Default::default()
        };
        let out = zeo::compile_to_rust_with(
            "require \"rubygems\"\nrequire \"bundler\"\np Gem::Version.new(\"1.0\").to_s\n",
            &opts,
        )
        .expect("rubygems + bundler reach codegen");
        let json = std::fs::read_to_string(&report).expect("report was written");
        let _ = std::fs::remove_file(&report);
        (out.rust_source, json)
    })
}

#[test]
fn the_whole_require_graph_reaches_codegen() {
    let (rust, _) = compiled_graph();
    // Reaching codegen is not enough on its own: a class the analyze walk never
    // registered is dropped silently, and the program still "compiles". Each
    // name below is one the goldens then exercise, spelled as `ruby_class!`
    // records it.
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
            rust.contains(&format!("{class:?}")),
            "generated program has no class named {class}"
        );
    }
}

#[test]
fn a_class_with_its_own_self_new_keeps_the_wrapper() {
    // `Gem::Package::TarWriter.new(io) { |tar| ... }` is a `def self.new` that
    // yields and closes. Routing `.new` past it would drop the block on the
    // floor, which compiles fine and writes an empty gem -- so assert the
    // class-method channel is what the generated program calls.
    let (rust, _) = compiled_graph();
    assert!(
        rust.contains("__cm_new"),
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
