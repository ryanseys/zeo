//! `Thread`/`Mutex`/`Queue` -- Ruby's threading surface over REAL OS
//! threads (8MiB stacks, one per `Thread.new`), truly parallel by default:
//! per-object `Arc`/`parking_lot::Mutex` state (Part 9) carries memory
//! safety, and per-builtin-op atomicity matches the granularity CRuby's
//! GVL actually guarantees for its C-implemented methods. `ZEO_GVL=1`
//! arms the opt-in serialized scheduling mode (see `gvl`). Asynchronous
//! interrupts (`#kill`/`#raise`) deliver at `check_ints` checkpoints --
//! loop back-edges, method prologues, block exits, and every blocking
//! primitive here.
//!
//! Error contract (verified against CRuby `thread.c`/`thread_sync.c`, see
//! the plan's Part 11 addendum): an uncaught exception inside a Thread is
//! STORED and re-raised in whoever calls `#join`/`#value` (`thread.c:1195`)
//! AND, separately, reported to stderr as the thread terminates unless
//! `report_on_exception` is off for it -- a program can see both; Ruby
//! `Mutex` is NOT reentrant and ownership is per-execution-
//! context (`ec_serial`, `thread_sync.c:9`) -- relock by the owner raises
//! `ThreadError: "deadlock; recursive locking"`, foreign/idle unlock raises
//! too; `Queue#pop` on a closed empty queue returns nil, `#push` to a
//! closed queue raises `ClosedQueueError` (`thread_sync.c:969,1034`). All
//! exception CONSTRUCTION happens in codegen (the `IndexError` division of
//! labor); runtime functions return `Err(message)` / enums.
//!
//! The Ruby `Mutex` is an OWNER CELL plus condvar (not a Rust mutex whose
//! RAII guard can't span Ruby's split `lock`/`unlock` calls -- a guard is
//! lifetime-bound to the locking scope; Ruby's isn't): `lock` parks on the
//! condvar until the cell reads `None`, `unlock` clears it and wakes one
//! waiter. Owner identity for the error semantics comes from
//! [`execution_id`], a lazily-assigned per-thread id -- per-EXECUTION-
//! CONTEXT like CRuby's `ec_serial`.

use crate::{RubyValue, Signal, Symbol};
use parking_lot::Mutex as PlMutex;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
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
    std::thread_local!(static ID: u64 = NEXT.fetch_add(1, Ordering::Relaxed));
    ID.with(|id| *id)
}

// ---------------------------------------------------------------------------
// Thread
// ---------------------------------------------------------------------------

enum ThreadState {
    Running(std::thread::JoinHandle<Result<RubyValue, Signal>>),
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
    /// `Thread#abort_on_exception` -- CRuby defaults this to false. Set, an
    /// uncaught exception in this thread takes the whole process down instead
    /// of dying quietly with the thread (minitest's parallel workers set it).
    abort_on_exception: AtomicBool,
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
    /// This thread's scheduling ctx, registered by its first interruptible
    /// sleep -- lets `#kill`/`#raise` wake exactly this sleeper instead of
    /// waiting out the timeout.
    ctx: PlMutex<Option<std::sync::Arc<crate::gvl::ThreadCtx>>>,
    /// Set by `#kill`; makes the thread's outcome a silent `nil` no matter what
    /// the unwinding exception was (so even a `rescue Exception` can't keep a
    /// killed thread alive).
    was_killed: AtomicBool,
    /// `.frozen?` state -- flag-only (a frozen Thread still runs, joins, and
    /// answers reflection in CRuby; only `#[]=`/`#name=`-style mutations
    /// check it, audited with the mutator family).
    frozen: AtomicBool,
    /// `file:line` where `Thread.new` was called -- shown in `#inspect`
    /// (`#<Thread:0xADDR file:line status>`). `None` for the main thread.
    origin: Option<String>,
    /// `Thread#priority`/`#priority=` -- stored and read back, never acted on.
    /// CRuby's is advisory too: on the platforms where `setpriority` needs
    /// privileges it also only records the number.
    priority: AtomicI64,
    /// Set while this thread is parked inside `Thread.stop`, cleared by
    /// `#wakeup`/`#run` -- what `#stop?` reports for a live thread.
    stopped: AtomicBool,
    /// The OS thread id, recorded by the thread ITSELF at start (no other
    /// thread can read it). Zero until then, which is what makes
    /// `#native_thread_id` answer nil for a thread that never ran.
    native_id: AtomicU64,
}

