//! `cargo run -p xtask -- conformance <run|triage|show|oracle-verify>`:
//! the conformance harness that compiles and runs external Ruby test corpora
//! (first: the C spinel project's golden corpus) against zeo, diffing
//! output against committed snapshots or the live `ruby` oracle, and ranking
//! failures into gap buckets that drive the implementation roadmap.

pub(crate) mod exec;
mod oracle;
mod rubyspec_suite;
mod runlock;
mod runner;
mod scoreboard;
mod skiplist;
mod spinel_suite;
mod stamps;
mod suite;
mod triage;
mod util;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use suite::{TestCase, TestResult, Verdict};

struct Opts {
    command: String,
    /// `--suite <name>` pins the run to one suite; `None` means "every suite"
    /// for `run` and defaults to `spinel` for the single-suite subcommands
    /// (triage/show/oracle-verify).
    suite: Option<String>,
    dir: Option<PathBuf>,
    filters: Vec<String>,
    jobs: usize,
    run_timeout: Duration,
    compile_timeout: Duration,
    force: bool,
    fail_fast: bool,
    update_scoreboard: bool,
    force_skiplist: bool,
    show_diffs: usize,
    top: usize,
    bucket: Option<String>,
    test_id: Option<String>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut opts = Opts {
        command: args.first().cloned().ok_or(USAGE)?,
        suite: None,
        dir: None,
        filters: Vec::new(),
        // Leave headroom below the core count: each worker runs a full `zeo`
        // subprocess whose `rustc` + link step already spawn several
        // threads, so `ncpu` workers on `ncpu` cores oversubscribe and inflate every
        // per-compile wall-clock (a single clean compile is ~1.5s but climbs to 4-6s
        // under `ncpu` workers). `ncpu - 2` keeps the box busy without the thrash and
        // is overridable via the `--jobs` flag.
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(1)),
        run_timeout: Duration::from_secs(10),
        compile_timeout: Duration::from_secs(60),
        force: false,
        fail_fast: false,
        update_scoreboard: false,
        force_skiplist: false,
        show_diffs: 0,
        top: 30,
        bucket: None,
        test_id: None,
    };
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.as_str() {
            "--dir" => opts.dir = Some(PathBuf::from(value("--dir")?)),
            "--filter" => opts.filters.push(value("--filter")?),
            "-j" => opts.jobs = value("-j")?.parse().map_err(|e| format!("-j: {e}"))?,
            "--timeout" => {
                opts.run_timeout =
                    Duration::from_secs(value("--timeout")?.parse().map_err(|e| format!("{e}"))?)
            }
            "--compile-timeout" => {
                opts.compile_timeout = Duration::from_secs(
                    value("--compile-timeout")?
                        .parse()
                        .map_err(|e| format!("{e}"))?,
                )
            }
            "--force" => opts.force = true,
            "--fail-fast" => opts.fail_fast = true,
            "--update-scoreboard" => opts.update_scoreboard = true,
            "--force-skiplist" => opts.force_skiplist = true,
            "--show-diffs" => {
                opts.show_diffs = value("--show-diffs")?.parse().map_err(|e| format!("{e}"))?
            }
            "--top" => opts.top = value("--top")?.parse().map_err(|e| format!("{e}"))?,
            "--bucket" => opts.bucket = Some(value("--bucket")?),
            "--suite" => {
                let s = value("--suite")?;
                if s != "spinel" && s != "rubyspec" {
                    return Err(format!(
                        "unknown suite {s:?} (expected `spinel` or `rubyspec`)"
                    ));
                }
                opts.suite = Some(s);
            }
            other if !other.starts_with('-') && opts.test_id.is_none() => {
                opts.test_id = Some(other.to_owned())
            }
            other => return Err(format!("unknown flag {other}\n{USAGE}")),
        }
    }
    Ok(opts)
}

