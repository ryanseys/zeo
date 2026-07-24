//! The `examples/*.rb` smoke suite as `cargo test`/nextest cases: each example
//! compiled by zeo, run, and diffed against its committed ruby-oracle golden
//! (`examples/<name>.expected`). Replaces the old `xtask test`/`regen`.
//!
//! `ZEO_BLESS=1 cargo test --test examples` re-records the goldens from ruby.

#[path = "support/golden.rs"]
mod golden;

use std::path::Path;

fn example(rb: &Path) -> datatest_stable::Result<()> {
    // Examples are a stdout smoke suite (their historical contract): ruby's
    // parse warnings / experimental notices / thread exception reports on
    // stderr aren't compared. Full-fidelity stderr lives in the corpus.
    golden::run_golden(rb, golden::Mode::Pass, &golden::examples_run_cwd(), false)
}

datatest_stable::harness! {
    { test = example, root = "../../examples", pattern = r"\.rb$" },
}
