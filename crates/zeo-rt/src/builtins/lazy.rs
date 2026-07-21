//! `Enumerator::Lazy` (CRuby's `enumerator.c` lazy layer) -- a chain of
//! transformations that only runs on demand, so it works over infinite
//! sources (`(1..Float::INFINITY).lazy.select(&:even?).first(3)`).
//!
//! Shape: an immutable `RObj` holding the ORIGINAL enumerable `source` plus an
//! ordered `ops` chain. Every lazy method (`map`/`select`/...) returns a NEW
//! `RLazy` that shares the same source and appends one op -- so a lazy is
//! re-runnable and cheap to extend (a `RubyValue`/`RProc` clone is an Arc
//! handle bump). A terminal method (`first`/`to_a`/`force`/`each`) drives the
//! source through the chain via the enumerator fiber pull (`pull_next`), which
//! only advances the source as far as the terminal actually needs.
//!
//! The chain runs as a PUSH transducer: each source value flows through the
//! ops into a [`Sink`], and a [`Flow::Stop`] short-circuits the pull (what
//! makes `take`/`first`/`take_while` terminate an infinite source).

use crate::builtins::enumerator::{enumerator_for, pull_next};
use crate::builtins::{arg_error, arity, builtin_methods};
use crate::dispatch::{RObj, RubyObject};
use crate::{RProc, RubyValue, Signal, array_new};
use std::collections::HashSet;
use std::sync::Arc;
use zeo_abi::LAZY_CLASS;

/// One link in a lazy chain. Block-bearing ops store the block; `Take`/`Drop`
/// store a count; `Grep` stores its `===` pattern, whether to invert
/// (`grep_v`), and an optional map block.
enum LazyOp {
    Map(RProc),
    FlatMap(RProc),
    FilterMap(RProc),
    Select(RProc),
    Reject(RProc),
    TakeWhile(RProc),
    DropWhile(RProc),
    Take(i64),
    Drop(i64),
    Grep(RubyValue, bool, Option<RProc>),
    Uniq(Option<RProc>),
    Compact,
}

/// The per-run mutable state for the stateful ops (a fresh set is built at the
/// start of every terminal drive, so re-running a lazy starts clean).
enum OpState {
    None,
    Count(i64),
    Dropping(bool),
    Seen(HashSet<crate::collections::HashKey>),
}

impl OpState {
    fn for_op(op: &LazyOp) -> OpState {
        match op {
            LazyOp::Take(_) | LazyOp::Drop(_) => OpState::Count(0),
            LazyOp::DropWhile(_) => OpState::Dropping(true),
            LazyOp::Uniq(_) => OpState::Seen(HashSet::new()),
            _ => OpState::None,
        }
    }
}

pub struct RLazy {
    source: RubyValue,
    ops: Vec<LazyOp>,
}

impl RubyObject for RLazy {
    fn class_id(&self) -> crate::ClassId {
        LAZY_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RLazy {
            source: self.source.clone(),
            ops: clone_ops(&self.ops),
        })
    }
}

/// The lazy source's element count, routed through an `each` enumerator so
/// every source kind answers by the one `Enumerator#size` rule (an endless
/// Range gives Float::INFINITY; an unsized source gives nil).
fn source_size(source: &RubyValue) -> RubyValue {
    let e = enumerator_for(source, "each", &[]);
    crate::dispatch::send_value(&e, crate::Symbol::intern("size"), &[], None)
        .unwrap_or(RubyValue::Nil)
}

/// `Enumerable#lazy` -- the entry point every enumerable dispatches to.
pub(crate) fn make_lazy(source: &RubyValue) -> RubyValue {
    RubyValue::Object(Arc::new(RLazy {
        source: source.clone(),
        ops: Vec::new(),
    }))
}

fn lazy_of(recv: &RubyValue) -> &RLazy {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RLazy>()
            .expect("the Lazy table only dispatches on Lazy receivers"),
        _ => unreachable!("the Lazy table only dispatches on Lazy receivers"),
    }
}

