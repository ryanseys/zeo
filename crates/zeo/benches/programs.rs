//! The runtime performance bank.
//!
//! Every `bench/bm_*.rb` is compiled with the release `zeo` (`-o`, the
//! shipped configuration) and the resulting NATIVE BINARY is timed
//! end-to-end -- one subprocess execution per iteration. The first run of
//! each program is a correctness gate against its committed `.expected`:
//! timing a wrong answer is meaningless, so a mismatch fails the bank.
//!
//! Two benchmark groups share the corpus:
//!
//! - `zeo/<name>` -- the compiled binary (always registered).
//! - `cruby/<name>` -- the oracle `ruby` on the same source, registered
//!   only when `ZEO_BENCH_ORACLE=1` (`ZEO_BENCH_ORACLE_RUBY` names the
//!   interpreter; default `ruby` from PATH).
//!
//! Workflows (criterion's own flags, after `--`):
//!
//! ```text
//! cargo bench -p zeo --bench programs                    # the whole bank
//! cargo bench -p zeo --bench programs -- 'zeo/bm_fib$'   # one benchmark
//! cargo bench -p zeo --bench programs -- --save-baseline before
//! cargo bench -p zeo --bench programs -- --baseline before
//! critcmp before after                                   # cross-run compare
//! ```
//!
//! An unfiltered bank compiles and gates every program up front, and the
//! gate run's duration sets that benchmark's target time -- so criterion's
//! 10 flat samples always fit and its "unable to complete 10 samples"
//! warning never fires. A FILTERED run keeps compilation lazy instead
//! (only what it times), at the price of that cosmetic warning on long
//! programs.
//!
//! The harness builds its OWN `zeo` + `libzeo.a` into an isolated target
//! dir (`target/bench/`), snapshotted once at bench start -- so editing
//! code, running tests, or `cargo build` in the ordinary target dir while
//! a bank runs cannot touch what is being timed.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use criterion::{Criterion, SamplingMode};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/zeo sits two levels below the repo root")
        .to_path_buf()
}

/// The harness's own `zeo` binary, built (with `libzeo.a` beside it) into
/// an isolated `target/bench/` dir the ordinary builds never touch. Built
/// ONCE at bench start: the whole bank times one source snapshot, however
/// long it runs and whatever happens in the main target dir meanwhile.
fn build_snapshot(root: &Path) -> PathBuf {
    let outer = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let bench_target = outer.join("bench");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "zeo"])
        .env("CARGO_TARGET_DIR", &bench_target)
        .current_dir(root)
        .status()
        .expect("spawn cargo");
    assert!(status.success(), "cargo build --release -p zeo failed");
    let zeo = bench_target.join("release/zeo");
    assert!(
        bench_target.join("release/libzeo.a").exists(),
        "libzeo.a missing beside {}",
        zeo.display()
    );
    zeo
}

/// The corpus: every `.rb` directly under `bench/`, sorted by name.
fn programs(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root.join("bench"))
        .expect("bench/ exists")
        .filter_map(|e| {
            let p = e.expect("readable bench/ entry").path();
            (p.extension().is_some_and(|x| x == "rb")).then_some(p)
        })
        .collect();
    v.sort();
    v
}

/// Whether the criterion CLI carries a positional filter (or `--list`).
/// With a filter present, eager setup would compile programs criterion
/// then skips, so the harness compiles lazily instead. Value-taking flags
/// are stepped over; `--flag=value` spellings and boolean flags fall to
/// the `-` check.
fn lazy_mode() -> bool {
    const VALUE_FLAGS: &[&str] = &[
        "-b",
        "--baseline",
        "-s",
        "--save-baseline",
        "--load-baseline",
        "--sample-size",
        "--warm-up-time",
        "--measurement-time",
        "--nresamples",
        "--noise-threshold",
        "--confidence-level",
        "--significance-level",
        "--profile-time",
        "--color",
        "--output-format",
        "--plotting-backend",
    ];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--list" {
            return true;
        }
        if VALUE_FLAGS.contains(&a.as_str()) {
            let _ = args.next();
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        return true;
    }
    false
}

