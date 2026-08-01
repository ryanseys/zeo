//! `Enumerator` -- fiber-backed external iteration, per
//! CRuby's enumerator.c (all mechanisms cited there were read against the
//! CRuby source and oracle-verified on ruby 4.0.6).
//!
//! An enumerator captures exactly what CRuby's `struct enumerator` does:
//! a receiver, a method name, and the trailing args (`enumerator_init`,
//! enumerator.c:416) -- OR, for `Enumerator.new { |y| ... }`, the
//! generator block (enumerator.c:488). Internal iteration
//! (`enum.each { b }`) simply re-invokes `recv.meth(*args) { b }`
//! (`enumerator_block_call` -> `rb_block_call`, enumerator.c:564);
//! the generator form calls the block with a fresh [`RubyValue::Yielder`]
//! wrapping `b`, so `y << v` / `y.yield v` forward each element to the
//! consumer's block (generator_each/yielder_yield, enumerator.c:1553/1389).
//!
//! External iteration (`next`/`peek`) lazily spins up a coroutine that
//! runs the FULL internal `each` with a shuttle block that suspends per
//! element (`next_i`/`next_ii`, enumerator.c:772/758). The coroutine is
//! the EXACT same `crate::coroutine` instantiation `fiber.rs` uses -- same
//! Send+Sync split (handle in the value, coroutine thread-pinned in a TLS
//! table), same ec-swap (see `crate::ec`), and, because the `(input, yield)`
//! TypeIds match, a `Fiber.yield` inside an enumerated `each` suspends
//! the enumerator's own fiber -- which is literally CRuby's semantics
//! (the enumerator's fiber IS the current fiber there).
//!
//! Exhaustion parks the underlying `each`'s RETURN VALUE and raises
//! `StopIteration` ("iteration reached an end") carrying it as `#result`
//! -- what `Kernel#loop` harvests (kernel.rb:151). Repeated `next` keeps
//! raising from the parked result; a mid-iteration EXCEPTION propagates
//! out of `next` and clears the fiber, so the following `next` re-inits
//! and RESTARTS the iteration -- CRuby's own `get_next_values` behavior,
//! oracle-verified.
//!
//! Documented divergences (Tier B unless noted): `#feed`, `#+` (Chain),
//! `Enumerator.produce`, `Enumerator::Lazy`, `rewind`'s receiver-`rewind`
//! hook, `each(*extra)`'s dup-and-append form, bignum `with_index`
//! offsets, and `size` returns nil for a handful of shapes CRuby computes
//! (e.g. `each_slice` over an infinite range). Cross-thread `next` and
//! `dup` of a live iteration are loud panics mirroring CRuby's
//! `FiberError`/`can't copy execution context` messages.

use crate::builtins::enumerable::pack;
use crate::builtins::{arg_error, type_error};
use crate::collections::array_new;
use crate::coroutine::CoroutineResult;
use crate::dispatch::{raise_stop_iteration, send_value};
use crate::signal::Signal;
use crate::value::RubyValue;
use crate::{RProc, Symbol};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::ThreadId;
use zeo_macros::ruby_class;

/// What `to_enum` captures -- CRuby's `struct enumerator`'s `obj`/`meth`/
/// `args` triple, or the `Enumerator.new` generator block. Cloned into
/// the external-iteration fiber, so it must stay cheap to clone
/// (everything inside is `Arc`-backed or small).
#[derive(Clone)]
enum EnumSource {
    Method {
        recv: RubyValue,
        meth: String,
        args: Vec<RubyValue>,
    },
    Generator {
        block: RProc,
    },
    /// `Enumerator.produce(initial) { |prev| ... }` -- an infinite generator.
    /// The first yielded value is `initial` (or, absent, `block.call(nil)`);
    /// each subsequent value is `block` applied to the previous one.
    Produce {
        initial: Option<RubyValue>,
        block: RProc,
    },
    /// `Enumerator#+` / `Enumerable#chain` -- the sources iterated back to
    /// back. Carried as an ordinary Enumerator; `class_of` reports
    /// `Enumerator::Chain` off this variant.
    Chain {
        sources: Vec<RubyValue>,
    },
    /// `Enumerator.product(*enums)` -- the cartesian product, yielded as one
    /// Array per combination, rightmost source varying fastest.
    Product {
        sources: Vec<RubyValue>,
    },
}

/// The mutable external-iteration half, all behind one short-held lock
/// (never held across a fiber switch -- see `get_next_values`).
#[derive(Default)]
struct ExternState {
    /// The thread the iteration fiber is pinned to (set at first
    /// `next`/`peek`; a fiber can only be resumed where it was created).
    owner: Option<ThreadId>,
    /// The live coroutine's key in [`ENUM_FIBERS`]; `None` = not started,
    /// finished, or cleared by an escaped exception/`rewind`.
    fiber: Option<u64>,
    /// `peek`'s cached element (CRuby's `lookahead`): `peek` fills it
    /// without consuming, `next` consumes it before resuming the fiber.
    lookahead: Option<Vec<RubyValue>>,
    /// `#feed`'s injected value -- what the generator's paused `y.yield`
    /// returns on the next resume (CRuby's `feedvalue`). Consumed by that
    /// resume; `#feed` twice before a `#next` is a TypeError.
    feed: Option<RubyValue>,
    /// The underlying `each`'s return value once iteration completed --
    /// every subsequent `next` re-raises `StopIteration` carrying it
    /// (CRuby rebuilds a fresh exception each time, from `stop_exc`).
    done: Option<RubyValue>,
    /// The iteration fiber's own execution context while suspended
    /// ($!/rescue stack, proc homes, catch tags, backtrace frames) --
    /// same ec-swap as `fiber.rs` (see `crate::ec`).
    saved_ec: crate::ec::Ec,
}

