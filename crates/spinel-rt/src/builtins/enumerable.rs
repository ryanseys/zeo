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
//! **Blockless forms return real Enumerators** (Phase 17.2, via the
//! `block_or_enum!` early return): each captures `(recv, method, args)`
//! and re-invokes the method when iterated -- see
//! `builtins::enumerator`'s module docs for the fiber-backed external
//! iteration behind `#next`/`#peek`.
//!
//! Documented divergences beyond the block-splat note above: `find`'s
//! optional `if_none` callable, `any?`/`all?`/`none?`/`one?`'s pattern-arg
//! (`===`) forms, and `min`/`max`'s `n`-largest forms are rejected loudly;
//! blockless `min`/`max` compare via a native Int/Float/String `<=>` only
//! (no user-defined `<=>` dispatch); `sum`'s float path IS
//! Kahan-Babuska-compensated like CRuby's, but the Rational leg of its
//! numeric tower doesn't exist here.

use crate::builtins::block_or_enum;
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
        "sort" => sort(recv, args, block),
        "sort_by" => sort_by(recv, args, block),
        "min_by" => min_max_by(recv, args, block, true),
        "max_by" => min_max_by(recv, args, block, false),
        "minmax" => minmax(recv, args, block),
        "group_by" => group_by(recv, args, block),
        "partition" => partition(recv, args, block),
        "flat_map" | "collect_concat" => flat_map(recv, args, block),
        "filter_map" => filter_map(recv, args, block),
        "each_slice" => each_slice(recv, args, block),
        "each_cons" => each_cons(recv, args, block),
        "each_with_object" => each_with_object(recv, args, block),
        "take" => take_drop(recv, args, true),
        "drop" => take_drop(recv, args, false),
        "take_while" => take_drop_while(recv, args, block, true),
        "drop_while" => take_drop_while(recv, args, block, false),
        "find_index" => enum_find_index(recv, args, block),
        "tally" => tally(recv, args),
        "uniq" => uniq(recv, args),
        "to_h" => enum_to_h(recv, args, block),
        "reverse_each" => reverse_each(recv, args, block),
        _ => return None,
    })
}

/// Name membership for `respond_to?`'s MRO walk (Phase 17.1) -- kept next
/// to `enumerable_send`'s match, which stays the single source of truth
/// for what actually DISPATCHES; this list must mirror its arms.
pub(crate) fn responds(name: &str) -> bool {
    matches!(
        name,
        "map" | "collect"
            | "select" | "filter" | "find_all"
            | "reject"
            | "to_a" | "entries"
            | "include?" | "member?"
            | "count"
            | "any?" | "all?" | "none?" | "one?"
            | "find" | "detect"
            | "first"
            | "reduce" | "inject"
            | "each_with_index"
            | "sum"
            | "min" | "max"
            | "sort" | "sort_by"
            | "min_by" | "max_by" | "minmax"
            | "group_by" | "partition"
            | "flat_map" | "collect_concat" | "filter_map"
            | "each_slice" | "each_cons" | "each_with_object"
            | "take" | "drop" | "take_while" | "drop_while"
            | "find_index"
            | "tally" | "uniq" | "to_h" | "reverse_each"
    )
}

/// CRuby's `rb_enum_values_pack` rule -- see the module docs. Shared
/// with `enumerator`'s with_index/with_object wrappers (Phase 17.2).
pub(crate) fn pack(args: &[RubyValue]) -> RubyValue {
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
    let proc_: RProc = RProc::new(f);
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

fn reject_args(args: &[RubyValue], method: &str, what: &str) {
    if !args.is_empty() {
        panic!("Enumerable#{method} with {what} isn't supported yet (spike scope)");
    }
}

/// map/collect: the user block receives the RAW yielded values
/// (`rb_yield_values2`, enum.c:631-633); results collect into an Array.
fn map(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "map", "arguments");
    let blk = block_or_enum!(recv, "map", args, block);
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
    let blk = block_or_enum!(recv, method, args, block);
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
    let blk = block_or_enum!(recv, "find", args, block);
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
    let blk = block_or_enum!(recv, "each_with_index", args, block);
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

/// min/max: blockless compares via `RubyValue::rb_cmp` (Phase 16.2 --
/// native Int/Float/String fast paths PLUS user-defined `<=>` dispatch,
/// retiring the documented native-only gap); the block form receives
/// `(candidate, current)` and must return a negative/zero/positive Int.
/// First element seeds; empty -> nil. The `n`-smallest/largest forms are
/// rejected (spike scope).
fn min_max(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    want_min: bool,
) -> Result<RubyValue, Signal> {
    let method = if want_min { "min" } else { "max" };
    // `min(n)`/`max(n)`: the n smallest/largest, as an Array -- sorted
    // ascending for `min`, descending for `max` (CRuby's nsmallest/
    // nlargest). Collect-then-sort (not a bounded heap): honest for the
    // corpus's enumerable sizes.
    if let Some(n_arg) = args.first() {
        let RubyValue::Int(n) = n_arg else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(n_arg)
                ),
            ));
        };
        if *n < 0 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("negative size ({n})"),
            ));
        }
        let items: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
        let items2 = items.clone();
        for_each(recv, move |yielded| {
            items2.lock().push(pack(yielded));
            Ok(RubyValue::Nil)
        })?;
        let mut items = std::mem::take(&mut *items.lock());
        crate::builtins::array::sort_items(&mut items, &block)?;
        if !want_min {
            items.reverse();
        }
        items.truncate(*n as usize);
        return Ok(RubyValue::Array(array_new(items)));
    }
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
                    None => cmp_or_fail(&elem, b.as_ref().expect("checked Some"), &method_name),
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

