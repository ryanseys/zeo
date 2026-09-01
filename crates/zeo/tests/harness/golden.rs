//! Golden-file helper for the datatest-stable suite (`tests/goldens.rs`).
//!
//! One function, [`run_golden`], drives every `.rb`: spawn the built `zeo`
//! CLI on the chosen backend, then diff stdout/stderr against the committed
//! **ruby-oracle** golden `.expected` (+ `.err.expected`/`.args`/`.stdin`
//! sidecars).
//!
//! A `.cext` sidecar names an extension directory under `tests/cext/` that
//! the case requires. It is built TWICE, once per engine's headers -- see
//! [`cext_root`] for that half and why the other one lives in `xtask bless`.
//!
//! **A golden's contract is its DIRECTORY**, which is what the suite passes
//! in as a [`Mode`]:
//!
//! - `Mode::Pass` (`tests/`, `tests/spinel/`, ...): zeo must MATCH the golden.
//! - `Mode::Xfail` (`tests/gaps/`): zeo must DIVERGE from the golden -- a match
//!   means the gap is fixed and the test FAILS with a "promote" message.
//! - `Mode::Divergence` (`tests/divergences/`): zeo must MATCH, but the golden
//!   records ZEO's own output, because answering differently there is a
//!   decision.
//!
//! A directory that only runs on one platform or one backend (`tests/macos/`,
//! `tests/jit/`) is skipped by its own suite entry. Nothing about a golden's
//! contract is decided per file.
//!
//! A program zeo REJECTS is a divergence like any other: the child prints
//! the compiler's own error on stderr and the comparison fails on it. (A
//! `Mode::CompileFail` once probed the emitter in process to classify
//! rejections exactly -- its suite is gone, and the probe compiled every
//! golden TWICE, so both went.)
//!
//!
//! A `.gccheck` sidecar records the exit cycle census the `ZEO_RT_GCCHECK=1`
//! leg gates against. It is not a leak report: it names the ring the program
//! builds on purpose. See [`check_gccheck_census`].
//!
//! `cargo xtask bless <filter>` re-records the goldens from the real `ruby` oracle
//! (`--disable-error_highlight --disable-did_you_mean`, resolved via `mise`)
//! instead of asserting. This is the single golden writer.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::normalize::{normalize_addresses, normalize_thread_ids};
use crate::paths::{resolve_ruby, workspace_root};
use crate::zeo_bin::zeo_cli;

/// Hard bounds on any child this harness runs (a compiled golden binary, or the
/// ruby oracle).
///
/// Without them a single miscompiled `.rb` takes the machine down rather than
/// failing: `Command::output()` reads both pipes to EOF into an unbounded
/// `Vec<u8>`, and because the parent always drains, the child is never
/// backpressured -- so `loop { puts "x" }` writes into the test process's memory
/// at full speed, forever. Neither nextest nor the harness had anything that
/// would stop it. A run that trips either bound is reported as an ordinary test
/// FAILURE naming the bound, which is what the corpus wants: a hang is a
/// divergence from ruby like any other.
const MAX_CAPTURE: usize = 64 << 20; // 64 MiB per stream
// 120s, not 60: a whole-gem golden (`require "rss"`) compiles in ~12s alone
// and ran past 60s under the loaded full suite. A hang still dies, just
// later.
const RUN_DEADLINE: Duration = Duration::from_secs(120);

/// The third bound, and the one the other two miss: a child that ALLOCATES
/// without writing. `(1..).to_a` -- an endless range zeo evaluates eagerly --
/// prints nothing, so `MAX_CAPTURE` never trips, and it spends the whole
/// deadline growing the heap.
///
/// Measured, on a 16 GiB machine: four such children at `--test-threads 4`
/// took free memory from 6.9 GiB to 0.06 GiB in five and a half seconds, and
/// the corpus has six of them. Against that, 741 ordinary golden children peak
/// at 13 MiB. So the cap can sit far below anything legitimate -- but the
/// corpus now carries whole-gem compiles too: `require "rss"` peaks at 385
/// MiB alone and crossed a 512 MiB cap only under a loaded suite. 768 MiB is
/// 2x that gem-graph peak and still a fraction of what a runaway wants.
const MAX_CHILD_RSS: u64 = 768 << 20; // 768 MiB

