//! Threads, fibers and mutexes.
//!
//! Almost everything here is a Ruby method call, because zeo's `Thread`,
//! `Fiber` and `Thread::Mutex` are real Ruby classes with real
//! implementations behind them -- so `rb_mutex_lock` is `Thread::Mutex#lock`
//! and cannot drift from it.
//!
//! # The two that are not
//!
//! `rb_thread_create(f, arg)` runs a C function on a new thread. There is no
//! Ruby method for that: the body is a raw function pointer, not a block. It
//! is wrapped in a `Proc` and handed to `Thread.new`, which is what makes the
//! new thread a real Ruby thread with a real `Ec` -- and that matters,
//! because the body will call back into Ruby.
//!
//! # Releasing the GVL, and the one thing it cannot promise
//!
//! `rb_thread_call_without_gvl(f, arg, ubf, ubf_arg)` runs `f` with the lock
//! released, so a sibling thread can make progress across a slow C call.
//! `bcrypt` wraps every hash in one, which is what a cost-12 bcrypt needs.
//!
//! The `ubf` -- the unblocking function -- is what MRI calls to interrupt a
//! thread parked inside `f`, so `Thread#kill` lands on a blocking read. zeo
//! never calls it: interrupting opaque C means knowing what `f` is blocked
//! ON, and zeo has no `ubf` registry to consult from a signal handler. So a
//! thread inside a C call is not killable until the call returns. That is a
//! divergence, not an omission, and it is stated here rather than left to be
//! discovered.
//!
//! # Sleeping is `Kernel#sleep`
//!
//! Every `rb_thread_sleep*` entry routes through it rather than
//! `std::thread::sleep`, so a sleeping C extension is interruptible by
//! `Thread#raise` and `Thread#kill` exactly as sleeping Ruby is.

use super::convert::{to_value, value_of};
use super::object::{args_of, send};
use super::symbol::{Id, symbol_of};
use super::value::{self, Value};
use crate::builtins::wrong_arg_type;
use crate::{RubyValue, Signal, Symbol};
use std::ffi::{c_int, c_void};

/// The class a top-level constant names, or a `NameError`.
fn class_named(name: &str) -> Result<RubyValue, Signal> {
    crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, name).ok_or_else(|| {
        crate::dispatch::raise_error("NameError", format!("uninitialized constant {name}"))
    })
}

/// `Thread::Mutex`, `Thread::Queue` and friends live under `Thread`.
fn nested(outer: &str, inner: &str) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = class_named(outer)? else {
        return Err(crate::dispatch::raise_error(
            "TypeError",
            format!("{outer} is not a class"),
        ));
    };
    crate::constants::const_get(cid.0, inner).ok_or_else(|| {
        crate::dispatch::raise_error(
            "NameError",
            format!("uninitialized constant {outer}::{inner}"),
        )
    })
}

/// A `VALUE (*)(VALUE, VALUE, int, const VALUE *, VALUE)` body as a `Proc`.
///
/// This is `rb_block_call_func_t`, the shape MRI passes a C block in. Its
/// arguments are `(yielded, callback_arg, argc, argv, blockarg)`, so the
/// `Proc` has to rebuild the `argc`/`argv` pair on every call.
///
/// # Safety
///
/// `f` must have that shape and must live as long as the process, which is
/// true of every function in a loaded extension.
pub(super) unsafe fn block_proc(
    f: unsafe extern "C" fn(Value, Value, c_int, *const Value, Value) -> Value,
    arg: Value,
) -> crate::RProc {
    // `usize` rather than the pointer, so the closure is `Send + Sync`.
    let addr = f as usize;
    crate::rproc::ProcBuilder::from_rust(
        move |_recv, args, block| {
            let scope = super::scope::Scope::enter();
            let raw: Vec<Value> = args
                .iter()
                .map(super::convert::to_value)
                .collect::<Result<_, _>>()?;
            let first = raw.first().copied().unwrap_or(value::Q_NIL);
            let blk = match block {
                Some(b) => to_value(&b)?,
                None => value::Q_NIL,
            };
            // SAFETY: the caller of `block_proc` promised the shape, and
            // every `VALUE` here was pinned one statement ago.
            let body: unsafe extern "C" fn(Value, Value, c_int, *const Value, Value) -> Value =
                unsafe { std::mem::transmute(addr) };
            let out = super::jmp::protect(|| unsafe {
                body(first, arg, raw.len() as c_int, raw.as_ptr(), blk)
            })?;
            scope.keep(out);
            let answer = unsafe { value_of(out) };
            drop(scope);
            Ok(answer)
        },
        RubyValue::Nil,
        -1,
        false,
    )
    .build()
}

