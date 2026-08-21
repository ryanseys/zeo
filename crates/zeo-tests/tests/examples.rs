//! The zeo-authored example programs (`tests/*.rb`) as `cargo test`/nextest
//! cases: each compiled by zeo, run, and diffed against its committed
//! ruby-oracle golden `.rb.expected` (stdout AND stderr). Replaces the old
//! `xtask test`/`regen`.
//!
//! The pattern matches the top level only, never the `tests/spinel/`,
//! `tests/gaps/`, or `tests/bench/` subdirectories (the first two are their
//! own suites; `tests/bench/` holds compile-bench INPUTS with no goldens --
//! whole-gem programs are the gem probe's coverage, not this suite's, since
//! each one costs minutes of rustc and bogged down every run).
//!
//! `cargo xtask bless examples::` re-records the goldens from ruby.

#[path = "support/golden.rs"]
mod golden;

use std::path::Path;

/// The goldens that compile a WHOLE gem's require graph into one program.
/// They are opted out of the default nextest profile for their cost (see
/// `.config/nextest.toml`); here they are opted out of the ordinary
/// child's resource bounds, which were measured against a golden that
/// prints a few lines.
const WHOLE_GEM: &[&str] = &["rspec_end_to_end", "prism_surface"];

fn example(rb: &Path) -> datatest_stable::Result<()> {
    if rb
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| WHOLE_GEM.contains(&s))
    {
        // SAFETY: nextest runs each test in its own process, and both are
        // read once through a `OnceLock` that nothing has touched yet.
        unsafe {
            std::env::set_var("ZEO_GOLDEN_MAX_RSS", "4096");
            std::env::set_var("ZEO_GOLDEN_RUN_DEADLINE", "300");
        }
    }
    golden::run_golden(rb, golden::Mode::Pass, &golden::tests_run_cwd(), true)
}

datatest_stable::harness! {
    { test = example, root = "../../tests", pattern = r"^[^/]+\.rb$" },
}
