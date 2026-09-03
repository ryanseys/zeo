//! The COMPILER-cost instrument: what a compile costs and produces per
//! program (frontend wall time, emitted CLIF lines, peak RSS, binary size)
//! across a fixed hello-world-to-bundler ladder.
//!
//! The RUNTIME performance suite lives in criterion:
//!
//! ```text
//! cargo bench -p zeo --bench programs
//! ```
//!
//! (see bench/README.md for baselines, comparisons, and profiling).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::exec::{self, Capture};
use crate::scratch::Scratch;
use crate::{Error, root, root_join, write_if_changed};

/// The fixed program set: names paired with repo-relative paths, spanning
/// hello-world to bundler scale. `hello` is synthesized so the smallest
/// possible program is always in the record.
const PROGRAMS: &[(&str, Option<&str>)] = &[
    ("hello", None),
    ("bm_fib", Some("test/bench/bm_fib.rb")),
    ("bm_template", Some("test/bench/bm_template.rb")),
    ("bm_json_parse", Some("test/bench/bm_json_parse.rb")),
    ("bm_micro_lisp", Some("test/bench/bm_micro_lisp.rb")),
    ("core_classes", Some("test/lang/classes/core_classes.rb")),
    ("uri_parse_and_build", Some("test/stdlib/uri/uri_parse_and_build.rb")),
    ("optparse_subset", Some("test/stdlib/optparse/optparse_subset.rb")),
    ("gem_rubygems", Some("test/bench/compile/rubygems.rb")),
    ("gem_bundler", Some("test/bench/compile/bundler.rb")),
];

const BASELINE: &str = "test/bench/compile-baseline.tsv";

const USAGE: &str =
    "usage: cargo xtask bench --compile [--filter <substr>] [--runs N] [--update-baseline]";

struct Row {
    frontend_ms: u128,
    lines: u64,
    peak_rss: u64,
    bin_bytes: u64,
}

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut filter = None;
    let mut runs: usize = 3;
    let mut update = false;
    let mut compile = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--filter" => {
                filter = Some(
                    rest.next()
                        .cloned()
                        .ok_or_else(|| Error::new(format!("--filter wants a value\n{USAGE}")))?,
                );
            }
            "--runs" => {
                let value = rest
                    .next()
                    .ok_or_else(|| Error::new(format!("--runs wants a value\n{USAGE}")))?;
                runs = value
                    .parse()
                    .map_err(|_| Error::new("--runs takes a positive integer"))?;
            }
            "--update-baseline" => update = true,
            "--compile" => compile = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(Error::new(format!("unknown option {other:?}\n{USAGE}"))),
        }
    }
    if runs == 0 {
        return Err(Error::new("--runs takes a positive integer"));
    }
    if !compile {
        return Err(Error::new(
            "the runtime suite moved to criterion -- run `cargo bench -p zeo --bench programs` \
             (this command keeps only --compile)",
        ));
    }

    let zeo = crate::build_zeo()?;
    let work = Scratch::new("bench")?;
    let mut rows: Vec<(&str, Row)> = Vec::new();
    for (name, rel) in PROGRAMS {
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        let rb = match rel {
            Some(rel) => root_join(rel),
            None => write_hello(work.path())?,
        };
        let row = match compile_one(&zeo, name, &rb, runs, work.path()) {
            Ok(row) => row,
            Err(e) => {
                progress(&format!("{name:<22} FAIL  {e}"));
                return Err(Error::reported());
            }
        };
        progress(&format!(
            "{name:<22} {:>6}ms  {:>6} lines  {:>5} MiB  {:>8} bytes",
            row.frontend_ms,
            row.lines,
            row.peak_rss / (1024 * 1024),
            row.bin_bytes
        ));
        rows.push((name, row));
    }
    if rows.is_empty() {
        println!("no programs matched");
        return Err(Error::reported());
    }
    if update {
        write_baseline(&rows)?;
        println!("baseline updated: {}", root_join(BASELINE).display());
    }
    Ok(())
}

