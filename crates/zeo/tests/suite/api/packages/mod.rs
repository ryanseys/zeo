//! Separate compilation's merge spine.
//!
//! A gem compiles ONCE to its own object plus a row manifest
//! (`--package`); a host program merges the manifest into its one
//! `ProgramDesc` and links the object beside its own
//! (`--with-package`). These tests hold the two contracts the
//! design rests on: the packaged program answers BYTE FOR BYTE what the
//! same program answers with the gem spliced (today's whole-program path),
//! and every refusal names itself rather than mislinking.

mod build;
mod cache;
mod install;
mod with_package;

use std::path::{Path, PathBuf};
use std::process::Command;
fn zeo() -> Command {
    Command::new(crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}")))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-pkg-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn fixture_gem() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/pureleaf")
}

fn run(cmd: &mut Command) -> std::process::Output {
    cmd.env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("spawn")
}

fn ok(cmd: &mut Command) -> String {
    let out = run(cmd);
    assert!(
        out.status.success(),
        "expected success\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Compile the fixture gem as a package into `dir`, answering the object
/// path (the manifest lands beside it as `pureleaf.zman`).
fn build_package(dir: &Path) -> PathBuf {
    let object = dir.join("pureleaf.o");
    ok(zeo()
        .arg("--package")
        .arg("pureleaf")
        .arg("-o")
        .arg(&object)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    assert!(object.is_file(), "the package object was written");
    assert!(
        object.with_extension("zman").is_file(),
        "the manifest was written beside the object"
    );
    object
}

/// Compile any entry file as a package named `feature` into `dir`.
fn build_named_package(dir: &Path, feature: &str, entry: &Path) -> PathBuf {
    let object = dir.join(format!("{feature}.o"));
    ok(zeo()
        .arg("--package")
        .arg(feature)
        .arg("-o")
        .arg(&object)
        .arg(entry)
        .env("ZEO_CACHE", "0"));
    object
}
