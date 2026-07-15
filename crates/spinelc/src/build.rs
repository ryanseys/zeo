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

static CRATES_BUILT: OnceLock<std::sync::Mutex<std::collections::HashMap<String, Result<(), String>>>> =
    OnceLock::new();

/// Builds one workspace crate (debug profile) into the shared workspace
/// `target/` exactly once per process per crate -- see this module's docs
/// for why this can't just be assumed already done. `spinel-rt` always goes
/// through here; a native package's `[native]` crate (Phase 14.3) does too,
/// the first time a program requiring it is built. Every subsequent call in
/// the same process (the common case: a test binary making hundreds of
/// `build_binary` calls) is a cached map hit.
fn ensure_crate_built(name: &str) -> Result<(), String> {
    let map = CRATES_BUILT.get_or_init(Default::default);
    let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = map.get(name) {
        return cached.clone();
    }
    let result = match std::process::Command::new("cargo")
        .args(["build", "--quiet", "-p", name])
        .current_dir(workspace_root())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("`cargo build -p {name}` exited with {status}")),
        Err(e) => Err(format!("running `cargo build -p {name}`: {e}")),
    };
    map.insert(name.to_string(), result.clone());
    result
}

/// The built rlib for a workspace lib crate, erroring loudly if it isn't
/// where the shared target dir says it should be.
fn rlib_for(crate_name: &str) -> Result<PathBuf, String> {
    let underscored = crate_name.replace('-', "_");
    let rlib = target_dir()
        .join("debug")
        .join(format!("lib{underscored}.rlib"));
    if !rlib.exists() {
        return Err(format!(
            "expected {} to exist after building {crate_name} -- was it built into a different target directory?",
            rlib.display()
        ));
    }
    Ok(rlib)
}

pub fn build_binary(rust_source: &str, output: &Path) -> Result<(), String> {
    build_binary_with_deps(rust_source, &[], output)
}

/// `build_binary` plus the extra workspace lib crates (`native_deps`, cargo
/// package names -- see `spinelc::CompileOutput`) the generated program
/// references: each is `cargo build`-built once and linked with its own
/// `--extern`, exactly the shape `spinel-rt` itself uses.
pub fn build_binary_with_deps(
    rust_source: &str,
    native_deps: &[String],
    output: &Path,
) -> Result<(), String> {
    ensure_crate_built("spinel-rt")?;
    for dep in native_deps {
        ensure_crate_built(dep)?;
    }

    let rlib = rlib_for("spinel-rt")?;
    let deps_dir = target_dir().join("debug").join("deps");

    let src_path = std::env::temp_dir().join(format!(
        "spinelc-gen-{}-{}.rs",
        std::process::id(),
        thread_unique_suffix()
    ));
    std::fs::write(&src_path, rust_source).map_err(|e| e.to_string())?;

    let mut cmd = std::process::Command::new("rustc");
    cmd.arg("--edition")
        .arg("2021")
        .arg(&src_path)
        .arg("-o")
        .arg(output)
        .arg("--extern")
        .arg(format!("spinel_rt={}", rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps_dir.display()));
    for dep in native_deps {
        let dep_rlib = rlib_for(dep)?;
        cmd.arg("--extern")
            .arg(format!("{}={}", dep.replace('-', "_"), dep_rlib.display()));
    }
    let status = cmd.status().map_err(|e| format!("running rustc: {e}"))?;

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
