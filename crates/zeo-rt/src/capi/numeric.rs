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