pub type RThread = Arc<ThreadData>;

/// `Thread.report_on_exception` -- the process-wide default each new thread
/// copies at spawn (true, as in CRuby). It lives here, beside the per-thread
/// flag it seeds, so `ThreadData::build` can read it; `Thread`'s class-method
/// rows are just accessors over it.
static REPORT_ON_EXCEPTION: AtomicBool = AtomicBool::new(true);

pub fn report_on_exception_default() -> bool {
    REPORT_ON_EXCEPTION.load(Ordering::Relaxed)
}

pub fn set_report_on_exception_default(on: bool) {
    REPORT_ON_EXCEPTION.store(on, Ordering::Relaxed);
}

/// `Thread.abort_on_exception` -- the process-wide default each new thread
/// copies at spawn. False, as in CRuby: a thread that dies of an uncaught
/// exception normally takes only itself down.
static ABORT_ON_EXCEPTION: AtomicBool = AtomicBool::new(false);

pub fn abort_on_exception_default() -> bool {
    ABORT_ON_EXCEPTION.load(Ordering::Relaxed)
}

pub fn set_abort_on_exception_default(on: bool) {
    ABORT_ON_EXCEPTION.store(on, Ordering::Relaxed);
}

impl ThreadData {
    fn build(state: Option<ThreadState>, is_main: bool, origin: Option<String>) -> RThread {
        Arc::new(ThreadData {
            state: PlMutex::new(state),
            name: PlMutex::new(None),
            report_on_exception: AtomicBool::new(report_on_exception_default()),
            abort_on_exception: AtomicBool::new(abort_on_exception_default()),
            locals: PlMutex::new(HashMap::new()),
            tvars: PlMutex::new(HashMap::new()),
            is_main,
            interrupt: PlMutex::new(None),
            ctx: PlMutex::new(None),
            was_killed: AtomicBool::new(false),
            frozen: AtomicBool::new(false),
            origin,
            priority: AtomicI64::new(0),
            stopped: AtomicBool::new(false),
            native_id: AtomicU64::new(0),
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
            let m = ThreadData::build(None, true, None);
            register_live(&m);
            m
        })
        .clone()
}

std::thread_local!(static CURRENT: PlMutex<Option<RThread>> = const { PlMutex::new(None) });

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

/// `Thread.pass` -- give up the current quantum; nil.
pub fn thread_pass() -> RubyValue {
    // Armed Gvl: rejoin the back of the FIFO queue (a real handoff);
    // parallel: a plain OS yield is all "pass" can mean.
    crate::gvl::process_gvl().yield_now();
    std::thread::yield_now();
    RubyValue::Nil
}

/// `Thread.new(*args) { |*params| ... }` -- `args` pass through to the
/// block's params (bound leniently by the ordinary Proc machinery),
/// matching CRuby.
pub fn thread_new(block: RubyValue, args: Vec<RubyValue>) -> RubyValue {
    let body = block.as_proc_unchecked();
    let origin = crate::frames::current_location().map(|(f, l)| format!("{f}:{l}"));
    let data = ThreadData::build(None, false, origin);
    register_live(&data);
    let for_thread = data.clone();
    let run = move || {
        // Record identity so `Thread.current` inside the body finds THIS
        // thread rather than falling through to main.
        CURRENT.with(|c| *c.lock() = Some(for_thread.clone()));
        for_thread.native_id.store(native_id(), Ordering::Relaxed);
        // The frame stack needs no management here: this closure runs on
        // a brand-new OS thread whose `frames` TLS starts empty.
        let result = body.call(&args);
        // A killed thread dies silently with a nil value, whatever exception
        // unwound it (its `ensure` blocks already ran during that unwind).
        if for_thread.was_killed.load(Ordering::Relaxed) {
            return Ok(RubyValue::Nil);
        }
        if let Err(Signal::Raise(exc)) = &result {
            report_terminated(&for_thread, exc);
        }
        result
    };
    // A real OS thread: CRuby-sized 8MiB stack, its own scheduling ctx
    // attached to the process Gvl, the (usually disabled, hence free) Gvl
    // held for the body's duration -- released even on panic via the guard.
    // BEFORE the spawn -- see `gvl::note_thread_spawn`. From here on nothing
    // may take the sole-thread ivar path.
    crate::gvl::note_thread_spawn();
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let _ctx = crate::gvl::install_ctx();
            let _held = crate::gvl::process_gvl().hold();
            run()
        })
        .expect("spawning a Ruby Thread's OS thread");
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
    wake_target(t);
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
    wake_target(t);
}

