//! `zeo_rt_main`: the emitted C `main`'s tail call. Ports the generated
//! `main` sequence step for step -- registration (`register_program`), the
//! big-stack `run_main` around the compiled toplevel, `at_exit`, finalizers,
//! and the exit-status match -- and OWNS argv: under an emitted C `main`,
//! `std::env::args()` is empty on musl, so the seeds read the stash here.

use crate::{RubyValue, Signal};
use std::ffi::{CStr, c_char, c_void};
use std::mem::MaybeUninit;
use zeo_abi::ClassId;
use zeo_abi::abi::{ProgramDesc, STATUS_OK, STATUS_SIGNAL};

/// The whole program: returns the process exit status for the emitted
/// `main` to return. `corelib` is null until the corelib mechanism (G8).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_main(
    argc: i32,
    argv: *const *const c_char,
    prog: *const ProgramDesc,
    corelib: *const c_void,
) -> i32 {
    let status = unsafe { main_inner(argc, argv, prog, corelib) };
    // This RETURNS to the emitted C `main`, which is past everything that
    // would otherwise write the buffer out (see `exec::flush_stdio`).
    crate::exec::flush_stdio();
    status
}

unsafe fn main_inner(
    argc: i32,
    argv: *const *const c_char,
    prog: *const ProgramDesc,
    corelib: *const c_void,
) -> i32 {
    assert!(
        corelib.is_null(),
        "zeo_rt_main: corelib registration is not yet emitted (G8)"
    );
    crate::mark_sole_thread();
    crate::gc::configure_from_env();
    let args: Vec<String> = (0..usize::try_from(argc).unwrap_or(0))
        .map(|i| {
            unsafe { CStr::from_ptr(*argv.add(i)) }
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    crate::exec::stash_cli_args(args);

    let desc = unsafe { &*prog };
    unsafe { super::registry::register_program(desc) };
    if let Some(init) = desc.unit_init {
        unsafe { init() };
    }
    // A program that can `eval` names the compiler's installer, and only
    // such a program links the compiler at all -- see `ProgramDesc`.
    if let Some(install) = desc.eval_install {
        unsafe { install() };
    }

    let toplevel = desc.toplevel;
    let result = crate::run_main(move || {
        let mut out = MaybeUninit::<RubyValue>::uninit();
        if unsafe { toplevel(out.as_mut_ptr().cast()) } == STATUS_OK {
            let v = unsafe { out.assume_init() };
            super::leakcheck::consumed(&v);
            Ok(v)
        } else {
            Err(crate::signal::take_pending()
                .expect("the toplevel answered STATUS_SIGNAL with an empty pending slot"))
        }
    });
    let at_exit_status = crate::run_at_exit();
    crate::run_finalizers();
    super::leakcheck::check_at_exit();
    if let Err(signal) = result {
        match signal {
            // An uncaught SystemExit is not an error; any other uncaught
            // raise renders CRuby's full report. Either way an `at_exit`
            // override outranks the body's status.
            Signal::Raise(exc) => {
                if let Some(code) = crate::system_exit_status(&exc) {
                    return at_exit_status.unwrap_or(code);
                }
                crate::report_uncaught(&exc);
                return at_exit_status.unwrap_or(1);
            }
            // A top-level `return` ends the program silently, successfully.
            Signal::Return(_) => {}
            signal @ (Signal::Break(_)
            | Signal::Next(_)
            | Signal::Redo
            | Signal::Retry
            | Signal::Throw(_)
            | Signal::Terminate) => {
                eprintln!("uncaught signal escaped the top level: {signal:?}");
                return 1;
            }
        }
    }
    at_exit_status.unwrap_or(0)
}

/// `Kernel#at_exit`'s registration half: the handler proc is MOVED in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_at_exit_register(handler: *mut RubyValue) {
    crate::at_exit_register(unsafe { std::ptr::read(handler) });
}

/// The head-of-toplevel alias check for one bodiless class -- raises
/// `NameError` for an alias source that resolves nowhere. Status only; no
/// value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_validate_class_aliases(cid: u32) -> i32 {
    match crate::validate_class_aliases(ClassId(cid)) {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}
