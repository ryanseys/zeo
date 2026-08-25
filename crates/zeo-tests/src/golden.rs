//! Shared golden-file test helper for the datatest-stable suites
//! (`tests/gaps.rs`, `tests/examples.rs`, `tests/spinel.rs`,
//! `tests/gemtests.rs`).
//!
//! One function, [`run_golden`], drives every `.rb`: probe the emitter in
//! process (so a rejection stays distinguishable), spawn the built `zeo`
//! CLI on the chosen backend, then diff stdout/stderr against the committed
//! **ruby-oracle** golden `.expected` (+ `.err.expected`/`.args`/`.stdin`
//! sidecars).
//!
//! - `Mode::Pass` (corpus, examples): zeo must MATCH the golden.
//! - `Mode::Xfail` (gaps): zeo must DIVERGE from the golden -- a match means the
//!   gap is fixed and the test FAILS with a "promote" message.
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
//! `tools/zeo-dev bless <filter>` re-records the goldens from the real `ruby` oracle
//! (`--disable-error_highlight --disable-did_you_mean`, resolved via `mise`)
//! instead of asserting. This is the single golden writer.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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
const RUN_DEADLINE: Duration = Duration::from_secs(60);

/// The third bound, and the one the other two miss: a child that ALLOCATES
/// without writing. `(1..).to_a` -- an endless range zeo evaluates eagerly --
/// prints nothing, so `MAX_CAPTURE` never trips, and it spends the whole
/// deadline growing the heap.
///
/// Measured, on a 16 GiB machine: four such children at `--test-threads 4`
/// took free memory from 6.9 GiB to 0.06 GiB in five and a half seconds, and
/// the corpus has six of them. Against that, 741 ordinary golden children peak
/// at 13 MiB. So the cap can sit far below anything legitimate: 512 MiB is 40x
/// the observed normal peak and a fraction of what a runaway wants.
const MAX_CHILD_RSS: u64 = 512 << 20; // 512 MiB

/// [`MAX_CHILD_RSS`], with an env override (`ZEO_GOLDEN_MAX_RSS`, in MiB)
/// for the cases that legitimately need more, the same way
/// `ZEO_GOLDEN_RUN_DEADLINE` stretches the clock for them. A case that
/// compiles a whole gem's require graph in the child (a gemtests case) is
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
/// gem's whole test file is one case in the `gemtests` suite. The deadline
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
}

