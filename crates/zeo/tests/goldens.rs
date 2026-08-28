//! Every golden corpus: compiled by zeo, run, and diffed against the
//! committed ruby-oracle `.rb.expected` (stdout AND stderr).
//!
//! **A golden's contract is its DIRECTORY**, never a per-file marker:
//!
//! - `tests/` -- zeo-authored examples; must match ruby.
//! - `tests/divergences/` -- zeo answers differently on purpose, so the
//!   golden records zeo's own output. See that directory's README.
//! - `tests/macos/` -- macOS-only; the output is platform-specific, so Linux
//!   CRuby diverges from the macOS-oracle golden exactly as zeo does.
//! - `tests/jit/` -- JIT-only; the program needs the compiler and itself in
//!   one process.
//! - `tests/gaps/` -- XFAIL: a diverging gap passes, one that starts matching
//!   ruby fails with a "promote" message (`cargo xtask promote-gap`).
//! - `tests/spinel/` -- the corpus vendored from spinel; must match ruby.
//!
//! Each pattern matches ONE level: a subdirectory holds FIXTURES, not tests.
//! `tests/bench/` is compile-side input with no goldens and stays out.
//! `tests/README.md` lays the corpus out as a 2x2 of cost against outcome.
//!
//! Two corpora cost minutes and gigabytes a case, so they raise the bounds
//! and take their own nextest deadline (`test(milestone::) | test(gemtest::)`,
//! which the default profile also opts out of):
//!
//! - `tests/milestones/` -- umbrella entry points, each a named case so a
//!   regression names itself. `pending/` is the XFAIL tier. Such a golden
//!   must print SHAPES, never versions: ruby has RubyGems loaded before line
//!   1, so its `require` answers `false` where zeo's answers `true`, and the
//!   bundled versions differ.
//! - `tests/gemtests/` -- one driver per gem requires a real upstream test
//!   file and the framework's autorun executes it. The trees are gitignored
//!   fetched into `vendor/gemtests/` on demand; an absent tree skips.
//!
//! Case names are `<fn>::<path>`, so `bless example::` / `gap::` / `spinel::`
//! / `milestone::` / `gemtest::` each name one corpus.

#[path = "harness/golden.rs"]
mod golden;
#[path = "harness/normalize.rs"]
mod normalize;
#[path = "harness/paths.rs"]
mod paths;
#[path = "harness/zeo_bin.rs"]
mod zeo_bin;

use std::path::Path;

fn run(rb: &Path, mode: golden::Mode) -> datatest_stable::Result<()> {
    golden::run_golden(rb, mode, &golden::tests_run_cwd())
}

fn example(rb: &Path) -> datatest_stable::Result<()> {
    run(rb, golden::Mode::Pass)
}

fn divergence(rb: &Path) -> datatest_stable::Result<()> {
    run(rb, golden::Mode::Divergence)
}

fn macos_only(rb: &Path) -> datatest_stable::Result<()> {
    if cfg!(not(target_os = "macos")) {
        eprintln!("tests/macos is macOS-only: skipped {}", rb.display());
        return Ok(());
    }
    run(rb, golden::Mode::Pass)
}

fn jit_only(rb: &Path) -> datatest_stable::Result<()> {
    if golden::backend_is_aot() {
        eprintln!("tests/jit needs the JIT backend: skipped {}", rb.display());
        return Ok(());
    }
    run(rb, golden::Mode::Pass)
}

fn gap(rb: &Path) -> datatest_stable::Result<()> {
    run(rb, golden::Mode::Xfail)
}

fn spinel(rb: &Path) -> datatest_stable::Result<()> {
    run(rb, golden::Mode::Pass)
}

/// Every milestone and gemtest case is a whole-graph compile by definition,
/// so the raise is unconditional rather than a by-name list.
///
/// SAFETY: nextest runs each test in its own process, and both are set
/// before the first `run_deadline()`/`max_child_rss()` read.
fn raise_the_bounds() {
    unsafe {
        std::env::set_var("ZEO_GOLDEN_RUN_DEADLINE", "300");
        std::env::set_var("ZEO_GOLDEN_MAX_RSS", "4096");
    }
}

fn milestone(rb: &Path) -> datatest_stable::Result<()> {
    raise_the_bounds();
    let pending = rb
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|d| d == "pending");
    let mode = match pending {
        true => golden::Mode::Xfail,
        false => golden::Mode::Pass,
    };
    // `an_rspec_suite_runs.rb` needs rspec, which ruby does not ship and zeo
    // deliberately does not vendor. The oracle reaches it through bundler;
    // zeo needs `-I` on each gem's `lib/`. Reading the bundle rather than
    // naming versions keeps the pins out of the program.
    let env = golden::SuiteEnv {
        load_roots: golden::bundle_rspec_libs(),
        ..Default::default()
    };
    golden::run_golden_env(rb, mode, &golden::tests_run_cwd(), &env)
}

/// Each case runs with the vendored gem root as its working directory (test
/// frameworks strip the `Dir.pwd` prefix from paths they print), the gem's
/// `test/` as a load root on BOTH sides, and `vendor/gemtests/` as a zeo
/// package dir (the stub gemspec makes each tree a requirable package).
fn gemtest(rb: &Path) -> datatest_stable::Result<()> {
    let rb = std::fs::canonicalize(rb).unwrap_or_else(|_| rb.to_path_buf());
    let gem = rb
        .parent()
        .and_then(Path::file_name)
        .expect("tests/gemtests/<gem>/<case>.rb")
        .to_string_lossy()
        .into_owned();
    let vendor = paths::workspace_root().join("vendor").join("gemtests");
    let gem_root = vendor.join(&gem);
    if !gem_root.is_dir() {
        // Fetch-on-demand: no tree, no test.
        return Ok(());
    }
    raise_the_bounds();
    let env = golden::SuiteEnv {
        package_dirs: vec![vendor.clone()],
        load_roots: vec![gem_root.join("test")],
        oracle_includes: vec![gem_root.join("lib"), gem_root.join("test")],
        ..Default::default()
    };
    golden::run_golden_env(&rb, golden::Mode::Pass, &gem_root, &env)
}

datatest_stable::harness! {
    { test = example, root = "../../tests", pattern = r"^[^/]+\.rb$" },
    { test = divergence, root = "../../tests/divergences", pattern = r"^[^/]+\.rb$" },
    { test = macos_only, root = "../../tests/macos", pattern = r"^[^/]+\.rb$" },
    { test = jit_only, root = "../../tests/jit", pattern = r"^[^/]+\.rb$" },
    { test = gap, root = "../../tests/gaps", pattern = r"^[^/]+\.rb$" },
    { test = spinel, root = "../../tests/spinel", pattern = r"^[^/]+\.rb$" },
    { test = milestone, root = "../../tests/milestones", pattern = r"^(pending/)?[^/]+\.rb$" },
    { test = gemtest, root = "../../tests/gemtests", pattern = r"^[^/]+/[^/]+\.rb$" },
}
