//! The OS-thread scheduling core: the (opt-in) GVL, per-thread interrupt
//! state, the preemption timer, and interruptible sleep.
//!
//! **The default mode is PARALLEL** -- [`Gvl::disabled`]: `acquire`/
//! `release`/`yield_now` are no-ops and Ruby threads run truly
//! concurrently. This is safe because every shared structure in this
//! runtime is `Arc` + `Mutex` (`RObj` is `Send + Sync` by construction),
//! which gives per-builtin-operation atomicity -- the same granularity
//! CRuby's GVL actually guarantees for its C-implemented methods (CRuby
//! never promised atomicity ACROSS operations; its timer switches threads
//! between any two bytecodes). `ZEO_GVL=1` arms the handoff for maximum
//! CRuby-fidelity scheduling: one runner at a time, FIFO handoff, timer
//! quanta.
//!
//! Interrupt delivery (`Thread#kill`/`#raise`, the timer) is identical in
//! both modes: a poster sets a bit on the target's [`ThreadCtx`] and bumps
//! the process-wide [`PENDING_GLOBAL`] counter; `check_ints`' fast path in
//! generated code is ONE relaxed load of that global (no TLS of any kind),
//! and only a nonzero value takes the slow path.
//!
//! `without_gvl` wraps every blocking primitive in the runtime (IO reads and
//! writes, `accept`, `Process.wait`, sleep), and `process_gvl` is what arms
//! them; the unit tests below pin the handoff contracts those depend on.

