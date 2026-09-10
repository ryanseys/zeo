//! `Kernel#catch`/`Kernel#throw`: the per-context live-catch-tag stack and
//! the identity-matched unwind. Core control-flow machinery (an `Ec` slice,
//! see `crate::ec`), not a method table -- which is why it lives here and
//! not in `builtins/kernel.rs`.

use crate::{RubyValue, Signal};

// This context's stack of tags with a live `catch` frame. `throw` consults
// it so an unmatched tag becomes an `UncaughtThrowError` AT THE THROW (as in
// CRuby), rather than a `Signal::Throw` leaking past every `rescue` to the top
// level. Per-context: each `Thread`/`Fiber` unwinds its own catch frames.
std::thread_local!(static CATCH_TAGS: std::cell::RefCell<Vec<RubyValue>> = const { std::cell::RefCell::new(Vec::new()) });

/// Install `new` as this context's live-catch-tag stack, returning the
/// previous one -- the fiber ec-swap's slice of this cell (see
/// `crate::ec`). Oracle-pinned: a `throw` inside a fiber cannot see the
/// resumer's `catch`.
pub(crate) fn swap_catch_tags(new: Vec<RubyValue>) -> Vec<RubyValue> {
    CATCH_TAGS.with(|s| s.replace(new))
}

/// `Kernel#catch(tag) { ... }` / `Kernel#throw(tag[, value])`.
///
/// A block-less `catch` is `rb_need_block`'s LocalJumpError, whose message
/// carries no `(yield)` suffix -- unlike a bare `yield` with no block.
pub fn kernel_catch(tag: RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = &block else {
        return Err(crate::builtins::local_jump_error!("no block given"));
    };
    CATCH_TAGS.with(|s| s.borrow_mut().push(tag.clone()));
    let result = p.call(std::slice::from_ref(&tag));
    CATCH_TAGS.with(|s| {
        s.borrow_mut().pop();
    });
    match result {
        // IDENTITY, not `==`: ruby matches a throw to its catch by object, so
        // `catch("s") { throw "s" }` does NOT match -- the two literals are
        // different objects, and the throw escapes as an UncaughtThrowError.
        // Symbols and Integers still match, being identical by value.
        Err(Signal::Throw(t)) if crate::builtins::basic_object::value_identity(&t.tag, &tag) => {
            Ok(t.value)
        }
        other => other,
    }
}

pub(crate) fn throw_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let tag = args[0].clone();
    // Only a tag with a live `catch` frame may unwind; otherwise it is an
    // `UncaughtThrowError` right here, catchable by an ordinary `rescue`.
    // Identity again, for the same reason as `kernel_catch`'s match.
    let has_live_catch = CATCH_TAGS.with(|s| {
        s.borrow()
            .iter()
            .any(|t| crate::builtins::basic_object::value_identity(t, &tag))
    });
    if has_live_catch {
        Err(Signal::Throw(Box::new(crate::signal::Thrown {
            tag,
            value: args.get(1).cloned().unwrap_or(RubyValue::Nil),
        })))
    } else {
        Err(crate::builtins::exception::raise_uncaught_throw(
            tag.clone(),
            args.get(1).cloned().unwrap_or(RubyValue::Nil),
        ))
    }
}
