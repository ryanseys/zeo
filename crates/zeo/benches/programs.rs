//! The runtime performance bank.
//!
//! Every `test/bench/bm_*.rb` is compiled with the release `zeo` (`-o`, the
//! shipped configuration) and the resulting NATIVE BINARY is timed
//! end-to-end -- one subprocess execution per iteration. The first run of
//! each program is a correctness gate against the answer recorded under its
//! `__END__`:
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
//! ZEO_BENCH_DIST=pgo cargo bench -p zeo --bench programs # ship config
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

// The corpus file format: the recorded answer under `__END__`.
#[path = "../tests/corpus/case.rs"]
#[allow(dead_code)]
mod case;

use std::cell::OnceCell;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use criterion::{Criterion, SamplingMode};
use sha2::{Digest, Sha256};

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
///
/// `ZEO_BENCH_DIST=pgo` swaps the snapshot for the SHIPPED configuration:
/// the full `cargo xtask dist --pgo` pipeline (instrument, train, profile-use
/// rebuild) staged inside the same isolated dir. It costs ~15 minutes of
/// setup, so the everyday bank stays on release -- and a dist-mode bank
/// never rewrites the committed `bench/results.tsv` (that file is the
/// release-profile diff chain); compare dist banks with
/// `--save-baseline` + `critcmp` instead.
fn build_snapshot(root: &Path) -> PathBuf {
    let outer = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let bench_target = outer.join("bench");
    match std::env::var("ZEO_BENCH_DIST") {
        Err(_) => {}
        Ok(v) if v == "pgo" => return build_dist_snapshot(root, &bench_target),
        Ok(v) => panic!("ZEO_BENCH_DIST={v} is not a mode (only \"pgo\")"),
    }
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

/// The `ZEO_BENCH_DIST=pgo` snapshot: `cargo xtask dist --pgo` staged into
/// the isolated bench target dir, so its builds and training profiles
/// never touch the ordinary target dir either. `--no-smoke` because the
/// bank's own recorded-answer gate is the stronger check.
fn build_dist_snapshot(root: &Path, bench_target: &Path) -> PathBuf {
    let stage = bench_target.join("dist-stage");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["xtask", "dist", "--pgo", "--no-smoke", "-o"])
        .arg(&stage)
        .env("CARGO_TARGET_DIR", bench_target)
        .current_dir(root)
        .status()
        .expect("spawn cargo xtask dist");
    assert!(status.success(), "cargo xtask dist --pgo failed");
    let tree = std::fs::read_dir(&stage)
        .expect("the dist stage exists")
        .filter_map(|e| {
            let p = e.expect("readable stage entry").path();
            p.is_dir().then_some(p)
        })
        .next()
        .expect("the stage holds one zeo-<version>-<triple> tree");
    let zeo = tree.join("bin/zeo");
    assert!(zeo.exists(), "no staged binary at {}", zeo.display());
    zeo
}

