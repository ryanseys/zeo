//! `Comparable` (CRuby compar.c) -- each method drives the receiver's own
//! `<=>` through FULL dynamic dispatch (`send_value`), so one
//! implementation serves user objects (registry `<=>`) and builtin values
//! (`String#<=>`/`Integer#<=>` table rows) alike -- `"abc" < "abd"` resolves
//! String (no `<`) -> Comparable -> `String#<=>`.
//!
//! Dispatched like any other builtin module: `class_table`/
//! `class_arity_table` map `COMPARABLE_CLASS` to this table's generated
//! `lookup`/`lookup_arity`, and the MRO walk finds it as an ordinary
//! ancestor hit.
//!
//! Real Ruby raises `ArgumentError: comparison of X with Y failed` when
//! `<=>` answers nil; here that is a real rescuable raise.
//! A MISSING `<=>` propagates the NoMethodError the
//! `<=>` dispatch itself raises, real Ruby's own failure shape.

use crate::builtins::{arg_error, type_error};
use crate::{RubyValue, Signal, Symbol};
use zeo_macros::ruby_module;

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

ruby_module! {
    Comparable = zeo_abi::COMPARABLE_CLASS;

    def "<" (recv, other) {
        Ok(RubyValue::Bool(cmp_or_fail(recv, other)? < 0))
    }
    def "<=" (recv, other) {
        Ok(RubyValue::Bool(cmp_or_fail(recv, other)? <= 0))
    }
    def ">" (recv, other) {
        Ok(RubyValue::Bool(cmp_or_fail(recv, other)? > 0))
    }
    def ">=" (recv, other) {
        Ok(RubyValue::Bool(cmp_or_fail(recv, other)? >= 0))
    }
    // Identity wins first (CRuby's `x == y` short-circuit); otherwise equal
    // iff `<=>` is 0. `Comparable#==` deliberately treats an incomparable
    // pair (`<=>` answering nil) as `false` -- real Ruby's one nil-tolerant
    // Comparable method; a non-numeric result raises (via `cmp`'s
    // `rb_cmpint`).
    def "==" (recv, other) {
        match (recv, other) {
            (RubyValue::Object(a), RubyValue::Object(b)) if std::sync::Arc::ptr_eq(a, b) => {
                Ok(RubyValue::Bool(true))
            }
            _ => Ok(RubyValue::Bool(cmp(recv, other)? == Some(0))),
        }
    }
    def "between?" (recv, arg1, arg2) {
        // SHORT-CIRCUITS, like `cmpint(x, min) >= 0 && cmpint(x, max) <= 0`:
        // once the receiver is below the lower bound the answer is false, and
        // the upper bound is never compared -- so `"Hello".between?("l", 0)`
        // answers false where comparing against `0` would have raised.
        let lo = cmp_or_fail(recv, arg1)?;
        if lo < 0 {
            return Ok(RubyValue::Bool(false));
        }
        Ok(RubyValue::Bool(cmp_or_fail(recv, arg2)? <= 0))
    }
    // `clamp(lo, hi)` or `clamp(range)`. In the two-argument form a `nil`
    // bound is open on that side (`5.clamp(1, nil)` -> 5), mirroring the
    // beginless/endless range form; an exclusive bounded range is CRuby's
    // ArgumentError. Two present bounds must be ordered (CRuby rejects a
    // reversed pair).
    def "clamp" cfunc (recv, min, max?) {
        let (lo, hi): (Option<RubyValue>, Option<RubyValue>) = if let Some(max) = max {
            let open = |v: &RubyValue| if v.is_nil() { None } else { Some(v.clone()) };
            (open(min), open(max))
        } else {
            let RubyValue::Range(__rg) = min else {
                return Err(type_error!(
                    "wrong argument type {} (expected Range)",
                    crate::builtins::class_name_of(min)
                ));
            };
            let (lo, hi, exclusive) = __rg.parts();
            if exclusive && hi.is_some() {
                return Err(arg_error!("cannot clamp with an exclusive range"));
            }
            (lo.cloned(), hi.cloned())
        };
        if let (Some(lo), Some(hi)) = (&lo, &hi)
            && cmp_or_fail(lo, hi)? > 0 {
                return Err(arg_error!(
                    "min argument must be less than or equal to max argument"
                ));
            }
        if let Some(lo) = &lo
            && cmp_or_fail(recv, lo)? < 0 {
                return Ok(lo.clone());
            }
        if let Some(hi) = &hi
            && cmp_or_fail(recv, hi)? > 0 {
                return Ok(hi.clone());
            }
        Ok(recv.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test-local dispatch helper wrapping `lookup`, so the coverage below
    /// reads uniformly: `None` = not a Comparable method.
    fn comparable_send(
        recv: &RubyValue,
        name: &str,
        args: &[RubyValue],
    ) -> Option<Result<RubyValue, Signal>> {
        lookup(name).map(|f| f(recv, args, None))
    }

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(v.to_string()))
    }

    #[test]
    fn string_ordering_resolves_through_the_string_spaceship_row() {
        let r = comparable_send(&s("abc"), "<", &[s("abd")])
            .unwrap()
            .unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = comparable_send(&s("b"), ">", &[s("a")]).unwrap().unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = comparable_send(&s("m"), "clamp", &[s("a"), s("f")])
            .unwrap()
            .unwrap();
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
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            comparable_send(&s("a"), "<", &[RubyValue::Int(1)])
        }));
        assert!(r.is_err()); // registry-less: ArgumentError surfaces as a panic
        let r = comparable_send(&s("a"), "==", &[RubyValue::Int(1)])
            .unwrap()
            .unwrap();
        assert!(matches!(r, RubyValue::Bool(false)));
    }

    #[test]
    fn non_comparable_names_fall_through() {
        assert!(comparable_send(&s("a"), "upcase", &[]).is_none());
        assert!(lookup("between?").is_some());
        assert!(lookup("upcase").is_none());
    }

    fn int(v: i64) -> RubyValue {
        RubyValue::Int(v)
    }

    fn range(lo: Option<i64>, hi: Option<i64>, exclusive: bool) -> RubyValue {
        crate::builtins::range::range_value(
            lo.map(RubyValue::Int),
            hi.map(RubyValue::Int),
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
        let out = comparable_send(&int(9), "clamp", &[int(1), int(5)])
            .unwrap()
            .unwrap();
        assert_eq!(out.inspect_string(), "5");
    }

    /// `clamp` is a name Comparable answers to (`respond_to?`'s MRO walk
    /// resolves it through this same generated `lookup`).
    #[test]
    fn responds_lists_clamp() {
        assert!(lookup("clamp").is_some());
        assert!(lookup("between?").is_some());
        assert!(lookup("nope").is_none());
    }
}
