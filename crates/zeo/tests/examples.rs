//! The zeo-authored example programs as `cargo test`/nextest cases: each
//! compiled by zeo, run, and diffed against its committed ruby-oracle golden
//! `.rb.expected` (stdout AND stderr).
//!
//! **A golden's contract is its DIRECTORY**, never a per-file marker. Four of
//! them live here, and each entry below is the whole rule for the directory it
//! names:
//!
//! - `tests/` -- must match ruby.
//! - `tests/divergences/` -- zeo answers differently on purpose, so the golden
//!   records zeo's own output. See that directory's README.
//! - `tests/macos/` -- must match ruby, and only runs on macOS: the output is
//!   inherently platform-specific, so Linux CRuby would diverge from the
//!   committed macOS-oracle golden exactly as zeo does.
//! - `tests/jit/` -- must match ruby, and only runs on the JIT backend: the
//!   program depends on the compiler and itself sharing one process.
//!
//! Each pattern matches ONE level, so no directory picks up another's cases,
//! and `tests/spinel/`, `tests/gaps/`, `tests/milestones/` and `tests/bench/`
//! stay out of this suite entirely (the first three are their own; `bench/`
//! holds compile-side inputs with no goldens).
//!
//! `tools/zeo-dev bless example::` re-records the goldens from ruby.

use zeo_tests::golden;

use std::path::Path;

fn run(rb: &Path, mode: golden::Mode) -> datatest_stable::Result<()> {
    golden::run_golden(rb, mode, &golden::tests_run_cwd())
}

fn example(rb: &Path) -> datatest_stable::Result<()> {
    run(rb, golden::Mode::Pass)
}

fn divergence(rb: &Path) -> datatest_stable::Result<()> {
    run(rb, golden::Mode::Divergence)
}

fn macos_only(rb: &Path) -> datatest_stable::Result<()> {
    if cfg!(not(target_os = "macos")) {
        eprintln!("tests/macos is macOS-only: skipped {}", rb.display());
        return Ok(());
    }
    run(rb, golden::Mode::Pass)
}

fn jit_only(rb: &Path) -> datatest_stable::Result<()> {
    if golden::backend_is_aot() {
        eprintln!("tests/jit needs the JIT backend: skipped {}", rb.display());
        return Ok(());
    }
    run(rb, golden::Mode::Pass)
}

datatest_stable::harness! {
    { test = example, root = "../../tests", pattern = r"^[^/]+\.rb$" },
    { test = divergence, root = "../../tests/divergences", pattern = r"^[^/]+\.rb$" },
    { test = macos_only, root = "../../tests/macos", pattern = r"^[^/]+\.rb$" },
    { test = jit_only, root = "../../tests/jit", pattern = r"^[^/]+\.rb$" },
}
