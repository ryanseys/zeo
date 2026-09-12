//! `Ractor` -- real OS threads (`std::thread::spawn`; a Ractor never
//! attaches to the process Gvl, so even the `ZEO_GVL=1` fidelity mode
//! keeps Ractors genuinely parallel) sharing the SAME global heap.
//! CRuby's real Ractors already share classes, methods, and the Symbol
//! table process-wide -- and this runtime's class registry/Symbol interner
//! are already global (`OnceLock`/`LazyLock`) -- so no separate namespace
//! or serialization subsystem exists or is needed. Memory safety is
//! unconditional (`Arc`/`parking_lot::Mutex` everywhere); what Ractor adds
//! is the Ruby-SEMANTIC isolation discipline, enforced at the boundaries.
//!
//! This is ruby 4.0's PORT MODEL: every Ractor owns a table of ports (the
//! default port pre-created as id 0, more via `Ractor::Port.new` on its own
//! thread). ANY ractor may `#send` to a port it holds; only the CREATOR may
//! `#receive`/`#close`. `Ractor#send` is default-port send; `Ractor.receive`
//! is default-port receive -- the main Ractor's included, so main can block
//! in `Ractor.receive` and be fed by workers. `#join`/`#value` ride the same
//! machinery through `#monitor`: a fresh port in the CALLER's table gets the
//! `:exited`/`:aborted` token at the target's termination, and the value-take
//! is successor-gated (first taker wins; an aborted ractor relays its
//! exception as `Ractor::RemoteError` with `#cause` set).
//!
//! Locking protocol (deadlock-free): no path ever holds two ractors' locks.
//! A send locks only the port CREATOR's table; a receive locks only the
//! CALLER's own (creator == caller, guarded); termination drains its own
//! table, then sends monitor tokens holding NOTHING.
//!
//! Documented divergences (all narrow, all loud rather than silently
//! wrong): globals/cvars stay process-shared (CRuby raises IsolationError
//! on non-main-Ractor access; here they genuinely share -- flagged for the
//! `Ruby::Box` work, which owns namespace isolation); a receive that can
//! never be fed blocks forever instead of CRuby's deadlock detection; a
//! ractor that aborts does not print CRuby's `#<Thread:0x...> terminated with
//! exception` report on stderr. Block isolation is CRuby's own: an outer-local
//! capture is [`ractor_new`]'s `ArgumentError`, and an ivar access raises
//! [`ivar_isolation_check`]'s `Ractor::IsolationError` inside the ractor,
//! whose `self` is the ractor. `move: true` is REAL: the graph transplants and
//! every source node is poisoned (`Ractor::MovedError` on any later send) --
//! [`cross_graph`] documents its own deliberate divergences from CRuby's
//! traversal accidents.

use crate::{FMap, RubyValue, Signal, Symbol};
use parking_lot::{Condvar, Mutex as PlMutex};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use crate::dispatch::{RObj, RubyObject, raise_error, raise_error_details};
use zeo_abi::ClassId;

// ---------------------------------------------------------------- data model

/// One port's pending messages. The entry is KEPT on close (queued items
/// stay receivable; `closed` gates new sends) and REMOVED at ractor
/// termination or internal port teardown (a missing entry reads as closed).
struct PortQueue {
    items: VecDeque<RubyValue>,
    closed: bool,
}

/// A ractor's ports, keyed by id. Id 0 is the default port, allocated
/// eagerly at construction.
struct PortTable {
    ports: FMap<u64, PortQueue>,
    next_port_id: u64,
}

/// `Ractor#status` values (also `#inspect`'s tail word).
const STATUS_CREATED: u8 = 0;
const STATUS_RUNNING: u8 = 1;
const STATUS_BLOCKING: u8 = 2;
const STATUS_TERMINATED: u8 = 3;

enum RactorLifecycle {
    Live,
    Terminated {
        result: Result<RubyValue, Signal>,
        aborted: bool,
        /// Whether `result` already crossed the boundary on a take --
        /// `#value` crosses ONCE, so a re-take (same successor) answers the
        /// SAME object: `r.value[0].equal?(r.value)` holds.
        crossed: bool,
    },
}

pub struct RactorData {
    /// Monotonic id; the main ractor is 1 (`#inspect`'s `#<Ractor:#1 ...>`).
    id: u32,
    /// The `name:` kwarg; `None` for main and unnamed ractors.
    name: Option<String>,
    /// `"file:line"` of the `Ractor.new` call; `None` for main.
    loc: Option<String>,
    ports: PlMutex<PortTable>,
    /// One condvar per ractor, paired with `ports` -- every receive/select
    /// waits on the whole table, so one pair is sufficient.
    recv_cv: Condvar,
    /// Ports (owned by OTHER ractors) to feed `:exited`/`:aborted` at
    /// termination -- `#monitor`'s registrations.
    monitors: PlMutex<Vec<Arc<RPort>>>,
    state: PlMutex<RactorLifecycle>,
    /// The ordering anchor: stored `STATUS_TERMINATED` strictly AFTER
    /// `state` becomes `Terminated`, so a `SeqCst` read of 3 guarantees the
    /// outcome is present.
    status: AtomicU8,
    /// `Ractor#[]`/`#[]=` storage.
    locals: PlMutex<FMap<Symbol, RubyValue>>,
    /// Serializes `store_if_absent`'s check-compute-store so the block runs
    /// at most once per key across the ractor's threads.
    store_lock: PlMutex<()>,
    /// The value-taker's ractor id; 0 = unclaimed (CAS'd once, same id may
    /// re-take).
    successor: AtomicU32,
    /// The default port's `RubyValue` wrapper, cached so `#default_port`
    /// answers the SAME object each call (CRuby's `.equal?` contract).
    /// Deliberately a self-referential `Arc` cycle (`RPort.creator` points
    /// back here): a ractor whose default port was materialized leaks its
    /// (small, drained-at-termination) `RactorData` -- bounded by the
    /// process's ractor count.
    default_port_cache: OnceLock<Arc<RPort>>,
    /// `.frozen?` state -- flag-only (a frozen Ractor still runs and
    /// receives; CRuby accepts `Ractor#freeze`).
    frozen: AtomicBool,
}

pub type RRactor = Arc<RactorData>;

impl RactorData {
    fn build(id: u32, name: Option<String>, loc: Option<String>) -> RactorData {
        let mut ports = FMap::default();
        ports.insert(
            0,
            PortQueue {
                items: VecDeque::new(),
                closed: false,
            },
        );
        RactorData {
            id,
            name,
            loc,
            ports: PlMutex::new(PortTable {
                ports,
                next_port_id: 1,
            }),
            recv_cv: Condvar::new(),
            monitors: PlMutex::new(Vec::new()),
            state: PlMutex::new(RactorLifecycle::Live),
            status: AtomicU8::new(STATUS_CREATED),
            locals: PlMutex::new(FMap::default()),
            store_lock: PlMutex::new(()),
            successor: AtomicU32::new(0),
            default_port_cache: OnceLock::new(),
            frozen: AtomicBool::new(false),
        }
    }

    /// `Ractor#frozen?` -- see the `frozen` field.
    pub fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    /// `Ractor#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }

    fn status_word(&self) -> &'static str {
        match self.status.load(Ordering::SeqCst) {
            STATUS_CREATED => "created",
            STATUS_RUNNING => "running",
            STATUS_BLOCKING => "blocking",
            _ => "terminated",
        }
    }
}

/// A `Ractor::Port` handle: which ractor's table, which entry. Two handles
/// naming the same `{creator, port_id}` ARE the same port (`dup` included).
/// The pair sits behind a lock because `Port#initialize`/`#initialize_copy`
/// re-seat a LIVE handle in place -- CRuby re-registers a fresh id (or
/// aliases the copied port's entry) even though ports are frozen.
pub(crate) struct RPort {
    target: PlMutex<PortRef>,
    frozen: AtomicBool,
}

/// The `{creator, port_id}` pair an `RPort` currently names.
#[derive(Clone)]
pub(crate) struct PortRef {
    creator: RRactor,
    port_id: u64,
}

impl RPort {
    fn new(creator: RRactor, port_id: u64) -> Arc<RPort> {
        Arc::new(RPort {
            target: PlMutex::new(PortRef { creator, port_id }),
            frozen: AtomicBool::new(false),
        })
    }

    /// The handle's current target, cloned out from under the lock.
    fn target(&self) -> PortRef {
        self.target.lock().clone()
    }
}

fn same_port(a: &PortRef, b: &PortRef) -> bool {
    Arc::ptr_eq(&a.creator, &b.creator) && a.port_id == b.port_id
}

