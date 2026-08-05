//! Program entry: the generated program's top level runs on a dedicated
//! big-stack thread (the OS main thread just joins it -- see `run_main`),
//! with its scheduling context installed and the process Gvl held for the
//! duration (see `gvl` -- the Gvl is DISABLED by default, so "held" is free
//! and Ruby `Thread`s run truly parallel; `ZEO_GVL=1` arms CRuby-fidelity
//! serialized scheduling). Every `Thread.new` is its own OS thread
//! (`thread`); `Fiber`'s corosensei coroutines nest inside whichever thread
//! resumes them.
//!
//! `Kernel#at_exit` handlers run in REVERSE registration order (CRuby's
//! rule) after the top-level body finishes -- including via `exit` (see
//! `kernel_exit`) and after an uncaught exception. An exception raised
//! INSIDE a handler is swallowed after the remaining handlers run (CRuby
//! reports it; a silent skip is this runtime's approximation --
//! TODO(plan P-A): report through the exception-message machinery).

use crate::{RubyValue, Signal};

static AT_EXIT: parking_lot::Mutex<Vec<RubyValue>> = parking_lot::Mutex::new(Vec::new());

pub fn at_exit_register(handler: RubyValue) {
    AT_EXIT.lock().push(handler);
}

pub fn run_at_exit() {
    loop {
        let Some(h) = AT_EXIT.lock().pop() else { break };
        if let RubyValue::Proc(p) = h {
            let _ = p.call(&[]);
        }
    }
}

pub fn run_main<F>(body: F) -> Result<RubyValue, Signal>
where
    F: FnOnce() -> Result<RubyValue, Signal> + Send + 'static,
{
    // A dedicated thread rather than the OS main: the main thread's stack is
    // a ulimit the program doesn't control (8MB typically), and unoptimized
    // native frames blow through it at recursion depths CRuby handles
    // routinely. 64MB keeps the depth platform-independent. `Thread.current`
    // still answers the main-thread object here (`thread::CURRENT` is only
    // set by `Thread.new` bodies), and the sole-thread ivar path stays valid
    // because the real main only blocks in `join` and touches no Ruby object
    // (so no `note_thread_spawn`, deliberately).
    let main = std::thread::Builder::new()
        .name("ruby-main".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            crate::gvl::mark_sole_thread();
            let _ctx = crate::gvl::install_ctx();
            let _held = crate::gvl::process_gvl().hold();
            body()
        })
        .expect("spawn the ruby main thread");
    let joined = main.join();
    // From here the REAL main runs `at_exit`/finalizers over objects the
    // ruby-main thread stamped -- give up this thread's sole-thread claim so
    // those accesses take the locks (see `gvl::clear_sole_thread`).
    crate::gvl::clear_sole_thread();
    match joined {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