/// Wake `t` if it registered an interruptible-sleep ctx (OS-thread mode) --
/// the queued interrupt delivers on its wake path rather than at timeout.
fn wake_target(t: &RThread) {
    if let Some(ctx) = &*t.ctx.lock() {
        ctx.wake();
    }
}

/// Register the calling OS thread's scheduling ctx on its own `ThreadData`
/// (main included, via the ordinary `Thread.current` fallback), so posters
/// can wake it -- `kernel_sleep`'s first os-mode call does this.
pub(crate) fn register_current_ctx(ctx: &std::sync::Arc<crate::gvl::ThreadCtx>) {
    let t = CURRENT
        .with(|c| c.lock().clone())
        .unwrap_or_else(main_thread);
    *t.ctx.lock() = Some(ctx.clone());
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

/// `Thread#pending_interrupt?` -- whether an asynchronous `#raise`/`#kill` is
/// queued for `t` and has not reached a checkpoint yet.
pub fn thread_pending_interrupt(t: &RThread) -> bool {
    t.interrupt.lock().is_some()
}

/// `Thread.pending_interrupt?` -- the same question about the caller.
pub fn current_pending_interrupt() -> bool {
    interrupt_pending()
}

// ---------------------------------------------------------------------------
// Scheduling state (`#priority`, `Thread.stop`/`#wakeup`/`#run`)
// ---------------------------------------------------------------------------

pub fn thread_priority(t: &RThread) -> i64 {
    t.priority.load(Ordering::Relaxed)
}

/// `Thread#priority=` -- stored, never acted on. Every thread here is a real
/// OS thread scheduled by the kernel, and CRuby's own priority is advisory on
/// the same platforms, so recording the number is the whole honest behavior.
pub fn thread_set_priority(t: &RThread, v: i64) {
    t.priority.store(v, Ordering::Relaxed);
}

/// `Thread.stop` -- park the caller until someone calls `#wakeup`/`#run` on
/// it. The park is the same interruptible sleep `Kernel#sleep` uses, so a
/// `#kill`/`#raise` still reaches a stopped thread.
pub fn thread_stop_current() -> Result<RubyValue, Signal> {
    let t = CURRENT
        .with(|c| c.lock().clone())
        .unwrap_or_else(main_thread);
    let ctx = crate::gvl::install_ctx();
    register_current_ctx(&ctx);
    t.stopped.store(true, Ordering::Relaxed);
    // A wake can arrive before the park starts (the flag inside `ThreadCtx`
    // is what keeps it from being lost), so loop on our own flag rather than
    // on the sleep's return.
    while t.stopped.load(Ordering::Relaxed) {
        crate::gvl::process_gvl().without(|| ctx.sleep(None));
        crate::check_ints()?;
    }
    Ok(RubyValue::Nil)
}

/// `Thread#wakeup` -- clear the stop flag and wake the target. `Err` when the
/// thread is already dead, which CRuby reports as a `ThreadError`.
pub fn thread_wakeup(t: &RThread) -> Result<(), &'static str> {
    if !thread_alive(t) {
        return Err("killed thread");
    }
    t.stopped.store(false, Ordering::Relaxed);
    wake_target(t);
    Ok(())
}

/// `Thread#stop?` -- true while parked in `Thread.stop`, and always true for a
/// thread that has finished (CRuby counts dead as stopped).
pub fn thread_is_stopped(t: &RThread) -> bool {
    !thread_alive(t) || t.stopped.load(Ordering::Relaxed)
}

