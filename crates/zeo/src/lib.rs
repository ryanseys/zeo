//! `zeo`: parse -> lower -> analyze -> clif (Cranelift IR) -> backend.
//! One binary, like ruby's own `main.c` driver: the backend either runs
//! the compiled code in-process (JIT) or emits object files and shells
//! out to `cc` to link them against the prebuilt `libzeo.a` -- the same
//! reason ruby shells out to `cc`: the compiler proper never touches an
//! object-file format directly.
//!
//! Exposed as a library (not just a `main.rs` binary) so both the CLI and
//! the in-process test suite (`tests/`) can call `compile_to_object_with`,
//! `check_program_with`, and `run_jit_with` directly -- a subprocess is
//! only unavoidable for the final `cc` link of an AOT artifact (and for
//! running the resulting binary).

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
/// programs link. The eval hook (`zeo_rt::eval::EvalCompiler`) installs
/// through this path too.
pub use zeo_rt;

/// The one allocator per process image: `libzeo.a` is the
/// only Rust staticlib an AOT program ever links, so the allocator it runs
/// on is declared HERE -- the `zeo` binary and the test harnesses inherit
/// it. The front end is allocation-heavy (arena nodes, interner strings,
/// token streams), and mimalloc reliably beats system malloc on that shape.
#[global_allocator]
static GLOBAL_ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Linking the C-API crate is its whole installation (it contributes one
// `zeo_rt::capi_hooks` element), so it only has to be named once.
#[cfg(feature = "capi")]
use zeo_capi as _;

pub mod analyze;
pub mod analyze_error;
pub mod autopkg;
pub mod backend;
pub mod builtin_surface;
pub mod bundled;
pub mod cext;
pub mod clif;
pub mod codegen_error;
pub mod compiler;
pub(crate) mod debug_flags;
pub mod default_gems;
pub mod diagnostics;
pub mod dump;
pub mod eval;
pub mod gem_report;
pub mod home;
pub mod memguard;
pub mod package;

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
mod embed;
mod ffi_vocab;
mod guard_fold;
pub(crate) mod names;
pub mod parse;
pub mod progcache;
pub mod project;
pub mod ruby_features;
pub mod subcommand;
pub mod types;

/// What a compile is FOR.
///
/// `Program` is the ordinary whole-program compile: it owns a class
/// table, so a top-level `def` or `class` REGISTERS and is emitted as a
/// row the runtime installs at boot. `Eval` is one snippet handed to a
/// program that is already running (`crate::eval`), where the same `def`
/// has to install through the runtime at its document position -- which
/// is what the emitter already does for a `def` written somewhere analyze
/// could not register.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CompileMode {
    #[default]
    Program,
    /// A run-time `eval` snippet. `cref` says whether the caller's scope
    /// has a lexical class for the snippet to resolve against -- a
    /// RUN-TIME fact the front end needs, because a `@@x` with no cref is
    /// ruby's toplevel `RuntimeError` and that stands up at LOWER time.
    Eval { cref: bool },
}

impl CompileMode {
    /// Whether this compile is one `eval` snippet.
    #[must_use]
    pub fn is_eval(self) -> bool {
        matches!(self, CompileMode::Eval { .. })
    }
}

