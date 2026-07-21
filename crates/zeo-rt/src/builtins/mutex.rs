//! `Mutex`'s Path-2 (runtime `send`) rows -- for a dynamically-typed Mutex
//! receiver (a mutex held in an Array/Hash/ivar, or a `Poly` `send` target).
//! The static Path-1 codegen arm (`codegen::call`) emits `mutex_lock`/
//! `mutex_unlock`/... directly and never reaches here; these rows mirror it
//! exactly -- same `ThreadError` message text, same self-returning shape --
//! so both paths agree. `Mutex` has a dedicated `RubyValue::Mutex` variant
//! (like Thread/Fiber), so a row unwraps it with `as_mutex_unchecked` rather
//! than downcasting an `Object` the way `ConditionVariable` does.

use crate::RubyValue;
use crate::builtins::{arity, builtin_methods, need_block, thread_error};
use crate::thread::{
    mutex_lock, mutex_locked, mutex_new, mutex_owned, mutex_try_lock, mutex_unlock,
};

/// A runtime lock/unlock `Err(&str)` as the `ThreadError` CRuby raises -- the
/// runtime carries the exact message, exception construction is ours.
fn thread_error(msg: &str) -> crate::Signal {
    thread_error!("{msg}")
}

builtin_methods! {
    pub(crate) fn lookup;

    // `lock`/`unlock` return self; a recursive or foreign lock/unlock is a
    // `ThreadError` carrying the runtime's message verbatim.
    "lock" => fn lock(recv, args, _block) {
        arity!(args, 0);
        let m = recv.as_mutex_unchecked();
        mutex_lock(&m).map_err(thread_error)?;
        Ok(RubyValue::Mutex(m))
    }
    "unlock" => fn unlock(recv, args, _block) {
        arity!(args, 0);
        let m = recv.as_mutex_unchecked();
        mutex_unlock(&m).map_err(thread_error)?;
        Ok(RubyValue::Mutex(m))
    }
    "locked?" => fn locked_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mutex_locked(&recv.as_mutex_unchecked())))
    }
    // `try_lock` -- acquire without blocking; `true` iff it was free.
    "try_lock" => fn try_lock(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mutex_try_lock(&recv.as_mutex_unchecked())))
    }
    "owned?" => fn owned_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mutex_owned(&recv.as_mutex_unchecked())))
    }
    // `synchronize { }` -- lock, run the block, ALWAYS unlock (even on a
    // signal: an exception/`break` inside the block must release the lock on
    // its way out), then re-propagate. `break` exits it with the break value.
    "synchronize" => fn synchronize(recv, _args, block) {
        let blk = need_block!(block);
        let m = recv.as_mutex_unchecked();
        mutex_lock(&m).map_err(thread_error)?;
        let r = crate::catch_break(blk.call(&[]));
        let _ = mutex_unlock(&m);
        r
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" => fn new_m(_recv, args, _block) {
        arity!(args, 0);
        Ok(mutex_new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_the_dynamic_mutex_surface() {
        assert!(lookup("lock").is_some());
        assert!(lookup("synchronize").is_some());
        assert!(lookup("nope").is_none());
        assert!(lookup_class("new").is_some());
    }

    #[test]
    fn lock_owned_unlock_round_trips() {
        let m = mutex_new();
        assert_eq!(owned_p(&m, &[], None).unwrap().inspect_string(), "false");
        lock(&m, &[], None).unwrap();
        assert_eq!(locked_p(&m, &[], None).unwrap().inspect_string(), "true");
        assert_eq!(owned_p(&m, &[], None).unwrap().inspect_string(), "true");
        unlock(&m, &[], None).unwrap();
        assert_eq!(locked_p(&m, &[], None).unwrap().inspect_string(), "false");
    }

    #[test]
    fn unlocking_an_unlocked_mutex_is_a_thread_error() {
        // Without a ClassRegistry installed the raise surfaces as a panic;
        // the point is that it does not silently succeed.
        let m = mutex_new();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unlock(&m, &[], None)));
        assert!(r.is_err() || r.unwrap().is_err());
    }
}
