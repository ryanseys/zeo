//! `Fiber` -- stackful coroutines via the `coroutine` module's
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
//! code executes -- matching this runtime's general "no ensure on
//! never-finished fibers" simplification.

use crate::coroutine::{Coroutine, CoroutineResult};
use crate::{RubyValue, Signal, Symbol};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// This fiber's OWN execution context while it's suspended -- the
    /// `$!`/rescue stack, proc return-homes, live `catch` tags, and
    /// backtrace frames, swapped into the ambient cells for the duration
    /// of every `resume` and back out on yield/return (see `crate::ec`),
    /// giving each fiber the isolated context CRuby's own per-fiber
    /// `saved_ec` provides (`cont.c`). Starts fresh: a new fiber has no
    /// exception in flight, homes, catch frames, or backtrace,
    /// regardless of what its creator was doing.
    saved_ec: parking_lot::Mutex<crate::ec::Ec>,
    /// `Fiber[]`/`Fiber.[]=` / `#storage` -- inheritable fiber-local storage.
    /// `None` until the first write, so `#storage` reads nil rather than an
    /// empty Hash (CRuby lazily allocates it). A new fiber COPIES its
    /// creator's storage at creation; writes are private thereafter.
    storage: parking_lot::Mutex<Option<HashMap<Symbol, RubyValue>>>,
    /// Whether this fiber was last ENTERED via `#transfer` rather than
    /// `#resume`. A transfer establishes no resumer, so `Fiber.yield` inside it
    /// raises `FiberError` (CRuby's rule); `#resume` clears it, `#transfer`
    /// sets it.
    entered_by_transfer: AtomicBool,
    /// `.frozen?` state -- flag-only (freezing a Fiber changes nothing
    /// observable: resuming a frozen fiber is legal in CRuby).
    frozen: AtomicBool,
    /// A handle minted by `Fiber#dup`/`#clone`: CRuby's shallow copy skips
    /// the machine stack, so the copy is a real Fiber object with NO
    /// execution context -- resuming it raises `FiberError: uninitialized
    /// fiber` (oracle-verified) while the original keeps working. Set only
    /// at construction, never cleared.
    uninitialized: bool,
}

impl FiberHandle {
    /// `Fiber#frozen?` -- see the `frozen` field.
    pub fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    /// `Fiber#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }
}

pub type RFiber = Arc<FiberHandle>;

/// What a `resume`/`raise` feeds into a suspended fiber: either the values to
/// return from its `Fiber.yield` (or bind as its first block args), or an
/// exception to raise AT that yield point (`Fiber#raise`).
pub enum FiberInput {
    Resume(Vec<RubyValue>),
    Raise(RubyValue),
    /// `#transfer(*args)` handed the fiber control -- same payload shape as
    /// `Resume`, but the entry marks the fiber transfer-entered.
    Transfer(Vec<RubyValue>),
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
                    saved_ec: parking_lot::Mutex::new(crate::ec::Ec::default()),
                    storage: parking_lot::Mutex::new(None),
                    entered_by_transfer: AtomicBool::new(false),
                    frozen: AtomicBool::new(false),
                    uninitialized: false,
                })
            })
            .clone()
    })
}

/// The fiber running right now on this thread -- the innermost resumed one,
/// or the root fiber when none is resumed.
fn current_handle() -> RFiber {
    CURRENT_FIBER
        .with(|s| s.borrow().last().cloned())
        .unwrap_or_else(root_fiber)
}

/// Whether `handle` is the fiber executing right now -- `#storage`/`#storage=`
/// are legal only on the current fiber (CRuby's own restriction).
pub fn fiber_is_current(handle: &RFiber) -> bool {
    Arc::ptr_eq(handle, &current_handle())
}

/// Whether `handle` is a thread's ROOT fiber (id 0) rather than one `Fiber.new`
/// made. It is the one fiber CRuby calls blocking, since it is the one no
/// scheduler ever created.
pub fn fiber_is_root(handle: &RFiber) -> bool {
    handle.id == 0
}

/// `Fiber#inspect`/`#to_s` -- `#<Fiber:0xADDR (state)>`, where the state is
/// `created`/`resumed`/`suspended`/`terminated` as in CRuby.
pub fn fiber_inspect(handle: &RFiber) -> String {
    let state = if !fiber_alive(handle) {
        "terminated"
    } else if fiber_is_current(handle) {
        "resumed"
    } else {
        "suspended"
    };
    format!(
        "#<Fiber:0x{:016x} ({state})>",
        Arc::as_ptr(handle) as *const () as usize
    )
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
    let coro = crate::coroutine::new_fiber(move |first: FiberInput| match first {
        FiberInput::Resume(args) | FiberInput::Transfer(args) => body.call(&args),
        FiberInput::Raise(exc) => Err(Signal::Raise(exc)),
    });
    FIBERS.with(|f| f.borrow_mut().insert(id, coro));
    // Inherit the creating fiber's storage (CRuby copies it at creation).
    let inherited = current_handle().storage.lock().clone();
    RubyValue::Fiber(Arc::new(FiberHandle {
        id,
        owner: std::thread::current().id(),
        finished: AtomicBool::new(false),
        saved_ec: parking_lot::Mutex::new(crate::ec::Ec::default()),
        storage: parking_lot::Mutex::new(inherited),
        entered_by_transfer: AtomicBool::new(false),
        frozen: AtomicBool::new(false),
        uninitialized: false,
    }))
}

