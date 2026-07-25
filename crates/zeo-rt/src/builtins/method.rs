//! `Method`: the object `Kernel#method(:name)` answers -- a bound
//! (receiver, name) pair whose `#call` dispatches through the ordinary
//! `send` machinery. Its sibling `UnboundMethod` (an unbound `(class, name)`
//! pair) lives in `unbound_method.rs`; the two reference each other's payload
//! (`#unbind` builds an `UnboundMethod`, `UnboundMethod#bind` builds a
//! `Method`) plus this file's shared `resolve_method_name`/`method_source`.

use std::sync::Arc;

use crate::builtins::{arity, name_error, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use zeo_abi::{ClassId, METHOD_CLASS};
use zeo_macros::ruby_class;

use crate::builtins::unbound_method::RUnboundMethod;

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
        Arc::new(RMethod {
            recv: self.recv.clone(),
            name: self.name,
        })
    }
}

/// Resolve a Symbol/String method-name argument to a `Symbol`, or the
/// `TypeError` CRuby raises for anything else. Shared by
/// `method`/`public_method`/`instance_method`.
pub(crate) fn resolve_method_name(name_arg: &RubyValue) -> Result<Symbol, Signal> {
    match name_arg {
        RubyValue::Symbol(s) => Ok(*s),
        RubyValue::Str(s) => Ok(Symbol::intern(&s.lock().to_utf8_lossy())),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

/// Constructs the `Method` value for `recv.method(name_arg)` -- shared by
/// the Kernel table row and any codegen fast path. An unknown method is a
/// `NameError` at CONSTRUCTION time, as in CRuby: `method(:nope)` raises
/// immediately rather than deferring to `#call`. The lookup is
/// `respond_to?`'s, with `include_all` -- `method(:private_helper)` is
/// legal (privacy limits CALL sites, not reflection).
pub fn method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    // `responds_to_value`, not a bare instance-MRO `responds_to`: a
    // class/module receiver binds its CLASS methods (`Process.method(
    // :clock_gettime)` -- the timeout gem's `GET_TIME`), which only the
    // value-aware probe sees (it mirrors `send_value_in`'s class-receiver
    // dispatch order, singletons included).
    // `respond_to_missing?` counts: `obj.method(:dyn)` succeeds when the
    // hook admits `:dyn`, returning a Method that dispatches through
    // `method_missing` at call time -- CRuby's `rb_obj_method`.
    if !crate::dispatch::responds_to_or_missing(recv, name, true)? {
        // CRuby's phrasing names the receiver's CLASS, not the receiver
        // ("undefined method 'nope' for class 'String'").
        return Err(name_error!(
            "undefined method '{}' for class '{}'",
            name.name(),
            crate::dispatch::class_name(recv.class_id()).unwrap_or_else(|| "Object".to_string())
        ));
    }
    Ok(RubyValue::Object(Arc::new(RMethod {
        recv: recv.clone(),
        name,
    })))
}

/// `Object#singleton_method(:name)` -- a `Method` bound to the receiver, but
/// ONLY for a per-object singleton method (`def obj.name` /
/// `define_singleton_method`). A name that resolves to an ordinary class
/// method is a `NameError`, exactly like CRuby -- reflection here is limited
/// to the object's own singletons.
pub fn singleton_method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    if !crate::runtime_meta::object_has_singleton_method(recv, name) {
        return Err(name_error!(
            "undefined singleton method '{}' for '{}'",
            name.name(),
            recv.inspect_string()
        ));
    }
    Ok(RubyValue::Object(Arc::new(RMethod {
        recv: recv.clone(),
        name,
    })))
}

/// `Kernel#public_method(:name)` -- like `method`, but a PRIVATE (or
/// protected) method raises `NameError` rather than binding: reflection here
/// is restricted to the public surface.
pub fn public_method_new(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = resolve_method_name(name_arg)?;
    let cid = recv.class_id();
    let class = crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string());
    if crate::dispatch::responds_to(cid, name, false) {
        return Ok(RubyValue::Object(Arc::new(RMethod {
            recv: recv.clone(),
            name,
        })));
    }
    let msg = if crate::dispatch::responds_to(cid, name, true) {
        format!("method '{}' for class '{}' is private", name.name(), class)
    } else {
        format!("undefined method '{}' for class '{}'", name.name(), class)
    };
    Err(raise_error("NameError", msg))
}

/// If `v` is a `Method` or `UnboundMethod`, its source `(owning class, method
/// name)` -- what `define_method(name, method_obj)` copies the definition of.
/// `None` for anything else (a Proc, a bare value).
pub fn method_source(v: &RubyValue) -> Option<(ClassId, Symbol)> {
    let RubyValue::Object(o) = v else {
        return None;
    };
    if let Some(m) = o.as_any().downcast_ref::<RMethod>() {
        return Some((m.recv.class_id(), m.name));
    }
    if let Some(u) = o.as_any().downcast_ref::<RUnboundMethod>() {
        return Some((u.class_id, u.name));
    }
    None
}

fn recv_method(recv: &RubyValue) -> &RMethod {
    let RubyValue::Object(o) = recv else {
        unreachable!("the Method table only dispatches on METHOD_CLASS receivers")
    };
    o.as_any()
        .downcast_ref::<RMethod>()
        .expect("class_id guarantees this downcast")
}

/// Shared body of `>>`/`<<`: both sides go through `#call`, so a Method, a
/// Proc, or any object answering `call` composes uniformly.
fn compose(recv: &RubyValue, args: &[RubyValue], forward: bool) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    let this = recv.clone();
    let other = args[0].clone();
    let call = Symbol::intern("call");
    Ok(RubyValue::Proc(crate::RProc::new(
        move |call_args: &[RubyValue]| {
            let (first, second) = if forward {
                (&this, &other)
            } else {
                (&other, &this)
            };
            let mid = crate::dispatch::send_value(first, call, call_args, None)?;
            crate::dispatch::send_value(second, call, &[mid], None)
        },
    )))
}

