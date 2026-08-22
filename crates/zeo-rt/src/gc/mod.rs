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

/// Record a freshly built captured local -- the cell a block shares with the
/// scope it closed over, and the one place a cycle can run through a local
/// rather than through an object graph.
#[inline]
pub fn record_cell(c: &crate::LocalCell) {
    if recording() {
        record(Node::Cell(std::sync::Arc::downgrade(c)));
    }
}
