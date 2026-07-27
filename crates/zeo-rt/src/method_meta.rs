//! The compile-time-baked facts about each user-defined method that Ruby can
//! ask back for: its parameter list (`Method#arity`/`#parameters`), where it
//! was written (`#source_location`, and the ` file:line` tail of `#inspect`),
//! and -- for an alias -- the name it was born under (`#original_name`).
//!
//! The dispatch tables themselves carry only fn pointers, so this side table
//! is where a signature lives. Codegen emits one [`MethodMeta`] registration
//! per user `def` from facts it already has (that method's `Params`, its
//! `def` node's span, its alias source); the table is populated once from
//! generated `main()` and read-only afterwards -- the same posture as the
//! class registry and the constant store.

use crate::{ClassId, RubyValue, Symbol};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

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

/// Which of a class's two method tables a row describes. `Foo#bar` and
/// `Foo.bar` are different methods that happen to share a name, so they need
/// to be keyed apart.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum MethodKind {
    Instance,
    Singleton,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MethodKey {
    class: u32,
    kind: MethodKind,
    name: Symbol,
}

/// One method's compile-time facts, and its own builder: generated code names
/// the method, adds only the facts that `def` actually has, and registers it.
///
/// ```ignore
/// zeo_rt::MethodMeta::instance(7, "greet")
///     .with_params(vec![(zeo_rt::ParamKind::Req, Some("sound".to_string()))])
///     .defined_at("animal.rb", 2)
///     .register();
///
/// zeo_rt::MethodMeta::instance(7, "yell").aliased_from("greet").register();
/// ```
pub struct MethodMeta {
    key: MethodKey,
    params: Descriptor,
    source: Option<(&'static str, u32)>,
    original_name: Option<Symbol>,
}

impl MethodMeta {
    /// A row for `class`'s instance method `name` (`def name`).
    pub fn instance(class: u32, name: &str) -> MethodMeta {
        MethodMeta::keyed(class, MethodKind::Instance, name)
    }

    /// A row for `class`'s singleton method `name` (`def self.name`).
    pub fn singleton(class: u32, name: &str) -> MethodMeta {
        MethodMeta::keyed(class, MethodKind::Singleton, name)
    }

    fn keyed(class: u32, kind: MethodKind, name: &str) -> MethodMeta {
        MethodMeta {
            key: MethodKey {
                class,
                kind,
                name: Symbol::intern(name),
            },
            params: Descriptor::new(),
            source: None,
            original_name: None,
        }
    }

    /// The signature, already in Ruby's canonical `#parameters` order.
    pub fn with_params(mut self, params: Descriptor) -> MethodMeta {
        self.params = params;
        self
    }

    /// Where the `def` keyword sits. `file` is a literal baked into the
    /// binary; `line` is 1-based, as Ruby reports it.
    pub fn defined_at(mut self, file: &'static str, line: u32) -> MethodMeta {
        self.source = Some((file, line));
        self
    }

    /// The name this method was born under, when it reached `name` through an
    /// `alias`/`alias_method`. Chains are pre-resolved by the compiler, so
    /// this is always the ORIGINAL, never an intermediate alias.
    pub fn aliased_from(mut self, original: &str) -> MethodMeta {
        self.original_name = Some(Symbol::intern(original));
        self
    }

    pub fn register(self) {
        META.write().insert(self.key, Arc::new(self));
    }

    pub fn params(&self) -> &Descriptor {
        &self.params
    }

    pub fn source(&self) -> Option<(&'static str, u32)> {
        self.source
    }

