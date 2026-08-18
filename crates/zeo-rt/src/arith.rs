//! Native numeric helpers backing codegen's generalized numeric-operator
//! dispatch. The `Integer` family moved to `builtins::integer` with the
//! bignum migration (operands/results are `&RubyValue` now --
//! an `Int`-typed value may carry either payload); this module keeps the
//! plain `f64` family (`Float` stays a single-payload type) and re-exports
//! the Integer core so generated programs and codegen keep one flat
//! `zeo_rt::int_*` namespace.

pub use crate::builtins::integer::{
    int_add, int_band, int_bnot, int_bor, int_bxor, int_cmp, int_div, int_eq, int_from_u32_digits,
    int_ge, int_gt, int_is_zero, int_le, int_lt, int_mod, int_mul, int_neg, int_neq, int_pos,
    int_pow, int_shl, int_shr, int_sub, int_value,
};
pub use crate::builtins::numeric::{
    num_add, num_cmp, num_div, num_eq, num_mod, num_mul, num_pow, num_quo, num_sub,
    num_to_f64_unchecked,
};

/// Native `f64` arithmetic/comparison -- see `codegen::call`'s
/// `FLOAT_BINARY_OPS`/mixed-`Int`/`Float`-promotion table. No
/// overflow/`Bignum` concerns (`f64` saturates to `inf`, matching real
/// Ruby's own `Float` behavior exactly, unlike `Integer`'s
/// promote-on-overflow) -- these are plain, unchecked IEEE 754 operations.
#[inline]
pub fn float_add(a: f64, b: f64) -> f64 {
    a + b
}
#[inline]
pub fn float_sub(a: f64, b: f64) -> f64 {
    a - b
}
#[inline]
pub fn float_mul(a: f64, b: f64) -> f64 {
    a * b
}
#[inline]
pub fn float_div(a: f64, b: f64) -> f64 {
    a / b
}

/// Ruby's `Float#%` takes the sign of the divisor (floored modulo), same
/// rule as `int_mod` -- unlike Rust's `%` (truncated remainder, C `fmod`
/// semantics).
#[inline]
pub fn float_mod(a: f64, b: f64) -> f64 {
    let r = a % b;
    if r != 0.0 && (r < 0.0) != (b < 0.0) {
        r + b
    } else {
        r
    }
}

/// `Float#%` with CRuby's zero-divisor rule: `x % 0` (or `% 0.0`) raises
/// ZeroDivisionError rather than answering NaN. The static Float `%` codegen
/// emits this checked form (unlike `/`, whose zero divisor is Infinity, not an
/// error).
pub fn float_mod_checked(a: f64, b: f64) -> Result<crate::RubyValue, crate::Signal> {
    if b == 0.0 {
        Err(crate::dispatch::raise_error(
            "ZeroDivisionError",
            "divided by 0".to_string(),
        ))
    } else {
        Ok(crate::RubyValue::Float(float_mod(a, b)))
    }
}
#[inline]
pub fn float_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
}

/// `Float#**`: a negative base to a FRACTIONAL power has no real result, so
/// ruby leaves the reals and answers a Complex -- `rb_float_pow` ends in
/// `rb_dbl_complex_new_polar_pi(pow(-dx, dy), dy)`, whose modulus is the
/// positive base's power and whose argument is `dy * pi`. A whole-valued
/// exponent (`(-2.0) ** 2.0`) still takes the ordinary real power.
///
/// This used to raise `Math::DomainError` instead, which was a divergence
/// rather than a limit: the Complex tower it needed was already here.
pub fn float_pow_checked(a: f64, b: f64) -> Result<crate::RubyValue, crate::Signal> {
    if a < 0.0 && b.is_finite() && b.fract() != 0.0 {
        return dbl_complex_polar_pi((-a).powf(b), b);
    }
    Ok(crate::RubyValue::Float(a.powf(b)))
}

