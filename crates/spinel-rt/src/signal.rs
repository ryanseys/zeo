//! Non-local control-flow propagation. Every generated method/closure body
//! returns `Result<RubyValue, Signal>` instead of a bare `RubyValue`, decided
//! once, up front, rather than retrofitted later: the moment any of
//! `break`/`next`/`redo`/`retry`/`raise`/an explicit `return` needs to unwind
//! across a function or closure boundary, a bare return value can't carry
//! "and also, unwind" information, and changing every already-generated
//! method/closure/trampoline signature after several phases assume a bare
//! `RubyValue` would be a much larger rewrite than fixing the ABI once, now.
//!
//! Most variants are dormant until later phases: loop-scoped `break`/`next`/
//! `redo` compile to native Rust `break`/`continue` on a labeled loop and
//! never touch `Signal` at all (see codegen's loop lowering); `Signal::Break`/
//! `Next`/`Redo` only become load-bearing once a block can be a real escaping
//! `Proc` invoked from a different Rust function than the loop that owns it,
//! and `Signal::Raise` isn't populated until `raise`/`rescue` exist. `Raise`'s
//! payload is a plain `RubyValue` for now (any raised exception is just a
//! value at this stage) rather than a dedicated exception type -- introducing
//! a `RubyException` type is deferred to the phase that actually needs
//! exception-class/backtrace machinery.

use crate::RubyValue;

#[derive(Clone, Debug)]
pub enum Signal {
    Break(RubyValue),
    Next(RubyValue),
    Redo,
    Retry,
    Return(RubyValue),
    Raise(RubyValue),
}