use parking_lot::{Condvar, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

/// Process-wide "some thread has an undelivered interrupt" counter -- the
/// `check_ints` fast path. Incremented when a bit is NEWLY set on some
/// [`ThreadCtx`], decremented when that bit is consumed, so it is exactly
/// the number of set bits across all live contexts: zero means every
/// thread's fast path can skip the slow path with one relaxed load.
///
/// Exported as a DATA symbol: emitted code loads it inline at loop
/// back-edges and only a nonzero value calls `zeo_rt_check_ints`. The
/// inline load is plain (the Rust reader is Relaxed too); a checkpoint
/// that races a post sees it on the next iteration, same as today.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static zeo_rt_pending_interrupts: AtomicU32 = AtomicU32::new(0);
use self::zeo_rt_pending_interrupts as PENDING_GLOBAL;

/// Whether any Ruby thread or Ractor has ever been spawned. Monotone in the
/// SAFE direction: it is only ever set, never cleared, so a program that
/// joins all its threads does not go back to claiming it is alone.
static MULTI_THREADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    /// Whether THIS thread is the only Ruby thread in the process, so an
    /// instance-variable access can read its slot directly instead of locking
    /// (see [`crate::IvarCell`]).
    ///
    /// The default is the SAFE answer, so a thread that never opts in --
    /// every spawned Ruby thread, and any thread this runtime does not know
    /// about -- keeps locking. Only the generated `main` opts in, at a point
    /// where exactly one Ruby thread exists and it is this one.
    static SOLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The generated `main` prologue: at this point one Ruby thread exists.
#[inline]
pub fn mark_sole_thread() {
    SOLE.with(|s| s.set(!MULTI_THREADED.load(Ordering::Acquire) && !sole_thread_refused()));
}

/// `ZEO_RT_NO_SOLE_THREAD=1`: never claim the sole-thread path, so every
/// ivar and container access takes its locking arm.
///
/// A MEASUREMENT dial, and it measures something the runtime does to itself:
/// loading any C extension calls [`arm_for_cext`], which clears the same
/// claim process-wide. So this is that cost with no gem in the way -- the
/// alternative is to A/B a Rust ext against its real gem, where the two
/// implementations differ by far more than the GVL and the number means
/// nothing. (One such attempt, 2026-08-29, compared 340ms against 344ms and
/// was invalid for a worse reason still: both runs had loaded zeo's builtin.)
///
/// MEASURED 2026-08-30, release bank, one subprocess per iteration:
/// attr_accessor 510->856ms (+68%), getivar 53.6->88.4ms (+65%), setivar and
/// setivar_object +17%, inline +9%. Every program that does not touch an ivar
/// or a container is flat. So the cost is not diffuse -- it is the four
/// `sole_thread()` sites, and it is why zeo keeps its own json/psych/date
/// rather than loading the C gem.
///
/// Read ONCE, and never from `sole_thread()` -- that one is on the path of
/// every container access, and a dial there would measure the dial.
fn sole_thread_refused() -> bool {
    static REFUSED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REFUSED.get_or_init(|| {
        std::env::var_os("ZEO_RT_NO_SOLE_THREAD").is_some_and(|v| !v.is_empty())
    })
}

/// Whether the caller may take a lock-free path over data only Ruby threads
/// reach. One thread-local byte; LLVM must reload it after any call it cannot
/// see through, which is exactly the condition under which the answer could
/// have changed.
#[inline(always)]
pub fn sole_thread() -> bool {
    SOLE.with(|s| s.get())
}

/// The exit handoff: the REAL main thread resumes after `ruby-main` joined
/// (`exec::run_main`) and runs `at_exit`/finalizers -- touching objects that
/// thread stamped. Sequentially safe, but a DIFFERENT thread, so it must not
/// keep a sole-thread claim of its own: clearing it routes the remaining
/// exit-path work through the locks.
pub fn clear_sole_thread() {
    SOLE.with(|s| s.set(false));
}

/// Called BEFORE spawning a Ruby thread or Ractor, never after.
///
/// Two happens-before edges close the invariant, and the second is the one a
/// naive design misses. Parent to CHILD: `thread::spawn` is itself a
/// synchronization point, so the child cannot start before the store. Parent
/// to its OWN later accesses: clearing `SOLE` is a plain write to a slot only
/// this thread reads, so being sequenced-before is all it needs. Since there
/// is exactly one Ruby thread before the first spawn, and it is the thread
/// calling this, no thread can be running with `SOLE` set once a second one
/// exists.
#[cold]
pub fn note_thread_spawn() {
    // A live sole-thread container guard (`Freezable::lock`'s fast arm) at
    // the moment a second thread comes into being would be a `&mut` no mutex
    // protects. No corpus program does this -- holding the LOCKED guard
    // across the same shape would deadlock -- but the invariant is asserted
    // where the whole golden corpus can test it.
    #[cfg(debug_assertions)]
    crate::collections::debug_assert_no_live_fast_guards("Thread/Ractor spawn");
    SOLE.with(|s| s.set(false));
    MULTI_THREADED.store(true, Ordering::Release);
}

/// Put this process back to "no Ruby thread has ever been spawned", so a test
/// that needs the sole-thread path can claim it whatever ran before.
///
/// `MULTI_THREADED` is deliberately monotone in production -- a program that
/// joins all its threads does not go back to claiming it is alone -- but a test
/// harness runs many programs in one process. `cargo nextest` gives each test
/// its own, so the gate never needs this; `cargo miri test` and plain
/// `cargo test` share one, and there the flag leaks between tests.
#[cfg(test)]
pub(crate) fn reset_thread_flags_for_test() {
    MULTI_THREADED.store(false, Ordering::Release);
    SOLE.with(|s| s.set(false));
}

/// The fast-path read: nonzero means SOME thread (possibly not the caller)
/// has an undelivered interrupt and the caller should run the slow path.
#[inline]
pub fn interrupts_pending_anywhere() -> bool {
    PENDING_GLOBAL.load(Ordering::Relaxed) != 0
}

/// Coroutine-mode shims: while Ruby threads are still coroutines with their
/// interrupt payload in `thread.rs`' per-thread slot (no [`ThreadCtx`] yet),
/// `Thread#kill`/`#raise` note a NEWLY filled slot here and
/// `check_interrupt` notes its consumption -- keeping the fast-path counter
/// exactly equal to the number of undelivered interrupts. The OS-thread
/// mode replaces these with [`ThreadCtx::post`]/[`ThreadCtx::take`].
pub(crate) fn note_posted() {
    PENDING_GLOBAL.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn note_consumed() {
    PENDING_GLOBAL.fetch_sub(1, Ordering::Relaxed);
}

/// Timer quantum expired -- the holder should `yield_now` (armed mode only).
pub const INT_TIMER: u32 = 1;
/// A `Thread#kill`/`#raise` is queued for this thread (the payload itself
/// stays in `thread.rs`' pending map; this bit is only the wakeup edge).
pub const INT_PENDING: u32 = 1 << 1;
/// A cycle collection wants this thread parked at its next checkpoint. It
/// rides the interrupt word rather than a gate of its own for the standing
/// reason: `check_ints`' fast path must stay one relaxed load, and a second
/// flag word beside it measured 2.6% on dispatch.
pub const INT_GC: u32 = 1 << 2;

/// Per-Ruby-thread scheduling state: the interrupt bits `check_ints`' slow
/// path filters on, and a private lock+condvar so `sleep` is interruptible
/// (a poster wakes the sleeper immediately rather than at quantum end).
#[derive(Default)]
pub struct ThreadCtx {
    ints: AtomicU32,
    /// The wake flag under its lock -- consumed by the sleeper, set by
    /// [`ThreadCtx::wake`]; a flag (not a bare notify) so a wake posted
    /// just BEFORE the sleep starts is not lost.
    sleep_mx: Mutex<bool>,
    sleep_cv: Condvar,
}

impl ThreadCtx {
    pub fn new() -> Arc<ThreadCtx> {
        Arc::new(ThreadCtx::default())
    }

    /// Set `bit` and wake the target if it is sleeping. The global counter
    /// moves only when the bit was NEWLY set, so double-posting one bit
    /// cannot strand the counter above the true set-bit population.
    pub fn post(&self, bit: u32) {
        let prev = self.ints.fetch_or(bit, Ordering::Relaxed);
        if prev & bit == 0 {
            PENDING_GLOBAL.fetch_add(1, Ordering::Relaxed);
        }
        self.wake();
    }

    /// Consume `bit`: `true` exactly when it was set (and is now cleared,
    /// with the global counter decremented to match).
    pub fn take(&self, bit: u32) -> bool {
        let prev = self.ints.fetch_and(!bit, Ordering::Relaxed);
        let had = prev & bit != 0;
        if had {
            PENDING_GLOBAL.fetch_sub(1, Ordering::Relaxed);
        }
        had
    }

    /// Whether `bit` is currently set (without consuming it).
    pub fn pending(&self, bit: u32) -> bool {
        self.ints.load(Ordering::Relaxed) & bit != 0
    }

    /// Wake a sleeper immediately (`Thread#run`, or any `post`).
    pub fn wake(&self) {
        let mut woke = self.sleep_mx.lock();
        *woke = true;
        self.sleep_cv.notify_all();
    }

    /// Interruptible sleep: parks until [`ThreadCtx::wake`] (true) or the
    /// deadline (false); `None` sleeps forever -- which is exactly what
    /// makes `sleep` with no argument finally interruptible instead of a
    /// panic. Consumes the wake flag.
    pub fn sleep(&self, dur: Option<Duration>) -> bool {
        let deadline = dur.map(|d| Instant::now() + d);
        let mut woke = self.sleep_mx.lock();
        loop {
            if *woke {
                *woke = false;
                return true;
            }
            match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d || self.sleep_cv.wait_until(&mut woke, d).timed_out() {
                        // A wake that raced the timeout still counts as a wake.
                        let was_woken = *woke;
                        *woke = false;
                        return was_woken;
                    }
                }
                None => self.sleep_cv.wait(&mut woke),
            }
        }
    }
}