/// The compile-time file context `require` resolution needs --
/// see `parse::parse_and_lower_with`. `Default` (no path, no roots) keeps
/// the pathless `-e` behavior: `require_relative` then fails with
/// CRuby's own "cannot infer basepath", and a plain `require` finds nothing.
#[derive(Clone, Default, Hash)]
pub struct CompileOptions {
    /// The main file's own path -- the base for its `require_relative`s.
    pub input_path: Option<std::path::PathBuf>,
    /// The name `__FILE__` and every span report, for a source with no path
    /// on disk: a run-time `eval` is named `(eval at f.rb:14)` and has no
    /// directory of its own (CRuby's `require_relative` in an eval raises
    /// for exactly that reason). `None` -- and no `input_path` -- is ruby's
    /// `"-e"`.
    pub file_name: Option<std::path::PathBuf>,
    /// What the source's FIRST line is numbered. Zero for a file on disk;
    /// a run-time `eval` given a `line` argument numbers from it, and
    /// every span, backtrace row and `__LINE__` follows.
    ///
    /// SIGNED, because ruby allows a zero or negative `line`: ERB passes
    /// `0` for a source whose first line is its own `#coding` preamble, so
    /// the template's line 1 must number 1 and the preamble line 0.
    pub line_offset: i32,
    /// What this compile is FOR -- see [`CompileMode`].
    pub mode: CompileMode,
    /// Ordered `-I` search roots for plain `require "feature"`.
    pub load_roots: Vec<std::path::PathBuf>,
    /// Ordered directories of vendored gems (each subdirectory with a
    /// `.gemspec` is a gem), searched AFTER every `-I` root, first name wins;
    /// a nonexistent dir contributes nothing. The CLI fills this from
    /// `--gems` plus the input file's sibling `gems/`; the compiler's own
    /// libraries are appended by the loader itself -- see `main.rs`.
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
    /// Directories whose `.rb` files travel INSIDE the program
    /// (`--embed-sources`), so a `require` the compiler could not resolve
    /// finds them at run time without a filesystem.
    ///
    /// Empty by default, and deliberately: a hermetic binary is the choice
    /// zeo makes, and embedding every source a program can see would double
    /// the artifact for a tier most programs never reach.
    pub embed_sources: Vec<std::path::PathBuf>,
    /// Turn a `require`/`load` target the compiler cannot resolve into a
    /// COMPILE-time error (`--strict-static-require`), rather than leaving
    /// it to the run-time loader. What a build that wants to know its whole
    /// dependency graph statically asks for.
    pub strict_static_require: bool,
    /// Libraries to require BEFORE the program's first line, in the order
    /// given -- ruby's `-r`, repeatable.
    ///
    /// Spliced rather than prepended to the source: prepending would shift
    /// every line number the program reports, and ruby's own `-r` runs in a
    /// file of its own.
    pub required_libraries: Vec<String>,
    /// Compile ONE gem entry file as a separately linked
    /// package -- an object whose bodies are exported plus a row manifest --
    /// instead of a runnable program. See [`package`].
    pub package_build: Option<package::PackageBuild>,
    /// Packages to merge into this program. Each
    /// contributes its manifest rows to THIS compile's one `ProgramDesc`;
    /// the caller links each package's object beside the emitted one.
    pub use_packages: Vec<package::UsePackage>,
    /// Consult the machine package cache for every bundled gem this
    /// compile activates, and link the artifacts it holds
    /// (see [`autopkg`]). Set by the CLI's LINKING roads only -- the
    /// in-process JIT cannot link a package object -- and never for a
    /// package build (a package compiles alone). Off by default, so
    /// library callers and eval compiles keep the pure source road.
    pub auto_package: bool,
    /// Extra arguments for the `cc` link line (`--link <arg>`, repeatable;
    /// `ZEO_LINK_ARGS` is the env spelling), passed VERBATIM and in order
    /// after the platform libraries, before the dead-strip flag. What carries
    /// a payload section (`-Wl,-sectcreate,...`), an object, or a framework
    /// into the binary. Meaningful only for a linked artifact; the derived
    /// `Hash` puts them in the program cache's key.
    pub link_args: Vec<String>,
}

