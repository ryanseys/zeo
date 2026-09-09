//! Every environment variable the docs name has a reader in the tree.
//!
//! A documented variable with no reader is a silent no-op: the reader was
//! renamed or deleted and the instruction outlived it, and a doc that names
//! the stale spelling sends the reader to a switch that does nothing at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Spellings the docs mention only to say they do not work. A variable named
/// to say it does NOT work is not a broken instruction.
const HISTORICAL: &[&str] = &["ZEO_BLESS", "ZEO_BLESS_FROM_TOOL", "ZEO_GOLDEN_DIFF_TYPED"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the workspace root")
        .to_path_buf()
}

fn files_under(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, exts, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
        {
            out.push(path);
        }
    }
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Every whole word in `bytes` that starts with `prefix`. Read as bytes
/// because a `.tsv` in the corpus can hold sequences that are not UTF-8.
fn names_with(bytes: &[u8], prefix: &[u8]) -> Vec<(usize, String)> {
    let n = prefix.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= bytes.len() {
        let Some(at) = bytes[i..].windows(n).position(|w| w == prefix) else {
            break;
        };
        let start = i + at;
        // `MYZEO_X` is not a mention of `ZEO_X`.
        if start > 0 && is_word(bytes[start - 1]) {
            i = start + n;
            continue;
        }
        let end = start + bytes[start..].iter().take_while(|&&b| is_word(b)).count();
        out.push((
            start,
            String::from_utf8_lossy(&bytes[start..end]).into_owned(),
        ));
        i = end;
    }
    out
}

fn names(bytes: &[u8]) -> Vec<(usize, String)> {
    names_with(bytes, b"ZEO_")
}

fn collect(root: &Path, dirs: &[&str], loose: &[&str], exts: &[&str]) -> Vec<(PathBuf, Vec<u8>)> {
    let mut paths: Vec<PathBuf> = loose.iter().map(|f| root.join(f)).collect();
    for dir in dirs {
        files_under(&root.join(dir), exts, &mut paths);
    }
    paths
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (p, bytes)
        })
        .collect()
}

#[test]
fn every_documented_env_var_has_a_reader() {
    let root = repo_root();

    let documented: BTreeSet<String> = collect(
        &root,
        &["docs", "test"],
        &["README.md", "CONTRIBUTING.md"],
        &["md", "tsv"],
    )
    .iter()
    .flat_map(|(_, b)| names(b).into_iter().map(|(_, n)| n))
    .collect();

    // A reader spells the name as a string literal: `env::var("X")`,
    // `var_os("X")`, `ENV["X"]` -- in Rust, in Ruby, or in a shim template.
    let readers: BTreeSet<String> = collect(&root, &["crates", "tools"], &[], &["rs", "rb", "in"])
        .iter()
        .flat_map(|(_, b)| {
            names(b)
                .into_iter()
                .filter(|(at, _)| *at > 0 && b[at - 1] == b'"')
                .map(|(_, n)| n)
        })
        .collect();

    // A floor, not a target: a scan that silently reads nothing would make
    // this test pass forever. The docs name 57 today.
    assert!(
        documented.len() >= 15,
        "only {} ZEO_* names found in the docs -- the scan is not reading them",
        documented.len()
    );
    let missing: Vec<&String> = documented
        .iter()
        .filter(|v| !HISTORICAL.contains(&v.as_str()))
        .filter(|v| !readers.contains(*v))
        .collect();
    assert!(
        missing.is_empty(),
        "documented environment variables with no reader: {missing:?}\n\
         Either restore the reader, correct the spelling, or add it to HISTORICAL."
    );
}

/// Names a reader spells that no page needs to: unit-test fixtures, and the
/// shim placeholders `build.rs` fills.
const UNDOCUMENTED_BY_DESIGN: &[&str] = &[];

