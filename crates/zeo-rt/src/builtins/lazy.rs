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
use crate::builtins::inherited_row;

use crate::builtins::arg_error;
use crate::builtins::enumerator::{enumerator_for, pull_next};
use crate::dispatch::{RObj, RubyObject};
use crate::{RProc, RubyValue, Signal, array_new};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use zeo_abi::LAZY_CLASS;
use zeo_macros::ruby_class;

/// One link in a lazy chain: the operation plus the NAME that built it.
/// CRuby stores the same pair, which is why `inspect` can print the chain
/// back as it was written -- `select` and `filter` share one implementation
/// but are two different names to a reader.
struct Link {
    name: &'static str,
    op: LazyOp,
}

/// What one link does. Block-bearing ops store the block; `Take`/`Drop`
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
    /// Pairs each value with an incrementing index (`with_index([offset])`),
    /// yielding `[value, index]`. WITH a block the block sees `(value, index)`
    /// and the ORIGINAL value flows on -- the block observes, it does not map
    /// (CRuby's `lazy_with_index`).
    WithIndex(i64, Option<RProc>),
    /// Every window of `n` CONSECUTIVE values, as an Array. A source shorter
    /// than `n` yields nothing.
    EachCons(usize),
    /// Consecutive non-overlapping runs of `n` values. The last run is short
    /// when the source does not divide evenly, and arrives at exhaustion (see
    /// [`flush`]).
    EachSlice(usize),
    /// `Enumerator::Lazy.new(source) { |yielder, *values| ... }`'s explicit
    /// per-element body: whatever the block hands the yielder (0..n values
    /// per input) flows downstream; the input itself does not.
    YielderBody(RProc),
    /// `zip(*others)` -- each value becomes `[value, others[0].next, ...]`,
    /// the others pulled ONE element per source element so an endless
    /// receiver stays workable. An exhausted other pads with nil, CRuby's
    /// rule. The receiver's length wins, so the op is size-preserving.
    Zip(Vec<RubyValue>),
}

/// The per-run mutable state for the stateful ops (a fresh set is built at the
/// start of every terminal drive, so re-running a lazy starts clean).
enum OpState {
    None,
    Count(i64),
    Dropping(bool),
    Seen(HashSet<crate::collections::HashKey>),
    /// The values `each_cons`/`each_slice` have buffered but not yet emitted.
    Window(Vec<RubyValue>),
    /// `zip`'s external enumerators over its others, built at the first
    /// accepted value so a re-run starts every other from its beginning.
    Iters(Option<Vec<RubyValue>>),
}

impl OpState {
    fn for_op(op: &LazyOp) -> OpState {
        match op {
            LazyOp::Take(_) | LazyOp::Drop(_) | LazyOp::WithIndex(..) => OpState::Count(0),
            LazyOp::DropWhile(_) => OpState::Dropping(true),
            LazyOp::Uniq(_) => OpState::Seen(HashSet::new()),
            LazyOp::EachCons(_) | LazyOp::EachSlice(_) => OpState::Window(Vec::new()),
            LazyOp::Zip(_) => OpState::Iters(None),
            _ => OpState::None,
        }
    }
}

/// The replaceable half of a lazy: the original source plus its op chain.
/// One value so the private `#initialize` row can swap both atomically.
struct LazyCore {
    source: RubyValue,
    links: Vec<Link>,
}

pub struct RLazy {
    /// Behind a `Mutex` only so `#initialize` can re-seed the lazy in place;
    /// every drive clones the pair out once (links are small), so no lock is
    /// held while blocks run.
    core: Mutex<LazyCore>,
    /// `next`/`peek`'s iteration, built on first use: an ordinary Enumerator
    /// over this very lazy's `each`. CRuby gets external iteration for free
    /// because `Enumerator::Lazy < Enumerator`; here one enumerator held on
    /// the side buys the same thing, and the chain still only advances as far
    /// as each `next` asks for.
    external: Mutex<Option<RubyValue>>,
}

