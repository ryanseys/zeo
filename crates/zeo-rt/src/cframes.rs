//! How many protected C frames an extension has open on this stack, and the
//! sole-thread claim the outermost one took away.
//!
//! The C-API crate owns the frames (its `protect` is the only door into
//! extension C); this crate owns the count, because a coroutine switch has
//! to carry it with the stack whether or not a C API is linked. A frame
//! opened on the resumer's stack is not open on the coroutine's.
//!
//! While any frame is open the thread's sole-thread claim is hidden
//! (`gvl::hide_sole_thread`): C holds raw views into Ruby objects, and the
//! lock-free `&mut` path would move them under it. The outermost frame's
//! close puts the claim back. A coroutine that yields from inside a frame
//! leaves the claim hidden until it comes back and closes it -- safe, and
//! rare enough not to pay for.

use std::cell::Cell;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
    /// Whether the outermost open frame took the sole-thread claim with it.
    static CLAIM: Cell<bool> = const { Cell::new(false) };
}

/// How many protected frames are open on this stack.
pub fn depth() -> usize {
    DEPTH.get()
}

/// One more protected frame is open.
pub fn enter() {
    let open = DEPTH.get();
    DEPTH.set(open + 1);
    if open == 0 {
        CLAIM.set(crate::gvl::hide_sole_thread());
    }
}

/// The innermost protected frame closed.
pub fn leave() {
    let open = DEPTH.get() - 1;
    DEPTH.set(open);
    if open == 0 && CLAIM.replace(false) {
        crate::gvl::mark_sole_thread();
    }
}

/// This stack's count and claim, for a coroutine switch to put back later.
#[derive(Clone, Copy)]
pub struct Saved {
    depth: usize,
    claim: bool,
}

pub fn save() -> Saved {
    Saved {
        depth: DEPTH.get(),
        claim: CLAIM.get(),
    }
}

pub fn restore(s: Saved) {
    DEPTH.set(s.depth);
    CLAIM.set(s.claim);
}

/// The resumer's count, hidden for the length of a coroutine's run and put
/// back when control comes back -- by yield, by completion, or by unwind.
pub struct SwitchGuard(Saved);

impl SwitchGuard {
    pub fn enter() -> SwitchGuard {
        let saved = save();
        // The coroutine starts on a stack of its own with no protected frame
        // on it.
        restore(Saved {
            depth: 0,
            claim: false,
        });
        SwitchGuard(saved)
    }
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        restore(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gvl;

    /// The claim is gone for the whole nest of frames and back after the
    /// outermost closes.
    #[test]
    fn a_c_frame_hides_the_sole_thread_claim() {
        gvl::reset_thread_flags_for_test();
        gvl::mark_sole_thread();
        assert!(gvl::sole_thread());
        enter();
        assert!(!gvl::sole_thread());
        enter();
        leave();
        assert!(
            !gvl::sole_thread(),
            "an inner frame's close is not the outer's"
        );
        leave();
        assert!(gvl::sole_thread());
    }

    /// A thread spawned while a frame is open means the claim never comes
    /// back: the close re-checks rather than restoring a stale `true`.
    #[test]
    fn a_spawn_inside_a_c_frame_keeps_the_claim_off() {
        gvl::reset_thread_flags_for_test();
        gvl::mark_sole_thread();
        enter();
        gvl::note_thread_spawn();
        leave();
        assert!(!gvl::sole_thread());
    }

    /// A thread that never held the claim gets nothing back.
    #[test]
    fn a_frame_on_a_thread_without_the_claim_leaves_it_off() {
        gvl::reset_thread_flags_for_test();
        enter();
        leave();
        assert!(!gvl::sole_thread());
    }
}
