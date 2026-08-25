//! Expected-to-fail conformance cases (`tests/gaps/`): Ruby programs zeo does
//! NOT yet match `ruby` on, checked in and tracked as XFAIL. Each runs through
//! the shared golden helper in `Mode::Xfail` -- a still-diverging gap PASSES; a
//! gap that starts matching ruby FAILS with a "promote" message. Promote with
//! `tools/zeo-dev promote-gap`, which moves its `.rb` + sidecars into `tests/`,
//! the zeo-authored suite -- NOT `tests/spinel/`, which mirrors the vendored
//! spinel corpus.
//!
//! The pattern matches the TOP LEVEL only: a subdirectory here holds a gap's
//! FIXTURES -- files another program requires -- and a fixture is not a test.
//!
//! `tools/zeo-dev bless gaps::` re-records each gap's golden from ruby.

use zeo_tests::golden;

use std::path::Path;

fn gap(rb: &Path) -> datatest_stable::Result<()> {
    golden::run_golden(rb, golden::Mode::Xfail, &golden::tests_run_cwd(), true)
}

datatest_stable::harness! {
    { test = gap, root = "../../tests/gaps", pattern = r"^[^/]+\.rb$" },
}
