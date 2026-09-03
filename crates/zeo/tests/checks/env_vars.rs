//! Every environment variable the docs name has a reader in the tree.
//!
//! A documented variable with no reader is a silent no-op: the reader was
//! renamed or deleted and the instruction outlived it. Three of those were
//! found at once -- `ZEO_BLESS=1` had been replaced by `ZEO_BLESS_FROM_TOOL`,
//! and two ledgers plus a doc still told the reader to use the old spelling,
//! which does nothing at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Spellings the docs mention only to say they are retired. A variable named
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

/// Every whole `ZEO_*` word in `bytes`. Read as bytes because a `.tsv` in the
/// corpus can hold sequences that are not UTF-8.
fn names(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let Some(at) = bytes[i..].windows(4).position(|w| w == b"ZEO_") else {
            break;
        };
        let start = i + at;
        // `MYZEO_X` is not a mention of `ZEO_X`.
        if start > 0 && is_word(bytes[start - 1]) {
            i = start + 4;
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
    // `var_os("X")`, `ENV["X"]`.
    let readers: BTreeSet<String> = collect(&root, &["crates", "tools"], &[], &["rs", "rb"])
        .iter()
        .flat_map(|(_, b)| {
            names(b)
                .into_iter()
                .filter(|(at, _)| *at > 0 && b[at - 1] == b'"')
                .map(|(_, n)| n)
        })
        .collect();

    // A floor, not a target: a scan that silently reads nothing would make
    // this test pass forever. The docs name 22 today.
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
