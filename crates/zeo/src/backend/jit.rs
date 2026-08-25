//! The in-process JIT run path (`--backend jit`): the same CLIF lowering
//! as AOT finalized straight into this process's memory and entered
//! through the emitted C `main` -- no object file, no linker, no on-disk
//! binary. The runtime is the one already linked into `zeo` (decision 9);
//! emitted imports resolve through `zeo_rt::capi::symbols`.

use crate::diagnostics::CompileError;
use std::ffi::CString;
use std::os::raw::c_char;

/// Compile `analyzed` into executable memory, run it, and exit this
/// process with the program's status. Takes the analysis BY VALUE so the
/// HIR arena is dropped before the program's `main` runs -- the compiler's
/// memory is handed back first, exactly like the AOT child process
/// starting fresh. A codegen error's span is resolved against the file
/// table HERE, the last point the table is alive.
pub fn run(
    analyzed: crate::analyze::Analyzed,
    program_name: &str,
    program_args: &[String],
) -> Result<std::convert::Infallible, CompileError> {
    let jitted = crate::clif::emit::compile_jit(&analyzed)
        .map_err(|e| CompileError::from_codegen(e, &analyzed.compiler.hir.files))?;
    drop(analyzed);
    crate::memguard::set_phase(crate::memguard::Phase::Build);

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

    let main: unsafe extern "C" fn(i32, *const *const c_char) -> i32 =
        unsafe { std::mem::transmute(jitted.main) };
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
