//! Expected-to-fail conformance cases (`tests/gaps/`): Ruby programs zeo does
//! NOT yet match `ruby` on, checked in and tracked as XFAIL. Each runs through
//! the shared golden helper in `Mode::Xfail` -- a still-diverging gap PASSES; a
//! gap that starts matching ruby FAILS with a "promote" message (move its `.rb`
//! + sidecars into `tests/spinel/`).
//!
//! `ZEO_BLESS=1 cargo test --test gaps` re-records each gap's golden from ruby.

#[path = "support/golden.rs"]
mod golden;

use std::path::Path;

fn gap(rb: &Path) -> datatest_stable::Result<()> {
    golden::run_golden(rb, golden::Mode::Xfail, &golden::tests_run_cwd(), true)
}

datatest_stable::harness! {
    { test = gap, root = "../../tests/gaps", pattern = r"\.rb$" },
}
