//! The per-test pipeline (compile -> run -> expect) and the worker pool that
//! drives it. `spinelc` runs as a subprocess -- many scope rejections are
//! `panic!`s that can't be caught or timed out in-process, and their stderr
//! is exactly what triage consumes.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use super::exec::run_with_timeout;
use super::oracle::Oracle;
use super::stamps::StampStore;
use super::suite::{Expectation, TestCase, TestResult, Verdict};
use super::triage;
use super::util::{normalize_crlf, sanitize_id, tail_lines};

/// The marker `spinelc::build` puts in its error when the *generated* Rust
/// failed to compile -- always a spinelc bug, never a scope gap.
const RUSTC_FAILURE_MARKER: &str = "rustc failed compiling the generated program";

pub struct Runner {
    pub spinelc: PathBuf,
    pub bin_dir: PathBuf,
    pub diff_dir: PathBuf,
    pub compile_timeout: Duration,
    pub run_timeout: Duration,
}

impl Runner {
    /// Execute every case (unless a fresh stamp exists), in parallel across
    /// `jobs` workers, printing one verdict line per test.
    pub fn run_all(
        &self,
        cases: Vec<(TestCase, u64)>, // (case, inputs hash)
        stamps: &StampStore,
        oracle: &Oracle,
        jobs: usize,
        force: bool,
        fail_fast: bool,
    ) -> Vec<TestResult> {
        std::fs::create_dir_all(&self.bin_dir).ok();
        std::fs::create_dir_all(&self.diff_dir).ok();

        let total = cases.len();
        // Longest-processing-time-first scheduling: draw the slowest cases
        // (by their last recorded compile+run cost) first, so the big compiles
        // start immediately and the many short ones backfill idle workers --
        // instead of a slow case landing last and stalling the tail while 11
        // cores sit idle (the "bursty" feel). A case with no prior stamp is
        // unknown-cost and scheduled first, so a new slow test isn't discovered
        // only at the very end. Purely a schedule; results are re-sorted by id.
        let mut cases = cases;
        cases.sort_by_key(|(case, _)| {
            std::cmp::Reverse(
                stamps
                    .load_any(&case.id)
                    .map(|r| r.compile_ms + r.run_ms)
                    .unwrap_or(u64::MAX),
            )
        });
        let queue = Mutex::new(cases.into_iter().collect::<VecDeque<_>>());
        let results = Mutex::new(Vec::with_capacity(total));
        let done = std::sync::atomic::AtomicUsize::new(0);
        let stop = std::sync::atomic::AtomicBool::new(false);

        std::thread::scope(|scope| {
            for _ in 0..jobs.max(1) {
                scope.spawn(|| loop {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let Some((case, hash)) = queue.lock().unwrap().pop_front() else {
                        break;
                    };
                    let result = match (!force).then(|| stamps.load(&case.id, hash)).flatten() {
                        Some(cached) => cached,
                        None => {
                            let result = self.run_one(&case, oracle);
                            if let Err(e) = stamps.save(&result, hash) {
                                eprintln!("warning: {e}");
                            }
                            result
                        }
                    };
                    let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    // Per-test compile/run timing inline, so a single slow case
                    // stands out in the stream (the "many fast, then one stalls"
                    // pattern) instead of only showing up in the ranking below.
                    println!(
                        "[{n}/{total}] {} {} (c:{}ms r:{}ms){}",
                        result.verdict.as_str(),
                        result.id,
                        result.compile_ms,
                        result.run_ms,
                        if result.cached { " (cached)" } else { "" }
                    );
                    if fail_fast && result.verdict != Verdict::Pass {
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    results.lock().unwrap().push(result);
                });
            }
        });

        let mut results = results.into_inner().unwrap();
        results.sort_by(|a, b| a.id.cmp(&b.id));
        results
    }

    fn run_one(&self, case: &TestCase, oracle: &Oracle) -> TestResult {
        let mut result = TestResult {
            id: case.id.clone(),
            verdict: Verdict::Pass,
            stage: "-",
            bucket: "-".to_owned(),
            cluster: "-".to_owned(),
            stderr_tail: String::new(),
            compile_ms: 0,
            run_ms: 0,
            cached: false,
        };

        // -- compile --------------------------------------------------------
        let bin_path = self.bin_dir.join(sanitize_id(&case.id));
        let mut cmd = Command::new(&self.spinelc);
        cmd.arg(&case.source)
            .arg("-o")
            .arg(&bin_path)
            .env("RUST_BACKTRACE", "0")
            // Corpus programs are compiled, run once, and deleted, so they can
            // link the runtime dynamically: 616K each instead of 9.8MB, which
            // is what keeps the compiled-program cache near 1GB rather than the
            // ~16GB this suite alone costs statically. `spinelc` defaults to
            // static because a shipped binary has to stand on its own.
            .env("SPINELC_LINK_DYNAMIC", "1");
            // No `SPINELC_ASSUME_BUILT` needed: `prebuild` above already built
            // `spinel-rt`, so each subprocess's `ensure_runtime_built` is a cheap
            // existence check and `build_binary` itself only links -- neither runs
            // cargo, so there's no build-lock to contend on.
        let compile = match run_with_timeout(cmd, None, self.compile_timeout) {
            Ok(e) => e,
            Err(e) => return harness_error(result, "compile", &e),
        };
        result.compile_ms = compile.duration.as_millis() as u64;

        if compile.timed_out {
            result.verdict = Verdict::TimeoutCompile;
            result.stage = "compile";
            return result;
        }

        // A `CompileFail` case passes when spinelc rejects it -- but if
        // spinel-rs's wider dynamic-dispatch scope legitimately compiles a
        // program C-spinel's analyzer rejects, run it against the live
        // oracle instead of failing: matching real ruby is strictly better
        // than rejecting.
        if matches!(case.expectation, Expectation::CompileFail) && !compile.success() {
            result.stage = "compile";
            return result;
        }

        if !compile.success() {
            let stderr = String::from_utf8_lossy(&compile.stderr);
            result.stage = "compile";
            result.stderr_tail = tail_lines(&compile.stderr, 5);
            if stderr.contains(RUSTC_FAILURE_MARKER) {
                result.verdict = Verdict::FailRustc;
                result.bucket = "rustc-failure".to_owned();
                result.cluster = "!".to_owned();
            } else {
                result.verdict = Verdict::FailCompile;
                let t = triage::classify(&stderr);
                result.bucket = t.bucket;
                result.cluster = t.cluster;
            }
            return result;
        }

        // -- run --------------------------------------------------------------
        let stdin = match &case.stdin {
            Some(p) => match std::fs::read(p) {
                Ok(b) => Some(b),
                Err(e) => return harness_error(result, "run", &format!("reading stdin: {e}")),
            },
            None => None,
        };
        let mut cmd = Command::new(&bin_path);
        cmd.args(&case.args).current_dir(&case.run_cwd);
        let run = match run_with_timeout(cmd, stdin, self.run_timeout) {
            Ok(e) => e,
            Err(e) => return harness_error(result, "run", &e),
        };
        result.run_ms = run.duration.as_millis() as u64;
        let _ = std::fs::remove_file(&bin_path);

        if run.timed_out {
            result.verdict = Verdict::TimeoutRun;
            result.stage = "run";
            return result;
        }

        // -- expect -----------------------------------------------------------
        // A compiled `CompileFail` case has no snapshots; the live oracle is
        // its reference (same as a snapshot-less test).
        let no_snapshot = (None, None);
        let (stdout, stderr) = match &case.expectation {
            Expectation::Snapshot { stdout, stderr } => (stdout, stderr),
            Expectation::CompileFail => (&no_snapshot.0, &no_snapshot.1),
        };
        let (expected_out, expected_err) = match (stdout, stderr) {
            (Some(out_path), err_path) => {
                let out = match std::fs::read(out_path) {
                    Ok(b) => b,
                    Err(e) => return harness_error(result, "expect", &format!("{e}")),
                };
                let err = match err_path {
                    Some(p) => match std::fs::read(p) {
                        Ok(b) => b,
                        Err(e) => return harness_error(result, "expect", &format!("{e}")),
                    },
                    None => Vec::new(), // no .err.expected => stderr must be empty
                };
                (out, err)
            }
            // No committed snapshot: the live oracle's stdout AND stderr are
            // the reference.
            (None, _) => {
                let o = match oracle.run(case, false) {
                    Ok(o) => o,
                    Err(e) => return harness_error(result, "expect", &e),
                };
                if !o.ok {
                    result.verdict = Verdict::OracleFail;
                    result.stage = "expect";
                    result.stderr_tail = tail_lines(&o.stderr, 5);
                    return result;
                }
                (o.stdout, o.stderr)
            }
        };

        let actual_out = normalize_crlf(&run.stdout);
        let actual_err = normalize_crlf(&run.stderr);
        let expected_out = normalize_crlf(&expected_out);
        let expected_err = normalize_crlf(&expected_err);

        if actual_out == expected_out && actual_err == expected_err {
            return result; // PASS
        }

        result.stage = "expect";
        result.verdict = if run.crashed() {
            Verdict::FailRun
        } else {
            Verdict::FailOutput
        };
        result.stderr_tail = tail_lines(&run.stderr, 5);
        let t = triage::classify(&String::from_utf8_lossy(&run.stderr));
        // Only bucket output failures that carry a recognizable message;
        // a silent wrong-stdout diff stays unbucketed for manual triage.
        if !result.stderr_tail.is_empty() {
            result.bucket = t.bucket;
            result.cluster = t.cluster;
        }
        self.write_diff(
            &case.id,
            &expected_out,
            &actual_out,
            &expected_err,
            &actual_err,
        );
        result
    }

    fn write_diff(
        &self,
        id: &str,
        expected_out: &[u8],
        actual_out: &[u8],
        expected_err: &[u8],
        actual_err: &[u8],
    ) {
        let mut buf = String::new();
        for (label, expected, actual) in [
            ("stdout", expected_out, actual_out),
            ("stderr", expected_err, actual_err),
        ] {
            if expected == actual {
                continue;
            }
            buf.push_str(&format!("=== {label} diff ===\n"));
            let exp = String::from_utf8_lossy(expected);
            let act = String::from_utf8_lossy(actual);
            let (exp_lines, act_lines): (Vec<_>, Vec<_>) =
                (exp.lines().collect(), act.lines().collect());
            let first_diff = exp_lines
                .iter()
                .zip(&act_lines)
                .position(|(a, b)| a != b)
                .unwrap_or(exp_lines.len().min(act_lines.len()));
            buf.push_str(&format!(
                "first difference at line {}\n--- expected ({} lines)\n{}\n--- actual ({} lines)\n{}\n",
                first_diff + 1,
                exp_lines.len(),
                exp,
                act_lines.len(),
                act,
            ));
        }
        let path = self.diff_dir.join(format!("{}.diff", sanitize_id(id)));
        if let Err(e) = std::fs::write(&path, buf) {
            eprintln!("warning: writing {}: {e}", path.display());
        }
    }

    pub fn diff_path(&self, id: &str) -> PathBuf {
        self.diff_dir.join(format!("{}.diff", sanitize_id(id)))
    }
}

fn harness_error(mut result: TestResult, stage: &'static str, msg: &str) -> TestResult {
    // Infrastructure problems (spawn failure, unreadable sidecar) are
    // surfaced as ORACLE_FAIL-adjacent noise rather than test failures.
    result.verdict = Verdict::OracleFail;
    result.stage = stage;
    result.stderr_tail = format!("harness error: {msg}");
    result
}

/// One `cargo build` up front so N parallel `spinelc` invocations don't race
/// each other into cargo (spinelc's own build.rs checks rlib freshness
/// per-process anyway).
pub fn prebuild(workspace_root: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .args(["build", "--quiet", "-p", "spinelc", "-p", "spinel-rt"])
        .current_dir(workspace_root)
        .status()
        .map_err(|e| format!("running cargo build: {e}"))?;
    if !status.success() {
        return Err("cargo build failed".to_owned());
    }
    Ok(())
}
