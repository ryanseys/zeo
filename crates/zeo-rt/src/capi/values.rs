//! Value lifetime and identity: retain/release, the frame-scoped release
//! pool, and the tag-level predicates compiled code cannot answer inline.

use crate::RubyValue;
use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

/// Bump the refcount of the heap value at `v` (a no-op-shaped clone-and-
/// forget; the caller now owns one more reference). Never called for
/// immediates -- the emitter tests the tag first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_retain(v: *const RubyValue) {
    std::mem::forget(unsafe { (*v).clone() });
}

/// Drop the value at `v` in place; the slot is dead afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_release(v: *mut RubyValue) {
    unsafe { std::ptr::drop_in_place(v) };
}

/// Move the heap temporary at `v` into the frame-scoped release pool; it
/// is released at the enclosing frame's pop (or the enclosing loop latch's
/// reset). The slot is dead afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pool_push(v: *mut RubyValue) {
    crate::release_pool::push(unsafe { std::ptr::read(v) });
}

/// The pool watermark, captured at a loop head for its latch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pool_mark() -> usize {
    crate::release_pool::mark()
}

/// Release everything pooled above `mark` (a loop latch).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_pool_reset(mark: usize) {
    crate::release_pool::reset(mark);
}

/// Ruby truthiness: everything but `nil` and `false`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_truthy(v: *const RubyValue) -> i8 {
    unsafe { (*v).truthy() as i8 }
}

/// The value's class id -- the moved-container-aware probe (a Ractor-moved
/// husk answers as the husk class).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_class_of(v: *const RubyValue) -> u32 {
    crate::value::observed_class_id(unsafe { &*v }).0
}

/// `v.is_a?(cid)` -- ancestry plus per-object `extend`s and singleton
/// classes, exactly the observable `is_a?`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_is_a(v: *const RubyValue, cid: u32) -> i8 {
    crate::dispatch::is_a_value(unsafe { &*v }, zeo_abi::ClassId(cid)) as i8
}

/// Ruby `==` with the user-`==` raise channel: writes the boolean to
/// `*eq_out` on `STATUS_OK`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eq(
    a: *const RubyValue,
    b: *const RubyValue,
    eq_out: *mut i8,
) -> i32 {
    match crate::value::rb_eq_checked(unsafe { &*a }, unsafe { &*b }) {
        Ok(eq) => {
            unsafe { eq_out.write(eq as i8) };
            STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// Debug aid: `inspect` the value to stderr (never part of program output).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_inspect_to_stderr(v: *const RubyValue) {
    eprintln!("{}", unsafe { &*v }.inspect_string());
}

/// An `Integer` from the decimal text of a literal too wide for an inline
/// `i64` -- normalized back to `Int` when it does fit (a literal like
/// `9_999...` the parser conservatively routed here). The text is the
/// emitter's own `.rodata`, so a parse failure is an internal compiler
/// error, not a raise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_bignum_from_decimal(
    ptr: *const u8,
    len: usize,
    out: *mut RubyValue,
) {
    let text = unsafe { super::str_slice(ptr, len) };
    let digits = text
        .parse::<num_bigint::BigInt>()
        .unwrap_or_else(|_| panic!("invalid emitted decimal literal: {text:?}"));
    let v = match i64::try_from(&digits) {
        Ok(n) => RubyValue::Int(n),
        Err(_) => RubyValue::BigInt(std::sync::Arc::new(digits)),
    };
    unsafe { out.write(v) };
}
