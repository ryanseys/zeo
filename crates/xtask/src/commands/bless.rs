//! Re-record goldens from the ruby oracle, for the tests you name and no
//! others.
//!
//! Blessing used to be `ZEO_BLESS=1 cargo test`, and the missing word there is
//! WHICH. An unfiltered run rewrote, created and DELETED goldens across the
//! whole suite in one go. Nothing about the spelling suggested that:
//! `ZEO_BLESS=1` reads like a mode, not like "and apply it to all 5000 of
//! them". The filter below is that guard, and it is still required.
//!
//! Recording runs HERE rather than inside the test binary. Driving it through
//! `cargo nextest` cost ~53 seconds to write one file -- the whole of it
//! cargo, since the dev loop builds `release` and the test profile is `debug`,
//! so every bless after a build paid for a fresh debug compile of zeo and its
//! test binaries. The test itself took 0.18s.
//!
//! Nothing here needs the compiler: recording a golden is "run the oracle,
//! write what it said", and the check that the recording was RIGHT is the
//! suite, which is where it belongs.
//!
//! There is deliberately no `--allow-delete`. Removing `<rb>.err.expected` is
//! part of a CORRECT bless -- an absent file is how a golden says "stderr must
//! be empty" -- so gating it would break the ordinary single-test case this
//! exists to make easy. The filter bounds the blast radius; the summary is
//! what makes a deletion impossible to miss.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::exec::{self, Capture};
use crate::ruby::Oracle;
use crate::{Error, root, root_join};

/// Everything a bless can write.
const WATCHED: &str = "tests";

/// Where `make install-deps` resolves Gemfile.lock.
const BUNDLE: &str = "vendor/bundle";

/// How a suite's cases sit under its root.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// `<root>/*.rb`
    Flat,
    /// `<root>/*.rb` and `<root>/pending/*.rb`
    FlatAndPending,
    /// `<root>/*/*.rb`
    OneDeep,
}

/// The golden suites, as `datatest_stable::harness!` declares them: the name
/// prefix a filter matches against, the root, and where cases sit. Kept in
/// step with `crates/zeo/tests/*.rs`.
struct Suite {
    prefix: &'static str,
    root: &'static str,
    layout: Layout,
    /// A `tests/divergences/` golden records ZEO's output, not the oracle's.
    records_zeo: bool,
    /// The one suite whose zeo side needs `vendor/bundle`'s rspec on its load
    /// path.
    zeo_reads_store: bool,
}

const SUITES: &[Suite] = &[
    Suite { prefix: "example", root: "tests", layout: Layout::Flat, records_zeo: false, zeo_reads_store: false },
    Suite { prefix: "divergence", root: "tests/divergences", layout: Layout::Flat, records_zeo: true, zeo_reads_store: false },
    Suite { prefix: "macos_only", root: "tests/macos", layout: Layout::Flat, records_zeo: false, zeo_reads_store: false },
    Suite { prefix: "jit_only", root: "tests/jit", layout: Layout::Flat, records_zeo: false, zeo_reads_store: false },
    Suite { prefix: "gap", root: "tests/gaps", layout: Layout::Flat, records_zeo: false, zeo_reads_store: false },
    Suite { prefix: "spinel", root: "tests/spinel", layout: Layout::Flat, records_zeo: false, zeo_reads_store: false },
    Suite { prefix: "milestone", root: "tests/milestones", layout: Layout::FlatAndPending, records_zeo: false, zeo_reads_store: true },
    Suite { prefix: "gemtest", root: "tests/gemtests", layout: Layout::OneDeep, records_zeo: false, zeo_reads_store: false },
];

const USAGE: &str = "\
usage: cargo xtask bless <filter>

<filter> is a substring of the test name -- `<suite>::<path>`, the same
spelling nextest reports -- and it is required: blessing everything at once is
what this command exists to prevent. Examples:

  cargo xtask bless forward_args
  cargo xtask bless spinel::yield_
  cargo xtask bless gap::

A `tests/divergences/` golden records ZEO's output instead of the oracle's,
and needs a built `target/release/zeo` (or `ZEO_BIN`).
";

pub fn run(args: &[String]) -> Result<(), Error> {
    let filter = args.iter().find(|a| !a.starts_with('-'));
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let Some(filter) = filter else {
        return Err(Error::new(USAGE.to_string()));
    };
    if let Some(why) = too_broad(filter) {
        return Err(Error::new(format!("refusing {filter:?} -- {why}")));
    }
    bless(filter)
}