/// A per-benchmark target time the 10 flat samples always fit in, derived
/// from the gate run's own duration. This never changes WHAT is measured
/// -- a long benchmark still takes exactly 10 single-execution samples --
/// it only sizes the plan so criterion stops warning about it.
fn target_for(one_run: Duration) -> Duration {
    (one_run * 12).max(Duration::from_secs(2))
}

/// The directory compiled programs land in -- beside the snapshot `zeo`
/// inside the isolated bench target dir, so concurrent trees (a worktree
/// control run beside the main tree) never collide on names.
fn scratch() -> PathBuf {
    let d = ZEO
        .get()
        .expect("main built the snapshot")
        .parent()
        .expect("the binary sits in release/")
        .join("programs");
    std::fs::create_dir_all(&d).expect("create the bench scratch dir");
    d
}

/// The snapshot `zeo` binary [`main`] built, for the per-benchmark
/// compiles.
static ZEO: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Compile `rb` with the snapshot `zeo` and answer the binary path.
fn compile(rb: &Path, name: &str) -> PathBuf {
    let bin = scratch().join(name);
    let out = Command::new(ZEO.get().expect("main built the snapshot"))
        .arg(rb)
        .args(["-o", bin.to_str().expect("utf-8 scratch path"), "-W0"])
        .output()
        .expect("spawn zeo");
    assert!(
        out.status.success(),
        "zeo failed on {}: {}",
        rb.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    bin
}

/// One gated run: stdout must match the committed `.expected` BYTES (some
/// benchmarks print binary output, e.g. bm_ao_render's PPM image).
/// Answers the run's wall time, the eager path's target-time estimate.
fn gate(cmd: &mut Command, rb: &Path, what: &str) -> Duration {
    let expected = std::fs::read(rb.with_extension("rb.expected"))
        .unwrap_or_else(|e| panic!("{}.expected: {e}", rb.display()));
    let t = Instant::now();
    let out = cmd.stderr(Stdio::null()).output().expect("spawn the benchmark");
    let took = t.elapsed();
    assert!(out.status.success(), "{what} exited {:?} on {}", out.status, rb.display());
    assert!(
        out.stdout == expected,
        "{what} output mismatch vs .expected on {}",
        rb.display()
    );
    took
}

/// Total wall time of `iters` silent executions of `cmd`.
fn time_runs(cmd: &mut Command, iters: u64) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let t = Instant::now();
        let status = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("spawn the benchmark");
        total += t.elapsed();
        assert!(status.success(), "benchmark exited {status:?}");
    }
    total
}

fn bench_zeo(c: &mut Criterion, corpus: &[PathBuf], lazy: bool) {
    let mut g = c.benchmark_group("zeo");
    g.sampling_mode(SamplingMode::Flat).sample_size(10);
    for rb in corpus {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        let id = name.clone();
        let rb = rb.clone();
        if lazy {
            // Compiled (and gated) on FIRST use, so a filtered run pays
            // only for what it selects. The cell outlives warmup +
            // measurement.
            let compiled: OnceCell<PathBuf> = OnceCell::new();
            g.bench_function(id, move |b| {
                let bin = compiled.get_or_init(|| {
                    let bin = compile(&rb, &name);
                    gate(&mut Command::new(&bin), &rb, "zeo binary");
                    bin
                });
                b.iter_custom(|iters| time_runs(&mut Command::new(bin), iters));
            });
        } else {
            let bin = compile(&rb, &name);
            let one_run = gate(&mut Command::new(&bin), &rb, "zeo binary");
            eprintln!("gate zeo/{name}: {:.3}s", one_run.as_secs_f64());
            g.measurement_time(target_for(one_run));
            g.bench_function(id, move |b| {
                b.iter_custom(|iters| time_runs(&mut Command::new(&bin), iters));
            });
        }
    }
    g.finish();
}

