//! Turns a compiled program into something that runs.
//!
//! Two modes live here, dispatched by [`Backend`]. [`jit`] finalizes
//! Cranelift output into this process and runs it in place -- the default for
//! run mode. [`object`] + [`link`] write a Cranelift object file and link it
//! against `libzeo.a` -- the default for `-o`/`--compile`, and what ships.
//!
//! There was a third, which emitted Rust text and shelled out to `rustc`. It
//! was zeo's original backend, then its differential oracle, and it was
//! retired on 2026-08-21 once the Cranelift path had been the product for a
//! release and the corpus agreed with CRuby on both. The branch
//! `archive/rustc-backend` keeps it readable.

pub mod jit;
pub mod link;
pub mod object;

use std::path::Path;

/// Which mode the Cranelift backend runs in.
///
/// `Aot`: HIR -> CLIF -> object file (`clif/`), linked against `libzeo.a`
/// (`link.rs`). `Jit`: the same CLIF finalized into THIS process's memory and
/// run in place (`jit.rs`) -- run mode only, no artifact.
///
/// There was a third, `Rustc`, which emitted Rust text and shelled out to
/// `rustc`. It was the original backend and then the differential oracle, and
/// it was retired on 2026-08-21; the branch `archive/rustc-backend` keeps it
/// readable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Aot,
    Jit,
}

impl Backend {
    /// A `--backend`/`ZEO_BACKEND` value.
    pub fn parse(value: &str) -> Result<Backend, String> {
        match value {
            "aot" => Ok(Backend::Aot),
            "jit" => Ok(Backend::Jit),
            other => Err(format!("unknown backend `{other}` (expected aot or jit)")),
        }
    }

    /// The backend this invocation uses: the CLI flag, else `ZEO_BACKEND`,
    /// else the default for the MODE. Cranelift is the default backend, and
    /// the two Cranelift modes are not interchangeable: the JIT runs a
    /// program in place, and only the AOT backend produces the artifact
    /// `-o`/`--compile` asks for.
    pub fn select(cli: Option<Backend>, wants_artifact: bool) -> Result<Backend, String> {
        if let Some(backend) = cli {
            return Ok(backend);
        }
        match std::env::var("ZEO_BACKEND") {
            Ok(value) if !value.is_empty() => Backend::parse(&value),
            Ok(_) | Err(_) => Ok(if wants_artifact {
                Backend::Aot
            } else {
                Backend::Jit
            }),
        }
    }
}

/// A compiled program in whichever form its backend produced -- the input
/// the two mode entries below dispatch on.
pub enum CompiledProgram<'a> {
    Aot(&'a crate::ObjectOutput),
}

/// A name in the shared temp directory that no other compile can claim.
///
/// The process id ALONE is not enough, and the difference is a real bug: a
/// run-mode binary is written, executed and then deleted under one name, so
/// two compiles landing on it can have one delete or replace the other's
/// program while it runs. Ids are reused, and a suite spawning a compile per
/// case reuses them quickly.
pub(crate) fn scratch_name(stem: &str) -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{stem}-{}-{nanos:x}-{seq}", std::process::id())
}

/// Run mode (`zeo file.rb`, `zeo -e`): produce a throwaway program for
/// `compiled`, run it with `program_args`, and exit this process with the
/// program's status. Never returns on success.
///
pub fn run_program(
    compiled: &CompiledProgram<'_>,
    program_args: &[String],
) -> Result<std::convert::Infallible, String> {
    match compiled {
        // Aot run mode links a throwaway binary and runs it; the in-process
        // JIT is what an ordinary `zeo file.rb` takes.
        CompiledProgram::Aot(compiled) => {
            let bin = std::env::temp_dir().join(scratch_name("zeo-e"));
            object::object_to_binary(
                &compiled.object,
                compiled.debuginfo,
                compiled.loads_cext,
                &compiled.extra_objects,
                &compiled.link_args,
                &bin,
            )?;
            let status = std::process::Command::new(&bin)
                .args(program_args)
                .status()
                .map_err(|e| format!("running compiled program: {e}"))?;
            let _ = std::fs::remove_file(&bin);
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}

/// Artifact mode (`zeo -o app file.rb`, `--compile`): produce the SHIPPED
/// binary at `output` for `compiled`.
pub fn build_artifact(compiled: &CompiledProgram<'_>, output: &Path) -> Result<(), String> {
    match compiled {
        CompiledProgram::Aot(compiled) => object::object_to_binary(
            &compiled.object,
            compiled.debuginfo,
            compiled.loads_cext,
            &compiled.extra_objects,
            &compiled.link_args,
            output,
        ),
    }
}