/// The per-Ractor scheduling lock. Armed (`ZEO_GVL=1`): a ticket-FIFO
/// mutual-exclusion handoff -- one runner at a time, waiters served in
/// arrival order (CRuby's own GVL queue discipline). Disabled (the
/// default): every scheduling operation is a no-op and threads run in
/// parallel; only the member registry (which feeds the timer and, later,
/// `Thread.list`) does anything.
pub struct Gvl {
    armed: bool,
    state: Mutex<GvlState>,
    cv: Condvar,
    /// The Ruby threads attached to this Gvl. The timer arms only when at
    /// least two are alive (a lone thread has nobody to be fair to).
    members: Mutex<Vec<Weak<ThreadCtx>>>,
}

struct GvlState {
    /// Handed to each arriving acquirer, in order.
    next_ticket: u64,
    /// The ticket currently allowed to run; `release` advances it.
    now_serving: u64,
    /// The OS thread currently holding (armed mode) -- what lets
    /// `release`/`without`/`yield_now` be safe no-ops on a thread that
    /// never acquired (a Ractor's own thread, a foreign callback): a
    /// non-holder advancing `now_serving` would corrupt the ticket queue
    /// for everyone (found as a live deadlock in the armed-mode gate).
    holder: Option<std::thread::ThreadId>,
}

impl Gvl {
    fn with_armed(armed: bool) -> Arc<Gvl> {
        Arc::new(Gvl {
            armed,
            state: Mutex::new(GvlState {
                next_ticket: 0,
                now_serving: 0,
                holder: None,
            }),
            cv: Condvar::new(),
            members: Mutex::new(Vec::new()),
        })
    }

    /// The armed, CRuby-fidelity handoff.
    pub fn armed() -> Arc<Gvl> {
        Gvl::with_armed(true)
    }

    /// The parallel default: no mutual exclusion, interrupts still flow.
    pub fn disabled() -> Arc<Gvl> {
        Gvl::with_armed(false)
    }

    /// The mode `ZEO_GVL` selects: `1`/`true` arms the handoff, anything
    /// else (including unset) is the parallel default.
    pub fn from_env() -> Arc<Gvl> {
        let armed = matches!(
            std::env::var("ZEO_GVL").as_deref(),
            Ok("1") | Ok("true") | Ok("on")
        );
        Gvl::with_armed(armed)
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Register a Ruby thread with this Gvl (spawn-time). Also prunes
    /// entries whose thread has exited.
    pub fn attach(&self, ctx: &Arc<ThreadCtx>) {
        let mut m = self.members.lock();
        m.retain(|w| w.strong_count() > 0);
        m.push(Arc::downgrade(ctx));
    }

    /// Post [`INT_GC`] to every attached thread but the caller, answering how
    /// many were asked. A thread whose ctx has been dropped is gone and is not
    /// counted.
    pub fn post_gc_to_others(&self) -> usize {
        let me = current_ctx();
        let m = self.members.lock();
        let mut asked = 0;
        for w in m.iter() {
            let Some(ctx) = w.upgrade() else {
                continue;
            };
            if me.as_ref().is_some_and(|mine| Arc::ptr_eq(mine, &ctx)) {
                continue;
            }
            ctx.post(INT_GC);
            asked += 1;
        }
        asked
    }

    /// The number of live attached threads -- the timer's arming predicate.
    pub fn live_members(&self) -> usize {
        self.members
            .lock()
            .iter()
            .filter(|w| w.strong_count() > 0)
            .count()
    }

    /// Whether the CALLING thread is the current holder (armed mode).
    fn holds(&self) -> bool {
        self.armed && self.state.lock().holder == Some(std::thread::current().id())
    }

    /// Block until it is this thread's turn to run (FIFO). No-op unarmed.
    pub fn acquire(&self) {
        if !self.armed {
            return;
        }
        let mut s = self.state.lock();
        let ticket = s.next_ticket;
        s.next_ticket += 1;
        while s.now_serving != ticket {
            self.cv.wait(&mut s);
        }
        s.holder = Some(std::thread::current().id());
    }

    /// Hand the lock to the next waiter in arrival order. No-op unarmed,
    /// and a SAFE no-op on a thread that isn't the holder (a Ractor's own
    /// thread reaching a shared blocking primitive) -- only the holder may
    /// advance the queue.
    pub fn release(&self) {
        if !self.armed {
            return;
        }
        let mut s = self.state.lock();
        if s.holder != Some(std::thread::current().id()) {
            return;
        }
        s.holder = None;
        s.now_serving += 1;
        drop(s);
        self.cv.notify_all();
    }

    /// Give up the current quantum and rejoin the back of the queue -- the
    /// timer's preemption action and `Thread.pass`. No-op unarmed or when
    /// the caller isn't the holder.
    pub fn yield_now(&self) {
        if !self.holds() {
            return;
        }
        self.release();
        self.acquire();
    }

    /// Run `f` with the lock released, re-acquiring afterwards EVEN IF `f`
    /// panics or unwinds (the guard re-acquires in `Drop`) -- the wrapper
    /// every blocking wait gets so an armed holder's block can't stall its
    /// siblings. Plain call-through when unarmed or when the caller never
    /// held (it has nothing to release and must NOT re-acquire).
    pub fn without<R>(&self, f: impl FnOnce() -> R) -> R {
        if !self.holds() {
            return f();
        }
        self.release();
        let _guard = ReacquireGuard { gvl: self };
        f()
    }

    /// One timer quantum: with two or more live threads in armed mode,
    /// post [`INT_TIMER`] to every member -- the holder notices at its next
    /// `check_ints` and yields; everyone else consumes it harmlessly. A
    /// lone thread (or the parallel mode) gets no tick at all.
    pub fn timer_tick(&self) {
        if !self.armed || self.live_members() < 2 {
            return;
        }
        for w in self.members.lock().iter() {
            if let Some(ctx) = w.upgrade() {
                ctx.post(INT_TIMER);
            }
        }
    }
}

struct ReacquireGuard<'a> {
    gvl: &'a Gvl,
}

