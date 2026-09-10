//! The in-process JIT run path (`--backend jit`): the same CLIF lowering
//! as AOT finalized straight into this process's memory and entered
//! through the emitted C `main` -- no object file, no linker, no on-disk
//! binary. The runtime is the one already linked into `zeo`; emitted
//! imports resolve through `zeo_rt::capi::symbols`.

use crate::diagnostics::CompileError;
use std::ffi::CString;
use std::os::raw::c_char;

/// A program finalized into this process's memory, waiting to be entered.
///
/// Built on the compiler thread (`Emitter` and the HIR arena need its 64
/// MiB), run on the process main thread -- the thread AppKit and friends
/// demand, and the one `zeo_rt::exec::run_main` runs the top level on for a
/// macOS program. The handoff is the only reason this type exists.
pub struct Ready {
    jitted: crate::clif::emit::Jitted,
}

// SAFETY: `Jitted` holds a `JITModule` (boxed lookup closures without a
// `Send` bound, so the auto trait is withheld) and a raw code pointer. Neither
// is bound to the thread that built it: the module's memory is a process-wide
// mapping, the closures are plain functions over `'static` data, and the
// pointer names a finalized function in that mapping. The value crosses
// exactly once, from a compiler thread that has already been joined to the
// thread that runs it, and no reference stays behind.
unsafe impl Send for Ready {}

/// Compile `analyzed` into executable memory. Takes the analysis BY VALUE so
/// the HIR arena is dropped before the program's `main` runs -- the
/// compiler's memory is handed back first, exactly like the AOT child process
/// starting fresh. A codegen error's span is resolved against the file table
/// HERE, the last point the table is alive.
pub fn compile(analyzed: crate::analyze::Analyzed) -> Result<Ready, CompileError> {
    let jitted = crate::clif::emit::compile_jit(&analyzed)
        .map_err(|e| CompileError::from_codegen(e, &analyzed.compiler.hir.files))?;
    drop(analyzed);
    crate::memguard::set_phase(crate::memguard::Phase::Build);
    Ok(Ready { jitted })
}

/// Run a compiled program on the calling thread and exit this process with
/// its status.
pub fn run(
    ready: Ready,
    program_name: &str,
    program_args: &[String],
) -> Result<std::convert::Infallible, CompileError> {
    let Ready { jitted } = ready;

    // argv as the program sees it: `$0` = the script (ruby's shape -- the
    // AOT binary's argv[0] is its own path only because a binary exists).
    let c_arg = |s: &str| {
        CString::new(s)
            .map_err(|_| CompileError::codegen(format!("argument contains a NUL byte: {s:?}")))
    };
    let mut argv_owned: Vec<CString> = Vec::with_capacity(program_args.len() + 1);
    argv_owned.push(c_arg(program_name)?);
    for arg in program_args {
        argv_owned.push(c_arg(arg)?);
    }
    let argv: Vec<*const c_char> = argv_owned.iter().map(|s| s.as_ptr()).collect();
    let argc = i32::try_from(argv.len()).expect("argc fits i32");

    // SAFETY: `jitted.main` is the finalized address of the emitted `main`,
    // which the emitter gives exactly this signature, in JIT memory `jitted`
    // keeps alive across the call.
    let main: unsafe extern "C" fn(i32, *const *const c_char) -> i32 =
        unsafe { std::mem::transmute(jitted.main) };
    // SAFETY: `argv` holds `argc` pointers to the NUL-terminated strings
    // `argv_owned` owns, and both outlive the call.
    let status = unsafe { main(argc, argv.as_ptr()) };

    // The emitted `main` returned; the module (and the code it owns) is
    // done. Flush what a C `exit` would flush, then leave with the
    // program's status -- same tail as the AOT binary.
    drop(argv);
    drop(argv_owned);
    drop(jitted);
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(status);
}
