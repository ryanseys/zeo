//! `Rational` (CRuby rational.c) -- an always-reduced `num/den` pair with
//! `den > 0` (rational.c's `f_gcd` normalization), bignum-capable
//! components (Rationals are rare; uniform `BigInt` arithmetic beats a
//! small/big split here -- extraction via `#numerator`/`#denominator`
//! demotes through `int_value`). A standalone Rational NEVER auto-demotes
//! to Integer (`Rational(4, 2)` is `(2/1)`, `4.quo(2)` stays `(2/1)` --
//! oracle-verified); only Complex's internal component arithmetic demotes
//! (see `complex.rs`).

use crate::builtins::{arity, builtin_methods};
use crate::{RubyValue, Signal};
use num_bigint::BigInt;
use num_integer::Integer as _;
use num_traits::{Signed, ToPrimitive, Zero};
use std::sync::Arc;

pub struct RRationalData {
    pub num: BigInt,
    pub den: BigInt,
}

pub type RRational = Arc<RRationalData>;

/// THE Rational constructor: reduces via gcd, normalizes the sign onto the
/// numerator, and raises real Ruby's ZeroDivisionError for a zero
/// denominator.
pub fn rational_new(num: BigInt, den: BigInt) -> Result<RubyValue, Signal> {
    if den.is_zero() {
        return Err(crate::dispatch::raise_error(
            "ZeroDivisionError",
            "divided by 0".to_string(),
        ));
    }
    let g = num.gcd(&den);
    let (mut n, mut d) = (num / &g, den / g);
    if d.is_negative() {
        n = -n;
        d = -d;
    }
    Ok(RubyValue::Rational(Arc::new(RRationalData { num: n, den: d })))
}

/// `String#to_r`'s lenient parse: a leading `[sign] digits [/ digits]` or
/// `[sign] digits . digits` prefix as a `(num, den)` pair (unreduced --
/// `rational_new` reduces). Anything with no leading digits is `(0, 1)`,
/// matching CRuby's "never raises, junk tail ignored" contract.
pub fn parse_str_to_r(s: &str) -> (BigInt, BigInt) {
    let mut chars = s.trim_start().chars().peekable();
    let negative = match chars.peek() {
        Some('+') => { chars.next(); false }
        Some('-') => { chars.next(); true }
        _ => false,
    };
    let take_digits = |chars: &mut std::iter::Peekable<std::str::Chars>| {
        let mut d = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                d.push(c);
                chars.next();
            } else if c == '_' && !d.is_empty() {
                chars.next(); // Ruby allows digit-group underscores.
            } else {
                break;
            }
        }
        d
    };
    let int_part = take_digits(&mut chars);
    let sign = if negative { BigInt::from(-1) } else { BigInt::from(1) };
    let (num, den) = match chars.peek() {
        Some('/') => {
            chars.next();
            let den_digits = take_digits(&mut chars);
            let num = int_part.parse::<BigInt>().unwrap_or_default();
            let den = den_digits.parse::<BigInt>().unwrap_or_else(|_| BigInt::from(1));
            (num, den)
        }
        Some('.') => {
            chars.next();
            let frac = take_digits(&mut chars);
            let combined = format!("{int_part}{frac}").parse::<BigInt>().unwrap_or_default();
            let den = format!("1{}", "0".repeat(frac.len())).parse::<BigInt>().unwrap_or_else(|_| BigInt::from(1));
            (combined, den)
        }
        _ => (int_part.parse::<BigInt>().unwrap_or_default(), BigInt::from(1)),
    };
    (sign * num, den)
}

/// A `3r`/`1.5r` LITERAL (codegen's emission target) -- infallible: the
/// denominator is positive and non-zero by Ruby syntax, digits are prism's
/// LSB-first u32 shape.
pub fn rational_from_digits(negative: bool, num: &[u32], den: &[u32]) -> RubyValue {
    let sign = if negative {
        num_bigint::Sign::Minus
    } else {
        num_bigint::Sign::Plus
    };
    rational_new(
        BigInt::from_slice(sign, num),
        BigInt::from_slice(num_bigint::Sign::Plus, den),
    )
    .expect("a rational literal's denominator is non-zero by syntax")
}

