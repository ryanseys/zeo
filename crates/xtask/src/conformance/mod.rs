//! `cargo run -p xtask -- conformance <run|triage|show|oracle-verify>`:
//! the conformance harness that compiles and runs external Ruby test corpora
//! (first: the C spinel project's golden corpus) against spinel-rs, diffing
//! output against committed snapshots or the live `ruby` oracle, and ranking
//! failures into gap buckets that drive the implementation roadmap.

pub(crate) mod exec;
mod oracle;
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

use suite::{Suite, TestCase, TestResult, Verdict};

struct Opts {
    command: String,
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
        dir: None,
        filters: Vec::new(),
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
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
                if s != "spinel" {
                    return Err(format!("suite {s:?} isn't implemented yet (only `spinel`)"));
                }
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
  run           [--dir PATH] [--filter GLOB]... [-j N] [--timeout SECS]\n\
                [--compile-timeout SECS] [--force] [--fail-fast]\n\
                [--show-diffs N] [--update-scoreboard] [--force-skiplist]\n\
  triage        [--top N] [--bucket NAME]   (--bucket lists each test + its stderr tail)\n\
  show <id>\n\
  oracle-verify [--dir PATH]\n\
  --help, -h    show this message\n\
corpus root: --dir, else $SPINEL_TEST_DIR";

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
    suite: spinel_suite::SpinelSuite,
    cases: Vec<TestCase>,
    stamps: stamps::StampStore,
    oracle: oracle::Oracle,
    runner: runner::Runner,
    skiplist: Vec<skiplist::SkipEntry>,
}

fn open_session(root: &Path, opts: &Opts, prebuild: bool) -> Result<Session, String> {
    let suite = spinel_suite::SpinelSuite;
    let corpus_dir = match &opts.dir {
        Some(d) => d.clone(),
        None => std::env::var_os(suite.root_env_var())
            .map(PathBuf::from)
            .ok_or_else(|| {
                format!(
                    "no corpus root: pass --dir or set ${}",
                    suite.root_env_var()
                )
            })?,
    };
    let cases = suite.discover(&corpus_dir)?;

    if prebuild {
        runner::prebuild(root)?;
    }
    let work_dir = root.join("target/conformance").join(suite.name());
    let oracle = oracle::Oracle::new(
        root.join("target/conformance/oracle"),
        opts.run_timeout,
    )?;
    let stamps = stamps::StampStore::new(work_dir.join("stamps"), root, &oracle.ruby_version)?;
    let runner = runner::Runner {
        spinelc: root.join("target/debug/spinelc"),
        bin_dir: work_dir.join("bin"),
        diff_dir: work_dir.join("diffs"),
        compile_timeout: opts.compile_timeout,
        run_timeout: opts.run_timeout,
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

fn cmd_run(root: &Path, opts: &Opts) -> Result<ExitCode, String> {
    let Session {
        suite,
        cases,
        stamps,
        oracle,
        runner,
        skiplist,
    } = open_session(root, opts, true)?;
    let suite_name = suite.name();
    let all_ids: Vec<String> = cases.iter().map(|c| c.id.clone()).collect();

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

    let ranked = scoreboard::ranked_buckets(&results);
    if !ranked.is_empty() {
        println!("\ntop {} failure categories:", ranked.len().min(10));
        println!("{:>8}  {:<28} {}", "blocked", "bucket", "sample");
        for b in ranked.iter().take(10) {
            println!("{:>8}  {:<28} {}", b.count, b.bucket, b.sample_id);
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
        scoreboard::write_all(&root.join("conformance"), &meta, &all_results)?;
        println!("\nwrote conformance/scoreboard.tsv, SCOREBOARD.md, TRIAGE.md");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_triage(root: &Path, opts: &Opts) -> Result<ExitCode, String> {
    let session = open_session(root, opts, false)?;
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
    let session = open_session(root, opts, false)?;
    let Some(r) = session.stamps.load_any(id) else {
        return Err(format!("no stamp for {id:?} -- run `conformance run` first"));
    };
    let case = session
        .cases
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("{id:?} isn't in the corpus"))?;
    println!("test:     {id}");
    println!("source:   {}", case.source.display());
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
    let session = open_session(root, opts, false)?;
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
    println!("verifying {} live-oracle tests (2 runs each)...", live.len());
    let mut nondet = 0;
    for case in live {
        let a = session.oracle.run(case, true)?;
        let b = session.oracle.run(case, true)?;
        if a.stdout != b.stdout || a.stderr != b.stderr || a.ok != b.ok {
            nondet += 1;
            println!("NONDETERMINISTIC {}", case.id);
            println!(
                "  skiplist line:\nspinel\t{}\tnondeterministic oracle output (oracle-verify)",
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
