//! `cargo run -p xtask -- compile-bench [--filter <substr>] [--runs N] [--update-baseline]`
//!
//! The compile-side twin of `bench.rs`: instead of timing the RUNTIME of the
//! generated binaries, this measures what the COMPILER produces and costs on a
//! fixed program set spanning hello-world to bundler scale. Per program:
//!
//! - `frontend_ms` -- best-of-N wall time of `zeo <file> -S` (parse -> lower ->
//!   analyze -> codegen -> emit, no rustc)
//! - `rust_lines`  -- line count of the `-S` (pretty) output
//! - `rust_bytes`  -- byte length of the build-path source (the `bytes=` field
//!   of the compile's `zeo-timings:` line)
//! - `rustc_ms` / `bin_bytes` -- one `zeo -o` build under `ZEO_CACHE=bypass`
//!   (release runtime, static linkage: the shipped configuration)
//! - `warnings` -- rustc warnings emitted while compiling the generated
//!   program (the generated-code cleanliness gauge; target is 0)
//!
//! Results land in `bench/compile-baseline.tsv` via `--update-baseline`, the
//! committed regression reference for compiler speed and generated-code size
//! -- same banking convention as `bench/baseline.tsv`.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Instant;

/// The fixed program set: names paired with repo-relative paths. `hello` is
/// synthesized (written to a temp file) so the smallest-possible program is
/// always in the record.
const PROGRAMS: &[(&str, &str)] = &[
    ("hello", ""),
    ("bm_fib", "bench/bm_fib.rb"),
    ("bm_template", "bench/bm_template.rb"),
    ("bm_json_parse", "bench/bm_json_parse.rb"),
    ("bm_micro_lisp", "bench/bm_micro_lisp.rb"),
    ("core_classes", "tests/core_classes.rb"),
    ("uri_parse_and_build", "tests/uri_parse_and_build.rb"),
    ("optparse_subset", "tests/optparse_subset.rb"),
    ("gem_rubygems", "tests/gem_rubygems.rb"),
    ("gem_bundler", "tests/gem_bundler.rb"),
];

#[derive(Clone)]
struct Row {
    name: String,
    frontend_ms: u64,
    rust_lines: u64,
    rust_bytes: u64,
    rustc_ms: u64,
    bin_bytes: u64,
    warnings: u64,
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
                runs = it.next().and_then(|n| n.parse().ok()).unwrap_or_else(|| {
                    eprintln!("compile-bench: --runs takes a positive integer");
                    std::process::exit(2);
                })
            }
            "--update-baseline" => update_baseline = true,
            other => {
                eprintln!("compile-bench: unknown flag {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    if runs == 0 {
        eprintln!("compile-bench: --runs takes a positive integer");
        return ExitCode::FAILURE;
    }

    // Release zeo: compiler wall time is the metric, so measure the shipped
    // configuration, not a debug build.
    let build = Command::new("cargo")
        .args(["build", "--quiet", "--release", "-p", "zeo"])
        .current_dir(root)
        .status()
        .unwrap_or_else(|e| panic!("running cargo build: {e}"));
    if !build.success() {
        eprintln!("compile-bench: zeo failed to build");
        return ExitCode::FAILURE;
    }
    let zeo_bin = root.join("target/release/zeo");

    let baseline_path = root.join("bench/compile-baseline.tsv");
    let baseline = read_baseline(&baseline_path);

    let hello = std::env::temp_dir().join("zeo-compile-bench-hello.rb");
    std::fs::write(&hello, "puts 1+1\n")
        .unwrap_or_else(|e| panic!("writing {}: {e}", hello.display()));

    let mut rows: Vec<Row> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (name, rel) in PROGRAMS {
        if let Some(f) = &filter
            && !name.contains(f.as_str()) {
                continue;
            }
        let rb = if rel.is_empty() {
            hello.clone()
        } else {
            root.join(rel)
        };
        match run_one(&zeo_bin, name, &rb, runs) {
            Ok(row) => {
                report(&row, baseline.iter().find(|b| b.name == row.name));
                rows.push(row);
                // Persist after EVERY program (same interruption-safety as
                // bench.rs): rows outside this run's filter are carried
                // forward, so a filtered or interrupted update never drops
                // the rest of the file.
                if update_baseline {
                    let mut merged: Vec<Row> = baseline
                        .iter()
                        .filter(|b| !rows.iter().any(|r| r.name == b.name))
                        .cloned()
                        .collect();
                    merged.extend(rows.iter().cloned());
                    merged.sort_by(|a, b| a.name.cmp(&b.name));
                    write_baseline(&baseline_path, &merged);
                }
            }
            Err(msg) => {
                progress(&format!("{name:<22} FAIL  {msg}"));
                failures.push(name.to_string());
            }
        }
    }

    if !failures.is_empty() {
        println!(
            "\n{} program(s) failed: {}",
            failures.len(),
            failures.join(", ")
        );
        return ExitCode::FAILURE;
    }
    if rows.is_empty() {
        println!("no programs matched");
        return ExitCode::FAILURE;
    }

    if update_baseline {
        println!("\nbaseline updated: {}", baseline_path.display());
    }
    ExitCode::SUCCESS
}

