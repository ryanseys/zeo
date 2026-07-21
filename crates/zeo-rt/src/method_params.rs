//! The compile-time-baked parameter descriptors backing `Method#arity`/
//! `#parameters` and `UnboundMethod#arity`/`#parameters`. Codegen emits one
//! `register_params` call per user `def` from that method's `Params` (fully
//! known at compile time); the dispatch tables themselves carry only fn
//! pointers, so this side table is where the signature lives. Process-wide
//! shared, populated once from generated `main()`, then read-only -- same
//! posture as the class registry and constant store.

use crate::{ClassId, RubyValue, Symbol};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// One parameter's kind, matching the leading symbol Ruby's `#parameters`
/// emits. A `post` (required arg after a splat) is a `Req` placed after the
/// `Rest`, so it needs no separate kind -- codegen emits the entries already
/// in Ruby's canonical order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Req,
    Opt,
    Rest,
    KeyReq,
    Key,
    KeyRest,
    Block,
}

impl ParamKind {
    fn tag(self) -> &'static str {
        match self {
            ParamKind::Req => "req",
            ParamKind::Opt => "opt",
            ParamKind::Rest => "rest",
            ParamKind::KeyReq => "keyreq",
            ParamKind::Key => "key",
            ParamKind::KeyRest => "keyrest",
            ParamKind::Block => "block",
        }
    }
}

pub type Descriptor = Vec<(ParamKind, Option<String>)>;

static PARAMS: LazyLock<Mutex<HashMap<(u32, Symbol), Descriptor>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Records one method's signature -- called once per user `def` from
/// generated `main()`. `entries` is already in Ruby's `#parameters` order.
pub fn register_params(cid: u32, name: &str, entries: Descriptor) {
    PARAMS.lock().insert((cid, Symbol::intern(name)), entries);
}

/// The descriptor for `name` as resolved on `class` -- walking the ancestry so
/// an inherited method resolves against the ancestor that defined it (matching
/// dispatch). `None` when no user `def` registered it (a builtin, or absent).
fn descriptor_of(class: ClassId, name: Symbol) -> Option<Descriptor> {
    {
        let map = PARAMS.lock();
        for &anc in crate::dispatch::ancestors_of_value(class) {
            if let Some(d) = map.get(&(anc.0, name)) {
                return Some(d.clone());
            }
        }
    }
    // Struct/Data member accessors are dispatched dynamically, with no
    // `register_params` descriptor -- derive their shape from the struct meta.
    crate::builtins::rstruct::accessor_params(class, name)
}

/// `Method#arity`: CRuby's signed count -- the number of mandatory parameters
/// (required positionals + post + one if any required keyword), negated and
/// decremented by one when the method takes a variable count (an optional
/// positional, a rest, or a keyword-rest; optional keywords alone do NOT flip
/// the sign). `None` for a method with no registered descriptor (a builtin),
/// letting the caller fall back to its `-1` catch-all.
pub fn arity(class: ClassId, name: Symbol) -> Option<i64> {
    if let Some(d) = descriptor_of(class, name) {
        return Some(arity_of(&d));
    }
    // A builtin (C-defined) method has no `Params` descriptor -- its arity is
    // the argc declared at its `builtin_methods!` definition site. Walk the
    // receiver's ancestry so an inherited builtin resolves against its owner.
    let n = name.name();
    let n = n.as_str();
    crate::dispatch::ancestors_of_value(class)
        .iter()
        .find_map(|&anc| crate::builtins::class_arity_table(anc).and_then(|f| f(n)))
}

/// CRuby's signed arity for one descriptor. Required positionals (a post arg is
/// emitted as a trailing `Req`, so it counts here too) form the mandatory base.
/// An optional positional or a rest makes the method variadic. Keywords act as
/// one unit: ANY required keyword adds a single mandatory slot (the keyword hash
/// is required) and keeps the arity fixed; otherwise an optional keyword or a
/// keyword-rest makes it variadic. A block parameter never affects arity.
pub(crate) fn arity_of(d: &[(ParamKind, Option<String>)]) -> i64 {
    let mut mandatory = d.iter().filter(|(k, _)| *k == ParamKind::Req).count() as i64;
    let mut variadic = d
        .iter()
        .any(|(k, _)| matches!(k, ParamKind::Opt | ParamKind::Rest));
    if d.iter().any(|(k, _)| *k == ParamKind::KeyReq) {
        mandatory += 1;
    } else if d
        .iter()
        .any(|(k, _)| matches!(k, ParamKind::Key | ParamKind::KeyRest))
    {
        variadic = true;
    }
    if variadic {
        -(mandatory + 1)
    } else {
        mandatory
    }
}

/// `Method#parameters`: the array of `[kind, name]` pairs (an anonymous
/// rest/keyrest/block is a one-element `[kind]`). `None` for an unregistered
/// (builtin) method.
pub fn parameters(class: ClassId, name: Symbol) -> Option<RubyValue> {
    let d = descriptor_of(class, name)?;
    let pairs = d
        .into_iter()
        .map(|(kind, pname)| {
            let mut entry = vec![RubyValue::Symbol(Symbol::intern(kind.tag()))];
            if let Some(n) = pname {
                entry.push(RubyValue::Symbol(Symbol::intern(&n)));
            }
            RubyValue::Array(crate::array_new(entry))
        })
        .collect();
    Some(RubyValue::Array(crate::array_new(pairs)))
}