/// The repo root (this crate's `../..`), for deriving per-suite run
/// directories.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the workspace root")
        .to_path_buf()
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
/// zeo both render it as `0x` followed by exactly 16 lowercase hex digits
/// (`#<Thread:0x0000000102cf6310 ...>`, `#<Object:0x...>`), so a golden can
/// assert the shape AROUND an address it could never match.
///
/// Deliberately exactly 16: `%x`-formatted output in the corpus (`0xff`,
/// `0x1.ffp+7`) is far shorter and stays untouched. Programs that would
/// rather scrub Ruby-side (`e.message.sub(/0x[0-9a-f]+/, "0xADDR")`, the
/// existing convention) keep working -- their output has no address left in
/// it by the time it gets here.
fn normalize_addresses(bytes: Vec<u8>) -> Vec<u8> {
    const WIDTH: usize = 16;
    let is_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let addr = bytes[i..].starts_with(b"0x")
            && bytes.len() >= i + 2 + WIDTH
            && bytes[i + 2..i + 2 + WIDTH].iter().all(|&b| is_hex(b))
            && !bytes.get(i + 2 + WIDTH).is_some_and(|&b| is_hex(b));
        if addr {
            out.extend_from_slice(b"0xADDR");
            i += 2 + WIDTH;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn norm(bytes: &[u8], source: &Path, run_cwd: &Path) -> Vec<u8> {
    normalize_addresses(normalize_source_path(
        normalize_crlf(bytes),
        source,
        run_cwd,
    ))
}

// ---- sidecars ----

struct Sidecars {
    args: Vec<String>,
    stdin: Option<Vec<u8>>,
    expected_out: Option<PathBuf>,
    expected_err: Option<PathBuf>,
    /// A `.divergence` sidecar marks a golden whose `.expected` records
    /// **zeo's own** output rather than the oracle's, because zeo has DECIDED
    /// to answer differently -- reproducing ruby here would make zeo's
    /// behaviour worse (an unstable sort, a `move:` that destroys the source
    /// before it refuses) or cost more than the divergence does.
    ///
    /// The file states the reason and carries the oracle's output verbatim, so
    /// the divergence stays executable evidence rather than prose. `bless`
    /// reads it and records zeo instead of ruby, which is what keeps these
    /// goldens machine-recorded like every other.
    divergence: Option<PathBuf>,
}

/// Resolve `<rb>.args` / `<rb>.stdin` / `<rb>.expected` / `<rb>.err.expected`
/// (the corpus sidecar convention; a `.rb` file's siblings by suffix).
fn sidecars(rb: &Path) -> std::io::Result<Sidecars> {
    let side = |suffix: &str| -> Option<PathBuf> {
        let p = PathBuf::from(format!("{}{suffix}", rb.display()));
        p.exists().then_some(p)
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
    Ok(Sidecars {
        args,
        stdin,
        expected_out: side(".expected"),
        expected_err: side(".err.expected"),
        divergence: side(".divergence"),
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

/// The built `zeo` CLI beside this test binary's profile dir.
///
/// Checked against the compiler's own sources, because nothing rebuilds it
/// for us: a test target has no cargo dependency edge to a BINARY target,
/// so a stale `zeo` would run yesterday's compiler over today's goldens and
/// report green. Stat-only, and it never shells cargo -- it says what to run.
/// Shared with the e2e harness's JIT-child runner.
pub(crate) fn zeo_cli() -> Result<PathBuf, String> {
    let mut p = std::env::current_exe().map_err(|e| format!("test binary path: {e}"))?;
    p.pop(); // deps/<test-bin> -> deps
    p.pop(); // deps -> target/<profile>
    p.push("zeo");
    if !p.is_file() {
        return Err(format!(
            "the golden harness needs the zeo CLI at {} (run `cargo build -p zeo` first)",
            p.display()
        ));
    }
    if let Some(source) = newer_compiler_source(&p) {
        return Err(format!(
            "the zeo CLI at {} is older than {} (run `cargo build -p zeo` first)",
            p.display(),
            source.display()
        ));
    }
    Ok(p)
}

/// The compiler crates whose sources build the `zeo` binary and `libzeo.a`.
const COMPILER_CRATES: &[&str] = &["zeo", "zeo-rt", "zeo-abi", "zeo-macros", "zeo-dsl"];

/// A compiler source newer than `binary`, if there is one. Walks exactly the
/// inputs cargo would rebuild for -- the crates' `src`/`build.rs`/manifest
/// plus the workspace manifest and lockfile -- and never the test corpus,
/// which is not an input to the compiler.
fn newer_compiler_source(binary: &Path) -> Option<PathBuf> {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let built = mtime(binary)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    let mut roots: Vec<PathBuf> = vec![root.join("Cargo.toml"), root.join("Cargo.lock")];
    for name in COMPILER_CRATES {
        let dir = root.join("crates").join(name);
        roots.push(dir.join("src"));
        roots.push(dir.join("build.rs"));
        roots.push(dir.join("Cargo.toml"));
    }
    let mut stack = roots;
    while let Some(path) = stack.pop() {
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                stack.extend(entries.flatten().map(|e| e.path()));
            }
        } else if meta.modified().is_ok_and(|m| m > built) {
            return Some(path);
        }
    }
    None
}

/// The Cranelift legs' runner: one spawned `zeo` child per golden, which
/// compiles AND runs the program. This used to be preceded by an in-process
/// object-compile probe -- a full second compile of every golden -- whose
/// only consumer was the retired `Mode::CompileFail` classification; a
/// rejection now surfaces as the compiler's own error on the child's
/// stderr, which the comparison fails on like any other divergence.
fn run_via_cli(
    backend: &str,
    rb: &Path,
    source: &str,
    opts: &zeo::CompileOptions,
    args: &[String],
    stdin: Option<&[u8]>,
    run_cwd: &Path,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut cmd = Command::new(zeo_cli()?);
    cmd.arg("--backend").arg(backend);
    for root in &opts.load_roots {
        cmd.arg("-I").arg(root);
    }
    for dir in &opts.package_dirs {
        cmd.arg("--gems").arg(dir);
    }
    // `--` first: a golden's own args are the PROGRAM's (`--seed 42` for a
    // minitest driver), and without the separator the CLI reads them as its
    // own options.
    cmd.arg(rb);
    if !args.is_empty() {
        cmd.arg("--").args(args);
    }
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
    let opts = zeo::CompileOptions {
        input_path: Some(rb.to_path_buf()),
        package_dirs: env.package_dirs.clone(),
        load_roots: env.load_roots.clone(),
        ..Default::default()
    };
    run_via_cli(&golden_backend(), rb, source, &opts, args, stdin, run_cwd)
}

// ---- ruby oracle (bless) ----

/// `mise which ruby` (falls back to bare `ruby`), the same resolution the old
/// oracle used so goldens come from the `mise.toml`-pinned ruby.
fn resolve_ruby(cwd: &Path) -> PathBuf {
    let out = Command::new("mise")
        .arg("which")
        .arg("ruby")
        .current_dir(cwd)
        .output();
    if let Ok(out) = out
        && out.status.success()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    PathBuf::from("ruby")
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

/// Under `tools/zeo-dev bless`: (re)write `<rb>.expected` (+ `.err.expected`)
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
    run_cwd: &Path,
    env: &SuiteEnv,
) -> datatest_stable::Result<()> {
    // A `.divergence` golden records ZEO's output on purpose -- see
    // `Sidecars::divergence`. Recording the oracle's here would replace the
    // golden with the very answer the file exists to differ from, and the
    // test would then fail for a reason nobody could read.
    let (stdout, stderr) = match &sc.divergence {
        Some(_) => compile_and_run(rb, source, &sc.args, sc.stdin.as_deref(), run_cwd, env)
            .map_err(|e| {
                format!(
                    "{}: this golden records zeo's own output ({}), and zeo failed: {e}",
                    rb.display(),
                    sc.divergence.as_ref().expect("just matched").display()
                )
            })?,
        None => run_oracle(rb, source, &sc.args, sc.stdin.as_deref(), run_cwd, env)?,
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

/// Per-suite compile/oracle environment beyond the shared defaults. The
/// `gemtests` suite points `package_dirs` at `vendor/gemtests/` (each fetched
/// gem is a package there) and `oracle_includes` at each gem's `lib/`, so the
/// zeo build and the CRuby oracle resolve the same `require "rack"`.
#[derive(Default)]
pub struct SuiteEnv {
    pub package_dirs: Vec<PathBuf>,
    /// Extra `-I` roots for the zeo compile (`CompileOptions::load_roots`).
    pub load_roots: Vec<PathBuf>,
    pub oracle_includes: Vec<PathBuf>,
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

    // A `.macos-only` sidecar marks a golden whose source or expected output
    // is inherently macOS-specific (a hardcoded ioctl number, the errno
    // constant surface, per-platform dlopen flag values) -- Linux CRuby would
    // diverge from the committed macOS-oracle `.expected` exactly as zeo
    // does. The file's content states the reason.
    if cfg!(not(target_os = "macos"))
        && std::fs::metadata(format!("{}.macos-only", rb.display())).is_ok()
    {
        eprintln!("golden skipped (.macos-only): {}", rb.display());
        return Ok(());
    }

    // A `.jit-only` sidecar marks a golden that depends on the COMPILER and
    // the program sharing one process, which only the JIT does. A linked
    // binary carries an embedded compiler in a process of its own, so any
    // compile-time state the whole-program compile published is absent from
    // it. The file's content states which state and what the fix would be.
    if std::env::var("ZEO_GOLDEN_BACKEND").is_ok_and(|b| b != "jit")
        && std::fs::metadata(format!("{}.jit-only", rb.display())).is_ok()
    {
        eprintln!("golden skipped (.jit-only): {}", rb.display());
        return Ok(());
    }

    let source = std::fs::read_to_string(rb)?;
    let sc = sidecars(rb)?;

    if std::env::var_os("ZEO_BLESS_FROM_TOOL").is_some() {
        return bless(rb, &source, &sc, run_cwd, env);
    }

    // Pass / Xfail: build + run, then diff against the golden.
    let actual = compile_and_run(rb, &source, &sc.args, sc.stdin.as_deref(), run_cwd, env);

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
    // live ruby-oracle run (a test without a committed stdout snapshot).
    let (expected_out, expected_err) = match &sc.expected_out {
        Some(p) => {
            let out = std::fs::read(p)?;
            let err = match &sc.expected_err {
                Some(e) => std::fs::read(e)?,
                None => Vec::new(), // no .err.expected => stderr must be empty
            };
            (out, err)
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
        Mode::Pass if matched => Ok(()),
        Mode::Pass => {
            Err(mismatch_message(rb, &actual, &expected_out, &expected_err, run_cwd).into())
        }
        Mode::Xfail if matched => Err(format!(
            "GAP FIXED -- {stem} now matches ruby. Promote it: \
             `tools/zeo-dev promote-gap {stem}` (moves it + its sidecars into tests/, \
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

#[cfg(test)]
mod tests {
    use super::normalize_addresses;

    fn scrub(s: &str) -> String {
        String::from_utf8(normalize_addresses(s.as_bytes().to_vec())).unwrap()
    }

    #[test]
    fn only_full_width_object_addresses_are_scrubbed() {
        assert_eq!(
            scrub("#<Thread:0x0000000102cf6310 t.rb:4 run>"),
            "#<Thread:0xADDR t.rb:4 run>"
        );
        assert_eq!(scrub("a 0xdeadbeefcafef00d b"), "a 0xADDR b");
        // `%x`/`%a` formatting is shorter, and a longer run isn't an address.
        assert_eq!(scrub("0xff / 010"), "0xff / 010");
        assert_eq!(scrub("\"0x1.ffp+7\""), "\"0x1.ffp+7\"");
        assert_eq!(scrub("0x00000001234567890"), "0x00000001234567890");
        // Uppercase hex is `%X` output, never an address rendering.
        assert_eq!(scrub("0xDEADBEEFCAFEF00D"), "0xDEADBEEFCAFEF00D");
    }
}