/// The core cloned out -- what every read site works from.
fn snapshot(l: &RLazy) -> LazyCore {
    let g = l.core.lock();
    LazyCore {
        source: g.source.clone(),
        links: clone_links(&g.links),
    }
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
        // The copy starts un-iterated, like `Enumerator#dup` (`fresh_copy`):
        // an in-flight fiber belongs to the object that started it.
        Arc::new(RLazy {
            core: Mutex::new(snapshot(self)),
            external: Mutex::new(None),
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
        core: Mutex::new(LazyCore {
            source: source.clone(),
            links: Vec::new(),
        }),
        external: Mutex::new(None),
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

fn clone_links(links: &[Link]) -> Vec<Link> {
    links
        .iter()
        .map(|l| Link {
            name: l.name,
            op: match &l.op {
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
                LazyOp::WithIndex(n, blk) => LazyOp::WithIndex(*n, blk.clone()),
                LazyOp::EachCons(n) => LazyOp::EachCons(*n),
                LazyOp::EachSlice(n) => LazyOp::EachSlice(*n),
                LazyOp::YielderBody(p) => LazyOp::YielderBody(p.clone()),
                LazyOp::Zip(others) => LazyOp::Zip(others.clone()),
            },
        })
        .collect()
}

/// A new lazy that is `recv` with one more link appended.
fn extend(recv: &RubyValue, name: &'static str, op: LazyOp) -> RubyValue {
    let mut core = snapshot(lazy_of(recv));
    core.links.push(Link { name, op });
    RubyValue::Object(Arc::new(RLazy {
        core: Mutex::new(core),
        external: Mutex::new(None),
    }))
}

/// A required block, or CRuby's ArgumentError shape for a lazy op missing one.
fn need_block(block: Option<RubyValue>, meth: &str) -> Result<RProc, Signal> {
    match block {
        Some(RubyValue::Proc(p)) => Ok(p),
        _ => Err(arg_error!("tried to call lazy {meth} without a block")),
    }
}

/// A block where one is optional.
fn opt_block(block: Option<RubyValue>) -> Option<RProc> {
    match block {
        Some(RubyValue::Proc(p)) => Some(p),
        _ => None,
    }
}

/// The three ops that answer to more than one name. The name reaches the
/// chain (for `inspect`); the missing-block message keeps CRuby's canonical
/// one, so `lazy.find_all` reports "lazy select".
fn mapping(
    recv: &RubyValue,
    block: Option<RubyValue>,
    name: &'static str,
) -> Result<RubyValue, Signal> {
    Ok(extend(recv, name, LazyOp::Map(need_block(block, "map")?)))
}

fn flat_mapping(
    recv: &RubyValue,
    block: Option<RubyValue>,
    name: &'static str,
) -> Result<RubyValue, Signal> {
    Ok(extend(
        recv,
        name,
        LazyOp::FlatMap(need_block(block, "flat_map")?),
    ))
}

fn filtering(
    recv: &RubyValue,
    block: Option<RubyValue>,
    name: &'static str,
) -> Result<RubyValue, Signal> {
    Ok(extend(
        recv,
        name,
        LazyOp::Select(need_block(block, "select")?),
    ))
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
/// (`first`/`to_a`/`force`), handed to a block (`each`), or dropped -- the
/// chain was run for a block's side effects alone (`each_with_index`).
enum Sink<'a> {
    Collect {
        out: &'a mut Vec<RubyValue>,
        limit: Option<usize>,
    },
    Each(&'a RProc),
    Drain,
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
            Sink::Drain => Ok(Flow::Continue),
        }
    }
}

/// Pushes one value through `links[idx..]` into `sink`. Returns `Stop` the
/// moment nothing more should be pulled from the source.
fn push(
    links: &[Link],
    st: &mut [OpState],
    idx: usize,
    val: RubyValue,
    sink: &mut Sink,
) -> Result<Flow, Signal> {
    if idx == links.len() {
        return sink.accept(val);
    }
    match &links[idx].op {
        LazyOp::Map(p) => {
            let v = p.call(std::slice::from_ref(&val))?;
            push(links, st, idx + 1, v, sink)
        }
        LazyOp::Select(p) => {
            if p.call(std::slice::from_ref(&val))?.truthy() {
                push(links, st, idx + 1, val, sink)
            } else {
                Ok(Flow::Continue)
            }
        }
        LazyOp::Reject(p) => {
            if p.call(std::slice::from_ref(&val))?.truthy() {
                Ok(Flow::Continue)
            } else {
                push(links, st, idx + 1, val, sink)
            }
        }
        LazyOp::FilterMap(p) => {
            let v = p.call(std::slice::from_ref(&val))?;
            if v.truthy() {
                push(links, st, idx + 1, v, sink)
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
                        if push(links, st, idx + 1, e, sink)? == Flow::Stop {
                            return Ok(Flow::Stop);
                        }
                    }
                    Ok(Flow::Continue)
                }
                other => push(links, st, idx + 1, other, sink),
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
                push(links, st, idx + 1, v, sink)
            } else {
                Ok(Flow::Continue)
            }
        }
        LazyOp::Compact => {
            if val.is_nil() {
                Ok(Flow::Continue)
            } else {
                push(links, st, idx + 1, val, sink)
            }
        }
        LazyOp::WithIndex(offset, blk) => {
            let i = match &mut st[idx] {
                OpState::Count(c) => {
                    let cur = *c;
                    *c += 1;
                    cur
                }
                _ => unreachable!("WithIndex state"),
            };
            let index = RubyValue::Int(offset + i);
            match blk {
                // The block is handed BOTH values and its answer is discarded;
                // what flows on is the value itself.
                Some(p) => {
                    p.call(&[val.clone(), index])?;
                    push(links, st, idx + 1, val, sink)
                }
                None => {
                    let paired = RubyValue::Array(array_new(vec![val, index]));
                    push(links, st, idx + 1, paired, sink)
                }
            }
        }
        LazyOp::TakeWhile(p) => {
            if p.call(std::slice::from_ref(&val))?.truthy() {
                push(links, st, idx + 1, val, sink)
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
            push(links, st, idx + 1, val, sink)
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
            let flow = push(links, st, idx + 1, val, sink)?;
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
                push(links, st, idx + 1, val, sink)
            }
        }
        LazyOp::EachCons(n) => {
            // The window slides by one: emit a COPY, then drop its head.
            let window = match &mut st[idx] {
                OpState::Window(buf) => {
                    buf.push(val);
                    if buf.len() < *n {
                        return Ok(Flow::Continue);
                    }
                    let full = buf.clone();
                    buf.remove(0);
                    full
                }
                _ => unreachable!("EachCons state"),
            };
            push(
                links,
                st,
                idx + 1,
                RubyValue::Array(array_new(window)),
                sink,
            )
        }
        LazyOp::EachSlice(n) => {
            let slice = match &mut st[idx] {
                OpState::Window(buf) => {
                    buf.push(val);
                    if buf.len() < *n {
                        return Ok(Flow::Continue);
                    }
                    std::mem::take(buf)
                }
                _ => unreachable!("EachSlice state"),
            };
            push(links, st, idx + 1, RubyValue::Array(array_new(slice)), sink)
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
                push(links, st, idx + 1, val, sink)
            } else {
                Ok(Flow::Continue)
            }
        }
        LazyOp::Zip(others) => {
            let iters = match &mut st[idx] {
                OpState::Iters(it) => it.get_or_insert_with(|| {
                    others
                        .iter()
                        .map(|o| enumerator_for(o, "each", &[]))
                        .collect()
                }),
                _ => unreachable!("Zip state"),
            };
            let mut tuple = vec![val];
            for it in iters.iter() {
                let RubyValue::Enumerator(e) = it else {
                    unreachable!("enumerator_for always builds an Enumerator");
                };
                tuple.push(match pull_next(e)? {
                    Some(vals) => pack(vals),
                    None => RubyValue::Nil,
                });
            }
            push(links, st, idx + 1, RubyValue::Array(array_new(tuple)), sink)
        }
        LazyOp::YielderBody(p) => {
            // The body runs against a COLLECTING yielder (the chain's
            // continuation cannot ride inside an `RProc`), then whatever it
            // emitted flows downstream in order.
            let bucket: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
            let sink_bucket = bucket.clone();
            let collector = crate::RProc::new(move |args: &[RubyValue]| {
                sink_bucket
                    .lock()
                    .push(crate::builtins::enumerable::pack(args));
                Ok(RubyValue::Nil)
            });
            p.call(&[RubyValue::Yielder(collector), val])?;
            let emitted = std::mem::take(&mut *bucket.lock());
            for v in emitted {
                if push(links, st, idx + 1, v, sink)? == Flow::Stop {
                    return Ok(Flow::Stop);
                }
            }
            Ok(Flow::Continue)
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

/// Emits what a BUFFERING op still holds once the source ends: `each_slice`'s
/// short final run. `each_cons` holds a partial window, which is never a
/// window, so it emits nothing. Runs left to right, so an earlier op's leftover
/// reaches a later one before that one flushes in turn.
fn flush(links: &[Link], st: &mut [OpState], sink: &mut Sink) -> Result<(), Signal> {
    for idx in 0..links.len() {
        if !matches!(links[idx].op, LazyOp::EachSlice(_)) {
            continue;
        }
        let rest = match &mut st[idx] {
            OpState::Window(buf) if !buf.is_empty() => std::mem::take(buf),
            _ => continue,
        };
        if push(links, st, idx + 1, RubyValue::Array(array_new(rest)), sink)? == Flow::Stop {
            return Ok(());
        }
    }
    Ok(())
}

/// Drives the source through the chain into `sink`, pulling only as far as the
/// sink/ops allow (a `Stop` ends the loop before the next pull). Only a source
/// that ends NATURALLY flushes: a `Stop` means the sink already has what it
/// asked for.
fn drive(lazy: &RLazy, sink: &mut Sink) -> Result<(), Signal> {
    let core = snapshot(lazy);
    let src = enumerator_for(&core.source, "each", &[]);
    let RubyValue::Enumerator(e) = &src else {
        unreachable!("enumerator_for always builds an Enumerator");
    };
    let mut st: Vec<OpState> = core.links.iter().map(|l| OpState::for_op(&l.op)).collect();
    while let Some(vals) = pull_next(e)? {
        if push(&core.links, &mut st, 0, pack(vals), sink)? == Flow::Stop {
            return Ok(());
        }
    }
    flush(&core.links, &mut st, sink)
}

/// One link as `inspect` prints it: the method name, plus the arguments it
/// captured for the ops that take any. A block is never shown -- CRuby prints
/// `:select`, not the block it holds.
fn link_label(link: &Link) -> String {
    let args = match &link.op {
        LazyOp::Take(n) | LazyOp::Drop(n) => format!("({n})"),
        LazyOp::EachCons(n) | LazyOp::EachSlice(n) => format!("({n})"),
        LazyOp::Grep(pat, ..) => format!("({})", pat.inspect_string()),
        LazyOp::Zip(others) => format!(
            "({})",
            others
                .iter()
                .map(|o| o.inspect_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // `each_with_index` takes no offset, so only `with_index` shows one.
        LazyOp::WithIndex(offset, _) if link.name == "with_index" => format!("({offset})"),
        _ => String::new(),
    };
    format!("{}{args}", link.name)
}

/// The chain, printed outside-in: every link wraps what came before it, so
/// `(1..3).lazy.map { }` reads as `#<Enumerator::Lazy: #<Enumerator::Lazy:
/// 1..3>:map>`. CRuby builds the same string by recursion, because there each
/// link IS a lazy holding the previous one.
fn chain_inspect(lazy: &RLazy) -> Result<String, Signal> {
    let core = snapshot(lazy);
    let mut s = format!("#<Enumerator::Lazy: {}>", core.source.try_inspect_string()?);
    for link in &core.links {
        s = format!("#<Enumerator::Lazy: {s}:{}>", link_label(link));
    }
    Ok(s)
}

/// The enumerator `next`/`peek`/`rewind` share -- one per lazy, built the
/// first time external iteration asks for it.
fn external_iter(recv: &RubyValue) -> RubyValue {
    lazy_of(recv)
        .external
        .lock()
        .get_or_insert_with(|| enumerator_for(recv, "each", &[]))
        .clone()
}

/// Hands one external-iteration call to that enumerator.
fn iterate(recv: &RubyValue, meth: &str) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(&external_iter(recv), crate::Symbol::intern(meth), &[], None)
}

// ---------------------------------------------------------------------------
// The Enumerator-row bridge. `Enumerator::Lazy < Enumerator` (real CRuby
// hierarchy), so `each`/`next`/`peek`/`rewind`/`size`/`inspect` are
// Enumerator's OWN rows -- `.owner` says so -- and those rows detect a lazy
// receiver and branch here instead of downcasting to `EnumeratorData`.
// ---------------------------------------------------------------------------

pub(crate) fn is_lazy(v: &RubyValue) -> bool {
    matches!(v, RubyValue::Object(o) if o.as_any().is::<RLazy>())
}

/// `each` with a block drives the chain; blockless answers the lazy itself.
pub(crate) fn lazy_each(recv: &RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    match block {
        Some(RubyValue::Proc(p)) => {
            drive(lazy_of(recv), &mut Sink::Each(&p))?;
            Ok(recv.clone())
        }
        _ => Ok(recv.clone()),
    }
}

/// `next`/`next_values`/`peek`/`peek_values` -- external iteration through
/// the cached side enumerator.
pub(crate) fn lazy_iterate(recv: &RubyValue, meth: &str) -> Result<RubyValue, Signal> {
    iterate(recv, meth)
}

/// `rewind` answers the LAZY, not the enumerator doing the iterating.
pub(crate) fn lazy_rewind(recv: &RubyValue) -> Result<RubyValue, Signal> {
    if lazy_of(recv).external.lock().is_some() {
        iterate(recv, "rewind")?;
    }
    Ok(recv.clone())
}

/// `feed` reaches the side enumerator's paused generator.
pub(crate) fn lazy_feed(recv: &RubyValue, arg: &RubyValue) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(
        &external_iter(recv),
        crate::Symbol::intern("feed"),
        std::slice::from_ref(arg),
        None,
    )
}

/// `size` never iterates: the source's size with each op's knowable effect
/// folded in. A filtering op makes the answer unknowable (nil) -- CRuby's
/// rule, since it can't be answered without running.
pub(crate) fn lazy_size(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let core = snapshot(lazy_of(recv));
    let mut size = source_size(&core.source);
    for link in &core.links {
        size = match (&link.op, size) {
            (LazyOp::Map(_) | LazyOp::Compact | LazyOp::Zip(_), s) => s,
            (LazyOp::Take(n), RubyValue::Int(s)) => RubyValue::Int(s.min(*n)),
            // `take` bounds even an endless source.
            (LazyOp::Take(n), RubyValue::Float(_)) => RubyValue::Int(*n),
            (LazyOp::Drop(n), RubyValue::Int(s)) => RubyValue::Int((s - n).max(0)),
            (LazyOp::Drop(_), s @ RubyValue::Float(_)) => s,
            // A window/slice op leaves an endless source endless.
            (LazyOp::EachCons(n), RubyValue::Int(s)) => RubyValue::Int((s - *n as i64 + 1).max(0)),
            (LazyOp::EachSlice(n), RubyValue::Int(s)) => {
                let n = *n as i64;
                RubyValue::Int((s + n - 1) / n)
            }
            (LazyOp::EachCons(_) | LazyOp::EachSlice(_), s @ RubyValue::Float(_)) => s,
            _ => RubyValue::Nil,
        };
        if matches!(size, RubyValue::Nil) {
            break;
        }
    }
    Ok(size)
}

/// The chain, printed outside-in -- see [`chain_inspect`].
pub(crate) fn lazy_inspect(recv: &RubyValue) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Str(crate::string_new(chain_inspect(lazy_of(
        recv,
    ))?)))
}

