//! Golden-sidecar hygiene for the datatest suites (`tests/`, `tests/gems/`,
//! `tests/spinel/`, `tests/gaps/`).
//!
//! Two invariants, each of which has silently broken before:
//!
//! - Every `*.expected` must be named `<stem>.rb.expected` or
//!   `<stem>.rb.err.expected`. The harness resolves sidecars by suffix-append
//!   (`golden.rs::sidecars`), so a `<stem>.expected` is never found -- the
//!   test silently falls back to a live ruby-oracle run every time
//!   (non-hermetic, and an invisible drift channel).
//! - No golden may embed a machine-specific absolute path. The normalizer
//!   rewrites only the test SOURCE path, so a home-directory or mise-install
//!   path recorded into a golden can never match on another machine.

use std::path::{Path, PathBuf};

fn suite_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo has a workspace root")
        .join("tests");
    vec![
        root.clone(),
        root.join("gems"),
        root.join("spinel"),
        root.join("gaps"),
    ]
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
