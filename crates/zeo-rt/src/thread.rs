//! `Thread`/`Mutex`/`Queue` -- thin Ruby-visible wrappers over
//! `may`'s green coroutines and sync primitives. A Ruby `Thread` is a `may`
//! coroutine, so under the default single scheduler worker (see `exec.rs`)
//! genuine parallelism is structurally impossible -- CRuby's GVL reality --
//! while `--no-gvl`/`ZEO_THREADS=N` gives real OS parallelism over the
//! same code, memory-safe via Part 9's `Arc`/`parking_lot::Mutex`
//! foundation.
//!
//! **Documented divergence from CRuby: scheduling is COOPERATIVE.** CRuby's
//! GVL preempts pure-Ruby threads on a timer (~100ms quantum); a `may`
//! coroutine only yields at blocking points (`join`/`value`, a contended
//! Ruby `Mutex`, an empty `Queue#pop`). A busy-loop thread that never
//! touches a synchronization point starves its siblings here, where CRuby
//! would interleave them. Well-SYNCHRONIZED programs -- the only kind whose
//! output order is even deterministic enough to test -- behave identically.
//!
//! Error contract (verified against CRuby `thread.c`/`thread_sync.c`, see
//! the plan's Part 11 addendum): an uncaught exception inside a Thread is
//! STORED and re-raised in whoever calls `#join`/`#value` (`thread.c:1195`;
//! CRuby's no-join `report_on_exception` stderr warning is a documented
//! skip); Ruby `Mutex` is NOT reentrant and ownership is per-execution-
//! context (`ec_serial`, `thread_sync.c:9`) -- relock by the owner raises
//! `ThreadError: "deadlock; recursive locking"`, foreign/idle unlock raises
//! too; `Queue#pop` on a closed empty queue returns nil, `#push` to a
//! closed queue raises `ClosedQueueError` (`thread_sync.c:969,1034`). All
//! exception CONSTRUCTION happens in codegen (the `IndexError` division of
//! labor); runtime functions return `Err(message)` / enums.
//!
//! The Ruby `Mutex` is a TOKEN CHANNEL (an mpmc channel holding at most one
//! `()`): `lock` = blocking `recv` (a real `may` yield point), `unlock` =
//! `send` it back. Not `may::sync::Mutex`, whose RAII guard can't span
//! Ruby's split `lock`/`unlock` calls (a guard is lifetime-bound to the
//! locking scope; Ruby's isn't). Owner identity for the error semantics
//! comes from [`execution_id`], a lazily-assigned per-coroutine id in
//! `may`'s coroutine-local storage -- per-EXECUTION-CONTEXT like CRuby's
//! `ec_serial`, and correct even if a coroutine migrates OS threads
//! (unlike a `thread_local!`).

use crate::{RubyValue, Signal, Symbol};
use parking_lot::Mutex as PlMutex;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

/// A pending asynchronous interrupt delivered by `Thread#kill`/`#raise`. It is
/// noticed at the next interruption CHECKPOINT the target thread reaches (an
/// empty `Queue#pop`, which the corpus uses as the deterministic delivery
/// point); the target then unwinds, running its `ensure` blocks.
enum InterruptKind {
    /// `Thread#kill`/`#exit`/`#terminate` -- unwind and die silently. Carried
    /// as an `Exception` (NOT a `StandardError`), so a bare `rescue` does not
    /// swallow it; the `was_killed` flag makes termination authoritative even
    /// past a `rescue Exception`.
    Kill,
    /// `Thread#raise(exc)` -- inject `exc`, which an ordinary `rescue` catches.
    Raise(RubyValue),
}

/// A unique id per execution context (the main coroutine, each Thread) --
/// CRuby's `ec_serial` analogue, backing `Mutex` ownership.
fn execution_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    crate::exec::exec_local!(static ID: u64 = NEXT.fetch_add(1, Ordering::Relaxed));
    ID.with(|id| *id)
}

// ---------------------------------------------------------------------------
// Thread
// ---------------------------------------------------------------------------