fn report(row: &Row, base: Option<&Row>) {
    let delta = |now: u64, then: u64| -> String {
        if then == 0 {
            return String::new();
        }
        format!(" ({:+.1}%)", (now as f64 / then as f64 - 1.0) * 100.0)
    };
    let (fd, ld, rd, bd) = match base {
        Some(b) => (
            delta(row.frontend_ms, b.frontend_ms),
            delta(row.rust_lines, b.rust_lines),
            delta(row.rustc_ms, b.rustc_ms),
            delta(row.bin_bytes, b.bin_bytes),
        ),
        None => (String::new(), String::new(), String::new(), String::new()),
    };
    progress(&format!(
        "{:<22} frontend {:>6}ms{fd}  lines {:>8}{ld}  rustc {:>7}ms{rd}  bin {:>9}B{bd}  warnings {}",
        row.name, row.frontend_ms, row.rust_lines, row.rustc_ms, row.bin_bytes, row.warnings
    ));
}

fn run_one(zeo_bin: &Path, name: &str, rb: &Path, runs: usize) -> Result<Row, String> {
    // Frontend: best-of-N `-S` wall time; the last run's stdout supplies the
    // pretty line count.
    let mut best_ms: Option<u64> = None;
    let mut rust_lines = 0u64;
    for _ in 0..runs {
        let started = Instant::now();
        let out = Command::new(zeo_bin)
            .arg(rb)
            .arg("-S")
            .arg("--no-report")
            .output()
            .map_err(|e| format!("invoking zeo -S: {e}"))?;
        let ms = started.elapsed().as_millis() as u64;
        if !out.status.success() {
            return Err(format!(
                "zeo -S failed: {}",
                String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .next()
                    .unwrap_or("")
            ));
        }
        rust_lines = out.stdout.iter().filter(|&&b| b == b'\n').count() as u64;
        best_ms = Some(best_ms.map_or(ms, |b| b.min(ms)));
    }

    // Build: one `-o` under cache bypass; `zeo-timings:` lines carry the
    // build-path source bytes, the rustc wall time, and the binary size.
    let bin_path = std::env::temp_dir().join(format!("zeo-compile-bench-{name}"));
    let out = Command::new(zeo_bin)
        .arg(rb)
        .arg("-o")
        .arg(&bin_path)
        .arg("--no-report")
        .env("ZEO_TIMINGS", "1")
        .env("ZEO_CACHE", "bypass")
        .output()
        .map_err(|e| format!("invoking zeo -o: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let msg = stderr
            .lines()
            .find(|l| !l.starts_with("zeo-timings:") && !l.trim().is_empty())
            .unwrap_or("");
        return Err(format!("zeo -o failed: {msg}"));
    }
    let _ = std::fs::remove_file(&bin_path);
    let field = |key: &str| -> Option<u64> {
        stderr
            .lines()
            .rev()
            .filter(|l| l.starts_with("zeo-timings:"))
            .find_map(|l| {
                l.split_whitespace()
                    .find_map(|tok| tok.strip_prefix(key))
                    .and_then(|v| v.trim_end_matches("ms").parse().ok())
            })
    };
    let rust_bytes = field("bytes=").ok_or("no bytes= in zeo-timings output")?;
    let rustc_ms = field("rustc=").ok_or("no rustc= in zeo-timings output")?;
    let bin_bytes = field("bin_bytes=").ok_or("no bin_bytes= in zeo-timings output")?;
    let warnings = stderr.matches("warning:").count() as u64;

    Ok(Row {
        name: name.to_string(),
        frontend_ms: best_ms.unwrap(),
        rust_lines,
        rust_bytes,
        rustc_ms,
        bin_bytes,
        warnings,
    })
}

fn progress(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

fn read_baseline(path: &Path) -> Vec<Row> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some(Row {
                name: f.next()?.to_string(),
                frontend_ms: f.next()?.parse().ok()?,
                rust_lines: f.next()?.parse().ok()?,
                rust_bytes: f.next()?.parse().ok()?,
                rustc_ms: f.next()?.parse().ok()?,
                bin_bytes: f.next()?.parse().ok()?,
                warnings: f.next()?.parse().ok()?,
            })
        })
        .collect()
}

fn write_baseline(path: &Path, rows: &[Row]) {
    let mut out = String::from(
        "# bench/compile-baseline.tsv -- xtask compile-bench --update-baseline\n\
         # name\tfrontend_ms\trust_lines\trust_bytes\trustc_ms\tbin_bytes\twarnings\n",
    );
    for r in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            r.name, r.frontend_ms, r.rust_lines, r.rust_bytes, r.rustc_ms, r.bin_bytes, r.warnings
        ));
    }
    std::fs::write(path, out).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}
