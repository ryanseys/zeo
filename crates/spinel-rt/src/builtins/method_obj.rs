//! `Method` (P4): the object `Kernel#method(:name)` answers -- a bound
//! (receiver, name) pair whose `#call` dispatches through the ordinary
//! `send` machinery. `UnboundMethod`/`bind_call`/source reflection are a
//! later phase (plan P-D).

use std::sync::Arc;

use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use spinel_abi::{ClassId, METHOD_CLASS};

pub struct RMethod {
    pub recv: RubyValue,
    pub name: Symbol,
}

impl RubyObject for RMethod {
    fn class_id(&self) -> ClassId {
        METHOD_CLASS
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
        Arc::new(RMethod { recv: self.recv.clone(), name: self.name })
    }
}

/// Constructs the `Method` value for `recv.method(name_arg)` -- shared by
/// the Kernel table row and any codegen fast path. An unknown method is a
/// `NameError` at CONSTRUCTION time, as in CRuby: `method(:nope)` raises
/// immediately rather than deferring to `#call`. The lookup is
/// `respond_to?`'s, with `include_all` -- `method(:private_helper)` is
/// legal (privacy limits CALL sites, not reflection).
pub fn method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = match name_arg {
        RubyValue::Symbol(s) => *s,
        RubyValue::Str(s) => Symbol::intern(&s.lock().to_utf8_lossy()),
        other => {
            return Err(raise_error(
                "TypeError",
                format!("{} is not a symbol nor a string", other.inspect_string()),
            ))
        }
    };
    if !crate::dispatch::responds_to(recv.class_id(), name, true) {
        // CRuby's phrasing names the receiver's CLASS, not the receiver
        // ("undefined method 'nope' for class 'String'").
        return Err(raise_error(
            "NameError",
            format!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(recv.class_id())
                    .unwrap_or_else(|| "Object".to_string())
            ),
        ));
    }
    Ok(RubyValue::Object(Arc::new(RMethod { recv: recv.clone(), name })))
}

fn recv_method(recv: &RubyValue) -> &RMethod {
    let RubyValue::Object(o) = recv else {
        unreachable!("the Method table only dispatches on METHOD_CLASS receivers")
    };
    o.as_any()
        .downcast_ref::<RMethod>()
        .expect("class_id guarantees this downcast")
}

fn m_call(recv: &RubyValue, args: &[RubyValue], blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    crate::dispatch::send_value(&m.recv, m.name, args, blk)
}

fn m_name(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Symbol(recv_method(recv).name))
}

fn m_receiver(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(recv_method(recv).recv.clone())
}

fn m_to_proc(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    let (target, name) = (m.recv.clone(), m.name);
    Ok(RubyValue::Proc(crate::RProc::new(move |args: &[RubyValue]| {
        crate::dispatch::send_value(&target, name, args, None)
    })))
}

fn m_arity(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    // A user `def` has a baked descriptor; a builtin has none, so `-1`
    // (var-args) stays the honest catch-all there.
    Ok(RubyValue::Int(
        crate::method_params::arity(m.recv.class_id(), m.name).unwrap_or(-1),
    ))
}

fn m_parameters(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    // Builtins have no baked signature -- CRuby reports them as a lone rest;
    // mirror that so `#parameters` is always an Array.
    Ok(crate::method_params::parameters(m.recv.class_id(), m.name)
        .unwrap_or_else(|| RubyValue::Array(crate::array_new(vec![]))))
}

fn m_unbind(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    Ok(RubyValue::Object(Arc::new(RUnboundMethod {
        class_id: m.recv.class_id(),
        name: m.name,
    })))
}

fn m_inspect(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    Ok(RubyValue::Str(crate::collections::string_new(format!(
        "#<Method: {}#{}>",
        crate::dispatch::class_name(m.recv.class_id()).unwrap_or_else(|| "Object".to_string()),
        m.name.name()
    ))))
}

pub fn lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "call" | "()" | "[]" | "===" => m_call,
        "name" => m_name,
        "receiver" => m_receiver,
        "to_proc" => m_to_proc,
        "arity" => m_arity,
        "parameters" => m_parameters,
        "unbind" => m_unbind,
        "inspect" | "to_s" => m_inspect,
        _ => return None,
    })
}