enum ThreadState {
    Running(may::coroutine::JoinHandle<Result<RubyValue, Signal>>),
    /// Cached after the first `join`/`value`, so repeats see the same
    /// outcome (CRuby: `join` on a dead thread returns immediately; the
    /// stored exception re-raises on EVERY join).
    Done(Result<RubyValue, Signal>),
}

pub struct ThreadData {
    state: PlMutex<Option<ThreadState>>,
    /// `Thread#name`/`name=` -- nil until set.
    name: PlMutex<Option<String>>,
    /// `Thread#report_on_exception` -- CRuby defaults this to true.
    report_on_exception: AtomicBool,
    /// `Thread#[]`/`#[]=` storage. CRuby scopes this per-FIBER; with no
    /// separate fiber identity here it is per-thread, a documented narrowing
    /// that the common one-fiber-per-thread programs can't observe.
    locals: PlMutex<HashMap<Symbol, RubyValue>>,
    /// `Thread#thread_variable_get`/`set` storage -- genuinely per-thread.
    tvars: PlMutex<HashMap<Symbol, RubyValue>>,
    /// The main thread never holds a join handle yet is always "running"; this
    /// flag lets `#status`/`#alive?` answer for it without a handle.
    is_main: bool,
    /// A pending `Thread#kill`/`#raise`, taken at the next checkpoint.
    interrupt: PlMutex<Option<InterruptKind>>,
    /// Set by `#kill`; makes the thread's outcome a silent `nil` no matter what
    /// the unwinding exception was (so even a `rescue Exception` can't keep a
    /// killed thread alive).
    was_killed: AtomicBool,
    /// `.frozen?` state -- flag-only (a frozen Thread still runs, joins, and
    /// answers reflection in CRuby; only `#[]=`/`#name=`-style mutations
    /// check it, audited with the mutator family).
    frozen: AtomicBool,
}

pub type RThread = Arc<ThreadData>;

impl ThreadData {
    fn build(state: Option<ThreadState>, is_main: bool) -> RThread {
        Arc::new(ThreadData {
            state: PlMutex::new(state),
            name: PlMutex::new(None),
            report_on_exception: AtomicBool::new(true),
            locals: PlMutex::new(HashMap::new()),
            tvars: PlMutex::new(HashMap::new()),
            is_main,
            interrupt: PlMutex::new(None),
            was_killed: AtomicBool::new(false),
            frozen: AtomicBool::new(false),
        })
    }

    /// `Thread#frozen?` -- see the `frozen` field.
    pub fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    /// `Thread#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// The live-thread registry (`Thread.list`)
// ---------------------------------------------------------------------------

/// Every thread ever spawned (plus main), held WEAKLY so a finished, dropped
/// thread leaves the list on its own. `Thread.list` upgrades and filters to
/// the ones still alive.
static LIVE: OnceLock<PlMutex<Vec<Weak<ThreadData>>>> = OnceLock::new();

fn live() -> &'static PlMutex<Vec<Weak<ThreadData>>> {
    LIVE.get_or_init(|| PlMutex::new(Vec::new()))
}

fn register_live(t: &RThread) {
    live().lock().push(Arc::downgrade(t));
}

/// `Thread.list` -- main plus every still-running spawned thread (a joined or
/// finished thread is excluded, matching CRuby's "live threads" contract).
/// Prunes dead weak refs as a side effect.
pub fn thread_list() -> Vec<RubyValue> {
    // Make sure main is registered even if no thread was ever spawned.
    let _ = main_thread();
    let mut guard = live().lock();
    guard.retain(|w| w.strong_count() > 0);
    guard
        .iter()
        .filter_map(|w| w.upgrade())
        .filter(thread_alive)
        .map(RubyValue::Thread)
        .collect()
}

/// The main thread's `Thread` object -- one process-wide instance, so
/// `Thread.main` and a top-level `Thread.current` are the same identity.
static MAIN_THREAD: OnceLock<RThread> = OnceLock::new();

