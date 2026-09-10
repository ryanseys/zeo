//! `SystemStackError` -- CRuby's `ruby_stack_check` shape: compare the stack
//! pointer against a cached per-execution-context floor, no syscalls on the
//! call path. Infinite recursion in a compiled program is otherwise a
//! MACHINE stack overflow -- a hardware fault no `rescue` sees and no
//! `ensure` runs through -- because Ruby frames are native frames here.
//!
//! The floor lives in a thread-local `Cell` that [`crate::ec::swap`] carries
//! per fiber (a fiber runs on its own coroutine stack, with its own floor).
//! `0` means unchecked: bounds unavailable, fail open rather than misfire.

use crate::signal::Signal;
use std::cell::Cell;

// `0` = uninitialized: the first `stack_check` on a thread derives the
// floor from its pthread bounds -- no per-thread init call sites to keep in
// sync. `UNAVAILABLE` = bounds could not be read; fail open (no real SP is
// below 1). A FIBER never lazy-inits: its entry installs its own floor
// (`fiber_floor_here`, carried by `Ec::swap` across suspensions) before any
// check runs on the coroutine stack, where the thread's bounds would be
// wrong.
thread_local! {
    static FLOOR: Cell<usize> = const { Cell::new(0) };
}

const UNAVAILABLE: usize = 1;

/// Headroom kept under the check line: enough native stack for the raise
/// itself -- `Signal` construction, backtrace formatting over ~1e5 frames
/// (heap work, but with real native callees), and rescue dispatch.
const MARGIN: usize = 512 * 1024;

#[cfg(target_os = "macos")]
fn current_thread_floor() -> usize {
    unsafe {
        let t = libc::pthread_self();
        // On macOS `stackaddr` is the HIGHEST address; the stack grows down.
        let top = libc::pthread_get_stackaddr_np(t) as usize;
        let size = libc::pthread_get_stacksize_np(t);
        if top == 0 || size == 0 {
            return UNAVAILABLE;
        }
        top.saturating_sub(size).saturating_add(MARGIN)
    }
}

#[cfg(target_os = "linux")]
fn current_thread_floor() -> usize {
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return UNAVAILABLE;
        }
        let mut addr: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let ok = libc::pthread_attr_getstack(&attr, &mut addr, &mut size) == 0;
        libc::pthread_attr_destroy(&mut attr);
        if !ok || addr.is_null() || size == 0 {
            return UNAVAILABLE;
        }
        (addr as usize).saturating_add(MARGIN)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_thread_floor() -> usize {
    UNAVAILABLE
}

/// Install a floor for a coroutine (fiber) stack whose bounds the entry code
/// derives itself, returning the previous floor. Paired with [`set_floor`]
/// on exit by the caller, or carried through `Ec::swap`.
pub fn set_floor(floor: usize) -> usize {
    FLOOR.with(|f| f.replace(floor))
}

/// A fiber's floor, derived at its entry: the entry stack pointer minus the
/// budget it may consume. Corosensei's default coroutine stack is small
/// (~1 MiB), so the budget stays conservatively under it.
pub fn fiber_floor_here() -> usize {
    let probe = 0u8;
    let sp = std::ptr::addr_of!(probe) as usize;
    const FIBER_BUDGET: usize = 512 * 1024;
    sp.saturating_sub(FIBER_BUDGET)
}

/// The per-call check: one TLS read, one compare, a never-taken branch.
/// Emitted by codegen in every compiled method prologue and proc body, and
/// taken by `send_value`'s dynamic entry -- the three carriers recursion can
/// ride.
#[inline(always)]
pub fn stack_check() -> Result<(), Signal> {
    let probe = 0u8;
    let sp = std::ptr::addr_of!(probe) as usize;
    let floor = FLOOR.with(|f| f.get());
    if floor == 0 {
        FLOOR.with(|f| f.set(current_thread_floor()));
        return Ok(());
    }
    if sp < floor {
        return Err(stack_overflow());
    }
    Ok(())
}

/// `SystemStackError` as an ORDINARY rescuable Signal: `rescue` catches it,
/// `ensure` runs during its unwind -- the whole point of checking above the
/// hardware fault line. `#[cold]` keeps the fast path's codegen tight.
#[cold]
#[inline(never)]
fn stack_overflow() -> Signal {
    crate::dispatch::raise_error("SystemStackError", "stack level too deep".to_string())
}