/// Reflection companion to `lookup` (hand-written table).
pub fn lookup_names() -> &'static [&'static str] {
    &[
        "call", "()", "[]", "===", "name", "receiver", "to_proc", "arity",
        "parameters", "unbind", "inspect", "to_s",
    ]
}

// ---------------------------------------------------------------------------
// UnboundMethod -- `Module#instance_method`'s result, a (class, name) pair not
// yet bound to a receiver. `#bind(obj)` re-binds it (obj must be an instance of
// the owning class or a subclass); `#bind_call(obj, *args)` binds and calls in
// one step.
// ---------------------------------------------------------------------------

pub struct RUnboundMethod {
    pub class_id: ClassId,
    pub name: Symbol,
}

impl RubyObject for RUnboundMethod {
    fn class_id(&self) -> ClassId {
        spinel_abi::UNBOUND_METHOD_CLASS
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
        Arc::new(RUnboundMethod { class_id: self.class_id, name: self.name })
    }
}

/// `Module#instance_method(:name)` -- the unbound method for `name` on `cid`.
/// Unknown name is a `NameError` at construction, as in CRuby.
pub fn unbound_method_new(cid: ClassId, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = match name_arg {
        RubyValue::Symbol(s) => *s,
        RubyValue::Str(s) => Symbol::intern(&s.lock().to_utf8_lossy()),
        other => {
            return Err(raise_error(
                "TypeError",
                format!("{} is not a symbol nor a string", other.inspect_string()),
            ))
        }
    };
    if !crate::dispatch::responds_to(cid, name, true) {
        return Err(raise_error(
            "NameError",
            format!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string())
            ),
        ));
    }
    Ok(RubyValue::Object(Arc::new(RUnboundMethod { class_id: cid, name })))
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
        return Err(raise_error(
            "TypeError",
            format!(
                "bind argument must be an instance of {}",
                crate::dispatch::class_name(um.class_id).unwrap_or_else(|| "Object".to_string())
            ),
        ));
    }
    Ok(RubyValue::Object(Arc::new(RMethod { recv: obj.clone(), name: um.name })))
}

pub fn lookup_unbound(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "name" => u_name,
        "arity" => u_arity,
        "parameters" => u_parameters,
        "bind" => u_bind,
        "bind_call" => u_bind_call,
        "inspect" | "to_s" => u_inspect,
        _ => return None,
    })
}

pub fn lookup_unbound_names() -> &'static [&'static str] {
    &["name", "arity", "parameters", "bind", "bind_call", "inspect", "to_s"]
}

fn u_name(recv: &RubyValue, _a: &[RubyValue], _b: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Symbol(recv_unbound(recv).name))
}

fn u_arity(recv: &RubyValue, _a: &[RubyValue], _b: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let um = recv_unbound(recv);
    Ok(RubyValue::Int(
        crate::method_params::arity(um.class_id, um.name).unwrap_or(-1),
    ))
}

fn u_parameters(recv: &RubyValue, _a: &[RubyValue], _b: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let um = recv_unbound(recv);
    Ok(crate::method_params::parameters(um.class_id, um.name)
        .unwrap_or_else(|| RubyValue::Array(crate::array_new(vec![]))))
}

fn u_bind(recv: &RubyValue, args: &[RubyValue], _b: Option<RubyValue>) -> Result<RubyValue, Signal> {
    if args.len() != 1 {
        return Err(raise_error(
            "ArgumentError",
            format!("wrong number of arguments (given {}, expected 1)", args.len()),
        ));
    }
    bind_target(recv_unbound(recv), &args[0])
}

fn u_bind_call(recv: &RubyValue, args: &[RubyValue], blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        return Err(raise_error(
            "ArgumentError",
            "wrong number of arguments (given 0, expected 1+)".to_string(),
        ));
    }
    let um = recv_unbound(recv);
    // The receiver must still be a valid bind target.
    bind_target(um, &args[0])?;
    crate::dispatch::send_value(&args[0], um.name, &args[1..], blk)
}

fn u_inspect(recv: &RubyValue, _a: &[RubyValue], _b: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let um = recv_unbound(recv);
    Ok(RubyValue::Str(crate::collections::string_new(format!(
        "#<UnboundMethod: {}#{}>",
        crate::dispatch::class_name(um.class_id).unwrap_or_else(|| "Object".to_string()),
        um.name.name()
    ))))
}