fn main_thread() -> RThread {
    MAIN_THREAD
        .get_or_init(|| {
            let m = ThreadData::build(None, true);
            register_live(&m);
            m
        })
        .clone()
}

crate::exec::exec_local!(static CURRENT: PlMutex<Option<RThread>> = PlMutex::new(None));

/// `Thread.current` -- the running thread's object, or the main thread's when
/// called outside any spawned coroutine.
pub fn thread_current() -> RubyValue {
    let here = CURRENT.with(|c| c.lock().clone());
    RubyValue::Thread(here.unwrap_or_else(main_thread))
}

/// `Thread.main` -- the process's main thread object.
pub fn thread_main() -> RubyValue {
    RubyValue::Thread(main_thread())
}

/// `Thread.pass` -- give up the current quantum; nil. Inside a coroutine
/// that is may's own yield; on a plain OS thread (main) a brief real sleep,
/// so freshly spawned coroutines genuinely get scheduler time before the
/// caller proceeds -- CRuby's `Thread.pass` likewise cedes the caller's
/// quantum, which is what the `Thread.new ...; Thread.pass; t.raise` idiom
/// relies on for the target to reach its blocking point (and its own
/// `begin`) before the interrupt lands.
pub fn thread_pass() -> RubyValue {
    if may::coroutine::is_coroutine() {
        may::coroutine::yield_now();
    } else {
        std::thread::sleep(Duration::from_millis(2));
    }
    RubyValue::Nil
}

/// `Thread.new(*args) { |*params| ... }` -- `args` pass through to the
/// block's params (bound leniently by the ordinary Proc machinery),
/// matching CRuby.
pub fn thread_new(block: RubyValue, args: Vec<RubyValue>) -> RubyValue {
    let body = block.as_proc_unchecked();
    let data = ThreadData::build(None, false);
    register_live(&data);
    let for_coro = data.clone();
    let handle = may::go!(move || {
        // Record identity so `Thread.current` inside the body finds THIS
        // thread rather than falling through to main.
        CURRENT.with(|c| *c.lock() = Some(for_coro.clone()));
        // A fresh backtrace-frame stack for this thread's body, the
        // spawner's restored on exit -- see `frames`' module docs for the
        // per-OS-thread TLS narrowing this bounds.
        let saved_frames = crate::frames::swap_stack(Vec::new());
        let result = body.call(&args);
        let _ = crate::frames::swap_stack(saved_frames);
        // A killed thread dies silently with a nil value, whatever exception
        // unwound it (its `ensure` blocks already ran during that unwind).
        if for_coro.was_killed.load(Ordering::Relaxed) {
            Ok(RubyValue::Nil)
        } else {
            result
        }
    });
    *data.state.lock() = Some(ThreadState::Running(handle));
    RubyValue::Thread(data)
}

// ---------------------------------------------------------------------------
// Asynchronous interrupts (`Thread#kill`/`#raise`) and their checkpoints
// ---------------------------------------------------------------------------

/// `Thread#kill`/`#exit`/`#terminate` -- request the thread unwind and die. It
/// takes effect at the target's next checkpoint (or immediately, when it next
/// reaches one); `was_killed` makes the eventual outcome an authoritative nil.
pub fn thread_kill(t: &RThread) {
    t.was_killed.store(true, Ordering::Relaxed);
    if t.interrupt.lock().replace(InterruptKind::Kill).is_none() {
        crate::gvl::note_posted();
    }
}

/// `Thread#raise(exc)` -- queue `exc` to be raised inside the target thread at
/// its next checkpoint, where an ordinary `rescue` can catch it. The main
/// thread is a first-class target: `check_ints` checkpoints run in its body
/// like any other thread's.
pub fn thread_raise(t: &RThread, exc: RubyValue) {
    if t.interrupt
        .lock()
        .replace(InterruptKind::Raise(exc))
        .is_none()
    {
        crate::gvl::note_posted();
    }
}