/// [`MAX_CHILD_RSS`], with an env override (`ZEO_GOLDEN_MAX_RSS`, in MiB)
/// for the cases that legitimately need more, the same way
/// `ZEO_GOLDEN_RUN_DEADLINE` stretches the clock for them. A case that
/// compiles a whole gem's require graph in the child (a milestone case) is
/// doing real work at a scale the 512 MiB figure was never measured
/// against.
fn max_child_rss() -> u64 {
    static M: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *M.get_or_init(|| {
        std::env::var("ZEO_GOLDEN_MAX_RSS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mib| mib << 20)
            .unwrap_or(MAX_CHILD_RSS)
    })
}

/// The address-space ceiling, which is a DIFFERENT quantity from the RSS cap
/// above and must not borrow its number.
///
/// `MAX_CHILD_RSS` was used here directly, and on macOS that was harmless
/// because **Darwin accepts `setrlimit(RLIMIT_AS)` and does not enforce it**
/// (a child measured 2.29 GiB under a 2 GiB limit). Linux enforces it, and a
/// perfectly ordinary zeo program RESERVES far more than 512 MiB of address
/// space without touching it: a 64 MiB `ruby-main` stack, 8 MiB per Ruby
/// thread, and mimalloc's arenas. Every thread golden died on Linux with
/// `pthread_create` -> EAGAIN and a runtime panic, which reads as a
/// concurrency bug and is a harness one.
///
/// 4 GiB is the ceiling instead: comfortably above anything legitimate, and
/// still under what a runaway wants -- the measured one took free memory from
/// 6.9 GiB to 0.06 GiB in five and a half seconds.
const MAX_CHILD_ADDRESS_SPACE: u64 = 4 << 30; // 4 GiB

/// `RLIMIT_AS` is the cheap half of the bound -- the child dies on its own
/// allocation failure, with no polling. It is NOT the enforcing half (see
/// above): [`child_rss`] and the watchdog in [`run_bounded`] hold the line.
/// This stays because it does work on Linux, where it kills a runaway sooner
/// and cheaper.
///
/// `pre_exec` is unsafe because the closure runs in the forked child, where
/// only async-signal-safe calls are legal; `setrlimit` is one of them, and the
/// closure does nothing else.
#[cfg(unix)]
fn bound_address_space(cmd: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    unsafe {
        cmd.pre_exec(|| {
            let lim = libc::rlimit {
                rlim_cur: MAX_CHILD_ADDRESS_SPACE,
                rlim_max: MAX_CHILD_ADDRESS_SPACE,
            };
            // A platform that ignores or refuses RLIMIT_AS must not fail the
            // spawn: the watchdog is the one that has to work everywhere.
            libc::setrlimit(libc::RLIMIT_AS, &lim);
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn bound_address_space(_cmd: &mut Command) {}

/// Resident set size of `pid`, in bytes, or `None` if it can't be read (the
/// process just exited, or the platform isn't covered -- either way the
/// watchdog simply doesn't fire and the other two bounds still apply).
#[cfg(target_os = "macos")]
fn child_rss(pid: u32) -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let ptr = std::ptr::addr_of_mut!(info).cast::<libc::c_void>();
    let n = unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTASKINFO, 0, ptr, size) };
    (n == size).then_some(info.pti_resident_size)
}

#[cfg(target_os = "linux")]
fn child_rss(pid: u32) -> Option<u64> {
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn child_rss(_pid: u32) -> Option<u64> {
    None
}

/// [`RUN_DEADLINE`], with an env override (`ZEO_GOLDEN_RUN_DEADLINE`, in
/// seconds) for suites whose cases legitimately run longer -- a vendored
/// library's whole require graph is one case in the `milestone` suite. The
/// deadline
/// also bounds the ruby oracle during bless, so both sides stretch together.
fn run_deadline() -> Duration {
    static D: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *D.get_or_init(|| {
        std::env::var("ZEO_GOLDEN_RUN_DEADLINE")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(RUN_DEADLINE)
    })
}

/// Run `cmd` to completion, capturing at most [`MAX_CAPTURE`] per stream and
/// killing it after [`RUN_DEADLINE`].
///
/// Both pipes are drained on their own threads, and `stdin` is written on one
/// too: writing it inline (as this harness used to) deadlocks whenever the child
/// fills its stdout pipe before consuming all of stdin.
fn run_bounded(
    cmd: &mut Command,
    stdin: Option<&[u8]>,
    what: &str,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    use std::io::Read as _;
    use std::process::Stdio;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    match stdin {
        Some(_) => cmd.stdin(Stdio::piped()),
        None => cmd.stdin(Stdio::null()),
    };
    bound_address_space(cmd);
    let mut child = cmd.spawn().map_err(|e| format!("spawn {what}: {e}"))?;

    // Set by either reader the moment its stream passes the cap.
    let overflowed = Arc::new(AtomicBool::new(false));

    let spawn_reader = |mut pipe: Box<dyn std::io::Read + Send>, flag: Arc<AtomicBool>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.len() > MAX_CAPTURE {
                            // Stop draining: the child blocks on its next write
                            // and the deadline loop kills it.
                            flag.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            }
            buf
        })
    };

    let out_h = spawn_reader(
        Box::new(child.stdout.take().expect("piped stdout")),
        Arc::clone(&overflowed),
    );
    let err_h = spawn_reader(
        Box::new(child.stderr.take().expect("piped stderr")),
        Arc::clone(&overflowed),
    );
    let in_h = stdin.map(|input| {
        let mut sink = child.stdin.take().expect("piped stdin");
        let input = input.to_vec();
        std::thread::spawn(move || {
            use std::io::Write as _;
            // A child that exits without reading stdin gives us EPIPE; that is
            // its business, not an error here.
            let _ = sink.write_all(&input);
        })
    });

    let started = Instant::now();
    let pid = child.id();
    let mut limit = None;
    let mut ticks: u32 = 0;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Err(e) => return Err(format!("waiting on {what}: {e}")),
            Ok(None) => {}
        }
        if overflowed.load(Ordering::SeqCst) {
            limit = Some(format!("wrote more than {} MiB", MAX_CAPTURE >> 20));
        } else if started.elapsed() > run_deadline() {
            limit = Some(format!("ran longer than {}s", run_deadline().as_secs()));
        } else if ticks.is_multiple_of(10) {
            // Every ~100ms, not every 10ms: reading RSS is a syscall per child
            // and a runaway needs seconds to matter, not milliseconds.
            if let Some(rss) = child_rss(pid)
                && rss > max_child_rss()
            {
                limit = Some(format!("allocated more than {} MiB", max_child_rss() >> 20));
            }
        }
        if limit.is_some() {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        ticks = ticks.wrapping_add(1);
        std::thread::sleep(Duration::from_millis(10));
    }

    let out = out_h.join().unwrap_or_default();
    let err = err_h.join().unwrap_or_default();
    if let Some(h) = in_h {
        let _ = h.join();
    }
    match limit {
        Some(why) => Err(format!("{what} killed: {why}")),
        None => Ok((out, err)),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The program must run and match the golden (corpus, examples).
    Pass,
    /// The program is a known failure: it must NOT match the golden yet.
    Xfail,
    /// `tests/divergences/`: zeo answers differently ON PURPOSE, so the
    /// golden records ZEO's own output rather than the oracle's. It must
    /// still MATCH -- what changes is only which engine `bless` reads.
    ///
    /// The DIRECTORY says so. A per-file sidecar used to, which meant the
    /// contract of a golden was invisible from its name.
    Divergence,
}

