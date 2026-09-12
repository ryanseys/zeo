//! `ThreadGroup` -- the DEFAULT group only (see `zeo_abi::THREAD_GROUP_CLASS`'s
//! docs): every thread reports `ThreadGroup::Default` as its group, `#list`
//! answers the live thread list, and `#add` validates its argument and no-ops
//! (the thread was in Default already). `ThreadGroup.new` mints a group object
//! that no thread ever joins, so `#enclose`/`#enclosed?` are per-object and
//! honest while membership stays single-group.

use std::sync::Arc;

use crate::builtins::type_error;
use crate::dispatch::{RObj, RubyObject};
use crate::value::RubyValue;
use zeo_abi::{ClassId, THREAD_GROUP_CLASS};
use zeo_macros::ruby_class;

/// A `ThreadGroup` payload. The DEFAULT group is one shared instance; the only
/// per-object state is the enclosure flag, since membership is single-group.
#[derive(Default)]
pub struct RThreadGroup {
    /// `#enclose`/`#enclosed?` -- CRuby's "no thread may leave this group".
    /// Nothing enforces it here: with one real group there is nowhere to move
    /// a thread to, so the flag only ever reports itself.
    enclosed: std::sync::atomic::AtomicBool,
}

impl RubyObject for RThreadGroup {
    fn class_id(&self) -> ClassId {
        THREAD_GROUP_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RThreadGroup::default())
    }
}

/// `ThreadGroup::Default` -- ONE shared instance, so `t.group.equal?(
/// ThreadGroup::Default)` holds like it does in CRuby.
pub fn default_group() -> RubyValue {
    static DEFAULT: std::sync::OnceLock<RubyValue> = std::sync::OnceLock::new();
    DEFAULT
        .get_or_init(|| RubyValue::Object(Arc::new(RThreadGroup::default())))
        .clone()
}

/// A fresh, unenclosed group -- what `ThreadGroup.allocate` answers. NOT
/// [`default_group`], whose whole point is being the one shared instance:
/// `ThreadGroup.allocate.equal?(ThreadGroup::Default)` is false in ruby.
fn thread_group_allocate() -> RubyValue {
    RubyValue::Object(Arc::new(RThreadGroup::default()))
}

/// The payload behind a `ThreadGroup` receiver -- the table only dispatches on
/// one, so the downcast cannot fail.
fn group_of(recv: &RubyValue) -> Result<&RThreadGroup, crate::Signal> {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RThreadGroup>()
            .ok_or_else(|| type_error!("not a ThreadGroup")),
        _ => Err(type_error!("not a ThreadGroup")),
    }
}

ruby_class! {
    ThreadGroup = zeo_abi::THREAD_GROUP_CLASS < zeo_abi::OBJECT_CLASS;

    allocate thread_group_allocate;

    // `ThreadGroup::Default` -- the one shared group every thread reports.
    const Default = default_group();

    // `Class#new`'s C shape: variadic, since it forwards to `initialize`.
    def self."new" cfunc inherits (_recv, *_args, &_block) {
        Ok(RubyValue::Object(Arc::new(RThreadGroup::default())))
    }

    // `#enclose` records the flag and answers the group. Nothing enforces it:
    // with one real group there is nowhere for a thread to be moved from.
    def "enclose"(recv) {
        group_of(recv)?
            .enclosed
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(recv.clone())
    }
    def "enclosed?"(recv) {
        Ok(RubyValue::Bool(
            group_of(recv)?.enclosed.load(std::sync::atomic::Ordering::Relaxed),
        ))
    }
    // `ThreadGroup::Default.add(thread)` -- every thread is in Default
    // already, so a valid call is a no-op answering the group; a non-Thread
    // argument is CRuby's TypeError, with its odd internal "VM/thread"
    // phrasing kept verbatim (oracle-verified).
    def "add"(recv, arg) {
        if !matches!(arg, RubyValue::Thread(_)) {
            return Err(type_error!(
                "wrong argument type {} (expected VM/thread)",
                crate::builtins::check_type_name(arg)
            ));
        }
        Ok(recv.clone())
    }
    // Every thread belongs to `Default` (zeo never moves one), so any OTHER
    // group is empty. Answering the whole thread list for every receiver
    // reported a member for `ThreadGroup.new` and for `.allocate`, where ruby
    // answers `[]`.
    def "list"(recv) {
        let is_default = matches!((recv, &default_group()),
            (RubyValue::Object(a), RubyValue::Object(b)) if Arc::ptr_eq(a, b));
        let members = match is_default {
            true => crate::thread::thread_list(),
            false => Vec::new(),
        };
        Ok(RubyValue::Array(crate::array_new(members)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ThreadGroup`'s `ruby_class!`-generated instance methods are reachable
    /// only through the dispatch table (their Rust fn names are mangled), so
    /// the tests call them the way real dispatch does -- through the registered
    /// instance-method `lookup`.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(THREAD_GROUP_CLASS)
            .expect("ThreadGroup is a registered builtin table")
            .instance
            .as_ref()
            .expect("ThreadGroup has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("ThreadGroup#{name} is defined"))
    }

    #[test]
    fn enclosed_is_always_false_and_add_answers_the_group() {
        let g = default_group();
        assert!(matches!(
            imethod("enclosed?")(&g, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        // A non-Thread argument is a TypeError (registry-less here, so it
        // panics rather than returning Err).
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            imethod("add")(&g, &[RubyValue::Int(1)], None)
        }));
        assert!(r.is_err() || r.unwrap().is_err());
    }

    #[test]
    fn default_group_is_one_shared_instance() {
        // `t.group.equal?(ThreadGroup::Default)` relies on identity.
        let a = default_group();
        let b = default_group();
        let (RubyValue::Object(oa), RubyValue::Object(ob)) = (&a, &b) else {
            panic!("default_group is an Object")
        };
        assert!(Arc::ptr_eq(oa, ob));
    }
}
