//! The cycle collector's machinery: an allocation registry, and (later) the
//! root-free reconciliation pass that reads it.
//!
//! Not to be confused with [`crate::builtins::gc`], which is the Ruby-visible
//! `GC` module's rows. This module is the mechanism those rows will drive.
//!
//! # Why a registry at all
//!
//! zeo is `Arc`-refcounted, so a cycle's counts never reach zero and the
//! objects in it are never dropped. That is a property of reference counting,
//! not a zeo bug: Rust has no collector, its answer for shared ownership is
//! `Arc`, and `Arc` leaks cycles by definition.
//!
//! A tracing mark-and-sweep needs every ROOT, and under THIS lowering it
//! cannot have them. Five independent strong-reference holders: values
//! Cranelift keeps in stack slots, the program's own per-site constant
//! caches, a `Proc` built from a Rust closure, a suspended fiber's native
//! stack, and the frame release pools.
//!
//! Only two of those five are hard. Cranelift supports user stack maps
//! (`declare_var_needs_stack_map`, a safepoint at every non-tail call), and
//! the constant caches and release pools are runtime tables that could
//! register themselves. What stays out of reach is a Rust closure's captures
//! -- which no reflection can see, and which is why a `Proc` is the one node
//! kind here that owns references nothing can enumerate -- and a fiber
//! suspended on a native stack. So tracing is a much larger project rather
//! than an impossible one, and its real prize would not be cycles: it would
//! be retiring the atomic refcount traffic every value copy pays today.
//!
//! What IS possible is the other family of algorithms: Bacon-Rajan's trial
//! deletion, which needs no roots at all. It asks, of a candidate set, "is
//! this node's reference count fully explained by references from inside the
//! set?" -- and every reference compiled code holds is a real `Arc` strong
//! count, because no capi entry hands out a borrow into a container. So the
//! counts ARE the input the algorithm wants; the one thing missing is the
//! candidate set, which is what this registry supplies.
//!
//! # The gate
//!
//! Recording is OFF by default and costs one relaxed load of [`RECORDING`]
//! per allocation when it is. Every `record_*` entry below reads the gate
//! BEFORE it builds a handle, so a program that never opts in pays no
//! `Arc::downgrade` and touches no thread-local.

mod collect;
pub(crate) mod registry;

pub use collect::collect;

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether allocations are recorded. Read once per allocation; see the
/// module docs for why the gate is checked before any handle is built.
static RECORDING: AtomicBool = AtomicBool::new(false);

/// A collection is DUE: the registry has grown past its threshold and the
/// next safepoint should run one.
///
/// It is a request, not an action, and that separation is the whole point.
/// An allocation can happen inside a container's own guard -- growing a Hash
/// while its lock is held -- and collecting there hands the pass a locked
/// node it has to read. The registry only ever ARMS this; `check_ints`, which
/// holds no guard by construction, is where it fires.
static DUE: AtomicBool = AtomicBool::new(false);

/// Ask for a collection at the next safepoint. Called from the registry when
/// it has grown enough, never from a `record_*` fast path.
pub(crate) fn arm_collection() {
    if !DUE.swap(true, Ordering::Relaxed) {
        // The `check_ints` fast path is one relaxed load of the interrupt
        // counter; a request has to move it or nothing will look.
        crate::gvl::note_posted();
    }
}

/// A safepoint: run the collection the registry asked for, if any.
///
/// Claimed with a swap, so a re-entrant allocation during the pass -- the
/// snapshot's own `Vec`, or a `Drop` that builds something -- re-arms for the
/// NEXT safepoint rather than recursing into this one.
pub(crate) fn service_due() {
    if !DUE.swap(false, Ordering::Relaxed) {
        return;
    }
    crate::gvl::note_consumed();
    // The SAME entry an explicit `GC.start` takes, so the two cannot drift:
    // `GC.disable` gates both, both bump `GC.count`, and both sweep the
    // finalizers a reclaimed cycle just made due. CRuby counts its automatic
    // collections too.
    crate::builtins::gc::run_collection();
}

/// Whether this process records allocations.
#[inline]
pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// Start recording. Called once at startup from `zeo_rt_main` when the
/// program asks for a collector; objects allocated before this point are
/// never candidates, which only ever means they are treated as live.
pub fn start_recording() {
    RECORDING.store(true, Ordering::Relaxed);
}

