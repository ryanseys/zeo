//! The allocation registry: every heap node this process has built, held
//! weakly, in the order it was built.
//!
//! # Shape
//!
//! Each thread bump-pushes into a local chunk; a full chunk is sealed into
//! [`CHUNKS`] and a fresh one started. Nothing is ever removed on the
//! allocation path -- the registry is append-only there, which is what keeps
//! the cost to a push.
//!
//! A `Weak` keeps the ALLOCATION alive after the value inside it is dropped
//! (that is what makes `upgrade` able to answer `None` rather than read freed
//! memory), so a registry that never forgets a dead entry retains one empty
//! block per object the program has ever made. [`compact`] is what stops
//! that: it drops the dead entries, and it runs when the registry has doubled
//! since the last compaction, so it is amortized O(1) per entry -- the same
//! bargain a `Vec`'s growth makes.
//!
//! # Why the node kind is carried
//!
//! The collector has to get from a weak handle back to a `RubyValue` to ask
//! the object what it points at, and a `Weak<Freezable<ArrayStore>>` and a
//! `Weak<Freezable<RHashData>>` are different types. The tag IS the way back,
//! exactly as it is for `RubyValue` itself.

use crate::RubyValue;
use crate::collections::{ArrayStore, Freezable, RHashData};
use crate::dispatch::RubyObject;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::sync::Weak;

/// One registered node, held weakly. The variants are exactly the heap
/// `RubyValue` payloads a program allocates in bulk; a value kind absent
/// here is simply never a candidate, which the collector reads as "live".
pub(crate) enum Node {
    Array(Weak<Freezable<ArrayStore>>),
    Hash(Weak<Freezable<RHashData>>),
    Object(Weak<dyn RubyObject>),
    Cell(Weak<Mutex<RubyValue>>),
}

impl Node {
    /// Whether the node is still alive, without building a value for it.
    /// The collector's way back to a `RubyValue` arrives with the collector.
    fn is_live(&self) -> bool {
        match self {
            Node::Array(w) => w.strong_count() > 0,
            Node::Hash(w) => w.strong_count() > 0,
            Node::Object(w) => w.strong_count() > 0,
            Node::Cell(w) => w.strong_count() > 0,
        }
    }
}

/// A registered node, held strongly for the duration of one collection.
///
/// The kind decides two things the collector cannot ask a bare pointer:
/// which references the node OWNS, and whether it can release them.
pub(crate) enum Strong {
    Array(crate::collections::RArray),
    Hash(crate::collections::RHash),
    Object(crate::dispatch::RObj),
    Cell(crate::LocalCell),
}

impl Strong {
    /// This node's identity: the payload address, which is what an edge
    /// pointing at it also spells.
    pub(crate) fn addr(&self) -> usize {
        match self {
            Strong::Array(a) => std::sync::Arc::as_ptr(a) as *const () as usize,
            Strong::Hash(h) => std::sync::Arc::as_ptr(h) as *const () as usize,
            Strong::Object(o) => std::sync::Arc::as_ptr(o) as *const () as usize,
            Strong::Cell(c) => std::sync::Arc::as_ptr(c) as *const () as usize,
        }
    }

    /// How many owners this node has, INCLUDING the handle held here. Every
    /// reference compiled code holds is one of these, which is what makes
    /// the count a sound input: no capi entry hands out a borrow into a
    /// container, so there is no reference the count does not see.
    pub(crate) fn owners(&self) -> usize {
        match self {
            Strong::Array(a) => std::sync::Arc::strong_count(a),
            Strong::Hash(h) => std::sync::Arc::strong_count(h),
            Strong::Object(o) => std::sync::Arc::strong_count(o),
            Strong::Cell(c) => std::sync::Arc::strong_count(c),
        }
    }

    /// See [`crate::dispatch::RubyObject::gc_visit`] for the contract, which
    /// is the same one here: clone for the walk, move out for the sweep, and
    /// report nothing you cannot release.
    pub(crate) fn gc_visit(&self, out: &mut Vec<RubyValue>, take: bool) {
        match self {
            Strong::Array(a) => a.lock().gc_visit(out, take),
            Strong::Hash(h) => h.lock().gc_visit(out, take),
            Strong::Object(o) => o.gc_visit(out, take),
            Strong::Cell(c) => {
                let mut g = c.lock();
                out.push(if take {
                    std::mem::replace(&mut *g, RubyValue::Nil)
                } else {
                    g.clone()
                });
            }
        }
    }
}

impl Node {
    /// The node this handle names, or `None` if it has already been dropped.
    fn upgrade(&self) -> Option<Strong> {
        match self {
            Node::Array(w) => w.upgrade().map(Strong::Array),
            Node::Hash(w) => w.upgrade().map(Strong::Hash),
            Node::Object(w) => w.upgrade().map(Strong::Object),
            Node::Cell(w) => w.upgrade().map(Strong::Cell),
        }
    }
}

