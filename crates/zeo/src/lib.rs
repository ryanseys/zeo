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

// `clippy::wildcard_enum_match_arm` is OFF (user-directed 2026-08-20): the
// lint fired on far more probes and folds -- where every unlisted variant is
// answered by its children or by one honest default -- than on walks that
// genuinely had to decide, so the 106 `allow`s it collected were noise around
// the handful of real cases. A walk that must name every variant still spells
// them out; one that asks "may I descend through this?" uses
// `HirNode::scope_kind`, which is exhaustive in one place.

/// The runtime, re-exported whole: the re-export is what forces `zeo-rt`
/// (its `zeo_rt_*` C surface and linkme tables included) into every output
/// of this lib -- the `zeo` binary AND the `libzeo.a` staticlib AOT
/// programs link. The eval hook (`zeo_rt::eval::EvalCompiler`, G6) installs
/// through this path too.
pub use zeo_rt;

/// The one allocator per process image (plan decision 9): `libzeo.a` is the
/// only Rust staticlib an AOT program ever links, so the allocator it runs
/// on is declared HERE -- the `zeo` binary and the test harnesses inherit
/// it. The front end is allocation-heavy (arena nodes, interner strings,
/// token streams), and mimalloc reliably beats system malloc on that shape.
#[global_allocator]
static GLOBAL_ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod analyze;
pub mod analyze_error;
pub mod backend;
pub mod builtin_surface;
pub mod clif;
pub mod codegen;
pub mod compiler;
pub(crate) mod debug_flags;
pub mod diagnostics;
pub mod gem_report;
pub mod home;
pub mod memguard;

pub use diagnostics::CompileError;

// The front end: the typed HIR arena (`hir`) plus the prism-tree -> HIR
// lowering (`lower`). It never touches the filesystem;
// `require` resolution/gems/search paths stay in the compiler's loader
// (`parse`), which drives lowering file-by-file via `lower::context`.
pub mod constpath;
pub mod hir;
pub mod lower;
pub mod lower_error;
pub mod rename;

pub use parse::gem_compat::{GemCompatEntry, GemCompatOutcome, gem_compat, gem_compat_installed};
mod guard_fold;
pub(crate) mod names;
pub mod parse;
pub mod types;

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
    /// Ordered directories of vendored gems (each subdirectory with a
    /// `.gemspec` is a gem), searched AFTER every `-I` root, first name wins;
    /// a nonexistent dir contributes nothing. The CLI fills this from
    /// `--gems` plus the input file's sibling `gems/`; the compiler's own
    /// bundled `gems/` is appended by the loader itself -- see `main.rs`.
    pub package_dirs: Vec<std::path::PathBuf>,
    /// Where to write the gem disclosure record (`gem_report`). `None` -- no
    /// file -- is the DEFAULT, for the library API and the CLI alike (the
    /// CLI's `--report` opts in with a path next to the output artifact).
    pub gem_report: Option<std::path::PathBuf>,
    /// Installed RubyGems store directories (`gem env gemdir`) to resolve
    /// locked gems against, probed in order (first hit per gem wins). The CLI
    /// fills this from `--gem-path` or `GEM_PATH` -- but only alongside a
    /// lockfile, so an ambient `GEM_PATH` alone never changes a compile.
    /// The library default is empty: no store resolution.
    pub gem_paths: Vec<std::path::PathBuf>,
    /// The `Gemfile.lock` naming the gem set to draw from the store (the CLI
    /// derives it from `--bundle-gemfile`/`BUNDLE_GEMFILE`). Only meaningful
    /// together with a non-empty `gem_paths`.
    pub lockfile: Option<std::path::PathBuf>,
    /// The distinguished ROOT package (`--root-gem`): the named gem outranks
    /// every other provider for an ambiguous feature -- Bundler's root
    /// semantics, where the app's own gem is the first activated. The gem
    /// probe passes its subject here so a squatted feature resolves to the
    /// gem actually under test.
    pub root_gem: Option<Gem>,
    /// Render the generated Rust through prettyplease (the `--dump=rust`
    /// human view).
    /// Off by default: the build path feeds rustc, which is insensitive to
    /// formatting, and the re-parse + pretty-print pair dominated emission
    /// at gem scale.
    pub pretty: bool,
}