/// Working directory every golden suite's programs run in (and the base their
/// source paths are relativized against, so backtraces read `spinel/x.rb`,
/// `gaps/x.rb`, or `x.rb`). The `.args` fixture paths (e.g. the ARGF input) are
/// relative to this too. All three suites live under `tests/`, so they share it.
pub fn tests_run_cwd() -> PathBuf {
    workspace_root().join("tests")
}

// ---- normalization ----

/// Strip a `\r` before every `\n` so goldens compare byte-exactly across OSes.
fn normalize_crlf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn replace_bytes(haystack: &[u8], needle: &[u8], repl: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(repl);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// The compiled binary and the oracle both embed the absolute source path in
/// `__FILE__`/backtraces; rewrite it to the `run_cwd`-relative form (else the
/// basename) so committed goldens are portable.
fn normalize_source_path(bytes: Vec<u8>, source: &Path, run_cwd: &Path) -> Vec<u8> {
    let abs = source.to_string_lossy();
    let rel = source
        .strip_prefix(run_cwd)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| source.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| abs.clone().into_owned());
    replace_bytes(&bytes, abs.as_bytes(), rel.as_bytes())
}

/// Scrub object identity, which is process-random on both sides: CRuby and
fn norm(bytes: &[u8], source: &Path, run_cwd: &Path) -> Vec<u8> {
    normalize_thread_ids(normalize_addresses(normalize_source_path(
        normalize_crlf(bytes),
        source,
        run_cwd,
    )))
}

// ---- sidecars ----

struct Sidecars {
    args: Vec<String>,
    stdin: Option<Vec<u8>>,
    expected_out: Option<PathBuf>,
    expected_err: Option<PathBuf>,
    /// A `.cext` sidecar's extension directory, resolved against the `.rb`'s
    /// own -- see [`cext_root`].
    cext: Option<PathBuf>,
}

/// The tag a per-platform golden carries. Empty everywhere but linux, so the
/// default `.expected` is macOS's answer and nothing else needs a file.
const PLATFORM: &str = if cfg!(target_os = "linux") {
    ".linux"
} else {
    ""
};

/// Resolve `<rb>.args` / `<rb>.stdin` / `<rb>.expected` / `<rb>.err.expected`
/// (the corpus sidecar convention; a `.rb` file's siblings by suffix).
///
/// A handful of goldens answer differently per PLATFORM, and no zeo bug is
/// involved: `ruby_qsort` IS the system `qsort_r`, so a tie's order is libc's,
/// and glibc's qsort does not break ties where BSD's does. Such a golden
/// carries `<rb>.linux.expected` beside its `<rb>.expected`, each recorded
/// from the same pinned ruby on its own platform. The pair is read as a UNIT:
/// when the platform file wins, its `.linux.err.expected` decides stderr too,
/// so a missing one still asserts "stderr must be empty" rather than falling
/// back to the other platform's.
fn sidecars(rb: &Path) -> std::io::Result<Sidecars> {
    let side = |suffix: &str| -> Option<PathBuf> {
        let p = PathBuf::from(format!("{}{suffix}", rb.display()));
        p.exists().then_some(p)
    };
    let (expected_out, expected_err) = match side(&format!("{PLATFORM}.expected")) {
        Some(p) if !PLATFORM.is_empty() => (Some(p), side(&format!("{PLATFORM}.err.expected"))),
        _ => (side(".expected"), side(".err.expected")),
    };
    let args = match side(".args") {
        Some(p) => std::fs::read_to_string(p)?
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        None => Vec::new(),
    };
    let stdin = match side(".stdin") {
        Some(p) => Some(std::fs::read(p)?),
        None => None,
    };
    // Read against the TESTS ROOT, not the `.rb`'s own directory: a golden
    // moves between `tests/` and `tests/gaps/` when it is promoted or filed,
    // and a name relative to the file would silently point somewhere else
    // afterwards. `tests/` is where every golden runs anyway.
    let cext = match side(".cext") {
        Some(p) => {
            let named = std::fs::read_to_string(p)?.trim().to_owned();
            let dir = tests_run_cwd().join(named);
            Some(std::fs::canonicalize(&dir).unwrap_or(dir))
        }
        None => None,
    };
    Ok(Sidecars {
        args,
        stdin,
        expected_out,
        expected_err,
        cext,
    })
}

