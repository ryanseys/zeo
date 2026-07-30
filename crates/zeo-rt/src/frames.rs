//! The runtime CALL-FRAME stack backing `Exception#backtrace`,
//! `Kernel#caller`, and CRuby-shaped uncaught-exception reports.
//!
//! Generated method prologues push one lightweight [`Frame`] through a
//! [`FrameGuard`] (an RAII zero-sized type: `Drop` pops on EVERY exit
//! path, including `?`-propagated signals -- Rust runs drops on early
//! `return`, so no unwind machinery is needed in this `Result`-based
//! control-flow world). Statement emission stamps the CURRENT frame's line
//! (`set_line`) whenever the source line changes, so a captured backtrace
//! shows each frame at the line it was actually executing -- CRuby's own
//! per-frame PC-to-line reporting, at statement granularity.
//!
//! Frame text is baked at compile time (`&'static str` file names and
//! `'Class#method'` labels from the span tables), so a push is three stores
//! and a pointer bump, and the common no-raise path never formats anything.
//! Backtraces are FORMATTED at capture (raise) time.
//!
//! # Why the stack is split in two
//!
//! A thread-local whose TYPE needs dropping registers a destructor and
//! checks for it on every single access, and that check -- not the `Vec`
//! bookkeeping it was hiding behind -- was most of the old cost. Measured
//! on an M-series laptop, push+pop: `RefCell<Vec<Frame>>` 4.48 ns, the same
//! bump-pointer stack behind a drop-needing TLS 4.06 ns, and a
//! `Cell<*mut Frame>` trio that owns nothing 0.84 ns. `set_line` moves
//! 0.74 ns -> 0.37 ns the same way. So [`STACK`], which every call touches,
//! holds only raw pointers and has no `Drop`; [`OWNER`], which owns the
//! buffer so a finished thread frees it, is touched only by `grow` and
//! `swap_stack`.
//!
//! Thread-local, and each Ruby `Thread` is its own OS thread -- a raise in
//! one `Thread` never sees another's frames, by construction. Fibers swap
//! in their OWN frame stack via the ec-swap (`crate::ec`), so a raise
//! inside a fiber backtraces only the fiber's frames -- CRuby's own
//! per-fiber stack semantics, oracle-verified.

use std::cell::{Cell, RefCell};
use std::ptr;

/// One executing method activation -- everything a backtrace line needs.
#[derive(Clone, Copy)]
pub struct Frame {
    pub file: &'static str,
    pub line: u32,
    pub method: &'static str,
    /// The scope's `end` keyword line, `TracePoint`'s `:return`/`:end`
    /// lineno. 0 marks a frame that never fires entry/exit trace events
    /// (`<main>`, blocks, synthetic C frames). Fills what was padding, so
    /// a `Frame` stays 40 bytes.
    pub end_line: u32,
}

impl Frame {
    const EMPTY: Frame = Frame {
        file: "",
        line: 0,
        method: "",
        end_line: 0,
    };
}

/// The hot half: `base <= top <= end` into [`OWNER`]'s buffer, all three
/// null before the first push. Deliberately owns nothing -- see the module
/// docs for the measurement that forces this.
struct Stack {
    top: Cell<*mut Frame>,
    base: Cell<*mut Frame>,
    end: Cell<*mut Frame>,
}

impl Stack {
    #[inline]
    fn len(&self) -> usize {
        let base = self.base.get();
        if base.is_null() {
            return 0;
        }
        // SAFETY: `top` and `base` index the same buffer, `top >= base`.
        unsafe { self.top.get().offset_from(base) as usize }
    }

    fn detach(&self) {
        self.top.set(ptr::null_mut());
        self.base.set(ptr::null_mut());
        self.end.set(ptr::null_mut());
    }
}

std::thread_local!(static STACK: Stack = const {
    Stack {
        top: Cell::new(ptr::null_mut()),
        base: Cell::new(ptr::null_mut()),
        end: Cell::new(ptr::null_mut()),
    }
});

/// The cold half: owns the frame buffer so a finished thread frees it.
struct Owner(Vec<Frame>);

impl Drop for Owner {
    fn drop(&mut self) {
        // Teardown order between two thread-locals is unspecified, so cut
        // the hot pointers loose before the buffer goes. A push after this
        // point then finds an empty stack and is dropped (see `grow`)
        // rather than writing into freed memory.
        let _ = STACK.try_with(Stack::detach);
    }
}

std::thread_local!(static OWNER: RefCell<Owner> = const {
    RefCell::new(Owner(Vec::new()))
});

