//! `Float` (CRuby numeric.c) -- the operator/comparison rows, each driving
//! the one tower matrix in `numeric.rs` (a Float receiver joins any real
//! operand in the Float lane; `Complex` operands lift higher). Ordering
//! (`<`/`>`/...) comes from Comparable driving `<=>` -- Float's chain runs
//! `[Float, Numeric, Comparable, ...]`. The Tier A breadth
//! (`nan?`/`round(n)`/`to_r`/...) lands with stage C's generics pass.

use crate::builtins::{arg_error, arity, type_error};
use crate::{RubyValue, Signal};
use zeo_macros::ruby_class;

/// CRuby's coercion TypeError shape (`1.0 + "x"` -> `String can't be
/// coerced into Float`).
fn coerce_error(arg: &RubyValue) -> Signal {
    type_error!(
        "{} can't be coerced into Float",
        crate::builtins::class_name_of(arg)
    )
}

use crate::builtins::numeric::num_op_row;

ruby_class! {
    Float = zeo_abi::FLOAT_CLASS < zeo_abi::NUMERIC_CLASS;

    // The IEEE-754 double constants, colocated here (formerly seeded from
    // numeric.rs). Values are CRuby's exactly.
    const INFINITY = RubyValue::Float(f64::INFINITY);
    const NAN = RubyValue::Float(f64::NAN);
    const EPSILON = RubyValue::Float(f64::EPSILON);
    const MAX = RubyValue::Float(f64::MAX);
    const MIN = RubyValue::Float(f64::MIN_POSITIVE);
    const DIG = RubyValue::Int(15);
    const MANT_DIG = RubyValue::Int(53);
    const MAX_EXP = RubyValue::Int(1024);
    const MIN_EXP = RubyValue::Int(-1021);
    const MAX_10_EXP = RubyValue::Int(308);
    const MIN_10_EXP = RubyValue::Int(-307);
    const RADIX = RubyValue::Int(2);

    def "+" arity 1 (recv, args, _block) { num_op_row!(args, recv, num_add, "+") }
    def "-" arity 1 (recv, args, _block) { num_op_row!(args, recv, num_sub, "-") }
    def "*" arity 1 (recv, args, _block) { num_op_row!(args, recv, num_mul, "*") }
    def "/" arity 1 (recv, args, _block) { num_op_row!(args, recv, num_div, "/") }
    def "%" arity 1 | "modulo" arity 1 (recv, args, _block) { num_op_row!(args, recv, num_mod, "%") }
    def "**" arity 1 (recv, args, _block) { num_op_row!(args, recv, num_pow, "**") }
    def "-@" arity 0 (recv, args, _block) {
        arity!(args, 0);
        match recv {
            RubyValue::Float(f) => Ok(RubyValue::Float(-f)),
            _ => unreachable!("Float table row dispatched on a non-Float receiver"),
        }
    }
    def "+@" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    def "<=>" arity 1 (recv, args, _block) {
        arity!(args, 1);
        Ok(match crate::builtins::numeric::num_cmp(recv, &args[0]) {
            Some(Some(c)) => RubyValue::Int(c),
            _ => RubyValue::Nil,
        })
    }
    def "==" arity 1 (recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    def "abs" arity 0 | "magnitude" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(recv_f64(recv).abs()))
    }
    def "nan?" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_f64(recv).is_nan()))
    }
    def "finite?" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_f64(recv).is_finite()))
    }
    // 1 / -1 / nil, real Ruby's exact shape.
    def "infinite?" arity 0 (recv, args, _block) {
        arity!(args, 0);
        let f = recv_f64(recv);
        Ok(if f == f64::INFINITY {
            RubyValue::Int(1)
        } else if f == f64::NEG_INFINITY {
            RubyValue::Int(-1)
        } else {
            RubyValue::Nil
        })
    }
    // The adjacent representable doubles toward +/-infinity.
    def "next_float" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(recv_f64(recv).next_up()))
    }
    def "prev_float" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(recv_f64(recv).next_down()))
    }
    // `coerce(other)` promotes both operands to Float (`[Float(other), self]`).
    def "coerce" arity 1 (recv, args, _block) {
        arity!(args, 1);
        let other = numeric_f64_arg(&args[0], "can't coerce")?;
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Float(other),
            recv.clone(),
        ])))
    }
    // `div` -- floored division returning an Integer (`7.0.div(2) == 3`).
    def "div" arity 1 (recv, args, _block) {
        arity!(args, 1);
        let d = numeric_f64_arg(&args[0], "can't coerce")?;
        float_to_integer((recv_f64(recv) / d).floor())
    }
    // `n.i` -- the pure-imaginary Complex `0 + n*i`.
    def "i" arity 0 (recv, args, _block) {
        arity!(args, 0);
        crate::builtins::complex::complex_new(RubyValue::Int(0), recv.clone())
    }
    def "to_f" arity 0 (recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    def "to_i" arity 0 | "to_int" arity 0 (recv, args, _block) {
        arity!(args, 0);
        float_to_integer(recv_f64(recv).trunc())
    }
    // EXACT: every finite double is a dyadic rational (mantissa * 2^exp).
    def "to_r" arity 0 (recv, args, _block) {
        arity!(args, 0);
        float_to_rational(recv_f64(recv))
    }
    // `rationalize([eps])` -- the SIMPLEST rational within half a ULP of this
    // double (no arg), or within `eps` (with arg). Port of CRuby's
    // `float_rationalize` (numeric.c) + `nurat_rationalize_internal`.
    def "rationalize" (recv, args, _block) {
        arity!(args, 0..=1);
        float_rationalize(recv_f64(recv), args.first())
    }
    def "numerator" arity 0 (recv, args, _block) {
        arity!(args, 0);
        let f = recv_f64(recv);
        // Infinity/NaN have no rational form, so CRuby skips the conversion
        // and returns the float itself (its denominator is 1) rather than
        // raising FloatDomainError.
        if !f.is_finite() {
            return Ok(RubyValue::Float(f));
        }
        match float_to_rational(f)? {
            RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(r.num.clone())),
            other => Ok(other),
        }
    }
    def "denominator" arity 0 (recv, args, _block) {
        arity!(args, 0);
        let f = recv_f64(recv);
        if !f.is_finite() {
            return Ok(RubyValue::Int(1));
        }
        match float_to_rational(f)? {
            RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(r.den.clone())),
            _ => Ok(RubyValue::Int(1)),
        }
    }
    // The rounding family: ndigits <= 0 produces an Integer, > 0 a Float.
    // Naive power-of-ten scaling -- CRuby switches to exact rational
    // arithmetic when double precision is insufficient (`2.675.round(2)`),
    // a documented divergence.
    def "round" (recv, args, _block) {
        // A trailing `half:` keyword selects the tie-break mode (:up default).
        let (positional, mode) = split_round_half(args)?;
        float_round_family(recv, positional, move |x| round_half(x, mode))
    }
    def "floor" (recv, args, _block) {
        float_round_family(recv, args, f64::floor)
    }
    def "ceil" (recv, args, _block) {
        float_round_family(recv, args, f64::ceil)
    }
    def "truncate" (recv, args, _block) {
        float_round_family(recv, args, f64::trunc)
    }
}

