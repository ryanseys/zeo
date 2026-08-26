//! The frame-scoped release pool -- the Cranelift backend's replacement for
//! rustc's drop glue. Compiled code moves each heap temporary it does not
//! store elsewhere into this per-coroutine pool (`zeo_rt_pool_push`);
//! `zeo_rt_frame_push` stamps the pool watermark into the frame it pushes
//! and `zeo_rt_frame_pop` releases everything above that mark, so a
//! function needs ONE landing block instead of a per-scope release chain.
//! Loops bracket their bodies with `zeo_rt_pool_mark`/`zeo_rt_pool_reset`
//! so a long loop does not accumulate. Immediates (tag <
//! `FIRST_HEAP_TAG`) are never pooled.
//!
//! The HOT state -- the value trio `pool_base <= pool_top <= pool_end` --
//! lives in [`crate::frames::FrameHot`] beside the frame trio, one
//! per-thread `repr(C)` header emitted code addresses directly. This
//! module owns the buffer (so a finished thread frees it, releasing any
//! values still pooled) and the grow path.
//!
//! Per-coroutine: a fiber's temporaries must drain with the fiber's own
//! frames, not its resumer's, so the pool is one [`crate::ec`] swap slice
//! (`swap_pool`).

use crate::RubyValue;
use crate::frames::STACK;
use std::cell::RefCell;
use std::mem::MaybeUninit;
use std::ptr;

/// The cold half: owns the pool buffer. Liveness is tracked ONLY by the
/// hot trio -- the `Vec`'s len stays 0, and the live range
/// `pool_base..pool_top` is dropped by hand wherever the buffer dies.
struct PoolOwner(Vec<MaybeUninit<RubyValue>>);

impl Drop for PoolOwner {
    fn drop(&mut self) {
        // Release whatever is still pooled, then cut the hot pointers
        // loose BEFORE the buffer goes -- teardown order between two
        // thread-locals is unspecified, and a push after this point must
        // find a detached (null) trio rather than freed memory.
        let _ = STACK.try_with(|s| {
            drain_to(s, 0);
            s.pool_top.set(ptr::null_mut());
            s.pool_base.set(ptr::null_mut());
            s.pool_end.set(ptr::null_mut());
        });
    }
}

std::thread_local!(static POOL_OWNER: RefCell<PoolOwner> = const {
    RefCell::new(PoolOwner(Vec::new()))
});

#[inline]
fn len_of(s: &crate::frames::FrameHot) -> usize {
    let base = s.pool_base.get();
    if base.is_null() {
        return 0;
    }
    // SAFETY: `pool_top` and `pool_base` index the same buffer.
    unsafe { s.pool_top.get().offset_from(base) as usize }
}

/// Release the values above `mark`, leaving `pool_top` at it.
///
/// The trio is RE-READ every iteration: a release can run arbitrary code
/// (a finalizer) that pools new values and even GROWS the buffer, and the
/// drops run with `pool_top` already lowered, so such a push appends
/// above the rewound mark instead of aliasing what is being dropped.
fn drain_to(s: &crate::frames::FrameHot, mark: usize) {
    loop {
        let base = s.pool_base.get();
        if base.is_null() {
            return;
        }
        let top = s.pool_top.get();
        // SAFETY: `mark <= len`, so `base + mark` stays in the buffer.
        let floor = unsafe { base.add(mark) };
        if top <= floor {
            return;
        }
        let top = unsafe { top.sub(1) };
        s.pool_top.set(top);
        // SAFETY: `top` was below the old `pool_top`, a live value.
        unsafe { ptr::drop_in_place(top) };
    }
}

/// Room for at least one more value. Once per thread, then once per
/// doubling -- `#[cold]` keeps the push fast path to the bump.
#[cold]
#[inline(never)]
fn grow() -> bool {
    STACK
        .try_with(|s| {
            let live = len_of(s);
            let want = (live + 1).next_power_of_two().max(256);
            POOL_OWNER
                .try_with(|owner| {
                    let mut owner = owner.borrow_mut();
                    let mut fresh: Vec<MaybeUninit<RubyValue>> = Vec::with_capacity(want);
                    let base = fresh.as_mut_ptr().cast::<RubyValue>();
                    // SAFETY: the live prefix MOVES to the fresh buffer
                    // (bitwise -- ownership transfers, nothing doubles);
                    // the old buffer is then dropped empty. Guarded: the
                    // first grow has a null source and nothing live.
                    if live > 0 {
                        unsafe {
                            ptr::copy_nonoverlapping(s.pool_base.get(), base, live);
                        }
                    }
                    owner.0 = fresh;
                    s.pool_base.set(base);
                    s.pool_top.set(unsafe { base.add(live) });
                    s.pool_end.set(unsafe { base.add(want) });
                })
                .is_ok()
        })
        .unwrap_or(false)
}

/// Move `v` into the pool; it lives until the enclosing frame pops or the
/// enclosing loop's latch resets to a mark below it. During thread
/// teardown the value is released on the spot instead (nothing can
/// observe it any more).
pub(crate) fn push(v: RubyValue) {
    let v = std::mem::ManuallyDrop::new(v);
    let src: *const RubyValue = &*v;
    let pushed = STACK
        .try_with(|s| {
            let top = s.pool_top.get();
            // Also catches the null initial state, which routes to grow.
            if top == s.pool_end.get() {
                return false;
            }
            // SAFETY: `top < end` is a live slot; the read MOVES the
            // value out of the caller's `ManuallyDrop` wrapper, which is
            // never touched again on this path.
            unsafe { top.write(ptr::read(src)) };
            s.pool_top.set(unsafe { top.add(1) });
            true
        })
        .unwrap_or(false);
    if pushed {
        return;
    }
    // The fast path never read it -- take ownership back for the slow one.
    let v = std::mem::ManuallyDrop::into_inner(v);
    if grow() {
        let mut v = Some(v);
        let _ = STACK.try_with(|s| {
            let top = s.pool_top.get();
            if top.is_null() || top == s.pool_end.get() {
                return; // teardown
            }
            // SAFETY: as above.
            unsafe { top.write(v.take().expect("moved once")) };
            s.pool_top.set(unsafe { top.add(1) });
        });
        // A remaining `v` (teardown) drops here, releasing it now.
    }
    // grow failed = teardown: `v` drops here, releasing it immediately.
}

/// The current watermark -- a loop head captures it for its latch's
/// reset, and a frame push stamps it into the frame.
pub(crate) fn mark() -> usize {
    STACK.try_with(len_of).unwrap_or(0)
}

/// Release everything pooled above `mark` (a loop latch, a frame pop, or
/// an early unwind's rewind to a captured watermark).
pub(crate) fn reset(mark: usize) {
    let _ = STACK.try_with(|s| drain_to(s, mark));
}

/// Install `new` as this context's pool, returning the previous one -- the
/// fiber ec-swap's slice of this cell (see `crate::ec`).
pub fn swap_pool(new: Vec<RubyValue>) -> Vec<RubyValue> {
    STACK
        .try_with(|s| {
            let live = len_of(s);
            let base = s.pool_base.get();
            let mut previous = Vec::with_capacity(live);
            // SAFETY: the live range MOVES out bitwise; `pool_top` rewinds
            // to base so nothing double-drops. Guarded: an unused pool has
            // a null base and nothing live.
            if live > 0 {
                unsafe {
                    ptr::copy_nonoverlapping(base, previous.as_mut_ptr(), live);
                    previous.set_len(live);
                }
            }
            if !base.is_null() {
                s.pool_top.set(base);
            }
            for v in new {
                push(v);
            }
            previous
        })
        .unwrap_or_default()
}