impl RubyObject for RPort {
    fn class_id(&self) -> ClassId {
        zeo_abi::RACTOR_PORT_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let t = self.target();
        let d = RPort::new(t.creator, t.port_id);
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

fn port_value(p: Arc<RPort>) -> RubyValue {
    RubyValue::Object(p)
}

fn as_port(v: &RubyValue) -> Option<Arc<RPort>> {
    match v {
        RubyValue::Object(o) => o.clone().as_any_rc().downcast::<RPort>().ok(),
        _ => None,
    }
}

// ---------------------------------------------------------------- registry

static MAIN_RACTOR: OnceLock<RRactor> = OnceLock::new();
static RACTORS: OnceLock<PlMutex<Vec<Weak<RactorData>>>> = OnceLock::new();
static NEXT_RACTOR_ID: AtomicU32 = AtomicU32::new(2);

thread_local! {
    /// THIS thread's ractor -- installed at ractor spawn, inherited by
    /// `Thread.new` children, `None` (= main) everywhere else.
    static CURRENT_RACTOR: RefCell<Option<RRactor>> = const { RefCell::new(None) };
}

fn registry() -> &'static PlMutex<Vec<Weak<RactorData>>> {
    RACTORS.get_or_init(|| PlMutex::new(Vec::new()))
}

fn main_ractor() -> RRactor {
    MAIN_RACTOR
        .get_or_init(|| {
            let data = Arc::new(RactorData::build(1, None, None));
            data.status.store(STATUS_RUNNING, Ordering::SeqCst);
            registry().lock().push(Arc::downgrade(&data));
            data
        })
        .clone()
}

/// The calling thread's ractor -- the TLS slot, or the (lazily built) main
/// ractor for any thread outside a spawned Ractor. Lazy so registry-less
/// unit tests work.
pub fn current_ractor() -> RRactor {
    CURRENT_RACTOR
        .with(|c| c.borrow().clone())
        .unwrap_or_else(main_ractor)
}

/// Whether this thread runs in the MAIN ractor -- the TLS slot alone, with no
/// `Arc` clone and no lazy build. The hot half of [`ivar_isolation_check`].
pub fn in_main_ractor() -> bool {
    CURRENT_RACTOR.with(|c| c.borrow().is_none())
}

/// CRuby's `rb_ivar_lookup`/`rb_ivar_set` guard: the instance variables of a
/// SHAREABLE object are unreachable from a non-main Ractor, because two
/// ractors would otherwise race on one table. `Ractor.new { @n }` is the shape
/// that meets it -- an isolated Proc's `self` is the ractor, which is
/// inherently shareable.
///
/// A class/module is exempt. CRuby checks those on the VALUE instead (a
/// class ivar holding an unshareable object is the error there, not the read
/// itself), and `K.peek` reading an Integer from a non-main ractor answers
/// normally -- oracle-verified.
pub fn ivar_isolation_check(recv: &RubyValue) -> Result<(), Signal> {
    if in_main_ractor() || matches!(recv, RubyValue::Class(_)) {
        return Ok(());
    }
    // FROZEN is the exemption CRuby's own guard carries: a frozen object's
    // table cannot change, so two ractors reading it race on nothing.
    // `Ractor.make_shareable(o)` freezes `o`, which is what makes its
    // reader legal inside the ractor it was sent to.
    if shareable(recv) && !recv.is_frozen() {
        return Err(raise_error(
            "Ractor::IsolationError",
            "can not access instance variables of shareable objects from non-main Ractors"
                .to_string(),
        ));
    }
    Ok(())
}

/// Seed the main ractor eagerly at bootstrap so `Ractor.count`/`#inspect`
/// never observe a world without it.
pub fn init_main_ractor() {
    let _ = main_ractor();
}

/// `Thread.new`'s inheritance half: a spawned Thread belongs to the ractor
/// that spawned it (CRuby's threads-within-a-ractor model).
pub(crate) fn install_current_ractor(r: RRactor) {
    CURRENT_RACTOR.with(|c| *c.borrow_mut() = Some(r));
}

/// `Ractor.count` -- the non-terminated ractors (main included). Dead weak
/// refs are pruned on the way.
fn ractor_count() -> i64 {
    let _ = main_ractor();
    let mut reg = registry().lock();
    reg.retain(|w| w.strong_count() > 0);
    reg.iter()
        .filter_map(Weak::upgrade)
        .filter(|r| r.status.load(Ordering::SeqCst) != STATUS_TERMINATED)
        .count() as i64
}

// ---------------------------------------------------------------- port ops

fn closed_port_error() -> Signal {
    raise_error(
        "Ractor::ClosedError",
        "The port was already closed".to_string(),
    )
}

/// The creator's cached default-port handle (id 0).
fn default_port(r: &RRactor) -> Arc<RPort> {
    r.default_port_cache
        .get_or_init(|| RPort::new(r.clone(), 0))
        .clone()
}

/// A fresh entry in `r`'s table.
fn create_port_ref(r: &RRactor) -> PortRef {
    let mut t = r.ports.lock();
    let id = t.next_port_id;
    t.next_port_id += 1;
    t.ports.insert(
        id,
        PortQueue {
            items: VecDeque::new(),
            closed: false,
        },
    );
    PortRef {
        creator: r.clone(),
        port_id: id,
    }
}

/// A fresh port in `r`'s table.
fn create_port(r: &RRactor) -> Arc<RPort> {
    let t = create_port_ref(r);
    RPort::new(t.creator, t.port_id)
}

/// Remove an internal (join/select) port's entry outright -- unlike a user
/// `#close`, nothing can hold this handle afterwards, so keeping the drained
/// queue would only grow the table.
fn drop_port(port: &RPort) {
    let t = port.target();
    t.creator.ports.lock().ports.remove(&t.port_id);
}

/// `Ractor::Port#send` -- callable from ANY thread, never blocks. The
/// payload crosses the boundary FIRST (before any lock), then only the
/// creator's table lock is taken.
pub(crate) fn port_send(port: &RPort, value: &RubyValue) -> Result<(), Signal> {
    port_send_in(port, value, false)
}

/// The `move:`-aware form behind `Ractor#send`/`Port#send`.
pub(crate) fn port_send_in(port: &RPort, value: &RubyValue, move_it: bool) -> Result<(), Signal> {
    let mode = if move_it {
        CrossMode::Move
    } else {
        CrossMode::Copy
    };
    let crossed = cross_graph(value, mode).map_err(|(cls, msg)| raise_error(cls, msg))?;
    let tgt = port.target();
    let r = &tgt.creator;
    let mut t = r.ports.lock();
    match t.ports.get_mut(&tgt.port_id) {
        Some(q) if !q.closed => {
            q.items.push_back(crossed);
            crate::gvl::deadlock::note_progress();
            r.recv_cv.notify_all();
            Ok(())
        }
        _ => Err(closed_port_error()),
    }
}

/// `Ractor::Port#receive` -- creator only. The whole lock-wait-take runs
/// under one armed-Gvl release, with `thread.rs`'s interrupt-aware 2ms wait
/// shape (drop the lock, take the interrupt, re-acquire).
pub(crate) fn port_receive(port: &RPort) -> Result<RubyValue, Signal> {
    if !Arc::ptr_eq(&port.target().creator, &current_ractor()) {
        return Err(raise_error(
            "Ractor::Error",
            "only allowed from the creator Ractor of this port".to_string(),
        ));
    }
    crate::thread::check_interrupt()?;
    crate::gvl::without_gvl(|| port_receive_locked(port))
}

fn port_receive_locked(port: &RPort) -> Result<RubyValue, Signal> {
    let tgt = port.target();
    let r = &tgt.creator;
    let mut table = r.ports.lock();
    loop {
        let Some(q) = table.ports.get_mut(&tgt.port_id) else {
            return Err(closed_port_error());
        };
        if let Some(v) = q.items.pop_front() {
            return Ok(v);
        }
        if q.closed {
            return Err(closed_port_error());
        }
        r.status.store(STATUS_BLOCKING, Ordering::SeqCst);
        let _ = r.recv_cv.wait_for(&mut table, Duration::from_millis(2));
        r.status.store(STATUS_RUNNING, Ordering::SeqCst);
        // POLLED but never REGISTERED, on purpose. The verdict needs
        // `BLOCKED >= live_thread_count`, so a pure-ractor deadlock can
        // never reach it -- and it should not: ruby 4.0.6 hangs on exactly
        // this program (two ractors each waiting on the other) rather than
        // raising, so registering here would raise where ruby does not.
        // The poll stays because a ractor waiting while every THREAD is
        // blocked is a verdict ruby does reach.
        if crate::gvl::deadlock::no_progress_possible() {
            drop(table);
            return Err(crate::gvl::deadlock::signal());
        }
        // Deliver a pending kill/raise now that we're awake, dropping the
        // lock first so the unwinding thread isn't holding the table mutex.
        if crate::thread::interrupt_pending() {
            drop(table);
            crate::thread::check_interrupt()?;
            table = r.ports.lock();
        }
    }
}

/// `Ractor::Port#close` -- creator only; the entry is KEPT (queued items
/// stay receivable), new sends and drained receives see `ClosedError`.
pub(crate) fn port_close(port: &RPort) -> Result<(), Signal> {
    let tgt = port.target();
    if !Arc::ptr_eq(&tgt.creator, &current_ractor()) {
        return Err(raise_error(
            "Ractor::Error",
            "closing port by other ractors is not allowed".to_string(),
        ));
    }
    let r = &tgt.creator;
    let mut t = r.ports.lock();
    if let Some(q) = t.ports.get_mut(&tgt.port_id) {
        q.closed = true;
    }
    crate::gvl::deadlock::note_progress();
    r.recv_cv.notify_all();
    Ok(())
}

fn port_closed(port: &RPort) -> bool {
    let tgt = port.target();
    let t = tgt.creator.ports.lock();
    t.ports.get(&tgt.port_id).is_none_or(|q| q.closed)
}

// ---------------------------------------------------------------- monitors

fn termination_token(r: &RRactor) -> Symbol {
    let aborted = matches!(
        &*r.state.lock(),
        RactorLifecycle::Terminated { aborted: true, .. }
    );
    Symbol::intern(if aborted { "aborted" } else { "exited" })
}

/// `Ractor#monitor(port)` -- register `port` for the termination token.
/// `false` (with the token sent immediately) when `target` already
/// terminated. The status read happens UNDER the monitors lock, mirroring
/// `finish`'s status-store-then-monitors-take order, so a registration can
/// neither race past termination nor double-deliver.
pub(crate) fn ractor_monitor(target: &RRactor, port: &Arc<RPort>) -> bool {
    let mut ms = target.monitors.lock();
    if target.status.load(Ordering::SeqCst) == STATUS_TERMINATED {
        drop(ms);
        let token = termination_token(target);
        let _ = port_send(port, &RubyValue::Symbol(token));
        return false;
    }
    ms.push(port.clone());
    true
}

/// `Ractor#unmonitor(port)` -- remove by port identity ({creator, id}).
pub(crate) fn ractor_unmonitor(target: &RRactor, port: &RPort) {
    let tgt = port.target();
    target
        .monitors
        .lock()
        .retain(|p| !same_port(&p.target(), &tgt));
}

// ---------------------------------------------------------------- lifecycle

/// The one exit path: store the outcome, flip the status anchor, drain the
/// own port table, then -- holding NOTHING -- feed every monitor.
fn finish(r: &RRactor, result: Result<RubyValue, Signal>, aborted: bool) {
    *r.state.lock() = RactorLifecycle::Terminated {
        result,
        aborted,
        crossed: false,
    };
    r.status.store(STATUS_TERMINATED, Ordering::SeqCst);
    {
        let mut t = r.ports.lock();
        t.ports.clear();
        crate::gvl::deadlock::note_progress();
        r.recv_cv.notify_all();
    }
    let token = Symbol::intern(if aborted { "aborted" } else { "exited" });
    let monitors = std::mem::take(&mut *r.monitors.lock());
    for port in monitors {
        // A closed monitor port is the monitor-holder's business, not this
        // exit path's.
        let _ = port_send(&port, &RubyValue::Symbol(token));
    }
}

/// A ractor that dies of an uncaught exception reports it on stderr as it
/// terminates, exactly as a Thread does -- CRuby runs a ractor's body on a
/// thread, so it prints that thread's own banner and then the ordinary
/// uncaught report. `#value` raising `Ractor::RemoteError` is a separate
/// mechanism, and a program sees both.
///
/// The banner carries no origin where `Thread#inspect`'s does: CRuby's ractor
/// thread was not created by `Thread.new`, so it has no call site to name.
/// `Thread.report_on_exception` governs it, as it governs a thread's.
fn report_terminated(r: &RRactor, exc: &RubyValue) {
    if !crate::thread::report_on_exception_default() {
        return;
    }
    let preamble = format!(
        "#<Thread:0x{:016x} run> terminated with exception (report_on_exception is true):",
        Arc::as_ptr(r) as *const () as usize
    );
    crate::builtins::exception::report_exception(exc, Some(&preamble));
}

/// Records the abort even when the body PANICS (a zeo-rt bug or a
/// resource-limit hit, not a Ruby exception) so joiners see `:aborted`
/// instead of hanging forever.
struct FinishGuard {
    r: RRactor,
    armed: bool,
}

impl FinishGuard {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        if self.armed {
            finish(
                &self.r,
                Err(crate::builtins::runtime_error!(
                    "ractor terminated by panic"
                )),
                true,
            );
        }
    }
}

