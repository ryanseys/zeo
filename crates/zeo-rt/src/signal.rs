//! Non-local control-flow propagation. Every generated method/closure body
//! returns `Result<RubyValue, Signal>` instead of a bare `RubyValue`, decided
//! once, up front, rather than retrofitted later: the moment any of
//! `break`/`next`/`redo`/`retry`/`raise`/an explicit `return` needs to unwind
//! across a function or closure boundary, a bare return value can't carry
//! "and also, unwind" information, and changing every already-generated
//! method/closure/trampoline signature after several phases assume a bare
//! `RubyValue` would be a much larger rewrite than fixing the ABI once, now.
//!
//! Most variants are dormant until later phases: loop-scoped `break`/`next`/
//! `redo` compile to native Rust `break`/`continue` on a labeled loop and
//! never touch `Signal` at all (see codegen's loop lowering); `Signal::Break`/
//! `Next`/`Redo` only become load-bearing once a block can be a real escaping
//! `Proc` invoked from a different Rust function than the loop that owns it,
//! and `Signal::Raise` isn't populated until `raise`/`rescue` exist. `Raise`'s
//! payload is a plain `RubyValue` for now (any raised exception is just a
//! value at this stage) rather than a dedicated exception type -- introducing
//! a `RubyException` type is deferred to the phase that actually needs
//! exception-class/backtrace machinery. `Raise` is also already the intended
//! vehicle for a *future* dynamic (runtime-string) `eval`'s parse/syntax
//! errors -- see `docs/EVAL_VM.md` -- so no `Signal` change is anticipated
//! for that either, once it's built.

use crate::RubyValue;

#[derive(Clone, Debug)]
pub enum Signal {
    Break(RubyValue),
    Next(RubyValue),
    Redo,
    Retry,
    Return(RubyValue),
    Raise(RubyValue),
    /// `Kernel#throw(tag, value)` unwinding toward the matching
    /// `Kernel#catch` -- `(tag, value)`. An uncaught throw
    /// surfaces at the top level as CRuby's UncaughtThrowError would
    /// (a loud abort; the error-class wrapper is a documented scope-cut).
    Throw(RubyValue, RubyValue),
}

/// Catches a `break`/`break value` that unwound out of a real `Proc` (see
/// `crate::rproc`'s docs) back to the call site that attached the block --
/// exactly where real Ruby's own `break` semantics land: it makes the WHOLE
/// method call (the one the block was passed to) evaluate to the break
/// value, not just the block invocation. Every call site whose callee might
/// invoke a block wraps its result in this (`codegen::params::emit_call_args`
/// and every `send`/`public_send` call site) -- a no-op match when no
/// `Break` was actually raised, so it's safe to apply unconditionally even
/// when the specific call didn't pass a block this time.
pub fn catch_break(result: Result<RubyValue, Signal>) -> Result<RubyValue, Signal> {
    match result {
        Err(Signal::Break(v)) => Ok(v),
        other => other,
    }
}

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A method activation's "home is still on the stack" flag. A non-lambda
/// `Proc` constructed while that method runs captures a clone of it (see
/// `RProc::with_home`); the method marks it dead the moment its body finishes
/// -- normally OR via a signal/exception (`home_pop`). A `return` in the Proc
/// then consults it (`RProc::call`): live home -> a real `Signal::Return` that
/// unwinds to the method (CRuby non-local return); dead home -> `LocalJumpError`,
/// rather than a `Signal::Return` longjmping into a freed frame.
pub type ProcHome = Arc<AtomicBool>;

// The per-coroutine stack of live method-activation homes, innermost on top.
// Coroutine-local (each `Thread`/`Fiber` is its own coroutine, so its frames
// never mingle with another's); the top-level program is itself a coroutine.
crate::exec::exec_local!(static HOME_STACK: RefCell<Vec<ProcHome>> = RefCell::new(Vec::new()));

/// Enter a method activation: push a fresh live home. Balanced by `home_pop`.
pub fn home_push() {
    HOME_STACK.with(|s| s.borrow_mut().push(Arc::new(AtomicBool::new(true))));
}

/// Leave a method activation: mark its home dead (any `Proc` that captured it
/// now sees a dead home) and pop it.
pub fn home_pop() {
    HOME_STACK.with(|s| {
        if let Some(home) = s.borrow_mut().pop() {
            home.store(false, Ordering::Relaxed);
        }
    });
}

/// The innermost live home, captured by a `Proc` at construction so its
/// `return` knows which method to unwind to (`None` at the top level -- a
/// `return` from such a `Proc` is an unconditional `LocalJumpError`).
pub fn home_current() -> Option<ProcHome> {
    HOME_STACK.with(|s| s.borrow().last().cloned())
}

/// Whether a captured home's method is still on the stack.
pub fn proc_home_alive(home: &ProcHome) -> bool {
    home.load(Ordering::Relaxed)
}