const USAGE: &str = "usage: cargo run -p xtask -- conformance <command>\n\
  run           [--suite spinel|rubyspec] [--dir PATH] [--filter GLOB]... [-j N]\n\
                [--timeout SECS] [--compile-timeout SECS] [--force] [--fail-fast]\n\
                [--show-diffs N] [--update-scoreboard] [--force-skiplist]\n\
  triage        [--suite NAME] [--top N] [--bucket NAME]\n\
  show <id>     [--suite NAME]\n\
  oracle-verify [--suite NAME] [--dir PATH]\n\
  clean-cache   remove the compiled-program cache (target/zeo-bin-cache)\n\
  --help, -h    show this message\n\
run with no --suite exercises every suite (spinel, then rubyspec); each suite's\n\
corpus comes from --dir, else its $ENV (SPINEL_TEST_DIR / RUBYSPEC_DIR) -- a\n\
suite whose corpus is absent is skipped.";

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let result = match opts.command.as_str() {
        "run" => cmd_run(root, &opts),
        "triage" => cmd_triage(root, &opts),
        "show" => cmd_show(root, &opts),
        "oracle-verify" => cmd_oracle_verify(root, &opts),
        "clean-cache" => cmd_clean_cache(root, &opts),
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("conformance: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Session {
    suite: Box<dyn suite::Suite>,
    cases: Vec<TestCase>,
    stamps: stamps::StampStore,
    oracle: oracle::Oracle,
    runner: runner::Runner,
    skiplist: Vec<skiplist::SkipEntry>,
}

fn make_suite(name: &str, root: &Path) -> Box<dyn suite::Suite> {
    match name {
        "rubyspec" => Box::new(rubyspec_suite::RubySpecSuite {
            repo_root: root.to_path_buf(),
        }),
        _ => Box::new(spinel_suite::SpinelSuite),
    }
}

/// The suites a `run` targets: the one named by `--suite`, else every suite.
/// Order matters -- `spinel` (the core corpus) runs first.
fn selected_suites(opts: &Opts) -> Vec<&'static str> {
    match opts.suite.as_deref() {
        Some("rubyspec") => vec!["rubyspec"],
        Some(_) => vec!["spinel"],
        None => vec!["spinel", "rubyspec"],
    }
}

/// Locate a suite's corpus: `--dir` wins, then `$ENV`. In a multi-suite run a
/// suite whose corpus can't be found is skipped (`Ok(None)`); a single
/// explicitly-selected suite errors instead.
fn resolve_corpus(
    opts: &Opts,
    suite: &dyn suite::Suite,
    allow_skip: bool,
) -> Result<Option<PathBuf>, String> {
    if let Some(d) = &opts.dir {
        return Ok(Some(d.clone()));
    }
    if let Some(v) = std::env::var_os(suite.root_env_var()) {
        if !v.is_empty() {
            return Ok(Some(PathBuf::from(v)));
        }
    }
    if allow_skip {
        Ok(None)
    } else {
        Err(format!(
            "no corpus root for suite `{}`: pass --dir or set ${}",
            suite.name(),
            suite.root_env_var()
        ))
    }
}

fn open_session(
    root: &Path,
    opts: &Opts,
    suite_name: &str,
    corpus_dir: &Path,
    prebuild: bool,
) -> Result<Session, String> {
    let suite = make_suite(suite_name, root);
    // A driver-synthesizing suite (rubyspec) writes into the work dir, so it
    // must exist before `discover`.
    let work_dir = root.join("target/conformance").join(suite.name());
    let cases = suite.discover(corpus_dir, &work_dir)?;

    if prebuild {
        // Conformance links every case against the RELEASE runtime by default:
        // the optimized runtime makes each per-program link ~12x faster, which
        // dominates a multi-thousand-case run. `ZEO_RUNTIME_PROFILE=debug` in
        // the environment opts back to the debug runtime for symbolicating a
        // panic.
        runner::prebuild(root)?;
    }
    let oracle = oracle::Oracle::new(
        root.join("target/conformance/oracle"),
        opts.run_timeout,
        root,
    )?;
    let stamps = stamps::StampStore::new(work_dir.join("stamps"), root, &oracle.ruby_version)?;
    let runner = runner::Runner {
        zeo: root.join("target/debug/zeo"),
        bin_dir: work_dir.join("bin"),
        diff_dir: work_dir.join("diffs"),
        compile_timeout: opts.compile_timeout,
        run_timeout: opts.run_timeout,
        // The rubyspec driver's `require_relative '../spec_helper'` (real mspec)
        // must collapse to a no-op -- the mspec_lite shim supplies the DSL.
        mspec_stubs: suite_name == "rubyspec",
    };
    let skiplist = skiplist::load(&root.join("conformance/skiplist.tsv"))?;
    Ok(Session {
        suite,
        cases,
        stamps,
        oracle,
        runner,
        skiplist,
    })
}

