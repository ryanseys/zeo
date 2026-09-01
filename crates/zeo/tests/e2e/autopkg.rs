//! First-use auto-packaging: a bare linking compile fills the machine
//! package cache with the bundled gems it spliced, and the NEXT compile
//! links the artifacts instead of splicing. Refusals are recorded so no
//! compile retries them, and `ZEO_DEBUG=no-auto-package` keeps the pure
//! source road.
//!
//! Every test pins its own `ZEO_PACKAGE_CACHE`/`ZEO_PROGRAM_CACHE`, so
//! the developer's machine caches are never read or written.

use std::path::{Path, PathBuf};
use std::process::Command;

fn zeo() -> Command {
    Command::new(crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}")))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-autopkg-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run `zeo -e <program>` on the default (cache-first) road with pinned
/// caches and debug logging; answer `(stdout, stderr)`.
fn run_eval(dir: &Path, program: &str) -> (String, String) {
    let out = zeo()
        .arg("--log-level")
        .arg("zeo=debug")
        .arg("-e")
        .arg(program)
        .env("ZEO_PACKAGE_CACHE", dir.join("pkg"))
        .env("ZEO_PROGRAM_CACHE", dir.join("prog"))
        .env_remove("ZEO_DEBUG")
        .env_remove("ZEO_CACHE")
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "expected success\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn first_use_packages_a_bundled_gem_and_the_next_compile_links_it() {
    let dir = scratch("firstuse");
    // Cold: no artifact, the gem splices, and the build+cache loop runs
    // after the program's own success.
    let (out, err) = run_eval(&dir, "require \"abbrev\"; p Abbrev.abbrev([\"ruby\"]).size");
    assert_eq!(out, "4\n");
    assert!(
        err.contains("autopkg: no artifact for 'abbrev', splicing"),
        "the cold run splices: {err}"
    );
    assert!(
        err.contains("autopkg: packaged 'abbrev' for the next compile"),
        "the cold run fills the cache: {err}"
    );
    // A DIFFERENT program (a fresh program-cache key), warm package
    // cache: the artifact links and the gem does not splice.
    let (out, err) = run_eval(&dir, "require \"abbrev\"; p Abbrev.abbrev([\"rub\"]).keys.sort");
    assert_eq!(out, "[\"r\", \"ru\", \"rub\"]\n");
    assert!(
        err.contains("autopkg: linking 'abbrev' from the package cache"),
        "the warm run links the artifact: {err}"
    );
    assert!(
        !err.contains("autopkg: packaged"),
        "nothing rebuilds on a hit: {err}"
    );
}

#[test]
fn a_refused_package_build_is_recorded_and_not_retried() {
    let dir = scratch("refusal");
    // net/protocol's package build refuses today: `Net::ReadTimeout <
    // Timeout::Error` names a foreign gem's class. The refusal must be
    // RECORDED, and the program must still answer from the splice.
    let program = "require \"net/protocol\"; p Net::ReadTimeout.new.message";
    let (out, err) = run_eval(&dir, program);
    assert_eq!(out, "\"Net::ReadTimeout\"\n");
    assert!(
        err.contains("autopkg: the package build for 'net/protocol' refused"),
        "the cold run records the refusal: {err}"
    );
    let refused: Vec<PathBuf> = std::fs::read_dir(dir.join("pkg"))
        .expect("the package cache exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "refused"))
        .collect();
    assert!(!refused.is_empty(), "a refusal sidecar was written");
    // A different program: the recorded refusal answers, no rebuild.
    let (_, err) = run_eval(&dir, "require \"net/protocol\"; p Net::ReadTimeout.new.class.to_s");
    assert!(
        err.contains("autopkg: 'net/protocol' has a recorded refusal, splicing from source"),
        "the warm run skips the retry: {err}"
    );
    assert!(
        !err.contains("autopkg: the package build for 'net/protocol' refused"),
        "the refused build does not run again: {err}"
    );
}

#[test]
fn no_auto_package_keeps_the_pure_source_road() {
    let dir = scratch("optout");
    let out = zeo()
        .arg("-e")
        .arg("require \"abbrev\"; p Abbrev.abbrev([\"x\"]).size")
        .env("ZEO_PACKAGE_CACHE", dir.join("pkg"))
        .env("ZEO_PROGRAM_CACHE", dir.join("prog"))
        .env("ZEO_DEBUG", "no-auto-package")
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("spawn");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    // No entry, no refusal, no object pool: the tier never engaged.
    let entries = std::fs::read_dir(dir.join("pkg"))
        .map(|es| es.count())
        .unwrap_or(0);
    assert_eq!(entries, 0, "the package cache stays untouched");
}

#[test]
fn a_packaged_frame_backtrace_shows_the_real_gem_path() {
    // A package's frame files are baked under the reproducible
    // `/zeopkg/<feature>/` spelling; the merge binds that prefix to the
    // real gem root the resolution recorded, so a backtrace shows the
    // path the spliced world would have baked.
    let dir = scratch("btroot");
    let program = "require \"shellwords\"\nbegin\n  Shellwords.split(\"\\\"\")\nrescue \
                   ArgumentError => e\n  puts e.backtrace.first\nend";
    // Cold run packages shellwords; the warm run links it.
    run_eval(&dir, program);
    let (out, err) = run_eval(&dir, &format!("{program}\n:warm"));
    assert!(
        err.contains("autopkg: linking 'shellwords' from the package cache"),
        "the warm run links the artifact: {err}"
    );
    assert!(
        out.contains("/shellwords.rb:") && out.contains(":in 'block in Shellwords.shellsplit'"),
        "the frame names the gem file: {out}"
    );
    assert!(
        !out.contains("/zeopkg/"),
        "the virtual root is bound to the real path: {out}"
    );
}
