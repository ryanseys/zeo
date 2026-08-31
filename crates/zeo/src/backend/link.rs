//! The AOT link driver's fixed knowledge: where the prebuilt runtime
//! archive lives and which native system libraries a program link must
//! name. The driver itself (object emission + the `cc` invocation) lands
//! with the `--backend aot` path; the pieces here have their contract
//! pinned now -- the archive location by `runtime_archive`'s presence
//! rule, the library lists by the `natlibs_table_matches_rustc` diff test.

use std::path::PathBuf;

/// `libzeo.a` -- the staticlib half of this crate's own build (see `[lib]
/// crate-type` in Cargo.toml), found wherever this zeo's install tier put it.
///
/// Four tiers, in the order they are probed: beside the binary (the dev
/// tree), one level up (a cargo TEST binary, which lives in `deps/`), the
/// payload of a release tarball or platform gem, and -- for a `cargo
/// install`ed zeo, which has no archive anywhere -- one built on demand into
/// the cache. In the dev tree, missing means the tree is half-built and the
/// fix is `cargo build`.
///
/// **In a dev tree the archive is brought up to date here, not assumed.**
/// `cargo build -p zeo --bin zeo` builds the binary and NOT the staticlib, so
/// an edited runtime would otherwise be compiled into the `zeo` you just built
/// and left out of every program that `zeo` runs -- including the ones the
/// compiled-program cache links. The failure is silent and reads exactly like
/// an edit that never landed. [`dev_tree_archive`] is the rule; the suites
/// call this function rather than carrying a second copy of it.
pub fn runtime_archive() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate the running zeo binary: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "the zeo binary has no parent directory".to_string())?;
    // A cargo TEST binary lives one level deeper, in `<profile>/deps/`, while
    // the archive stays in `<profile>/`.
    let dir = match dir.file_name().is_some_and(|n| n == "deps") {
        true => dir.parent().unwrap_or(dir),
        false => dir,
    };
    // The dev tree's own build directory, and only that: a zeo STAGED into an
    // install layout resolves to `DevTree` too whenever the source tree still
    // exists on the machine, and cargo cannot build an archive into
    // `<prefix>/bin`. Its missing-payload error has to survive.
    if let crate::home::ZeoHome::DevTree { root } = crate::home::zeo_home()
        && is_cargo_profile_dir(dir, root)
    {
        return dev_tree_archive(dir, root);
    }
    let archive = dir.join("libzeo.a");
    if archive.is_file() {
        return Ok(archive);
    }
    // A `cargo install`ed zeo: nothing put an archive anywhere, so build one
    // once into the cache. See `registry_archive`.
    if let crate::home::ZeoHome::Registry { cache } = crate::home::zeo_home() {
        return registry_archive(cache);
    }
    // An installed zeo: the archive rides in the payload rather than beside
    // the binary, because `<prefix>/bin` is for executables.
    if let crate::home::ZeoHome::Installed { payload, .. } = crate::home::zeo_home() {
        let staged = crate::home::payload_archive(payload);
        if staged.is_file() {
            return Ok(staged);
        }
        return Err(format!(
            "runtime archive missing: {} -- this zeo install cannot compile a \
             program. Reinstall from a release tarball built for {}.",
            staged.display(),
            host_triple()
        ));
    }
    Err(format!(
        "runtime archive missing: {} (rerun `cargo build` -- \
         `libzeo.a` is built beside the `zeo` binary)",
        archive.display()
    ))
}

/// Whether `dir` is cargo's own output directory for this tree --
/// `<root>/target/<profile>` or `<root>/target/<triple>/<profile>`.
///
/// A zeo STAGED into an install layout still resolves to `DevTree` whenever
/// the source tree exists on the machine, and one staged under `target/`
/// passes a plain prefix test. Only cargo's own output directory may be built
/// into: an install prefix has no profile to build for, and its
/// missing-payload error is the answer a broken install needs.
fn is_cargo_profile_dir(dir: &std::path::Path, root: &std::path::Path) -> bool {
    let target = root.join("target");
    dir.parent() == Some(target.as_path())
        || dir.parent().and_then(std::path::Path::parent) == Some(target.as_path())
}

