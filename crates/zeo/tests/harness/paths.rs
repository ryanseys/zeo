//! Where the repo and the runtime archive are. Every suite needs these; only
//! the golden suites need the rest of the harness.

use std::path::{Path, PathBuf};

/// The repo root (this crate's `../..`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the workspace root")
        .to_path_buf()
}

/// The ruby oracle: `mise which ruby` (the `mise.toml` pin), else the `ruby`
/// on PATH. CI has no mise and puts the same pinned ruby on PATH.
///
/// Always an ABSOLUTE path: a caller that clears the child's environment
/// (the parity test) has no PATH left to resolve a bare name with.
#[allow(dead_code)] // the `checks` binary includes this file and asks no oracle
pub fn resolve_ruby(cwd: &Path) -> PathBuf {
    let out = std::process::Command::new("mise")
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
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).map(|d| d.join("ruby")).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("ruby"))
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
/// One line, because the rule belongs to the PRODUCT: `zeo` itself has to
/// bring a dev tree's archive up to date before it links anything, or a
/// runtime edit reaches the binary and not the programs the binary runs. The
/// suites just ask the same question the compiler asks
/// (`zeo::backend::link::runtime_archive`), so the two cannot answer
/// differently.
pub fn runtime_archive() -> Result<PathBuf, String> {
    zeo::backend::link::runtime_archive()
}