// ---- compile + run via the zeo library (same path as e2e `run_ruby`) ----

/// `ZEO_GOLDEN_BACKEND`: which mode runs the goldens. Unset or `jit` = the
/// in-process default; `aot` is the same leg through a linked binary. Both
/// spawn the built `zeo` CLI, one child per golden.
fn golden_backend() -> String {
    match std::env::var("ZEO_GOLDEN_BACKEND") {
        Ok(v) if !v.is_empty() => v,
        _ => "jit".to_string(),
    }
}

/// Whether this leg runs goldens through a linked binary rather than the
/// in-process JIT. `tests/jit/` needs the compiler and the program to share
/// one process, so that directory is skipped here.
pub fn backend_is_aot() -> bool {
    golden_backend() != "jit"
}

/// The Cranelift legs' runner: one spawned `zeo` child per golden, which
/// compiles AND runs the program. This used to be preceded by an in-process
/// object-compile probe -- a full second compile of every golden -- whose
/// only consumer was the retired `Mode::CompileFail` classification; a
/// rejection now surfaces as the compiler's own error on the child's
/// stderr, which the comparison fails on like any other divergence.
#[allow(clippy::too_many_arguments)] // the golden's sidecars, one per parameter
fn run_via_cli(
    backend: &str,
    rb: &Path,
    source: &str,
    opts: &zeo::CompileOptions,
    args: &[String],
    stdin: Option<&[u8]>,
    run_cwd: &Path,
    extra_debug: Option<&str>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    // The AOT leg links the archive, which a test run does not build.
    if backend != "jit" {
        crate::paths::runtime_archive()?;
    }
    let mut cmd = Command::new(zeo_cli()?);
    cmd.arg("--backend").arg(backend);
    // A differential-oracle child: the same compile with one debug flag
    // added (`no-typed-calls`, `packaged-ids`). APPENDS to an ambient
    // ZEO_DEBUG so the leg composes with other debug flags.
    if let Some(flag) = extra_debug {
        let ambient = std::env::var("ZEO_DEBUG").unwrap_or_default();
        let joined = if ambient.is_empty() {
            flag.to_string()
        } else {
            format!("{ambient},{flag}")
        };
        cmd.env("ZEO_DEBUG", joined);
    }
    for root in &opts.load_roots {
        cmd.arg("-I").arg(root);
    }
    for dir in &opts.package_dirs {
        cmd.arg("--gems").arg(dir);
    }
    // Exactly what the oracle gets: the file, then the golden's own args.
    // Option parsing stops at the file name on both sides now, so a `--seed
    // 42` reaches the program without a separator -- and adding one would
    // hand the program a literal `"--"` as `ARGV[0]`, which ruby does too.
    cmd.arg(rb).args(args);
    // `RUBY_BOX=1` is a RUN-TIME flag both sides read, so the child gets
    // exactly what the oracle gets (see `run_oracle`).
    if source.contains("Ruby::Box.new") {
        cmd.env("RUBY_BOX", "1");
    }
    // A `.gc` sidecar marks a golden that needs zeo's cycle collector armed
    // to answer what ruby answers. The oracle needs no counterpart: CRuby
    // always collects. The file's content states what the program depends on.
    if std::fs::metadata(format!("{}.gc", rb.display())).is_ok() {
        cmd.env("ZEO_GC", "1");
    }
    // A `.leakcheck` sidecar runs the program under the compiled-ownership
    // ledger. Only a golden ABOUT that ledger wants it -- it aborts the
    // process on the first bad slot, which is exactly what makes such a
    // golden fail until the emitter stops producing one.
    if std::fs::metadata(format!("{}.leakcheck", rb.display())).is_ok() {
        cmd.env("ZEO_RT_LEAKCHECK", "1");
    }
    cmd.current_dir(run_cwd);
    run_bounded(&mut cmd, stdin, "zeo CLI (ZEO_GOLDEN_BACKEND)")
}

/// Compile `source` with zeo and run the produced binary in `run_cwd` with
/// `args`/`stdin`. `Ok((stdout, stderr))` when it produced a runnable binary;
/// `Err(reason)` when zeo rejected the program or the link failed (which a
/// `Pass` test treats as a mismatch and an `Xfail` test as expected divergence).
fn compile_and_run(
    rb: &Path,
    source: &str,
    args: &[String],
    stdin: Option<&[u8]>,
    run_cwd: &Path,
    env: &SuiteEnv,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    compile_and_run_debug(rb, source, args, stdin, run_cwd, env, None)
}

/// [`compile_and_run`] with one extra `ZEO_DEBUG` flag exposed -- the
/// differential-oracle legs' second child (`no-typed-calls`,
/// `packaged-ids`).
fn compile_and_run_debug(
    rb: &Path,
    source: &str,
    args: &[String],
    stdin: Option<&[u8]>,
    run_cwd: &Path,
    env: &SuiteEnv,
    extra_debug: Option<&str>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut load_roots = env.load_roots.clone();
    if let Some(fixture) = &env.cext {
        load_roots.push(cext_root(fixture)?);
    }
    let opts = zeo::CompileOptions {
        input_path: Some(rb.to_path_buf()),
        package_dirs: env.package_dirs.clone(),
        load_roots,
        ..Default::default()
    };
    run_via_cli(
        &golden_backend(),
        rb,
        source,
        &opts,
        args,
        stdin,
        run_cwd,
        extra_debug,
    )
}

