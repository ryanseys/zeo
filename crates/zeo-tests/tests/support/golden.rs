//! Shared golden-file test helper for the datatest-stable suites
//! (`tests/gaps.rs`, `tests/examples.rs`, `tests/spinel.rs`).
//!
//! One function, [`run_golden`], drives every `.rb` the same way the e2e
//! `run_ruby` helper does -- `zeo::compile_to_rust_with` ->
//! `zeo::backend::build_binary` (content-addressed cache) -> spawn -- then diffs
//! stdout/stderr against the committed **ruby-oracle** golden `.expected`
//! (+ `.err.expected`/`.args`/`.stdin` sidecars).
//!
//! - `Mode::Pass` (corpus, examples): zeo must MATCH the golden.
//! - `Mode::CompileFail` (`analyze_fail/`): zeo must REJECT the program.
//! - `Mode::Xfail` (gaps): zeo must DIVERGE from the golden -- a match means the
//!   gap is fixed and the test FAILS with a "promote" message.
//!
//! `cargo xtask bless <filter>` re-records the goldens from the real `ruby` oracle
//! (`--disable-error_highlight --disable-did_you_mean`, resolved via `mise`)
//! instead of asserting. This is the single golden writer.

#![allow(dead_code)] // each test target includes its own copy; not all use every item.

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

/// `RLIMIT_AS` is the cheap half of the bound -- the child dies on its own
/// allocation failure, with no polling. It is NOT the enforcing half: **Darwin
/// accepts `setrlimit(RLIMIT_AS)` and does not enforce it**, which is how a
/// child measured 2.29 GiB under a 2 GiB limit. [`child_rss`] and the watchdog
/// in [`run_bounded`] are what actually hold the line; this stays because it
/// does work on Linux (CI), where it kills a runaway sooner and cheaper.
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
                rlim_cur: MAX_CHILD_RSS,
                rlim_max: MAX_CHILD_RSS,
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
                && rss > MAX_CHILD_RSS
            {
                limit = Some(format!("allocated more than {} MiB", MAX_CHILD_RSS >> 20));
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
    /// zeo must reject the program at compile time (`analyze_fail/`).
    CompileFail,
}

/// The repo root (`crates/zeo/../..`), for deriving per-suite run directories.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo has a workspace root")
        .to_path_buf()
}

/// Working directory every golden suite's programs run in (and the base their
/// source paths are relativized against, so backtraces read `spinel/x.rb`,
/// `gaps/x.rb`, or `x.rb`). The `.args` fixture paths (e.g. the ARGF input) are
/// relative to this too. All three suites live under `tests/`, so they share it.
pub fn tests_run_cwd() -> PathBuf {
    workspace_root().join("tests")
}

// ---- normalization (ported verbatim from xtask/src/conformance/{util,runner}.rs) ----

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
    })
}

// ---- compile + run via the zeo library (same path as e2e `run_ruby`) ----

/// `-O0`. A golden asserts what a program PRINTS, never how fast it runs, and
/// `-C opt-level=2` is pure cost here -- ruinous once a vendored-gem golden
/// splices a whole require graph into one crate (`gems/net_http.rb` generates
/// 45MB of Rust, which rustc will not optimize inside ten minutes).
/// `ZEO_RUNTIME_PROFILE=release` forces the optimized build back.
fn profile() -> zeo::backend::Profile {
    zeo::backend::Profile::from_env_or(zeo::backend::Profile::Debug)
}

/// `ZEO_GOLDEN_BACKEND`: which backend runs the goldens. Unset or `rustc`
/// = today's in-process compile + `build_binary` path. `jit`/`aot` = the
/// Cranelift legs: spawn the built `zeo` CLI, one child per golden (the
/// isolation the plan requires). The default flips with the M1 backend
/// flip, not before -- until CLIF parity, the ratchet legs run on demand.
fn golden_backend() -> Option<String> {
    std::env::var("ZEO_GOLDEN_BACKEND")
        .ok()
        .filter(|v| !v.is_empty() && v != "rustc")
}

