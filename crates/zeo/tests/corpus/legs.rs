//! How a program is run: in memory, or through a real link.
//!
//! There are two, and almost every program takes only the first. A program
//! is compiled ONCE per leg -- the corpus is not re-run under variations.
//! What used to be separate passes is folded in instead: the ownership
//! ledger and the cycle census ride on the ordinary run, because they are
//! switches on the same process rather than a different execution.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::case::Case;
use crate::suites::Suite;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    /// The in-process JIT: compile and run in memory, nothing on disk.
    Jit,
    /// The same CLIF through an object file and a real link: what ships.
    /// Only `test/aot/` takes it -- the link line is what that leg asks
    /// about, and the curated set covers it.
    Aot,
}

impl Leg {
    pub fn is_aot(self) -> bool {
        self == Leg::Aot
    }

    fn backend(self) -> &'static str {
        match self {
            Leg::Aot => "aot",
            Leg::Jit => "jit",
        }
    }
}

/// The `zeo` command for `case` on `leg`.
pub fn zeo_command(
    leg: Leg,
    case: &Case,
    rb: &Path,
    suite: &Suite,
    run_cwd: &Path,
) -> Result<Command, String> {
    let mut cmd = Command::new(crate::common::zeo_cli()?);
    cmd.arg("--backend").arg(leg.backend());
    // Both memory checks, on every run. They are switches on the same
    // process, they compose (verified over the whole corpus), and neither
    // changes a program's answer -- the census line is split out of stderr
    // before the comparison. A separate instrumented pass would double the
    // corpus to ask a question this run can answer for nearly nothing.
    //
    // ZEO_RT_LEAKCHECK: the compiled-ownership ledger. A non-zero balance at
    //   exit is a leak or a double-consume in the emitted lowering.
    // ZEO_GC + ZEO_RT_GCCHECK: the cycle collector, and the census it writes
    //   at exit, checked against the program's `#@ gccheck` line.
    cmd.env("ZEO_RT_LEAKCHECK", "1")
        .env("ZEO_GC", "1")
        .env("ZEO_RT_GCCHECK", "1");
    if suite.whole_graph {
        for root in oracle_store_rspec_libs(&crate::common::workspace_root()) {
            cmd.arg("-I").arg(root);
        }
    }
    cmd.args(&case.directives.zeo);
    // Exactly what the oracle gets: the file, then the program's own args.
    // Option parsing stops at the file name on both sides, so a `--seed 42`
    // reaches the program without a separator.
    cmd.arg(rb).args(&case.directives.args);
    for (k, v) in &case.directives.env {
        cmd.env(k, v);
    }
    for (k, v) in &case.directives.zeo_env {
        cmd.env(k, v);
    }
    crate::common::child_env(&mut cmd)?;
    cmd.current_dir(run_cwd);
    Ok(cmd)
}

/// The oracle store's rspec trees, for the zeo side of the milestone that
/// runs a real suite. The oracle reaches them through bundler; zeo needs `-I`
/// on each `lib/`. Scoped to rspec rather than the whole store, which would
/// put upstream copies of gems zeo ships ahead of its own.
pub fn oracle_store_rspec_libs(repo: &Path) -> Vec<PathBuf> {
    let gems = repo.join("vendor").join("bundle").join("ruby");
    let mut libs: Vec<PathBuf> = std::fs::read_dir(&gems)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|abi| {
            std::fs::read_dir(abi.path().join("gems"))
                .into_iter()
                .flatten()
        })
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("rspec") || n.starts_with("diff-lcs")
            })
        })
        .map(|p| p.join("lib"))
        .filter(|p| p.is_dir())
        .collect();
    libs.sort();
    libs
}
