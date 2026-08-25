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
//! Long programs (tens of seconds) exceed the 2s target time; criterion
//! prints a warning and takes its 10 flat samples anyway -- that warning is
//! expected, not a problem. Compilation happens lazily inside each
//! benchmark, so a filtered run compiles only what it times.
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

/// The snapshot `zeo` binary [`main`] built, for the lazy per-benchmark
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
fn gate(cmd: &mut Command, rb: &Path, what: &str) {
    let expected = std::fs::read(rb.with_extension("rb.expected"))
        .unwrap_or_else(|e| panic!("{}.expected: {e}", rb.display()));
    let out = cmd.stderr(Stdio::null()).output().expect("spawn the benchmark");
    assert!(out.status.success(), "{what} exited {:?} on {}", out.status, rb.display());
    assert!(
        out.stdout == expected,
        "{what} output mismatch vs .expected on {}",
        rb.display()
    );
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

fn bench_zeo(c: &mut Criterion, corpus: &[PathBuf]) {
    let mut g = c.benchmark_group("zeo");
    g.sampling_mode(SamplingMode::Flat).sample_size(10);
    for rb in corpus {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        let id = name.clone();
        let rb = rb.clone();
        // Compiled (and gated) on FIRST use, so a filtered run pays only
        // for what it selects. The cell outlives warmup + measurement.
        let compiled: OnceCell<PathBuf> = OnceCell::new();
        g.bench_function(id, move |b| {
            let bin = compiled.get_or_init(|| {
                let bin = compile(&rb, &name);
                gate(&mut Command::new(&bin), &rb, "zeo binary");
                bin
            });
            b.iter_custom(|iters| time_runs(&mut Command::new(bin), iters));
        });
    }
    g.finish();
}

fn bench_cruby(c: &mut Criterion, corpus: &[PathBuf]) {
    let ruby =
        std::env::var("ZEO_BENCH_ORACLE_RUBY").unwrap_or_else(|_| "ruby".to_string());
    let mut g = c.benchmark_group("cruby");
    g.sampling_mode(SamplingMode::Flat).sample_size(10);
    for rb in corpus {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        let rb = rb.clone();
        let ruby = ruby.clone();
        let gated: OnceCell<()> = OnceCell::new();
        g.bench_function(name, move |b| {
            // An oracle mismatch means the .expected snapshot is stale --
            // fail loudly rather than banking a wrong comparison.
            gated.get_or_init(|| {
                gate(Command::new(&ruby).arg(&rb), &rb, "oracle ruby")
            });
            b.iter_custom(|iters| time_runs(Command::new(&ruby).arg(&rb), iters));
        });
    }
    g.finish();
}

fn main() {
    let root = repo_root();
    ZEO.set(build_snapshot(&root)).expect("main runs once");
    let corpus = programs(&root);
    assert!(!corpus.is_empty(), "no programs under bench/");
    // Defaults BEFORE configure_from_args, so criterion's own CLI flags
    // (--warm-up-time, --measurement-time, ...) still win.
    let mut c = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .configure_from_args();
    bench_zeo(&mut c, &corpus);
    if std::env::var_os("ZEO_BENCH_ORACLE").is_some_and(|v| v == "1") {
        bench_cruby(&mut c, &corpus);
    }
    c.final_summary();
}
