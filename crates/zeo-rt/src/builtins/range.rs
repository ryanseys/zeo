//! `Range` (CRuby range.c) -- stage B: the `each` primitive Enumerable
//! drives (migrated), plus `===`/`cover?`/`include?`-family over the shared
//! `range_covers` (the `Range#===` fix that makes `case x when 1..5` real).
//! The remaining Tier A rows land in stage E.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::builtins::enumerable::{self, own_row};
use crate::builtins::{arg_error, block_or_enum, inherited_row, range_error, type_error};
use crate::{RProc, RubyValue, Signal};
use zeo_macros::ruby_class;

/// `a..b` / `a...b`, as one shared payload. Either endpoint may be absent --
/// a beginless or endless range -- and `nil` is normalized to absent at
/// construction (`nil..5` IS `..5`; keeping the two apart once made
/// `(1..nil).min` hang).
///
/// A Ruby Range is immutable: no method mutates one in place, and the
/// permanently-frozen tier (`value_ivars`) denies it ivars. So the `Arc` here
/// buys sharing without any of the interior mutability `Array`/`Hash`/`Str`
/// need -- a `RubyValue::clone` bumps one refcount instead of deep-copying
/// both endpoints, and the pointer gives `Range` the object identity CRuby
/// has and zeo's two `Box`es could not: `equal?`, `object_id` and the
/// identity hash key all read it.
pub struct RangeData {
    pub start: Option<RubyValue>,
    pub end: Option<RubyValue>,
    pub exclusive: bool,
}

pub type RRange = Arc<RangeData>;

impl RangeData {
    /// The three fields as the borrow shape almost every caller wants.
    #[inline]
    pub fn parts(&self) -> (Option<&RubyValue>, Option<&RubyValue>, bool) {
        (self.start.as_ref(), self.end.as_ref(), self.exclusive)
    }
}

/// The one Range constructor. Every caller goes through it, so the
/// nil-normalization above holds everywhere.
pub fn range_new(start: Option<RubyValue>, end: Option<RubyValue>, exclusive: bool) -> RRange {
    let strip = |v: Option<RubyValue>| match v {
        Some(RubyValue::Nil) | None => None,
        other => other,
    };
    Arc::new(RangeData {
        start: strip(start),
        end: strip(end),
        exclusive,
    })
}

/// `range_new` as a `RubyValue` -- what codegen emits for a range literal.
pub fn range_value(start: Option<RubyValue>, end: Option<RubyValue>, exclusive: bool) -> RubyValue {
    RubyValue::Range(range_new(start, end, exclusive))
}

/// The values a BOUNDED range covers, walked by the builtin `each` fetched
/// straight from Range's own table -- never through dispatch, so a runtime
/// `Range#each` override cannot reach a row ruby owns on Range (`#count`,
/// `#minmax`, `#reverse_each`, `#to_set`). CRuby's C bodies read the endpoints
/// or walk integers directly and never call `each` either.
///
/// `None` for an endless or beginless range: it has no finite storage, so its
/// rows keep sending `each` exactly as before.
pub(crate) fn finite_values(recv: &RubyValue) -> Option<Vec<RubyValue>> {
    let (start, end, _) = range_parts(recv);
    start?;
    match end {
        None => return None,
        Some(RubyValue::Float(f)) if f.is_infinite() && *f > 0.0 => return None,
        Some(_) => {}
    }
    let each = crate::builtins::class_table(zeo_abi::RANGE_CLASS)?("each")?;
    let out: Arc<Mutex<Vec<RubyValue>>> = Arc::new(Mutex::new(Vec::new()));
    let out2 = out.clone();
    let collect = RProc::new(move |yielded: &[RubyValue]| {
        out2.lock().push(crate::builtins::enumerable::pack(yielded));
        Ok(RubyValue::Nil)
    });
    each(recv, &[], Some(RubyValue::Proc(collect))).ok()?;
    let items = std::mem::take(&mut *out.lock());
    Some(items)
}

fn range_parts(recv: &RubyValue) -> (Option<&RubyValue>, Option<&RubyValue>, bool) {
    match recv {
        RubyValue::Range(r) => (r.start.as_ref(), r.end.as_ref(), r.exclusive),
        _ => unreachable!("Range table row dispatched on a non-Range receiver"),
    }
}

/// An endpoint that is absent -- `(1..)`, `(..5)`, and the `nil` spelling
/// `range_endpoint` normalizes to the same thing.
fn open_endpoint(v: Option<&RubyValue>) -> bool {
    matches!(v, None | Some(RubyValue::Nil))
}

