//! The `zeo` CLI: argument parsing plus calling into the `zeo`
//! library's `compile_to_rust`/`build::build_binary` -- see `lib.rs` for the
//! actual parse -> analyze -> codegen -> build pipeline.

use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    /// The input source: either a `.rb` file path or, with `-e`, an inline
    /// program string (`ruby -e`'s shape). Exactly one is required.
    source: Source,
    output: Option<PathBuf>,
    print_rust: bool,
    load_roots: Vec<PathBuf>,
    package_dirs: Vec<PathBuf>,
    /// `--no-report`: suppress the Phase-2b `zeo-gems.json` disclosure
    /// record. Default is ON for an artifact-producing compile; harnesses that
    /// already know substitutions happen pass this.
    no_report: bool,
    /// `--nowarn=<slug>`: suppress a disclosure warning category (repeatable).
    nowarn: std::collections::HashSet<String>,
    /// `--gem-path <dir>` + `--lockfile <Gemfile.lock>`: the external gem store
    ///. Explicit opt-in, must be given together.
    gem_path: Option<PathBuf>,
    lockfile: Option<PathBuf>,
}

enum Source {
    File(PathBuf),
    /// `-e <code>` (repeatable; joined with newlines, like `ruby -e`). Compiles
    /// AND runs immediately, forwarding stdout/stderr and the exit status --
    /// the shape the `ruby`-differential harness drives.
    Eval(String),
}

const HELP: &str = "\
zeo -- compile Ruby to a native binary

usage: zeo (<input.rb> | -e <code>) [options]

modes:
  <input.rb>            compile the file to a native binary (default output:
                        the input path with its extension stripped)
  -e <code>             compile and run an inline program immediately,
                        forwarding stdout/stderr and the exit status
                        (repeatable; snippets are joined with newlines);
                        with -o, write the binary instead of running it

options:
  -o <output>           where to write the compiled binary
  -S                    print the generated Rust source and exit (no build)
  -I <dir>              add a `require` search root, like ruby's -I
                        (repeatable; `-I<dir>` also accepted)
  --packages <dir>      an extra gem directory, searched before the default
                        project-local/bundled ones (repeatable)
  --gem-path <dir>      the external gem store; requires --lockfile
  --lockfile <path>     the Gemfile.lock resolving --gem-path versions
  --no-report           suppress the `zeo-gems.json` disclosure record
  --nowarn <slug>       suppress a disclosure warning category
                        (repeatable; `--nowarn=<slug>` also accepted)
  -h, --help            show this message

environment:
  ZEO_RUNTIME_PROFILE   `debug` or `release` -- override the runtime profile
                        (default: debug for -e, release for -o compiles)
";

fn parse_args() -> Result<Args, String> {
    let mut input = None;
    let mut eval: Option<String> = None;
    let mut output = None;
    let mut print_rust = false;
    let mut load_roots = Vec::new();
    let mut package_dirs = Vec::new();
    let mut no_report = false;
    let mut nowarn = std::collections::HashSet::new();
    let mut gem_path = None;
    let mut lockfile = None;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => {
                output = Some(PathBuf::from(iter.next().ok_or("-o requires a path")?));
            }
            // Inline program, `ruby -e` style: repeatable, lines joined.
            "-e" => {
                let code = iter.next().ok_or("-e requires a code string")?;
                match &mut eval {
                    Some(acc) => {
                        acc.push('\n');
                        acc.push_str(&code);
                    }
                    None => eval = Some(code),
                }
            }
            // A `require` search root, like ruby's own -I (repeatable, first
            // hit wins in the order given) -- see `zeo::CompileOptions`.
            // The attached form `-I<dir>` is handled in the catch-all below,
            // matching ruby (both `-I lib` and `-Ilib` work).
            "-I" => {
                load_roots.push(PathBuf::from(iter.next().ok_or("-I requires a directory")?));
            }
            // An extra gem directory (repeatable; searched
            // before the default project-local/bundled ones). See
            // `zeo::CompileOptions::package_dirs`.
            "--packages" => {
                package_dirs.push(PathBuf::from(
                    iter.next().ok_or("--packages requires a directory")?,
                ));
            }
            "-S" => print_rust = true,
            // Suppress the Phase-2b disclosure record (see `gem_report`).
            "--no-report" => no_report = true,
            // `--nowarn <slug>` -- suppress a disclosure warning category.
            "--nowarn" => {
                nowarn.insert(iter.next().ok_or("--nowarn requires a slug")?);
            }
            // The external gem store.
            "--gem-path" => {
                gem_path = Some(PathBuf::from(
                    iter.next().ok_or("--gem-path requires a directory")?,
                ));
            }
            "--lockfile" => {
                lockfile = Some(PathBuf::from(
                    iter.next().ok_or("--lockfile requires a path")?,
                ));
            }
            "--help" | "-h" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => {
                // Attached `-I<dir>` (ruby's own spelling, no space).
                if let Some(dir) = other.strip_prefix("-I").filter(|d| !d.is_empty()) {
                    load_roots.push(PathBuf::from(dir));
                } else if let Some(slug) = other.strip_prefix("--nowarn=") {
                    // Attached `--nowarn=<slug>` spelling.
                    nowarn.insert(slug.to_string());
                } else if input.is_some() {
                    return Err(format!("unexpected argument `{other}`"));
                } else {
                    input = Some(PathBuf::from(other));
                }
            }
        }
    }
    let source = match (eval, input) {
        (Some(_), Some(_)) => return Err("cannot combine -e with a file argument".to_string()),
        (Some(code), None) => Source::Eval(code),
        (None, Some(path)) => Source::File(path),
        // Bare `zeo` shows the full help like `--help`, but exits nonzero:
        // an invocation that compiled nothing must not look like success to a
        // caller that expected an artifact.
        (None, None) => {
            print!("{HELP}");
            std::process::exit(1);
        }
    };
    // The gem store is an opt-in PAIR -- one without the other can't resolve.
    if gem_path.is_some() != lockfile.is_some() {
        return Err("--gem-path and --lockfile must be given together".to_string());
    }

    Ok(Args {
        source,
        output,
        print_rust,
        load_roots,
        package_dirs,
        no_report,
        nowarn,
        gem_path,
        lockfile,
    })
}