fn clone_ops(ops: &[LazyOp]) -> Vec<LazyOp> {
    ops.iter()
        .map(|op| match op {
            LazyOp::Map(p) => LazyOp::Map(p.clone()),
            LazyOp::FlatMap(p) => LazyOp::FlatMap(p.clone()),
            LazyOp::FilterMap(p) => LazyOp::FilterMap(p.clone()),
            LazyOp::Select(p) => LazyOp::Select(p.clone()),
            LazyOp::Reject(p) => LazyOp::Reject(p.clone()),
            LazyOp::TakeWhile(p) => LazyOp::TakeWhile(p.clone()),
            LazyOp::DropWhile(p) => LazyOp::DropWhile(p.clone()),
            LazyOp::Take(n) => LazyOp::Take(*n),
            LazyOp::Drop(n) => LazyOp::Drop(*n),
            LazyOp::Grep(pat, inv, blk) => LazyOp::Grep(pat.clone(), *inv, blk.clone()),
            LazyOp::Uniq(k) => LazyOp::Uniq(k.clone()),
            LazyOp::Compact => LazyOp::Compact,
        })
        .collect()
}

/// A new lazy that is `recv` with `op` appended.
fn extend(recv: &RubyValue, op: LazyOp) -> RubyValue {
    let l = lazy_of(recv);
    let mut ops = clone_ops(&l.ops);
    ops.push(op);
    RubyValue::Object(Arc::new(RLazy {
        source: l.source.clone(),
        ops,
    }))
}

/// A required block, or CRuby's ArgumentError shape for a lazy op missing one.
fn need_block(block: Option<RubyValue>, meth: &str) -> Result<RProc, Signal> {
    match block {
        Some(RubyValue::Proc(p)) => Ok(p),
        _ => Err(arg_error!("tried to call lazy {meth} without a block")),
    }
}

fn count_arg(v: &RubyValue, meth: &str) -> Result<i64, Signal> {
    let n = crate::builtins::convert::to_index(v)?;
    if n < 0 {
        return Err(arg_error!("attempt to {meth} negative size"));
    }
    Ok(n)
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

/// Where a driven value ends up: collected into a bounded buffer
/// (`first`/`to_a`/`force`) or handed to a block (`each`).
enum Sink<'a> {
    Collect {
        out: &'a mut Vec<RubyValue>,
        limit: Option<usize>,
    },
    Each(&'a RProc),
}

impl Sink<'_> {
    fn accept(&mut self, val: RubyValue) -> Result<Flow, Signal> {
        match self {
            Sink::Collect { out, limit } => {
                out.push(val);
                Ok(if limit.is_some_and(|l| out.len() >= l) {
                    Flow::Stop
                } else {
                    Flow::Continue
                })
            }
            Sink::Each(p) => {
                p.call(std::slice::from_ref(&val))?;
                Ok(Flow::Continue)
            }
        }
    }
}

