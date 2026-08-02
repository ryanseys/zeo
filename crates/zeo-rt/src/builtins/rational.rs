//! `Rational` (CRuby rational.c) -- an always-reduced `num/den` pair with
//! `den > 0` (rational.c's `f_gcd` normalization), bignum-capable
//! components (Rationals are rare; uniform `BigInt` arithmetic beats a
//! small/big split here -- extraction via `#numerator`/`#denominator`
//! demotes through `int_value`). A standalone Rational NEVER auto-demotes
//! to Integer (`Rational(4, 2)` is `(2/1)`, `4.quo(2)` stays `(2/1)` --
//! oracle-verified); only Complex's internal component arithmetic demotes
//! (see `complex.rs`).

use crate::builtins::{arg_error, inherited_row, type_error};
use crate::{RubyValue, Signal};
use num_bigint::BigInt;
use num_integer::Integer as _;
use num_traits::{Signed, ToPrimitive, Zero};
use std::sync::Arc;
use zeo_macros::ruby_class;

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
    Ok(RubyValue::Rational(Arc::new(RRationalData {
        num: n,
        den: d,
    })))
}

/// `String#to_r`'s lenient parse: a leading `[sign] digits [/ digits]` or
/// `[sign] digits . digits` prefix as a `(num, den)` pair (unreduced --
/// `rational_new` reduces). Anything with no leading digits is `(0, 1)`,
/// matching CRuby's "never raises, junk tail ignored" contract.
pub fn parse_str_to_r(s: &str) -> (BigInt, BigInt) {
    let mut chars = s.trim_start().chars().peekable();
    let negative = match chars.peek() {
        Some('+') => {
            chars.next();
            false
        }
        Some('-') => {
            chars.next();
            true
        }
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
    let sign = if negative {
        BigInt::from(-1)
    } else {
        BigInt::from(1)
    };
    let (num, den) = match chars.peek() {
        Some('/') => {
            chars.next();
            let den_digits = take_digits(&mut chars);
            let num = int_part.parse::<BigInt>().unwrap_or_default();
            let den = den_digits
                .parse::<BigInt>()
                .unwrap_or_else(|_| BigInt::from(1));
            (num, den)
        }
        Some('.') => {
            chars.next();
            let frac = take_digits(&mut chars);
            let combined = format!("{int_part}{frac}")
                .parse::<BigInt>()
                .unwrap_or_default();
            let den = format!("1{}", "0".repeat(frac.len()))
                .parse::<BigInt>()
                .unwrap_or_else(|_| BigInt::from(1));
            (combined, den)
        }
        _ => (
            int_part.parse::<BigInt>().unwrap_or_default(),
            BigInt::from(1),
        ),
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
        other => panic!(
            "expected an exact numeric, got {}",
            other.to_display_string()
        ),
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

    /// `rationalize` is now a `ruby_class!` method (mangled fn name), so the
    /// test reaches it through Rational's registered instance lookup.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::RATIONAL_CLASS)
            .expect("Rational is registered")
            .instance
            .as_ref()
            .expect("Rational has instance methods")
            .lookup)(name)
        .unwrap_or_else(|| panic!("Rational#{name} is defined"))
    }

    fn rat(n: i64, d: i64) -> RubyValue {
        rational_new(BigInt::from(n), BigInt::from(d)).unwrap()
    }

    fn parts(v: &RubyValue) -> (i64, i64) {
        let RubyValue::Rational(r) = v else {
            panic!("not a Rational")
        };
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
        assert_eq!(
            parts(&rat_add(&rat(1, 2), &RubyValue::Int(1)).unwrap()),
            (3, 2)
        );
    }

    #[test]
    fn pow_stays_exact_for_integer_exponents() {
        assert_eq!(
            parts(&rat_pow(&rat(3, 4), &RubyValue::Int(2)).unwrap()),
            (9, 16)
        );
        assert_eq!(
            parts(&rat_pow(&rat(3, 4), &RubyValue::Int(-1)).unwrap()),
            (4, 3)
        );
        let r = rat_pow(&rat(4, 1), &RubyValue::Float(0.5)).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f == 2.0));
    }

    /// All shapes oracle-verified against ruby 4.0.6.
    #[test]
    fn rationalize_finds_the_simplest_rational_within_eps() {
        let go =
            |r: RubyValue, e: RubyValue| parts(&imethod("rationalize")(&r, &[e], None).unwrap());
        assert_eq!(go(rat(1, 3), rat(1, 10)), (1, 3));
        assert_eq!(go(rat(5000, 10001), rat(1, 100)), (1, 2));
        assert_eq!(go(rat(3, 4), rat(1, 10)), (2, 3));
        // Float eps reads exactly (dyadic), negative receivers negate through.
        assert_eq!(go(rat(22, 7), RubyValue::Float(0.01)), (22, 7));
        assert_eq!(go(rat(-1, 3), rat(1, 10)), (-1, 3));
        // Zero/absent eps answer self; a span past an integer picks ceil(a).
        assert_eq!(go(rat(1, 3), RubyValue::Int(0)), (1, 3));
        assert_eq!(
            parts(&imethod("rationalize")(&rat(1, 3), &[], None).unwrap()),
            (1, 3)
        );
        assert_eq!(go(rat(1, 3), RubyValue::Int(2)), (-1, 1));
        assert_eq!(go(rat(1, 3), rat(1, 2)), (0, 1));
    }

    #[test]
    fn cmp_is_exact_and_rendering_matches() {
        assert_eq!(rat_cmp(&rat(1, 2), &rat(2, 3)), -1);
        assert_eq!(rat_cmp(&rat(1, 2), &rat(2, 4)), 0);
        let RubyValue::Rational(r) = rat(-1, 2) else {
            panic!()
        };
        assert_eq!(rat_to_s(&r), "-1/2");
    }
}

