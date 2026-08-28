//! Where the repo and the runtime archive are. Every suite needs these; only
//! the golden suites need the rest of the harness.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The repo root (this crate's `../..`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the workspace root")
        .to_path_buf()
}

/// `target/<profile>/`, where cargo puts this package's binaries.
pub fn profile_dir() -> Result<PathBuf, String> {
    let mut p = std::env::current_exe().map_err(|e| format!("test binary path: {e}"))?;
    p.pop(); // deps/<test-bin> -> deps
    p.pop(); // deps -> target/<profile>
    Ok(p)
}

/// `libzeo.a`, built when it is missing or older than the sources in it.
///
/// A test run never emits it on its own: only `cargo build` asks the lib
/// target for every crate-type, and a test binary's dependency edge asks for
/// the rlib alone. Every path that links an AOT program goes through here,
/// so a clean tree needs no build ritual and an edited runtime cannot be
/// linked stale.
///
/// One `cargo` per process at most, and only when the archive is actually
/// out of date. Concurrent test processes serialize on cargo's own build
/// lock, so the first builds and the rest find it fresh.
pub fn runtime_archive() -> Result<PathBuf, String> {
    static ONCE: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let dir = profile_dir()?;
        let lib = dir.join("libzeo.a");
        if let Some(stale) = staler_than(&lib) {
            build_archive(&dir, &stale)?;
        }
        lib.is_file()
            .then_some(lib.clone())
            .ok_or_else(|| format!("{} is still missing after cargo build", lib.display()))
    })
    .clone()
}

/// The newest runtime source newer than `lib`, or `None` when `lib` is up to
/// date. A missing archive reports the newest source there is.
fn staler_than(lib: &Path) -> Option<PathBuf> {
    let lib_at = std::fs::metadata(lib).and_then(|m| m.modified()).ok();
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut stack: Vec<PathBuf> = ["zeo", "zeo-rt", "zeo-abi", "zeo-macros"]
        .iter()
        .map(|c| workspace_root().join("crates").join(c).join("src"))
        .collect();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs")
                && let Ok(at) = e.metadata().and_then(|m| m.modified())
                && newest.as_ref().is_none_or(|(n, _)| at > *n)
            {
                newest = Some((at, p));
            }
        }
    }
    let (at, path) = newest?;
    match lib_at {
        Some(lib_at) if lib_at >= at => None,
        _ => Some(path),
    }
}

fn build_archive(dir: &Path, because: &Path) -> Result<(), String> {
    // `debug` is the `dev` profile's output directory, not its name.
    let profile = match dir.file_name().and_then(|n| n.to_str()) {
        Some("debug") | None => "dev",
        Some(other) => other,
    };
    eprintln!(
        "harness: building libzeo.a ({} is newer)",
        because.display()
    );
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["build", "-p", "zeo", "--lib", "--profile", profile])
        .current_dir(workspace_root())
        .output()
        .map_err(|e| format!("spawning cargo to build libzeo.a: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "building libzeo.a failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    ))
}
