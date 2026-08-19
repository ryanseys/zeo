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
    /// `Kernel#catch`. An uncaught throw surfaces at the top level as CRuby's
    /// UncaughtThrowError would (a loud abort; the error-class wrapper is a
    /// documented scope-cut).
    ///
    /// BOXED, and the only arm that is. Every other variant carries at most
    /// one `RubyValue` (24 bytes); this one carried two, and since an enum is
    /// as wide as its widest variant, that one outlier set the size of
    /// `Signal` -- and so of `Result<RubyValue, Signal>`, the return type of
    /// EVERY ruby method call in every generated program:
    ///
    ///     size_of::<Result<RubyValue, Signal>>()   48 -> 32
    ///
    /// The allocation lands on `throw`, which is rare control flow already
    /// unwinding through arbitrary frames, instead of on every return.
    ///
    /// This is NOT the experiment ROADMAP records measuring worse (4.479 ->
    /// 5.15 ns): that boxed `Signal` as a WHOLE, putting an allocation on
    /// `Break`/`Next`/`Return`, which are ordinary block control flow.
    Throw(Box<Thrown>),
    /// Fiber/enumerator TEARDOWN: a suspended coroutine being disposed of is
    /// resumed one last time with this in flight, so its live locals and
    /// temporaries release through ordinary `Result` propagation -- never
    /// native stack unwinding (which Cranelift-compiled frames cannot
    /// support on Mach-O). Rescue clauses never match it and `ensure`
    /// bodies are SKIPPED for it (today's force-unwind runs no Ruby
    /// `ensure` either -- the documented "no ensure on never-finished
    /// fibers" rule); every landing propagates it until the coroutine entry
    /// finishes on `Err(Signal::Terminate)`.
    Terminate,
}

/// [`Signal::Throw`]'s payload. A named struct rather than a tuple because
/// the two halves are easy to swap at a use site and impossible to tell
/// apart by type.
#[derive(Clone, Debug)]
pub struct Thrown {
    pub tag: RubyValue,
    pub value: RubyValue,
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
std::thread_local!(static HOME_STACK: RefCell<Vec<ProcHome>> = const { RefCell::new(Vec::new()) });

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
            // A `Signal::Return` aimed at this activation can no longer be
            // absorbed by anyone, so the mark must not outlive it and mislead
            // the next unwind.
            RETURN_TARGET.with(|t| {
                let mut t = t.borrow_mut();
                if t.as_ref().is_some_and(|h| Arc::ptr_eq(h, &home)) {
                    *t = None;
                }
            });
        }
    });
}

// Which method activation the `Signal::Return` currently unwinding is aimed at.
// A `Signal` is a value with nowhere to carry that, and only one return can be
// in flight per coroutine at a time, so one slot is the whole record. `None`
// means "unmarked" -- read as "the nearest catcher", which is what every
// `Signal::Return` this runtime did not raise itself has always meant.
std::thread_local!(static RETURN_TARGET: RefCell<Option<ProcHome>> = const { RefCell::new(None) });

/// A `return` from a non-lambda `Proc`, aimed at the activation the Proc
/// captured -- which may be several frames below the one it is unwinding
/// through.
///
/// Marks only an UNMARKED return, which is what tells the proc that raised it
/// apart from every proc it then unwinds through: the innermost one always gets
/// here first, and a relay must leave the real target alone. A return raised
/// anywhere else stays unmarked and so belongs, as it always has, to the
/// nearest catcher.
pub(crate) fn signal_return_to(home: &ProcHome, value: RubyValue) -> Signal {
    RETURN_TARGET.with(|t| {
        let mut t = t.borrow_mut();
        if t.is_none() {
            *t = Some(home.clone());
        }
    });
    Signal::Return(value)
}

/// Whether the in-flight `Signal::Return` belongs to the activation on top of
/// the home stack -- asked by a method's catch BEFORE its `home_pop`. An
/// unmarked return belongs to the nearest catcher, which is the behaviour every
/// caller had before the mark existed.
pub fn return_targets_here() -> bool {
    RETURN_TARGET.with(|t| {
        let mine = match t.borrow().as_ref() {
            None => true,
            Some(target) => {
                HOME_STACK.with(|s| s.borrow().last().is_some_and(|h| Arc::ptr_eq(h, target)))
            }
        };
        if mine {
            *t.borrow_mut() = None;
        }
        mine
    })
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

/// Install `new` as this context's home stack, returning the previous one
/// -- the fiber ec-swap's slice of this cell (see `crate::ec`).
pub fn swap_home_stack(new: Vec<ProcHome>) -> Vec<ProcHome> {
    HOME_STACK.with(|s| s.replace(new))
}

// The pending-signal slot of the C-ABI status protocol: a compiled function
// that returns `STATUS_SIGNAL` has parked its `Signal` here for the caller
// to read back (`zeo_rt_signal_take`/`_kind`, arriving with `capi/`). One
// slot per coroutine -- exactly one signal is in flight at a time, the same
// invariant `RETURN_TARGET` rests on -- and fiber-swapped through `Ec` so a
// fiber switch never observes another fiber's signal.
std::thread_local!(static PENDING: RefCell<Option<Signal>> = const { RefCell::new(None) });

/// Park `sig` as the pending signal. Debug-asserts the slot is empty: a
/// second set before a take means a landing forgot to consume or forward.
#[expect(
    dead_code,
    reason = "consumed by the capi/ status-protocol wrappers (M0-3)"
)]
pub fn set_pending(sig: Signal) {
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        debug_assert!(
            p.is_none(),
            "pending-signal slot set while already occupied"
        );
        *p = Some(sig);
    });
}

/// Take the pending signal, emptying the slot.
#[expect(
    dead_code,
    reason = "consumed by the capi/ status-protocol wrappers (M0-3)"
)]
pub fn take_pending() -> Option<Signal> {
    PENDING.with(|p| p.borrow_mut().take())
}

/// Install `new` as this context's pending signal, returning the previous
/// one -- the fiber ec-swap's slice of this cell (see `crate::ec`).
pub fn swap_pending(new: Option<Signal>) -> Option<Signal> {
    PENDING.with(|p| p.replace(new))
}

/// Install `new` as this context's in-flight `Signal::Return` target,
/// returning the previous one -- the fiber ec-swap's slice of this cell (see
/// `crate::ec`). Per-coroutine by its own definition ("only one return can
/// be in flight per coroutine"): a `Fiber.yield` inside an `ensure` running
/// under a propagating return must not let another fiber's return retarget
/// this one.
pub fn swap_return_target(new: Option<ProcHome>) -> Option<ProcHome> {
    RETURN_TARGET.with(|t| t.replace(new))
}