/// The `Exception` (deliberately NOT a `StandardError`) a `#kill` unwinds with.
/// Its message is irrelevant -- the `was_killed` flag turns the outcome into
/// nil regardless -- but its CLASS being `Exception` keeps a bare `rescue`
/// (which only catches `StandardError`) from swallowing the kill mid-unwind.
fn kill_signal() -> Signal {
    crate::dispatch::raise_error("Exception", String::new())
}

/// A checkpoint: if the CURRENT thread has a pending interrupt, take it and
/// return the `Signal` that delivers it (unwinding the thread's body). Called
/// from every blocking primitive that can park a thread indefinitely.
pub fn check_interrupt() -> Result<(), Signal> {
    // Outside any spawned coroutine the current thread IS main -- the same
    // fallback `Thread.current` makes, and what lets `Thread#raise` reach
    // the main thread's checkpoints.
    let t = CURRENT
        .with(|c| c.lock().clone())
        .unwrap_or_else(main_thread);
    let taken = t.interrupt.lock().take();
    if taken.is_some() {
        crate::gvl::note_consumed();
    }
    match taken {
        Some(InterruptKind::Kill) => Err(kill_signal()),
        Some(InterruptKind::Raise(exc)) => Err(Signal::Raise(exc)),
        None => Ok(()),
    }
}

/// Whether the CURRENT thread has a pending interrupt (a cheap peek used inside
/// a condvar-wait loop before committing to `check_interrupt`'s take).
fn interrupt_pending() -> bool {
    let t = CURRENT
        .with(|c| c.lock().clone())
        .unwrap_or_else(main_thread);
    t.interrupt.lock().is_some()
}

/// `Thread#status` -- "run" while alive, `false` after a clean finish, `nil`
/// after one that ended in an exception. (We can only observe the exit code
/// once the outcome is cached, i.e. after a `join`; before that a finished
/// thread still reads "run", matching what this cooperative scheduler can
/// see without joining.)
pub fn thread_status(t: &RThread) -> RubyValue {
    if t.is_main {
        return RubyValue::Str(crate::string_new("run".to_string()));
    }
    match &*t.state.lock() {
        Some(ThreadState::Running(_)) | None => {
            RubyValue::Str(crate::string_new("run".to_string()))
        }
        Some(ThreadState::Done(Ok(_))) => RubyValue::Bool(false),
        Some(ThreadState::Done(Err(_))) => RubyValue::Nil,
    }
}

pub fn thread_name(t: &RThread) -> RubyValue {
    match &*t.name.lock() {
        Some(n) => RubyValue::Str(crate::string_new(n.clone())),
        None => RubyValue::Nil,
    }
}

pub fn thread_set_name(t: &RThread, name: Option<String>) {
    *t.name.lock() = name;
}

pub fn thread_report_on_exception(t: &RThread) -> bool {
    t.report_on_exception.load(Ordering::Relaxed)
}

pub fn thread_set_report_on_exception(t: &RThread, v: bool) {
    t.report_on_exception.store(v, Ordering::Relaxed);
}

/// `Thread#[]` -- a fiber-local value, or nil.
pub fn thread_local_get(t: &RThread, key: Symbol) -> RubyValue {
    t.locals.lock().get(&key).cloned().unwrap_or(RubyValue::Nil)
}

pub fn thread_local_set(t: &RThread, key: Symbol, value: RubyValue) {
    t.locals.lock().insert(key, value);
}

pub fn thread_local_key(t: &RThread, key: Symbol) -> bool {
    t.locals.lock().contains_key(&key)
}

pub fn thread_local_keys(t: &RThread) -> Vec<RubyValue> {
    t.locals
        .lock()
        .keys()
        .map(|k| RubyValue::Symbol(*k))
        .collect()
}

pub fn thread_variable_get(t: &RThread, key: Symbol) -> RubyValue {
    t.tvars.lock().get(&key).cloned().unwrap_or(RubyValue::Nil)
}

pub fn thread_variable_set(t: &RThread, key: Symbol, value: RubyValue) {
    t.tvars.lock().insert(key, value);
}

