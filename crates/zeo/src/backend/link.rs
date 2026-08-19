//! The AOT link driver's fixed knowledge: where the prebuilt runtime
//! archive lives and which native system libraries a program link must
//! name. The driver itself (object emission + the `cc` invocation) lands
//! with the `--backend aot` path; the pieces here have their contract
//! pinned now -- the archive location by `runtime_archive`'s staleness
//! rule, the library lists by the `natlibs_table_matches_rustc` diff test.

use std::path::PathBuf;

/// `libzeo.a` beside the running `zeo` binary -- the staticlib half of
/// this crate's own build (see `[lib] crate-type` in Cargo.toml), which an
/// installed payload places beside the executable too. Missing or older
/// than the binary means the developer's tree is half-built: the fix is
/// `cargo build`, never shelling cargo from here (the same purity rule
/// `build_binary` keeps).
pub fn runtime_archive() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate the running zeo binary: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "the zeo binary has no parent directory".to_string())?;
    let archive = dir.join("libzeo.a");
    let meta = std::fs::metadata(&archive).map_err(|_| {
        format!(
            "runtime archive missing: {} (rerun `cargo build` -- \
             `libzeo.a` is built beside the `zeo` binary)",
            archive.display()
        )
    })?;
    let exe_meta =
        std::fs::metadata(&exe).map_err(|e| format!("cannot stat the running zeo binary: {e}"))?;
    if let (Ok(archive_time), Ok(exe_time)) = (meta.modified(), exe_meta.modified())
        && archive_time < exe_time
    {
        return Err(format!(
            "runtime archive is older than the zeo binary: {} (rerun `cargo build`)",
            archive.display()
        ));
    }
    Ok(archive)
}

/// What `rustc --print=native-static-libs` reports for the `zeo` staticlib
/// on each target -- the system libraries the final `cc` link must name
/// AFTER the archive. Checked against the live answer by the ignored diff
/// test below (a CI leg: it compiles the lib in its own target dir).
const NATLIBS_MACOS: &[&str] = &["-liconv", "-lSystem", "-lc", "-lm"];
/// The glibc set rustc documents for static linking; confirmed per-host by
/// the same diff test when it runs on Linux (the podman loop / CI).
const NATLIBS_LINUX_GNU: &[&str] = &[
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

#[cfg(test)]
mod tests {
    use super::*;

    fn host_triple() -> &'static str {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "aarch64-apple-darwin"
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            "x86_64-apple-darwin"
        } else if cfg!(all(target_os = "linux", target_env = "gnu")) {
            "x86_64-unknown-linux-gnu"
        } else if cfg!(all(target_os = "linux", target_env = "musl")) {
            "x86_64-unknown-linux-musl"
        } else {
            panic!("no host-triple mapping for this platform")
        }
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
            .env("CARGO_TARGET_DIR", workspace.join("target/natlibs-probe"))
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