/// Every live node, one handle each, in allocation order. Publishes this
/// thread's partial chunk first: a node still sitting in a buffer is
/// invisible to the walk, which only ever costs a cycle one more collection
/// to be noticed.
pub(crate) fn snapshot() -> Vec<Strong> {
    flush_local();
    let chunks = CHUNKS.lock();
    let mut seen = crate::FMap::default();
    let mut out = Vec::new();
    for node in chunks.iter().flat_map(|c| c.iter()) {
        let Some(strong) = node.upgrade() else {
            continue;
        };
        // A constructor that registered twice must not become two candidates:
        // the second handle would inflate the node's own owner count and make
        // it look externally referenced forever.
        if seen.insert(strong.addr(), ()).is_none() {
            out.push(strong);
        }
    }
    out
}

/// Publish this thread's partial chunk.
fn flush_local() {
    let _ = LOCAL.try_with(|l| {
        let taken = std::mem::take(&mut l.borrow_mut().0);
        publish(taken);
    });
}

/// How many nodes a thread buffers before publishing. Big enough that the
/// global lock is touched once per thousands of allocations, small enough
/// that a thread parked at a safepoint hides only a bounded number of
/// candidates from a collection in progress.
const CHUNK: usize = 4096;

/// Every published chunk, oldest first.
static CHUNKS: Mutex<Vec<Vec<Node>>> = Mutex::new(Vec::new());

/// Entries published since the last [`compact`], plus what survived it --
/// the threshold that decides when compaction is worth its scan.
static COMPACT_AT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(2 * CHUNK);
/// Total entries currently in [`CHUNKS`], live or dead.
static PUBLISHED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// This thread's unpublished chunk. Dropped on thread exit, which publishes
/// whatever it still holds -- `CHUNKS` is a `static`, so it outlives every
/// thread-local destructor that reaches it.
struct Local(Vec<Node>);

impl Drop for Local {
    fn drop(&mut self) {
        publish(std::mem::take(&mut self.0));
    }
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local(Vec::with_capacity(CHUNK)));
}

/// Append one node. The caller has already checked the gate.
#[inline]
pub(crate) fn record(node: Node) {
    // A thread-local access can fail during that thread's own teardown, once
    // this key's destructor has run. Dropping the record is right there: the
    // program is past the point where anything it allocates can be collected.
    let _ = LOCAL.try_with(|l| {
        let mut l = l.borrow_mut();
        l.0.push(node);
        if l.0.len() >= CHUNK {
            publish(std::mem::replace(&mut l.0, Vec::with_capacity(CHUNK)));
        }
    });
}

/// Move a sealed chunk into the global registry, compacting when the
/// registry has doubled since it was last swept.
fn publish(chunk: Vec<Node>) {
    use std::sync::atomic::Ordering::Relaxed;
    if chunk.is_empty() {
        return;
    }
    let n = chunk.len();
    let mut chunks = CHUNKS.lock();
    chunks.push(chunk);
    let total = PUBLISHED.fetch_add(n, Relaxed) + n;
    if total >= COMPACT_AT.load(Relaxed) {
        let live = compact_locked(&mut chunks);
        PUBLISHED.store(live, Relaxed);
        COMPACT_AT.store((live * 2).max(2 * CHUNK), Relaxed);
    }
}

/// Drop every dead entry, keeping allocation order. Answers what is left.
fn compact_locked(chunks: &mut Vec<Vec<Node>>) -> usize {
    let mut live = 0;
    for chunk in chunks.iter_mut() {
        chunk.retain(Node::is_live);
        live += chunk.len();
    }
    chunks.retain(|c| !c.is_empty());
    live
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RubyValue;
    use crate::collections::array_new;

    /// How many registered nodes are still alive. Test-only until the
    /// collector reads the registry for real.
    fn live_count() -> usize {
        flush_local();
        CHUNKS
            .lock()
            .iter()
            .flat_map(|c| c.iter())
            .filter(|n| n.is_live())
            .count()
    }

    #[test]
    fn the_gate_is_off_by_default_and_records_nothing() {
        assert!(!crate::gc::recording());
        let before = live_count();
        let a = array_new(vec![RubyValue::Int(1)]);
        assert_eq!(live_count(), before, "an unrecorded array is not a node");
        drop(a);
    }

    #[test]
    fn a_recorded_node_leaves_the_registry_when_it_dies() {
        crate::gc::start_recording();
        let before = live_count();
        let a = array_new(vec![RubyValue::Int(1)]);
        assert_eq!(live_count(), before + 1);
        drop(a);
        assert_eq!(live_count(), before, "a dropped array is no longer live");
        crate::gc::stop_recording();
    }

    #[test]
    fn compaction_drops_dead_entries_rather_than_growing_forever() {
        crate::gc::start_recording();
        for _ in 0..(8 * CHUNK) {
            drop(array_new(Vec::new()));
        }
        crate::gc::stop_recording();
        // Every one of them died immediately, so the registry must have swept
        // them rather than kept 32k weak handles (and the empty allocations
        // they pin) around.
        assert!(
            PUBLISHED.load(std::sync::atomic::Ordering::Relaxed) < 4 * CHUNK,
            "registry did not compact"
        );
    }
}
