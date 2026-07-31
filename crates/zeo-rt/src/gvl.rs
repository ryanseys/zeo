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
static PENDING_GLOBAL: AtomicU32 = AtomicU32::new(0);

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
    SOLE.with(|s| s.set(!MULTI_THREADED.load(Ordering::Acquire)));
}

/// Whether the caller may take a lock-free path over data only Ruby threads
/// reach. One thread-local byte; LLVM must reload it after any call it cannot
/// see through, which is exactly the condition under which the answer could
/// have changed.
#[inline(always)]
pub fn sole_thread() -> bool {
    SOLE.with(|s| s.get())
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
    SOLE.with(|s| s.set(false));
    MULTI_THREADED.store(true, Ordering::Release);
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

/// Run `f` with the process Gvl released -- the wrapper every potentially
/// blocking syscall site uses so an armed (`ZEO_GVL=1`) holder can't stall
/// its siblings behind a read/accept/child-wait. A call-through when the
/// Gvl is disabled or this thread isn't the holder. Wrap the WHOLE
/// lock-op-unlock section of the blocking primitive (never re-acquire the
/// Gvl while still holding the primitive's own lock -- lock-order
/// inversion; see `thread::queue_push_locked`).
pub fn without_gvl<R>(f: impl FnOnce() -> R) -> R {
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
                if window.contains("zeo-timer") || lines[i..(i + 4).min(lines.len())].join("\n").contains("zeo-timer") {
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