/// The `(num, den)` view of any exact (Integer or Rational) value -- the
/// lane-lifting `numeric.rs`'s matrix uses (`Int n` lifts to `n/1`).
pub(crate) fn as_ratio(v: &RubyValue) -> (BigInt, BigInt) {
    match v {
        RubyValue::Int(i) => (BigInt::from(*i), BigInt::from(1)),
        RubyValue::BigInt(b) => ((**b).clone(), BigInt::from(1)),
        RubyValue::Rational(r) => (r.num.clone(), r.den.clone()),
        other => panic!("expected an exact numeric, got {}", other.to_display_string()),
    }
}

pub(crate) fn rat_add(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((an, ad), (bn, bd)) = (as_ratio(a), as_ratio(b));
    rational_new(&an * &bd + &bn * &ad, ad * bd)
}

pub(crate) fn rat_sub(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((an, ad), (bn, bd)) = (as_ratio(a), as_ratio(b));
    rational_new(&an * &bd - &bn * &ad, ad * bd)
}

pub(crate) fn rat_mul(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((an, ad), (bn, bd)) = (as_ratio(a), as_ratio(b));
    rational_new(an * bn, ad * bd)
}

pub(crate) fn rat_div(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    let ((an, ad), (bn, bd)) = (as_ratio(a), as_ratio(b));
    // Dividing by zero (bn == 0) falls out of rational_new's den check.
    rational_new(an * bd, ad * bn)
}

/// `rational ** integer` stays exact (a negative exponent inverts);
/// any other exponent shape computes in Float (real Ruby: `Rational(4,1)
/// ** 0.5` is `2.0`).
pub(crate) fn rat_pow(a: &RubyValue, b: &RubyValue) -> Result<RubyValue, Signal> {
    match b {
        RubyValue::Int(_) | RubyValue::BigInt(_) => {
            let (an, ad) = as_ratio(a);
            let e = crate::builtins::integer::to_bigint(b);
            let (base_n, base_d, e) = if e.is_negative() {
                (ad, an, -e)
            } else {
                (an, ad, e)
            };
            let Some(e) = e.to_u32() else {
                return Ok(RubyValue::Float(
                    crate::builtins::numeric::num_to_f64_unchecked(a)
                        .powf(crate::builtins::numeric::num_to_f64_unchecked(b)),
                ));
            };
            rational_new(
                num_traits::pow::Pow::pow(base_n, e),
                num_traits::pow::Pow::pow(base_d, e),
            )
        }
        _ => Ok(RubyValue::Float(
            crate::builtins::numeric::num_to_f64_unchecked(a)
                .powf(crate::builtins::numeric::num_to_f64_unchecked(b)),
        )),
    }
}

/// Exact ordering between two exact values: `a/b <=> c/d` is
/// `a*d <=> c*b` (both denominators positive by construction).
pub(crate) fn rat_cmp(a: &RubyValue, b: &RubyValue) -> i64 {
    let ((an, ad), (bn, bd)) = (as_ratio(a), as_ratio(b));
    match (an * bd).cmp(&(bn * ad)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// The `f64` view (`Rational#to_f`).
pub(crate) fn rat_to_f64(r: &RRationalData) -> f64 {
    r.num.to_f64().unwrap_or(f64::INFINITY) / r.den.to_f64().unwrap_or(f64::INFINITY)
}

/// `to_s` is `"3/4"`; `inspect` is `"(3/4)"` -- both oracle-verified.
pub(crate) fn rat_to_s(r: &RRationalData) -> String {
    format!("{}/{}", r.num, r.den)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rat(n: i64, d: i64) -> RubyValue {
        rational_new(BigInt::from(n), BigInt::from(d)).unwrap()
    }

    fn parts(v: &RubyValue) -> (i64, i64) {
        let RubyValue::Rational(r) = v else { panic!("not a Rational") };
        (r.num.to_i64().unwrap(), r.den.to_i64().unwrap())
    }

    #[test]
    fn construction_reduces_and_normalizes_sign() {
        assert_eq!(parts(&rat(4, 8)), (1, 2));
        assert_eq!(parts(&rat(1, -2)), (-1, 2));
        assert_eq!(parts(&rat(4, 2)), (2, 1)); // never demotes to Integer
        let r = std::panic::catch_unwind(|| rational_new(BigInt::from(1), BigInt::from(0)));
        assert!(r.is_err()); // ZeroDivisionError, panicking registry-less
    }

    #[test]
    fn arithmetic_matches_the_oracle() {
        // (1/2)+(1/3) == (5/6); (1/2)*(2/3) == (1/3); (1/2)/(3/4) == (2/3)
        assert_eq!(parts(&rat_add(&rat(1, 2), &rat(1, 3)).unwrap()), (5, 6));
        assert_eq!(parts(&rat_mul(&rat(1, 2), &rat(2, 3)).unwrap()), (1, 3));
        assert_eq!(parts(&rat_div(&rat(1, 2), &rat(3, 4)).unwrap()), (2, 3));
        // Int lane lifts: (1/2) + 1 == (3/2)
        assert_eq!(parts(&rat_add(&rat(1, 2), &RubyValue::Int(1)).unwrap()), (3, 2));
    }

    #[test]
    fn pow_stays_exact_for_integer_exponents() {
        assert_eq!(parts(&rat_pow(&rat(3, 4), &RubyValue::Int(2)).unwrap()), (9, 16));
        assert_eq!(parts(&rat_pow(&rat(3, 4), &RubyValue::Int(-1)).unwrap()), (4, 3));
        let r = rat_pow(&rat(4, 1), &RubyValue::Float(0.5)).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f == 2.0));
    }

    #[test]
    fn cmp_is_exact_and_rendering_matches() {
        assert_eq!(rat_cmp(&rat(1, 2), &rat(2, 3)), -1);
        assert_eq!(rat_cmp(&rat(1, 2), &rat(2, 4)), 0);
        let RubyValue::Rational(r) = rat(-1, 2) else { panic!() };
        assert_eq!(rat_to_s(&r), "-1/2");
    }
}

