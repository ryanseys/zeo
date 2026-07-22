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
//! `'Class#method'` labels from the span tables), so a push is two words +
//! a `u32` into a thread-local `Vec` and the common no-raise path never
//! formats anything. Backtraces are FORMATTED at capture (raise) time.
//!
//! Thread-local, and each Ruby `Thread` is its own OS thread -- a raise in
//! one `Thread` never sees another's frames, by construction. Fibers swap
//! in their OWN frame stack via the ec-swap (`crate::ec`), so a raise
//! inside a fiber backtraces only the fiber's frames -- CRuby's own
//! per-fiber stack semantics, oracle-verified.

use std::cell::RefCell;

/// One executing method activation -- everything a backtrace line needs.
#[derive(Clone, Copy)]
pub struct Frame {
    pub file: &'static str,
    pub line: u32,
    pub method: &'static str,
}

// A frame push/pop pair runs on EVERY method call -- plain TLS keeps it
// two words + a u32 with no registry lookup. Each Ruby `Thread` is its own
// OS thread with its own (fresh) slot.
std::thread_local!(static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) });

/// Install `new` as this context's frame stack, returning the previous one
/// -- the fiber ec-swap's slice of this cell (see `crate::ec`).
pub fn swap_stack(new: Vec<Frame>) -> Vec<Frame> {
    FRAMES.with(|f| std::mem::replace(&mut *f.borrow_mut(), new))
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
    pub fn push(file: &'static str, method: &'static str, line: u32) -> FrameGuard {
        FRAMES.with(|f| f.borrow_mut().push(Frame { file, line, method }));
        FrameGuard(())
    }
}

impl Drop for FrameGuard {
    #[inline]
    fn drop(&mut self) {
        FRAMES.with(|f| {
            f.borrow_mut().pop();
        });
    }
}

/// A frame for a C-implemented callee: CRuby shows such frames at the
/// CALLER's file:line (there is no Ruby-level line inside a C function),
/// so this clones the current innermost frame's location under `method`'s
/// label -- e.g. `'BasicObject#initialize'` when an `initialize`-less
/// `.new` rejects arguments. Same RAII contract as [`FrameGuard::push`].
pub fn synthetic_c_frame(method: &'static str) -> FrameGuard {
    FRAMES.with(|f| {
        let mut stack = f.borrow_mut();
        let (file, line) = stack.last().map_or(("", 0), |fr| (fr.file, fr.line));
        stack.push(Frame { file, line, method });
    });
    FrameGuard(())
}

/// Stamp the innermost frame's current line -- emitted before a statement
/// whose source line differs from the previous statement's.
#[inline]
pub fn set_line(line: u32) {
    FRAMES.with(|f| {
        if let Some(fr) = f.borrow_mut().last_mut() {
            fr.line = line;
        }
    });
}

/// `FILE:LINE:in 'METHOD'` -- CRuby's backtrace-entry shape.
fn format_frame(fr: &Frame) -> String {
    format!("{}:{}:in '{}'", fr.file, fr.line, fr.method)
}

/// The current stack as formatted backtrace lines, INNERMOST FIRST --
/// what a raise stamps onto the exception (`Exception#backtrace`).
pub fn capture_backtrace() -> Vec<String> {
    FRAMES.with(|f| f.borrow().iter().rev().map(format_frame).collect())
}

/// `Kernel#caller(start = 1)`: the formatted stack above the CALLING
/// frame. `start` counts frames to skip beyond the caller's own (CRuby's
/// contract; `caller` runs as a builtin with no frame of its own, so the
/// innermost frame IS the caller and `start = 1` skips exactly it).
pub fn caller_lines(start: usize) -> Vec<String> {
    FRAMES.with(|f| {
        f.borrow()
            .iter()
            .rev()
            .skip(start)
            .map(format_frame)
            .collect()
    })
}
