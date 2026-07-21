//! The implicit-conversion protocol -- CRuby's `rb_convert_type` /
//! `rb_check_convert_type` family, implemented ONCE. A builtin that needs "an
//! Integer-ish argument" must accept not just `Integer` but any object whose
//! class answers `to_int` (same for `to_str`/`to_ary`/`to_hash`), raising
//! CRuby's exact `TypeError` shapes when the protocol fails:
//!
//! - method absent (strict form): `no implicit conversion of X into Y`
//!   (`nil`/`true`/`false` spell their value, not their class -- see
//!   [`super::convert_name_of`]);
//! - method present but answering the wrong type:
//!   `can't convert X to Y (X#meth gives Z)`.
//!
//! The `check_*` variants are `rb_check_convert_type`: an absent method (or a
//! `nil` answer) is `Ok(None)` instead of a raise -- the caller's "duck or
//! fall through" probe -- while a present-but-wrong-typed answer still
//! raises (a lying `to_x` is a bug worth surfacing, CRuby's own rule).
//!
//! Replaces the per-module coerce helpers that grew up before this module
//! existed. NOTE the numeric tower's `X can't be coerced into Y` operator
//! errors are a DIFFERENT protocol (`coerce`, `numeric.rs`) and stay there.

use super::{convert_name_of, type_error};
use crate::dispatch::{responds_to_value, send_value};
use crate::{RubyValue, Signal, Symbol};

/// Does `v` already satisfy `target` without conversion? `target` is the
/// Ruby class name used in error messages, so the mapping is by that name.
fn is_already(v: &RubyValue, target: &str) -> bool {
    matches!(
        (v, target),
        (RubyValue::Int(_) | RubyValue::BigInt(_), "Integer")
            | (RubyValue::Str(_), "String")
            | (RubyValue::Array(_), "Array")
            | (RubyValue::Hash(_), "Hash")
            | (RubyValue::Float(_), "Float")
    )
}

/// `rb_convert_type`: `v` itself when already a `target`, else the result of
/// `v.meth` -- absent method raises the strict "no implicit conversion"
/// TypeError, wrong-typed answer the "gives Z" TypeError.
pub fn convert(v: &RubyValue, target: &str, meth: &str) -> Result<RubyValue, Signal> {
    if is_already(v, target) {
        return Ok(v.clone());
    }
    match try_convert(v, target, meth)? {
        Some(converted) => Ok(converted),
        None => Err(type_error!(
            "no implicit conversion of {} into {target}",
            convert_name_of(v)
        )),
    }
}

/// `rb_check_convert_type`: like [`convert`], but an absent method or a `nil`
/// answer is `Ok(None)` -- only a present-and-lying `meth` raises.
pub fn check_convert(v: &RubyValue, target: &str, meth: &str) -> Result<Option<RubyValue>, Signal> {
    if is_already(v, target) {
        return Ok(Some(v.clone()));
    }
    try_convert(v, target, meth)
}

/// The shared probe: call `meth` if the receiver's class answers it,
/// type-checking a non-nil result. `Ok(None)` = no method / nil answer.
fn try_convert(v: &RubyValue, target: &str, meth: &str) -> Result<Option<RubyValue>, Signal> {
    let sym = Symbol::intern(meth);
    if !responds_to_value(v, sym, true) {
        return Ok(None);
    }
    let answer = send_value(v, sym, &[], None)?;
    if matches!(answer, RubyValue::Nil) {
        return Ok(None);
    }
    if !is_already(&answer, target) {
        return Err(type_error!(
            "can't convert {0} to {target} ({0}#{meth} gives {1})",
            convert_name_of(v),
            crate::builtins::class_name_of(&answer)
        ));
    }
    Ok(Some(answer))
}

/// `to_int` protocol, strict: the argument as an Integer value.
pub fn to_int(v: &RubyValue) -> Result<RubyValue, Signal> {
    convert(v, "Integer", "to_int")
}

pub fn check_to_int(v: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    check_convert(v, "Integer", "to_int")
}

/// `to_str` protocol, strict: the argument as a String value.
pub fn to_str(v: &RubyValue) -> Result<RubyValue, Signal> {
    convert(v, "String", "to_str")
}

pub fn check_to_str(v: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    check_convert(v, "String", "to_str")
}

pub fn to_ary(v: &RubyValue) -> Result<RubyValue, Signal> {
    convert(v, "Array", "to_ary")
}

pub fn check_to_ary(v: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    check_convert(v, "Array", "to_ary")
}

pub fn to_hash(v: &RubyValue) -> Result<RubyValue, Signal> {
    convert(v, "Hash", "to_hash")
}

// The one probe form without a caller yet -- kept so the protocol surface
// stays complete alongside its three siblings.
#[allow(dead_code)]
pub fn check_to_hash(v: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    check_convert(v, "Hash", "to_hash")
}

