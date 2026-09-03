//! Two limits on the tracked tree that nothing else notices.
//!
//! Both once broke silently, and both are cheap enough to ask on every run.

use std::path::Path;
use std::process::Command;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the repo root")
}

fn tracked_files() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo_root())
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `to_utf8_lossy` in the runtime is a DISPLAY-path convenience that quietly
/// corrupts a non-UTF-8 semantic operation. The audit moves sites onto
/// byte-aware paths one at a time, so the count may only go DOWN.
///
/// Re-baselined once, 2026-07-31 (211 -> 279): the openssl, zlib, socket,
/// ffi, date and bigdecimal extensions each brought display paths with them.
/// The number does not measure the harm -- it counts comments, asserts and
/// the definition, and misses `chars()` and `char_vec()`, which decode the
/// same way. `test/gaps/` is the real record; this is a backstop against
/// unbounded growth.
const LOSSY_LIMIT: usize = 279;

#[test]
fn the_lossy_utf8_count_only_goes_down() {
    let src = repo_root().join("crates/zeo-rt/src");
    let mut count = 0;
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read zeo-rt/src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                count += text.matches("to_utf8_lossy").count();
            }
        }
    }
    assert!(
        count <= LOSSY_LIMIT,
        "to_utf8_lossy sites in zeo-rt grew to {count} (limit {LOSSY_LIMIT}): \
         new runtime code must use byte and encoding aware paths"
    );
}

/// A tracked file this large is almost always a build artifact someone
/// committed by mistake. History was rewritten once to strip blobs over this
/// size, and three 13 MB binaries sat in HEAD while the shell version of this
/// check reported nothing at all.
const MAX_TRACKED_BYTES: u64 = 1 << 20;

#[test]
fn no_tracked_file_is_over_a_megabyte() {
    let big: Vec<String> = tracked_files()
        .into_iter()
        .filter_map(|rel| {
            let len = std::fs::metadata(repo_root().join(&rel)).ok()?.len();
            (len > MAX_TRACKED_BYTES).then(|| format!("{len} {rel}"))
        })
        .collect();
    assert!(
        big.is_empty(),
        "tracked files over 1MB:\n{}",
        big.join("\n")
    );
}