// ---- C extensions, built for the zeo side ----

/// Build a `.cext` fixture against zeo's headers and answer the directory to
/// put on zeo's `require` path.
///
/// A golden that requires a C extension compares two BUILDS, not one library.
/// An extension is compiled against a set of headers and linked against a
/// runtime, so each engine needs its own: the oracle cannot load a bundle
/// linked against zeo's runtime, and zeo cannot load one built for MRI's ABI.
/// Pointing both at one build would prove nothing, and that is why the twelve
/// `crates/zeo-rt/cext/README.md` divergences had no instrument until now.
///
/// This side is here; the ORACLE side is built by `cargo xtask bless`, which
/// is the only thing that ever runs ruby (recording moved out of the test
/// binary for speed -- see that module's docs). The split is why
/// [`run_golden_env`] refuses a `.cext` golden with no committed `.expected`:
/// the live-oracle path would otherwise compare zeo against a ruby raising
/// `LoadError`, which is exactly the wrong answer to record.
///
/// Built ONCE PER PROCESS, into a scratch tree cleared first, so a stale
/// product from an earlier run can never be the thing under test.
fn cext_root(fixture: &Path) -> Result<PathBuf, String> {
    type Memo = std::collections::HashMap<PathBuf, Result<PathBuf, String>>;
    static BUILDS: std::sync::Mutex<Option<Memo>> = std::sync::Mutex::new(None);

    let mut guard = BUILDS
        .lock()
        .expect("the cext build memo is never poisoned");
    let memo = guard.get_or_insert_with(Memo::default);
    if let Some(hit) = memo.get(fixture) {
        return hit.clone();
    }
    let built = build_cext_for_zeo(fixture);
    memo.insert(fixture.to_path_buf(), built.clone());
    built
}

fn build_cext_for_zeo(fixture: &Path) -> Result<PathBuf, String> {
    let name = fixture
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{}: not a directory name", fixture.display()))?;
    // The same convention `cargo xtask bless` uses for the oracle side, one
    // directory over. Cleared first: a product left by an earlier run must
    // never be the thing under test.
    let dir = workspace_root()
        .join("target/cext-goldens")
        .join(&name)
        .join("zeo");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    // An extension directory is an `extconf.rb` beside its `.c`, flat, which
    // is the shape mkmf expects to be run in.
    let entries =
        std::fs::read_dir(fixture).map_err(|e| format!("reading {}: {e}", fixture.display()))?;
    for entry in entries.flatten() {
        let from = entry.path();
        if from.is_file() {
            std::fs::copy(&from, dir.join(entry.file_name()))
                .map_err(|e| format!("copying {}: {e}", from.display()))?;
        }
    }

    // `zeo::cext::configure` re-enters the zeo binary as a subprocess and
    // exports the header directories, which is the whole path an installed
    // zeo takes. `build_extension` then drives the compile and the link
    // itself; `make` is not on this path.
    let zeo = zeo_cli().map_err(|e| e.to_string())?;
    zeo::cext::configure(&zeo, &dir, Path::new("extconf.rb"), &[])
        .map_err(|e| format!("{}: zeo extconf failed: {e}", fixture.display()))?;
    zeo::cext::build_extension(&dir, 4)
        .map_err(|e| format!("{}: zeo build failed: {e}", fixture.display()))?;
    Ok(dir)
}

// ---- ruby oracle (bless) ----

/// Resolve the oracle against `Gemfile.lock` -- the same set the compiler
/// vendors, so neither side can answer a `require` with a version the other
/// does not have. `-rbundler/setup` is what `bundle exec` does, one process
/// cheaper.
///
/// The removals matter as much as the additions: whatever anybody has `gem
/// install`ed, or points RUBYLIB at, must not decide what a golden records.
///
/// Needs `make deps` to have run.
fn point_oracle_at_the_bundle(cmd: &mut Command) {
    cmd.env("BUNDLE_GEMFILE", workspace_root().join("Gemfile"))
        .env("RUBYOPT", "-rbundler/setup")
        .env_remove("RUBYLIB")
        .env_remove("GEM_HOME")
        .env_remove("GEM_PATH")
        .env_remove("GEM_SPEC_CACHE");
}

/// `vendor/bundle`'s rspec trees, for the zeo side of the milestone that runs
/// a real suite. The oracle reaches them through bundler; zeo needs `-I` on
/// each `lib/`. Scoped to rspec rather than the whole bundle, which would put
/// upstream copies of gems zeo vendors ahead of its own.
pub fn bundle_rspec_libs() -> Vec<PathBuf> {
    let gems = workspace_root().join("vendor").join("bundle").join("ruby");
    let mut libs: Vec<PathBuf> = std::fs::read_dir(&gems)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|abi| {
            std::fs::read_dir(abi.path().join("gems"))
                .into_iter()
                .flatten()
        })
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("rspec") || n.starts_with("diff-lcs")
            })
        })
        .map(|p| p.join("lib"))
        .filter(|p| p.is_dir())
        .collect();
    libs.sort();
    libs
}

