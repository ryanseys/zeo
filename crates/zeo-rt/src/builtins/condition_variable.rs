//! `ConditionVariable` (CRuby `Thread::ConditionVariable`, exposed top-level
//! as `ConditionVariable` -- matching how `Mutex`/`Queue` are already
//! simplified from `Thread::*`). A runtime-resident `RObj` wrapping a `may`
//! `Condvar` so `#wait` yields the coroutine (like `Queue#pop`) rather than
//! blocking the single-worker scheduler.
//!
//! `#wait(mutex, timeout=nil)` atomically releases the Ruby `Mutex` and parks:
//! the internal `may::sync::Mutex` is held across the release so a concurrent
//! `#signal`/`#broadcast` (which must take that same lock) cannot slip a wakeup
//! in between the release and the park -- the classic no-lost-wakeup guarantee.
//! On wake (or timeout) the Ruby mutex is re-acquired before returning.

use crate::RubyValue;
use crate::builtins::{arity, builtin_methods, thread_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use zeo_abi::CONDITION_VARIABLE_CLASS;

pub struct RConditionVariable {
    /// Guards the wait/signal handoff -- see the module docs' no-lost-wakeup
    /// note. The `()` payload is unused; the lock itself is the token.
    lock: may::sync::Mutex<()>,
    cond: may::sync::Condvar,
    frozen: AtomicBool,
}

impl RConditionVariable {
    fn new() -> RConditionVariable {
        RConditionVariable {
            lock: may::sync::Mutex::new(()),
            cond: may::sync::Condvar::new(),
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

builtin_methods! {
    pub(crate) fn lookup;

    // `wait(mutex, timeout=nil)` -- release `mutex`, park until signaled or
    // `timeout` seconds elapse, re-acquire `mutex`, return self.
    "wait" => fn wait(recv, args, _block) {
        arity!(args, 1..=2);
        let RubyValue::Mutex(rm) = &args[0] else {
            return Err(type_error!("no implicit conversion into Mutex"));
        };
        let timeout = match args.get(1) {
            None | Some(RubyValue::Nil) => None,
            Some(RubyValue::Int(i)) => Some(Duration::from_secs_f64(*i as f64)),
            Some(RubyValue::Float(f)) => Some(Duration::from_secs_f64(*f)),
            Some(other) => {
                return Err(type_error!("no implicit conversion of {} into Float", crate::builtins::convert_name_of(other)));
            }
        };
        let cv = cv_of(recv);
        // Hold the handoff lock across the mutex release so a signaller can't
        // race a wakeup in before we park.
        let guard = cv.lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(msg) = crate::thread::mutex_unlock(rm) {
            return Err(thread_error!("{msg}"));
        }
        match timeout {
            None => {
                let g = cv.cond.wait(guard).unwrap_or_else(|e| e.into_inner());
                drop(g);
            }
            Some(dur) => {
                let (g, _timed_out) =
                    cv.cond.wait_timeout(guard, dur).unwrap_or_else(|e| e.into_inner());
                drop(g);
            }
        }
        if let Err(msg) = crate::thread::mutex_lock(rm) {
            return Err(thread_error!("{msg}"));
        }
        Ok(recv.clone())
    }
    // Wake at most one waiter; returns self.
    "signal" => fn signal(recv, args, _block) {
        arity!(args, 0);
        let cv = cv_of(recv);
        let _g = cv.lock.lock().unwrap_or_else(|e| e.into_inner());
        cv.cond.notify_one();
        Ok(recv.clone())
    }
    // Wake all waiters; returns self.
    "broadcast" => fn broadcast(recv, args, _block) {
        arity!(args, 0);
        let cv = cv_of(recv);
        let _g = cv.lock.lock().unwrap_or_else(|e| e.into_inner());
        cv.cond.notify_all();
        Ok(recv.clone())
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" => fn new_m(_recv, args, _block) {
        arity!(args, 0);
        Ok(new_cv())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_produces_a_condition_variable_object() {
        let cv = new_cv();
        assert!(matches!(&cv, RubyValue::Object(o) if o.class_id() == CONDITION_VARIABLE_CLASS));
    }

    #[test]
    fn signal_and_broadcast_return_self_and_dont_block_without_waiters() {
        let cv = new_cv();
        // Signaling/broadcasting a waiter-less CV is a no-op that returns self.
        let r = signal(&cv, &[], None).unwrap();
        assert!(matches!(&r, RubyValue::Object(o) if o.class_id() == CONDITION_VARIABLE_CLASS));
        let r = broadcast(&cv, &[], None).unwrap();
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