pub struct EnumeratorData {
    source: EnumSource,
    /// `Enumerator.new(size) { ... }`'s stored hint; method-backed
    /// enumerators derive size lazily from their source instead.
    size_hint: Option<RubyValue>,
    state: Mutex<ExternState>,
    /// `.frozen?` state -- flag-only (CRuby happily iterates a frozen
    /// enumerator; external-iteration state isn't Ruby-visible mutation).
    frozen: std::sync::atomic::AtomicBool,
}

pub type REnumerator = Arc<EnumeratorData>;

impl EnumeratorData {
    /// Whether external iteration has begun and not finished -- what
    /// makes `dup` unsafe (CRuby: "can't copy execution context").
    pub(crate) fn iteration_live(&self) -> bool {
        self.state.lock().fiber.is_some()
    }

    /// A fresh, never-iterated enumerator over the same source --
    /// `dup`/`clone`'s payload (which starts unfrozen; `clone`'s flag copy
    /// is `dup_value`'s job).
    pub(crate) fn fresh_copy(&self) -> REnumerator {
        Arc::new(EnumeratorData {
            source: self.source.clone(),
            size_hint: self.size_hint.clone(),
            state: Mutex::new(ExternState::default()),
            frozen: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// `Enumerator#frozen?` -- see the `frozen` field.
    pub fn is_frozen(&self) -> bool {
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `Enumerator#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Deliberately the same instantiation as `fiber.rs`'s `FiberCoro` (see
/// the module docs for why the matching TypeIds are a feature).
type EnumCoro = crate::coroutine::Coroutine<Vec<RubyValue>, RubyValue, Result<RubyValue, Signal>>;

thread_local! {
    static ENUM_FIBERS: RefCell<HashMap<u64, EnumCoro>> = RefCell::new(HashMap::new());
}

/// Process-wide so ids stay unambiguous even if handles travel between
/// threads (only RESUMING is thread-pinned, not holding).
static NEXT_ITER_ID: AtomicU64 = AtomicU64::new(1);

/// The one constructor every blockless iteration method funnels through
/// (`rb_enumeratorize`): captures the receiver, the method to re-invoke,
/// and the trailing args -- nothing else.
pub(crate) fn enumerator_for(recv: &RubyValue, meth: &str, args: &[RubyValue]) -> RubyValue {
    RubyValue::Enumerator(Arc::new(EnumeratorData {
        source: EnumSource::Method {
            recv: recv.clone(),
            meth: meth.to_string(),
            args: args.to_vec(),
        },
        size_hint: None,
        state: Mutex::new(ExternState::default()),
        frozen: std::sync::atomic::AtomicBool::new(false),
    }))
}

/// A Generator-backed Enumerator that yields each precomputed value in
/// `values`. `chunk_while`/`slice_when`/`chunk` (block form) answer one of
/// these -- CRuby wraps the runs in a Generator, so `#inspect` shows
/// `#<Enumerator::Generator:...>` and `.to_a`/`.each` replay the runs.
pub(crate) fn generator_of(values: Vec<RubyValue>) -> RubyValue {
    let generator = crate::RProc::new(move |a: &[RubyValue]| {
        if let Some(RubyValue::Yielder(y)) = a.first() {
            for v in &values {
                y.call(std::slice::from_ref(v))?;
            }
        }
        Ok(RubyValue::Nil)
    });
    RubyValue::Enumerator(Arc::new(EnumeratorData {
        source: EnumSource::Generator { block: generator },
        size_hint: None,
        state: Mutex::new(ExternState::default()),
        frozen: std::sync::atomic::AtomicBool::new(false),
    }))
}

/// `Enumerator::Chain` over `sources`, iterated back to back. The public
/// constructor behind `Enumerator#+` and `Enumerable#chain`.
pub(crate) fn chain_of(sources: Vec<RubyValue>) -> RubyValue {
    RubyValue::Enumerator(Arc::new(EnumeratorData {
        source: EnumSource::Chain { sources },
        size_hint: None,
        state: Mutex::new(ExternState::default()),
        frozen: std::sync::atomic::AtomicBool::new(false),
    }))
}

/// The class an enumerator reports: the source variant picks
/// `Enumerator::Chain`/`Enumerator::Product` over plain `Enumerator`
/// (see `value.rs`).
pub fn enumerator_class_id(e: &REnumerator) -> crate::ClassId {
    match e.source {
        EnumSource::Chain { .. } => zeo_abi::ENUMERATOR_CHAIN_CLASS,
        EnumSource::Product { .. } => zeo_abi::ENUMERATOR_PRODUCT_CLASS,
        _ => zeo_abi::ENUMERATOR_CLASS,
    }
}

/// `Enumerator.new([size]) { |y| ... }` -- reached through the dynamic
/// `Class#new` row (`rclass`'s `new` special-cases
/// `ENUMERATOR_CLASS`); parse deliberately skips the static `New` node
/// for literal `Enumerator` receivers so the block rides the ordinary
/// dynamic-call plumbing.
pub(crate) fn enumerator_new(
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::check_arity(args.len(), 0, Some(1))?;
    let Some(RubyValue::Proc(generator)) = block else {
        return Err(arg_error!("tried to create Enumerator without a block"));
    };
    let size_hint = match args.first() {
        None | Some(RubyValue::Nil) => None,
        // An Integer/Float is the size directly; a Proc/lambda is a size
        // CALLABLE, invoked lazily by `#size` (never at construction), so it is
        // stored as-is here.
        Some(
            v @ (RubyValue::Int(_)
            | RubyValue::BigInt(_)
            | RubyValue::Float(_)
            | RubyValue::Proc(_)),
        ) => Some(v.clone()),
        Some(other) => {
            return Err(type_error!(
                "can't convert {} into Integer",
                crate::builtins::convert_name_of(other)
            ));
        }
    };
    Ok(RubyValue::Enumerator(Arc::new(EnumeratorData {
        source: EnumSource::Generator { block: generator },
        size_hint,
        state: Mutex::new(ExternState::default()),
        frozen: std::sync::atomic::AtomicBool::new(false),
    })))
}

fn recv_enum(recv: &RubyValue) -> &REnumerator {
    match recv {
        RubyValue::Enumerator(e) => e,
        _ => unreachable!("Enumerator table row dispatched on a non-Enumerator receiver"),
    }
}

/// One internal iteration pass: re-invoke the captured method with
/// `block`, or hand the generator a fresh Yielder wrapping it. Returns
/// the underlying call's return value (what `StopIteration#result`
/// carries at exhaustion).
fn internal_each(source: &EnumSource, block: RubyValue) -> Result<RubyValue, Signal> {
    match source {
        EnumSource::Method { recv, meth, args } => {
            send_value(recv, Symbol::intern(meth), args, Some(block))
        }
        EnumSource::Generator { block: generator } => {
            let each_block = block.as_proc_unchecked();
            generator.call(&[RubyValue::Yielder(each_block)])
        }
        EnumSource::Produce {
            initial,
            block: generator,
        } => {
            let each_block = block.as_proc_unchecked();
            let mut cur = match initial {
                Some(v) => v.clone(),
                None => generator.call(&[RubyValue::Nil])?,
            };
            loop {
                each_block.call(&[cur.clone()])?;
                cur = generator.call(&[cur])?;
            }
        }
        EnumSource::Chain { sources } => {
            for src in sources {
                send_value(src, Symbol::intern("each"), &[], Some(block.clone()))?;
            }
            Ok(RubyValue::Nil)
        }
        EnumSource::Product { sources } => {
            let lists = product_lists(sources)?;
            let each_block = block.as_proc_unchecked();
            product_walk(&lists, &mut Vec::with_capacity(lists.len()), &each_block)?;
            Ok(RubyValue::Nil)
        }
    }
}

/// Materialize each product source once (CRuby snapshots them up front, then
/// replays the inner ones for every outer element).
fn product_lists(sources: &[RubyValue]) -> Result<Vec<Vec<RubyValue>>, Signal> {
    sources
        .iter()
        .map(
            |s| match send_value(s, Symbol::intern("to_a"), &[], None)? {
                RubyValue::Array(a) => Ok(a.lock().to_vec()),
                _ => Ok(Vec::new()),
            },
        )
        .collect()
}

/// Yield one Array per combination, rightmost source varying fastest.
fn product_walk(
    lists: &[Vec<RubyValue>],
    prefix: &mut Vec<RubyValue>,
    block: &RProc,
) -> Result<(), Signal> {
    let Some((head, rest)) = lists.split_first() else {
        return block
            .call(&[RubyValue::Array(array_new(prefix.clone()))])
            .map(|_| ());
    };
    for v in head {
        prefix.push(v.clone());
        product_walk(rest, prefix, block)?;
        prefix.pop();
    }
    Ok(())
}

/// Lazily creates the iteration fiber (CRuby's `next_init`) and returns
/// its table key. The fiber body runs the full internal `each` with the
/// shuttle block; each yielded tuple crosses back ARITY-PRESERVED as a
/// fresh Array payload (CRuby's `next_ii` packs `argc/argv` the same
/// way), and the shuttle's return value is the block's value inside the
/// iterated method (`#feed` would inject here -- Tier B, always nil).
fn ensure_fiber(e: &REnumerator) -> u64 {
    let mut st = e.state.lock();
    if let Some(id) = st.fiber {
        assert!(
            st.owner == Some(std::thread::current().id()),
            "FiberError: an Enumerator's iteration fiber can only be resumed on the thread that started it"
        );
        return id;
    }
    let id = NEXT_ITER_ID.fetch_add(1, Ordering::Relaxed);
    let source = e.source.clone();
    let coro: EnumCoro = crate::coroutine::new_fiber(move |_: Vec<RubyValue>| {
        let shuttle: RProc = RProc::new(|raw: &[RubyValue]| {
            // `y.yield` suspends, then returns the value `#feed` injected on the
            // resume (empty resume -> nil), so `got = y.yield(x)` sees it.
            let fed = crate::coroutine::yield_current::<Vec<RubyValue>, RubyValue>(
                RubyValue::Array(array_new(raw.to_vec())),
            );
            Ok(fed
                .and_then(|v| v.into_iter().next())
                .unwrap_or(RubyValue::Nil))
        });
        internal_each(&source, RubyValue::Proc(shuttle))
    });
    ENUM_FIBERS.with(|f| f.borrow_mut().insert(id, coro));
    st.fiber = Some(id);
    st.owner = Some(std::thread::current().id());
    id
}

/// Pulls one element (as its arity-preserving value vector), `Ok(None)` at
/// natural exhaustion instead of raising `StopIteration` -- the driver
/// `Enumerator::Lazy` uses to consume a source incrementally (so an infinite
/// source is only advanced as far as a `first`/`take` actually needs). A
/// StopIteration sets the enumerator's `done`; a genuine user exception does
/// not, which is how the two are told apart here.
pub(crate) fn pull_next(e: &REnumerator) -> Result<Option<Vec<RubyValue>>, Signal> {
    match get_next_values(e) {
        Ok(v) => Ok(Some(v)),
        Err(sig) => {
            if e.state.lock().done.is_some() {
                Ok(None)
            } else {
                Err(sig)
            }
        }
    }
}

/// CRuby's `get_next_values`: always ADVANCES the fiber (the lookahead
/// interplay lives in [`take_next`]/[`fill_peek`]). The state lock is
/// never held across the fiber switch -- the iterated block may touch
/// this same enumerator (e.g. call `size`).
fn get_next_values(e: &REnumerator) -> Result<Vec<RubyValue>, Signal> {
    if let Some(result) = e.state.lock().done.clone() {
        return Err(raise_stop_iteration(result));
    }
    let id = ensure_fiber(e);
    let Some(mut coro) = ENUM_FIBERS.with(|f| f.borrow_mut().remove(&id)) else {
        panic!(
            "Enumerator#next re-entered while its own iteration is running (the fiber can't resume itself)"
        );
    };
    // The same execution-context swap as `Fiber#resume` (fiber.rs): the
    // iteration runs with its own `$!`/rescue-nesting stack.
    // The fed value (if `#feed` set one) crosses in as the resume payload --
    // the shuttle returns it from the paused `y.yield`. Cleared once consumed.
    let feed_in: Vec<RubyValue> = e.state.lock().feed.take().into_iter().collect();
    let caller_ec = crate::ec::swap(std::mem::take(&mut e.state.lock().saved_ec));
    let outcome = crate::coroutine::resume(&mut coro, feed_in);
    e.state.lock().saved_ec = crate::ec::swap(caller_ec);
    match outcome {
        // The shuttle's arity-preserving Array payload -- the normal case.
        CoroutineResult::Yield(RubyValue::Array(a)) => {
            ENUM_FIBERS.with(|f| f.borrow_mut().insert(id, coro));
            let raw = a.lock().to_vec();
            Ok(raw)
        }
        // A `Fiber.yield` from user code inside the iterated `each`
        // suspends this fiber directly (module docs); its packed payload
        // surfaces as a single element.
        CoroutineResult::Yield(other) => {
            ENUM_FIBERS.with(|f| f.borrow_mut().insert(id, coro));
            Ok(vec![other])
        }
        CoroutineResult::Return(Ok(result)) => {
            let mut st = e.state.lock();
            st.fiber = None;
            st.done = Some(result.clone());
            drop(st);
            Err(raise_stop_iteration(result))
        }
        // The iteration raised (or leaked a jump): propagate it; the dead
        // fiber is CLEARED, so the following `next` re-inits and RESTARTS
        // the iteration -- CRuby's own behavior (oracle-verified).
        CoroutineResult::Return(Err(sig)) => {
            e.state.lock().fiber = None;
            Err(sig)
        }
    }
}

/// `next`'s element source: consume the lookahead if `peek` filled it,
/// else advance.
fn take_next(e: &REnumerator) -> Result<Vec<RubyValue>, Signal> {
    if let Some(v) = e.state.lock().lookahead.take() {
        return Ok(v);
    }
    get_next_values(e)
}

/// `peek`'s element source: fill (advancing once) without consuming --
/// repeated peeks return the same cached element.
fn fill_peek(e: &REnumerator) -> Result<Vec<RubyValue>, Signal> {
    if let Some(v) = e.state.lock().lookahead.clone() {
        return Ok(v);
    }
    let v = get_next_values(e)?;
    e.state.lock().lookahead = Some(v.clone());
    Ok(v)
}

/// CRuby's `ary2sv` (enumerator.c:889) -- the `next`/`peek` scalar
/// boundary: `yield` -> nil, `yield 1` -> 1, `yield 1, 2` -> [1, 2].
/// (`next_values` skips this, returning the raw tuple.)
fn ary2sv(mut vals: Vec<RubyValue>) -> RubyValue {
    match vals.len() {
        0 => RubyValue::Nil,
        1 => vals.pop().expect("len checked"),
        _ => RubyValue::Array(array_new(vals)),
    }
}

fn list(vals: &[RubyValue]) -> String {
    let rendered: Vec<String> = vals.iter().map(|v| v.inspect_string()).collect();
    format!("[{}]", rendered.join(", "))
}

pub(crate) fn enum_inspect(e: &EnumeratorData) -> String {
    match &e.source {
        // CRuby prints the generator with its address (the conformance test
        // normalizes it to `0xADDR`), so this one carries one.
        EnumSource::Generator { .. } => format!(
            "#<Enumerator: #<Enumerator::Generator:0x{:016x}>:each>",
            e as *const EnumeratorData as usize
        ),
        EnumSource::Produce { .. } => "#<Enumerator: #<Enumerator::Producer>:each>".to_string(),
        // A chain/product prints its sources verbatim (CRuby renders the
        // held array, so `[1,2].chain([3])` shows the arrays themselves
        // while `a.each + b.each` shows the two enumerators).
        EnumSource::Chain { sources } => format!("#<Enumerator::Chain: {}>", list(sources)),
        EnumSource::Product { sources } => format!("#<Enumerator::Product: {}>", list(sources)),
        EnumSource::Method { recv, meth, args } => {
            let mut s = format!("#<Enumerator: {}:{meth}", recv.inspect_string());
            if !args.is_empty() {
                let last = args.len() - 1;
                let rendered: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| match a {
                        // A trailing symbol-keyed Hash is keyword arguments:
                        // render its pairs bare (`chomp: true`), not `{…}`.
                        RubyValue::Hash(h) if i == last => {
                            render_kwargs(h).unwrap_or_else(|| a.inspect_string())
                        }
                        _ => a.inspect_string(),
                    })
                    .collect();
                s.push('(');
                s.push_str(&rendered.join(", "));
                s.push(')');
            }
            s.push('>');
            s
        }
    }
}

/// Render a symbol-keyed Hash as bare keyword arguments (`k: v, ...`) for an
/// enumerator's inspect; `None` if any key is not a Symbol (so it prints as a
/// normal `{…}` positional hash instead).
fn render_kwargs(h: &crate::RHash) -> Option<String> {
    let g = h.lock();
    if g.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for (k, v) in g.values() {
        let RubyValue::Symbol(s) = k else {
            return None;
        };
        parts.push(format!("{}: {}", s.name(), v.inspect_string()));
    }
    Some(parts.join(", "))
}

/// Lazy size (never iterates, CRuby's rule): the generator's stored hint,
/// or a size derived from the captured method. The same-size set covers
/// the methods whose enumerator has exactly the receiver's element count;
/// nil for everything else (a few shapes CRuby computes -- e.g.
/// `each_slice` over an infinite range -- return nil here, documented).
fn enum_size(e: &EnumeratorData) -> RubyValue {
    match &e.source {
        // A stored size CALLABLE is invoked lazily here (CRuby calls it from
        // `#size`); a plain Integer/Float hint answers directly; none -> nil.
        EnumSource::Generator { .. } => match &e.size_hint {
            Some(RubyValue::Proc(p)) => p.call(&[]).unwrap_or(RubyValue::Nil),
            Some(v) => v.clone(),
            None => RubyValue::Nil,
        },
        // A produced sequence is endless -> Float::INFINITY (CRuby's rule).
        EnumSource::Produce { .. } => RubyValue::Float(f64::INFINITY),
        EnumSource::Method { recv, meth, args } => match meth.as_str() {
            "each" | "map" | "collect" | "select" | "filter" | "find_all" | "reject"
            | "sort_by" | "min_by" | "max_by" | "group_by" | "partition" | "flat_map"
            | "collect_concat" | "each_with_index" | "each_with_object" | "with_index"
            | "with_object" | "each_char" | "each_key" | "each_value" | "each_pair"
            | "each_index" | "map!" | "select!" | "reject!" | "transform_keys"
            | "transform_values" => receiver_size(recv),
            "times" => recv.clone(),
            "upto" | "downto" => int_span(recv, args.first(), meth == "upto"),
            "each_slice" | "each_cons" => {
                let (RubyValue::Int(size), Some(RubyValue::Int(n))) =
                    (receiver_size(recv), args.first())
                else {
                    return RubyValue::Nil;
                };
                if *n <= 0 {
                    return RubyValue::Nil;
                }
                if meth == "each_slice" {
                    RubyValue::Int((size + n - 1) / n)
                } else {
                    RubyValue::Int((size - n + 1).max(0))
                }
            }
            _ => RubyValue::Nil,
        },
        // A chain is as long as its parts summed; a product is their lengths
        // multiplied. Either is nil if any part's size is unknown, and
        // infinite if any part is (CRuby's rule).
        EnumSource::Chain { sources } => fold_sizes(sources, 0, |acc, n| acc + n),
        EnumSource::Product { sources } => fold_sizes(sources, 1, |acc, n| acc * n),
    }
}

/// Fold the sources' sizes with `f`, propagating nil (unknown) and
/// Float::INFINITY (endless) rather than folding them numerically.
fn fold_sizes(sources: &[RubyValue], identity: i64, f: impl Fn(i64, i64) -> i64) -> RubyValue {
    let mut acc = identity;
    for src in sources {
        match receiver_size(src) {
            RubyValue::Int(n) => acc = f(acc, n),
            RubyValue::Float(x) if x.is_infinite() => return RubyValue::Float(x),
            _ => return RubyValue::Nil,
        }
    }
    RubyValue::Int(acc)
}

fn receiver_size(recv: &RubyValue) -> RubyValue {
    match recv {
        RubyValue::Array(_) | RubyValue::Hash(_) | RubyValue::Str(_) | RubyValue::Range(..) => {
            send_value(recv, Symbol::intern("size"), &[], None).unwrap_or(RubyValue::Nil)
        }
        RubyValue::Enumerator(inner) => enum_size(inner),
        _ => RubyValue::Nil,
    }
}

/// `n.upto(m)` / `n.downto(m)` element counts, over Integer/Bignum endpoints
/// (a Float or other endpoint answers nil). Computed as BigInt so a bignum
/// range (`(2**100).downto(2**100 - 2).size`) answers its true small count.
fn int_span(recv: &RubyValue, to: Option<&RubyValue>, ascending: bool) -> RubyValue {
    use num_bigint::BigInt;
    let big = |v: &RubyValue| -> Option<BigInt> {
        match v {
            RubyValue::Int(n) => Some(BigInt::from(*n)),
            RubyValue::BigInt(b) => Some((**b).clone()),
            _ => None,
        }
    };
    let (Some(a), Some(b)) = (big(recv), to.and_then(big)) else {
        return RubyValue::Nil;
    };
    let one = BigInt::from(1);
    let span = if ascending { b - a + one } else { a - b + one };
    crate::builtins::integer::int_value(span.max(BigInt::from(0)))
}

/// `with_index(offset)`/`each_with_index`'s driver: re-runs the internal
/// iteration with a counting wrapper -- multi-value yields pack into an
/// array as the block's first param, the index appends (CRuby's
/// `enumerator_with_index_i`).
fn drive_with_index(e: &REnumerator, block: RubyValue, offset: i64) -> Result<RubyValue, Signal> {
    let blk = block.as_proc_unchecked();
    let counter = Arc::new(Mutex::new(offset));
    let wrapper: RProc = RProc::new(move |raw: &[RubyValue]| {
        let el = pack(raw);
        let i = {
            let mut c = counter.lock();
            let i = *c;
            *c += 1;
            i
        };
        blk.call(&[el, RubyValue::Int(i)])
    });
    internal_each(&e.source, RubyValue::Proc(wrapper))
}

/// `with_object(memo)`/`each_with_object`'s driver: yields
/// `(packed_element, memo)` and returns the memo.
fn drive_with_object(
    recv: &RubyValue,
    obj: &RubyValue,
    block: Option<RubyValue>,
    label: &str,
) -> Result<RubyValue, Signal> {
    let e = recv_enum(recv);
    let Some(block) = block else {
        return Ok(enumerator_for(recv, label, std::slice::from_ref(obj)));
    };
    let blk = block.as_proc_unchecked();
    let memo = obj.clone();
    let memo_for_block = memo.clone();
    let wrapper: RProc =
        RProc::new(move |raw: &[RubyValue]| blk.call(&[pack(raw), memo_for_block.clone()]));
    internal_each(&e.source, RubyValue::Proc(wrapper))?;
    Ok(memo)
}

ruby_class! {
    Enumerator = zeo_abi::ENUMERATOR_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // `Enumerator.produce([initial]) { |prev| ... }` -- an endless generator
    // (#2483). With `initial`, that value is yielded first; then each block
    // result is yielded, forever (bounded by the consumer, e.g. `take`/`first`).
    def self."produce"(_recv, arg?, &block) {
        let Some(RubyValue::Proc(generator)) = block else {
            return Err(arg_error!("tried to create Producer without a block"));
        };
        Ok(RubyValue::Enumerator(Arc::new(EnumeratorData {
            source: EnumSource::Produce { initial: arg.cloned(), block: generator },
            size_hint: None,
            state: Mutex::new(ExternState::default()),
            frozen: std::sync::atomic::AtomicBool::new(false),
        })))
    }

    // `Enumerator.product(*enums)` -- every combination as an Array, rightmost
    // source varying fastest (#2484). No args yields one empty combination.
    def self."product"(_recv, *args, &_block) {
        Ok(RubyValue::Enumerator(Arc::new(EnumeratorData {
            source: EnumSource::Product { sources: args.to_vec() },
            size_hint: None,
            state: Mutex::new(ExternState::default()),
            frozen: std::sync::atomic::AtomicBool::new(false),
        })))
    }

    def "each"(recv, *args, &block) {
        let e = recv_enum(recv);
        if !args.is_empty() {
            panic!("Enumerator#each with extra arguments isn't supported yet (zeo limitation; CRuby appends them to the captured args on a dup)");
        }
        match block {
            // Re-invoke the captured method with the caller's block; the
            // return value is the underlying method's own.
            Some(b) => internal_each(&e.source, b),
            // Blockless `each` returns SELF (`equal?`-identical, oracle).
            None => Ok(recv.clone()),
        }
    }

    // `e + other` -- an `Enumerator::Chain` over the two, in order. Chaining
    // a chain nests rather than flattens, matching CRuby.
    def "+"(recv, other) {
        Ok(chain_of(vec![recv.clone(), (*other).clone()]))
    }

    def "next"(recv) {
        Ok(ary2sv(take_next(recv_enum(recv))?))
    }

    def "next_values"(recv) {
        Ok(RubyValue::Array(array_new(take_next(recv_enum(recv))?)))
    }

    def "peek"(recv) {
        Ok(ary2sv(fill_peek(recv_enum(recv))?))
    }

    def "peek_values"(recv) {
        Ok(RubyValue::Array(array_new(fill_peek(recv_enum(recv))?)))
    }

    // `#feed(value)` -- set the value the generator's paused `y.yield` returns
    // on the next `#next`. Setting it twice before a `#next` consumes it is a
    // TypeError; the call itself answers nil.
    def "feed"(recv, arg) {
        let mut st = recv_enum(recv).state.lock();
        if st.feed.is_some() {
            return Err(type_error!("feed value already set"));
        }
        st.feed = Some((*arg).clone());
        Ok(RubyValue::Nil)
    }

    def "rewind"(recv) {
        let e = recv_enum(recv);
        let mut st = e.state.lock();
        if let Some(id) = st.fiber.take() {
            // The dropped coroutine force-unwinds (Rust destructors only
            // -- fiber.rs's module docs). A fiber pinned to ANOTHER
            // thread can't be removed from here; it unwinds when that
            // thread's table drops (documented leak-until-thread-exit).
            if st.owner == Some(std::thread::current().id()) {
                ENUM_FIBERS.with(|f| f.borrow_mut().remove(&id));
            }
        }
        st.owner = None;
        st.lookahead = None;
        st.done = None;
        st.saved_ec = crate::ec::Ec::default();
        // CRuby also calls the receiver's own `rewind` hook when it
        // responds -- Tier B (rare protocol; documented).
        Ok(recv.clone())
    }

    def "size"(recv) {
        Ok(enum_size(recv_enum(recv)))
    }

    // `Enumerator#to_s` is inherited `Object#to_s` in CRuby but prints the
    // same `#<Enumerator: ...>` shape via #inspect in practice; sharing
    // one implementation matches the observable output.
    def "inspect" | "to_s" (recv) {
        Ok(RubyValue::Str(crate::string_new(enum_inspect(recv_enum(recv)))))
    }

    def "with_index"(recv, offset?, &block) {
        let e = recv_enum(recv);
        let offset = match offset {
            // An explicit nil offset is accepted as absent (oracle:
            // `with_index(nil)` starts at 0).
            None | Some(RubyValue::Nil) => 0,
            Some(other) => crate::builtins::convert::to_index(other)?,
        };
        match block {
            Some(b) => drive_with_index(e, b, offset),
            // Blockless: a wrapping enumerator over SELF -- iterating it
            // re-dispatches this very row with a block.
            None => Ok(enumerator_for(recv, "with_index", __args)),
        }
    }

    def "each_with_index"(recv, &block) {
        let e = recv_enum(recv);
        match block {
            Some(b) => drive_with_index(e, b, 0),
            None => Ok(enumerator_for(recv, "each_with_index", &[])),
        }
    }

    def "with_object"(recv, obj, &block) {
        drive_with_object(recv, obj, block, "with_object")
    }

    def "each_with_object"(recv, obj, &block) {
        drive_with_object(recv, obj, block, "each_with_object")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `Enumerator` instance methods are `ruby_class!`-generated (their
    /// Rust fn names are mangled), so the tests reach them the way dispatch
    /// does -- through the registered instance table.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::ENUMERATOR_CLASS)
            .expect("Enumerator is a registered builtin table")
            .instance
            .as_ref()
            .expect("Enumerator has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Enumerator#{name} is defined"))
    }
    fn each(
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        imethod("each")(recv, args, block)
    }
    fn rewind(
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        imethod("rewind")(recv, args, block)
    }
    fn size(
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        imethod("size")(recv, args, block)
    }
    fn with_index(
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        imethod("with_index")(recv, args, block)
    }

    fn ints(ns: &[i64]) -> RubyValue {
        RubyValue::Array(array_new(ns.iter().map(|&n| RubyValue::Int(n)).collect()))
    }

    fn collecting_block() -> (RubyValue, Arc<Mutex<Vec<RubyValue>>>) {
        let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = out.clone();
        let blk: RProc = RProc::new(move |raw: &[RubyValue]| {
            sink.lock().push(pack(raw));
            Ok(RubyValue::Nil)
        });
        (RubyValue::Proc(blk), out)
    }

    #[test]
    fn method_backed_internal_each_reinvokes_the_source() {
        let e = enumerator_for(&ints(&[1, 2, 3]), "each", &[]);
        let (blk, out) = collecting_block();
        let ret = each(&e, &[], Some(blk)).unwrap();
        // Array#each returns the array itself.
        assert!(matches!(ret, RubyValue::Array(_)));
        let got = out.lock();
        assert_eq!(got.len(), 3);
        assert!(matches!(got[0], RubyValue::Int(1)));
    }

    #[test]
    fn blockless_each_returns_self_identically() {
        let e = enumerator_for(&ints(&[1]), "each", &[]);
        let same = each(&e, &[], None).unwrap();
        let (RubyValue::Enumerator(a), RubyValue::Enumerator(b)) = (&e, &same) else {
            panic!("expected enumerators");
        };
        assert!(Arc::ptr_eq(a, b));
    }

    #[test]
    fn external_iteration_advances_and_peek_caches() {
        let e = enumerator_for(&ints(&[7, 8, 9]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else {
            panic!()
        };
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(7)));
        assert!(matches!(ary2sv(fill_peek(h).unwrap()), RubyValue::Int(8)));
        assert!(matches!(ary2sv(fill_peek(h).unwrap()), RubyValue::Int(8)));
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(8)));
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(9)));
    }