/// `rb_num2long` over the protocol: an integer-argument site's value as
/// `i64`, with CRuby's exact acceptance set and error shapes (all
/// oracle-verified):
///
/// - `Int` passes through; `Float` TRUNCATES (`"x" * 2.9 == "xx"`), with
///   out-of-`i64`-range/NaN/Inf raising `RangeError: float <%g> out of
///   range of integer`;
/// - `nil` has its own special TypeError: `no implicit conversion from nil
///   to integer` (lowercase -- a different shape than the generic one);
/// - anything else goes through `to_int`, ducks accepted, absent method
///   raising the generic `no implicit conversion of X into Integer`;
/// - bignum-range answers raise `RangeError: bignum too big to convert
///   into 'long'`.
pub fn to_index(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i),
        RubyValue::Float(f) => float_to_long(*f),
        RubyValue::BigInt(_) => Err(bignum_range_error()),
        RubyValue::Nil => Err(type_error!("no implicit conversion from nil to integer")),
        other => match to_int(other)? {
            RubyValue::Int(i) => Ok(i),
            RubyValue::BigInt(_) => Err(bignum_range_error()),
            _ => unreachable!("to_int post-checks its answer"),
        },
    }
}

fn bignum_range_error() -> Signal {
    crate::builtins::range_error!("bignum too big to convert into 'long'")
}

/// CRuby's `NUM2LONG` on a Float: truncate when the value fits `i64`
/// (`[-2^63, 2^63)` -- both bounds exact as doubles), else the RangeError.
fn float_to_long(f: f64) -> Result<i64, Signal> {
    const TWO_63: f64 = 9_223_372_036_854_775_808.0;
    if f.is_finite() && (-TWO_63..TWO_63).contains(&f) {
        Ok(f.trunc() as i64)
    } else {
        Err(crate::builtins::range_error!(
            "float {} out of range of integer",
            fmt_g(f)
        ))
    }
}

/// C's `%g` as CRuby's error messages render floats: 6 significant digits,
/// trailing zeros trimmed, `e+NN`/`e-NN` exponent form -- with CRuby's
/// capitalized specials (`NaN`, `Inf`, `-Inf`). Only consulted for
/// out-of-integer-range values, which always take the exponent form.
fn fmt_g(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Inf" } else { "-Inf" }.to_string();
    }
    // `{:.5e}` = 6 significant digits; then trim the mantissa's trailing
    // zeros and normalize the exponent to `e+NN` (Rust writes `e300`).
    let s = format!("{f:.5e}");
    let (mantissa, exp) = s.split_once('e').expect("{:e} always has an exponent");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    let (sign, digits) = match exp.strip_prefix('-') {
        Some(d) => ('-', d),
        None => ('+', exp),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

/// [`to_str`] unwrapped to the string handle -- what `arg_str!` call sites
/// consume (they go straight to `.lock()`).
pub fn to_rstr(v: &RubyValue) -> Result<crate::collections::RStr, Signal> {
    match to_str(v)? {
        RubyValue::Str(s) => Ok(s),
        _ => unreachable!("to_str post-checks its answer"),
    }
}

/// [`to_ary`] unwrapped to the array handle.
pub fn to_rary(v: &RubyValue) -> Result<crate::collections::RArray, Signal> {
    match to_ary(v)? {
        RubyValue::Array(a) => Ok(a),
        _ => unreachable!("to_ary post-checks its answer"),
    }
}

/// [`to_hash`] unwrapped to the hash handle.
pub fn to_rhash(v: &RubyValue) -> Result<crate::collections::RHash, Signal> {
    match to_hash(v)? {
        RubyValue::Hash(h) => Ok(h),
        _ => unreachable!("to_hash post-checks its answer"),
    }
}

// Deliberately no registry-less unit tests: every interesting path (duck
// types, the two TypeError shapes, nil answers) dispatches through the live
// class registry, so coverage lives in the e2e tier (`tests/e2e/`) where a
// real program exercises the protocol end-to-end.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_new;

    /// The one dispatch-free path: an argument already of the target type
    /// passes through without consulting the registry at all.
    #[test]
    fn already_target_passes_through() {
        assert!(matches!(
            to_int(&RubyValue::Int(3)).unwrap(),
            RubyValue::Int(3)
        ));
        assert!(matches!(to_index(&RubyValue::Int(7)).unwrap(), 7));
        let s = RubyValue::Str(string_new("x".into()));
        assert!(matches!(to_str(&s).unwrap(), RubyValue::Str(_)));
    }

    /// NUM2LONG's Float acceptance: truncation inside `[-2^63, 2^63)`.
    #[test]
    fn floats_truncate_at_integer_sites() {
        assert_eq!(to_index(&RubyValue::Float(2.9)).unwrap(), 2);
        assert_eq!(to_index(&RubyValue::Float(-2.9)).unwrap(), -2);
        assert_eq!(
            to_index(&RubyValue::Float(-9_223_372_036_854_775_808.0)).unwrap(),
            i64::MIN
        );
    }

    /// The `%g` rendering CRuby's float RangeError uses, oracle shapes.
    #[test]
    fn fmt_g_matches_cruby_error_rendering() {
        assert_eq!(fmt_g(1e300), "1e+300");
        assert_eq!(fmt_g(1.5e300), "1.5e+300");
        assert_eq!(fmt_g(2e19), "2e+19");
        assert_eq!(fmt_g(9.3e18), "9.3e+18");
        assert_eq!(fmt_g(1.797e308), "1.797e+308");
        assert_eq!(fmt_g(f64::NAN), "NaN");
        assert_eq!(fmt_g(f64::NEG_INFINITY), "-Inf");
    }
}