/// Run the ruby oracle for `rb` and return its `(stdout, stderr)`.
fn run_oracle(
    rb: &Path,
    source: &str,
    args: &[String],
    stdin: Option<&[u8]>,
    run_cwd: &Path,
    env: &SuiteEnv,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let ruby = resolve_ruby(run_cwd);
    let mut cmd = Command::new(&ruby);
    cmd.arg("--disable-error_highlight")
        .arg("--disable-did_you_mean");
    point_oracle_at_the_bundle(&mut cmd);
    for inc in &env.oracle_includes {
        cmd.arg("-I").arg(inc);
    }
    // `Ruby::Box` examples need the experimental namespace flag, and one
    // that ALLOCATES a box needs the gate. Naming the class is not enough:
    // CRuby's disabled-mode surface is a smaller one, and
    // `ruby_box_surface.rb` is what pins it.
    if source.contains("Ruby::Box") {
        cmd.arg("-W:no-experimental");
        if source.contains("Ruby::Box.new") {
            cmd.env("RUBY_BOX", "1");
        }
    }
    cmd.arg(rb).args(args).current_dir(run_cwd);
    run_bounded(&mut cmd, stdin, "ruby oracle")
}

/// Under `cargo xtask bless`: (re)write `<rb>.expected` (+ `.err.expected`)
/// from the oracle -- or, for a `.divergence` golden, from zeo.
/// A stdout-only suite (`check_stderr == false`) never keeps a stderr golden:
/// ruby's parse warnings, experimental notices and thread exception reports
/// aren't part of the contract there. Every suite passes `true` today, so the
/// arm exists for a caller that does not want stderr rather than for one that
/// exists.
fn bless(
    rb: &Path,
    source: &str,
    sc: &Sidecars,
    mode: Mode,
    run_cwd: &Path,
    env: &SuiteEnv,
) -> datatest_stable::Result<()> {
    // A `tests/divergences/` golden records ZEO's output on purpose.
    // Recording the oracle's here would replace the golden with the very
    // answer the program exists to differ from, and the test would then fail
    // for a reason nobody could read.
    let (stdout, stderr) = match mode {
        Mode::Divergence => {
            compile_and_run(rb, source, &sc.args, sc.stdin.as_deref(), run_cwd, env).map_err(
                |e| {
                    format!(
                        "{}: a decided divergence records zeo's own output, and zeo failed: {e}",
                        rb.display()
                    )
                },
            )?
        }
        Mode::Pass | Mode::Xfail => {
            run_oracle(rb, source, &sc.args, sc.stdin.as_deref(), run_cwd, env)?
        }
    };
    let out = norm(&stdout, rb, run_cwd);
    std::fs::write(format!("{}.expected", rb.display()), &out)?;
    let err_path = format!("{}.err.expected", rb.display());
    let err = norm(&stderr, rb, run_cwd);
    if !err.is_empty() {
        std::fs::write(&err_path, &err)?;
    } else {
        let _ = std::fs::remove_file(&err_path); // absent => "stderr must be empty"
    }
    Ok(())
}

// ---- the entry point ----

/// Per-suite compile/oracle environment beyond the shared defaults: a suite
/// that needs its own package dirs or `-I` roots sets them here, so the zeo
/// build and the CRuby oracle resolve the same `require`.
#[derive(Default)]
pub struct SuiteEnv {
    pub package_dirs: Vec<PathBuf>,
    /// Extra `-I` roots for the zeo compile (`CompileOptions::load_roots`).
    pub load_roots: Vec<PathBuf>,
    pub oracle_includes: Vec<PathBuf>,
    /// A C-extension fixture directory this case needs on both sides, set
    /// from the `.cext` sidecar rather than by a suite. Each side builds it
    /// against its own headers -- see [`cext_root`].
    pub cext: Option<PathBuf>,
}

fn suite_env_default() -> &'static SuiteEnv {
    static ENV: std::sync::OnceLock<SuiteEnv> = std::sync::OnceLock::new();
    ENV.get_or_init(SuiteEnv::default)
}

/// Run one golden case. See the module docs for the per-`Mode` contract.
/// Every suite asserts full stdout+stderr fidelity.
pub fn run_golden(rb: &Path, mode: Mode, run_cwd: &Path) -> datatest_stable::Result<()> {
    run_golden_env(rb, mode, run_cwd, suite_env_default())
}

