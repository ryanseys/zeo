//! How many protected C frames an extension has open on this stack.
//!
//! The C-API crate owns the frames (its `protect` is the only door into
//! extension C); this crate owns the count, because a coroutine switch has
//! to carry it with the stack whether or not a C API is linked. A frame
//! opened on the resumer's stack is not open on the coroutine's.

use std::cell::Cell;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// How many protected frames are open on this stack.
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

/// This stack's count, for a coroutine switch to put back later.
#[derive(Clone, Copy)]
pub struct Saved(usize);

pub fn save() -> Saved {
    Saved(DEPTH.get())
}

pub fn restore(s: Saved) {
    DEPTH.set(s.0);
}

/// The resumer's count, hidden for the length of a coroutine's run and put
/// back when control comes back -- by yield, by completion, or by unwind.
pub struct SwitchGuard(Saved);

impl SwitchGuard {
    pub fn enter() -> SwitchGuard {
        let saved = save();
        // The coroutine starts on a stack of its own with no protected frame
        // on it.
        restore(Saved(0));
        SwitchGuard(saved)
    }
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        restore(self.0);
    }
}