/// `rb_cmp` with real Ruby's incomparable-elements failure applied
/// (`ArgumentError: comparison of X with Y failed` -- raised as a loud
/// panic, the established no-exception-channel posture).
fn cmp_or_fail(a: &RubyValue, b: &RubyValue, method: &str) -> i64 {
    a.rb_cmp(b).unwrap_or_else(|| {
        panic!(
            "Enumerable#{method}: comparison of {} with {} failed (`<=>` returned nil; ArgumentError in real Ruby -- raised as a panic, spike scope)",
            a.to_display_string(),
            b.to_display_string()
        )
    })
}

/// Materializes the receiver's elements -- the shared front half of every
/// whole-collection method below (sort/group_by/...; CRuby's own enum.c
/// materializes for these too). The RAW yielded values are kept alongside
/// the packed element: user blocks receive the raw shape (`|k, v|` on a
/// Hash -- the auto-splat CRuby's rb_yield does at proc-call time),
/// result collections carry the packed one.
struct Element {
    raw: Vec<RubyValue>,
    packed: RubyValue,
}

fn collect_elements(recv: &RubyValue) -> Result<Vec<Element>, Signal> {
    let out: Arc<Mutex<Vec<Element>>> = Arc::new(Mutex::new(Vec::new()));
    let out2 = out.clone();
    for_each(recv, move |yielded| {
        out2.lock().push(Element {
            raw: yielded.to_vec(),
            packed: pack(yielded),
        });
        Ok(RubyValue::Nil)
    })?;
    let items = std::mem::take(&mut *out.lock());
    Ok(items)
}

fn collect_packed(recv: &RubyValue) -> Result<Vec<RubyValue>, Signal> {
    Ok(collect_elements(recv)?.into_iter().map(|e| e.packed).collect())
}

fn sort(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "sort", "arguments");
    let mut items = collect_packed(recv)?;
    crate::builtins::array::sort_items(&mut items, &block)?;
    Ok(RubyValue::Array(array_new(items)))
}

fn sort_by(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "sort_by", "arguments");
    let blk = block_or_enum!(recv, "sort_by", args, block);
    let items = collect_elements(recv)?;
    // Decorate-sort-undecorate, keys ordered by rb_cmp.
    let mut decorated: Vec<(RubyValue, RubyValue)> = Vec::with_capacity(items.len());
    for e in items {
        let key = blk(&e.raw)?;
        decorated.push((key, e.packed));
    }
    let mut failure = false;
    decorated.sort_by(|a, b| match a.0.rb_cmp(&b.0) {
        Some(c) => c.cmp(&0),
        None => {
            failure = true;
            std::cmp::Ordering::Equal
        }
    });
    if failure {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "comparison failed".to_string(),
        ));
    }
    Ok(RubyValue::Array(array_new(
        decorated.into_iter().map(|(_, e)| e).collect(),
    )))
}

fn min_max_by(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    min: bool,
) -> Result<RubyValue, Signal> {
    reject_args(args, if min { "min_by" } else { "max_by" }, "arguments");
    let blk = block_or_enum!(recv, if min { "min_by" } else { "max_by" }, args, block);
    let items = collect_elements(recv)?;
    let mut best: Option<(RubyValue, RubyValue)> = None;
    for e in items {
        let key = blk(&e.raw)?;
        let e = e.packed;
        let better = match &best {
            None => true,
            Some((bk, _)) => match key.rb_cmp(bk) {
                Some(c) => (min && c < 0) || (!min && c > 0),
                None => false,
            },
        };
        if better {
            best = Some((key, e));
        }
    }
    Ok(best.map(|(_, e)| e).unwrap_or(RubyValue::Nil))
}