pub fn thread_variable_key(t: &RThread, key: Symbol) -> bool {
    t.tvars.lock().contains_key(&key)
}

pub fn thread_variable_keys(t: &RThread) -> Vec<RubyValue> {
    t.tvars
        .lock()
        .keys()
        .map(|k| RubyValue::Symbol(*k))
        .collect()
}

/// Blocks (a real `may` yield point) until the thread finishes, then
/// returns its stored outcome -- `Err(Signal)` re-raises in the CALLER,
/// CRuby's `thread_join` semantics. A Rust PANIC inside the thread
/// propagates here with its original payload (same whole-process posture
/// as every other runtime panic). `join` and `value` share this; only what
/// the codegen arm does with the `Ok` differs (`join` -> the thread
/// itself, `value` -> the block's result).
pub fn thread_outcome(t: &RThread) -> Result<RubyValue, Signal> {
    // Take the handle OUT of the lock before joining: holding a
    // parking_lot guard across a may yield point would block the whole
    // worker thread, not just this coroutine.
    let taken = t.state.lock().take();
    match taken {
        Some(ThreadState::Running(handle)) => {
            let outcome = match handle.join() {
                Ok(result) => result,
                Err(panic_payload) => std::panic::resume_unwind(panic_payload),
            };
            *t.state.lock() = Some(ThreadState::Done(outcome.clone()));
            outcome
        }
        Some(ThreadState::Done(outcome)) => {
            *t.state.lock() = Some(ThreadState::Done(outcome.clone()));
            outcome
        }
        // Another coroutine is currently INSIDE `handle.join()` for this
        // same thread (it took the handle; the state slot is empty until it
        // stores `Done`). CRuby lets every joiner wait and hand each the
        // same outcome -- poll for the first joiner's stored result, the
        // same 2ms cadence the queue waits use. (If the target PANICKED,
        // the first joiner re-threw and the process is already dying;
        // spinning here briefly is moot.)
        None => loop {
            if let Some(ThreadState::Done(outcome)) = &*t.state.lock() {
                return outcome.clone();
            }
            may::coroutine::sleep(std::time::Duration::from_millis(2));
        },
    }
}

/// Whether the thread is still running (`Thread#alive?`) -- a peek at the
/// state that, unlike `thread_outcome`, never joins or consumes the handle.
pub fn thread_alive(t: &RThread) -> bool {
    t.is_main || matches!(&*t.state.lock(), Some(ThreadState::Running(_)))
}

// ---------------------------------------------------------------------------
// Mutex
// ---------------------------------------------------------------------------

pub struct MutexData {
    /// Holds the single availability token -- see module docs.
    token_tx: may::sync::mpmc::Sender<()>,
    token_rx: may::sync::mpmc::Receiver<()>,
    owner: PlMutex<Option<u64>>,
    /// `.frozen?` state -- flag-only (CRuby allows locking a frozen Mutex,
    /// oracle-verified).
    frozen: AtomicBool,
}

pub type RMutex = Arc<MutexData>;

impl MutexData {
    /// `Mutex#frozen?` -- see the `frozen` field.
    pub fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    /// `Mutex#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }
}

pub fn mutex_new() -> RubyValue {
    let (token_tx, token_rx) = may::sync::mpmc::channel();
    token_tx.send(()).expect("priming a fresh mutex's token");
    RubyValue::Mutex(Arc::new(MutexData {
        token_tx,
        token_rx,
        owner: PlMutex::new(None),
        frozen: AtomicBool::new(false),
    }))
}

/// `Err` carries the exact CRuby `ThreadError` message; construction of the
/// exception itself is codegen's job.
pub fn mutex_lock(m: &RMutex) -> Result<(), &'static str> {
    let me = execution_id();
    if *m.owner.lock() == Some(me) {
        return Err("deadlock; recursive locking");
    }
    m.token_rx
        .recv()
        .expect("mutex token channel can't disconnect while the mutex is alive");
    *m.owner.lock() = Some(me);
    Ok(())
}