crate::cext_fn! {
    // ---- releasing the GVL -----------------------------------------------

    /// `rb_thread_call_without_gvl(f, arg, ubf, ubf_arg)`. See this module's
    /// docs for what the `ubf` does not do.
    ///
    /// `f` must not call back into Ruby -- MRI's contract too, and the
    /// reason `rb_thread_call_with_gvl` exists.
    fn rb_thread_call_without_gvl(
        f: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
        _ubf: *mut c_void,
        _ubf_arg: *mut c_void,
    ) -> *mut c_void {
        // SAFETY: the caller's own function and its own argument. It runs
        // OUTSIDE any protected frame, so it must not raise -- which is
        // MRI's contract for it too.
        Ok(crate::gvl::without_gvl(|| unsafe { f(arg) }))
    }

    /// `rb_thread_call_without_gvl2`: the same, and MRI answers null when the
    /// thread was interrupted before `f` ran. zeo has no interrupt to check
    /// at that point, so `f` always runs.
    fn rb_thread_call_without_gvl2(
        f: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
        ubf: *mut c_void,
        ubf_arg: *mut c_void,
    ) -> *mut c_void {
        unsafe { Ok(rb_thread_call_without_gvl(f, arg, ubf, ubf_arg)) }
    }

    /// `rb_thread_call_with_gvl(f, arg)`: re-acquire, for a callback that
    /// needs Ruby from inside a `without_gvl` body.
    ///
    /// zeo's release is SCOPED -- `without_gvl` restores the lock when its
    /// closure returns -- so there is no out-of-band release to invert and
    /// the call is direct. What that costs: a nested `with_gvl` does not
    /// serialise against a sibling thread the way MRI's does, which matters
    /// only to an extension that relies on the nesting for mutual exclusion
    /// rather than for correctness of the Ruby calls inside.
    fn rb_thread_call_with_gvl(
        f: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
    ) -> *mut c_void {
        // SAFETY: the caller's own function and argument.
        Ok(unsafe { f(arg) })
    }

    /// `rb_thread_lock_native_thread()`: pin this Ruby thread to its OS
    /// thread, which a gem needs when a C library keeps thread-local state.
    /// zeo runs every Ruby thread on its own OS thread already, so the
    /// pinning it asks for is what it already has.
    fn rb_thread_lock_native_thread() -> bool {
        Ok(true)
    }

    // ---- threads ---------------------------------------------------------

    fn rb_thread_current() -> Value {
        to_value(&send(&class_named("Thread")?, "current", &[])?)
    }

    fn rb_thread_main() -> Value {
        to_value(&send(&class_named("Thread")?, "main", &[])?)
    }

    /// `rb_thread_alone()`: is this the only live thread? An extension asks
    /// before taking a lock it can skip.
    fn rb_thread_alone() -> c_int {
        let list = send(&class_named("Thread")?, "list", &[])?;
        let n = match list {
            RubyValue::Array(a) => a.lock().len(),
            _ => 1,
        };
        Ok(c_int::from(n <= 1))
    }

    /// `rb_thread_create(f, arg)`: run a C function on a new thread.
    ///
    /// The body is a bare `VALUE (*)(void *)`, not `rb_block_call_func_t`, so
    /// it needs its own wrapper rather than [`block_proc`].
    fn rb_thread_create(f: unsafe extern "C" fn(*mut c_void) -> Value, arg: *mut c_void) -> Value {
        let (addr, argp) = (f as usize, arg as usize);
        let body = crate::rproc::ProcBuilder::from_rust(
            move |_recv, _args, _block| {
                let scope = super::scope::Scope::enter();
                // SAFETY: the caller promised the shape and the lifetime.
                let f: unsafe extern "C" fn(*mut c_void) -> Value =
                    unsafe { std::mem::transmute(addr) };
                let out = super::jmp::protect(|| unsafe { f(argp as *mut c_void) })?;
                scope.keep(out);
                let answer = unsafe { value_of(out) };
                drop(scope);
                Ok(answer)
            },
            RubyValue::Nil,
            0,
            false,
        )
        .build();
        let thread = crate::dispatch::send_value(
            &class_named("Thread")?,
            Symbol::intern("new"),
            &[],
            Some(RubyValue::Proc(body)),
        )?;
        to_value(&thread)
    }

    fn rb_thread_wakeup_alive(t: Value) -> Value {
        let th = unsafe { value_of(t) };
        // `wakeup` raises on a dead thread; the `_alive` form answers nil.
        Ok(match send(&th, "wakeup", &[]) {
            Ok(v) => to_value(&v)?,
            Err(_) => value::Q_NIL,
        })
    }

    fn rb_thread_stop() -> Value {
        to_value(&send(&class_named("Thread")?, "stop", &[])?)
    }

    fn rb_thread_schedule() -> () {
        send(&class_named("Thread")?, "pass", &[])?;
        Ok(())
    }

    fn rb_thread_local_aref(t: Value, id: Id) -> Value {
        let th = unsafe { value_of(t) };
        to_value(&send(&th, "[]", &[RubyValue::Symbol(symbol_of(id))])?)
    }

    fn rb_thread_local_aset(t: Value, id: Id, v: Value) -> Value {
        let th = unsafe { value_of(t) };
        let val = unsafe { value_of(v) };
        send(&th, "[]=", &[RubyValue::Symbol(symbol_of(id)), val])?;
        Ok(v)
    }

    /// `rb_thread_interrupted(t)`: does the thread have a pending raise or
    /// kill? An extension polls it inside a long C loop.
    fn rb_thread_interrupted(_t: Value) -> c_int {
        Ok(c_int::from(crate::gvl::interrupts_pending_anywhere()))
    }

    /// `rb_thread_check_ints()`: the safepoint. Every long-running C loop is
    /// supposed to call it, and it is what makes `Thread#kill` land.
    fn rb_thread_check_ints() -> () {
        crate::check_ints()?;
        Ok(())
    }

    /// `rb_thread_sleep(sec)`. Through `Kernel#sleep`, so it is
    /// interruptible -- `std::thread::sleep` would not be.
    fn rb_thread_sleep(sec: c_int) -> () {
        sleep_for(Some(RubyValue::Int(sec.max(0) as i64)))
    }

    fn rb_thread_sleep_forever() -> () {
        sleep_for(None)
    }

    /// `rb_thread_sleep_deadly()`: sleep in a way the deadlock detector
    /// counts. zeo's detector reads the same wait, so the two are one call.
    fn rb_thread_sleep_deadly() -> () {
        sleep_for(None)
    }

    /// `rb_thread_atfork` and `_before_exec`: MRI rebuilds its thread table
    /// in the child, where only the forking thread survives. zeo's fork
    /// already leaves the child with one thread, so there is no table to
    /// repair -- and doing nothing is what keeps that true.
    fn rb_thread_atfork() -> () {
        Ok(())
    }

    fn rb_thread_atfork_before_exec() -> () {
        Ok(())
    }

    // ---- mutexes ---------------------------------------------------------

    fn rb_mutex_new() -> Value {
        to_value(&send(&nested("Thread", "Mutex")?, "new", &[])?)
    }

    fn rb_mutex_trylock(m: Value) -> Value {
        let mu = unsafe { value_of(m) };
        to_value(&send(&mu, "try_lock", &[])?)
    }

    /// `rb_mutex_synchronize(mutex, func, arg)`: hold the lock across a C
    /// call. The unlock runs on the raising path too, which is the whole
    /// reason an extension uses this rather than lock/unlock by hand.
    fn rb_mutex_synchronize(
        m: Value,
        func: unsafe extern "C" fn(Value) -> Value,
        arg: Value,
    ) -> Value {
        let mu = unsafe { value_of(m) };
        send(&mu, "lock", &[])?;
        let out = super::jmp::protect(|| unsafe { func(arg) });
        send(&mu, "unlock", &[])?;
        out
    }

    // ---- fibers ----------------------------------------------------------

    fn rb_fiber_current() -> Value {
        to_value(&send(&class_named("Fiber")?, "current", &[])?)
    }

    fn rb_fiber_new(
        f: unsafe extern "C" fn(Value, Value, c_int, *const Value, Value) -> Value,
        arg: Value,
    ) -> Value {
        new_fiber(f, arg, None)
    }

    /// `rb_fiber_new_storage(f, arg, storage)`: the same, with the fiber's
    /// initial `Fiber#storage`.
    fn rb_fiber_new_storage(
        f: unsafe extern "C" fn(Value, Value, c_int, *const Value, Value) -> Value,
        arg: Value,
        storage: Value,
    ) -> Value {
        new_fiber(f, arg, Some(unsafe { value_of(storage) }))
    }

    fn rb_fiber_resume(fiber: Value, argc: c_int, argv: *const Value) -> Value {
        fiber_send(fiber, "resume", argc, argv)
    }

    fn rb_fiber_resume_kw(fiber: Value, argc: c_int, argv: *const Value, _kw: c_int) -> Value {
        fiber_send(fiber, "resume", argc, argv)
    }

    fn rb_fiber_transfer(fiber: Value, argc: c_int, argv: *const Value) -> Value {
        fiber_send(fiber, "transfer", argc, argv)
    }

    fn rb_fiber_transfer_kw(fiber: Value, argc: c_int, argv: *const Value, _kw: c_int) -> Value {
        fiber_send(fiber, "transfer", argc, argv)
    }

    fn rb_fiber_raise(fiber: Value, argc: c_int, argv: *mut Value) -> Value {
        fiber_send(fiber, "raise", argc, argv)
    }

    /// `rb_fiber_yield(argc, argv)`: `Fiber.yield`, a CLASS method -- the
    /// running fiber is implicit.
    fn rb_fiber_yield(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&send(&class_named("Fiber")?, "yield", &args)?)
    }

    fn rb_fiber_yield_kw(argc: c_int, argv: *const Value, _kw: c_int) -> Value {
        unsafe { Ok(rb_fiber_yield(argc, argv)) }
    }
}

