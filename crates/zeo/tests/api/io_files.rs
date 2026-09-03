use crate::support::{compile_packages, run_ruby_packages};

// `case/in` pattern matching. Every test below was oracle-verified
// against real `ruby` first, per this project's established convention.

#[test]
fn pathless_require_relative_cannot_infer_basepath() {
    // `compile_to_rust` (no input path) mirrors CRuby's eval/irb context:
    // require_relative has no requiring-file directory to resolve against.
    let err = zeo::check_program("require_relative \"x\"\n").unwrap_err();
    assert!(
        err.contains("cannot infer basepath"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_gem_can_override_require_paths_to_a_flat_layout() {
    let result = run_ruby_packages(
        &[
            (
                "packages/flat/flat.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"flat\"\n  s.version = \"1.0.0\"\n  s.require_paths = [\".\"]\nend\n",
            ),
            ("packages/flat/flat.rb", "FLAT = \"flat pkg\"\n"),
            ("main.rb", "require \"flat\"\nputs FLAT\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "flat pkg\n");
}

#[test]
fn dash_i_roots_shadow_packages_and_earlier_package_dirs_shadow_later_ones() {
    // CRuby's own ordering: -I beats even default gems; and one package
    // NAME resolves to exactly one package, nearest package-dir first.
    let result = run_ruby_packages(
        &[
            ("override/dual.rb", "puts \"from -I root\"\n"),
            (
                "projpkgs/thing/thing.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"thing\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "projpkgs/thing/lib/thing.rb",
                "puts \"thing from projpkgs\"\n",
            ),
            (
                "bundledpkgs/thing/thing.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"thing\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "bundledpkgs/thing/lib/thing.rb",
                "puts \"thing from bundledpkgs\"\n",
            ),
            (
                "bundledpkgs/dual/dual.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"dual\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "bundledpkgs/dual/lib/dual.rb",
                "puts \"dual from package\"\n",
            ),
            ("main.rb", "require \"dual\"\nrequire \"thing\"\n"),
        ],
        "main.rb",
        &["override"],
        &["projpkgs", "bundledpkgs"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from -I root\nthing from projpkgs\n");
}

#[test]
fn a_directory_without_a_manifest_is_not_a_gem() {
    // No `.gemspec` -> the directory is ignored entirely, so `require "plain"`
    // resolves to nothing. Under whole-program AOT that is not a compile error:
    // the unresolvable require lowers to a runtime `Kernel#require` (raising
    // LoadError), so it COMPILES -- `plain.rb` is never spliced (it isn't a gem).
    assert!(
        compile_packages(
            &[
                ("packages/plain/lib/plain.rb", "puts 1\n"),
                ("main.rb", "require \"plain\"\n"),
            ],
            "main.rb",
            &[],
            &["packages"],
        )
        .is_ok(),
        "a require of a non-gem directory should compile (defers to runtime)"
    );
}

// ---- Wave-1 in-tree extensions (stringio/strscan/cgi/digest) + json/yaml/zlib ----

// --- plan P-B: the core classes (File, Dir, Time, Process, ENV) ------------

/// A LITERAL require whose feature can't be found is a clean compile error
/// even off top level (a RESOLVABLE feature is spliced there instead).
#[test]
fn require_of_a_missing_feature_off_top_level_defers_to_runtime() {
    // A plain `require` of an unresolvable feature -- anywhere, including a
    // non-top-level position -- lowers to a runtime `Kernel#require` (raising
    // LoadError), so it COMPILES rather than failing the build.
    assert!(
        zeo::check_program("if true\n  require \"some_lib\"\nend\n").is_ok(),
        "a missing require off top level should compile (defers to runtime)"
    );
}

// --- Binary output fidelity (print/puts/putc/write/<<) ----------------
//
// The display pipeline used to promote a BINARY string's high bytes to
// UTF-8 on the way to the fd (`0xB4` -> `0xC2 0xB4`), corrupting
// bm_ao_render's and bm_so_mandelbrot's image output byte streams. The
// whole print family now accumulates and writes RAW bytes; these tests
// pin the byte streams through a File round-trip (assertions stay ASCII
// via `bytes`), all outputs oracle-verified.
