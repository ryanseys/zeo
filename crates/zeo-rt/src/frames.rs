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
//! a `u32` into a coroutine-local `Vec` and the common no-raise path never
//! formats anything. Backtraces are FORMATTED at capture (raise) time.
//!
//! Coroutine-local (the same `may` TLS every execution-context stack here
//! uses -- see `signal::HOME_STACK`): each green thread gets its own
//! stack, so a raise in one `Thread` never sees another's frames. Fibers
//! currently share their owner thread's stack (the ec-swap covers `$!`
//! only) -- a documented approximation, same boundary `signal` has.

use std::cell::RefCell;

/// One executing method activation -- everything a backtrace line needs.
#[derive(Clone, Copy)]
pub struct Frame {
    pub file: &'static str,
    pub line: u32,
    pub method: &'static str,
}

// Plain per-OS-thread TLS, NOT `may::coroutine_local!` (whose per-access
// cost through the coroutine registry made a recursion-heavy benchmark 6x
// slower -- a frame push/pop pair runs on EVERY method call). Each Ruby
// `Thread` body swaps in a fresh stack on entry (`swap_stack` from
// `thread::thread_new`'s wrapper), so a spawned thread's raises capture
// its own frames. Known, temporary narrowing while threads are `may`
// coroutines multiplexed on worker OS threads: a thread that YIELDS
// mid-call leaves its frames beneath whichever coroutine runs next on the
// same worker, so a backtrace captured exactly there can include a parked
// sibling's frames below its own. The OS-thread migration (plan P3)
// makes this per-thread by construction.
std::thread_local!(static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) });

/// Install `new` as this execution context's frame stack, returning the
/// previous one -- a Ruby `Thread` body swaps in a fresh stack on entry
/// and restores its parent's on exit.
pub fn swap_stack(new: Vec<Frame>) -> Vec<Frame> {
    FRAMES.with(|f| std::mem::replace(&mut *f.borrow_mut(), new))
}

/// The RAII half: construction pushes, `Drop` pops -- bind it to a `let`
/// at the top of a generated method body (`let __frame = ...;`) and every
/// exit path (tail value, early `return`, `?`) pops exactly once.
pub struct FrameGuard(());

impl FrameGuard {
    pub fn push(file: &'static str, method: &'static str, line: u32) -> FrameGuard {
        FRAMES.with(|f| f.borrow_mut().push(Frame { file, line, method }));
        FrameGuard(())
    }
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        FRAMES.with(|f| {
            f.borrow_mut().pop();
        });
    }
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
