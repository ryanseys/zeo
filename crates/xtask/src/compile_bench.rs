//! `cargo run -p xtask -- compile-bench [--filter <substr>] [--runs N] [--update-baseline]`
//!
//! The compile-side twin of `bench.rs`: instead of timing the RUNTIME of the
//! generated binaries, this measures what the COMPILER produces and costs on a
//! fixed program set spanning hello-world to bundler scale. Per program:
//!
//! - `frontend_ms` -- best-of-N wall time of `zeo <file> --emit-rust` (parse -> lower ->
//!   analyze -> codegen -> emit, no rustc)
//! - `rust_lines`  -- line count of the emitted (compact) output
//! - `rust_bytes`  -- byte length of the build-path source (the `bytes=` field
//!   of the compile's `zeo-timings:` line)
//! - `peak_rss`    -- the front-end run's own peak resident memory
//!   (`zeo::memguard`'s poller, via the `peak_rss=` field). The metric the
//!   compiler-memory work is measured by: one gem-scale compile reached
//!   9.8 GB and panicked a 16 GB machine's kernel, so how much the compiler
//!   HOLDS is a first-class number here, not a footnote to how long it takes.
//!   `0` for a compile that finished inside the poller's first interval.
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
    ("gem_rubygems", "tests/bench/rubygems.rb"),
    ("gem_bundler", "tests/bench/bundler.rb"),
];

#[derive(Clone)]
struct Row {
    name: String,
    frontend_ms: u64,
    rust_lines: u64,
    rust_bytes: u64,
    peak_rss: u64,
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
            && !name.contains(f.as_str())
        {
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
    let (fd, ld, pd, rd, bd) = match base {
        Some(b) => (
            delta(row.frontend_ms, b.frontend_ms),
            delta(row.rust_lines, b.rust_lines),
            delta(row.peak_rss, b.peak_rss),
            delta(row.rustc_ms, b.rustc_ms),
            delta(row.bin_bytes, b.bin_bytes),
        ),
        None => (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
    };
    progress(&format!(
        "{:<22} frontend {:>6}ms{fd}  lines {:>8}{ld}  peak {:>7}MiB{pd}  rustc {:>7}ms{rd}  bin {:>9}B{bd}  warnings {}",
        row.name,
        row.frontend_ms,
        row.rust_lines,
        row.peak_rss / (1024 * 1024),
        row.rustc_ms,
        row.bin_bytes,
        row.warnings
    ));
}

fn run_one(zeo_bin: &Path, name: &str, rb: &Path, runs: usize) -> Result<Row, String> {
    // Frontend: best-of-N `--emit-rust` wall time, which is the renderer the
    // BUILD path uses. It measured `--dump=rust` until the peak-memory work
    // showed what that was costing -- a `syn` re-parse, a prettyplease pass
    // and the whole program held as a `String`, none of which any build does.
    // `rust_lines` and `peak_rss` moved with it, so both re-baselined.
    //
    // `peak_rss` takes the WORST run rather than the best: time is a
    // best-of-N measurement because the fastest run is the one least
    // disturbed by the machine, but memory is a ceiling question, and the
    // most a compile ever held is the number that decides whether it fits.
    let emitted = std::env::temp_dir().join(format!("zeo-compile-bench-{name}.rs"));
    let mut best_ms: Option<u64> = None;
    let mut rust_lines = 0u64;
    let mut peak_rss = 0u64;
    for _ in 0..runs {
        let started = Instant::now();
        let out = Command::new(zeo_bin)
            .arg(rb)
            .arg(format!("--emit-rust={}", emitted.display()))
            .arg("-W0")
            .env("ZEO_TIMINGS", "1")
            .output()
            .map_err(|e| format!("invoking zeo --emit-rust: {e}"))?;
        let ms = started.elapsed().as_millis() as u64;
        if !out.status.success() {
            return Err(format!(
                "zeo --emit-rust failed: {}",
                String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .next()
                    .unwrap_or("")
            ));
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        rust_lines = timing_field(&stderr, "lines=").unwrap_or(0);
        peak_rss = peak_rss.max(timing_field(&stderr, "peak_rss=").unwrap_or(0));
        best_ms = Some(best_ms.map_or(ms, |b| b.min(ms)));
    }
    let _ = std::fs::remove_file(&emitted);

    // Build: one `-o` under cache bypass; `zeo-timings:` lines carry the
    // build-path source bytes, the rustc wall time, and the binary size.
    let bin_path = std::env::temp_dir().join(format!("zeo-compile-bench-{name}"));
    let out = Command::new(zeo_bin)
        .arg(rb)
        .arg("-o")
        .arg(&bin_path)
        .arg("-W0")
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
    let field = |key: &str| timing_field(&stderr, key);
    let rust_bytes = field("bytes=").ok_or("no bytes= in zeo-timings output")?;
    let rustc_ms = field("rustc=").ok_or("no rustc= in zeo-timings output")?;
    let bin_bytes = field("bin_bytes=").ok_or("no bin_bytes= in zeo-timings output")?;
    let warnings = stderr.matches("warning:").count() as u64;

    Ok(Row {
        name: name.to_string(),
        frontend_ms: best_ms.unwrap(),
        rust_lines,
        rust_bytes,
        peak_rss,
        rustc_ms,
        bin_bytes,
        warnings,
    })
}

/// One `key=value` field off the last `zeo-timings:` line that carries it.
/// `bytes=` and `rustc=ms` share a shape; the `ms` suffix is trimmed so both
/// parse as a number.
fn timing_field(stderr: &str, key: &str) -> Option<u64> {
    stderr
        .lines()
        .rev()
        .filter(|l| l.starts_with("zeo-timings:"))
        .find_map(|l| {
            l.split_whitespace()
                .find_map(|tok| tok.strip_prefix(key))
                .and_then(|v| v.trim_end_matches("ms").parse().ok())
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
                // Appended, never inserted: a baseline banked before this
                // column existed has to keep parsing as itself, and a new
                // field anywhere but the end would silently shift every
                // column after it onto the wrong name. Missing reads as 0,
                // which `report`'s delta already treats as "no comparison".
                peak_rss: f.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            })
        })
        .collect()
}

fn write_baseline(path: &Path, rows: &[Row]) {
    let mut out = String::from(
        "# bench/compile-baseline.tsv -- xtask compile-bench --update-baseline\n\
         # name\tfrontend_ms\trust_lines\trust_bytes\trustc_ms\tbin_bytes\twarnings\tpeak_rss\n",
    );
    for r in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            r.name,
            r.frontend_ms,
            r.rust_lines,
            r.rust_bytes,
            r.rustc_ms,
            r.bin_bytes,
            r.warnings,
            r.peak_rss
        ));
    }
    std::fs::write(path, out).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}
