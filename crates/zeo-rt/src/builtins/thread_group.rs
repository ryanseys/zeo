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

crate::builtins::builtin_methods! {
    pub(crate) fn lookup;

    // Nothing ever encloses the default group.
    "enclosed?" => fn enclosed_p(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(false))
    }
    // `ThreadGroup::Default.add(thread)` -- every thread is in Default
    // already, so a valid call is a no-op answering the group; a non-Thread
    // argument is CRuby's TypeError, with its odd internal "VM/thread"
    // phrasing kept verbatim (oracle-verified).
    "add" => fn add(recv, args, _block) {
        arity!(args, 1);
        if !matches!(&args[0], RubyValue::Thread(_)) {
            return Err(type_error!(
                "wrong argument type {} (expected VM/thread)",
                crate::builtins::class_name_of(&args[0])
            ));
        }
        Ok(recv.clone())
    }
    "list" => fn list(_recv, args, _block) {
        arity!(args, 0);
        crate::builtins::thread::lookup_class("list").expect("Thread.list is registered")(
            &RubyValue::Class(zeo_abi::THREAD_CLASS),
            &[],
            None,
        )
    }
}