/// A gem named by the caller -- the public identity type `CompileOptions`
/// speaks, distinct from the loader's internal resolved model (which carries
/// roots, version, and provenance the caller doesn't have). A struct rather
/// than a bare `String` so a gem's name can't be confused with any other
/// string option, and so identity can grow fields (a version pin) without an
/// API break.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// A Cranelift-compiled program: one object file's bytes, ready for
/// `backend::link` (the `--backend aot` pipeline).
pub struct ObjectOutput {
    pub object: Vec<u8>,
    /// Whether the object carries DWARF. The LINK reads it: debug info in
    /// a Mach-O program lives in the object file the binary points back
    /// at, so a `-g` link keeps that object and keeps the symbol table
    /// that names it.
    pub debuginfo: bool,
    /// Whether the program loads a C extension. The LINK reads this too:
    /// an extension resolves every `rb_*` against the host at load, so the
    /// binary has to EXPORT the C API rather than dead-strip it.
    ///
    /// A flag rather than always exporting, because the export table is what
    /// `-dead_strip` prunes against: exporting everything unconditionally
    /// would keep the whole runtime in every hello-world.
    pub loads_cext: bool,
    /// Every source file this compile read, with the exact text it compiled.
    /// [`progcache`] writes them into a manifest so a later run can tell
    /// whether the cached binary is still the right answer.
    ///
    /// Collecting them costs an `Arc` bump per file, not a copy.
    pub inputs: Vec<progcache::Input>,
    /// Package objects the LINK must include beside this
    /// one (`--with-package`). Empty for every ordinary compile.
    pub extra_objects: Vec<std::path::PathBuf>,
    /// Extra `cc` arguments the LINK appends verbatim
    /// ([`CompileOptions::link_args`]).
    pub link_args: Vec<String>,
    /// Features a `require` names that resolve NOWHERE -- the loader's
    /// resolvability pre-scan. The package fallback reads this to tell a
    /// drop that recompiled from source apart from one that left the
    /// program unable to load the feature at all.
    pub unresolvable_requires: Vec<String>,
    /// Bundled gems this compile spliced that the package cache could not
    /// answer. The CLI builds each one AFTER the program succeeds
    /// ([`autopkg::build_and_cache`]), so the next compile links it.
    pub auto_package_misses: Vec<autopkg::Candidate>,
}

/// The Cranelift pipeline: front end, then `clif::emit`.
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

/// A precompiled artifact a compile refused and fell back from -- its
/// feature compiles from source instead. See
/// [`compile_to_object_with_package_fallback`].
pub struct DroppedPackage {
    pub feature: String,
    /// The refusal's first line -- why the artifact could not merge.
    pub reason: String,
}

/// [`compile_to_object_with`], with the package fallback tier: a compile
/// that REFUSES one of the named packages retries without that artifact,
/// so its require resolves from source like any other. Precompilation is
/// an optimization with a validity predicate, never a semantic change --
/// the program built after a drop is the program a source compile always
/// built.
///
/// Attribution rests on one contract: every package refusal (the
/// interface registration, the row merge, the host-edit checks) names its
/// package as `package '<feature>'`.
///
/// A drop stands only when the gem really recompiled from source. When
/// the retry instead DEFERRED the feature to the runtime loader -- no
/// source anywhere -- the program would be born unable to load it, so the
/// original refusal comes back as the error. `on_drop` fires once per
/// drop that stands, before this returns.
pub fn compile_to_object_with_package_fallback(
    source: &str,
    opts: &CompileOptions,
    debuginfo: bool,
    mut on_drop: impl FnMut(&DroppedPackage),
) -> Result<ObjectOutput, CompileError> {
    let mut opts = opts.clone();
    let mut drops: Vec<(DroppedPackage, CompileError)> = Vec::new();
    let outcome = loop {
        match compile_to_object_with(source, &opts, debuginfo) {
            Ok(compiled) => break Ok(compiled),
            Err(e) => {
                let msg = e.to_string();
                let refused = opts.use_packages.iter().position(|p| {
                    package::feature_of_manifest_text(&p.manifest_text)
                        .is_some_and(|f| msg.contains(&format!("package '{f}'")))
                });
                if let Some(i) = refused {
                    let p = opts.use_packages.remove(i);
                    let dropped = DroppedPackage {
                        feature: package::feature_of_manifest_text(&p.manifest_text)
                            .unwrap_or_else(|| "?".to_string()),
                        reason: msg.lines().next().unwrap_or_default().to_string(),
                    };
                    drops.push((dropped, e));
                    continue;
                }
                // A refusal naming a DISCOVERED artifact: auto-packaging
                // merged it inside the compile, so it is not in
                // `use_packages` and cannot be removed one at a time.
                // Retry once with discovery off -- the pure source road,
                // which is what every drop falls back to anyway.
                if opts.auto_package && msg.contains("package '") {
                    tracing::debug!(
                        "a discovered package artifact refused; recompiling from source: {}",
                        msg.lines().next().unwrap_or_default()
                    );
                    opts.auto_package = false;
                    continue;
                }
                break Err(e);
            }
        }
    };
    let compiled = outcome?;
    let unresolved = |feature: &str| {
        compiled
            .unresolvable_requires
            .iter()
            .any(|d| d == feature || d.starts_with(&format!("{feature}/")))
    };
    if let Some(i) = drops.iter().position(|(d, _)| unresolved(&d.feature)) {
        return Err(drops.swap_remove(i).1);
    }
    for (dropped, _) in &drops {
        on_drop(dropped);
    }
    Ok(compiled)
}

