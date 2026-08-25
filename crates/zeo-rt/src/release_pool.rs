//! The frame-scoped release pool -- the Cranelift backend's replacement for
//! rustc's drop glue. Compiled code moves each heap temporary it does not
//! store elsewhere into this per-coroutine pool (`zeo_rt_pool_push`);
//! `zeo_rt_frame_push` records the pool watermark and `zeo_rt_frame_pop`
//! releases everything above it, so a function needs ONE landing block
//! instead of a per-scope release chain. Loops bracket their bodies with
//! `zeo_rt_pool_mark`/`zeo_rt_pool_reset` so a long loop does not
//! accumulate. Immediates (tag < `FIRST_HEAP_TAG`) are never pooled.
//!
//! Per-coroutine: a fiber's temporaries must drain with the fiber's own
//! frames, not its resumer's, so the whole pool state is one [`crate::ec`]
//! swap slice.

use crate::RubyValue;
use std::cell::RefCell;

/// One coroutine's pool: the pooled values plus the per-frame watermarks
/// (innermost frame's mark on top). Frame marks live here rather than in
/// `frames::Frame` -- capi frame pushes/pops are strictly balanced, so a
/// parallel stack carries the same information without widening the frame
/// struct the rustc backend still uses.
#[derive(Default)]
pub struct PoolState {
    vals: Vec<RubyValue>,
    marks: Vec<usize>,
}

std::thread_local!(static POOL: RefCell<PoolState> = const {
    RefCell::new(PoolState {
        vals: Vec::new(),
        marks: Vec::new(),
    })
});

/// Move `v` into the pool; it lives until the enclosing frame pops or the
/// enclosing loop's latch resets to a mark below it.
pub(crate) fn push(v: RubyValue) {
    POOL.with(|p| p.borrow_mut().vals.push(v));
}

/// The current watermark -- a loop head captures it for its latch's reset.
pub(crate) fn mark() -> usize {
    POOL.with(|p| p.borrow().vals.len())
}

/// Release everything pooled above `mark` (a loop latch, or an early
/// unwind's rewind to a captured watermark).
pub(crate) fn reset(mark: usize) {
    POOL.with(|p| p.borrow_mut().vals.truncate(mark));
}

/// A capi frame push: remember where this frame's temporaries begin.
pub(crate) fn push_frame_mark() {
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        let mark = p.vals.len();
        p.marks.push(mark);
    });
}

/// A capi frame pop: release this frame's temporaries. Tolerates an empty
/// mark stack (a pop during thread teardown after the pool already drained).
pub(crate) fn pop_frame_mark() {
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        if let Some(mark) = p.marks.pop() {
            p.vals.truncate(mark);
        }
    });
}

/// Install `new` as this context's pool, returning the previous one -- the
/// fiber ec-swap's slice of this cell (see `crate::ec`).
pub fn swap_pool(new: PoolState) -> PoolState {
    POOL.with(|p| p.replace(new))
}
