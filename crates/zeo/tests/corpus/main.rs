//! The corpus: every `.rb` under `test/`, compiled by zeo, run, and held to
//! the answer recorded under its `__END__` (stdout, stderr and the exit
//! status).
//!
//! ONE nextest case per program, and one compile. The corpus is not re-run
//! under variations: the ownership ledger and the cycle census ride on that
//! single run (`legs.rs`), and the two roads that need a second process are
//! narrow -- `test/aot/` takes a real link, and a `#@ pkggap` program takes
//! the packaged road.
//!
//! A program's contract is its DIRECTORY (`suites.rs`): `test/lang`,
//! `test/core`, `test/stdlib` and `test/compiler` must match ruby; `errors`,
//! `features` and `divergences` record zeo's own answer and must match it;
//! `milestones` splice whole require graphs; `ze0` takes three roads.
//! Anything deeper than a suite's depth is a fixture, not a test.
//!
//! A program zeo does NOT get right lives in `todo/`, which nothing runs.
//!
//! Case names are `<suite>::<path>`. The one exception is `aot_link::`, the
//! curated link tier over the same `test/aot/` files.

// Shared with `bless`, `checks` and `api`, so each binary uses a part of
// each: the reader here, the writer there.
#[allow(dead_code)]
mod case;
#[path = "../common/mod.rs"]
mod common;
mod compare;
mod golden;
mod legs;
mod normalize;
#[allow(dead_code)]
mod oracle;
mod run;
#[allow(dead_code)]
mod suites;
mod ze0;

use std::path::Path;

use legs::Leg;

macro_rules! suite_fns {
    ($( $name:ident => $suite:ident ),* $(,)?) => {
        $(
            fn $name(rb: &Path) -> datatest_stable::Result<()> {
                golden::run(rb, &suites::$suite, Leg::Jit)
            }
        )*
    };
}

suite_fns! {
    lang => LANG,
    core => CORE,
    stdlib => STDLIB,
    compiler => COMPILER,
    aot => AOT,
}

/// The one curated link tier: the same `test/aot/` programs through an
/// object file and a real `cc`. What ships, asked of the programs written to
/// ask it.
fn aot_link(rb: &Path) -> datatest_stable::Result<()> {
    golden::run(rb, &suites::AOT, Leg::Aot)
}

fn errors(rb: &Path) -> datatest_stable::Result<()> {
    golden::run(rb, &suites::ERRORS, Leg::Jit)
}

fn features(rb: &Path) -> datatest_stable::Result<()> {
    golden::run(rb, &suites::FEATURES, Leg::Jit)
}

fn divergences(rb: &Path) -> datatest_stable::Result<()> {
    golden::run(rb, &suites::DIVERGENCES, Leg::Jit)
}

fn milestones(rb: &Path) -> datatest_stable::Result<()> {
    golden::run(rb, &suites::MILESTONES, Leg::Jit)
}

fn ze0(rb: &Path) -> datatest_stable::Result<()> {
    ze0::run(rb)
}

datatest_stable::harness! {
    { test = lang, root = "../../test/lang", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = core, root = "../../test/core", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = stdlib, root = "../../test/stdlib", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = compiler, root = "../../test/compiler", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = aot, root = "../../test/aot", pattern = r"^[^/]+\.rb$" },
    { test = aot_link, root = "../../test/aot", pattern = r"^[^/]+\.rb$" },
    { test = errors, root = "../../test/errors", pattern = r"^[^/]+\.rb$" },
    { test = features, root = "../../test/features", pattern = r"^[^/]+\.rb$" },
    { test = divergences, root = "../../test/divergences", pattern = r"^[^/]+\.rb$" },
    { test = milestones, root = "../../test/milestones", pattern = r"^[^/]+\.rb$" },
    { test = ze0, root = "../../test/ze0", pattern = r"^[^/]+\.rb$" },
}