impl Drop for ReacquireGuard<'_> {
    fn drop(&mut self) {
        self.gvl.acquire();
    }
}

/// Holds the lock from construction to drop -- the panic-safe way a thread
/// body (or `run_main`) keeps the armed GVL for its whole execution: an
/// unwinding panic still releases, so siblings aren't stranded.
pub struct HoldGuard<'a> {
    gvl: &'a Gvl,
}

impl Gvl {
    pub fn hold(&self) -> HoldGuard<'_> {
        self.acquire();
        HoldGuard { gvl: self }
    }

    /// Hold for a FOREIGN re-entry -- an FFI callback trampolining into
    /// ruby. Acquires only when armed AND this thread is not already the
    /// holder: a callback invoked synchronously by an ordinary C call
    /// arrives with the GVL still held (acquiring again would self-deadlock
    /// on the ticket queue), while one reached under `blocking: true` -- or
    /// from a thread C spawned itself -- arrives without it. The guard
    /// releases only what it took.
    pub fn hold_reentrant(&self) -> Option<HoldGuard<'_>> {
        if !self.armed || self.holds() {
            return None;
        }
        Some(self.hold())
    }
}

impl Drop for HoldGuard<'_> {
    fn drop(&mut self) {
        self.gvl.release();
    }
}

// ---------------------------------------------------------------------------
// The process scheduler surface the OS-thread execution mode uses: one
// process-wide Gvl (per-Ractor Gvls arrive with the Ractor rework -- the
// existing Ractor threads simply never touch this one), a per-OS-thread
// ThreadCtx slot, and the lazy 100ms preemption timer.
// ---------------------------------------------------------------------------

static PROCESS_GVL: std::sync::OnceLock<Arc<Gvl>> = std::sync::OnceLock::new();

/// The main Ractor's Gvl -- `ZEO_GVL=1` arms it, anything else (the
/// default) is the parallel no-op. First access decides; safe to call from
/// any mode (a disabled Gvl's operations are free no-ops).
pub fn process_gvl() -> &'static Arc<Gvl> {
    PROCESS_GVL.get_or_init(Gvl::from_env)
}

/// A C extension has loaded: from here the process runs under CRuby's rules.
///
/// Two things change, and the second is the one that cannot be skipped.
///
/// The Gvl is ARMED, so two Ruby threads no longer run at once -- an
/// extension's `Init_` may start a thread, and its C code holds Ruby objects
/// in locals no other thread's view accounts for. This only takes effect when
/// nothing has read [`process_gvl`] yet: the mode is decided once, on first
/// access, and the `Arc` cannot be swapped under a thread already holding it.
/// A program that had already touched it keeps the mode it chose, which is
/// reported rather than silently ignored.
///
/// The sole-thread claim is CLEARED unconditionally. That is the lock-free
/// fast path over containers, and its premise -- exactly one Ruby thread, and
/// it is this one -- is exactly what an extension can break without telling
/// anyone. Clearing it costs a lock per container access and is never wrong.
///
/// Answers whether the Gvl actually armed.
pub fn arm_for_cext() -> bool {
    clear_sole_thread();
    MULTI_THREADED.store(true, Ordering::Release);
    // A `false` here is not a failure: the program asked for the parallel
    // default and got it, and the fast path -- the part an extension can
    // actually corrupt -- is off either way. The caller decides what to say.
    PROCESS_GVL.get_or_init(Gvl::armed).is_armed()
}

/// Run `f` with the process Gvl released -- the wrapper every potentially
/// blocking syscall site uses so an armed (`ZEO_GVL=1`) holder can't stall
/// its siblings behind a read/accept/child-wait. A call-through when the
/// Gvl is disabled or this thread isn't the holder. Wrap the WHOLE
/// lock-op-unlock section of the blocking primitive (never re-acquire the
/// Gvl while still holding the primitive's own lock -- lock-order
/// inversion; see `thread::queue_push_locked`).
///
/// It is also where the calling Ruby thread is marked ASLEEP, which is what
/// `Thread#status` reports. That has to happen here rather than in
/// [`Gvl::without`]: `without` returns `f()` immediately when this thread
/// does not hold the Gvl, which is always true in the default parallel mode.
pub fn without_gvl<R>(f: impl FnOnce() -> R) -> R {
    let _blocked = crate::thread::block_guard();
    process_gvl().without(f)
}

std::thread_local! {
    static CTX: std::cell::RefCell<Option<Arc<ThreadCtx>>> =
        const { std::cell::RefCell::new(None) };
}