/// `Ractor.new(*args, name: nil) { |*params| ... }` -- `args` cross the
/// boundary NOW (CRuby sends them through the ordinary message path), so a
/// rejection raises the same typed error a `#send` would (TypeError
/// "allocator undefined for Proc", `Ractor::Error` for a Thread, ...).
/// `loc` is the call site (`"file:line"`) when codegen could resolve it,
/// else the current frame's.
pub fn ractor_new(
    block: RubyValue,
    args: Vec<RubyValue>,
    name: Option<RubyValue>,
    loc: Option<String>,
) -> Result<RubyValue, Signal> {
    crate::builtins::warning::warn_ractor_experimental();
    let name = match name {
        None => None,
        Some(RubyValue::Str(s)) => Some(s.lock().to_utf8_lossy().into_owned()),
        Some(other) => {
            return Err(crate::builtins::type_error!(
                "no implicit conversion of {} into String",
                crate::builtins::class_name_of(&other)
            ));
        }
    };
    let body = block.as_proc_unchecked();
    // A dynamic proc (`Ractor.new(&pred)`) refuses HERE, where CRuby's own
    // Proc-isolation check runs -- a plain ArgumentError, message verbatim
    // from ruby 4.0.6 (`proc.c`, `rb_proc_isolate`). A literal block never
    // arrives with the tag: codegen already rejected it at compile time
    // (the stricter-earlier check `emit_call`'s Ractor arm documents).
    if let Some(outer) = body.outer_capture() {
        return Err(crate::builtins::arg_error!(
            "can not isolate a Proc because it accesses outer variables ({outer})."
        ));
    }
    let crossed: Vec<RubyValue> = args
        .iter()
        .map(|a| cross_graph(a, CrossMode::Copy).map_err(|(cls, msg)| raise_error(cls, msg)))
        .collect::<Result<_, _>>()?;
    let loc = loc.or_else(|| crate::frames::current_location().map(|(f, l)| format!("{f}:{l}")));
    let data = Arc::new(RactorData::build(
        NEXT_RACTOR_ID.fetch_add(1, Ordering::Relaxed),
        name,
        loc,
    ));
    let _ = main_ractor();
    registry().lock().push(Arc::downgrade(&data));
    let for_thread = data.clone();
    // BEFORE the spawn -- see `gvl::note_thread_spawn`.
    crate::gvl::note_thread_spawn();
    std::thread::Builder::new()
        // CRuby-sized, like `Thread.new`'s.
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            CURRENT_RACTOR.with(|c| *c.borrow_mut() = Some(for_thread.clone()));
            for_thread.status.store(STATUS_RUNNING, Ordering::SeqCst);
            let guard = FinishGuard {
                r: for_thread.clone(),
                armed: true,
            };
            // An isolated Proc's `self` is the RACTOR, not whatever object
            // created it -- `Ractor.new { self.class }` answers `Ractor`
            // (oracle-verified). That is what makes `@n` in the block an ivar
            // read of a SHAREABLE object, which `ivar_isolation_check` then
            // turns into the `Ractor::IsolationError` CRuby raises.
            let result = body.call_with_self(&RubyValue::Ractor(for_thread.clone()), &crossed);
            guard.disarm();
            let aborted = result.is_err();
            if let Err(Signal::Raise(exc)) = &result {
                report_terminated(&for_thread, exc);
            }
            finish(&for_thread, result, aborted);
        })
        .expect("spawning a Ractor's OS thread");
    Ok(RubyValue::Ractor(data))
}

/// Park until `r` terminates; the token says how. Runs through a fresh
/// monitor port in the CALLER's table, unmonitored and dropped on every
/// exit path (the `ensure` of CRuby's ractor.rb join).
fn wait_for_termination(r: &RRactor) -> Result<Symbol, Signal> {
    let me = current_ractor();
    let port = create_port(&me);
    ractor_monitor(r, &port);
    let received = port_receive(&port);
    ractor_unmonitor(r, &port);
    drop_port(&port);
    match received? {
        RubyValue::Symbol(s) => Ok(s),
        // The port was ours alone; only tokens ever land on it.
        _ => Ok(Symbol::intern("exited")),
    }
}

