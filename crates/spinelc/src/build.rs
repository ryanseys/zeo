//! Compiles generated Rust source directly against the WORKSPACE's own
//! already-built `spinel-rt` artifacts -- a single `rustc` invocation with
//! `--extern spinel_rt=target/debug/libspinel_rt.rlib` and `-L
//! dependency=target/debug/deps`, not a fresh throwaway `cargo` project.
//!
//! This deliberately does NOT enumerate `spinel-rt`'s own transitive
//! dependencies (`indexmap`/`parking_lot`/`regex`/...) by hand: `rustc`
//! resolves those automatically via the `-L` search path, using the crate
//! metadata already embedded in `libspinel_rt.rlib` itself -- the generated
//! program only ever references `spinel_rt` directly (`use spinel_rt::...`),
//! never its transitive deps by name, so only ONE `--extern` is ever needed.
//! Confirmed both correct (a real generated program links and runs) and
//! dramatically cheaper this way: a fresh-`cargo`-project build (the
//! previous approach here) recompiled `spinel-rt` and its whole dependency
//! graph from scratch on EVERY call (no target-dir reuse across throwaway
//! projects), costing ~3s per call; direct `rustc` against the already-built
//! artifacts costs ~0.2s. This is the same "compile one generated file
//! against already-built deps" shape tools like `trybuild`/`compiletest` use
//! for the identical reason.
//!
//! `ensure_spinel_rt_built` (a `std::sync::OnceLock`-guarded `cargo build -p
//! spinel-rt`) is what keeps this correct rather than merely fast: it
//! guarantees the rlib this reads is fresh relative to `spinel-rt`'s current
//! source, regardless of how this function's CALLER was invoked (the
//! in-process test harness has no other guarantee spinel-rt was built
//! first, since `spinelc`'s own `Cargo.toml` has no ordinary dependency on
//! it -- see that crate's dev-dependency addition). Run once per process
//! (cheap and idempotent every time after the first -- Cargo's own
//! fingerprinting makes a no-op rebuild check near-instant), not once per
//! call, so the hundreds of calls a test suite makes don't each pay a
//! `cargo` subprocess-startup cost just to confirm nothing changed.
//!
//! Lives in the library (not `main.rs`) so the in-process test harness
//! (`tests/support`) can call the exact same function the CLI uses, rather
//! than a re-derived copy of the same logic.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The workspace root -- two levels up from `crates/spinelc` (this crate's
/// own `CARGO_MANIFEST_DIR`), i.e. wherever the top-level `Cargo.toml`/
/// `target/` actually live.
fn workspace_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// Respects `CARGO_TARGET_DIR` (the standard Cargo override) if set, else
/// the ordinary `<workspace_root>/target` default -- not a full `cargo
/// metadata` query (this project's build layout is simple enough that the
/// common cases are all that's needed).
fn target_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => workspace_root().join("target"),
    }
}

static SPINEL_RT_BUILT: OnceLock<Result<(), String>> = OnceLock::new();

/// Builds `spinel-rt` (debug profile) into the shared workspace `target/`
/// exactly once per process -- see this module's docs for why this can't
/// just be assumed already done. Every subsequent call in the same process
/// (the common case: a test binary making hundreds of `build_binary` calls)
/// skips straight past the `OnceLock`, at no cost.
fn ensure_spinel_rt_built() -> Result<(), String> {
    SPINEL_RT_BUILT
        .get_or_init(|| {
            let result = std::process::Command::new("cargo")
                .args(["build", "--quiet", "-p", "spinel-rt"])
                .current_dir(workspace_root())
                .status();
            match result {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("`cargo build -p spinel-rt` exited with {status}")),
                Err(e) => Err(format!("running `cargo build -p spinel-rt`: {e}")),
            }
        })
        .clone()
}

pub fn build_binary(rust_source: &str, output: &Path) -> Result<(), String> {
    ensure_spinel_rt_built()?;

    let debug_dir = target_dir().join("debug");
    let rlib = debug_dir.join("libspinel_rt.rlib");
    if !rlib.exists() {
        return Err(format!(
            "expected {} to exist after building spinel-rt -- was it built into a different target directory?",
            rlib.display()
        ));
    }
    let deps_dir = debug_dir.join("deps");

    let src_path = std::env::temp_dir().join(format!(
        "spinelc-gen-{}-{}.rs",
        std::process::id(),
        thread_unique_suffix()
    ));
    std::fs::write(&src_path, rust_source).map_err(|e| e.to_string())?;

    let status = std::process::Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg(&src_path)
        .arg("-o")
        .arg(output)
        .arg("--extern")
        .arg(format!("spinel_rt={}", rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps_dir.display()))
        .status()
        .map_err(|e| format!("running rustc: {e}"))?;

    if !status.success() {
        return Err(format!(
            "rustc failed compiling the generated program (source left at {})",
            src_path.display()
        ));
    }
    let _ = std::fs::remove_file(&src_path);
    Ok(())
}

/// Multiple tests in the same test-binary process share `process::id()`; a
/// per-thread suffix keeps their temp source files from colliding when the
/// test runner parallelizes `#[test]` functions.
fn thread_unique_suffix() -> String {
    format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}