/// Filters that would defeat the point.
fn too_broad(filter: &str) -> Option<&'static str> {
    match filter.trim() {
        "" => Some("an empty filter selects every test"),
        "*" | "all" => Some("this is a substring, not a glob -- it selects every test"),
        _ => None,
    }
}

/// Bless every matching golden. Public so `diff` can file a gap through the
/// ONE golden writer rather than capturing the oracle itself -- a hand-rolled
/// capture there once skipped the address scrub, so a snippet printing
/// `#<Object:0x...>` recorded a process-random address into its golden.
pub fn bless(filter: &str) -> Result<(), Error> {
    let cases = select_cases(filter)?;
    if cases.is_empty() {
        eprintln!("bless: no golden matches {filter:?}");
        return Ok(());
    }
    let before = changed_goldens()?;
    eprintln!(
        "bless: re-recording {} golden(s) matching {filter:?}",
        cases.len()
    );
    // Each case is one oracle process writing its own two files, so the work
    // is subprocess-bound and shares nothing. Recording the whole examples
    // suite serially is ~52s of ruby startup; at core width it is a few
    // seconds.
    record_all(&cases)?;
    report(&before, &changed_goldens()?, filter);
    Ok(())
}

/// Every golden whose `<suite>::<relative path>` name contains `filter`.
fn select_cases(filter: &str) -> Result<Vec<(PathBuf, &'static Suite)>, Error> {
    let mut seen: BTreeMap<PathBuf, &'static Suite> = BTreeMap::new();
    for suite in SUITES {
        let root = root_join(suite.root);
        for rel in cases_under(&root, suite.layout)? {
            if format!("{}::{rel}", suite.prefix).contains(filter) {
                seen.entry(root.join(&rel)).or_insert(suite);
            }
        }
    }
    Ok(seen.into_iter().collect())
}

