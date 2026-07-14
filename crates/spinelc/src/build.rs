//! Compiles generated Rust source against `spinel-rt` by dropping it into a
//! throwaway `cargo` project with a path dependency on the workspace
//! `spinel-rt` crate, then copying the resulting binary to `output`. Using
//! `cargo` (not a bare `rustc` invocation with hand-guessed rlib paths) is
//! the robust choice: it resolves `spinel-rt`'s own dependency graph
//! correctly even as that crate grows, exactly as spinel's `main.c` prefers
//! letting `cc` do real dependency/library resolution over hand-assembling
//! a link line.
//!
//! Lives in the library (not `main.rs`) so the in-process test harness
//! (`tests/support`) can call the exact same function the CLI uses, rather
//! than a re-derived copy of the same temp-project logic.

use std::path::{Path, PathBuf};

pub fn build_binary(rust_source: &str, output: &Path) -> Result<(), String> {
    let spinel_rt_dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../spinel-rt"));

    let build_dir = std::env::temp_dir().join(format!(
        "spinelc-build-{}-{}",
        std::process::id(),
        thread_unique_suffix()
    ));
    let src_dir = build_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| e.to_string())?;

    let manifest = format!(
        "[package]\nname = \"spinelc-generated\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [[bin]]\nname = \"program\"\npath = \"src/main.rs\"\n\n\
         [dependencies]\nspinel-rt = {{ path = {:?} }}\n",
        spinel_rt_dir.to_string_lossy()
    );
    std::fs::write(build_dir.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    std::fs::write(src_dir.join("main.rs"), rust_source).map_err(|e| e.to_string())?;

    let status = std::process::Command::new("cargo")
        .args(["build", "--quiet", "--manifest-path"])
        .arg(build_dir.join("Cargo.toml"))
        .status()
        .map_err(|e| format!("running cargo: {e}"))?;

    if !status.success() {
        return Err(format!(
            "cargo build failed for generated program (source left at {})",
            build_dir.display()
        ));
    }

    let built = build_dir.join("target/debug/program");
    std::fs::copy(&built, output)
        .map_err(|e| format!("copying {} to {}: {e}", built.display(), output.display()))?;
    let _ = std::fs::remove_dir_all(&build_dir);
    Ok(())
}

/// Multiple tests in the same test-binary process share `process::id()`; a
/// per-thread suffix keeps their temp build directories from colliding when
/// the test runner parallelizes `#[test]` functions.
fn thread_unique_suffix() -> String {
    format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}
