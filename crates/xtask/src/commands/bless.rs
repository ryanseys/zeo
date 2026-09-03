//! Record a program's answer under its `__END__`, for the programs you name
//! and no others.
//!
//! The filter is required: an unfiltered bless once rewrote every golden in
//! one go, and nothing about the spelling suggested that. `--all` says it.
//!
//! Recording runs HERE rather than inside the test binary: driving it through
//! `cargo nextest` paid for a fresh debug compile of zeo and its test
//! binaries for one 0.18s recording. Nothing here needs the compiler for a
//! ruby-recorded suite; a zeo-recorded one (`errors/`, `divergences/`) runs
//! the release `zeo` (or `ZEO_BIN`).
//!
//! This is the ONE trailer writer. The harness only reads.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::case::{Answer, Case, Trailer};
use crate::compare::norm;
use crate::exec::{self, Capture};
use crate::oracle::{self, Oracle};
use crate::suites::{RUN_CWD, Recorder, SUITES, Suite};
use crate::{Error, root, root_join};

const USAGE: &str = "\
usage: cargo xtask bless <filter>... | --all

<filter> is a substring of the case name -- `<suite>::<path>`, the spelling
nextest reports -- or a path under test/. Examples:

  cargo xtask bless forward_args
  cargo xtask bless core::string/
  cargo xtask bless test/lang/blocks/a_lambda_takes_a_literal_block.rb

`errors/`, `features/` and `divergences/` record ZEO's own answer and need a built
`target/release/zeo` (or `ZEO_BIN`). Everything else records the pinned
ruby (`.ruby-version`, `ZEO_RUBY`).
";

pub fn run(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let all = args.iter().any(|a| a == "--all");
    let filters: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with("--"))
        .collect();
    if filters.is_empty() && !all {
        return Err(Error::new(USAGE.to_string()));
    }
    for f in &filters {
        if let Some(why) = too_broad(f) {
            return Err(Error::new(format!(
                "refusing {f:?} -- {why}; say --all if you mean it"
            )));
        }
    }
    bless(&filters, all)
}

fn too_broad(filter: &str) -> Option<&'static str> {
    match filter.trim() {
        "" => Some("an empty filter selects every program"),
        "*" | "all" => Some("this is a substring, not a glob -- it selects every program"),
        _ => None,
    }
}

/// Bless every matching program. Public so `diff` can file a gap through the
/// ONE writer rather than capturing the oracle itself.
pub fn bless(filters: &[&str], all: bool) -> Result<(), Error> {
    let cases = select_cases(filters, all)?;
    if cases.is_empty() {
        eprintln!("bless: no program matches {filters:?}");
        return Ok(());
    }
    let needs_oracle = cases.iter().any(|(_, s)| s.recorder == Recorder::Ruby);
    let oracle = match needs_oracle {
        true => {
            // The oracle reads `Gemfile.lock` through `bundler/setup`, so its
            // own store has to hold the lock's gems before a recording means
            // anything. `deps --oracle` is idempotent and says nothing when
            // the store is already complete.
            crate::commands::deps::run(&["--oracle".to_string()])?;
            Some(oracle::find(root()).map_err(Error::new)?)
        }
        false => None,
    };
    let needs_zeo = cases.iter().any(|(_, s)| s.recorder == Recorder::Zeo);
    if needs_zeo && !zeo_bin().is_file() {
        return Err(Error::new(format!(
            "a zeo-recorded suite is selected but {} is not built -- `cargo build --release -p zeo` first",
            zeo_bin().display()
        )));
    }
    eprintln!("bless: recording {} program(s)", cases.len());
    let written = record_all(&cases, oracle.as_ref())?;
    match written.len() {
        0 => {
            eprintln!("bless: nothing changed -- every selected program already records its answer")
        }
        n => {
            eprintln!("bless: {n} program(s) changed:");
            for p in &written {
                eprintln!("  {}", p.strip_prefix(root()).unwrap_or(p).display());
            }
        }
    }
    Ok(())
}

/// Every program whose `<suite>::<path>` name or `test/...` path contains a
/// filter.
fn select_cases(filters: &[&str], all: bool) -> Result<Vec<(PathBuf, &'static Suite)>, Error> {
    let mut out = Vec::new();
    for suite in SUITES {
        for rel in suite.cases(root()).map_err(|e| Error::new(e.to_string()))? {
            let name = format!("{}::{}", suite.name, rel.display());
            let path = format!("{}/{}", suite.root, rel.display());
            if all
                || filters
                    .iter()
                    .any(|f| name.contains(f) || path.contains(f.trim_start_matches("./")))
            {
                out.push((root_join(&path), suite));
            }
        }
    }
    Ok(out)
}

fn run_cwd() -> PathBuf {
    root_join(RUN_CWD)
}