fn recv_f64(recv: &RubyValue) -> f64 {
    match recv {
        RubyValue::Float(f) => *f,
        _ => unreachable!("Float table row dispatched on a non-Float receiver"),
    }
}

/// A numeric argument as `f64` for `coerce`/`div`; a non-numeric argument is
/// a TypeError with the given verb (`can't coerce X into Float`).
fn numeric_f64_arg(v: &RubyValue, verb: &str) -> Result<f64, Signal> {
    use crate::builtins::numeric::num_to_f64_unchecked;
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(num_to_f64_unchecked(v))
        }
        // A real-valued Complex (imaginary part zero) coerces to its real part;
        // a non-real one can't become a Float.
        RubyValue::Complex(c) if num_to_f64_unchecked(&c.imag) == 0.0 => {
            Ok(num_to_f64_unchecked(&c.real))
        }
        other => Err(type_error!(
            "{verb} {} into Float",
            crate::builtins::class_name_of(other)
        )),
    }
}

/// A finite float's integral value as an Integer (bignum-capable:
/// `1e20.to_i` works); non-finite raises real Ruby's FloatDomainError.
pub(crate) fn float_to_integer(f: f64) -> Result<RubyValue, Signal> {
    use num_traits::FromPrimitive;
    if !f.is_finite() {
        return Err(crate::dispatch::raise_error(
            "FloatDomainError",
            RubyValue::Float(f).to_display_string(),
        ));
    }
    Ok(crate::builtins::integer::int_value(
        num_bigint::BigInt::from_f64(f).expect("finite float"),
    ))
}

