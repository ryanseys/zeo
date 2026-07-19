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

use crate::{RubyValue, Signal, Symbol};
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
    /// This fiber's OWN `$!`/rescue-nesting stack while it's suspended
    /// (Phase 13.6) -- swapped into `handling`'s ambient slot for the
    /// duration of every `resume` and back out on yield/return, giving each
    /// fiber the isolated execution context CRuby's own per-fiber
    /// `saved_ec.errinfo` provides (`cont.c:238`, verified in the plan's
    /// research addendum). Starts empty: a fresh fiber has no exception in
    /// flight regardless of what its creator was rescuing.
    handling: parking_lot::Mutex<Vec<RubyValue>>,
    /// `Fiber[]`/`Fiber.[]=` / `#storage` -- inheritable fiber-local storage.
    /// `None` until the first write, so `#storage` reads nil rather than an
    /// empty Hash (CRuby lazily allocates it). A new fiber COPIES its
    /// creator's storage at creation; writes are private thereafter.
    storage: parking_lot::Mutex<Option<HashMap<Symbol, RubyValue>>>,
}

pub type RFiber = Arc<FiberHandle>;

/// What a `resume`/`raise` feeds into a suspended fiber: either the values to
/// return from its `Fiber.yield` (or bind as its first block args), or an
/// exception to raise AT that yield point (`Fiber#raise`).
pub enum FiberInput {
    Resume(Vec<RubyValue>),
    Raise(RubyValue),
}

type FiberCoro = Coroutine<FiberInput, RubyValue, Result<RubyValue, Signal>>;

thread_local! {
    static FIBERS: RefCell<HashMap<u64, FiberCoro>> = RefCell::new(HashMap::new());
    /// The stack of fibers currently executing on this thread (innermost
    /// last) -- backs `Fiber.current`. Empty means the root fiber is running.
    static CURRENT_FIBER: RefCell<Vec<RFiber>> = const { RefCell::new(Vec::new()) };
    /// This thread's root fiber -- the implicit fiber the thread runs in
    /// before any `Fiber.new`. Lazily created so its identity stays stable
    /// (`Fiber.current.equal?(Fiber.current)` at the top level).
    static ROOT_FIBER: RefCell<Option<RFiber>> = const { RefCell::new(None) };
}

/// This thread's root fiber handle, created on first need. It never holds a
/// coroutine (the thread's native stack IS its stack) and is always alive.
fn root_fiber() -> RFiber {
    ROOT_FIBER.with(|r| {
        r.borrow_mut()
            .get_or_insert_with(|| {
                Arc::new(FiberHandle {
                    id: 0,
                    owner: std::thread::current().id(),
                    finished: AtomicBool::new(false),
                    handling: parking_lot::Mutex::new(Vec::new()),
                    storage: parking_lot::Mutex::new(None),
                })
            })
            .clone()
    })
}

/// The fiber running right now on this thread -- the innermost resumed one,
/// or the root fiber when none is resumed.
fn current_handle() -> RFiber {
    CURRENT_FIBER.with(|s| s.borrow().last().cloned()).unwrap_or_else(root_fiber)
}

/// Whether `handle` is the fiber executing right now -- `#storage`/`#storage=`
/// are legal only on the current fiber (CRuby's own restriction).
pub fn fiber_is_current(handle: &RFiber) -> bool {
    Arc::ptr_eq(handle, &current_handle())
}

