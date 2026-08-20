//! The `&[Value]`-shaped Kernel intrinsics the emitter calls by name.

use super::dispatch::status_out;
use crate::RubyValue;

unsafe fn arg_view<'a>(argv: *const RubyValue, argc: usize) -> &'a [RubyValue] {
    if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    }
}

/// `Kernel#puts`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_kernel_puts(
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    status_out(crate::kernel_puts(unsafe { arg_view(argv, argc) }), out)
}

/// `Kernel#p`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_kernel_p(
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    status_out(crate::kernel_p(unsafe { arg_view(argv, argc) }), out)
}

/// `Kernel#print`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_kernel_print(
    argv: *const RubyValue,
    argc: usize,
    out: *mut RubyValue,
) -> i32 {
    status_out(crate::kernel_print(unsafe { arg_view(argv, argc) }), out)
}

/// Whether the flip-flop numbered `id` is currently latched on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_flip_flop_on(id: u32) -> u8 {
    u8::from(crate::flipflop::flip_flop_on(id))
}

/// Latches the flip-flop numbered `id` on or off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_flip_flop_set(id: u32, on: u8) {
    crate::flipflop::flip_flop_set(id, on != 0);
}