/// The successor-gated outcome take: first taker claims the slot (the same
/// ractor may re-take); an aborted ractor relays as `Ractor::RemoteError`
/// with `#ractor` and `#cause` set. Callers must have observed termination.
fn take_value(r: &RRactor) -> Result<RubyValue, Signal> {
    let me = current_ractor().id;
    if let Err(prev) = r
        .successor
        .compare_exchange(0, me, Ordering::SeqCst, Ordering::SeqCst)
        && prev != me
    {
        return Err(raise_error(
            "Ractor::Error",
            "Only the successor ractor can take a value".to_string(),
        ));
    }
    let (outcome, already_crossed) = match &*r.state.lock() {
        RactorLifecycle::Terminated {
            result, crossed, ..
        } => (result.clone(), *crossed),
        RactorLifecycle::Live => {
            unreachable!("take_value before termination was observed")
        }
    };
    match outcome {
        Ok(v) if already_crossed => Ok(v),
        Ok(v) => {
            let crossed = cross_boundary(&v).map_err(|msg| {
                raise_error(
                    "Ractor::Error",
                    format!("this Ractor's final value can't cross back: {msg}"),
                )
            })?;
            *r.state.lock() = RactorLifecycle::Terminated {
                result: Ok(crossed.clone()),
                aborted: false,
                crossed: true,
            };
            Ok(crossed)
        }
        Err(sig) => {
            let cause = match &sig {
                Signal::Raise(exc) => Some(exc.clone()),
                _ => None,
            };
            let err = raise_error_details(
                "Ractor::RemoteError",
                "thrown by remote Ractor.".to_string(),
                &[("ractor", RubyValue::Ractor(r.clone()))],
            );
            if let (Signal::Raise(exc), Some(c)) = (&err, cause) {
                let _ = crate::builtins::exception::set_explicit_cause(exc, c);
            }
            Err(err)
        }
    }
}

/// `Ractor#join` -- park until termination; an aborted target relays its
/// `Ractor::RemoteError` here (the value-take, so the successor slot is
/// claimed exactly as `#value` would).
pub fn ractor_join(r: &RRactor) -> Result<(), Signal> {
    let token = wait_for_termination(r)?;
    if token == Symbol::intern("aborted") {
        take_value(r)?;
    }
    Ok(())
}

/// `Ractor#value` -- join, then the successor-gated take.
pub fn ractor_value(r: &RRactor) -> Result<RubyValue, Signal> {
    wait_for_termination(r)?;
    take_value(r)
}

/// The pre-port-model spelling of `#value` -- kept as a thin shim for
/// existing callers.
pub fn ractor_outcome(r: &RRactor) -> Result<RubyValue, Signal> {
    ractor_value(r)
}

/// `Ractor#send`/`Ractor.receive`'s runtime halves, spelled on the default
/// port. Exported for codegen's typed-receiver fast paths.
pub fn ractor_send(r: &RRactor, value: &RubyValue) -> Result<(), Signal> {
    port_send(&default_port(r), value)
}

/// `Ractor#send(obj, move: bool)`'s runtime half -- codegen's typed-receiver
/// intrinsic calls this when the call site carries the kwarg.
pub fn ractor_send_mode(r: &RRactor, value: &RubyValue, move_it: bool) -> Result<(), Signal> {
    port_send_in(&default_port(r), value, move_it)
}

pub fn ractor_receive() -> Result<RubyValue, Signal> {
    port_receive(&default_port(&current_ractor()))
}

// ---------------------------------------------------------------- select

struct SelectEntry {
    port: Arc<RPort>,
    source: RubyValue,
    monitored: Option<RRactor>,
}

fn select_cleanup(entries: &[SelectEntry]) {
    for e in entries {
        if let Some(r) = &e.monitored {
            ractor_unmonitor(r, &e.port);
            drop_port(&e.port);
        }
    }
}

/// `Ractor.select(*ports_or_ractors)` -- ruby 4.0's ractor.rb shape: a
/// Ractor argument gets a synthesized monitor port (torn down on every exit
/// path); the first ready port in ARGUMENT order wins. A port hit answers
/// `[port, obj]`; a ractor hit runs the value-take and answers
/// `[ractor, value]` (an aborted ractor raises its `RemoteError` here).
pub fn ractor_select(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        return Err(crate::builtins::arg_error!(
            "specify at least one Ractor::Port or Ractor"
        ));
    }
    let me = current_ractor();
    let mut entries: Vec<SelectEntry> = Vec::new();
    for a in args {
        if let RubyValue::Ractor(r) = a {
            let port = create_port(&me);
            ractor_monitor(r, &port);
            entries.push(SelectEntry {
                port,
                source: a.clone(),
                monitored: Some(r.clone()),
            });
            continue;
        }
        let Some(p) = as_port(a) else {
            select_cleanup(&entries);
            return Err(crate::builtins::arg_error!(
                "should be Ractor::Port or Ractor"
            ));
        };
        if !Arc::ptr_eq(&p.target().creator, &me) {
            select_cleanup(&entries);
            return Err(raise_error(
                "Ractor::Error",
                "only allowed from the creator Ractor of this port".to_string(),
            ));
        }
        entries.push(SelectEntry {
            port: p,
            source: a.clone(),
            monitored: None,
        });
    }
    let hit = crate::thread::check_interrupt()
        .and_then(|()| crate::gvl::without_gvl(|| select_wait_locked(&me, &entries)));
    let result = hit.and_then(|(idx, obj)| {
        let entry = &entries[idx];
        match &entry.monitored {
            Some(r) => take_value(r).map(|v| vec![entry.source.clone(), v]),
            None => Ok(vec![entry.source.clone(), obj]),
        }
    });
    select_cleanup(&entries);
    result.map(|pair| RubyValue::Array(crate::array_new(pair)))
}

fn select_wait_locked(me: &RRactor, entries: &[SelectEntry]) -> Result<(usize, RubyValue), Signal> {
    let mut table = me.ports.lock();
    loop {
        for (i, e) in entries.iter().enumerate() {
            let Some(q) = table.ports.get_mut(&e.port.target().port_id) else {
                return Err(closed_port_error());
            };
            if let Some(v) = q.items.pop_front() {
                return Ok((i, v));
            }
            if q.closed {
                return Err(closed_port_error());
            }
        }
        me.status.store(STATUS_BLOCKING, Ordering::SeqCst);
        let _ = me.recv_cv.wait_for(&mut table, Duration::from_millis(2));
        me.status.store(STATUS_RUNNING, Ordering::SeqCst);
        if crate::gvl::deadlock::no_progress_possible() {
            drop(table);
            return Err(crate::gvl::deadlock::signal());
        }
        if crate::thread::interrupt_pending() {
            drop(table);
            crate::thread::check_interrupt()?;
            table = me.ports.lock();
        }
    }
}

// ---------------------------------------------------------------- sharing

/// `Ractor.shareable?` -- the recursive predicate (CRuby's rule: frozen AND
/// everything reachable shareable; immediates/Symbols/Ractors/Ports
/// inherently shareable; `Regexp` immutable here so always shareable).
/// Cycle-guarded via the same visited-set mechanism as `inspect_string`'s
/// (`value::container_identity`): a container already under examination
/// contributes `true` at its re-entry point -- the cycle itself never makes
/// a graph unshareable, only an unfrozen/unshareable NODE does, and every
/// node is still visited exactly once.
pub fn shareable(v: &RubyValue) -> bool {
    shareable_guarded(v, &mut Vec::new())
}

fn shareable_guarded(v: &RubyValue, seen: &mut Vec<usize>) -> bool {
    // A husk left by `move: true` reports shareable (oracle-verified:
    // `Ractor.shareable?(husk)` answers true in 4.0.6), and sending one on
    // passes it by reference like any other shareable.
    if crate::runtime_meta::any_moved() && crate::dispatch::value_moved(v) {
        return true;
    }
    if let Some(ptr) = crate::value::container_identity(v) {
        if seen.contains(&ptr) {
            return true;
        }
        seen.push(ptr);
    }
    match v {
        RubyValue::Nil
        | RubyValue::Bool(_)
        | RubyValue::Int(_)
        | RubyValue::BigInt(_)
        | RubyValue::Rational(_)
        | RubyValue::Complex(_)
        | RubyValue::Float(_)
        | RubyValue::Symbol(_)
        | RubyValue::Regexp(_)
        | RubyValue::Ractor(_)
        // A class handle is inherently shareable: classes are
        // process-wide in real Ruby too.
        | RubyValue::Class(_) => true,
        RubyValue::Range(__rg) => {
            let (start, end, _) = __rg.parts();
            start.is_none_or(|s| shareable_guarded(s, seen))
                && end.is_none_or(|e| shareable_guarded(e, seen))
        }
        RubyValue::Str(s) => s.is_frozen(),
        RubyValue::Array(a) => a.is_frozen() && a.lock().iter().all(|e| shareable_guarded(e, seen)),
        RubyValue::Hash(h) => {
            h.is_frozen()
                && h.lock()
                    .values()
                    .all(|(k, val)| shareable_guarded(k, seen) && shareable_guarded(val, seen))
        }
        RubyValue::Object(o) => {
            // A Port handle is inherently shareable (oracle-verified:
            // `Ractor.shareable?(Ractor::Port.new)` is true in 4.0.6) --
            // it's a {creator, id} reference, and the queue behind it is
            // fed through the crossing discipline anyway.
            o.as_any().downcast_ref::<RPort>().is_some()
                || (o.is_frozen() && o.ivar_values().iter().all(|iv| shareable_guarded(iv, seen)))
        }
        // A frozen Proc counts as shareable (the post-`make_shareable`
        // state; see the freeze-without-isolation-check note there).
        RubyValue::Proc(p) => p.is_frozen(),
        RubyValue::Fiber(_)
        | RubyValue::Enumerator(_)
        | RubyValue::Yielder(_)
        | RubyValue::Thread(_)
        | RubyValue::Mutex(_)
        | RubyValue::Queue(_)
        | RubyValue::MatchData(_) => false,
    }
}

