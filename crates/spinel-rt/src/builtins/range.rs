//! `Range` (CRuby range.c) -- stage B: the `each` primitive Enumerable
//! drives (migrated), plus `===`/`cover?`/`include?`-family over the shared
//! `range_covers` (the `Range#===` fix that makes `case x when 1..5` real).
//! The remaining Tier A rows land in stage E.

use crate::builtins::{arity, builtin_methods};
use crate::RubyValue;

fn range_parts(recv: &RubyValue) -> (Option<&RubyValue>, Option<&RubyValue>, bool) {
    match recv {
        RubyValue::Range(s, e, x) => (s.as_deref(), e.as_deref(), *x),
        _ => unreachable!("Range table row dispatched on a non-Range receiver"),
    }
}

builtin_methods! {
    pub(crate) fn lookup;

    "each" => fn each(recv, args, block) {
        arity!(args, 0);
        let Some(RubyValue::Proc(p)) = &block else {
            panic!("Range#each without a block isn't supported (no Enumerator; spike scope)");
        };
        let (start, end, exclusive) = range_parts(recv);
        let (Some(s), Some(e)) = (start, end) else {
            panic!("can't iterate from a beginless/endless Range (spike scope)");
        };
        match (s, e) {
            (RubyValue::Int(s), RubyValue::Int(e)) => {
                let last = if exclusive { *e - 1 } else { *e };
                let mut i = *s;
                while i <= last {
                    p(&[RubyValue::Int(i)])?;
                    i += 1;
                }
            }
            // String ranges iterate via `succ` until passing the end
            // (CRuby's rule, incl. the length guard: `"a".."e"` walks
            // b/c/d/e; a longer successor stops the walk).
            (RubyValue::Str(s), RubyValue::Str(e)) => {
                let end = e.lock().clone();
                let mut cur = s.lock().clone();
                loop {
                    if cur.len() > end.len() || (cur.len() == end.len() && cur > end) {
                        break;
                    }
                    if exclusive && cur == end {
                        break;
                    }
                    p(&[RubyValue::Str(crate::string_new(cur.clone()))])?;
                    if cur == end {
                        break;
                    }
                    cur = crate::builtins::string::succ_str(&cur);
                }
            }
            _ => panic!("can't iterate a non-Integer/non-String Range (spike scope)"),
        }
        Ok(recv.clone())
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
        let (Some(RubyValue::Int(s)), Some(RubyValue::Int(e))) = (start, end) else {
            panic!("Range#size on a non-Integer/beginless/endless range isn't supported (spike scope)");
        };
        let last = if exclusive { e - 1 } else { *e };
        Ok(RubyValue::Int((last - s + 1).max(0)))
    }
    // `step(n)` with a block (the Enumerator-returning form is 17.2).
    "step" | "%" => fn step(recv, args, block) {
        arity!(args, 1);
        let Some(RubyValue::Proc(p)) = &block else {
            panic!("Range#step without a block isn't supported (no Enumerator; spike scope)");
        };
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
            p(&[RubyValue::Int(i)])?;
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
