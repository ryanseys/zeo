//! `Enumerable`, implemented in Rust (Phase 14.4 rev.2) -- the direct
//! analog of CRuby's `enum.c`, which implements every Enumerable method as
//! C code driving the receiver's own `each` via
//! `rb_block_call(obj, id_each, ..., callback, memo)`. Here: each method
//! calls `send_value(recv, :each, &[], Some(<rust-built Proc>))`, with
//! captured state in `Arc<Mutex<...>>` cells instead of a `MEMO`, and
//! early termination via `Err(Signal::Break(Nil))` from the internal proc
//! -- the exact analogue of `rb_iter_break()` (which also carries no
//! meaningful value in enum.c: every callback stores its answer into the
//! MEMO first and breaks with Qnil; the outer method reads the state).
//!
//! Reached ONLY through dynamic dispatch (`send`'s fallback for an Object
//! whose registered ancestors include `ENUMERABLE_CLASS`, and
//! `send_value`'s fallback for the builtin Array/Hash/Range receivers --
//! real Ruby's own `include Enumerable` set). `include Enumerable` on a
//! user class works purely via method dispatch on `:each`, exactly like
//! CRuby ("nothing is registered anywhere -- enum.c only ever does
//! rb_block_call(obj, id_each, ...)").
//!
//! **Element packing** (CRuby's `rb_enum_values_pack`, enum.c:52-62): when
//! `each` yields N raw values, the Enumerable-level ELEMENT is N==0 ->
//! nil, N==1 -> the value, N>1 -> an Array (so `Hash#each`'s two-value
//! yields make the element `[k, v]`). Methods that consume the element as
//! a value (to_a/include?/find/first/reduce/min/max/sum, and
//! select/reject's RESULT arrays) use the packed form; invocations of the
//! USER's block forward the RAW yielded values (CRuby's
//! `rb_yield_values2` rule for map/count -- and our approximation of
//! `enum_yield`'s force-blockarg splatting for select/find/any?-family:
//! passing raw values is equivalent for the idiomatic multi-param block
//! `{|k, v| ...}`; a SINGLE-param block over a Hash sees only `k` where
//! real Ruby would hand it the packed `[k, v]` -- the same documented
//! no-auto-splat approximation this runtime's block binding already makes
//! everywhere else).
//!
//! **Enumerator is deliberately absent** (each blockless form that returns
//! an Enumerator in real Ruby panics with a clear message): external
//! iteration is fiber-backed in CRuby (`enumerator.c`'s `next_i` runs the
//! full internal iteration inside `rb_fiber_new`, shuttling each yield out
//! via `rb_fiber_yield` and parking a `StopIteration` at the end) -- our
//! corosensei substrate (`spinel-fiber`, Phase 13.3) is exactly the right
//! foundation for that design when something needs it; nothing does yet.
//!
//! Documented divergences beyond the block-splat note above: `find`'s
//! optional `if_none` callable, `any?`/`all?`/`none?`/`one?`'s pattern-arg
//! (`===`) forms, and `min`/`max`'s `n`-largest forms are rejected loudly;
//! blockless `min`/`max` compare via a native Int/Float/String `<=>` only
//! (no user-defined `<=>` dispatch); `sum`'s float path IS
//! Kahan-Babuska-compensated like CRuby's, but the Rational leg of its
//! numeric tower doesn't exist here.

use crate::collections::array_new;
use crate::dispatch::send_value;
use crate::signal::Signal;
use crate::value::RubyValue;
use crate::{RProc, Symbol};
use parking_lot::Mutex;
use std::sync::Arc;

