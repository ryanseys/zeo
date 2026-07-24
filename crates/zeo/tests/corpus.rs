//! The conformance corpus (`conformance/corpus/test/`, the vendored spinel
//! suite) as `cargo test`/nextest cases -- one per `.rb`, compiled by zeo and
//! diffed against its committed ruby-oracle golden (stdout AND stderr). Replaces
//! the bespoke `xtask conformance run`/scoreboard harness.
//!
//! Top-level `test/*.rb` -> `Mode::Pass` (zeo must match the golden). Fixture
//! SUBDIRECTORIES (e.g. `frozen_string_literal_per_file/`) are data for other
//! tests, not tests themselves -- the pattern matches only the top level, never
//! a deeper `.rb`. Compile-REJECTION coverage lives in the e2e suite
//! (`compile_project(...).unwrap_err()`), so there is no `analyze_fail/` harness
//! entry here (a datatest-stable entry that matches zero files panics, and the
//! two spinel-era `analyze_fail/` cases were dynamic `define_method` -- which
//! zeo, unlike the spinel subset, compiles and runs, so they moved to the corpus
//! proper as passing tests).
//!
//! `ZEO_BLESS=1 cargo test --test corpus` re-records the goldens from ruby.

#[path = "support/golden.rs"]
mod golden;

use std::path::Path;

fn corpus(rb: &Path) -> datatest_stable::Result<()> {
    golden::run_golden(rb, golden::Mode::Pass, &golden::corpus_run_cwd(), true)
}

datatest_stable::harness! {
    { test = corpus, root = "../../conformance/corpus/test", pattern = r"^[^/]+\.rb$" },
}
