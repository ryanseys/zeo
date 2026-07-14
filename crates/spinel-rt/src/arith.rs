//! Native `i64` arithmetic/comparison/bitwise helpers backing codegen's
//! generalized numeric-operator dispatch (see `docs/PORTING_ANALYSIS.md`'s
//! Phase 1: any operand statically typed `Int` -- not just a literal-on-
//! literal pair -- routes through one of these). Mirrors spinel's
//! `sp_int_*` family (`lib/sp_runtime.h:140-242`): the default
//! `--int-overflow=raise` mode, panicking rather than silently wrapping or
//! promoting to a `Bignum` (the spike has neither `raise`/`rescue` nor
//! `Bignum` yet -- still fail-fast, not silent-wrong).

pub fn int_add(a: i64, b: i64) -> i64 {
    a.checked_add(b).expect("Integer overflow")
}

pub fn int_sub(a: i64, b: i64) -> i64 {
    a.checked_sub(b).expect("Integer overflow")
}

pub fn int_mul(a: i64, b: i64) -> i64 {
    a.checked_mul(b).expect("Integer overflow")
}

/// Ruby's `Integer#/` floors toward negative infinity (`-7 / 2 == -4`),
/// unlike Rust's `/`, which truncates toward zero (`-7 / 2 == -3`).
pub fn int_div(a: i64, b: i64) -> i64 {
    if b == 0 {
        panic!("divided by 0");
    }
    let q = a / b;
    let r = a % b;
    if r != 0 && (r < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

/// Ruby's `Integer#%` takes the sign of the divisor (floored modulo),
/// unlike Rust's `%` (truncated remainder, sign of the dividend).
pub fn int_mod(a: i64, b: i64) -> i64 {
    if b == 0 {
        panic!("divided by 0");
    }
    let r = a % b;
    if r != 0 && (r < 0) != (b < 0) {
        r + b
    } else {
        r
    }
}

/// A negative exponent would be a `Rational` result in real Ruby -- out of
/// scope until a `Rational` type exists.
pub fn int_pow(a: i64, b: i64) -> i64 {
    let exp: u32 = b
        .try_into()
        .expect("negative exponent (a Rational result) isn't supported yet");
    a.checked_pow(exp).expect("Integer overflow")
}

pub fn int_band(a: i64, b: i64) -> i64 {
    a & b
}
pub fn int_bor(a: i64, b: i64) -> i64 {
    a | b
}
pub fn int_bxor(a: i64, b: i64) -> i64 {
    a ^ b
}

/// Ruby's `Integer#~` (`~a == -(a + 1)`) is exactly Rust's bitwise-not on a
/// two's-complement `i64`.
pub fn int_bnot(a: i64) -> i64 {
    !a
}

pub fn int_neg(a: i64) -> i64 {
    a.checked_neg().expect("Integer overflow")
}

pub fn int_pos(a: i64) -> i64 {
    a
}

/// Negative shift amounts (which real Ruby redirects to the opposite shift
/// direction) and shifts that would need `Bignum` promotion are out of
/// scope for the spike -- a clean panic, not silent truncation.
pub fn int_shl(a: i64, b: i64) -> i64 {
    let amount: u32 = b
        .try_into()
        .expect("negative shift amount isn't supported yet (spike scope)");
    a.checked_shl(amount)
        .expect("shift amount out of range, or result needs Bignum promotion (spike scope)")
}

pub fn int_shr(a: i64, b: i64) -> i64 {
    let amount: u32 = b
        .try_into()
        .expect("negative shift amount isn't supported yet (spike scope)");
    if amount >= 64 {
        panic!("shift amount out of range (spike scope)");
    }
    a >> amount
}

pub fn int_eq(a: i64, b: i64) -> bool {
    a == b
}
pub fn int_neq(a: i64, b: i64) -> bool {
    a != b
}
pub fn int_lt(a: i64, b: i64) -> bool {
    a < b
}
pub fn int_gt(a: i64, b: i64) -> bool {
    a > b
}
pub fn int_le(a: i64, b: i64) -> bool {
    a <= b
}
pub fn int_ge(a: i64, b: i64) -> bool {
    a >= b
}

/// Mirrors `Integer#<=>`: -1/0/1, never `nil` here since both operands are
/// statically known `Int` (a genuinely incomparable pair returning `nil` is
/// a `Poly`-typed case that doesn't route through this fast path at all).
pub fn int_cmp(a: i64, b: i64) -> i64 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Native `f64` arithmetic/comparison, mirroring `int_*` above -- see
/// `codegen::call`'s `FLOAT_BINARY_OPS`/mixed-`Int`/`Float`-promotion table.
/// No overflow/`Bignum` concerns (`f64` saturates to `inf`, matching real
/// Ruby's own `Float` behavior exactly, unlike `Integer`'s raise-on-overflow
/// default) -- these are plain, unchecked IEEE 754 operations.
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
pub fn float_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
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