/// Dispatches one Enumerable method against `recv` (whose class must
/// provide `each`). `None` = not an Enumerable method this runtime
/// implements (the caller falls through to its `NoMethodError` path) --
/// this match is the SINGLE source of truth for what Enumerable supports;
/// codegen deliberately keeps no mirror list (see `codegen::call`'s
/// Enumerable fallback docs).
pub(crate) fn enumerable_send(
    recv: &RubyValue,
    name: &str,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Option<Result<RubyValue, Signal>> {
    Some(match name {
        "map" | "collect" => map(recv, args, block),
        "select" | "filter" | "find_all" => select(recv, args, block, true),
        "reject" => select(recv, args, block, false),
        "to_a" | "entries" => to_a(recv, args),
        "include?" | "member?" => include(recv, args),
        "count" => count(recv, args, block),
        "any?" => any_all(recv, args, block, Quantifier::Any),
        "all?" => any_all(recv, args, block, Quantifier::All),
        "none?" => any_all(recv, args, block, Quantifier::None),
        "one?" => any_all(recv, args, block, Quantifier::One),
        "find" | "detect" => find(recv, args, block),
        "first" => first(recv, args),
        "reduce" | "inject" => reduce(recv, args, block),
        "each_with_index" => each_with_index(recv, args, block),
        "sum" => sum(recv, args, block),
        "min" => min_max(recv, args, block, true),
        "max" => min_max(recv, args, block, false),
        _ => return None,
    })
}

/// CRuby's `rb_enum_values_pack` rule -- see the module docs.
fn pack(args: &[RubyValue]) -> RubyValue {
    match args.len() {
        0 => RubyValue::Nil,
        1 => args[0].clone(),
        _ => RubyValue::Array(array_new(args.to_vec())),
    }
}

/// Drives `recv`'s `each` with `f` as the block; `f` returning
/// `Err(Signal::Break(_))` terminates iteration early and is swallowed
/// here (the `rb_iter_break()` analogue -- results travel through captured
/// state, never the break payload). Any other signal (a raise, a `break`
/// from the USER's block targeting the enclosing call, `Signal::Return`)
/// propagates untouched.
fn for_each(
    recv: &RubyValue,
    f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
) -> Result<(), Signal> {
    let proc_: RProc = Arc::new(f);
    match send_value(
        recv,
        Symbol::intern("each"),
        &[],
        Some(RubyValue::Proc(proc_)),
    ) {
        Ok(_) => Ok(()),
        Err(Signal::Break(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

fn require_block(block: Option<RubyValue>, method: &str) -> RProc {
    match block {
        Some(RubyValue::Proc(p)) => p,
        _ => panic!(
            "Enumerable#{method} without a block would return an Enumerator, which isn't supported yet (spike scope) -- pass a block"
        ),
    }
}

fn reject_args(args: &[RubyValue], method: &str, what: &str) {
    if !args.is_empty() {
        panic!("Enumerable#{method} with {what} isn't supported yet (spike scope)");
    }
}

/// map/collect: the user block receives the RAW yielded values
/// (`rb_yield_values2`, enum.c:631-633); results collect into an Array.
fn map(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "map", "arguments");
    let blk = require_block(block, "map");
    let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
    let out2 = out.clone();
    for_each(recv, move |yielded| {
        let v = blk(yielded)?;
        out2.lock().push(v);
        Ok(RubyValue::Nil)
    })?;
    let items = std::mem::take(&mut *out.lock());
    Ok(RubyValue::Array(array_new(items)))
}

/// select/filter (`keep == true`) and reject (`keep == false`): the block
/// sees the raw values; the RESULT array collects the PACKED element
/// (enum.c:483/591 -- `Hash#select` through generic Enumerable returns an
/// Array of `[k, v]` pairs).
fn select(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    keep: bool,
) -> Result<RubyValue, Signal> {
    let method = if keep { "select" } else { "reject" };
    reject_args(args, method, "arguments");
    let blk = require_block(block, method);
    let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
    let out2 = out.clone();
    for_each(recv, move |yielded| {
        if blk(yielded)?.truthy() == keep {
            out2.lock().push(pack(yielded));
        }
        Ok(RubyValue::Nil)
    })?;
    let items = std::mem::take(&mut *out.lock());
    Ok(RubyValue::Array(array_new(items)))
}

fn to_a(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    reject_args(args, "to_a", "arguments (forwarding them to #each)");
    let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
    let out2 = out.clone();
    for_each(recv, move |yielded| {
        out2.lock().push(pack(yielded));
        Ok(RubyValue::Nil)
    })?;
    let items = std::mem::take(&mut *out.lock());
    Ok(RubyValue::Array(array_new(items)))
}

/// `==`-based membership (`rb_equal`, enum.c:2960) with break-on-hit.
fn include(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.len() != 1 {
        panic!("Enumerable#include? takes exactly one argument");
    }
    let needle = args[0].clone();
    let found = Arc::new(Mutex::new(false));
    let found2 = found.clone();
    for_each(recv, move |yielded| {
        if pack(yielded).rb_eq(&needle) {
            *found2.lock() = true;
            return Err(Signal::Break(RubyValue::Nil));
        }
        Ok(RubyValue::Nil)
    })?;
    let result = *found.lock();
    Ok(RubyValue::Bool(result))
}

/// count: 0-arg counts yields; block form counts truthy raw-yield results;
/// 1-arg counts `==` matches (arg + block: the arg wins, block ignored --
/// CRuby warns "given block not used", enum.c:320). Always iterates
/// (Enumerable#count has no size fast path -- enum.c:302-328).
fn count(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let n = Arc::new(Mutex::new(0i64));
    let n2 = n.clone();
    match (args.len(), block) {
        (0, None) => for_each(recv, move |_| {
            *n2.lock() += 1;
            Ok(RubyValue::Nil)
        })?,
        (0, Some(RubyValue::Proc(blk))) => for_each(recv, move |yielded| {
            if blk(yielded)?.truthy() {
                *n2.lock() += 1;
            }
            Ok(RubyValue::Nil)
        })?,
        (1, _) => {
            let item = args[0].clone();
            for_each(recv, move |yielded| {
                if pack(yielded).rb_eq(&item) {
                    *n2.lock() += 1;
                }
                Ok(RubyValue::Nil)
            })?
        }
        _ => panic!("Enumerable#count takes at most one argument"),
    }
    let result = *n.lock();
    Ok(RubyValue::Int(result))
}

enum Quantifier {
    Any,
    All,
    None,
    One,
}

/// any?/all?/none?/one? -- blockless tests the PACKED element's
/// truthiness; the block form tests the raw-yield result (enum.c's
/// DEFINE_ENUMFUNCS pair). Empty-collection seeds match CRuby exactly:
/// `[].all?`/`[].none?` are true, `[].any?`/`[].one?` are false. The
/// pattern-arg (`===`) forms are rejected (no case-equality dispatch
/// surface exists here yet).
fn any_all(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    q: Quantifier,
) -> Result<RubyValue, Signal> {
    reject_args(args, match q {
        Quantifier::Any => "any?",
        Quantifier::All => "all?",
        Quantifier::None => "none?",
        Quantifier::One => "one?",
    }, "a pattern argument (`===` form)");
    let blk = match block {
        Some(RubyValue::Proc(p)) => Some(p),
        _ => None,
    };
    let test = move |yielded: &[RubyValue]| -> Result<bool, Signal> {
        match &blk {
            Some(b) => Ok(b(yielded)?.truthy()),
            None => Ok(pack(yielded).truthy()),
        }
    };
    match q {
        Quantifier::Any => {
            let hit = Arc::new(Mutex::new(false));
            let hit2 = hit.clone();
            for_each(recv, move |yielded| {
                if test(yielded)? {
                    *hit2.lock() = true;
                    return Err(Signal::Break(RubyValue::Nil));
                }
                Ok(RubyValue::Nil)
            })?;
            let result = *hit.lock();
            Ok(RubyValue::Bool(result))
        }
        Quantifier::All => {
            let ok = Arc::new(Mutex::new(true));
            let ok2 = ok.clone();
            for_each(recv, move |yielded| {
                if !test(yielded)? {
                    *ok2.lock() = false;
                    return Err(Signal::Break(RubyValue::Nil));
                }
                Ok(RubyValue::Nil)
            })?;
            let result = *ok.lock();
            Ok(RubyValue::Bool(result))
        }
        Quantifier::None => {
            let ok = Arc::new(Mutex::new(true));
            let ok2 = ok.clone();
            for_each(recv, move |yielded| {
                if test(yielded)? {
                    *ok2.lock() = false;
                    return Err(Signal::Break(RubyValue::Nil));
                }
                Ok(RubyValue::Nil)
            })?;
            let result = *ok.lock();
            Ok(RubyValue::Bool(result))
        }
        Quantifier::One => {
            // Qundef -> first truthy -> true; second truthy -> false +
            // break; never-truthy -> false (enum.c:1933-1943).
            let state = Arc::new(Mutex::new(0u8)); // 0=none, 1=one, 2=many
            let state2 = state.clone();
            for_each(recv, move |yielded| {
                if test(yielded)? {
                    let mut s = state2.lock();
                    *s += 1;
                    if *s >= 2 {
                        return Err(Signal::Break(RubyValue::Nil));
                    }
                }
                Ok(RubyValue::Nil)
            })?;
            let result = *state.lock() == 1;
            Ok(RubyValue::Bool(result))
        }
    }
}

/// find/detect: first PACKED element whose raw-yield block result is
/// truthy; nil otherwise. The optional `if_none` callable argument is
/// rejected (spike scope).
fn find(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "find", "an `if_none` argument");
    let blk = require_block(block, "find");
    let hit: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
    let hit2 = hit.clone();
    for_each(recv, move |yielded| {
        if blk(yielded)?.truthy() {
            *hit2.lock() = Some(pack(yielded));
            return Err(Signal::Break(RubyValue::Nil));
        }
        Ok(RubyValue::Nil)
    })?;
    let result = hit.lock().take().unwrap_or(RubyValue::Nil);
    Ok(result)
}

/// first / first(n): break-on-first(-nth) yield; `first(0)` returns `[]`
/// WITHOUT calling `each` at all (enum.c:3585); `first` on empty -> nil.
fn first(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match args.len() {
        0 => {
            let hit: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
            let hit2 = hit.clone();
            for_each(recv, move |yielded| {
                *hit2.lock() = Some(pack(yielded));
                Err(Signal::Break(RubyValue::Nil))
            })?;
            let result = hit.lock().take().unwrap_or(RubyValue::Nil);
            Ok(result)
        }
        1 => {
            let RubyValue::Int(n) = &args[0] else {
                panic!("Enumerable#first: no implicit conversion into Integer");
            };
            let n = *n;
            if n < 0 {
                panic!("attempt to take negative size (ArgumentError; spike scope: raised as a panic)");
            }
            if n == 0 {
                return Ok(RubyValue::Array(array_new(Vec::new())));
            }
            let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
            let out2 = out.clone();
            for_each(recv, move |yielded| {
                let mut o = out2.lock();
                o.push(pack(yielded));
                if o.len() as i64 >= n {
                    return Err(Signal::Break(RubyValue::Nil));
                }
                Ok(RubyValue::Nil)
            })?;
            let items = std::mem::take(&mut *out.lock());
            Ok(RubyValue::Array(array_new(items)))
        }
        _ => panic!("Enumerable#first takes at most one argument"),
    }
}

/// reduce/inject, all three CRuby forms (enum.c:1046-1096): `{block}`,
/// `(init){block}`, and the operator forms `(:sym)` / `(init, :sym)`
/// (each step `acc = acc.send(op, elem)` -- dispatched through
/// `send_value`, so user-defined operator methods compose). No-init: the
/// first element seeds the accumulator WITHOUT invoking the block; empty
/// with no init -> nil. The block is always called with exactly
/// `(acc, packed_element)`.
fn reduce(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    enum Step {
        Block(RProc),
        Op(Symbol),
    }
    let (init, step) = match (args.len(), &block) {
        (0, Some(RubyValue::Proc(p))) => (None, Step::Block(p.clone())),
        // One arg + block: the arg is the INIT (CRuby's 2-args+block
        // "block not used" warning case only fires with op present).
        (1, Some(RubyValue::Proc(p))) => (Some(args[0].clone()), Step::Block(p.clone())),
        (1, None) => match &args[0] {
            RubyValue::Symbol(op) => (None, Step::Op(*op)),
            _ => panic!("Enumerable#reduce: a single non-Symbol argument needs a block (the argument is the initial value)"),
        },
        (2, _) => match &args[1] {
            RubyValue::Symbol(op) => (Some(args[0].clone()), Step::Op(*op)),
            _ => panic!("Enumerable#reduce: the second argument must be an operator Symbol"),
        },
        _ => panic!("Enumerable#reduce needs a block or an operator Symbol"),
    };
    let acc: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(init));
    let acc2 = acc.clone();
    for_each(recv, move |yielded| {
        let elem = pack(yielded);
        let mut a = acc2.lock();
        let next = match a.take() {
            None => elem, // first element seeds, block NOT called
            Some(current) => {
                // Drop the guard across the user call: the block itself
                // may (indirectly) re-enter this same accumulator's
                // Enumerable machinery.
                drop(a);
                let next = match &step {
                    Step::Block(b) => b(&[current, elem])?,
                    Step::Op(op) => send_value(&current, *op, &[elem], None)?,
                };
                *acc2.lock() = Some(next);
                return Ok(RubyValue::Nil);
            }
        };
        *a = Some(next);
        Ok(RubyValue::Nil)
    })?;
    let result = acc.lock().take().unwrap_or(RubyValue::Nil);
    Ok(result)
}

/// each_with_index: yields `(packed_element, index)` as TWO args
/// (enum.c:2993-3000) and returns the receiver itself, not an array.
fn each_with_index(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    reject_args(args, "each_with_index", "arguments (forwarding them to #each)");
    let blk = require_block(block, "each_with_index");
    let idx = Arc::new(Mutex::new(0i64));
    let idx2 = idx.clone();
    for_each(recv, move |yielded| {
        let i = {
            let mut n = idx2.lock();
            let i = *n;
            *n += 1;
            i
        };
        blk(&[pack(yielded), RubyValue::Int(i)])?;
        Ok(RubyValue::Nil)
    })?;
    Ok(recv.clone())
}

/// sum(init = 0) { |elem| ... } -- CRuby's numeric ladder minus the
/// Rational leg: pure-Int accumulation, switching to Kahan-Babuska
/// compensated f64 summation on the first Float (enum.c:4659-4710's
/// algorithm), and to generic `+` dispatch on the first non-numeric
/// (sticking there). The optional block is applied to the PACKED element
/// (single argument -- `rb_yield(i)`, enum.c:4712-4746).
fn sum(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    enum Acc {
        Int(i64),
        Float { sum: f64, compensation: f64 },
        Generic(RubyValue),
    }
    impl Acc {
        fn add(self, v: RubyValue) -> Result<Acc, Signal> {
            Ok(match (self, v) {
                (Acc::Int(a), RubyValue::Int(b)) => match a.checked_add(b) {
                    Some(n) => Acc::Int(n),
                    None => panic!("integer overflow in Enumerable#sum (Bignum isn't supported, spike scope)"),
                },
                (Acc::Int(a), RubyValue::Float(b)) => Acc::Float {
                    sum: a as f64 + b,
                    compensation: 0.0,
                },
                (Acc::Float { sum, compensation }, v @ (RubyValue::Int(_) | RubyValue::Float(_))) => {
                    let x = match v {
                        RubyValue::Int(i) => i as f64,
                        RubyValue::Float(f) => f,
                        _ => unreachable!(),
                    };
                    // Kahan-Babuska: t = sum + x, compensating with
                    // whichever operand lost precision.
                    let t = sum + x;
                    let compensation = if sum.abs() >= x.abs() {
                        compensation + ((sum - t) + x)
                    } else {
                        compensation + ((x - t) + sum)
                    };
                    Acc::Float { sum: t, compensation }
                }
                (Acc::Int(a), other) => {
                    Acc::Generic(send_value(&RubyValue::Int(a), Symbol::intern("+"), &[other], None)?)
                }
                (Acc::Float { sum, compensation }, other) => Acc::Generic(send_value(
                    &RubyValue::Float(sum + compensation),
                    Symbol::intern("+"),
                    &[other],
                    None,
                )?),
                (Acc::Generic(a), other) => {
                    Acc::Generic(send_value(&a, Symbol::intern("+"), &[other], None)?)
                }
            })
        }
        fn finish(self) -> RubyValue {
            match self {
                Acc::Int(n) => RubyValue::Int(n),
                Acc::Float { sum, compensation } => RubyValue::Float(sum + compensation),
                Acc::Generic(v) => v,
            }
        }
    }
    let init = match args.len() {
        0 => RubyValue::Int(0),
        1 => args[0].clone(),
        _ => panic!("Enumerable#sum takes at most one argument"),
    };
    let acc = Arc::new(Mutex::new(Some(match init {
        RubyValue::Int(n) => Acc::Int(n),
        RubyValue::Float(f) => Acc::Float { sum: f, compensation: 0.0 },
        other => Acc::Generic(other),
    })));
    let blk = match block {
        Some(RubyValue::Proc(p)) => Some(p),
        _ => None,
    };
    let acc2 = acc.clone();
    for_each(recv, move |yielded| {
        let mut elem = pack(yielded);
        if let Some(b) = &blk {
            elem = b(&[elem])?;
        }
        let current = acc2.lock().take().expect("accumulator always present");
        let next = current.add(elem)?;
        *acc2.lock() = Some(next);
        Ok(RubyValue::Nil)
    })?;
    let result = acc.lock().take().expect("accumulator always present").finish();
    Ok(result)
}

/// min/max: blockless compares via a native Int/Float/String `<=>`
/// (CRuby's OPTIMIZED_CMP fast paths -- user-defined `<=>` dispatch is a
/// documented gap); the block form receives `(candidate, current)` and
/// must return a negative/zero/positive Int. First element seeds; empty
/// -> nil. The `n`-smallest/largest forms are rejected (spike scope).
fn min_max(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    want_min: bool,
) -> Result<RubyValue, Signal> {
    let method = if want_min { "min" } else { "max" };
    reject_args(args, method, "an `n` argument");
    let blk = match block {
        Some(RubyValue::Proc(p)) => Some(p),
        _ => None,
    };
    let best: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
    let best2 = best.clone();
    let method_name = method.to_string();
    for_each(recv, move |yielded| {
        let elem = pack(yielded);
        let mut b = best2.lock();
        let replace = match &*b {
            None => true,
            Some(current) => {
                let ord = match &blk {
                    Some(cmp) => {
                        let current = current.clone();
                        drop(b);
                        let r = cmp(&[elem.clone(), current])?;
                        b = best2.lock();
                        match r {
                            RubyValue::Int(n) => n,
                            _ => panic!("Enumerable#{method_name}: comparison block must return an Integer"),
                        }
                    }
                    None => native_cmp(&elem, b.as_ref().expect("checked Some"), &method_name),
                };
                if want_min {
                    ord < 0
                } else {
                    ord > 0
                }
            }
        };
        if replace {
            *b = Some(elem);
        }
        Ok(RubyValue::Nil)
    })?;
    let result = best.lock().take().unwrap_or(RubyValue::Nil);
    Ok(result)
}

fn native_cmp(a: &RubyValue, b: &RubyValue, method: &str) -> i64 {
    let ord = match (a, b) {
        (RubyValue::Int(x), RubyValue::Int(y)) => x.cmp(y) as i64,
        (RubyValue::Float(x), RubyValue::Float(y)) => x
            .partial_cmp(y)
            .map(|o| o as i64)
            .unwrap_or_else(|| panic!("comparison of Float with Float failed (NaN)")),
        (RubyValue::Int(x), RubyValue::Float(y)) => (*x as f64)
            .partial_cmp(y)
            .map(|o| o as i64)
            .unwrap_or_else(|| panic!("comparison of Integer with Float failed (NaN)")),
        (RubyValue::Float(x), RubyValue::Int(y)) => x
            .partial_cmp(&(*y as f64))
            .map(|o| o as i64)
            .unwrap_or_else(|| panic!("comparison of Float with Integer failed (NaN)")),
        (RubyValue::Str(x), RubyValue::Str(y)) => {
            let x = x.lock().clone();
            let y = y.lock().clone();
            x.cmp(&y) as i64
        }
        (a, b) => panic!(
            "Enumerable#{method}: comparison of {} with {} isn't supported yet (only Int/Float/String compare natively; user-defined <=> dispatch is spike scope)",
            a.to_display_string(),
            b.to_display_string()
        ),
    };
    ord
}
