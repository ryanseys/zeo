//! `Comparable` (CRuby compar.c) -- each method drives the receiver's own
//! `<=>` through FULL dynamic dispatch (`send_value`), so one
//! implementation serves user objects (registry `<=>`) and builtin values
//! (`String#<=>`/`Integer#<=>` table rows) alike -- the Phase 17.1 rework
//! that retired the `&RObj`-only version and made `"abc" < "abd"` resolve
//! String(no `<`) -> Comparable -> `String#<=>`.
//!
//! Real Ruby raises `ArgumentError: comparison of X with Y failed` when
//! `<=>` answers nil; with the Phase 17.1 exception factory that is now a
//! real rescuable raise. A MISSING `<=>` propagates the NoMethodError the
//! `<=>` dispatch itself raises, real Ruby's own failure shape.

use crate::{RubyValue, Signal, Symbol};

/// The receiver's own `<=>`, reduced to a sign -- `Ok(None)` is Ruby's
/// `nil` (incomparable).
fn cmp(recv: &RubyValue, other: &RubyValue) -> Result<Option<i64>, Signal> {
    let result = crate::dispatch::send_value(
        recv,
        Symbol::intern("<=>"),
        std::slice::from_ref(other),
        None,
    )?;
    crate::value::cmp_int(&result)
}

/// `cmp`, with real Ruby's incomparable-operands `ArgumentError` applied.
fn cmp_or_fail(recv: &RubyValue, other: &RubyValue) -> Result<i64, Signal> {
    match cmp(recv, other)? {
        Some(ord) => Ok(ord),
        None => Err(crate::value::cmp_error(recv, other)),
    }
}

/// The Comparable method surface -- `None` when `name` isn't a Comparable
/// method at all (the MRO walk continues past this ancestor).
/// `Comparable#==` deliberately treats an incomparable pair as `false`
/// (real Ruby's one nil-tolerant Comparable method).
pub(crate) fn comparable_send(
    recv: &RubyValue,
    name: &str,
    args: &[RubyValue],
) -> Option<Result<RubyValue, Signal>> {
    let result = match (name, args.len()) {
        ("<", 1) => cmp_or_fail(recv, &args[0]).map(|o| RubyValue::Bool(o < 0)),
        ("<=", 1) => cmp_or_fail(recv, &args[0]).map(|o| RubyValue::Bool(o <= 0)),
        (">", 1) => cmp_or_fail(recv, &args[0]).map(|o| RubyValue::Bool(o > 0)),
        (">=", 1) => cmp_or_fail(recv, &args[0]).map(|o| RubyValue::Bool(o >= 0)),
        // Identity wins first (CRuby's `x == y` short-circuit); otherwise
        // equal iff `<=>` is 0. A `nil` result is `false`; a non-numeric
        // result raises (via `cmp`'s `rb_cmpint`).
        ("==", 1) => match (recv, &args[0]) {
            (RubyValue::Object(a), RubyValue::Object(b)) if std::sync::Arc::ptr_eq(a, b) => {
                Ok(RubyValue::Bool(true))
            }
            _ => cmp(recv, &args[0]).map(|o| RubyValue::Bool(o == Some(0))),
        },
        ("between?", 2) => (|| {
            let lo = cmp_or_fail(recv, &args[0])?;
            let hi = cmp_or_fail(recv, &args[1])?;
            Ok(RubyValue::Bool(lo >= 0 && hi <= 0))
        })(),
        // A `nil` bound is open on that side (`5.clamp(1, nil)` -> 5),
        // mirroring the beginless/endless range form below. Two present
        // bounds must be ordered (CRuby rejects a reversed pair).
        ("clamp", 2) => (|| {
            let (lo, hi) = (&args[0], &args[1]);
            if !lo.is_nil() && !hi.is_nil() && cmp_or_fail(lo, hi)? > 0 {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    "min argument must be less than or equal to max argument".to_string(),
                ));
            }
            if !lo.is_nil() && cmp_or_fail(recv, lo)? < 0 {
                return Ok(lo.clone());
            }
            if !hi.is_nil() && cmp_or_fail(recv, hi)? > 0 {
                return Ok(hi.clone());
            }
            Ok(recv.clone())
        })(),
        // `clamp(range)` -- either bound may be absent (beginless/endless);
        // an exclusive range is CRuby's ArgumentError.
        ("clamp", 1) => (|| {
            let RubyValue::Range(lo, hi, exclusive) = &args[0] else {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "wrong argument type {} (expected Range)",
                        crate::builtins::class_name_of(&args[0])
                    ),
                ));
            };
            if *exclusive && hi.is_some() {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    "cannot clamp with an exclusive range".to_string(),
                ));
            }
            if let (Some(lo), Some(hi)) = (lo.as_deref(), hi.as_deref()) {
                if cmp_or_fail(lo, hi)? > 0 {
                    return Err(crate::dispatch::raise_error(
                        "ArgumentError",
                        "min argument must be less than or equal to max argument".to_string(),
                    ));
                }
            }
            if let Some(lo) = lo.as_deref() {
                if cmp_or_fail(recv, lo)? < 0 {
                    return Ok(lo.clone());
                }
            }
            if let Some(hi) = hi.as_deref() {
                if cmp_or_fail(recv, hi)? > 0 {
                    return Ok(hi.clone());
                }
            }
            Ok(recv.clone())
        })(),
        _ => return None,
    };
    Some(result)
}

