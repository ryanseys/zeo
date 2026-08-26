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
use crate::builtins::{arg_error, arg_int, frozen_error, inherited_row, need_block, type_error};
use crate::collections::array_new;
use crate::coroutine::CoroutineResult;
use crate::dispatch::{raise_stop_iteration, send_value};
use crate::signal::Signal;
use crate::value::RubyValue;
use crate::{RProc, Symbol};
use parking_lot::Mutex;
use std::cell::RefCell;
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
        /// Whether the METHOD ITSELF built this enumerator (`block_or_enum!`,
        /// CRuby's `RETURN_SIZED_ENUMERATOR`) rather than an explicit
        /// `to_enum`/`enum_for`. Only the first supplies a size function, so
        /// `[1, 2].each.size` is 2 while `[1, 2].to_enum(:each).size` is nil.
        sized: bool,
    },
    /// `Enumerator::Generator` -- the object `Enumerator.new { |y| ... }`
    /// holds as its source, and which `Enumerator::Generator.new` builds
    /// directly. Not an Enumerator: it answers `#each` and what `Enumerable`
    /// derives from it, and nothing else.
    Generator { block: RProc },
    /// `Enumerator::Producer` -- what `Enumerator.produce(initial) { |prev| }`
    /// holds. The first yielded value is `initial` (or, absent,
    /// `block.call(nil)`); each subsequent one is `block` applied to the
    /// previous, forever.
    Produce {
        initial: Option<RubyValue>,
        block: RProc,
    },
    /// `Enumerator#+` / `Enumerable#chain` -- the sources iterated back to
    /// back. Carried as an ordinary Enumerator; `class_of` reports
    /// `Enumerator::Chain` off this variant.
    Chain { sources: Vec<RubyValue> },
    /// `Enumerator.product(*enums)` -- the cartesian product, yielded as one
    /// Array per combination, rightmost source varying fastest.
    Product { sources: Vec<RubyValue> },
    /// `Enumerator::ArithmeticSequence` -- a blockless `Range#step`,
    /// `Range#%` or `Numeric#step` over a numeric receiver. It is an
    /// Enumerator that also KNOWS its quadruple, which is what lets `#size`
    /// and `#last` compute instead of iterate.
    ArithSeq {
        /// The receiver and call `#inspect` replays -- purely cosmetic, but
        /// CRuby renders `((1..10).step(2))` and `(1.step(10, 3))`
        /// differently, and only the original call can tell them apart.
        recv: RubyValue,
        meth: &'static str,
        args: Vec<RubyValue>,
        begin: RubyValue,
        /// `Nil` for an endless sequence.
        end: RubyValue,
        step: RubyValue,
        exclude_end: bool,
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

/// The replaceable half of an enumerator: its source plus the
/// `Enumerator.new(size) { ... }` hint. One value so the private
/// `#initialize`/`#initialize_copy` rows can swap both atomically.
#[derive(Clone)]
struct EnumCore {
    source: EnumSource,
    /// `Enumerator.new(size) { ... }`'s stored hint; method-backed
    /// enumerators derive size lazily from their source instead.
    size_hint: Option<RubyValue>,
}

pub struct EnumeratorData {
    /// The construction-time core. Its VARIANT KIND is authoritative for the
    /// enumerator's class -- re-init never changes it (the re-init rows are
    /// same-class checked, and an ArithmeticSequence refuses re-init) -- so
    /// `enumerator_class_id` and `arith_seq_parts` read it without a lock.
    seed: EnumCore,
    /// `#initialize`/`#initialize_copy`'s in-place replacement, if any.
    /// Everything that iterates or describes the enumerator reads through
    /// [`Self::core`], which folds this in.
    reinit: Mutex<Option<EnumCore>>,
    state: Mutex<ExternState>,
    /// `.frozen?` state -- flag-only (CRuby happily iterates a frozen
    /// enumerator; external-iteration state isn't Ruby-visible mutation).
    frozen: std::sync::atomic::AtomicBool,
}

pub type REnumerator = Arc<EnumeratorData>;

impl EnumeratorData {
    fn new(source: EnumSource, size_hint: Option<RubyValue>) -> EnumeratorData {
        EnumeratorData {
            seed: EnumCore { source, size_hint },
            reinit: Mutex::new(None),
            state: Mutex::new(ExternState::default()),
            frozen: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The CURRENT core -- the re-init replacement when one was stored, else
    /// the seed. Cloned out (everything inside is Arc-backed or small) so no
    /// lock is held while iteration runs.
    fn core(&self) -> EnumCore {
        self.reinit
            .lock()
            .clone()
            .unwrap_or_else(|| self.seed.clone())
    }

    /// The current source alone -- the common read.
    fn source(&self) -> EnumSource {
        self.core().source
    }

    /// Whether external iteration has begun and not finished -- what
    /// makes `dup` unsafe (CRuby: "can't copy execution context").
    pub(crate) fn iteration_live(&self) -> bool {
        self.state.lock().fiber.is_some()
    }

    /// A fresh, never-iterated enumerator over the same source --
    /// `dup`/`clone`'s payload (which starts unfrozen; `clone`'s flag copy
    /// is `dup_value`'s job).
    pub(crate) fn fresh_copy(&self) -> REnumerator {
        let core = self.core();
        Arc::new(EnumeratorData::new(core.source, core.size_hint))
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

/// Genuinely the same instantiation as `fiber.rs`'s `FiberCoro` -- the
/// matching TypeIds are what let a user `Fiber.yield` inside an iterated
/// block suspend this fiber directly (the coroutine shim's invariant-3
/// check compares `(Input, Yield)` pairs; the earlier `Vec<RubyValue>`
/// input CLAIMED this and didn't have it), and what lets
/// `fiber::terminate_coro` dispose of either kind.
type EnumCoro = crate::fiber::FiberCoro;

thread_local! {
    static ENUM_FIBERS: RefCell<crate::fiber::CoroTable> =
        RefCell::new(crate::fiber::CoroTable::new());
}

/// The Terminate-protocol drain for this thread's Enumerator iteration
/// fibers -- `fiber::terminate_thread_fibers`' sibling, called from the
/// same thread tails.
pub fn terminate_thread_enum_fibers() {
    let drained: Vec<EnumCoro> = ENUM_FIBERS.with(|f| {
        let mut t = f.borrow_mut();
        let keys: Vec<u64> = t.keys().copied().collect();
        keys.iter().filter_map(|k| t.remove(k)).collect()
    });
    for coro in drained {
        crate::fiber::terminate_coro(coro);
    }
}

/// Process-wide so ids stay unambiguous even if handles travel between
/// threads (only RESUMING is thread-pinned, not holding).
static NEXT_ITER_ID: AtomicU64 = AtomicU64::new(1);

/// The one constructor every blockless iteration method funnels through
/// (`rb_enumeratorize`): captures the receiver, the method to re-invoke,
/// and the trailing args -- nothing else.
pub(crate) fn enumerator_for(recv: &RubyValue, meth: &str, args: &[RubyValue]) -> RubyValue {
    // The METHOD's own blockless return, so it carries a size function.
    enumerator_for_sized(recv, meth, args, None, true)
}

/// `to_enum`/`enum_for` with the optional block that SUPPLIES the size
/// lazily. CRuby writes the reified block straight into the same `size` slot
/// an Integer would occupy (`obj_to_enum`), and `#size` probes it with `call`
/// -- one field, no type dispatch, and the count is computed only if someone
/// asks.
pub(crate) fn enumerator_for_with_size(
    recv: &RubyValue,
    meth: &str,
    args: &[RubyValue],
    size_hint: Option<RubyValue>,
) -> RubyValue {
    enumerator_for_sized(recv, meth, args, size_hint, false)
}

/// [`enumerator_for_with_size`] plus the `sized` flag -- see
/// [`EnumSource::Method::sized`].
pub(crate) fn enumerator_for_sized(
    recv: &RubyValue,
    meth: &str,
    args: &[RubyValue],
    size_hint: Option<RubyValue>,
    sized: bool,
) -> RubyValue {
    RubyValue::Enumerator(Arc::new(EnumeratorData::new(
        EnumSource::Method {
            recv: recv.clone(),
            meth: meth.to_string(),
            args: args.to_vec(),
            sized,
        },
        size_hint,
    )))
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
    enumerator_over(generator_object(generator), None)
}

/// `Enumerator::Chain` over `sources`, iterated back to back. The public
/// constructor behind `Enumerator#+` and `Enumerable#chain`.
pub(crate) fn chain_of(sources: Vec<RubyValue>) -> RubyValue {
    RubyValue::Enumerator(Arc::new(EnumeratorData::new(
        EnumSource::Chain { sources },
        None,
    )))
}

/// `Enumerator::ArithmeticSequence` over `(begin, end, step, exclude_end)`.
/// `recv`/`meth`/`args` are the call that produced it, which only `#inspect`
/// reads back.
pub(crate) fn arith_seq_of(
    recv: &RubyValue,
    meth: &'static str,
    args: &[RubyValue],
    begin: RubyValue,
    end: RubyValue,
    step: RubyValue,
    exclude_end: bool,
) -> RubyValue {
    RubyValue::Enumerator(Arc::new(EnumeratorData::new(
        EnumSource::ArithSeq {
            recv: recv.clone(),
            meth,
            args: args.to_vec(),
            begin,
            end,
            step,
            exclude_end,
        },
        None,
    )))
}

/// The `(begin, end, step, exclude_end)` quadruple, for a value that is an
/// arithmetic sequence -- what `Array#[]` slices with and what the
/// `Enumerator::ArithmeticSequence` rows read. `None` for any other value,
/// including a plain Enumerator.
pub(crate) fn arith_seq_parts(
    v: &RubyValue,
) -> Option<(&RubyValue, Option<&RubyValue>, &RubyValue, bool)> {
    let RubyValue::Enumerator(e) = v else {
        return None;
    };
    // The seed is authoritative here: an ArithmeticSequence refuses re-init
    // (see `#initialize`), so its quadruple can be borrowed without a lock.
    let EnumSource::ArithSeq {
        begin,
        end,
        step,
        exclude_end,
        ..
    } = &e.seed.source
    else {
        return None;
    };
    let end = match end {
        RubyValue::Nil => None,
        other => Some(other),
    };
    Some((begin, end, step, *exclude_end))
}

/// The class an enumerator reports: the source variant picks
/// `Enumerator::Chain`/`Enumerator::Product`/`::ArithmeticSequence` over
/// plain `Enumerator` (see `value.rs`).
pub fn enumerator_class_id(e: &REnumerator) -> crate::ClassId {
    // The seed's variant kind never changes (re-init is same-class checked),
    // so the class read stays lock-free.
    match e.seed.source {
        EnumSource::Chain { .. } => zeo_abi::ENUMERATOR_CHAIN_CLASS,
        EnumSource::Product { .. } => zeo_abi::ENUMERATOR_PRODUCT_CLASS,
        EnumSource::ArithSeq { .. } => zeo_abi::ENUMERATOR_ARITHMETIC_SEQUENCE_CLASS,
        // A generator/producer is the enumerator's SOURCE, not an enumerator
        // -- `Enumerator.new { }` holds one and delegates `each` to it.
        EnumSource::Generator { .. } => zeo_abi::ENUMERATOR_GENERATOR_CLASS,
        EnumSource::Produce { .. } => zeo_abi::ENUMERATOR_PRODUCER_CLASS,
        _ => zeo_abi::ENUMERATOR_CLASS,
    }
}

/// Wrap a generator/producer as the SOURCE of an ordinary Enumerator, which
/// is the pairing CRuby builds: `Enumerator.new { }` answers an Enumerator
/// whose `each` re-invokes the generator's.
fn enumerator_over(source: RubyValue, size_hint: Option<RubyValue>) -> RubyValue {
    RubyValue::Enumerator(Arc::new(EnumeratorData::new(
        EnumSource::Method {
            recv: source,
            meth: "each".to_string(),
            args: Vec::new(),
            sized: true,
        },
        size_hint,
    )))
}

/// A bare `Enumerator::Generator` over `block`.
fn generator_object(block: RProc) -> RubyValue {
    RubyValue::Enumerator(Arc::new(EnumeratorData::new(
        EnumSource::Generator { block },
        None,
    )))
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
    Ok(enumerator_over(generator_object(generator), size_hint))
}

fn recv_enum(recv: &RubyValue) -> &REnumerator {
    match recv {
        RubyValue::Enumerator(e) => e,
        _ => unreachable!("Enumerator table row dispatched on a non-Enumerator receiver"),
    }
}

/// `StopIteration` raised inside a producer block ENDS the sequence; it is not
/// an error. `Enumerator.produce` has no length and no terminating predicate,
/// so raising it is the only way to say the sequence is over -- CRuby wraps the
/// whole generation in `rb_rescue2(..., rb_eStopIteration)` (`producer_each`)
/// and answers the exception's `result`. Subclasses count, exactly as a
/// `rescue StopIteration` clause would; every other signal passes through, so a
/// `break` from a consumer still unwinds and a real error still raises.
fn ended_by_stop_iteration(sig: Signal) -> Result<RubyValue, Signal> {
    let Signal::Raise(exc) = &sig else {
        return Err(sig);
    };
    let RubyValue::Object(o) = exc else {
        return Err(sig);
    };
    if !crate::dispatch::is_a(o.class_id(), zeo_abi::STOP_ITERATION_CLASS) {
        return Err(sig);
    }
    send_value(exc, Symbol::intern("result"), &[], None)
}

/// One internal iteration pass: re-invoke the captured method with
/// `block`, or hand the generator a fresh Yielder wrapping it. Returns
/// the underlying call's return value (what `StopIteration#result`
/// carries at exhaustion).
fn internal_each(source: &EnumSource, block: RubyValue) -> Result<RubyValue, Signal> {
    match source {
        EnumSource::Method {
            recv, meth, args, ..
        } => send_value(recv, Symbol::intern(meth), args, Some(block)),
        EnumSource::Generator { block: generator } => {
            let each_block = block.as_proc_unchecked();
            generator.call(&[RubyValue::Yielder(each_block)])
        }
        EnumSource::Produce {
            initial,
            block: generator,
        } => {
            let each_block = block.as_proc_unchecked();
            let produce = || -> Result<RubyValue, Signal> {
                let mut cur = match initial {
                    Some(v) => v.clone(),
                    None => generator.call(&[RubyValue::Nil])?,
                };
                loop {
                    each_block.call(&[cur.clone()])?;
                    cur = generator.call(&[cur])?;
                }
            };
            produce().or_else(ended_by_stop_iteration)
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
        EnumSource::ArithSeq { .. } => {
            let each_block = block.as_proc_unchecked();
            arith_walk(source, |v| {
                each_block.call(std::slice::from_ref(v))?;
                Ok(())
            })?;
            Ok(RubyValue::Nil)
        }
    }
}

/// Walk an `ArithSeq` source through the one shared numeric walk, so a
/// sequence's `to_a` matches the `Range#step`/`Numeric#step` block form that
/// would have built it.
fn arith_walk(
    source: &EnumSource,
    f: impl FnMut(&RubyValue) -> Result<(), Signal>,
) -> Result<(), Signal> {
    let EnumSource::ArithSeq {
        begin,
        end,
        step,
        exclude_end,
        ..
    } = source
    else {
        unreachable!("arith_walk on a non-ArithSeq source");
    };
    let end = match end {
        RubyValue::Nil => None,
        other => Some(other),
    };
    crate::builtins::numeric::step_walk(begin, end, step, *exclude_end, f)
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
    let source = e.source();
    let coro: EnumCoro = crate::coroutine::new_fiber(move |first: crate::fiber::FiberInput| {
        // See `fiber_new`: the coroutine stack needs its own overflow floor.
        crate::stack_guard::set_floor(crate::stack_guard::fiber_floor_here());
        // Torn down before the iteration ever ran: nothing to unwind. Any
        // other first input carries no information here (the fed value only
        // matters at a suspended `y.yield`).
        if matches!(first, crate::fiber::FiberInput::Terminate) {
            return Err(Signal::Terminate);
        }
        let shuttle: RProc = RProc::new(|raw: &[RubyValue]| {
            // `y.yield` suspends, then returns the value `#feed` injected on the
            // resume (empty resume -> nil), so `got = y.yield(x)` sees it.
            let fed = match crate::coroutine::yield_current::<crate::fiber::FiberInput, RubyValue>(
                RubyValue::Array(array_new(raw.to_vec())),
            ) {
                None => Vec::new(),
                Some(crate::fiber::FiberInput::Resume(vals))
                | Some(crate::fiber::FiberInput::Transfer(vals)) => vals,
                Some(crate::fiber::FiberInput::Raise(exc)) => return Err(Signal::Raise(exc)),
                Some(crate::fiber::FiberInput::Terminate) => return Err(Signal::Terminate),
            };
            Ok(fed.into_iter().next().unwrap_or(RubyValue::Nil))
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
    let outcome = crate::coroutine::resume(&mut coro, crate::fiber::FiberInput::Resume(feed_in));
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

/// Drops any external-iteration state (fiber, lookahead, fed value, parked
/// result) -- the re-init rows discard the position exactly as a fresh
/// enumerator would start.
fn clear_iteration(e: &EnumeratorData) {
    // The coroutine is taken out UNDER the lock but disposed of outside it:
    // the Terminate resume drops arbitrary values, and a drop that reaches
    // back into this enumerator must not deadlock on `state`.
    let coro = {
        let mut st = e.state.lock();
        let taken = match st.fiber.take() {
            Some(id) if st.owner == Some(std::thread::current().id()) => {
                ENUM_FIBERS.with(|f| f.borrow_mut().remove(&id))
            }
            _ => None,
        };
        st.owner = None;
        st.lookahead = None;
        st.feed = None;
        st.done = None;
        st.saved_ec = crate::ec::Ec::default();
        taken
    };
    if let Some(coro) = coro {
        // A suspended iteration being discarded mid-program (a re-init row)
        // releases its live values through the Terminate protocol, never a
        // stack unwind.
        crate::fiber::terminate_coro(coro);
    }
}

/// The FrozenError every re-init row raises on a frozen receiver.
fn check_reinit_frozen(e: &REnumerator) -> Result<(), Signal> {
    if !e.is_frozen() {
        return Ok(());
    }
    let name = crate::dispatch::class_name(enumerator_class_id(e))
        .unwrap_or_else(|| "Enumerator".to_string());
    Err(frozen_error!(
        "can't modify frozen {name}: {}",
        enum_inspect(e)
    ))
}

/// Stores a replacement core and discards iteration state -- the shared tail
/// of every `initialize`/`initialize_copy` row.
fn replace_core(e: &EnumeratorData, core: EnumCore) {
    *e.reinit.lock() = Some(core);
    clear_iteration(e);
}

/// `initialize_copy`'s shared body: `other` must be an enumerator of the
/// receiver's own class (CRuby's check). An ArithmeticSequence pair is
/// refused: its quadruple is read lock-free (`arith_seq_parts`), so it cannot
/// be swapped in place.
fn enum_init_copy(recv: &RubyValue, other: &RubyValue) -> Result<RubyValue, Signal> {
    let e = recv_enum(recv);
    check_reinit_frozen(e)?;
    let RubyValue::Enumerator(o) = other else {
        return Err(type_error!("initialize_copy should take same class object"));
    };
    if enumerator_class_id(e) != enumerator_class_id(o) {
        return Err(type_error!("initialize_copy should take same class object"));
    }
    if matches!(e.seed.source, EnumSource::ArithSeq { .. }) {
        return Err(crate::builtins::not_impl_error!(
            "can't re-initialize an Enumerator::ArithmeticSequence (zeo reads its quadruple lock-free)"
        ));
    }
    let core = o.core();
    replace_core(e, core);
    Ok(recv.clone())
}

fn list(vals: &[RubyValue]) -> String {
    let rendered: Vec<String> = vals.iter().map(|v| v.inspect_string()).collect();
    format!("[{}]", rendered.join(", "))
}

/// `Object#to_s`'s address form under the enumerator's own class name --
/// what `Enumerator#to_s`, `puts`, and interpolation print (inspect alone
/// describes the iteration). One helper for the method row and value.rs's
/// display arm, so the two cannot drift.
pub(crate) fn enum_to_s(e: &REnumerator) -> String {
    let name = crate::dispatch::class_name(enumerator_class_id(e))
        .unwrap_or_else(|| "Enumerator".to_string());
    format!("#<{name}:0x{:016x}>", std::sync::Arc::as_ptr(e) as usize)
}

pub(crate) fn enum_inspect(e: &EnumeratorData) -> String {
    match &e.source() {
        // A generator/producer is an object in its own right, and CRuby
        // prints it with its address (the conformance test normalizes that to
        // `0xADDR`). The enumerator WRAPPING one renders through the `Method`
        // arm below, which is where `#<Enumerator: #<...Generator:0x..>:each>`
        // comes from.
        EnumSource::Generator { .. } => format!(
            "#<Enumerator::Generator:0x{:016x}>",
            e as *const EnumeratorData as usize
        ),
        EnumSource::Produce { .. } => format!(
            "#<Enumerator::Producer:0x{:016x}>",
            e as *const EnumeratorData as usize
        ),
        // A chain/product prints its sources verbatim (CRuby renders the
        // held array, so `[1,2].chain([3])` shows the arrays themselves
        // while `a.each + b.each` shows the two enumerators).
        EnumSource::Chain { sources } => format!("#<Enumerator::Chain: {}>", list(sources)),
        EnumSource::Product { sources } => format!("#<Enumerator::Product: {}>", list(sources)),
        // CRuby replays the CALL rather than the quadruple, so
        // `(1..10).step(2)` and `1.step(10, 2)` -- the same sequence -- print
        // differently. The receiver goes through `to_s` (a Rational shows as
        // `1/2`, not `(1/2)`) and a Range receiver gains explicit parentheses.
        EnumSource::ArithSeq {
            recv, meth, args, ..
        } => {
            let rendered = recv.to_display_string();
            let head = match recv {
                RubyValue::Range(..) => format!("({rendered})"),
                _ => rendered,
            };
            let mut s = format!("({head}.{meth}");
            if !args.is_empty() {
                let last = args.len() - 1;
                let rendered: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| match a {
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
            s.push(')');
            s
        }
        EnumSource::Method {
            recv, meth, args, ..
        } => {
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
    let core = e.core();
    // `Enumerator.new(size) { }`'s stored hint wins over anything derived
    // from the source. A size CALLABLE is invoked lazily here, which is when
    // CRuby calls it too.
    if let Some(hint) = &core.size_hint {
        return match hint {
            // The captured arguments are forwarded to a size callable, so
            // `to_enum(:pairs, n) { n }` can read them back -- CRuby passes
            // `e->args` through the same `call` probe.
            RubyValue::Proc(p) => {
                let args = match &core.source {
                    EnumSource::Method { args, .. } => args.as_slice(),
                    _ => &[],
                };
                p.call(args).unwrap_or(RubyValue::Nil)
            }
            v => v.clone(),
        };
    }
    match &core.source {
        EnumSource::Generator { .. } => RubyValue::Nil,
        // A produced sequence is endless -> Float::INFINITY (CRuby's rule).
        EnumSource::Produce { .. } => RubyValue::Float(f64::INFINITY),
        EnumSource::Method {
            recv,
            meth,
            args,
            sized,
        } => match meth.as_str() {
            // A `to_enum`/`enum_for` enumerator supplies no size function, so
            // it has no size at all unless a block was given for one.
            _ if !*sized => RubyValue::Nil,
            "each" | "each_entry" | "map" | "collect" | "select" | "filter" | "find_all"
            | "reject" | "sort_by" | "min_by" | "max_by" | "group_by" | "partition"
            | "flat_map" | "collect_concat" | "each_with_index" | "each_with_object"
            | "with_index" | "with_object" | "each_char" | "each_key" | "each_value"
            | "each_pair" | "each_index" | "map!" | "select!" | "reject!" | "transform_keys"
            | "transform_values" => receiver_size(recv),
            "times" => recv.clone(),
            // `then`/`yield_self` yields the receiver exactly once.
            "then" | "yield_self" => RubyValue::Int(1),
            // `cycle` repeats the receiver forever, or `n` times: `n * size`,
            // and 0 for an empty receiver or a non-positive count.
            "cycle" => {
                let RubyValue::Int(size) = receiver_size(recv) else {
                    return RubyValue::Nil;
                };
                match args.first() {
                    None | Some(RubyValue::Nil) if size == 0 => RubyValue::Int(0),
                    None | Some(RubyValue::Nil) => RubyValue::Float(f64::INFINITY),
                    Some(RubyValue::Int(n)) => RubyValue::Int((n * size).max(0)),
                    Some(_) => RubyValue::Nil,
                }
            }
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
        // Computed from the quadruple, never walked -- which is the only way
        // an endless sequence can answer at all.
        EnumSource::ArithSeq { .. } => arith_size(&core.source).unwrap_or(RubyValue::Nil),
    }
}

/// `arith_size`'s fallible form -- a beginless sequence raises the coercion
/// TypeError rather than answering a count.
fn arith_size(source: &EnumSource) -> Result<RubyValue, Signal> {
    let EnumSource::ArithSeq {
        begin,
        end,
        step,
        exclude_end,
        ..
    } = source
    else {
        unreachable!("arith_size on a non-ArithSeq source");
    };
    let end = match end {
        RubyValue::Nil => None,
        other => Some(other),
    };
    crate::builtins::numeric::step_size(begin, end, step, *exclude_end)
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
    // An `AtomicI64`, not a `Mutex<i64>`: the counter is incremented once per
    // ELEMENT, and a mutex acquire/release pair per element is real work for a
    // fetch-add. Relaxed is enough -- nothing else is published through it, and
    // an interleaved `each_with_index` across threads has no defined order for
    // this index anyway.
    let counter = Arc::new(std::sync::atomic::AtomicI64::new(offset));
    let wrapper: RProc = RProc::new(move |raw: &[RubyValue]| {
        let el = pack(raw);
        let i = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        blk.call(&[el, RubyValue::Int(i)])
    });
    internal_each(&e.source(), RubyValue::Proc(wrapper))
}

/// `with_object(memo)`/`each_with_object`'s driver: yields
/// `(packed_element, memo)` and returns the memo.
fn drive_with_object(
    recv: &RubyValue,
    obj: &RubyValue,
    block: Option<RubyValue>,
    label: &str,
) -> Result<RubyValue, Signal> {
    let Some(block) = block else {
        return Ok(enumerator_for(recv, label, std::slice::from_ref(obj)));
    };
    let blk = block.as_proc_unchecked();
    let memo = obj.clone();
    let memo_for_block = memo.clone();
    let wrapper: RProc =
        RProc::new(move |raw: &[RubyValue]| blk.call(&[pack(raw), memo_for_block.clone()]));
    // A LAZY receiver drives its own chain (`Enumerator::Lazy < Enumerator`
    // puts it through this row); an ordinary enumerator re-invokes its
    // captured source.
    if crate::builtins::lazy::is_lazy(recv) {
        crate::builtins::lazy::lazy_each(recv, Some(RubyValue::Proc(wrapper)))?;
    } else {
        internal_each(&recv_enum(recv).source(), RubyValue::Proc(wrapper))?;
    }
    Ok(memo)
}

ruby_class! {
    Enumerator = zeo_abi::ENUMERATOR_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // `Enumerator.new([size]) { |y| ... }`. `Class#new` intercepts this for
    // `Enumerator` itself (`builtins::rclass`), so the row exists for the
    // SUBCLASS path: `value_subclass::construct_root_payload` builds a payload
    // by calling the root's own `new` out of this table, which is what lets
    // `class NdjsonToMessageEnumerator < Enumerator` seat one through `super()`.
    def self."new" allocs (_recv, size?, &block) {
        let args: Vec<RubyValue> = size.into_iter().cloned().collect();
        enumerator_new(&args, block)
    }

    // `Enumerator.produce([initial]) { |prev| ... }` -- an endless generator
    // (#2483). With `initial`, that value is yielded first; then each block
    // result is yielded, forever (bounded by the consumer, e.g. `take`/`first`).
    def self."produce"(_recv, arg?, &block) {
        let Some(RubyValue::Proc(generator)) = block else {
            return Err(arg_error!("tried to create Producer without a block"));
        };
        let producer = RubyValue::Enumerator(Arc::new(EnumeratorData::new(
            EnumSource::Produce { initial: arg.cloned(), block: generator },
            None,
        )));
        Ok(enumerator_over(producer, None))
    }

    // `Enumerator.product(*enums)` -- every combination as an Array, rightmost
    // source varying fastest (#2484). No args yields one empty combination.
    def self."product"(_recv, *args, &block) {
        let product = RubyValue::Enumerator(Arc::new(EnumeratorData::new(
            EnumSource::Product { sources: args.to_vec() },
            None,
        )));
        // With a block, ruby RUNS it over each tuple and answers nil; the
        // enumerator is only what a blockless call gets. The block used to be
        // accepted and dropped.
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(product);
        };
        crate::dispatch::send_value(&product, crate::Symbol::intern("each"), &[], Some(RubyValue::Proc(p)))?;
        Ok(RubyValue::Nil)
    }

    def "each"(recv, *args, &block) {
        // `Enumerator::Lazy < Enumerator`: a lazy receiver reaches this row
        // (its own class adds no `each`, matching CRuby's `.owner`) and
        // drives its chain instead of downcasting to `EnumeratorData`.
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_each(recv, block);
        }
        let e = recv_enum(recv);
        // `each(*extra)` appends the extras to the captured args on a DUP --
        // `enumerator.c`'s `enumerator_each`. Only a method-backed source
        // has captured args to extend; every other source raises the same
        // way CRuby's underlying call would on surplus arguments.
        if !args.is_empty() {
            let src = match e.source() {
                EnumSource::Method {
                    recv,
                    meth,
                    args: captured,
                    sized,
                } => {
                    let mut all = captured;
                    all.extend(args.iter().cloned());
                    EnumSource::Method {
                        recv,
                        meth,
                        args: all,
                        sized,
                    }
                }
                _ => {
                    return Err(crate::builtins::arity_err(args.len(), 0, Some(0)));
                }
            };
            return match block {
                Some(b) => internal_each(&src, b),
                None => Ok(recv.clone()),
            };
        }
        match block {
            // Re-invoke the captured method with the caller's block; the
            // return value is the underlying method's own.
            Some(b) => internal_each(&e.source(), b),
            // Blockless `each` returns SELF (`equal?`-identical, oracle).
            None => Ok(recv.clone()),
        }
    }

    // `e + other` -- an `Enumerator::Chain` over the two, in order. Chaining
    // a chain nests rather than flattens, matching CRuby.
    def "+"(recv, other) {
        Ok(chain_of(vec![recv.clone(), (*other).clone()]))
    }

    // Re-init: the receiver becomes the fresh block-driven enumerator
    // `Enumerator.new([size]) { |y| ... }` would build, discarding previous
    // state including any external-iteration position.
    private def "initialize" cfunc (recv, *args, &block) {
        let e = recv_enum(recv);
        check_reinit_frozen(e)?;
        if matches!(e.seed.source, EnumSource::ArithSeq { .. }) {
            return Err(crate::builtins::not_impl_error!(
                "can't re-initialize an Enumerator::ArithmeticSequence (zeo reads its quadruple lock-free)"
            ));
        }
        if !matches!(&block, Some(RubyValue::Proc(_))) {
            return Err(arg_error!("tried to create Proc object without a block"));
        }
        let RubyValue::Enumerator(fresh) = enumerator_new(args, block)? else {
            unreachable!("enumerator_new builds an Enumerator");
        };
        replace_core(e, fresh.core());
        Ok(recv.clone())
    }

    private def "initialize_copy"(recv, other) {
        enum_init_copy(recv, other)
    }

    def "next"(recv) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_iterate(recv, "next");
        }
        Ok(ary2sv(take_next(recv_enum(recv))?))
    }

    def "next_values"(recv) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_iterate(recv, "next_values");
        }
        Ok(RubyValue::Array(array_new(take_next(recv_enum(recv))?)))
    }

    def "peek"(recv) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_iterate(recv, "peek");
        }
        Ok(ary2sv(fill_peek(recv_enum(recv))?))
    }

    def "peek_values"(recv) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_iterate(recv, "peek_values");
        }
        Ok(RubyValue::Array(array_new(fill_peek(recv_enum(recv))?)))
    }

    // `#feed(value)` -- set the value the generator's paused `y.yield` returns
    // on the next `#next`. Setting it twice before a `#next` consumes it is a
    // TypeError; the call itself answers nil.
    def "feed"(recv, arg) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_feed(recv, arg);
        }
        let mut st = recv_enum(recv).state.lock();
        if st.feed.is_some() {
            return Err(type_error!("feed value already set"));
        }
        st.feed = Some((*arg).clone());
        Ok(RubyValue::Nil)
    }

    def "rewind"(recv) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_rewind(recv);
        }
        let e = recv_enum(recv);
        let mut st = e.state.lock();
        if let Some(id) = st.fiber.take() {
            // Disposed through the Terminate protocol, never by DROPPING the
            // coroutine: corosensei's drop force-UNWINDS the suspended stack,
            // and a compiled frame cannot support native unwinding (the abort
            // reads "failed to initiate panic"). A fiber pinned to ANOTHER
            // thread can't be removed from here; it is disposed when that
            // thread's table drops (documented leak-until-thread-exit).
            if st.owner == Some(std::thread::current().id())
                && let Some(coro) = ENUM_FIBERS.with(|f| f.borrow_mut().remove(&id))
            {
                drop(st);
                crate::fiber::terminate_coro(coro);
                st = e.state.lock();
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
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_size(recv);
        }
        Ok(enum_size(recv_enum(recv)))
    }

    def "inspect"(recv) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_inspect(recv);
        }
        Ok(RubyValue::Str(crate::string_new(enum_inspect(recv_enum(recv)))))
    }

    // `Enumerator#to_s` is inherited `Object#to_s` in CRuby: the ADDRESS
    // form, not #inspect's iteration description. `puts e`/interpolation go
    // through the same helper (value.rs's display arm), so the two halves
    // cannot drift.
    def "to_s"(recv) {
        if crate::builtins::lazy::is_lazy(recv) {
            return crate::builtins::lazy::lazy_to_s(recv);
        }
        Ok(RubyValue::Str(crate::string_new(enum_to_s(recv_enum(recv)))))
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

    // `Enumerator::Generator` -- what `Enumerator.new { |y| ... }` holds.
    // Constructible in its own right, and `Enumerable` is all it inherits:
    // it answers `#each` and what `Enumerable` derives from that, which is
    // why `Enumerator::Generator.new { |y| y << 1 }.to_a` works but `#next`
    // does not.
    class Generator = zeo_abi::ENUMERATOR_GENERATOR_CLASS < zeo_abi::OBJECT_CLASS {
        include zeo_abi::ENUMERABLE_CLASS;

        // Not the bare-`yield` message: CRuby raises this one from the
        // constructor itself, without the `(yield)` suffix.
        def self."new"(_recv, *_args, &block) {
            let Some(RubyValue::Proc(p)) = block else {
                return Err(crate::builtins::local_jump_error!("no block given"));
            };
            Ok(generator_object(p))
        }
        // Answers the generator block's OWN return value, not self -- CRuby
        // hands back what `rb_proc_call` gave it.
        def "each" cfunc (recv, *_args, &block) {
            let consumer = need_block!(block);
            internal_each(&recv_enum(recv).source(), RubyValue::Proc(consumer))
        }
        // Re-init replaces the generator's proc; blockless keeps `self.new`'s
        // own refusal shape.
        private def "initialize" cfunc (recv, *_args, &block) {
            let e = recv_enum(recv);
            check_reinit_frozen(e)?;
            let Some(RubyValue::Proc(p)) = block else {
                return Err(crate::builtins::local_jump_error!("no block given"));
            };
            replace_core(e, EnumCore { source: EnumSource::Generator { block: p }, size_hint: None });
            Ok(recv.clone())
        }
        private def "initialize_copy"(recv, other) {
            enum_init_copy(recv, other)
        }
    }

    // `Enumerator::Producer` -- what `Enumerator.produce` holds. Endless by
    // construction, and NOT an Enumerable: only the enumerator wrapping it
    // can be bounded (`first(n)`, `take(n)`).
    class Producer = zeo_abi::ENUMERATOR_PRODUCER_CLASS < zeo_abi::OBJECT_CLASS {
        def "each"(recv, &block) {
            let consumer = need_block!(block);
            internal_each(&recv_enum(recv).source(), RubyValue::Proc(consumer))
        }
    }

    // `Enumerator::ArithmeticSequence` -- a blockless `Range#step`,
    // `Range#%` or `Numeric#step` over a numeric receiver. Everything else
    // it answers comes down the chain from `Enumerator` and `Enumerable`;
    // these thirteen are the ones CRuby defines here, and they exist because
    // the quadruple lets `#size` and `#last` compute instead of walk.
    // `Enumerator::Chain` and `::Product` add no behaviour of their own --
    // everything they answer comes down the chain from `Enumerator`, and the
    // `EnumSource` variant is what makes `#each` walk the right shape. ruby
    // OWNS these four on each subclass all the same, so the rows exist to
    // make `.owner` and `instance_methods(false)` agree; each calls the very
    // `Enumerator` row it would otherwise have inherited.
    class Chain = zeo_abi::ENUMERATOR_CHAIN_CLASS < zeo_abi::ENUMERATOR_CLASS {
        def "each" cfunc (recv, *_args, &block) {
            inherited_row!(enumerator, "each", recv, __args, block)
        }
        def "inspect"(recv) { inherited_row!(enumerator, "inspect", recv, __args, None) }
        def "rewind"(recv) { inherited_row!(enumerator, "rewind", recv, __args, None) }
        def "size"(recv) { inherited_row!(enumerator, "size", recv, __args, None) }
        // `Enumerator::Chain.new(a, b)`. Enumerator's own `self.new` takes a
        // size and a block, so inheriting it made the two-enumerable form an
        // arity error -- this class's constructor is its own.
        def self."new" allocs cfunc (_recv, *args, &_block) {
            Ok(chain_of(args.to_vec()))
        }
        // Re-init: the receiver becomes a chain over the given enumerables.
        private def "initialize" cfunc (recv, *args) {
            let e = recv_enum(recv);
            check_reinit_frozen(e)?;
            replace_core(e, EnumCore {
                source: EnumSource::Chain { sources: args.to_vec() },
                size_hint: None,
            });
            Ok(recv.clone())
        }
        private def "initialize_copy"(recv, other) {
            enum_init_copy(recv, other)
        }
    }

    class Product = zeo_abi::ENUMERATOR_PRODUCT_CLASS < zeo_abi::ENUMERATOR_CLASS {
        def "each" arity 0 (recv, *_args, &block) {
            inherited_row!(enumerator, "each", recv, __args, block)
        }
        def "inspect"(recv) { inherited_row!(enumerator, "inspect", recv, __args, None) }
        def "rewind"(recv) { inherited_row!(enumerator, "rewind", recv, __args, None) }
        def "size"(recv) { inherited_row!(enumerator, "size", recv, __args, None) }
        // `Enumerator::Product.new(a, b)` -- its own constructor, for the same
        // reason `Chain` needs one.
        def self."new" allocs cfunc (_recv, *args, &_block) {
            Ok(RubyValue::Enumerator(Arc::new(EnumeratorData::new(
                EnumSource::Product { sources: args.to_vec() },
                None,
            ))))
        }
        // Re-init: the receiver becomes the product of the given axes.
        private def "initialize" cfunc (recv, *args) {
            let e = recv_enum(recv);
            check_reinit_frozen(e)?;
            replace_core(e, EnumCore {
                source: EnumSource::Product { sources: args.to_vec() },
                size_hint: None,
            });
            Ok(recv.clone())
        }
        private def "initialize_copy"(recv, other) {
            enum_init_copy(recv, other)
        }
    }

    class ArithmeticSequence = zeo_abi::ENUMERATOR_ARITHMETIC_SEQUENCE_CLASS
        < zeo_abi::ENUMERATOR_CLASS
    {
        def "begin"(recv) {
            Ok(arith_parts(recv).0.clone())
        }

        def "end"(recv) {
            Ok(arith_parts(recv).1.cloned().unwrap_or(RubyValue::Nil))
        }

        def "step"(recv) {
            Ok(arith_parts(recv).2.clone())
        }

        def "exclude_end?"(recv) {
            Ok(RubyValue::Bool(arith_parts(recv).3))
        }

        // Answers SELF, not the walk's return value -- and a blockless call
        // answers self too, rather than wrapping in another enumerator.
        def "each"(recv, &block) {
            if let Some(RubyValue::Proc(p)) = &block {
                let e = recv_enum(recv);
                arith_walk(&e.seed.source, |v| {
                    p.call(std::slice::from_ref(v))?;
                    Ok(())
                })?;
            }
            Ok(recv.clone())
        }

        def "size"(recv) {
            arith_size(&recv_enum(recv).seed.source)
        }

        def "inspect"(recv) {
            Ok(RubyValue::Str(crate::string_new(enum_inspect(recv_enum(recv)))))
        }

        // One comparison behind all three: CRuby compares the quadruple with
        // `==`, so `(1..10).step(2)` equals `(1..10).step(2.0)` and equals
        // `1.step(10, 2)` -- the call that built it does not enter into it.
        // `#hash` deliberately does NOT match that, exactly as in CRuby:
        // `2.hash` and `2.0.hash` differ.
        def "==" | "===" | "eql?"(recv, other) {
            let (Some(a), Some(b)) = (arith_seq_parts(recv), arith_seq_parts(other)) else {
                return Ok(RubyValue::Bool(false));
            };
            let eq = |x: &RubyValue, y: &RubyValue| {
                crate::builtins::basic_object::value_identity(x, y) || x.rb_eq(y)
            };
            let ends = match (a.1, b.1) {
                (Some(x), Some(y)) => eq(x, y),
                (None, None) => true,
                _ => false,
            };
            Ok(RubyValue::Bool(eq(a.0, b.0) && ends && eq(a.2, b.2) && a.3 == b.3))
        }

        def "hash"(recv) {
            let (begin, end, step, exclude_end) = arith_parts(recv);
            let members = array_new(vec![
                begin.clone(),
                end.cloned().unwrap_or(RubyValue::Nil),
                step.clone(),
                RubyValue::Bool(exclude_end),
            ]);
            send_value(&RubyValue::Array(members), Symbol::intern("hash"), &[], None)
        }

        // The 0-arg form answers `begin` without walking (so an endless
        // sequence answers at all); the n-arg form walks, which is why
        // `(1..10.5).step(2).first(3)` is Floats while `#begin` is `1`.
        def "first"(recv, n?) {
            let (begin, end, step, _) = arith_parts(recv);
            let Some(n) = n else {
                if let Some(end) = end {
                    let dir = crate::builtins::numeric::num_cmp(step, &RubyValue::Int(0));
                    let past = match crate::builtins::numeric::num_cmp(begin, end) {
                        Some(Some(o)) => match dir {
                            Some(Some(d)) if d > 0 => o > 0,
                            Some(Some(d)) if d < 0 => o < 0,
                            _ => false,
                        },
                        _ => false,
                    };
                    if past {
                        return Ok(RubyValue::Nil);
                    }
                }
                return Ok(begin.clone());
            };
            let want = arg_int!(n);
            if want < 0 {
                return Err(arg_error!("attempt to take negative size"));
            }
            let mut out: Vec<RubyValue> = Vec::new();
            if want > 0 {
                // `Signal::Break` is the walk's only exit for an endless
                // sequence -- the same stop `Enumerable#first` uses.
                let taken = arith_walk(&recv_enum(recv).seed.source, |v| {
                    out.push(v.clone());
                    if out.len() as i64 >= want {
                        return Err(Signal::Break(RubyValue::Nil));
                    }
                    Ok(())
                });
                match taken {
                    Ok(()) | Err(Signal::Break(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(RubyValue::Array(array_new(out)))
        }

        // Computed from the quadruple, so it neither walks nor inherits the
        // walk's Float lane: `(1..10.5).step(2).last` is the Integer `9`.
        def "last"(recv, n?) {
            let (begin, end, step, exclude_end) = arith_parts(recv);
            let Some(end) = end else {
                return Err(crate::builtins::range_error!("cannot get the last element of endless arithmetic sequence"));
            };
            use crate::builtins::numeric::{num_cmp, step_hops, step_nth};
            let hops = step_hops(begin, end, step)?;
            if matches!(num_cmp(&hops, &RubyValue::Int(0)), Some(Some(-1))) {
                return Ok(match n {
                    Some(_) => RubyValue::Array(array_new(Vec::new())),
                    None => RubyValue::Nil,
                });
            }
            let nth = |i: i64| step_nth(begin, step, &RubyValue::Int(i));
            // An exclusive end that the last hop lands exactly on drops that
            // element -- and with it one from the count `last(n)` counts back.
            let last = step_nth(begin, step, &hops)?;
            let dropped = exclude_end && matches!(num_cmp(&last, end), Some(Some(0)));
            let Some(n) = n else {
                return if dropped {
                    let RubyValue::Int(h) = hops else {
                        return Ok(last);
                    };
                    nth(h - 1)
                } else {
                    Ok(last)
                };
            };
            let want = arg_int!(n);
            if want < 0 {
                return Err(arg_error!("negative array size"));
            }
            let len = match &hops {
                RubyValue::Int(h) if !dropped => h + 1,
                RubyValue::Int(h) => *h,
                // A bignum hop count cannot be materialized as an Array
                // anyway; the take is bounded by `want`.
                _ => want,
            };
            let take = want.min(len);
            let mut out = Vec::with_capacity(take as usize);
            for i in (len - take)..len {
                out.push(nth(i)?);
            }
            Ok(RubyValue::Array(array_new(out)))
        }
    }
}

/// The quadruple behind every `Enumerator::ArithmeticSequence` row (those
/// rows only ever dispatch on an arithmetic sequence).
fn arith_parts(recv: &RubyValue) -> (&RubyValue, Option<&RubyValue>, &RubyValue, bool) {
    arith_seq_parts(recv)
        .expect("an ArithmeticSequence table row dispatched on a non-sequence receiver")
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