fn minmax(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "minmax", "arguments");
    let lo = min_max(recv, &[], block.clone(), true)?;
    let hi = min_max(recv, &[], block, false)?;
    Ok(RubyValue::Array(array_new(vec![lo, hi])))
}

fn group_by(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "group_by", "arguments");
    let blk = block_or_enum!(recv, "group_by", args, block);
    let items = collect_elements(recv)?;
    let groups = crate::hash_new(Vec::new());
    for e in items {
        let key = blk(&e.raw)?;
        let e = e.packed;
        let bucket = crate::hash_get(&groups, &key);
        match bucket {
            RubyValue::Array(a) => {
                a.lock().push(e);
            }
            _ => {
                crate::hash_set(&groups, key, RubyValue::Array(array_new(vec![e])));
            }
        }
    }
    Ok(RubyValue::Hash(groups))
}

fn partition(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "partition", "arguments");
    let blk = block_or_enum!(recv, "partition", args, block);
    let items = collect_elements(recv)?;
    let (mut yes, mut no) = (Vec::new(), Vec::new());
    for e in items {
        if blk(&e.raw)?.truthy() {
            yes.push(e.packed);
        } else {
            no.push(e.packed);
        }
    }
    Ok(RubyValue::Array(array_new(vec![
        RubyValue::Array(array_new(yes)),
        RubyValue::Array(array_new(no)),
    ])))
}

fn flat_map(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "flat_map", "arguments");
    let blk = block_or_enum!(recv, "flat_map", args, block);
    let items = collect_elements(recv)?;
    let mut out = Vec::new();
    for e in items {
        match blk(&e.raw)? {
            // ONE level of flattening (real Ruby's rule).
            RubyValue::Array(a) => out.extend(a.lock().iter().cloned()),
            other => out.push(other),
        }
    }
    Ok(RubyValue::Array(array_new(out)))
}

fn filter_map(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "filter_map", "arguments");
    let blk = block_or_enum!(recv, "filter_map", args, block);
    let items = collect_elements(recv)?;
    let mut out = Vec::new();
    for e in items {
        let mapped = blk(&e.raw)?;
        if mapped.truthy() {
            out.push(mapped);
        }
    }
    Ok(RubyValue::Array(array_new(out)))
}

fn slice_size(args: &[RubyValue], method: &str) -> Result<usize, Signal> {
    let Some(RubyValue::Int(n)) = args.first() else {
        panic!("Enumerable#{method} takes one Integer argument");
    };
    if *n < 1 {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "invalid size".to_string(),
        ));
    }
    Ok(*n as usize)
}

fn each_slice(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let n = slice_size(args, "each_slice")?;
    let blk = block_or_enum!(recv, "each_slice", args, block);
    let items = collect_packed(recv)?;
    for chunk in items.chunks(n) {
        blk(&[RubyValue::Array(array_new(chunk.to_vec()))])?;
    }
    Ok(RubyValue::Nil)
}

fn each_cons(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let n = slice_size(args, "each_cons")?;
    let blk = block_or_enum!(recv, "each_cons", args, block);
    let items = collect_packed(recv)?;
    if items.len() >= n {
        for window in items.windows(n) {
            blk(&[RubyValue::Array(array_new(window.to_vec()))])?;
        }
    }
    Ok(RubyValue::Nil)
}

fn each_with_object(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    if args.len() != 1 {
        panic!("Enumerable#each_with_object takes exactly one argument");
    }
    let blk = block_or_enum!(recv, "each_with_object", args, block);
    let memo = args[0].clone();
    let items = collect_packed(recv)?;
    for e in items {
        blk(&[e, memo.clone()])?;
    }
    Ok(memo)
}