/// Name membership for `respond_to?`'s MRO walk (argument counts aren't
/// its concern -- real `respond_to?` is name-only too).
pub(crate) fn responds(name: &str) -> bool {
    matches!(name, "<" | "<=" | ">" | ">=" | "==" | "between?" | "clamp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(v.to_string()))
    }

    #[test]
    fn string_ordering_resolves_through_the_string_spaceship_row() {
        let r = comparable_send(&s("abc"), "<", &[s("abd")]).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = comparable_send(&s("b"), ">", &[s("a")]).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = comparable_send(&s("m"), "clamp", &[s("a"), s("f")]).unwrap().unwrap();
        let RubyValue::Str(clamped) = r else { panic!() };
        assert_eq!(&*clamped.lock().to_utf8_lossy(), "f");
    }

    #[test]
    fn between_drives_the_receivers_spaceship() {
        let r = comparable_send(&s("m"), "between?", &[s("a"), s("z")])
            .unwrap()
            .unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
    }

    #[test]
    fn incomparable_ordering_fails_loudly_eq_is_tolerant() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| comparable_send(&s("a"), "<", &[RubyValue::Int(1)])));
        assert!(r.is_err()); // registry-less: ArgumentError surfaces as a panic
        let r = comparable_send(&s("a"), "==", &[RubyValue::Int(1)]).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Bool(false)));
    }

    #[test]
    fn non_comparable_names_fall_through() {
        assert!(comparable_send(&s("a"), "upcase", &[]).is_none());
        assert!(responds("between?"));
        assert!(!responds("upcase"));
    }

    fn int(v: i64) -> RubyValue {
        RubyValue::Int(v)
    }

    fn range(lo: Option<i64>, hi: Option<i64>, exclusive: bool) -> RubyValue {
        RubyValue::Range(
            lo.map(|v| Box::new(RubyValue::Int(v))),
            hi.map(|v| Box::new(RubyValue::Int(v))),
            exclusive,
        )
    }

    /// `clamp(range)` -- the one-argument form, alongside the long-standing
    /// `clamp(lo, hi)`. Oracle-verified; the `comparable_clamp` coverage in
    /// `value_obj_boxing_clamp` exercises it against a user class too.
    #[test]
    fn clamp_with_a_range_pins_to_either_bound() {
        let below = comparable_send(&int(0), "clamp", &[range(Some(1), Some(5), false)])
            .unwrap()
            .unwrap();
        assert_eq!(below.inspect_string(), "1");

        let above = comparable_send(&int(9), "clamp", &[range(Some(1), Some(5), false)])
            .unwrap()
            .unwrap();
        assert_eq!(above.inspect_string(), "5");

        let inside = comparable_send(&int(3), "clamp", &[range(Some(1), Some(5), false)])
            .unwrap()
            .unwrap();
        assert_eq!(inside.inspect_string(), "3");
    }

    /// A beginless/endless range clamps on one side only.
    #[test]
    fn clamp_with_an_open_ended_range_pins_one_side() {
        let endless = comparable_send(&int(0), "clamp", &[range(Some(1), None, false)])
            .unwrap()
            .unwrap();
        assert_eq!(endless.inspect_string(), "1");
        let endless_high = comparable_send(&int(99), "clamp", &[range(Some(1), None, false)])
            .unwrap()
            .unwrap();
        assert_eq!(endless_high.inspect_string(), "99");

        let beginless = comparable_send(&int(99), "clamp", &[range(None, Some(5), false)])
            .unwrap()
            .unwrap();
        assert_eq!(beginless.inspect_string(), "5");
    }

    /// An exclusive range has no representable maximum to clamp to --
    /// CRuby's ArgumentError. (Registry-less, it surfaces as a panic.)
    #[test]
    fn clamp_with_an_exclusive_range_is_an_argument_error() {
        let r = std::panic::catch_unwind(|| {
            comparable_send(&int(9), "clamp", &[range(Some(1), Some(5), true)])
        });
        assert!(r.is_err());
    }

    /// An exclusive range with NO upper bound is fine -- nothing to exclude.
    #[test]
    fn clamp_with_an_exclusive_endless_range_is_allowed() {
        let out = comparable_send(&int(0), "clamp", &[range(Some(1), None, true)])
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "1");
    }

    #[test]
    fn clamp_still_takes_the_two_argument_form() {
        let out = comparable_send(&int(9), "clamp", &[int(1), int(5)]).unwrap().unwrap();
        assert_eq!(out.inspect_string(), "5");
    }

    /// `clamp` is a name Comparable answers to (respond_to?'s MRO walk).
    #[test]
    fn responds_lists_clamp() {
        assert!(responds("clamp"));
        assert!(responds("between?"));
        assert!(!responds("nope"));
    }
}
