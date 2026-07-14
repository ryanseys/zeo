//! `spinelc`: parse -> lower -> analyze -> codegen -> `cargo build`. Mirrors
//! spinel's `main.c` driver -- a single binary, no separate parse/analyze/codegen
//! executables, and the final artifact is a genuine native binary produced by
//! shelling out to the real Rust toolchain (spinel shells out to `cc`; we shell
//! out to `cargo`, for the same reason: neither compiler touches machine code
//! or an object-file format directly).

mod analyze;
mod codegen;
mod compiler;
mod hir;
mod parse;
mod types;

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

    let (hir, root) = parse::parse_and_lower(&source)?;
    let analyzed = analyze::analyze(hir, root)?;
    let rust_source = codegen::codegen(&analyzed);

    if args.print_rust {
        println!("{rust_source}");
        return Ok(());
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = args.input.clone();
        p.set_extension("");
        p
    });
    build_binary(&rust_source, &output)
}

/// Compiles the generated Rust source against `spinel-rt` by dropping it
/// into a throwaway `cargo` project with a path dependency on the workspace
/// `spinel-rt` crate, then copying the resulting binary to `output`. Using
/// `cargo` (not a bare `rustc` invocation with hand-guessed rlib paths) is
/// the robust choice: it resolves `spinel-rt`'s own dependency graph
/// correctly even as that crate grows, exactly as spinel's `main.c` prefers
/// letting `cc` do real dependency/library resolution over hand-assembling
/// a link line.
fn build_binary(rust_source: &str, output: &std::path::Path) -> Result<(), String> {
    let spinel_rt_dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../spinel-rt"));

    let build_dir = std::env::temp_dir().join(format!("spinelc-build-{}", std::process::id()));
    let src_dir = build_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| e.to_string())?;

    let manifest = format!(
        "[package]\nname = \"spinelc-generated\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [[bin]]\nname = \"program\"\npath = \"src/main.rs\"\n\n\
         [dependencies]\nspinel-rt = {{ path = {:?} }}\n",
        spinel_rt_dir.to_string_lossy()
    );
    std::fs::write(build_dir.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    std::fs::write(src_dir.join("main.rs"), rust_source).map_err(|e| e.to_string())?;

    let status = std::process::Command::new("cargo")
        .args(["build", "--quiet", "--manifest-path"])
        .arg(build_dir.join("Cargo.toml"))
        .status()
        .map_err(|e| format!("running cargo: {e}"))?;

    if !status.success() {
        return Err(format!(
            "cargo build failed for generated program (source left at {})",
            build_dir.display()
        ));
    }

    let built = build_dir.join("target/debug/program");
    std::fs::copy(&built, output)
        .map_err(|e| format!("copying {} to {}: {e}", built.display(), output.display()))?;
    let _ = std::fs::remove_dir_all(&build_dir);
    Ok(())
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