/// `Thread#native_thread_id` -- the OS-level id, which only the thread itself
/// can read, so it is recorded at start. `None` before that and for a thread
/// that has finished, matching CRuby's nil.
pub fn thread_native_id(t: &RThread) -> Option<u64> {
    if t.is_main {
        // Main never runs `thread_new`'s prologue; fill it in on first ask,
        // which is necessarily from main itself when it is the live thread.
        let _ = t
            .native_id
            .compare_exchange(0, native_id(), Ordering::Relaxed, Ordering::Relaxed);
    }
    if !thread_alive(t) {
        return None;
    }
    match t.native_id.load(Ordering::Relaxed) {
        0 => None,
        id => Some(id),
    }
}

/// This OS thread's kernel-visible id.
fn native_id() -> u64 {
    #[cfg(target_vendor = "apple")]
    {
        let mut id: u64 = 0;
        // SAFETY: the null pthread_t means "this thread"; `id` is a live u64.
        unsafe { libc::pthread_threadid_np(0, &mut id) };
        id
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        // SAFETY: `gettid` takes no arguments and cannot fail.
        (unsafe { libc::gettid() }) as u64
    }
}

/// `Thread.ignore_deadlock` -- stored, and nothing reads it: zeo runs real OS
/// threads with no deadlock detector to switch off, so a program that sets it
/// gets exactly the behavior it asked for.
static IGNORE_DEADLOCK: AtomicBool = AtomicBool::new(false);

pub fn ignore_deadlock() -> bool {
    IGNORE_DEADLOCK.load(Ordering::Relaxed)
}

pub fn set_ignore_deadlock(on: bool) {
    IGNORE_DEADLOCK.store(on, Ordering::Relaxed);
}

/// `Thread#[]`'s `fetch` twin -- the stored value, or `None` for a miss (the
/// caller supplies CRuby's default/block/`KeyError` handling).
pub fn thread_local_fetch(t: &RThread, key: Symbol) -> Option<RubyValue> {
    t.locals.lock().get(&key).cloned()
}

/// `Thread#status` -- "run" while alive, `false` after a clean finish, `nil`
/// after one that ended in an exception. (We can only observe the exit code
/// once the outcome is cached, i.e. after a `join`; before that a finished
/// thread still reads "run", since the outcome isn't observable until
/// joined.)
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

/// The `Thread.new` call site (`file:line`) shown in `#inspect`; `None` for
/// the main thread.
/// `Thread#inspect` -- `#<Thread:0xADDR file:line status>`, CRuby's shape,
/// including the `Thread.new` call site (the main thread has none). Shared
/// with the at-termination report, which names the thread the same way.
///
/// `status` is the caller's word rather than [`thread_alive`]'s, because a
/// thread reporting its OWN uncaught exception is plainly still running even
/// though its state cell may not hold the join handle yet -- `Thread.new`
/// fills that in after `spawn` returns, which the body can outrun.
pub fn thread_inspect(t: &RThread, status: &str) -> String {
    let addr = Arc::as_ptr(t) as usize;
    let origin = thread_origin(t).map_or_else(String::new, |o| format!(" {o}"));
    format!("#<Thread:0x{addr:016x}{origin} {status}>")
}

/// CRuby reports a thread that dies of an uncaught exception to stderr as it
/// terminates -- the thread's own `#inspect`, then the ordinary uncaught
/// report -- unless `report_on_exception` is off for it. This happens whether
/// or not anyone later joins: a `#join` re-raise and this report are separate
/// mechanisms, and a program can see both.
fn report_terminated(t: &RThread, exc: &RubyValue) {
    if t.report_on_exception.load(Ordering::Relaxed) {
        let preamble = format!(
            "{} terminated with exception (report_on_exception is true):",
            thread_inspect(t, "run")
        );
        crate::builtins::exception::report_exception(exc, Some(&preamble));
    }
    // `abort_on_exception` is the separate, louder flag: CRuby carries the
    // exception into the MAIN thread, which then dies of it. There is no main
    // thread to re-raise into from here, so the effect is produced directly --
    // the ordinary uncaught report, `at_exit` handlers, exit 1 -- which is
    // what the main thread's own death would have printed and returned.
    if t.abort_on_exception.load(Ordering::Relaxed)
        || (ABORT_ON_EXCEPTION.load(Ordering::Relaxed) && !t.is_main)
    {
        if !t.report_on_exception.load(Ordering::Relaxed) {
            crate::builtins::exception::report_exception(exc, None);
        }
        crate::run_at_exit();
        std::process::exit(1);
    }
}