/// Pushes one value through `ops[idx..]` into `sink`. Returns `Stop` the
/// moment nothing more should be pulled from the source.
fn push(
    ops: &[LazyOp],
    st: &mut [OpState],
    idx: usize,
    val: RubyValue,
    sink: &mut Sink,
) -> Result<Flow, Signal> {
    if idx == ops.len() {
        return sink.accept(val);
    }
    match &ops[idx] {
        LazyOp::Map(p) => {
            let v = p.call(std::slice::from_ref(&val))?;
            push(ops, st, idx + 1, v, sink)
        }
        LazyOp::Select(p) => {
            if p.call(std::slice::from_ref(&val))?.truthy() {
                push(ops, st, idx + 1, val, sink)
            } else {
                Ok(Flow::Continue)
            }
        }
        LazyOp::Reject(p) => {
            if p.call(std::slice::from_ref(&val))?.truthy() {
                Ok(Flow::Continue)
            } else {
                push(ops, st, idx + 1, val, sink)
            }
        }
        LazyOp::FilterMap(p) => {
            let v = p.call(std::slice::from_ref(&val))?;
            if v.truthy() {
                push(ops, st, idx + 1, v, sink)
            } else {
                Ok(Flow::Continue)
            }
        }
        LazyOp::FlatMap(p) => {
            let v = p.call(std::slice::from_ref(&val))?;
            match v {
                RubyValue::Array(a) => {
                    let elems = a.lock().clone();
                    for e in elems {
                        if push(ops, st, idx + 1, e, sink)? == Flow::Stop {
                            return Ok(Flow::Stop);
                        }
                    }
                    Ok(Flow::Continue)
                }
                other => push(ops, st, idx + 1, other, sink),
            }
        }
        LazyOp::Grep(pat, invert, blk) => {
            // `case_eq`, matching EAGER `grep` (`enumerable.rs`, which sends
            // `===`): the native ladder made `lazy.grep(user_pattern)` answer
            // differently from `grep(user_pattern)` on the same pattern.
            if crate::value::case_eq(pat, &val)? != *invert {
                let v = match blk {
                    Some(p) => p.call(std::slice::from_ref(&val))?,
                    None => val,
                };
                push(ops, st, idx + 1, v, sink)
            } else {
                Ok(Flow::Continue)
            }
        }
        LazyOp::Compact => {
            if val.is_nil() {
                Ok(Flow::Continue)
            } else {
                push(ops, st, idx + 1, val, sink)
            }
        }
        LazyOp::TakeWhile(p) => {
            if p.call(std::slice::from_ref(&val))?.truthy() {
                push(ops, st, idx + 1, val, sink)
            } else {
                Ok(Flow::Stop)
            }
        }
        LazyOp::DropWhile(p) => {
            let dropping = matches!(st[idx], OpState::Dropping(true));
            if dropping {
                if p.call(std::slice::from_ref(&val))?.truthy() {
                    return Ok(Flow::Continue);
                }
                st[idx] = OpState::Dropping(false);
            }
            push(ops, st, idx + 1, val, sink)
        }
        LazyOp::Take(n) => {
            let (over, last) = match &mut st[idx] {
                OpState::Count(c) if *c >= *n => (true, false),
                OpState::Count(c) => {
                    *c += 1;
                    (false, *c >= *n)
                }
                _ => unreachable!("Take state"),
            };
            if over {
                return Ok(Flow::Stop);
            }
            let flow = push(ops, st, idx + 1, val, sink)?;
            Ok(if flow == Flow::Stop || last {
                Flow::Stop
            } else {
                Flow::Continue
            })
        }
        LazyOp::Drop(n) => {
            let skip = match &mut st[idx] {
                OpState::Count(c) if *c < *n => {
                    *c += 1;
                    true
                }
                OpState::Count(_) => false,
                _ => unreachable!("Drop state"),
            };
            if skip {
                Ok(Flow::Continue)
            } else {
                push(ops, st, idx + 1, val, sink)
            }
        }
        LazyOp::Uniq(key) => {
            let probe = match key {
                Some(p) => p.call(std::slice::from_ref(&val))?,
                None => val.clone(),
            };
            let hk = crate::collections::hash_key(&probe);
            let fresh = match &mut st[idx] {
                OpState::Seen(seen) => seen.insert(hk),
                _ => unreachable!("Uniq state"),
            };
            if fresh {
                push(ops, st, idx + 1, val, sink)
            } else {
                Ok(Flow::Continue)
            }
        }
    }
}

/// Packs one source pull into a single value the block ABI expects: a lone
/// element as itself, a multi-value yield (e.g. a Hash's `[k, v]`) as an
/// Array so `{ |k, v| }` blocks auto-splat, an empty yield as `nil`.
fn pack(mut vals: Vec<RubyValue>) -> RubyValue {
    match vals.len() {
        0 => RubyValue::Nil,
        1 => vals.pop().unwrap(),
        _ => RubyValue::Array(array_new(vals)),
    }
}

/// Drives the source through the chain into `sink`, pulling only as far as the
/// sink/ops allow (a `Stop` ends the loop before the next pull).
fn drive(lazy: &RLazy, sink: &mut Sink) -> Result<(), Signal> {
    let src = enumerator_for(&lazy.source, "each", &[]);
    let RubyValue::Enumerator(e) = &src else {
        unreachable!("enumerator_for always builds an Enumerator");
    };
    let mut st: Vec<OpState> = lazy.ops.iter().map(OpState::for_op).collect();
    while let Some(vals) = pull_next(e)? {
        if push(&lazy.ops, &mut st, 0, pack(vals), sink)? == Flow::Stop {
            break;
        }
    }
    Ok(())
}