fn bench_cruby(c: &mut Criterion, corpus: &[PathBuf], lazy: bool) {
    let ruby =
        std::env::var("ZEO_BENCH_ORACLE_RUBY").unwrap_or_else(|_| "ruby".to_string());
    let mut g = c.benchmark_group("cruby");
    g.sampling_mode(SamplingMode::Flat).sample_size(10);
    for rb in corpus {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        let rb = rb.clone();
        let ruby = ruby.clone();
        // An oracle mismatch means the .expected snapshot is stale -- fail
        // loudly rather than banking a wrong comparison.
        if lazy {
            let gated: OnceCell<()> = OnceCell::new();
            g.bench_function(name, move |b| {
                gated.get_or_init(|| {
                    gate(Command::new(&ruby).arg(&rb), &rb, "oracle ruby");
                });
                b.iter_custom(|iters| time_runs(Command::new(&ruby).arg(&rb), iters));
            });
        } else {
            let one_run = gate(Command::new(&ruby).arg(&rb), &rb, "oracle ruby");
            eprintln!("gate cruby/{name}: {:.3}s", one_run.as_secs_f64());
            g.measurement_time(target_for(one_run));
            g.bench_function(name, move |b| {
                b.iter_custom(|iters| time_runs(Command::new(&ruby).arg(&rb), iters));
            });
        }
    }
    g.finish();
}

/// After an unfiltered bank: every benchmark's median, written to the
/// CHECKED-IN `bench/results.tsv`. Overwrite and commit -- the git diff
/// against the previous bank IS the progress record. `cruby` rows are
/// written only when the oracle group ran THIS bank, so a skipped oracle
/// never re-publishes stale numbers.
fn export_results(root: &Path, oracle: bool) {
    let outer = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let mut rows: Vec<(&str, String, f64)> = Vec::new();
    let groups: &[&str] = if oracle { &["zeo", "cruby"] } else { &["zeo"] };
    for group in groups {
        let dir = outer.join("criterion").join(group);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries {
            let p = e.expect("readable criterion entry").path();
            let Ok(bytes) = std::fs::read(p.join("new").join("estimates.json")) else {
                continue;
            };
            let v: serde_json::Value =
                serde_json::from_slice(&bytes).expect("criterion estimates.json parses");
            let ns = v["median"]["point_estimate"]
                .as_f64()
                .expect("a median point estimate");
            let name = p.file_name().expect("a benchmark dir").to_string_lossy().into_owned();
            rows.push((group, name, ns / 1e9));
        }
    }
    rows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let mut out = format!(
        "# bench/results.tsv -- medians from the last full bank (make bench) at {sha}\n\
         # group\tbenchmark\tmedian_secs\n"
    );
    for (g, n, s) in &rows {
        out.push_str(&format!("{g}\t{n}\t{s:.4}\n"));
    }
    std::fs::write(root.join("bench/results.tsv"), out).expect("write bench/results.tsv");
    eprintln!("wrote bench/results.tsv ({} rows)", rows.len());
}

fn main() {
    let root = repo_root();
    ZEO.set(build_snapshot(&root)).expect("main runs once");
    let corpus = programs(&root);
    assert!(!corpus.is_empty(), "no programs under bench/");
    let lazy = lazy_mode();
    let oracle = std::env::var_os("ZEO_BENCH_ORACLE").is_some_and(|v| v == "1");
    // Defaults BEFORE configure_from_args, so criterion's own CLI flags
    // (--warm-up-time, --measurement-time, ...) still win.
    let mut c = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .configure_from_args();
    bench_zeo(&mut c, &corpus, lazy);
    if oracle {
        bench_cruby(&mut c, &corpus, lazy);
    }
    c.final_summary();
    // A filtered run measured a subset; only a full bank rewrites the
    // committed record.
    if !lazy {
        export_results(&root, oracle);
    }
}
