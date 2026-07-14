//! `spinelc`: parse -> lower -> analyze -> codegen -> `cargo build`. Mirrors
//! spinel's `main.c` driver -- a single binary, no separate parse/analyze/
//! codegen executables, and the final artifact is a genuine native binary
//! produced by shelling out to the real Rust toolchain (spinel shells out to
//! `cc`; we shell out to `cargo`, for the same reason: neither compiler
//! touches machine code or an object-file format directly).
//!
//! Exposed as a library (not just a `main.rs` binary) so both the CLI and
//! the in-process test suite (`tests/`) can call `compile_to_rust` and
//! `build::build_binary` directly -- no subprocess spawn needed just to
//! invoke the compiler itself; a subprocess is only unavoidable for the
//! final `cargo build` of the *generated* program (and running the
//! resulting binary), since that's a genuinely separate compilation unit.

pub mod analyze;
pub mod build;
pub mod codegen;
pub mod compiler;
pub mod hir;
pub mod parse;
pub mod types;

/// The full parse -> analyze -> codegen pipeline: Ruby source in, formatted
/// Rust source text out. Both `main.rs` (the CLI) and the test harness call
/// this directly.
pub fn compile_to_rust(source: &str) -> Result<String, String> {
    let (hir, root) = parse::parse_and_lower(source)?;
    let analyzed = analyze::analyze(hir, root)?;
    codegen::codegen_to_string(&analyzed)
}