/// The four crates that land in `libzeo.a`. A `.rs` newer than the archive
/// anywhere under their `src/` means the archive no longer holds this tree's
/// runtime.
const ARCHIVE_CRATES: &[&str] = &["zeo", "zeo-rt", "zeo-abi", "zeo-macros"];

/// `libzeo.a` in a dev tree, BUILT when a runtime source is newer than it.
///
/// The staleness rule compares the archive against the sources, not against
/// the `zeo` binary: an archive older than the binary is the normal state of
/// every build, while an archive older than a source file is always wrong.
///
/// Building rather than refusing is what makes the failure impossible instead
/// of merely reported. A dev tree has cargo by definition, cargo does nothing
/// when the archive is already current, and concurrent zeo processes serialize
/// on cargo's own build lock -- so the first builds and the rest find it fresh.
fn dev_tree_archive(dir: &std::path::Path, root: &std::path::Path) -> Result<PathBuf, String> {
    static ONCE: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let archive = dir.join("libzeo.a");
        if let Some(newer) = source_newer_than(&archive, root) {
            build_archive(dir, &newer)?;
        }
        archive.is_file().then_some(archive.clone()).ok_or_else(|| {
            format!(
                "runtime archive missing: {} (rerun `cargo build -p zeo` -- \
                 `libzeo.a` is built beside the `zeo` binary)",
                archive.display()
            )
        })
    })
    .clone()
}

/// The newest source under [`ARCHIVE_CRATES`] that is newer than `archive`, or
/// `None` when the archive is up to date. A missing archive reports the newest
/// source there is, so it builds too.
fn source_newer_than(archive: &std::path::Path, root: &std::path::Path) -> Option<PathBuf> {
    let archive_at = std::fs::metadata(archive).and_then(|m| m.modified()).ok();
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut stack: Vec<PathBuf> = ARCHIVE_CRATES
        .iter()
        .map(|c| root.join("crates").join(c).join("src"))
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
    match archive_at {
        Some(archive_at) if archive_at >= at => None,
        _ => Some(path),
    }
}

