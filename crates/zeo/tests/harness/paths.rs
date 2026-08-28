//! Where the repo and the built binary are. Every suite needs these; only
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

/// The built `zeo` CLI beside this test binary's profile dir. Cargo builds
/// it before the package's integration tests; it does NOT build `libzeo.a`,
/// which the AOT leg links -- see `golden::assert_staticlib_is_fresh`.
pub fn zeo_cli() -> Result<PathBuf, String> {
    let mut p = std::env::current_exe().map_err(|e| format!("test binary path: {e}"))?;
    p.pop(); // deps/<test-bin> -> deps
    p.pop(); // deps -> target/<profile>
    p.push("zeo");
    if !p.is_file() {
        return Err(format!(
            "the harness needs the zeo CLI at {} (run `cargo build -p zeo` first)",
            p.display()
        ));
    }
    Ok(p)
}
