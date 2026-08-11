//! Golden-sidecar hygiene for the datatest suites (`tests/`, `tests/spinel/`,
//! `tests/gaps/`).
//!
//! Invariants that have each broken silently before, since none of them fails
//! loudly on its own -- they degrade into non-hermetic or unprotected tests.

use std::path::{Path, PathBuf};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo-tests sits two levels under the workspace root")
}

fn suite_dirs() -> Vec<PathBuf> {
    let root = repo_root().join("tests");
    let mut dirs = vec![root.clone(), root.join("spinel"), root.join("gaps")];
    // One subdirectory per gem under tests/gemtests/.
    if let Ok(entries) = std::fs::read_dir(root.join("gemtests")) {
        dirs.extend(
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir()),
        );
    }
    dirs
}

fn goldens() -> impl Iterator<Item = PathBuf> {
    suite_dirs().into_iter().flat_map(|dir| {
        std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".expected"))
            .collect::<Vec<_>>()
    })
}

#[test]
fn every_golden_uses_the_rb_sidecar_convention() {
    let misnamed: Vec<String> = goldens()
        .filter(|p| {
            let name = p.to_string_lossy();
            !name.ends_with(".rb.expected") && !name.ends_with(".rb.err.expected")
        })
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        misnamed.is_empty(),
        "goldens the harness will never find (rename to <stem>.rb.expected):\n{}",
        misnamed.join("\n")
    );
}

#[test]
fn no_golden_embeds_a_machine_specific_path() {
    // `/home/user` is a fixture constant some tests PRINT (a stubbed HOME),
    // so only genuinely machine-specific prefixes are flagged.
    const MARKERS: &[&str] = &["/Users/", ".local/share/mise"];
    let mut offenders = Vec::new();
    for p in goldens() {
        let content = std::fs::read_to_string(&p).unwrap_or_default();
        for m in MARKERS {
            if content.contains(m) {
                offenders.push(format!("{}: contains {m:?}", p.display()));
                break;
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "goldens with unportable absolute paths (re-record or scrub in-test):\n{}",
        offenders.join("\n")
    );
}

/// `tests/bench/` holds compile-bench INPUT programs (whole-gem require
/// graphs), deliberately outside every datatest pattern: a golden appearing
/// there would silently never run, and a bench input gaining a golden means
/// someone thinks it is a test again -- whole-gem coverage belongs to the gem
/// probe, which is why `tests/gems/` was retired.
#[test]
fn bench_inputs_carry_no_goldens() {
    let dir = repo_root().join("tests").join("bench");
    let stray: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".expected"))
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        stray.is_empty(),
        "tests/bench/ is not a datatest suite; these goldens would never run:\n{}",
        stray.join("\n")
    );
}
