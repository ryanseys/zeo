//! The collection pass: root-free refcount reconciliation over the
//! allocation registry.
//!
//! # The question it answers
//!
//! For every registered node, "is this node's reference count fully
//! explained by references from inside the registry?" A node whose count is
//! NOT fully explained is referenced by something the walk cannot see -- a
//! Cranelift stack slot, a constant cache, a global, a release pool, a
//! suspended fiber's stack, a Rust local in a frame further up -- so it is
//! live, and so is everything it reaches. Whatever is left over is
//! unreachable from anywhere, cycles included.
//!
//! This is Bacon-Rajan's trial-deletion TEST with the candidate set supplied
//! by the registry rather than by a decrement hook, which is what makes it
//! buildable here: decrements are implicit `Arc::drop` from everywhere in
//! the runtime and cannot be hooked without replacing `Arc` throughout.
//!
//! # Why it CLEARS rather than frees
//!
//! The pass never calls a deallocator. It empties containers and nils ivars,
//! and ordinary refcounting then drops everything in dependency order,
//! running real `Drop` impls. If the edge enumeration is ever wrong in the
//! unsafe direction the worst outcome is a Ruby-visible bug -- an object
//! mysteriously emptied -- and never a use-after-free. In a runtime this size
//! that asymmetry is worth more than any performance argument.
//!
//! The debug self-check turns the whole golden corpus into an over-counting
//! detector: once the sweep is done, a node it reclaimed must have exactly
//! one owner left (the pass's own handle). More than that means an edge was
//! reported and not released.
//!
//! # What it cannot reach
//!
//! Only the kinds the pass can act on are registered at all: an Array, a
//! Hash, an object, and the cell a block shares with the scope it closed
//! over. A `Str` owns no `RubyValue` and can never be in a cycle. A `Range`
//! and a `Proc` own several, but through immutable fields with nothing to
//! release them through -- reporting an edge a sweep cannot clear would let
//! the pass reclaim a node that edge still points at, so they report nothing
//! and a cycle running through one survives. `Fiber`, `Enumerator`, `Thread`
//! and `Ractor` hold state on native stacks or another OS thread.
//!
//! An object pinned by a side table is permanently live by construction and
//! shows up here as "externally referenced": anything with an ivar written on
//! a bare value (`value_ivars` pins every owner so an address can never be
//! reused) and anything carrying a per-object singleton.

use super::registry;
use crate::{FMap, RubyValue};

/// Run one collection. Answers how many nodes it reclaimed.
///
/// A no-op unless the registry is recording and this is the only Ruby
/// thread: the multi-thread rendezvous is not built yet, and abandoning a
/// collection is always safe -- it leaks exactly what leaks today.
pub fn collect() -> usize {
    if !super::recording() || !crate::gvl::sole_thread() {
        return 0;
    }
    let nodes = registry::snapshot();
    if nodes.is_empty() {
        return 0;
    }

    let mut index: FMap<usize, usize> = FMap::with_capacity_and_hasher(nodes.len(), <_>::default());
    for (i, n) in nodes.iter().enumerate() {
        index.insert(n.addr(), i);
    }

    // Every owner count is read BEFORE any edge is cloned, so the walk's own
    // temporary handles cannot be mistaken for a program's references.
    // Minus one for the handle this pass holds.
    let mut unexplained: Vec<isize> = nodes.iter().map(|n| n.owners() as isize - 1).collect();

    // The graph is held flat -- one run of targets per node, indexed by a
    // start offset -- rather than as a `Vec` per node. A real program's heap
    // has hundreds of thousands of nodes, and a `Vec` each would be that
    // many tiny allocations, costing far more in memory and allocator churn
    // than the graph itself.
    let mut edge_starts: Vec<u32> = Vec::with_capacity(nodes.len() + 1);
    let mut edge_targets: Vec<u32> = Vec::with_capacity(nodes.len());
    let mut buf: Vec<RubyValue> = Vec::new();
    for node in &nodes {
        edge_starts.push(edge_targets.len() as u32);
        buf.clear();
        node.gc_visit(&mut buf, false);
        for target in buf.drain(..).filter_map(|v| value_addr(&v)) {
            if let Some(&t) = index.get(&target) {
                unexplained[t] -= 1;
                edge_targets.push(t as u32);
            }
        }
    }
    edge_starts.push(edge_targets.len() as u32);
    let targets_of = |i: usize| {
        let (lo, hi) = (edge_starts[i] as usize, edge_starts[i + 1] as usize);
        &edge_targets[lo..hi]
    };

    // A count driven below zero means an edge was reported that is not an
    // owned reference, or one reference was reported twice -- the one
    // direction that can reclaim a live node. Checked before anything is
    // cleared, so the corpus reports the bug rather than the corruption.
    #[cfg(debug_assertions)]
    for (i, n) in unexplained.iter().enumerate() {
        assert!(
            *n >= 0,
            "gc: node {i} has {n} unexplained owners -- an edge was reported \
             that is not an owned reference, or one was reported twice"
        );
    }

    // A node with an owner the registry cannot account for is reachable from
    // outside it, and so is everything it reaches.
    let mut live = vec![false; nodes.len()];
    let mut stack: Vec<usize> = (0..nodes.len()).filter(|&i| unexplained[i] > 0).collect();
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut live[i], true) {
            continue;
        }
        stack.extend(targets_of(i).iter().map(|&t| t as usize));
    }

    let mut drained: Vec<RubyValue> = Vec::new();
    let mut reclaimed = 0;
    for (i, node) in nodes.iter().enumerate() {
        if !live[i] {
            node.gc_visit(&mut drained, true);
            reclaimed += 1;
        }
    }
    // Released outside every guard the sweep held: a drop cascades.
    drop(drained);

    // `ZEO_RT_GCSTATS=1` reports one line per collection, beside
    // `ZEO_RT_LEAKCHECK`'s counters. A pass that reclaims nothing looks
    // exactly like one that never ran, and telling those apart from outside
    // the process is otherwise guesswork.
    if std::env::var_os("ZEO_RT_GCSTATS").is_some() {
        eprintln!(
            "gc: nodes={} edges={} live={} reclaimed={reclaimed}",
            nodes.len(),
            edge_targets.len(),
            nodes.len() - reclaimed,
        );
    }

    #[cfg(debug_assertions)]
    for (i, node) in nodes.iter().enumerate() {
        assert!(
            live[i] || node.owners() == 1,
            "gc: a reclaimed node still has {} owners -- an edge was counted \
             but not released",
            node.owners()
        );
    }
    reclaimed
}