/// The built `zeo` CLI beside this test binary's profile dir.
fn zeo_cli() -> Result<PathBuf, String> {
    let mut p = std::env::current_exe().map_err(|e| format!("test binary path: {e}"))?;
    p.pop(); // deps/<test-bin> -> deps
    p.pop(); // deps -> target/<profile>
    p.push("zeo");
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!(
            "ZEO_GOLDEN_BACKEND needs the zeo CLI at {} (run `cargo build -p zeo` first)",
            p.display()
        ))
    }
}

/// The Cranelift legs' runner. Rejection must stay distinguishable from a
/// program that built and then failed at run time (the `CompileFail`
/// contract), and a spawned CLI folds both into "nonzero exit" -- so the
/// program is object-compiled IN PROCESS first (the identical lowering the
/// JIT finalizes; decision-free duplication, correctness over speed on a
/// leg that runs on demand), and only a program that compiles is spawned.
fn run_via_cli(
    backend: &str,
    rb: &Path,
    source: &str,
    opts: &zeo::CompileOptions,
    args: &[String],
    stdin: Option<&[u8]>,
    run_cwd: &Path,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    zeo::compile_to_object_with(source, opts).map_err(String::from)?;
    let mut cmd = Command::new(zeo_cli()?);
    cmd.arg("--backend").arg(backend);
    for root in &opts.load_roots {
        cmd.arg("-I").arg(root);
    }
    for dir in &opts.package_dirs {
        cmd.arg("--gems").arg(dir);
    }
    cmd.arg(rb).args(args).current_dir(run_cwd);
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
    if let Some(backend) = golden_backend() {
        return run_via_cli(&backend, rb, source, &opts, args, stdin, run_cwd);
    }
    let compiled = zeo::compile_to_rust_with(source, &opts).map_err(String::from)?;
    let bin = std::env::temp_dir().join(format!(
        "zeo-golden-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let runtime = zeo::backend::Runtime::for_prism(compiled.needs_prism_runtime);
    // Golden binaries are throwaway: link the runtime DYNAMICALLY (shared dylib,
    // to keep the bin-cache small. See `run_ruby_packages`.
    let linkage = zeo::backend::Linkage::Dynamic;
    zeo::backend::ensure_runtime_built(profile(), runtime, linkage)?;
    // Unoptimized: a golden test only diffs OUTPUT, its hot paths live in the
    // release runtime dylib, and `-O2` over a gem-scale generated main costs
    // rustc tens of minutes on a cold cache.
    zeo::backend::build_binary(
        &compiled.rust_source,
        &bin,
        profile(),
        runtime,
        linkage,
        zeo::backend::GenOpt::Unoptimized,
    )?;

    let mut cmd = Command::new(&bin);
    cmd.args(args).current_dir(run_cwd);
    let out = run_bounded(&mut cmd, stdin, "compiled binary");
    let _ = std::fs::remove_file(&bin);
    out
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
    // `Ruby::Box` examples need the experimental namespace flag + env.
    if source.contains("Ruby::Box") {
        cmd.arg("-W:no-experimental").env("RUBY_BOX", "1");
    }
    cmd.arg(rb).args(args).current_dir(run_cwd);
    run_bounded(&mut cmd, stdin, "ruby oracle")
}

/// Under `cargo xtask bless`: (re)write `<rb>.expected` (+ `.err.expected`)
/// from the oracle.
/// A stdout-only suite (`check_stderr == false`, i.e. examples) never keeps a
/// stderr golden -- ruby's parse warnings / experimental notices / thread
/// exception reports aren't part of the contract there.
fn bless(
    rb: &Path,
    source: &str,
    sc: &Sidecars,
    run_cwd: &Path,
    check_stderr: bool,
    env: &SuiteEnv,
) -> datatest_stable::Result<()> {
    let (stdout, stderr) = run_oracle(rb, source, &sc.args, sc.stdin.as_deref(), run_cwd, env)?;
    let out = norm(&stdout, rb, run_cwd);
    std::fs::write(format!("{}.expected", rb.display()), &out)?;
    let err_path = format!("{}.err.expected", rb.display());
    let err = norm(&stderr, rb, run_cwd);
    if check_stderr && !err.is_empty() {
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
/// `check_stderr` is false for the stdout-only examples suite, true for the
/// corpus/gaps (full stdout+stderr fidelity).
pub fn run_golden(
    rb: &Path,
    mode: Mode,
    run_cwd: &Path,
    check_stderr: bool,
) -> datatest_stable::Result<()> {
    run_golden_env(rb, mode, run_cwd, check_stderr, suite_env_default())
}

/// [`run_golden`] with a per-suite [`SuiteEnv`].
pub fn run_golden_env(
    rb: &Path,
    mode: Mode,
    run_cwd: &Path,
    check_stderr: bool,
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
        return Ok(());
    }

    let source = std::fs::read_to_string(rb)?;
    let sc = sidecars(rb)?;

    if std::env::var_os("ZEO_BLESS_FROM_XTASK").is_some() && mode != Mode::CompileFail {
        return bless(rb, &source, &sc, run_cwd, check_stderr, env);
    }

    if mode == Mode::CompileFail {
        // "Rejected" means zeo can't produce a RUNNABLE binary -- a clean
        // analyze/codegen error OR generated Rust that rustc refuses (the old
        // CLI-based harness treated a nonzero `zeo <src> -o bin` exit, from
        // either stage, as the rejection). `compile_and_run` returns `Err`
        // exactly when compile or link fails (a program that builds and then
        // crashes at runtime returns `Ok`, so it does NOT count as rejected).
        return match compile_and_run(rb, &source, &sc.args, sc.stdin.as_deref(), run_cwd, env) {
            Err(_) => Ok(()),
            Ok(_) => Err(format!(
                "{}: expected zeo to REJECT this program, but it built and ran",
                rb.display()
            )
            .into()),
        };
    }

    // Pass / Xfail: build + run, then diff against the golden.
    let actual = compile_and_run_contained(rb, &source, &sc, run_cwd, env);

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
                && (!check_stderr || norm(err, rb, run_cwd) == norm(&expected_err, rb, run_cwd))
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
             `scripts/promote-gap.sh {stem}` (moves it + its sidecars into tests/, \
             the zeo-authored suite -- NOT tests/spinel/, which mirrors the vendored \
             spinel corpus).",
            stem = rb.file_stem().unwrap_or(rb.as_os_str()).to_string_lossy()
        )
        .into()),
        Mode::Xfail => Ok(()), // still diverges: expected.
        Mode::CompileFail => unreachable!("handled above"),
    }
}