#[test]
fn every_env_var_read_is_documented() {
    let root = repo_root();

    let documented: BTreeSet<String> = collect(
        &root,
        &["docs"],
        &["README.md", "CONTRIBUTING.md"],
        &["md"],
    )
    .iter()
    .flat_map(|(_, b)| names(b).into_iter().map(|(_, n)| n))
    .collect();

    // A reader outside a test: `env::var("X")` in `src/`, `ext/` or the
    // shim templates. Tests set what they read, and xtask documents its own
    // switches in its usage text.
    let readers: BTreeSet<String> = collect(
        &root,
        &["crates/zeo/src", "crates/zeo-rt/src", "crates/zeo-rt/ext", "crates/zeo-capi/src", "crates/xtask/src"],
        &[],
        &["rs", "rb", "in"],
    )
    .iter()
    .flat_map(|(_, b)| {
        names(b)
            .into_iter()
            .filter(|(at, _)| *at > 0 && b[at - 1] == b'"')
            .map(|(_, n)| n)
    })
    .collect();

    assert!(readers.len() >= 20, "only {} ZEO_* readers found -- the scan is not reading them", readers.len());
    let missing: Vec<&String> = readers
        .iter()
        .filter(|v| !v.starts_with("ZEO_TEST_"))
        .filter(|v| !UNDOCUMENTED_BY_DESIGN.contains(&v.as_str()))
        .filter(|v| !documented.contains(*v))
        .collect();
    assert!(
        missing.is_empty(),
        "environment variables the tree reads that no page documents: {missing:?}\n\
         Add a row to docs/reference/environment-variables.md."
    );
}

/// `RUBY_*` names ruby itself owns, which the runtime honours and the docs
/// must therefore state. These are the ones a user already knows from CRuby,
/// so a reader who does not find them assumes zeo ignores them.
///
/// The scan looks only at string literals in the runtime, which is where a
/// read has to be spelled; `RUBY_VERSION` and the other CONSTANTS are Ruby
/// identifiers, not environment reads, and are skipped by name.
const RUBY_NOT_ENV: &[&str] = &[
    "RUBY_VERSION",
    "RUBY_PLATFORM",
    "RUBY_ENGINE",
    "RUBY_ENGINE_VERSION",
    "RUBY_PATCHLEVEL",
    "RUBY_RELEASE_DATE",
    "RUBY_REVISION",
    "RUBY_COPYRIGHT",
    "RUBY_DESCRIPTION",
    "RUBY_BOX_CLASS",
    "RUBY_BOX_ENTRY_CLASS",
    "RUBY_BOX_LOADER_MODULE",
];

#[test]
fn every_ruby_env_var_the_runtime_reads_is_documented() {
    let root = repo_root();

    let documented: BTreeSet<String> = collect(
        &root,
        &["docs"],
        &["README.md", "CONTRIBUTING.md"],
        &["md"],
    )
    .iter()
    .flat_map(|(_, b)| names_with(b, b"RUBY_").into_iter().map(|(_, n)| n))
    .collect();

    let readers: BTreeSet<String> = collect(
        &root,
        &["crates/zeo-rt/src", "crates/zeo-rt/ext", "crates/zeo/src"],
        &[],
        &["rs"],
    )
    .iter()
    .flat_map(|(_, b)| {
        names_with(b, b"RUBY_")
            .into_iter()
            // A read spells the name inside a string literal.
            .filter(|(at, _)| *at > 0 && b[at - 1] == b'"')
            .map(|(_, n)| n)
    })
    .filter(|n| !RUBY_NOT_ENV.contains(&n.as_str()))
    .collect();

    // A floor, not a target: a scan that read nothing would pass forever.
    assert!(
        readers.contains("RUBY_BOX"),
        "the scan did not find RUBY_BOX, which crates/zeo-rt/src/boxes.rs reads"
    );
    let missing: Vec<&String> = readers.iter().filter(|v| !documented.contains(*v)).collect();
    assert!(
        missing.is_empty(),
        "RUBY_* environment variables the runtime reads that no page documents: {missing:?}\n\
         Add a row to docs/reference/environment-variables.md."
    );
}
