//! The MILESTONE programs (`tests/milestones/`): the umbrella entry points
//! whose working is the point of a whole effort, each its own named case so a
//! regression names itself rather than arriving as one line inside a bigger
//! suite. `require "rubygems"` is the headline.
//!
//! Two tiers, the same contract `tests/` and `tests/gaps/` already carry:
//!
//! - `tests/milestones/*.rb` runs in `Mode::Pass` -- it works, and it must
//!   keep working.
//! - `tests/milestones/pending/*.rb` runs in `Mode::Xfail` -- it does not work
//!   yet, it is tracked, and the day it starts matching ruby the suite says
//!   so instead of staying quietly green.
//!
//! Why a suite of its own rather than a few more files in `tests/`: each of
//! these splices a whole library's require graph, which is ~55s of compile and
//! past the ordinary goldens' 60s / 512 MiB bounds. Same reasoning, and the
//! same raise, as `gemtests.rs`. The default nextest profile opts this binary
//! out so the dev loop stays fast; `make ci-milestones` and `make gate` run it.
//!
//! **A milestone golden must print SHAPES, never versions.** Ruby has RubyGems
//! loaded before the program starts, so its `require` answers `false` where
//! zeo's answers `true`, and the bundled version is two patch releases ahead
//! of the reference ruby's. Both sides agree on what the API DOES; neither
//! agrees on those two, and a golden that prints them records a difference
//! that means nothing.
//!
//! `tools/zeo-dev bless milestones::` re-records each golden from ruby.

use zeo_tests::golden;

use std::path::Path;

fn milestone(rb: &Path) -> datatest_stable::Result<()> {
    // A whole library's require graph in one program. 60s is the ordinary
    // goldens' bound and 512 MiB was measured against a golden that prints a
    // few lines; every case here is an umbrella compile by definition, so the
    // raise is unconditional rather than a by-name list.
    // SAFETY: nextest runs each test in its own process, and both are set
    // before the first `run_deadline()`/`max_child_rss()` read.
    unsafe {
        std::env::set_var("ZEO_GOLDEN_RUN_DEADLINE", "300");
        std::env::set_var("ZEO_GOLDEN_MAX_RSS", "4096");
    }
    let pending = rb
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|d| d == "pending");
    let mode = match pending {
        true => golden::Mode::Xfail,
        false => golden::Mode::Pass,
    };
    golden::run_golden(rb, mode, &golden::tests_run_cwd())
}

datatest_stable::harness! {
    { test = milestone, root = "../../tests/milestones", pattern = r"^(pending/)?[^/]+\.rb$" },
}
