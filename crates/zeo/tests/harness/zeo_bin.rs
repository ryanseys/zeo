//! The built `zeo` CLI, for the suites that spawn it.

use std::path::PathBuf;

/// Cargo builds the binary before the package's integration tests, so this
/// only reports where it is.
pub fn zeo_cli() -> Result<PathBuf, String> {
    let p = crate::paths::profile_dir()?.join("zeo");
    if !p.is_file() {
        return Err(format!("the harness needs the zeo CLI at {}", p.display()));
    }
    Ok(p)
}