/// What the front end spent, for `ZEO_TIMINGS`. Recorded by
/// [`analyze_on_this_thread`] and reported by whichever emitter ran, since
/// only the emitter knows how much output there was.
///
/// The arena is a first-class term in the compiler's peak -- one `HirNode` is
/// as wide as the largest variant -- so the node count and its width ride
/// along.
pub(crate) struct FrontEnd {
    start: std::time::Instant,
    parse_lower: std::time::Duration,
    analyze: std::time::Duration,
    nodes: usize,
    node_bytes: usize,
}

impl FrontEnd {
    /// One `zeo-timings:` line, once the emitter knows its output size.
    /// `lines` is 0 where the emitter does not count them.
    pub(crate) fn report(&self, emit: std::time::Duration, bytes: u64, lines: u64) {
        if !timings_enabled() {
            return;
        }
        let (nodes, node_bytes) = (self.nodes, self.node_bytes);
        eprintln!(
            "zeo-timings: parse_lower={}ms analyze={}ms codegen={}ms total={}ms bytes={bytes} lines={lines} nodes={nodes} node_bytes={node_bytes} peak_rss={}",
            self.parse_lower.as_millis(),
            self.analyze.as_millis(),
            emit.as_millis(),
            self.start.elapsed().as_millis(),
            // `0` for a compile that finished inside the poller's first
            // interval -- absent, not zero. A reader treats it as such.
            memguard::peak_bytes().unwrap_or(0),
        );
    }
}

/// The shared Cranelift front half: parse, gem reporting, analyze -- what
/// every clif-backed entry (`aot` object, `jit` run, `--emit-clif`) does
/// before its own emission. Runs on the caller's (compile) thread.
fn analyze_on_this_thread(
    source: &str,
    opts: &CompileOptions,
) -> Result<(analyze::Analyzed, FrontEnd, autopkg::AutoPackages), CompileError> {
    let start = std::time::Instant::now();
    memguard::set_phase(memguard::Phase::ParseLower);
    let (mut hir, mut root, mut gem_records) = parse::parse_and_lower_with(source, opts)?;
    // First-use auto-packaging: the parse already DEFERRED every bundled
    // gem the machine cache holds (nothing of theirs spliced or lowered);
    // this loop merges those artifacts and re-parses. A rejection (an
    // artifact that cannot serve this compile whole) re-parses too, so
    // the gem splices after all. Bounded: hits and rejections only grow.
    let mut auto = autopkg::AutoPackages::default();
    if opts.auto_package && opts.package_build.is_none() && progcache::enabled() {
        // Rejections are per COMPILE; the compile thread is fresh per
        // compile, but a retry on the same thread must not inherit them.
        autopkg::clear_rejected();
        let mut local = opts.clone();
        for _ in 0..6 {
            let (new, rejected_any) = autopkg::consult_new(&hir, &local.use_packages, &mut auto);
            if new.is_empty() && !rejected_any {
                break;
            }
            local.use_packages.extend(new);
            (hir, root, gem_records) = parse::parse_and_lower_with(source, &local)?;
        }
        auto.use_packages = local.use_packages;
    }
    // A `require`/`load` that SURVIVED lowering is one the loader could not
    // resolve. `--strict-static-require` makes that an error HERE rather
    // than a run-time question -- the same predicate
    // `Hir::uses_runtime_eval` reads to decide whether the compiler travels
    // with the program.
    if opts.strict_static_require
        && let Some(name) = hir.unresolved_require()
    {
        return Err(CompileError::Report {
            message: format!(
                "`{name}` has a target this compile cannot resolve, and \
                 --strict-static-require refuses one"
            ),
        });
    }
    hir.loader.embedded_sources = embed::collect(&opts.embed_sources)?;
    let parse_lower = start.elapsed();
    if let Some(path) = &opts.gem_report {
        gem_report::write_report(&gem_records, path)
            .map_err(|message| CompileError::Report { message })?;
    }
    let (nodes, node_bytes) = (
        hir.all_nodes().len(),
        std::mem::size_of_val(hir.all_nodes()),
    );
    let t_analyze = std::time::Instant::now();
    memguard::set_phase(memguard::Phase::Analyze);
    let analyzed = analyze::analyze(hir, root)?;
    let analyze = t_analyze.elapsed();
    memguard::set_phase(memguard::Phase::Codegen);
    Ok((
        analyzed,
        FrontEnd {
            start,
            parse_lower,
            analyze,
            nodes,
            node_bytes,
        },
        auto,
    ))
}