/// `cargo build -p zeo --lib` for the profile this binary was built into.
fn build_archive(dir: &std::path::Path, because: &std::path::Path) -> Result<(), String> {
    // `debug` is the `dev` profile's OUTPUT directory, not its name.
    let profile = match dir.file_name().and_then(|n| n.to_str()) {
        Some("debug") | None => "dev",
        Some(other) => other,
    };
    tracing::info!(
        newer = %because.display(),
        "libzeo.a is out of date; building it"
    );
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["build", "-p", "zeo", "--lib", "--profile", profile])
        .current_dir(match crate::home::zeo_home() {
            crate::home::ZeoHome::DevTree { root } => root.clone(),
            _ => PathBuf::from("."),
        })
        .output()
        .map_err(|e| format!("spawning cargo to build libzeo.a: {e}"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(format!(
            "building libzeo.a failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )),
    }
}

/// `libzeo.a` for a `cargo install`ed zeo, built once into the per-user cache.
///
/// `cargo install` copies BINARIES and nothing else. The staticlib is built
/// during the install -- it is a crate-type of the same lib the binary links
/// -- and then discarded with the temporary target directory, so an installed
/// zeo has no archive and cannot link a program at all. crates.io cannot carry
/// one either: the archive is 78 MB against a 10 MB crate limit.
///
/// So it is materialized on first `zeo -o`, through an anchor workspace that
/// depends on this exact `zeo` version. `cargo build -p zeo` inside it builds
/// the dependency's lib target with every crate-type it declares, `libzeo.a`
/// included -- verified, not assumed.
///
/// This is the ONE place zeo shells out to cargo, and it is deliberate: the
/// alternative is an install that silently cannot compile. Every other tier
/// (dev tree, release tarball, platform gem) ships an archive and never
/// reaches here. Nothing is fetched that `cargo install zeo` did not already
/// download, so the build runs offline against the local registry cache.
fn registry_archive(cache: &std::path::Path) -> Result<PathBuf, String> {
    let version = env!("CARGO_PKG_VERSION");
    let dest = cache.join(format!("runtime-{version}")).join("libzeo.a");
    if dest.is_file() {
        return Ok(dest);
    }
    let anchor = cache.join(format!(".runtime-build-{version}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&anchor);
    std::fs::create_dir_all(anchor.join("src"))
        .map_err(|e| format!("creating {}: {e}", anchor.display()))?;
    std::fs::write(
        anchor.join("Cargo.toml"),
        format!(
            "[package]\nname = \"zeo-runtime-anchor\"\nversion = \"0.0.0\"\n\
             edition = \"2021\"\n\n[dependencies]\nzeo = \"={version}\"\n\n[workspace]\n"
        ),
    )
    .map_err(|e| format!("writing the anchor manifest: {e}"))?;
    std::fs::write(anchor.join("src/main.rs"), "fn main() {}\n")
        .map_err(|e| format!("writing the anchor main: {e}"))?;

    // It takes minutes. A compile that looks hung is worse than a slow one.
    eprintln!("zeo: building the runtime archive for {version} (once, a few minutes)...");
    let out = std::process::Command::new("cargo")
        .args(["build", "--release", "-p", "zeo"])
        .current_dir(&anchor)
        .output()
        .map_err(|e| {
            format!(
                "zeo was installed with `cargo install`, which ships no runtime \
                 archive, and building one needs cargo on PATH: {e}"
            )
        })?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&anchor);
        return Err(format!(
            "building the runtime archive failed:\n{}",
            link_diagnostics(&String::from_utf8_lossy(&out.stderr))
        ));
    }
    let built = anchor.join("target").join("release").join("libzeo.a");
    if !built.is_file() {
        let _ = std::fs::remove_dir_all(&anchor);
        return Err(format!(
            "the runtime build produced no {} -- this is a zeo bug",
            built.display()
        ));
    }
    // Publish through a rename so a concurrent first run never reads a
    // half-copied archive; last writer wins on identical content.
    let dir = dest
        .parent()
        .expect("the destination always has a parent directory");
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let staging = dir.join(format!(".libzeo.a.{}", std::process::id()));
    std::fs::copy(&built, &staging).map_err(|e| format!("staging the archive: {e}"))?;
    std::fs::rename(&staging, &dest).map_err(|e| format!("publishing the archive: {e}"))?;
    let _ = std::fs::remove_dir_all(&anchor);
    Ok(dest)
}

/// What `rustc --print=native-static-libs` reports for the `zeo` staticlib
/// on each target -- the system libraries the final `cc` link must name
/// AFTER the archive. Checked against the live answer by the ignored diff
/// test below (a CI leg: it compiles the lib in its own target dir).
const NATLIBS_MACOS: &[&str] = &["-liconv", "-lSystem", "-lc", "-lm"];
/// What rustc reports on glibc, VERBATIM -- captured live by the diff test
/// below on 2026-08-20, replacing a list seeded from documentation that had
/// never been checked on the platform it describes.
///
/// The head of it is not std's: `-lcrypt` and the first `-lutil` come from
/// zeo-rt's own `#[link(name = ..)]` attributes (`String#crypt`, `PTY`),
/// which rustc honours for its own link and reports here, but which nothing
/// tells a hand-written table about -- so the AOT link on Linux failed with
/// `undefined reference to 'crypt'` while every macOS run stayed green
/// (libSystem carries both). `-lutil` appearing twice is rustc's own output
/// and is kept: the assert is equality with what rustc says, and a
/// de-duplicated list would fail it while changing nothing about the link.
///
/// **Adding a `#[link(name = ..)]` anywhere in zeo-rt means adding it here.**
/// The diff test is what catches a miss, and it only runs on Linux.
const NATLIBS_LINUX_GNU: &[&str] = &[
    "-lcrypt",
    "-lutil",
    "-lgcc_s",
    "-lutil",
    "-lrt",
    "-lpthread",
    "-lm",
    "-ldl",
    "-lc",
];
/// musl's self-contained mode needs nothing beyond the archive.
const NATLIBS_LINUX_MUSL: &[&str] = &[];

/// The native-library tail of a link line for `triple`.
pub fn natlibs_for(triple: &str) -> Result<&'static [&'static str], String> {
    if triple.contains("apple-darwin") {
        Ok(NATLIBS_MACOS)
    } else if triple.contains("linux-musl") {
        Ok(NATLIBS_LINUX_MUSL)
    } else if triple.contains("linux-gnu") {
        Ok(NATLIBS_LINUX_GNU)
    } else {
        Err(format!("no native-library table for target {triple}"))
    }
}

/// The host's target triple, for the natlibs table and the link shape.
pub fn host_triple() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "aarch64",
        target_env = "gnu"
    )) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "aarch64",
        target_env = "musl"
    )) {
        "aarch64-unknown-linux-musl"
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "musl"
    )) {
        "x86_64-unknown-linux-musl"
    } else {
        panic!("no host-triple mapping for this platform")
    }
}