/// Open the single suite the non-`run` subcommands operate on: the one named by
/// `--suite`, else `spinel`.
fn open_single(root: &Path, opts: &Opts, prebuild: bool) -> Result<Session, String> {
    let name = match opts.suite.as_deref() {
        Some("rubyspec") => "rubyspec",
        _ => "spinel",
    };
    let suite = make_suite(name, root);
    let corpus = resolve_corpus(opts, suite.as_ref(), false)?
        .expect("resolve_corpus with allow_skip=false is Some or Err");
    open_session(root, opts, name, &corpus, prebuild)
}

/// The workspace target directory (honoring `CARGO_TARGET_DIR`, else
/// `<root>/target`) -- matches how `zeo`'s `build` module locates it, so the
/// run lock and the cache both land where `build_binary` expects.
fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
}

/// Remove the ENTIRE compiled-program cache (`target/zeo-bin-cache`),
/// reclaiming disk. Fully regenerable, so this is always safe between runs.
/// Stale generations are also swept automatically at prebuild time
/// (`zeo::build::sweep_stale_cache_generations`); sweeping never happens in the
/// build hot path itself, where a `remove_dir_all` could delete a generation a
/// sibling process was still writing into.
fn cmd_clean_cache(root: &Path, _opts: &Opts) -> Result<ExitCode, String> {
    // Mirrors `zeo::build`'s `cache_dir` -- kept in sync by name.
    let cache = target_dir(root).join("zeo-bin-cache");
    let Ok(entries) = std::fs::read_dir(&cache) else {
        println!("nothing to clean: {} does not exist", cache.display());
        return Ok(ExitCode::SUCCESS);
    };
    let mut generations = 0u64;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = path.is_dir();
        bytes += dir_size(&path);
        // A generation is a directory; tolerate stray files too (e.g. a macOS
        // `.DS_Store`), which `remove_dir_all` would reject.
        let removed = if is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.map_err(|e| format!("removing {}: {e}", path.display()))?;
        if is_dir {
            generations += 1;
        }
    }
    println!(
        "cleaned {generations} cache generation(s), reclaimed {}",
        human_bytes(bytes)
    );
    Ok(ExitCode::SUCCESS)
}