/// Create this OS thread's [`ThreadCtx`], attach it to the process Gvl,
/// and remember it in TLS. Called once per Ruby thread (main included) in
/// the OS-thread execution mode; repeat calls answer the existing ctx.
pub fn install_ctx() -> Arc<ThreadCtx> {
    CTX.with(|c| {
        let mut slot = c.borrow_mut();
        if let Some(ctx) = &*slot {
            return ctx.clone();
        }
        let ctx = ThreadCtx::new();
        let gvl = process_gvl();
        gvl.attach(&ctx);
        // The preemption timer becomes worth running once a SECOND thread
        // exists (armed mode only -- `timer_tick` no-ops otherwise).
        if gvl.is_armed() && gvl.live_members() >= 2 {
            ensure_timer_thread();
        }
        *slot = Some(ctx.clone());
        ctx
    })
}

/// This OS thread's ctx, if `install_ctx` ran here (`None` under the
/// coroutine mode, where no ctx exists at all).
pub fn current_ctx() -> Option<Arc<ThreadCtx>> {
    CTX.with(|c| c.borrow().clone())
}

/// Deadlock detection: nobody left who could make progress.
///
/// CRuby raises `fatal: No live threads left. Deadlock?` when the thread about
/// to sleep would leave no runnable thread behind. zeo asks the same question
/// from the other end, because its blocking waits already poll: a wait that
/// only another RUBY thread can end registers itself here, and when every live
/// thread is registered, none of them can be the one to end another's wait.
///
/// Only untimed waits count. `sleep 2` ends by itself, so a thread inside one
/// is not blocked on anybody -- CRuby draws the same line.
///
/// The verdict needs the condition to hold across two consecutive polls. A
/// thread between two waits is briefly absent from the count without being
/// runnable in any useful sense, and one poll would call that a deadlock.
pub mod deadlock {
    use super::{AtomicU32, Ordering};

    /// Threads parked in a wait only another Ruby thread can end.
    static BLOCKED: AtomicU32 = AtomicU32::new(0);
    /// How many consecutive polls have seen every thread blocked.
    static STREAK: AtomicU32 = AtomicU32::new(0);

    /// Registers a blocking wait for as long as it lives. A wait on a thread
    /// ruby does not know about registers nothing -- see
    /// [`crate::thread::is_ruby_thread`].
    pub struct Waiting {
        counted: bool,
    }

    impl Waiting {
        /// Enter a wait only another Ruby thread can end.
        #[must_use]
        pub fn enter() -> Waiting {
            let counted = crate::thread::is_ruby_thread();
            if counted {
                BLOCKED.fetch_add(1, Ordering::SeqCst);
            }
            Waiting { counted }
        }
    }

    impl Drop for Waiting {
        fn drop(&mut self) {
            if self.counted {
                BLOCKED.fetch_sub(1, Ordering::SeqCst);
            }
            STREAK.store(0, Ordering::SeqCst);
        }
    }

    /// A wake that can end another thread's wait -- every queue push, queue
    /// pop, mutex unlock and ractor send calls this beside its `notify`.
    ///
    /// A woken thread is still PARKED for as long as the scheduler takes to
    /// run it, and it stays in `BLOCKED` that whole time. Counting it as
    /// blocked is how a BUSY MACHINE turned a working program into a
    /// deadlock verdict: `SizedQueue(1)` with a producer and a consumer
    /// reads as "both blocked" in the gap between the pop that frees a slot
    /// and the producer actually running again.
    ///
    /// So a wake resets the streak. Under a real deadlock nobody wakes
    /// anybody, the streak builds, and the verdict still lands in
    /// milliseconds.
    pub fn note_progress() {
        STREAK.store(0, Ordering::SeqCst);
    }

    /// Called once per poll from inside a wait. `true` means nothing in this
    /// process can make progress.
    ///
    /// The denominator is the live RUBY thread count, not the number of
    /// threads that installed a scheduling context. A context is installed
    /// lazily -- the first time a thread sleeps or waits -- so a runnable
    /// thread that has never waited is invisible to `live_members`, and using
    /// that count declares a deadlock while the thread about to push is still
    /// running. minitest's parallel executor does exactly that on every run.
    pub fn no_progress_possible() -> bool {
        if !crate::thread::is_ruby_thread() {
            return false;
        }
        let live = crate::thread::live_thread_count().max(1) as u32;
        if BLOCKED.load(Ordering::SeqCst) < live {
            STREAK.store(0, Ordering::SeqCst);
            return false;
        }
        STREAK.fetch_add(1, Ordering::SeqCst) >= STREAK_VERDICT
    }

    /// Consecutive all-blocked polls with no wake in between before the
    /// verdict lands -- `STREAK_VERDICT * SLICE`, so about 16ms.
    ///
    /// One poll was enough to be wrong, and two were enough to be wrong on a
    /// loaded machine: a thread woken by another's push can sit unscheduled
    /// for several milliseconds while sixteen test jobs run. A deadlock has
    /// nobody to wake it ever, so waiting longer costs a real verdict
    /// nothing and costs a false one everything.
    const STREAK_VERDICT: u32 = 8;

    /// How long one supervised park lasts before the verdict is re-polled.
    /// Short enough that a deadlock is reported promptly, long enough that a
    /// contended wait is not a spin.
    const SLICE: std::time::Duration = std::time::Duration::from_millis(2);

