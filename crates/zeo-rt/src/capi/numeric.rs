//! The numeric SLOW paths -- what the emitted inline `sadd_overflow`/
//! `fcmp` fast paths fall back to for overflow and bignum operands. The
//! fully dynamic arm (a mixed pair, a user numeric) goes through
//! `zeo_rt_send_value_in` instead, exactly as the rustc backend's match
//! arms do.

use crate::RubyValue;
use crate::builtins::integer;

/// `a + b` past the inline overflow check (Int/BigInt operands).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_add_slow(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) {
    let v = unsafe { integer::int_add(&*a, &*b) };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `a - b` past the inline overflow check.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_sub_slow(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) {
    let v = unsafe { integer::int_sub(&*a, &*b) };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `a * b` past the inline overflow check.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_mul_slow(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) {
    let v = unsafe { integer::int_mul(&*a, &*b) };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Floored `a / b`. The emitter pre-guards the zero divisor (raising
/// `ZeroDivisionError` at its own site), same contract as the Rust helper.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_div(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) {
    let v = unsafe { integer::int_div(&*a, &*b) };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Floored `a % b`. Same zero-divisor contract as `zeo_rt_int_div`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_mod(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) {
    let v = unsafe { integer::int_mod(&*a, &*b) };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Three-way compare past the inline i64 path: negative/zero/positive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_cmp_slow(a: *const RubyValue, b: *const RubyValue) -> i64 {
    integer::int_cmp(unsafe { &*a }, unsafe { &*b })
}

/// A bignum literal from its baked u32 digits (little-endian, the
/// `num_bigint` order) -- the runtime demotes to `Int` whenever it fits,
/// so one Ruby Integer class covers both payloads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_digits(
    negative: u8,
    digits: *const u32,
    n: usize,
    out: *mut RubyValue,
) {
    let ds = if n == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(digits, n) }
    };
    let v = crate::int_from_u32_digits(negative != 0, ds);
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A rational literal (`3r`, `1.5r`) from its numerator and denominator
/// digits. Prism pre-rationalizes the decimal forms, so both travel as
/// digit arrays and the denominator is positive by syntax.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_rational_digits(
    negative: u8,
    num: *const u32,
    num_n: usize,
    den: *const u32,
    den_n: usize,
    out: *mut RubyValue,
) {
    let slice = |p: *const u32, n: usize| {
        if n == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(p, n) }
        }
    };
    let v = crate::rational_from_digits(negative != 0, slice(num, num_n), slice(den, den_n));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// An imaginary literal (`4i`): `Complex(0, inner)` over the already
/// lowered inner numeric literal, which this BORROWS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_complex_lit(imag: *const RubyValue, out: *mut RubyValue) {
    let v = crate::complex_from_literal(unsafe { &*imag }.clone());
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `a ** b` over Integer operands -- fallible: a negative exponent is a
/// Rational, `0 ** -n` raises.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_pow(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    super::dispatch::status_out(unsafe { integer::int_pow(&*a, &*b) }, out)
}

/// `a << b` -- fallible: a shift width beyond `u32` raises RangeError.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_shl(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    super::dispatch::status_out(unsafe { integer::int_shl(&*a, &*b) }, out)
}

/// `a >> b` -- same contract as [`zeo_rt_int_shl`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_int_shr(
    a: *const RubyValue,
    b: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    super::dispatch::status_out(unsafe { integer::int_shr(&*a, &*b) }, out)
}

/// `Float#%` -- a zero divisor raises where every other Float operator is
/// total, so it takes the fallible shape.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_float_mod_checked(a: f64, b: f64, out: *mut RubyValue) -> i32 {
    super::dispatch::status_out(crate::arith::float_mod_checked(a, b), out)
}

/// `Float#**` -- a negative base to a fractional power leaves the reals,
/// so the result may be a Complex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_float_pow_checked(a: f64, b: f64, out: *mut RubyValue) -> i32 {
    super::dispatch::status_out(crate::arith::float_pow_checked(a, b), out)
}

/// `Float#<=>` -- `nil` against a NaN, which is why it is not one of the
/// inline comparisons.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_float_cmp(a: f64, b: f64, out: *mut RubyValue) {
    let v = match crate::arith::float_cmp(a, b) {
        Some(n) => RubyValue::Int(n),
        None => RubyValue::Nil,
    };
    unsafe { out.write(v) };
}