ruby_class! {
    Method = zeo_abi::METHOD_CLASS < zeo_abi::OBJECT_CLASS;

    def "call" | "()" | "[]" | "===" (recv, args, blk) {
        let m = recv_method(recv);
        crate::dispatch::send_value(&m.recv, m.name, args, blk)
    }
    def "name"(recv, _args, _blk) {
        Ok(RubyValue::Symbol(recv_method(recv).name))
    }
    def "receiver"(recv, _args, _blk) {
        Ok(recv_method(recv).recv.clone())
    }
    // `Method#to_proc` yields a lambda (`lambda? == true`) carrying the
    // method's arity, which is what `#curry` needs to know how many arguments
    // to gather before invoking.
    def "to_proc" as m_to_proc (recv, _args, _blk) {
        let m = recv_method(recv);
        let (target, name) = (m.recv.clone(), m.name);
        let arity = crate::method_params::arity(m.recv.class_id(), m.name).unwrap_or(-1) as i32;
        Ok(RubyValue::Proc(crate::RProc::with_meta(
            move |args: &[RubyValue]| crate::dispatch::send_value(&target, name, args, None),
            arity,
            true,
        )))
    }
    def "arity"(recv, _args, _blk) {
        let m = recv_method(recv);
        // A user `def` has a baked descriptor; a builtin has none, so `-1`
        // (var-args) stays the honest catch-all there.
        Ok(RubyValue::Int(
            crate::method_params::arity(m.recv.class_id(), m.name).unwrap_or(-1),
        ))
    }
    def "parameters"(recv, _args, _blk) {
        let m = recv_method(recv);
        // Builtins have no baked signature -- CRuby reports them as a lone rest;
        // mirror that so `#parameters` is always an Array.
        Ok(crate::method_params::parameters(m.recv.class_id(), m.name)
            .unwrap_or_else(|| RubyValue::Array(crate::array_new(vec![]))))
    }
    def "unbind"(recv, _args, _blk) {
        let m = recv_method(recv);
        Ok(RubyValue::Object(Arc::new(RUnboundMethod {
            class_id: m.recv.class_id(),
            name: m.name,
        })))
    }
    // `Method#owner` -- the class or module in the receiver's ancestry that
    // actually defines the method (which may be an ancestor of the receiver's
    // class, not the class itself).
    def "owner"(recv, _args, _blk) {
        let m = recv_method(recv);
        let owner =
            crate::dispatch::method_owner(m.recv.class_id(), m.name).unwrap_or(m.recv.class_id());
        Ok(RubyValue::Class(owner))
    }
    // `Method#original_name` -- the name the method was defined under. We don't
    // record aliases, so this is `#name` (exact except for aliased methods).
    def "original_name"(recv, _args, _blk) {
        Ok(RubyValue::Symbol(recv_method(recv).name))
    }
    // `Method#source_location` -- `nil` for a method with no Ruby source
    // location this AOT runtime tracks (builtins, and any method whose defining
    // `.rb` span isn't recorded), matching CRuby's `nil` for C-defined methods.
    def "source_location"(_recv, _args, _blk) {
        Ok(RubyValue::Nil)
    }
    // `Method#super_method` -- `nil` (this runtime tracks no super-method chain
    // for a bound Method object).
    def "super_method"(_recv, _args, _blk) {
        Ok(RubyValue::Nil)
    }
    // `meth >> other` -- a Proc running `meth` then piping its result into
    // `other` (`other.call(meth.call(*args))`). `other` is any callable.
    def ">>"(recv, args, _blk) {
        compose(recv, args, true)
    }
    // `meth << other` -- the reverse pipe: `meth.call(other.call(*args))`.
    def "<<"(recv, args, _blk) {
        compose(recv, args, false)
    }
    // `Method#==`/`#eql?` -- same defining method (name + owner) bound to an
    // equal receiver.
    def "==" | "eql?" (recv, args, _blk) {
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
        let recv_eq = crate::dispatch::send_value(
            &m.recv,
            Symbol::intern("=="),
            std::slice::from_ref(&other.recv),
            None,
        )?;
        Ok(RubyValue::Bool(recv_eq.truthy()))
    }
    // `Method#hash` -- consistent with `#==`: keyed on name and owner (an equal
    // receiver is required for `==`, but folding it in isn't needed for the
    // equal-objects-hash-equal contract, so name+owner is enough).
    def "hash"(recv, _args, _blk) {
        use std::hash::{Hash, Hasher};
        let m = recv_method(recv);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        m.name.hash(&mut h);
        crate::dispatch::method_owner(m.recv.class_id(), m.name)
            .unwrap_or(m.recv.class_id())
            .hash(&mut h);
        Ok(RubyValue::Int(h.finish() as i64))
    }
    // `Method#curry` -- curries the equivalent Proc (`to_proc.curry`).
    def "curry"(recv, args, _blk) {
        let proc = m_to_proc(recv, &[], None)?;
        crate::dispatch::send_value(&proc, Symbol::intern("curry"), args, None)
    }
    def "inspect" | "to_s" (recv, _args, _blk) {
        let m = recv_method(recv);
        Ok(RubyValue::Str(crate::collections::string_new(format!(
            "#<Method: {}#{}>",
            crate::dispatch::class_name(m.recv.class_id()).unwrap_or_else(|| "Object".to_string()),
            m.name.name()
        ))))
    }
}