/// Link one emitted object against `libzeo.a` into `output`, through the
/// system `cc` (the same external-tool requirement rustc's link step has).
/// Whole-archive so every member is a candidate, since a class table is
/// reached by name and nothing else in the archive references it;
/// dead-strip/gc-sections then drops everything unreferenced (the compiler
/// half of an eval-free program included).
///
/// `-x` discards the local symbol table. Nothing reads it: a Ruby
/// backtrace is built from zeo's own frame stack, and the JIT resolves
/// the runtime through an in-process pointer table rather than by name.
/// It is what the rustc path got from `-C strip=symbols`, and without it
/// an eval-free `hello` carries 2 MB of Rust symbol names.
///
/// `debuginfo` is exactly when those symbols ARE read: the Mach-O debug
/// map is a set of local stab entries naming `object`, so `-x` would
/// throw away the only pointer to the DWARF.
/// `loads_cext` publishes the C API. An extension's `.so` leaves every `rb_*`
/// undefined and resolves it against the host at load, and two separate
/// things would otherwise stop that: `-dead_strip` prunes what nothing calls,
/// and a Mach-O export table holds only what was asked for. `-export_dynamic`
/// answers both -- an exported symbol is a dead-strip root.
///
/// It is a flag rather than always on because the export table is exactly
/// what `-dead_strip` prunes against: exporting unconditionally would keep
/// the whole runtime in every hello-world.
pub fn link_binary(
    object: &std::path::Path,
    extra_objects: &[std::path::PathBuf],
    output: &std::path::Path,
    debuginfo: bool,
    loads_cext: bool,
) -> Result<(), String> {
    let archive = runtime_archive()?;
    let natlibs = natlibs_for(host_triple())?;
    let mut cmd = std::process::Command::new("cc");
    cmd.arg("-o").arg(output).arg(object);
    // EXPERIMENTAL (M0): separately compiled package objects, before the
    // runtime archive so their imports resolve the same way the program's do.
    cmd.args(extra_objects);
    if cfg!(target_os = "macos") {
        cmd.arg(format!("-Wl,-force_load,{}", archive.display()));
        cmd.args(natlibs);
        cmd.arg("-Wl,-dead_strip");
        if loads_cext {
            cmd.arg("-Wl,-export_dynamic");
        }
        if !debuginfo {
            cmd.arg("-Wl,-x");
        }
    } else {
        cmd.arg("-Wl,--whole-archive");
        cmd.arg(&archive);
        cmd.arg("-Wl,--no-whole-archive");
        cmd.args(natlibs);
        cmd.arg("-Wl,--gc-sections");
        if loads_cext {
            cmd.arg("-Wl,--export-dynamic");
        }
        if !debuginfo {
            cmd.arg("-Wl,-x");
        }
    }
    let out = cmd
        .output()
        .map_err(|e| format!("running cc to link {}: {e}", output.display()))?;
    if !out.status.success() {
        return Err(format!(
            "linking {} failed:\n{}",
            output.display(),
            link_diagnostics(&String::from_utf8_lossy(&out.stderr))
        ));
    }
    Ok(())
}