/// Room for at least one more frame. Runs once per thread, then once per
/// doubling of peak recursion depth -- never on an ordinary call, hence
/// `#[cold]`, which also keeps `push`'s inlined body down to the bump.
#[cold]
#[inline(never)]
fn grow() {
    // Snapshot through the raw pointers BEFORE borrowing the owner, so the
    // read and the reallocation never alias.
    let live: Vec<Frame> = with_frames(|f| f.to_vec());
    let want = (live.len() + 1).next_power_of_two().max(256);

    // During thread teardown `OWNER` may already be gone. Frames pushed
    // then are unobservable -- nothing left can capture a backtrace -- so
    // dropping the push is both harmless and the only answer that cannot
    // write into a freed buffer.
    let _ = OWNER.try_with(|owner| {
        let mut owner = owner.borrow_mut();
        owner.0 = vec![Frame::EMPTY; want];
        owner.0[..live.len()].copy_from_slice(&live);
        // Moving a `Vec` does not move its heap buffer, so this pointer
        // stays valid for as long as `owner.0` is not reassigned -- which
        // only this function does.
        let base = owner.0.as_mut_ptr();
        STACK.with(|s| {
            s.base.set(base);
            // SAFETY: `want > live.len()`, both inside the fresh buffer.
            s.top.set(unsafe { base.add(live.len()) });
            s.end.set(unsafe { base.add(want) });
        });
    });
}

#[inline]
fn push_frame(fr: Frame) {
    let pushed = STACK.with(|s| {
        let top = s.top.get();
        // Also catches the null/null initial state, which routes to `grow`.
        if top == s.end.get() {
            return false;
        }
        // SAFETY: `top < end`, so it addresses a live slot in the buffer.
        unsafe { top.write(fr) };
        s.top.set(unsafe { top.add(1) });
        true
    });
    if !pushed {
        grow_and_push(fr);
    }
}

#[cold]
#[inline(never)]
fn grow_and_push(fr: Frame) {
    grow();
    STACK.with(|s| {
        let top = s.top.get();
        if top.is_null() || top == s.end.get() {
            return; // teardown -- see `grow`
        }
        // SAFETY: as in `push_frame`.
        unsafe { top.write(fr) };
        s.top.set(unsafe { top.add(1) });
    });
}

/// Pop without reading the frame back. The old code's `Drop` moved the
/// popped `Frame` out unconditionally and that cost bm_fib ~8%; only the
/// tracing path actually needs the value.
#[inline]
fn pop_frame_discard() {
    STACK.with(|s| {
        let top = s.top.get();
        if top != s.base.get() {
            // SAFETY: `top > base`, so `top - 1` is a live slot.
            s.top.set(unsafe { top.sub(1) });
        }
    });
}

/// The live frames as a slice, outermost first.
fn with_frames<R>(f: impl FnOnce(&[Frame]) -> R) -> R {
    STACK.with(|s| {
        let base = s.base.get();
        let len = s.len();
        // `from_raw_parts` rejects a null base even at length 0.
        let ptr = if base.is_null() {
            ptr::NonNull::dangling().as_ptr()
        } else {
            base
        };
        // SAFETY: `base..base+len` is the initialized prefix of the buffer,
        // which nothing else aliases while `f` runs -- `f` never pushes.
        f(unsafe { std::slice::from_raw_parts(ptr, len) })
    })
}

/// Install `new` as this context's frame stack, returning the previous one
/// -- the fiber ec-swap's slice of this cell (see `crate::ec`).
///
/// Copies, where the old `Vec`-backed stack could hand the buffer over
/// whole. A fiber switch is a coroutine stack switch either side of this
/// call, so a memcpy of the live frames does not register; an ordinary
/// method call, which is what the split above is protecting, never gets
/// here at all.
pub fn swap_stack(new: Vec<Frame>) -> Vec<Frame> {
    let previous = with_frames(|f| f.to_vec());
    STACK.with(|s| s.top.set(s.base.get()));
    for fr in new {
        push_frame(fr);
    }
    previous
}

/// The RAII half: construction pushes, `Drop` pops -- bind it to a `let`
/// at the top of a generated method body (`let __frame = ...;`) and every
/// exit path (tail value, early `return`, `?`) pops exactly once.
pub struct FrameGuard(());

impl FrameGuard {
    // `#[inline]` on push/drop: these run on EVERY method call from the
    // generated crate, which is a separate rustc invocation -- without the
    // hint (and an optimized generated build) each is a cross-crate call.
    #[inline]
    pub fn push(
        file: &'static str,
        method: &'static str,
        line: u32,
        end_line: u32,
    ) -> FrameGuard {
        push_frame(Frame {
            file,
            line,
            method,
            end_line,
        });
        #[cfg(feature = "ext-tracepoint")]
        if end_line != 0 && crate::ext::tracepoint::tracing() {
            crate::ext::tracepoint::fire_entry(file, method, line);
        }
        FrameGuard(())
    }
}

