//! Where zeo finds its payload -- the bundled `gems/` and the runtime it
//! links programs against -- and where it writes build artifacts.
//!
//! Two homes exist today:
//!
//! - **Dev tree**: the zeo repo itself. `cargo run`, the test harness, and
//!   xtask all execute from binaries under `target/`, and the repo root (baked
//!   in via `CARGO_MANIFEST_DIR` at compile time) is both the payload (its
//!   libraries, its `crates/zeo-rt`) and the build root (its `target/`).
//! - **Installed**: a relocatable prefix laid out as `<prefix>/bin/zeo` +
//!   `<prefix>/share/zeo/{gems,lib}`, assembled by `cargo xtask dist`.
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

/// The resolved home. See the module docs for the shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZeoHome {
    /// The zeo repo: payload dirs and `target/` both live at `root`.
    DevTree { root: PathBuf },
    /// A relocatable install: read-only `payload` (= `<prefix>/share/zeo`),
    /// build output under the per-user `cache`.
    Installed { payload: PathBuf, cache: PathBuf },
    /// A `cargo install`ed zeo: no payload directory anywhere. The gems ride
    /// EMBEDDED in the binary (`gems.pregen.tar.gz`, staged into the crate at
    /// publish time) and extract once into the cache; `libzeo.a` is built once
    /// into the cache through a materialized anchor workspace pinning
    /// `zeo = "=X.Y.Z"` (`backend::link::registry_archive`), because
    /// `cargo install` copies binaries and nothing else. Only constructible
    /// when the binary carries the embedded archive (`zeo_embedded_gems` cfg).
    Registry { cache: PathBuf },
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
fn resolve(
    exe: Option<&Path>,
    env_home: Option<&OsStr>,
    dev_root: &Path,
) -> Result<ZeoHome, String> {
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
             (expected `lib/{}/libzeo.a` beneath it)",
            payload.display(),
            crate::backend::link::host_triple(),
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

    // 4. No payload anywhere, but the binary carries the embedded gems
    //    archive: a `cargo install`ed zeo, self-sufficient through the cache
    //    and the crates.io-fetched runtime.
    if cfg!(zeo_embedded_gems) {
        return Ok(ZeoHome::Registry {
            cache: cache_root(),
        });
    }

    Err(format!(
        "zeo cannot find its runtime payload.\n\
         Probed, in order:\n\
         - ZEO_HOME: not set\n\
         - executable-relative: {}\n\
         - dev tree: {} (no crates/zeo-rt there)\n\
         An installed zeo expects `share/zeo/{{gems,lib}}` next to its \
         `bin/` directory; set ZEO_HOME to point at a payload directory to \
         override.",
        probed
            .map(|p| format!("{} (no lib/<triple>/libzeo.a there)", p.display()))
            .unwrap_or_else(|| "could not determine the executable's path".into()),
        dev_root.display(),
    ))
}

/// The embedded gems archive, staged into the published crate by
/// `cargo xtask stage-publish`. Absent (and the cfg off) in every dev build.
#[cfg(zeo_embedded_gems)]
static EMBEDDED_GEMS: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/gems.pregen.tar.gz"));

