//! End-to-end gem TEST SUITES under zeo: each driver in
//! `tests/gemtests/<gem>/` requires one vendored test file, the framework's
//! autorun executes it for real, and the output is diffed byte-for-byte
//! against the CRuby oracle's golden -- seeded (`.args` carries `--seed`),
//! wall-clock lines stubbed by the driver.
//!
//! The test trees come from `cargo xtask gemtests sync`
//! (`conformance/gemtests.toml` pins them) into gitignored
//! `vendor/gemtests/<gem>/`; when a tree hasn't been fetched the suite skips
//! rather than failing, so a fresh clone stays green until it opts in.
//!
//! Each case runs with the vendored gem root as its working directory (test
//! frameworks strip the `Dir.pwd` prefix from paths they print), the gem's
//! `test/` as a load root on BOTH sides, and `vendor/gemtests/` as a zeo
//! package dir (the stub gemspec makes each tree a requirable package).

#[path = "support/golden.rs"]
mod golden;

use std::path::Path;

fn gemtest(rb: &Path) -> datatest_stable::Result<()> {
    let rb = std::fs::canonicalize(rb).unwrap_or_else(|_| rb.to_path_buf());
    let gem = rb
        .parent()
        .and_then(Path::file_name)
        .expect("tests/gemtests/<gem>/<case>.rb")
        .to_string_lossy()
        .into_owned();
    let vendor = golden::workspace_root().join("vendor").join("gemtests");
    let gem_root = vendor.join(&gem);
    if !gem_root.is_dir() {
        // Fetch-on-demand: no tree, no test. `cargo xtask gemtests sync`.
        return Ok(());
    }
    // A whole test file is one case; its compile splices the gem's full
    // require graph and the run executes every test in it. 60s is the
    // ordinary goldens' bound, not this suite's.
    // SAFETY: nextest runs each test in its own process, and this is set
    // before the first `run_deadline()` read.
    unsafe { std::env::set_var("ZEO_GOLDEN_RUN_DEADLINE", "300") };
    let env = golden::SuiteEnv {
        package_dirs: vec![vendor.clone()],
        load_roots: vec![gem_root.join("test")],
        oracle_includes: vec![gem_root.join("lib"), gem_root.join("test")],
    };
    golden::run_golden_env(&rb, golden::Mode::Pass, &gem_root, true, &env)
}

datatest_stable::harness! {
    { test = gemtest, root = "../../tests/gemtests", pattern = r"^[^/]+/[^/]+\.rb$" },
}
