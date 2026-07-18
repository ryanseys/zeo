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
            // A beginless range (or an otherwise non-iterable element type)
            // can't be walked forward -- CRuby's own TypeError.
            _ => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    "can't iterate from the given Range".to_string(),
                ))
            }
        }
        Ok(recv.clone())
    }
    // `Range#bsearch` in find-minimum mode over an integer range: the block
    // partitions the range into a false prefix then a true suffix, and the
    // first true element is answered (`nil` if none). Binary search on the
    // bounds -- no materialization, so a huge range is fine. A numeric block
    // result (find-any mode) is the same documented gap as `Array#bsearch`.
    "bsearch" => fn bsearch(recv, args, block) {
        arity!(args, 0);
        let p = crate::builtins::need_block!(block);
        let (start, end, exclusive) = range_parts(recv);
        let (Some(RubyValue::Int(lo0)), Some(RubyValue::Int(hi0))) = (start, end) else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                "can't do binary search for the given Range".to_string(),
            ));
        };
        let (mut lo, mut hi) = (*lo0, if exclusive { *hi0 } else { *hi0 + 1 });
        let mut found = None;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match p.call(&[RubyValue::Int(mid)])? {
                RubyValue::Int(_) | RubyValue::Float(_) => {
                    return Err(crate::dispatch::raise_error(
                        "NotImplementedError",
                        "Range#bsearch's find-any mode (a numeric block result) isn't supported yet (spike scope)".to_string(),
                    ))
                }
                r if r.truthy() => { found = Some(mid); hi = mid; }
                _ => lo = mid + 1,
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
                    crate::builtins::class_name_of(other)
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
            Some(other) => return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("can't iterate from {}", crate::builtins::class_name_of(other)),
            )),
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
                format!("no implicit conversion of {} into Integer", crate::builtins::class_name_of(other)),
            )),
        };
        Ok(RubyValue::Int((last - s + 1).max(0)))
    }
    // `step(n)`: the blockless form returns an Enumerator (Phase 17.2).
    "step" | "%" => fn step(recv, args, block) {
        arity!(args, 1);
        let p = block_or_enum!(recv, "step", args, block);
        let (start, end, exclusive) = range_parts(recv);
        let (Some(RubyValue::Int(s)), Some(RubyValue::Int(e))) = (start, end) else {
            panic!("Range#step on a non-Integer range isn't supported (spike scope)");
        };
        let RubyValue::Int(by) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
        };
        if *by <= 0 {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "step can't be 0 or negative".to_string(),
            ));
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
