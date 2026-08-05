//! Where zeo finds its payload -- the bundled `gems/` and the runtime
//! workspace `ensure_runtime_built` compiles -- and where it writes build
//! artifacts.
//!
//! Two homes exist today:
//!
//! - **Dev tree**: the zeo repo itself. `cargo run`, the test harness, and
//!   xtask all execute from binaries under `target/`, and the repo root (baked
//!   in via `CARGO_MANIFEST_DIR` at compile time) is both the payload (its
//!   `gems/`, its `crates/zeo-rt`) and the build root (its `target/`).
//! - **Installed**: a relocatable prefix laid out as `<prefix>/bin/zeo` +
//!   `<prefix>/share/zeo/{gems,runtime}`, assembled by `cargo xtask dist`.
//!   The payload is found relative to the executable, and all build output
//!   goes to a per-user cache -- the prefix itself is never written to (it
//!   may be root-owned, as in a Homebrew Cellar).
//!
//! Resolution order: `ZEO_HOME` (explicit payload dir, validated), then the
//! executable-relative probe, then the dev tree. The order needs no build-time
//! flag to keep dev workflows on the dev tree: dev binaries live under
//! `target/{debug,release}/`, where the exe-relative probe
//! (`target/share/zeo`) can never hit.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The resolved home. See the module docs for the two shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZeoHome {
    /// The zeo repo: payload dirs and `target/` both live at `root`.
    DevTree { root: PathBuf },
    /// A relocatable install: read-only `payload` (= `<prefix>/share/zeo`),
    /// build output under the per-user `cache`.
    Installed { payload: PathBuf, cache: PathBuf },
}

/// The process-wide home, resolved once.
///
/// Panics with the full probe trail when no home resolves; the CLI calls
/// [`ensure_resolved`] first so a broken install reports as an ordinary error
/// instead. Library/test entrypoints run from the dev tree, where resolution
/// cannot fail.
pub fn zeo_home() -> &'static ZeoHome {
    static HOME: OnceLock<ZeoHome> = OnceLock::new();
    HOME.get_or_init(|| match try_resolve() {
        Ok(home) => home,
        Err(e) => panic!("{e}"),
    })
}

/// CLI-facing pre-flight: resolve (and memoize) the home, reporting failure
/// as an error instead of the panic [`zeo_home`] falls back on.
pub fn ensure_resolved() -> Result<(), String> {
    // Probe without touching the OnceLock so a failure stays reportable; the
    // success result is re-derived (cheaply) on first real use.
    try_resolve().map(|_| ())
}

fn try_resolve() -> Result<ZeoHome, String> {
    resolve(
        std::env::current_exe().ok().as_deref(),
        std::env::var_os("ZEO_HOME").as_deref(),
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")),
    )
}

/// The pure resolution function -- every input injected so precedence is
/// testable without faking `current_exe` or the environment.
fn resolve(exe: Option<&Path>, env_home: Option<&OsStr>, dev_root: &Path) -> Result<ZeoHome, String> {
    // 1. Explicit override. A set-but-broken ZEO_HOME is a hard error, never
    //    a silent fall-through -- the user asked for THIS payload.
    if let Some(dir) = env_home {
        let payload = PathBuf::from(dir);
        if payload_is_valid(&payload) {
            return Ok(ZeoHome::Installed {
                cache: cache_root(),
                payload,
            });
        }
        return Err(format!(
            "ZEO_HOME is set to `{}`, but that is not a zeo payload directory \
             (expected `runtime/Cargo.toml` beneath it)",
            payload.display()
        ));
    }

    // 2. Executable-relative: <prefix>/bin/zeo -> <prefix>/share/zeo.
    //    Canonicalize FIRST: an installed `zeo` is routinely reached through a
    //    symlink (Homebrew links `$(brew --prefix)/bin/zeo` into the Cellar),
    //    and the payload lives next to the real binary, not the link.
    let probed = exe
        .and_then(|e| std::fs::canonicalize(e).ok())
        .and_then(|e| Some(e.parent()?.parent()?.join("share").join("zeo")));
    if let Some(payload) = probed.clone().filter(|p| payload_is_valid(p)) {
        return Ok(ZeoHome::Installed {
            cache: cache_root(),
            payload,
        });
    }

    // 3. The dev tree this binary was compiled in, if it still exists.
    if dev_root.join("crates/zeo-rt/Cargo.toml").is_file() {
        // Collapse the `crates/zeo/../..` self-reference so every derived
        // path (target dir, gems dir, error messages) reads cleanly.
        let root = std::fs::canonicalize(dev_root).unwrap_or_else(|_| dev_root.to_path_buf());
        return Ok(ZeoHome::DevTree { root });
    }

    Err(format!(
        "zeo cannot find its runtime payload.\n\
         Probed, in order:\n\
         - ZEO_HOME: not set\n\
         - executable-relative: {}\n\
         - dev tree: {} (no crates/zeo-rt there)\n\
         An installed zeo expects `share/zeo/{{gems,runtime}}` next to its \
         `bin/` directory; set ZEO_HOME to point at a payload directory to \
         override.",
        probed
            .map(|p| format!("{} (no runtime/Cargo.toml there)", p.display()))
            .unwrap_or_else(|| "could not determine the executable's path".into()),
        dev_root.display(),
    ))
}

