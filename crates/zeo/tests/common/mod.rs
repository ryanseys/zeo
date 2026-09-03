//! What every test binary needs: where the repo is, where the built `zeo`
//! is, and one scratch root for everything a test writes.

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

/// The built `zeo` CLI. Cargo builds the binary before the package's
/// integration tests, so this only reports where it is.
pub fn zeo_cli() -> Result<PathBuf, String> {
    let p = profile_dir()?.join("zeo");
    if !p.is_file() {
        return Err(format!("the harness needs the zeo CLI at {}", p.display()));
    }
    Ok(p)
}

/// `libzeo.a`, built when it is missing or older than the sources in it.
///
/// The rule belongs to the PRODUCT: `zeo` itself brings a dev tree's archive
/// up to date before it links anything, and the suites ask the same question
/// (`zeo::backend::link::runtime_archive`), so the two cannot disagree.
#[allow(dead_code)]
pub fn runtime_archive() -> Result<PathBuf, String> {
    zeo::backend::link::runtime_archive()
}

/// The one directory tests write under: `target/zeo-test/<stamp>/`, where
/// the stamp names THIS build of `zeo` and `libzeo.a`. A rebuilt compiler
/// must never read artifacts an older one wrote, so the first use of a new
/// stamp deletes every other one. Nothing test-related goes to `$TMPDIR`.
///
/// Children get `ZEO_CACHE_DIR` under it too, so the product cache a test
/// exercises is never the developer's own.
pub fn scratch_root() -> Result<PathBuf, String> {
    static ROOT: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let stamp = |p: &Path| -> Result<String, String> {
            let m = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let t = m
                .modified()
                .map_err(|e| e.to_string())?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            Ok(format!("{:x}-{:x}", t.as_millis(), m.len()))
        };
        let zeo = zeo_cli()?;
        let archive = profile_dir()?.join("libzeo.a");
        let key = match archive.is_file() {
            true => format!("{}_{}", stamp(&zeo)?, stamp(&archive)?),
            false => stamp(&zeo)?,
        };
        let root = zeo
            .parent()
            .and_then(Path::parent)
            .ok_or("the zeo binary has no target directory")?
            .join("zeo-test");
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                if e.file_name() != *key.as_str() {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        let dir = root.join(key);
        for sub in ["bin", "projects", "cache", "tmp"] {
            std::fs::create_dir_all(dir.join(sub))
                .map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        Ok(dir)
    })
    .clone()
}

/// A fresh, empty directory under the scratch root for one test.
#[allow(dead_code)]
pub fn scratch_dir(name: &str) -> Result<PathBuf, String> {
    let dir = scratch_root()?
        .join("tmp")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// The environment every spawned `zeo` gets: its own cache and temp dir
/// under the scratch root.
pub fn child_env(cmd: &mut std::process::Command) -> Result<(), String> {
    let root = scratch_root()?;
    cmd.env("ZEO_CACHE_DIR", root.join("cache"))
        .env("TMPDIR", root.join("bin"));
    Ok(())
}
