//! `Range` (CRuby range.c) -- stage B: the `each` primitive Enumerable
//! drives (migrated), plus `===`/`cover?`/`include?`-family over the shared
//! `range_covers` (the `Range#===` fix that makes `case x when 1..5` real).
//! The remaining Tier A rows land in stage E.

use crate::builtins::{arity, block_or_enum, builtin_methods};
use crate::RubyValue;

fn range_parts(recv: &RubyValue) -> (Option<&RubyValue>, Option<&RubyValue>, bool) {
    match recv {
        RubyValue::Range(s, e, x) => (s.as_deref(), e.as_deref(), *x),
        _ => unreachable!("Range table row dispatched on a non-Range receiver"),
    }
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
        return Err(crate::dispatch::raise_error(
            "TypeError",
            "can't do binary search for the given Range".to_string(),
        ));
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
        let x = u2f(mid);
        let r = p.call(&[RubyValue::Float(x)])?;
        let cmp = match r {
            RubyValue::Int(n) => Some(n.cmp(&0)),
            RubyValue::Float(f) => f.partial_cmp(&0.0),
            _ => None,
        };
        match cmp {
            Some(std::cmp::Ordering::Equal) => return Ok(RubyValue::Float(x)),
            Some(std::cmp::Ordering::Less) => { numeric_mode = true; hi = mid; }
            Some(std::cmp::Ordering::Greater) => { numeric_mode = true; lo = mid + 1; }
            None if r.truthy() => { satisfied = Some(x); hi = mid; }
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
        _ => false,
    }
}