/// Total size in bytes of a file or directory tree (best-effort; unreadable
/// entries count as 0).
fn dir_size(path: &Path) -> u64 {
    match std::fs::read_dir(path) {
        Ok(entries) => entries.flatten().map(|e| dir_size(&e.path())).sum(),
        Err(_) => std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    }
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn cmd_run(root: &Path, opts: &Opts) -> Result<ExitCode, String> {
    // Hold an advisory run lock for the whole run so a second `conformance run`
    // can't silently contend on the shared target/binary-cache or race the
    // scoreboard. Released when `_lock` drops at the end of this function.
    let _lock = runlock::RunLock::acquire(&target_dir(root))?;

    let suites = selected_suites(opts);
    if opts.dir.is_some() && suites.len() > 1 {
        return Err(
            "--dir names a single corpus; pass --suite to pick which suite it applies to"
                .to_owned(),
        );
    }

    // Prebuild (zeo + zeo-rt) once, before the first suite that actually
    // runs; later suites reuse the built artifacts.
    let mut prebuilt = false;
    for (i, &name) in suites.iter().enumerate() {
        let suite = make_suite(name, root);
        let corpus = match resolve_corpus(opts, suite.as_ref(), suites.len() > 1)? {
            Some(c) => c,
            None => {
                println!(
                    "\n=== suite `{name}` skipped: no corpus (pass --dir or set ${}) ===",
                    suite.root_env_var()
                );
                continue;
            }
        };
        if suites.len() > 1 {
            println!(
                "\n{}=== suite `{name}` (corpus {}) ===",
                if i == 0 { "" } else { "\n" },
                corpus.display()
            );
        }
        run_one_suite(root, opts, name, &corpus, !prebuilt)?;
        prebuilt = true;
    }
    Ok(ExitCode::SUCCESS)
}

fn run_one_suite(
    root: &Path,
    opts: &Opts,
    suite_name: &str,
    corpus: &Path,
    prebuild: bool,
) -> Result<ExitCode, String> {
    let Session {
        suite,
        cases,
        stamps,
        oracle,
        runner,
        skiplist,
    } = open_session(root, opts, suite_name, corpus, prebuild)?;
    let suite_name = suite.name();
    let all_ids: Vec<String> = cases.iter().map(|c| c.id.clone()).collect();
    // Capture each case's source + reference paths now, before the cases are
    // consumed by the run, so the failures document can name them for every
    // test (including ones later filled from stamps in a filtered run).
    let case_meta = build_case_meta(&cases);

    // Stale skiplist entries rot loudly.
    for entry in &skiplist {
        if entry.suite == suite_name
            && !cases
                .iter()
                .any(|c| skiplist::glob_match(&entry.pattern, &c.id))
            && !opts.force_skiplist
        {
            return Err(format!(
                "skiplist pattern {:?} matches no test (use --force-skiplist to override)",
                entry.pattern
            ));
        }
    }

    let mut skipped = Vec::new();
    let mut to_run = Vec::new();
    for case in cases {
        if !opts.filters.is_empty()
            && !opts
                .filters
                .iter()
                .any(|f| skiplist::glob_match(f, &case.id))
        {
            continue;
        }
        if let Some(reason) = skiplist::skip_reason(&skiplist, suite_name, &case.id) {
            skipped.push(TestResult {
                id: case.id,
                verdict: Verdict::Skip,
                stage: "-",
                bucket: "-".to_owned(),
                cluster: "-".to_owned(),
                stderr_tail: reason.to_owned(),
                compile_ms: 0,
                run_ms: 0,
                cached: false,
            });
            continue;
        }
        let hash = stamps.inputs_hash(&case)?;
        to_run.push((case, hash));
    }

    println!(
        "running {} tests ({} skipped) with {} workers, oracle: {}",
        to_run.len(),
        skipped.len(),
        opts.jobs,
        oracle.ruby_version
    );
    let mut results = runner.run_all(
        to_run,
        &stamps,
        &oracle,
        opts.jobs,
        opts.force,
        opts.fail_fast,
    );
    results.extend(skipped);
    results.sort_by(|a, b| a.id.cmp(&b.id));

    // Summary: every verdict category (zero-filled) + total, then the top
    // failure buckets so a run ends with an actionable ranking.
    println!();
    for (v, n) in scoreboard::verdict_counts(&results) {
        println!("{v:>16} {n}");
    }
    println!("{:>16} {}", "TOTAL", results.len());
    // Pass RATE, so a run ends with the one number that should climb over time.
    let passed = results
        .iter()
        .filter(|r| r.verdict == Verdict::Pass)
        .count();
    let total = results.len();
    let pct = if total > 0 {
        passed as f64 / total as f64 * 100.0
    } else {
        0.0
    };
    println!("{:>16} {passed}/{total} ({pct:.1}%)", "PASS RATE");

    let ranked = scoreboard::ranked_buckets(&results);
    if !ranked.is_empty() {
        println!("\ntop {} failure categories:", ranked.len().min(10));
        println!("{:>8}  {:<28} sample", "blocked", "bucket");
        for b in ranked.iter().take(10) {
            println!("{:>8}  {:<28} {}", b.count, b.bucket, b.sample_id);
        }
    }

    // The slowest tests by wall time -- surfaces a single pathological compile
    // or run that stalls a worker while the rest fly by (the "bursty" feel).
    // `compile` is `zeo` + `rustc`; `run` is the compiled program. A high
    // `compile` points at a program zeo generates a lot of Rust for; a high
    // `run` at a genuinely slow (or nearly-hanging) program.
    let mut by_time: Vec<&TestResult> = results.iter().collect();
    by_time.sort_by_key(|r| std::cmp::Reverse(r.compile_ms + r.run_ms));
    let slowest = by_time.len().min(15);
    if slowest > 0 && by_time[0].compile_ms + by_time[0].run_ms > 0 {
        println!("\ntop {slowest} slowest tests (ms):");
        println!("{:>10} {:>10} {:>10}  test", "total", "compile", "run");
        for r in by_time.iter().take(slowest) {
            println!(
                "{:>10} {:>10} {:>10}  {}",
                r.compile_ms + r.run_ms,
                r.compile_ms,
                r.run_ms,
                r.id
            );
        }
    }

    if opts.show_diffs > 0 {
        for r in results
            .iter()
            .filter(|r| r.verdict == Verdict::FailOutput)
            .take(opts.show_diffs)
        {
            if let Ok(diff) = std::fs::read_to_string(runner.diff_path(&r.id)) {
                println!("\n### {}\n{diff}", r.id);
            }
        }
    }

    if opts.update_scoreboard {
        // Scoreboard covers the whole corpus; a filtered run fills the
        // unfiltered tests' rows from their existing stamps.
        let mut by_id: std::collections::BTreeMap<String, TestResult> =
            results.into_iter().map(|r| (r.id.clone(), r)).collect();
        for id in &all_ids {
            if !by_id.contains_key(id) {
                if let Some(r) = stamps.load_any(id) {
                    by_id.insert(id.clone(), r);
                }
            }
        }
        let all_results: Vec<TestResult> = by_id.into_values().collect();
        let meta = scoreboard::RunMeta {
            suite: suite_name,
            corpus: all_ids.len(),
            ruby_version: &oracle.ruby_version,
            git_sha: &git_sha(root),
        };
        scoreboard::write_all(
            &root.join("conformance"),
            &meta,
            &all_results,
            &case_meta,
            &runner.diff_dir,
        )?;
        let prefix = if suite_name == "spinel" {
            ""
        } else {
            suite_name
        };
        let sep = if prefix.is_empty() { "" } else { "-" };
        println!(
            "\nwrote conformance/{p}{s}scoreboard.tsv, {p}{s}SCOREBOARD.md, {p}{s}TRIAGE.md, {p}{s}FAILURES.md",
            p = prefix,
            s = sep
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Index every discovered case by id, recording where its reference output
/// comes from — the concrete `.expected` snapshot paths, or the mode
/// (live-oracle / compile-fail / self-report) when there's no snapshot file —
/// so the failures document can cite exact paths for each test.
fn build_case_meta(cases: &[TestCase]) -> std::collections::BTreeMap<String, scoreboard::CaseMeta> {
    cases
        .iter()
        .map(|c| {
            let (expected_stdout, expected_stderr, reference) = match &c.expectation {
                suite::Expectation::Snapshot {
                    stdout: Some(o),
                    stderr,
                } => (Some(o.clone()), stderr.clone(), "snapshot"),
                suite::Expectation::Snapshot { stdout: None, .. } => (None, None, "live-oracle"),
                suite::Expectation::CompileFail => (None, None, "compile-fail"),
                suite::Expectation::SelfReport => (None, None, "self-report"),
            };
            (
                c.id.clone(),
                scoreboard::CaseMeta {
                    source: c.source.clone(),
                    expected_stdout,
                    expected_stderr,
                    reference,
                },
            )
        })
        .collect()
}

fn cmd_triage(root: &Path, opts: &Opts) -> Result<ExitCode, String> {
    let session = open_single(root, opts, false)?;
    let mut results = Vec::new();
    for case in &session.cases {
        if let Some(r) = session.stamps.load_any(&case.id) {
            results.push(r);
        }
    }
    if results.is_empty() {
        return Err("no stamps found -- run `conformance run` first".to_owned());
    }

    if let Some(bucket) = &opts.bucket {
        let mut rows: Vec<_> = results.iter().filter(|r| &r.bucket == bucket).collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        println!("{} test(s) in bucket {bucket:?}:", rows.len());
        for r in rows {
            let tail = r.stderr_tail.lines().last().unwrap_or("");
            println!("  {:<40} {:<12} {}", r.id, r.verdict.as_str(), tail);
        }
        return Ok(ExitCode::SUCCESS);
    }

    let buckets = scoreboard::bucket_stats(&results);
    let mut ranked: Vec<_> = buckets.values().collect();
    ranked.sort_by(|a, b| b.count.cmp(&a.count).then(a.bucket.cmp(&b.bucket)));
    println!("{:<8} {:<28} {:>8}  sample", "cluster", "bucket", "blocked");
    for b in ranked.iter().take(opts.top) {
        println!(
            "{:<8} {:<28} {:>8}  {}",
            b.cluster, b.bucket, b.count, b.sample_id
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_show(root: &Path, opts: &Opts) -> Result<ExitCode, String> {
    let id = opts.test_id.as_deref().ok_or("show requires a test id")?;
    let session = open_single(root, opts, false)?;
    let Some(r) = session.stamps.load_any(id) else {
        return Err(format!(
            "no stamp for {id:?} -- run `conformance run` first"
        ));
    };
    let case = session
        .cases
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("{id:?} isn't in the corpus"))?;
    println!("test:     {id}");
    println!("source:   {}", case.source.display());
    match &case.expectation {
        suite::Expectation::Snapshot {
            stdout: Some(o),
            stderr,
        } => {
            println!("expected: {}", o.display());
            match stderr {
                Some(e) => println!("exp-err:  {}", e.display()),
                None => println!("exp-err:  (must be empty)"),
            }
        }
        suite::Expectation::Snapshot { stdout: None, .. } => println!("expected: (live oracle)"),
        suite::Expectation::CompileFail => println!("expected: (zeo must reject)"),
        suite::Expectation::SelfReport => println!("expected: (self-reported)"),
    }
    println!("verdict:  {} (stage: {})", r.verdict.as_str(), r.stage);
    println!("bucket:   {} (cluster {})", r.bucket, r.cluster);
    println!("timing:   compile {}ms, run {}ms", r.compile_ms, r.run_ms);
    if !r.stderr_tail.is_empty() {
        println!("stderr:\n{}", indent(&r.stderr_tail));
    }
    if let Ok(diff) = std::fs::read_to_string(session.runner.diff_path(id)) {
        println!("diff:\n{}", indent(&diff));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_oracle_verify(root: &Path, opts: &Opts) -> Result<ExitCode, String> {
    let session = open_single(root, opts, false)?;
    let live: Vec<_> = session
        .cases
        .iter()
        .filter(|c| {
            matches!(
                &c.expectation,
                suite::Expectation::Snapshot { stdout: None, .. }
            )
        })
        .collect();
    println!(
        "verifying {} live-oracle tests (2 runs each)...",
        live.len()
    );
    let mut nondet = 0;
    for case in live {
        let a = session.oracle.run(case, true)?;
        let b = session.oracle.run(case, true)?;
        if a.stdout != b.stdout || a.stderr != b.stderr || a.ok != b.ok {
            nondet += 1;
            println!("NONDETERMINISTIC {}", case.id);
            println!(
                "  skiplist line:\nzeo\t{}\tnondeterministic oracle output (oracle-verify)",
                case.id
            );
        }
    }
    println!("{nondet} nondeterministic test(s)");
    Ok(ExitCode::SUCCESS)
}

fn git_sha(root: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
