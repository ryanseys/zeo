//! `Float` (CRuby numeric.c) -- the operator/comparison rows, each driving
//! the one tower matrix in `numeric.rs` (a Float receiver joins any real
//! operand in the Float lane; `Complex` operands lift higher). Ordering
//! (`<`/`>`/...) comes from Comparable driving `<=>` -- Float's chain runs
//! `[Float, Numeric, Comparable, ...]`. The Tier A breadth
//! (`nan?`/`round(n)`/`to_r`/...) lands with stage C's generics pass.

use crate::builtins::{arity, builtin_methods};
use crate::{RubyValue, Signal};

/// CRuby's coercion TypeError shape (`1.0 + "x"` -> `String can't be
/// coerced into Float`).
fn coerce_error(arg: &RubyValue) -> Signal {
    crate::dispatch::raise_error(
        "TypeError",
        format!(
            "{} can't be coerced into Float",
            crate::builtins::class_name_of(arg)
        ),
    )
}

macro_rules! float_op_row {
    ($args:ident, $recv:ident, $num_fn:ident) => {{
        arity!($args, 1);
        match crate::builtins::numeric::$num_fn($recv, &$args[0]) {
            Some(r) => r,
            None => Err(coerce_error(&$args[0])),
        }
    }};
}

builtin_methods! {
    pub(crate) fn lookup;

    "+" => fn add(recv, args, _block) { float_op_row!(args, recv, num_add) }
    "-" => fn sub(recv, args, _block) { float_op_row!(args, recv, num_sub) }
    "*" => fn mul(recv, args, _block) { float_op_row!(args, recv, num_mul) }
    "/" => fn div(recv, args, _block) { float_op_row!(args, recv, num_div) }
    "%" | "modulo" => fn modulo(recv, args, _block) { float_op_row!(args, recv, num_mod) }
    "**" => fn pow(recv, args, _block) { float_op_row!(args, recv, num_pow) }
    "-@" => fn neg(recv, args, _block) {
        arity!(args, 0);
        match recv {
            RubyValue::Float(f) => Ok(RubyValue::Float(-f)),
            _ => unreachable!("Float table row dispatched on a non-Float receiver"),
        }
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
        Ok(RubyValue::Float(recv_f64(recv).abs()))
    }
    "nan?" => fn nan_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_f64(recv).is_nan()))
    }
    "finite?" => fn finite_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_f64(recv).is_finite()))
    }
    // 1 / -1 / nil, real Ruby's exact shape.
    "infinite?" => fn infinite_p(recv, args, _block) {
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
    "to_f" => fn to_f(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "to_i" | "to_int" => fn to_i(recv, args, _block) {
        arity!(args, 0);
        float_to_integer(recv_f64(recv).trunc())
    }
    // EXACT: every finite double is a dyadic rational (mantissa * 2^exp).
    "to_r" => fn to_r(recv, args, _block) {
        arity!(args, 0);
        float_to_rational(recv_f64(recv))
    }
    "numerator" => fn numerator(recv, args, _block) {
        arity!(args, 0);
        match float_to_rational(recv_f64(recv))? {
            RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(r.num.clone())),
            other => Ok(other),
        }
    }
    "denominator" => fn denominator(recv, args, _block) {
        arity!(args, 0);
        match float_to_rational(recv_f64(recv))? {
            RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(r.den.clone())),
            _ => Ok(RubyValue::Int(1)),
        }
    }
    // The rounding family: ndigits <= 0 produces an Integer, > 0 a Float.
    // Naive power-of-ten scaling -- CRuby switches to exact rational
    // arithmetic when double precision is insufficient (`2.675.round(2)`),
    // a documented divergence.
    "round" => fn round(recv, args, _block) {
        float_round_family(recv, args, f64::round)
    }
    "floor" => fn floor(recv, args, _block) {
        float_round_family(recv, args, f64::floor)
    }
    "ceil" => fn ceil(recv, args, _block) {
        float_round_family(recv, args, f64::ceil)
    }
    "truncate" => fn truncate(recv, args, _block) {
        float_round_family(recv, args, f64::trunc)
    }
}

fn recv_f64(recv: &RubyValue) -> f64 {
    match recv {
        RubyValue::Float(f) => *f,
        _ => unreachable!("Float table row dispatched on a non-Float receiver"),
    }
}

/// A finite float's integral value as an Integer (bignum-capable:
/// `1e20.to_i` works); non-finite raises real Ruby's FloatDomainError.
fn float_to_integer(f: f64) -> Result<RubyValue, Signal> {
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

/// `Float#to_r`'s exact dyadic decomposition: f == mantissa * 2^exp.
fn float_to_rational(f: f64) -> Result<RubyValue, Signal> {
    use num_bigint::BigInt;
    if !f.is_finite() {
        return Err(crate::dispatch::raise_error(
            "FloatDomainError",
            RubyValue::Float(f).to_display_string(),
        ));
    }
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
        crate::builtins::rational::rational_new(num << exp as usize, BigInt::from(1))
    } else {
        crate::builtins::rational::rational_new(num, BigInt::from(1) << (-exp) as usize)
    }
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
        let scale = 10f64.powi(ndigits as i32);
        return Ok(RubyValue::Float(op(f * scale) / scale));
    }
    if ndigits == 0 {
        return float_to_integer(op(f));
    }
    let scale = 10f64.powi((-ndigits) as i32);
    float_to_integer(op(f / scale) * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_compares_as_nil() {
        let r = spaceship(&RubyValue::Float(f64::NAN), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(r, RubyValue::Nil));
    }

    #[test]
    fn arithmetic_rows_join_the_float_lane() {
        let r = add(&RubyValue::Float(1.5), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f == 2.5));
        let r = div(&RubyValue::Float(1.0), &[RubyValue::Int(0)], None).unwrap();
        assert!(matches!(r, RubyValue::Float(f) if f.is_infinite()));
    }

    #[test]
    fn coercion_failures_carry_cruby_shape() {
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            add(&RubyValue::Float(1.0), &[s], None)
        }));
        assert!(r.is_err());
    }
}
