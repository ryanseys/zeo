//! zeo-fiber: the "current yielder" shim that lets `Fiber.yield` suspend
//! the innermost running coroutine from ARBITRARY call depth -- the one
//! piece corosensei deliberately doesn't ship (its maintainer's words, issue
//! #71: "implement this yourself based on thread-local storage... switch a
//! pointer... every time a coroutine switch happens", declined upstream only
//! because it would tax every switch for users who don't need it). Wasmer 7
//! and open-coroutine both implement this same pattern over the same crate.
//!
//! **This is deliberately the ONLY crate in this workspace containing
//! `unsafe` code** (`zeo`/`zeo-rt` keep `#![forbid(unsafe_code)]`) --
//! kept tiny and self-contained so the entire unsafety surface is auditable
//! in one sitting. The single `unsafe` operation is the raw-pointer deref in
//! [`yield_current`]; its safety rests on three invariants:
//!
//! 1. **The pointer is valid for the coroutine's whole life.** corosensei's
//!    `Yielder` is `#[repr(transparent)]` over the parent-link slot at a
//!    FIXED location on the coroutine's own stack (`src/coroutine.rs:571`,
//!    the `&Yielder` handed to the entry closure is a reinterpret-cast of
//!    that slot) -- its address never changes between creation and
//!    completion/drop. This is structural in corosensei 0.3.x but not a
//!    documented API guarantee, hence the `0.3`-pinned dependency.
//! 2. **The TLS cell always points at the innermost RUNNING coroutine (or
//!    is `None` at the root).** Set on entry ([`new_fiber`]'s wrapper),
//!    re-established immediately after every wake ([`yield_current`], since
//!    the resumer's restore ran while we were suspended), and
//!    saved/restored around every switch ON THE RESUMER'S OWN STACK
//!    ([`resume`]'s drop guard) -- so the save/restore pairing is genuinely
//!    LIFO per OS thread, unlike a scoped-TLS guard on the SUSPENDED
//!    coroutine's stack (which freezes mid-scope at the moment of
//!    suspension and would leave the cell dangling; the classic hazard this
//!    design exists to avoid). The guard restores on unwind too, so a panic
//!    propagating out of `resume` can't leave the cell stale.
//! 3. **No type confusion.** The cell stores a type-erased pointer plus the
//!    `TypeId` of the `(Input, Yield)` pair; a mismatched instantiation
//!    panics cleanly instead of reinterpreting memory. (`zeo-rt` only
//!    ever instantiates one shape, but the check makes that a verified
//!    property rather than an assumed one.)
//!
//! `Yielder` is `!Sync` and everything here is `thread_local!`, so no
//! cross-thread aliasing is possible by construction. One documented
//! residual: a `Coroutine` dropped while suspended force-unwinds on its own
//! stack; if a Rust destructor running during that unwind called
//! [`yield_current`], the cell might name a coroutine that isn't running.
//! zeo-generated code never suspends from a destructor (Ruby-level
//! control flow, including `ensure`, compiles to ordinary `Result`
//! propagation, not `Drop` impls), so this is unreachable from compiled
//! programs.

use corosensei::Yielder;
pub use corosensei::{Coroutine, CoroutineResult};
use std::any::TypeId;
use std::cell::Cell;

thread_local! {
    /// `(TypeId of (Input, Yield), type-erased pointer to the innermost
    /// running coroutine's Yielder)` -- see the module docs' invariant 2.
    static CURRENT: Cell<Option<(TypeId, *const ())>> = const { Cell::new(None) };
}

/// Restores the saved TLS value when dropped -- including on unwind, so a
/// panic propagating out of a coroutine can't leave `CURRENT` dangling.
struct RestoreOnExit(Option<(TypeId, *const ())>);

impl Drop for RestoreOnExit {
    fn drop(&mut self) {
        CURRENT.with(|c| c.set(self.0));
    }
}

/// A coroutine whose body can suspend via [`yield_current`] from any call
/// depth -- `f` receives the first `resume`'s input; its return value
/// becomes the final `CoroutineResult::Return`.
pub fn new_fiber<I, Y, R, F>(f: F) -> Coroutine<I, Y, R>
where
    I: 'static,
    Y: 'static,
    R: 'static,
    F: FnOnce(I) -> R + 'static,
{
    Coroutine::new(move |yielder: &Yielder<I, Y>, input: I| {
        CURRENT.with(|c| {
            c.set(Some((
                TypeId::of::<(I, Y)>(),
                yielder as *const Yielder<I, Y> as *const (),
            )))
        });
        f(input)
    })
}

