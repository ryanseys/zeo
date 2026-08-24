//! An INSTALLED zeo compiles, not just runs.
//!
//! `zeo -o` links a program against `libzeo.a`. In the dev tree the archive
//! sits beside the binary and `runtime_archive` finds it there, so every
//! other test in this suite exercises the dev-tree path and none of them can
//! see the installed one.
//!
//! That gap hid a real break for the whole life of `dist`: the staged tree
//! never carried `libzeo.a` at all, so `zeo -o` from a release tarball could
//! not link anything. The smoke test passed regardless because it used `zeo
//! -e`, which takes the JIT path and needs no archive. Nothing else in CI
//! compiles from an install.
//!
//! So this stages a real prefix -- `<prefix>/bin/zeo` plus
//! `<prefix>/share/zeo/lib/<triple>/libzeo.a`, the layout `tools/zeo-dev
//! dist` writes -- and compiles through it.
//!
//! Both halves are asserted. A prefix WITH the archive must compile and run;
//! the same prefix with the archive removed must FAIL, because a payload
//! that cannot link is the bug being gated and "it fell back to something
//! else" would pass a test that proves nothing.

use std::path::{Path, PathBuf};

/// `target/<profile>/`, where both the `zeo` binary and `libzeo.a` are built.
/// The test binary itself lives one level deeper, in `deps/`.
fn profile_dir() -> PathBuf {
    let mut dir = std::env::current_exe().expect("test binary path");
    dir.pop();
    dir.pop();
    dir
}

/// Stage a prefix at `root` and answer its `bin/zeo`.
///
/// HARD LINKS, not copies: the debug archive is ~300 MB and the binary ~85 MB,
/// and a copy per run would dominate the suite. Not symlinks either --
/// `home::resolve` canonicalizes the executable before probing (an installed
/// zeo is routinely reached through one, as Homebrew does), so a symlinked
/// `bin/zeo` would resolve back to `target/<profile>/zeo` and probe
/// `target/share/zeo`, which is not the layout under test. A hard link has no
/// target to resolve, so the probe sees the staged path.
fn stage_prefix(root: &Path, with_archive: bool) -> PathBuf {
    let built = profile_dir();
    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("staging bin/");
    let zeo = bin_dir.join("zeo");
    std::fs::hard_link(built.join("zeo"), &zeo).expect("hard-linking the zeo binary");

    if with_archive {
        let lib = root
            .join("share")
            .join("zeo")
            .join("lib")
            .join(zeo::backend::link::host_triple());
        std::fs::create_dir_all(&lib).expect("staging share/zeo/lib/<triple>/");
        std::fs::hard_link(built.join("libzeo.a"), lib.join("libzeo.a"))
            .expect("hard-linking libzeo.a");
    } else {
        // The payload directory exists and holds no archive -- a tarball
        // staged by the code this replaces.
        std::fs::create_dir_all(root.join("share").join("zeo")).expect("staging share/zeo/");
    }
    zeo
}

/// A scratch directory under `target/`, so the hard links land on the same
/// filesystem as the files they point at.
fn scratch(name: &str) -> PathBuf {
    let dir = profile_dir().join("install-e2e").join(format!(
        "{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creating the scratch dir");
    dir
}

/// The environment an installed zeo must work in: no `ZEO_HOME`, no ambient
/// cargo target dir, and a cache of its own.
fn run(zeo: &Path, args: &[&std::ffi::OsStr], cache: &Path) -> std::process::Output {
    std::process::Command::new(zeo)
        .args(args)
        .env_remove("ZEO_HOME")
        .env_remove("CARGO_TARGET_DIR")
        .env("ZEO_CACHE_DIR", cache)
        .output()
        .expect("running the staged zeo")
}

#[test]
fn an_installed_zeo_compiles_a_program() {
    let dir = scratch("ok");
    let zeo = stage_prefix(&dir.join("prefix"), true);
    let src = dir.join("smoke.rb");
    std::fs::write(&src, "puts 1 + 1\n").expect("writing the program");
    let bin = dir.join("smoke");

    let out = run(
        &zeo,
        &["-o".as_ref(), bin.as_os_str(), src.as_os_str()],
        &dir.join("cache"),
    );
    assert!(
        out.status.success(),
        "`zeo -o` from an installed prefix must link:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ran = std::process::Command::new(&bin)
        .output()
        .expect("running the compiled program");
    assert!(
        ran.status.success(),
        "the compiled program must run:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_payload_without_the_archive_cannot_compile() {
    let dir = scratch("no-archive");
    let zeo = stage_prefix(&dir.join("prefix"), false);
    let src = dir.join("smoke.rb");
    std::fs::write(&src, "puts 1 + 1\n").expect("writing the program");

    let out = run(
        &zeo,
        &[
            "-o".as_ref(),
            dir.join("smoke").as_os_str(),
            src.as_os_str(),
        ],
        &dir.join("cache"),
    );
    assert!(
        !out.status.success(),
        "a payload with no archive must fail loudly, not link something else"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("runtime archive missing"),
        "the failure must name the missing archive, got:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `zeo -e` takes the JIT path, which needs no archive at all. Pinned because
/// it is exactly why the old smoke test passed while `zeo -o` was broken: a
/// green `-e` says nothing about whether an install can compile.
#[test]
fn a_payload_without_the_archive_still_runs_a_program() {
    let dir = scratch("jit");
    let zeo = stage_prefix(&dir.join("prefix"), false);

    let out = run(
        &zeo,
        &["-e".as_ref(), "puts 1 + 1".as_ref()],
        &dir.join("cache"),
    );
    assert!(
        out.status.success(),
        "`zeo -e` needs no archive:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
    let _ = std::fs::remove_dir_all(&dir);
}
