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

/// Push a call frame (`.rodata` file/label text), stamping the release-
/// pool watermark the matching [`zeo_rt_frame_pop`] drains to into the
/// frame itself (`Frame::pool_mark`).
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
    let mark = crate::release_pool::mark() as u32;
    crate::frames::frame_push_raw(file, label, line, end_line, mark);
}

/// Pop the frame and release its pooled temporaries (drains to the
/// popped frame's own `pool_mark`; a markless frame drains nothing).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_frame_pop() {
    let mark = crate::frames::frame_pop_raw();
    if mark != crate::frames::Frame::NO_MARK {
        crate::release_pool::reset(mark as usize);
    }
}

/// The address of this thread's hot frame/pool header
/// ([`crate::frames::FrameHot`]) -- emitted prologues fetch it once per
/// function and then push, pop, stamp lines and pool temporaries through
/// plain loads and stores (offsets pinned in `zeo_abi::abi::FRAMEHOT_*`).
/// The address is stable for the thread's lifetime; fibers swap the
/// CONTENTS through this same header, never the address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_frame_hot() -> *mut crate::frames::FrameHot {
    crate::frames::STACK.with(|s| s as *const crate::frames::FrameHot as *mut _)
}

/// Stamp the innermost frame's current line (emitted when the source line
/// changes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_set_line(line: u32) {
    crate::frames::set_line(line);
}

/// [`zeo_rt_frame_push`] and [`zeo_rt_check_ints`] fused: the pair every
/// prologue emits back-to-back, as ONE call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_frame_enter(
    file: *const u8,
    file_len: usize,
    label: *const u8,
    label_len: usize,
    line: u32,
    end_line: u32,
) -> i32 {
    unsafe { zeo_rt_frame_push(file, file_len, label, label_len, line, end_line) };
    unsafe { zeo_rt_check_ints() }
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

/// `__callee__` in a compiled method: the name a run-time alias called it
/// through, else `fallback`, the name the emitter knows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_frame_callee(fallback: u32, out: *mut RubyValue) {
    let sym = crate::frames::current_frame_callee()
        .unwrap_or_else(|| crate::Symbol::from_u32(fallback));
    let v = RubyValue::Symbol(sym);
    super::leakcheck::created(&v);
    // SAFETY: `out` is the caller's uninitialized result slot, written once.
    unsafe { out.write(v) };
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
