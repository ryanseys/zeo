//! Every command the docs tell a reader to run exists.
//!
//! A guide is a promise, and a `cargo xtask <verb>` whose verb was renamed
//! reads exactly like one that works until someone types it. The Makefile's
//! `make check` outlived the target by months that way, and `cargo xtask
//! bench` outlived its rename by an afternoon.
//!
//! Only two shapes are checked, because only two can be checked cheaply and
//! exactly: an xtask verb, and a nextest profile. Everything else in a fenced
//! block is prose to a machine.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the workspace root")
        .to_path_buf()
}

/// Every tracked `.md`, and `README.md` beside a crate.
fn pages() -> Vec<PathBuf> {
    let root = repo_root();
    let mut out = Vec::new();
    let mut walk = vec![root.clone()];
    while let Some(dir) = walk.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                // Nothing generated, fetched, or belonging to a tool.
                if matches!(
                    name.as_str(),
                    "target" | "vendor" | ".git" | ".jj" | ".claude" | "notes" | "measurements"
                ) {
                    continue;
                }
                walk.push(path);
            } else if name.ends_with(".md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The verbs `cargo xtask --help` lists.
fn xtask_verbs() -> BTreeSet<String> {
    let path = repo_root().join("crates/xtask/src/main.rs");
    let text = std::fs::read_to_string(&path).expect("xtask's main.rs is committed");
    let usage = text
        .split("const USAGE: &str = \"\\")
        .nth(1)
        .expect("xtask states its usage")
        .split("\";")
        .next()
        .expect("the usage block ends");
    usage
        .lines()
        .skip_while(|l| !l.starts_with("commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  ") && !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .map(String::from)
        .collect()
}

/// The profiles `.config/nextest.toml` declares.
fn nextest_profiles() -> BTreeSet<String> {
    let path = repo_root().join(".config/nextest.toml");
    let text = std::fs::read_to_string(&path).expect("the nextest config is committed");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("[profile."))
        .filter_map(|l| l.split(['.', ']']).next())
        .map(String::from)
        .collect()
}

#[test]
fn every_documented_xtask_verb_exists() {
    let verbs = xtask_verbs();
    assert!(verbs.len() > 5, "parsed no xtask verbs: {verbs:?}");
    let mut bad = Vec::new();
    for page in pages() {
        let Ok(text) = std::fs::read_to_string(&page) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let Some(rest) = line.split("cargo xtask ").nth(1) else {
                continue;
            };
            let Some(verb) = rest.split_whitespace().next() else {
                continue;
            };
            // `cargo xtask <command>` in a usage line names no verb.
            if verb.starts_with('<') || verb.starts_with('-') {
                continue;
            }
            let verb = verb.trim_end_matches(['`', '.', ',', ')', '"']);
            if !verbs.contains(verb) {
                bad.push(format!(
                    "{}:{}: `cargo xtask {verb}`",
                    page.display(),
                    n + 1
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "the docs name xtask verbs that do not exist:\n  {}\n\nthe verbs are: {}",
        bad.join("\n  "),
        verbs.iter().cloned().collect::<Vec<_>>().join(", ")
    );
}

#[test]
fn every_documented_nextest_profile_exists() {
    let profiles = nextest_profiles();
    assert!(
        profiles.contains("default") && profiles.contains("full"),
        "parsed no nextest profiles: {profiles:?}"
    );
    let mut bad = Vec::new();
    for page in pages() {
        let Ok(text) = std::fs::read_to_string(&page) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for spelling in ["nextest run -P ", "nextest run --profile "] {
                let Some(rest) = line.split(spelling).nth(1) else {
                    continue;
                };
                let Some(profile) = rest.split_whitespace().next() else {
                    continue;
                };
                let profile = profile.trim_end_matches(['`', '.', ',', ')', '"']);
                if profile.starts_with('<') {
                    continue;
                }
                if !profiles.contains(profile) {
                    bad.push(format!("{}:{}: -P {profile}", page.display(), n + 1));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "the docs name nextest profiles that do not exist:\n  {}\n\nthe profiles are: {}",
        bad.join("\n  "),
        profiles.iter().cloned().collect::<Vec<_>>().join(", ")
    );
}