/// A finite double's EXACT value as `(num, den)`, `den` a power of two --
/// the dyadic decomposition `f == mantissa * 2^exp` every exact reading of a
/// Float goes through. Shared with `Time`, whose sub-second fields are
/// oracle-exact rather than recomputed in floating point (`Time.at(0.7).nsec`
/// is 699999999, because that IS what the double holds; doing the arithmetic
/// in f64 rounds it back to 700000000 and hides the very thing Ruby shows).
pub(crate) fn float_exact_parts(f: f64) -> (num_bigint::BigInt, num_bigint::BigInt) {
    use num_bigint::BigInt;
    let bits = f.to_bits();
    let sign: i64 = if bits >> 63 == 0 { 1 } else { -1 };
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    let mantissa = if exponent == 0 {
        (bits & 0xf_ffff_ffff_ffff) << 1
    } else {
        (bits & 0xf_ffff_ffff_ffff) | 0x10_0000_0000_0000
    };
    let exp = exponent - 1075;
    let num = BigInt::from(sign) * BigInt::from(mantissa);
    if exp >= 0 {
        (num << exp as usize, BigInt::from(1))
    } else {
        (num, BigInt::from(1) << (-exp) as usize)
    }
}

/// `Float#to_r`'s exact dyadic decomposition: f == mantissa * 2^exp.
fn float_to_rational(f: f64) -> Result<RubyValue, Signal> {
    if !f.is_finite() {
        return Err(crate::dispatch::raise_error(
            "FloatDomainError",
            RubyValue::Float(f).to_display_string(),
        ));
    }
    let (num, den) = float_exact_parts(f);
    crate::builtins::rational::rational_new(num, den)
}

/// The `(significand, exponent)` of `|d|` such that `|d| == significand *
/// 2^exp` -- the `frexp`/`ldexp` decomposition CRuby's `float_decode_internal`
/// produces (`significand` is the raw 53-bit mantissa, sign dropped).
fn frexp_parts(ad: f64) -> (num_bigint::BigInt, i64) {
    use num_bigint::BigInt;
    let bits = ad.to_bits();
    let exp_field = ((bits >> 52) & 0x7ff) as i64;
    let mant = bits & 0xf_ffff_ffff_ffff;
    if exp_field == 0 {
        // Subnormal: no implicit leading bit, fixed exponent.
        (BigInt::from(mant), -1074)
    } else {
        (BigInt::from(mant | 0x10_0000_0000_0000), exp_field - 1075)
    }
}

