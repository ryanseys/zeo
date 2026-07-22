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
pub fn float_add(a: f64, b: f64) -> f64 {
    a + b
}
pub fn float_sub(a: f64, b: f64) -> f64 {
    a - b
}
pub fn float_mul(a: f64, b: f64) -> f64 {
    a * b
}
pub fn float_div(a: f64, b: f64) -> f64 {
    a / b
}

/// Ruby's `Float#%` takes the sign of the divisor (floored modulo), same
/// rule as `int_mod` -- unlike Rust's `%` (truncated remainder, C `fmod`
/// semantics).
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
pub fn float_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
}

/// `Float#**` with zeo's negative-base rule: a negative base to a fractional
/// (non-integer) power has no real result, so it raises Math::DomainError
/// loudly (CRuby promotes to Complex -- a documented divergence). A
/// whole-valued exponent (`(-2.0) ** 2.0`) still takes the ordinary power.
/// Mirrors `float_pow`'s use in `builtins::numeric::num_pow`.
pub fn float_pow_checked(a: f64, b: f64) -> Result<crate::RubyValue, crate::Signal> {
    if a < 0.0 && b.is_finite() && b.fract() != 0.0 {
        Err(crate::dispatch::raise_error(
            "Math::DomainError",
            "Numerical argument is out of domain".to_string(),
        ))
    } else {
        Ok(crate::RubyValue::Float(a.powf(b)))
    }
}
pub fn float_neg(a: f64) -> f64 {
    -a
}
pub fn float_pos(a: f64) -> f64 {
    a
}
pub fn float_eq(a: f64, b: f64) -> bool {
    a == b
}
pub fn float_neq(a: f64, b: f64) -> bool {
    a != b
}
pub fn float_lt(a: f64, b: f64) -> bool {
    a < b
}
pub fn float_gt(a: f64, b: f64) -> bool {
    a > b
}
pub fn float_le(a: f64, b: f64) -> bool {
    a <= b
}
pub fn float_ge(a: f64, b: f64) -> bool {
    a >= b
}

/// Mirrors `Float#<=>`: -1/0/1, or `nil` for a `NaN` comparison (real Ruby's
/// actual behavior -- `Float::NAN <=> 1.0` is `nil`, not an arbitrary
/// ordering). Returns `Option<i64>` (unlike `int_cmp`'s infallible `i64`)
/// for exactly this reason; `codegen::call` maps `None` to `RubyValue::Nil`.
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
