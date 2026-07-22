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
//! Nothing calls this module yet -- it lands inert ahead of the
//! `check_ints` emission and the OS-thread execution mode, so its
//! contracts are pinned by the unit tests below before any generated code
//! depends on them.

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

/// The fast-path read: nonzero means SOME thread (possibly not the caller)
/// has an undelivered interrupt and the caller should run the slow path.
#[inline]
pub fn interrupts_pending_anywhere() -> bool {
    PENDING_GLOBAL.load(Ordering::Relaxed) != 0
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
}

impl Gvl {
    fn with_armed(armed: bool) -> Arc<Gvl> {
        Arc::new(Gvl {
            armed,
            state: Mutex::new(GvlState {
                next_ticket: 0,
                now_serving: 0,
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
    }

    /// Hand the lock to the next waiter in arrival order. No-op unarmed.
    /// Internal contract: called only by the current holder.
    pub fn release(&self) {
        if !self.armed {
            return;
        }
        let mut s = self.state.lock();
        s.now_serving += 1;
        drop(s);
        self.cv.notify_all();
    }

    /// Give up the current quantum and rejoin the back of the queue -- the
    /// timer's preemption action and `Thread.pass`. No-op unarmed.
    pub fn yield_now(&self) {
        if !self.armed {
            return;
        }
        self.release();
        self.acquire();
    }

    /// Run `f` with the lock released, re-acquiring afterwards EVEN IF `f`
    /// panics or unwinds (the guard re-acquires in `Drop`) -- the wrapper
    /// every blocking syscall gets in armed mode so one thread's IO can't
    /// stall its siblings. No-op wrapping unarmed.
    pub fn without<R>(&self, f: impl FnOnce() -> R) -> R {
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
}