/// The simplest rational `p/q` in the CLOSED interval `[a, b]` (`a <= b`, each
/// a `(num, den)` pair with `den > 0`) -- CRuby's `nurat_rationalize_internal`
/// continued-fraction (Stern-Brocot) search.
fn simplest_between(
    mut a: (num_bigint::BigInt, num_bigint::BigInt),
    mut b: (num_bigint::BigInt, num_bigint::BigInt),
) -> (num_bigint::BigInt, num_bigint::BigInt) {
    use num_bigint::BigInt;
    use num_integer::Integer as _;
    use num_traits::{One, Zero};
    // `d/n` normalized to a positive denominator.
    fn recip(n: BigInt, d: BigInt) -> (BigInt, BigInt) {
        if n.sign() == num_bigint::Sign::Minus {
            (-d, -n)
        } else {
            (d, n)
        }
    }
    let (mut p0, mut p1) = (BigInt::zero(), BigInt::one());
    let (mut q0, mut q1) = (BigInt::one(), BigInt::zero());
    loop {
        let c = a.0.div_ceil(&a.1);
        // Break once `ceil(a) < b`; the result uses this `c`.
        if &c * &b.1 < b.0 {
            return (&c * &p1 + &p0, &c * &q1 + &q0);
        }
        let k = &c - 1;
        let p2 = &k * &p1 + &p0;
        let q2 = &k * &q1 + &q0;
        let t = recip(&b.0 - &k * &b.1, b.1.clone());
        b = recip(&a.0 - &k * &a.1, a.1.clone());
        a = t;
        (p0, q0, p1, q1) = (p1, q1, p2, q2);
    }
}

/// `|eps|` as an exact `(num, den)` rational (`den > 0`), for the eps form.
fn exact_abs_rational(v: &RubyValue) -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
    use num_bigint::BigInt;
    use num_traits::{One, Signed};
    Ok(match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) => {
            (crate::builtins::integer::to_bigint(v).abs(), BigInt::one())
        }
        RubyValue::Float(f) => float_exact_parts(f.abs()),
        RubyValue::Rational(r) => (r.num.abs(), r.den.clone()),
        other => {
            return Err(type_error!(
                "can't convert {} into Float",
                crate::builtins::convert_name_of(other)
            ));
        }
    })
}

/// The full `Float#rationalize` body.
fn float_rationalize(d: f64, eps: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    use num_bigint::BigInt;
    use num_traits::{One, Zero};
    if !d.is_finite() {
        return Err(crate::dispatch::raise_error(
            "FloatDomainError",
            RubyValue::Float(d).to_display_string(),
        ));
    }
    let neg = d < 0.0;
    let ad = d.abs();
    let (p, q) = match eps {
        // `[|d| - |eps|, |d| + |eps|]` over exact rationals.
        Some(e) => {
            let (fn_, fd) = float_exact_parts(ad);
            let (en, ed) = exact_abs_rational(e)?;
            // a = f - e, b = f + e over the common denominator fd*ed.
            let den = &fd * &ed;
            let lo = &fn_ * &ed - &en * &fd;
            let hi = &fn_ * &ed + &en * &fd;
            if lo == hi {
                // eps == 0: fall back to the exact dyadic value.
                float_exact_parts(ad)
            } else {
                simplest_between((lo, den.clone()), (hi, den))
            }
        }
        // Half-ULP interval `[(2f-1)/2^(1-n), (2f+1)/2^(1-n)]`.
        None => {
            let (f, n) = frexp_parts(ad);
            if f.is_zero() || n >= 0 {
                let val = if n >= 0 {
                    f << (n as usize)
                } else {
                    BigInt::zero()
                };
                (val, BigInt::one())
            } else {
                let two_f = &f * 2;
                let den = BigInt::one() << ((1 - n) as usize);
                simplest_between((&two_f - 1, den.clone()), (&two_f + 1, den))
            }
        }
    };
    crate::builtins::rational::rational_new(if neg { -p } else { p }, q)
}

/// `Float#round`'s tie-break mode (the `half:` keyword).
#[derive(Clone, Copy)]
enum HalfMode {
    Up,
    Down,
    Even,
}