/// `Ractor.make_shareable(obj)` -- the deep-freeze traversal (CRuby's
/// `rb_ractor_make_shareable`: walk the reachable subgraph, freeze each
/// node). Returns the (now shareable) value itself; `Err` when the graph
/// contains something that can never be shareable. Cycle-guarded: an
/// already-visited container is already frozen-and-being-walked, so its
/// re-entry is a no-op. Note `seen` here is a PERMANENT visited set, not
/// `display_with`'s pop-on-exit stack -- freezing is idempotent and each
/// node needs walking only once, and (unlike printing) there's no output
/// that would differ.
pub fn make_shareable(v: &RubyValue) -> Result<RubyValue, String> {
    make_shareable_guarded(v, &mut Vec::new())?;
    Ok(v.clone())
}

/// [`make_shareable`] with the failure typed as CRuby's `Ractor::Error` --
/// the shape both the codegen intrinsic and the dynamic row raise.
pub fn make_shareable_value(v: &RubyValue) -> Result<RubyValue, Signal> {
    make_shareable(v).map_err(|msg| raise_error("Ractor::Error", msg))
}

fn make_shareable_guarded(v: &RubyValue, seen: &mut Vec<usize>) -> Result<(), String> {
    if let Some(ptr) = crate::value::container_identity(v) {
        if seen.contains(&ptr) {
            return Ok(());
        }
        seen.push(ptr);
    }
    match v {
        RubyValue::Nil
        | RubyValue::Bool(_)
        | RubyValue::Int(_)
        | RubyValue::BigInt(_)
        | RubyValue::Rational(_)
        | RubyValue::Complex(_)
        | RubyValue::Float(_)
        | RubyValue::Symbol(_)
        | RubyValue::Regexp(_)
        | RubyValue::Ractor(_)
        | RubyValue::Class(_) => {}
        RubyValue::Range(__rg) => {
            let (start, end, _) = __rg.parts();
            if let Some(s) = start {
                make_shareable_guarded(s, seen)?;
            }
            if let Some(e) = end {
                make_shareable_guarded(e, seen)?;
            }
        }
        RubyValue::Str(s) => s.set_frozen(),
        RubyValue::Array(a) => {
            a.set_frozen();
            for elem in a.lock().iter() {
                make_shareable_guarded(elem, seen)?;
            }
        }
        RubyValue::Hash(h) => {
            h.set_frozen();
            for (k, val) in h.lock().values() {
                make_shareable_guarded(k, seen)?;
                make_shareable_guarded(val, seen)?;
            }
        }
        RubyValue::Object(o) => {
            o.set_frozen();
            for iv in o.ivar_values() {
                make_shareable_guarded(&iv, seen)?;
            }
        }
        // CRuby isolates a Proc (snapshots its captured outers, requiring
        // self and each captured value be shareable -- `Proc#isolate`).
        // A compiled zeo closure's captures can't be inspected, so the
        // proc is frozen and accepted WITHOUT the isolation check -- over-
        // permissive vs CRuby's IsolationError, but every zeo value is
        // Send+Sync by construction, so nothing unsafe can result
        // (ostruct's `Ractor.make_shareable(getter_proc)` is the driving
        // use, and its captures are shareable anyway).
        RubyValue::Proc(p) => p.set_frozen(),
        other => {
            return Err(format!(
                "can't make shareable object: {}",
                other.to_display_string()
            ));
        }
    }
    Ok(())
}

/// The one `Ractor::MovedError` every husk raises -- dispatch's moved
/// probes, the container `_checked` twins, and codegen's accessor guard all
/// funnel here.
pub fn moved_object_error() -> Signal {
    raise_error(
        "Ractor::MovedError",
        "can not send any methods to a moved object".to_string(),
    )
}

/// A cross-boundary refusal: the exception class to raise, and its message.
type CrossFail = (&'static str, String);

/// How a graph crosses: `Copy` deep-copies every unshareable node; `Move`
/// additionally POISONS each source node afterwards (`send(obj, move:
/// true)` -- the source raises `Ractor::MovedError` from then on).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum CrossMode {
    Copy,
    Move,
}

/// One `cross_graph` run's working state. `built` is the seen-table: cycle
/// guard AND duplicate preservation -- CRuby's own traversal husks a
/// duplicated sibling reference (`[x, x]` delivers a husk in slot 1); zeo
/// deliberately maps every re-encounter through this table instead.
struct CrossState {
    mode: CrossMode,
    built: FMap<usize, RubyValue>,
    /// Move mode: every source node to poison at commit, in visit order.
    sources: Vec<RubyValue>,
}

impl CrossState {
    /// Register `built` under `src`'s identity BEFORE the children are
    /// walked -- what makes a cycle re-enter the table and reconstruct.
    fn remember(&mut self, src: &RubyValue, built: &RubyValue) {
        if let Some(id) = cross_identity(src) {
            self.built.insert(id, built.clone());
        }
        if self.mode == CrossMode::Move {
            self.sources.push(src.clone());
        }
    }
}

/// The seen-table key: `value::container_identity`'s three recursive kinds
/// plus `Str` (never cyclic, but a duplicated reference must map to ONE
/// copy, and a move must poison it once).
fn cross_identity(v: &RubyValue) -> Option<usize> {
    match v {
        RubyValue::Str(s) => Some(Arc::as_ptr(s) as usize),
        RubyValue::Array(a) => Some(Arc::as_ptr(a) as usize),
        RubyValue::Hash(h) => Some(Arc::as_ptr(h) as usize),
        RubyValue::Object(o) => Some(Arc::as_ptr(o) as *const () as usize),
        _ => None,
    }
}

/// By reference when shareable, rebuilt otherwise -- the one walker behind
/// both crossing modes, in two phases:
///
/// 1. BUILD (no source mutation anywhere): the receiver-side graph is
///    constructed with each parent registered before its children are
///    walked, so cycles reconstruct. Every refusal raises out of this phase
///    BEFORE any source is touched -- unlike CRuby, which poisons the part
///    of the graph it walked before a refused move (destroying data).
/// 2. COMMIT (move only, infallible): each source's MOVED flag is set, then
///    its payload gutted under the lock; an `RObj` retags its class word to
///    `Ractor::MovedObject`; finally ONE `GATE_MOVED` arm per send.
pub(crate) fn cross_graph(v: &RubyValue, mode: CrossMode) -> Result<RubyValue, CrossFail> {
    let mut st = CrossState {
        mode,
        built: FMap::default(),
        sources: Vec::new(),
    };
    let out = cross_build(v, &mut st)?;
    if !st.sources.is_empty() {
        for src in &st.sources {
            poison(src);
        }
        crate::runtime_meta::mark_moved();
    }
    Ok(out)
}

