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

use crate::builtins::class_name_of;
use crate::{RubyValue, Signal, Symbol};

/// The receiver's own `<=>`, reduced to a sign -- `Ok(None)` is Ruby's
/// `nil` (incomparable).
fn cmp(recv: &RubyValue, other: &RubyValue) -> Result<Option<i64>, Signal> {
    match crate::dispatch::send_value(
        recv,
        Symbol::intern("<=>"),
        std::slice::from_ref(other),
        None,
    )? {
        RubyValue::Int(i) => Ok(Some(i.signum())),
        _ => Ok(None),
    }
}

/// `cmp`, with real Ruby's incomparable-operands `ArgumentError` applied.
fn cmp_or_fail(recv: &RubyValue, other: &RubyValue) -> Result<i64, Signal> {
    match cmp(recv, other)? {
        Some(ord) => Ok(ord),
        None => Err(crate::dispatch::raise_error(
            "ArgumentError",
            format!(
                "comparison of {} with {} failed",
                class_name_of(recv),
                class_name_of(other)
            ),
        )),
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
        ("==", 1) => cmp(recv, &args[0]).map(|o| RubyValue::Bool(o == Some(0))),
        ("between?", 2) => (|| {
            let lo = cmp_or_fail(recv, &args[0])?;
            let hi = cmp_or_fail(recv, &args[1])?;
            Ok(RubyValue::Bool(lo >= 0 && hi <= 0))
        })(),
        ("clamp", 2) => (|| {
            if cmp_or_fail(recv, &args[0])? < 0 {
                return Ok(args[0].clone());
            }
            if cmp_or_fail(recv, &args[1])? > 0 {
                return Ok(args[1].clone());
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
        assert_eq!(&*clamped.lock(), "f");
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
}
