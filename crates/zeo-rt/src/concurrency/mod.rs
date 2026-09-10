//! The four things that run ruby code somewhere other than here: a native
//! thread, a fiber, a Ractor, and the lock that decides which of them holds
//! the interpreter.
//!
//! Each carries an execution context of its own ([`ec`]), and each runs on a
//! stack whose overflow floor it sets ([`stack_guard`]). `Signal`, the
//! control-flow enum, lives at the crate root -- it is not a thread.

pub(crate) mod coroutine;
pub(crate) mod ec;
pub(crate) mod fiber;
pub mod gvl;
pub(crate) mod ractor;
pub(crate) mod stack_guard;
pub(crate) mod thread;
