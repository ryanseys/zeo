//! `Thread`'s Path-2 (runtime `send`) rows -- for a dynamically-typed Thread
//! receiver (a thread stored in an Array/Hash/ivar, or `threads.each { |t|
//! t.join }` where `t` is `Poly`). The static Path-1 codegen arm emits
//! `thread_outcome` directly and never reaches here; these rows mirror it,
//! so both paths agree on join/value semantics.

use crate::thread::{thread_alive, thread_outcome};
use crate::value::RubyValue;
use crate::Signal;

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

pub fn lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "join" => t_join,
        "value" => t_value,
        "alive?" => t_alive_p,
        _ => return None,
    })
}

/// Reflection companion to `lookup` (hand-written table).
pub fn lookup_names() -> &'static [&'static str] {
    &["join", "value", "alive?"]
}