fn cross_build(v: &RubyValue, st: &mut CrossState) -> Result<RubyValue, CrossFail> {
    // Shareable nodes pass by REFERENCE and terminate their subtree
    // un-poisoned -- moving a shareable directly is a no-op poison-wise
    // (the source stays usable, the receiver holds the same reference).
    if shareable(v) {
        return Ok(v.clone());
    }
    if let Some(id) = cross_identity(v)
        && let Some(done) = st.built.get(&id)
    {
        return Ok(done.clone());
    }
    match v {
        RubyValue::Str(s) => {
            // A `StrBuf` clone: byte- and encoding-faithful.
            let out = RubyValue::Str(crate::string_wrap(s.lock().clone()));
            st.remember(v, &out);
            Ok(out)
        }
        RubyValue::Array(a) => {
            let dst = crate::array_new(Vec::new());
            let out = RubyValue::Array(dst.clone());
            st.remember(v, &out);
            for e in crate::collections::array_snapshot(a) {
                let crossed = cross_build(&e, st)?;
                dst.lock().push(crossed);
            }
            Ok(out)
        }
        RubyValue::Hash(h) => {
            let dst = crate::hash_new(Vec::new());
            let out = RubyValue::Hash(dst.clone());
            st.remember(v, &out);
            let (default, default_proc, by_identity) = {
                let g = h.lock();
                (
                    g.default.clone(),
                    g.default_proc.clone(),
                    g.compare_by_identity,
                )
            };
            if by_identity {
                dst.lock().compare_by_identity = true;
            }
            for (k, val) in crate::collections::hash_pairs_snapshot(h) {
                let ck = cross_build(&k, st)?;
                let cv = cross_build(&val, st)?;
                crate::hash_set(&dst, ck, cv);
            }
            // The per-instance default travels with the pairs.
            let crossed_default = cross_build(&default, st)?;
            let crossed_proc = match &default_proc {
                Some(p) => Some(cross_build(p, st)?),
                None => None,
            };
            {
                let mut g = dst.lock();
                g.default = crossed_default;
                g.default_proc = crossed_proc;
            }
            Ok(out)
        }
        RubyValue::Range(__rg) => {
            let (start, end, excl) = __rg.parts();
            // A Range is an inline value here (no shared identity): its
            // ENDPOINTS cross -- and move -- while the shell itself
            // survives un-poisoned, unlike CRuby's husked Range object.
            let mut cross_end = |e: Option<&RubyValue>| -> Result<_, CrossFail> {
                Ok(match e {
                    Some(inner) => Some(cross_build(inner, st)?),
                    None => None,
                })
            };
            Ok(crate::builtins::range::range_value(
                cross_end(start)?,
                cross_end(end)?,
                excl,
            ))
        }
        RubyValue::Object(o) => {
            // zeo refuses to move an IO (CRuby moves them; a moved fd's
            // husk-vs-live-handle split has no safe answer here).
            if st.mode == CrossMode::Move && crate::dispatch::is_a(o.class_id(), zeo_abi::IO_CLASS)
            {
                return Err(("Ractor::Error", "can not move IO object.".to_string()));
            }
            // A plain object crosses as CRuby's does: a shallow dup whose
            // ivars (and a value-subclass's payload) are rewritten with
            // their crossed counterparts.
            let dup = o.dup_object(true);
            let out = RubyValue::Object(dup.clone());
            st.remember(v, &out);
            for (name, val) in o.ivar_pairs() {
                let crossed = cross_build(&val, st)?;
                dup.ivar_set_named(name.strip_prefix('@').unwrap_or(&name), crossed);
            }
            if let Some(p) = o.builtin_payload() {
                let crossed = cross_build(&p, st)?;
                dup.set_builtin_payload(crossed);
            }
            Ok(out)
        }
        RubyValue::Proc(_) => Err(match st.mode {
            // CRuby's copy path fails at allocation; its move path refuses
            // by class name. Both messages verbatim.
            CrossMode::Copy => ("TypeError", "allocator undefined for Proc".to_string()),
            CrossMode::Move => ("Ractor::Error", "can not move Proc object.".to_string()),
        }),
        other => Err(match st.mode {
            CrossMode::Copy => (
                "Ractor::Error",
                format!(
                    "{} can't cross a Ractor boundary",
                    other.to_display_string()
                ),
            ),
            CrossMode::Move => (
                "Ractor::Error",
                format!("can not move {} object.", crate::class_name_of_value(other)),
            ),
        }),
    }
}

/// The commit half: MOVED flag first, then the payload gutted under its own
/// lock, so a racing reader sees the poison before (or with) the empty husk.
/// An object retags its class word where its concrete type supports it; a
/// builtin `RObj` that cannot retag stays usable -- the safe direction.
fn poison(src: &RubyValue) {
    match src {
        RubyValue::Str(s) => {
            s.set_moved();
            let mut g = s.lock();
            let enc = g.encoding();
            *g = crate::encoding::StrBuf::from_bytes(Vec::new(), enc);
        }
        RubyValue::Array(a) => {
            a.set_moved();
            a.lock().clear();
        }
        RubyValue::Hash(h) => {
            h.set_moved();
            let mut g = h.lock();
            g.clear();
            g.default = RubyValue::Nil;
            g.default_proc = None;
        }
        RubyValue::Object(o) => {
            o.retag_moved();
        }
        _ => {}
    }
}

/// The copy-mode crossing with message-only errors -- `Ractor.new`'s arg
/// path and the value-take keep their existing plumbing.
pub fn cross_boundary(v: &RubyValue) -> Result<RubyValue, String> {
    cross_graph(v, CrossMode::Copy).map_err(|(_, msg)| msg)
}

// ---------------------------------------------------------------- row glue

/// `#send`'s argument split: exactly one payload, plus the `move:` kwarg
/// (recognized only as codegen's kwargs-marked trailing Hash, so a plain
/// Hash PAYLOAD stays a payload).
fn send_payload(args: &[RubyValue]) -> Result<(&RubyValue, bool), Signal> {
    let (opts, pos): (Option<&crate::RHash>, &[RubyValue]) = match args.last() {
        Some(RubyValue::Hash(h)) if crate::collections::hash_is_kwargs(h) => {
            (Some(h), &args[..args.len() - 1])
        }
        _ => (None, args),
    };
    crate::builtins::check_arity(pos.len(), 1, Some(1))?;
    Ok((&pos[0], opts.is_some_and(move_requested)))
}

fn move_requested(opts: &crate::RHash) -> bool {
    crate::collections::hash_get(opts, &RubyValue::Symbol(Symbol::intern("move"))).truthy()
}

/// A `Ractor#[]`/`.[]` key: a Symbol, or a String interned to one
/// (oracle-verified: `Ractor["s"]` answers nil rather than raising).
fn local_key(v: &RubyValue) -> Result<Symbol, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(crate::builtins::type_error!(
            "{} is not a symbol",
            other.inspect_string()
        )),
    }
}

pub(crate) fn ractor_inspect(r: &RRactor) -> String {
    let mut s = format!("#<Ractor:#{}", r.id);
    if let Some(name) = &r.name {
        s.push(' ');
        s.push_str(name);
    }
    if let Some(loc) = &r.loc {
        s.push(' ');
        s.push_str(loc);
    }
    s.push(' ');
    s.push_str(r.status_word());
    s.push('>');
    s
}

/// `Ractor.shareable_proc`/`.shareable_lambda`'s shared body: enforce the
/// `self:` kwarg's shareability, then freeze-and-accept the proc itself --
/// the documented over-permissive fallback (a compiled closure's captures
/// can't be isolation-checked at runtime; the compile-time intrinsic is a
/// separate work item).
fn shareable_callable(
    opts: Option<&RubyValue>,
    block: Option<RubyValue>,
    lambda: bool,
) -> Result<RubyValue, Signal> {
    if let Some(RubyValue::Hash(h)) = opts {
        let self_v = crate::collections::hash_get(h, &RubyValue::Symbol(Symbol::intern("self")));
        if !matches!(self_v, RubyValue::Nil) && !shareable(&self_v) {
            return Err(raise_error(
                "Ractor::IsolationError",
                format!("self should be shareable: {}", self_v.inspect_string()),
            ));
        }
    }
    let Some(block) = block else {
        return Err(crate::builtins::arg_error!(
            "tried to create Proc object without a block"
        ));
    };
    let p = block.as_proc_unchecked();
    let p = if lambda && !p.is_lambda() {
        p.as_lambda()
    } else {
        p
    };
    p.set_frozen();
    Ok(RubyValue::Proc(p))
}

fn recv_port(recv: &RubyValue) -> Arc<RPort> {
    as_port(recv).expect("Ractor::Port row dispatched on a non-Port receiver")
}