/// `Kernel#sleep`, with `None` for "until woken".
fn sleep_for(sec: Option<RubyValue>) -> Result<(), Signal> {
    let main = crate::dispatch::main_object();
    let args: Vec<RubyValue> = sec.into_iter().collect();
    crate::dispatch::send_value(&main, Symbol::intern("sleep"), &args, None)?;
    Ok(())
}

fn new_fiber(
    f: unsafe extern "C" fn(Value, Value, c_int, *const Value, Value) -> Value,
    arg: Value,
    storage: Option<RubyValue>,
) -> Result<Value, Signal> {
    // SAFETY: the caller's own function, which lives in the loaded image.
    let body = unsafe { block_proc(f, arg) };
    let args: Vec<RubyValue> = storage
        .map(|s| {
            let key = RubyValue::Symbol(Symbol::intern("storage"));
            RubyValue::Hash(crate::value::collections::hash_new(vec![(key, s)]))
        })
        .into_iter()
        .collect();
    let fiber = crate::dispatch::send_value(
        &class_named("Fiber")?,
        Symbol::intern("new"),
        &args,
        Some(RubyValue::Proc(body)),
    )?;
    to_value(&fiber)
}

fn fiber_send(fiber: Value, meth: &str, argc: c_int, argv: *const Value) -> Result<Value, Signal> {
    let f = unsafe { value_of(fiber) };
    if crate::dispatch::class_name(f.class_id()).as_deref() != Some("Fiber") {
        return Err(wrong_arg_type(&f, "Fiber"));
    }
    let args = unsafe { args_of(argc, argv) };
    to_value(&send(&f, meth, &args)?)
}
