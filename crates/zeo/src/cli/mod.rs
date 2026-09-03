//! The `zeo` CLI: argument parsing plus calling into the compiler
//! (`run_jit_with` to run, `compile_to_object_with` + the `cc` link for an
//! artifact) -- see `lib.rs` for the parse -> lower -> analyze -> clif
//! pipeline.
//!
//! In the library rather than in `main.rs` so the api suite can drive a verb
//! without spawning a process, and so `src/` has one shape.

use std::io::IsTerminal;
use std::process::ExitCode;

// The subcommand drivers live in the library, where the parity probe reads
// the same text to run under CRuby.
use crate::gems::bundle::driver as subcommand_driver;

// One file per concern, and each reaches its siblings through this module --
// they all `use super::*`.
pub(crate) mod args;
pub(crate) mod backend;
/// `--enable`/`--disable`: ruby's own feature switches, and which of them
/// zeo answers.
pub mod features;
pub(crate) mod flags;
pub(crate) mod help;
pub(crate) mod install;
pub(crate) mod run;

pub(crate) use args::*;
pub(crate) use backend::*;
pub(crate) use flags::*;
pub(crate) use help::*;
pub(crate) use install::*;
pub(crate) use run::*;

/// What `run` can fail with: a compile error renders as a miette diagnostic
/// (annotated source excerpt, auto-degrading for pipes/NO_COLOR); everything
/// else (argument parsing, IO, the `cc` link step) keeps the
/// plain `zeo: <msg>` line.
pub(crate) enum MainError {
    Plain(String),
    Compile(crate::CompileError),
}

impl From<String> for MainError {
    fn from(msg: String) -> MainError {
        MainError::Plain(msg)
    }
}

impl From<crate::CompileError> for MainError {
    fn from(err: crate::CompileError) -> MainError {
        MainError::Compile(err)
    }
}

/// Whether zeo was invoked from an interactive terminal, which is what makes
/// a bare `zeo` a shell rather than an error. Both ends are asked: a piped
/// stdin has a program to read, and a redirected stdout has nothing to draw a
/// prompt on.
fn interactive_terminal() -> bool {
    // SAFETY: `isatty` reads a descriptor number and touches nothing else.
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

/// Replace this process with `bin`. `exec` rather than spawn-and-wait, so the
/// program keeps zeo's pid, its stdio, and its signal disposition -- the same
/// single process an in-process JIT run is.
///
/// `arg0` is the PROGRAM's name, not the cached binary's. `$0` is argv[0]
/// (`globals::seed_default_globals`), so without this a program would report
/// a path inside the cache: mkmf derives an extension's `srcdir` from `$0`,
/// and every generated Makefile came out pointing at the cache directory.
///
/// Returns only when the exec FAILED, which leaves the caller free to fall
/// back; the error is reported, because a cached binary that will not run is
/// worth knowing about even though the program still runs.
fn exec(bin: &std::path::Path, program_name: &str, program_args: &[String]) {
    use std::os::unix::process::CommandExt as _;
    let e = std::process::Command::new(bin)
        .arg0(program_name)
        .args(program_args)
        .exec();
    tracing::warn!("could not run the cached program {}: {e}", bin.display());
}

/// Install a `tracing` subscriber (stderr) for the compiler pipeline:
/// `--log-level` first, then `ZEO_LOG`, then `RUST_LOG` -- a flag is more
/// specific than an ambient variable, so it wins. All three take a full
/// `EnvFilter` directive (`zeo=debug`, `crate::analyze=trace`); `--log-level`
/// additionally accepts a bare level, which widens to the whole compiler.
/// With none of them set, no subscriber is installed, so every
/// `trace!`/`debug!`/`instrument` in the pipeline compiles to a cheap disabled
/// check -- a normal compile stays silent and never interleaves with the
/// miette diagnostics.
fn init_tracing(flag: Option<&str>) {
    let Some(directive) = flag
        .map(widen_bare_level)
        .or_else(|| std::env::var("ZEO_LOG").ok().filter(|s| !s.is_empty()))
        .or_else(|| std::env::var("RUST_LOG").ok().filter(|s| !s.is_empty()))
    else {
        return;
    };
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(directive))
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

/// `--log-level debug` means the whole compiler at debug, which is what the
/// six code comments naming this flag describe. A directive -- anything
/// carrying a `=` or a `,` -- is passed to `EnvFilter` untouched, so
/// `--log-level crate::analyze=trace` still narrows to one module.
fn widen_bare_level(v: &str) -> String {
    match v.contains(['=', ',']) {
        true => v.to_string(),
        false => format!("zeo={v},zeo_rt={v}"),
    }
}

pub fn main() -> ExitCode {
    // A diagnostic read by a machine must not be reflowed. miette hard-wraps at
    // the terminal width and breaks after `/` and `-`, so an absolute path in a
    // message arrives split across two lines -- and whoever rejoins them cannot
    // tell the inserted space from a real one. That silently defeated a
    // harness's path scrubber and recorded 12 rows carrying this machine's
    // home directory. A pipe gets the message on one line; a terminal keeps the
    // wrapping, and colour still degrades on its own either way.
    if !std::io::stderr().is_terminal() {
        let _ = miette::set_hook(Box::new(|_| {
            Box::new(miette::MietteHandlerOpts::new().wrap_lines(false).build())
        }));
    }
    // The compiler IS the runtime's evaluator. Every `eval` is a run-time
    // compile; there is no interpreter to fall back to.
    crate::eval::install();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(MainError::Plain(e)) => {
            eprintln!("zeo: {e}");
            ExitCode::FAILURE
        }
        Err(MainError::Compile(e)) => {
            // miette's report rendering: graphical with the source excerpt
            // on a terminal, degrading automatically when piped or under
            // NO_COLOR/TERM=dumb.
            eprintln!("{:?}", miette::Report::new(e));
            ExitCode::FAILURE
        }
    }
}
