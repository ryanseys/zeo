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
/// in flight, no live homes, no catch frames, an empty backtrace, and its
/// own empty `Thread#[]` map, regardless of what its creator was doing.
pub struct Ec {
    handling: Vec<RubyValue>,
    home_stack: Vec<crate::signal::ProcHome>,
    catch_tags: Vec<RubyValue>,
    frames: Vec<crate::frames::Frame>,
    /// The stack-overflow check floor (`stack_guard`) -- each fiber runs on
    /// its own coroutine stack with its own floor. `0` (a fresh context) =
    /// unchecked until the fiber entry derives one.
    stack_floor: usize,
    /// `Thread#[]` fiber-local storage (`thread::FiberLocals`): a fresh map
    /// per fiber; `None` = "not yet bound", re-seeded from the thread's root
    /// map on next access.
    fiber_locals: Option<crate::thread::FiberLocals>,
    /// The `$~` svar bundle (`lastmatch`): base slot + scope stack.
    svar_base: Option<crate::regexp::RMatchData>,
    svar_scopes: Vec<Option<crate::regexp::RMatchData>>,
}

impl Default for Ec {
    fn default() -> Ec {
        Ec {
            handling: Vec::new(),
            home_stack: Vec::new(),
            catch_tags: Vec::new(),
            frames: Vec::new(),
            stack_floor: 0,
            fiber_locals: Some(std::sync::Arc::default()),
            svar_base: None,
            svar_scopes: Vec::new(),
        }
    }
}

/// Install `ec` as the ambient execution context, returning the previous
/// one -- called either side of a fiber switch (`fiber::run_fiber`,
/// `enumerator`'s internal resume).
pub fn swap(ec: Ec) -> Ec {
    let (svar_base, svar_scopes) = crate::lastmatch::swap_svars(ec.svar_base, ec.svar_scopes);
    Ec {
        handling: crate::handling::swap_handling(ec.handling),
        home_stack: crate::signal::swap_home_stack(ec.home_stack),
        catch_tags: crate::builtins::kernel::swap_catch_tags(ec.catch_tags),
        frames: crate::frames::swap_stack(ec.frames),
        stack_floor: crate::stack_guard::set_floor(ec.stack_floor),
        fiber_locals: crate::thread::swap_fiber_locals(ec.fiber_locals),
        svar_base,
        svar_scopes,
    }
}