    #[test]
    #[should_panic(expected = "StopIteration")]
    fn exhaustion_raises_stop_iteration() {
        // No exception factory in unit tests: raise_stop_iteration panics
        // loudly instead (the raise_error posture).
        let e = enumerator_for(&ints(&[1]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else {
            panic!()
        };
        take_next(h).unwrap();
        let _ = take_next(h);
    }

    #[test]
    fn generator_yielder_preserves_arity_for_next_values() {
        // Enumerator.new { |y| y.yield; y.yield nil; y.yield 1, 2 }
        let generator: RProc = RProc::new(|args: &[RubyValue]| {
            let y = &args[0];
            let RubyValue::Yielder(f) = y else {
                panic!("expected a Yielder")
            };
            f.call(&[])?;
            f.call(&[RubyValue::Nil])?;
            f.call(&[RubyValue::Int(1), RubyValue::Int(2)])?;
            Ok(RubyValue::Nil)
        });
        let e = enumerator_new(&[], Some(RubyValue::Proc(generator))).unwrap();
        let RubyValue::Enumerator(h) = &e else {
            panic!()
        };
        assert_eq!(take_next(h).unwrap().len(), 0); // yield        -> []
        assert_eq!(take_next(h).unwrap().len(), 1); // yield nil    -> [nil]
        let two = take_next(h).unwrap(); //            yield 1, 2   -> [1, 2]
        assert_eq!(two.len(), 2);
        assert!(matches!(two[1], RubyValue::Int(2)));
    }

    #[test]
    fn a_failed_iteration_propagates_then_restarts() {
        // Enumerator.new { |y| y << 1; raise } -- next -> 1, next -> the
        // error, next again -> a fresh fiber restarting at 1 (oracle).
        let generator: RProc = RProc::new(|args: &[RubyValue]| {
            let RubyValue::Yielder(f) = &args[0] else {
                panic!()
            };
            f.call(&[RubyValue::Int(1)])?;
            Err(Signal::Raise(RubyValue::Int(99)))
        });
        let e = enumerator_new(&[], Some(RubyValue::Proc(generator))).unwrap();
        let RubyValue::Enumerator(h) = &e else {
            panic!()
        };
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(1)));
        assert!(matches!(
            take_next(h),
            Err(Signal::Raise(RubyValue::Int(99)))
        ));
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(1)));
    }

    #[test]
    fn rewind_restarts_from_the_top() {
        let e = enumerator_for(&ints(&[5, 6]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else {
            panic!()
        };
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(5)));
        rewind(&e, &[], None).unwrap();
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(5)));
    }

    #[test]
    fn size_derives_from_the_source_without_iterating() {
        assert!(matches!(
            size(&enumerator_for(&ints(&[1, 2, 3]), "map", &[]), &[], None).unwrap(),
            RubyValue::Int(3)
        ));
        assert!(matches!(
            size(&enumerator_for(&RubyValue::Int(5), "times", &[]), &[], None).unwrap(),
            RubyValue::Int(5)
        ));
        assert!(matches!(
            size(
                &enumerator_for(&ints(&[1, 2, 3]), "each_slice", &[RubyValue::Int(2)]),
                &[],
                None
            )
            .unwrap(),
            RubyValue::Int(2)
        ));
        // Generator without a hint: nil; with one: the hint.
        let generator: RProc = RProc::new(|_| Ok(RubyValue::Nil));
        let bare = enumerator_new(&[], Some(RubyValue::Proc(generator.clone()))).unwrap();
        assert!(matches!(size(&bare, &[], None).unwrap(), RubyValue::Nil));
        let hinted =
            enumerator_new(&[RubyValue::Int(4)], Some(RubyValue::Proc(generator))).unwrap();
        assert!(matches!(
            size(&hinted, &[], None).unwrap(),
            RubyValue::Int(4)
        ));
    }

    #[test]
    fn inspect_prints_the_cruby_shape() {
        let e = enumerator_for(&ints(&[1, 2]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else {
            panic!()
        };
        assert_eq!(enum_inspect(h), "#<Enumerator: [1, 2]:each>");
        let sliced = enumerator_for(&ints(&[1, 2]), "each_slice", &[RubyValue::Int(2)]);
        let RubyValue::Enumerator(h) = &sliced else {
            panic!()
        };
        assert_eq!(enum_inspect(h), "#<Enumerator: [1, 2]:each_slice(2)>");
    }

    #[test]
    fn with_index_counts_from_the_offset() {
        let e = enumerator_for(&ints(&[10, 20]), "each", &[]);
        let out: Arc<Mutex<Vec<(i64, i64)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = out.clone();
        let blk: RProc = RProc::new(move |raw: &[RubyValue]| {
            let (RubyValue::Int(v), RubyValue::Int(i)) = (&raw[0], &raw[1]) else {
                panic!("expected (value, index)");
            };
            sink.lock().push((*v, *i));
            Ok(RubyValue::Nil)
        });
        with_index(&e, &[RubyValue::Int(5)], Some(RubyValue::Proc(blk))).unwrap();
        assert_eq!(&*out.lock(), &[(10, 5), (20, 6)]);
    }

    #[test]
    fn dup_copies_the_source_but_not_the_iteration() {
        let e = enumerator_for(&ints(&[1, 2]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else {
            panic!()
        };
        take_next(h).unwrap();
        assert!(h.iteration_live());
        let copy = h.fresh_copy();
        assert!(!copy.iteration_live());
        assert!(matches!(
            ary2sv(take_next(&copy).unwrap()),
            RubyValue::Int(1)
        ));
        // The original is unaffected: still at element 2.
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(2)));
    }
}
