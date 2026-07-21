//! `Monitor` -- the reentrant lock behind `require "monitor"`.
//!
//! The one thing that distinguishes it from `Mutex` is re-entrancy: a `Mutex`
//! answers `ThreadError: deadlock; recursive locking` when its owner locks it
//! again (`thread.rs`'s `mutex_lock`), whereas a `Monitor` just counts the
//! nesting and unlocks once on the way back out. CRuby builds it the same way
//! -- `ext/monitor/monitor.c`'s `rb_monitor` is a mutex plus an owner and a
//! `count`, and `lib/monitor.rb`'s `MonitorMixin` is a thin delegation layer
//! over exactly that object held in `@mon_data`.
//!
//! Ownership is asked of the underlying mutex rather than tracked separately,
//! so there is one source of truth: `count` is only ever read or written by
//! the execution that holds the lock, which is what makes a plain relaxed
//! atomic sufficient here.
//!
//! **Not implemented: `MonitorMixin`.** Its methods all delegate through a
//! `@mon_data` ivar that `extend_object`/`mon_initialize` install, which is a
//! module-mixin lifecycle this runtime has no other user for. `include
//! MonitorMixin` is therefore a loud `NameError` rather than a half-working
//! mixin whose `@mon_data` is nil.

use crate::RubyValue;
use crate::builtins::{arity, builtin_methods, need_block};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::thread::{
    RMutex, mutex_lock, mutex_locked, mutex_new, mutex_owned, mutex_try_lock, mutex_unlock,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use zeo_abi::MONITOR_CLASS;

pub struct RMonitor {
    mutex: RMutex,
    /// Nesting depth: 0 when unheld, incremented per re-entry. Only the
    /// owning execution touches it, and only while holding `mutex`.
    count: AtomicU64,
    frozen: AtomicBool,
}

impl RMonitor {
    fn new() -> RMonitor {
        let RubyValue::Mutex(mutex) = mutex_new() else {
            unreachable!("mutex_new answers a Mutex");
        };
        RMonitor {
            mutex,
            count: AtomicU64::new(0),
            frozen: AtomicBool::new(false),
        }
    }

    /// Acquire, blocking unless this execution already owns it.
    fn enter(&self) -> Result<(), crate::Signal> {
        if !mutex_owned(&self.mutex) {
            mutex_lock(&self.mutex).map_err(thread_error)?;
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Release one nesting level, unlocking only at the outermost.
    fn exit(&self) -> Result<(), crate::Signal> {
        if !mutex_owned(&self.mutex) {
            return Err(thread_error("current thread not owner"));
        }
        if self.count.fetch_sub(1, Ordering::Relaxed) == 1 {
            mutex_unlock(&self.mutex).map_err(thread_error)?;
        }
        Ok(())
    }
}

fn thread_error(msg: &str) -> crate::Signal {
    raise_error("ThreadError", msg.to_string())
}

impl RubyObject for RMonitor {
    fn class_id(&self) -> crate::ClassId {
        MONITOR_CLASS
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
        // A dup is a fresh, unheld monitor -- lock state never carries over,
        // the same choice `ConditionVariable` makes.
        let m = RMonitor::new();
        if copy_frozen {
            m.set_frozen();
        }
        Arc::new(m)
    }
}

/// The `RMonitor` behind a receiver -- the table only dispatches on one.
fn monitor_of(recv: &RubyValue) -> &RMonitor {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RMonitor>()
            .expect("the Monitor table only dispatches on Monitor receivers"),
        _ => unreachable!("the Monitor table only dispatches on Monitor receivers"),
    }
}

builtin_methods! {
    pub(crate) fn lookup;

    // `mon_enter` is the `MonitorMixin` spelling of `enter`; both names reach
    // the same C function in CRuby, so both are rows here.
    "enter" | "mon_enter" => fn enter(recv, args, _block) {
        arity!(args, 0);
        monitor_of(recv).enter()?;
        Ok(RubyValue::Nil)
    }
    "exit" | "mon_exit" => fn exit(recv, args, _block) {
        arity!(args, 0);
        monitor_of(recv).exit()?;
        Ok(RubyValue::Nil)
    }
    // `try_enter` never blocks: `true` iff this execution now holds it,
    // which a re-entry always does.
    "try_enter" | "mon_try_enter" => fn try_enter(recv, args, _block) {
        arity!(args, 0);
        let m = monitor_of(recv);
        if !mutex_owned(&m.mutex) && !mutex_try_lock(&m.mutex) {
            return Ok(RubyValue::Bool(false));
        }
        m.count.fetch_add(1, Ordering::Relaxed);
        Ok(RubyValue::Bool(true))
    }
    "mon_locked?" => fn locked_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mutex_locked(&monitor_of(recv).mutex)))
    }
    "mon_owned?" => fn owned_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mutex_owned(&monitor_of(recv).mutex)))
    }
    // Enter, run the block, ALWAYS leave -- an exception or `break` out of
    // the block must still unwind one nesting level. Same shape as
    // `Mutex#synchronize`, which is what `mon_synchronize` aliases to.
    "synchronize" | "mon_synchronize" => fn synchronize(recv, _args, block) {
        let blk = need_block!(block);
        let m = monitor_of(recv);
        m.enter()?;
        let r = crate::catch_break(blk.call(&[]));
        let _ = m.exit();
        r
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" => fn new_monitor(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Object(Arc::new(RMonitor::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_monitor() -> RubyValue {
        RubyValue::Object(Arc::new(RMonitor::new()))
    }

    #[test]
    fn the_table_covers_the_monitor_surface() {
        assert!(lookup("synchronize").is_some());
        assert!(lookup("mon_enter").is_some());
        assert!(lookup("nope").is_none());
        assert!(lookup_class("new").is_some());
    }

    #[test]
    fn enter_is_reentrant_and_unlocks_only_at_the_outermost_exit() {
        let m = a_monitor();
        let inner = monitor_of(&m);
        inner.enter().unwrap();
        inner.enter().unwrap();
        assert!(mutex_locked(&inner.mutex), "still held after one exit");
        inner.exit().unwrap();
        assert!(mutex_locked(&inner.mutex), "still held after one exit");
        inner.exit().unwrap();
        assert!(!mutex_locked(&inner.mutex));
    }

    #[test]
    fn exiting_an_unheld_monitor_is_a_thread_error() {
        // Without a ClassRegistry installed the raise surfaces as a panic;
        // the point is that it does not silently succeed.
        let m = a_monitor();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| monitor_of(&m).exit()));
        assert!(r.is_err() || r.unwrap().is_err());
    }
}