/// A progress line the caller sees IMMEDIATELY, even piped or backgrounded:
/// stdout through a pipe is block-buffered.
fn progress(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

fn write_hello(work: &Path) -> Result<PathBuf, Error> {
    let path = work.join("hello.rb");
    std::fs::write(&path, "puts \"hello\"\n")
        .map_err(|e| Error::new(format!("writing {}: {e}", path.display())))?;
    Ok(path)
}

/// `frontend_ms` is best-of-N `--emit-clif` wall time, the renderer the BUILD
/// path uses. `peak_rss` takes the WORST run rather than the best: time is
/// best-of-N because the fastest run is the least disturbed, but memory is a
/// ceiling question, and the most a compile ever held is the number that
/// decides whether it fits.
fn compile_one(zeo: &Path, name: &str, rb: &Path, runs: usize, work: &Path) -> Result<Row, Error> {
    let emitted = work.join(format!("{name}.clif"));
    let mut best_ms = None;
    let mut lines = 0;
    let mut peak = 0;
    for _ in 0..runs {
        let started = Instant::now();
        // Options BEFORE the file. zeo follows ruby's own convention, where
        // everything after the script name is the PROGRAM's ARGV -- so
        // `zeo prog.rb --emit-clif=x` runs the program and hands it the flag,
        // which is how this instrument came to time a full run and report it
        // as a compile.
        let emit = format!("--emit-clif={}", emitted.display());
        let out = exec::run(
            &[zeo, Path::new(&emit), Path::new("-W0"), rb],
            root(),
            &[("ZEO_TIMINGS", Some("1"))],
            Capture::Both,
        )?;
        let ms = started.elapsed().as_millis();
        if !out.success() {
            return Err(Error::new(format!(
                "zeo --emit-clif failed: {}",
                out.stderr_text().lines().next().unwrap_or_default().trim()
            )));
        }
        lines = timing_field(&out.stderr_text(), "lines=").unwrap_or(0);
        peak = peak.max(timing_field(&out.stderr_text(), "peak_rss=").unwrap_or(0));
        best_ms = Some(best_ms.map_or(ms, |b: u128| b.min(ms)));
    }

    // One `-o` for the binary size. Nothing caches a built binary, so this
    // measures the real cost.
    let bin = work.join(format!("bin-{name}"));
    let out = exec::run(
        &[zeo, Path::new("-o"), &bin, Path::new("-W0"), rb],
        root(),
        &[],
        Capture::Both,
    )?;
    if !out.success() {
        let text = out.stderr_text();
        let first = text
            .lines()
            .find(|l| !l.starts_with("zeo-timings:"))
            .unwrap_or_default();
        return Err(Error::new(format!("zeo -o failed: {}", first.trim())));
    }
    let bin_bytes = std::fs::metadata(&bin)
        .map_err(|e| Error::new(format!("reading {}: {e}", bin.display())))?
        .len();
    Ok(Row {
        frontend_ms: best_ms.unwrap_or(0),
        lines,
        peak_rss: peak,
        bin_bytes,
    })
}

/// One `key=value` field off the LAST `zeo-timings:` line carrying it.
fn timing_field(stderr: &str, key: &str) -> Option<u64> {
    for line in stderr.lines().rev() {
        if !line.starts_with("zeo-timings:") {
            continue;
        }
        let Some(token) = line.split_whitespace().find(|t| t.starts_with(key)) else {
            continue;
        };
        return token
            .trim_start_matches(key)
            .trim_end_matches("ms")
            .parse()
            .ok();
    }
    None
}

fn write_baseline(rows: &[(&str, Row)]) -> Result<(), Error> {
    let mut sorted: Vec<&(&str, Row)> = rows.iter().collect();
    sorted.sort_by_key(|(name, _)| *name);
    let mut out = String::from(
        "# bench/compile-baseline.tsv -- cargo xtask bench --compile --update-baseline\n\
         # name\tfrontend_ms\tlines\tpeak_rss\tbin_bytes\n",
    );
    for (name, r) in sorted {
        out.push_str(&format!(
            "{name}\t{}\t{}\t{}\t{}\n",
            r.frontend_ms, r.lines, r.peak_rss, r.bin_bytes
        ));
    }
    write_if_changed(&root_join(BASELINE), out.as_bytes())
}