fn take_drop(recv: &RubyValue, args: &[RubyValue], take: bool) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Int(n)) = args.first() else {
        panic!("Enumerable#take/drop takes one Integer argument");
    };
    if *n < 0 {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "attempt to take negative size".to_string(),
        ));
    }
    if take {
        // Early termination once n elements are in (CRuby's take_i breaks
        // via rb_iter_break) -- what makes `take` on an INFINITE
        // Enumerator.new generator terminate (Phase 17.2).
        let cap = *n as usize;
        if cap == 0 {
            return Ok(RubyValue::Array(array_new(Vec::new())));
        }
        let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
        let out2 = out.clone();
        for_each(recv, move |yielded| {
            let mut o = out2.lock();
            o.push(pack(yielded));
            if o.len() >= cap {
                return Err(Signal::Break(RubyValue::Nil));
            }
            Ok(RubyValue::Nil)
        })?;
        let items = std::mem::take(&mut *out.lock());
        return Ok(RubyValue::Array(array_new(items)));
    }
    let items = collect_packed(recv)?;
    let n = (*n as usize).min(items.len());
    Ok(RubyValue::Array(array_new(items[n..].to_vec())))
}

fn take_drop_while(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    take: bool,
) -> Result<RubyValue, Signal> {
    reject_args(args, if take { "take_while" } else { "drop_while" }, "arguments");
    let blk = block_or_enum!(recv, if take { "take_while" } else { "drop_while" }, args, block);
    let items = collect_elements(recv)?;
    let mut boundary = items.len();
    for (i, e) in items.iter().enumerate() {
        if !blk(&e.raw)?.truthy() {
            boundary = i;
            break;
        }
    }
    let packed: Vec<RubyValue> = items.into_iter().map(|e| e.packed).collect();
    Ok(RubyValue::Array(array_new(if take {
        packed[..boundary].to_vec()
    } else {
        packed[boundary..].to_vec()
    })))
}

fn tally(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    reject_args(args, "tally", "arguments");
    let items = collect_packed(recv)?;
    let counts = crate::hash_new(Vec::new());
    for e in items {
        let n = match crate::hash_get(&counts, &e) {
            RubyValue::Int(n) => n + 1,
            _ => 1,
        };
        crate::hash_set(&counts, e, RubyValue::Int(n));
    }
    Ok(RubyValue::Hash(counts))
}

fn uniq(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    reject_args(args, "uniq", "arguments");
    let items = collect_packed(recv)?;
    let mut out: Vec<RubyValue> = Vec::new();
    for e in items {
        if !out.iter().any(|x| e.rb_eq(x)) {
            out.push(e);
        }
    }
    Ok(RubyValue::Array(array_new(out)))
}

fn enum_to_h(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "to_h", "arguments");
    let items = collect_elements(recv)?;
    let pairs = to_h_pairs(items.iter().map(|e| (e.raw.as_slice(), &e.packed)), &block)?;
    Ok(RubyValue::Hash(crate::hash_new(pairs)))
}

/// The shared back half of every `to_h` (Enumerable's, `Array`'s and
/// `Hash`'s rows all route here): each element must BE a two-element Array,
/// or -- with a block -- must be MAPPED to one by it. The block receives the
/// RAW yielded values (`rb_yield_values2`: `{a: 1}.to_h { |pair| }` sees
/// just the key, while `{ |k, v| }` sees both), so a Hash's pair-shaped
/// element and an Array's array-shaped element behave exactly as in CRuby.
/// Both failure messages carry the element's index, matching enum.c.
pub(crate) fn to_h_pairs<'a>(
    elements: impl Iterator<Item = (&'a [RubyValue], &'a RubyValue)>,
    block: &Option<RubyValue>,
) -> Result<Vec<(RubyValue, RubyValue)>, Signal> {
    let mut pairs = Vec::new();
    for (i, (raw, packed)) in elements.enumerate() {
        let e = match block {
            Some(RubyValue::Proc(p)) => p(raw)?,
            _ => packed.clone(),
        };
        let RubyValue::Array(pair) = &e else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "wrong element type {} at {i} (expected array)",
                    crate::builtins::class_name_of(&e)
                ),
            ));
        };
        let pair = pair.lock().clone();
        if pair.len() != 2 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("wrong array length at {i} (expected 2, was {})", pair.len()),
            ));
        }
        pairs.push((pair[0].clone(), pair[1].clone()));
    }
    Ok(pairs)
}

fn reverse_each(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    reject_args(args, "reverse_each", "arguments");
    let blk = block_or_enum!(recv, "reverse_each", args, block);
    let items = collect_elements(recv)?;
    for e in items.iter().rev() {
        blk(&e.raw)?;
    }
    Ok(recv.clone())
}