/// `compile_and_run`, with a compiler panic turned into an ordinary `Err`.
///
/// A panic is a divergence like any other -- ruby ran the program, zeo did not
/// -- but an uncaught one takes the whole test binary down, so a gap that
/// panics could not be recorded at all. Containing it lets `tests/gaps/` hold
/// the panicking program as an XFAIL, which is where a known bug belongs. The
/// message is kept and prefixed, so a `Mode::Pass` failure still says plainly
/// that zeo crashed rather than printing a bare output mismatch.
fn compile_and_run_contained(
    rb: &Path,
    source: &str,
    sc: &Sidecars,
    run_cwd: &Path,
    env: &SuiteEnv,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compile_and_run(rb, source, &sc.args, sc.stdin.as_deref(), run_cwd, env)
    }));
    caught.unwrap_or_else(|payload| {
        let msg = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic payload>");
        Err(format!("{PANIC_PREFIX}{msg}"))
    })
}

/// Marks an `Err` that came from a panic rather than a reported error.
pub const PANIC_PREFIX: &str = "zeo PANICKED: ";

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
        // A panic and a reported error both arrive as `Err`, but they mean
        // different things to whoever reads the failure: one is a bug in the
        // compiler, the other a limit it stated.
        Err(e) if e.starts_with(PANIC_PREFIX) => format!(
            "{}: THE COMPILER PANICKED (an internal error, not a reported \
             limitation): {}",
            rb.display(),
            e.trim_start_matches(PANIC_PREFIX)
        ),
        Err(e) => format!("{}: zeo failed to compile/run it: {e}", rb.display()),
    }
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
