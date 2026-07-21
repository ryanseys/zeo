//! `cargo run -p xtask -- bench [--filter <substr>] [--runs N] [--update-baseline]`
//!
//! The performance governor for the structural work: compiles every
//! `bench/bm_*.rb` with `zeo -o` (so generated programs link the RELEASE
//! runtime, the shipped configuration), verifies its stdout against the
//! committed `.expected` (a wrong answer fails the run outright -- timing a
//! wrong answer is meaningless), then times the binary and compares against
//! `bench/baseline.tsv`.
//!
//! Timing discipline: `--runs N` (default 3) executions, keeping the MINIMUM
//! wall time -- the standard best-of-N convention for wall-clock benches,
//! since noise on a quiet machine is strictly additive. Perf-sensitive
//! changes (preemption checkpoints, dispatch changes, engine swaps) report
//! the printed delta table at their phase gate; intentional shifts are
//! banked by committing `--update-baseline`'s diff.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

/// One benchmark's timing outcome, in seconds.
struct BenchResult {
    name: String,
    secs: f64,
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let mut filter: Option<String> = None;
    let mut runs: usize = 3;
    let mut update_baseline = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--filter" => filter = it.next().cloned(),
            "--runs" => {
                runs = it
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| usage("--runs takes a positive integer"))
            }
            "--update-baseline" => update_baseline = true,
            other => usage(&format!("unknown flag {other}")),
        }
    }
    if runs == 0 {
        usage("--runs takes a positive integer");
    }

    // Release zeo binary: the compiler itself is not what's being measured,
    // but a debug zeo drags per-bench compile time; and `-o` selects the
    // release runtime for the GENERATED programs, which is what we time.
    let build = Command::new("cargo")
        .args(["build", "--quiet", "--release", "-p", "zeo"])
        .current_dir(root)
        .status()
        .unwrap_or_else(|e| panic!("running cargo build: {e}"));
    if !build.success() {
        eprintln!("xtask bench: zeo failed to build");
        return ExitCode::FAILURE;
    }
    let zeo_bin = root.join("target/release/zeo");

    let baseline_path = root.join("bench/baseline.tsv");
    let baseline = read_baseline(&baseline_path);

    let mut results: Vec<BenchResult> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for rb in bench_programs(root) {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        if let Some(f) = &filter {
            if !name.contains(f.as_str()) {
                continue;
            }
        }
        match run_one(&zeo_bin, &rb, runs) {
            Ok(secs) => {
                let delta = baseline
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, base)| format!("{:+6.1}% vs {base:.3}s", (secs / base - 1.0) * 100.0))
                    .unwrap_or_else(|| "(no baseline)".to_string());
                println!("{name:<28} {secs:>8.3}s  {delta}");
                results.push(BenchResult { name, secs });
            }
            Err(msg) => {
                println!("{name:<28} FAIL  {msg}");
                failures.push(name);
            }
        }
    }

    if !failures.is_empty() {
        println!(
            "\n{} benchmark(s) failed: {}",
            failures.len(),
            failures.join(", ")
        );
        return ExitCode::FAILURE;
    }
    if results.is_empty() {
        println!("no benchmarks matched");
        return ExitCode::FAILURE;
    }

    // Geometric mean of per-bench ratios -- the one aggregate that treats a
    // 2x win on a fast bench and a 2x loss on a slow one symmetrically.
    let ratios: Vec<f64> = results
        .iter()
        .filter_map(|r| {
            baseline
                .iter()
                .find(|(n, _)| *n == r.name)
                .map(|(_, base)| r.secs / base)
        })
        .collect();
    if !ratios.is_empty() {
        let geomean = (ratios.iter().map(|r| r.ln()).sum::<f64>() / ratios.len() as f64).exp();
        println!(
            "\ngeomean vs baseline: {:+.1}% over {} benchmark(s)",
            (geomean - 1.0) * 100.0,
            ratios.len()
        );
    }

    if update_baseline {
        let mut out = String::from("# bench/baseline.tsv -- xtask bench --update-baseline\n");
        for r in &results {
            out.push_str(&format!("{}\t{:.3}\n", r.name, r.secs));
        }
        std::fs::write(&baseline_path, out)
            .unwrap_or_else(|e| panic!("writing {}: {e}", baseline_path.display()));
        println!("baseline updated: {}", baseline_path.display());
    }
    ExitCode::SUCCESS
}

/// Compile, verify against `.expected`, and time (min of `runs`).
fn run_one(zeo_bin: &Path, rb: &Path, runs: usize) -> Result<f64, String> {
    let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
    let expected_path = PathBuf::from(format!("{}.expected", rb.display()));
    let expected = std::fs::read_to_string(&expected_path)
        .map_err(|e| format!("reading {}: {e}", expected_path.display()))?;

    let bin_path = std::env::temp_dir().join(format!("zeo-bench-{name}"));
    let compile = Command::new(zeo_bin)
        .arg(rb)
        .arg("-o")
        .arg(&bin_path)
        .arg("--no-report")
        .output()
        .map_err(|e| format!("invoking zeo: {e}"))?;
    if !compile.status.success() {
        return Err(format!(
            "zeo failed: {}",
            String::from_utf8_lossy(&compile.stderr)
                .lines()
                .next()
                .unwrap_or("")
        ));
    }

    let mut best: Option<f64> = None;
    for i in 0..runs {
        let started = Instant::now();
        let run = Command::new(&bin_path)
            .output()
            .map_err(|e| format!("running compiled binary: {e}"))?;
        let secs = started.elapsed().as_secs_f64();
        if !run.status.success() {
            let _ = std::fs::remove_file(&bin_path);
            return Err(format!("exited {:?}", run.status.code()));
        }
        // Correctness gate on the first run only; repeats time a known-good
        // binary without re-diffing identical output.
        if i == 0 {
            let actual = String::from_utf8_lossy(&run.stdout);
            if actual != expected {
                let _ = std::fs::remove_file(&bin_path);
                return Err("output mismatch vs .expected".to_string());
            }
        }
        best = Some(best.map_or(secs, |b: f64| b.min(secs)));
    }
    let _ = std::fs::remove_file(&bin_path);
    Ok(best.expect("runs >= 1"))
}

fn bench_programs(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("bench");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "rb"))
        .collect();
    files.sort();
    files
}

/// `name\tseconds` rows; `#`-prefixed lines are comments. Missing file =
/// empty baseline (every bench reports "(no baseline)").
fn read_baseline(path: &Path) -> Vec<(String, f64)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let (name, secs) = l.split_once('\t')?;
            Some((name.to_string(), secs.parse().ok()?))
        })
        .collect()
}

fn usage(msg: &str) -> ! {
    eprintln!("xtask bench: {msg}");
    eprintln!(
        "usage: cargo run -p xtask -- bench [--filter <substr>] [--runs N] [--update-baseline]"
    );
    std::process::exit(2);
}
