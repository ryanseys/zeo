//! The suite-adapter surface: a `Suite` turns an on-disk test corpus into
//! `TestCase`s, and the runner consumes only `TestCase` -- so adding ruby/spec
//! or CRuby `test/` support later means writing a new `discover`, not a new
//! runner.

use std::path::{Path, PathBuf};

/// One runnable conformance test, fully described.
pub struct TestCase {
    /// Stable id used in the scoreboard/skiplist, e.g. `alias_global` or
    /// `analyze_fail/attributes_non_symbol`.
    pub id: String,
    /// The `.rb` file handed to `spinelc`.
    pub source: PathBuf,
    /// ARGV for both the compiled binary and the oracle.
    pub args: Vec<String>,
    /// File whose bytes are fed to the program's stdin (and the oracle's).
    pub stdin: Option<PathBuf>,
    /// Working directory for the compiled binary and the oracle -- the spinel
    /// corpus's `.args` files reference repo-root-relative paths.
    pub run_cwd: PathBuf,
    pub expectation: Expectation,
}

pub enum Expectation {
    /// Diff stdout (+stderr) against snapshots. A missing stdout snapshot
    /// means "generate it live from the oracle `ruby`"; a missing stderr
    /// snapshot means "stderr must be empty" (the C Makefile's exact rule)
    /// unless the test is live-oracle, in which case the oracle's stderr is
    /// the reference.
    Snapshot {
        stdout: Option<PathBuf>,
        stderr: Option<PathBuf>,
    },
    /// `analyze_fail/`: spinelc must reject the program (nonzero exit). The
    /// C corpus's `.stderr.expected` wording is C-spinel's, not ours, so it
    /// is not diffed.
    CompileFail,
}

pub trait Suite {
    fn name(&self) -> &'static str;
    fn discover(&self, root: &Path) -> Result<Vec<TestCase>, String>;
    /// Environment variable consulted when `--dir` isn't given.
    fn root_env_var(&self) -> &'static str;
}

/// Outcome of one test, in scoreboard vocabulary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    /// Output didn't match the reference.
    FailOutput,
    /// spinelc rejected the program (a scope gap or compiler bug).
    FailCompile,
    /// spinelc emitted Rust that `rustc` refused -- always a spinelc bug,
    /// never a scope gap, so it gets its own verdict.
    FailRustc,
    /// The compiled binary crashed (killed by a signal) where the reference
    /// produced output.
    FailRun,
    TimeoutCompile,
    TimeoutRun,
    Skip,
    /// The oracle `ruby` itself failed or timed out -- neither a pass nor a
    /// spinel-rs failure; surfaced separately.
    OracleFail,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::FailOutput => "FAIL_OUTPUT",
            Verdict::FailCompile => "FAIL_COMPILE",
            Verdict::FailRustc => "FAIL_RUSTC",
            Verdict::FailRun => "FAIL_RUN",
            Verdict::TimeoutCompile => "TIMEOUT_COMPILE",
            Verdict::TimeoutRun => "TIMEOUT_RUN",
            Verdict::Skip => "SKIP",
            Verdict::OracleFail => "ORACLE_FAIL",
        }
    }

    pub fn from_str(s: &str) -> Option<Verdict> {
        Some(match s {
            "PASS" => Verdict::Pass,
            "FAIL_OUTPUT" => Verdict::FailOutput,
            "FAIL_COMPILE" => Verdict::FailCompile,
            "FAIL_RUSTC" => Verdict::FailRustc,
            "FAIL_RUN" => Verdict::FailRun,
            "TIMEOUT_COMPILE" => Verdict::TimeoutCompile,
            "TIMEOUT_RUN" => Verdict::TimeoutRun,
            "SKIP" => Verdict::Skip,
            "ORACLE_FAIL" => Verdict::OracleFail,
            _ => return None,
        })
    }
}

/// One test's recorded result -- what stamps persist and the scoreboard reads.
pub struct TestResult {
    pub id: String,
    pub verdict: Verdict,
    /// Pipeline stage the verdict was decided at: `compile`, `run`,
    /// `expect`, or `-`.
    pub stage: &'static str,
    /// Triage bucket (e.g. `dyn-kwargs` or `auto-1a2b3c4d`); `-` if none.
    pub bucket: String,
    /// Gap cluster letter from the implementation plan; `-` if none.
    pub cluster: String,
    /// Last lines of the failing stage's stderr, for triage.
    pub stderr_tail: String,
    pub compile_ms: u64,
    pub run_ms: u64,
    /// Whether this result was replayed from a fresh stamp rather than
    /// executed.
    pub cached: bool,
}
