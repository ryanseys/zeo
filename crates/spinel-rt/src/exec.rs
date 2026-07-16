//! Program entry (Phase 13.4): the generated program's whole top level runs
//! as `may`'s FIRST coroutine, not as a bare `fn main()` body -- so the main
//! program, every `Thread` (a `may` green coroutine, Phase 13.5), and
//! `Fiber`'s ambient state all share one uniform execution-context model,
//! with no "top-level vs. inside-a-Thread" special-casing anywhere in
//! codegen.
//!
//! **The GVL switch is the scheduler's worker count.** `may` multiplexes all
//! coroutines onto a configurable pool of OS worker threads; with exactly
//! ONE worker (the default here), genuine parallelism between Ruby `Thread`s
//! is structurally impossible -- precisely CRuby's own GVL-limited reality
//! for pure-Ruby code, obtained from the scheduler's own configuration
//! rather than a hand-rolled global lock. `--no-gvl` (argv) or
//! `SPINEL_THREADS=N` (env, explicit count, takes precedence) opt into real
//! OS-level parallelism, which Part 9's `Arc<parking_lot::Mutex<_>>`
//! foundation makes memory-safe. `may::config()` only takes effect before
//! the scheduler starts, which is why this runs before the first spawn.
//!
//! `may::go!` (not the raw, internally-`unsafe` `may::coroutine::spawn`)
//! keeps this crate lint-level unsafe-free; the safety obligation it
//! delegates -- never set a `thread_local!`, cross a `may` scheduling
//! point, then read it back -- is inherited program-wide and documented
//! where it bites (the `handling` module's `$!` stack, migrated to
//! coroutine-local storage in Phase 13.6; `fiber`'s thread-pinned table,
//! which fails CLOSED with a real `FiberError` on migration rather than
//! corrupting -- see that module's docs). Under the default single worker,
//! no coroutine can ever migrate and the obligation is trivially met.

use crate::{RubyValue, Signal};

/// The per-coroutine stack size, in MACHINE WORDS, not bytes -- confirmed
/// against the actual implementation (may's `set_stack_size` value flows
/// into generator's `Stack::new`, which multiplies by
/// `size_of::<usize>()`; `generator-0.8.9/src/stack/mod.rs:318`): 1 Mi
/// words = 8 MiB on 64-bit, matching Rust's own main-thread default.
/// `may`'s tiny library default is sized for IO tasks, not compiled Ruby's
/// ordinary recursion depth, and overflow past the guard page is a hard
/// fault, not a catchable error. (Passing bytes here read as words -- 64
/// MiB -- trips macOS's RLIMIT_STACK hard cap at startup; found
/// empirically, hence this comment.)
const STACK_SIZE_WORDS: usize = 1024 * 1024;

/// Runs `body` -- the generated program's entire top level -- as `may`'s
/// first coroutine, blocking the real OS main thread on its completion.
/// Called exactly once, from generated `main()`, AFTER
/// `install_class_registry` (registration must finish before anything that
/// could spawn). A Rust panic inside the coroutine (a runtime
/// `unchecked`-helper failure, etc.) is re-raised on the main thread with
/// its original payload -- same observable behavior (message at panic time,
/// exit 101) as the pre-coroutine `main`.
/// `Kernel#at_exit` handlers, run in REVERSE registration order (CRuby's
/// rule) after the top-level body finishes -- including via `exit` (see
/// `kernel_exit`) and after an uncaught exception. An exception raised
/// INSIDE a handler is swallowed after the remaining handlers run (CRuby
/// reports it; a silent skip is the spike-scope approximation --
/// TODO(plan P-A): report through the exception-message machinery).
static AT_EXIT: parking_lot::Mutex<Vec<RubyValue>> = parking_lot::Mutex::new(Vec::new());

pub fn at_exit_register(handler: RubyValue) {
    AT_EXIT.lock().push(handler);
}

pub fn run_at_exit() {
    loop {
        let Some(h) = AT_EXIT.lock().pop() else { break };
        if let RubyValue::Proc(p) = h {
            let _ = p.call(&[]);
        }
    }
}

pub fn run_main<F>(body: F) -> Result<RubyValue, Signal>
where
    F: FnOnce() -> Result<RubyValue, Signal> + Send + 'static,
{
    may::config().set_workers(worker_count()).set_stack_size(STACK_SIZE_WORDS);
    match may::go!(body).join() {
        Ok(result) => result,
        Err(panic_payload) => std::panic::resume_unwind(panic_payload),
    }
}

/// `SPINEL_THREADS=N` (explicit, wins) > `--no-gvl` (all available cores) >
/// the GVL-emulating default of 1. A malformed/zero `SPINEL_THREADS` is a
/// loud startup panic, not a silent fallback -- a concurrency knob set
/// wrong should never quietly change the program's execution model.
fn worker_count() -> usize {
    if let Ok(raw) = std::env::var("SPINEL_THREADS") {
        let n: usize = raw
            .parse()
            .unwrap_or_else(|_| panic!("SPINEL_THREADS must be a positive integer, got `{raw}`"));
        assert!(n >= 1, "SPINEL_THREADS must be >= 1, got {n}");
        return n;
    }
    if std::env::args().any(|a| a == "--no-gvl") {
        return std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    }
    1
}