/// CRuby's `linear_object_p` (range.c): a value ordered densely enough that
/// comparing the endpoints ANSWERS membership, so `include?` may take
/// `cover?`'s shortcut. Every Numeric and every Time; nothing else.
fn linear_endpoint(v: Option<&RubyValue>) -> bool {
    match v {
        None | Some(RubyValue::Nil) => false,
        Some(v) => {
            let cid = v.class_id();
            cid == zeo_abi::TIME_CLASS || crate::dispatch::is_a(cid, zeo_abi::NUMERIC_CLASS)
        }
    }
}

/// CRuby's `range_integer_edge_p`: either endpoint converts with `to_int`,
/// which makes the range integer-shaped even where neither end IS a Numeric.
fn integer_edge(start: Option<&RubyValue>, end: Option<&RubyValue>) -> bool {
    let convertible = |v: Option<&RubyValue>| match v {
        None | Some(RubyValue::Nil) => false,
        Some(v) => matches!(crate::builtins::convert::check_to_int(v), Ok(Some(_))),
    };
    convertible(start) || convertible(end)
}

/// `Range#step` and `Range#%`, which differ only in the name the
/// `Enumerator::ArithmeticSequence` they answer prints back
/// (`((1..10).step(2))` against `((1..10).%(2))`).
///
/// A numeric range walks through the ONE shared arithmetic-sequence walk in
/// `numeric.rs`, so a `.step(n).to_a` can never disagree with the block form
/// that built it -- and so an endless or Rational range walks at all.
fn range_step(
    recv: &RubyValue,
    meth: &'static str,
    args: &[RubyValue],
    n: &RubyValue,
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    use crate::builtins::numeric::{num_cmp, step_walk};
    let (start, end, exclusive) = range_parts(recv);
    let numeric = |v: Option<&RubyValue>| {
        matches!(
            v,
            Some(
                RubyValue::Int(_)
                    | RubyValue::BigInt(_)
                    | RubyValue::Float(_)
                    | RubyValue::Rational(_)
            )
        )
    };
    if !numeric(start) && !numeric(end) {
        // Any other element type walks by `succ`, and its blockless form is
        // an ordinary Enumerator rather than a sequence.
        block_or_enum!(recv, meth, args, block);
        panic!("Range#step on a non-numeric range isn't supported (zeo limitation)");
    }
    // A non-numeric step is not refused until the walk actually needs it:
    // `(1..5).step("x")` blocklessly answers an ordinary Enumerator, and
    // only iterating it raises. NOT an implicit-conversion site either --
    // CRuby raises the numeric-tower coerce shape ("String can't be coerced
    // into Integer").
    if !numeric(Some(n)) {
        block_or_enum!(recv, meth, args, block);
        return Err(type_error!(
            "{} can't be coerced into Integer",
            crate::builtins::coerce_operand_name(n)
        ));
    }
    if matches!(num_cmp(n, &RubyValue::Int(0)), Some(Some(0))) {
        return Err(arg_error!("step can't be 0"));
    }
    let Some(RubyValue::Proc(p)) = block else {
        return Ok(crate::builtins::enumerator::arith_seq_of(
            recv,
            meth,
            args,
            start.cloned().unwrap_or(RubyValue::Nil),
            end.cloned().unwrap_or(RubyValue::Nil),
            n.clone(),
            exclusive,
        ));
    };
    let nil = RubyValue::Nil;
    step_walk(start.unwrap_or(&nil), end, n, exclusive, |v| {
        p.call(std::slice::from_ref(v))?;
        Ok(())
    })?;
    Ok(recv.clone())
}

/// `Range#cover?(other_range)` -- true iff every element of `other` lies within
/// `self`: `other`'s begin is at/after self's, and its end at/before self's
/// (honoring exclusive ends). A missing bound on `self` covers that side; a
/// bound `self` has that `other` lacks (endless/beginless `other`) is not
/// covered.
fn range_covers_range(
    s_start: Option<&RubyValue>,
    s_end: Option<&RubyValue>,
    s_excl: bool,
    other: &RubyValue,
) -> bool {
    let (o_start, o_end, o_excl) = range_parts(other);
    // Begin side: self.begin <= other.begin.
    match (s_start, o_start) {
        (Some(ss), Some(os)) => {
            if !ss.rb_cmp(os).is_some_and(|c| c <= 0) {
                return false;
            }
        }
        (Some(_), None) => return false,
        _ => {}
    }
    // End side: self.end >= other.end, with an EQUAL end covered unless self
    // excludes it while other includes it.
    match (s_end, o_end) {
        (Some(se), Some(oe)) => match se.rb_cmp(oe) {
            Some(c) if c > 0 => {}
            Some(0) if !s_excl || o_excl => {}
            _ => return false,
        },
        (Some(_), None) => return false,
        _ => {}
    }
    true
}