/// Round `x` to the nearest integer, breaking an exact `.5` tie per `mode`
/// (`:up` = away from zero, `:down` = toward zero, `:even` = banker's).
fn round_half(x: f64, mode: HalfMode) -> f64 {
    let fl = x.floor();
    let diff = x - fl;
    if diff < 0.5 {
        fl
    } else if diff > 0.5 {
        fl + 1.0
    } else {
        match mode {
            HalfMode::Up => {
                if x >= 0.0 {
                    fl + 1.0
                } else {
                    fl
                }
            }
            HalfMode::Down => {
                if x >= 0.0 {
                    fl
                } else {
                    fl + 1.0
                }
            }
            HalfMode::Even => {
                if (fl as i64) % 2 == 0 {
                    fl
                } else {
                    fl + 1.0
                }
            }
        }
    }
}

/// Split a trailing `half:` keyword Hash off `round`'s arguments, returning the
/// positional slice and the selected mode (`:up` when absent).
fn split_round_half(args: &[RubyValue]) -> Result<(&[RubyValue], HalfMode), Signal> {
    if let Some(RubyValue::Hash(h)) = args.last() {
        let mode = match crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("half"))) {
            RubyValue::Nil => HalfMode::Up,
            RubyValue::Symbol(s) => match s.name().as_str() {
                "up" => HalfMode::Up,
                "down" => HalfMode::Down,
                "even" => HalfMode::Even,
                other => {
                    return Err(arg_error!("invalid rounding mode: {other}"));
                }
            },
            other => {
                return Err(arg_error!(
                    "invalid rounding mode: {}",
                    other.to_display_string()
                ));
            }
        };
        return Ok((&args[..args.len() - 1], mode));
    }
    Ok((args, HalfMode::Up))
}

fn float_round_family(
    recv: &RubyValue,
    args: &[RubyValue],
    op: impl Fn(f64) -> f64,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let f = recv_f64(recv);
    let ndigits = match args.first() {
        Some(RubyValue::Int(n)) => *n,
        Some(other) => return Err(coerce_error(other)),
        None => 0,
    };
    if ndigits > 0 {
        // A very large `ndigits` overflows the `10^n` scale to infinity; scaling
        // then rounding then unscaling would be `op(Inf)/Inf == NaN`. But asking
        // for more fractional digits than a Float carries leaves the value
        // unchanged, so answer `f` directly rather than the NaN.
        let scale = 10f64.powi(ndigits.min(1024) as i32);
        if !scale.is_finite() || !(f * scale).is_finite() {
            return Ok(RubyValue::Float(f));
        }
        return Ok(RubyValue::Float(op(f * scale) / scale));
    }
    if ndigits == 0 {
        return float_to_integer(op(f));
    }
    // Negative `ndigits`: round to the `10^|n|` place. A very large `|n|`
    // overflows the scale; the place then dwarfs the value, so the result is 0
    // (the `0.0 * Inf == NaN` the naive path would produce is the bug).
    let scale = 10f64.powi((-ndigits).min(1024) as i32);
    if !scale.is_finite() {
        return float_to_integer(0.0);
    }
    float_to_integer(op(f / scale) * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Float rows are `ruby_class!`-generated (mangled Rust fn names), so
    /// reach them the way dispatch does -- through the registered table.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::FLOAT_CLASS)
            .expect("Float is a registered builtin table")
            .instance
            .as_ref()
            .expect("Float has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Float#{name} is defined"))
    }

    #[test]
    fn nan_compares_as_nil() {
        let r = imethod("<=>")(&RubyValue::Float(f64::NAN), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(r, RubyValue::Nil));
    }

    #[test]
    fn arithmetic_rows_join_the_float_lane() {
        let r = imethod("+")(&RubyValue::Float(1.5), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f == 2.5));
        let r = imethod("/")(&RubyValue::Float(1.0), &[RubyValue::Int(0)], None).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f.is_infinite()));
    }

    #[test]
    fn coercion_failures_carry_cruby_shape() {
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            imethod("+")(&RubyValue::Float(1.0), &[s], None)
        }));
        assert!(r.is_err());
    }
}
