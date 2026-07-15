//! `Enumerator` (Phase 17.2) -- fiber-backed external iteration, per
//! CRuby's enumerator.c (all mechanisms cited there were read against the
//! CRuby source and oracle-verified on ruby 4.0.5).
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
//! the EXACT same `spinel_fiber` instantiation `fiber.rs` uses -- same
//! Send+Sync split (handle in the value, coroutine thread-pinned in a TLS
//! table), same `$!`-stack swap, and, because the `(input, yield)`
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
use crate::builtins::{arity, builtin_methods, class_name_of};
use crate::collections::array_new;
use crate::dispatch::{raise_error, raise_stop_iteration, send_value};
use crate::signal::Signal;
use crate::value::RubyValue;
use crate::{RProc, Symbol};
use parking_lot::Mutex;
use spinel_fiber::CoroutineResult;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::ThreadId;

/// What `to_enum` captures -- CRuby's `struct enumerator`'s `obj`/`meth`/
/// `args` triple, or the `Enumerator.new` generator block. Cloned into
/// the external-iteration fiber, so it must stay cheap to clone
/// (everything inside is `Arc`-backed or small).
#[derive(Clone)]
enum EnumSource {
    Method { recv: RubyValue, meth: String, args: Vec<RubyValue> },
    Generator { block: RProc },
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
    /// The underlying `each`'s return value once iteration completed --
    /// every subsequent `next` re-raises `StopIteration` carrying it
    /// (CRuby rebuilds a fresh exception each time, from `stop_exc`).
    done: Option<RubyValue>,
    /// The iteration fiber's own `$!`/rescue-nesting stack while
    /// suspended -- same execution-context swap as `fiber.rs`.
    handling: Vec<RubyValue>,
}

pub struct EnumeratorData {
    source: EnumSource,
    /// `Enumerator.new(size) { ... }`'s stored hint; method-backed
    /// enumerators derive size lazily from their source instead.
    size_hint: Option<RubyValue>,
    state: Mutex<ExternState>,
}

pub type REnumerator = Arc<EnumeratorData>;

impl EnumeratorData {
    /// Whether external iteration has begun and not finished -- what
    /// makes `dup` unsafe (CRuby: "can't copy execution context").
    pub(crate) fn iteration_live(&self) -> bool {
        self.state.lock().fiber.is_some()
    }

    /// A fresh, never-iterated enumerator over the same source --
    /// `dup`/`clone`'s payload.
    pub(crate) fn fresh_copy(&self) -> REnumerator {
        Arc::new(EnumeratorData {
            source: self.source.clone(),
            size_hint: self.size_hint.clone(),
            state: Mutex::new(ExternState::default()),
        })
    }
}

/// Deliberately the same instantiation as `fiber.rs`'s `FiberCoro` (see
/// the module docs for why the matching TypeIds are a feature).
type EnumCoro = spinel_fiber::Coroutine<Vec<RubyValue>, RubyValue, Result<RubyValue, Signal>>;

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
    }))
}

/// `Enumerator.new([size]) { |y| ... }` -- reached through the dynamic
/// `Class#new` arm (`class_module::class_new` special-cases
/// `ENUMERATOR_CLASS`); parse deliberately skips the static `New` node
/// for literal `Enumerator` receivers so the block rides the ordinary
/// dynamic-call plumbing.
pub(crate) fn enumerator_new(
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    arity!(args, 0..=1);
    let Some(RubyValue::Proc(generator)) = block else {
        return Err(raise_error(
            "ArgumentError",
            "tried to create Enumerator without a block".to_string(),
        ));
    };
    let size_hint = match args.first() {
        None | Some(RubyValue::Nil) => None,
        Some(v @ (RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_))) => {
            Some(v.clone())
        }
        Some(other) => {
            return Err(raise_error(
                "TypeError",
                format!("can't convert {} into Integer", class_name_of(other)),
            ))
        }
    };
    Ok(RubyValue::Enumerator(Arc::new(EnumeratorData {
        source: EnumSource::Generator { block: generator },
        size_hint,
        state: Mutex::new(ExternState::default()),
    })))
}

