//! `zeo <file>`: the ordinary compile, and the program cache in front of it.

use super::*;

pub(crate) fn run() -> Result<(), MainError> {
    let mut args = match parse_args()? {
        Parsed::Run(args) => args,
        Parsed::Install(cmd) => return run_install(cmd),
        Parsed::Flags(cmd) => return run_flags(cmd),
        Parsed::Backend(cmd) => return run_backend(cmd),
        Parsed::GemPrecompile => return run_gem_precompile(),
        Parsed::Help => {
            print_help();
            return Ok(());
        }
        Parsed::Version => {
            println!("zeo {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Parsed::NoInput => {
            print_help();
            std::process::exit(1);
        }
    };
    init_tracing(args.log_level.as_deref());
    // Pre-flight: a broken install (payload missing next to the executable)
    // reports here as an ordinary error instead of panicking mid-compile.
    crate::home::ensure_resolved()?;
    let (source, input_path) = match &args.source {
        Source::File(path) => (crate::parse::read_source(path)?, Some(path.clone())),
        Source::Eval(code) => (code.clone(), None),
        Source::Irb => (IRB_DRIVER.to_string(), None),
    };
    // A package build compiles its entry as a FEATURE
    // UNIT, not as `<main>` -- the main source is empty and the entry rides
    // `CompileOptions::package_build` into the loader.
    let source = if args.pkg_feature.is_some() {
        String::new()
    } else {
        source
    };
    // Armed before the compile, not inside it: the ceiling covers the whole
    // compile, emission, and link. The
    // library entry point deliberately does NOT arm one -- an in-process
    // caller (the test harness) owns its own process and must not have it
    // exited out from under it.
    crate::memguard::arm(&match &args.source {
        Source::File(path) => path.display().to_string(),
        Source::Eval(_) => "-e".to_string(),
        Source::Irb => "irb".to_string(),
    });

    let mut package_dirs = args.package_dirs.clone();
    package_dirs.extend(default_package_dirs(input_path.as_deref()));

    // A store asked for by FLAG must be usable; one assembled purely from
    // ambient env degrades to inactive when its lockfile is missing.
    if let Some(lock) = &args.lockfile
        && !lock.is_file()
    {
        if args.store_from_flags {
            return Err(
                format!("lockfile {} not found (run `bundle lock`?)", lock.display()).into(),
            );
        }
        args.gem_paths = Vec::new();
        args.lockfile = None;
    }

    let gem_report = match &args.report {
        Report::Off => None,
        Report::Path(path) => Some(path.clone()),
        Report::DefaultPath => {
            let artifact = args.output.clone().unwrap_or_else(|| {
                let mut p = input_path.clone().expect("a file source always has a path");
                p.set_extension("");
                p
            });
            let dir = artifact
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            Some(dir.join("zeo-gems.json"))
        }
    };
    // The package options. A package build takes the
    // positional file as its ENTRY and `-o` as its object; the manifest
    // lands beside the object as `<output>.zman`. A host names package
    // artifacts with `--with-package`; their manifests are read here
    // so the compile is a function of their TEXT (and the object digest
    // keeps the program cache honest about a body-only rebuild).
    let package_build = match &args.pkg_feature {
        Some(feature) => {
            let Source::File(entry) = &args.source else {
                return Err("--package needs a gem entry file".to_string().into());
            };
            let Some(out) = &args.output else {
                return Err("--package needs -o <artifact path>".to_string().into());
            };
            Some(crate::packages::package::PackageBuild {
                entry: entry.clone(),
                feature: feature.clone(),
                manifest_out: out.with_extension("zman"),
                root: entry
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .canonicalize()
                    .map_err(|e| format!("resolving {}: {e}", entry.display()))?,
            })
        }
        None => None,
    };
    // The store tier: a locked project's precompiled artifacts link instead
    // of recompiling. Only a compile with an ACTIVE store consults it (the
    // same pairing rule the store roots follow), a package build never does
    // (a package compiles alone), and an artifact the merge later refuses
    // drops back to a source compile with a warning.
    let mut package_roots: std::collections::HashMap<std::path::PathBuf, std::path::PathBuf> =
        Default::default();
    if package_build.is_none()
        && !args.gem_paths.is_empty()
        && let Some(lock) = &args.lockfile
    {
        for (artifact, root) in crate::gems::project::store_linkable(lock, &args.gem_paths) {
            if !args.with_packages.contains(&artifact) {
                if let Some(root) = root {
                    package_roots.insert(artifact.clone(), root);
                }
                args.with_packages.push(artifact);
            }
        }
    }
    let use_packages: Vec<crate::packages::package::UsePackage> = args
        .with_packages
        .iter()
        .map(|obj| {
            // Two artifact spellings: a bare object with the manifest
            // beside it, or the single-file `.zeopkg` bundle. A bundle's
            // object lands in the content-addressed pool, where the link
            // line can name it.
            if obj.extension().is_some_and(|e| e == "zeopkg") {
                let (manifest_text, bytes) = crate::packages::package::read_zeopkg(obj)?;
                let object_digest = crate::packages::package::fnv64(&bytes);
                let object = crate::packages::progcache::pkg_object_file(object_digest, &bytes)
                    .map_err(|e| format!("unpacking {}: {e}", obj.display()))?;
                return Ok(crate::packages::package::UsePackage {
                    manifest_path: obj.clone(),
                    manifest_text,
                    object,
                    object_digest,
                    source_root: package_roots.get(obj).cloned(),
                });
            }
            let manifest_path = obj.with_extension("zman");
            let manifest_text = std::fs::read_to_string(&manifest_path)
                .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
            let bytes =
                std::fs::read(obj).map_err(|e| format!("reading {}: {e}", obj.display()))?;
            Ok(crate::packages::package::UsePackage {
                manifest_path,
                manifest_text,
                object: obj.clone(),
                object_digest: crate::packages::package::fnv64(&bytes),
                source_root: package_roots.get(obj).cloned(),
            })
        })
        .collect::<Result<_, String>>()?;
    let opts = crate::CompileOptions {
        input_path: input_path.clone(),
        // A package build's MAIN source is synthetic (empty -- the entry
        // rides in as a feature unit), so it carries a synthetic name: the
        // entry path would otherwise register with empty text, and the
        // package cache's manifest could never verify that row.
        file_name: package_build
            .as_ref()
            .map(|pb| std::path::PathBuf::from(format!("<package {}>", pb.feature))),
        line_offset: 0,
        mode: crate::CompileMode::Program,
        load_roots: args.load_roots.clone(),
        package_dirs,
        gem_report,
        gem_paths: args.gem_paths.clone(),
        lockfile: args.lockfile.clone(),
        root_gem: args.root_gem.clone().map(crate::Gem::named),
        embed_sources: args.embed_sources.clone(),
        strict_static_require: args.strict_static_require,
        required_libraries: args.required_libraries.clone(),
        loaded_features: Vec::new(),
        package_build,
        use_packages,
        // The LINKING roads flip this on below (`link_opts`); a package
        // build compiles alone and the in-process JIT links nothing.
        auto_package: false,
        link_args: args.link_args.clone(),
    };
    // Parse only, then say so -- ruby's `Syntax OK`, byte for byte. A syntax
    // error reports itself the way every other compile error does, so the
    // exit status separates the two.
    if args.check_syntax {
        crate::check_syntax(&source)?;
        println!("Syntax OK");
        return Ok(());
    }
    // The two front-end reports, before anything is emitted: which files
    // became units and under which spellings, and which classes wait for a
    // unit to reveal them. Both are analyze-level answers, so both cost the
    // front end and nothing more -- seconds against the minute a full
    // compile of a 500-unit gem takes.
    if let Some(kind) = &args.dump_front_end {
        let analyzed = crate::analyze_program(&source, &opts)?;
        print!(
            "{}",
            match kind {
                FrontEndDump::Units => crate::dump::units(&analyzed),
                FrontEndDump::Classes(filter) => crate::dump::classes(&analyzed, filter.as_deref()),
                FrontEndDump::Methods(top) => crate::dump::methods(&analyzed, *top),
            }
        );
        return Ok(());
    }
    if let Some(target) = &args.emit_clif {
        let clif = crate::compile_to_clif_text(&source, &opts)?;
        match target {
            EmitTarget::Stdout => print!("{}", clif.text),
            EmitTarget::File(path) => std::fs::write(path, &clif.text)
                .map_err(|e| format!("writing {}: {e}", path.display()))?,
        }
        if let Some(path) = &args.emit_zeodata {
            clif.sidecar
                .map_err(|why| format!("--emit-zeodata: {why}"))?
                .write(path)?;
        }
        return Ok(());
    }

    // The backend decides whether this compile runs in place or produces an
    // object file, so it is selected before compiling.
    let wants_artifact = args.compile || args.output.is_some();
    let backend = crate::backend::Backend::select(args.backend, wants_artifact)?;
    // The JIT is run-in-place by definition: compile into this process and
    // exit with the program's status. An artifact request needs a backend
    // that produces one.
    // A package build writes its artifact and stops -- there
    // is nothing to run or link. It goes through the machine-wide package
    // cache first.
    if opts.package_build.is_some() {
        let out = args.output.as_ref().expect("--package checked -o above");
        return build_package(&source, &opts, out);
    }
    // The LINKING roads consult the first-use package cache (the cached
    // binary and the AOT artifact both link objects); the in-process JIT
    // cannot, so its options keep the flag off.
    let link_opts = {
        let mut o = opts.clone();
        o.auto_package = crate::packages::autopkg::enabled();
        o
    };
    if backend == crate::backend::Backend::Jit {
        // Silently honouring nothing is the one answer that would be
        // wrong: DWARF describes an artifact, and the in-process JIT
        // leaves none behind.
        if args.debuginfo {
            return Err(
                "-g describes a compiled artifact and the in-process JIT produces none (use -o/--compile)"
                    .to_string()
                    .into(),
            );
        }
        if wants_artifact {
            return Err(
                "--backend jit runs in place and produces no artifact (use --backend aot for -o/--compile)"
                    .to_string()
                    .into(),
            );
        }
        // The cache runs a program AHEAD of time and execs it, so an
        // unchanged program never compiles twice. It is tried first, and
        // falling through to the JIT below is the answer whenever it cannot
        // help -- see `run_from_cache`.
        let program_name = match &args.source {
            Source::File(path) => path.display().to_string(),
            Source::Eval(_) => "-e".to_string(),
            Source::Irb => "irb".to_string(),
        };
        if args.backend.is_none() && crate::packages::progcache::enabled() {
            run_from_cache(&source, &link_opts, &program_name, &args.program_args);
        }
        // The cache's linked binary took them (they are in its key); the
        // in-process JIT links nothing, so here they would be dropped.
        if !opts.link_args.is_empty() {
            eprintln!(
                "zeo: warning: --link arguments apply to a linked binary only; \
                 this run is in-process (use -o/--compile)"
            );
        }
        if !opts.use_packages.is_empty() {
            return Err(
                "--with-package needs the AOT path: the in-process JIT cannot \
                 link a package object (use -o/--compile, or leave the program cache on)"
                    .to_string()
                    .into(),
            );
        }
        match crate::run_jit_with(&source, &opts, &program_name, &args.program_args)? {}
    }
    let compiled = match backend {
        crate::backend::Backend::Aot => {
            compile_dropping_refused_packages(&source, &link_opts, args.debuginfo)?
        }
        crate::backend::Backend::Jit => unreachable!("the jit branch above never falls through"),
    };
    build_missed_packages(&compiled);
    crate::memguard::set_phase(crate::memguard::Phase::Build);
    let program = crate::backend::CompiledProgram::Aot(&compiled);

    // The default mode -- a bare file or `-e`, no artifact asked for: build a
    // throwaway program, run it, and exit with ITS status (stdout/stderr
    // stream straight through), like ruby. With `-o` or `--compile`, produce
    // an artifact instead. The mode bodies live in `backend`; this is just the
    // mode decision.
    if !args.compile && args.output.is_none() {
        match crate::backend::run_program(&program, &args.program_args)? {}
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = match &args.source {
            Source::File(path) => path.clone(),
            Source::Eval(_) | Source::Irb => {
                unreachable!("a pathless source always has -o")
            }
        };
        p.set_extension("");
        p
    });
    Ok(crate::backend::build_artifact(&program, &output)?)
}