/// A gem named by the caller -- the public identity type `CompileOptions`
/// speaks, distinct from the loader's internal resolved model (which carries
/// roots, version, and provenance the caller doesn't have). A struct rather
/// than a bare `String` so a gem's name can't be confused with any other
/// string option, and so identity can grow fields (a version pin) without an
/// API break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gem {
    name: String,
}

impl Gem {
    /// Identify a gem by its RubyGems name (`"rack"`, `"activesupport"`).
    pub fn named(name: impl Into<String>) -> Gem {
        Gem { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A compiled program: the generated Rust source, ready for
/// `backend::build_binary`.
#[derive(Debug)]
pub struct CompileOutput {
    pub rust_source: String,
    /// Whether this program needs prism at RUNTIME -- a runtime eval site,
    /// or `require "prism"` (see `Hir::needs_prism_runtime`). Selects which
    /// `backend::Runtime` variant the binary links: `true` -> the
    /// prism-linked `eval-vm` runtime, `false` -> the lean, parser-free
    /// default. The build step maps it via `backend::Runtime::for_prism`.
    pub needs_prism_runtime: bool,
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
    let (produced, needs_prism_runtime) = compile_with_emit(source, opts, Emit::Memory)?;
    let Produced::Memory(rust_source) = produced else {
        unreachable!("Emit::Memory produces Produced::Memory")
    };
    Ok(CompileOutput {
        rust_source,
        needs_prism_runtime,
    })
}

/// `compile_to_rust_with`, streamed straight to `path` instead of returned.
///
/// For a caller that wants the Rust as a FILE -- the CLI's `--emit-rust`, and
/// every sweep behind it. Nothing holds the program as text, so the peak drops
/// by one whole copy of the output; at gem scale that copy is the difference
/// between a compile that fits and one that does not. Always the compact
/// renderer: `--dump=rust`'s `syn` round-trip and prettyplease exist to make
/// output a PERSON reads, and add two more whole-program copies to do it.
pub fn compile_to_file(
    source: &str,
    opts: &CompileOptions,
    path: &std::path::Path,
) -> Result<EmitOutput, CompileError> {
    let (produced, needs_prism_runtime) = compile_with_emit(source, opts, Emit::File(path))?;
    let Produced::File(stats) = produced else {
        unreachable!("Emit::File produces Produced::File")
    };
    Ok(EmitOutput {
        bytes: stats.bytes,
        lines: stats.lines,
        needs_prism_runtime,
    })
}

/// A Cranelift-compiled program: one object file's bytes, ready for
/// `backend::link` (the `--backend aot` pipeline).
pub struct ObjectOutput {
    pub object: Vec<u8>,
    /// Whether the object carries DWARF. The LINK reads it: debug info in
    /// a Mach-O program lives in the object file the binary points back
    /// at, so a `-g` link keeps that object and keeps the symbol table
    /// that names it.
    pub debuginfo: bool,
}

/// The Cranelift pipeline: the same front end as `compile_to_rust_with`,
/// then `clif::emit` instead of the Rust emitter.
pub fn compile_to_object_with(
    source: &str,
    opts: &CompileOptions,
    debuginfo: bool,
) -> Result<ObjectOutput, CompileError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("zeo-compile".into())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, || {
                compile_object_on_this_thread(source, opts, debuginfo)
            })
            .expect("spawning the compiler thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// The shared Cranelift front half: parse, gem reporting, analyze -- what
/// every clif-backed entry (`aot` object, `jit` run, `--emit-clif`) does
/// before its own emission. Runs on the caller's (compile) thread.
fn analyze_on_this_thread(
    source: &str,
    opts: &CompileOptions,
) -> Result<analyze::Analyzed, CompileError> {
    memguard::set_phase(memguard::Phase::ParseLower);
    let (hir, root, gem_records) = parse::parse_and_lower_with(
        source,
        opts.input_path.as_deref(),
        &opts.load_roots,
        &opts.package_dirs,
        &opts.gem_paths,
        opts.lockfile.as_deref(),
        opts.root_gem.as_ref(),
    )?;
    if let Some(path) = &opts.gem_report {
        gem_report::write_report(&gem_records, path)
            .map_err(|message| CompileError::Report { message })?;
    }
    memguard::set_phase(memguard::Phase::Analyze);
    let analyzed = analyze::analyze(hir, root)?;
    memguard::set_phase(memguard::Phase::Codegen);
    Ok(analyzed)
}

fn compile_object_on_this_thread(
    source: &str,
    opts: &CompileOptions,
    debuginfo: bool,
) -> Result<ObjectOutput, CompileError> {
    let analyzed = analyze_on_this_thread(source, opts)?;
    let object = clif::emit::compile(&analyzed, debuginfo).map_err(CompileError::codegen)?;
    Ok(ObjectOutput { object, debuginfo })
}

/// The JIT run mode (`--backend jit`): compile in-process and run without
/// an object file, linker, or on-disk binary. Never returns on success --
/// the process exits with the program's status.
pub fn run_jit_with(
    source: &str,
    opts: &CompileOptions,
    program_name: &str,
    program_args: &[String],
) -> Result<std::convert::Infallible, CompileError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("zeo-compile".into())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let analyzed = analyze_on_this_thread(source, opts)?;
                match backend::jit::run(analyzed, program_name, program_args) {
                    Err(message) => Err(CompileError::codegen(message)),
                    Ok(never) => match never {},
                }
            })
            .expect("spawning the compiler thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// The per-function CLIF text (`--emit-clif`, the snapshot tests): the
/// same front end, the Cranelift lowering, and the pre-machine IR of
/// every emitted function.
pub fn compile_to_clif_text(source: &str, opts: &CompileOptions) -> Result<String, CompileError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("zeo-compile".into())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let analyzed = analyze_on_this_thread(source, opts)?;
                let (_bytes, text) =
                    clif::emit::compile_with_clif(&analyzed).map_err(CompileError::codegen)?;
                Ok(text)
            })
            .expect("spawning the compiler thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// What `compile_to_file` wrote.
#[derive(Debug)]
pub struct EmitOutput {
    pub bytes: u64,
    pub lines: u64,
    /// As [`CompileOutput::needs_prism_runtime`].
    pub needs_prism_runtime: bool,
}

fn compile_with_emit(
    source: &str,
    opts: &CompileOptions,
    emit: Emit<'_>,
) -> Result<(Produced, bool), CompileError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("zeo-compile".into())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, || compile_on_this_thread(source, opts, emit))
            .expect("spawning the compiler thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// Stack depth scales with source nesting depth, and `resolv` alone exceeds the
/// ~2 MiB a spawned thread gets by default -- the CLI survived only because a
/// main thread gets 8 MiB. Compiling on an explicitly-sized thread makes that
/// headroom the compiler's property rather than the caller's.
const COMPILE_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Whether `ZEO_TIMINGS` is set: the phase-timing report emitted by
/// `compile_on_this_thread`, `codegen_to_string`, and `backend::build_binary`
/// as machine-parseable `zeo-timings:` stderr lines (consumed by
/// `xtask compile-bench`).
pub fn timings_enabled() -> bool {
    std::env::var_os("ZEO_TIMINGS").is_some()
}

/// Where a compile's generated Rust goes.
///
/// The distinction is the whole point of `compile_to_file`: a program held as
/// a `String` is a second whole-program copy in memory beside the tokens it
/// was rendered from, and at gem scale that copy is hundreds of megabytes.
/// A caller that only wants the FILE never has to pay for it.
#[derive(Clone, Copy)]
enum Emit<'a> {
    Memory,
    File(&'a std::path::Path),
}

/// What a compile produced, matching the `Emit` it was given.
enum Produced {
    Memory(String),
    File(codegen::EmitStats),
}

fn compile_on_this_thread(
    source: &str,
    opts: &CompileOptions,
    emit: Emit<'_>,
) -> Result<(Produced, bool), CompileError> {
    let t_start = std::time::Instant::now();
    memguard::set_phase(memguard::Phase::ParseLower);
    let (hir, root, gem_records) = parse::parse_and_lower_with(
        source,
        opts.input_path.as_deref(),
        &opts.load_roots,
        &opts.package_dirs,
        &opts.gem_paths,
        opts.lockfile.as_deref(),
        opts.root_gem.as_ref(),
    )?;
    let t_parse_lower = t_start.elapsed();
    // The disclosure record is fully known once lowering resolved
    // every require. Write it (and warn) BEFORE analyze/codegen, so the ledger
    // lands even if a later stage fails.
    if let Some(path) = &opts.gem_report {
        gem_report::write_report(&gem_records, path)
            .map_err(|message| CompileError::Report { message })?;
    }
    // Computed from the arena BEFORE `analyze` consumes it: a whole-program
    // fact (does any eval site survive lowering?), so it belongs here rather
    // than downstream where the arena is already owned by `Analyzed`.
    let needs_prism_runtime = hir.needs_prism_runtime();
    // Reported with the timings because the arena is a first-class term in the
    // compiler's peak: one `HirNode` is as wide as the largest variant, so the
    // count times that width is a floor on what the front end holds.
    let (node_count, node_bytes) = (
        hir.all_nodes().len(),
        std::mem::size_of_val(hir.all_nodes()),
    );
    let t_analyze_start = std::time::Instant::now();
    memguard::set_phase(memguard::Phase::Analyze);
    let analyzed = analyze::analyze(hir, root)?;
    let t_analyze = t_analyze_start.elapsed();
    let t_codegen_start = std::time::Instant::now();
    memguard::set_phase(memguard::Phase::Codegen);
    let produced = match emit {
        Emit::Memory if opts.pretty => {
            Produced::Memory(codegen::codegen_to_string_pretty(&analyzed)?)
        }
        Emit::Memory => Produced::Memory(codegen::codegen_to_string(&analyzed)?),
        Emit::File(path) => {
            let file = std::fs::File::create(path)
                .map_err(|e| CompileError::codegen(format!("creating {}: {e}", path.display())))?;
            // Buffered: the writer is handed one token at a time, and an
            // unbuffered `File` would make each of those a syscall.
            let mut out = std::io::BufWriter::with_capacity(256 * 1024, file);
            match codegen::codegen_to_writer(&analyzed, &mut out) {
                Ok(stats) => Produced::File(stats),
                Err(e) => {
                    // Emission STREAMS, so a construct codegen refuses is only
                    // reported once a partial program is already on disk. Half
                    // a program still parses as a whole one, and whatever ran
                    // next would blame rustc for a file zeo knew was wrong.
                    drop(out);
                    let _ = std::fs::remove_file(path);
                    return Err(e);
                }
            }
        }
    };
    if timings_enabled() {
        let bytes = match &produced {
            Produced::Memory(s) => s.len() as u64,
            Produced::File(stats) => stats.bytes,
        };
        eprintln!(
            "zeo-timings: parse_lower={}ms analyze={}ms codegen={}ms total={}ms bytes={bytes} lines={} nodes={node_count} node_bytes={} peak_rss={}",
            t_parse_lower.as_millis(),
            t_analyze.as_millis(),
            t_codegen_start.elapsed().as_millis(),
            t_start.elapsed().as_millis(),
            match &produced {
                Produced::Memory(_) => 0,
                Produced::File(stats) => stats.lines,
            },
            // `0` for a compile that finished inside the poller's first
            // interval -- absent, not zero. `compile-bench` reads it as such.
            node_bytes,
            memguard::peak_bytes().unwrap_or(0),
        );
    }
    Ok((produced, needs_prism_runtime))
}