fn recv_enum(recv: &RubyValue) -> &REnumerator {
    match recv {
        RubyValue::Enumerator(e) => e,
        _ => unreachable!("Enumerator table row dispatched on a non-Enumerator receiver"),
    }
}

fn recv_yielder(recv: &RubyValue) -> &RProc {
    match recv {
        RubyValue::Yielder(p) => p,
        _ => unreachable!("Yielder table row dispatched on a non-Yielder receiver"),
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
            generator(&[RubyValue::Yielder(each_block)])
        }
    }
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
    let coro: EnumCoro = spinel_fiber::new_fiber(move |_: Vec<RubyValue>| {
        let shuttle: RProc = RProc::new(|raw: &[RubyValue]| {
            spinel_fiber::yield_current::<Vec<RubyValue>, RubyValue>(RubyValue::Array(
                array_new(raw.to_vec()),
            ));
            Ok(RubyValue::Nil)
        });
        internal_each(&source, RubyValue::Proc(shuttle))
    });
    ENUM_FIBERS.with(|f| f.borrow_mut().insert(id, coro));
    st.fiber = Some(id);
    st.owner = Some(std::thread::current().id());
    id
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
    let saved = crate::handling::swap_handling(std::mem::take(&mut e.state.lock().handling));
    let outcome = spinel_fiber::resume(&mut coro, Vec::new());
    e.state.lock().handling = crate::handling::swap_handling(saved);
    match outcome {
        // The shuttle's arity-preserving Array payload -- the normal case.
        CoroutineResult::Yield(RubyValue::Array(a)) => {
            ENUM_FIBERS.with(|f| f.borrow_mut().insert(id, coro));
            let raw = a.lock().clone();
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

pub(crate) fn enum_inspect(e: &EnumeratorData) -> String {
    match &e.source {
        // CRuby prints the generator with its address; addresses are
        // omitted crate-wide (the 16.2 posture).
        EnumSource::Generator { .. } => {
            "#<Enumerator: #<Enumerator::Generator>:each>".to_string()
        }
        EnumSource::Method { recv, meth, args } => {
            let mut s = format!("#<Enumerator: {}:{meth}", recv.inspect_string());
            if !args.is_empty() {
                let rendered: Vec<String> = args.iter().map(|a| a.inspect_string()).collect();
                s.push('(');
                s.push_str(&rendered.join(", "));
                s.push(')');
            }
            s.push('>');
            s
        }
    }
}

/// Lazy size (never iterates, CRuby's rule): the generator's stored hint,
/// or a size derived from the captured method. The same-size set covers
/// the methods whose enumerator has exactly the receiver's element count;
/// nil for everything else (a few shapes CRuby computes -- e.g.
/// `each_slice` over an infinite range -- return nil here, documented).
fn enum_size(e: &EnumeratorData) -> RubyValue {
    match &e.source {
        EnumSource::Generator { .. } => e.size_hint.clone().unwrap_or(RubyValue::Nil),
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
    }
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

/// `n.upto(m)` / `n.downto(m)` element counts (i64 pairs only; anything
/// else answers nil).
fn int_span(recv: &RubyValue, to: Option<&RubyValue>, ascending: bool) -> RubyValue {
    let (RubyValue::Int(a), Some(RubyValue::Int(b))) = (recv, to) else {
        return RubyValue::Nil;
    };
    let span = if ascending { *b - *a + 1 } else { *a - *b + 1 };
    RubyValue::Int(span.max(0))
}

/// `with_index(offset)`/`each_with_index`'s driver: re-runs the internal
/// iteration with a counting wrapper -- multi-value yields pack into an
/// array as the block's first param, the index appends (CRuby's
/// `enumerator_with_index_i`).
fn drive_with_index(
    e: &REnumerator,
    block: RubyValue,
    offset: i64,
) -> Result<RubyValue, Signal> {
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
        blk(&[el, RubyValue::Int(i)])
    });
    internal_each(&e.source, RubyValue::Proc(wrapper))
}

/// `with_object(memo)`/`each_with_object`'s driver: yields
/// `(packed_element, memo)` and returns the memo.
fn drive_with_object(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    label: &str,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    let e = recv_enum(recv);
    let Some(block) = block else {
        return Ok(enumerator_for(recv, label, args));
    };
    let blk = block.as_proc_unchecked();
    let memo = args[0].clone();
    let memo_for_block = memo.clone();
    let wrapper: RProc =
        RProc::new(move |raw: &[RubyValue]| blk(&[pack(raw), memo_for_block.clone()]));
    internal_each(&e.source, RubyValue::Proc(wrapper))?;
    Ok(memo)
}

builtin_methods! {
    pub(crate) fn lookup;

    "each" => fn each(recv, args, block) {
        let e = recv_enum(recv);
        if !args.is_empty() {
            panic!("Enumerator#each with extra arguments isn't supported yet (spike scope; CRuby appends them to the captured args on a dup)");
        }
        match block {
            // Re-invoke the captured method with the caller's block; the
            // return value is the underlying method's own.
            Some(b) => internal_each(&e.source, b),
            // Blockless `each` returns SELF (`equal?`-identical, oracle).
            None => Ok(recv.clone()),
        }
    }

    "next" => fn next(recv, args, _block) {
        arity!(args, 0);
        Ok(ary2sv(take_next(recv_enum(recv))?))
    }

    "next_values" => fn next_values(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(array_new(take_next(recv_enum(recv))?)))
    }

    "peek" => fn peek(recv, args, _block) {
        arity!(args, 0);
        Ok(ary2sv(fill_peek(recv_enum(recv))?))
    }

    "peek_values" => fn peek_values(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(array_new(fill_peek(recv_enum(recv))?)))
    }

    "rewind" => fn rewind(recv, args, _block) {
        arity!(args, 0);
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
        st.handling.clear();
        // CRuby also calls the receiver's own `rewind` hook when it
        // responds -- Tier B (rare protocol; documented).
        Ok(recv.clone())
    }

    "size" => fn size(recv, args, _block) {
        arity!(args, 0);
        Ok(enum_size(recv_enum(recv)))
    }

    // `Enumerator#to_s` is inherited `Object#to_s` in CRuby but prints the
    // same `#<Enumerator: ...>` shape via #inspect in practice; sharing
    // one implementation matches the observable output.
    "inspect" | "to_s" => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(enum_inspect(recv_enum(recv)))))
    }

    "with_index" => fn with_index(recv, args, block) {
        arity!(args, 0..=1);
        let e = recv_enum(recv);
        let offset = match args.first() {
            None => 0,
            Some(RubyValue::Int(n)) => *n,
            Some(other) => {
                return Err(raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into Integer",
                        class_name_of(other)
                    ),
                ))
            }
        };
        match block {
            Some(b) => drive_with_index(e, b, offset),
            // Blockless: a wrapping enumerator over SELF -- iterating it
            // re-dispatches this very row with a block.
            None => Ok(enumerator_for(recv, "with_index", args)),
        }
    }

    "each_with_index" => fn each_with_index(recv, args, block) {
        arity!(args, 0);
        let e = recv_enum(recv);
        match block {
            Some(b) => drive_with_index(e, b, 0),
            None => Ok(enumerator_for(recv, "each_with_index", &[])),
        }
    }

    "with_object" => fn with_object(recv, args, block) {
        drive_with_object(recv, args, block, "with_object")
    }

    "each_with_object" => fn each_with_object(recv, args, block) {
        drive_with_object(recv, args, block, "each_with_object")
    }
}

