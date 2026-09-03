//! The corpus: every `.rb` under `test/`, compiled by zeo, run, and held to
//! the answer recorded under its `__END__` (stdout, stderr and the exit
//! status). One nextest case per program per leg.
//!
//! A program's contract is its DIRECTORY (`suites.rs`): `test/lang`,
//! `test/core`, `test/stdlib` and `test/compiler` must match ruby; `errors`,
//! `features` and `divergences` record zeo's own answer and must match it; `gaps` must
//! NOT match yet; `aot` runs on both the JIT and a real link in every
//! profile; `milestones` splice whole require graphs; `ze0` takes three
//! roads. Anything deeper than a suite's depth is a fixture, not a test.
//!
//! Case names are `<suite>::<path>`, and a leg other than the JIT prefixes
//! the suite: `aot_core::string/upcase.rb`, `memcheck_lang::...`,
//! `diff_stdlib::...`. The default nextest profile runs the JIT legs and
//! `aot_link`; `-P full` runs everything (`.config/nextest.toml`).

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
    ($( $jit:ident / $aot:ident / $mem:ident / $diff:ident => $suite:ident ),* $(,)?) => {
        $(
            fn $jit(rb: &Path) -> datatest_stable::Result<()> { golden::run(rb, &suites::$suite, Leg::Jit) }
            fn $aot(rb: &Path) -> datatest_stable::Result<()> { golden::run(rb, &suites::$suite, Leg::Aot) }
            fn $mem(rb: &Path) -> datatest_stable::Result<()> { golden::run(rb, &suites::$suite, Leg::Memcheck) }
            fn $diff(rb: &Path) -> datatest_stable::Result<()> { golden::run(rb, &suites::$suite, Leg::Differential) }
        )*
    };
}

suite_fns! {
    lang / aot_lang / memcheck_lang / diff_lang => LANG,
    core / aot_core / memcheck_core / diff_core => CORE,
    stdlib / aot_stdlib / memcheck_stdlib / diff_stdlib => STDLIB,
    compiler / aot_compiler / memcheck_compiler / diff_compiler => COMPILER,
}

fn aot(rb: &Path) -> datatest_stable::Result<()> {
    golden::run(rb, &suites::AOT, Leg::Jit)
}

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

fn gaps(rb: &Path) -> datatest_stable::Result<()> {
    golden::run(rb, &suites::GAPS, Leg::Jit)
}

/// `pending/` is the XFAIL tier of the milestones: same bounds, opposite
/// verdict.
fn milestones(rb: &Path) -> datatest_stable::Result<()> {
    let pending = rb
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|d| d == "pending");
    let suite = suites::Suite {
        verdict: if pending {
            suites::Verdict::Xfail
        } else {
            suites::Verdict::Match
        },
        ..suites::MILESTONES
    };
    golden::run(rb, &suite, Leg::Jit)
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
    { test = gaps, root = "../../test/gaps", pattern = r"^[^/]+\.rb$" },
    { test = milestones, root = "../../test/milestones", pattern = r"^(pending/)?[^/]+\.rb$" },
    { test = ze0, root = "../../test/ze0", pattern = r"^[^/]+\.rb$" },
    { test = aot_lang, root = "../../test/lang", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = aot_core, root = "../../test/core", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = aot_stdlib, root = "../../test/stdlib", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = aot_compiler, root = "../../test/compiler", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = memcheck_lang, root = "../../test/lang", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = memcheck_core, root = "../../test/core", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = memcheck_stdlib, root = "../../test/stdlib", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = memcheck_compiler, root = "../../test/compiler", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = diff_lang, root = "../../test/lang", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = diff_core, root = "../../test/core", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = diff_stdlib, root = "../../test/stdlib", pattern = r"^[^/]+/[^/]+\.rb$" },
    { test = diff_compiler, root = "../../test/compiler", pattern = r"^[^/]+/[^/]+\.rb$" },
}
