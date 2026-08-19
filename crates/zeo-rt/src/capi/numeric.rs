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
