//! `ThreadGroup` -- the DEFAULT group only (see `zeo_abi::THREAD_GROUP_CLASS`'s
//! docs): every thread reports `ThreadGroup::Default` as its group,
//! `#list` answers the live thread list, `#enclosed?` is always false, and
//! `#add` validates its argument and no-ops (the thread was in Default
//! already). `ThreadGroup.new`/`#enclose` are honestly absent -- creating a
//! second group has no representation here yet.

use std::sync::Arc;

use crate::builtins::{arity, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::value::RubyValue;
use zeo_abi::{ClassId, THREAD_GROUP_CLASS};
use zeo_macros::ruby_class;

/// The (only) `ThreadGroup` payload -- stateless: the default group's
/// identity is the value itself.
pub struct RThreadGroup;

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
        Arc::new(RThreadGroup)
    }
}

/// `ThreadGroup::Default` -- ONE shared instance, so `t.group.equal?(
/// ThreadGroup::Default)` holds like it does in CRuby.
pub fn default_group() -> RubyValue {
    static DEFAULT: std::sync::OnceLock<RubyValue> = std::sync::OnceLock::new();
    DEFAULT
        .get_or_init(|| RubyValue::Object(Arc::new(RThreadGroup)))
        .clone()
}

ruby_class! {
    ThreadGroup = zeo_abi::THREAD_GROUP_CLASS < zeo_abi::OBJECT_CLASS;

    // `ThreadGroup::Default` -- the one shared group every thread reports.
    const Default = default_group();

    // Nothing ever encloses the default group.
    def "enclosed?"(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(false))
    }
    // `ThreadGroup::Default.add(thread)` -- every thread is in Default
    // already, so a valid call is a no-op answering the group; a non-Thread
    // argument is CRuby's TypeError, with its odd internal "VM/thread"
    // phrasing kept verbatim (oracle-verified).
    def "add"(recv, args, _block) {
        arity!(args, 1);
        if !matches!(&args[0], RubyValue::Thread(_)) {
            return Err(type_error!(
                "wrong argument type {} (expected VM/thread)",
                crate::builtins::class_name_of(&args[0])
            ));
        }
        Ok(recv.clone())
    }
    def "list"(_recv, args, _block) {
        arity!(args, 0);
        crate::builtins::thread::lookup_class("list").expect("Thread.list is registered")(
            &RubyValue::Class(zeo_abi::THREAD_CLASS),
            &[],
            None,
        )
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