pub fn mutex_unlock(m: &RMutex) -> Result<(), &'static str> {
    let me = execution_id();
    let mut owner = m.owner.lock();
    match *owner {
        None => return Err("Attempt to unlock a mutex which is not locked"),
        Some(o) if o != me => {
            return Err("Attempt to unlock a mutex which is locked by another thread/fiber");
        }
        Some(_) => {}
    }
    *owner = None;
    drop(owner);
    m.token_tx
        .send(())
        .expect("mutex token channel can't disconnect while the mutex is alive");
    Ok(())
}

pub fn mutex_locked(m: &RMutex) -> bool {
    m.owner.lock().is_some()
}

/// `Mutex#try_lock` -- acquire without blocking. `true` if the lock was free
/// and is now ours; `false` if it was already held (by anyone, including
/// ourselves -- CRuby never deadlocks on a recursive `try_lock`).
pub fn mutex_try_lock(m: &RMutex) -> bool {
    let me = execution_id();
    if *m.owner.lock() == Some(me) {
        return false;
    }
    match m.token_rx.try_recv() {
        Ok(()) => {
            *m.owner.lock() = Some(me);
            true
        }
        Err(_) => false,
    }
}

pub fn mutex_owned(m: &RMutex) -> bool {
    *m.owner.lock() == Some(execution_id())
}

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

struct QueueInner {
    items: VecDeque<RubyValue>,
    closed: bool,
    /// `Some(n)` for a `SizedQueue` -- `push` back-pressures at `n` items.
    /// `None` for an unbounded `Queue`. Mutable via `SizedQueue#max=`.
    max: Option<usize>,
}

pub struct QueueData {
    /// `may::sync::Mutex` (not `parking_lot`) because `pop`'s wait must be
    /// a real `may::sync::Condvar` wait -- a coroutine-yielding block, not
    /// an OS-thread block that would stall the whole single-worker
    /// scheduler.
    inner: may::sync::Mutex<QueueInner>,
    not_empty: may::sync::Condvar,
    /// Wakes a `push` back-pressured on a full `SizedQueue` after a `pop`
    /// frees a slot.
    not_full: may::sync::Condvar,
    /// Whether this value is a `SizedQueue` (vs a plain `Queue`) -- fixed at
    /// construction, so `class_id` reads it lock-free. Distinct from `max`,
    /// which is the (mutable) bound: the class never changes even if `max=`
    /// does.
    is_sized: bool,
}

pub type RQueue = Arc<QueueData>;

/// Whether a queue value is a `SizedQueue` -- drives `RubyValue::class_id`.
pub fn queue_is_sized(q: &RQueue) -> bool {
    q.is_sized
}

fn queue_with(max: Option<usize>, is_sized: bool) -> RubyValue {
    RubyValue::Queue(Arc::new(QueueData {
        inner: may::sync::Mutex::new(QueueInner {
            items: VecDeque::new(),
            closed: false,
            max,
        }),
        not_empty: may::sync::Condvar::new(),
        not_full: may::sync::Condvar::new(),
        is_sized,
    }))
}

pub fn queue_new() -> RubyValue {
    queue_with(None, false)
}

/// `SizedQueue.new(n)` -- a bounded queue whose `push` blocks (coroutine-
/// yieldingly) once `n` items are enqueued, until a `pop` frees a slot.
pub fn sized_queue_new(n: i64) -> RubyValue {
    queue_with(Some(n.max(0) as usize), true)
}

/// `SizedQueue#max` -- the current bound (`None` for an unbounded `Queue`).
pub fn queue_max(q: &RQueue) -> Option<i64> {
    q.inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .max
        .map(|m| m as i64)
}

/// `SizedQueue#max=` -- raise or lower the bound; a raised bound wakes any
/// back-pressured pushers.
pub fn queue_set_max(q: &RQueue, n: i64) {
    let mut inner = q.inner.lock().unwrap_or_else(|e| e.into_inner());
    inner.max = Some(n.max(0) as usize);
    drop(inner);
    q.not_full.notify_all();
}

