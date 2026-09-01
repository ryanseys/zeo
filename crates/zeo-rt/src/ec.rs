//! The per-fiber execution context (EC) bundle.
//!
//! CRuby gives every fiber its own `rb_execution_context_t` and switches
//! `th->ec` on fiber entry/exit (`cont.c`). This runtime keeps the same
//! ambient state in plain thread-locals, each owned by its home module with
//! a `swap_*` fn -- so a fiber switch swaps ALL of them as one unit:
//! [`swap`] installs the suspended context and returns the running one.
//! Sound because a fiber never runs concurrently with its resumer (both
//! swaps happen on the resumer's own stack, either side of the switch).
//!
//! # The fiber-scoped TLS inventory
//!
//! THE RULE: any thread-local whose value can outlive a call that may reach
//! `Fiber.yield` belongs in this bundle. `Ec`'s fields are the joined set;
//! the AUDITED EXEMPTIONS (2026-08-18) are:
//!
//! * `dispatch::CURRENT_METHOD` -- the `ZEO_ARITY_DEBUG` attribution
//!   breadcrumb. Diagnostic-only; a cross-fiber misattribution mislabels a
//!   debug message, never behavior.
//! * `value::PENDING_CMP` -- a raising user `<=>` stashed by the infallible
//!   `rb_cmp` and consumed by its fallible driver in the SAME call, on the
//!   same stack; no suspension point sits between stash and consume.
//! * `runtime_meta::SINGLETON_DEFINEE` / `PENDING_DEFS` -- definition-time
//!   state. A `Fiber.yield` inside a runtime class-definition body would
//!   leak the definee across fibers; accepted as a known limit beside the
//!   documented box/definee divergences (nothing exercises it).
//!
//! A new thread-local that fails the rule joins `Ec`: add the field, its
//! `Default` arm, one line in [`swap`], and a `swap_*` fn beside the cell
//! (5 edit points -- `signal::swap_return_target` is the template).
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
    /// The in-flight `Signal::Return` target (`signal::RETURN_TARGET`):
    /// per-coroutine by definition -- only one return is in flight per
    /// coroutine, and it must not retarget another fiber's.
    return_target: Option<crate::signal::ProcHome>,
    /// The C-ABI status protocol's pending signal (`signal::PENDING`):
    /// per-coroutine for the same reason as `return_target` -- one signal
    /// in flight per coroutine, and a fiber switch must not let another
    /// fiber observe or consume it.
    pending: Option<crate::Signal>,
    catch_tags: Vec<RubyValue>,
    /// The frame-scoped release pool (`release_pool`): compiled-code heap
    /// temporaries drain with their own coroutine's frames, so the pool
    /// swaps as one slice (the per-frame watermarks ride the frames
    /// themselves, `Frame::pool_mark`).
    pool: Vec<RubyValue>,
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
    /// The block channel and `super` target a snippet's `yield`/`super`
    /// read (`eval::EvalHome`): published per compiled scope, so it swaps
    /// with the frames it brackets.
    eval_homes: Vec<crate::eval::EvalHome>,
    /// The lexical cref chains eval-containing scopes published
    /// (`eval::CREF_STACK`) -- swaps with the frames that pushed them.
    cref_stack: Vec<Vec<zeo_abi::ClassId>>,
    /// Which copy of a DUPLICATED module the running body is
    /// (`dispatch::MRO_RESUME`): published by the `super` walk that entered
    /// it, so it is live across every call the body makes -- `Fiber.yield`
    /// among them.
    mro_resume: Option<crate::dispatch::MroResume>,
    /// The same fact for the CLASS-METHOD channel
    /// (`dispatch::CLASS_MRO_RESUME`). A second cell rather than a wider
    /// first one: the two index different sequences -- an instance ancestry
    /// against a singleton walk -- so a module used both ways would have them
    /// collide on `defining_class`.
    class_mro_resume: Option<crate::dispatch::ClassResume>,
}

impl Default for Ec {
    fn default() -> Ec {
        Ec {
            handling: Vec::new(),
            home_stack: Vec::new(),
            return_target: None,
            pending: None,
            catch_tags: Vec::new(),
            pool: Vec::new(),
            frames: Vec::new(),
            stack_floor: 0,
            fiber_locals: Some(std::sync::Arc::default()),
            svar_base: None,
            svar_scopes: Vec::new(),
            eval_homes: Vec::new(),
            cref_stack: Vec::new(),
            mro_resume: None,
            class_mro_resume: None,
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
        return_target: crate::signal::swap_return_target(ec.return_target),
        pending: crate::signal::swap_pending(ec.pending),
        catch_tags: crate::catch::swap_catch_tags(ec.catch_tags),
        pool: crate::release_pool::swap_pool(ec.pool),
        frames: crate::frames::swap_stack(ec.frames),
        stack_floor: crate::stack_guard::set_floor(ec.stack_floor),
        fiber_locals: crate::thread::swap_fiber_locals(ec.fiber_locals),
        svar_base,
        svar_scopes,
        eval_homes: crate::eval::swap_eval_homes(ec.eval_homes),
        cref_stack: crate::eval::swap_cref_stack(ec.cref_stack),
        mro_resume: crate::dispatch::swap_mro_resume(ec.mro_resume),
        class_mro_resume: crate::dispatch::swap_class_mro_resume(ec.class_mro_resume),
    }
}