/// [`run_golden`] with a per-suite [`SuiteEnv`].
pub fn run_golden_env(
    rb: &Path,
    mode: Mode,
    run_cwd: &Path,
    env: &SuiteEnv,
) -> datatest_stable::Result<()> {
    // datatest-stable hands us a path relative to the crate manifest dir (the
    // test process's cwd); absolutize it so ruby/the binary find it after we
    // `current_dir(run_cwd)`, and so source-path normalization matches.
    let rb = &std::fs::canonicalize(rb).unwrap_or_else(|_| rb.to_path_buf());

    let source = std::fs::read_to_string(rb)?;
    let sc = sidecars(rb)?;

    // A `.cext` sidecar is per CASE, not per suite, so it overlays the
    // suite's environment rather than living in it.
    let overlay;
    let env = match &sc.cext {
        None => env,
        // The oracle side of an extension is built by `cargo xtask bless`
        // alone, so a `.cext` golden's answer has to be RECORDED. Without a
        // committed one the live-oracle path would run a ruby that cannot
        // load the extension and compare zeo against its `LoadError` -- a
        // green-looking gap that proves nothing.
        Some(_) if sc.expected_out.is_none() => {
            return Err(format!(
                "{}: a `.cext` golden must carry a committed `.expected` -- run \
                 `cargo xtask bless {}`. The live oracle has no build of the \
                 extension and would answer LoadError.",
                rb.display(),
                rb.file_stem().unwrap_or_default().to_string_lossy()
            )
            .into());
        }
        Some(fixture) => {
            overlay = SuiteEnv {
                package_dirs: env.package_dirs.clone(),
                load_roots: env.load_roots.clone(),
                oracle_includes: env.oracle_includes.clone(),
                cext: Some(fixture.clone()),
            };
            &overlay
        }
    };

    if std::env::var_os("ZEO_BLESS_FROM_TOOL").is_some() {
        return bless(rb, &source, &sc, mode, run_cwd, env);
    }

    // Pass / Xfail: build + run, then diff against the golden.
    let actual = compile_and_run(rb, &source, &sc.args, sc.stdin.as_deref(), run_cwd, env);

    // The zeo-vs-zeo differential oracles. The same program compiles and
    // runs a SECOND time with one debug flag added, and the two zeo
    // outputs must agree byte-for-byte after normalization. Ruby is not
    // consulted: each leg catches its emission mode changing ANY
    // observable behavior, including behavior the CRuby oracle could not
    // distinguish. Runs for Pass and Xfail alike -- a gap's divergence
    // from ruby must still be the SAME divergence in the other mode. A
    // program zeo cannot build at all is skipped here (that failure is
    // already the case's own divergence).
    //
    //   ZEO_GOLDEN_DIFF_TYPED=1   every TyKind-driven emission off
    //                             (a wrong static type is a miscompile)
    //   ZEO_GOLDEN_DIFF_PKGIDS=1  the Packaged id mode forced program-wide
    //                             over an identity table -- decision 11's
    //                             leg at M2 scope: it proves the packaged
    //                             CODEGEN (id-table loads, the variable
    //                             patched-bit guard) over the full corpus
    //                             until M6 compiles gems as packages
    for (gate, flag, label) in [
        ("ZEO_GOLDEN_DIFF_TYPED", "no-typed-calls", "TYPED-EMISSION"),
        ("ZEO_GOLDEN_DIFF_PKGIDS", "packaged-ids", "PACKAGED-ID"),
    ] {
        if !std::env::var_os(gate).is_some_and(|v| v == "1") {
            continue;
        }
        let Ok((on_out, on_err)) = &actual else {
            continue;
        };
        let (off_out, off_err) = compile_and_run_debug(
            rb,
            &source,
            &sc.args,
            sc.stdin.as_deref(),
            run_cwd,
            env,
            Some(flag),
        )
        .map_err(|e| {
            format!(
                "{}: the {flag} compile/run failed where the plain one ran: {e}",
                rb.display()
            )
        })?;
        if norm(on_out, rb, run_cwd) != norm(&off_out, rb, run_cwd)
            || norm(on_err, rb, run_cwd) != norm(&off_err, rb, run_cwd)
        {
            return Err(format!(
                "{}: {label} DIVERGENCE -- the same program answers \
                 differently with `{flag}` on vs off (a miscompile in one \
                 mode).
--- plain stdout ---
{}
--- {flag} stdout ---
{}
--- plain stderr ---
{}
--- {flag} stderr ---
{}",
                rb.display(),
                String::from_utf8_lossy(&norm(on_out, rb, run_cwd)),
                String::from_utf8_lossy(&norm(&off_out, rb, run_cwd)),
                String::from_utf8_lossy(&norm(on_err, rb, run_cwd)),
                String::from_utf8_lossy(&norm(&off_err, rb, run_cwd)),
            )
            .into());
        }
    }

    // Under the `ZEO_RT_GCCHECK` leg the runtime writes one census line to
    // stderr at exit. Take it out of the ordinary comparison and gate it on
    // its own sidecar -- see `check_gccheck_census`.
    let (actual, census) = match (std::env::var_os("ZEO_RT_GCCHECK"), actual) {
        (Some(_), Ok((out, err))) => {
            let (err, census) = split_gccheck(&err);
            (Ok((out, err)), Some(census))
        }
        (_, other) => (other, None),
    };
    if let Some(census) = census {
        check_gccheck_census(rb, &census)?;
    }

    // The reference: committed `.expected` (+ optional `.err.expected`), else a
    // live ruby-oracle run (a test without a committed stdout snapshot). On CI
    // the live run is refused: whatever ruby a runner has must not arbitrate
    // a golden nobody blessed. `ZEO_GOLDEN_REQUIRE_EXPECTED=1` asks the same
    // question locally.
    let (expected_out, expected_err) = match &sc.expected_out {
        Some(p) => {
            let out = std::fs::read(p)?;
            let err = match &sc.expected_err {
                Some(e) => std::fs::read(e)?,
                None => Vec::new(), // no .err.expected => stderr must be empty
            };
            (out, err)
        }
        None if std::env::var_os("CI").is_some()
            || std::env::var_os("ZEO_GOLDEN_REQUIRE_EXPECTED").is_some() =>
        {
            return Err(format!(
                "{}: no committed .expected, and this run refuses the live oracle. \
                 Bless it: `cargo xtask bless {}`",
                rb.display(),
                rb.file_stem().unwrap_or(rb.as_os_str()).to_string_lossy()
            )
            .into());
        }
        None => run_oracle(rb, &source, &sc.args, sc.stdin.as_deref(), run_cwd, env)?,
    };

    let matched = match &actual {
        Ok((out, err)) => {
            norm(out, rb, run_cwd) == norm(&expected_out, rb, run_cwd)
                && norm(err, rb, run_cwd) == norm(&expected_err, rb, run_cwd)
        }
        Err(_) => false, // zeo couldn't produce/run a binary: it diverges.
    };

    match mode {
        Mode::Pass | Mode::Divergence if matched => Ok(()),
        Mode::Pass | Mode::Divergence => {
            Err(mismatch_message(rb, &actual, &expected_out, &expected_err, run_cwd).into())
        }
        Mode::Xfail if matched => Err(format!(
            "GAP FIXED -- {stem} now matches ruby. Promote it: \
             `cargo xtask promote-gap {stem}` (moves it + its sidecars into tests/, \
             the zeo-authored suite -- NOT tests/spinel/, which mirrors the vendored \
             spinel corpus).",
            stem = rb.file_stem().unwrap_or(rb.as_os_str()).to_string_lossy()
        )
        .into()),
        Mode::Xfail => Ok(()), // still diverges: expected.
    }
}