/// Drives `coro` forward -- a thin wrapper over `Coroutine::resume` that
/// keeps the module-docs invariant 2: whatever `CURRENT` held on THIS side
/// of the switch is restored once control comes back (by yield, completion,
/// or unwind), on this (the resumer's) stack.
pub fn resume<I, Y, R>(coro: &mut Coroutine<I, Y, R>, input: I) -> CoroutineResult<Y, R> {
    let _restore = RestoreOnExit(CURRENT.with(|c| c.get()));
    coro.resume(input)
}

/// Suspends the innermost running coroutine, yielding `value` to its
/// resumer; returns `Some(next_resume_input)` once resumed again, or `None`
/// immediately (suspending nothing) when called with no coroutine running --
/// the caller's "can't yield from root fiber" case.
///
/// Panics if the running coroutine was created with a different
/// `(Input, Yield)` instantiation than this call's -- see module docs
/// invariant 3.
pub fn yield_current<I: 'static, Y: 'static>(value: Y) -> Option<I> {
    let (tid, ptr) = CURRENT.with(|c| c.get())?;
    assert_eq!(
        tid,
        TypeId::of::<(I, Y)>(),
        "yield_current instantiated with a different (Input, Yield) than the running coroutine"
    );
    // SAFETY: `ptr` was created in `new_fiber`'s entry wrapper from the
    // `&Yielder` corosensei hands the entry closure -- a fixed slot on the
    // coroutine's own stack, valid until the coroutine completes or is
    // dropped (invariant 1). `CURRENT` names the innermost RUNNING
    // coroutine (invariant 2), which cannot have completed or been dropped
    // while it is the one executing this very call. The TypeId check above
    // rules out a mismatched cast (invariant 3).
    let yielder = unsafe { &*(ptr as *const Yielder<I, Y>) };
    let input = yielder.suspend(value);
    // Awake again: the resumer's `RestoreOnExit` reset `CURRENT` to ITS
    // context while we slept -- re-establish ourselves as innermost.
    CURRENT.with(|c| c.set(Some((tid, ptr))));
    Some(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the shim: suspend from inside a plain nested
    /// function that never saw a Yielder.
    #[test]
    fn yields_from_arbitrary_call_depth() {
        fn deep(n: i64) -> i64 {
            if n == 0 {
                yield_current::<i64, i64>(99).expect("running inside a fiber")
            } else {
                deep(n - 1)
            }
        }
        let mut coro = new_fiber::<i64, i64, i64, _>(|start| deep(5) + start);
        match resume(&mut coro, 1) {
            CoroutineResult::Yield(v) => assert_eq!(v, 99),
            CoroutineResult::Return(_) => panic!("expected a yield"),
        }
        match resume(&mut coro, 10) {
            // deep() returned resume's input (10), + start (1)
            CoroutineResult::Return(v) => assert_eq!(v, 11),
            CoroutineResult::Yield(_) => panic!("expected completion"),
        }
    }

    /// The invariant the resumer-stack save/restore exists for: fiber A
    /// resumes fiber B; after B yields, A's OWN yield must suspend A (not
    /// touch B's suspended yielder -- the scoped-TLS-guard design would get
    /// exactly this wrong).
    #[test]
    fn nested_fibers_yield_to_their_own_resumers() {
        let mut outer = new_fiber::<i64, i64, i64, _>(|_| {
            let mut inner = new_fiber::<i64, i64, i64, _>(|_| {
                yield_current::<i64, i64>(1).unwrap();
                2
            });
            let CoroutineResult::Yield(from_inner) = resume(&mut inner, 0) else {
                panic!("inner should yield first");
            };
            // Inner is suspended; yielding HERE must suspend OUTER.
            let got = yield_current::<i64, i64>(from_inner + 10).unwrap();
            let CoroutineResult::Return(done) = resume(&mut inner, 0) else {
                panic!("inner should complete");
            };
            got + done
        });
        match resume(&mut outer, 0) {
            CoroutineResult::Yield(v) => assert_eq!(v, 11), // inner's 1 + 10
            CoroutineResult::Return(_) => panic!("expected outer's yield"),
        }
        match resume(&mut outer, 100) {
            CoroutineResult::Return(v) => assert_eq!(v, 102), // resumed 100 + inner's 2
            CoroutineResult::Yield(_) => panic!("expected completion"),
        }
    }

    #[test]
    fn yield_current_outside_any_fiber_returns_none() {
        assert_eq!(yield_current::<i64, i64>(1), None);
    }

    /// A panic unwinding out of `resume` must not leave the TLS cell
    /// pointing at the dead coroutine (the `RestoreOnExit` guard's job).
    #[test]
    fn panic_inside_a_fiber_restores_the_tls_cell() {
        let mut coro = new_fiber::<i64, i64, i64, _>(|_| panic!("boom"));
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| resume(&mut coro, 0)));
        assert!(result.is_err());
        assert_eq!(
            yield_current::<i64, i64>(1),
            None,
            "cell must be back to root state"
        );
    }
}