/// `Range#bsearch` over a FLOAT range. Bisects on the doubles' monotonic
/// integer image (a positive-float's bits are already monotonic; the sign flip
/// extends that to the whole line), so a representable boundary is found
/// exactly. Both CRuby modes are supported: find-minimum for a boolean/nil
/// block result, find-any for a Numeric comparator result.
fn range_bsearch_float(
    start: Option<&RubyValue>,
    end: Option<&RubyValue>,
    exclusive: bool,
    p: &crate::RProc,
) -> Result<RubyValue, crate::Signal> {
    let to_f = |v: Option<&RubyValue>| match v {
        Some(RubyValue::Int(n)) => Some(*n as f64),
        Some(RubyValue::Float(f)) => Some(*f),
        _ => None,
    };
    let (Some(lo_f), Some(hi_f)) = (to_f(start), to_f(end)) else {
        return Err(type_error!("can't do binary search for the given Range"));
    };
    // Map a double to a u64 that is monotonically increasing in its value.
    let f2u = |f: f64| -> u64 {
        let b = f.to_bits();
        if b >> 63 == 1 { !b } else { b | (1 << 63) }
    };
    let u2f = |u: u64| -> f64 {
        let b = if u >> 63 == 1 { u & !(1 << 63) } else { !u };
        f64::from_bits(b)
    };
    let mut lo = f2u(lo_f);
    // Inclusive end includes `hi_f`, so search up to the next representable u.
    let mut hi = f2u(hi_f) + u64::from(!exclusive);
    let mut satisfied: Option<f64> = None;
    let mut numeric_mode = false;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        // CRuby's float bsearch collapses the two signed zeros (its
        // `double_as_int64` maps both to 0), so a boundary at zero is reported
        // as +0.0; `+ 0.0` normalizes -0.0 without disturbing any other value.
        let x = u2f(mid) + 0.0;
        let r = p.call(&[RubyValue::Float(x)])?;
        let cmp = match r {
            RubyValue::Int(n) => Some(n.cmp(&0)),
            RubyValue::Float(f) => f.partial_cmp(&0.0),
            _ => None,
        };
        match cmp {
            Some(std::cmp::Ordering::Equal) => return Ok(RubyValue::Float(x)),
            Some(std::cmp::Ordering::Less) => {
                numeric_mode = true;
                hi = mid;
            }
            Some(std::cmp::Ordering::Greater) => {
                numeric_mode = true;
                lo = mid + 1;
            }
            None if r.truthy() => {
                satisfied = Some(x);
                hi = mid;
            }
            None => lo = mid + 1,
        }
    }
    Ok(if numeric_mode {
        RubyValue::Nil
    } else {
        satisfied.map_or(RubyValue::Nil, RubyValue::Float)
    })
}

/// Whether integer `i` is still within an integer-start range whose end is
/// `end` -- unbounded for an endless (`nil`/`None`) or `+Float::INFINITY`
/// end, so an infinite range keeps yielding until its consumer stops pulling.
fn int_in_range(i: i64, end: Option<&RubyValue>, exclusive: bool) -> bool {
    match end {
        None | Some(RubyValue::Nil) => true,
        Some(RubyValue::Int(e)) => {
            if exclusive {
                i < *e
            } else {
                i <= *e
            }
        }
        Some(RubyValue::Float(f)) if f.is_infinite() => *f > 0.0,
        Some(RubyValue::Float(f)) => {
            if exclusive {
                (i as f64) < *f
            } else {
                (i as f64) <= *f
            }
        }
        // A canonical BigInt end is out of i64 range entirely: a positive one
        // admits every i64, a negative one admits none.
        Some(RubyValue::BigInt(e)) => e.sign() == num_bigint::Sign::Plus,
        _ => false,
    }
}

