//! The `spinelc` CLI: argument parsing plus calling into the `spinelc`
//! library's `compile_to_rust`/`build::build_binary` -- see `lib.rs` for the
//! actual parse -> analyze -> codegen -> build pipeline.

use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    input: PathBuf,
    output: Option<PathBuf>,
    print_rust: bool,
    load_roots: Vec<PathBuf>,
    package_dirs: Vec<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut input = None;
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
            // A `require` search root, like ruby's own -I (repeatable, first
            // hit wins in the order given) -- see `spinelc::CompileOptions`.
            "-I" => {
                load_roots.push(PathBuf::from(iter.next().ok_or("-I requires a directory")?));
            }
            // An extra spin.toml packages directory (repeatable; searched
            // before the default project-local/bundled ones). See
            // `spinelc::CompileOptions::package_dirs`.
            "--packages" => {
                package_dirs.push(PathBuf::from(
                    iter.next().ok_or("--packages requires a directory")?,
                ));
            }
            "-S" => print_rust = true,
            other => {
                if input.is_some() {
                    return Err(format!("unexpected argument `{other}`"));
                }
                input = Some(PathBuf::from(other));
            }
        }
    }
    Ok(Args {
        input: input.ok_or(
            "usage: spinelc <input.rb> [-I <dir>]... [--packages <dir>]... [-o <output>] [-S]",
        )?,
        output,
        print_rust,
        load_roots,
        package_dirs,
    })
}

/// The default package-dir candidates appended AFTER any explicit
/// `--packages` dirs (explicit dirs get first-name-wins priority): the
/// input file's sibling `packages/` (project-local packages), then the
/// compiler's own bundled `packages/` (the repo-root directory holding the
/// stdlib-style packages of Phases 14.3/14.4). The bundled path is baked in
/// via `CARGO_MANIFEST_DIR` -- honest for a dev-tree spike compiler
/// (both `cargo run` and the test harness live in the repo); an installed
/// distribution would locate it relative to the executable instead, the
/// reference project's approach.
fn default_package_dirs(input: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = input.parent() {
        dirs.push(parent.join("packages"));
    }
    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("packages");
    dirs.push(bundled);
    dirs
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let source = std::fs::read_to_string(&args.input)
        .map_err(|e| format!("reading {}: {e}", args.input.display()))?;

    let mut package_dirs = args.package_dirs.clone();
    package_dirs.extend(default_package_dirs(&args.input));
    let opts = spinelc::CompileOptions {
        input_path: Some(args.input.clone()),
        load_roots: args.load_roots.clone(),
        package_dirs,
    };
    let compiled = spinelc::compile_to_rust_with(&source, &opts)?;

    if args.print_rust {
        println!("{}", compiled.rust_source);
        return Ok(());
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = args.input.clone();
        p.set_extension("");
        p
    });
    spinelc::build::build_binary_with_deps(
        &compiled.rust_source,
        &compiled.native_deps,
        &output,
        spinelc::build::Linkage::from_env(),
    )
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
