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
#[allow(
    dead_code,
    reason = "shared through `#[path]`; each binary uses a part of it"
)]
pub fn runtime_archive() -> Result<PathBuf, String> {
    zeo::backend::link::runtime_archive()
}

/// The one directory tests write under: `target/zeo-test/<stamp>/`, where
/// the stamp names THIS build of `zeo` and `libzeo.a`. A rebuilt compiler
/// must never read artifacts an older one wrote, so a new stamp sweeps the
/// old ones. Nothing test-related goes to `$TMPDIR`.
///
/// The sweep spares any directory touched in the last [`IN_USE`], because a
/// stamp can change DURING a run: an edit to a runtime source rebuilds
/// `libzeo.a`, and every process started after it keys differently. Without
/// the reprieve those processes delete the directory their siblings are
/// linking in, and a dozen unrelated cases fail with a missing object file.
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
                if e.file_name() != *key.as_str() && idle(&e.path()) {
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

/// How long a scratch directory counts as belonging to a run still in
/// flight. Well above the slowest case's own bounds.
const IN_USE: std::time::Duration = std::time::Duration::from_secs(900);

/// Whether nothing has written under `dir` recently. A directory's own mtime
/// only moves when its top-level entries change, so this reads the
/// subdirectories the children actually write into as well.
fn idle(dir: &Path) -> bool {
    let touched = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
    };
    std::iter::once(dir.to_path_buf())
        .chain(
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path()),
        )
        .filter_map(|p| touched(&p))
        .min()
        .is_none_or(|age| age > IN_USE)
}

/// A fresh, empty directory under the scratch root for one test.
#[allow(
    dead_code,
    reason = "shared through `#[path]`; each binary uses a part of it"
)]
pub fn scratch_dir(name: &str) -> Result<PathBuf, String> {
    let dir = scratch_root()?
        .join("tmp")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// The environment every spawned `zeo` gets.
///
/// `ZEO_CACHE=0` is the one that matters for the corpus: the program cache
/// writes a compiled object per program, and 5,757 of those every run is a
/// gigabyte written and thrown away. A corpus child compiles in memory and
/// leaves nothing behind. The cache has its own tests
/// (`test/compiler/cache/`, `api::packages`, `checks::cli`), which turn it
/// back on for the programs that are about it.
///
/// The two paths still point under the scratch root for the children that
/// DO write -- a real link, and a test that turns the cache on -- so nothing
/// reaches the developer's own cache or `$TMPDIR`.
pub fn child_env(cmd: &mut std::process::Command) -> Result<(), String> {
    let root = scratch_root()?;
    cmd.env("ZEO_CACHE", "0")
        .env("ZEO_CACHE_DIR", root.join("cache"))
        .env("TMPDIR", link_scratch(&root));
    Ok(())
}

/// Where a link writes its object file and its binary. Only the curated
/// `test/aot/` tier reaches this -- every other program compiles and runs in
/// memory -- but a linked binary is ~21 MB, so where those bytes land is
/// worth choosing.
///
/// `/dev/shm` is a tmpfs, so on Linux they never reach the disk at all.
/// macOS has no tmpfs; a RAM disk there costs an `hdiutil` mount to set up
/// and another to tear down, which is more test machinery than ~800 MB of
/// transient writes per run is worth.
fn link_scratch(root: &Path) -> PathBuf {
    let shm = Path::new("/dev/shm");
    if cfg!(target_os = "linux") && shm.is_dir() {
        let dir = shm.join("zeo-link");
        if std::fs::create_dir_all(&dir).is_ok() {
            return dir;
        }
    }
    root.join("bin")
}
