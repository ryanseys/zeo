//! The zeo-authored example programs (`tests/*.rb`) as `cargo test`/nextest
//! cases: each compiled by zeo, run, and diffed against its committed
//! ruby-oracle golden `.rb.expected` (stdout AND stderr). Replaces the old
//! `zeo-dev test`/`regen`.
//!
//! The pattern matches the top level only, never the `tests/spinel/`,
//! `tests/gaps/`, or `tests/bench/` subdirectories (the first two are their
//! own suites; `tests/bench/` holds compile-side INPUTS with no goldens --
//! whole-gem programs are the gem probe's coverage, not this suite's, since
//! each one costs minutes of rustc and bogged down every run).
//!
//! `tools/zeo-dev bless examples::` re-records the goldens from ruby.

use zeo_tests::golden;

use std::path::Path;

fn example(rb: &Path) -> datatest_stable::Result<()> {
    golden::run_golden(rb, golden::Mode::Pass, &golden::tests_run_cwd(), true)
}

datatest_stable::harness! {
    { test = example, root = "../../tests", pattern = r"^[^/]+\.rb$" },
}