/// The default package-dir candidates appended AFTER any explicit
/// `--packages` dirs (explicit dirs get first-name-wins priority): the
/// input file's sibling `gems/` (project-local gems).
///
/// The compiler's OWN bundled `gems/` is not listed here: the loader appends
/// it unconditionally, so it is found whether zeo is driven through this
/// CLI or used as a library. The bundled path is baked in
/// via `CARGO_MANIFEST_DIR` -- honest for a dev-tree spike compiler
/// (both `cargo run` and the test harness live in the repo); an installed
/// distribution would locate it relative to the executable instead, the
/// reference project's approach.
fn default_package_dirs(input: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = input.and_then(|p| p.parent()) {
        dirs.push(parent.join("gems"));
    }
    dirs
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let (source, input_path) = match &args.source {
        Source::File(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            (text, Some(path.clone()))
        }
        Source::Eval(code) => (code.clone(), None),
    };

    let mut package_dirs = args.package_dirs.clone();
    package_dirs.extend(default_package_dirs(input_path.as_deref()));

    // Phase 2b: an artifact-producing compile writes `zeo-gems.json` next to
    // its output and warns about substitutions -- UNLESS `--no-report`. The
    // `-e` path is a throwaway differential-harness run, so it stays silent
    // (no file, no warnings) regardless.
    let is_eval = matches!(args.source, Source::Eval(_));
    let gem_report = if args.no_report || is_eval {
        None
    } else {
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
    };
    let opts = zeo::CompileOptions {
        input_path: input_path.clone(),
        load_roots: args.load_roots.clone(),
        package_dirs,
        // At the CLI the warnings ride with the report: `--no-report` (the
        // harness "be quiet" flag) silences both, while `--nowarn <slug>`
        // trims individual categories with the report still on. The library
        // fields stay independent for callers that want a finer split.
        gem_warnings: gem_report.is_some(),
        gem_report,
        nowarn: args.nowarn.clone(),
        gem_path: args.gem_path.clone(),
        lockfile: args.lockfile.clone(),
    };
    let compiled = zeo::compile_to_rust_with(&source, &opts)?;

    if args.print_rust {
        println!("{}", compiled.rust_source);
        return Ok(());
    }

    use zeo::build::{Profile, Runtime, build_binary, ensure_runtime_built};

    // Which runtime variant this program's binary links: the lean, parser-free
    // default, or the prism-backed `eval-vm` one iff the compiler saw a runtime
    // eval site. The single mapping point for both build paths below.
    let runtime = Runtime::for_eval(compiled.needs_eval_vm);

    // `-e` without `-o`: compile to a throwaway binary, run it, and exit with
    // ITS status (stdout/stderr stream straight through) -- the
    // differential-harness path. Run-once, so it DEFAULTS to the fast-to-build
    // `Debug` runtime; a harness that compiles thousands of programs can flip
    // this to `Release` via `ZEO_RUNTIME_PROFILE` for a ~12x faster
    // per-program link. With `-o`, `-e` produces an artifact like the file
    // mode below instead of running.
    if matches!(args.source, Source::Eval(_)) && args.output.is_none() {
        let profile = Profile::from_env_or(Profile::Debug);
        ensure_runtime_built(profile, runtime)?;
        let bin = std::env::temp_dir().join(format!("zeo-e-{}", std::process::id()));
        build_binary(&compiled.rust_source, &bin, profile, runtime)?;
        let status = std::process::Command::new(&bin)
            .status()
            .map_err(|e| format!("running compiled program: {e}"))?;
        let _ = std::fs::remove_file(&bin);
        std::process::exit(status.code().unwrap_or(1));
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = match &args.source {
            Source::File(path) => path.clone(),
            Source::Eval(_) => unreachable!("-e without -o is handled above"),
        };
        p.set_extension("");
        p
    });
    // `zeo foo.rb -o app` produces a SHIPPED binary: DEFAULT to the
    // release-profiled runtime (optimized + stripped) so the artifact is small and
    // fast, rather than embedding the unoptimized debug runtime. One-time `cargo
    // build --release -p zeo-rt` on first use. Overridable to `debug` via
    // `ZEO_RUNTIME_PROFILE` (e.g. to symbolicate a runtime panic).
    let profile = Profile::from_env_or(Profile::Release);
    ensure_runtime_built(profile, runtime)?;
    build_binary(&compiled.rust_source, &output, profile, runtime)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zeo: {e}");
            ExitCode::FAILURE
        }
    }
}
