//! Golden-sidecar hygiene for the datatest suites (`tests/`, `tests/gems/`,
//! `tests/spinel/`, `tests/gaps/`).
//!
//! Invariants that have each broken silently before, since none of them fails
//! loudly on its own -- they degrade into non-hermetic or unprotected tests.

use std::path::{Path, PathBuf};

/// Named once because `.config/nextest.toml` must anchor on the same word.
const GEM_GOLDEN_DIR: &str = "gems";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo-tests sits two levels under the workspace root")
}

fn suite_dirs() -> Vec<PathBuf> {
    let root = repo_root().join("tests");
    let mut dirs = vec![
        root.clone(),
        root.join(GEM_GOLDEN_DIR),
        root.join("spinel"),
        root.join("gaps"),
    ];
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

/// A datatest name is the golden's path, and the nextest override that grants
/// these their longer deadline selects on an anchored prefix of it. Renaming
/// the directory makes that pattern match nothing, with no error anywhere.
#[test]
fn the_gem_goldens_still_match_their_nextest_deadline() {
    let dir = repo_root().join("tests").join(GEM_GOLDEN_DIR);
    let count = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "rb"))
        .count();
    assert!(count > 0, "{} holds no goldens", dir.display());

    let config_path = repo_root().join(".config/nextest.toml");
    let config = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", config_path.display()));

    // The `\\/` is the escaped `/` as it appears in the TOML string.
    let anchor = format!("example::{GEM_GOLDEN_DIR}\\\\/");
    assert!(
        config.contains(&anchor),
        "{} no longer anchors the slow-timeout override on `tests/{}` -- \
         all {count} vendored-gem goldens just lost their deadline. \
         Expected a filter containing `{anchor}`.",
        config_path.display(),
        GEM_GOLDEN_DIR,
    );
}
