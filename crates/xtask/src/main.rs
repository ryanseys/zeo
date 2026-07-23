//! `cargo run -p xtask -- <command>`: the project's build/test automation,
//! mirroring zeo's `make test` / `make regen-expected` without pulling in
//! a task-runner crate -- the Rust community's usual way to add custom
//! project automation without extra dependencies.
//!
//! - `xtask test`: compiles each `examples/*.rb` via `zeo`, runs the
//!   resulting binary, and diffs its stdout against `examples/<name>.expected`
//!   (oracle = real `ruby`, exactly like zeo's own `test/*.rb` +
//!   `.rb.expected` golden-file discipline).
//! - `xtask regen`: re-runs real `ruby` over every `examples/*.rb` and
//!   overwrites the matching `.expected` file (mirrors
//!   `zeo-regen-expected-from-ruby`).
//! - `xtask bench [--filter <substr>] [--runs N] [--update-baseline]`: the
//!   golden-output performance suite under `bench/` (see `bench.rs`).
//! - `xtask conformance <run|triage|show|oracle-verify>`: the external-corpus
//!   conformance harness (see `conformance/mod.rs`).
//! - `xtask stdlib-status [<lib-dir>]`: sweeps the installed Ruby stdlib `lib`
//!   (dropped in via `-I`, no bespoke flag) and records which files `zeo`
//!   can compile -- the stdlib progress tracker (see `stdlib_status.rs`).

mod bench;
mod conformance;
mod gem;
mod gem_compat;
mod stdlib_status;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn workspace_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn examples(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("examples");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "rb"))
        .collect();
    files.sort();
    files
}

fn regen(root: &Path) -> ExitCode {
    for rb in examples(root) {
        let expected = rb.with_extension("expected");
        let mut cmd = Command::new("ruby");
        // A `Ruby::Box` example (Phase 18) needs the experimental feature
        // enabled on the oracle side; every other example runs plain ruby
        // exactly as before.
        let source = std::fs::read_to_string(&rb)
            .unwrap_or_else(|e| panic!("reading {}: {e}", rb.display()));
        if source.contains("Ruby::Box") {
            cmd.env("RUBY_BOX", "1").arg("-W:no-experimental");
        }
        let output = cmd
            .arg(&rb)
            .output()
            .unwrap_or_else(|e| panic!("running ruby: {e}"));
        std::fs::write(&expected, &output.stdout)
            .unwrap_or_else(|e| panic!("writing {}: {e}", expected.display()));
        println!("regenerated {}", expected.display());
    }
    ExitCode::SUCCESS
}

fn test(root: &Path) -> ExitCode {
    let zeo_manifest = root.join("crates/zeo/Cargo.toml");
    let build = Command::new("cargo")
        .args(["build", "--quiet", "--manifest-path"])
        .arg(&zeo_manifest)
        .status()
        .unwrap_or_else(|e| panic!("running cargo build: {e}"));
    if !build.success() {
        eprintln!("xtask: zeo failed to build");
        return ExitCode::FAILURE;
    }
    let zeo_bin = root.join("target/debug/zeo");

    let mut failures = Vec::new();
    for rb in examples(root) {
        let name = rb.file_stem().unwrap().to_string_lossy().into_owned();
        let expected_path = rb.with_extension("expected");
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|e| {
            panic!(
                "reading {} (run `xtask regen` first): {e}",
                expected_path.display()
            )
        });

        let bin_path = std::env::temp_dir().join(format!("zeors-xtask-{name}"));
        let compile = Command::new(&zeo_bin)
            .arg(&rb)
            .arg("-o")
            .arg(&bin_path)
            // The example suite exercises substitutions on purpose; skip the
            // Phase-2b disclosure record so it doesn't drop a per-example file.
            .arg("--no-report")
            .output();

        let actual = match compile {
            Ok(out) if out.status.success() => match Command::new(&bin_path).output() {
                Ok(run) => String::from_utf8_lossy(&run.stdout).into_owned(),
                Err(e) => format!("<failed to run compiled binary: {e}>"),
            },
            Ok(out) => format!("<zeo failed: {}>", String::from_utf8_lossy(&out.stderr)),
            Err(e) => format!("<failed to invoke zeo: {e}>"),
        };
        let _ = std::fs::remove_file(&bin_path);

        if actual == expected {
            println!("PASS {name}");
        } else {
            println!("FAIL {name}");
            println!("  expected: {expected:?}");
            println!("  actual:   {actual:?}");
            failures.push(name);
        }
    }

    if failures.is_empty() {
        println!("\nall {} examples passed", examples(root).len());
        ExitCode::SUCCESS
    } else {
        println!(
            "\n{} example(s) failed: {}",
            failures.len(),
            failures.join(", ")
        );
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    let root = workspace_root();
    let args: Vec<String> = std::env::args().skip(2).collect();
    match std::env::args().nth(1).as_deref() {
        Some("test") => test(&root),
        Some("bench") => bench::main(&root, &args),
        Some("regen") => regen(&root),
        Some("conformance") => conformance::main(&root, &args),
        Some("stdlib-status") => stdlib_status::main(&root, &args),
        Some("gem-compat") => gem_compat::main(&root, &args),
        Some("gem") => gem::main(&root, &args),
        _ => {
            eprintln!(
                "usage: cargo run -p xtask -- <test|regen|bench|conformance|stdlib-status|gem-compat|gem>"
            );
            ExitCode::FAILURE
        }
    }
}
