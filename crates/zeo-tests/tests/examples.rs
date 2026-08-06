//! The zeo-authored example programs (`tests/*.rb` and the per-gem programs in
//! `tests/gems/`) as `cargo test`/nextest cases: each compiled by zeo, run, and
//! diffed against its committed ruby-oracle golden `.rb.expected` (stdout AND
//! stderr). Replaces the old `xtask test`/`regen`.
//!
//! The pattern matches the top level plus `tests/gems/`, never the
//! `tests/spinel/` or `tests/gaps/` subdirectories (those are their own suites).
//!
//! `cargo xtask bless examples::` re-records the goldens from ruby.

#[path = "support/golden.rs"]
mod golden;

use std::path::Path;

fn example(rb: &Path) -> datatest_stable::Result<()> {
    golden::run_golden(rb, golden::Mode::Pass, &golden::tests_run_cwd(), true)
}

datatest_stable::harness! {
    { test = example, root = "../../tests", pattern = r"^(?:gems/)?[^/]+\.rb$" },
}