use crate::builtins::numeric::num_op_row;

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
    HalfEven,
    HalfDown,
}

/// The optional `ndigits` precision argument (default 0).
fn precision_arg(ndigits: Option<&RubyValue>) -> Result<i64, Signal> {
    match ndigits {
        None => Ok(0),
        Some(RubyValue::Int(n)) => Ok(*n),
        // NOT an implicit-conversion site: CRuby's Rational rounding family
        // requires a literal Integer -- even a `to_int` duck raises
        // "not an integer" (oracle-verified).
        Some(_) => Err(type_error!("not an integer")),
    }
}

/// Split a trailing `half:` options Hash off `round`'s argument list, returning
/// the positional arguments and the tie-breaking mode it selects (`HalfUp` when
/// no `half:` is given). `#round`/`#floor`/`#ceil` take at most a precision, so
/// any trailing Hash is the keyword arguments.
fn half_kwarg(opts: Option<&RubyValue>) -> Result<RoundMode, Signal> {
    let Some(RubyValue::Hash(h)) = opts else {
        return Ok(RoundMode::HalfUp);
    };
    let half = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("half")));
    let mode = match &half {
        RubyValue::Nil => RoundMode::HalfUp,
        RubyValue::Symbol(s) => match s.name().as_str() {
            "up" => RoundMode::HalfUp,
            "even" => RoundMode::HalfEven,
            "down" => RoundMode::HalfDown,
            other => {
                return Err(arg_error!("invalid rounding mode: {other}"));
            }
        },
        other => {
            return Err(arg_error!(
                "invalid rounding mode: {}",
                other.inspect_string()
            ));
        }
    };
    Ok(mode)
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
        // Half toward zero: a tie truncates. sign * ((2|num| + den - 1) / (2 den)).
        RoundMode::HalfDown => {
            let m = (BigInt::from(2) * num.abs() + den - BigInt::from(1)) / (BigInt::from(2) * den);
            if num.is_negative() { -m } else { m }
        }
        // Banker's rounding: a tie goes to the even neighbor.
        RoundMode::HalfEven => {
            use num_traits::Zero;
            let two = BigInt::from(2);
            let abs = num.abs();
            let k = &abs / den; // floor magnitude
            let rem2 = &two * (&abs - &k * den); // 2 * fractional * den
            let m = match rem2.cmp(den) {
                std::cmp::Ordering::Less => k,
                std::cmp::Ordering::Greater => k + BigInt::from(1),
                std::cmp::Ordering::Equal => {
                    if (&k % &two).is_zero() {
                        k
                    } else {
                        k + BigInt::from(1)
                    }
                }
            };
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

/// `rationalize`'s epsilon as an exact non-negative `(num, den)` ratio:
/// exact numerics as themselves, a Float by its dyadic decomposition (a
/// non-finite Float keeps `Float#to_r`'s FloatDomainError). Anything else
/// gets CRuby's NoMethodError -- the C code's first touch is `f_abs(eps)`.
fn eps_ratio(v: &RubyValue) -> Result<(BigInt, BigInt), Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
            let (n, d) = as_ratio(v);
            Ok((n.abs(), d))
        }
        RubyValue::Float(f) => {
            if !f.is_finite() {
                return Err(crate::dispatch::raise_error(
                    "FloatDomainError",
                    RubyValue::Float(*f).to_display_string(),
                ));
            }
            let (n, d) = crate::builtins::float::float_exact_parts(*f);
            Ok((n.abs(), d))
        }
        other => Err(crate::dispatch::raise_error(
            "NoMethodError",
            format!(
                "undefined method 'abs' for an instance of {}",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// CRuby's `nurat_rationalize_internal` (rational.c): the continued-fraction
/// walk for the simplest rational `p/q` with `a <= p/q <= b`, run in exact
/// `(num, den)` arithmetic (`den > 0`, `a < b`).
fn simplest_ratio(mut a: (BigInt, BigInt), mut b: (BigInt, BigInt)) -> (BigInt, BigInt) {
    let (mut p0, mut p1) = (BigInt::from(0), BigInt::from(1));
    let (mut q0, mut q1) = (BigInt::from(1), BigInt::from(0));
    loop {
        let c = a.0.div_ceil(&a.1);
        if &c * &b.1 <= b.0 {
            return (&c * &p1 + &p0, &c * &q1 + &q0);
        }
        let k = &c - 1;
        let p2 = &k * &p1 + &p0;
        let q2 = &k * &q1 + &q0;
        // a' = 1/(b - k), b' = 1/(a - k) -- both differences are positive
        // (`k = ceil(a) - 1 < a <= b`), so the flips keep den > 0.
        let t = (b.1.clone(), &b.0 - &k * &b.1);
        b = (a.1.clone(), &a.0 - &k * &a.1);
        a = t;
        (p0, q0) = (p1, q1);
        (p1, q1) = (p2, q2);
    }
}

ruby_class! {
    Rational = zeo_abi::RATIONAL_CLASS < zeo_abi::NUMERIC_CLASS;

    def "+" (recv, other) { num_op_row!(other, recv, num_add, "+") }
    def "-" (recv, other) { num_op_row!(other, recv, num_sub, "-") }
    def "*" (recv, other) { num_op_row!(other, recv, num_mul, "*") }
    def "/" (recv, other) { num_op_row!(other, recv, num_div, "/") }
    def "%" | "modulo" (recv, other) { num_op_row!(other, recv, num_mod, "%") }
    def "**" (recv, other) { num_op_row!(other, recv, num_pow, "**") }
    def "-@" (recv) {
        let r = recv_rational(recv);
        rational_new(-r.num.clone(), r.den.clone())
    }
    def "+@" (recv) {
        Ok(recv.clone())
    }
    def "<=>" (recv, other) {
        Ok(match crate::builtins::numeric::num_cmp(recv, other) {
            Some(Some(c)) => RubyValue::Int(c),
            _ => RubyValue::Nil,
        })
    }
    def "==" (recv, other) {
        Ok(RubyValue::Bool(recv.rb_eq(other)))
    }
    def "abs" | "magnitude" (recv) {
        let r = recv_rational(recv);
        rational_new(r.num.abs(), r.den.clone())
    }
    def "numerator" (recv) {
        Ok(crate::builtins::integer::int_value(recv_rational(recv).num.clone()))
    }
    def "denominator" (recv) {
        Ok(crate::builtins::integer::int_value(recv_rational(recv).den.clone()))
    }
    def "to_f" (recv) {
        Ok(RubyValue::Float(rat_to_f64(recv_rational(recv))))
    }
    // Truncation toward zero (BigInt's `/` truncates).
    def "to_i" | "to_int" (recv) {
        let r = recv_rational(recv);
        Ok(crate::builtins::integer::int_value(&r.num / &r.den))
    }
    def "to_r" (recv) {
        Ok(recv.clone())
    }
    // `rationalize(eps)`: the simplest rational within `eps` of self
    // (CRuby's `nurat_rationalize`); the no-argument form is exact already
    // and answers self.
    def "rationalize"(recv, arg?) {
        let Some(eps) = arg else { return Ok(recv.clone()) };
        let (en, ed) = eps_ratio(eps)?;
        if en.is_zero() {
            return Ok(recv.clone());
        }
        // Negative receivers rationalize their absolute value and negate
        // the result (rational.c's negation dance).
        let r = recv_rational(recv);
        let neg = r.num.is_negative();
        let (sn, sd) = (r.num.abs(), r.den.clone());
        let a = (&sn * &ed - &en * &sd, &sd * &ed);
        let b = (&sn * &ed + &en * &sd, &sd * &ed);
        let (p, q) = simplest_ratio(a, b);
        rational_new(if neg { -p } else { p }, q)
    }
    // `floor`/`ceil`/`round`/`truncate` accept an optional precision: a
    // positive `ndigits` answers a Rational, zero/negative an Integer.
    def "floor"(recv, ndigits?) {
        round_with_precision(recv_rational(recv), precision_arg(ndigits)?, RoundMode::Floor)
    }
    def "ceil"(recv, ndigits?) {
        round_with_precision(recv_rational(recv), precision_arg(ndigits)?, RoundMode::Ceil)
    }
    def "truncate"(recv, ndigits?) {
        round_with_precision(recv_rational(recv), precision_arg(ndigits)?, RoundMode::Trunc)
    }
    def "round"(recv, ndigits?, **opts) {
        let mode = half_kwarg(opts)?;
        round_with_precision(recv_rational(recv), precision_arg(ndigits)?, mode)
    }
    // A Rational is always a finite value.
    def "finite?" (recv) {
        let _ = recv;
        Ok(RubyValue::Bool(true))
    }
    def "infinite?" (recv) {
        let _ = recv;
        Ok(RubyValue::Nil)
    }
    // `coerce(other)`: a Float partner pulls both operands to Float; any
    // other numeric promotes to Rational (`(3/2).coerce(2) == [(2/1), (3/2)]`).
    def "coerce" (recv, arg) {
        let pair = match arg {
            RubyValue::Float(f) => vec![
                RubyValue::Float(*f),
                RubyValue::Float(rat_to_f64(recv_rational(recv))),
            ],
            RubyValue::Int(_) | RubyValue::BigInt(_) => vec![
                rational_new(crate::builtins::integer::to_bigint(arg), 1.into())?,
                recv.clone(),
            ],
            RubyValue::Rational(_) => vec![(*arg).clone(), recv.clone()],
            other => {
                return Err(type_error!("{} can't be coerced into Rational",
                        crate::builtins::class_name_of(other)))
            }
        };
        Ok(RubyValue::Array(crate::array_new(pair)))
    }
    // `div` -- floored integer division (`Rational(7,2).div(2) == 1`).
    def "div" (recv, arg) {
        let q = crate::builtins::numeric::num_div(recv, arg)
            .ok_or_else(|| type_error!("{} can't be coerced into Rational",
                    crate::builtins::coerce_operand_name(arg)))??;
        match q {
            RubyValue::Rational(r) => {
                Ok(crate::builtins::integer::int_value(r.num.div_floor(&r.den)))
            }
            RubyValue::Float(f) => crate::builtins::float::float_to_integer(f.floor()),
            other => Ok(other),
        }
    }
    // `n.i` -- the pure-imaginary Complex `0 + n*i`.
    def "i" (recv) {
        crate::builtins::complex::complex_new(RubyValue::Int(0), recv.clone())
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
    def "inspect"(recv) { inherited_row!(kernel, "inspect", recv, __args, None) }
    // NOT an alias of `#inspect`: `Complex`, `Rational` and `Regexp` all
    // spell the two differently, so each goes to its own Kernel row.
    def "to_s"(recv) { inherited_row!(kernel, "to_s", recv, __args, None) }
    def "fdiv"(recv, _other) { inherited_row!(numeric, "fdiv", recv, __args, None) }
    def "quo"(recv, _other) { inherited_row!(numeric, "quo", recv, __args, None) }
    def "negative?"(recv) { inherited_row!(numeric, "negative?", recv, __args, None) }
    def "positive?"(recv) { inherited_row!(numeric, "positive?", recv, __args, None) }
}