/// `to_s` stays the ADDRESS form (CRuby leaves it to `Object#to_s`), even
/// though dispatch now reaches `Enumerator`'s own `to_s` row first.
pub(crate) fn lazy_to_s(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Object(o) = recv else {
        unreachable!("is_lazy gated this branch");
    };
    Ok(RubyValue::Str(crate::string_new(
        crate::value::default_object_repr(o, false, &mut Vec::new())?,
    )))
}

/// `each_cons`/`each_slice` -- one grouping op appended. WITH a block CRuby
/// runs the whole chain at once for the block's side effects and answers the
/// RECEIVER, so the groups are not passed on and an endless source never
/// returns -- there as here.
fn each_group(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    slice: bool,
) -> Result<RubyValue, Signal> {
    // The caller's own name, so `each_slice(0)` reports "invalid slice size"
    // and `each_cons(0)` reports "invalid size", exactly as the eager rows do.
    let n = crate::builtins::enumerable::slice_size(
        args,
        if slice { "each_slice" } else { "each_cons" },
    )?;
    let grouped = if slice {
        extend(recv, "each_slice", LazyOp::EachSlice(n))
    } else {
        extend(recv, "each_cons", LazyOp::EachCons(n))
    };
    let Some(RubyValue::Proc(p)) = block else {
        return Ok(grouped);
    };
    drive(lazy_of(&grouped), &mut Sink::Each(&p))?;
    Ok(recv.clone())
}

