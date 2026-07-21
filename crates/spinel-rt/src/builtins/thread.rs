//! `Thread`'s Path-2 (runtime `send`) rows -- for a dynamically-typed Thread
//! receiver (a thread stored in an Array/Hash/ivar, or `threads.each { |t|
//! t.join }` where `t` is `Poly`). The static Path-1 codegen arm emits
//! `thread_outcome` directly and never reaches here; these rows mirror it,
//! so both paths agree on join/value semantics. The reflection/state rows
//! (name/status/`[]`/thread-variables) live only here.

use crate::dispatch::raise_error;
use crate::thread::{
    self, thread_alive, thread_kill, thread_list, thread_outcome, thread_raise, thread_status,
};
use crate::value::RubyValue;
use crate::{Signal, Symbol};

/// A Symbol/String storage-key argument as a `Symbol`.
fn key_sym(v: &RubyValue) -> Result<Symbol, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(raise_error(
            "TypeError",
            format!("{} is not a symbol nor a string", other.inspect_string()),
        )),
    }
}

// `Thread#join(limit = nil)` -- block until the thread finishes, re-raising a
// stored exception in the caller, then answer the thread itself. A timeout
// argument is accepted and ignored (this scheduler always runs to completion).
fn t_join(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    thread_outcome(&recv.as_thread_unchecked())?;
    Ok(recv.clone())
}

// `Thread#value` -- join, then answer the BLOCK's result (vs `join`'s thread).
fn t_value(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    thread_outcome(&recv.as_thread_unchecked())
}

// `Thread#alive?` -- true until the thread has finished (a non-joining peek).
fn t_alive_p(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(thread_alive(&recv.as_thread_unchecked())))
}

fn t_status(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(thread_status(&recv.as_thread_unchecked()))
}

/// `Thread#inspect`/`#to_s` -- `#<Thread:0xADDR STATUS>`, where a finished
/// thread reports `dead` (not the `false`/`nil` that `#status` answers).
fn t_inspect(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let t = recv.as_thread_unchecked();
    let status = if thread_alive(&t) { "run" } else { "dead" };
    let addr = std::sync::Arc::as_ptr(&t) as usize;
    Ok(RubyValue::Str(crate::string_new(format!("#<Thread:0x{addr:016x} {status}>"))))
}

fn t_name(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(thread::thread_name(&recv.as_thread_unchecked()))
}

fn t_set_name(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let name = match &args[0] {
        RubyValue::Nil => None,
        RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
        other => {
            return Err(raise_error(
                "TypeError",
                format!("no implicit conversion of {} into String", crate::builtins::convert_name_of(other)),
            ))
        }
    };
    thread::thread_set_name(&recv.as_thread_unchecked(), name);
    Ok(args[0].clone())
}

fn t_report_on_exception(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(thread::thread_report_on_exception(&recv.as_thread_unchecked())))
}

fn t_set_report_on_exception(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    thread::thread_set_report_on_exception(&recv.as_thread_unchecked(), args[0].truthy());
    Ok(args[0].clone())
}

fn t_aref(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(thread::thread_local_get(&recv.as_thread_unchecked(), key_sym(&args[0])?))
}

fn t_aset(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    thread::thread_local_set(&recv.as_thread_unchecked(), key_sym(&args[0])?, args[1].clone());
    Ok(args[1].clone())
}

fn t_key_p(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(thread::thread_local_key(&recv.as_thread_unchecked(), key_sym(&args[0])?)))
}

fn t_keys(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Array(crate::array_new(thread::thread_local_keys(&recv.as_thread_unchecked()))))
}

fn t_tvar_get(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(thread::thread_variable_get(&recv.as_thread_unchecked(), key_sym(&args[0])?))
}

fn t_tvar_set(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    thread::thread_variable_set(&recv.as_thread_unchecked(), key_sym(&args[0])?, args[1].clone());
    Ok(args[1].clone())
}

fn t_tvar_p(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(thread::thread_variable_key(&recv.as_thread_unchecked(), key_sym(&args[0])?)))
}

fn t_tvars(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Array(crate::array_new(thread::thread_variable_keys(&recv.as_thread_unchecked()))))
}