builtin_methods! {
    pub(crate) fn lookup;

    // Materializing an UNBOUNDED range would spin forever growing a vector
    // until the process died, so CRuby guards `Range#to_a` specifically
    // (`range_to_a`, range.c:1023) and delegates to Enumerable otherwise.
    // Registered for both spellings because they are separate entries there
    // (range.c:2986) -- an alias would silently skip the guard on one.
    //
    // The test is on the END only, matching CRuby: a BEGINLESS range isn't
    // caught here and instead fails in `each`, which cannot start.
    "to_a" | "entries" => fn to_a(recv, args, _block) {
        arity!(args, 0);
        let (_, end, _) = range_parts(recv);
        let unbounded = match end {
            None => true,
            Some(RubyValue::Float(f)) => f.is_infinite() && *f > 0.0,
            Some(_) => false,
        };
        if unbounded {
            return Err(crate::dispatch::raise_error(
                "RangeError",
                "cannot convert endless range to an array".to_string(),
            ));
        }
        crate::builtins::enumerable::enumerable_send(recv, "to_a", &[], None)
            .expect("Enumerable implements to_a")
    }

    "each" => fn each(recv, args, block) {
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
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("can't iterate from {ty}"),
                ));
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
    "bsearch" => fn bsearch(recv, args, block) {
        arity!(args, 0);
        let p = crate::builtins::need_block!(block);
        let (start, end, exclusive) = range_parts(recv);
        // A float range bisects over the doubles' monotonic integer image
        // (CRuby's approach), so a representable boundary converges exactly.
        if matches!(start, Some(RubyValue::Float(_))) || matches!(end, Some(RubyValue::Float(_))) {
            return range_bsearch_float(start, end, exclusive, &p);
        }
        let (Some(RubyValue::Int(lo0)), Some(RubyValue::Int(hi0))) = (start, end) else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                "can't do binary search for the given Range".to_string(),
            ));
        };
        let (mut lo, mut hi) = (*lo0, if exclusive { *hi0 } else { *hi0 + 1 });
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
    // succ) -- for the numeric/comparable cases this spike supports the
    // cover check is the faithful behavior for all four names.
    "===" | "cover?" | "include?" | "member?" => fn case_eq(recv, args, _block) {
        arity!(args, 1);
        let (start, end, exclusive) = range_parts(recv);
        Ok(RubyValue::Bool(crate::value::range_covers(
            start, end, exclusive, &args[0],
        )))
    }
    // `overlap?(other)` -- do two ranges share at least one element? False
    // when either range lies wholly beyond the other's end (CRuby range.c's
    // empty-region test); a beginless/endless bound never bounds that side.
    "overlap?" => fn overlap_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Range(ob, oe, ox) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "wrong argument type {} (expected Range)",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
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
    "last" => fn last_m(recv, args, _block) {
        arity!(args, 0..=1);
        let (start, end, exclusive) = range_parts(recv);
        match args.first() {
            None => Ok(end.cloned().unwrap_or(RubyValue::Nil)),
            Some(RubyValue::Int(n)) => {
                // Materialize (Enumerable to_a) and take the tail --
                // `last(n)` INCLUDES an exclusive end's predecessor set.
                let _ = (start, exclusive);
                let all = crate::builtins::enumerable::enumerable_send(recv, "to_a", &[], None)
                    .expect("Enumerable implements to_a")?;
                let RubyValue::Array(all) = all else { unreachable!() };
                let items = all.lock().clone();
                let n = (*n).max(0) as usize;
                let skip = items.len().saturating_sub(n);
                Ok(RubyValue::Array(crate::array_new(items[skip..].to_vec())))
            }
            Some(other) => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::convert_name_of(other)
                ),
            )),
        }
    }
    "size" => fn size(recv, args, _block) {
        arity!(args, 0);
        let (start, end, exclusive) = range_parts(recv);
        // The begin must be an Integer (CRuby iterates from it via `succ`).
        let s = match start {
            Some(RubyValue::Int(s)) => *s,
            None => return Err(crate::dispatch::raise_error(
                "TypeError",
                "can't iterate from NilClass".to_string(),
            )),
            // A numeric-but-non-Integer begin (Float, Rational, Complex, or a
            // bignum this path doesn't yet count) can't be succ-iterated, so
            // CRuby raises; a NON-numeric begin (String, Symbol, ...) simply
            // has no numeric size, so `Range#size` is nil rather than an error.
            Some(other @ (RubyValue::BigInt(_)
                | RubyValue::Float(_)
                | RubyValue::Rational(_)
                | RubyValue::Complex(_))) => return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("can't iterate from {}", crate::builtins::class_name_of(other)),
            )),
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
            Some(other) => return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("no implicit conversion of {} into Integer", crate::builtins::convert_name_of(other)),
            )),
        };
        Ok(RubyValue::Int((last - s + 1).max(0)))
    }
    // `step(n)`: the blockless form returns an Enumerator (Phase 17.2).
    "step" | "%" => fn step(recv, args, block) {
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
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    "can't iterate from the given Range".to_string(),
                ));
            };
            let unit = match &args[0] {
                RubyValue::Int(n) => *n as f64,
                RubyValue::Float(f) => *f,
                _ => unreachable!(),
            };
            if unit == 0.0 {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    "step can't be 0".to_string(),
                ));
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
            panic!("Range#step on a non-Integer range isn't supported (spike scope)");
        };
        let RubyValue::Int(by) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::convert_name_of(&args[0])
                ),
            ));
        };
        if *by == 0 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "step can't be 0".to_string(),
            ));
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
    "exclude_end?" => fn exclude_end_p(recv, args, _block) {
        arity!(args, 0);
        let (_, _, exclusive) = range_parts(recv);
        Ok(RubyValue::Bool(exclusive))
    }
    "begin" | "first" => fn begin_m(recv, args, _block) {
        // `first` with an argument is Enumerable's n-form; only the 0-arg
        // endpoint accessor lives here. Falling through on arity would be
        // wrong (Enumerable#first(n) IS reachable next in the chain), so:
        if !args.is_empty() {
            if let Some(RubyValue::Int(n)) = args.first() {
                if *n < 0 {
                    return Err(crate::dispatch::raise_error(
                        "ArgumentError",
                        "negative array size (or size too big)".to_string(),
                    ));
                }
            }
            return crate::builtins::enumerable::enumerable_send(recv, "first", args, None)
                .expect("Enumerable implements first(n)");
        }
        let (start, _, _) = range_parts(recv);
        Ok(start.cloned().unwrap_or(RubyValue::Nil))
    }
    "end" => fn end_m(recv, args, _block) {
        arity!(args, 0);
        let (_, end, _) = range_parts(recv);
        Ok(end.cloned().unwrap_or(RubyValue::Nil))
    }
    // `min`/`max` on a FLOAT range are O(1) endpoints -- a float range can't be
    // iterated (`each`/`to_a` raise), so the Enumerable fallback would fail.
    // Other element types keep iterating through Enumerable, whose behavior is
    // already correct (and whose exclusive-`max` differs by type).
    "min" => fn range_min(recv, args, block) {
        let (start, end, _) = range_parts(recv);
        let is_float = matches!(start, Some(RubyValue::Float(_)))
            || matches!(end, Some(RubyValue::Float(_)));
        if args.is_empty() && block.is_none() && is_float {
            // The begin, or nil for an empty range (begin > end).
            return Ok(match (start, end) {
                (Some(s), Some(e)) if s.rb_cmp(e).is_some_and(|c| c > 0) => RubyValue::Nil,
                (Some(s), _) => s.clone(),
                _ => RubyValue::Nil,
            });
        }
        crate::builtins::enumerable::enumerable_send(recv, "min", args, block)
            .expect("Enumerable implements min")
    }
    "max" => fn range_max(recv, args, block) {
        let (start, end, exclusive) = range_parts(recv);
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
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    "cannot exclude non Integer end value".to_string(),
                ));
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
        assert!(matches!(case_eq(&r, &[RubyValue::Int(3)], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(
            case_eq(&r, &[RubyValue::Float(5.5)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        let r6 = int_range(1, 6, false);
        assert!(matches!(
            case_eq(&r6, &[RubyValue::Float(5.5)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let excl = int_range(1, 5, true);
        assert!(matches!(
            case_eq(&excl, &[RubyValue::Int(5)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        // Incomparable subject: false, not an error.
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(matches!(case_eq(&r, &[s], None).unwrap(), RubyValue::Bool(false)));
    }

    #[test]
    fn endpoints_and_exclusion_report() {
        let r = int_range(1, 5, true);
        assert!(matches!(begin_m(&r, &[], None).unwrap(), RubyValue::Int(1)));
        assert!(matches!(end_m(&r, &[], None).unwrap(), RubyValue::Int(5)));
        assert!(matches!(
            exclude_end_p(&r, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}