/// `Err` = `ClosedQueueError: "queue closed"` (message via codegen, as
/// always). `may`'s std-style lock poisoning is unwrapped into the inner
/// guard -- a panicking coroutine mid-queue-op is already a dying process
/// in this runtime's posture, matching `parking_lot`'s no-poison stance
/// everywhere else.
pub fn queue_push(q: &RQueue, value: RubyValue) -> Result<(), &'static str> {
    let mut inner = q.inner.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if inner.closed {
            return Err("queue closed");
        }
        // A `SizedQueue` at capacity back-pressures until a `pop` frees a
        // slot; an unbounded `Queue` (`max` = None) never waits.
        match inner.max {
            Some(m) if inner.items.len() >= m => {
                inner = q.not_full.wait(inner).unwrap_or_else(|e| e.into_inner());
            }
            _ => break,
        }
    }
    inner.items.push_back(value);
    q.not_empty.notify_one();
    Ok(())
}

/// Blocks (coroutine-yielding) while empty and open; a CLOSED empty queue
/// returns nil -- CRuby `thread_sync.c:1034`. This is an interruption
/// CHECKPOINT: a `Thread#kill`/`#raise` on the blocked thread is delivered
/// here (`Err(Signal)`), so the wait uses a short TIMEOUT and re-checks the
/// interrupt each cycle (the killer doesn't own this condvar to notify it).
pub fn queue_pop(q: &RQueue) -> Result<RubyValue, Signal> {
    check_interrupt()?;
    let mut inner = q.inner.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if let Some(v) = inner.items.pop_front() {
            // A freed slot may unblock a `SizedQueue` pusher.
            q.not_full.notify_one();
            return Ok(v);
        }
        if inner.closed {
            return Ok(RubyValue::Nil);
        }
        let (guard, _timed_out) = q
            .not_empty
            .wait_timeout(inner, Duration::from_millis(2))
            .unwrap_or_else(|e| e.into_inner());
        inner = guard;
        // Deliver a pending kill/raise now that we're awake, dropping the lock
        // first so the unwinding thread isn't holding the queue mutex.
        if interrupt_pending() {
            drop(inner);
            check_interrupt()?;
            // No interrupt after all (a spurious peek); re-acquire and continue.
            inner = q.inner.lock().unwrap_or_else(|e| e.into_inner());
        }
    }
}

pub fn queue_close(q: &RQueue) {
    let mut inner = q.inner.lock().unwrap_or_else(|e| e.into_inner());
    inner.closed = true;
    // Every parked popper must wake to observe closure (and drain or nil).
    q.not_empty.notify_all();
}

pub fn queue_closed(q: &RQueue) -> bool {
    q.inner.lock().unwrap_or_else(|e| e.into_inner()).closed
}

