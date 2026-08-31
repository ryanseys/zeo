//! Promote a FIXED gap out of `tests/gaps/` into the passing suite.
//!
//! When a gap starts matching ruby, the gaps harness fails it with "GAP FIXED
//! -- promote". This moves the gap's `.rb` and every sidecar into `tests/` --
//! the zeo-authored golden suite, the `examples` target -- and confirms it
//! passes there.
//!
//! `tests/spinel/` is NOT a promotion target: it mirrors a vendored corpus,
//! and a spinel-origin gap belongs in `tests/` like any other.

use crate::exec::{self, Capture};
use crate::{Error, root, root_join};

/// EVERY suffix the harness recognizes (golden.rs is the reference).
///
/// This list once knew only five of them, and promoting a gap that carried a
/// `.gccheck`, `.gc` or `.leakcheck` silently left the sidecar behind in
/// tests/gaps/ -- changing the promoted test's behavior and orphaning a file.
///
/// What a golden IS -- a divergence, macOS-only, JIT-only -- is its DIRECTORY,
/// not a suffix, so a promotion that changes the kind is a move to a different
/// directory and nothing here has to know about it.
const SIDECARS: &[&str] = &[
    "rb",
    "rb.expected",
    "rb.err.expected",
    "rb.linux.expected",
    "rb.linux.err.expected",
    "rb.args",
    "rb.stdin",
    "rb.gc",
    "rb.leakcheck",
    "rb.gccheck",
    // Names the extension directory both engines build. It reads against the
    // TESTS ROOT rather than the `.rb`'s own, so a promotion moves the file
    // and the name still points at the same fixture.
    "rb.cext",
];

const USAGE: &str = "usage: cargo xtask promote-gap <gap-stem>";

pub fn run(args: &[String]) -> Result<(), Error> {
    let Some(stem) = args.first() else {
        return Err(Error::new(USAGE.to_string()));
    };
    if stem == "--help" || stem == "-h" {
        println!("{USAGE}");
        return Ok(());
    }
    let stem = stem.trim_end_matches(".rb");
    let gaps = root_join("tests/gaps");
    let dest = root_join("tests");
    if !gaps.join(format!("{stem}.rb")).is_file() {
        return Err(Error::new(format!("no such gap: tests/gaps/{stem}.rb")));
    }

    let mut moved = Vec::new();
    for suffix in SIDECARS {
        let from = gaps.join(format!("{stem}.{suffix}"));
        if !from.is_file() {
            continue;
        }
        let to = dest.join(format!("{stem}.{suffix}"));
        std::fs::rename(&from, &to).map_err(|e| {
            Error::new(format!(
                "moving {} to {}: {e}",
                from.display(),
                to.display()
            ))
        })?;
        moved.push(format!("{stem}.{suffix}"));
    }
    println!(
        "promoted tests/gaps/{stem}.rb -> tests/ ({} files: {})",
        moved.len(),
        moved.join(" ")
    );

    // `--test goldens`, which is the ONE test target: every suite the harness
    // declares -- example, gap, spinel and the rest -- is a case inside it.
    // This named `--test examples` for as long as a separate binary by that
    // name existed, and went on naming it after the suites were merged, so
    // the verification step failed to build rather than failing to pass.
    println!("verifying it passes in the examples suite ...");
    let out = exec::run(
        &[
            "cargo",
            "nextest",
            "run",
            "-p",
            "zeo",
            "--test",
            "goldens",
            "-E",
            &format!("test(example::{stem})"),
        ],
        root(),
        &[],
        Capture::Nothing,
    )?;
    if !out.success() {
        return Err(Error::new(format!(
            "{stem} does not pass in the examples suite -- it is still a gap"
        )));
    }
    Ok(())
}