fn collect(lazy: &RLazy, limit: Option<usize>) -> Result<Vec<RubyValue>, Signal> {
    let mut out = Vec::new();
    drive(
        lazy,
        &mut Sink::Collect {
            out: &mut out,
            limit,
        },
    )?;
    Ok(out)
}

builtin_methods! {
    pub(crate) fn lookup;

    // `size` never iterates: it takes the source's size and folds the ops that
    // have a knowable effect on it. A filtering op makes the result unknown
    // (nil) -- CRuby's rule, since it can't be answered without running.
    "size" => fn size(recv, args, _block) {
        arity!(args, 0);
        let lz = lazy_of(recv);
        let mut size = source_size(&lz.source);
        for op in &lz.ops {
            size = match (op, size) {
                (LazyOp::Map(_) | LazyOp::Compact, s) => s,
                (LazyOp::Take(n), RubyValue::Int(s)) => RubyValue::Int(s.min(*n)),
                // `take` bounds even an endless source.
                (LazyOp::Take(n), RubyValue::Float(_)) => RubyValue::Int(*n),
                (LazyOp::Drop(n), RubyValue::Int(s)) => RubyValue::Int((s - n).max(0)),
                (LazyOp::Drop(_), s @ RubyValue::Float(_)) => s,
                _ => RubyValue::Nil,
            };
            if matches!(size, RubyValue::Nil) {
                break;
            }
        }
        Ok(size)
    }

    "map" | "collect" => fn map(recv, args, block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::Map(need_block(block, "map")?)))
    }
    "flat_map" | "collect_concat" => fn flat_map(recv, args, block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::FlatMap(need_block(block, "flat_map")?)))
    }
    "filter_map" => fn filter_map(recv, args, block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::FilterMap(need_block(block, "filter_map")?)))
    }
    "select" | "filter" | "find_all" => fn select(recv, args, block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::Select(need_block(block, "select")?)))
    }
    "reject" => fn reject(recv, args, block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::Reject(need_block(block, "reject")?)))
    }
    "take_while" => fn take_while(recv, args, block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::TakeWhile(need_block(block, "take_while")?)))
    }
    "drop_while" => fn drop_while(recv, args, block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::DropWhile(need_block(block, "drop_while")?)))
    }
    "take" => fn take(recv, args, _block) {
        arity!(args, 1);
        Ok(extend(recv, LazyOp::Take(count_arg(&args[0], "take")?)))
    }
    "drop" => fn drop(recv, args, _block) {
        arity!(args, 1);
        Ok(extend(recv, LazyOp::Drop(count_arg(&args[0], "drop")?)))
    }
    "grep" => fn grep(recv, args, block) {
        arity!(args, 1);
        let blk = match block { Some(RubyValue::Proc(p)) => Some(p), _ => None };
        Ok(extend(recv, LazyOp::Grep(args[0].clone(), false, blk)))
    }
    "grep_v" => fn grep_v(recv, args, block) {
        arity!(args, 1);
        let blk = match block { Some(RubyValue::Proc(p)) => Some(p), _ => None };
        Ok(extend(recv, LazyOp::Grep(args[0].clone(), true, blk)))
    }
    "uniq" => fn uniq(recv, args, block) {
        arity!(args, 0);
        let key = match block { Some(RubyValue::Proc(p)) => Some(p), _ => None };
        Ok(extend(recv, LazyOp::Uniq(key)))
    }
    "compact" => fn compact(recv, args, _block) {
        arity!(args, 0);
        Ok(extend(recv, LazyOp::Compact))
    }
    "lazy" => fn lazy(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    // Terminal operations: these run the chain.
    "first" => fn first(recv, args, _block) {
        arity!(args, 0..=1);
        match args.first() {
            None => Ok(collect(lazy_of(recv), Some(1))?.into_iter().next().unwrap_or(RubyValue::Nil)),
            Some(v) => {
                let n = count_arg(v, "take")?;
                Ok(RubyValue::Array(array_new(collect(lazy_of(recv), Some(n as usize))?)))
            }
        }
    }
    "to_a" | "force" | "entries" => fn to_a(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(array_new(collect(lazy_of(recv), None)?)))
    }
    "each" => fn each(recv, args, block) {
        arity!(args, 0);
        match block {
            Some(RubyValue::Proc(p)) => {
                drive(lazy_of(recv), &mut Sink::Each(&p))?;
                Ok(recv.clone())
            }
            // Blockless `each` on a lazy is just the lazy itself.
            _ => Ok(recv.clone()),
        }
    }
    "inspect" | "to_s" => fn inspect(recv, args, _block) {
        arity!(args, 0);
        let _ = recv;
        // CRuby renders the full source+ops chain; this stable placeholder
        // avoids an address in the output (a documented simplification).
        Ok(RubyValue::Str(crate::string_new("#<Enumerator::Lazy>".to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arr(xs: &[i64]) -> RubyValue {
        RubyValue::Array(array_new(xs.iter().map(|&i| RubyValue::Int(i)).collect()))
    }

    fn ints(v: &RubyValue) -> Vec<i64> {
        let RubyValue::Array(a) = v else {
            panic!("expected array")
        };
        a.lock()
            .iter()
            .map(|e| match e {
                RubyValue::Int(i) => *i,
                _ => panic!("expected ints"),
            })
            .collect()
    }

    fn times_two() -> RubyValue {
        RubyValue::Proc(RProc::new(|args: &[RubyValue]| {
            let RubyValue::Int(i) = args[0] else {
                unreachable!()
            };
            Ok(RubyValue::Int(i * 2))
        }))
    }

    fn is_even() -> RubyValue {
        RubyValue::Proc(RProc::new(|args: &[RubyValue]| {
            let RubyValue::Int(i) = args[0] else {
                unreachable!()
            };
            Ok(RubyValue::Bool(i % 2 == 0))
        }))
    }

    // The registry-free unit tier can't drive a source to natural exhaustion
    // (constructing the terminating `StopIteration` needs a registry and
    // panics without one), so these exercise the transducer through
    // `first(n)`, which stops before the exhausting pull. The exhaustion path
    // (`to_a`/`force`) is covered by the e2e suite, which runs with a registry.
    fn first_n(l: &RubyValue, n: i64) -> Vec<i64> {
        ints(&first(l, &[RubyValue::Int(n)], None).unwrap())
    }

    #[test]
    fn map_transforms_every_element() {
        let mapped = map(&make_lazy(&arr(&[1, 2, 3])), &[], Some(times_two())).unwrap();
        assert_eq!(first_n(&mapped, 3), vec![2, 4, 6]);
    }

    #[test]
    fn select_then_map_chains_left_to_right() {
        let sel = select(&make_lazy(&arr(&[1, 2, 3, 4, 5, 6])), &[], Some(is_even())).unwrap();
        let mapped = map(&sel, &[], Some(times_two())).unwrap();
        assert_eq!(first_n(&mapped, 3), vec![4, 8, 12]);
    }

    #[test]
    fn take_bounds_the_pull() {
        let taken = take(
            &make_lazy(&arr(&[10, 20, 30, 40, 50])),
            &[RubyValue::Int(2)],
            None,
        )
        .unwrap();
        assert_eq!(first_n(&taken, 2), vec![10, 20]);
    }

    #[test]
    fn first_without_arg_returns_one_element() {
        let mapped = map(&make_lazy(&arr(&[1, 2, 3, 4])), &[], Some(times_two())).unwrap();
        assert!(matches!(
            first(&mapped, &[], None).unwrap(),
            RubyValue::Int(2)
        ));
    }

    #[test]
    fn drop_uniq_and_compact() {
        let dropped = drop(&make_lazy(&arr(&[1, 2, 3, 4])), &[RubyValue::Int(2)], None).unwrap();
        assert_eq!(first_n(&dropped, 2), vec![3, 4]);

        let uniqued = uniq(&make_lazy(&arr(&[1, 1, 2, 2, 3, 1])), &[], None).unwrap();
        assert_eq!(first_n(&uniqued, 3), vec![1, 2, 3]);

        let with_nils = RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Nil,
            RubyValue::Int(2),
            RubyValue::Nil,
            RubyValue::Int(3),
        ]));
        let compacted = compact(&make_lazy(&with_nils), &[], None).unwrap();
        assert_eq!(first_n(&compacted, 2), vec![1, 2]);
    }
}