macro_rules! rat_op_row {
    ($args:ident, $recv:ident, $num_fn:ident, $op:literal) => {{
        arity!($args, 1);
        crate::builtins::numeric::num_coerce_bin(
            $recv,
            &$args[0],
            crate::builtins::numeric::$num_fn($recv, &$args[0]),
            $op,
        )
    }};
}

fn recv_rational(recv: &RubyValue) -> &RRationalData {
    match recv {
        RubyValue::Rational(r) => r,
        _ => unreachable!("Rational table row dispatched on a non-Rational receiver"),
    }
}

/// How `floor`/`ceil`/`round`/`truncate` break a fraction to an integer.
#[derive(Clone, Copy)]
enum RoundMode {
    Floor,
    Ceil,
    Trunc,
    HalfUp,
}

/// The optional `ndigits` precision argument (default 0).
fn precision_arg(args: &[RubyValue]) -> Result<i64, Signal> {
    match args.first() {
        None => Ok(0),
        Some(RubyValue::Int(n)) => Ok(*n),
        Some(other) => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// `num/den` reduced to an integer under `mode` (`den` is always positive here).
fn round_int(num: &BigInt, den: &BigInt, mode: RoundMode) -> BigInt {
    match mode {
        RoundMode::Floor => num.div_floor(den),
        RoundMode::Ceil => num.div_ceil(den),
        RoundMode::Trunc => num / den,
        // Half away from zero: sign * ((2|num| + den) / (2 den)).
        RoundMode::HalfUp => {
            let m = (BigInt::from(2) * num.abs() + den) / (BigInt::from(2) * den);
            if num.is_negative() { -m } else { m }
        }
    }
}

/// `round`/`floor`/`ceil`/`truncate` with an optional decimal precision: a
/// positive `n` scales by `10^n`, rounds, and answers a Rational; zero or a
/// negative `n` answers an Integer (a negative `n` rounds to the `10^|n|` place).
fn round_with_precision(r: &RRationalData, n: i64, mode: RoundMode) -> Result<RubyValue, Signal> {
    use crate::builtins::integer::int_value;
    if n == 0 {
        return Ok(int_value(round_int(&r.num, &r.den, mode)));
    }
    if n < 0 {
        let p = BigInt::from(10).pow((-n) as u32);
        let q = round_int(&r.num, &(&r.den * &p), mode);
        return Ok(int_value(q * p));
    }
    let p = BigInt::from(10).pow(n as u32);
    let q = round_int(&(&r.num * &p), &r.den, mode);
    rational_new(q, p)
}

builtin_methods! {
    pub(crate) fn lookup;

    "+" => fn add(recv, args, _block) { rat_op_row!(args, recv, num_add, "+") }
    "-" => fn sub(recv, args, _block) { rat_op_row!(args, recv, num_sub, "-") }
    "*" => fn mul(recv, args, _block) { rat_op_row!(args, recv, num_mul, "*") }
    "/" => fn div(recv, args, _block) { rat_op_row!(args, recv, num_div, "/") }
    "%" | "modulo" => fn modulo(recv, args, _block) { rat_op_row!(args, recv, num_mod, "%") }
    "**" => fn pow(recv, args, _block) { rat_op_row!(args, recv, num_pow, "**") }
    "-@" => fn neg(recv, args, _block) {
        arity!(args, 0);
        let r = recv_rational(recv);
        rational_new(-r.num.clone(), r.den.clone())
    }
    "+@" => fn pos(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "<=>" => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        Ok(match crate::builtins::numeric::num_cmp(recv, &args[0]) {
            Some(Some(c)) => RubyValue::Int(c),
            _ => RubyValue::Nil,
        })
    }
    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "abs" | "magnitude" => fn abs(recv, args, _block) {
        arity!(args, 0);
        let r = recv_rational(recv);
        rational_new(r.num.abs(), r.den.clone())
    }
    "numerator" => fn numerator(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::builtins::integer::int_value(recv_rational(recv).num.clone()))
    }
    "denominator" => fn denominator(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::builtins::integer::int_value(recv_rational(recv).den.clone()))
    }
    "to_f" => fn to_f(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(rat_to_f64(recv_rational(recv))))
    }
    // Truncation toward zero (BigInt's `/` truncates).
    "to_i" | "to_int" => fn to_i(recv, args, _block) {
        arity!(args, 0);
        let r = recv_rational(recv);
        Ok(crate::builtins::integer::int_value(&r.num / &r.den))
    }
    "to_r" | "rationalize" => fn to_r(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    // `floor`/`ceil`/`round`/`truncate` accept an optional precision: a
    // positive `ndigits` answers a Rational, zero/negative an Integer.
    "floor" => fn floor(recv, args, _block) {
        arity!(args, 0..=1);
        round_with_precision(recv_rational(recv), precision_arg(args)?, RoundMode::Floor)
    }
    "ceil" => fn ceil(recv, args, _block) {
        arity!(args, 0..=1);
        round_with_precision(recv_rational(recv), precision_arg(args)?, RoundMode::Ceil)
    }
    "truncate" => fn truncate(recv, args, _block) {
        arity!(args, 0..=1);
        round_with_precision(recv_rational(recv), precision_arg(args)?, RoundMode::Trunc)
    }
    "round" => fn round(recv, args, _block) {
        arity!(args, 0..=1);
        round_with_precision(recv_rational(recv), precision_arg(args)?, RoundMode::HalfUp)
    }
    // A Rational is always a finite value.
    "finite?" => fn finite_p(recv, args, _block) {
        arity!(args, 0);
        let _ = recv;
        Ok(RubyValue::Bool(true))
    }
    "infinite?" => fn infinite_p(recv, args, _block) {
        arity!(args, 0);
        let _ = recv;
        Ok(RubyValue::Nil)
    }
    // `coerce(other)`: a Float partner pulls both operands to Float; any
    // other numeric promotes to Rational (`(3/2).coerce(2) == [(2/1), (3/2)]`).
    "coerce" => fn coerce(recv, args, _block) {
        arity!(args, 1);
        let pair = match &args[0] {
            RubyValue::Float(f) => vec![
                RubyValue::Float(*f),
                RubyValue::Float(rat_to_f64(recv_rational(recv))),
            ],
            RubyValue::Int(_) | RubyValue::BigInt(_) => vec![
                rational_new(crate::builtins::integer::to_bigint(&args[0]), 1.into())?,
                recv.clone(),
            ],
            RubyValue::Rational(_) => vec![args[0].clone(), recv.clone()],
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "{} can't be coerced into Rational",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        Ok(RubyValue::Array(crate::array_new(pair)))
    }
    // `div` -- floored integer division (`Rational(7,2).div(2) == 1`).
    "div" => fn int_div(recv, args, _block) {
        arity!(args, 1);
        let q = crate::builtins::numeric::num_div(recv, &args[0])
            .ok_or_else(|| crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "{} can't be coerced into Rational",
                    crate::builtins::class_name_of(&args[0])
                ),
            ))??;
        match q {
            RubyValue::Rational(r) => {
                Ok(crate::builtins::integer::int_value(r.num.div_floor(&r.den)))
            }
            RubyValue::Float(f) => crate::builtins::float::float_to_integer(f.floor()),
            other => Ok(other),
        }
    }
    // `n.i` -- the pure-imaginary Complex `0 + n*i`.
    "i" => fn imaginary(recv, args, _block) {
        arity!(args, 0);
        crate::builtins::complex::complex_new(RubyValue::Int(0), recv.clone())
    }
}