/// `Fiber.current` -- the running fiber as a Ruby value.
pub fn fiber_current() -> RubyValue {
    RubyValue::Fiber(current_handle())
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
    // The first input becomes the block's args -- or, if the very first thing
    // done to the fiber is `#raise`, the body raises before running at all.
    let coro = spinel_fiber::new_fiber(move |first: FiberInput| match first {
        FiberInput::Resume(args) => body.call(&args),
        FiberInput::Raise(exc) => Err(Signal::Raise(exc)),
    });
    FIBERS.with(|f| f.borrow_mut().insert(id, coro));
    // Inherit the creating fiber's storage (CRuby copies it at creation).
    let inherited = current_handle().storage.lock().clone();
    RubyValue::Fiber(Arc::new(FiberHandle {
        id,
        owner: std::thread::current().id(),
        finished: AtomicBool::new(false),
        handling: parking_lot::Mutex::new(Vec::new()),
        storage: parking_lot::Mutex::new(inherited),
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

/// `Fiber#resume(*args)` -- feed values in and run to the next yield/return.
pub fn fiber_resume(handle: &RFiber, args: Vec<RubyValue>) -> FiberResume {
    fiber_drive(handle, FiberInput::Resume(args))
}

/// `Fiber#raise(exc)` -- resume the fiber but make its suspended `Fiber.yield`
/// raise `exc` instead of returning a value. A fresh fiber (never resumed)
/// raises before its body runs. Same outcome shape as `resume`: the exception
/// either is rescued inside the fiber (which then yields/returns normally) or
/// propagates back here as `RubyError`.
pub fn fiber_raise(handle: &RFiber, exc: RubyValue) -> FiberResume {
    fiber_drive(handle, FiberInput::Raise(exc))
}

fn fiber_drive(handle: &RFiber, input: FiberInput) -> FiberResume {
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
    // Execution-context swap (Phase 13.6): install the fiber's own
    // `$!`/rescue-nesting stack for the duration of the switch, exactly as
    // CRuby swaps `th->ec` to the fiber's `saved_ec` -- the resumer's
    // rescue state is invisible inside the fiber and vice versa. Sound
    // because a fiber never runs CONCURRENTLY with its resumer (both swaps
    // happen here, on the resumer's own stack, either side of the switch).
    // A Rust panic propagating out of the resume skips the swap-back --
    // acceptable: a runtime panic is already a dying process in this
    // spike's posture.
    let resumer_stack = crate::handling::swap_handling(std::mem::take(&mut handle.handling.lock()));
    // Mark THIS fiber as current for the duration of the switch, so
    // `Fiber.current` inside the body finds it (and nested resumes stack).
    CURRENT_FIBER.with(|s| s.borrow_mut().push(handle.clone()));
    let result = spinel_fiber::resume(&mut coro, input);
    CURRENT_FIBER.with(|s| { s.borrow_mut().pop(); });
    *handle.handling.lock() = crate::handling::swap_handling(resumer_stack);
    match result {
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

/// What a `Fiber.yield` returns to the compiled call site.
pub enum FiberYield {
    /// The value the next `resume` passed in.
    Value(RubyValue),
    /// The next call was `Fiber#raise`: the yield must raise this exception.
    Raise(RubyValue),
    /// No fiber is running -- the "can't yield from root fiber" `FiberError`.
    Root,
}

/// `Fiber.yield(*args)` -- packs `args` per the CRuby convention (module
/// docs), suspends the innermost running fiber, and reports what the NEXT
/// `resume`/`raise` fed in.
pub fn fiber_yield(args: Vec<RubyValue>) -> FiberYield {
    let payload = pack_values(args);
    match spinel_fiber::yield_current::<FiberInput, RubyValue>(payload) {
        None => FiberYield::Root,
        Some(FiberInput::Resume(vals)) => FiberYield::Value(pack_values(vals)),
        Some(FiberInput::Raise(exc)) => FiberYield::Raise(exc),
    }
}

pub fn fiber_alive(handle: &RFiber) -> bool {
    !handle.finished.load(Ordering::Relaxed)
}

/// `Fiber[key]` -- read the CURRENT fiber's storage (nil if unset).
pub fn fiber_storage_get(key: Symbol) -> RubyValue {
    current_handle()
        .storage
        .lock()
        .as_ref()
        .and_then(|m| m.get(&key).cloned())
        .unwrap_or(RubyValue::Nil)
}

/// `Fiber[key] = value` -- write the current fiber's storage (allocating it
/// on first write).
pub fn fiber_storage_set(key: Symbol, value: RubyValue) {
    current_handle()
        .storage
        .lock()
        .get_or_insert_with(HashMap::new)
        .insert(key, value);
}

/// `Fiber#storage` -- a Hash of `handle`'s storage, or nil if it was never
/// written (CRuby's lazy allocation).
pub fn fiber_storage_hash(handle: &RFiber) -> RubyValue {
    match &*handle.storage.lock() {
        None => RubyValue::Nil,
        Some(map) => {
            let pairs = map.iter().map(|(k, v)| (RubyValue::Symbol(*k), v.clone())).collect();
            RubyValue::Hash(crate::hash_new(pairs))
        }
    }
}

/// `Fiber#storage = { ... }` -- replace `handle`'s storage from `pairs`
/// (Symbol keys); the caller answers the assigned value.
pub fn fiber_set_storage(handle: &RFiber, pairs: Vec<(Symbol, RubyValue)>) {
    *handle.storage.lock() = Some(pairs.into_iter().collect());
}

/// `Fiber#kill` -- terminate a suspended fiber: drop its coroutine (whose
/// `Drop` force-unwinds the parked stack, running only Rust destructors) and
/// mark it finished. A cross-thread kill is a no-op, mirroring resume's
/// thread-pinning. Returns nil.
pub fn fiber_kill(handle: &RFiber) -> RubyValue {
    if std::thread::current().id() == handle.owner {
        handle.finished.store(true, Ordering::Relaxed);
        FIBERS.with(|f| {
            f.borrow_mut().remove(&handle.id);
        });
    }
    RubyValue::Nil
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
        RubyValue::Proc(crate::RProc::new(|args: &[RubyValue]| {
            let first = args.first().cloned().unwrap_or(RubyValue::Nil);
            let second = match fiber_yield(vec![first]) {
                FiberYield::Value(v) => v,
                _ => panic!("expected a resume value inside a fiber"),
            };
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
        assert!(matches!(fiber_yield(vec![RubyValue::Int(1)]), FiberYield::Root));
    }

    #[test]
    fn multiple_yield_args_pack_into_an_array() {
        let body = RubyValue::Proc(crate::RProc::new(|_args: &[RubyValue]| {
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