pub fn thread_origin(t: &RThread) -> Option<String> {
    t.origin.clone()
}

pub fn thread_report_on_exception(t: &RThread) -> bool {
    t.report_on_exception.load(Ordering::Relaxed)
}

pub fn thread_set_report_on_exception(t: &RThread, v: bool) {
    t.report_on_exception.store(v, Ordering::Relaxed);
}

pub fn thread_abort_on_exception(t: &RThread) -> bool {
    t.abort_on_exception.load(Ordering::Relaxed)
}

pub fn thread_set_abort_on_exception(t: &RThread, v: bool) {
    t.abort_on_exception.store(v, Ordering::Relaxed);
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

/// Blocks until the thread finishes, then returns its stored outcome --
/// `Err(Signal)` re-raises in the CALLER, CRuby's `thread_join` semantics.
/// A Rust PANIC inside the thread propagates here with its original
/// payload (same whole-process posture as every other runtime panic).
/// `join` and `value` share this; only what the codegen arm does with the
/// `Ok` differs (`join` -> the thread itself, `value` -> the block's
/// result).
pub fn thread_outcome(t: &RThread) -> Result<RubyValue, Signal> {
    // Take the handle OUT of the lock before joining: a concurrent joiner
    // must be able to lock the state slot while we block in join.
    let taken = t.state.lock().take();
    match taken {
        Some(ThreadState::Running(handle)) => {
            // The joiner must not sit on an ARMED Gvl across the blocking
            // join -- the target needs it to finish (the release is free
            // when the Gvl is disabled, the default).
            let joined = crate::gvl::process_gvl().without(|| handle.join());
            let outcome = match joined {
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
            crate::gvl::process_gvl()
                .without(|| std::thread::sleep(std::time::Duration::from_millis(2)));
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
    /// `Some(execution_id)` while held. The one cell both the CRuby error
    /// semantics (owner identity) and the blocking handoff key off --
    /// waiters park on `freed` until it reads `None`.
    owner: PlMutex<Option<u64>>,
    freed: parking_lot::Condvar,
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
    RubyValue::Mutex(Arc::new(MutexData {
        owner: PlMutex::new(None),
        freed: parking_lot::Condvar::new(),
        frozen: AtomicBool::new(false),
    }))
}

/// `Err` carries the exact CRuby `ThreadError` message; construction of the
/// exception itself is codegen's job.
pub fn mutex_lock(m: &RMutex) -> Result<(), &'static str> {
    let me = execution_id();
    // The whole potentially-blocking section runs with an armed Gvl
    // released (free otherwise) -- a waiter holding the scheduling lock
    // would starve the very owner it waits on. The recursive-lock check
    // lives inside so the read and the park see one consistent owner.
    crate::gvl::process_gvl().without(|| {
        let mut owner = m.owner.lock();
        if *owner == Some(me) {
            return Err("deadlock; recursive locking");
        }
        while owner.is_some() {
            m.freed.wait(&mut owner);
        }
        *owner = Some(me);
        Ok(())
    })
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
    m.freed.notify_one();
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
    let mut owner = m.owner.lock();
    match *owner {
        None => {
            *owner = Some(me);
            true
        }
        Some(_) => false,
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
    inner: PlMutex<QueueInner>,
    not_empty: parking_lot::Condvar,
    /// Wakes a `push` back-pressured on a full `SizedQueue` after a `pop`
    /// frees a slot.
    not_full: parking_lot::Condvar,
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
        inner: PlMutex::new(QueueInner {
            items: VecDeque::new(),
            closed: false,
            max,
        }),
        not_empty: parking_lot::Condvar::new(),
        not_full: parking_lot::Condvar::new(),
        is_sized,
    }))
}

pub fn queue_new() -> RubyValue {
    queue_with(None, false)
}

/// `SizedQueue.new(n)` -- a bounded queue whose `push` blocks once `n`
/// items are enqueued, until a `pop` frees a slot.
pub fn sized_queue_new(n: i64) -> RubyValue {
    queue_with(Some(n.max(0) as usize), true)
}

/// `SizedQueue#max` -- the current bound (`None` for an unbounded `Queue`).
pub fn queue_max(q: &RQueue) -> Option<i64> {
    q.inner.lock().max.map(|m| m as i64)
}

/// `SizedQueue#max=` -- raise or lower the bound; a raised bound wakes any
/// back-pressured pushers.
pub fn queue_set_max(q: &RQueue, n: i64) {
    let mut inner = q.inner.lock();
    inner.max = Some(n.max(0) as usize);
    drop(inner);
    q.not_full.notify_all();
}

/// `Err` = `ClosedQueueError: "queue closed"` (message via codegen, as
/// always).
pub fn queue_push(q: &RQueue, value: RubyValue) -> Result<(), &'static str> {
    // The WHOLE lock-wait-store section runs with an armed Gvl released
    // (plain call-through otherwise). The release must wrap the queue's
    // own mutex region, not sit inside it: re-acquiring the Gvl while
    // still holding the queue guard inverts lock order against a holder
    // trying to lock this same queue -- the armed-mode gate found exactly
    // that deadlock on the SizedQueue back-pressure tests.
    crate::gvl::process_gvl().without(|| queue_push_locked(q, value))
}