fn cases_under(root: &Path, layout: Layout) -> Result<Vec<String>, Error> {
    let mut out = ruby_files(root, "")?;
    match layout {
        Layout::Flat => {}
        Layout::FlatAndPending => out.extend(ruby_files(&root.join("pending"), "pending/")?),
        Layout::OneDeep => {
            out.clear();
            for dir in sorted_dirs(root)? {
                let name = dir.file_name().expect("a directory name").to_string_lossy();
                out.extend(ruby_files(&dir, &format!("{name}/"))?);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn ruby_files(dir: &Path, prefix: &str) -> Result<Vec<String>, Error> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".rb") && entry.path().is_file() {
            out.push(format!("{prefix}{name}"));
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

/// Goldens run with the tests directory as cwd (`golden::tests_run_cwd`),
/// which is what a relative path inside one resolves against.
fn run_cwd() -> PathBuf {
    root_join("tests")
}

/// One worker per core, each pulling the next case. The oracle dominates, so
/// the split does not need to be clever -- only wide.
fn record_all(cases: &[(PathBuf, &'static Suite)]) -> Result<(), Error> {
    let oracle = Oracle::find();
    let width = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(cases.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..width {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((rb, suite)) = cases.get(i) else {
                        return;
                    };
                    if let Err(e) = record(rb, suite, &oracle) {
                        failures.lock().expect("no panicking worker").push(e);
                    }
                }
            });
        }
    });
    match failures.into_inner().expect("no panicking worker").pop() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn record(rb: &Path, suite: &Suite, oracle: &Oracle) -> Result<(), Error> {
    let source = std::fs::read_to_string(rb)
        .map_err(|e| Error::new(format!("reading {}: {e}", rb.display())))?;
    let argv: Vec<String> = sidecar(rb, ".args")?
        .map(|b| String::from_utf8_lossy(&b).split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let stdin = sidecar(rb, ".stdin")?;
    let out = if suite.records_zeo {
        run_zeo(rb, &source, &argv, stdin.as_deref(), suite)?
    } else {
        run_oracle(rb, &source, &argv, stdin.as_deref(), oracle)?
    };
    let expected = with_suffix(rb, ".expected");
    std::fs::write(&expected, norm(&out.stdout, rb))
        .map_err(|e| Error::new(format!("writing {}: {e}", expected.display())))?;

    let err_path = with_suffix(rb, ".err.expected");
    let err = norm(&out.stderr, rb);
    // An absent `.err.expected` is a real assertion: "stderr must be empty".
    // So an empty capture DELETES rather than writing nothing.
    if err.is_empty() {
        match std::fs::remove_file(&err_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::new(format!("removing {}: {e}", err_path.display()))),
        }
    } else {
        std::fs::write(&err_path, err)
            .map_err(|e| Error::new(format!("writing {}: {e}", err_path.display())))?;
    }
    Ok(())
}

fn with_suffix(rb: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", rb.display()))
}

fn sidecar(rb: &Path, suffix: &str) -> Result<Option<Vec<u8>>, Error> {
    let path = with_suffix(rb, suffix);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::new(format!("reading {}: {e}", path.display()))),
    }
}

/// The oracle invocation the harness uses, flag for flag: no error_highlight
/// or did_you_mean (zeo implements neither), the hermetic bundle env, and the
/// experimental-namespace flags a `Ruby::Box` example needs -- naming the
/// class is not enough, because CRuby's disabled-mode surface is a smaller
/// one.
fn run_oracle(
    rb: &Path,
    source: &str,
    argv: &[String],
    stdin: Option<&[u8]>,
    oracle: &Oracle,
) -> Result<exec::Output, Error> {
    let mut flags: Vec<&str> = Vec::new();
    if source.contains("Ruby::Box") {
        flags.push("-W:no-experimental");
    }
    let mut cmd: Vec<String> = oracle.argv(&flags);
    cmd.push(rb.display().to_string());
    cmd.extend(argv.iter().cloned());
    let mut env = oracle.env();
    if source.contains("Ruby::Box.new") {
        env.push(("RUBY_BOX", Some("1")));
    }
    exec::run_with_stdin(&cmd, &run_cwd(), &env, Capture::Both, stdin)
}

/// A `tests/divergences/` golden records zeo's own answer on purpose.
/// Recording the oracle's would replace the golden with the very output the
/// program exists to differ from.
///
/// The run-time dials the harness gives zeo have to be given here too, or the
/// recording is of a program that refused to start: without `RUBY_BOX` the box
/// goldens record "Ruby Box is disabled" as their stderr, which then asserts
/// that forever.
fn run_zeo(
    rb: &Path,
    source: &str,
    argv: &[String],
    stdin: Option<&[u8]>,
    suite: &Suite,
) -> Result<exec::Output, Error> {
    let bin = zeo_bin();
    if !bin.is_file() {
        return Err(Error::new(format!(
            "{} is a decided divergence and records zeo, but {} is not built -- \
             `cargo build --release -p zeo` first",
            rb.display(),
            bin.display()
        )));
    }
    let mut env: Vec<(&str, Option<&str>)> = Vec::new();
    if source.contains("Ruby::Box.new") {
        env.push(("RUBY_BOX", Some("1")));
    }
    if with_suffix(rb, ".gc").exists() {
        env.push(("ZEO_GC", Some("1")));
    }
    if with_suffix(rb, ".leakcheck").exists() {
        env.push(("ZEO_RT_LEAKCHECK", Some("1")));
    }
    let mut cmd = vec![bin.display().to_string()];
    if suite.zeo_reads_store {
        for dir in gem_store_libs()? {
            cmd.push("-I".into());
            cmd.push(dir.display().to_string());
        }
    }
    cmd.push(rb.display().to_string());
    cmd.extend(argv.iter().cloned());
    exec::run_with_stdin(&cmd, &run_cwd(), &env, Capture::Both, stdin)
}

/// The zeo binary a command should drive. `ZEO_BIN` overrides; otherwise the
/// release build, which is what every golden was recorded against.
pub fn zeo_bin() -> PathBuf {
    match std::env::var_os("ZEO_BIN") {
        Some(bin) => PathBuf::from(bin),
        None => root_join("target/release/zeo"),
    }
}

/// `vendor/bundle`'s rspec trees, for the one suite whose ZEO side needs those
/// gems too. The oracle reaches them through its own env. Scoped to rspec
/// rather than the whole bundle, which would put upstream copies of gems zeo
/// vendors ahead of its own.
fn gem_store_libs() -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    for abi in sorted_dirs(&root_join(BUNDLE).join("ruby"))? {
        for gem in sorted_dirs(&abi.join("gems"))? {
            let name = gem.file_name().expect("a directory name").to_string_lossy();
            if (name.starts_with("rspec") || name.starts_with("diff-lcs")) && gem.join("lib").is_dir()
            {
                out.push(gem.join("lib"));
            }
        }
    }
    Ok(out)
}