/// The bundled-gems dir for a [`ZeoHome::Registry`] zeo: the embedded archive,
/// extracted once per zeo version into the cache. Concurrent first runs race
/// benignly: each extracts into its own temp dir and the `rename` into place
/// is last-writer-wins on a directory that is content-identical either way.
pub fn registry_gems_dir(cache: &Path) -> Option<PathBuf> {
    #[cfg(zeo_embedded_gems)]
    {
        let dest = cache.join(format!("gems-{}", env!("CARGO_PKG_VERSION")));
        if dest.is_dir() {
            return Some(dest);
        }
        let staging = cache.join(format!(
            ".gems-extract-{}-{}",
            env!("CARGO_PKG_VERSION"),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).ok()?;
        let tar = flate2::read::GzDecoder::new(EMBEDDED_GEMS);
        if tar::Archive::new(tar).unpack(&staging).is_err() {
            let _ = std::fs::remove_dir_all(&staging);
            return None;
        }
        if std::fs::rename(&staging, &dest).is_err() {
            // A concurrent extraction won the rename; ours is redundant.
            let _ = std::fs::remove_dir_all(&staging);
        }
        dest.is_dir().then_some(dest)
    }
    #[cfg(not(zeo_embedded_gems))]
    {
        let _ = cache;
        None
    }
}

/// A payload directory is one `cargo xtask dist` laid out. The archive is
/// the load-bearing half -- `zeo -o` links against it and can do nothing
/// without it. `gems/` is optional: its absence just contributes no bundled
/// gems, as in the dev tree.
///
/// It keys on THIS host's triple, so a payload built for another platform
/// reads as no payload at all rather than as one whose every link fails.
fn payload_is_valid(payload: &Path) -> bool {
    payload_archive(payload).is_file()
}

/// Where an installed zeo's `libzeo.a` sits. Keyed by triple so a payload can
/// one day carry two (a mac-to-mac cross ships both Darwin arches).
pub fn payload_archive(payload: &Path) -> PathBuf {
    payload
        .join("lib")
        .join(crate::backend::link::host_triple())
        .join("libzeo.a")
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
        touch(&payload_archive(&payload));
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

    /// A payload staged for a DIFFERENT platform is not a payload here.
    ///
    /// The archive is the whole point of the directory, and one built for
    /// another triple cannot link a thing. Reading it as a valid payload
    /// would turn a wrong-platform download into a link error per compile
    /// instead of one honest message at resolution.
    #[test]
    fn a_payload_for_another_triple_is_not_a_payload() {
        let tmp = tempdir("wrong-triple");
        let payload = tmp.join("share/zeo");
        touch(&payload.join("lib/powerpc-unknown-linux-gnu/libzeo.a"));
        let dev = dev_fixture(&tmp);
        let err = resolve(None, Some(payload.as_os_str()), &dev).unwrap_err();
        assert!(err.contains("not a zeo payload directory"), "{err}");
    }

    /// `gems/` is optional; the archive is not.
    #[test]
    fn a_payload_without_the_archive_is_not_a_payload() {
        let tmp = tempdir("no-archive");
        let payload = tmp.join("share/zeo");
        touch(&payload.join("gems/json/json.gemspec"));
        let dev = dev_fixture(&tmp);
        let err = resolve(None, Some(payload.as_os_str()), &dev).unwrap_err();
        assert!(err.contains("libzeo.a"), "{err}");
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
        let resolved = resolve(Some(&exe), None, &tmp.join("no-repo"));
        if cfg!(zeo_embedded_gems) {
            // With the embedded archive armed (a staged tree), "no payload
            // anywhere" IS the registry tier, not an error.
            assert!(matches!(resolved, Ok(ZeoHome::Registry { .. })));
        } else {
            let err = resolved.unwrap_err();
            assert!(err.contains("executable-relative"), "{err}");
            assert!(err.contains("dev tree"), "{err}");
        }
    }

    #[test]
    fn the_test_harness_itself_runs_in_the_dev_tree() {
        assert!(matches!(zeo_home(), ZeoHome::DevTree { .. }));
    }

    /// Runs only when a staged `gems.pregen.tar.gz` armed the embedded-gems
    /// cfg (i.e. after `cargo xtask stage-publish`): the archive must extract
    /// into a cache dir whose layout IS the bundled-gems dir.
    #[cfg(zeo_embedded_gems)]
    #[test]
    fn embedded_gems_extract_into_the_cache() {
        let cache = tempdir("registry-gems");
        let dir = registry_gems_dir(&cache).expect("extraction succeeds");
        assert!(dir.join("uri/lib/uri.rb").is_file());
        assert!(dir.join("erb/lib/erb.rb").is_file());
        // Second call takes the already-extracted fast path.
        assert_eq!(registry_gems_dir(&cache), Some(dir));
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zeo-home-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