impl Drop for FrameGuard {
    #[inline]
    fn drop(&mut self) {
        // The tracing gate comes FIRST so the untraced path pops in place.
        #[cfg(feature = "ext-tracepoint")]
        if crate::ext::tracepoint::tracing() {
            return traced_pop();
        }
        pop_frame_discard();
    }
}

/// The pop while tracing is on: `:return`/`:end` for an event-bearing
/// frame. `#[cold]`-outlined so `Drop`'s inlined fast path stays small.
/// The frame is read out BEFORE the handler runs, since the handler runs
/// Ruby code that pushes frames of its own.
#[cfg(feature = "ext-tracepoint")]
#[cold]
fn traced_pop() {
    let popped = STACK.with(|s| {
        let top = s.top.get();
        if top == s.base.get() {
            return None;
        }
        let top = unsafe { top.sub(1) };
        s.top.set(top);
        Some(unsafe { *top })
    });
    if let Some(fr) = popped {
        if fr.end_line != 0 {
            crate::ext::tracepoint::fire_exit(&fr);
        }
    }
}

/// A frame for a C-implemented callee: CRuby shows such frames at the
/// CALLER's file:line (there is no Ruby-level line inside a C function),
/// so this clones the current innermost frame's location under `method`'s
/// label -- e.g. `'BasicObject#initialize'` when an `initialize`-less
/// `.new` rejects arguments. Same RAII contract as [`FrameGuard::push`].
pub fn synthetic_c_frame(method: &'static str) -> FrameGuard {
    let (file, line) = current_location().unwrap_or(("", 0));
    push_frame(Frame {
        file,
        line,
        method,
        end_line: 0,
    });
    FrameGuard(())
}

/// Stamp the innermost frame's current line -- emitted before a statement
/// whose source line differs from the previous statement's.
#[inline]
pub fn set_line(line: u32) {
    STACK.with(|s| {
        let top = s.top.get();
        if top != s.base.get() {
            // SAFETY: `top > base`, so `top - 1` is the innermost frame.
            unsafe { (*top.sub(1)).line = line };
        }
    });
    #[cfg(feature = "ext-tracepoint")]
    if crate::ext::tracepoint::tracing() {
        crate::ext::tracepoint::fire_line(line);
    }
}

/// The innermost frame, copied out -- `TracePoint`'s `:raise` event derives
/// its path/lineno/method from the frame executing the raise.
#[cfg(feature = "ext-tracepoint")]
pub fn current_frame() -> Option<Frame> {
    with_frames(|f| f.last().copied())
}

/// The innermost frame's `file:line`, for a caller-location label like
/// `Thread#inspect`'s creation site. A builtin C function has no frame of its
/// own, so the top frame is its caller.
pub fn current_location() -> Option<(&'static str, u32)> {
    with_frames(|f| f.last().map(|fr| (fr.file, fr.line)))
}

/// `FILE:LINE:in 'METHOD'` -- CRuby's backtrace-entry shape.
fn format_frame(fr: &Frame) -> String {
    format!("{}:{}:in '{}'", fr.file, fr.line, fr.method)
}

/// The current stack as formatted backtrace lines, INNERMOST FIRST --
/// what a raise stamps onto the exception (`Exception#backtrace`).
pub fn capture_backtrace() -> Vec<String> {
    with_frames(|f| f.iter().rev().map(format_frame).collect())
}

/// `Kernel#caller(start = 1)`: the formatted stack above the CALLING
/// frame. `start` counts frames to skip beyond the caller's own (CRuby's
/// contract; `caller` runs as a builtin with no frame of its own, so the
/// innermost frame IS the caller and `start = 1` skips exactly it).
pub fn caller_lines(start: usize) -> Vec<String> {
    with_frames(|f| f.iter().rev().skip(start).map(format_frame).collect())
}

/// `caller_lines`' structured twin, for `Kernel#caller_locations`: the same
/// frames as `(file, line, method)` rather than pre-formatted strings, so each
/// becomes a `Thread::Backtrace::Location` with real `#path`/`#lineno`/`#label`.
pub fn caller_frames(start: usize) -> Vec<(&'static str, u32, &'static str)> {
    with_frames(|f| {
        f.iter()
            .rev()
            .skip(start)
            .map(|fr| (fr.file, fr.line, fr.method))
            .collect()
    })
}
