//! The tree carries no C.
//!
//! zeo's compiler, runtime and C API are Rust. C reaches a build only
//! through a `-sys` crate that compiles the library it wraps, and the
//! extension a test builds is text the test writes. A tracked `.c` is the
//! start of a second implementation language, so the rule is a file list
//! and a crate list rather than a review note. `make no-c-files` is the
//! push-tier half of the first test, which builds nothing; this holds the
//! exceptions exact.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the repo root")
}

/// C, C++, Objective-C and assembly sources, and a patch to any of them.
const C_EXTENSIONS: &[&str] = &[
    "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "m", "mm", "S", "s", "patch",
];

/// The tracked C that remains, each row with the day it goes.
///
/// `zeo-capi/csrc/*.c` are the variadic entry points (`rb_raise`,
/// `rb_sprintf`, `rb_scan_args`, ...) that Rust cannot write without
/// `c_variadic`, which stabilizes in Rust 1.99. They go when the toolchain
/// pin reaches it, expected 2026-10.
const ALLOWED: &[&str] = &[
    "crates/zeo-capi/csrc/cext_err.c",
    "crates/zeo-capi/csrc/cext_fmt.c",
    "crates/zeo-capi/csrc/cext_va.c",
];

/// The crates that compile C, closed. A package that depends on `cc`,
/// `bindgen`, `cmake` or `cxx-build` in any way is one that builds C, and
/// the set has to be this one -- `deny.toml`'s `wrappers` say "may", this
/// says "does".
const C_COMPILING_CRATES: &[&str] = &[
    // Dev-only, through criterion.
    "alloca",
    "libffi-sys",
    "libmimalloc-sys",
    "onig_sys",
    "openssl-src",
    "openssl-sys",
    "ruby-prism-sys",
    // The `csrc/*.c` rows above; leaves with them.
    "zeo-capi",
];

const C_BUILD_TOOLS: &[&str] = &["cc", "bindgen", "cmake", "cxx-build"];

#[test]
fn no_tracked_c_source() {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo_root())
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git ls-files failed");
    let files = String::from_utf8_lossy(&out.stdout);
    let c: Vec<&str> = files
        .split('\0')
        .filter(|f| {
            Path::new(f)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| C_EXTENSIONS.contains(&e))
        })
        .collect();
    let stray: Vec<&str> = c.iter().copied().filter(|f| !ALLOWED.contains(f)).collect();
    assert!(
        stray.is_empty(),
        "tracked C source (the tree carries none):\n{}",
        stray.join("\n")
    );
    let gone: Vec<&str> = ALLOWED.iter().copied().filter(|f| !c.contains(f)).collect();
    assert!(
        gone.is_empty(),
        "no longer tracked -- drop from ALLOWED:\n{}",
        gone.join("\n")
    );
}

#[test]
fn c_is_compiled_only_by_the_named_crates() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--all-features", "--format-version", "1"])
        .current_dir(repo_root())
        .output()
        .expect("cargo runs");
    assert!(
        out.status.success(),
        "cargo metadata: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("metadata is JSON");
    let compiling: BTreeSet<&str> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter(|package| {
            package["dependencies"]
                .as_array()
                .expect("dependencies")
                .iter()
                .any(|dep| C_BUILD_TOOLS.contains(&dep["name"].as_str().unwrap_or_default()))
        })
        .map(|package| package["name"].as_str().expect("a name"))
        .collect();
    let named: BTreeSet<&str> = C_COMPILING_CRATES.iter().copied().collect();
    let new: Vec<&&str> = compiling.difference(&named).collect();
    let gone: Vec<&&str> = named.difference(&compiling).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "crates that compile C:\n  new (a deliberate decision, then name it here and in \
         deny.toml): {new:?}\n  gone (drop from both): {gone:?}"
    );
}