// A compiler PANIC needs no containment here any more: the compile happens
// in the spawned child, whose death is an output divergence like any other
// -- which is what lets `tests/gaps/` hold a panicking program as an XFAIL.

fn mismatch_message(
    rb: &Path,
    actual: &Result<(Vec<u8>, Vec<u8>), String>,
    expected_out: &[u8],
    expected_err: &[u8],
    run_cwd: &Path,
) -> String {
    // Truncate BEFORE `norm`: it makes three full copies of its input, and its
    // path-replacement pass scans for the needle at every byte offset, so
    // handing it a multi-megabyte buffer costs more than the message is worth.
    const SHOW_LIMIT: usize = 8 << 10;
    let show = |b: &[u8]| {
        let clipped = &b[..b.len().min(SHOW_LIMIT)];
        let mut s = String::from_utf8_lossy(&norm(clipped, rb, run_cwd)).into_owned();
        if b.len() > SHOW_LIMIT {
            s.push_str(&format!("\n[... {} more bytes]", b.len() - SHOW_LIMIT));
        }
        s
    };
    match actual {
        Ok((out, err)) => format!(
            "{}: output differs from ruby.\n--- expected stdout ---\n{}\n--- actual stdout ---\n{}\n\
             --- expected stderr ---\n{}\n--- actual stderr ---\n{}",
            rb.display(),
            show(expected_out),
            show(out),
            show(expected_err),
            show(err),
        ),
        // `Err` here is a harness-level failure (spawn, or a tripped
        // capture/deadline/RSS bound) -- a compile error or panic reaches
        // the Ok arm as the child's own stderr.
        Err(e) => format!("{}: zeo failed to compile/run it: {e}", rb.display()),
    }
}

/// The `cycle leak:` line `ZEO_RT_GCCHECK=1` writes at exit, split out of
/// stderr so it does not break every other comparison.
fn split_gccheck(err: &[u8]) -> (Vec<u8>, String) {
    let text = String::from_utf8_lossy(err).into_owned();
    let mut census = String::new();
    let mut kept = String::new();
    for line in text.split_inclusive('\n') {
        if line.starts_with("cycle leak: ") {
            census = line.trim_end().to_string();
        } else {
            kept.push_str(line);
        }
    }
    (kept.into_bytes(), census)
}

/// Gate one program's exit census against its `.gccheck` sidecar.
///
/// **A cycle alive at exit is not a defect.** A program that builds a ring and
/// never breaks it -- `a << a`, a lambda that calls itself through a captured
/// local, a grid of cells that link their neighbours -- is SUPPOSED to have
/// one, and every residue the corpus reports today is of that kind. So the leg
/// cannot gate on zero.
///
/// What it gates is CHANGE. The census is deterministic, so a new shape or a
/// new count means the program's object graph moved or the collector stopped
/// seeing part of it, and a sidecar records what each program is known to
/// build. No sidecar means no cycle.
fn check_gccheck_census(rb: &Path, census: &str) -> datatest_stable::Result<()> {
    let path = format!("{}.gccheck", rb.display());
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    let want = want
        .lines()
        .find(|l| l.starts_with("cycle leak: "))
        .unwrap_or("")
        .trim_end();
    if census == want {
        return Ok(());
    }
    Err(format!(
        "{}: the exit cycle census changed.\n  expected: {}\n  actual:   {}\n\
         Record it in {} (with a line saying WHICH cycle the program builds), \
         or find what stopped the collector seeing it.",
        rb.display(),
        if want.is_empty() { "<no cycle>" } else { want },
        if census.is_empty() {
            "<no cycle>"
        } else {
            census
        },
        path,
    )
    .into())
}
