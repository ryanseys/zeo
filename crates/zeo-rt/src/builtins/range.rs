//! `Range` (CRuby range.c) -- stage B: the `each` primitive Enumerable
//! drives (migrated), plus `===`/`cover?`/`include?`-family over the shared
//! `range_covers` (the `Range#===` fix that makes `case x when 1..5` real).
//! The remaining Tier A rows land in stage E.

use crate::RubyValue;
use crate::builtins::{arg_error, arity, block_or_enum, range_error, type_error};
use zeo_macros::ruby_class;

fn range_parts(recv: &RubyValue) -> (Option<&RubyValue>, Option<&RubyValue>, bool) {
    match recv {
        RubyValue::Range(s, e, x) => (s.as_deref(), e.as_deref(), *x),
        _ => unreachable!("Range table row dispatched on a non-Range receiver"),
    }
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

    // Materializing an UNBOUNDED range would spin forever growing a vector
    // until the process died, so CRuby guards `Range#to_a` specifically
    // (`range_to_a`, range.c:1023) and delegates to Enumerable otherwise.
    // Registered for both spellings because they are separate entries there
    // (range.c:2986) -- an alias would silently skip the guard on one.
    //
    // The test is on the END only, matching CRuby: a BEGINLESS range isn't
    // caught here and instead fails in `each`, which cannot start.
    def "to_a" arity 0 | "entries" arity 0 (recv, args, _block) {
        arity!(args, 0);
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

    def "each" arity 0 (recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each", args, block);
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
    def "bsearch" arity 0 (recv, args, block) {
        arity!(args, 0);
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
    // `Range#===` IS `#cover?`; `include?`/`member?` differ from `cover?`
    // in real Ruby only for non-linear element types (String ranges walk
    // succ) -- for the numeric/comparable cases this runtime supports the
    // cover check is the faithful behavior for all four names.
    def "===" arity 1 | "include?" arity 1 | "member?" arity 1 (recv, args, _block) {
        arity!(args, 1);
        let (start, end, exclusive) = range_parts(recv);
        Ok(RubyValue::Bool(crate::value::range_covers(
            start, end, exclusive, &args[0],
        )))
    }
    // `cover?` alone accepts a RANGE argument (range containment); `===`/
    // `include?`/`member?` treat a Range as an ordinary value (never covered).
    def "cover?" arity 1 (recv, args, _block) {
        arity!(args, 1);
        let (start, end, exclusive) = range_parts(recv);
        if matches!(&args[0], RubyValue::Range(..)) {
            return Ok(RubyValue::Bool(range_covers_range(start, end, exclusive, &args[0])));
        }
        Ok(RubyValue::Bool(crate::value::range_covers(start, end, exclusive, &args[0])))
    }
    // `overlap?(other)` -- do two ranges share at least one element? False
    // when either range lies wholly beyond the other's end (CRuby range.c's
    // empty-region test); a beginless/endless bound never bounds that side.
    def "overlap?" arity 1 (recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Range(ob, oe, ox) = &args[0] else {
            return Err(type_error!("wrong argument type {} (expected Range)",
                    crate::builtins::class_name_of(&args[0])));
        };
        let (sb, se, sx) = range_parts(recv);
        let (ob, oe, ox) = (ob.as_deref(), oe.as_deref(), *ox);
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
    def "last"(recv, args, _block) {
        arity!(args, 0..=1);
        let (start, end, exclusive) = range_parts(recv);
        match args.first() {
            None => Ok(end.cloned().unwrap_or(RubyValue::Nil)),
            Some(v) => {
                let n = crate::builtins::convert::to_index(v)?;
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
    def "size" arity 0 (recv, args, _block) {
        arity!(args, 0);
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
    // `step(n)`: the blockless form returns an Enumerator.
    def "step" | "%" arity 1 (recv, args, block) {
        arity!(args, 1);
        let p = block_or_enum!(recv, "step", args, block);
        let (start, end, exclusive) = range_parts(recv);
        // Float mode when any endpoint or the step is a Float. CRuby computes
        // the element COUNT and multiplies (`beg + i*unit`) rather than
        // repeatedly adding, so there's no drift and `1.0` lands exactly.
        let is_float = matches!(start, Some(RubyValue::Float(_)))
            || matches!(end, Some(RubyValue::Float(_)))
            || matches!(&args[0], RubyValue::Float(_));
        if is_float {
            let to_f = |v: Option<&RubyValue>| match v {
                Some(RubyValue::Int(n)) => Some(*n as f64),
                Some(RubyValue::Float(f)) => Some(*f),
                _ => None,
            };
            let (Some(beg), Some(fin)) = (to_f(start), to_f(end)) else {
                return Err(type_error!("can't iterate from the given Range"));
            };
            let unit = match &args[0] {
                RubyValue::Int(n) => *n as f64,
                RubyValue::Float(f) => *f,
                _ => unreachable!(),
            };
            if unit == 0.0 {
                return Err(arg_error!("step can't be 0"));
            }
            let n_f = (fin - beg) / unit;
            let err = (((beg.abs() + fin.abs() + (fin - beg).abs()) / unit.abs())
                * f64::EPSILON)
                .min(0.5);
            let n = if exclusive { (n_f - err).floor() } else { (n_f + err).floor() };
            let mut i = 0.0;
            while i <= n {
                p.call(&[RubyValue::Float(i * unit + beg)])?;
                i += 1.0;
            }
            return Ok(recv.clone());
        }
        let (Some(RubyValue::Int(s)), Some(RubyValue::Int(e))) = (start, end) else {
            panic!("Range#step on a non-Integer range isn't supported (zeo limitation)");
        };
        // NOT an implicit-conversion site: CRuby's Range#step raises the
        // numeric-tower coerce shape here (oracle: `(1..5).step("x")` is
        // "String can't be coerced into Integer").
        let RubyValue::Int(by) = &args[0] else {
            return Err(type_error!("{} can't be coerced into Integer",
                    crate::builtins::coerce_operand_name(&args[0])));
        };
        if *by == 0 {
            return Err(arg_error!("step can't be 0"));
        }
        // A negative step walks a descending range downward (`(10..2).step(-2)`
        // is 10,8,6,4,2); a step against the range's direction yields nothing.
        if *by < 0 {
            let mut i = *s;
            while if exclusive { i > *e } else { i >= *e } {
                p.call(&[RubyValue::Int(i)])?;
                i += by;
            }
            return Ok(recv.clone());
        }
        let last = if exclusive { e - 1 } else { *e };
        let mut i = *s;
        while i <= last {
            p.call(&[RubyValue::Int(i)])?;
            i += by;
        }
        Ok(recv.clone())
    }
    def "exclude_end?" arity 0 (recv, args, _block) {
        arity!(args, 0);
        let (_, _, exclusive) = range_parts(recv);
        Ok(RubyValue::Bool(exclusive))
    }
    def "begin" arity 0 | "first"(recv, args, _block) {
        // `first` with an argument is Enumerable's n-form; only the 0-arg
        // endpoint accessor lives here. Falling through on arity would be
        // wrong (Enumerable#first(n) IS reachable next in the chain), so:
        if !args.is_empty() {
            if let Some(RubyValue::Int(n)) = args.first() {
                if *n < 0 {
                    return Err(arg_error!("negative array size (or size too big)"));
                }
            }
            return crate::builtins::enumerable::enumerable_send(recv, "first", args, None)
                .expect("Enumerable implements first(n)");
        }
        let (start, _, _) = range_parts(recv);
        Ok(start.cloned().unwrap_or(RubyValue::Nil))
    }
    def "end" arity 0 (recv, args, _block) {
        arity!(args, 0);
        let (_, end, _) = range_parts(recv);
        Ok(end.cloned().unwrap_or(RubyValue::Nil))
    }
    // `min`/`max` on a FLOAT range are O(1) endpoints -- a float range can't be
    // iterated (`each`/`to_a` raise), so the Enumerable fallback would fail.
    // Other element types keep iterating through Enumerable, whose behavior is
    // already correct (and whose exclusive-`max` differs by type).
    def "min"(recv, args, block) {
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
            if let Some(s) = start {
                if end.is_none() {
                    return Ok(s.clone());
                }
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
        crate::builtins::enumerable::enumerable_send(recv, "min", args, block)
            .expect("Enumerable implements min")
    }
    def "max"(recv, args, block) {
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
            if let Some(s) = start {
                if s.rb_cmp(e).is_some_and(|c| c > 0) {
                    return Ok(RubyValue::Nil);
                }
            }
            // An exclusive float end has no maximum element -- CRuby's exact
            // TypeError (only an Integer end can be decremented).
            if exclusive {
                return Err(type_error!("cannot exclude non Integer end value"));
            }
            return Ok(e.clone());
        }
        crate::builtins::enumerable::enumerable_send(recv, "max", args, block)
            .expect("Enumerable implements max")
    }
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
        RubyValue::Range(
            Some(Box::new(RubyValue::Int(s))),
            Some(Box::new(RubyValue::Int(e))),
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
        assert!(matches!(imethod("begin")(&r, &[], None).unwrap(), RubyValue::Int(1)));
        assert!(matches!(imethod("end")(&r, &[], None).unwrap(), RubyValue::Int(5)));
        assert!(matches!(
            imethod("exclude_end?")(&r, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}
