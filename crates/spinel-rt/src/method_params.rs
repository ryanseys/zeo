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

type Descriptor = Vec<(ParamKind, Option<String>)>;

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
    let map = PARAMS.lock();
    for &anc in crate::dispatch::ancestors_of_value(class) {
        if let Some(d) = map.get(&(anc.0, name)) {
            return Some(d.clone());
        }
    }
    None
}

/// `Method#arity`: CRuby's signed count -- the number of mandatory parameters
/// (required positionals + post + one if any required keyword), negated and
/// decremented by one when the method takes a variable count (an optional
/// positional, a rest, or a keyword-rest; optional keywords alone do NOT flip
/// the sign). `None` for a method with no registered descriptor (a builtin),
/// letting the caller fall back to its `-1` catch-all.
pub fn arity(class: ClassId, name: Symbol) -> Option<i64> {
    let d = descriptor_of(class, name)?;
    let mandatory = d
        .iter()
        .filter(|(k, _)| matches!(k, ParamKind::Req | ParamKind::KeyReq))
        .count() as i64;
    // A required keyword contributes at most one to the mandatory count, no
    // matter how many there are (CRuby: `def f(a:, b:)` has arity 1).
    let req_kw = d.iter().filter(|(k, _)| *k == ParamKind::KeyReq).count() as i64;
    let mandatory = mandatory - req_kw + i64::from(req_kw > 0);
    let variadic = d
        .iter()
        .any(|(k, _)| matches!(k, ParamKind::Opt | ParamKind::Rest | ParamKind::KeyRest));
    Some(if variadic { -(mandatory + 1) } else { mandatory })
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