/// The corpus: every `.rb` directly under `test/bench/`, sorted by name.
fn programs(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root.join("test/bench"))
        .expect("test/bench/ exists")
        .filter_map(|e| {
            let p = e.expect("readable test/bench/ entry").path();
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
///
/// Options BEFORE the file. zeo follows ruby's own convention, where
/// everything after the script name is the PROGRAM's ARGV -- so `zeo
/// prog.rb -o bin` RUNS prog.rb and hands it `-o bin`, exits 0, and writes
/// no binary. This bank spelled it that way and died on the first
/// benchmark, at the spawn of a file that was never produced.
fn compile(rb: &Path, name: &str) -> PathBuf {
    let bin = scratch().join(name);
    let out = Command::new(ZEO.get().expect("main built the snapshot"))
        .args(["-o", bin.to_str().expect("utf-8 scratch path"), "-W0"])
        .arg(rb)
        .output()
        .expect("spawn zeo");
    assert!(
        out.status.success(),
        "zeo failed on {}: {}",
        rb.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        bin.is_file(),
        "zeo exited 0 on {} but wrote no binary at {} -- are the options \
         before the file?",
        rb.display(),
        bin.display()
    );
    bin
}

/// One gated run: stdout must match the recorded answer's BYTES (some
/// benchmarks print binary output, e.g. bm_ao_render's PPM image).
/// Answers the run's wall time, the eager path's target-time estimate.
fn gate(cmd: &mut Command, rb: &Path, what: &str) -> Duration {
    let expected = case::Case::read(rb)
        .unwrap_or_else(|e| panic!("{e}"))
        .trailer
        .unwrap_or_else(|| panic!("{}: no recorded answer under __END__", rb.display()))
        .answer
        .stdout;
    let t = Instant::now();
    let out = cmd
        .stderr(Stdio::null())
        .output()
        .expect("spawn the benchmark");
    let took = t.elapsed();
    assert!(
        out.status.success(),
        "{what} exited {:?} on {}",
        out.status,
        rb.display()
    );
    assert!(
        out.stdout == expected,
        "{what} output mismatch vs the recorded answer on {}",
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

/// The outer target dir -- where criterion writes and the journal lives.
/// NOT `target/bench/`, which holds only the snapshot build.
fn outer_target(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
}

/// Where criterion leaves one benchmark's analysis.
fn estimates_path(root: &Path, group: &str, name: &str) -> PathBuf {
    outer_target(root)
        .join("criterion")
        .join(group)
        .join(name)
        .join("new")
        .join("estimates.json")
}

/// One benchmark's median, off criterion's own analysis.
fn criterion_median(root: &Path, group: &str, name: &str) -> Option<f64> {
    let bytes = std::fs::read(estimates_path(root, group, name)).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(v["median"]["point_estimate"].as_f64()? / 1e9)
}

/// Short content hash. 16 hex chars: a bank is ~120 rows, so collision is
/// not the risk being managed here -- staleness is.
fn digest(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        // Length-prefixed, so ("ab","c") and ("a","bc") differ.
        h.update((p.len() as u64).to_le_bytes());
        h.update(p);
    }
    format!("{:x}", h.finalize())[..16].to_string()
}

/// The append-as-you-go record, one row per finished benchmark.
///
/// `export_results` runs ONCE, after the last benchmark. A bank is 40+
/// minutes, so anything that stops it early -- a timeout, a Ctrl-C, a
/// laptop lid -- would discard every number it had already paid for. The
/// journal is the durable half: appended and flushed per benchmark, in
/// `target/bench/journal.tsv`.
///
/// `key` is what makes a row reusable rather than merely readable. It
/// hashes the program, its `.expected`, and the IDENTITY of what was timed
/// -- for zeo the snapshot `zeo` plus `libzeo.a` (compiler and runtime),
/// for the oracle `ruby -v` (version and revision). A row whose key still
/// matches measured what a re-run would measure.
///
/// **Reuse is opt-in** (`ZEO_BENCH_RESUME=1`). Quietly mixing sittings is
/// how a bank starts lying: ambient drift of +2.6%, and once a whole-host
/// shift of ~65%, have both been measured on this machine. A resumed bank
/// is for finishing an interrupted run, not for skipping work.
struct Journal {
    path: PathBuf,
    prior: HashMap<(String, String), (String, f64)>,
    resume: bool,
    /// When this bank started. An `estimates.json` older than this was
    /// left by an EARLIER run: criterion keeps the last result on disk for
    /// every benchmark, including ones the current filter skipped, so
    /// "the file is there" does not mean "it was measured just now".
    /// Journalling those would stamp stale numbers with a fresh timestamp,
    /// which is the exact lie this file exists to prevent.
    started: SystemTime,
}

impl Journal {
    fn open(root: &Path) -> Self {
        let dir = outer_target(root).join("bench");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("journal.tsv");
        let mut prior = HashMap::new();
        for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
            if line.starts_with('#') {
                continue;
            }
            // group, benchmark, median_secs, key, measured_at_epoch
            let f: Vec<&str> = line.split('\t').collect();
            let [group, name, secs, key, _at] = f[..] else {
                continue;
            };
            let Ok(secs) = secs.parse() else { continue };
            prior.insert((group.into(), name.into()), (key.to_string(), secs));
        }
        if !path.exists() {
            let _ = std::fs::write(
                &path,
                "# appended per benchmark by `cargo bench -p zeo --bench programs`\n\
                 # group\tbenchmark\tmedian_secs\tkey\tmeasured_at_epoch\n",
            );
        }
        let resume = std::env::var_os("ZEO_BENCH_RESUME").is_some_and(|v| v == "1");
        Self {
            path,
            prior,
            resume,
            started: SystemTime::now(),
        }
    }

    /// A usable prior measurement for this exact input, when resuming.
    fn reusable(&self, group: &str, name: &str, key: &str) -> Option<f64> {
        if !self.resume {
            return None;
        }
        let (had, secs) = self.prior.get(&(group.into(), name.into()))?;
        (had == key).then_some(*secs)
    }

    fn record(&self, group: &str, name: &str, key: &str, secs: f64) {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&self.path) else {
            eprintln!("journal: cannot append {group}/{name}");
            return;
        };
        // Flushed per row on purpose: the whole point is surviving a kill.
        let _ = writeln!(f, "{group}\t{name}\t{secs:.6}\t{key}\t{at}");
        let _ = f.flush();
    }

    /// Journal what criterion measured for this benchmark IN THIS RUN.
    ///
    /// A filtered bank walks the whole corpus and calls this for every
    /// benchmark, but criterion only re-times the ones the filter selected
    /// -- so the estimate's mtime, not its existence, is what says whether
    /// there is a fresh number to record.
    fn record_measured(&self, root: &Path, group: &str, name: &str, key: &str) {
        let path = estimates_path(root, group, name);
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .is_ok_and(|m| m >= self.started);
        if !fresh {
            return;
        }
        match criterion_median(root, group, name) {
            Some(secs) => self.record(group, name, key, secs),
            // Loud: a silently unjournalled row is the failure this exists
            // to prevent.
            None => eprintln!("journal: no criterion estimate for {group}/{name}"),
        }
    }
}

/// `[N/M pct]` progress line ahead of one benchmark -- what a `tail -f`
/// of a bank's log reads to see how far along the run is.
fn progress(done: usize, total: usize, group: &str, name: &str) {
    let n = done + 1;
    eprintln!(
        "[{n}/{total} {:.0}%] {group}/{name}",
        done as f64 * 100.0 / total as f64
    );
}

/// This benchmark's journal key: the program, its expected output, and the
/// identity of whatever runs it.
fn key_for(group: &str, name: &str, rb: &Path, identity: &str) -> String {
    let src = std::fs::read(rb).unwrap_or_default();
    let expected = std::fs::read(rb.with_extension("rb.expected")).unwrap_or_default();
    digest(&[
        group.as_bytes(),
        name.as_bytes(),
        &src,
        &expected,
        identity.as_bytes(),
    ])
}

/// What a zeo timing is OF: the snapshot compiler and the runtime archive
/// it links into every benchmark. Rebuild either and all 61 rows go stale.
fn zeo_identity(zeo: &Path) -> String {
    let bin = std::fs::read(zeo).expect("read the snapshot zeo");
    let lib = zeo
        .parent()
        .map(|d| d.join("libzeo.a"))
        .and_then(|p| std::fs::read(p).ok())
        .unwrap_or_default();
    digest(&[&bin, &lib])
}

/// `ruby -v` carries version, revision and platform -- enough to say a
/// re-run would time the same interpreter.
///
/// `RECIPE` covers the other half: HOW the oracle is invoked. Changing the
/// command line changes the timing without changing the interpreter, so a
/// journal row measured under a different recipe must not look reusable. Bump
/// it whenever [`oracle_cmd`] changes.
fn ruby_identity(ruby: &str) -> String {
    const RECIPE: &str = "bare-ruby-no-bundler-v2";
    let out = Command::new(ruby)
        .arg("-v")
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    digest(&[&out, RECIPE.as_bytes()])
}

fn bench_zeo(
    c: &mut Criterion,
    corpus: &[PathBuf],
    lazy: bool,
    done: &mut usize,
    total: usize,
    journal: &Journal,
) {
    let identity = zeo_identity(ZEO.get().expect("main built the snapshot"));
    let root = repo_root();
    let mut g = c.benchmark_group("zeo");
    g.sampling_mode(SamplingMode::Flat).sample_size(10);
    for rb in corpus {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        let key = key_for("zeo", &name, rb, &identity);
        let id = name.clone();
        let rb = rb.clone();
        if lazy {
            // Compiled (and gated) on FIRST use, so a filtered run pays
            // only for what it selects. The cell outlives warmup +
            // measurement.
            let compiled: OnceCell<PathBuf> = OnceCell::new();
            g.bench_function(id, |b| {
                let bin = compiled.get_or_init(|| {
                    let bin = compile(&rb, &name);
                    gate(&mut Command::new(&bin), &rb, "zeo binary");
                    bin
                });
                b.iter_custom(|iters| time_runs(&mut Command::new(bin), iters));
            });
            journal.record_measured(&root, "zeo", &name, &key);
        } else {
            progress(*done, total, "zeo", &name);
            *done += 1;
            if let Some(secs) = journal.reusable("zeo", &name, &key) {
                eprintln!("resume zeo/{name}: {secs:.4}s from the journal");
                continue;
            }
            let bin = compile(&rb, &name);
            let one_run = gate(&mut Command::new(&bin), &rb, "zeo binary");
            eprintln!("gate zeo/{name}: {:.3}s", one_run.as_secs_f64());
            g.measurement_time(target_for(one_run));
            g.bench_function(id, |b| {
                b.iter_custom(|iters| time_runs(&mut Command::new(&bin), iters));
            });
            journal.record_measured(&root, "zeo", &name, &key);
        }
    }
    g.finish();
}

/// The oracle: a bare `ruby <prog.rb>` with the ambient library dials
/// cleared, so nothing anybody `gem install`ed decides a timing.
///
/// **No `-rbundler/setup` here, unlike the goldens' oracle.** That costs 60
/// ms of interpreter startup, and this bank charges zeo nothing comparable
/// -- it times a native binary with no loader at all. The goldens need
/// bundler because they compare OUTPUT and a wrongly-resolved library
/// answers differently; the bank compares TIME, and 60 ms against
/// programs a third of which finish inside 100 ms is not a difference
/// between zeo and ruby, it is a difference between two command lines.
///
/// It was measured. With `bundler/setup` the CRuby side of 25 benchmarks
/// came out 35% slower and zeo's ratio rose from 1.93x to 2.92x, having
/// changed nothing about zeo. That number would have been a fabrication.
///
/// This is sound only while no bench program requires anything --
/// [`assert_corpus_needs_no_bundler`] holds the bank to it.
fn oracle_cmd(ruby: &str, rb: &Path) -> Command {
    let mut cmd = Command::new(ruby);
    cmd.arg(rb)
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .env_remove("GEM_HOME")
        .env_remove("GEM_PATH")
        .env_remove("GEM_SPEC_CACHE");
    cmd
}

/// The bench corpus is pure computation: 61 programs, not one `require`.
/// That is what lets the oracle skip bundler and stay a fair comparison.
/// Add a program that requires a gem and this fails, which is the moment
/// to decide how BOTH sides should resolve it -- not to quietly hand one
/// of them a loader the other never runs.
fn assert_corpus_needs_no_bundler(corpus: &[PathBuf]) {
    let mut needy = Vec::new();
    for rb in corpus {
        let src = std::fs::read_to_string(rb).unwrap_or_default();
        if src.lines().any(|l| {
            l.trim_start().starts_with("require ") || l.trim_start().starts_with("require(")
        }) {
            needy.push(rb.file_stem().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(
        needy.is_empty(),
        "these bench programs `require` something, so the oracle's library \
         resolution is back in question: {needy:?}\nSee oracle_cmd."
    );
}

fn bench_cruby(
    c: &mut Criterion,
    corpus: &[PathBuf],
    lazy: bool,
    done: &mut usize,
    total: usize,
    journal: &Journal,
) {
    let ruby = std::env::var("ZEO_BENCH_ORACLE_RUBY").unwrap_or_else(|_| "ruby".to_string());
    let identity = ruby_identity(&ruby);
    let root = repo_root();
    let mut g = c.benchmark_group("cruby");
    g.sampling_mode(SamplingMode::Flat).sample_size(10);
    for rb in corpus {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        let key = key_for("cruby", &name, rb, &identity);
        let rb = rb.clone();
        let ruby = ruby.clone();
        // An oracle mismatch means the .expected snapshot is stale -- fail
        // loudly rather than banking a wrong comparison.
        if lazy {
            let gated: OnceCell<()> = OnceCell::new();
            g.bench_function(name.clone(), |b| {
                gated.get_or_init(|| {
                    gate(&mut oracle_cmd(&ruby, &rb), &rb, "oracle ruby");
                });
                b.iter_custom(|iters| time_runs(&mut oracle_cmd(&ruby, &rb), iters));
            });
            journal.record_measured(&root, "cruby", &name, &key);
        } else {
            progress(*done, total, "cruby", &name);
            *done += 1;
            if let Some(secs) = journal.reusable("cruby", &name, &key) {
                eprintln!("resume cruby/{name}: {secs:.4}s from the journal");
                continue;
            }
            let one_run = gate(&mut oracle_cmd(&ruby, &rb), &rb, "oracle ruby");
            eprintln!("gate cruby/{name}: {:.3}s", one_run.as_secs_f64());
            g.measurement_time(target_for(one_run));
            g.bench_function(name.clone(), |b| {
                b.iter_custom(|iters| time_runs(&mut oracle_cmd(&ruby, &rb), iters));
            });
            journal.record_measured(&root, "cruby", &name, &key);
        }
    }
    g.finish();
}

/// After an unfiltered bank: every benchmark's median, written to the
/// CHECKED-IN `bench/results.tsv`. Overwrite and commit -- the git diff
/// against the previous bank IS the progress record. `cruby` rows carry
/// over from whatever data the last oracle run left in
/// `target/criterion` -- the oracle is a MANUAL, once-in-a-while group
/// (`ZEO_BENCH_ORACLE=1`); a zeo-only bank keeps the standing CRuby
/// medians beside its fresh zeo ones so the comparison never vanishes.
fn export_results(root: &Path, _oracle: bool) {
    let outer = outer_target(root);
    let mut rows: Vec<(&str, String, f64)> = Vec::new();
    let groups: &[&str] = &["zeo", "cruby"];
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
            let name = p
                .file_name()
                .expect("a benchmark dir")
                .to_string_lossy()
                .into_owned();
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
    assert_corpus_needs_no_bundler(&corpus);
    let lazy = lazy_mode();
    let oracle = std::env::var_os("ZEO_BENCH_ORACLE").is_some_and(|v| v == "1");
    // Defaults BEFORE configure_from_args, so criterion's own CLI flags
    // (--warm-up-time, --measurement-time, ...) still win.
    let mut c = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .configure_from_args();
    let total = corpus.len() * if oracle { 2 } else { 1 };
    let mut done = 0;
    let journal = Journal::open(&root);
    bench_zeo(&mut c, &corpus, lazy, &mut done, total, &journal);
    if oracle {
        bench_cruby(&mut c, &corpus, lazy, &mut done, total, &journal);
    }
    c.final_summary();
    // A filtered run measured a subset, and a dist-mode bank measured a
    // different profile than the committed release chain records; only a
    // full RELEASE bank rewrites the committed record.
    if !lazy {
        if std::env::var_os("ZEO_BENCH_DIST").is_some() {
            eprintln!("dist-mode bank: bench/results.tsv (release chain) left untouched");
        } else {
            export_results(&root, oracle);
        }
    }
}
