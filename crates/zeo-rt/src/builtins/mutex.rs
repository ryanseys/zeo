//! `Mutex`'s Path-2 (runtime `send`) rows -- for a dynamically-typed Mutex
//! receiver (a mutex held in an Array/Hash/ivar, or a `Poly` `send` target).
//! The static Path-1 codegen arm (`codegen::call`) emits `mutex_lock`/
//! `mutex_unlock`/... directly and never reaches here; these rows mirror it
//! exactly -- same `ThreadError` message text, same self-returning shape --
//! so both paths agree. `Mutex` has a dedicated `RubyValue::Mutex` variant
//! (like Thread/Fiber), so a row unwraps it with `as_mutex_unchecked` rather
//! than downcasting an `Object` the way `ConditionVariable` does.

use crate::RubyValue;
use crate::builtins::{need_block, thread_error};
use crate::thread::{
    mutex_lock, mutex_locked, mutex_new, mutex_owned, mutex_try_lock, mutex_unlock,
};
use zeo_macros::ruby_class;

/// A runtime lock/unlock `Err(&str)` as the `ThreadError` CRuby raises -- the
/// runtime carries the exact message, exception construction is ours.
fn thread_error(msg: &str) -> crate::Signal {
    thread_error!("{msg}")
}

ruby_class! {
    Mutex = zeo_abi::MUTEX_CLASS < zeo_abi::OBJECT_CLASS;

    def self."new" cfunc (_recv) {
        Ok(mutex_new())
    }

    // `lock`/`unlock` return self; a recursive or foreign lock/unlock is a
    // `ThreadError` carrying the runtime's message verbatim.
    def "lock"(recv) {
        let m = recv.as_mutex_unchecked();
        mutex_lock(&m).map_err(thread_error)?;
        Ok(RubyValue::Mutex(m))
    }
    def "unlock"(recv) {
        let m = recv.as_mutex_unchecked();
        mutex_unlock(&m).map_err(thread_error)?;
        Ok(RubyValue::Mutex(m))
    }
    // `#sleep(timeout = nil)` -- release the lock, sleep, re-acquire. The
    // answer is the whole-seconds count `Kernel#sleep` gives.
    def "sleep" cfunc (recv, timeout?) {
        let m = recv.as_mutex_unchecked();
        mutex_unlock(&m).map_err(thread_error)?;
        let slept = crate::builtins::kernel::sleep_impl(match timeout {
            Some(v) if !v.is_nil() => std::slice::from_ref(v),
            _ => &[],
        });
        mutex_lock(&m).map_err(thread_error)?;
        // CRuby answers nil, not the elapsed seconds `Kernel#sleep` gives.
        slept.map(|_| RubyValue::Nil)
    }
    // Re-init is a permitted no-op (oracle: nil even on a frozen Mutex).
    private def "initialize"(_recv) {
        Ok(RubyValue::Nil)
    }
    def "locked?"(recv) {
        Ok(RubyValue::Bool(mutex_locked(&recv.as_mutex_unchecked())))
    }
    // `try_lock` -- acquire without blocking; `true` iff it was free.
    def "try_lock"(recv) {
        Ok(RubyValue::Bool(mutex_try_lock(&recv.as_mutex_unchecked())))
    }
    def "owned?"(recv) {
        Ok(RubyValue::Bool(mutex_owned(&recv.as_mutex_unchecked())))
    }
    // `synchronize { }` -- lock, run the block, ALWAYS unlock (even on a
    // signal: an exception/`break` inside the block must release the lock on
    // its way out), then re-propagate. `break` exits it with the break value.
    def "synchronize"(recv, &block) {
        let blk = need_block!(block);
        let m = recv.as_mutex_unchecked();
        mutex_lock(&m).map_err(thread_error)?;
        let r = crate::catch_break(blk.call(&[]));
        let _ = mutex_unlock(&m);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `ruby_class!`-generated methods are reachable only through the
    /// dispatch tables (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through Mutex's registered lookups.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::MUTEX_CLASS)
            .expect("Mutex is a registered builtin table")
            .instance
            .as_ref()
            .expect("Mutex has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Mutex#{name} is defined"))
    }

    #[test]
    fn the_tables_cover_the_dynamic_mutex_surface() {
        let t = crate::builtins::registered_table(zeo_abi::MUTEX_CLASS).unwrap();
        let inst = t.instance.as_ref().unwrap();
        assert!((inst.lookup)("lock").is_some());
        assert!((inst.lookup)("synchronize").is_some());
        assert!((inst.lookup)("nope").is_none());
        assert!((t.class.as_ref().unwrap().lookup)("new").is_some());
    }

    #[test]
    fn lock_owned_unlock_round_trips() {
        let m = mutex_new();
        assert_eq!(
            imethod("owned?")(&m, &[], None).unwrap().inspect_string(),
            "false"
        );
        imethod("lock")(&m, &[], None).unwrap();
        assert_eq!(
            imethod("locked?")(&m, &[], None).unwrap().inspect_string(),
            "true"
        );
        assert_eq!(
            imethod("owned?")(&m, &[], None).unwrap().inspect_string(),
            "true"
        );
        imethod("unlock")(&m, &[], None).unwrap();
        assert_eq!(
            imethod("locked?")(&m, &[], None).unwrap().inspect_string(),
            "false"
        );
    }

    #[test]
    fn unlocking_an_unlocked_mutex_is_a_thread_error() {
        // Without a ClassRegistry installed the raise surfaces as a panic;
        // the point is that it does not silently succeed.
        let m = mutex_new();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            imethod("unlock")(&m, &[], None)
        }));
        assert!(r.is_err() || r.unwrap().is_err());
    }
}
