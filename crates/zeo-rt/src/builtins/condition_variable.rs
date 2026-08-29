//! `ConditionVariable` (CRuby `Thread::ConditionVariable`, exposed top-level
//! as `ConditionVariable` -- matching how `Mutex`/`Queue` are already
//! simplified from `Thread::*`). A runtime-resident `RObj` over a
//! parking_lot condvar.
//!
//! `#wait(mutex, timeout=nil)` atomically releases the Ruby `Mutex` and parks:
//! the internal handoff lock is held across the release so a concurrent
//! `#signal`/`#broadcast` (which must take that same lock) cannot slip a wakeup
//! in between the release and the park -- the classic no-lost-wakeup guarantee.
//! On wake (or timeout) the Ruby mutex is re-acquired before returning.

use crate::RubyValue;
use crate::builtins::{thread_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use zeo_abi::CONDITION_VARIABLE_CLASS;
use zeo_macros::ruby_class;

pub struct RConditionVariable {
    /// Guards the wait/signal handoff -- see the module docs' no-lost-wakeup
    /// note. The `()` payload is unused; the lock itself is the token.
    lock: parking_lot::Mutex<()>,
    cond: parking_lot::Condvar,
    frozen: AtomicBool,
}

impl RConditionVariable {
    fn new() -> RConditionVariable {
        RConditionVariable {
            lock: parking_lot::Mutex::new(()),
            cond: parking_lot::Condvar::new(),
            frozen: AtomicBool::new(false),
        }
    }
}

impl RubyObject for RConditionVariable {
    fn class_id(&self) -> crate::ClassId {
        CONDITION_VARIABLE_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        // A dup is a fresh, independent condition variable (waiters/native
        // state never carry over) -- CRuby dups these to a clean object too.
        let cv = RConditionVariable::new();
        if copy_frozen {
            cv.set_frozen();
        }
        Arc::new(cv)
    }
}

/// A fresh `ConditionVariable` value.
fn new_cv() -> RubyValue {
    RubyValue::Object(Arc::new(RConditionVariable::new()))
}

/// The `RConditionVariable` behind a receiver -- the table only dispatches on one.
fn cv_of(recv: &RubyValue) -> &RConditionVariable {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RConditionVariable>()
            .expect("the ConditionVariable table only dispatches on CV receivers"),
        _ => unreachable!("the ConditionVariable table only dispatches on CV receivers"),
    }
}

ruby_class! {
    ConditionVariable = zeo_abi::CONDITION_VARIABLE_CLASS < zeo_abi::OBJECT_CLASS;

    // Nothing to initialize: a condition variable with no waiters IS the
    // blank, which is why ruby lets `allocate` answer a usable one.
    allocate new_cv;

    def self."new" cfunc (_recv) {
        Ok(new_cv())
    }

    // `wait(mutex, timeout=nil)` -- release `mutex`, park until signaled or
    // `timeout` seconds elapse, re-acquire `mutex`, return self.
    def "wait" params "mutex, timeout = nil"(recv, arg1, arg2?) {
        let RubyValue::Mutex(rm) = arg1 else {
            return Err(type_error!("no implicit conversion into Mutex"));
        };
        let timeout = match arg2 {
            None | Some(RubyValue::Nil) => None,
            Some(RubyValue::Int(i)) => Some(Duration::from_secs_f64(*i as f64)),
            Some(RubyValue::Float(f)) => Some(Duration::from_secs_f64(*f)),
            Some(other) => {
                return Err(crate::builtins::no_implicit(other, "Float"));
            }
        };
        let cv = cv_of(recv);
        // Hold the handoff lock across the mutex release so a signaller can't
        // race a wakeup in before we park.
        let mut guard = cv.lock.lock();
        if let Err(msg) = crate::thread::mutex_unlock(rm) {
            return Err(thread_error!("{msg}"));
        }
        // The park itself runs with an armed process Gvl released (a no-op
        // when disabled, the default) -- the signaller needs to RUN to
        // signal.
        crate::gvl::without_gvl(|| {
            match timeout {
                None => cv.cond.wait(&mut guard),
                Some(dur) => {
                    let _ = cv.cond.wait_for(&mut guard, dur);
                }
            }
            drop(guard);
        });
        crate::thread::mutex_lock(rm).map_err(crate::thread::WaitFailure::signal)?;
        Ok(recv.clone())
    }
    // Wake at most one waiter; returns self.
    // Re-init is a permitted no-op, like Mutex's.
    private def "initialize"(_recv) {
        Ok(RubyValue::Nil)
    }
    def "signal"(recv) {
        let cv = cv_of(recv);
        let _g = cv.lock.lock();
        crate::gvl::deadlock::note_progress();
        cv.cond.notify_one();
        Ok(recv.clone())
    }
    // Wake all waiters; returns self.
    def "broadcast"(recv) {
        let cv = cv_of(recv);
        let _g = cv.lock.lock();
        crate::gvl::deadlock::note_progress();
        cv.cond.notify_all();
        Ok(recv.clone())
    }
    // A condition variable owns a condvar and its parked waiters, none of
    // which survives a round trip, so CRuby refuses to dump one.
    def "marshal_dump"(recv) {
        Err(type_error!("can't dump {}", crate::builtins::class_name_of(recv)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ConditionVariable`'s `ruby_class!`-generated instance methods are
    /// reachable only through the dispatch table (their Rust fn names are
    /// mangled), so the tests call them through the registered lookup.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(CONDITION_VARIABLE_CLASS)
            .expect("ConditionVariable is a registered builtin table")
            .instance
            .as_ref()
            .expect("ConditionVariable has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("ConditionVariable#{name} is defined"))
    }

    #[test]
    fn new_produces_a_condition_variable_object() {
        let cv = new_cv();
        assert!(matches!(&cv, RubyValue::Object(o) if o.class_id() == CONDITION_VARIABLE_CLASS));
    }

    #[test]
    fn signal_and_broadcast_return_self_and_dont_block_without_waiters() {
        let cv = new_cv();
        // Signaling/broadcasting a waiter-less CV is a no-op that returns self.
        let r = imethod("signal")(&cv, &[], None).unwrap();
        assert!(matches!(&r, RubyValue::Object(o) if o.class_id() == CONDITION_VARIABLE_CLASS));
        let r = imethod("broadcast")(&cv, &[], None).unwrap();
        assert!(matches!(&r, RubyValue::Object(o) if o.class_id() == CONDITION_VARIABLE_CLASS));
    }

    #[test]
    fn dup_is_a_fresh_independent_object() {
        let RubyValue::Object(o) = new_cv() else {
            unreachable!()
        };
        let dup = o.dup_object(false);
        assert_eq!(dup.class_id(), CONDITION_VARIABLE_CLASS);
        assert!(!Arc::ptr_eq(
            &(o.as_any_rc().downcast::<RConditionVariable>().unwrap()),
            &(dup.as_any_rc().downcast::<RConditionVariable>().unwrap())
        ));
    }
}