    /// The supervised half of a blocking wait: registration, plus the
    /// verdict.
    ///
    /// Construct it once you know you are about to block, then park through
    /// [`SupervisedWait::slice`] instead of an untimed `wait`. Registration
    /// becomes a property of the PRIMITIVE rather than something each site
    /// remembers -- which is the whole point. One of six blocking waits
    /// registered; the other five hung forever where CRuby raises, and the
    /// sixth was the only one anybody had noticed.
    ///
    /// Deliberately NOT constructed on an uncontended fast path: a
    /// `Mutex#lock` that takes a free lock must not pay an atomic for a wait
    /// it never enters.
    pub struct SupervisedWait {
        _waiting: Waiting,
    }

    impl SupervisedWait {
        #[must_use]
        pub fn enter() -> SupervisedWait {
            SupervisedWait {
                _waiting: Waiting::enter(),
            }
        }

        /// Parks on `cv` for one slice, then answers whether the program can
        /// still make progress. `false` means every live Ruby thread is
        /// blocked on another and none can be the one to wake it -- CRuby's
        /// `fatal`, not a hang.
        pub fn slice<T>(
            &self,
            cv: &parking_lot::Condvar,
            guard: &mut parking_lot::MutexGuard<'_, T>,
        ) -> bool {
            let _ = cv.wait_for(guard, SLICE);
            !no_progress_possible()
        }
    }

    /// The `fatal` CRuby raises. Its message is the FIRST LINE of CRuby's,
    /// which is the part that describes the program rather than the VM: the
    /// rest is a thread dump of native addresses and `rb_thread_t` pointers
    /// that no other implementation can produce and no program can act on.
    #[must_use]
    pub fn signal() -> crate::Signal {
        crate::dispatch::raise_error("fatal", "No live threads left. Deadlock?".to_string())
    }
}

/// The stop-the-world rendezvous a cycle collection needs.
///
/// Reference-count reconciliation reads every node's owner count and compares
/// it against the edges it can enumerate. A thread mutating the heap while
/// that runs would make the two disagree, and the pass would read a live node
/// as garbage -- the one direction that can corrupt. So every other Ruby
/// thread parks first.
///
/// Parking happens ONLY at a `check_ints` checkpoint, never at an allocation
/// site. An allocation can happen inside a container's own guard -- growing a
/// Hash while its lock is held -- and stopping the world there would hand the
/// collector a locked node it must read. The checkpoint sites hold no guard by
/// construction.
///
/// Abandoning is always safe and is the answer to every thread that will not
/// come. One parked in a blocking syscall through [`without_gvl`] never
/// reaches a checkpoint; rather than reason about what it might be holding,
/// the request expires and the collection does not run. That leaks exactly
/// what leaks today.
mod rendezvous {
    use super::{Condvar, INT_GC, Mutex, Ordering, process_gvl};
    use std::time::{Duration, Instant};

    /// How long a request waits for the world to stop. Generous next to a
    /// checkpoint's spacing (every loop body and every method prologue) and
    /// short next to a person noticing a pause.
    const DEADLINE: Duration = Duration::from_millis(250);

    struct World {
        /// Set for the length of one stop. A thread that reaches a
        /// checkpoint while it is set parks, which is what stops one woken
        /// by an unrelated interrupt from running through the middle of a
        /// collection.
        stopped: bool,
        /// Which stop this is. A straggler from an ABANDONED request wakes
        /// up in a later generation and leaves without touching its count --
        /// the alternative, decrementing on the way out, underflows exactly
        /// when a request has already given up on it.
        generation: u64,
        /// How many threads have parked in `generation`. Reset by the next
        /// stop rather than by the threads leaving this one.
        parked: usize,
    }

    static WORLD: Mutex<World> = Mutex::new(World {
        stopped: false,
        generation: 0,
        parked: 0,
    });
    static CV: Condvar = Condvar::new();
    /// One collection at a time, whichever thread asked.
    static COLLECTING: Mutex<()> = Mutex::new(());

