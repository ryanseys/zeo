//! What a linked binary costs, defended as a number rather than as prose.
//!
//! `#[ignore]`d: it builds the RELEASE compiler and links a program, which is
//! minutes rather than milliseconds. `cargo nextest run -P full` runs it.
//!
//! `docs/explanation/binary-size.md` explains what dominates the number and why.

use std::path::PathBuf;
use std::process::Command;

use crate::paths::workspace_root;

/// The committed size of `puts 1`, in bytes of the whole binary. Measured
/// 2026-09-01 on aarch64-apple-darwin, release profile.
const SIZE_BASELINE: u64 = 6_512_472;

/// Allowed drift. Wide enough that a toolchain bump does not fail the gate,
/// narrow enough that a table joining the always-on set does.
const TOLERANCE: u64 = 262_144;

const PROGRAM: &str = "puts 1\n";

fn release_zeo() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["build", "--release", "-p", "zeo"])
        .current_dir(workspace_root())
        .output()
        .expect("spawning cargo");
    assert!(
        out.status.success(),
        "cargo build --release -p zeo failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exe = crate::paths::profile_dir()
        .expect("the test binary has a profile directory")
        .parent()
        .expect("target/")
        .join("release")
        .join("zeo");
    assert!(
        exe.is_file(),
        "{} is missing after the build",
        exe.display()
    );
    exe
}

/// A scratch directory of this test's own, under the system temp dir and
/// named by pid so two runs cannot collide.
fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("{SCRATCH_PREFIX}{}", std::process::id()))
}

const SCRATCH_PREFIX: &str = "zeo-size-";

/// Remove the scratch tree, refusing any path this test did not mint.
///
/// The argument is computed rather than handed over, so the guard is what
/// stands between a bad `TMPDIR` and a recursive delete somewhere real. A
/// directory that is already gone is success; anything else is reported.
fn remove_scratch(dir: &std::path::Path) {
    assert!(
        dir.starts_with(std::env::temp_dir())
            && dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(SCRATCH_PREFIX)),
        "refusing to delete {} -- not a scratch directory this test made",
        dir.display()
    );
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("removing {}: {e}", dir.display()),
    }
}

#[test]
#[ignore = "builds the release compiler and links a program"]
fn a_linked_binary_stays_within_the_size_baseline() {
    let scratch = scratch_dir();
    remove_scratch(&scratch);
    std::fs::create_dir_all(&scratch)
        .unwrap_or_else(|e| panic!("creating {}: {e}", scratch.display()));
    let src = scratch.join("hello.rb");
    std::fs::write(&src, PROGRAM).expect("writing the program");
    let out = scratch.join("hello");

    let status = Command::new(release_zeo())
        .arg("-o")
        .arg(&out)
        .arg(&src)
        .current_dir(workspace_root())
        .status()
        .expect("spawning zeo");
    assert!(status.success(), "`zeo -o` did not link `puts 1`");

    let bytes = std::fs::metadata(&out).expect("the linked binary").len();
    remove_scratch(&scratch);
    let drift = bytes.abs_diff(SIZE_BASELINE);
    assert!(
        drift <= TOLERANCE,
        "`puts 1` links to {bytes} bytes, {drift} from the {SIZE_BASELINE} baseline \
         (tolerance {TOLERANCE}). If this is intended, move SIZE_BASELINE in \
         crates/zeo/tests/suite/checks/binary_size.rs and say why in the commit."
    );
}
