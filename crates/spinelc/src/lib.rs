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

/// The compile-time file context Phase 14.1's `require` resolution needs --
/// see `parse::parse_and_lower_with`. `Default` (no path, no roots) keeps
/// `compile_to_rust`'s pathless behavior: `require_relative` then fails with
/// CRuby's own "cannot infer basepath", and a plain `require` finds nothing.
#[derive(Default)]
pub struct CompileOptions {
    /// The main file's own path -- the base for its `require_relative`s.
    pub input_path: Option<std::path::PathBuf>,
    /// Ordered `-I` search roots for plain `require "feature"`.
    pub load_roots: Vec<std::path::PathBuf>,
    /// Ordered directories to discover `spin.toml` packages under (Phase
    /// 14.2), searched AFTER every `-I` root; a nonexistent dir contributes
    /// nothing. The CLI defaults to the input file's sibling `packages/`
    /// then the compiler's own bundled `packages/` -- see `main.rs`.
    pub package_dirs: Vec<std::path::PathBuf>,
}

/// A compiled program: the generated Rust source, ready for
/// `build::build_binary`.
#[derive(Debug)]
pub struct CompileOutput {
    pub rust_source: String,
}

/// The full parse -> analyze -> codegen pipeline: Ruby source in, formatted
/// Rust source text out. Both `main.rs` (the CLI) and the test harness call
/// this directly.
pub fn compile_to_rust(source: &str) -> Result<String, String> {
    compile_to_rust_with(source, &CompileOptions::default()).map(|out| out.rust_source)
}

/// `compile_to_rust` plus the require-resolution context (Phase 14.1).
pub fn compile_to_rust_with(source: &str, opts: &CompileOptions) -> Result<CompileOutput, String> {
    let (hir, root) = parse::parse_and_lower_with(
        source,
        opts.input_path.as_deref(),
        &opts.load_roots,
        &opts.package_dirs,
    )?;
    let analyzed = analyze::analyze(hir, root)?;
    Ok(CompileOutput {
        rust_source: codegen::codegen_to_string(&analyzed)?,
    })
}