builtin_methods! {
    pub(crate) fn lookup_yielder;

    // `y << v` forwards to the consumer's block and returns the yielder
    // (chainable: `y << 1 << 2`).
    "<<" => fn yielder_push(recv, args, _block) {
        recv_yielder(recv)(args)?;
        Ok(recv.clone())
    }

    // `y.yield(*vs)` forwards and returns the block's own return value.
    "yield" => fn yielder_yield(recv, args, _block) {
        recv_yielder(recv)(args)
    }

    "to_proc" => fn yielder_to_proc(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Proc(recv_yielder(recv).clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let RubyValue::Enumerator(h) = &e else { panic!() };
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
        let RubyValue::Enumerator(h) = &e else { panic!() };
        take_next(h).unwrap();
        let _ = take_next(h);
    }

    #[test]
    fn generator_yielder_preserves_arity_for_next_values() {
        // Enumerator.new { |y| y.yield; y.yield nil; y.yield 1, 2 }
        let gen: RProc = RProc::new(|args: &[RubyValue]| {
            let y = &args[0];
            let RubyValue::Yielder(f) = y else { panic!("expected a Yielder") };
            f(&[])?;
            f(&[RubyValue::Nil])?;
            f(&[RubyValue::Int(1), RubyValue::Int(2)])?;
            Ok(RubyValue::Nil)
        });
        let e = enumerator_new(&[], Some(RubyValue::Proc(gen))).unwrap();
        let RubyValue::Enumerator(h) = &e else { panic!() };
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
        let gen: RProc = RProc::new(|args: &[RubyValue]| {
            let RubyValue::Yielder(f) = &args[0] else { panic!() };
            f(&[RubyValue::Int(1)])?;
            Err(Signal::Raise(RubyValue::Int(99)))
        });
        let e = enumerator_new(&[], Some(RubyValue::Proc(gen))).unwrap();
        let RubyValue::Enumerator(h) = &e else { panic!() };
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(1)));
        assert!(matches!(take_next(h), Err(Signal::Raise(RubyValue::Int(99)))));
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(1)));
    }

    #[test]
    fn rewind_restarts_from_the_top() {
        let e = enumerator_for(&ints(&[5, 6]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else { panic!() };
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
        let gen: RProc = RProc::new(|_| Ok(RubyValue::Nil));
        let bare = enumerator_new(&[], Some(RubyValue::Proc(gen.clone()))).unwrap();
        assert!(matches!(size(&bare, &[], None).unwrap(), RubyValue::Nil));
        let hinted =
            enumerator_new(&[RubyValue::Int(4)], Some(RubyValue::Proc(gen))).unwrap();
        assert!(matches!(size(&hinted, &[], None).unwrap(), RubyValue::Int(4)));
    }

    #[test]
    fn inspect_prints_the_cruby_shape() {
        let e = enumerator_for(&ints(&[1, 2]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else { panic!() };
        assert_eq!(enum_inspect(h), "#<Enumerator: [1, 2]:each>");
        let sliced = enumerator_for(&ints(&[1, 2]), "each_slice", &[RubyValue::Int(2)]);
        let RubyValue::Enumerator(h) = &sliced else { panic!() };
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
    fn yielder_push_chains_and_yield_returns_the_block_value() {
        let blk: RProc = RProc::new(|_raw| Ok(RubyValue::Int(42)));
        let y = RubyValue::Yielder(blk);
        let back = yielder_push(&y, &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(back, RubyValue::Yielder(_)));
        assert!(matches!(
            yielder_yield(&y, &[RubyValue::Int(1)], None).unwrap(),
            RubyValue::Int(42)
        ));
    }

    #[test]
    fn dup_copies_the_source_but_not_the_iteration() {
        let e = enumerator_for(&ints(&[1, 2]), "each", &[]);
        let RubyValue::Enumerator(h) = &e else { panic!() };
        take_next(h).unwrap();
        assert!(h.iteration_live());
        let copy = h.fresh_copy();
        assert!(!copy.iteration_live());
        assert!(matches!(ary2sv(take_next(&copy).unwrap()), RubyValue::Int(1)));
        // The original is unaffected: still at element 2.
        assert!(matches!(ary2sv(take_next(h).unwrap()), RubyValue::Int(2)));
    }
}