/// The identity an edge points at, for the value kinds the registry records.
/// Anything else is not a candidate and needs no lookup.
fn value_addr(v: &RubyValue) -> Option<usize> {
    use std::sync::Arc;
    Some(match v {
        RubyValue::Array(a) => Arc::as_ptr(a) as *const () as usize,
        RubyValue::Hash(h) => Arc::as_ptr(h) as *const () as usize,
        RubyValue::Object(o) => Arc::as_ptr(o) as *const () as usize,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::array_new;
    use std::sync::{Arc, Weak};

    /// The registry and the sole-thread claim are process-global; nextest
    /// gives each test its own process, which is the gate this crate runs
    /// under (`cargo test` shares one and would let them collide).
    fn armed() {
        crate::gvl::mark_sole_thread();
        crate::gc::start_recording();
    }

    /// Two arrays pointing at each other, both otherwise unreferenced.
    fn garbage_cycle() -> (
        Weak<crate::collections::Freezable<crate::collections::ArrayStore>>,
        Weak<crate::collections::Freezable<crate::collections::ArrayStore>>,
    ) {
        let a = array_new(Vec::new());
        let b = array_new(Vec::new());
        a.lock().push(RubyValue::Array(b.clone()));
        b.lock().push(RubyValue::Array(a.clone()));
        (Arc::downgrade(&a), Arc::downgrade(&b))
    }

    #[test]
    fn a_cycle_outlives_refcounting_and_the_collector_reclaims_it() {
        armed();
        let (a, b) = garbage_cycle();
        assert!(
            a.upgrade().is_some() && b.upgrade().is_some(),
            "refcounting alone never reclaims a cycle -- that is the premise"
        );
        assert_eq!(collect(), 2);
        assert!(a.upgrade().is_none() && b.upgrade().is_none());
    }

    #[test]
    fn a_cycle_something_still_holds_survives() {
        armed();
        let keeper = array_new(Vec::new());
        let (a, b) = garbage_cycle();
        keeper.lock().push(RubyValue::Array(a.upgrade().unwrap()));
        collect();
        assert!(
            a.upgrade().is_some() && b.upgrade().is_some(),
            "an owner the registry cannot account for makes a node live, and \
             everything it reaches with it"
        );
        assert_eq!(
            keeper.lock().len(),
            1,
            "the survivor is intact, not emptied"
        );
    }

    #[test]
    fn a_reachable_acyclic_graph_is_left_alone() {
        armed();
        let leaf = array_new(vec![RubyValue::Int(7)]);
        let root = array_new(vec![RubyValue::Array(leaf.clone())]);
        collect();
        assert_eq!(leaf.lock().len(), 1);
        assert_eq!(root.lock().len(), 1);
    }

    #[test]
    fn nothing_is_reclaimed_while_the_registry_is_not_recording() {
        crate::gvl::mark_sole_thread();
        let (a, b) = garbage_cycle();
        assert_eq!(collect(), 0);
        assert!(a.upgrade().is_some() && b.upgrade().is_some());
    }
}