/// `Fiber#dup`/`#clone`'s payload: a fresh handle with NO coroutine behind
/// it -- see [`FiberHandle::uninitialized`]. Storage is copied (`dup`'s
/// shallow ivar rule); the frozen flag is the caller's business
/// (`dup_value`'s `keep_frozen`).
pub fn dup_uninitialized(src: &FiberHandle) -> RFiber {
    Arc::new(FiberHandle {
        id: NEXT_FIBER_ID.fetch_add(1, Ordering::Relaxed),
        owner: std::thread::current().id(),
        finished: AtomicBool::new(false),
        saved_ec: parking_lot::Mutex::new(crate::ec::Ec::default()),
        storage: parking_lot::Mutex::new(src.storage.lock().clone()),
        entered_by_transfer: AtomicBool::new(false),
        frozen: AtomicBool::new(false),
        uninitialized: true,
    })
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
    /// `FiberError: uninitialized fiber` -- a `dup`/`clone` copy, which has
    /// no execution context to enter (see `dup_uninitialized`).
    Uninitialized,
    /// `FiberError: attempt to resume a resumed fiber (double resume)`
    /// -- its coroutine is checked out of the table but not finished, so it
    /// is somewhere below us on this very thread's resume chain.
    DoubleResume,
    /// `FiberError: fiber called across threads`.
    CrossThread,
}

/// `Fiber#resume(*args)` -- feed values in and run to the next yield/return.
pub fn fiber_resume(handle: &RFiber, args: Vec<RubyValue>) -> FiberResume {
    // Entering by resume establishes a resumer, so `Fiber.yield` is legal again.
    handle.entered_by_transfer.store(false, Ordering::Relaxed);
    fiber_drive(handle, FiberInput::Resume(args))
}

/// `Fiber#transfer(*args)` -- symmetric control transfer. Transferring to the
/// ROOT fiber suspends the current fiber and hands the value back to whoever is
/// driving it (a yield through the root trampoline); transferring to any other
/// fiber drives it forward, marking it transfer-entered (so its `Fiber.yield`
/// raises, per CRuby). Sibling-to-sibling transfer between two non-root fibers
/// is a documented gap (the corpus only transfers to and from root).
pub fn fiber_transfer(handle: &RFiber, args: Vec<RubyValue>) -> FiberResume {
    if handle.uninitialized {
        return FiberResume::Uninitialized;
    }
    if std::thread::current().id() != handle.owner {
        return FiberResume::CrossThread;
    }
    // The root fiber (id 0) has no coroutine of its own; "transfer to root" is a
    // suspend of the CURRENT fiber back to its driver.
    if handle.id == 0 {
        let payload = pack_values(args);
        return match crate::coroutine::yield_current::<FiberInput, RubyValue>(payload) {
            // Called from the root itself (nothing suspended) -- a no-op.
            None => FiberResume::Value(RubyValue::Nil),
            Some(FiberInput::Resume(vals)) | Some(FiberInput::Transfer(vals)) => {
                FiberResume::Value(pack_values(vals))
            }
            Some(FiberInput::Raise(exc)) => FiberResume::RubyError(Signal::Raise(exc)),
        };
    }
    if handle.finished.load(Ordering::Relaxed) {
        return FiberResume::Dead;
    }
    handle.entered_by_transfer.store(true, Ordering::Relaxed);
    fiber_drive(handle, FiberInput::Transfer(args))
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
    if handle.uninitialized {
        return FiberResume::Uninitialized;
    }
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
    // Execution-context swap: install the fiber's own saved context
    // ($!/rescue stack, proc homes, catch tags, backtrace frames) for the
    // duration of the switch, exactly as CRuby swaps `th->ec` to the
    // fiber's `saved_ec` -- the resumer's state is invisible inside the
    // fiber and vice versa (see `crate::ec`). Sound because a fiber never
    // runs CONCURRENTLY with its resumer (both swaps happen here, on the
    // resumer's own stack, either side of the switch). A Rust panic
    // propagating out of the resume skips the swap-back -- acceptable: a
    // runtime panic is already a dying process in this runtime's posture.
    let resumer_ec = crate::ec::swap(std::mem::take(&mut handle.saved_ec.lock()));
    // Mark THIS fiber as current for the duration of the switch, so
    // `Fiber.current` inside the body finds it (and nested resumes stack).
    CURRENT_FIBER.with(|s| s.borrow_mut().push(handle.clone()));
    let result = crate::coroutine::resume(&mut coro, input);
    CURRENT_FIBER.with(|s| {
        s.borrow_mut().pop();
    });
    *handle.saved_ec.lock() = crate::ec::swap(resumer_ec);
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
    // A fiber entered by `#transfer` (like the root fiber) has no resumer to
    // yield back to -- CRuby raises FiberError rather than suspending.
    if current_handle().entered_by_transfer.load(Ordering::Relaxed) {
        return FiberYield::Root;
    }
    let payload = pack_values(args);
    match crate::coroutine::yield_current::<FiberInput, RubyValue>(payload) {
        None => FiberYield::Root,
        Some(FiberInput::Resume(vals)) | Some(FiberInput::Transfer(vals)) => {
            FiberYield::Value(pack_values(vals))
        }
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
            let pairs = map
                .iter()
                .map(|(k, v)| (RubyValue::Symbol(*k), v.clone()))
                .collect();
            RubyValue::Hash(crate::hash_new(pairs))
        }
    }
}