pub fn queue_len(q: &RQueue) -> i64 {
    q.inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .items
        .len() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit tests run on plain OS threads (no may coroutine ambient), which
    /// exercises that the primitives work from thread context too -- may's
    /// sync types support both.
    #[test]
    fn queue_rendezvous_and_close_semantics() {
        let q_val = queue_new();
        let RubyValue::Queue(q) = &q_val else {
            panic!()
        };
        queue_push(q, RubyValue::Int(1)).unwrap();
        queue_push(q, RubyValue::Int(2)).unwrap();
        assert_eq!(queue_len(q), 2);
        assert!(matches!(queue_pop(q), Ok(RubyValue::Int(1))));
        queue_close(q);
        assert!(
            queue_push(q, RubyValue::Int(3)).is_err(),
            "push to closed queue"
        );
        // Closed queues still DRAIN before returning nil.
        assert!(matches!(queue_pop(q), Ok(RubyValue::Int(2))));
        assert!(matches!(queue_pop(q), Ok(RubyValue::Nil)));
    }

    #[test]
    fn mutex_error_semantics_match_cruby() {
        let m_val = mutex_new();
        let RubyValue::Mutex(m) = &m_val else {
            panic!()
        };
        assert_eq!(
            mutex_unlock(m).unwrap_err(),
            "Attempt to unlock a mutex which is not locked"
        );
        mutex_lock(m).unwrap();
        assert!(mutex_locked(m));
        assert!(mutex_owned(m));
        assert_eq!(mutex_lock(m).unwrap_err(), "deadlock; recursive locking");
        mutex_unlock(m).unwrap();
        assert!(!mutex_locked(m));
    }

    // The next four tests pin the load-bearing assumptions of the staged
    // OS-thread migration: during the flag-staged window, generated code
    // runs on BARE OS THREADS while `may` is still linked, so (a) `may`'s
    // coroutine-local storage must fall back to a fresh per-OS-thread slot
    // outside any coroutine, and (b) its sync primitives must park and wake
    // plain threads. If any of these fail, the migration must be a single
    // cutover instead of a flag-staged one.

    /// `execution_id` (the Mutex-ownership key) is a coroutine-local read:
    /// stable within one OS thread, distinct across OS threads.
    #[test]
    fn execution_ids_isolate_per_bare_os_thread() {
        let main_a = execution_id();
        let main_b = execution_id();
        assert_eq!(main_a, main_b, "stable within a thread");
        let t1 = std::thread::spawn(execution_id).join().unwrap();
        let t2 = std::thread::spawn(execution_id).join().unwrap();
        assert_ne!(t1, main_a);
        assert_ne!(t2, main_a);
        assert_ne!(t1, t2);
    }

    /// A mutated `RefCell`-valued coroutine-local (the shape of the
    /// handling/catch-tag/proc-home stacks) must NOT leak into a fresh OS
    /// thread, and the writer's own slot must survive the other thread.
    #[test]
    fn refcell_coroutine_locals_do_not_leak_across_os_threads() {
        may::coroutine_local!(
            static PROBE: std::cell::RefCell<Vec<u64>> = std::cell::RefCell::new(Vec::new())
        );
        PROBE.with(|p| p.borrow_mut().push(1));
        let seen = std::thread::spawn(|| PROBE.with(|p| p.borrow().len()))
            .join()
            .unwrap();
        assert_eq!(seen, 0, "a fresh OS thread must get a fresh slot");
        assert_eq!(PROBE.with(|p| p.borrow().len()), 1);
    }

    /// An empty `Queue#pop` parks a bare OS thread; a push from ANOTHER
    /// bare OS thread wakes it with the value.
    #[test]
    fn queue_pop_parks_and_wakes_across_bare_os_threads() {
        let q_val = queue_new();
        let RubyValue::Queue(q) = &q_val else {
            panic!()
        };
        let q2 = q.clone();
        let popper = std::thread::spawn(move || queue_pop(&q2));
        // Let the popper reach the parked wait before the push.
        std::thread::sleep(Duration::from_millis(20));
        queue_push(q, RubyValue::Int(7)).unwrap();
        assert!(matches!(popper.join().unwrap(), Ok(RubyValue::Int(7))));
    }

    /// A contended Ruby `Mutex` (the may mpmc token channel) blocks a bare
    /// OS thread until the owning THREAD unlocks, and ownership (keyed by
    /// per-thread execution id) transfers to the waiter.
    #[test]
    fn mutex_token_hands_off_across_bare_os_threads() {
        let m_val = mutex_new();
        let RubyValue::Mutex(m) = &m_val else {
            panic!()
        };
        mutex_lock(m).unwrap();
        let m2 = m.clone();
        let waiter = std::thread::spawn(move || {
            mutex_lock(&m2).unwrap();
            let owned = mutex_owned(&m2);
            mutex_unlock(&m2).unwrap();
            owned
        });
        std::thread::sleep(Duration::from_millis(20));
        mutex_unlock(m).unwrap();
        assert!(
            waiter.join().unwrap(),
            "the waiter thread must own the mutex after the handoff"
        );
    }
}