// The half-turn trig `rb_dbl_complex_new_polar_pi` uses, whose argument is
// measured in HALF TURNS rather than radians. Darwin's libm has them, which
// is exactly the `#ifdef` CRuby takes (complex.c) -- and the reason its
// results are clean where `cos(x * PI)` leaves 1e-17 dust.
#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    #[link_name = "__cospi"]
    fn c_cospi(x: f64) -> f64;
    #[link_name = "__sinpi"]
    fn c_sinpi(x: f64) -> f64;
}
#[cfg(target_vendor = "apple")]
fn cospi(x: f64) -> f64 {
    unsafe { c_cospi(x) }
}
#[cfg(target_vendor = "apple")]
fn sinpi(x: f64) -> f64 {
    unsafe { c_sinpi(x) }
}
#[cfg(not(target_vendor = "apple"))]
fn cospi(x: f64) -> f64 {
    (x * std::f64::consts::PI).cos()
}
#[cfg(not(target_vendor = "apple"))]
fn sinpi(x: f64) -> f64 {
    (x * std::f64::consts::PI).sin()
}

/// `rb_dbl_complex_new_polar_pi`: a polar constructor whose ANGLE is in half
/// turns, so the quarter turns land EXACTLY. A half-integer angle is purely
/// imaginary and an integer angle purely real -- taken as special cases rather
/// than computed, which is what keeps `(-2.0) ** 0.5` at a clean `0.0` real
/// part instead of 8.66e-17.
fn dbl_complex_polar_pi(abs: f64, ang: f64) -> Result<crate::RubyValue, crate::Signal> {
    let mut abs = abs;
    let fi = ang.trunc();
    let fr = ang - fi;
    // `frac(fi / 2)`: whether the integer half-turn count is odd, which is
    // what decides the sign.
    let half = fi / 2.0;
    let half_fr = half - half.trunc();
    let pos = fr == 0.5;
    if pos || fr == -0.5 {
        if (half_fr != fr) ^ pos {
            abs = -abs;
        }
        return crate::builtins::complex::complex_new(
            crate::RubyValue::Float(0.0),
            crate::RubyValue::Float(abs),
        );
    }
    if fr == 0.0 {
        if half_fr != 0.0 {
            abs = -abs;
        }
        return Ok(crate::RubyValue::Float(abs));
    }
    crate::builtins::complex::complex_new(
        crate::RubyValue::Float(abs * cospi(ang)),
        crate::RubyValue::Float(abs * sinpi(ang)),
    )
}
#[inline]
pub fn float_neg(a: f64) -> f64 {
    -a
}
#[inline]
pub fn float_pos(a: f64) -> f64 {
    a
}
#[inline]
pub fn float_eq(a: f64, b: f64) -> bool {
    a == b
}
#[inline]
pub fn float_neq(a: f64, b: f64) -> bool {
    a != b
}
#[inline]
pub fn float_lt(a: f64, b: f64) -> bool {
    a < b
}
#[inline]
pub fn float_gt(a: f64, b: f64) -> bool {
    a > b
}
#[inline]
pub fn float_le(a: f64, b: f64) -> bool {
    a <= b
}
#[inline]
pub fn float_ge(a: f64, b: f64) -> bool {
    a >= b
}

/// Mirrors `Float#<=>`: -1/0/1, or `nil` for a `NaN` comparison (real Ruby's
/// actual behavior -- `Float::NAN <=> 1.0` is `nil`, not an arbitrary
/// ordering). Returns `Option<i64>` (unlike `int_cmp`'s infallible `i64`)
/// for exactly this reason; `codegen::call` maps `None` to `RubyValue::Nil`.
#[inline]
pub fn float_cmp(a: f64, b: f64) -> Option<i64> {
    a.partial_cmp(&b).map(|o| match o {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_mod_takes_the_divisor_sign() {
        assert_eq!(float_mod(-7.0, 3.0), 2.0);
        assert_eq!(float_mod(7.0, -3.0), -2.0);
    }

    #[test]
    fn float_cmp_nils_out_on_nan() {
        assert_eq!(float_cmp(1.0, 2.0), Some(-1));
        assert_eq!(float_cmp(f64::NAN, 2.0), None);
    }
}
