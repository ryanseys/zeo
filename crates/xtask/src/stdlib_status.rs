//! `cargo run -p xtask -- stdlib-status [<lib-dir>]`: sweeps every `*.rb`
//! under a Ruby stdlib `lib` directory and records whether `zeo` can
//! COMPILE it -- Ruby -> Rust codegen only (via `zeo -S`; no `rustc`, no
//! execution), the fast first-cut triage of how much real stdlib the compiler
//! accepts today.
//!
//! This is the tracking half of the `#96` stdlib work: there is deliberately
//! no bespoke `--stdlib` flag -- stdlib is delivered as ordinary `-I <lib>`
//! load-path roots (see the `*_i_root*` e2e tests), and this harness drives
//! that exact mechanism against a whole `lib` tree. `<lib-dir>` defaults to the
//! installed oracle's `RbConfig::CONFIG["rubylibdir"]` (the 4.0.5 stdlib the
//! runtime is matched against), and may be overridden with a path argument to
//! point at any other checkout's `lib`.
//!
//! Two artifacts land under `conformance/`, both machine-independent (relative
//! paths, Ruby version, no absolute paths or timestamps) so they diff cleanly
//! when committed as a progress record:
//!   - `stdlib-status.tsv`  -- one `relpath<TAB>status<TAB>reason` row per file
//!   - `STDLIB_STATUS.md`   -- the summary counts + the top failure reasons

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::conformance::exec::run_with_timeout;

/// Codegen of a single file is fast and terminating; the timeout only guards
/// against a pathological compiler hang so one bad file can't stall the sweep.
const COMPILE_TIMEOUT: Duration = Duration::from_secs(30);

/// The classification of one stdlib file against `zeo -S`.
enum Status {
    /// Ruby -> Rust codegen succeeded (rustc/runtime NOT exercised).
    Pass,
    /// A clean `zeo` rejection or compiler panic; the string is the
    /// aggregated failure reason (see `reason_bucket`).
    Fail(String),
    /// The compiler didn't finish within `COMPILE_TIMEOUT`.
    Timeout,
    /// The harness itself couldn't run `zeo` on this file.
    HarnessError(String),
}

