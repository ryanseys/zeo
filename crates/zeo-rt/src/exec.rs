//! Program entry: the generated program's top level runs with its
//! scheduling context installed and the process Gvl held for the duration
//! (see `gvl` -- the Gvl is DISABLED by default, so "held" is free and Ruby
//! `Thread`s run truly parallel; `ZEO_GVL=1` arms CRuby-fidelity serialized
//! scheduling). Every `Thread.new` is its own OS thread (`thread`);
//! `Fiber`'s corosensei coroutines nest inside whichever thread resumes
//! them.
//!
//! WHICH OS thread runs the top level is a platform decision -- see
//! `run_main`. On macOS it is the process main thread, the one CRuby uses
//! and the one Apple's frameworks insist on. Elsewhere it is a dedicated
//! 64 MiB thread, because that is the only way to size the stack.
//!
//! `Kernel#at_exit` handlers run in REVERSE registration order (CRuby's
//! rule) after the top-level body finishes -- including via `exit` (see
//! `kernel_exit`) and after an uncaught exception. A handler can OVERRIDE
//! the process exit status: `exit N` inside one sets it to N, an exception
//! raised inside one is reported immediately and sets it to 1, and either
//! way the remaining handlers still run -- the last override to happen
//! wins (all CRuby rules, oracle-verified). Minitest reports test failures
//! exactly this way (`exit code` inside its autorun handler).

use crate::{RubyValue, Signal};

static AT_EXIT: parking_lot::Mutex<Vec<RubyValue>> = parking_lot::Mutex::new(Vec::new());

/// The C `argv`, stashed by `zeo_rt_main` before registration runs. Under
/// an emitted C `main`, `std::env::args()` works on glibc and macOS but is
/// EMPTY on musl -- so `ARGV`/`$0` seeding reads this first and falls back
/// to `std::env::args()` for the rustc backend, whose generated `main` never
/// stashes.
static CLI_ARGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

pub(crate) fn stash_cli_args(args: Vec<String>) {
    let _ = CLI_ARGS.set(args);
}

/// The program's arguments, binary name first.
pub(crate) fn program_args() -> Vec<String> {
    match CLI_ARGS.get() {
        Some(args) => args.clone(),
        None => std::env::args().collect(),
    }
}

/// Flush the standard streams on the way out of the process.
///
/// Rust flushes its own `stdout` from `lang_start`, which an emitted C
/// `main` never enters -- so an AOT program that RETURNS its status (the
/// ordinary end, and the `SystemExit` end) left the buffer unwritten.
/// Output ending in a newline survived regardless, because stdout is
/// line-buffered; only a program printing raw bytes lost its tail, which
/// is why every newline-terminated golden passed over it.
pub fn flush_stdio() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

pub fn at_exit_register(handler: RubyValue) {
    AT_EXIT.lock().push(handler);
}

/// Drain the handlers; the status the last-run overriding handler chose,
/// or `None` when no handler exited or raised.
pub fn run_at_exit() -> Option<i32> {
    let mut status = None;
    loop {
        let Some(h) = AT_EXIT.lock().pop() else { break };
        if let RubyValue::Proc(p) = h
            && let Err(Signal::Raise(exc)) = p.call(&[])
        {
            status = Some(match crate::system_exit_status(&exc) {
                Some(code) => code,
                None => {
                    crate::report_uncaught(&exc);
                    1
                }
            });
        }
    }
    status
}

/// The native stack the top level runs on. The main thread's default is a
/// ulimit the program does not control (8 MiB typically), and unoptimized
/// native frames blow through it at recursion depths CRuby handles
/// routinely; 64 MiB keeps the depth platform-independent. Every platform
/// delivers it differently: the darwin link line stamps it into the
/// executable (`-stack_size`, see `link_binary`), and the spawn below sizes
/// its own thread with it.
pub const MAIN_STACK_SIZE: usize = 64 * 1024 * 1024;

pub fn run_main<F>(body: F) -> Result<RubyValue, Signal>
where
    F: FnOnce() -> Result<RubyValue, Signal> + Send + 'static,
{
    // Arm the `ZEO_ARITY_DEBUG` breadcrumb before any Ruby code dispatches --
    // the bit rides in the dispatch gate byte (see `runtime_meta`).
    if std::env::var_os("ZEO_ARITY_DEBUG").is_some() {
        crate::runtime_meta::arm_arity_debug();
    }
    crate::log::init();
    // `Thread.current` answers the main-thread object on whichever OS thread
    // this runs (`thread::CURRENT` is only set by `Thread.new` bodies), and
    // the sole-thread ivar path is valid because exactly one Ruby thread
    // exists here and it is this one.
    let program = move || {
        crate::gvl::mark_sole_thread();
        crate::thread::claim_main_os_thread();
        let _ctx = crate::gvl::install_ctx();
        let _held = crate::gvl::process_gvl().hold();
        let result = body();
        // Tear down this thread's suspended fibers HERE, while every
        // thread-local is alive -- the Terminate protocol, not a stack
        // unwind (see `fiber::terminate_thread_fibers`). `at_exit` runs
        // later, and on Linux on a DIFFERENT thread, where a cross-thread
        // resume of these fibers was already a FiberError.
        crate::fiber::terminate_thread_fibers();
        crate::builtins::enumerator::terminate_thread_enum_fibers();
        result
    };
    run_program(program)
}

/// The caller's thread is the process main thread (the emitted C `main`
/// calls straight in, and the JIT driver enters from its own `main`), and
/// the top level runs ON it. Apple's frameworks require exactly that
/// thread: AppKit refuses to instantiate an `NSWindow` anywhere else
/// (`pthread_main_np`, which no dispatch trick satisfies), and WebKit,
/// Metal and the Cocoa run loop share the rule -- so a spawned thread, big
/// stack or not, could never host a native macOS app. The stack depth
/// guarantee is the link line's job here (see [`MAIN_STACK_SIZE`]).
#[cfg(target_os = "macos")]
fn run_program<F>(program: F) -> Result<RubyValue, Signal>
where
    F: FnOnce() -> Result<RubyValue, Signal> + Send + 'static,
{
    program()
}

/// A dedicated thread rather than the OS main: an ELF cannot size the main
/// thread's stack at link time (it is `RLIMIT_STACK`, read at exec), and
/// musl reports only the TOUCHED extent of the main stack from
/// `pthread_getattr_np`, which would put the `SystemStackError` floor
/// (`stack_guard`) above the live stack pointer. No Linux framework demands
/// the main thread, so the spawn stays and the stack guarantee rides on it.
/// The real main only blocks in `join` and touches no Ruby object (so no
/// `note_thread_spawn`, deliberately).
#[cfg(not(target_os = "macos"))]
fn run_program<F>(program: F) -> Result<RubyValue, Signal>
where
    F: FnOnce() -> Result<RubyValue, Signal> + Send + 'static,
{
    let main = std::thread::Builder::new()
        .name("ruby-main".into())
        .stack_size(MAIN_STACK_SIZE)
        .spawn(program)
        .expect("spawn the ruby main thread");
    let joined = main.join();
    // From here the REAL main runs `at_exit`/finalizers over objects the
    // ruby-main thread stamped -- give up this thread's sole-thread claim so
    // those accesses take the locks (see `gvl::clear_sole_thread`).
    crate::gvl::clear_sole_thread();
    match joined {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
