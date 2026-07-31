//! `UnboundMethod` -- `Module#instance_method`'s result, a `(class, name)` pair
//! not yet bound to a receiver. `#bind(obj)` re-binds it into a `Method` (obj
//! must be an instance of the owning class or a subclass); `#bind_call(obj,
//! *args)` binds and calls in one step. Its sibling `Method` lives in
//! `method.rs`, from which this file borrows `method_value` (the bind result)
//! and the shared `resolve_method_name`.

use std::sync::Arc;

use crate::builtins::method::{method_value, resolve_method_name};
use crate::builtins::{arg_error, name_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::method_meta::MethodKind;
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use zeo_abi::ClassId;
use zeo_macros::ruby_class;

pub struct RUnboundMethod {
    /// The class it was FETCHED from -- the `#bind` target check, and the MRO
    /// a `#super_method` walk runs over.
    pub class_id: ClassId,
    pub name: Symbol,
    /// Where the lookup sits in that chain -- see [`RMethod::home`]. Unlike a
    /// bound `Method`, an UnboundMethod prints its owner unqualified, so this
    /// only drives the super walk and the metadata lookup.
    pub home: ClassId,
    pub kind: MethodKind,
}

impl RUnboundMethod {
    /// The class or module that actually defines this method.
    pub(crate) fn owner(&self) -> Option<ClassId> {
        match self.kind {
            MethodKind::Instance => crate::dispatch::method_owner(self.home, self.name),
            MethodKind::Singleton => crate::dispatch::class_method_owner(self.home, self.name),
        }
    }
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
            home: self.home,
            kind: self.kind,
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
        // `instance_method` always asks the INSTANCE chain -- `Foo.bar` is
        // reached through `Foo.singleton_class.instance_method(:bar)`.
        home: crate::dispatch::method_owner(cid, name).unwrap_or(cid),
        kind: MethodKind::Instance,
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
    Ok(method_value(obj.clone(), um.name, um.home, um.kind))
}

ruby_class! {
    UnboundMethod = zeo_abi::UNBOUND_METHOD_CLASS < zeo_abi::OBJECT_CLASS;

    def "name"(recv, *_a, &_b) {
        Ok(RubyValue::Symbol(recv_unbound(recv).name))
    }
    def "arity"(recv, *_a, &_b) {
        let um = recv_unbound(recv);
        Ok(RubyValue::Int(
            crate::method_meta::arity(None, um.home, um.kind, um.name).unwrap_or(-1),
        ))
    }
    def "parameters"(recv, *_a, &_b) {
        let um = recv_unbound(recv);
        Ok(crate::method_meta::parameters(None, um.home, um.kind, um.name)
            .unwrap_or_else(|| RubyValue::Array(crate::array_new(vec![]))))
    }
    def "bind"(recv, arg) {
        bind_target(recv_unbound(recv), arg)
    }
    def "bind_call"(recv, *args, &blk) {
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
    def "owner"(recv, *_a, &_b) {
        let um = recv_unbound(recv);
        Ok(RubyValue::Class(um.owner().unwrap_or(um.home)))
    }
    def "source_location"(recv, *_a, &_b) {
        let um = recv_unbound(recv);
        Ok(crate::method_meta::source_location(um.home, um.kind, um.name))
    }
    // The unbound twin of `Method#super_method`, walking the chain of the
    // class this method was fetched from.
    def "super_method"(recv, *_a, &_b) {
        let um = recv_unbound(recv);
        let Some(owner) = um.owner() else {
            return Ok(RubyValue::Nil);
        };
        let next = match um.kind {
            MethodKind::Instance => {
                crate::dispatch::method_owner_after(um.class_id, owner, um.name)
            }
            MethodKind::Singleton => {
                crate::dispatch::class_method_owner_after(um.class_id, owner, um.name)
            }
        };
        Ok(match next {
            Some(home) => RubyValue::Object(Arc::new(RUnboundMethod {
                class_id: um.class_id,
                name: um.name,
                home,
                kind: um.kind,
            })),
            None => RubyValue::Nil,
        })
    }
    // `UnboundMethod#original_name` -- the name it was DEFINED under, which
    // differs from `#name` only for one reached through an alias.
    def "original_name"(recv, *_a, &_b) {
        let um = recv_unbound(recv);
        Ok(RubyValue::Symbol(crate::method_meta::original_name(um.home, um.kind, um.name)))
    }
    // Unbound, there is no receiver to qualify against: CRuby prints the
    // OWNER alone, never the class the method was fetched from.
    def "inspect" | "to_s" (recv, *_a, &_b) {
        let um = recv_unbound(recv);
        let owner = um.owner().unwrap_or(um.home);
        Ok(RubyValue::Str(crate::collections::string_new(
            crate::method_meta::Inspect {
                label: "UnboundMethod",
                home: crate::dispatch::class_name(owner)
                    .unwrap_or_else(|| "Object".to_string()),
                owner: None,
                separator: if um.kind == MethodKind::Singleton { '.' } else { '#' },
                name: um.name,
                original: crate::method_meta::alias_origin(um.home, um.kind, um.name),
                params: crate::method_meta::printable_params(None, um.home, um.kind, um.name),
                source: crate::method_meta::source_of(um.home, um.kind, um.name),
            }
            .render(),
        )))
    }
}