impl Status {
    fn tag(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail(_) => "fail",
            Status::Timeout => "timeout",
            Status::HarnessError(_) => "harness-error",
        }
    }

    /// The `reason` column: empty for a pass, the bucket/detail otherwise.
    fn reason(&self) -> &str {
        match self {
            Status::Pass | Status::Timeout => "",
            Status::Fail(r) | Status::HarnessError(r) => r,
        }
    }
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let lib_dir = match args.first() {
        Some(arg) => PathBuf::from(arg),
        None => match ruby_query(r#"print RbConfig::CONFIG["rubylibdir"]"#) {
            Ok(p) => PathBuf::from(p),
            Err(e) => {
                eprintln!("stdlib-status: locating the installed stdlib: {e}");
                eprintln!("  (pass a lib directory explicitly: `stdlib-status <lib-dir>`)");
                return ExitCode::FAILURE;
            }
        },
    };
    if !lib_dir.is_dir() {
        eprintln!("stdlib-status: not a directory: {}", lib_dir.display());
        return ExitCode::FAILURE;
    }

    // `zeo -S` is the classifier; build it once up front so the parallel
    // invocations below don't race each other into cargo.
    eprintln!("stdlib-status: building zeo...");
    let built = Command::new("cargo")
        .args(["build", "--quiet", "-p", "zeo"])
        .current_dir(root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !built {
        eprintln!("stdlib-status: `cargo build -p zeo` failed");
        return ExitCode::FAILURE;
    }
    let zeo = root.join("target").join("debug").join("zeo");

    let mut files = Vec::new();
    collect_rb(&lib_dir, &mut files);
    files.sort();
    if files.is_empty() {
        eprintln!("stdlib-status: no `.rb` files under {}", lib_dir.display());
        return ExitCode::FAILURE;
    }
    let total = files.len();
    eprintln!(
        "stdlib-status: classifying {total} files under {} (codegen only)",
        lib_dir.display()
    );

    let queue = Mutex::new(files.into_iter().collect::<VecDeque<_>>());
    let results: Mutex<Vec<(String, Status)>> = Mutex::new(Vec::with_capacity(total));
    let done = AtomicUsize::new(0);
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let Some(file) = queue.lock().unwrap().pop_front() else {
                        break;
                    };
                    let status = classify(&zeo, &lib_dir, &file);
                    let rel = file
                        .strip_prefix(&lib_dir)
                        .unwrap_or(&file)
                        .display()
                        .to_string();
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    eprintln!("[{n}/{total}] {:>13}  {rel}", status.tag());
                    results.lock().unwrap().push((rel, status));
                }
            });
        }
    });

    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.0.cmp(&b.0));

    let ruby_version = ruby_query("print RUBY_VERSION").unwrap_or_else(|_| "unknown".to_string());
    match write_artifacts(root, &ruby_version, &results) {
        Ok((tsv, md)) => {
            print_summary(&results);
            eprintln!(
                "stdlib-status: wrote {} and {}",
                tsv.display(),
                md.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("stdlib-status: writing artifacts: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Compiles one file with `zeo <file> -I <lib-dir> -S` (codegen only) and
/// maps the outcome to a [`Status`]. Exit 0 = codegen succeeded; a non-zero
/// exit (a clean rejection or a compiler panic) = a failure, bucketed by its
/// message.
fn classify(zeo: &Path, lib_dir: &Path, file: &Path) -> Status {
    let mut cmd = Command::new(zeo);
    cmd.arg(file).arg("-I").arg(lib_dir).arg("-S");
    match run_with_timeout(cmd, None, COMPILE_TIMEOUT) {
        Err(e) => Status::HarnessError(e),
        Ok(ex) if ex.timed_out => Status::Timeout,
        Ok(ex) if ex.success() => Status::Pass,
        Ok(ex) => Status::Fail(reason_bucket(&ex.stderr)),
    }
}

/// Distills `zeo`'s stderr into a short, aggregatable reason. A CLI-level
/// rejection is reported as `zeo: <path>: <message>`; a compiler panic puts
/// its message on the line after `panicked at <loc>`. Either way we keep the
/// message head (trimming the file-specific detail after ` -- `) so the same
/// class of rejection buckets together across files.
fn reason_bucket(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if let Some(l) = lines.iter().find(|l| l.starts_with("zeo: ")) {
        return normalize_reason(l.strip_prefix("zeo: ").unwrap());
    }
    if let Some(i) = lines.iter().position(|l| l.contains("panicked at")) {
        let msg = lines.get(i + 1).copied().unwrap_or(lines[i]);
        return format!("panic: {}", normalize_reason(msg));
    }
    normalize_reason(lines.first().copied().unwrap_or("(no error output)"))
}

fn normalize_reason(msg: &str) -> String {
    // Peel off any leading `<path>.rb: ` segments: zeo reports a
    // required-file failure as `<main.rb>: <required-abs-path>.rb: <message>`,
    // and those absolute paths are machine-specific -- keep only the message so
    // the same rejection buckets together (and the committed artifact stays
    // machine-independent). A path token has no spaces, so this can't eat a
    // real message (which starts with a word or a backtick).
    let mut msg = msg.trim();
    while let Some(pos) = msg.find(": ") {
        let head = &msg[..pos];
        if head.contains(' ') || !head.ends_with(".rb") {
            break;
        }
        msg = msg[pos + 2..].trim_start();
    }
    let head = msg.split(" -- ").next().unwrap_or(msg);
    let head = head.split(" (source at").next().unwrap_or(head).trim();
    truncate(head, 100)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Recursively collects every `.rb` file under `dir` (following the whole
/// tree; the full sweep, not just top-level entries).
fn collect_rb(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rb(&path, out);
        } else if path.extension().is_some_and(|e| e == "rb") {
            out.push(path);
        }
    }
}

/// Writes the TSV (per-file) and the Markdown (summary) artifacts under
/// `conformance/`, returning their paths.
fn write_artifacts(
    root: &Path,
    ruby_version: &str,
    results: &[(String, Status)],
) -> Result<(PathBuf, PathBuf), String> {
    let dir = root.join("conformance");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;

    let mut tsv = String::from("path\tstatus\treason\n");
    for (rel, status) in results {
        tsv.push_str(&format!("{rel}\t{}\t{}\n", status.tag(), status.reason()));
    }
    let tsv_path = dir.join("stdlib-status.tsv");
    std::fs::write(&tsv_path, tsv).map_err(|e| format!("writing {}: {e}", tsv_path.display()))?;

    let total = results.len();
    let pass = results
        .iter()
        .filter(|(_, s)| matches!(s, Status::Pass))
        .count();
    let timeout = results
        .iter()
        .filter(|(_, s)| matches!(s, Status::Timeout))
        .count();
    let harness = results
        .iter()
        .filter(|(_, s)| matches!(s, Status::HarnessError(_)))
        .count();
    let fail = total - pass - timeout - harness;

    // Aggregate failure reasons, most common first (count desc, then reason).
    let mut buckets: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, status) in results {
        if let Status::Fail(reason) = status {
            *buckets.entry(reason.as_str()).or_insert(0) += 1;
        }
    }
    let mut ranked: Vec<(&str, usize)> = buckets.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    let pct = |n: usize| {
        if total == 0 {
            0.0
        } else {
            n as f64 * 100.0 / total as f64
        }
    };
    let mut md = String::new();
    md.push_str("# stdlib compile status\n\n");
    md.push_str(
        "`cargo run -p xtask -- stdlib-status` -- whether `zeo` can compile \
         (Ruby -> Rust codegen only, no `rustc`/runtime) each `.rb` in the \
         installed Ruby stdlib `lib`, dropped in via `-I` (no bespoke flag).\n\n",
    );
    md.push_str(&format!(
        "- Ruby stdlib: **{ruby_version}** (`RbConfig rubylibdir`)\n"
    ));
    md.push_str(&format!("- Files swept: **{total}**\n"));
    md.push_str(&format!("- Compiles: **{pass}** ({:.1}%)\n", pct(pass)));
    md.push_str(&format!(
        "- Compile-errors: **{fail}** ({:.1}%)\n",
        pct(fail)
    ));
    if timeout > 0 {
        md.push_str(&format!("- Timeouts: **{timeout}**\n"));
    }
    if harness > 0 {
        md.push_str(&format!("- Harness errors: **{harness}**\n"));
    }
    md.push_str("\n## Top compile-error reasons\n\n");
    md.push_str("| count | reason |\n|------:|--------|\n");
    for (reason, count) in ranked.iter().take(30) {
        // Escape the table-cell delimiter so a `|` in a reason can't break the row.
        md.push_str(&format!("| {count} | {} |\n", reason.replace('|', "\\|")));
    }
    md.push_str("\nPer-file detail: `conformance/stdlib-status.tsv`.\n");
    let md_path = dir.join("STDLIB_STATUS.md");
    std::fs::write(&md_path, md).map_err(|e| format!("writing {}: {e}", md_path.display()))?;

    Ok((tsv_path, md_path))
}

fn print_summary(results: &[(String, Status)]) {
    let total = results.len();
    let pass = results
        .iter()
        .filter(|(_, s)| matches!(s, Status::Pass))
        .count();
    let pct = if total == 0 {
        0.0
    } else {
        pass as f64 * 100.0 / total as f64
    };
    eprintln!("stdlib-status: {pass}/{total} files compile ({pct:.1}%)");
}

/// Runs `ruby -e <expr>` and returns its trimmed stdout.
fn ruby_query(expr: &str) -> Result<String, String> {
    let out = Command::new("ruby")
        .arg("-e")
        .arg(expr)
        .output()
        .map_err(|e| format!("running ruby: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ruby -e exited non-zero: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
