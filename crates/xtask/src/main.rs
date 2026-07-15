//! `cargo run -p xtask -- <command>`: the project's build/test automation,
//! mirroring spinel's `make test` / `make regen-expected` without pulling in
//! a task-runner crate -- the Rust community's usual way to add custom
//! project automation without extra dependencies.
//!
//! - `xtask test`: compiles each `examples/*.rb` via `spinelc`, runs the
//!   resulting binary, and diffs its stdout against `examples/<name>.expected`
//!   (oracle = real `ruby`, exactly like spinel's own `test/*.rb` +
//!   `.rb.expected` golden-file discipline).
//! - `xtask regen`: re-runs real `ruby` over every `examples/*.rb` and
//!   overwrites the matching `.expected` file (mirrors
//!   `spinel-regen-expected-from-ruby`).

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
    let spinelc_manifest = root.join("crates/spinelc/Cargo.toml");
    let build = Command::new("cargo")
        .args(["build", "--quiet", "--manifest-path"])
        .arg(&spinelc_manifest)
        .status()
        .unwrap_or_else(|e| panic!("running cargo build: {e}"));
    if !build.success() {
        eprintln!("xtask: spinelc failed to build");
        return ExitCode::FAILURE;
    }
    let spinelc_bin = root.join("target/debug/spinelc");

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

        let bin_path = std::env::temp_dir().join(format!("spinelrs-xtask-{name}"));
        let compile = Command::new(&spinelc_bin)
            .arg(&rb)
            .arg("-o")
            .arg(&bin_path)
            .output();

        let actual = match compile {
            Ok(out) if out.status.success() => match Command::new(&bin_path).output() {
                Ok(run) => String::from_utf8_lossy(&run.stdout).into_owned(),
                Err(e) => format!("<failed to run compiled binary: {e}>"),
            },
            Ok(out) => format!("<spinelc failed: {}>", String::from_utf8_lossy(&out.stderr)),
            Err(e) => format!("<failed to invoke spinelc: {e}>"),
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
    match std::env::args().nth(1).as_deref() {
        Some("test") => test(&root),
        Some("regen") => regen(&root),
        _ => {
            eprintln!("usage: cargo run -p xtask -- <test|regen>");
            ExitCode::FAILURE
        }
    }
}