/// A payload directory is one `cargo xtask dist` laid out: the runtime
/// workspace is the load-bearing half (`gems/` is optional -- its absence
/// just contributes no bundled gems, as in the dev tree).
fn payload_is_valid(payload: &Path) -> bool {
    payload.join("runtime").join("Cargo.toml").is_file()
}

/// The per-user cache root for installed mode: `ZEO_CACHE_DIR`, else the
/// platform convention (`~/Library/Caches/zeo` on macOS, XDG on everything
/// else). Deliberately NOT `CARGO_TARGET_DIR`: ambient cargo configuration
/// from an unrelated shell must not redirect an installed zeo's cache.
fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZEO_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Caches/zeo");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
            return PathBuf::from(xdg).join("zeo");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".cache/zeo");
        }
    }
    // No HOME at all (a bare daemon environment): a relative fallback keeps
    // zeo functional in the CWD rather than failing outright.
    PathBuf::from(".zeo-cache")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    fn payload_fixture(dir: &Path) -> PathBuf {
        let payload = dir.join("share/zeo");
        touch(&payload.join("runtime/Cargo.toml"));
        payload
    }

    fn dev_fixture(dir: &Path) -> PathBuf {
        let root = dir.join("repo");
        touch(&root.join("crates/zeo-rt/Cargo.toml"));
        root
    }

    #[test]
    fn env_home_wins_over_everything() {
        let tmp = tempdir("env-wins");
        let payload = payload_fixture(&tmp);
        let dev = dev_fixture(&tmp);
        let exe = tmp.join("bin/zeo");
        touch(&exe);
        let home = resolve(Some(&exe), Some(payload.as_os_str()), &dev).unwrap();
        assert!(matches!(home, ZeoHome::Installed { payload: p, .. } if p == payload));
    }

    #[test]
    fn broken_env_home_is_a_hard_error_not_a_fallthrough() {
        let tmp = tempdir("env-broken");
        let dev = dev_fixture(&tmp);
        let missing = tmp.join("nope");
        let err = resolve(None, Some(missing.as_os_str()), &dev).unwrap_err();
        assert!(err.contains("ZEO_HOME"), "{err}");
    }

    #[test]
    fn exe_relative_probe_finds_a_prefix() {
        let tmp = tempdir("exe-probe");
        let payload = payload_fixture(&tmp);
        let exe = tmp.join("bin/zeo");
        touch(&exe);
        let dev = tmp.join("no-repo-here");
        let home = resolve(Some(&exe), None, &dev).unwrap();
        // Canonicalized comparison: the temp root itself may sit behind a
        // symlink (macOS /var -> /private/var), and the probe canonicalizes.
        let want = std::fs::canonicalize(&payload).unwrap();
        assert!(matches!(home, ZeoHome::Installed { payload: p, .. } if p == want));
    }

    #[test]
    fn exe_probe_resolves_through_a_symlinked_bin() {
        let tmp = tempdir("exe-symlink");
        let payload = payload_fixture(&tmp);
        let real = tmp.join("bin/zeo");
        touch(&real);
        // A Homebrew-style opt/bin symlink two prefixes away.
        let linkdir = tmp.join("opt/bin");
        std::fs::create_dir_all(&linkdir).unwrap();
        let link = linkdir.join("zeo");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let dev = tmp.join("no-repo-here");
        let home = resolve(Some(&link), None, &dev).unwrap();
        // Canonicalize both sides: the fixture itself may sit under a
        // symlinked temp root (macOS /tmp -> /private/tmp).
        let want = std::fs::canonicalize(&payload).unwrap();
        assert!(
            matches!(home, ZeoHome::Installed { payload: p, .. } if p == want),
            "resolved through the symlink to the real prefix"
        );
    }

    #[test]
    fn dev_tree_is_the_fallback() {
        let tmp = tempdir("dev-fallback");
        let dev = dev_fixture(&tmp);
        let exe = tmp.join("repo/target/debug/zeo");
        touch(&exe);
        let home = resolve(Some(&exe), None, &dev).unwrap();
        let want = std::fs::canonicalize(&dev).unwrap();
        assert!(matches!(home, ZeoHome::DevTree { root } if root == want));
    }

    #[test]
    fn nothing_resolving_names_every_probe() {
        let tmp = tempdir("nothing");
        let exe = tmp.join("bin/zeo");
        touch(&exe);
        let err = resolve(Some(&exe), None, &tmp.join("no-repo")).unwrap_err();
        assert!(err.contains("executable-relative"), "{err}");
        assert!(err.contains("dev tree"), "{err}");
    }

    #[test]
    fn the_test_harness_itself_runs_in_the_dev_tree() {
        assert!(matches!(zeo_home(), ZeoHome::DevTree { .. }));
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zeo-home-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