/// A failed link's stderr, without the linker's warnings. A macOS link of
/// `libzeo.a` emits one `built for newer 'macOS' version` line per vendored
/// C object plus a duplicate-`-lSystem` line -- fifty lines of noise that
/// push the error itself out of any bounded report.
fn link_diagnostics(stderr: &str) -> String {
    let kept: Vec<&str> = stderr
        .lines()
        .filter(|l| !l.trim_start().starts_with("ld: warning:"))
        .collect();
    if kept.iter().all(|l| l.trim().is_empty()) {
        return stderr.to_string();
    }
    kept.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cached archive is returned as-is. The expensive path shells out to
    /// cargo, so "already built" has to be decided before anything else --
    /// this is what stops every `zeo -o` on an installed zeo taking minutes.
    #[test]
    fn a_cached_registry_archive_is_reused() {
        let cache = std::env::temp_dir().join(format!("zeo-regcache-{}", std::process::id()));
        let dir = cache.join(format!("runtime-{}", env!("CARGO_PKG_VERSION")));
        std::fs::create_dir_all(&dir).expect("creating the fake cache");
        let archive = dir.join("libzeo.a");
        std::fs::write(&archive, b"not really an archive").expect("writing the fake archive");
        assert_eq!(
            registry_archive(&cache).expect("a cached archive needs no build"),
            archive
        );
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// `cargo build -p zeo` from a package that merely DEPENDS on zeo builds
    /// the dependency's lib target with every crate-type it declares, so
    /// `libzeo.a` appears. `registry_archive` stands on exactly that, and
    /// nothing in cargo's documented behaviour promises it.
    ///
    /// Ignored by default: it is a full release build of the workspace in a
    /// target dir of its own, minutes rather than milliseconds. It uses a
    /// PATH dependency because the real one resolves `zeo = "=X.Y.Z"` from
    /// crates.io, which cannot be exercised before the version is published.
    #[test]
    #[ignore = "CI leg: a release build in a probe target dir"]
    fn a_dependency_position_zeo_still_builds_the_staticlib() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let anchor = std::env::temp_dir().join(format!("zeo-anchor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&anchor);
        std::fs::create_dir_all(anchor.join("src")).expect("creating the anchor");
        std::fs::write(
            anchor.join("Cargo.toml"),
            format!(
                "[package]\nname = \"zeo-anchor-probe\"\nversion = \"0.0.0\"\n\
                 edition = \"2021\"\n\n[dependencies]\nzeo = {{ path = {:?} }}\n\n\
                 [workspace]\n",
                root
            ),
        )
        .expect("writing the anchor manifest");
        std::fs::write(anchor.join("src/main.rs"), "fn main() {}\n").expect("writing main");
        let out = std::process::Command::new(env!("CARGO"))
            .args(["build", "--release", "-p", "zeo"])
            .current_dir(&anchor)
            .output()
            .expect("cargo must run");
        assert!(
            out.status.success(),
            "the anchor build failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let built = anchor.join("target").join("release").join("libzeo.a");
        assert!(
            built.is_file(),
            "a dependency-position zeo built no {} -- `registry_archive` cannot work",
            built.display()
        );
        let _ = std::fs::remove_dir_all(&anchor);
    }

    #[test]
    fn every_supported_triple_has_a_table() {
        for t in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
        ] {
            natlibs_for(t).unwrap();
        }
        natlibs_for("wasm32-unknown-unknown").unwrap_err();
    }

    /// Where the probe build writes. Beside the ambient target dir when
    /// there is one, because the Linux leg mounts the repo READ-ONLY --
    /// `<workspace>/target` is not writable there, and the probe must not
    /// share the ambient dir either (it would churn the real build's
    /// fingerprints).
    fn natlibs_probe_dir(workspace: &std::path::Path) -> PathBuf {
        match std::env::var_os("CARGO_TARGET_DIR") {
            Some(dir) => PathBuf::from(dir).join("natlibs-probe"),
            None => workspace.join("target/natlibs-probe"),
        }
    }

    /// The CI diff leg: asks rustc for the live answer and compares it to
    /// the table for this host. Compiles the `zeo` lib in its own target
    /// dir so the main tree's fingerprints stay put -- expensive on a cold
    /// cache, which is why it is ignored by default.
    #[test]
    #[ignore = "CI leg: compiles the zeo lib in a probe target dir to diff the natlibs table"]
    fn natlibs_table_matches_rustc() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest.parent().unwrap().parent().unwrap();
        let out = std::process::Command::new(env!("CARGO"))
            .args([
                "rustc",
                "-p",
                "zeo",
                "--lib",
                "--",
                "--print=native-static-libs",
            ])
            .current_dir(workspace)
            .env("CARGO_TARGET_DIR", natlibs_probe_dir(workspace))
            .output()
            .expect("cargo rustc must run");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "cargo rustc failed:\n{stderr}");
        let line = stderr
            .lines()
            .find_map(|l| l.split("native-static-libs:").nth(1))
            .expect("rustc must print the native-static-libs note")
            .trim();
        let live: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(live, natlibs_for(host_triple()).unwrap());
    }
}