    /// Hold the world stopped for as long as this lives.
    pub(crate) struct Stopped(#[allow(dead_code)] parking_lot::MutexGuard<'static, ()>);

    impl Drop for Stopped {
        fn drop(&mut self) {
            let mut w = WORLD.lock();
            w.stopped = false;
            CV.notify_all();
        }
    }

    /// Stop every other Ruby thread, or answer `None` if they do not all
    /// arrive in time.
    pub(crate) fn stop_the_world() -> Option<Stopped> {
        let guard = COLLECTING.try_lock()?;
        let generation = {
            let mut w = WORLD.lock();
            w.generation += 1;
            w.parked = 0;
            w.stopped = true;
            w.generation
        };
        // Built BEFORE anything can fail, so every exit below clears the flag
        // and releases whoever did park.
        let held = Stopped(guard);
        // The flag is set first: a thread that reaches its checkpoint between
        // the post and the flag would see nothing to park for, and the wait
        // below would then time out for no reason.
        let others = process_gvl().post_gc_to_others();
        if others == 0 {
            return Some(held);
        }
        let deadline = Instant::now() + DEADLINE;
        let mut w = WORLD.lock();
        while w.parked < others {
            if CV.wait_until(&mut w, deadline).timed_out() {
                // Release the lock before `held` drops -- its own `Drop`
                // takes it.
                drop(w);
                return None;
            }
        }
        debug_assert_eq!(w.generation, generation);
        drop(w);
        Some(held)
    }

    /// A checkpoint on a thread the collector asked to stop: park until the
    /// collection finishes. Answers whether this thread parked at all.
    pub(crate) fn park_if_asked() -> bool {
        let Some(ctx) = super::current_ctx() else {
            return false;
        };
        if !ctx.take(INT_GC) {
            return false;
        }
        // The collector is about to read every node's owner count, so a node
        // this thread has LOCKED would be read under that guard. Checkpoint
        // sites hold none by construction; the whole golden corpus is what
        // tests that claim.
        #[cfg(debug_assertions)]
        crate::collections::debug_assert_no_live_fast_guards("a GC safepoint");
        let mut w = WORLD.lock();
        if !w.stopped {
            // The request was abandoned before this thread noticed it.
            return false;
        }
        let mine = w.generation;
        w.parked += 1;
        CV.notify_all();
        while w.stopped && w.generation == mine {
            CV.wait(&mut w);
        }
        true
    }

    /// Whether a collection is running right now, for a test that wants to
    /// know without parking.
    #[cfg(test)]
    pub(crate) fn world_is_stopped() -> bool {
        WORLD.lock().stopped
    }

    /// Silence the unused-import warning in builds where nothing reads it.
    const _: Ordering = Ordering::Relaxed;
}

pub(crate) use rendezvous::{park_if_asked, stop_the_world};

/// Consume a pending quantum tick: under an ARMED Gvl the holder rejoins
/// the back of the queue (CRuby's preemption action); the bit is consumed
/// harmlessly everywhere else. Called from `check_ints`' slow path.
pub fn service_timer() {
    if let Some(ctx) = current_ctx()
        && ctx.take(INT_TIMER)
    {
        process_gvl().yield_now();
    }
}

/// The one lazy, detached 100ms timer thread (CRuby's quantum). It only
/// ever posts bits -- delivery happens at the members' own checkpoints --
/// and it dies with the process (detached daemon, like CRuby's own timer).
fn ensure_timer_thread() {
    static TIMER: std::sync::Once = std::sync::Once::new();
    TIMER.call_once(|| {
        std::thread::Builder::new()
            .name("zeo-timer".into())
            .spawn(|| {
                loop {
                    std::thread::sleep(Duration::from_millis(100));
                    process_gvl().timer_tick();
                }
            })
            .expect("spawning the GVL timer thread");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The armed handoff serves waiters strictly in arrival order.
    #[test]
    fn armed_handoff_is_fifo_by_arrival() {
        let gvl = Gvl::armed();
        gvl.acquire();
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut waiters = Vec::new();
        for i in 0..3 {
            let (g, o) = (gvl.clone(), order.clone());
            waiters.push(std::thread::spawn(move || {
                g.acquire();
                o.lock().push(i);
                g.release();
            }));
            // Stagger arrivals so ticket order == spawn order.
            std::thread::sleep(Duration::from_millis(20));
        }
        gvl.release();
        for w in waiters {
            w.join().unwrap();
        }
        assert_eq!(*order.lock(), vec![0, 1, 2]);
    }

    /// The parallel default never excludes: two threads sit INSIDE the
    /// "critical section" simultaneously (this rendezvous would deadlock
    /// under an armed Gvl).
    #[test]
    fn disabled_gvl_runs_threads_in_parallel() {
        let gvl = Gvl::disabled();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let mut both = Vec::new();
        for _ in 0..2 {
            let (g, b) = (gvl.clone(), barrier.clone());
            both.push(std::thread::spawn(move || {
                g.acquire();
                // Only reachable together if both hold "the lock" at once.
                b.wait();
                g.release();
            }));
        }
        for t in both {
            t.join().unwrap();
        }
    }

    /// `without` re-acquires even when the body panics, leaving the
    /// handoff consistent (a later waiter still gets served).
    #[test]
    fn without_gvl_reacquires_on_panic() {
        let gvl = Gvl::armed();
        gvl.acquire();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            gvl.without(|| panic!("syscall body blew up"))
        }));
        assert!(r.is_err());
        // Still the holder after the unwind: release hands off cleanly.
        gvl.release();
        let g = gvl.clone();
        std::thread::spawn(move || {
            g.acquire();
            g.release();
        })
        .join()
        .unwrap();
    }

    /// The rendezvous stops a running thread and lets it go again. Worth a
    /// unit test rather than only a golden: this is the one place a bug is a
    /// hang, and `cargo miri test` runs it for the data races a golden
    /// cannot see.
    #[test]
    fn the_world_stops_and_starts_again() {
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        // The worker is ATTACHED before this barrier releases, so the request
        // below always has someone to wait for.
        let entered = Arc::new(std::sync::Barrier::new(2));
        let worker = {
            let (running, entered) = (running.clone(), entered.clone());
            std::thread::spawn(move || {
                install_ctx();
                entered.wait();
                // A checkpoint loop, which is what a compiled body is.
                while running.load(Ordering::Relaxed) {
                    park_if_asked();
                    std::thread::yield_now();
                }
            })
        };
        install_ctx();
        entered.wait();

        let held = stop_the_world().expect("a thread at a checkpoint parks");
        assert!(rendezvous::world_is_stopped());
        drop(held);

        // A second stop must work too: the generation counter is what stops a
        // straggler from the first one being counted in the second.
        let held = stop_the_world().expect("the rendezvous is reusable");
        drop(held);

        running.store(false, Ordering::Relaxed);
        worker.join().unwrap();
    }