    /// The pre-alias name, or this method's own name when it is not an alias
    /// -- what `Method#original_name` answers either way.
    pub fn original_name(&self) -> Symbol {
        self.original_name.unwrap_or(self.key.name)
    }
}

static META: LazyLock<RwLock<HashMap<MethodKey, Arc<MethodMeta>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The row for `name` as resolved on `class`, walking the ancestry so an
/// inherited method resolves against the ancestor that defined it (matching
/// dispatch). `None` when the compiler registered nothing -- a builtin, a
/// `define_method`, or a name that simply isn't there.
pub fn lookup(class: ClassId, kind: MethodKind, name: Symbol) -> Option<Arc<MethodMeta>> {
    let map = META.read();
    crate::dispatch::ancestors_of_value(class)
        .iter()
        .find_map(|&anc| map.get(&MethodKey { class: anc.0, kind, name }).cloned())
}

/// The parameter descriptor for `name` on `class`. Struct/Data member
/// accessors are dispatched dynamically with no registration of their own, so
/// their shape is derived from the struct meta instead.
fn descriptor_of(class: ClassId, kind: MethodKind, name: Symbol) -> Option<Descriptor> {
    if let Some(meta) = lookup(class, kind, name) {
        return Some(meta.params.clone());
    }
    crate::builtins::rstruct::accessor_params(class, name)
}

/// `Method#arity`: CRuby's signed count -- the number of mandatory parameters
/// (required positionals + post + one if any required keyword), negated and
/// decremented by one when the method takes a variable count (an optional
/// positional, a rest, or a keyword-rest; optional keywords alone do NOT flip
/// the sign). `None` for a method with no registered descriptor (a builtin),
/// letting the caller fall back to its `-1` catch-all.
pub fn arity(class: ClassId, kind: MethodKind, name: Symbol) -> Option<i64> {
    if let Some(d) = descriptor_of(class, kind, name) {
        return Some(arity_of(&d));
    }
    builtin_arity(class, name)
}

/// A builtin (C-defined) method has no `Params` descriptor -- its arity is the
/// argc declared in its class's `ruby_class!`/`ruby_module!` definition. Walk
/// the receiver's ancestry so an inherited builtin resolves against its owner.
pub(crate) fn builtin_arity(class: ClassId, name: Symbol) -> Option<i64> {
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
/// rest/keyrest/block is a one-element `[kind]`). A builtin has no descriptor,
/// so its shape is synthesized from its declared arity the way CRuby reports a
/// C function: `n >= 0` mandatory anonymous slots, or `-n-1` of them followed
/// by a rest.
pub fn parameters(class: ClassId, kind: MethodKind, name: Symbol) -> Option<RubyValue> {
    let d = descriptor_of(class, kind, name)
        .or_else(|| builtin_arity(class, name).map(anonymous_descriptor))?;
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

/// `Method#source_location` / `UnboundMethod#source_location`: the
/// `[file, line]` codegen baked from the `def` keyword's span, or `nil` for a
/// method with no Ruby source here -- a builtin, or a body this runtime
/// synthesized. CRuby answers `nil` for its own C methods the same way.
pub fn source_location(class: ClassId, kind: MethodKind, name: Symbol) -> RubyValue {
    let Some((file, line)) = lookup(class, kind, name).and_then(|m| m.source) else {
        return RubyValue::Nil;
    };
    RubyValue::Array(crate::array_new(vec![
        RubyValue::Str(crate::string_new(file.to_string())),
        RubyValue::Int(line as i64),
    ]))
}

/// The nameless descriptor CRuby reports for a method defined in C, derived
/// from its arity alone: `2` is `[[:req], [:req]]`, `-1` is `[[:rest]]`, and
/// `-2` is `[[:req], [:rest]]`.
pub(crate) fn anonymous_descriptor(arity: i64) -> Descriptor {
    let (required, rest) = if arity >= 0 {
        (arity, false)
    } else {
        (-arity - 1, true)
    };
    let mut d: Descriptor = (0..required).map(|_| (ParamKind::Req, None)).collect();
    if rest {
        d.push((ParamKind::Rest, None));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arity_counts_mandatory_and_detects_variadic() {
        let req = |n: &str| (ParamKind::Req, Some(n.to_string()));
        assert_eq!(arity_of(&[]), 0);
        assert_eq!(arity_of(&[req("a"), req("b")]), 2);
        assert_eq!(arity_of(&[req("a"), (ParamKind::Opt, Some("b".into()))]), -2);
        assert_eq!(arity_of(&[(ParamKind::Rest, None)]), -1);
        // A required keyword adds one mandatory slot and stays fixed; an
        // optional one only makes the method variadic.
        assert_eq!(arity_of(&[req("a"), (ParamKind::KeyReq, Some("k".into()))]), 2);
        assert_eq!(arity_of(&[req("a"), (ParamKind::Key, Some("k".into()))]), -2);
        // A block parameter never counts.
        assert_eq!(arity_of(&[req("a"), (ParamKind::Block, Some("b".into()))]), 1);
    }

    #[test]
    fn a_builtin_arity_becomes_an_anonymous_descriptor() {
        let kinds = |d: Descriptor| d.into_iter().map(|(k, n)| (k.tag(), n)).collect::<Vec<_>>();
        assert!(anonymous_descriptor(0).is_empty());
        assert_eq!(kinds(anonymous_descriptor(2)), [("req", None), ("req", None)]);
        assert_eq!(kinds(anonymous_descriptor(-1)), [("rest", None)]);
        assert_eq!(
            kinds(anonymous_descriptor(-3)),
            [("req", None), ("req", None), ("rest", None)]
        );
    }
}