fn queue_push_locked(q: &RQueue, value: RubyValue) -> Result<(), &'static str> {
    let mut inner = q.inner.lock();
    loop {
        if inner.closed {
            return Err("queue closed");
        }
        // A `SizedQueue` at capacity back-pressures until a `pop` frees a
        // slot; an unbounded `Queue` (`max` = None) never waits.
        match inner.max {
            Some(m) if inner.items.len() >= m => {
                q.not_full.wait(&mut inner);
            }
            _ => break,
        }
    }
    inner.items.push_back(value);
    q.not_empty.notify_one();
    Ok(())
}

/// Blocks while empty and open; a CLOSED empty queue
/// returns nil -- CRuby `thread_sync.c:1034`. This is an interruption
/// CHECKPOINT: a `Thread#kill`/`#raise` on the blocked thread is delivered
/// here (`Err(Signal)`), so the wait uses a short TIMEOUT and re-checks the
/// interrupt each cycle (the killer doesn't own this condvar to notify it).
pub fn queue_pop(q: &RQueue) -> Result<RubyValue, Signal> {
    check_interrupt()?;
    // Whole lock-wait-take section under one armed-Gvl release -- see
    // `queue_push` for the lock-order rationale (the Gvl must never be
    // re-acquired while the queue's own guard is held).
    crate::gvl::process_gvl().without(|| queue_pop_locked(q))
}

fn queue_pop_locked(q: &RQueue) -> Result<RubyValue, Signal> {
    let mut inner = q.inner.lock();
    loop {
        if let Some(v) = inner.items.pop_front() {
            // A freed slot may unblock a `SizedQueue` pusher.
            q.not_full.notify_one();
            return Ok(v);
        }
        if inner.closed {
            return Ok(RubyValue::Nil);
        }
        let _ = q.not_empty.wait_for(&mut inner, Duration::from_millis(2));
        // Deliver a pending kill/raise now that we're awake, dropping the lock
        // first so the unwinding thread isn't holding the queue mutex.
        if interrupt_pending() {
            drop(inner);
            check_interrupt()?;
            // No interrupt after all (a spurious peek); re-acquire and continue.
            inner = q.inner.lock();
        }
    }
}

pub fn queue_close(q: &RQueue) {
    let mut inner = q.inner.lock();
    inner.closed = true;
    // Every parked popper must wake to observe closure (and drain or nil).
    q.not_empty.notify_all();
}

pub fn queue_closed(q: &RQueue) -> bool {
    q.inner.lock().closed
}

pub fn queue_len(q: &RQueue) -> i64 {
    q.inner.lock().items.len() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// `execution_id` (the Mutex-ownership key) is a thread-local read:
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

    /// A contended Ruby `Mutex` blocks a bare OS thread until the owning
    /// THREAD unlocks, and ownership (keyed by per-thread execution id)
    /// transfers to the waiter.
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
