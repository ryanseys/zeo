//! `zeo`: parse -> lower -> analyze -> codegen -> `cargo build`. Mirrors
//! zeo's `main.c` driver -- a single binary, no separate parse/analyze/
//! codegen executables, and the final artifact is a genuine native binary
//! produced by shelling out to the real Rust toolchain (zeo shells out to
//! `cc`; we shell out to `cargo`, for the same reason: neither compiler
//! touches machine code or an object-file format directly).
//!
//! Exposed as a library (not just a `main.rs` binary) so both the CLI and
//! the in-process test suite (`tests/`) can call `compile_to_rust` and
//! `backend::build_binary` directly -- no subprocess spawn needed just to
//! invoke the compiler itself; a subprocess is only unavoidable for the
//! final `cargo build` of the *generated* program (and running the
//! resulting binary), since that's a genuinely separate compilation unit.

pub mod analyze;
pub mod backend;
pub mod builtin_surface;
pub mod codegen;
pub mod compiler;
pub mod diagnostics;
pub mod gem_report;

pub use diagnostics::CompileError;

// The front end: the typed HIR arena (`hir`) plus the prism-tree -> HIR
// lowering (`lower`), formerly the standalone `zeo-hir` crate, folded in as
// ordinary modules (§ crate consolidation). It never touches the filesystem;
// `require` resolution/gems/search paths stay in the compiler's loader
// (`parse`), which drives lowering file-by-file via `lower::context`.
pub mod constpath;
pub mod hir;
pub mod lower;
pub mod lower_error;
pub mod rename;

pub use parse::gem_compat::{GemCompatEntry, GemCompatOutcome, gem_compat, gem_compat_installed};
pub mod parse;
pub mod types;
mod guard_fold;

/// The compile-time file context `require` resolution needs --
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
    /// Where to write the gem disclosure record (`gem_report`).
    /// `None` is the `--no-report` opt-out -- and the DEFAULT for the library
    /// API, so in-process callers (the e2e/conformance harnesses) don't litter
    /// the tree. The CLI defaults it to a path next to the output artifact.
    pub gem_report: Option<std::path::PathBuf>,
    /// Emit the once-per-library substitution warnings to stderr. Off by
    /// default (keeps the harness path silent); the CLI turns it on.
    pub gem_warnings: bool,
    /// Warning slugs suppressed via `--nowarn=<slug>` -- a dial independent of
    /// `gem_report`, so a caller can silence the noise but keep the file.
    pub nowarn: std::collections::HashSet<String>,
    /// `--gem-path <dir>`: an installed RubyGems store (`gem env gemdir`) to
    /// resolve locked gems against. Explicit opt-in, paired with `lockfile`;
    /// neither is ambient (a compile that silently depends on `$GEM_HOME` is
    /// not reproducible).
    pub gem_path: Option<std::path::PathBuf>,
    /// `--lockfile <Gemfile.lock>`: the resolved gem set to draw from the
    /// store. Only meaningful together with `gem_path`.
    pub lockfile: Option<std::path::PathBuf>,
}

/// A compiled program: the generated Rust source, ready for
/// `backend::build_binary`.
#[derive(Debug)]
pub struct CompileOutput {
    pub rust_source: String,
    /// Whether this program can reach the runtime eval VM (see
    /// `Hir::uses_runtime_eval`). Selects which `backend::Runtime` variant the
    /// binary links: `true` -> the prism-backed `eval-vm` runtime, `false` ->
    /// the lean, parser-free default. The build step maps it via
    /// `backend::Runtime::for_eval`.
    pub needs_eval_vm: bool,
}

/// The full parse -> analyze -> codegen pipeline: Ruby source in, formatted
/// Rust source text out. Both `main.rs` (the CLI) and the test harness call
/// this directly.
pub fn compile_to_rust(source: &str) -> Result<String, String> {
    compile_to_rust_with(source, &CompileOptions::default())
        .map(|out| out.rust_source)
        .map_err(String::from)
}

/// `compile_to_rust` plus the require-resolution context. The typed error
/// carries the failing stage and (for lowering rejections) the source span --
/// see `diagnostics`; `String` consumers convert via `From`/`Display`.
pub fn compile_to_rust_with(
    source: &str,
    opts: &CompileOptions,
) -> Result<CompileOutput, CompileError> {
    let (hir, root, gem_records) = parse::parse_and_lower_with(
        source,
        opts.input_path.as_deref(),
        &opts.load_roots,
        &opts.package_dirs,
        opts.gem_path.as_deref(),
        opts.lockfile.as_deref(),
    )?;
    // The disclosure record is fully known once lowering resolved
    // every require. Write it (and warn) BEFORE analyze/codegen, so the ledger
    // lands even if a later stage fails.
    if opts.gem_warnings {
        gem_report::emit_warnings(&gem_records, &opts.nowarn);
    }
    if let Some(path) = &opts.gem_report {
        gem_report::write_report(&gem_records, path)
            .map_err(|message| CompileError::Report { message })?;
    }
    // Computed from the arena BEFORE `analyze` consumes it: a whole-program
    // fact (does any eval site survive lowering?), so it belongs here rather
    // than downstream where the arena is already owned by `Analyzed`.
    let needs_eval_vm = hir.uses_runtime_eval();
    let analyzed = analyze::analyze(hir, root)?;
    Ok(CompileOutput {
        rust_source: codegen::codegen_to_string(&analyzed)?,
        needs_eval_vm,
    })
}