/// `Lazy.new(source, size = nil) { |yielder, *values| ... }`'s argument
/// parsing, shared with the private `#initialize` row: the source plus the
/// explicit per-element body as the chain's first link. The size hint is
/// accepted and unused (`#size` derives from the source here).
fn lazy_new_core(args: &[RubyValue], block: Option<RubyValue>) -> Result<LazyCore, Signal> {
    crate::builtins::check_arity(args.len(), 1, Some(2))?;
    let Some(RubyValue::Proc(p)) = &block else {
        return Err(arg_error!("tried to call lazy new without a block"));
    };
    Ok(LazyCore {
        source: args[0].clone(),
        links: vec![Link {
            name: "each",
            op: LazyOp::YielderBody(p.clone()),
        }],
    })
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

ruby_class! {
    Lazy = zeo_abi::LAZY_CLASS < zeo_abi::ENUMERATOR_CLASS;

    // `Enumerator::Lazy.new(source, size = nil) { |yielder, *values| ... }`
    // -- a lazy over `source` with an explicit per-element body: what the
    // block hands the yielder flows on, so it can filter, transform, or fan
    // out. The size hint is accepted and unused (`#size` derives from the
    // source here).
    def self."new" cfunc (_recv, *args, &block) {
        Ok(RubyValue::Object(Arc::new(RLazy {
            core: Mutex::new(lazy_new_core(args, block)?),
            external: Mutex::new(None),
        })))
    }

    // Re-init: the receiver becomes what `Lazy.new(source[, size]) { ... }`
    // builds, discarding the previous chain and any external iteration.
    private def "initialize" cfunc (recv, *args, &block) {
        let l = lazy_of(recv);
        let core = lazy_new_core(args, block)?;
        *l.core.lock() = core;
        *l.external.lock() = None;
        Ok(recv.clone())
    }


    // Each alias appends the name it was CALLED by, so `inspect` prints the
    // chain the way it was written. The missing-block message does not follow
    // suit: `lazy.filter` reports "lazy select", CRuby's own wording.
    def "map"(recv, &block) { mapping(recv, block, "map") }
    def "collect"(recv, &block) { mapping(recv, block, "collect") }
    def "flat_map"(recv, &block) { flat_mapping(recv, block, "flat_map") }
    def "collect_concat"(recv, &block) { flat_mapping(recv, block, "collect_concat") }
    def "select"(recv, &block) { filtering(recv, block, "select") }
    def "filter"(recv, &block) { filtering(recv, block, "filter") }
    def "find_all"(recv, &block) { filtering(recv, block, "find_all") }

    // `with_index([offset]) { |item, idx| ... }` -- lazily pairs each value
    // with an incrementing index. Blockless it yields the `[item, idx]` pairs;
    // WITH a block the block sees the two and the item itself flows on.
    def "with_index"(recv, arg?, &block) {
        let offset = match arg {
            Some(v) => crate::builtins::convert::to_index(v)?,
            None => 0,
        };
        Ok(extend(recv, "with_index", LazyOp::WithIndex(offset, opt_block(block))))
    }
    // Unlike `with_index`, ruby does NOT give this one a lazy override, so a
    // block reaches `Enumerator`'s row: the whole chain runs at once for the
    // block's side effects and the receiver comes back. Blockless it stays
    // lazy, through the `to_enum` this class does override.
    def "each_with_index"(recv, &block) {
        let blk = opt_block(block);
        let indexed = extend(recv, "each_with_index", LazyOp::WithIndex(0, blk.clone()));
        if blk.is_none() {
            return Ok(indexed);
        }
        drive(lazy_of(&indexed), &mut Sink::Drain)?;
        Ok(recv.clone())
    }
    def "filter_map"(recv, &block) {
        Ok(extend(recv, "filter_map", LazyOp::FilterMap(need_block(block, "filter_map")?)))
    }
    def "reject"(recv, &block) {
        Ok(extend(recv, "reject", LazyOp::Reject(need_block(block, "reject")?)))
    }
    def "take_while"(recv, &block) {
        Ok(extend(recv, "take_while", LazyOp::TakeWhile(need_block(block, "take_while")?)))
    }
    def "drop_while"(recv, &block) {
        Ok(extend(recv, "drop_while", LazyOp::DropWhile(need_block(block, "drop_while")?)))
    }
    def "take"(recv, arg) {
        Ok(extend(recv, "take", LazyOp::Take(count_arg(arg, "take")?)))
    }
    def "drop"(recv, arg) {
        Ok(extend(recv, "drop", LazyOp::Drop(count_arg(arg, "drop")?)))
    }
    def "grep"(recv, arg, &block) {
        Ok(extend(recv, "grep", LazyOp::Grep((*arg).clone(), false, opt_block(block))))
    }
    def "grep_v"(recv, arg, &block) {
        Ok(extend(recv, "grep_v", LazyOp::Grep((*arg).clone(), true, opt_block(block))))
    }
    def "uniq"(recv, &block) {
        Ok(extend(recv, "uniq", LazyOp::Uniq(opt_block(block))))
    }
    def "compact"(recv) {
        Ok(extend(recv, "compact", LazyOp::Compact))
    }
    // `each_cons(n)` / `each_slice(n)` -- lazy since ruby 3.1, so an infinite
    // source stays workable.
    def "each_cons"(recv, n, &block) {
        each_group(recv, std::slice::from_ref(n), block, false)
    }
    def "each_slice"(recv, n, &block) {
        each_group(recv, std::slice::from_ref(n), block, true)
    }
    def "lazy"(recv) {
        Ok(recv.clone())
    }
    // `#eager` -- the same sequence as a NON-lazy Enumerator, so every later
    // `map`/`select` evaluates at once. An ordinary Enumerator over this very
    // lazy is exactly that, and it still costs nothing until someone iterates.
    def "eager"(recv) {
        Ok(enumerator_for(recv, "each", &[]))
    }
    // The one terminal ruby puts ON this class -- `to_a`/`entries`/`first`
    // are Enumerable's rows (they drive `each`, which `Enumerator`'s row
    // routes back through this lazy's chain), and `each`/`next`/`peek`/
    // `rewind`/`size`/`inspect` are Enumerator's own rows with a lazy
    // branch. `.owner` agrees with CRuby on every one of them.
    def "force" cfunc (recv, *_args) {
        Ok(RubyValue::Array(array_new(collect(lazy_of(recv), None)?)))
    }

    // `zip(*others)` stays LAZY -- one element pulled from each other per
    // source element. CRuby falls back to the eager super for a block or an
    // argument it can't re-iterate; the eager Enumerable row is that
    // fallback here.
    def "zip" cfunc (recv, *args, &block) {
        let lazy_ok = block.is_none()
            && args.iter().all(|a| {
                matches!(
                    a,
                    RubyValue::Array(_) | RubyValue::Range(..) | RubyValue::Enumerator(_)
                ) || is_lazy(a)
            });
        if !lazy_ok {
            return inherited_row!(enumerable, "zip", recv, __args, block);
        }
        Ok(extend(recv, "zip", LazyOp::Zip(args.to_vec())))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "chunk" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "chunk", recv, __args, block) }
    def "chunk_while" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "chunk_while", recv, __args, block) }
    def "slice_after" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "slice_after", recv, __args, block) }
    def "slice_before" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "slice_before", recv, __args, block) }
    def "slice_when" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "slice_when", recv, __args, block) }
    def "enum_for" cfunc (recv, *_args, &block) { inherited_row!(kernel, "enum_for", recv, __args, block) }
    def "to_enum" cfunc (recv, *_args, &block) { inherited_row!(kernel, "to_enum", recv, __args, block) }

    // ---- `_enumerable_*` -- ruby snapshots the EAGER Enumerable bodies onto
    // Lazy under these names BEFORE defining the lazy overrides, then makes
    // them private (enumerator.c). Real `private def` rows, not aliases: the
    // DSL's `alias` would bind the lazy override and copy its public
    // visibility. Each drives the eager ancestor row, so calling one on a
    // lazy runs the chain at once -- CRuby's exact behavior.
    private def "_enumerable_map" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "map", recv, __args, block) }
    private def "_enumerable_collect" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "collect", recv, __args, block) }
    private def "_enumerable_flat_map" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "flat_map", recv, __args, block) }
    private def "_enumerable_collect_concat" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "collect_concat", recv, __args, block) }
    private def "_enumerable_select" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "select", recv, __args, block) }
    private def "_enumerable_filter" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "filter", recv, __args, block) }
    private def "_enumerable_find_all" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "find_all", recv, __args, block) }
    private def "_enumerable_filter_map" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "filter_map", recv, __args, block) }
    private def "_enumerable_reject" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "reject", recv, __args, block) }
    private def "_enumerable_grep" arity 1 (recv, *_args, &block) { inherited_row!(enumerable, "grep", recv, __args, block) }
    private def "_enumerable_grep_v" arity 1 (recv, *_args, &block) { inherited_row!(enumerable, "grep_v", recv, __args, block) }
    private def "_enumerable_zip" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "zip", recv, __args, block) }
    private def "_enumerable_take" arity 1 (recv, *_args, &block) { inherited_row!(enumerable, "take", recv, __args, block) }
    private def "_enumerable_take_while" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "take_while", recv, __args, block) }
    private def "_enumerable_drop" arity 1 (recv, *_args, &block) { inherited_row!(enumerable, "drop", recv, __args, block) }
    private def "_enumerable_drop_while" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "drop_while", recv, __args, block) }
    private def "_enumerable_uniq" arity 0 (recv, *_args, &block) { inherited_row!(enumerable, "uniq", recv, __args, block) }
    // The odd one out: it snapshots Enumerator#with_index, not an Enumerable
    // row (there is none by that name). Enumerator's row needs an Enumerator
    // receiver, so the lazy goes through its EAGER self first -- same
    // sequence, eager drive, which is exactly what the snapshot is for.
    private def "_enumerable_with_index" cfunc (recv, *_args, &block) {
        let eager = enumerator_for(recv, "each", &[]);
        inherited_row!(enumerator, "with_index", &eager, __args, block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lazy's `ruby_class!` methods have mangled fn names, so tests reach
    /// them through the registered instance lookup (as real dispatch does).
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::LAZY_CLASS)
            .expect("Lazy is registered")
            .instance
            .as_ref()
            .expect("Lazy has instance methods")
            .lookup)(name)
        .unwrap_or_else(|| panic!("Lazy#{name} is defined"))
    }

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
    // panics without one), so these exercise the transducer through a
    // bounded `collect`, which stops before the exhausting pull -- the same
    // drive `Enumerable#first` performs (`first` is Enumerable's row now,
    // unreachable from Lazy's own table). The exhaustion path (`force`) is
    // covered by the e2e suite, which runs with a registry.
    fn first_n(l: &RubyValue, n: i64) -> Vec<i64> {
        let mut out = Vec::new();
        drive(
            lazy_of(l),
            &mut Sink::Collect {
                out: &mut out,
                limit: Some(n as usize),
            },
        )
        .unwrap();
        ints(&RubyValue::Array(array_new(out)))
    }

    #[test]
    fn map_transforms_every_element() {
        let mapped = imethod("map")(&make_lazy(&arr(&[1, 2, 3])), &[], Some(times_two())).unwrap();
        assert_eq!(first_n(&mapped, 3), vec![2, 4, 6]);
    }

    #[test]
    fn select_then_map_chains_left_to_right() {
        let sel =
            imethod("select")(&make_lazy(&arr(&[1, 2, 3, 4, 5, 6])), &[], Some(is_even())).unwrap();
        let mapped = imethod("map")(&sel, &[], Some(times_two())).unwrap();
        assert_eq!(first_n(&mapped, 3), vec![4, 8, 12]);
    }

    #[test]
    fn take_bounds_the_pull() {
        let taken = imethod("take")(
            &make_lazy(&arr(&[10, 20, 30, 40, 50])),
            &[RubyValue::Int(2)],
            None,
        )
        .unwrap();
        assert_eq!(first_n(&taken, 2), vec![10, 20]);
    }

    #[test]
    fn first_without_arg_returns_one_element() {
        let mapped =
            imethod("map")(&make_lazy(&arr(&[1, 2, 3, 4])), &[], Some(times_two())).unwrap();
        assert_eq!(first_n(&mapped, 1), vec![2]);
    }

    #[test]
    fn drop_uniq_and_compact() {
        let dropped =
            imethod("drop")(&make_lazy(&arr(&[1, 2, 3, 4])), &[RubyValue::Int(2)], None).unwrap();
        assert_eq!(first_n(&dropped, 2), vec![3, 4]);

        let uniqued = imethod("uniq")(&make_lazy(&arr(&[1, 1, 2, 2, 3, 1])), &[], None).unwrap();
        assert_eq!(first_n(&uniqued, 3), vec![1, 2, 3]);

        let with_nils = RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Nil,
            RubyValue::Int(2),
            RubyValue::Nil,
            RubyValue::Int(3),
        ]));
        let compacted = imethod("compact")(&make_lazy(&with_nils), &[], None).unwrap();
        assert_eq!(first_n(&compacted, 2), vec![1, 2]);
    }
}
