//! Program entry: the generated program's top level runs directly on the
//! REAL OS main thread, with its scheduling context installed and the
//! process Gvl held for the duration (see `gvl` -- the Gvl is DISABLED by
//! default, so "held" is free and Ruby `Thread`s run truly parallel;
//! `ZEO_GVL=1` arms CRuby-fidelity serialized scheduling). Every
//! `Thread.new` is its own OS thread (`thread`); `Fiber`'s corosensei
//! coroutines nest inside whichever thread resumes them.
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
    let _ctx = crate::gvl::install_ctx();
    let _held = crate::gvl::process_gvl().hold();
    body()
}