// `Thread#kill`/`#exit`/`#terminate` -- request termination, running the
// thread's `ensure` blocks; answers the thread itself.
fn t_kill(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    thread_kill(&recv.as_thread_unchecked());
    Ok(recv.clone())
}

// `Thread#raise(exc = RuntimeError)` -- inject an exception into the thread,
// caught by its next `rescue`. A String becomes a RuntimeError; an exception
// class/instance raises as itself (the shared `raise` coercion). A two-argument
// `raise(Class, message)` builds `Class` with that message.
fn t_raise(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let exc = match args {
        [] => crate::dispatch::coerce_raise_arg(RubyValue::Str(crate::string_new(String::new()))),
        [one] => crate::dispatch::coerce_raise_arg(one.clone()),
        [class, msg, ..] => {
            // `Class.exception(message)` -- the CRuby two-arg form.
            crate::dispatch::send_value(class, crate::Symbol::intern("exception"), &[msg.clone()], None)?
        }
    };
    thread_raise(&recv.as_thread_unchecked(), exc);
    Ok(RubyValue::Nil)
}

// Two Thread objects are equal iff they are the same thread (identity).
fn t_eq(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let same = matches!(&args[0], RubyValue::Thread(o) if std::sync::Arc::ptr_eq(&recv.as_thread_unchecked(), o));
    Ok(RubyValue::Bool(same))
}

/// `Method#arity` twin of `lookup` -- this hand-rolled table declares no
/// per-method argc, so every method it defines reports CRuby's
/// variadic-cfunc default (`-1`).
pub fn lookup_arity(name: &str) -> Option<i64> {
    lookup(name).map(|_| -1)
}

pub fn lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "join" => t_join,
        "value" => t_value,
        "kill" | "exit" | "terminate" => t_kill,
        "raise" => t_raise,
        "alive?" => t_alive_p,
        "status" => t_status,
        "name" => t_name,
        "name=" => t_set_name,
        "report_on_exception" => t_report_on_exception,
        "report_on_exception=" => t_set_report_on_exception,
        "[]" => t_aref,
        "[]=" => t_aset,
        "key?" => t_key_p,
        "keys" => t_keys,
        "thread_variable_get" => t_tvar_get,
        "thread_variable_set" => t_tvar_set,
        "thread_variable?" => t_tvar_p,
        "thread_variables" => t_tvars,
        "==" | "eql?" | "equal?" => t_eq,
        "inspect" | "to_s" => t_inspect,
        _ => return None,
    })
}

/// Reflection companion to `lookup` (hand-written table).
pub fn lookup_names() -> &'static [&'static str] {
    &[
        "join", "value", "kill", "exit", "terminate", "raise", "alive?", "status", "name", "name=",
        "report_on_exception", "report_on_exception=", "[]", "[]=", "key?",
        "keys", "thread_variable_get", "thread_variable_set",
        "thread_variable?", "thread_variables", "==", "eql?", "equal?", "inspect", "to_s",
    ]
}

// --- Class methods (`Thread.current`/`.main`/`.pass`) -----------------------

fn c_current(_recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(thread::thread_current())
}

fn c_main(_recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(thread::thread_main())
}

fn c_pass(_recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(thread::thread_pass())
}

/// `Thread.report_on_exception` -- the process-wide default a new thread
/// inherits (true by CRuby default); setting it false silences the
/// at-termination stderr report.
static REPORT_ON_EXCEPTION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn c_report_on_exception(_recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(REPORT_ON_EXCEPTION.load(std::sync::atomic::Ordering::Relaxed)))
}

fn c_set_report_on_exception(_recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    REPORT_ON_EXCEPTION.store(args[0].truthy(), std::sync::atomic::Ordering::Relaxed);
    Ok(args[0].clone())
}

pub fn lookup_class(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "current" => c_current,
        "main" => c_main,
        "list" => c_list,
        "pass" => c_pass,
        "report_on_exception" => c_report_on_exception,
        "report_on_exception=" => c_set_report_on_exception,
        _ => return None,
    })
}

pub fn lookup_class_names() -> &'static [&'static str] {
    &["current", "main", "list", "pass", "report_on_exception", "report_on_exception="]
}

/// `Thread.list` -- main plus every still-running spawned thread, from the
/// process-wide live registry (finished/joined threads are pruned out).
fn c_list(_recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Array(crate::array_new(thread_list())))
}
