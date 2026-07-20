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
/// Resolve a Symbol/String method-name argument to a `Symbol`, or the
/// `TypeError` CRuby raises for anything else. Shared by
/// `method`/`public_method`/`instance_method`.
fn resolve_method_name(name_arg: &RubyValue) -> Result<Symbol, Signal> {
    match name_arg {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(raise_error(
            "TypeError",
            format!("{} is not a symbol nor a string", other.inspect_string()),
        )),
    }
}

pub fn method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
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

/// `Object#singleton_method(:name)` -- a `Method` bound to the receiver, but
/// ONLY for a per-object singleton method (`def obj.name` /
/// `define_singleton_method`). A name that resolves to an ordinary class
/// method is a `NameError`, exactly like CRuby -- reflection here is limited
/// to the object's own singletons.
pub fn singleton_method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    if !crate::runtime_meta::object_has_singleton_method(recv, name) {
        return Err(raise_error(
            "NameError",
            format!(
                "undefined singleton method '{}' for '{}'",
                name.name(),
                recv.inspect_string()
            ),
        ));
    }
    Ok(RubyValue::Object(Arc::new(RMethod { recv: recv.clone(), name })))
}

/// `Kernel#public_method(:name)` -- like `method`, but a PRIVATE (or
/// protected) method raises `NameError` rather than binding: reflection here
/// is restricted to the public surface.
pub fn public_method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    let cid = recv.class_id();
    let class = crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string());
    if crate::dispatch::responds_to(cid, name, false) {
        return Ok(RubyValue::Object(Arc::new(RMethod { recv: recv.clone(), name })));
    }
    let msg = if crate::dispatch::responds_to(cid, name, true) {
        format!("method '{}' for class '{}' is private", name.name(), class)
    } else {
        format!("undefined method '{}' for class '{}'", name.name(), class)
    };
    Err(raise_error("NameError", msg))
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
    // `Method#to_proc` yields a lambda (`lambda? == true`) carrying the
    // method's arity, which is what `#curry` needs to know how many arguments
    // to gather before invoking.
    let arity = crate::method_params::arity(m.recv.class_id(), m.name).unwrap_or(-1) as i32;
    Ok(RubyValue::Proc(crate::RProc::with_meta(
        move |args: &[RubyValue]| crate::dispatch::send_value(&target, name, args, None),
        arity,
        true,
    )))
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

/// `Method#owner` -- the class or module in the receiver's ancestry that
/// actually defines the method (which may be an ancestor of the receiver's
/// class, not the class itself).
fn m_owner(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    let owner = crate::dispatch::method_owner(m.recv.class_id(), m.name).unwrap_or(m.recv.class_id());
    Ok(RubyValue::Class(owner))
}

/// `Method#original_name` -- the name the method was defined under. We don't
/// record aliases, so this is `#name` (exact except for aliased methods).
fn m_original_name(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Symbol(recv_method(recv).name))
}

/// `meth >> other` -- a Proc running `meth` then piping its result into
/// `other` (`other.call(meth.call(*args))`). `other` is any callable.
fn m_compose_forward(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    compose(recv, args, true)
}

/// `meth << other` -- the reverse pipe: `meth.call(other.call(*args))`.
fn m_compose_backward(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    compose(recv, args, false)
}

/// Shared body of `>>`/`<<`: both sides go through `#call`, so a Method, a
/// Proc, or any object answering `call` composes uniformly.
fn compose(recv: &RubyValue, args: &[RubyValue], forward: bool) -> Result<RubyValue, Signal> {
    if args.len() != 1 {
        return Err(raise_error(
            "ArgumentError",
            format!("wrong number of arguments (given {}, expected 1)", args.len()),
        ));
    }
    let this = recv.clone();
    let other = args[0].clone();
    let call = Symbol::intern("call");
    Ok(RubyValue::Proc(crate::RProc::new(move |call_args: &[RubyValue]| {
        let (first, second) = if forward { (&this, &other) } else { (&other, &this) };
        let mid = crate::dispatch::send_value(first, call, call_args, None)?;
        crate::dispatch::send_value(second, call, &[mid], None)
    })))
}

/// `Method#==`/`#eql?` -- same defining method (name + owner) bound to an
/// equal receiver.
fn m_eq(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let m = recv_method(recv);
    let RubyValue::Object(o) = &args[0] else {
        return Ok(RubyValue::Bool(false));
    };
    let Some(other) = o.as_any().downcast_ref::<RMethod>() else {
        return Ok(RubyValue::Bool(false));
    };
    let same_method = m.name == other.name
        && crate::dispatch::method_owner(m.recv.class_id(), m.name)
            == crate::dispatch::method_owner(other.recv.class_id(), other.name);
    if !same_method {
        return Ok(RubyValue::Bool(false));
    }
    let recv_eq = crate::dispatch::send_value(&m.recv, Symbol::intern("=="), &[other.recv.clone()], None)?;
    Ok(RubyValue::Bool(recv_eq.truthy()))
}

/// `Method#hash` -- consistent with `#==`: keyed on name and owner (an equal
/// receiver is required for `==`, but folding it in isn't needed for the
/// equal-objects-hash-equal contract, so name+owner is enough).
fn m_hash(recv: &RubyValue, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    use std::hash::{Hash, Hasher};
    let m = recv_method(recv);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m.name.hash(&mut h);
    crate::dispatch::method_owner(m.recv.class_id(), m.name)
        .unwrap_or(m.recv.class_id())
        .hash(&mut h);
    Ok(RubyValue::Int(h.finish() as i64))
}

/// `Method#curry` -- curries the equivalent Proc (`to_proc.curry`).
fn m_curry(recv: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let proc = m_to_proc(recv, &[], None)?;
    crate::dispatch::send_value(&proc, Symbol::intern("curry"), args, None)
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
        "owner" => m_owner,
        "original_name" => m_original_name,
        ">>" => m_compose_forward,
        "<<" => m_compose_backward,
        "==" | "eql?" => m_eq,
        "hash" => m_hash,
        "curry" => m_curry,
        "inspect" | "to_s" => m_inspect,
        _ => return None,
    })
}

/// Reflection companion to `lookup` (hand-written table).
pub fn lookup_names() -> &'static [&'static str] {
    &[
        "call", "()", "[]", "===", "name", "receiver", "to_proc", "arity",
        "parameters", "unbind", "owner", "original_name", ">>", "<<", "==",
        "eql?", "hash", "curry", "inspect", "to_s",
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
    let name = resolve_method_name(name_arg)?;
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
        "owner" => u_owner,
        "original_name" => u_original_name,
        "inspect" | "to_s" => u_inspect,
        _ => return None,
    })
}

pub fn lookup_unbound_names() -> &'static [&'static str] {
    &[
        "name", "arity", "parameters", "bind", "bind_call", "owner",
        "original_name", "inspect", "to_s",
    ]
}

/// `UnboundMethod#owner` -- the defining class/module in the owning class's
/// ancestry (may differ from the class the unbound method was fetched from).
fn u_owner(recv: &RubyValue, _a: &[RubyValue], _b: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let um = recv_unbound(recv);
    let owner = crate::dispatch::method_owner(um.class_id, um.name).unwrap_or(um.class_id);
    Ok(RubyValue::Class(owner))
}

/// `UnboundMethod#original_name` -- the defined name (we track no aliases).
fn u_original_name(recv: &RubyValue, _a: &[RubyValue], _b: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Symbol(recv_unbound(recv).name))
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
