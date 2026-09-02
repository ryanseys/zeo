//! Per-stack state of the C frames an extension has open: the innermost
//! landing pad a raise jumps to, and how many protected frames sit above
//! it.
//!
//! The C-API crate owns the frames; this crate owns WHERE the state lives,
//! because a coroutine switch has to carry it with the stack whether or
//! not a C API is linked. A `jmp_buf` names addresses on one stack, so the
//! resumer's chain must not be visible from the coroutine's stack and must
//! come back when control does.

use std::cell::Cell;
use std::ffi::c_void;

thread_local! {
    /// The innermost landing pad, or null.
    static HEAD: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    /// How many protected frames are open on this stack.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub fn head() -> *mut c_void {
    HEAD.get()
}

pub fn set_head(head: *mut c_void) {
    HEAD.set(head);
}

/// How many protected frames are open. The C-API scope stack and the fiber
/// switch both read it.
pub fn depth() -> usize {
    DEPTH.get()
}

/// One more protected frame is open.
pub fn enter() {
    DEPTH.set(DEPTH.get() + 1);
}

/// The innermost protected frame closed.
pub fn leave() {
    DEPTH.set(DEPTH.get() - 1);
}

/// This stack's chain, for a coroutine switch to put back later.
#[derive(Clone, Copy)]
pub struct Saved {
    head: *mut c_void,
    depth: usize,
}

pub fn save() -> Saved {
    Saved {
        head: HEAD.get(),
        depth: DEPTH.get(),
    }
}

pub fn restore(s: Saved) {
    HEAD.set(s.head);
    DEPTH.set(s.depth);
}

/// The resumer's chain, hidden for the length of a coroutine's run and put
/// back when control comes back -- by yield, by completion, or by unwind.
pub struct SwitchGuard(Saved);

impl SwitchGuard {
    pub fn enter() -> SwitchGuard {
        let saved = save();
        // The coroutine starts on a stack of its own with no protected frame
        // on it. Leaving the resumer's head in place would let a raise there
        // jump into a stack that is not running.
        restore(Saved {
            head: std::ptr::null_mut(),
            depth: 0,
        });
        SwitchGuard(saved)
    }
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        restore(self.0);
    }
}
