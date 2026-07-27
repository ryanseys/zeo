//! `UnboundMethod` -- `Module#instance_method`'s result, a `(class, name)` pair
//! not yet bound to a receiver. `#bind(obj)` re-binds it into a `Method` (obj
//! must be an instance of the owning class or a subclass); `#bind_call(obj,
//! *args)` binds and calls in one step. Its sibling `Method` lives in
//! `method.rs`, from which this file borrows `RMethod` (the bind result) and
//! the shared `resolve_method_name`.

use std::sync::Arc;

use crate::builtins::method::{RMethod, resolve_method_name};
use crate::builtins::{arg_error, arity, name_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use zeo_abi::ClassId;
use zeo_macros::ruby_class;

pub struct RUnboundMethod {
    pub class_id: ClassId,
    pub name: Symbol,
}

impl RubyObject for RUnboundMethod {
    fn class_id(&self) -> ClassId {
        zeo_abi::UNBOUND_METHOD_CLASS
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
        Arc::new(RUnboundMethod {
            class_id: self.class_id,
            name: self.name,
        })
    }
}

/// `Module#instance_method(:name)` -- the unbound method for `name` on `cid`.
/// Unknown name is a `NameError` at construction, as in CRuby.
pub fn unbound_method_new(cid: ClassId, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    if !crate::dispatch::responds_to(cid, name, true) {
        return Err(name_error!(
            "undefined method '{}' for class '{}'",
            name.name(),
            crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string())
        ));
    }
    Ok(RubyValue::Object(Arc::new(RUnboundMethod {
        class_id: cid,
        name,
    })))
}

fn recv_unbound(recv: &RubyValue) -> &RUnboundMethod {
    let RubyValue::Object(o) = recv else {
        unreachable!("the UnboundMethod table only dispatches on UNBOUND_METHOD_CLASS receivers")
    };
    o.as_any()
        .downcast_ref::<RUnboundMethod>()
        .expect("class_id guarantees this downcast")
}

/// The shared `bind` check: `obj` must be an instance of the unbound method's
/// owning class (or a descendant).
fn bind_target(um: &RUnboundMethod, obj: &RubyValue) -> Result<RubyValue, Signal> {
    if !crate::dispatch::is_a(obj.class_id(), um.class_id) {
        return Err(type_error!(
            "bind argument must be an instance of {}",
            crate::dispatch::class_name(um.class_id).unwrap_or_else(|| "Object".to_string())
        ));
    }
    Ok(RubyValue::Object(Arc::new(RMethod {
        recv: obj.clone(),
        name: um.name,
    })))
}

ruby_class! {
    UnboundMethod = zeo_abi::UNBOUND_METHOD_CLASS < zeo_abi::OBJECT_CLASS;

    def "name"(recv, _a, _b) {
        Ok(RubyValue::Symbol(recv_unbound(recv).name))
    }
    def "arity"(recv, _a, _b) {
        let um = recv_unbound(recv);
        Ok(RubyValue::Int(
            crate::method_meta::arity(um.class_id, crate::MethodKind::Instance, um.name).unwrap_or(-1),
        ))
    }
    def "parameters"(recv, _a, _b) {
        let um = recv_unbound(recv);
        Ok(crate::method_meta::parameters(um.class_id, crate::MethodKind::Instance, um.name)
            .unwrap_or_else(|| RubyValue::Array(crate::array_new(vec![]))))
    }
    def "bind"(recv, args, _b) {
        arity!(args, 1);
        bind_target(recv_unbound(recv), &args[0])
    }
    def "bind_call"(recv, args, blk) {
        if args.is_empty() {
            return Err(arg_error!(
                "wrong number of arguments (given 0, expected 1+)"
            ));
        }
        let um = recv_unbound(recv);
        // The receiver must still be a valid bind target.
        bind_target(um, &args[0])?;
        crate::dispatch::send_value(&args[0], um.name, &args[1..], blk)
    }
    // `UnboundMethod#owner` -- the defining class/module in the owning class's
    // ancestry (may differ from the class the unbound method was fetched from).
    def "owner"(recv, _a, _b) {
        let um = recv_unbound(recv);
        let owner = crate::dispatch::method_owner(um.class_id, um.name).unwrap_or(um.class_id);
        Ok(RubyValue::Class(owner))
    }
    // `UnboundMethod#original_name` -- the defined name (we track no aliases).
    def "original_name"(recv, _a, _b) {
        Ok(RubyValue::Symbol(recv_unbound(recv).name))
    }
    def "inspect" | "to_s" (recv, _a, _b) {
        let um = recv_unbound(recv);
        Ok(RubyValue::Str(crate::collections::string_new(format!(
            "#<UnboundMethod: {}#{}>",
            crate::dispatch::class_name(um.class_id).unwrap_or_else(|| "Object".to_string()),
            um.name.name()
        ))))
    }
}
