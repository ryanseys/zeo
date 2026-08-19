//! The method-prologue/epilogue surface: stack guard, call frames (which
//! carry the release-pool watermark), line stamping, and the interrupt
//! checkpoint.

use crate::RubyValue;
use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

/// The recursion guard: `STATUS_SIGNAL` parks the rescuable
/// `SystemStackError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_stack_check() -> i32 {
    match crate::stack_guard::stack_check() {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// Push a call frame (`.rodata` file/label text) and record the release-
/// pool watermark the matching [`zeo_rt_frame_pop`] drains to.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_frame_push(
    file: *const u8,
    file_len: usize,
    label: *const u8,
    label_len: usize,
    line: u32,
    end_line: u32,
) {
    let file = unsafe { super::static_str(file, file_len) };
    let label = unsafe { super::static_str(label, label_len) };
    crate::release_pool::push_frame_mark();
    crate::frames::frame_push_raw(file, label, line, end_line);
}

/// Pop the frame and release its pooled temporaries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_frame_pop() {
    crate::frames::frame_pop_raw();
    crate::release_pool::pop_frame_mark();
}

/// Stamp the innermost frame's current line (emitted when the source line
/// changes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_set_line(line: u32) {
    crate::frames::set_line(line);
}

/// The interruption checkpoint (loop back-edges, prologues): a queued
/// `Thread#kill`/`#raise` lands as `STATUS_SIGNAL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_check_ints() -> i32 {
    match crate::check_ints() {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// Push a synthetic C frame under `label` (caller's location); answers
/// whether one was pushed (an exact repeat is deduped) -- pass that to
/// [`zeo_rt_synthetic_c_frame_pop`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_synthetic_c_frame_push(label: *const u8, label_len: usize) -> i8 {
    let label = unsafe { super::static_str(label, label_len) };
    crate::frames::synthetic_c_frame_push_raw(label) as i8
}

/// The pop matching [`zeo_rt_synthetic_c_frame_push`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_synthetic_c_frame_pop(pushed: i8) {
    crate::frames::synthetic_c_frame_pop_raw(pushed != 0);
}

/// Stamp the current backtrace onto a raised exception (the raise channels
/// that build the exception in compiled code rather than through
/// `zeo_rt_raise_error`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_stamp_backtrace(exc: *const RubyValue) {
    crate::builtins::exception::attach_backtrace(unsafe { &*exc });
}

/// Note the frame's `self` for `TracePoint` binding capture -- a no-op
/// unless tracing is armed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_trace_frame_self(v: *const RubyValue) {
    crate::trace_frame_self(|| unsafe { (*v).clone() });
}