ruby_class! {
    Range = zeo_abi::RANGE_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // `Range.new(begin, end, exclude_end = false)` -- the literal `a..b` under
    // another name, so the endpoints normalize the same way and must be
    // comparable (`Range.new(1, "a")` is the ArgumentError a literal could
    // never reach). `Class#new` intercepts this for `Range` itself
    // (`builtins::rclass`); the row exists for the SUBCLASS path, where
    // `value_subclass::construct_root_payload` builds a payload by calling the
    // root's own `new` out of this table -- chronic's `Span < Range`.
    def self."new" allocs cfunc (_recv, *args, &_block) {
        crate::builtins::check_arity(args.len(), 2, Some(3))?;
        let excl = args.get(2).is_some_and(|v| v.truthy());
        crate::range_checked(
            crate::range_endpoint(args[0].clone()),
            crate::range_endpoint(args[1].clone()),
            excl,
        )
    }

    // Materializing an UNBOUNDED range would spin forever growing a vector
    // until the process died, so CRuby guards `Range#to_a` specifically
    // (`range_to_a`, range.c:1023) and delegates to Enumerable otherwise.
    // Registered for both spellings because they are separate entries there
    // (range.c:2986) -- an alias would silently skip the guard on one.
    //
    // The test is on the END only, matching CRuby: a BEGINLESS range isn't
    // caught here and instead fails in `each`, which cannot start.
    def "to_a" | "entries" (recv) {
        let (_, end, _) = range_parts(recv);
        let unbounded = match end {
            None => true,
            Some(RubyValue::Float(f)) => f.is_infinite() && *f > 0.0,
            Some(_) => false,
        };
        if unbounded {
            return Err(range_error!("cannot convert endless range to an array"));
        }
        crate::builtins::enumerable::enumerable_send(recv, "to_a", &[], None)
            .expect("Enumerable implements to_a")
    }

    def "each" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let (start, end, exclusive) = range_parts(recv);
        match start {
            // An integer start iterates integers upward. A finite Int/Float
            // end bounds it; an endless (`nil`) or `+Float::INFINITY` end
            // iterates forever, so a lazy pull or a block `break` is what
            // stops it (`(1..Float::INFINITY).lazy.first(3)`).
            Some(RubyValue::Int(s)) => {
                let mut i = *s;
                while int_in_range(i, end, exclusive) {
                    p.call(&[RubyValue::Int(i)])?;
                    i += 1;
                }
            }
            // String ranges iterate via `succ` until passing the end
            // (CRuby's rule, incl. the length guard: `"a".."e"` walks
            // b/c/d/e; a longer successor stops the walk).
            Some(RubyValue::Str(s)) if matches!(end, Some(RubyValue::Str(_))) => {
                let RubyValue::Str(e) = end.unwrap() else { unreachable!() };
                let end = e.lock().to_utf8_lossy().into_owned();
                let mut cur = s.lock().to_utf8_lossy().into_owned();
                loop {
                    if cur.len() > end.len() || (cur.len() == end.len() && cur > end) {
                        break;
                    }
                    if exclusive && cur == end {
                        break;
                    }
                    p.call(&[RubyValue::Str(crate::string_new(cur.clone()))])?;
                    if cur == end {
                        break;
                    }
                    cur = crate::builtins::string::succ_str(&cur);
                }
            }
            // Symbol ranges iterate by NAME succession (like String ranges),
            // yielding Symbols: `(:a..:e)` walks :a,:b,:c,:d,:e.
            Some(RubyValue::Symbol(s)) if matches!(end, Some(RubyValue::Symbol(_))) => {
                let RubyValue::Symbol(e) = end.unwrap() else { unreachable!() };
                let end = e.name();
                let mut cur = s.name();
                loop {
                    if cur.len() > end.len() || (cur.len() == end.len() && cur > end) {
                        break;
                    }
                    if exclusive && cur == end {
                        break;
                    }
                    p.call(&[RubyValue::Symbol(crate::Symbol::intern(&cur))])?;
                    if cur == end {
                        break;
                    }
                    cur = crate::builtins::string::succ_str(&cur);
                }
            }
            // A beginless range, or a non-iterable element type (Float, ...),
            // can't be walked forward -- CRuby names the begin's class:
            // `(1.0..2.0).each` is "can't iterate from Float".
            _ => {
                let ty = match start {
                    Some(v) => crate::builtins::class_name_of(v),
                    None => "NilClass".to_string(),
                };
                return Err(type_error!("can't iterate from {ty}"));
            }
        }
        Ok(recv.clone())
    }
    // `Range#bsearch` over an integer range, in both CRuby modes selected by
    // the block's return type (see `Array#bsearch`'s `bsearch_find`):
    // find-minimum for a boolean/nil result (first true, `nil` if none) and
    // find-any for a Numeric comparator result (`0` hits, negative searches
    // low, positive high; `nil` on no hit). Binary search on the bounds -- no
    // materialization, so a huge range is fine.
    def "bsearch" (recv, &block) {
        let p = crate::builtins::need_block!(block);
        let (start, end, exclusive) = range_parts(recv);
        // A float range bisects over the doubles' monotonic integer image
        // (CRuby's approach), so a representable boundary converges exactly.
        if matches!(start, Some(RubyValue::Float(_))) || matches!(end, Some(RubyValue::Float(_))) {
            return range_bsearch_float(start, end, exclusive, &p);
        }
        let Some(RubyValue::Int(lo0)) = start else {
            return Err(type_error!("can't do binary search for the given Range"));
        };
        let hi0 = match end {
            Some(RubyValue::Int(h)) => Some(if exclusive { *h } else { *h + 1 }),
            None => None,
            Some(_) => return Err(type_error!("can't do binary search for the given Range")),
        };
        let (mut lo, mut hi) = match hi0 {
            Some(h) => (*lo0, h),
            // Endless (`(1..).bsearch`): bracket the answer first by
            // doubling an offset from the start, then bisect as usual. A
            // block that never brackets walks off the fixnum end -> nil.
            None => {
                let mut lo = *lo0;
                let mut hi = None;
                let mut offset: i64 = 1;
                while hi.is_none() {
                    let Some(cand) = lo0.checked_add(offset) else {
                        return Ok(RubyValue::Nil);
                    };
                    let r = p.call(&[RubyValue::Int(cand)])?;
                    let cmp = match r {
                        RubyValue::Int(n) => Some(n.cmp(&0)),
                        RubyValue::Float(f) => f.partial_cmp(&0.0),
                        _ => None,
                    };
                    match cmp {
                        Some(std::cmp::Ordering::Equal) => return Ok(RubyValue::Int(cand)),
                        Some(std::cmp::Ordering::Less) => hi = Some(cand),
                        Some(std::cmp::Ordering::Greater) => lo = cand + 1,
                        None if r.truthy() => hi = Some(cand + 1),
                        None => lo = cand + 1,
                    }
                    let Some(next) = offset.checked_mul(2) else {
                        return Ok(RubyValue::Nil);
                    };
                    offset = next;
                }
                (lo, hi.expect("loop exits with a bound"))
            }
        };
        // `found` tracks the first true (find-minimum) or an exact `0` hit
        // (find-any); a non-zero numeric result only narrows the bounds, so
        // find-any with no hit leaves `found` unset -> nil.
        let mut found = None;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let r = p.call(&[RubyValue::Int(mid)])?;
            let cmp = match r {
                RubyValue::Int(n) => Some(n.cmp(&0)),
                RubyValue::Float(f) => f.partial_cmp(&0.0),
                _ => None,
            };
            match cmp {
                Some(std::cmp::Ordering::Equal) => { found = Some(mid); break; }
                Some(std::cmp::Ordering::Less) => hi = mid,
                Some(std::cmp::Ordering::Greater) => lo = mid + 1,
                None if r.truthy() => { found = Some(mid); hi = mid; }
                None => lo = mid + 1,
            }
        }
        Ok(found.map_or(RubyValue::Nil, RubyValue::Int))
    }
    // `Range#===` IS `#cover?` -- an endpoint comparison, whatever the
    // element type (CRuby's `range_eqq` goes straight to `r_cover_p`).
    def "===" (recv, other) {
        let (start, end, exclusive) = range_parts(recv);
        Ok(RubyValue::Bool(crate::value::range_covers(
            start, end, exclusive, other,
        )))
    }
    // `include?`/`member?` are NOT `===`. Ruby takes the endpoint shortcut
    // only where the element type is dense enough for it to be exact
    // (`range.c`'s `range_include_internal`) and WALKS otherwise -- which is
    // why `("a".."e").include?("bb")` is false while `cover?("bb")` is true.
    // The walk is `Enumerable#include?` over this range's own `each`, so a
    // String range steps by `succ` exactly as ruby's `rb_str_upto_each` does.
    def "include?" | "member?" (recv, other) {
        let (start, end, exclusive) = range_parts(recv);
        if linear_endpoint(start) || linear_endpoint(end) || integer_edge(start, end) {
            return Ok(RubyValue::Bool(crate::value::range_covers(
                start, end, exclusive, other,
            )));
        }
        // Neither end is orderable-by-comparison, so membership needs the
        // walk -- and an unbounded side has no walk to make.
        if open_endpoint(start) && open_endpoint(end) {
            return Ok(RubyValue::Bool(linear_endpoint(Some(other))));
        }
        if open_endpoint(start) || open_endpoint(end) {
            return Err(type_error!(
                "cannot determine inclusion in beginless/endless ranges"
            ));
        }
        enumerable::enumerable_send(recv, "include?", __args, None)
            .expect("Enumerable implements include?")
    }
    // `cover?` alone accepts a RANGE argument (range containment); `===`/
    // `include?`/`member?` treat a Range as an ordinary value (never covered).
    def "cover?" (recv, arg) {
        let (start, end, exclusive) = range_parts(recv);
        if matches!(arg, RubyValue::Range(..)) {
            return Ok(RubyValue::Bool(range_covers_range(start, end, exclusive, arg)));
        }
        Ok(RubyValue::Bool(crate::value::range_covers(start, end, exclusive, arg)))
    }
    // `overlap?(other)` -- do two ranges share at least one element? False
    // when either range lies wholly beyond the other's end (CRuby range.c's
    // empty-region test); a beginless/endless bound never bounds that side.
    def "overlap?" (recv, arg) {
        let RubyValue::Range(__rg) = arg else {
            return Err(type_error!("wrong argument type {} (expected Range)",
                    crate::builtins::class_name_of(arg)));
        };
        let (sb, se, sx) = range_parts(recv);
        let (ob, oe, ox) = __rg.parts();
        // `empty_region(beg, end, excl)`: beg lies past end, so nothing between.
        let empty_region = |beg: Option<&RubyValue>, end: Option<&RubyValue>, excl: bool| {
            match (beg, end) {
                (None, _) | (_, None) => false,
                (Some(RubyValue::Nil), _) | (_, Some(RubyValue::Nil)) => false,
                (Some(b), Some(e)) => match b.rb_cmp(e) {
                    Some(c) => c > 0 || (excl && c == 0),
                    None => false,
                },
            }
        };
        Ok(RubyValue::Bool(
            !empty_region(sb, oe, ox) && !empty_region(ob, se, sx),
        ))
    }
    def "last"(recv, arg?) {
        let (start, end, exclusive) = range_parts(recv);
        match arg {
            // Like `first`, `last` is NOT `end` under another name: an endless
            // range HAS no last element, where its `end` is plainly nil.
            None => match end {
                Some(v) => Ok(v.clone()),
                None => Err(crate::range_endpoint_error(false)),
            },
            Some(v) => {
                let n = crate::builtins::convert::to_index(v)?;
                // An endless range has no tail to take. This has to be checked
                // HERE rather than left to the `to_a` below: that goes through
                // Enumerable, which walks `each` and so never returns, instead
                // of through Range's own to_a and its endless guard.
                if end.is_none() {
                    return Err(crate::range_endpoint_error(false));
                }
                // Materialize (Enumerable to_a) and take the tail --
                // `last(n)` INCLUDES an exclusive end's predecessor set.
                let _ = (start, exclusive);
                let all = crate::builtins::enumerable::enumerable_send(recv, "to_a", &[], None)
                    .expect("Enumerable implements to_a")?;
                let RubyValue::Array(all) = all else { unreachable!() };
                let items = all.lock().clone();
                let n = n.max(0) as usize;
                let skip = items.len().saturating_sub(n);
                Ok(RubyValue::Array(crate::array_new(items[skip..].to_vec())))
            }
        }
    }
    def "size" (recv) {
        let (start, end, exclusive) = range_parts(recv);
        // The begin must be an Integer (CRuby iterates from it via `succ`).
        let s = match start {
            Some(RubyValue::Int(s)) => *s,
            None => return Err(type_error!("can't iterate from NilClass")),
            // A numeric-but-non-Integer begin (Float, Rational, Complex, or a
            // bignum this path doesn't yet count) can't be succ-iterated, so
            // CRuby raises; a NON-numeric begin (String, Symbol, ...) simply
            // has no numeric size, so `Range#size` is nil rather than an error.
            Some(other @ (RubyValue::BigInt(_)
                | RubyValue::Float(_)
                | RubyValue::Rational(_)
                | RubyValue::Complex(_))) => return Err(type_error!("can't iterate from {}", crate::builtins::class_name_of(other))),
            Some(_) => return Ok(RubyValue::Nil),
        };
        // The last integer the range covers: an endless (or +Infinity) range is
        // infinite; a Float end floors (inclusive) or `ceil - 1` (exclusive).
        let last = match end {
            None => return Ok(RubyValue::Float(f64::INFINITY)),
            Some(RubyValue::Int(e)) => if exclusive { e - 1 } else { *e },
            Some(RubyValue::Float(f)) => {
                if f.is_infinite() && f.is_sign_positive() {
                    return Ok(RubyValue::Float(f64::INFINITY));
                }
                if exclusive { f.ceil() as i64 - 1 } else { f.floor() as i64 }
            }
            Some(other) => return Err(type_error!("no implicit conversion of {} into Integer", crate::builtins::convert_name_of(other))),
        };
        Ok(RubyValue::Int((last - s + 1).max(0)))
    }
    // `step(n)` / `% n`: over a NUMERIC range the blockless form answers an
    // `Enumerator::ArithmeticSequence`. The two names share one body and
    // differ only in what that sequence prints back.
    def "step" cfunc (recv, n, &block) {
        range_step(recv, "step", __args, n, block)
    }
    def "%" (recv, n, &block) {
        range_step(recv, "%", __args, n, block)
    }
    def "exclude_end?" (recv) {
        let (_, _, exclusive) = range_parts(recv);
        Ok(RubyValue::Bool(exclusive))
    }
    def "begin"(recv) {
        let (start, _, _) = range_parts(recv);
        Ok(start.cloned().unwrap_or(RubyValue::Nil))
    }
    // `first` is NOT `begin` under another name: a beginless range HAS no
    // first element, where its `begin` is plainly nil.
    def "first"(recv, *args, &_block) {
        // `first` with an argument is Enumerable's n-form; only the 0-arg
        // endpoint accessor lives here. Falling through on arity would be
        // wrong (Enumerable#first(n) IS reachable next in the chain), so:
        if !args.is_empty() {
            if let Some(RubyValue::Int(n)) = args.first()
                && *n < 0 {
                    return Err(arg_error!("negative array size (or size too big)"));
                }
            return crate::builtins::enumerable::enumerable_send(recv, "first", args, None)
                .expect("Enumerable implements first(n)");
        }
        let (start, _, _) = range_parts(recv);
        match start {
            Some(v) => Ok(v.clone()),
            None => Err(crate::range_endpoint_error(true)),
        }
    }
    def "end" (recv) {
        let (_, end, _) = range_parts(recv);
        Ok(end.cloned().unwrap_or(RubyValue::Nil))
    }
    // `min`/`max` on a FLOAT range are O(1) endpoints -- a float range can't be
    // iterated (`each`/`to_a` raise), so the Enumerable fallback would fail.
    // Other element types keep iterating through Enumerable, whose behavior is
    // already correct (and whose exclusive-`max` differs by type).
    def "min"(recv, *args, &block) {
        let (start, end, _) = range_parts(recv);
        // A beginless range has no minimum -- CRuby raises rather than iterate
        // (which a bare `enumerable_send` would attempt endlessly). Holds
        // regardless of arg/block (verified against ruby 4.0.6).
        if start.is_none() {
            return Err(range_error!("cannot get the minimum of beginless range"));
        }
        if args.is_empty() && block.is_none() {
            // The minimum of an ascending range with no block is its begin. An
            // ENDLESS range has one (returned here without iterating, which
            // would loop forever); a Float-bounded range returns the begin, or
            // nil for an empty range (begin > end).
            if let Some(s) = start
                && end.is_none() {
                    return Ok(s.clone());
                }
            let is_float = matches!(start, Some(RubyValue::Float(_)))
                || matches!(end, Some(RubyValue::Float(_)));
            if is_float {
                return Ok(match (start, end) {
                    (Some(s), Some(e)) if s.rb_cmp(e).is_some_and(|c| c > 0) => RubyValue::Nil,
                    (Some(s), _) => s.clone(),
                    _ => RubyValue::Nil,
                });
            }
        }
        // A custom comparator has to walk the whole range to find the smallest,
        // so an endless one has no answer -- CRuby says so instead of hanging.
        if block.is_some() && end.is_none() {
            return Err(range_error!(
                "cannot get the minimum of endless range with custom comparison method"
            ));
        }
        // `min(n)` with no block is just the first n counting up from `begin`
        // -- CRuby walks rather than sorting, which is what lets an ENDLESS
        // range answer. `first(n)` already breaks at n, and inherits the right
        // errors for free: a Float begin raises "can't iterate from Float", a
        // descending range answers [], a String range walks by succ.
        if !args.is_empty() && block.is_none() {
            // Range's own message for a negative count, not Enumerable's
            // ("attempt to take negative size") -- delegating below reaches
            // `Enumerable#first`, which never sees that this began as a Range.
            if let Some(v) = args.first()
                && crate::builtins::convert::to_index(v)? < 0
            {
                return Err(arg_error!("negative array size (or size too big)"));
            }
            return crate::builtins::enumerable::enumerable_send(recv, "first", args, None)
                .expect("Enumerable implements first");
        }
        crate::builtins::enumerable::enumerable_send(recv, "min", args, block)
            .expect("Enumerable implements min")
    }
    def "max"(recv, *args, &block) {
        let (start, end, exclusive) = range_parts(recv);
        // An endless range has no maximum -- CRuby raises before iterating
        // (which would loop forever). Holds regardless of arg/block (verified
        // against ruby 4.0.6, including a Float begin: `(1.0..).max`).
        if end.is_none() {
            return Err(range_error!("cannot get the maximum of endless range"));
        }
        let is_float = matches!(start, Some(RubyValue::Float(_)))
            || matches!(end, Some(RubyValue::Float(_)));
        if args.is_empty() && block.is_none() && is_float {
            let Some(e) = end else { return Ok(RubyValue::Nil) };
            if let Some(s) = start
                && s.rb_cmp(e).is_some_and(|c| c > 0) {
                    return Ok(RubyValue::Nil);
                }
            // An exclusive float end has no maximum element -- CRuby's exact
            // TypeError (only an Integer end can be decremented).
            if exclusive {
                return Err(type_error!("cannot exclude non Integer end value"));
            }
            return Ok(e.clone());
        }
        // Mirror of `min`'s: a comparator would have to walk down from a
        // begin this range does not have.
        if block.is_some() && start.is_none() {
            return Err(range_error!(
                "cannot get the maximum of beginless range with custom comparison method"
            ));
        }
        // A BEGINLESS range with an Int end has a maximum, and its n largest
        // count DOWN from that end with nothing to stop them -- so they are
        // answered here, arithmetically. Falling through would send `each`,
        // which raises "can't iterate from NilClass". A Float end has no
        // decrementable element, so it keeps that fallthrough (CRuby agrees:
        // `(..5.0).max(2)` is the same TypeError).
        if start.is_none()
            && block.is_none()
            && let Some(RubyValue::Int(e)) = end
        {
            let top = if exclusive { e - 1 } else { *e };
            return match args.first() {
                None => Ok(RubyValue::Int(top)),
                Some(v) => {
                    let n = crate::builtins::convert::to_index(v)?;
                    if n < 0 {
                        return Err(arg_error!("negative array size (or size too big)"));
                    }
                    let vals = (0..n).map(|i| RubyValue::Int(top - i)).collect();
                    Ok(RubyValue::Array(crate::array_new(vals)))
                }
            };
        }
        crate::builtins::enumerable::enumerable_send(recv, "max", args, block)
            .expect("Enumerable implements max")
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "=="(recv, _other) { inherited_row!(basic_object, "==", recv, __args, None) }
    def "eql?"(recv, _other) { inherited_row!(kernel, "eql?", recv, __args, None) }
    // ruby 4 freezes every Range at construction, so both private rows can
    // only ever answer FrozenError for a reachable receiver -- which is the
    // oracle's answer too. (`Range.allocate`'s blank is a by-value zeo
    // Range no row could re-seat; it takes the same refusal.)
    private def "initialize" cfunc (recv, *_args) {
        Err(range_reinit_refusal(recv))
    }
    private def "initialize_copy"(recv, _other) {
        Err(range_reinit_refusal(recv))
    }
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
    def "inspect"(recv) { inherited_row!(kernel, "inspect", recv, __args, None) }
    // NOT an alias of `#inspect`: `Complex`, `Rational` and `Regexp` all
    // spell the two differently, so each goes to its own Kernel row.
    def "to_s"(recv) { inherited_row!(kernel, "to_s", recv, __args, None) }
    // An unbounded range's count is `Infinity`, answered WITHOUT iterating --
    // `range.c`'s `range_count`, which takes the shortcut only for the bare
    // form. An argument or a block has to walk, and legitimately never
    // terminates; ruby calls that odd rather than wrong.
    def "count" cfunc (recv, *_args, &block) {
        if __args.is_empty() && block.is_none() {
            let (start, end, _) = range_parts(recv);
            if open_endpoint(start) || open_endpoint(end) {
                return Ok(RubyValue::Float(f64::INFINITY));
            }
        }
        own_row!(recv, |s| enumerable::count_own(s, __args, block))
    }
    def "minmax" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::minmax_own(s, __args, block)) }
    def "reverse_each" arity 0 (recv, *_args, &block) { own_row!(recv, |s| enumerable::reverse_each_own(s, __args, block)) }
    def "to_set" cfunc (recv, *_args, &_block) { own_row!(recv, |s| enumerable::to_set_own(s, __args, None)) }
}

