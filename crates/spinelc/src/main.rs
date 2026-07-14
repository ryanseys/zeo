//! The `spinelc` CLI: argument parsing plus calling into the `spinelc`
//! library's `compile_to_rust`/`build::build_binary` -- see `lib.rs` for the
//! actual parse -> analyze -> codegen -> build pipeline.

use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    input: PathBuf,
    output: Option<PathBuf>,
    print_rust: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut input = None;
    let mut output = None;
    let mut print_rust = false;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => {
                output = Some(PathBuf::from(iter.next().ok_or("-o requires a path")?));
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
        input: input.ok_or("usage: spinelc <input.rb> [-o <output>] [-S]")?,
        output,
        print_rust,
    })
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let source = std::fs::read_to_string(&args.input)
        .map_err(|e| format!("reading {}: {e}", args.input.display()))?;

    let rust_source = spinelc::compile_to_rust(&source)?;

    if args.print_rust {
        println!("{rust_source}");
        return Ok(());
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = args.input.clone();
        p.set_extension("");
        p
    });
    spinelc::build::build_binary(&rust_source, &output)
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
