//! The AOT link driver's fixed knowledge: where the prebuilt runtime
//! archive lives and which native system libraries a program link must
//! name. The driver itself (object emission + the `cc` invocation) lands
//! with the `--backend aot` path; the pieces here have their contract
//! pinned now -- the archive location by `runtime_archive`'s presence
//! rule, the library lists by the `natlibs_table_matches_rustc` diff test.

use std::path::PathBuf;

/// `libzeo.a` beside the running `zeo` binary -- the staticlib half of
/// this crate's own build (see `[lib] crate-type` in Cargo.toml), which an
/// installed payload places beside the executable too. Missing means the
/// tree is half-built: the fix is `cargo build`, never shelling cargo from
/// here (the same purity rule `build_binary` keeps).
///
/// No mtime staleness check: the archive and the binary come out of ONE
/// cargo build with the archive written first, so "archive older than the
/// binary" is true of every fresh build -- and a bin-only rebuild leaves an
/// older archive that is still correct (the lib didn't change). Cargo's own
/// dependency tracking is the freshness guarantee in the dev tree; the
/// installed payload gets a content fingerprint at G12.
pub fn runtime_archive() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate the running zeo binary: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "the zeo binary has no parent directory".to_string())?;
    let archive = dir.join("libzeo.a");
    if archive.is_file() {
        return Ok(archive);
    }
    // A cargo TEST binary lives one level deeper, in `<profile>/deps/`,
    // while the archive stays in `<profile>/` -- the e2e harness links from
    // there.
    if dir.file_name().is_some_and(|n| n == "deps")
        && let Some(up) = dir.parent()
    {
        let archive = up.join("libzeo.a");
        if archive.is_file() {
            return Ok(archive);
        }
    }
    Err(format!(
        "runtime archive missing: {} (rerun `cargo build` -- \
         `libzeo.a` is built beside the `zeo` binary)",
        archive.display()
    ))
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
pub(crate) fn host_triple() -> &'static str {
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
/// Whole-archive because linkme `BUILTIN_TABLES` elements live in
/// otherwise-unreferenced members; dead-strip/gc-sections then drops
/// everything unreferenced (the compiler half of an eval-free program
/// included).
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
pub fn link_binary(
    object: &std::path::Path,
    output: &std::path::Path,
    debuginfo: bool,
) -> Result<(), String> {
    let archive = runtime_archive()?;
    let natlibs = natlibs_for(host_triple())?;
    let mut cmd = std::process::Command::new("cc");
    cmd.arg("-o").arg(output).arg(object);
    if cfg!(target_os = "macos") {
        cmd.arg(format!("-Wl,-force_load,{}", archive.display()));
        cmd.args(natlibs);
        cmd.arg("-Wl,-dead_strip");
        if !debuginfo {
            cmd.arg("-Wl,-x");
        }
    } else {
        cmd.arg("-Wl,--whole-archive");
        cmd.arg(&archive);
        cmd.arg("-Wl,--no-whole-archive");
        cmd.args(natlibs);
        cmd.arg("-Wl,--gc-sections");
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
