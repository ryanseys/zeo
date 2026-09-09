//! Every command the docs tell a reader to run exists, and every path they
//! cite is a file in the tree.
//!
//! A guide is a promise, and a `cargo xtask <verb>` whose verb was renamed
//! reads exactly like one that works until someone types it. A cited path
//! rots the same way when a directory moves.
//!
//! Three shapes are checked, because three can be checked cheaply and
//! exactly: an xtask verb, a nextest profile, and a repo-relative path in
//! backticks or a Markdown link. Everything else in a fenced block is prose
//! to a machine.

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
                // Nothing generated or fetched, and none of the local-only
                // directories .gitignore names.
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
            // Take the identifier and stop: prose puts a backtick, a
            // possessive or a comma straight after it.
            let verb: String = verb
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            let verb = verb.as_str();
            if verb.is_empty() {
                continue;
            }
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
                let profile: String = profile
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                    .collect();
                let profile = profile.as_str();
                if profile.is_empty() {
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

/// Pages that describe history rather than the tree: a path named there was
/// true when it was written.
const EXEMPT: &[&str] = &["CHANGELOG.md"];

/// A path with no leading `test/` or `crates/` is shorthand for one of these.
const SHORTHAND_ROOTS: &[&str] = &[
    "crates",
    "crates/zeo/src",
    "crates/zeo/tests/suite",
    "crates/zeo-rt/src",
    "crates/zeo-capi/src",
];

/// `rel` exists under `root` with exactly this spelling. `Path::exists` says
/// yes to `COMPATIBILITY.md` on a case-insensitive volume and no on Linux;
/// `read_dir` names do not.
fn exists_exactly(root: &Path, rel: &str) -> bool {
    let mut parts: Vec<&str> = Vec::new();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return false;
                }
            }
            p => parts.push(p),
        }
    }
    let mut dir = root.to_path_buf();
    for part in parts {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return false;
        };
        if !entries.flatten().any(|e| e.file_name() == part) {
            return false;
        }
        dir.push(part);
    }
    true
}

/// Why `token`, cited on `page`, does not name a file in the tree; `None` if
/// it does or if it is not a path citation at all.
fn stale_citation(root: &Path, page_dir: &str, token: &str) -> Option<&'static str> {
    // A placeholder, a glob, a Rust path or a phrase is prose, not a path.
    if token.contains(['*', '<', '{', '(']) || token.contains("::") || token.contains(char::is_whitespace) {
        return None;
    }
    let path = token.split(':').next().unwrap_or("").trim_end_matches('/');
    if path.starts_with("tests/") {
        return Some("the corpus is `test/`, singular");
    }
    let anchored = ["test/", "crates/", "docs/", ".github/", "exe/"]
        .iter()
        .any(|a| path.starts_with(a));
    if anchored {
        return (!exists_exactly(root, path)).then_some("no such file (spelling is case-exact)");
    }
    // Shorthand: `clif/verify.rs`, `zeo-rt/Cargo.toml`, `src/lib.rs` beside a
    // README. Ruby shorthand is nearly always another project's tree.
    let shorthand = path.contains('/') && [".rs", ".toml", ".md"].iter().any(|e| path.ends_with(e));
    if !shorthand {
        return None;
    }
    let found = exists_exactly(root, &format!("{page_dir}/{path}"))
        || SHORTHAND_ROOTS.iter().any(|r| exists_exactly(root, &format!("{r}/{path}")));
    (!found).then_some("no such file under the page's directory or any crate root")
}

#[test]
fn every_cited_path_exists() {
    let root = repo_root();
    let mut checked = 0usize;
    let mut bad = Vec::new();
    for page in pages() {
        let rel = page.strip_prefix(&root).unwrap_or(&page).to_string_lossy().into_owned();
        if EXEMPT.contains(&rel.as_str()) {
            continue;
        }
        let page_dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
        let Ok(text) = std::fs::read_to_string(&page) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            // Inline code, in prose or in a table cell alike.
            for token in line.split('`').skip(1).step_by(2) {
                if let Some(why) = stale_citation(&root, page_dir, token) {
                    bad.push(format!("{rel}:{}: `{token}` -- {why}", n + 1));
                }
                checked += 1;
            }
            // Relative links resolve from the page, absolute ones from the root.
            for target in line.split("](").skip(1).filter_map(|r| r.split(')').next()) {
                let target = target.split('#').next().unwrap_or("");
                if target.is_empty() || target.contains(':') {
                    continue;
                }
                let resolved = match target.strip_prefix('/') {
                    Some(abs) => abs.to_string(),
                    None => format!("{page_dir}/{target}"),
                };
                if !exists_exactly(&root, &resolved) {
                    bad.push(format!(
                        "{rel}:{}: [..]({target}) -- resolves to {resolved}, which does not exist (spelling is case-exact)",
                        n + 1
                    ));
                }
                checked += 1;
            }
        }
    }
    // A floor, not a target: the docs cite a few hundred paths.
    assert!(checked >= 100, "only {checked} citations scanned -- the scan is not reading them");
    assert!(
        bad.is_empty(),
        "the docs cite paths that do not exist:\n  {}",
        bad.join("\n  ")
    );
}
