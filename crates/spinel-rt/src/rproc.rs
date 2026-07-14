//! The real, escaping `Proc`/closure type (Phase 6) -- deliberately a plain
//! Rust closure type, not a hand-rolled trait + per-call-site env struct: a
//! `move |args| { ... }` closure generated at each escaping block's call
//! site already IS a concrete, heap-allocated environment (Rust's own
//! compiler builds it for us), so there is nothing left for a hand-rolled
//! `ProcBody` trait to add except ceremony. `redo` is handled entirely
//! inside the closure body (a plain `loop` around the block's own code, see
//! `codegen`'s Proc-construction docs) -- no runtime support needed beyond
//! `Signal::Redo` already existing.
//!
//! Must be `'static`: `RubyValue` (which stores this) has no lifetime
//! parameter anywhere in this codebase, so anything it holds has to be
//! independently owned, not borrowed -- this is exactly why an escaping
//! block captures OWNED `Rc<RefCell<RubyValue>>` cells (and an owned
//! `Rc<Self>` for `self`/ivar access) rather than references.

use crate::{RubyValue, Signal};
use std::rc::Rc;

pub type RProc = Rc<dyn Fn(&[RubyValue]) -> Result<RubyValue, Signal>>;
