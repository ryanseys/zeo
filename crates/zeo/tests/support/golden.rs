//! Shared golden-file test helper for the datatest-stable suites
//! (`tests/gaps.rs`, `tests/examples.rs`, `tests/corpus.rs`).
//!
//! One function, [`run_golden`], drives every `.rb` the same way the e2e
//! `run_ruby` helper does -- `zeo::compile_to_rust_with` ->
//! `zeo::build::build_binary` (content-addressed cache) -> spawn -- then diffs
//! stdout/stderr against the committed **ruby-oracle** golden `.expected`
//! (+ `.err.expected`/`.args`/`.stdin` sidecars).
//!
//! - `Mode::Pass` (corpus, examples): zeo must MATCH the golden.
//! - `Mode::CompileFail` (`analyze_fail/`): zeo must REJECT the program.
//! - `Mode::Xfail` (gaps): zeo must DIVERGE from the golden -- a match means the
//!   gap is fixed and the test FAILS with a "promote" message.
//!
//! `ZEO_BLESS=1 cargo test` re-records the goldens from the real `ruby` oracle
//! (`--disable-error_highlight --disable-did_you_mean`, resolved via `mise`)
//! instead of asserting. This is the single golden writer.

#![allow(dead_code)] // each test target includes its own copy; not all use every item.

use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Working directory a suite's programs run in (and the base its source paths
/// are relativized against). The corpus/gaps `.args` use paths relative to
/// these, matching how the old conformance harness ran them.
pub fn corpus_run_cwd() -> PathBuf {
    workspace_root().join("conformance/corpus")
}
pub fn gaps_run_cwd() -> PathBuf {
    workspace_root().join("conformance")
}
pub fn examples_run_cwd() -> PathBuf {
    workspace_root()
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

fn norm(bytes: &[u8], source: &Path, run_cwd: &Path) -> Vec<u8> {
    normalize_source_path(normalize_crlf(bytes), source, run_cwd)
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

fn profile() -> zeo::build::Profile {
    zeo::build::Profile::from_env_or(zeo::build::Profile::Release)
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
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let opts = zeo::CompileOptions {
        input_path: Some(rb.to_path_buf()),
        ..Default::default()
    };
    let compiled = zeo::compile_to_rust_with(source, &opts).map_err(String::from)?;
    let bin = std::env::temp_dir().join(format!(
        "zeo-golden-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let runtime = zeo::build::Runtime::for_eval(compiled.needs_eval_vm);
    zeo::build::ensure_runtime_built(profile(), runtime)?;
    zeo::build::build_binary(&compiled.rust_source, &bin, profile(), runtime)?;

    let mut cmd = Command::new(&bin);
    cmd.args(args).current_dir(run_cwd);
    let out = if let Some(input) = stdin {
        use std::io::Write as _;
        use std::process::Stdio;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("spawn: {e}"))?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input)
            .map_err(|e| format!("write stdin: {e}"))?;
        child.wait_with_output()
    } else {
        cmd.output()
    }
    .map_err(|e| format!("running compiled binary: {e}"));
    let _ = std::fs::remove_file(&bin);
    let out = out?;
    Ok((out.stdout, out.stderr))
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
    if let Ok(out) = out {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if !path.is_empty() {
                return PathBuf::from(path);
            }
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
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let ruby = resolve_ruby(run_cwd);
    let mut cmd = Command::new(&ruby);
    cmd.arg("--disable-error_highlight").arg("--disable-did_you_mean");
    // `Ruby::Box` examples need the experimental namespace flag + env, mirroring
    // the old `xtask regen`.
    if source.contains("Ruby::Box") {
        cmd.arg("-W:no-experimental").env("RUBY_BOX", "1");
    }
    cmd.arg(rb).args(args).current_dir(run_cwd);
    let out = if let Some(input) = stdin {
        use std::io::Write as _;
        use std::process::Stdio;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("spawn ruby: {e}"))?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input)
            .map_err(|e| format!("write ruby stdin: {e}"))?;
        child.wait_with_output()
    } else {
        cmd.output()
    }
    .map_err(|e| format!("running ruby oracle: {e}"))?;
    Ok((out.stdout, out.stderr))
}

/// `ZEO_BLESS=1`: (re)write `<rb>.expected` (+ `.err.expected`) from the oracle.
fn bless(rb: &Path, source: &str, sc: &Sidecars, run_cwd: &Path) -> datatest_stable::Result<()> {
    let (stdout, stderr) = run_oracle(rb, source, &sc.args, sc.stdin.as_deref(), run_cwd)?;
    let out = norm(&stdout, rb, run_cwd);
    let err = norm(&stderr, rb, run_cwd);
    std::fs::write(format!("{}.expected", rb.display()), &out)?;
    let err_path = format!("{}.err.expected", rb.display());
    if err.is_empty() {
        let _ = std::fs::remove_file(&err_path); // absent => "stderr must be empty"
    } else {
        std::fs::write(&err_path, &err)?;
    }
    Ok(())
}

// ---- the entry point ----

/// Run one golden case. See the module docs for the per-`Mode` contract.
pub fn run_golden(rb: &Path, mode: Mode, run_cwd: &Path) -> datatest_stable::Result<()> {
    // datatest-stable hands us a path relative to the crate manifest dir (the
    // test process's cwd); absolutize it so ruby/the binary find it after we
    // `current_dir(run_cwd)`, and so source-path normalization matches.
    let rb = &std::fs::canonicalize(rb).unwrap_or_else(|_| rb.to_path_buf());
    let source = std::fs::read_to_string(rb)?;
    let sc = sidecars(rb)?;

    if std::env::var_os("ZEO_BLESS").is_some() && mode != Mode::CompileFail {
        return bless(rb, &source, &sc, run_cwd);
    }

    if mode == Mode::CompileFail {
        let opts = zeo::CompileOptions {
            input_path: Some(rb.to_path_buf()),
            ..Default::default()
        };
        return match zeo::compile_to_rust_with(&source, &opts) {
            Err(_) => Ok(()), // rejected, as required
            Ok(_) => Err(format!(
                "{}: expected zeo to REJECT this program, but it compiled",
                rb.display()
            )
            .into()),
        };
    }

    // Pass / Xfail: build + run, then diff against the golden.
    let actual = compile_and_run(rb, &source, &sc.args, sc.stdin.as_deref(), run_cwd);

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
        None => run_oracle(rb, &source, &sc.args, sc.stdin.as_deref(), run_cwd)?,
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
        Mode::Pass => Err(mismatch_message(rb, &actual, &expected_out, &expected_err, run_cwd).into()),
        Mode::Xfail if matched => Err(format!(
            "GAP FIXED -- {} now matches ruby. Promote it: `git mv` its .rb (+ sidecars) \
             from conformance/gaps/ to conformance/corpus/test/.",
            rb.file_name().unwrap_or(rb.as_os_str()).to_string_lossy()
        )
        .into()),
        Mode::Xfail => Ok(()), // still diverges: expected.
        Mode::CompileFail => unreachable!("handled above"),
    }
}

fn mismatch_message(
    rb: &Path,
    actual: &Result<(Vec<u8>, Vec<u8>), String>,
    expected_out: &[u8],
    expected_err: &[u8],
    run_cwd: &Path,
) -> String {
    let show = |b: &[u8]| String::from_utf8_lossy(&norm(b, rb, run_cwd)).into_owned();
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
        Err(e) => format!("{}: zeo failed to compile/run it: {e}", rb.display()),
    }
}
