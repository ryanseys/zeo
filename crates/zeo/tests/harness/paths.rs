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