#[cfg(test)]
mod archive_freshness_tests {
    use std::path::PathBuf;

    /// A scratch tree named for this test, removed on the way in so a killed
    /// run cannot leave one behind that the next run reads.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zeo-archive-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// The rule that makes a stale `libzeo.a` impossible: a runtime source
    /// newer than the archive reports that file, and an archive newer than
    /// every source reports nothing.
    ///
    /// The regression is silent and expensive. `cargo build -p zeo --bin zeo`
    /// builds the binary and NOT the staticlib, so an edited runtime reached
    /// the compiler and not the programs the compiler ran -- including every
    /// one served from the compiled-program cache. It reads exactly like an
    /// edit that never landed, and it cost a long session to diagnose once.
    #[test]
    fn a_source_newer_than_the_archive_is_reported() {
        let root = scratch("newer");
        let src = root.join("crates/zeo-rt/src/deep");
        std::fs::create_dir_all(&src).expect("the crate tree");
        let archive = root.join("libzeo.a");
        std::fs::write(&archive, b"archive").expect("the archive");

        // No sources at all: nothing can be newer.
        assert!(super::source_newer_than(&archive, &root).is_none());

        // A source written AFTER the archive is reported by name, and the
        // walk reaches a nested directory rather than only the top one.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = src.join("payload.rs");
        std::fs::write(&newer, b"// edited").expect("the source");
        assert_eq!(
            super::source_newer_than(&archive, &root).as_deref(),
            Some(newer.as_path())
        );

        // Rebuilding the archive clears it.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&archive, b"rebuilt").expect("the archive");
        assert!(super::source_newer_than(&archive, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A MISSING archive reports the newest source there is, so the caller
    /// builds one rather than reporting it as up to date.
    #[test]
    fn a_missing_archive_asks_to_be_built() {
        let root = scratch("missing");
        let src = root.join("crates/zeo-abi/src");
        std::fs::create_dir_all(&src).expect("the crate tree");
        std::fs::write(src.join("lib.rs"), b"// a source").expect("the source");
        let missing = root.join("libzeo.a");
        assert!(super::source_newer_than(&missing, &root).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The four crates that land in the archive are the four the walk reads.
    /// A fifth crate joining `libzeo.a` without joining this list would make
    /// the staleness check blind to it.
    #[test]
    fn the_walk_covers_the_crates_the_archive_holds() {
        assert_eq!(
            super::ARCHIVE_CRATES,
            &["zeo", "zeo-rt", "zeo-abi", "zeo-macros"]
        );
    }
}