// The `Ractor` surface. `Ractor#send` on a statically-typed receiver stays a
// compiler intrinsic (its `TyKind` shadow); these rows serve dynamic
// dispatch, `::Ractor` cpath forms, and everything codegen doesn't fold.
zeo_macros::ruby_class! {
    Ractor = zeo_abi::RACTOR_CLASS < zeo_abi::OBJECT_CLASS;

    // Every LITERAL `Ractor.new { ... }` compiles through codegen's
    // intrinsic; only a computed/dynamic send lands here.
    def self."new" params "*args, name: nil, &block" cfunc (_recv, *_args, &_block) {
        Err(crate::builtins::not_impl_error!(
            "dynamic Ractor.new isn't supported (zeo compiles literal Ractor.new blocks)"
        ))
    }
    def self."current"(_recv) {
        Ok(RubyValue::Ractor(current_ractor()))
    }
    def self."main"(_recv) {
        Ok(RubyValue::Ractor(main_ractor()))
    }
    def self."main?"(_recv) {
        Ok(RubyValue::Bool(Arc::ptr_eq(&current_ractor(), &main_ractor())))
    }
    def self."count"(_recv) {
        Ok(RubyValue::Int(ractor_count()))
    }
    def self."select" params "*ports" cfunc (_recv, *ports) {
        ractor_select(ports)
    }
    def self."receive" | "recv" (_recv) {
        ractor_receive()
    }
    def self."[]" params "sym"(_recv, sym) {
        let r = current_ractor();
        let key = local_key(sym)?;
        Ok(r.locals.lock().get(&key).cloned().unwrap_or(RubyValue::Nil))
    }
    def self."[]=" params "sym, val"(_recv, sym, val) {
        let r = current_ractor();
        r.locals.lock().insert(local_key(sym)?, val.clone());
        Ok(val.clone())
    }
    // Double-checked under `store_lock`: the block runs at most once per
    // key across the ractor's threads.
    def self."store_if_absent" params "sym"(_recv, sym, &block) {
        let r = current_ractor();
        let key = local_key(sym)?;
        if let Some(v) = r.locals.lock().get(&key) {
            return Ok(v.clone());
        }
        let _guard = r.store_lock.lock();
        if let Some(v) = r.locals.lock().get(&key) {
            return Ok(v.clone());
        }
        let Some(b) = block else {
            return Err(crate::builtins::local_jump_error!("no block given"));
        };
        let v = b.as_proc_unchecked().call(&[])?;
        r.locals.lock().insert(key, v.clone());
        Ok(v)
    }
    // `copy:` is accepted and ignored: zeo deep-freezes in place, which is what
    // `copy: false` asks for, and the copying form would need a deep clone the
    // runtime does not have yet.
    def self."make_shareable" params "obj, copy: nil"(_recv, obj, **_opts) {
        make_shareable_value(obj)
    }
    def self."shareable?" params "obj"(_recv, obj) {
        Ok(RubyValue::Bool(shareable(obj)))
    }
    def self."shareable_proc" params "self: nil" cfunc (_recv, **opts, &block) {
        shareable_callable(opts, block, false)
    }
    def self."shareable_lambda" params "self: nil" cfunc (_recv, **opts, &block) {
        shareable_callable(opts, block, true)
    }
    // ruby 4.0's `Ractor._require(feature)` -- the require that runs on the
    // main ractor in CRuby; here it shares `Kernel#require`'s dynamic body.
    def self."_require" params "feature"(_recv, feature) {
        crate::builtins::kernel::dynamic_require(feature)
    }

    def "send" params "*, **, &" | "<<" params "*, **, &" cfunc (recv, *args) {
        let (payload, move_it) = send_payload(args)?;
        ractor_send_mode(&recv.as_ractor_unchecked(), payload, move_it)?;
        Ok(recv.clone())
    }
    private def "receive" | "recv" (recv) {
        port_receive(&default_port(&recv.as_ractor_unchecked()))
    }
    def "default_port"(recv) {
        Ok(port_value(default_port(&recv.as_ractor_unchecked())))
    }
    def "join"(recv) {
        ractor_join(&recv.as_ractor_unchecked())?;
        Ok(recv.clone())
    }
    def "value"(recv) {
        ractor_value(&recv.as_ractor_unchecked())
    }
    def "monitor" params "port"(recv, port) {
        let Some(p) = as_port(port) else {
            // CRuby 4.0.6 SEGFAULTS here ([BUG] in <internal:ractor>); a
            // TypeError is this runtime's strictly-better answer.
            return Err(crate::builtins::wrong_arg_type(port, "Ractor::Port"));
        };
        Ok(RubyValue::Bool(ractor_monitor(&recv.as_ractor_unchecked(), &p)))
    }
    def "unmonitor" params "port"(recv, port) {
        if let Some(p) = as_port(port) {
            ractor_unmonitor(&recv.as_ractor_unchecked(), &p);
        }
        Ok(recv.clone())
    }
    def "close"(recv) {
        let r = recv.as_ractor_unchecked();
        let p = default_port(&r);
        port_close(&p)?;
        Ok(port_value(p))
    }
    def "name"(recv) {
        Ok(match &recv.as_ractor_unchecked().name {
            Some(n) => RubyValue::Str(crate::string_new(n.clone())),
            None => RubyValue::Nil,
        })
    }
    def "inspect" | "to_s" (recv) {
        Ok(RubyValue::Str(crate::string_new(ractor_inspect(&recv.as_ractor_unchecked()))))
    }
    def "[]" params "sym"(recv, sym) {
        let r = recv.as_ractor_unchecked();
        if !Arc::ptr_eq(&r, &current_ractor()) {
            return Err(crate::builtins::runtime_error!(
                "Cannot get ractor local storage for non-current ractor"
            ));
        }
        let key = local_key(sym)?;
        Ok(r.locals.lock().get(&key).cloned().unwrap_or(RubyValue::Nil))
    }
    def "[]=" params "sym, val"(recv, sym, val) {
        let r = recv.as_ractor_unchecked();
        if !Arc::ptr_eq(&r, &current_ractor()) {
            return Err(crate::builtins::runtime_error!(
                "Cannot set ractor local storage for non-current ractor"
            ));
        }
        r.locals.lock().insert(local_key(sym)?, val.clone());
        Ok(val.clone())
    }

    class Port = zeo_abi::RACTOR_PORT_CLASS < zeo_abi::OBJECT_CLASS {
        def self."new" cfunc inherits (_recv) {
            Ok(port_value(create_port(&current_ractor())))
        }
        def "receive"(recv) {
            port_receive(&recv_port(recv))
        }
        def "send" params "obj, move: nil" | "<<" params "obj, move: nil" (recv, obj, **opts) {
            let move_it = matches!(opts, Some(RubyValue::Hash(h)) if move_requested(h));
            port_send_in(&recv_port(recv), obj, move_it)?;
            Ok(recv.clone())
        }
        def "close"(recv) {
            port_close(&recv_port(recv))?;
            Ok(recv.clone())
        }
        def "closed?"(recv) {
            Ok(RubyValue::Bool(port_closed(&recv_port(recv))))
        }
        def "inspect"(recv) {
            let t = recv_port(recv).target();
            Ok(RubyValue::Str(crate::string_new(format!(
                "#<Ractor::Port to:#{} id:{}>",
                t.creator.id, t.port_id
            ))))
        }
        // Re-seats the (frozen!) handle onto a FRESH entry in the current
        // ractor's table -- CRuby's re-init gets a new id and re-opens a
        // closed port. The old entry is left behind, exactly like CRuby's.
        private def "initialize"(recv) {
            let p = recv_port(recv);
            *p.target.lock() = create_port_ref(&current_ractor());
            Ok(recv.clone())
        }
        // Aliases the copied port's entry: after `icopy(o)` the receiver IS
        // `o`'s port (same `{creator, id}`), frozen or not.
        private def "initialize_copy"(recv, other) {
            let p = recv_port(recv);
            let Some(o) = as_port(other) else {
                return Err(crate::builtins::type_error!(
                    "initialize_copy should take same class object"
                ));
            };
            *p.target.lock() = o.target();
            Ok(recv.clone())
        }
    }

    // The husk a `move: true` send leaves behind. No allocator/constructor;
    // a moved `RObj` RETAGS its class word to this id (`cross_graph`'s
    // commit), and dispatch short-circuits the id straight to
    // `moved_object_error` -- these rows serve reflection
    // (`instance_methods(false)`) and any direct table probe.
    class MovedObject = zeo_abi::RACTOR_MOVED_OBJECT_CLASS < zeo_abi::BASIC_OBJECT_CLASS {
        def "method_missing" | "__send__" | "!" | "==" | "!=" | "__id__" | "equal?" | "instance_eval" | "instance_exec" cfunc (_recv, *_args, &_block) {
            Err(raise_error(
                "Ractor::MovedError",
                "can not send any methods to a moved object".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_ractor(
        f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
        args: Vec<RubyValue>,
    ) -> RRactor {
        let block = RubyValue::Proc(crate::RProc::new(f));
        ractor_new(block, args, None, None)
            .expect("spawn")
            .as_ractor_unchecked()
    }

    #[test]
    fn shareability_tiering_and_deep_copy() {
        assert!(shareable(&RubyValue::Int(1)));
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(!shareable(&s));
        make_shareable(&s).unwrap();
        assert!(shareable(&s));

        // Deep copy: mutating the original after crossing leaves the copy
        // untouched.
        let arr = RubyValue::Array(crate::array_new(vec![RubyValue::Int(1)]));
        let crossed = cross_boundary(&arr).unwrap();
        crate::array_set(&arr.as_array_unchecked(), 0, RubyValue::Int(99));
        assert_eq!(
            crossed.as_array_unchecked().lock()[0].to_display_string(),
            "1"
        );

        // A deeply-frozen array crosses by REFERENCE (same storage).
        let frozen = RubyValue::Array(crate::array_new(vec![RubyValue::Int(2)]));
        make_shareable(&frozen).unwrap();
        let shared = cross_boundary(&frozen).unwrap();
        assert!(Arc::ptr_eq(
            &frozen.as_array_unchecked(),
            &shared.as_array_unchecked()
        ));

        // A Proc is frozen in place and counts as shareable afterwards
        // (freeze-without-isolation-check -- see `make_shareable_guarded`).
        let pr = RubyValue::Proc(crate::RProc::new(|_| Ok(RubyValue::Nil)));
        assert!(!shareable(&pr));
        make_shareable(&pr).unwrap();
        assert!(pr.is_frozen());
        assert!(shareable(&pr));
    }

    /// A Port handle is inherently shareable and crosses by reference.
    #[test]
    fn a_port_handle_is_shareable() {
        let me = current_ractor();
        let port = create_port(&me);
        let v = port_value(port.clone());
        assert!(shareable(&v));
        let crossed = cross_boundary(&v).unwrap();
        let p2 = as_port(&crossed).expect("still a port");
        assert!(same_port(&p2.target(), &port.target()));
        drop_port(&port);
    }

    /// Cycle guards: every node in a self-referential graph is visited once,
    /// so both traversals terminate instead of overflowing the stack.
    #[test]
    fn make_shareable_handles_a_self_referential_array() {
        let arr = crate::array_new(vec![RubyValue::Int(1)]);
        crate::array_push(&arr, RubyValue::Array(arr.clone()));
        let v = RubyValue::Array(arr.clone());

        make_shareable(&v).unwrap();
        assert!(arr.is_frozen());
        assert!(
            shareable(&v),
            "a frozen cycle is shareable (every node frozen)"
        );
    }

    #[test]
    fn make_shareable_handles_a_self_referential_hash() {
        let h = crate::hash_new(vec![(
            RubyValue::Symbol(crate::Symbol::intern("k")),
            RubyValue::Int(1),
        )]);
        crate::hash_set(
            &h,
            RubyValue::Symbol(crate::Symbol::intern("me")),
            RubyValue::Hash(h.clone()),
        );
        let v = RubyValue::Hash(h.clone());

        make_shareable(&v).unwrap();
        assert!(h.is_frozen());
        assert!(shareable(&v));
    }

    /// The predicate must also TERMINATE (not just avoid wrong answers) on
    /// an unfrozen cycle -- and answer false, since the nodes are unfrozen.
    #[test]
    fn shareable_terminates_and_rejects_an_unfrozen_cycle() {
        let arr = crate::array_new(vec![RubyValue::Int(1)]);
        crate::array_push(&arr, RubyValue::Array(arr.clone()));

        assert!(!shareable(&RubyValue::Array(arr)));
    }

    /// A cycle that runs THROUGH two containers (array -> hash -> array),
    /// not just direct self-reference.
    #[test]
    fn make_shareable_handles_a_cross_container_cycle() {
        let arr = crate::array_new(vec![RubyValue::Int(1)]);
        let h = crate::hash_new(vec![(
            RubyValue::Symbol(crate::Symbol::intern("back")),
            RubyValue::Array(arr.clone()),
        )]);
        crate::array_push(&arr, RubyValue::Hash(h.clone()));
        let v = RubyValue::Array(arr.clone());

        make_shareable(&v).unwrap();
        assert!(arr.is_frozen());
        assert!(h.is_frozen());
        assert!(shareable(&v));
    }

    /// The port pipeline end to end: default-port send feeds a spawned
    /// ractor's `Ractor.receive`, its final value comes back via `#value`.
    #[test]
    fn send_receive_and_value_through_the_default_port() {
        let r = spawn_ractor(
            |_| {
                let v = ractor_receive()?;
                let n = match v {
                    RubyValue::Int(n) => n,
                    other => panic!("expected an Int payload, got {}", other.to_display_string()),
                };
                Ok(RubyValue::Int(n * 10))
            },
            vec![],
        );
        ractor_send(&r, &RubyValue::Int(7)).unwrap();
        let out = ractor_value(&r).unwrap();
        assert_eq!(out.to_display_string(), "70");
        // The same caller may re-take (successor slot already ours).
        assert_eq!(ractor_value(&r).unwrap().to_display_string(), "70");
        assert_eq!(r.status.load(Ordering::SeqCst), STATUS_TERMINATED);
    }

    /// `#join` parks until termination and leaves the value takeable.
    #[test]
    fn join_then_value() {
        let r = spawn_ractor(|_| Ok(RubyValue::Int(42)), vec![]);
        ractor_join(&r).unwrap();
        assert_eq!(ractor_value(&r).unwrap().to_display_string(), "42");
    }

    /// A named port created by this (main) ractor round-trips a message
    /// fed from a spawned ractor -- the cross-ractor send path.
    #[test]
    fn a_port_is_fed_cross_ractor() {
        let me = current_ractor();
        let port = create_port(&me);
        let r = spawn_ractor(
            move |args| {
                let port = as_port(&args[0]).expect("a port arg");
                port_send(&port, &RubyValue::Int(31))?;
                Ok(RubyValue::Nil)
            },
            vec![port_value(port.clone())],
        );
        let got = port_receive(&port).unwrap();
        assert_eq!(got.to_display_string(), "31");
        ractor_join(&r).unwrap();
        drop_port(&port);
    }

    /// `monitor` on a live ractor delivers `:exited` at termination;
    /// on an already-terminated one it answers false and still delivers.
    #[test]
    fn monitor_tokens() {
        let me = current_ractor();
        let r = spawn_ractor(|_| Ok(RubyValue::Nil), vec![]);
        let port = create_port(&me);
        // Whichever side of termination we landed on, the token arrives.
        let registered = ractor_monitor(&r, &port);
        let token = port_receive(&port).unwrap();
        assert_eq!(token.to_display_string(), "exited");
        drop_port(&port);

        // Now definitely terminated: monitor reports false and delivers
        // immediately.
        ractor_join(&r).unwrap();
        let port2 = create_port(&me);
        assert!(!ractor_monitor(&r, &port2));
        assert_eq!(port_receive(&port2).unwrap().to_display_string(), "exited");
        drop_port(&port2);
        let _ = registered;
    }

    /// `select` over a ready port answers `[port, obj]` in argument order.
    #[test]
    fn select_pops_the_first_ready_port() {
        let me = current_ractor();
        let a = create_port(&me);
        let b = create_port(&me);
        port_send(&b, &RubyValue::Int(5)).unwrap();
        let out = ractor_select(&[port_value(a.clone()), port_value(b.clone())]).unwrap();
        let arr = out.as_array_unchecked();
        let items = arr.lock();
        assert!(as_port(&items[0]).is_some_and(|p| same_port(&p.target(), &b.target())));
        assert_eq!(items[1].to_display_string(), "5");
        drop(items);
        drop_port(&a);
        drop_port(&b);
    }

    /// `select` over a Ractor argument answers `[ractor, value]`.
    #[test]
    fn select_takes_a_terminated_ractors_value() {
        let r = spawn_ractor(|_| Ok(RubyValue::Int(9)), vec![]);
        let out = ractor_select(&[RubyValue::Ractor(r.clone())]).unwrap();
        let arr = out.as_array_unchecked();
        let items = arr.lock();
        assert!(matches!(&items[0], RubyValue::Ractor(got) if Arc::ptr_eq(got, &r)));
        assert_eq!(items[1].to_display_string(), "9");
    }

    /// Ractor-locals live per ractor and `local_key` interns strings.
    #[test]
    fn ractor_locals() {
        let me = current_ractor();
        let key = Symbol::intern("unit_test_key");
        me.locals.lock().insert(key, RubyValue::Int(3));
        assert_eq!(me.locals.lock().get(&key).unwrap().to_display_string(), "3");
        assert_eq!(local_key(&RubyValue::Symbol(key)).unwrap(), key);
        assert_eq!(
            local_key(&RubyValue::Str(crate::string_new("unit_test_key".into()))).unwrap(),
            key
        );
    }

    /// A closed, drained port raises `Ractor::ClosedError` -- which the
    /// registry-less unit-test harness surfaces as a panic.
    #[test]
    #[should_panic(expected = "Ractor::ClosedError")]
    fn a_drained_closed_port_raises_closed_error() {
        let me = current_ractor();
        let port = create_port(&me);
        port_send(&port, &RubyValue::Int(1)).unwrap();
        port_close(&port).unwrap();
        // The queued item survives the close...
        assert_eq!(port_receive(&port).unwrap().to_display_string(), "1");
        assert!(port_closed(&port));
        // ...and the drained port is closed for good.
        let _ = port_receive(&port);
    }

    #[test]
    #[should_panic(expected = "Ractor::ClosedError")]
    fn sending_to_a_closed_port_raises_closed_error() {
        let me = current_ractor();
        let port = create_port(&me);
        port_close(&port).unwrap();
        let _ = port_send(&port, &RubyValue::Int(1));
    }

    /// `#inspect` composes id, optional name/loc, and the status word.
    #[test]
    fn inspect_shape() {
        let data = Arc::new(RactorData::build(
            77,
            Some("worker".to_string()),
            Some("x.rb:3".to_string()),
        ));
        assert_eq!(ractor_inspect(&data), "#<Ractor:#77 worker x.rb:3 created>");
        data.status.store(STATUS_TERMINATED, Ordering::SeqCst);
        assert_eq!(
            ractor_inspect(&data),
            "#<Ractor:#77 worker x.rb:3 terminated>"
        );
    }
}
