//! The per-fiber execution context (EC) bundle.
//!
//! CRuby gives every fiber its own `rb_execution_context_t` and switches
//! `th->ec` on fiber entry/exit (`cont.c`). This runtime keeps the same
//! ambient state in plain thread-locals -- the `$!`/rescue stack, proc
//! return-homes, live `catch` tags, and backtrace frames -- so a fiber
//! switch swaps ALL FOUR as one unit: [`swap`] installs the suspended
//! context and returns the running one. Sound because a fiber never runs
//! concurrently with its resumer (both swaps happen on the resumer's own
//! stack, either side of the switch).
//!
//! Oracle-pinned consequences (ruby 4.0.6): `throw` inside a fiber
//! cannot see the resumer's `catch` (`UncaughtThrowError` at the throw),
//! a raise inside a fiber backtraces only the fiber's own frames
//! (`caller` at the block top is empty), and a method suspended by
//! `Fiber.yield` keeps its proc return-home alive until IT returns.

use crate::RubyValue;

/// One suspended execution context: what a non-running fiber owns.
/// `default()` is a FRESH context -- a new fiber starts with no exception
/// in flight, no live homes, no catch frames, and an empty backtrace,
/// regardless of what its creator was doing.
#[derive(Default)]
pub struct Ec {
    handling: Vec<RubyValue>,
    home_stack: Vec<crate::signal::ProcHome>,
    catch_tags: Vec<RubyValue>,
    frames: Vec<crate::frames::Frame>,
}

/// Install `ec` as the ambient execution context, returning the previous
/// one -- called either side of a fiber switch (`fiber::run_fiber`,
/// `enumerator`'s internal resume).
pub fn swap(ec: Ec) -> Ec {
    Ec {
        handling: crate::handling::swap_handling(ec.handling),
        home_stack: crate::signal::swap_home_stack(ec.home_stack),
        catch_tags: crate::builtins::kernel::swap_catch_tags(ec.catch_tags),
        frames: crate::frames::swap_stack(ec.frames),
    }
}