/// `Fiber#storage = { ... }` -- replace `handle`'s storage from `pairs`
/// (Symbol keys); the caller answers the assigned value.
pub fn fiber_set_storage(handle: &RFiber, pairs: Vec<(Symbol, RubyValue)>) {
    *handle.storage.lock() = Some(pairs.into_iter().collect());
}

/// `Fiber#kill` -- terminate a fiber, RUNNING its `ensure` blocks. The fiber is
/// driven once more with a kill exception injected at its suspended
/// `Fiber.yield` (or before its body runs, for an unstarted fiber): the
/// exception is an `Exception` (NOT a `StandardError`), so a bare `rescue`
/// can't swallow it, and whatever the fiber unwinds to is discarded -- the
/// fiber is then dead regardless. Idempotent; a cross-thread kill is a no-op,
/// mirroring resume's thread-pinning. Returns nil.
pub fn fiber_kill(handle: &RFiber) -> RubyValue {
    if std::thread::current().id() != handle.owner {
        return RubyValue::Nil;
    }
    if handle.finished.load(Ordering::Relaxed) {
        return RubyValue::Nil;
    }
    // Only a fiber still holding a coroutine can run ensure; if its coro is
    // absent (already running below us, or gone) just mark it dead.
    let has_coro = FIBERS.with(|f| f.borrow().contains_key(&handle.id));
    if has_coro {
        let _ = fiber_drive(handle, FiberInput::Raise(kill_exception()));
    }
    handle.finished.store(true, Ordering::Relaxed);
    FIBERS.with(|f| {
        f.borrow_mut().remove(&handle.id);
    });
    RubyValue::Nil
}

/// The `Exception` (deliberately NOT a `StandardError`) a `#kill` unwinds a
/// fiber with -- a bare `rescue` skips it, so only `ensure` runs.
fn kill_exception() -> RubyValue {
    match crate::dispatch::raise_error("Exception", String::new()) {
        Signal::Raise(exc) => exc,
        _ => RubyValue::Nil,
    }
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
        let RubyValue::Fiber(h) = &f else {
            panic!("expected a Fiber")
        };
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
        let RubyValue::Fiber(h) = f else {
            panic!("expected a Fiber")
        };
        let h2 = h.clone();
        let outcome = std::thread::spawn(move || {
            matches!(fiber_resume(&h2, vec![]), FiberResume::CrossThread)
        })
        .join()
        .unwrap();
        assert!(outcome, "resume from another thread must be CrossThread");
        assert!(
            fiber_alive(&h),
            "the rejected resume must not kill the fiber"
        );
    }

    #[test]
    fn yield_with_no_running_fiber_is_the_root_fiber_case() {
        assert!(matches!(
            fiber_yield(vec![RubyValue::Int(1)]),
            FiberYield::Root
        ));
    }

    #[test]
    fn multiple_yield_args_pack_into_an_array() {
        let body = RubyValue::Proc(crate::RProc::new(|_args: &[RubyValue]| {
            fiber_yield(vec![RubyValue::Int(1), RubyValue::Int(2)]);
            Ok(RubyValue::Nil)
        }));
        let RubyValue::Fiber(h) = fiber_new(body) else {
            panic!()
        };
        match fiber_resume(&h, vec![]) {
            FiberResume::Value(RubyValue::Array(a)) => {
                assert_eq!(a.lock().len(), 2);
            }
            _ => panic!("expected a packed Array"),
        }
    }
}