    /// A thread that never reaches a checkpoint costs a bounded pause and a
    /// missed collection, never a hang.
    #[test]
    fn a_thread_that_never_parks_is_abandoned() {
        // Two barriers, and the first one is the test: without it the worker
        // may not have ATTACHED yet, the request finds nobody to wait for,
        // and the abandon path is never exercised at all.
        let attached = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker = {
            let (attached, release) = (attached.clone(), release.clone());
            std::thread::spawn(move || {
                install_ctx();
                attached.wait();
                release.wait();
            })
        };
        attached.wait();
        // The worker is attached and will not poll, so the request expires.
        assert!(
            stop_the_world().is_none(),
            "a thread that never checkpoints must not be waited on forever"
        );
        assert!(
            !rendezvous::world_is_stopped(),
            "an abandoned request leaves nothing stopped"
        );
        release.wait();
        worker.join().unwrap();
    }

    /// Bits post/consume exactly once each, and the global fast-path
    /// counter tracks the set-bit population (double-post included).
    #[test]
    fn interrupt_bits_round_trip_through_the_global_counter() {
        let before = PENDING_GLOBAL.load(Ordering::Relaxed);
        let ctx = ThreadCtx::new();
        ctx.post(INT_PENDING);
        ctx.post(INT_PENDING); // dedup: same bit, no double count
        ctx.post(INT_TIMER);
        assert_eq!(PENDING_GLOBAL.load(Ordering::Relaxed), before + 2);
        assert!(interrupts_pending_anywhere());
        assert!(ctx.pending(INT_PENDING));
        assert!(ctx.take(INT_PENDING));
        assert!(!ctx.take(INT_PENDING)); // already consumed
        assert!(ctx.take(INT_TIMER));
        assert_eq!(PENDING_GLOBAL.load(Ordering::Relaxed), before);
    }

    /// A poster wakes a parked sleeper immediately -- the raise-into-sleep
    /// shape (`Thread#raise` against a `sleep 5` target must not wait out
    /// the 5 seconds).
    #[test]
    fn posting_wakes_an_interruptible_sleeper_early() {
        let ctx = ThreadCtx::new();
        let c2 = ctx.clone();
        let start = Instant::now();
        let sleeper = std::thread::spawn(move || c2.sleep(Some(Duration::from_secs(5))));
        std::thread::sleep(Duration::from_millis(20));
        ctx.post(INT_PENDING);
        assert!(sleeper.join().unwrap(), "woken, not timed out");
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(ctx.take(INT_PENDING));
    }

    /// A wake posted BEFORE the sleep starts is not lost, and an unwoken
    /// short sleep reports its timeout.
    #[test]
    fn sleep_consumes_a_prior_wake_and_times_out_otherwise() {
        let ctx = ThreadCtx::new();
        ctx.wake();
        assert!(ctx.sleep(Some(Duration::from_secs(5)))); // immediate
        assert!(!ctx.sleep(Some(Duration::from_millis(10)))); // timeout
    }

    /// The timer arms only when a Gvl has two or more LIVE threads, and
    /// never in the parallel mode.
    #[test]
    fn timer_ticks_only_with_two_live_members_and_only_armed() {
        let gvl = Gvl::armed();
        let a = ThreadCtx::new();
        gvl.attach(&a);
        gvl.timer_tick();
        assert!(!a.pending(INT_TIMER), "a lone thread gets no quantum tick");
        let b = ThreadCtx::new();
        gvl.attach(&b);
        gvl.timer_tick();
        assert!(a.take(INT_TIMER));
        assert!(b.take(INT_TIMER));
        // A dead member disarms it again.
        drop(b);
        gvl.timer_tick();
        assert!(!a.pending(INT_TIMER));

        let parallel = Gvl::disabled();
        let (c, d) = (ThreadCtx::new(), ThreadCtx::new());
        parallel.attach(&c);
        parallel.attach(&d);
        parallel.timer_tick();
        assert!(!c.pending(INT_TIMER) && !d.pending(INT_TIMER));
    }

    /// The sole-thread ivar path is only sound while every RUBY thread spawn
    /// is preceded by `note_thread_spawn`. That is a two-line discipline no
    /// type can enforce, so it is checked against the source.
    ///
    /// The runtime's other spawn is the GVL preemption timer, whose body reads
    /// a clock and posts a bit -- it touches no Ruby object and so needs no
    /// mark. Anything NEW that spawns has to be classified here deliberately.
    #[test]
    fn every_ruby_thread_spawn_is_marked() {
        let mut unmarked = Vec::new();
        for (file, src) in [
            ("thread.rs", include_str!("thread.rs")),
            ("ractor.rs", include_str!("ractor.rs")),
            ("gvl.rs", include_str!("gvl.rs")),
            ("fiber.rs", include_str!("fiber.rs")),
            ("coroutine.rs", include_str!("coroutine.rs")),
        ] {
            let lines: Vec<&str> = src.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if !line.contains("thread::spawn(") && !line.contains("thread::Builder::new()") {
                    continue;
                }
                // Test-only spawns live under a `#[cfg(test)] mod tests`, which
                // is always the tail of these files.
                if src[..src.find(line).unwrap_or(0)].contains("mod tests {") {
                    continue;
                }
                let window = lines[i.saturating_sub(6)..i].join("\n");
                if window.contains("note_thread_spawn()") {
                    continue;
                }
                // The preemption timer: classified, not forgotten.
                if window.contains("zeo-timer")
                    || lines[i..(i + 4).min(lines.len())]
                        .join("\n")
                        .contains("zeo-timer")
                {
                    continue;
                }
                unmarked.push(format!("{file}:{}", i + 1));
            }
        }
        assert!(
            unmarked.is_empty(),
            "these spawn a thread without `gvl::note_thread_spawn()` first: {unmarked:?}. \
             If the new thread cannot reach a Ruby object, say so here; otherwise mark it."
        );
    }
}