/// Package every bundled gem the compile spliced because the cache could
/// not answer, so the NEXT compile links it. After the program's own
/// success only -- a broken program should not pay for gem builds -- and
/// best-effort: a failed build is a log line, never a failed run.
pub(crate) fn build_missed_packages(compiled: &crate::ObjectOutput) {
    for cand in &compiled.auto_package_misses {
        if let Err(e) = crate::packages::autopkg::build_and_cache(cand) {
            tracing::warn!("could not package '{}': {e}", cand.feature);
        }
    }
}

/// The library's package-fallback compile, with each drop reported to
/// stderr as it happens -- so the warning still lands when a later error
/// (a feature whose source is nowhere) ends the compile.
pub(crate) fn compile_dropping_refused_packages(
    source: &str,
    opts: &crate::CompileOptions,
    debuginfo: bool,
) -> Result<crate::ObjectOutput, crate::CompileError> {
    crate::compile_to_object_with_package_fallback(source, opts, debuginfo, |d| {
        eprintln!(
            "zeo: warning: dropping the precompiled artifact for '{}' and \
             compiling it from source: {}",
            d.feature, d.reason
        );
    })
}

/// Run this program from the compiled-program cache, and never return.
///
/// RETURNS when the cache cannot answer, and every such path is a fall-through
/// to the in-process JIT rather than an error. That is the whole safety
/// argument for making this the default: the cache is an accelerator, and a
/// program it cannot build ahead of time runs exactly the way it always did.
/// Three shapes reach the JIT --
///
/// * `--report` writes a file the compile produces, which a cache HIT would
///   silently skip;
/// * the object compile refuses (a program that needs the compiler in its own
///   process is the JIT-only tier, `tests/jit/`), or the link does;
/// * writing into the cache directory fails.
///
/// A compile ERROR reaches the JIT too, which reports the same error one front
/// end later. A wrong program is slow to fail here, which is the right way
/// round.
pub(crate) fn run_from_cache(
    source: &str,
    opts: &crate::CompileOptions,
    program_name: &str,
    program_args: &[String],
) {
    if opts.gem_report.is_some() {
        return;
    }
    let key = crate::packages::progcache::key(source, opts);
    if let Some(bin) = crate::packages::progcache::lookup(&key) {
        exec(&bin, program_name, program_args);
        return;
    }
    let Ok(compiled) = compile_dropping_refused_packages(source, opts, false) else {
        return;
    };
    let Ok(bin) = crate::packages::progcache::reserve(&key) else {
        return;
    };
    let program = crate::backend::CompiledProgram::Aot(&compiled);
    if crate::backend::build_artifact(&program, &bin).is_err() {
        return;
    }
    // The manifest is what makes the entry a HIT next time. Without it the
    // binary is there and unread, which costs a rebuild and nothing else.
    if let Err(e) = crate::packages::progcache::commit(&key, &compiled.inputs) {
        tracing::warn!("could not record the program cache manifest: {e}");
    }
    build_missed_packages(&compiled);
    exec(&bin, program_name, program_args);
}