fn compile_object_on_this_thread(
    source: &str,
    opts: &CompileOptions,
    debuginfo: bool,
) -> Result<ObjectOutput, CompileError> {
    let (analyzed, front, auto) = analyze_on_this_thread(source, opts)?;
    let t_emit = std::time::Instant::now();
    // Does any require in the whole program load a C extension? The arena is
    // the only place that is written down, and the LINK needs to know: an
    // extension resolves `rb_*` against the host, so the binary has to export
    // the C API rather than let `-dead_strip` prune it.
    let loads_cext = analyzed
        .compiler
        .hir
        .all_nodes()
        .iter()
        .any(|n| matches!(n, hir::HirNode::CExtLoaded { .. }));
    let object = clif::emit::compile(&analyzed, debuginfo)
        .map_err(|e| CompileError::from_codegen(e, &analyzed.compiler.hir.files))?;
    front.report(t_emit.elapsed(), object.len() as u64, 0);
    let inputs = analyzed
        .compiler
        .hir
        .files
        .iter()
        .map(|f| progcache::Input {
            // A package build's respelled file names its REAL path here, so
            // the cache manifest re-reads the file that actually exists.
            name: f.real_path.clone().unwrap_or_else(|| f.name.clone()),
            source: std::sync::Arc::clone(&f.source),
        })
        .collect();
    let unresolvable_requires = analyzed
        .compiler
        .hir
        .loader
        .unresolvable_requires
        .iter()
        .cloned()
        .collect();
    // The EFFECTIVE merge set: discovery may have widened the caller's
    // list, and the discovered artifacts' sources join the cache manifest
    // so a gem edit invalidates the cached program too.
    let linked = if auto.use_packages.is_empty() {
        &opts.use_packages
    } else {
        &auto.use_packages
    };
    let mut inputs: Vec<progcache::Input> = inputs;
    inputs.extend(auto.extra_inputs);
    Ok(ObjectOutput {
        object,
        debuginfo,
        loads_cext,
        inputs,
        extra_objects: linked.iter().map(|p| p.object.clone()).collect(),
        link_args: opts.link_args.clone(),
        unresolvable_requires,
        auto_package_misses: auto.misses,
    })
}

/// Ruby's `-c` / `--dump=syntax`: does this source PARSE?
///
/// Deliberately not `check_program_with`. Ruby's syntax check stops at the
/// parser -- it resolves no `require`, and a program it cannot otherwise
/// run still answers `Syntax OK` -- so anything zeo rejects LATER (an
/// unsupported construct, an unresolvable feature) must not be reported
/// here, or the two tools disagree about what "syntax" means.
pub fn check_syntax(source: &str) -> Result<(), CompileError> {
    let result = ruby_prism::parse(source.as_bytes());
    match result.errors().next() {
        None => Ok(()),
        // No `files` to resolve a span against: the check runs before any
        // arena exists, and prism's own message already names the place.
        Some(err) => Err(CompileError::lower(
            lower_error::LowerError::syntax(format!("parse error: {}", err.message())),
            &[],
        )),
    }
}

