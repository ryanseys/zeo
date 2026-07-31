//! `Enumerable`, implemented in Rust -- the direct
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
//! **Blockless forms return real Enumerators** (via the
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
use crate::builtins::{arg_error, type_error};
use crate::collections::array_new;
use crate::dispatch::send_value;
use crate::signal::Signal;
use crate::value::RubyValue;
use crate::{RProc, Symbol};
use parking_lot::Mutex;
use std::sync::Arc;
use zeo_macros::ruby_module;

/// Compat shim over the generated `lookup` table, for the runtime's
/// internal callers (`range`/`array`/`kernel` forward whole-collection
/// methods here). `None` = not an Enumerable method this runtime implements
/// (the caller falls through to its `NoMethodError` path) -- the table
/// below is the SINGLE source of truth for what Enumerable supports;
/// codegen deliberately keeps no mirror list (see `codegen::call`'s
/// Enumerable fallback docs).
pub(crate) fn enumerable_send(
    recv: &RubyValue,
    name: &str,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Option<Result<RubyValue, Signal>> {
    lookup(name).map(|f| f(recv, args, block))
}

/// CRuby's `rb_enum_values_pack` rule -- see the module docs. Shared
/// with `enumerator`'s with_index/with_object wrappers.
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
        crate::symbol::wk::each(),
        &[],
        Some(RubyValue::Proc(proc_)),
    ) {
        Ok(_) => Ok(()),
        Err(Signal::Break(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Every value `recv`'s `each` yields, in order -- the primitive a `for` loop
/// over an arbitrary object needs.
///
/// Real Ruby's `for x in obj` compiles to nothing more than `obj.each { |x|
/// ... }` (`compile_iter`, compile.c:8548), so `for` works on ANY receiver
/// answering `each`, and one that doesn't raises NoMethodError at runtime.
/// A multi-value yield packs into an Array exactly as a block param would,
/// which is what lets `for k, v in pairs` destructure.
pub fn each_values(recv: &RubyValue) -> Result<Vec<RubyValue>, Signal> {
    let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
    let out2 = out.clone();
    for_each(recv, move |yielded| {
        out2.lock().push(pack(yielded));
        Ok(RubyValue::Nil)
    })?;
    let items = std::mem::take(&mut *out.lock());
    Ok(items)
}

/// Invokes a user block from inside a `for_each` driver, routing a user
/// `break <v>` into `stash` and converting it to the internal early-stop
/// signal so `for_each` ends the iteration. The Enumerable method then
/// returns the stashed value (`user_break`) -- CRuby's TAG_BREAK: a `break`
/// in the block makes the whole iterator call evaluate to that value, rather
/// than the partial accumulator. Ordinary results and other signals (a raise,
/// a `Signal::Return`) pass through untouched. Without this, `for_each`'s
/// blanket `Break` swallow (which exists for internal early-stop like `find`)
/// would discard the user's break value.
fn yield_block(
    blk: &RProc,
    args: &[RubyValue],
    stash: &Mutex<Option<RubyValue>>,
) -> Result<RubyValue, Signal> {
    match blk.call(args) {
        Err(Signal::Break(v)) => {
            *stash.lock() = Some(v);
            Err(Signal::Break(RubyValue::Nil))
        }
        other => other,
    }
}

/// The value a user `break` stashed (see [`yield_block`]), if any -- the
/// early-return an Enumerable method makes before yielding its normal result.
fn user_break(stash: &Mutex<Option<RubyValue>>) -> Option<RubyValue> {
    stash.lock().take()
}

fn reject_args(args: &[RubyValue], method: &str, what: &str) {
    if !args.is_empty() {
        panic!("Enumerable#{method} with {what} isn't supported yet (zeo limitation)");
    }
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
    let brk: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
    let brk2 = brk.clone();
    for_each(recv, move |yielded| {
        if yield_block(&blk, yielded, &brk2)?.truthy() == keep {
            out2.lock().push(pack(yielded));
        }
        Ok(RubyValue::Nil)
    })?;
    if let Some(v) = user_break(&brk) {
        return Ok(v);
    }
    let items = std::mem::take(&mut *out.lock());
    Ok(RubyValue::Array(array_new(items)))
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
    // `any?(pattern)`/... tests `pattern === element` (case equality). A
    // pattern argument takes precedence over a block, matching CRuby.
    let pattern = args.first().cloned();
    let blk = match block {
        Some(RubyValue::Proc(p)) => Some(p),
        _ => None,
    };
    let test = move |yielded: &[RubyValue]| -> Result<bool, Signal> {
        if let Some(pat) = &pattern {
            let elem = pack(yielded);
            return Ok(crate::dispatch::send_value(
                pat,
                crate::Symbol::intern("==="),
                std::slice::from_ref(&elem),
                None,
            )?
            .truthy());
        }
        match &blk {
            Some(b) => Ok(b.call(yielded)?.truthy()),
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

/// min/max: blockless compares via `RubyValue::rb_cmp` (native
/// Int/Float/String fast paths PLUS user-defined `<=>` dispatch,
/// retiring the documented native-only gap); the block form receives
/// `(candidate, current)` and must return a negative/zero/positive Int.
/// First element seeds; empty -> nil. The `n`-smallest/largest forms are
/// rejected (zeo limitation).
fn min_max(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    want_min: bool,
) -> Result<RubyValue, Signal> {
    // `min(n)`/`max(n)`: the n smallest/largest, as an Array -- sorted
    // ascending for `min`, descending for `max` (CRuby's nsmallest/
    // nlargest). Collect-then-sort (not a bounded heap): honest for the
    // corpus's enumerable sizes.
    if let Some(n_arg) = args.first().filter(|v| !matches!(v, RubyValue::Nil)) {
        let n = &crate::builtins::convert::to_index(n_arg)?;
        if *n < 0 {
            return Err(arg_error!("negative size ({n})"));
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
                        let r = cmp.call(&[elem.clone(), current.clone()])?;
                        b = best2.lock();
                        match crate::value::cmp_int(&r)? {
                            Some(n) => n,
                            None => return Err(crate::value::cmp_error(&elem, &current)),
                        }
                    }
                    None => crate::value::cmp_or_raise(&elem, b.as_ref().expect("checked Some"))?,
                };
                if want_min { ord < 0 } else { ord > 0 }
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
    Ok(collect_elements(recv)?
        .into_iter()
        .map(|e| e.packed)
        .collect())
}

fn min_max_by(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    min: bool,
) -> Result<RubyValue, Signal> {
    let name = if min { "min_by" } else { "max_by" };
    // Optional count `n`: the n smallest (min_by, ascending) / largest (max_by,
    // descending) elements by key, as an Array. A negative count raises
    // ArgumentError; no count answers the single best element.
    let count = match args.first() {
        None | Some(RubyValue::Nil) => None,
        Some(v) => {
            let n = crate::builtins::convert::to_index(v)?;
            if n < 0 {
                return Err(arg_error!("negative size ({n})"));
            }
            Some(n as usize)
        }
    };
    let blk = block_or_enum!(recv, name, &[], block);
    let items = collect_elements(recv)?;

    if let Some(n) = count {
        let mut keyed: Vec<(RubyValue, RubyValue)> = Vec::with_capacity(items.len());
        for e in items {
            let key = blk.call(&e.raw)?;
            keyed.push((key, e.packed));
        }
        // Ascending by key for min_by, descending for max_by; ties are
        // order-unspecified in CRuby (a heap), and the corpus uses distinct
        // keys, so a stable sort on the comparison is faithful enough.
        keyed.sort_by(|a, b| {
            let ord =
                a.0.rb_cmp(&b.0)
                    .map_or(std::cmp::Ordering::Equal, |c| c.cmp(&0));
            if min { ord } else { ord.reverse() }
        });
        let out = keyed.into_iter().take(n).map(|(_, e)| e).collect();
        return Ok(RubyValue::Array(array_new(out)));
    }

    let mut best: Option<(RubyValue, RubyValue)> = None;
    for e in items {
        let key = blk.call(&e.raw)?;
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

pub(crate) fn slice_size(args: &[RubyValue], method: &str) -> Result<usize, Signal> {
    let Some(RubyValue::Int(n)) = args.first() else {
        panic!("Enumerable#{method} takes one Integer argument");
    };
    if *n < 1 {
        // CRuby names it differently per method: `each_slice` says "invalid
        // slice size", `each_cons` (and the rest) just "invalid size".
        let msg = if method == "each_slice" {
            "invalid slice size"
        } else {
            "invalid size"
        };
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            msg.to_string(),
        ));
    }
    Ok(*n as usize)
}

fn take_drop(recv: &RubyValue, args: &[RubyValue], take: bool) -> Result<RubyValue, Signal> {
    let n = &crate::builtins::convert::to_index(args.first().unwrap_or(&RubyValue::Nil))?;
    if *n < 0 {
        // CRuby names the actual method: `drop(-1)` says "drop", not "take".
        let verb = if take { "take" } else { "drop" };
        return Err(arg_error!("attempt to {verb} negative size"));
    }
    if take {
        // Early termination once n elements are in (CRuby's take_i breaks
        // via rb_iter_break) -- what makes `take` on an INFINITE
        // Enumerator.new generator terminate.
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
    reject_args(
        args,
        if take { "take_while" } else { "drop_while" },
        "arguments",
    );
    let blk = block_or_enum!(
        recv,
        if take { "take_while" } else { "drop_while" },
        args,
        block
    );
    let items = collect_elements(recv)?;
    let mut boundary = items.len();
    for (i, e) in items.iter().enumerate() {
        if !blk.call(&e.raw)?.truthy() {
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
            Some(RubyValue::Proc(p)) => p.call(raw)?,
            _ => packed.clone(),
        };
        let RubyValue::Array(pair) = &e else {
            return Err(type_error!(
                "wrong element type {} at {i} (expected array)",
                crate::builtins::class_name_of(&e)
            ));
        };
        let pair = pair.lock().clone();
        if pair.len() != 2 {
            return Err(arg_error!(
                "wrong array length at {i} (expected 2, was {})",
                pair.len()
            ));
        }
        pairs.push((pair[0].clone(), pair[1].clone()));
    }
    Ok(pairs)
}

/// `pattern === value`, dispatched -- the test `grep`/`slice_before`/
/// `slice_after`'s argument forms are defined in terms of. Sent rather
/// than matched on: `===` means something different for a Range (cover?),
/// a Module (is_a?), a Regexp (match?), a Proc (call) and a plain value
/// (==), and every one of those already has its own row. A user class's
/// own `def ===` works for free for the same reason.
fn case_eq(pattern: &RubyValue, value: &RubyValue) -> Result<bool, Signal> {
    Ok(send_value(
        pattern,
        Symbol::intern("==="),
        std::slice::from_ref(value),
        None,
    )?
    .truthy())
}

/// `grep(pattern)` / `grep(pattern) { |e| ... }` and their `grep_v`
/// negations: select the elements the pattern `===` matches (or doesn't),
/// mapping each through the block first if one is given.
fn grep(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    keep: bool,
) -> Result<RubyValue, Signal> {
    let name = if keep { "grep" } else { "grep_v" };
    if args.len() != 1 {
        panic!("Enumerable#{name} takes exactly one pattern argument");
    }
    let mut out = Vec::new();
    for e in collect_packed(recv)? {
        if case_eq(&args[0], &e)? != keep {
            continue;
        }
        out.push(match &block {
            Some(b) => b.as_proc_unchecked().call(&[e])?,
            None => e,
        });
    }
    Ok(RubyValue::Array(array_new(out)))
}

/// `chunk_while { |a, b| ... }` and `slice_when { |a, b| ... }` -- exact
/// negations of each other: both walk adjacent PAIRS and cut between them,
/// `chunk_while` when the block is FALSE ("keep them together while true"),
/// `slice_when` when it is TRUE ("start a new slice when true"). One
/// implementation with a flipped test, since that is genuinely all the
/// difference is.
///
/// An empty receiver answers `[]`, and a one-element one `[[x]]` -- the
/// block never runs in either case (there is no adjacent pair).
///
/// With a block, answers a Generator-backed Enumerator over the eagerly
/// materialized runs (CRuby's shape: first-class Enumerator, Generator
/// inspect); blockless returns a Method enumerator via `block_or_enum!`.
fn chunk_while(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    cut_on: bool,
) -> Result<RubyValue, Signal> {
    let name = if cut_on { "slice_when" } else { "chunk_while" };
    reject_args(args, name, "arguments");
    let blk = block_or_enum!(recv, name, args, block);
    let items = collect_packed(recv)?;
    let mut out: Vec<RubyValue> = Vec::new();
    let mut cur: Vec<RubyValue> = Vec::new();
    for e in items {
        if let Some(prev) = cur.last().cloned() {
            if blk.call(&[prev, e.clone()])?.truthy() == cut_on {
                out.push(RubyValue::Array(array_new(std::mem::take(&mut cur))));
            }
        }
        cur.push(e);
    }
    if !cur.is_empty() {
        out.push(RubyValue::Array(array_new(cur)));
    }
    // CRuby wraps the runs in a Generator-backed Enumerator (lazy); we
    // materialize eagerly but present the same first-class Enumerator.
    Ok(crate::builtins::enumerator::generator_of(out))
}

/// `slice_before` / `slice_after`, in both their block and pattern-argument
/// forms. `before` cuts so the matching element STARTS the next slice;
/// `after` cuts so it ENDS the current one.
///
/// The leading-empty-slice case is why `before` can't just push on every
/// match: `[1,2,3].slice_before { |x| x == 1 }` is `[[1, 2, 3]]`, not
/// `[[], [1, 2, 3]]` -- a match at the very start opens the first slice
/// rather than closing an empty one.
fn slice_before_after(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    before: bool,
) -> Result<RubyValue, Signal> {
    let name = if before {
        "slice_before"
    } else {
        "slice_after"
    };
    let mut out: Vec<RubyValue> = Vec::new();
    let mut cur: Vec<RubyValue> = Vec::new();
    // Exactly one of a pattern argument or a block, real Ruby's own rule.
    type SliceTest = Box<dyn Fn(&RubyValue) -> Result<bool, Signal>>;
    let test: SliceTest = match (args.len(), &block) {
        (1, None) => {
            let pattern = args[0].clone();
            Box::new(move |e: &RubyValue| case_eq(&pattern, e))
        }
        (0, Some(b)) => {
            let b = b.clone();
            Box::new(move |e: &RubyValue| {
                Ok(b.as_proc_unchecked()
                    .call(std::slice::from_ref(e))?
                    .truthy())
            })
        }
        _ => panic!("Enumerable#{name} takes exactly one pattern argument OR a block"),
    };
    for e in collect_packed(recv)? {
        let hit = test(&e)?;
        if before && hit && !cur.is_empty() {
            out.push(RubyValue::Array(array_new(std::mem::take(&mut cur))));
        }
        cur.push(e);
        if !before && hit {
            out.push(RubyValue::Array(array_new(std::mem::take(&mut cur))));
        }
    }
    if !cur.is_empty() {
        out.push(RubyValue::Array(array_new(cur)));
    }
    Ok(RubyValue::Array(array_new(out)))
}

// The Enumerable method table. Arity annotations (`[n]` = Method#arity
// metadata, unannotated = -1/variadic) are oracle-derived:
// `Enumerable.instance_method(name).arity`. Shared implementations
// (select/reject, the any? family, min/max, take/drop, ...) live in the
// discriminator-parameter fns above; their rows are thin `enum_*`
// forwarders. Everything else is defined directly in its row.
/// `Enumerable#to_a` as a plain fn: drive `#each` and collect. Shared by the
/// `to_a`/`entries` rows and by the sort/uniq/group rows that need the whole
/// materialized sequence internally (they can't call the mangled row fn).
fn collect_to_a(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
    let out2 = out.clone();
    for_each(recv, move |yielded| {
        out2.lock().push(pack(yielded));
        Ok(RubyValue::Nil)
    })?;
    let items = std::mem::take(&mut *out.lock());
    Ok(RubyValue::Array(array_new(items)))
}

/// `Enumerable#sum`'s accumulator -- CRuby's numeric ladder minus the
/// Rational leg: pure-Int accumulation, switching to Kahan-Babuska
/// compensated f64 summation on the first Float (enum.c:4659-4710's
/// algorithm), and to generic `+` dispatch on the first non-numeric
/// (sticking there). Public because the fused inline `sum` loop
/// (`codegen`'s typed-iterator splices) accumulates through the very same
/// type -- one definition of the arithmetic.
pub enum SumAcc {
    Int(i64),
    Float { sum: f64, compensation: f64 },
    Generic(RubyValue),
}

impl SumAcc {
    pub fn new(init: RubyValue) -> SumAcc {
        match init {
            RubyValue::Int(n) => SumAcc::Int(n),
            RubyValue::Float(f) => SumAcc::Float {
                sum: f,
                compensation: 0.0,
            },
            other => SumAcc::Generic(other),
        }
    }
    // Not `std::ops::Add`: this add is fallible (a `Generic` leg dispatches
    // a user `+`) and consumes self by design.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, v: RubyValue) -> Result<SumAcc, Signal> {
        Ok(match (self, v) {
            (SumAcc::Int(a), RubyValue::Int(b)) => match a.checked_add(b) {
                Some(n) => SumAcc::Int(n),
                None => panic!(
                    "integer overflow in Enumerable#sum (zeo limitation: no Bignum promotion here)"
                ),
            },
            (SumAcc::Int(a), RubyValue::Float(b)) => SumAcc::Float {
                sum: a as f64 + b,
                compensation: 0.0,
            },
            (
                SumAcc::Float { sum, compensation },
                v @ (RubyValue::Int(_) | RubyValue::Float(_)),
            ) => {
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
                SumAcc::Float {
                    sum: t,
                    compensation,
                }
            }
            (SumAcc::Int(a), other) => SumAcc::Generic(send_value(
                &RubyValue::Int(a),
                Symbol::intern("+"),
                &[other],
                None,
            )?),
            (SumAcc::Float { sum, compensation }, other) => SumAcc::Generic(send_value(
                &RubyValue::Float(sum + compensation),
                Symbol::intern("+"),
                &[other],
                None,
            )?),
            (SumAcc::Generic(a), other) => {
                SumAcc::Generic(send_value(&a, Symbol::intern("+"), &[other], None)?)
            }
        })
    }
    pub fn finish(self) -> RubyValue {
        match self {
            SumAcc::Int(n) => RubyValue::Int(n),
            SumAcc::Float { sum, compensation } => RubyValue::Float(sum + compensation),
            SumAcc::Generic(v) => v,
        }
    }
}

ruby_module! {
    Enumerable = zeo_abi::ENUMERABLE_CLASS;

    // map/collect: the user block receives the RAW yielded values
    // (`rb_yield_values2`, enum.c:631-633); results collect into an Array.
    def "map" arity 0 | "collect" arity 0 (recv, args, block) {
        reject_args(args, "map", "arguments");
        let blk = block_or_enum!(recv, "map", args, block);
        let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
        let out2 = out.clone();
        let brk: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
        let brk2 = brk.clone();
        for_each(recv, move |yielded| {
            let v = yield_block(&blk, yielded, &brk2)?;
            out2.lock().push(v);
            Ok(RubyValue::Nil)
        })?;
        if let Some(v) = user_break(&brk) {
            return Ok(v);
        }
        let items = std::mem::take(&mut *out.lock());
        Ok(RubyValue::Array(array_new(items)))
    }
    def "select" arity 0 | "filter" arity 0 | "find_all" arity 0 (recv, args, block) {
        select(recv, args, block, true)
    }
    def "reject" arity 0 (recv, args, block) {
        select(recv, args, block, false)
    }
    def "to_a" | "entries"(recv, args, _block) {
        reject_args(args, "to_a", "arguments (forwarding them to #each)");
        collect_to_a(recv)
    }
    // `==`-based membership (`rb_equal`, enum.c:2960) with break-on-hit.
    def "include?" arity 1 | "member?" arity 1 (recv, args, _block) {
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
    // count: 0-arg counts yields; block form counts truthy raw-yield results;
    // 1-arg counts `==` matches (arg + block: the arg wins, block ignored --
    // CRuby warns "given block not used", enum.c:320). Always iterates
    // (Enumerable#count has no size fast path -- enum.c:302-328).
    def "count"(recv, args, block) {
        let n = Arc::new(Mutex::new(0i64));
        let n2 = n.clone();
        let brk: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
        match (args.len(), block) {
            (0, None) => for_each(recv, move |_| {
                *n2.lock() += 1;
                Ok(RubyValue::Nil)
            })?,
            (0, Some(RubyValue::Proc(blk))) => {
                let brk2 = brk.clone();
                for_each(recv, move |yielded| {
                    if yield_block(&blk, yielded, &brk2)?.truthy() {
                        *n2.lock() += 1;
                    }
                    Ok(RubyValue::Nil)
                })?
            }
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
        if let Some(v) = user_break(&brk) {
            return Ok(v);
        }
        let result = *n.lock();
        Ok(RubyValue::Int(result))
    }
    def "any?"(recv, args, block) {
        any_all(recv, args, block, Quantifier::Any)
    }
    def "all?"(recv, args, block) {
        any_all(recv, args, block, Quantifier::All)
    }
    def "none?"(recv, args, block) {
        any_all(recv, args, block, Quantifier::None)
    }
    def "one?"(recv, args, block) {
        any_all(recv, args, block, Quantifier::One)
    }
    // find/detect: first PACKED element whose raw-yield block result is truthy.
    // The optional `ifnone` callable argument is invoked (with no arguments) only
    // when NO element matches, and its result becomes the answer; a match --
    // including a `nil` element -- ignores it. With no `ifnone` and no match, nil.
    def "find" | "detect"(recv, args, block) {
        let ifnone = args.first().cloned();
        let blk = block_or_enum!(recv, "find", &[], block);
        let hit: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
        let hit2 = hit.clone();
        let brk: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
        let brk2 = brk.clone();
        for_each(recv, move |yielded| {
            if yield_block(&blk, yielded, &brk2)?.truthy() {
                *hit2.lock() = Some(pack(yielded));
                return Err(Signal::Break(RubyValue::Nil));
            }
            Ok(RubyValue::Nil)
        })?;
        if let Some(v) = user_break(&brk) {
            return Ok(v);
        }
        let found = hit.lock().take();
        match found {
            Some(v) => Ok(v),
            None => match ifnone {
                Some(p) => p.as_proc_unchecked().call(&[]),
                None => Ok(RubyValue::Nil),
            },
        }
    }
    // first / first(n): break-on-first(-nth) yield; `first(0)` returns `[]`
    // WITHOUT calling `each` at all (enum.c:3585); `first` on empty -> nil.
    def "first"(recv, args, _block) {
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
                let n = crate::builtins::convert::to_index(&args[0])?;
                if n < 0 {
                    // Generic Enumerable#first(n<0) -- an Enumerator (`cycle.first`),
                    // Set, etc. Array and Range override with their own messages
                    // before routing here.
                    return Err(arg_error!("attempt to take negative size"));
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
    // reduce/inject, all three CRuby forms (enum.c:1046-1096): `{block}`,
    // `(init){block}`, and the operator forms `(:sym)` / `(init, :sym)`
    // (each step `acc = acc.send(op, elem)` -- dispatched through
    // `send_value`, so user-defined operator methods compose). No-init: the
    // first element seeds the accumulator WITHOUT invoking the block; empty
    // with no init -> nil. The block is always called with exactly
    // `(acc, packed_element)`.
    def "reduce" | "inject"(recv, args, block) {
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
                _ => panic!(
                    "Enumerable#reduce: a single non-Symbol argument needs a block (the argument is the initial value)"
                ),
            },
            (2, _) => match &args[1] {
                RubyValue::Symbol(op) => (Some(args[0].clone()), Step::Op(*op)),
                _ => panic!("Enumerable#reduce: the second argument must be an operator Symbol"),
            },
            _ => panic!("Enumerable#reduce needs a block or an operator Symbol"),
        };
        let acc: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(init));
        let acc2 = acc.clone();
        let brk: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
        let brk2 = brk.clone();
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
                        // A user `break` in the block propagates as the reduce
                        // value (`yield_block`); an operator Symbol step can't break.
                        Step::Block(b) => yield_block(b, &[current, elem], &brk2)?,
                        Step::Op(op) => send_value(&current, *op, &[elem], None)?,
                    };
                    *acc2.lock() = Some(next);
                    return Ok(RubyValue::Nil);
                }
            };
            *a = Some(next);
            Ok(RubyValue::Nil)
        })?;
        if let Some(v) = user_break(&brk) {
            return Ok(v);
        }
        let result = acc.lock().take().unwrap_or(RubyValue::Nil);
        Ok(result)
    }
    // each_with_index: yields `(packed_element, index)` as TWO args
    // (enum.c:2993-3000) and returns the receiver itself, not an array.
    def "each_with_index"(recv, args, block) {
        reject_args(
            args,
            "each_with_index",
            "arguments (forwarding them to #each)",
        );
        let blk = block_or_enum!(recv, "each_with_index", args, block);
        let idx = Arc::new(Mutex::new(0i64));
        let idx2 = idx.clone();
        let brk: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
        let brk2 = brk.clone();
        for_each(recv, move |yielded| {
            let i = {
                let mut n = idx2.lock();
                let i = *n;
                *n += 1;
                i
            };
            yield_block(&blk, &[pack(yielded), RubyValue::Int(i)], &brk2)?;
            Ok(RubyValue::Nil)
        })?;
        if let Some(v) = user_break(&brk) {
            return Ok(v);
        }
        Ok(recv.clone())
    }
    // sum(init = 0) { |elem| ... } -- CRuby's numeric ladder minus the
    // Rational leg: pure-Int accumulation, switching to Kahan-Babuska
    // compensated f64 summation on the first Float (enum.c:4659-4710's
    // algorithm), and to generic `+` dispatch on the first non-numeric
    // (sticking there). The optional block is applied to the PACKED element
    // (single argument -- `rb_yield(i)`, enum.c:4712-4746).
    def "sum"(recv, args, block) {
        let init = match args.len() {
            0 => RubyValue::Int(0),
            1 => args[0].clone(),
            _ => panic!("Enumerable#sum takes at most one argument"),
        };
        let acc = Arc::new(Mutex::new(Some(SumAcc::new(init))));
        let blk = match block {
            Some(RubyValue::Proc(p)) => Some(p),
            _ => None,
        };
        let acc2 = acc.clone();
        for_each(recv, move |yielded| {
            let mut elem = pack(yielded);
            if let Some(b) = &blk {
                elem = b.call(&[elem])?;
            }
            let current = acc2.lock().take().expect("accumulator always present");
            let next = current.add(elem)?;
            *acc2.lock() = Some(next);
            Ok(RubyValue::Nil)
        })?;
        let result = acc
            .lock()
            .take()
            .expect("accumulator always present")
            .finish();
        Ok(result)
    }
    def "min"(recv, args, block) {
        min_max(recv, args, block, true)
    }
    def "max"(recv, args, block) {
        min_max(recv, args, block, false)
    }
    def "sort" arity 0 (recv, args, block) {
        reject_args(args, "sort", "arguments");
        let mut items = collect_packed(recv)?;
        crate::builtins::array::sort_items(&mut items, &block)?;
        Ok(RubyValue::Array(array_new(items)))
    }
    def "sort_by" arity 0 (recv, args, block) {
        reject_args(args, "sort_by", "arguments");
        let blk = block_or_enum!(recv, "sort_by", args, block);
        let items = collect_elements(recv)?;
        // Decorate-sort-undecorate, keys ordered by rb_cmp.
        let mut decorated: Vec<(RubyValue, RubyValue)> = Vec::with_capacity(items.len());
        for e in items {
            let key = blk.call(&e.raw)?;
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
            return Err(arg_error!("comparison failed"));
        }
        Ok(RubyValue::Array(array_new(
            decorated.into_iter().map(|(_, e)| e).collect(),
        )))
    }
    def "min_by"(recv, args, block) {
        min_max_by(recv, args, block, true)
    }
    def "max_by"(recv, args, block) {
        min_max_by(recv, args, block, false)
    }
    def "minmax" arity 0 (recv, args, block) {
        reject_args(args, "minmax", "arguments");
        let lo = min_max(recv, &[], block.clone(), true)?;
        let hi = min_max(recv, &[], block, false)?;
        Ok(RubyValue::Array(array_new(vec![lo, hi])))
    }
    def "group_by" arity 0 (recv, args, block) {
        reject_args(args, "group_by", "arguments");
        let blk = block_or_enum!(recv, "group_by", args, block);
        let items = collect_elements(recv)?;
        let groups = crate::hash_new(Vec::new());
        for e in items {
            let key = blk.call(&e.raw)?;
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
    def "partition" arity 0 (recv, args, block) {
        reject_args(args, "partition", "arguments");
        let blk = block_or_enum!(recv, "partition", args, block);
        let items = collect_elements(recv)?;
        let (mut yes, mut no) = (Vec::new(), Vec::new());
        for e in items {
            if blk.call(&e.raw)?.truthy() {
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
    def "flat_map" arity 0 | "collect_concat" arity 0 (recv, args, block) {
        reject_args(args, "flat_map", "arguments");
        let blk = block_or_enum!(recv, "flat_map", args, block);
        let items = collect_elements(recv)?;
        let mut out = Vec::new();
        for e in items {
            match blk.call(&e.raw)? {
                // ONE level of flattening (real Ruby's rule).
                RubyValue::Array(a) => out.extend(a.lock().iter().cloned()),
                other => out.push(other),
            }
        }
        Ok(RubyValue::Array(array_new(out)))
    }
    def "filter_map" arity 0 (recv, args, block) {
        reject_args(args, "filter_map", "arguments");
        let blk = block_or_enum!(recv, "filter_map", args, block);
        let items = collect_elements(recv)?;
        let mut out = Vec::new();
        for e in items {
            let mapped = blk.call(&e.raw)?;
            if mapped.truthy() {
                out.push(mapped);
            }
        }
        Ok(RubyValue::Array(array_new(out)))
    }
    def "each_slice" arity 1 (recv, args, block) {
        let n = slice_size(args, "each_slice")?;
        let blk = block_or_enum!(recv, "each_slice", args, block);
        let items = collect_packed(recv)?;
        for chunk in items.chunks(n) {
            blk.call(&[RubyValue::Array(array_new(chunk.to_vec()))])?;
        }
        // The block form answers the receiver (Ruby 3.1+), not nil.
        Ok(recv.clone())
    }
    def "each_cons" arity 1 (recv, args, block) {
        let n = slice_size(args, "each_cons")?;
        let blk = block_or_enum!(recv, "each_cons", args, block);
        let items = collect_packed(recv)?;
        if items.len() >= n {
            for window in items.windows(n) {
                blk.call(&[RubyValue::Array(array_new(window.to_vec()))])?;
            }
        }
        // The block form answers the receiver (Ruby 3.1+), not nil.
        Ok(recv.clone())
    }
    def "each_with_object" arity 1 (recv, args, block) {
        if args.len() != 1 {
            panic!("Enumerable#each_with_object takes exactly one argument");
        }
        let blk = block_or_enum!(recv, "each_with_object", args, block);
        let memo = args[0].clone();
        // Drive the receiver's own `each` (rather than collect-then-iterate) so
        // the block runs INSIDE that `each` frame: a block-raised exception then
        // shows both the receiver's `each` C-frame and this
        // 'Enumerable#each_with_object' one, exactly as CRuby's backtrace does.
        let _frame = crate::frames::synthetic_c_frame("Enumerable#each_with_object");
        let memo2 = memo.clone();
        let brk: Arc<Mutex<Option<RubyValue>>> = Arc::new(Mutex::new(None));
        let brk2 = brk.clone();
        for_each(recv, move |yielded| {
            yield_block(&blk, &[pack(yielded), memo2.clone()], &brk2)?;
            Ok(RubyValue::Nil)
        })?;
        if let Some(v) = user_break(&brk) {
            return Ok(v);
        }
        Ok(memo)
    }
    def "take" arity 1 (recv, args, _block) {
        take_drop(recv, args, true)
    }
    def "drop" arity 1 (recv, args, _block) {
        take_drop(recv, args, false)
    }
    def "take_while" arity 0 (recv, args, block) {
        take_drop_while(recv, args, block, true)
    }
    def "drop_while" arity 0 (recv, args, block) {
        take_drop_while(recv, args, block, false)
    }
    // `find_index(value)` / `find_index { |e| ... }`.
    def "find_index"(recv, args, block) {
        let items = collect_elements(recv)?;
        if let Some(RubyValue::Proc(p)) = &block {
            for (i, e) in items.iter().enumerate() {
                if p.call(&e.raw)?.truthy() {
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
    def "tally"(recv, args, _block) {
        // Optional accumulator hash: counts add onto its existing values and the
        // same hash is returned (Enumerable#tally(hash)). No arg -> a fresh
        // hash.
        let counts = match args.first() {
            None => crate::hash_new(Vec::new()),
            Some(v) => crate::builtins::convert::to_rhash(v)?,
        };
        let items = collect_packed(recv)?;
        for e in items {
            let n = match crate::hash_get(&counts, &e) {
                RubyValue::Int(n) => n + 1,
                _ => 1,
            };
            crate::hash_set(&counts, e, RubyValue::Int(n));
        }
        Ok(RubyValue::Hash(counts))
    }
    def "uniq" arity 0 (recv, args, _block) {
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
    def "to_h"(recv, args, block) {
        reject_args(args, "to_h", "arguments");
        let items = collect_elements(recv)?;
        let pairs = to_h_pairs(items.iter().map(|e| (e.raw.as_slice(), &e.packed)), &block)?;
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    def "reverse_each"(recv, args, block) {
        reject_args(args, "reverse_each", "arguments");
        let blk = block_or_enum!(recv, "reverse_each", args, block);
        let items = collect_elements(recv)?;
        for e in items.iter().rev() {
            blk.call(&e.raw)?;
        }
        Ok(recv.clone())
    }
    def "grep" arity 1 (recv, args, block) {
        grep(recv, args, block, true)
    }
    def "grep_v" arity 1 (recv, args, block) {
        grep(recv, args, block, false)
    }
    def "chunk_while" arity 0 (recv, args, block) {
        chunk_while(recv, args, block, false)
    }
    def "slice_when" arity 0 (recv, args, block) {
        chunk_while(recv, args, block, true)
    }
    def "slice_before"(recv, args, block) {
        slice_before_after(recv, args, block, true)
    }
    def "slice_after"(recv, args, block) {
        slice_before_after(recv, args, block, false)
    }
    // `minmax_by { |e| ... }` -- `[min_by, max_by]`, computed in ONE pass so
    // the block runs once per element, as CRuby's does. `[nil, nil]` for an
    // empty receiver (not `[]`).
    def "minmax_by" arity 0 (recv, args, block) {
        reject_args(args, "minmax_by", "arguments");
        let blk = block_or_enum!(recv, "minmax_by", args, block);
        let mut lo: Option<(RubyValue, RubyValue)> = None;
        let mut hi: Option<(RubyValue, RubyValue)> = None;
        for e in collect_packed(recv)? {
            let k = blk.call(std::slice::from_ref(&e))?;
            if lo
                .as_ref()
                .is_none_or(|(bk, _)| k.rb_cmp(bk).is_some_and(|o| o < 0))
            {
                lo = Some((k.clone(), e.clone()));
            }
            if hi
                .as_ref()
                .is_none_or(|(bk, _)| k.rb_cmp(bk).is_some_and(|o| o > 0))
            {
                hi = Some((k, e));
            }
        }
        let pick = |o: Option<(RubyValue, RubyValue)>| o.map_or(RubyValue::Nil, |(_, v)| v);
        Ok(RubyValue::Array(array_new(vec![pick(lo), pick(hi)])))
    }
    // `each_entry` -- like `each`, but yields the PACKED element where `each`
    // passes the raw values through. That is the entire difference, and it is
    // only observable when the receiver's `each` yields MORE THAN ONE value
    // (oracle-verified against a class whose `each` does `yield 1; yield 2, 3;
    // yield`):
    //
    //   each_entry { |x| }  sees  1, [2, 3], nil
    //   each       { |x| }  sees  1,  2,     nil
    //
    // Answers the receiver.
    def "each_entry"(recv, args, block) {
        reject_args(args, "each_entry", "arguments");
        let blk = block_or_enum!(recv, "each_entry", args, block);
        for e in collect_packed(recv)? {
            blk.call(&[e])?;
        }
        Ok(recv.clone())
    }
    // `chunk { |x| key }` -- groups CONSECUTIVE elements sharing a `==`-equal
    // block key into `[key, [elements...]]` pairs. Like the other slicing
    // methods here, the result materializes as an Array (responding to the
    // Array/Enumerable surface a real Enumerator would).
    def "chunk" arity 0 (recv, args, block) {
        reject_args(args, "chunk", "arguments");
        let blk = block_or_enum!(recv, "chunk", args, block);
        let items = collect_packed(recv)?;
        let mut out: Vec<RubyValue> = Vec::new();
        let mut cur_key: Option<RubyValue> = None;
        let mut cur: Vec<RubyValue> = Vec::new();
        let flush = |key: Option<RubyValue>, cur: &mut Vec<RubyValue>, out: &mut Vec<RubyValue>| {
            if let Some(k) = key {
                out.push(RubyValue::Array(array_new(vec![
                    k,
                    RubyValue::Array(array_new(std::mem::take(cur))),
                ])));
            }
        };
        for e in items {
            let key = blk.call(std::slice::from_ref(&e))?;
            let sym = matches!(&key, RubyValue::Symbol(s) if matches!(s.name().as_str(), "_separator" | "_alone"));
            // `nil`/`:_separator` DROP the element and end the current run; the
            // next element starts a fresh chunk (CRuby's chunk semantics).
            if matches!(&key, RubyValue::Nil)
                || matches!(&key, RubyValue::Symbol(s) if s.name() == "_separator")
            {
                flush(cur_key.take(), &mut cur, &mut out);
                continue;
            }
            // `:_alone` never merges, even with an adjacent `:_alone`.
            let alone = sym && matches!(&key, RubyValue::Symbol(s) if s.name() == "_alone");
            let same = !alone && cur_key.as_ref().is_some_and(|k| k.rb_eq(&key));
            if !same {
                flush(cur_key.take(), &mut cur, &mut out);
                cur_key = Some(key);
            }
            cur.push(e);
        }
        flush(cur_key.take(), &mut cur, &mut out);
        Ok(RubyValue::Array(array_new(out)))
    }
    // `zip(*others)` -- pairs each element of the receiver with the same-index
    // element of every `other` (nil past an `other`'s end), returning the Array
    // of tuples, or yielding each tuple to a block and answering nil.
    def "zip"(recv, args, block) {
        let RubyValue::Array(base) = collect_to_a(recv)? else {
            unreachable!("to_a always answers an Array")
        };
        let base = base.lock().clone();
        let mut others: Vec<Vec<RubyValue>> = Vec::with_capacity(args.len());
        for other in args {
            let arr = match other {
                RubyValue::Array(a) => a.lock().to_vec(),
                _ => {
                    let v = send_value(other, crate::Symbol::intern("to_a"), &[], None)?;
                    match v {
                        RubyValue::Array(a) => a.lock().to_vec(),
                        _ => Vec::new(),
                    }
                }
            };
            others.push(arr);
        }
        let tuples: Vec<RubyValue> = base
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut tuple = Vec::with_capacity(1 + others.len());
                tuple.push(e.clone());
                for o in &others {
                    tuple.push(o.get(i).cloned().unwrap_or(RubyValue::Nil));
                }
                RubyValue::Array(array_new(tuple))
            })
            .collect();
        if let Some(RubyValue::Proc(p)) = &block {
            for t in tuples {
                p.call(&[t])?;
            }
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(array_new(tuples)))
    }
    // `compact` -- the receiver's elements as an Array with every `nil` dropped.
    def "compact" arity 0 (recv, args, _block) {
        reject_args(args, "compact", "arguments");
        let RubyValue::Array(all) = collect_to_a(recv)? else {
            unreachable!("to_a always answers an Array")
        };
        let kept: Vec<RubyValue> = all.lock().iter().filter(|e| !e.is_nil()).cloned().collect();
        Ok(RubyValue::Array(array_new(kept)))
    }
    // `cycle([n]) { ... }` -- yields every element `n` times (forever when `n`
    // is omitted); a blockless call answers an Enumerator. An empty receiver
    // (or `n <= 0`) yields nothing and returns nil.
    def "cycle"(recv, args, block) {
        let times = match args.first() {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(crate::builtins::convert::to_index(v)?),
        };
        let p = block_or_enum!(recv, "cycle", args, block);
        let RubyValue::Array(all) = collect_to_a(recv)? else {
            unreachable!("to_a always answers an Array")
        };
        let items = all.lock().clone();
        if items.is_empty() {
            return Ok(RubyValue::Nil);
        }
        match times {
            Some(n) => {
                for _ in 0..n.max(0) {
                    for e in &items {
                        p.call(std::slice::from_ref(e))?;
                    }
                }
            }
            None => loop {
                for e in &items {
                    p.call(std::slice::from_ref(e))?;
                }
            },
        }
        Ok(RubyValue::Nil)
    }
    // `chain(*others)` -- an Enumerator over this collection's elements followed
    // by each `other`'s (materialized eagerly; the enumerator drives `each`).
    def "chain"(recv, args, _block) {
        let mut sources = Vec::with_capacity(args.len() + 1);
        sources.push(recv.clone());
        sources.extend(args.iter().cloned());
        Ok(crate::builtins::enumerator::chain_of(sources))
    }
    // `to_set` -- a `Set` of the receiver's elements (deduplicated on insert).
    def "to_set"(recv, args, _block) {
        reject_args(args, "to_set", "arguments");
        let RubyValue::Array(all) = collect_to_a(recv)? else {
            unreachable!("to_a always answers an Array")
        };
        let items = all.lock().clone();
        Ok(crate::builtins::set::set_from(items))
    }
    def "lazy" arity 0 (recv, _args, _block) {
        Ok(crate::builtins::lazy::make_lazy(recv))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::array_new;

    fn ints(vals: &[i64]) -> RubyValue {
        RubyValue::Array(array_new(vals.iter().map(|v| RubyValue::Int(*v)).collect()))
    }

    /// Drive a lazy Enumerator (e.g. `chunk_while`'s Generator) to its Array and
    /// return that Array's `#inspect`.
    fn drive(e: RubyValue) -> String {
        crate::dispatch::send_value(&e, crate::Symbol::intern("to_a"), &[], None)
            .unwrap()
            .inspect_string()
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
        let min = enumerable_send(&ints(&[5, 1, 4]), "min", &[], None)
            .unwrap()
            .unwrap();
        assert_eq!(min.inspect_string(), "1");
        let max = enumerable_send(&ints(&[5, 1, 4]), "max", &[], None)
            .unwrap()
            .unwrap();
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
        let out = enumerable_send(
            &ints(&[5, 1, 4]),
            "min",
            &[RubyValue::Int(2)],
            Some(RubyValue::Proc(cmp)),
        )
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
            let RubyValue::Int(i) = &args[0] else {
                panic!("ints only")
            };
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

    /// `chunk_while` and `slice_when` are exact negations -- the same cut
    /// points, chosen on opposite truth values. Both return a lazy Enumerator
    /// (driven here with `to_a`). Oracle-verified:
    ///   [1,2,4,9,10,11,12,15].slice_when  { |i,j| i+1 != j }.to_a
    ///   [1,2,4,9,10,11,12,15].chunk_while { |i,j| i+1 == j }.to_a
    /// both => [[1,2],[4],[9,10,11,12],[15]]
    #[test]
    fn chunk_while_and_slice_when_are_negations_of_each_other() {
        let a = ints(&[1, 2, 4, 9, 10, 11, 12, 15]);
        let adjacent = RProc::new(|args: &[RubyValue]| {
            let (RubyValue::Int(i), RubyValue::Int(j)) = (&args[0], &args[1]) else {
                unreachable!()
            };
            Ok(RubyValue::Bool(i + 1 == *j))
        });
        let gap = RProc::new(|args: &[RubyValue]| {
            let (RubyValue::Int(i), RubyValue::Int(j)) = (&args[0], &args[1]) else {
                unreachable!()
            };
            Ok(RubyValue::Bool(i + 1 != *j))
        });
        let want = "[[1, 2], [4], [9, 10, 11, 12], [15]]";
        let c = enumerable_send(&a, "chunk_while", &[], Some(RubyValue::Proc(adjacent)))
            .unwrap()
            .unwrap();
        assert_eq!(drive(c), want);
        let s = enumerable_send(&a, "slice_when", &[], Some(RubyValue::Proc(gap)))
            .unwrap()
            .unwrap();
        assert_eq!(drive(s), want);
    }

    /// Neither an empty nor a one-element receiver ever runs the block
    /// (there is no adjacent pair to test); the resulting Enumerator drives to
    /// `[]` and `[[7]]`.
    #[test]
    fn chunk_while_on_short_receivers_never_calls_the_block() {
        let never = || RProc::new(|_: &[RubyValue]| unreachable!("no adjacent pair exists"));
        let e = enumerable_send(
            &ints(&[]),
            "chunk_while",
            &[],
            Some(RubyValue::Proc(never())),
        )
        .unwrap()
        .unwrap();
        assert_eq!(drive(e), "[]");
        let one = enumerable_send(
            &ints(&[7]),
            "chunk_while",
            &[],
            Some(RubyValue::Proc(never())),
        )
        .unwrap()
        .unwrap();
        assert_eq!(drive(one), "[[7]]");
    }

    /// A `slice_before` match at the very START opens the first slice
    /// rather than closing an empty one -- `[[1,2,3]]`, not `[[], [1,2,3]]`.
    #[test]
    fn slice_before_does_not_emit_a_leading_empty_slice() {
        let is_one = RProc::new(|args: &[RubyValue]| {
            Ok(RubyValue::Bool(matches!(args[0], RubyValue::Int(1))))
        });
        let out = enumerable_send(
            &ints(&[1, 2, 3]),
            "slice_before",
            &[],
            Some(RubyValue::Proc(is_one)),
        )
        .unwrap()
        .unwrap();
        assert_eq!(out.inspect_string(), "[[1, 2, 3]]");
    }

    /// `minmax_by` answers `[nil, nil]` for an empty receiver -- not `[]`.
    #[test]
    fn minmax_by_on_an_empty_receiver_is_a_nil_pair() {
        let id = RProc::new(|args: &[RubyValue]| Ok(args[0].clone()));
        let out = enumerable_send(&ints(&[]), "minmax_by", &[], Some(RubyValue::Proc(id)))
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "[nil, nil]");
    }

    /// `grep` selects by `===`, so a Range argument covers.
    #[test]
    fn grep_selects_by_case_equality() {
        let out = enumerable_send(
            &ints(&[1, 2, 3, 4, 5]),
            "grep",
            &[RubyValue::Range(
                Some(Box::new(RubyValue::Int(2))),
                Some(Box::new(RubyValue::Int(4))),
                false,
            )],
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(out.inspect_string(), "[2, 3, 4]");
    }
}
