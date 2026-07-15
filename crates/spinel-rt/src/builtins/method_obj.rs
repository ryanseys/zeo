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
        RubyValue::Str(s) => Symbol::intern(&s.lock().clone()),
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
    // The bound method's true arity isn't recorded in the dispatch tables;
    // `-1` (var-args) is the honest catch-all.
    // TODO(plan P-D): thread real arity through method registration.
    let _ = recv;
    Ok(RubyValue::Int(-1))
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
        "inspect" | "to_s" => m_inspect,
        _ => return None,
    })
}