/// `analyze_program` reduced to its verdict: `Ok(())` if zeo accepts this
/// program, the typed error if it does not. For an accept/reject check that
/// has no use for the analysis itself.
pub fn check_program_with(source: &str, opts: &CompileOptions) -> Result<(), CompileError> {
    analyze_program(source, opts).map(|_| ())
}

/// `analyze_program` with the default options, for a caller that only wants
/// the verdict: `Ok(())` if zeo accepts this program, the rendered message if
/// it does not.
pub fn check_program(source: &str) -> Result<(), String> {
    analyze_program(source, &CompileOptions::default())
        .map(|_| ())
        .map_err(String::from)
}

/// The front end alone, on the compile thread: a whole PROGRAM in, its
/// analysis out.
///
/// What a caller wants when the question is "does zeo accept this?" rather
/// than "what does it emit?" -- loader policy, gem resolution, require
/// splicing and every analyze rejection, without paying for emission. The
/// test suite's accept/reject checks are all this shape, and the gem sweeps
/// would pay for object emission they never read.
pub fn analyze_program(
    source: &str,
    opts: &CompileOptions,
) -> Result<analyze::Analyzed, CompileError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("zeo-compile".into())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, || {
                analyze_on_this_thread(source, opts).map(|(a, _, _)| a)
            })
            .expect("spawning the compiler thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// The front end alone, on the CALLER's thread: one snippet in, its
/// analysis out. What a run-time `eval` compiles (see `crate::eval`) --
/// no loader roots, no gem report, and no compile thread, because a
/// snippet arrives on a Ruby thread that already has a stack.
pub fn analyze_snippet(
    source: &str,
    opts: &CompileOptions,
) -> Result<analyze::Analyzed, CompileError> {
    let (hir, root, _records) = parse::parse_and_lower_with(source, opts)?;
    analyze::analyze(hir, root)
}

/// The JIT run mode (`--backend jit`): compile in-process and run without
/// an object file, linker, or on-disk binary. Never returns on success --
/// the process exits with the program's status.
///
/// The compile happens on the compiler thread; the program then runs on the
/// CALLER's thread, which from the CLI is the process main thread -- the one
/// an AOT binary's top level runs on too (`zeo_rt::exec::run_main`), so the
/// two modes agree about `Thread.main`, and a macOS program may open a
/// window under either. The driver's `build.rs` sizes that thread's stack.
pub fn run_jit_with(
    source: &str,
    opts: &CompileOptions,
    program_name: &str,
    program_args: &[String],
) -> Result<std::convert::Infallible, CompileError> {
    let ready = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("zeo-compile".into())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, || {
                let (analyzed, _front, _auto) = analyze_on_this_thread(source, opts)?;
                backend::jit::compile(analyzed)
            })
            .expect("spawning the compiler thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })?;
    backend::jit::run(ready, program_name, program_args)
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
                let (analyzed, front, _auto) = analyze_on_this_thread(source, opts)?;
                let t_emit = std::time::Instant::now();
                let (_bytes, text) = clif::emit::compile_with_clif(&analyzed)
                    .map_err(|e| CompileError::from_codegen(e, &analyzed.compiler.hir.files))?;
                front.report(
                    t_emit.elapsed(),
                    text.len() as u64,
                    text.lines().count() as u64,
                );
                Ok(text)
            })
            .expect("spawning the compiler thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// Stack depth scales with source nesting depth, and `resolv` alone exceeds the
/// ~2 MiB a spawned thread gets by default -- the CLI survived only because a
/// main thread gets 8 MiB. Compiling on an explicitly-sized thread makes that
/// headroom the compiler's property rather than the caller's.
pub(crate) const COMPILE_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Whether `ZEO_TIMINGS` is set: the phase-timing report `FrontEnd::report`
/// writes as machine-parseable `zeo-timings:` stderr lines.
pub fn timings_enabled() -> bool {
    std::env::var_os("ZEO_TIMINGS").is_some()
}
