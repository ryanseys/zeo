//! The "currently handled exception" stack -- backs a bare `raise` (with no
//! arguments) re-raising whatever exception the nearest enclosing `rescue`
//! clause is handling, mirroring real Ruby's `$!`. Pushed/popped around a
//! `rescue` clause's own body -- see `codegen::exceptions::emit_begin`'s
//! docs for exactly where. A plain `Vec` (not a single `Option`) so a
//! `rescue` clause nested inside another `rescue` clause's own body
//! correctly restores the OUTER exception once the inner one's handling
//! finishes, mirroring real Ruby's own nesting of `$!`.
//!
//! Explicit scope-cut: this does NOT drive automatic `.cause` chaining
//! (setting a newly-raised exception's `cause` to whatever's currently being
//! handled) -- that would need a way to set an arbitrary field on an
//! arbitrary `RObj` by name at runtime, which this object model
//! doesn't have (every ivar is a concrete, typed struct field, not a
//! runtime name-keyed map). Left as a documented future item, not attempted
//! here; likewise the explicit `raise ..., cause: e` override, already a
//! documented lowering-time rejection (see `parse/mod.rs`'s `raise`
//! recognizer).
//!
//! **Storage is `thread_local!`**: `$!`/rescue-nesting is per-EXECUTION-
//! CONTEXT state, and every Ruby `Thread` is its own OS thread, so plain
//! TLS is exactly per-context.
//!
//! `Fiber` needs one more twist (also this phase): CRuby gives each fiber
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
/// constructs a fresh `RuntimeError` instead -- see
/// `codegen::expr::emit_raise`'s docs).
pub fn current_exception() -> Option<RubyValue> {
    HANDLING.with(|h| h.borrow().last().cloned())
}

/// Installs `new` as this execution context's handling stack and returns
/// the previous one -- `fiber::fiber_resume`'s entry/exit swap (see module
/// docs). Not a general-purpose API: only the fiber boundary may call it,
/// and always in save/restore pairs on the resumer's own stack.
pub fn swap_handling(new: Vec<RubyValue>) -> Vec<RubyValue> {
    HANDLING.with(|h| h.replace(new))
}