/// `golden::norm`, ported. The harness applies it to BOTH sides at compare
/// time, so recording without it still PASSES -- but object addresses and
/// thread ids are process-random, so an unscrubbed recording rewrites those
/// goldens with fresh noise on every run. A bless has to be a no-op when
/// nothing changed, which is what makes its summary readable.
///
/// It works on BYTES. A golden may hold output that is not valid UTF-8 -- the
/// encoding suites exist to produce exactly that -- and a lossy decode here
/// would rewrite those bytes into replacement characters.
fn norm(bytes: &[u8], rb: &Path) -> Vec<u8> {
    use regex::bytes::Regex;
    use std::sync::OnceLock;

    let out = replace_bytes(bytes, b"\r\n", b"\n");
    // Both engines embed the absolute source path in `__FILE__` and in
    // backtraces; the committed form is run-cwd-relative so a golden is
    // portable. The program is given the ABSOLUTE path, exactly as the harness
    // gives it, and this rewrites it back. Handing it the relative path
    // instead looks equivalent and is not: what a program derives from
    // `__FILE__`/`$0` changes with it -- rspec prints `./milestones/foo.rb`
    // from an absolute argument and `milestones/foo.rb` from a relative one.
    let abs = rb.display().to_string();
    let rel = abs
        .strip_prefix(&format!("{}/", run_cwd().display()))
        .unwrap_or(&abs)
        .to_string();
    let out = replace_bytes(&out, abs.as_bytes(), rel.as_bytes());

    // `0x` + 8..16 hex digits: ruby's own `#<Object:0x...>` (16) and the
    // ASLR'd frame addresses a Rust abort prints (9-12). The run must END
    // there -- a LONGER run is a value, not an address, and taking its first
    // 16 digits turned `0x400000000000000000` into `0xADDR00`. Shorter runs
    // stay too: a program printing `0x1f` keeps its value. The whole run is
    // matched and its length checked, because this crate's regex engine has
    // no lookahead to spell "and no hex digit follows".
    static ADDRESS: OnceLock<Regex> = OnceLock::new();
    let address = ADDRESS.get_or_init(|| Regex::new(r"0x[0-9a-f]{8,}").expect("a valid pattern"));
    let out = address.replace_all(&out, |caps: &regex::bytes::Captures| {
        let whole = &caps[0];
        if whole.len() <= 2 + 16 {
            b"0xADDR".to_vec()
        } else {
            whole.to_vec()
        }
    });

    // `thread 'ruby-main' (156051069) panicked` -- the OS thread id differs
    // per process.
    static TID: OnceLock<Regex> = OnceLock::new();
    let tid = TID.get_or_init(|| Regex::new(r"' \(\d+\) panicked").expect("a valid pattern"));
    tid.replace_all(&out, &b"' (TID) panicked"[..]).into_owned()
}

fn replace_bytes(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() || haystack.len() < from.len() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut at = 0;
    while at < haystack.len() {
        if haystack[at..].starts_with(from) {
            out.extend_from_slice(to);
            at += from.len();
        } else {
            out.push(haystack[at]);
            at += 1;
        }
    }
    out
}

/// `(status, path)` for every golden git reports as changed.
fn changed_goldens() -> Result<Vec<(String, String)>, Error> {
    // `-uall`, because git COLLAPSES an untracked directory to one entry: a
    // whole new golden dir read as a single unchanged line before and after,
    // so a bless that wrote its first goldens reported nothing.
    let out = exec::run(
        &["git", "status", "--porcelain", "-uall", "--", WATCHED],
        root(),
        &[],
        Capture::Both,
    )?;
    if !out.success() {
        return Ok(Vec::new());
    }
    Ok(out
        .stdout_text()
        .lines()
        .filter(|l| l.len() >= 4)
        .map(|l| (l[..3].trim().to_string(), l[3..].to_string()))
        .collect())
}

fn report(before: &[(String, String)], after: &[(String, String)], filter: &str) {
    let changed: Vec<&(String, String)> = after.iter().filter(|e| !before.contains(e)).collect();
    if changed.is_empty() {
        eprintln!("bless: no golden changed -- {filter:?} already records the oracle");
        return;
    }
    eprintln!("bless: {} golden(s) changed:", changed.len());
    for (code, path) in &changed {
        eprintln!("  {code:>2}  {path}");
    }
    let deleted = changed.iter().filter(|(code, _)| code.contains('D')).count();
    if deleted > 0 {
        // An absent `.err.expected` is a real assertion ("stderr must be
        // empty"), so a deletion changes the contract as much as a rewrite --
        // and it is the one change `git diff` shows nothing for.
        eprintln!(
            "\nbless: {deleted} golden(s) were DELETED. That asserts their stderr is now empty;\n\
             if that is not what you meant, `git checkout` them."
        );
    }
}
