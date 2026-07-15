//! `Fiber` (Phase 13.3) -- stackful coroutines via `spinel-fiber`'s
//! corosensei shim, mirroring CRuby's own architecture (a userspace stack
//! switch per `resume`/`yield`, NOT a thread handoff -- see
//! `coroutine/arm64/Context.S` in the CRuby source; the pthread-parked
//! variant is only CRuby's exotic-platform fallback, and the
//! JRuby/TruffleRuby thread-backed years are the cautionary tale for why).
//!
//! **The thread-pinned table.** `corosensei::Coroutine` is deliberately
//! `!Send` (no sound stackful-coroutine library can prove a suspended
//! stack's contents are Send), so a coroutine can never live inside
//! `RubyValue` -- which must stay `Send + Sync` (Part 9's foundation).
//! Instead, `RubyValue::Fiber` carries only an [`RFiber`] handle (id +
//! owning thread + state flag, trivially Send+Sync), and the coroutine
//! itself lives in this thread-local table on the OS thread that created
//! it. The restriction this imposes -- a fiber can only be resumed from its
//! creating thread -- is EXACTLY real Ruby's own rule (`FiberError: "fiber
//! called across threads"`, CRuby `cont.c:2838`), so nothing is lost.
//!
//! Error/exception contract (all verified against CRuby `cont.c`): the
//! fiber body's uncaught `Signal` (a Ruby exception, `break`, etc.)
//! surfaces at the RESUMER as [`FiberResume::RubyError`] and re-raises
//! there, with the fiber left dead; `resume` on a dead fiber and
//! cross-thread `resume` are distinct `FiberError`s CONSTRUCTED BY CODEGEN
//! (this crate can't build exception objects -- the same division of labor
//! as `array_set`'s `IndexError` contract, see `codegen::call`'s fiber
//! dispatch); `Fiber.yield` with no running fiber is the "can't yield from
//! root fiber" `FiberError`, signalled here by a `None`.
//!
//! Value-passing convention (CRuby `make_passing_arg`, `cont.c:1978`):
//! zero args -> nil, one arg -> the value itself, more -> an Array. Applies
//! to `Fiber.yield`'s payload, to `resume`'s value-for-the-suspended-yield,
//! and to the block's own params (bound leniently from the first `resume`'s
//! args by the ordinary Proc binding machinery).
//!
//! A fiber never resumed to completion is force-unwound when its thread's
//! table drops (corosensei's `Drop`) -- deterministic cleanup, no
//! GC-finalizer dependence (the leak JRuby's thread-backed fibers were
//! notorious for). Only Rust destructors run during that unwind; compiled
//! Ruby control flow (including `ensure`) is `Result`-based, so no Ruby
//! code executes -- matching this spike's general "no ensure on
//! never-finished fibers" simplification.

use crate::{RubyValue, Signal};
use spinel_fiber::{Coroutine, CoroutineResult};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::thread::ThreadId;

/// The Send+Sync half of a Fiber -- what `RubyValue::Fiber` actually
/// carries. The coroutine itself is in [`FIBERS`] on `owner`'s thread.
pub struct FiberHandle {
    id: u64,
    owner: ThreadId,
    /// Set exactly once, when the body returns or raises -- backs
    /// `Fiber#alive?` (`!finished`, matching CRuby's `!FIBER_TERMINATED_P`:
    /// created/suspended/running all count as alive).
    finished: AtomicBool,
}

pub type RFiber = Arc<FiberHandle>;

type FiberCoro = Coroutine<Vec<RubyValue>, RubyValue, Result<RubyValue, Signal>>;

thread_local! {
    static FIBERS: RefCell<HashMap<u64, FiberCoro>> = RefCell::new(HashMap::new());
}

/// Process-wide (not per-thread) so a handle's id says which fiber it is
/// unambiguously even if handles travel between threads (only RESUMING is
/// thread-pinned, not holding).
static NEXT_FIBER_ID: AtomicU64 = AtomicU64::new(1);

/// `Fiber.new { |args| ... }` -- `block` must be a `Proc` value (codegen
/// guarantees this: the literal block is compiled through the ordinary
/// escaping-Proc machinery before reaching here).
pub fn fiber_new(block: RubyValue) -> RubyValue {
    let body = block.as_proc_unchecked();
    let id = NEXT_FIBER_ID.fetch_add(1, Ordering::Relaxed);
    let coro = spinel_fiber::new_fiber(move |args: Vec<RubyValue>| body(&args));
    FIBERS.with(|f| f.borrow_mut().insert(id, coro));
    RubyValue::Fiber(Arc::new(FiberHandle {
        id,
        owner: std::thread::current().id(),
        finished: AtomicBool::new(false),
    }))
}

/// What a `Fiber#resume` call site does with the outcome -- the error
/// variants become codegen-constructed `FiberError`s (see module docs).
pub enum FiberResume {
    /// The value the fiber yielded, or its body's final value (fiber now
    /// dead) -- indistinguishable at the resume site, exactly like CRuby.
    Value(RubyValue),
    /// The fiber's body terminated with an uncaught signal (exception /
    /// stray break): re-raise in the resumer, fiber is dead.
    RubyError(Signal),
    /// `FiberError: attempt to resume a terminated fiber`.
    Dead,
    /// `FiberError: attempt to resume the current fiber (double resume)`
    /// -- its coroutine is checked out of the table but not finished, so it
    /// is somewhere below us on this very thread's resume chain.
    DoubleResume,
    /// `FiberError: fiber called across threads`.
    CrossThread,
}

