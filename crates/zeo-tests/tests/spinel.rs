//! The conformance corpus (`tests/spinel/`, the suite vendored from spinel) as
//! `cargo test`/nextest cases -- one per `.rb`, compiled by zeo and diffed
//! against its committed ruby-oracle golden (stdout AND stderr). Replaces the
//! bespoke `xtask conformance run`/scoreboard harness.
//!
//! Top-level `spinel/*.rb` -> `Mode::Pass` (zeo must match the golden). Fixture
//! SUBDIRECTORIES (e.g. `frozen_string_literal_per_file/`, `require/`) are data
//! for other tests, not tests themselves -- the `^[^/]+\.rb$` pattern matches
//! only the top level, never a deeper `.rb`. Compile-REJECTION coverage lives in
//! the e2e suite (`compile_project(...).unwrap_err()`).
//!
//! `ZEO_BLESS=1 cargo test --test spinel` re-records the goldens from ruby.

#[path = "support/golden.rs"]
mod golden;

use std::path::Path;

fn spinel(rb: &Path) -> datatest_stable::Result<()> {
    golden::run_golden(rb, golden::Mode::Pass, &golden::tests_run_cwd(), true)
}

datatest_stable::harness! {
    { test = spinel, root = "../../tests/spinel", pattern = r"^[^/]+\.rb$" },
}