/// The one answer both private re-init rows can give: FrozenError with the
/// receiver, CRuby's response for every constructed (frozen) Range.
fn range_reinit_refusal(recv: &crate::RubyValue) -> crate::Signal {
    crate::dispatch::raise_error_details(
        "FrozenError",
        format!("can't modify frozen Range: {}", recv.inspect_string()),
        &[("receiver", recv.clone())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `ruby_class!`-generated methods are reachable only through the
    /// dispatch table (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through Range's registered lookup.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::RANGE_CLASS)
            .expect("Range is a registered builtin table")
            .instance
            .as_ref()
            .expect("Range has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Range#{name} is defined"))
    }

    fn int_range(s: i64, e: i64, exclusive: bool) -> RubyValue {
        crate::builtins::range::range_value(
            Some(RubyValue::Int(s)),
            Some(RubyValue::Int(e)),
            exclusive,
        )
    }

    #[test]
    fn case_eq_covers_the_oracle_matrix() {
        let r = int_range(1, 5, false);
        assert!(matches!(
            imethod("===")(&r, &[RubyValue::Int(3)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            imethod("===")(&r, &[RubyValue::Float(5.5)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        let r6 = int_range(1, 6, false);
        assert!(matches!(
            imethod("===")(&r6, &[RubyValue::Float(5.5)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let excl = int_range(1, 5, true);
        assert!(matches!(
            imethod("===")(&excl, &[RubyValue::Int(5)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        // Incomparable subject: false, not an error.
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(matches!(
            imethod("===")(&r, &[s], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn endpoints_and_exclusion_report() {
        let r = int_range(1, 5, true);
        assert!(matches!(
            imethod("begin")(&r, &[], None).unwrap(),
            RubyValue::Int(1)
        ));
        assert!(matches!(
            imethod("end")(&r, &[], None).unwrap(),
            RubyValue::Int(5)
        ));
        assert!(matches!(
            imethod("exclude_end?")(&r, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}
