//! `Thread`/`Mutex`/`Queue` (Phase 13.5) -- thin Ruby-visible wrappers over
//! `may`'s green coroutines and sync primitives. A Ruby `Thread` is a `may`
//! coroutine, so under the default single scheduler worker (see `exec.rs`)
//! genuine parallelism is structurally impossible -- CRuby's GVL reality --
//! while `--no-gvl`/`SPINEL_THREADS=N` gives real OS parallelism over the
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
use std::sync::{Arc, OnceLock};

/// A unique id per execution context (the main coroutine, each Thread) --
/// CRuby's `ec_serial` analogue, backing `Mutex` ownership.
fn execution_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    may::coroutine_local!(static ID: u64 = NEXT.fetch_add(1, Ordering::Relaxed));
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
        })
    }
}

/// The main thread's `Thread` object -- one process-wide instance, so
/// `Thread.main` and a top-level `Thread.current` are the same identity.
static MAIN_THREAD: OnceLock<RThread> = OnceLock::new();

fn main_thread() -> RThread {
    MAIN_THREAD.get_or_init(|| ThreadData::build(None, true)).clone()
}

may::coroutine_local!(static CURRENT: PlMutex<Option<RThread>> = PlMutex::new(None));

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

/// `Thread.pass` -- a scheduler hint to let other coroutines run; nil.
pub fn thread_pass() -> RubyValue {
    may::coroutine::yield_now();
    RubyValue::Nil
}

/// `Thread.new(*args) { |*params| ... }` -- `args` pass through to the
/// block's params (bound leniently by the ordinary Proc machinery),
/// matching CRuby.
pub fn thread_new(block: RubyValue, args: Vec<RubyValue>) -> RubyValue {
    let body = block.as_proc_unchecked();
    let data = ThreadData::build(None, false);
    let for_coro = data.clone();
    let handle = may::go!(move || {
        // Record identity so `Thread.current` inside the body finds THIS
        // thread rather than falling through to main.
        CURRENT.with(|c| *c.lock() = Some(for_coro));
        body.call(&args)
    });
    *data.state.lock() = Some(ThreadState::Running(handle));
    RubyValue::Thread(data)
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
        Some(ThreadState::Running(_)) | None => RubyValue::Str(crate::string_new("run".to_string())),
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
    t.locals.lock().keys().map(|k| RubyValue::Symbol(*k)).collect()
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
    t.tvars.lock().keys().map(|k| RubyValue::Symbol(*k)).collect()
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
        // same thread. Rare enough (two joiners racing) that the honest
        // spike answer is a loud failure, not a silent wrong one.
        None => panic!("concurrent join/value on the same Thread isn't supported yet (spike scope)"),
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
}

pub type RMutex = Arc<MutexData>;

pub fn mutex_new() -> RubyValue {
    let (token_tx, token_rx) = may::sync::mpmc::channel();
    token_tx.send(()).expect("priming a fresh mutex's token");
    RubyValue::Mutex(Arc::new(MutexData {
        token_tx,
        token_rx,
        owner: PlMutex::new(None),
    }))
}

/// `Err` carries the exact CRuby `ThreadError` message; construction of the
/// exception itself is codegen's job.
pub fn mutex_lock(m: &RMutex) -> Result<(), &'static str> {
    let me = execution_id();
    if *m.owner.lock() == Some(me) {
        return Err("deadlock; recursive locking");
    }
    m.token_rx.recv().expect("mutex token channel can't disconnect while the mutex is alive");
    *m.owner.lock() = Some(me);
    Ok(())
}

pub fn mutex_unlock(m: &RMutex) -> Result<(), &'static str> {
    let me = execution_id();
    let mut owner = m.owner.lock();
    match *owner {
        None => return Err("Attempt to unlock a mutex which is not locked"),
        Some(o) if o != me => {
            return Err("Attempt to unlock a mutex which is locked by another thread/fiber")
        }
        Some(_) => {}
    }
    *owner = None;
    drop(owner);
    m.token_tx.send(()).expect("mutex token channel can't disconnect while the mutex is alive");
    Ok(())
}

pub fn mutex_locked(m: &RMutex) -> bool {
    m.owner.lock().is_some()
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
}

pub struct QueueData {
    /// `may::sync::Mutex` (not `parking_lot`) because `pop`'s wait must be
    /// a real `may::sync::Condvar` wait -- a coroutine-yielding block, not
    /// an OS-thread block that would stall the whole single-worker
    /// scheduler.
    inner: may::sync::Mutex<QueueInner>,
    not_empty: may::sync::Condvar,
}

pub type RQueue = Arc<QueueData>;

pub fn queue_new() -> RubyValue {
    RubyValue::Queue(Arc::new(QueueData {
        inner: may::sync::Mutex::new(QueueInner {
            items: VecDeque::new(),
            closed: false,
        }),
        not_empty: may::sync::Condvar::new(),
    }))
}

/// `Err` = `ClosedQueueError: "queue closed"` (message via codegen, as
/// always). `may`'s std-style lock poisoning is unwrapped into the inner
/// guard -- a panicking coroutine mid-queue-op is already a dying process
/// in this spike's posture, matching `parking_lot`'s no-poison stance
/// everywhere else.
pub fn queue_push(q: &RQueue, value: RubyValue) -> Result<(), &'static str> {
    let mut inner = q.inner.lock().unwrap_or_else(|e| e.into_inner());
    if inner.closed {
        return Err("queue closed");
    }
    inner.items.push_back(value);
    q.not_empty.notify_one();
    Ok(())
}

/// Blocks (coroutine-yielding) while empty and open; a CLOSED empty queue
/// returns nil -- CRuby `thread_sync.c:1034`.
pub fn queue_pop(q: &RQueue) -> RubyValue {
    let mut inner = q.inner.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if let Some(v) = inner.items.pop_front() {
            return v;
        }
        if inner.closed {
            return RubyValue::Nil;
        }
        inner = q.not_empty.wait(inner).unwrap_or_else(|e| e.into_inner());
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
    q.inner.lock().unwrap_or_else(|e| e.into_inner()).items.len() as i64
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
        let RubyValue::Queue(q) = &q_val else { panic!() };
        queue_push(q, RubyValue::Int(1)).unwrap();
        queue_push(q, RubyValue::Int(2)).unwrap();
        assert_eq!(queue_len(q), 2);
        assert!(matches!(queue_pop(q), RubyValue::Int(1)));
        queue_close(q);
        assert!(queue_push(q, RubyValue::Int(3)).is_err(), "push to closed queue");
        // Closed queues still DRAIN before returning nil.
        assert!(matches!(queue_pop(q), RubyValue::Int(2)));
        assert!(matches!(queue_pop(q), RubyValue::Nil));
    }

    #[test]
    fn mutex_error_semantics_match_cruby() {
        let m_val = mutex_new();
        let RubyValue::Mutex(m) = &m_val else { panic!() };
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
}
