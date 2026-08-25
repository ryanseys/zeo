//! The "currently handled exception" stack -- backs a bare `raise` (with no
//! arguments) re-raising whatever exception the nearest enclosing `rescue`
//! clause is handling, mirroring real Ruby's `$!`. Pushed/popped around a
//! `rescue` clause's own body -- see `clif::control::lower_begin`'s
//! docs for exactly where. A plain `Vec` (not a single `Option`) so a
//! `rescue` clause nested inside another `rescue` clause's own body
//! correctly restores the OUTER exception once the inner one's handling
//! finishes, mirroring real Ruby's own nesting of `$!`.
//!
//! This is also what drives automatic `.cause` chaining: every raise channel
//! reads [`current_exception`] through `exception::attach_cause`, so whatever
//! is being handled here becomes the new exception's cause.
//!
//! An `ensure` running while an exception PROPAGATES is the second writer --
//! ruby puts the in-flight exception in `$!` there too, which is what gives a
//! raise inside an `ensure` its cause. [`PropagatingGuard`] is that push, held
//! across the ensure body by `clif::control::lower_begin`.
//!
//! **Storage is `thread_local!`**: `$!`/rescue-nesting is per-EXECUTION-
//! CONTEXT state, and every Ruby `Thread` is its own OS thread, so plain
//! TLS is exactly per-context.
//!
//! `Fiber` needs one more twist: CRuby gives each fiber
//! its OWN execution context (`fiber->cont.saved_ec.errinfo` -- rescue
//! state inside a fiber is invisible to its resumer and vice versa), but a
//! fiber here runs ON its resumer's coroutine. The fiber ec-swap
//! (`crate::ec`, of which [`swap_handling`] is one slice) exchanges this
//! stack for the fiber's own saved one around every switch -- sufficient
//! BECAUSE a fiber never runs concurrently with its resumer, mirroring
//! how CRuby itself just swaps `th->ec`.

use crate::RubyValue;
use std::cell::RefCell;

std::thread_local! {
    static HANDLING: RefCell<Vec<RubyValue>> = const { RefCell::new(Vec::new()) }
}

/// Called on entry to a `rescue` clause's own body, with the exception it's
/// handling.
pub fn push_handling(exc: RubyValue) {
    HANDLING.with(|h| h.borrow_mut().push(exc));
}

/// Called unconditionally on exit from a `rescue` clause's own body
/// (however it exits -- normally, via `retry`, or via a further `raise`),
/// restoring whatever the enclosing `rescue` (if any) was handling.
pub fn pop_handling() {
    HANDLING.with(|h| {
        h.borrow_mut().pop();
    });
}

/// The innermost currently-executing `rescue` clause's exception, if any --
/// what a bare `raise` (re-raise) re-raises. `None` when called outside any
/// `rescue` clause (a bare top-level `raise`, matching real Ruby: it
/// constructs a fresh `RuntimeError` instead -- see the
/// emitter's raise lowering in `clif::stmt`).
pub fn current_exception() -> Option<RubyValue> {
    HANDLING.with(|h| h.borrow().last().cloned())
}

/// `$!` for the duration of an `ensure` body, when that `ensure` is running
/// because an exception is propagating through it. Ruby puts the in-flight
/// exception in `$!` there, so a raise inside the `ensure` takes it as its
/// `cause` and a bare `raise` re-raises it.
///
/// A guard rather than a push/pop pair because the ensure body is emitted
/// INLINE, not in a closure: a `break` or `return` written inside it compiles
/// to a literal Rust jump out of this scope, and only `Drop` runs on every one
/// of those paths. Same reason [`crate::FrameGuard`] is shaped this way.
///
/// Nothing is pushed for an `ensure` reached by `break`/`next`/`return`/
/// `retry` -- `$!` is nil there, matching ruby, which tests the throw's tag
/// rather than treating every unwind as an exception.
pub struct PropagatingGuard(bool);

impl PropagatingGuard {
    /// `outcome` is the `begin`'s settled result, about to be propagated.
    pub fn enter(outcome: &Result<RubyValue, crate::Signal>) -> PropagatingGuard {
        let Err(crate::Signal::Raise(exc)) = outcome else {
            return PropagatingGuard(false);
        };
        push_handling(exc.clone());
        PropagatingGuard(true)
    }
}

impl Drop for PropagatingGuard {
    fn drop(&mut self) {
        if self.0 {
            pop_handling();
        }
    }
}

/// [`PropagatingGuard::enter`] without the guard, for the capi `ensure`
/// bracket (Cranelift-compiled code has no Rust drops): pushes `$!` when
/// the propagating signal is a raise, answering whether it pushed -- pass
/// that back to [`propagating_leave_raw`].
pub(crate) fn propagating_enter_raw(sig: &crate::Signal) -> bool {
    let crate::Signal::Raise(exc) = sig else {
        return false;
    };
    push_handling(exc.clone());
    true
}

/// The explicit pop matching [`propagating_enter_raw`].
pub(crate) fn propagating_leave_raw(pushed: bool) {
    if pushed {
        pop_handling();
    }
}

/// Installs `new` as this execution context's handling stack and returns
/// the previous one -- `fiber::fiber_resume`'s entry/exit swap (see module
/// docs). Not a general-purpose API: only the fiber boundary may call it,
/// and always in save/restore pairs on the resumer's own stack.
pub fn swap_handling(new: Vec<RubyValue>) -> Vec<RubyValue> {
    HANDLING.with(|h| h.replace(new))
}