pub fn fiber_resume(handle: &RFiber, args: Vec<RubyValue>) -> FiberResume {
    if std::thread::current().id() != handle.owner {
        return FiberResume::CrossThread;
    }
    if handle.finished.load(Ordering::Relaxed) {
        return FiberResume::Dead;
    }
    // Checked OUT of the table while running (not borrowed in place): the
    // body may itself create/resume other fibers on this same thread, which
    // needs the `RefCell` free -- and its absence is what makes a
    // double-resume detectable at all.
    let Some(mut coro) = FIBERS.with(|f| f.borrow_mut().remove(&handle.id)) else {
        return FiberResume::DoubleResume;
    };
    match spinel_fiber::resume(&mut coro, args) {
        CoroutineResult::Yield(v) => {
            FIBERS.with(|f| f.borrow_mut().insert(handle.id, coro));
            FiberResume::Value(v)
        }
        CoroutineResult::Return(outcome) => {
            handle.finished.store(true, Ordering::Relaxed);
            match outcome {
                Ok(v) => FiberResume::Value(v),
                Err(sig) => FiberResume::RubyError(sig),
            }
        }
    }
}

/// `Fiber.yield(*args)` -- packs `args` per the CRuby convention (module
/// docs), suspends the innermost running fiber, and returns the value the
/// NEXT `resume` passes in (packed the same way). `None` = no fiber is
/// running: the call site raises `FiberError` ("can't yield from root
/// fiber").
pub fn fiber_yield(args: Vec<RubyValue>) -> Option<RubyValue> {
    let payload = pack_values(args);
    spinel_fiber::yield_current::<Vec<RubyValue>, RubyValue>(payload).map(pack_values)
}

pub fn fiber_alive(handle: &RFiber) -> bool {
    !handle.finished.load(Ordering::Relaxed)
}

/// CRuby's `make_passing_arg` (`cont.c:1978`): 0 -> nil, 1 -> the value,
/// N -> an Array.
fn pack_values(mut vals: Vec<RubyValue>) -> RubyValue {
    match vals.len() {
        0 => RubyValue::Nil,
        1 => vals.pop().expect("len checked"),
        _ => RubyValue::Array(crate::array_new(vals)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;

    fn proc_counting_yields() -> RubyValue {
        RubyValue::Proc(StdArc::new(|args: &[RubyValue]| {
            let first = args.first().cloned().unwrap_or(RubyValue::Nil);
            let second = fiber_yield(vec![first]).expect("inside a fiber");
            Ok(second)
        }))
    }

    #[test]
    fn resume_and_yield_pass_values_both_ways() {
        let f = fiber_new(proc_counting_yields());
        let RubyValue::Fiber(h) = &f else { panic!("expected a Fiber") };
        // First resume: args become block params; body yields them back.
        match fiber_resume(h, vec![RubyValue::Int(7)]) {
            FiberResume::Value(RubyValue::Int(7)) => {}
            _ => panic!("expected the first arg yielded back"),
        }
        assert!(fiber_alive(h));
        // Second resume: its arg becomes the yield's return, then the body
        // returns it as its final value.
        match fiber_resume(h, vec![RubyValue::Int(42)]) {
            FiberResume::Value(RubyValue::Int(42)) => {}
            _ => panic!("expected the body's final value"),
        }
        assert!(!fiber_alive(h));
        assert!(matches!(fiber_resume(h, vec![]), FiberResume::Dead));
    }

    #[test]
    fn cross_thread_resume_is_rejected_like_cruby() {
        let f = fiber_new(proc_counting_yields());
        let RubyValue::Fiber(h) = f else { panic!("expected a Fiber") };
        let h2 = h.clone();
        let outcome = std::thread::spawn(move || {
            matches!(fiber_resume(&h2, vec![]), FiberResume::CrossThread)
        })
        .join()
        .unwrap();
        assert!(outcome, "resume from another thread must be CrossThread");
        assert!(fiber_alive(&h), "the rejected resume must not kill the fiber");
    }

    #[test]
    fn yield_with_no_running_fiber_is_the_root_fiber_case() {
        assert!(fiber_yield(vec![RubyValue::Int(1)]).is_none());
    }

    #[test]
    fn multiple_yield_args_pack_into_an_array() {
        let body = RubyValue::Proc(StdArc::new(|_args: &[RubyValue]| {
            fiber_yield(vec![RubyValue::Int(1), RubyValue::Int(2)]);
            Ok(RubyValue::Nil)
        }));
        let RubyValue::Fiber(h) = fiber_new(body) else { panic!() };
        match fiber_resume(&h, vec![]) {
            FiberResume::Value(RubyValue::Array(a)) => {
                assert_eq!(a.lock().len(), 2);
            }
            _ => panic!("expected a packed Array"),
        }
    }
}