/// Arm the registry when the program asks for it. `ZEO_GC=1` is the whole
/// surface for now: recording costs one relaxed load per allocation when it
/// is off, so the default stays off until the collector that reads the
/// registry is worth its price.
pub fn configure_from_env() {
    if std::env::var_os("ZEO_GC").is_some_and(|v| v != "0" && v != "") {
        start_recording();
    }
}

/// Stop recording, keeping what is already registered. Only the tests use
/// this -- a program that arms a collector keeps it armed.
#[cfg(test)]
pub(crate) fn stop_recording() {
    RECORDING.store(false, Ordering::Relaxed);
}

use crate::collections::{RArray, RHash};
use crate::dispatch::RObj;
use registry::{Node, record};

/// Record a freshly built Array.
#[inline]
pub fn record_array(a: &RArray) {
    if recording() {
        record(Node::Array(std::sync::Arc::downgrade(a)));
    }
}

/// Record a freshly built Hash.
#[inline]
pub fn record_hash(h: &RHash) {
    if recording() {
        record(Node::Hash(std::sync::Arc::downgrade(h)));
    }
}

/// Record a freshly allocated object -- a compiled class's instance, a
/// `Class.new` instance, or one of the runtime's own.
#[inline]
pub fn record_object(o: &RObj) {
    if recording() {
        record(Node::Object(std::sync::Arc::downgrade(o)));
    }
}

/// Record a freshly built `Proc`. Its captured cells are the one channel a
/// closure can close a cycle through, and no reflection reaches inside an
/// `Arc<dyn Fn>` -- which is why [`crate::rproc::ProcBuilder`] exists.
#[inline]
pub fn record_proc(p: &crate::RProc) {
    if recording() {
        record(Node::Proc(p.downgrade()));
    }
}

/// Every live registered node as a Ruby value, in allocation order --
/// `ObjectSpace.each_object`'s walk. Empty when nothing is recording, which
/// the caller must tell apart from "the heap is empty".
#[must_use]
pub fn live_values() -> Vec<crate::RubyValue> {
    if !recording() {
        return Vec::new();
    }
    registry::snapshot()
        .iter()
        .filter_map(registry::Strong::value)
        .collect()
}

/// `GC.stat[:heap_live_slots]`.
#[must_use]
pub fn live_count() -> usize {
    registry::live_count()
}

/// `GC.stat[:total_allocated_objects]`.
#[must_use]
pub fn total_allocated() -> u64 {
    registry::total_allocated()
}

/// Record a freshly built `Range`, which the caller has already checked can
/// hold a heap endpoint. A Range is immutable, so the sweep cannot clear its
/// endpoints -- it reports them anyway, for the reason [`crate::RProc`] does.
#[inline]
pub fn record_range(r: &crate::builtins::range::RRange) {
    if recording() {
        record(Node::Range(std::sync::Arc::downgrade(r)));
    }
}

/// Whether `v` is a heap value, and therefore something an edge can point at.
/// An immediate is a 24-byte copy that owns nothing.
#[inline]
#[must_use]
pub fn can_close_a_cycle(v: &crate::RubyValue) -> bool {
    // The tag byte sits at offset 0 -- the `abi_layout` test pins it.
    let tag = unsafe { *(v as *const crate::RubyValue).cast::<u8>() };
    tag >= zeo_abi::abi::FIRST_HEAP_TAG
}

/// Record a freshly built captured local -- the cell a block shares with the
/// scope it closed over, and the one place a cycle can run through a local
/// rather than through an object graph.
#[inline]
pub fn record_cell(c: &crate::LocalCell) {
    if recording() {
        record(Node::Cell(std::sync::Arc::downgrade(c)));
    }
}

/// `ZEO_RT_GCCHECK=1`: run one last collection at exit and report what it
/// found as `cycle leak: N objects`.
///
/// Anything reclaimed HERE is a cycle the program built and never collected:
/// by this point `at_exit` has run and the finalizers have swept, so a node
/// the pass can prove unreachable was leaked for the program's whole life.
/// That turns every golden into a cycle-leak regression test, which is the
/// only way to notice the shape arriving -- a leaked cycle changes no output.
pub fn check_at_exit() {
    if !std::env::var_os("ZEO_RT_GCCHECK").is_some_and(|v| !v.is_empty()) {
        return;
    }
    // The check needs the registry, and arming it at exit records nothing
    // that was allocated before, so it has to have been on all along.
    if !recording() {
        eprintln!(
            "ZEO_RT_GCCHECK: nothing to check -- the allocation registry was never armed (ZEO_GC=1)"
        );
        return;
    }
    let leaked = collect();
    if leaked > 0 {
        eprintln!("cycle leak: {leaked} objects");
    }
}
