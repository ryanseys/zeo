//! The `spinelc` CLI: argument parsing plus calling into the `spinelc`
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
}

enum Source {
    File(PathBuf),
    /// `-e <code>` (repeatable; joined with newlines, like `ruby -e`). Compiles
    /// AND runs immediately, forwarding stdout/stderr and the exit status --
    /// the shape the `ruby`-differential harness drives.
    Eval(String),
}

fn parse_args() -> Result<Args, String> {
    let mut input = None;
    let mut eval: Option<String> = None;
    let mut output = None;
    let mut print_rust = false;
    let mut load_roots = Vec::new();
    let mut package_dirs = Vec::new();
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
            // hit wins in the order given) -- see `spinelc::CompileOptions`.
            // The attached form `-I<dir>` is handled in the catch-all below,
            // matching ruby (both `-I lib` and `-Ilib` work).
            "-I" => {
                load_roots.push(PathBuf::from(iter.next().ok_or("-I requires a directory")?));
            }
            // An extra gem directory (repeatable; searched
            // before the default project-local/bundled ones). See
            // `spinelc::CompileOptions::package_dirs`.
            "--packages" => {
                package_dirs.push(PathBuf::from(
                    iter.next().ok_or("--packages requires a directory")?,
                ));
            }
            "-S" => print_rust = true,
            other => {
                // Attached `-I<dir>` (ruby's own spelling, no space).
                if let Some(dir) = other.strip_prefix("-I").filter(|d| !d.is_empty()) {
                    load_roots.push(PathBuf::from(dir));
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
        (None, None) => return Err(
            "usage: spinelc (<input.rb> | -e <code>) [-I <dir>]... [--packages <dir>]... [-o <output>] [-S]".to_string(),
        ),
    };
    Ok(Args {
        source,
        output,
        print_rust,
        load_roots,
        package_dirs,
    })
}

/// The default package-dir candidates appended AFTER any explicit
/// `--packages` dirs (explicit dirs get first-name-wins priority): the
/// input file's sibling `gems/` (project-local gems).
///
/// The compiler's OWN bundled `gems/` is not listed here: the loader appends
/// it unconditionally, so it is found whether spinelc is driven through this
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
    let opts = spinelc::CompileOptions {
        input_path: input_path.clone(),
        load_roots: args.load_roots.clone(),
        package_dirs,
    };
    let compiled = spinelc::compile_to_rust_with(&source, &opts)?;

    if args.print_rust {
        println!("{}", compiled.rust_source);
        return Ok(());
    }

    use spinelc::build::{build_binary, ensure_runtime_built, Linkage, Profile};

    // `-e`: compile to a throwaway binary, run it, and exit with ITS status
    // (stdout/stderr stream straight through) -- the differential-harness path.
    // Run-once, so it DEFAULTS to the fast-to-build `Debug` runtime; a harness
    // that compiles thousands of programs can flip this to `Release` via
    // `SPINELC_RUNTIME_PROFILE` for a ~12x faster per-program link.
    if matches!(args.source, Source::Eval(_)) {
        let profile = Profile::from_env_or(Profile::Debug);
        ensure_runtime_built(profile)?;
        let bin = std::env::temp_dir().join(format!("spinelc-e-{}", std::process::id()));
        build_binary(&compiled.rust_source, &bin, Linkage::from_env(), profile)?;
        let status = std::process::Command::new(&bin)
            .status()
            .map_err(|e| format!("running compiled program: {e}"))?;
        let _ = std::fs::remove_file(&bin);
        std::process::exit(status.code().unwrap_or(1));
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = match &args.source {
            Source::File(path) => path.clone(),
            Source::Eval(_) => unreachable!("handled above"),
        };
        p.set_extension("");
        p
    });
    // `spinelc foo.rb -o app` produces a SHIPPED binary: DEFAULT to the
    // release-profiled runtime (optimized + stripped) so the artifact is small and
    // fast, rather than embedding the unoptimized debug runtime. One-time `cargo
    // build --release -p spinel-rt` on first use. Overridable to `debug` via
    // `SPINELC_RUNTIME_PROFILE` (e.g. to symbolicate a runtime panic).
    let profile = Profile::from_env_or(Profile::Release);
    ensure_runtime_built(profile)?;
    build_binary(&compiled.rust_source, &output, Linkage::from_env(), profile)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("spinelc: {e}");
            ExitCode::FAILURE
        }
    }
}