/// `find_index(value)` / `find_index { |e| ... }`.
fn enum_find_index(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let items = collect_elements(recv)?;
    if let Some(RubyValue::Proc(p)) = &block {
        for (i, e) in items.iter().enumerate() {
            if p(&e.raw)?.truthy() {
                return Ok(RubyValue::Int(i as i64));
            }
        }
        return Ok(RubyValue::Nil);
    }
    if args.len() != 1 {
        panic!("Enumerable#find_index takes a value or a block");
    }
    for (i, e) in items.iter().enumerate() {
        if e.packed.rb_eq(&args[0]) {
            return Ok(RubyValue::Int(i as i64));
        }
    }
    Ok(RubyValue::Nil)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::array_new;

    fn ints(vals: &[i64]) -> RubyValue {
        RubyValue::Array(array_new(vals.iter().map(|v| RubyValue::Int(*v)).collect()))
    }

    /// `min(n)`/`max(n)` answer the n smallest/largest as an Array --
    /// ascending for `min`, DESCENDING for `max` (CRuby's nsmallest/
    /// nlargest). Oracle-verified.
    #[test]
    fn min_with_a_count_answers_the_n_smallest_ascending() {
        let out = enumerable_send(&ints(&[5, 1, 4, 2, 3]), "min", &[RubyValue::Int(3)], None)
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "[1, 2, 3]");
    }

    #[test]
    fn max_with_a_count_answers_the_n_largest_descending() {
        let out = enumerable_send(&ints(&[5, 1, 4, 2, 3]), "max", &[RubyValue::Int(3)], None)
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "[5, 4, 3]");
    }

    /// A count larger than the collection just yields everything.
    #[test]
    fn min_max_with_a_count_past_the_end_yields_everything() {
        let out = enumerable_send(&ints(&[2, 1]), "min", &[RubyValue::Int(9)], None)
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "[1, 2]");
    }

    #[test]
    fn min_with_a_zero_count_is_empty() {
        let out = enumerable_send(&ints(&[3, 1]), "min", &[RubyValue::Int(0)], None)
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "[]");
    }

    /// The blockless no-count forms are unaffected.
    #[test]
    fn min_max_without_a_count_still_answer_a_single_element() {
        let min = enumerable_send(&ints(&[5, 1, 4]), "min", &[], None).unwrap().unwrap();
        assert_eq!(min.inspect_string(), "1");
        let max = enumerable_send(&ints(&[5, 1, 4]), "max", &[], None).unwrap().unwrap();
        assert_eq!(max.inspect_string(), "5");
    }

    /// A comparison block drives the ordering for the n-form too.
    #[test]
    fn min_with_a_count_and_a_comparison_block() {
        // Reverse the comparison, so "min" picks the largest.
        let cmp: crate::RProc = crate::RProc::new(|args: &[RubyValue]| {
            let (RubyValue::Int(a), RubyValue::Int(b)) = (&args[0], &args[1]) else {
                panic!("ints only")
            };
            Ok(RubyValue::Int((b - a).signum()))
        });
        let out = enumerable_send(&ints(&[5, 1, 4]), "min", &[RubyValue::Int(2)], Some(RubyValue::Proc(cmp)))
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "[5, 4]");
    }

    /// A negative count is CRuby's ArgumentError (registry-less: a panic).
    #[test]
    fn min_with_a_negative_count_is_an_argument_error() {
        let r = std::panic::catch_unwind(|| {
            enumerable_send(&ints(&[1]), "min", &[RubyValue::Int(-1)], None)
        });
        assert!(r.is_err());
    }

    /// `to_h`'s block maps each element to its pair; the block sees the RAW
    /// yielded values (an Array yields one value per element).
    #[test]
    fn to_h_maps_elements_through_a_block() {
        let blk: crate::RProc = crate::RProc::new(|args: &[RubyValue]| {
            let RubyValue::Int(i) = &args[0] else { panic!("ints only") };
            Ok(RubyValue::Array(array_new(vec![
                RubyValue::Int(*i),
                RubyValue::Int(i * i),
            ])))
        });
        let out = enumerable_send(&ints(&[1, 2, 3]), "to_h", &[], Some(RubyValue::Proc(blk)))
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "{1 => 1, 2 => 4, 3 => 9}");
    }

    /// A block answering a non-pair is a TypeError/ArgumentError citing the
    /// element's INDEX, matching enum.c.
    #[test]
    fn to_h_rejects_a_non_pair_block_result() {
        let blk: crate::RProc = crate::RProc::new(|_| Ok(RubyValue::Int(1)));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            enumerable_send(&ints(&[1]), "to_h", &[], Some(RubyValue::Proc(blk)))
        }));
        assert!(r.is_err());
    }
}