/// One worker per core, each pulling the next program. The child dominates,
/// so the split does not need to be clever -- only wide.
fn record_all(
    cases: &[(PathBuf, &'static Suite)],
    oracle: Option<&Oracle>,
) -> Result<Vec<PathBuf>, Error> {
    let width = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(cases.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    let written = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..width {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((rb, suite)) = cases.get(i) else {
                        return;
                    };
                    match record(rb, suite, oracle) {
                        Ok(true) => written
                            .lock()
                            .expect("no panicking worker")
                            .push(rb.clone()),
                        Ok(false) => {}
                        Err(e) => failures.lock().expect("no panicking worker").push(e),
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().expect("no panicking worker");
    let mut written = written.into_inner().expect("no panicking worker");
    written.sort();
    match failures.len() {
        0 => Ok(written),
        n => {
            for e in &failures {
                eprintln!("bless: {e}");
            }
            Err(Error::new(format!("{n} program(s) could not be recorded")))
        }
    }
}

fn stdin_of(case: &Case, rb: &Path) -> Result<Option<Vec<u8>>, Error> {
    let Some(rel) = &case.directives.stdin else {
        return Ok(None);
    };
    let path = rb.parent().unwrap_or(rb).join(rel);
    std::fs::read(&path)
        .map(Some)
        .map_err(|e| Error::new(format!("{}: stdin {}: {e}", rb.display(), path.display())))
}

/// Record one program; `true` when the file changed.
fn record(rb: &Path, suite: &Suite, oracle: Option<&Oracle>) -> Result<bool, Error> {
    let bytes = std::fs::read(rb).map_err(|e| Error::new(format!("{}: {e}", rb.display())))?;
    let case = Case::parse(&bytes).map_err(|e| Error::new(format!("{}: {e}", rb.display())))?;
    let stdin = stdin_of(&case, rb)?;
    let out = match suite.recorder {
        Recorder::Ruby => {
            let oracle = oracle.expect("an oracle was found for a ruby-recorded suite");
            let mut cmd = oracle.command(root());
            cmd.args(&case.directives.ruby);
            for (k, v) in &case.directives.env {
                cmd.env(k, v);
            }
            cmd.arg(rb)
                .args(&case.directives.args)
                .current_dir(run_cwd());
            exec::run_command(&mut cmd, Capture::Both, stdin.as_deref())?
        }
        Recorder::Zeo => {
            let mut cmd = Command::new(zeo_bin());
            cmd.arg("--backend").arg("jit");
            cmd.args(&case.directives.zeo);
            for (k, v) in case.directives.env.iter().chain(&case.directives.zeo_env) {
                cmd.env(k, v);
            }
            if suite.whole_graph {
                for dir in oracle_store_rspec_libs()? {
                    cmd.arg("-I").arg(dir);
                }
            }
            cmd.arg(rb)
                .args(&case.directives.args)
                .current_dir(run_cwd());
            exec::run_command(&mut cmd, Capture::Both, stdin.as_deref())?
        }
    };
    let cwd = run_cwd();
    let answer = Answer {
        stdout: norm(&out.stdout, rb, &cwd),
        stderr: norm(&out.stderr, rb, &cwd),
        exit: out.exit(),
    };
    // A program whose answer is the PLATFORM's rather than ruby's carries a
    // second, linux section. Bless updates that one only when it is already
    // there: creating the split is a deliberate act, and without this rule a
    // bless on linux would replace the macOS answer with the linux one.
    let old = case.trailer.clone().unwrap_or_default();
    let trailer = match (cfg!(target_os = "linux"), old.linux.is_some()) {
        (true, true) => Trailer {
            answer: old.answer,
            linux: Some(answer),
        },
        (true, false) => Trailer {
            answer,
            linux: None,
        },
        (false, _) => Trailer {
            answer,
            linux: old.linux,
        },
    };
    let rendered = Case::render(&case.program, &trailer);
    if rendered == bytes {
        return Ok(false);
    }
    std::fs::write(rb, rendered)
        .map_err(|e| Error::new(format!("writing {}: {e}", rb.display())))?;
    Ok(true)
}

/// The zeo binary a recording drives. `ZEO_BIN` overrides; otherwise the
/// release build, which is what every answer was recorded against.
pub fn zeo_bin() -> PathBuf {
    match std::env::var_os("ZEO_BIN") {
        Some(bin) => PathBuf::from(bin),
        None => root_join("target/release/zeo"),
    }
}

/// The oracle store's rspec trees, for the one suite whose ZEO side needs
/// those gems too.
fn oracle_store_rspec_libs() -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    let ruby = root_join("vendor/bundle/ruby");
    for abi in sorted_dirs(&ruby)? {
        for gem in sorted_dirs(&abi.join("gems"))? {
            let name = gem
                .file_name()
                .expect("a directory name")
                .to_string_lossy()
                .into_owned();
            if (name.starts_with("rspec") || name.starts_with("diff-lcs"))
                && gem.join("lib").is_dir()
            {
                out.push(gem.join("lib"));
            }
        }
    }
    Ok(out)
}

fn sorted_dirs(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|e| Error::new(format!("reading {}: {e}", root.display())))?
            .path();
        if path.is_dir() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
