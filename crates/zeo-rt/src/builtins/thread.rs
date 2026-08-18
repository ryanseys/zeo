//! `Thread`'s Path-2 (runtime `send`) rows -- for a dynamically-typed Thread
//! receiver (a thread stored in an Array/Hash/ivar, or `threads.each { |t|
//! t.join }` where `t` is `Poly`). The static Path-1 codegen arm emits
//! `thread_outcome` directly and never reaches here; these rows mirror it,
//! so both paths agree on join/value semantics. The reflection/state rows
//! (name/status/`[]`/thread-variables) live only here.
//!
//! Three rows report less than CRuby's, all for the same reason -- one OS
//! thread cannot read another's execution state:
//!
//! - `#backtrace`/`#backtrace_locations` answer real frames for the CURRENT
//!   thread and `nil` for a dead one (both as CRuby does), but `[]` for
//!   another live thread.
//! - `#priority=` records the number and nothing acts on it. CRuby's is
//!   advisory on the same platforms.
//! - `#set_trace_func`/`#add_trace_func` accept `nil` (there is no hook to
//!   clear) and REFUSE a Proc, rather than accepting one that would silently
//!   never fire -- the rule `TracePoint.new` already follows for the events
//!   zeo cannot raise.

use crate::builtins::type_error;
use crate::dispatch::raise_error;
use crate::thread::{
    self, RThread, thread_alive, thread_kill, thread_list, thread_outcome, thread_raise,
    thread_status,
};
use crate::value::RubyValue;
use crate::{Signal, Symbol};
use zeo_macros::ruby_class;

/// A Symbol/String storage-key argument as a `Symbol`.
fn key_sym(v: &RubyValue) -> Result<Symbol, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

/// The calling thread's own handle -- what the `Thread.exit`/`.stop`/
/// `.pending_interrupt?` class rows operate on.
fn current() -> RThread {
    thread::thread_current().as_thread_unchecked()
}

/// Whether `t` is the thread asking, which is the only one whose frames this
/// process can walk.
fn is_current(t: &RThread) -> bool {
    std::sync::Arc::ptr_eq(t, &current())
}

/// `#set_trace_func`/`#add_trace_func`: `nil` clears a hook that was never
/// installed, so it succeeds; anything else would be accepted and then never
/// called, so it is refused instead.
fn no_trace_func(arg: &RubyValue) -> Result<RubyValue, Signal> {
    if matches!(arg, RubyValue::Nil) {
        return Ok(RubyValue::Nil);
    }
    Err(raise_error(
        "NotImplementedError",
        "Thread#set_trace_func is not supported; use TracePoint".to_string(),
    ))
}

