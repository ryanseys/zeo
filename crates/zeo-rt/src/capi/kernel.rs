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

/// `eval(src, binding = nil, file = nil, line = nil)` and its reflective
/// spelling. `scope` is the Binding of the CALLING scope, which the call
/// site materializes so the source can read and write the caller's locals;
/// an explicit `binding` argument wins over it. Every optional slot is
/// `nil` when absent, exactly as `eval_value_in_scope` reads them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_eval_value_in_scope(
    src: *const RubyValue,
    scope: *const RubyValue,
    binding: *const RubyValue,
    file: *const RubyValue,
    line: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let arg = |p: *const RubyValue| match unsafe { p.as_ref() } {
        Some(v) => v.clone(),
        None => RubyValue::Nil,
    };
    status_out(
        crate::eval::eval_value_in_scope(arg(src), arg(scope), arg(binding), arg(file), arg(line)),
        out,
    )
}

/// One statement hit -- emitted beside every `set_line` stamp of a
/// coverage-activated program, and nothing at all in one without.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cov_line(file: *const u8, len: usize, line: u32) {
    crate::ext::coverage::cov_line(unsafe { super::static_str(file, len) }, line);
}

/// A spliced file's top level is beginning: the file is reported iff
/// measurement is set up at this moment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cov_file_loaded(file: *const u8, len: usize) {
    crate::ext::coverage::cov_file_loaded(unsafe { super::static_str(file, len) });
}