ruby_class! {
    Thread = zeo_abi::THREAD_CLASS < zeo_abi::OBJECT_CLASS;

    receiver t = crate::RubyValue::Thread;

    // `Thread.new`/`.start`/`.fork(*args) { |*params| }` -- CRuby's three
    // spellings of one constructor. Codegen intercepts the literal
    // `Thread.new` form; this row serves the rest and is what makes all
    // three appear in `Thread.singleton_methods`.
    def self."new" | "start" | "fork" allocs (_recv, *args, &block) {
        let Some(block) = block else {
            return Err(raise_error("ThreadError", "must be called with a block".to_string()));
        };
        Ok(thread::thread_new(block, args.to_vec()))
    }

    def self."current"(_recv) {
        Ok(thread::thread_current())
    }
    def self."main"(_recv) {
        Ok(thread::thread_main())
    }
    def self."pass"(_recv) {
        Ok(thread::thread_pass())
    }
    // `Thread.list` -- main plus every still-running spawned thread, from the
    // process-wide live registry (finished/joined threads are pruned out).
    def self."list"(_recv) {
        Ok(RubyValue::Array(crate::array_new(thread_list())))
    }
    // `Thread.kill(thr)` and `Thread.exit` -- the class spellings of `#kill`,
    // aimed at a named thread and at the caller.
    def self."kill"(_recv, target) {
        let RubyValue::Thread(target) = target else {
            return Err(type_error!(
                "wrong argument type {} (expected VM/thread)",
                crate::builtins::class_name_of(target)
            ));
        };
        thread_kill(target);
        Ok(RubyValue::Thread(target.clone()))
    }
    def self."exit"(_recv) {
        let me = current();
        thread_kill(&me);
        Ok(RubyValue::Thread(me))
    }
    // `Thread.stop` -- park the caller until another thread wakes it. The park
    // is the same interruptible sleep `Kernel#sleep` uses, so `#kill`/`#raise`
    // still reach a stopped thread.
    def self."stop"(_recv) {
        thread::thread_stop_current()
    }

    // `Thread.report_on_exception` -- the process-wide default a new thread
    // copies at spawn; setting it false silences the at-termination stderr
    // report for threads spawned AFTERWARDS (already-running ones keep the
    // value they were born with, as in CRuby).
    def self."report_on_exception"(_recv) {
        Ok(RubyValue::Bool(thread::report_on_exception_default()))
    }
    def self."report_on_exception="(_recv, on) {
        thread::set_report_on_exception_default(on.truthy());
        Ok(on.clone())
    }
    def self."abort_on_exception"(_recv) {
        Ok(RubyValue::Bool(thread::abort_on_exception_default()))
    }
    def self."abort_on_exception="(_recv, on) {
        thread::set_abort_on_exception_default(on.truthy());
        Ok(on.clone())
    }
    // `Thread.ignore_deadlock` -- stored and read back. There is no deadlock
    // detector here to switch off: threads are real OS threads, so a program
    // that sets this gets exactly the behavior it asked for.
    def self."ignore_deadlock"(_recv) {
        Ok(RubyValue::Bool(thread::ignore_deadlock()))
    }
    def self."ignore_deadlock="(_recv, on) {
        thread::set_ignore_deadlock(on.truthy());
        Ok(on.clone())
    }
    def self."pending_interrupt?" cfunc (_recv, _error?) {
        Ok(RubyValue::Bool(thread::current_pending_interrupt()))
    }
    // `Thread.each_caller_location { |loc| }` -- the caller's frames as
    // `Thread::Backtrace::Location` objects, yielded rather than collected.
    def self."each_caller_location" cfunc (_recv, *_args, &block) {
        let Some(block) = block else {
            return Err(crate::builtins::arg_error!("no block given (yield)"));
        };
        let block = block.as_proc_unchecked();
        for (f, l, m) in crate::frames::caller_frames(1) {
            block.call(&[crate::builtins::backtrace_location::location_new(f, l, m)])?;
        }
        Ok(RubyValue::Nil)
    }

    // `Thread.handle_interrupt(hash) { ... }` -- CRuby defers/unmasks async
    // interrupt (`Thread#raise`/`#kill`) delivery inside the block per the
    // `ExceptionClass => :immediate/:on_blocking/:never` mask. Async interrupts
    // here only ever deliver at an interruption checkpoint the target itself
    // reaches, which is `:on_blocking` behavior already -- so the mask is a
    // no-op and the block just runs. Argument shapes are validated like CRuby
    // (a Hash, and the block is mandatory).
    def self."handle_interrupt"(_recv, mask, &block) {
        if !matches!(mask, RubyValue::Hash(_)) {
            return Err(crate::builtins::arg_error!("unknown mask signature"));
        }
        let Some(b) = block else {
            return Err(crate::builtins::arg_error!("block is needed"));
        };
        b.as_proc_unchecked().call(&[])
    }

    // `Thread#join(limit = nil)` -- block until the thread finishes, re-raising
    // a stored exception in the caller, then answer the thread itself. A
    // timeout argument is accepted and ignored; the join always waits for
    // completion.
    def "join" cfunc (recv, _limit?) {
        thread_outcome(t)?;
        Ok(recv.clone())
    }
    // `Thread#value` -- join, then answer the BLOCK's result (vs `join`'s thread).
    def "value"(_recv) {
        thread_outcome(t)
    }
    // `Thread#alive?` -- true until the thread has finished (a non-joining peek).
    // Every reachable Thread is running or ran -- CRuby's ThreadError,
    // with the ORIGINAL spawn site appended when one is recorded.
    private def "initialize" cfunc (recv, *_args, &_block) {
        let _ = recv;
        let msg = match crate::thread::thread_origin(t) {
            Some(loc) => format!("already initialized thread - {loc}"),
            None => "already initialized thread".to_string(),
        };
        Err(crate::dispatch::raise_error("ThreadError", msg))
    }
    def "alive?"(_recv) {
        Ok(RubyValue::Bool(thread_alive(t)))
    }
    def "status"(_recv) {
        Ok(thread_status(t))
    }
    // `Thread#inspect`/`#to_s` -- `#<Thread:0xADDR STATUS>`, where a finished
    // thread reports `dead` (not the `false`/`nil` that `#status` answers).
    def "inspect" | "to_s"(_recv) {
        let status = if thread_alive(t) { "run" } else { "dead" };
        Ok(RubyValue::Str(crate::string_new(thread::thread_inspect(t, status))))
    }
    def "name"(_recv) {
        Ok(thread::thread_name(t))
    }
    def "name="(_recv, name) {
        let text = match name {
            RubyValue::Nil => None,
            other => Some(
                crate::builtins::convert::to_rstr(other)?
                    .lock()
                    .to_utf8_lossy()
                    .into_owned(),
            ),
        };
        thread::thread_set_name(t, text);
        Ok(name.clone())
    }
    // `Thread#group` -- always `ThreadGroup::Default` (the only group this
    // runtime models; see `builtins::thread_group`).
    def "group"(_recv) {
        Ok(crate::builtins::thread_group::default_group())
    }
    def "report_on_exception"(_recv) {
        Ok(RubyValue::Bool(thread::thread_report_on_exception(t)))
    }
    def "report_on_exception="(_recv, on) {
        thread::thread_set_report_on_exception(t, on.truthy());
        Ok(on.clone())
    }
    def "abort_on_exception"(_recv) {
        Ok(RubyValue::Bool(thread::thread_abort_on_exception(t)))
    }
    def "abort_on_exception="(_recv, on) {
        thread::thread_set_abort_on_exception(t, on.truthy());
        Ok(on.clone())
    }

    def "[]"(_recv, key) {
        Ok(thread::thread_local_get(t, key_sym(key)?))
    }
    def "[]="(_recv, key, value) {
        thread::thread_local_set(t, key_sym(key)?, value.clone());
        Ok(value.clone())
    }
    def "key?"(_recv, key) {
        Ok(RubyValue::Bool(thread::thread_local_key(t, key_sym(key)?)))
    }
    def "keys"(_recv) {
        Ok(RubyValue::Array(crate::array_new(thread::thread_local_keys(t))))
    }
    // `Thread#fetch(key, default = nil)` -- the `#[]` read with CRuby's
    // three-way miss: the block, then the default, then a KeyError.
    def "fetch" cfunc (_recv, key, default?, &block) {
        let sym = key_sym(key)?;
        if let Some(v) = thread::thread_local_fetch(t, sym) {
            return Ok(v);
        }
        if let Some(b) = block {
            return b.as_proc_unchecked().call(std::slice::from_ref(key));
        }
        match default {
            Some(v) => Ok(v.clone()),
            None => Err(raise_error(
                "KeyError",
                format!("key not found: {}", key.inspect_string()),
            )),
        }
    }
    def "thread_variable_get"(_recv, key) {
        Ok(thread::thread_variable_get(t, key_sym(key)?))
    }
    def "thread_variable_set"(_recv, key, value) {
        thread::thread_variable_set(t, key_sym(key)?, value.clone());
        Ok(value.clone())
    }
    def "thread_variable?"(_recv, key) {
        Ok(RubyValue::Bool(thread::thread_variable_key(t, key_sym(key)?)))
    }
    def "thread_variables"(_recv) {
        Ok(RubyValue::Array(crate::array_new(thread::thread_variable_keys(t))))
    }

    // `Thread#kill`/`#exit`/`#terminate` -- request termination, running the
    // thread's `ensure` blocks; answers the thread itself.
    def "kill" | "exit" | "terminate" (recv) {
        thread_kill(t);
        Ok(recv.clone())
    }
    // `Thread#raise(exc = RuntimeError)` -- inject an exception into the thread,
    // caught by its next `rescue`. A String becomes a RuntimeError; an exception
    // class/instance raises as itself (the shared `raise` coercion). A two-argument
    // `raise(Class, message)` builds `Class` with that message.
    def "raise"(_recv, *args) {
        let exc = match args {
            [] => crate::dispatch::coerce_raise_arg(RubyValue::Str(crate::string_new(String::new())))?,
            [one] => crate::dispatch::coerce_raise_arg(one.clone())?,
            [class, msg, ..] => {
                // `Class.exception(message)` -- the CRuby two-arg form.
                crate::dispatch::send_value(
                    class,
                    crate::Symbol::intern("exception"),
                    std::slice::from_ref(msg),
                    None,
                )?
            }
        };
        thread_raise(t, exc);
        Ok(RubyValue::Nil)
    }
    // `Thread#wakeup` -- clear the stop flag and wake the target; `#run` also
    // gives up the caller's quantum so the woken thread can make progress.
    def "wakeup"(recv) {
        match thread::thread_wakeup(t) {
            Ok(()) => Ok(recv.clone()),
            Err(msg) => Err(raise_error("ThreadError", msg.to_string())),
        }
    }
    def "run"(recv) {
        match thread::thread_wakeup(t) {
            Ok(()) => {
                thread::thread_pass();
                Ok(recv.clone())
            }
            Err(msg) => Err(raise_error("ThreadError", msg.to_string())),
        }
    }
    def "stop?"(_recv) {
        Ok(RubyValue::Bool(thread::thread_is_stopped(t)))
    }
    def "priority"(_recv) {
        Ok(RubyValue::Int(thread::thread_priority(t)))
    }
    def "priority="(_recv, level) {
        thread::thread_set_priority(t, level.as_int_unchecked());
        Ok(level.clone())
    }
    def "native_thread_id"(_recv) {
        Ok(match thread::thread_native_id(t) {
            Some(id) => RubyValue::Int(id as i64),
            None => RubyValue::Nil,
        })
    }
    def "pending_interrupt?" cfunc (_recv, _error?) {
        Ok(RubyValue::Bool(thread::thread_pending_interrupt(t)))
    }
    def "set_trace_func" | "add_trace_func" (_recv, func) {
        no_trace_func(func)
    }
    // A dead thread has no stack left to read, which is CRuby's nil; a live
    // one that is not the caller has a stack no other OS thread can walk.
    def "backtrace" cfunc (_recv, *_args) {
        if !thread_alive(t) {
            return Ok(RubyValue::Nil);
        }
        let lines = if is_current(t) { crate::frames::caller_lines(0) } else { Vec::new() };
        let lines = lines
            .into_iter()
            .map(|l| RubyValue::Str(crate::string_new(l)))
            .collect();
        Ok(RubyValue::Array(crate::array_new(lines)))
    }
    def "backtrace_locations" cfunc (_recv, *_args) {
        if !thread_alive(t) {
            return Ok(RubyValue::Nil);
        }
        let frames = if is_current(t) { crate::frames::caller_frames(0) } else { Vec::new() };
        let locations = frames
            .iter()
            .map(|(f, l, m)| crate::builtins::backtrace_location::location_new(f, *l, m))
            .collect();
        Ok(RubyValue::Array(crate::array_new(locations)))
    }

    // Two Thread objects are equal iff they are the same thread (identity).
    // Ruby declares none of the three on Thread -- they are Kernel's `==`/
    // `eql?` and BasicObject's `equal?`, all of which already mean identity.
    // The rows exist because a `RubyValue::Thread` receiver reaches this
    // table, so they are marked `inherits` and reflection looks past them.
    def "==" | "eql?" | "equal?" inherits (_recv, other) {
        let same = matches!(other, RubyValue::Thread(o) if std::sync::Arc::ptr_eq(t, o));
        Ok(RubyValue::Bool(same))
    }
}
