//! `def`/`class`/`module`/singleton-class lowering: parameter lists, class
//! bodies (including the runtime-class/reopen desugars), `attr_*`/
//! `private`/`public`/`protected`/`module_function`/`alias`/`undef`
//! handling, and the const-holds-a-runtime-class checks that pick between
//! the static and runtime class-lowering paths. Split out of
//! `parse/mod.rs`.

use super::assign::lower_multi_target_group;
use super::consts::constant_path_name;
use super::control::static_bool;
use super::ffi::{
    as_ffi_layout, is_extend_ffi_library, lower_ffi_directive, synthesize_ffi_struct,
};
use super::{PResult, lower_node, parse_and_lower_into};
use crate::hir::{ArrayElem, Hir, HirNode, KeywordParam, NodeId, Params, StrPart, Visibility};
use ruby_prism::{Node, ParseResult};

/// Lowers the branch a statically-folded class-body `if`/`unless` selected --
/// a `StatementsNode` (the `then`/`unless` body), an `ElseNode` (a final
/// `else`), a nested `IfNode` (an `elsif`, re-entering the fold), or `None`
/// (an omitted branch) -- routing each contained statement back through
/// `lower_class_body_statement` so an `alias`/`def`/visibility directive
/// inside the guard still registers.
fn lower_class_body_selected(
    result: &ParseResult,
    hir: &mut Hir,
    chosen: Option<Node<'_>>,
    visibility: &mut Visibility,
    module_function: &mut bool,
    out: &mut Vec<NodeId>,
) -> PResult<()> {
    let Some(node) = chosen else { return Ok(()) };
    if let Some(stmts) = node.as_statements_node() {
        for stmt in stmts.body().iter() {
            lower_class_body_statement(result, hir, &stmt, visibility, module_function, out)?;
        }
        return Ok(());
    }
    if let Some(else_node) = node.as_else_node() {
        if let Some(stmts) = else_node.statements() {
            for stmt in stmts.body().iter() {
                lower_class_body_statement(result, hir, &stmt, visibility, module_function, out)?;
            }
        }
        return Ok(());
    }
    // A nested `elsif` `IfNode`, or any single statement: re-enter the
    // class-body path (which folds the `elsif` in turn).
    lower_class_body_statement(result, hir, &node, visibility, module_function, out)
}

/// `class << obj; def a; ...; end; ...; end` on a NON-`self` receiver:
/// desugar each `def` in the singleton body into a runtime
/// `obj.define_singleton_method(:a, ->(params) { body })`, the same shape
/// `def obj.a` uses. Returns the desugared statement nodes (empty for an empty
/// body). The receiver is re-lowered per def -- exact for the usual simple
/// receiver (a local, `@ivar`, or constant); a side-effecting receiver
/// EXPRESSION would re-evaluate (rare, documented divergence). Caller must have
/// already checked the receiver is not a bare `self`.
pub(crate) fn desugar_singleton_class_defs(
    result: &ruby_prism::ParseResult,
    hir: &mut Hir,
    singleton: &ruby_prism::SingletonClassNode<'_>,
) -> PResult<Vec<NodeId>> {
    let recv_node = singleton.expression();
    // `class << obj` opens no cref of its own -- CRuby walks past a
    // singleton cref, so the body still resolves against the enclosing one.
    let inner = lower_class_body(result, hir, singleton.body(), None, None)?;
    desugar_singleton_items(result, hir, &recv_node, inner)
}

/// Maps each lowered `class << obj` body node onto `recv`: a `def` becomes
/// `recv.define_singleton_method(:name) { body }`; a constant/nested class is
/// HOISTED to the enclosing scope (zeo has no per-object singleton-class
/// namespace -- the singleton methods reference them lexically); a conditional
/// guarding definitions keeps its runtime `if` with each branch mapped the same
/// way. Recursive so a nested `if RUBY_VERSION < "3.2"; module PathAttr; end`
/// (tempfile) composes.
fn desugar_singleton_items(
    result: &ruby_prism::ParseResult,
    hir: &mut Hir,
    recv_node: &Node<'_>,
    ids: Vec<NodeId>,
) -> PResult<Vec<NodeId>> {
    // Classify without holding the `&hir[id]` borrow across the node-building.
    // A short-lived local `Vec<Item>` built and consumed in this one function;
    // boxing the wide `Def` variant to shave the enum would trade a real
    // allocation per def for a lint that doesn't matter at this lifetime.
    #[allow(clippy::large_enum_variant)]
    enum Item {
        Def(String, Params, Vec<NodeId>),
        Const,
        Nested(String, Option<String>, Vec<NodeId>, bool),
        Cond(NodeId, Vec<NodeId>, Vec<NodeId>),
        Alias(String, String),
        SingletonSelf,
        SelfSend,
        Skip,
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let item = match &hir[id] {
            HirNode::DefMethod {
                name,
                params,
                body,
                is_class_method: false,
                ..
            } => Item::Def(name.clone(), params.clone(), body.clone()),
            // A constant inside `class << obj` (`class << RANDOM; MAX = ...;
            // def next; MAX; end; end`, tmpdir) lives on the object's singleton
            // class in real Ruby. zeo has no per-object singleton-class
            // namespace, so it HOISTS the constant to the enclosing lexical
            // scope -- where the singleton methods' bodies resolve it lexically,
            // the same place a bare `MAX` reference in this scope looks (see the
            // `singleton_class_constant` e2e). Documented divergence: it also
            // becomes reachable as `Enclosing::MAX`.
            HirNode::ConstWrite { .. } => Item::Const,
            // A nested class/module in a singleton (`class << Tempfile; module
            // PathAttr; ...; end; end`, tempfile) -- rebuilt as a runtime
            // `Const = Class.new/Module.new { body }`, same as in a runtime
            // class body, and hoisted to the enclosing scope (the singleton-
            // class namespace divergence the constant hoist above documents).
            HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } => Item::Nested(name.clone(), superclass.clone(), body.clone(), *is_module),
            // A conditional guarding definitions (`if RUBY_VERSION < "3.2"; ...`)
            // -- map each branch and keep the runtime `if`.
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => Item::Cond(*cond, then_body.clone(), else_body.clone()),
            // A `private def foo` already lowered the def WITH its visibility
            // and matches the `Def` arm above; a bare `private`/`private :m`
            // leaves a `MethodVisibility` with no per-object singleton spelling
            // -- drop it (compile-only best-effort: the method is still defined
            // on the singleton, just not marked private there).
            HirNode::MethodVisibility { .. } => Item::Skip,
            // `alias new old` inside a singleton (`class << IPSocket; alias
            // getaddress_orig getaddress; ...`, ipaddr) aliases a method on the
            // object's singleton class -- rebind it to
            // `recv.singleton_class.alias_method(:new, :old)`, the same runtime
            // form the runtime-class-body transform emits for an `alias`.
            HirNode::AliasMethod {
                new_name, old_name, ..
            } => Item::Alias(new_name.clone(), old_name.clone()),
            // `class << obj; self; end` -- the idiom that RETURNS the object's
            // singleton class (`self` inside the singleton body IS that class,
            // e.g. bundler's `def gem_class; class << Gem; self; end; end`).
            HirNode::SelfRef => Item::SingletonSelf,
            // A receiver-less call (`class << self; undef_method(:options)`,
            // optparse) runs with the singleton class as `self` in real Ruby --
            // rebind it onto `recv.singleton_class` so it targets the object's
            // singleton, not the enclosing method's self.
            HirNode::Call { receiver: None, .. } => Item::SelfSend,
            _ => {
                return Err("`class << obj` (a per-instance singleton class) supports only instance `def`s, constants, nested classes, aliases, and conditionals here (zeo limitation)".to_string().into());
            }
        };
        match item {
            Item::Def(mname, params, body) => {
                let recv = lower_node(result, hir, recv_node)?;
                let lambda = hir.push(HirNode::Lambda {
                    params,
                    body,
                    method_body: true,
                });
                let sym = hir.push(HirNode::SymbolLit(mname));
                out.push(hir.push(HirNode::Call {
                    receiver: Some(recv),
                    name: "define_singleton_method".to_string(),
                    args: vec![ArrayElem::Single(sym), ArrayElem::Single(lambda)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
            }
            Item::Const => out.push(id),
            Item::Nested(name, superclass, body, is_module) => {
                out.push(runtime_nested_class(
                    hir, name, superclass, body, is_module,
                )?);
            }
            Item::Cond(cond, then_body, else_body) => {
                let then_body = desugar_singleton_items(result, hir, recv_node, then_body)?;
                let else_body = desugar_singleton_items(result, hir, recv_node, else_body)?;
                out.push(hir.push(HirNode::If {
                    cond,
                    then_body,
                    else_body,
                }));
            }
            Item::Alias(new_name, old_name) => {
                let recv = lower_node(result, hir, recv_node)?;
                let singleton = hir.push(HirNode::Call {
                    receiver: Some(recv),
                    name: "singleton_class".to_string(),
                    args: vec![],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                });
                let new_sym = hir.push(HirNode::SymbolLit(new_name));
                let old_sym = hir.push(HirNode::SymbolLit(old_name));
                out.push(hir.push(HirNode::Call {
                    receiver: Some(singleton),
                    name: "alias_method".to_string(),
                    args: vec![ArrayElem::Single(new_sym), ArrayElem::Single(old_sym)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
            }
            Item::SingletonSelf => {
                let recv = lower_node(result, hir, recv_node)?;
                out.push(hir.push(HirNode::Call {
                    receiver: Some(recv),
                    name: "singleton_class".to_string(),
                    args: vec![],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
            }
            Item::SelfSend => {
                let recv = lower_node(result, hir, recv_node)?;
                let singleton = hir.push(HirNode::Call {
                    receiver: Some(recv),
                    name: "singleton_class".to_string(),
                    args: vec![],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                });
                if let HirNode::Call { receiver, .. } = &mut hir[id] {
                    *receiver = Some(singleton);
                }
                out.push(id);
            }
            Item::Skip => {}
        }
    }
    Ok(out)
}

/// Maps each lowered `class << self` body node onto the ENCLOSING class (the
/// `self`-receiver singleton path in `lower_class_body_stmt`): a `def` is
/// retagged as a class method, an `include` becomes an `extend`, a constant
/// passes through, a conditional guarding definitions keeps its runtime `if`
/// with each branch mapped the same way. Recursive so a nested
/// `if defined?(Ractor); def register_scanner ...` (erb/compiler.rb) composes;
/// the branch that runs at load time defines the class method, and
/// `analyze::register_conditional_defs` makes the names visible to compile-time
/// reflection either way.
/// Whether `id`'s subtree ever consults `self` -- an explicit `self`, or a
/// receiver-less call, which sends to it. Statements that do are the only ones
/// a `class << self` body cannot simply hand to the enclosing class body:
/// `self` there is the class, not its singleton.
fn mentions_self(hir: &Hir, id: NodeId) -> bool {
    let mut stack = vec![id];
    while let Some(n) = stack.pop() {
        match &hir[n] {
            HirNode::SelfRef | HirNode::Call { receiver: None, .. } => return true,
            _ => {}
        }
        hir[n].for_each_child(&mut |child| stack.push(child));
    }
    false
}

/// Whether a `class << self` body opens ANOTHER singleton class -- directly,
/// or inside a conditional the mapping keeps. That inner body belongs to the
/// singleton's OWN singleton, one level beyond the enclosing-class retagging
/// this mapping performs: flattened onto the enclosing class it would define
/// `Foo.x` where ruby defines `Foo.singleton_class.x`. Rejected rather than
/// quietly moved to the wrong owner.
fn opens_a_nested_singleton(body: Option<&Node<'_>>) -> bool {
    let Some(node) = body else { return false };
    if node.as_singleton_class_node().is_some() {
        return true;
    }
    if let Some(s) = node.as_statements_node() {
        return s.body().iter().any(|n| opens_a_nested_singleton(Some(&n)));
    }
    if let Some(i) = node.as_if_node() {
        return opens_a_nested_singleton(i.statements().map(|s| s.as_node()).as_ref())
            || opens_a_nested_singleton(i.subsequent().as_ref());
    }
    if let Some(u) = node.as_unless_node() {
        return opens_a_nested_singleton(u.statements().map(|s| s.as_node()).as_ref())
            || opens_a_nested_singleton(u.else_clause().map(|e| e.as_node()).as_ref());
    }
    if let Some(e) = node.as_else_node() {
        return opens_a_nested_singleton(e.statements().map(|s| s.as_node()).as_ref());
    }
    false
}

/// The names of an all-literal-symbol argument list (`:a, :b`), or `None` when
/// any argument is computed -- a directive zeo can only serve at run time.
fn literal_symbol_args(hir: &Hir, args: &[ArrayElem]) -> Option<Vec<String>> {
    if args.is_empty() {
        return None;
    }
    args.iter()
        .map(|a| match a {
            ArrayElem::Single(id) => match &hir[*id] {
                HirNode::SymbolLit(s) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// `self.singleton_class` evaluated in the enclosing class body -- `self` there
/// is the class object, so this is the very class `class << self` opens.
fn own_singleton_class(hir: &mut Hir) -> NodeId {
    let me = hir.push(HirNode::SelfRef);
    hir.push(HirNode::Call {
        receiver: Some(me),
        name: "singleton_class".to_string(),
        args: vec![],
        kwargs: vec![],
        block: None,
        block_arg: None,
        safe: false,
    })
}

fn map_class_self_items(hir: &mut Hir, ids: &[NodeId], out: &mut Vec<NodeId>) -> PResult<()> {
    // Classify without holding the `&hir[id]` borrow across the mutations below.
    enum Item {
        Method,
        ClassAlias,
        Passthrough,
        Extend(String),
        ClassPrepend(String),
        ClassUndef(Vec<String>),
        ClassVisibility(String, crate::hir::Visibility),
        ExtendSingleton(String),
        SingletonIvarWrite(String, NodeId),
        SingletonIvarRead(String),
        SingletonSelf,
        SelfSend,
        Cond(NodeId, Vec<NodeId>, Vec<NodeId>),
        Guarded(
            Vec<NodeId>,
            Vec<crate::hir::RescueClause>,
            Option<Vec<NodeId>>,
            Option<Vec<NodeId>>,
        ),
        Reject,
    }
    for &id in ids {
        let item = match &hir[id] {
            HirNode::DefMethod { .. } => Item::Method,
            // `alias new old` here aliases a SINGLETON method (`class <<
            // self; alias split shellsplit`) -- retag it so analyze/mro
            // resolve it against `own_class_methods`. A same-body target
            // was already cloned as a class-method `DefMethod` by
            // `push_alias` (it preserves `is_class_method`); only a
            // cross-body/inherited source reaches here as an `AliasMethod`.
            HirNode::AliasMethod { .. } => Item::ClassAlias,
            HirNode::ConstWrite { .. } => Item::Passthrough,
            // A `class`/`module` written here belongs to the SINGLETON class
            // (`IRB::Color`'s `class << self; class ColorizeVisitor <
            // Prism::Visitor`). zeo hands it to the enclosing class instead,
            // which gets the property that matters -- the singleton methods
            // beside it reach it by bare name -- at the cost of also
            // answering `Color::ColorizeVisitor`, where real Ruby raises.
            // See `docs/COMPATIBILITY.md`.
            HirNode::ClassDef { .. } => Item::Passthrough,
            // A `@@x` inside `class << self` belongs to the ENCLOSING class,
            // not the singleton: cvar lookup walks past singleton crefs (see
            // `Hir::cvar_is_toplevel`), so passing the node through to the
            // class body is both the simplest and the correct mapping.
            HirNode::ClassVarWrite(..) | HirNode::ClassVarRead(_) => Item::Passthrough,
            HirNode::Include(m) => Item::Extend(m.clone()),
            // `extend M` here mixes M into the singleton's OWN singleton, one
            // level further out than any compile-time ancestry zeo models. It
            // is written for its macros (`extend Forwardable` so the
            // `def_delegators` beside it resolves), so it becomes the runtime
            // `self.singleton_class.extend(M)` real Ruby performs, and the
            // macro call lands on the same receiver.
            HirNode::Extend(m) => Item::ExtendSingleton(m.clone()),
            // `prepend M` here mixes M into the SINGLETON class, so its
            // instance methods become the enclosing class's class methods
            // ahead of its own `def self.x` -- google-protobuf's
            // `TypeSafety`, debug's `ForkInterceptor`. The singleton half of
            // `Include`'s mapping just above, and the exact equivalent of the
            // `C.singleton_class.prepend(M)` call form.
            HirNode::Prepend(m) => Item::ClassPrepend(m.clone()),
            // `undef :m` / `undef_method :m` here retires a CLASS method,
            // inherited ones included (optparse's `undef_method :options`).
            HirNode::Undef(names) => Item::ClassUndef(names.clone()),
            // `private :m` naming a method this body does NOT define is the
            // singleton half of `MethodVisibility` -- i.e. exactly
            // `private_class_method :m` on the enclosing class. (A name the
            // body DOES define was already marked in place on its `DefMethod`,
            // which the retag above carries to the class-method side.)
            HirNode::MethodVisibility { name, visibility } => {
                Item::ClassVisibility(name.clone(), *visibility)
            }
            // A conditional guarding class-method defs (erb/compiler.rb's
            // `class << self; if defined?(Ractor); def register_scanner ...`):
            // map each branch the same way and KEEP the runtime `if`, so the
            // branch that executes at load time defines the class method.
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => Item::Cond(*cond, then_body.clone(), else_body.clone()),
            // A `begin/rescue` guarding class-method definitions -- securerandom
            // picks its `gen_random` implementation this way (`begin;
            // Random.urandom(1); alias gen_random gen_random_urandom; rescue
            // RuntimeError; require "openssl"; ...`). Map every clause the same
            // way and keep the runtime control flow, exactly as the `if` above.
            HirNode::Begin {
                body,
                rescues,
                else_body,
                ensure_body,
            } => Item::Guarded(
                body.clone(),
                rescues.clone(),
                else_body.clone(),
                ensure_body.clone(),
            ),
            // An `@x = v` here writes an ivar of the SINGLETON class, which is
            // a DIFFERENT object from the class -- so the `attr_accessor`
            // written beside it does not read what this wrote (oracle-verified:
            // `class << self; @slack = "x"; attr_accessor :slack; end` leaves
            // `Foo.slack` nil). Writing it on `self.singleton_class` keeps both
            // halves of that: the accessor still answers nil, and a direct
            // `Foo.singleton_class.instance_variable_get(:@slack)` answers what
            // was written.
            HirNode::IvarWrite(name, value) => Item::SingletonIvarWrite(name.clone(), *value),
            HirNode::IvarRead(name) => Item::SingletonIvarRead(name.clone()),
            // `private_constant :X` names a constant of the SINGLETON class,
            // and zeo hoists such a constant into the enclosing class (the
            // `ConstWrite` passthrough above). Passing the directive through
            // WITH it keeps the pair together: `Foo::X` then raises NameError,
            // which is what real Ruby answers too -- there the constant never
            // lived on `Foo` at all. csv's `class << self; ON_WINDOWS = ...;
            // private_constant :ON_WINDOWS`.
            HirNode::ConstantVisibility { .. } => Item::Passthrough,
            // Any other call runs with the SINGLETON class as `self` -- the
            // DSL half of `class << self; extend Forwardable; def_delegators
            // :@config, :timeout`, where the macro defines instance methods of
            // the singleton, i.e. class methods of the enclosing class. Rebind
            // it onto `self.singleton_class` so it reaches the receiver real
            // Ruby gives it, exactly as `desugar_singleton_items` does for the
            // per-object `class << obj` form. Dropping these silently defined
            // nothing at all (fileutils' `public(*METHODS)`, memoist's
            // `memoize`, `Gem::Deprecate`'s `deprecate`).
            // `undef_method :m` is the `undef` keyword by another name, and the
            // corpus writes it far more often (optparse, rspec-mocks). Taking
            // it at compile time is what actually retires the CLASS method: as
            // a runtime send it would only tombstone the singleton, which
            // statically-resolved `Foo.m` call sites never consult.
            HirNode::Call {
                receiver: None,
                name,
                args,
                kwargs,
                block: None,
                block_arg: None,
                ..
            } if name == "undef_method"
                && kwargs.is_empty()
                && let Some(names) = literal_symbol_args(hir, args) =>
            {
                Item::ClassUndef(names)
            }
            HirNode::Call { receiver: None, .. } => Item::SelfSend,
            // An explicit `self` receiver is the singleton class here too.
            HirNode::Call {
                receiver: Some(r), ..
            } if matches!(hir[*r], HirNode::SelfRef) => Item::SelfSend,
            // A call on any OTHER receiver doesn't depend on `self` at all
            // (backports' `class << self; attr_accessor :warned;
            // Backports.warned = {}`), so it runs unchanged at this position.
            HirNode::Call { .. } => Item::Passthrough,
            // `class << self; self; end` -- the idiom whose VALUE is the
            // singleton class (`SINGLETON = class << self; self; end`).
            HirNode::SelfRef => Item::SingletonSelf,
            // Anything else runs unchanged IF it never consults `self`: it
            // then means the same thing in the enclosing class body, at the
            // same position. backports' `Backports.warned = {}` is this --
            // an attribute assignment, which lowering expands into a `Seq`
            // around a temporary. A statement that DOES reach for `self`
            // would silently retarget the enclosing class, so it is rejected.
            _ if !mentions_self(hir, id) => Item::Passthrough,
            _ => Item::Reject,
        };
        match item {
            Item::Method => {
                hir.set_method_is_class_method(id);
                out.push(id);
            }
            Item::ClassAlias => {
                hir.set_alias_is_class_method(id);
                out.push(id);
            }
            Item::Passthrough => out.push(id),
            Item::Extend(m) => out.push(hir.push(HirNode::Extend(m))),
            Item::ClassPrepend(m) => out.push(hir.push(HirNode::ClassMethodPrepend(m))),
            Item::ClassUndef(names) => out.push(hir.push(HirNode::ClassMethodUndef(names))),
            Item::ClassVisibility(name, visibility) => {
                out.push(hir.push(HirNode::ClassMethodVisibility { name, visibility }))
            }
            Item::ExtendSingleton(m) => {
                let singleton = own_singleton_class(hir);
                let module = hir.push(HirNode::ClassRef(m));
                out.push(hir.push(HirNode::Call {
                    receiver: Some(singleton),
                    name: "extend".to_string(),
                    args: vec![ArrayElem::Single(module)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
            }
            Item::SingletonIvarWrite(name, value) => {
                let singleton = own_singleton_class(hir);
                // `IvarWrite`/`IvarRead` carry the BARE name; the reflection
                // methods want the sigil.
                let sym = hir.push(HirNode::SymbolLit(format!("@{name}")));
                out.push(hir.push(HirNode::Call {
                    receiver: Some(singleton),
                    name: "instance_variable_set".to_string(),
                    args: vec![ArrayElem::Single(sym), ArrayElem::Single(value)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
            }
            Item::SingletonIvarRead(name) => {
                let singleton = own_singleton_class(hir);
                let sym = hir.push(HirNode::SymbolLit(format!("@{name}")));
                out.push(hir.push(HirNode::Call {
                    receiver: Some(singleton),
                    name: "instance_variable_get".to_string(),
                    args: vec![ArrayElem::Single(sym)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                }));
            }
            Item::SingletonSelf => {
                let singleton = own_singleton_class(hir);
                out.push(singleton);
            }
            Item::Guarded(body, rescues, else_body, ensure_body) => {
                let mut mapped_body = Vec::with_capacity(body.len());
                map_class_self_items(hir, &body, &mut mapped_body)?;
                let mut mapped_rescues = Vec::with_capacity(rescues.len());
                for r in rescues {
                    let mut rbody = Vec::with_capacity(r.body.len());
                    map_class_self_items(hir, &r.body, &mut rbody)?;
                    mapped_rescues.push(crate::hir::RescueClause { body: rbody, ..r });
                }
                let map_opt = |hir: &mut Hir, b: Option<Vec<NodeId>>| -> PResult<_> {
                    b.map(|b| {
                        let mut out = Vec::with_capacity(b.len());
                        map_class_self_items(hir, &b, &mut out)?;
                        Ok(out)
                    })
                    .transpose()
                };
                let else_body = map_opt(hir, else_body)?;
                let ensure_body = map_opt(hir, ensure_body)?;
                out.push(hir.push(HirNode::Begin {
                    body: mapped_body,
                    rescues: mapped_rescues,
                    else_body,
                    ensure_body,
                }));
            }
            Item::SelfSend => {
                let singleton = own_singleton_class(hir);
                if let HirNode::Call { receiver, .. } = &mut hir[id] {
                    *receiver = Some(singleton);
                }
                out.push(id);
            }
            Item::Cond(cond, then_body, else_body) => match eval_static_class_self_guard(hir, cond)
            {
                // A statically-decidable version/`defined?` guard: register ONLY
                // the taken branch's class methods, exactly the branch real Ruby
                // runs. (A runtime `if` would leave BOTH branches registered and
                // static dispatch would resolve to the last-wins body -- wrong
                // when the branches differ, e.g. erb's Ractor vs non-Ractor
                // `register_scanner`.)
                Some(true) => map_class_self_items(hir, &then_body, out)?,
                Some(false) => map_class_self_items(hir, &else_body, out)?,
                // Runtime-dependent guard: keep the `if` and map each branch
                // (best-effort -- the guard genuinely can't be decided here).
                None => {
                    let mut then_out = Vec::with_capacity(then_body.len());
                    map_class_self_items(hir, &then_body, &mut then_out)?;
                    let mut else_out = Vec::with_capacity(else_body.len());
                    map_class_self_items(hir, &else_body, &mut else_out)?;
                    out.push(hir.push(HirNode::If {
                        cond,
                        then_body: then_out,
                        else_body: else_out,
                    }));
                }
            },
            Item::Reject => {
                return Err(
                    "unsupported statement in `class << self` (zeo limitation): it consults \
                     `self`, which is the singleton class here, and zeo has no compile-time \
                     class to retarget it to. `def`s, constants, `include`/`extend`/`prepend`, \
                     `undef`, visibility directives, ivars, conditionals, `attr_*`/`alias` and \
                     ordinary calls are all handled"
                        .to_string()
                        .into(),
                );
            }
        }
    }
    Ok(())
}

/// Marks every `DefMethod` in a constant-bearing `class << self` body
/// (recursing into kept conditional branches) -- see
/// `Hir::singleton_body_defs`.
fn tag_singleton_body_defs(hir: &mut Hir, ids: &[NodeId]) {
    for &id in ids {
        match &hir[id] {
            HirNode::DefMethod { .. } => {
                hir.singleton_body_defs.insert(id);
            }
            HirNode::If {
                then_body,
                else_body,
                ..
            } => {
                let (t, e) = (then_body.clone(), else_body.clone());
                tag_singleton_body_defs(hir, &t);
                tag_singleton_body_defs(hir, &e);
            }
            _ => {}
        }
    }
}

/// The Ruby version zeo targets -- kept in lockstep with `zeo_rt::bootstrap`'s
/// `VERSION` (the value of the runtime `RUBY_VERSION` constant). Mirrors the
/// loader's hardcoded `RUBY_ENGINE`: zeo compiles to one fixed target, so a
/// `RUBY_VERSION`-gated definition is statically decidable.
const TARGET_RUBY_VERSION: &str = "4.0.6";

/// Toplevel constants zeo's runtime ALWAYS defines, so `defined?(C)` is
/// statically true. Used to pick the live branch of a feature-probe like
/// erb/compiler.rb's `if defined?(Ractor)`. Extend as more `defined?`-gated
/// definitions surface in the require graph.
const ALWAYS_DEFINED_CONSTS: &[&str] = &["Ractor"];

/// A class/module-body `if`/`unless` guard zeo can decide at COMPILE time --
/// [`static_bool`]'s literals, plus the two probes that gate definitions all
/// over the gem graph: `defined?(C)` for a constant the runtime always provides,
/// and `RUBY_VERSION <cmp> "x"` against the one version zeo targets.
///
/// Deciding it matters more than it saves: a `def` in each branch of a guard
/// that stays dynamic leaves TWO definitions of one name in the body, and the
/// later one simply wins -- so `if RUBY_VERSION >= "3.4."` picked the pre-3.4
/// method. This is the same three-valued evaluation
/// [`eval_static_class_self_guard`] does for a `class << self` body, over prism
/// nodes rather than HIR because the class-body path folds before lowering.
fn static_guard(node: &Node<'_>) -> Option<bool> {
    if let Some(b) = static_bool(node) {
        return Some(b);
    }
    if let Some(paren) = node.as_parentheses_node() {
        let stmts = paren.body()?.as_statements_node()?;
        let body: Vec<_> = stmts.body().iter().collect();
        if let [only] = body.as_slice() {
            return static_guard(only);
        }
    }
    if let Some(defined) = node.as_defined_node() {
        let name = defined.value().as_constant_read_node()?.name();
        let name = String::from_utf8_lossy(name.as_slice());
        return Some(ALWAYS_DEFINED_CONSTS.contains(&name.as_ref()));
    }
    let call = node.as_call_node()?;
    let recv = call.receiver()?;
    if String::from_utf8_lossy(recv.as_constant_read_node()?.name().as_slice()) != "RUBY_VERSION" {
        return None;
    }
    let args: Vec<_> = call.arguments()?.arguments().iter().collect();
    let [only] = args.as_slice() else {
        return None;
    };
    let rhs = only.as_string_node()?.unescaped().to_vec();
    let rhs = String::from_utf8(rhs).ok()?;
    let ord = TARGET_RUBY_VERSION.cmp(rhs.as_str());
    use std::cmp::Ordering::{Equal, Greater, Less};
    let op = String::from_utf8_lossy(call.name().as_slice()).into_owned();
    Some(match op.as_str() {
        ">=" => ord != Less,
        ">" => ord == Greater,
        "<" => ord == Less,
        "<=" => ord != Greater,
        "==" => ord == Equal,
        "!=" => ord != Equal,
        _ => return None,
    })
}

/// Three-valued static evaluation of a `class << self` conditional-def guard,
/// enough for the platform/version probes that gate class-method definitions in
/// the stdlib/gem graph: literal `true`/`false`/`nil`, `defined?(C)` for a
/// runtime-provided constant, and `RUBY_VERSION <cmp> "x"` (String#<=>
/// lexicographic, matching how Ruby compares these version strings). `None` when
/// the guard depends on runtime state -- the caller then keeps the runtime `if`.
fn eval_static_class_self_guard(hir: &Hir, cond: NodeId) -> Option<bool> {
    match &hir[cond] {
        HirNode::BoolLit(b) => Some(*b),
        HirNode::NilLit => Some(false),
        HirNode::Defined(inner) => match &hir[*inner] {
            HirNode::ClassRef(name) => Some(ALWAYS_DEFINED_CONSTS.contains(&name.as_str())),
            _ => None,
        },
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } => {
            let HirNode::ClassRef(cname) = &hir[*recv] else {
                return None;
            };
            if cname != "RUBY_VERSION" {
                return None;
            }
            let [ArrayElem::Single(arg)] = args.as_slice() else {
                return None;
            };
            let HirNode::StringLit(parts) = &hir[*arg] else {
                return None;
            };
            let [StrPart::Lit(rhs)] = parts.as_slice() else {
                return None;
            };
            let ord = TARGET_RUBY_VERSION.cmp(rhs.as_str());
            use std::cmp::Ordering::{Equal, Greater, Less};
            Some(match name.as_str() {
                ">=" => ord != Less,
                ">" => ord == Greater,
                "<" => ord == Less,
                "<=" => ord != Greater,
                "==" => ord == Equal,
                "!=" => ord != Equal,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Whether the program ASSIGNS this constant a value anywhere already lowered
/// (`B = Box.new`, `Foo = Class.new`) -- which makes it a value-holding
/// constant rather than the name of a compile-time class. Scans the arena
/// rather than threading a set through lowering: the assignment is lowered
/// before any later statement that reads it, which is the same
/// "defined earlier in the file" rule `Compiler::resolve_class` applies.
///
/// `scope` is folded in so a namespaced `M::D` is matched exactly, never by
/// its leaf alone.
pub(crate) fn const_is_assigned(hir: &Hir, name: &str) -> bool {
    hir.nodes().iter().any(|node| match node {
        HirNode::ConstWrite { scope, name: n, .. } => match scope {
            Some(s) => format!("{s}::{n}") == name,
            None => n == name,
        },
        _ => false,
    })
}

/// Whether every statement in a class body can be expressed as the BLOCK the
/// runtime-class forms lower to. The runtime form runs the body as a block, so
/// a statement that only the static class path can emit (`include`, a
/// visibility modifier, `alias`, a nested class) has no runtime spelling --
/// see `lower_runtime_class_body`, which rejects the same set.
///
/// Only the reopen form consults this, and only to FALL BACK to the static
/// path; `class X < <expression>` has no static fallback (that shape is why
/// the runtime form exists) and reports the rejection instead. A prism-level
/// scan rather than a lowered one so the fallback costs no orphan nodes in the
/// arena -- a stray `ConstWrite` left behind would perturb `const_is_assigned`.
pub(crate) fn runtime_class_body_is_expressible(body: Option<Node<'_>>) -> bool {
    let stmts: Vec<Node<'_>> = match body {
        None => return true,
        Some(n) => match n.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![n],
        },
    };
    stmts.iter().all(|stmt| {
        if stmt.as_alias_method_node().is_some()
            || stmt.as_undef_node().is_some()
            || stmt.as_class_node().is_some()
            || stmt.as_module_node().is_some()
        {
            return false;
        }
        // A local write too: a class body opens its OWN scope, while the block
        // the runtime form becomes closes over the enclosing one. The static
        // path gets that right, so falling back to it is strictly better than
        // either diverging or refusing to compile.
        if stmt.as_local_variable_write_node().is_some()
            || stmt.as_local_variable_operator_write_node().is_some()
            || stmt.as_local_variable_and_write_node().is_some()
            || stmt.as_local_variable_or_write_node().is_some()
            || stmt.as_multi_write_node().is_some()
        {
            return false;
        }
        // `include M` / `private` and friends are receiverless calls, not
        // their own node kinds.
        if let Some(call) = stmt.as_call_node()
            && call.receiver().is_none()
        {
            let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
            return !matches!(
                name.as_str(),
                "include"
                    | "extend"
                    | "prepend"
                    | "private"
                    | "public"
                    | "protected"
                    | "module_function"
                    | "alias_method"
            );
        }
        true
    })
}

/// Whether a constant of this name is assigned a value that MINTS a class at
/// RUNTIME (`Data.define`, `Struct.new`, `Class.new`) -- the only shapes a bare
/// `class Name ... end` should REOPEN (via `class_eval`) rather than define
/// fresh. Stricter than [`const_is_assigned`]: an ordinary value constant
/// (`C = 7`) is ignored, so a same-named constant in an UNRELATED lexical scope
/// (a bare `C = 7` inside `module B`, which lowers to a scope-less `ConstWrite`)
/// does not misroute a fresh nested `class C` onto the runtime-reopen path.
pub(crate) fn const_holds_runtime_class(hir: &Hir, name: &str) -> bool {
    hir.nodes().iter().any(|node| match node {
        HirNode::ConstWrite {
            scope,
            name: n,
            value,
        } => {
            let matches_name = match scope {
                Some(s) => format!("{s}::{n}") == name,
                None => n == name,
            };
            matches_name && value_mints_runtime_class(hir, *value)
        }
        _ => false,
    })
}

/// The member list of a `Struct.new(:a, :b)` zeo can compile to a REAL class,
/// or `None` to leave it on the runtime path.
///
/// Deliberately narrow, and everything it rejects keeps today's complete
/// `rstruct::struct_new` fallback. A compiled struct is an ordinary generated
/// class whose members are hidden slots, so an accessor reaches a field
/// instead of a dynamic send into an overlay closure. What it will not take:
///
/// - `Data.define` (readers only, frozen on construct, `#with`),
/// - `keyword_init:` or any other keyword, and any block body,
/// - a computed or non-symbol member,
/// - a member whose name is not a plain lowercase identifier
///   (`Struct.new(:verbose?)`), or that is a Ruby KEYWORD (`Struct.new(:class)`,
///   which is legal and even shadows `Kernel#class`) -- the synthesized source
///   below spells members as accessor `def` names and `@name` ivars, and
///   neither can be either of those.
///
/// `ZEO_STRUCT=runtime` turns the whole thing off.
/// Legal `Struct` member names that cannot be spelled as a Rust-side parameter
/// or `def` name in the synthesized source. `Struct.new(:class)` is real code
/// -- it even shadows `Kernel#class`, which `issue_2975.rb` pins.
const RUBY_KEYWORDS: &[&str] = &[
    "alias", "and", "begin", "break", "case", "class", "def", "defined", "do", "else", "elsif",
    "end", "ensure", "false", "for", "if", "in", "module", "next", "nil", "not", "or", "redo",
    "rescue", "retry", "return", "self", "super", "then", "true", "undef", "unless", "until",
    "when", "while", "yield",
];

pub(crate) fn as_compiled_struct(value: &Node<'_>) -> Option<Vec<String>> {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *OFF.get_or_init(|| std::env::var("ZEO_STRUCT").is_ok_and(|v| v == "runtime")) {
        return None;
    }
    let call = value.as_call_node()?;
    if String::from_utf8_lossy(call.name().as_slice()) != "new" || call.block().is_some() {
        return None;
    }
    let recv = call.receiver()?;
    if String::from_utf8_lossy(recv.as_constant_read_node()?.name().as_slice()) != "Struct" {
        return None;
    }
    let mut members = Vec::new();
    for arg in call.arguments()?.arguments().iter() {
        let name = String::from_utf8_lossy(arg.as_symbol_node()?.unescaped()).into_owned();
        let mut chars = name.chars();
        let plain = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain || RUBY_KEYWORDS.contains(&name.as_str()) || members.contains(&name) {
            return None;
        }
        members.push(name);
    }
    (!members.is_empty()).then_some(members)
}

/// `NAME = Struct.new(:a, :b)` as an ordinary `class NAME < Struct` with an
/// accessor per member -- so the member is a field, and accessor
/// devirtualization applies to reading it.
///
/// The class body is SOURCE handed to `parse_and_lower_into`, the same route
/// `synthesize_ffi_struct` takes, rather than hand-built HIR: the accessors,
/// the constructor and its nil-filling defaults are exactly what an ordinary
/// `attr_accessor` and `def initialize` already lower to, and going through
/// the parser is what keeps them that way.
///
/// The `@member` slots are recorded as HIDDEN, which is what makes
/// `instance_variables` answer `[]` and `instance_variable_get(:@a)` answer
/// `nil`, as CRuby does. Everything else in the protocol -- `to_a`, `[]`, `==`,
/// `each`, `dig`, `inspect`, `Marshal` -- comes from `Struct`'s own shared
/// table, reached by MRO, and works unchanged because it asks for members by
/// INDEX.
pub(crate) fn synthesize_struct_class(
    hir: &mut Hir,
    name: &str,
    members: &[String],
) -> PResult<NodeId> {
    let accessors = members
        .iter()
        .map(|m| format!(":{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    // `initialize` binds by CALL SHAPE, as `rstruct::bind_members` does on the
    // runtime path (the F-C rule: one semantic kernel, and this synthesized
    // body is its compiled spelling). Keywords bind by member name only when
    // they arrive ALONE -- CRuby's `rb_keyword_given_p && argc == 1` rule --
    // so a positional Hash and the mixed form both stay positional, a short
    // arg list nil-fills, and too many positionals report ruby's exact
    // `struct size differs`.
    let n = members.len();
    let member_syms = members
        .iter()
        .map(|m| format!(":{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    let pos_assigns = members
        .iter()
        .enumerate()
        .map(|(i, m)| format!("      @{m} = args[{i}]\n"))
        .collect::<String>();
    let kw_assigns = members
        .iter()
        .map(|m| format!("      @{m} = kw[:{m}]\n"))
        .collect::<String>();
    let src = format!(
        "class {name} < Struct\n  attr_accessor {accessors}\n  \
         def initialize(*args, **kw)\n    \
         if kw.empty? || !args.empty?\n      \
         args = args + [kw] unless kw.empty?\n      \
         raise ArgumentError, \"struct size differs\" if args.size > {n}\n\
         {pos_assigns}    \
         else\n      \
         bad = kw.keys.reject {{ |k| [{member_syms}].include?(k) }}\n      \
         raise ArgumentError, \"unknown keywords: #{{bad.join(', ')}}\" unless bad.empty?\n\
         {kw_assigns}    \
         end\n  end\nend\n"
    );
    // The offsets `parse_and_lower_into` produces index `src`, not the file
    // being lowered, so the class would claim a position it never occupied --
    // one that reads as EARLIER than everything above it. Re-stamp it with the
    // `NAME = Struct.new(...)` the user actually wrote, which is where ruby
    // reports the class as declared and where `const_added` announces it.
    let written_at = hir.current_span();
    let nodes = parse_and_lower_into(hir, &src)?;
    let [class_def] = nodes[..] else {
        return Err(crate::lower_error::LowerError::syntax(
            "a synthesized struct class must lower to exactly one ClassDef",
        ));
    };
    hir.set_span(class_def, written_at);
    hir.struct_members.insert(class_def, members.to_vec());
    Ok(class_def)
}

/// Whether `value` is a `Data.define(...)` / `Struct.new(...)` / `Class.new(...)`
/// call -- an expression that produces a class object at runtime.
fn value_mints_runtime_class(hir: &Hir, value: NodeId) -> bool {
    let HirNode::Call {
        receiver: Some(r),
        name,
        ..
    } = &hir[value]
    else {
        return false;
    };
    let HirNode::ClassRef(recv) = &hir[*r] else {
        return false;
    };
    let recv = recv.strip_prefix("::").unwrap_or(recv);
    matches!(
        (recv, name.as_str()),
        ("Data", "define") | ("Struct", "new") | ("Class", "new") | ("Module", "new")
    )
}

/// A QUALIFIED reference (`Aws::EmptyStructure`) to a constant that was written
/// unqualified inside its own module.
///
/// `class EmptyStructure < Struct.new(...)` inside `module Aws` lowers to a
/// bare `EmptyStructure = Class.new(...)`: the module nesting is the body's
/// context, not part of the name. A later `class Output < Aws::EmptyStructure`
/// spells it in full, so [`const_is_assigned`]'s exact match misses and the
/// subclass takes the static path, where the name resolves to nothing.
///
/// Matching on the LEAF alone is safe only because the value must mint a
/// class: an ordinary `X = 7` in some unrelated scope cannot misroute a
/// subclass onto the runtime path.
pub(crate) fn qualified_const_mints_runtime_class(hir: &Hir, name: &str) -> bool {
    let Some(leaf) = name.rsplit("::").next() else {
        return false;
    };
    if leaf == name {
        return false;
    }
    hir.nodes().iter().any(|node| match node {
        HirNode::ConstWrite {
            scope: None,
            name: n,
            value,
        } => n == leaf && value_mints_runtime_class(hir, *value),
        _ => false,
    })
}

/// Whether an already-lowered `class`/`module` DEFINES this name, making it a
/// compile-time class even if some later statement also assigns the constant.
pub(crate) fn const_is_class_def(hir: &Hir, name: &str) -> bool {
    hir.nodes()
        .iter()
        .any(|node| matches!(node, HirNode::ClassDef { name: n, .. } if n == name))
}

/// `AliasMethodNode`'s `new_name`/`old_name` -- always a `SymbolNode` in
/// practice (confirmed via `Prism.parse`: both the bareword `alias new old`
/// and symbol `alias :new :old` spellings produce the identical node shape),
/// but checked defensively (a clean `Err`, not a panic) rather than assumed.
pub fn alias_target_name(node: &Node<'_>) -> PResult<String> {
    let sym = node
        .as_symbol_node()
        .ok_or("`alias`'s target must be a plain method name (zeo limitation)")?;
    Ok(String::from_utf8_lossy(sym.unescaped()).into_owned())
}

/// Registers `alias new old` / `alias_method :new, :old` into the current
/// class/module body. When `old` is defined EARLIER IN THIS SAME BODY, the
/// source `DefMethod` is cloned directly (nothing to defer -- no runtime
/// target needed). Otherwise `old` is an INHERITED method whose definition
/// isn't in this body and whose ancestry isn't linearized until `analyze`, so
/// a deferred `HirNode::AliasMethod` is emitted for `mro::resolve_aliases` to
/// resolve later. See `HirNode::AliasMethod`.
/// Turns the instance `def` at `id` into a module function: real Ruby keeps
/// BOTH halves, a public module method and a PRIVATE instance method for the
/// `include`-mixin, so the original becomes the private half and a class-method
/// copy joins it. `false` (and nothing pushed) when `id` isn't a `DefMethod`.
///
/// Whether `id` is already in `out` is the caller's business: the bare
/// `module_function` mode pushes it here, while `module_function :name` found
/// it there in the first place.
fn promote_to_module_function(hir: &mut Hir, id: NodeId, out: &mut Vec<NodeId>) -> bool {
    let HirNode::DefMethod {
        name, params, body, ..
    } = &hir[id]
    else {
        return false;
    };
    let (name, params, body) = (name.clone(), params.clone(), body.clone());
    hir.set_method_visibility(id, Visibility::Private);
    if !out.contains(&id) {
        out.push(id);
    }
    // `push_from`, not `push`: the module copy is the SAME definition, so ruby
    // reports the `def`'s own line for both halves (oracle-verified). Under a
    // plain `push` the copy inherited the enclosing module's span, which made
    // `Mod.method(:m).source_location` name the `module` line.
    out.push(hir.push_from(
        HirNode::DefMethod {
            name,
            params,
            body,
            is_class_method: true,
            visibility: Visibility::Public,
            is_def: true,
        },
        id,
    ));
    true
}

fn push_alias(hir: &mut Hir, out: &mut Vec<NodeId>, new_name: String, old_name: String) {
    if let Some(&old_id) = out
        .iter()
        .rev()
        .find(|&&id| matches!(&hir[id], HirNode::DefMethod { name, .. } if *name == old_name))
    {
        let HirNode::DefMethod {
            params,
            body,
            is_class_method,
            visibility,
            is_def,
            ..
        } = &hir[old_id]
        else {
            unreachable!("guarded by the `find` above")
        };
        let (params, body, is_class_method, visibility, is_def) = (
            params.clone(),
            body.clone(),
            *is_class_method,
            *visibility,
            *is_def,
        );
        // Carrying the SOURCE's span, not the `alias` line's: an alias
        // reports its original's `source_location`, as in CRuby.
        let cloned = hir.push_from(
            HirNode::DefMethod {
                name: new_name,
                params,
                body,
                is_class_method,
                visibility,
                is_def,
            },
            old_id,
        );
        hir.record_alias_origin(cloned, old_name);
        out.push(cloned);
    } else {
        out.push(hir.push(HirNode::AliasMethod {
            new_name,
            old_name,
            is_class_method: false,
        }));
    }
}

/// Required-parameter-only helper for a `posts`/`requireds` entry -- both
/// only ever contain `RequiredParameterNode`s (Ruby's grammar guarantees a
/// splat's "post" params are always plain required names, same as the
/// params before it).
fn required_param_name(node: &Node<'_>, where_: &str) -> PResult<String> {
    let p = node.as_required_parameter_node().ok_or_else(|| {
        format!("only plain required parameters are supported {where_} (zeo limitation)")
    })?;
    Ok(String::from_utf8_lossy(p.name().as_slice()).into_owned())
}

/// One entry of `requireds()`/`posts()`: either a plain name, or a
/// parenthesized DESTRUCTURING target list (`|a, (b, c)|`), which prism
/// surfaces as a `MultiTargetNode` in the very same slot -- the same node
/// type, with the same `lefts()`/`rest()`/`rights()` grammar, that a
/// multi-assignment's nested group uses. So it lowers through the same
/// `lower_multi_target_group`, and the slot itself gets an internal name
/// (`__destr_<i>`) that behaves as an ordinary required param everywhere
/// else -- see `Params::destructures`.
///
/// Returns the slot's name, pushing onto `destructures` when it destructures.
fn required_param_slot(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    where_: &str,
    destructures: &mut Vec<(NodeId, crate::hir::MultiTargetGroup)>,
) -> PResult<String> {
    let Some(mt) = node.as_multi_target_node() else {
        return required_param_name(node, where_);
    };
    let group = lower_multi_target_group(result, hir, mt.lefts(), mt.rest(), mt.rights())?;
    let slot = format!("__destr_{}", destructures.len());
    let read = hir.push(HirNode::LocalRead(slot.clone()));
    destructures.push((read, group));
    Ok(slot)
}

/// Full `ParametersNode` lowering: required -> optional (default evaluated
/// LAZILY by the callee -- see `Params::optional`'s docs, so its expression
/// is only lowered here, never eagerly evaluated at every call site) ->
/// rest (`*`/`*name`) -> post (required params after a splat) -> keyword
/// (required/optional) -> keyword_rest (`**`/`**name`/explicit `**nil`) ->
/// `&block`/anonymous `&` (same `None`/`Some(None)`/`Some(Some(name))` shape
/// as `rest`/`keyword_rest` -- see `hir::Params::block`'s docs). Bare `...`
/// forwarding (positional + keyword + block all at once) is a separate,
/// still-unsupported call-site construct -- see the `keyword_rest` match arm
/// below, which gives it a dedicated rejection message.
pub(crate) fn lower_params(
    result: &ParseResult,
    hir: &mut Hir,
    params: Option<ruby_prism::ParametersNode<'_>>,
) -> PResult<Params> {
    let Some(params) = params else {
        return Ok(Params::default());
    };
    // `def m(...)` -- bare forwarding. Prism surfaces it as a
    // `ForwardingParameterNode` occupying the `keyword_rest` slot (with
    // `.rest()`/`.block()` both `None`). Desugared here into three
    // compiler-internal named params (`*__fwd_rest, **__fwd_kw,
    // &__fwd_blk`); the call-site `n(...)` (a `ForwardingArgumentsNode`)
    // references the same names -- no new HIR shape, no special runtime.
    let forwarding = params
        .keyword_rest()
        .is_some_and(|n| n.as_forwarding_parameter_node().is_some());
    // Anonymous `&` (`def m(&)`) forwards via the same internal-name trick
    // (`n(&)` references it); a named `&blk` stays itself.
    let block = match params.block() {
        Some(b) => Some(Some(match b.name() {
            Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
            None => "__anon_blk".to_string(),
        })),
        None if forwarding => Some(Some("__fwd_blk".to_string())),
        None => None,
    };

    let mut destructures = Vec::new();
    let required = params
        .requireds()
        .iter()
        .map(|n| required_param_slot(result, hir, &n, "before a `*rest`", &mut destructures))
        .collect::<PResult<Vec<_>>>()?;

    let optional = params
        .optionals()
        .iter()
        .map(|n| {
            let p = n
                .as_optional_parameter_node()
                .ok_or("expected an optional parameter (zeo limitation)")?;
            let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
            let default = lower_node(result, hir, &p.value())?;
            Ok((name, default))
        })
        .collect::<PResult<Vec<_>>>()?;

    let rest = match params.rest() {
        None if forwarding => Some(Some("__fwd_rest".to_string())),
        None => None,
        // A TRAILING COMMA (`|a, |`) -- prism's `ImplicitRestNode`. It means
        // "this block takes more than one parameter", which is what turns on
        // auto-splat, and then discards everything past the named ones:
        // `m([1, 2]) { |a, | a }` is `1`, not `[1, 2]` (oracle-verified).
        // That is exactly an anonymous `*`, so it lowers as one and the
        // existing arity/auto-splat rules cover it with no special case.
        Some(n) if n.as_implicit_rest_node().is_some() => Some(None),
        Some(n) => {
            let r = n
                .as_rest_parameter_node()
                .ok_or("unsupported rest-parameter form (zeo limitation)")?;
            // Anonymous `*` (`def m(*)`) gets an internal name so `n(*)`
            // can forward it (Ruby 3.2's anonymous-forwarding semantics).
            Some(Some(match r.name() {
                Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
                None => "__anon_rest".to_string(),
            }))
        }
    };

    let post = params
        .posts()
        .iter()
        .map(|n| required_param_slot(result, hir, &n, "after a `*rest`", &mut destructures))
        .collect::<PResult<Vec<_>>>()?;

    let keywords = params
        .keywords()
        .iter()
        .map(|n| {
            if let Some(p) = n.as_required_keyword_parameter_node() {
                Ok(KeywordParam::Required(
                    String::from_utf8_lossy(p.name().as_slice()).into_owned(),
                ))
            } else if let Some(p) = n.as_optional_keyword_parameter_node() {
                let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
                let default = lower_node(result, hir, &p.value())?;
                Ok(KeywordParam::Optional(name, default))
            } else {
                Err("unsupported keyword parameter form (zeo limitation)".into())
            }
        })
        .collect::<PResult<Vec<_>>>()?;

    // `**nil` binds nothing, so it takes no `keyword_rest` slot; what it
    // declares is recorded on `no_keywords` instead.
    let no_keywords = params
        .keyword_rest()
        .is_some_and(|n| n.as_no_keywords_parameter_node().is_some());
    let keyword_rest = match params.keyword_rest() {
        None => None,
        // Bare `...` forwarding (a `ForwardingParameterNode` in this slot)
        // -- desugared to `**__fwd_kw` here; `rest`/`block` above already
        // synthesized their `__fwd_*` halves.
        Some(n) if n.as_forwarding_parameter_node().is_some() => Some(Some("__fwd_kw".to_string())),
        Some(n) if n.as_no_keywords_parameter_node().is_some() => None,
        Some(n) => {
            let r = n
                .as_keyword_rest_parameter_node()
                .ok_or("unsupported keyword-rest parameter form (zeo limitation)")?;
            // Anonymous `**` gets an internal name so `n(**)` can forward
            // it, same as the anonymous-`*` rule above.
            Some(Some(match r.name() {
                Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
                None => "__anon_kwrest".to_string(),
            }))
        }
    };

    Ok(Params {
        required,
        destructures,
        optional,
        rest,
        post,
        keywords,
        keyword_rest,
        no_keywords,
        block,
        // Filled in by `lower_block_like_params` for a block: prism keeps
        // `|x; sum|`'s locals on the BlockParametersNode, not here on the
        // ParametersNode. Always empty for a method's params -- the syntax
        // doesn't exist there.
        block_locals: Vec::new(),
        // Populated by `lower_block` from prism's block-scope local table;
        // always empty for a method's params.
        implicit_block_locals: Vec::new(),
    })
}

/// A class body's statement list -- like `lower_statement_list`, but
/// recognizes a handful of zero-receiver call shapes at this exact position
/// (mirroring `lower_node`'s own `define_method`/`loop` desugars) that a
/// strict 1-statement-to-1-node map can't express: `attr_reader`/
/// `attr_writer`/`attr_accessor` each expand into MULTIPLE synthesized
/// `DefMethod`s from one statement, and `private`/`public`/`protected` expand
/// into NONE.
/// `class Name < <expression>` -- a subclass of a class that does not exist
/// until run time (`Struct.new(:x, :y)`, `Class.new`, a class held in a
/// variable). `analyze::register_class` can only link a subclass to a parent
/// it already knows statically, so this desugars to the runtime form zeo
/// already supports end to end: `Name = Class.new(<expression>) { <body> }`.
///
/// The one place the desugar is NOT a faithful rewrite is local-variable
/// scope: a class body opens a FRESH scope, while the block body it becomes
/// closes over the enclosing one, so `y = 1` in the body would assign the
/// caller's `y` instead of a new one. A `def` in the body is unaffected (its
/// own body already resolves in a fresh scope -- verified against the
/// oracle), so only a direct local write is rejected, rather than left to
/// diverge silently.
pub(crate) fn lower_runtime_class(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    superclass: &Node<'_>,
    body: Option<Node<'_>>,
) -> PResult<NodeId> {
    let parent = lower_node(result, hir, superclass)?;
    let block = lower_runtime_class_body(result, hir, name, body)?;
    let class_class = hir.push(HirNode::ClassRef("Class".to_string()));
    let new_call = hir.push(HirNode::Call {
        receiver: Some(class_class),
        name: "new".to_string(),
        args: vec![ArrayElem::Single(parent)],
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    });
    // `class NS::Item < ...` has to write `Item` INSIDE `NS`, not a flat
    // constant that happens to be spelled `"NS::Item"` -- the latter reads
    // back only through the identical spelling, and leaves `NS.constants`
    // empty.
    let path = crate::constpath::ConstPath::parse(name);
    Ok(hir.push(HirNode::ConstWrite {
        scope: path.scope().map(str::to_string),
        name: path.base().to_string(),
        value: new_call,
    }))
}

/// `class D ... end` REOPENING a constant that holds a runtime class
/// (`D = Data.define(:x)`) -- lowered to `D.class_eval { <body> }`, which
/// installs onto the existing class. The static path would instead register a
/// brand-new, memberless class `D`, so a generated `Data`/`Struct` reader
/// could not resolve inside the reopened body.
pub(crate) fn lower_runtime_class_reopen(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    body: Option<Node<'_>>,
) -> PResult<NodeId> {
    let block = lower_runtime_class_body(result, hir, name, body)?;
    let target = hir.push(HirNode::ClassRef(name.to_string()));
    Ok(hir.push(HirNode::Call {
        receiver: Some(target),
        name: "class_eval".to_string(),
        args: Vec::new(),
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    }))
}

/// The shared body half of the two runtime-class desugars: lowers the class
/// body and wraps it as the block those forms pass. See `lower_runtime_class`
/// for why a local write in the body is rejected rather than diverging.
fn lower_runtime_class_body(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    body: Option<Node<'_>>,
) -> PResult<NodeId> {
    // A runtime class body runs as an ordinary BLOCK, which opens no cref.
    let body = lower_class_body(result, hir, body, None, None)?;
    if body
        .iter()
        .any(|&n| matches!(hir[n], HirNode::LocalWrite(..)))
    {
        return Err(format!(
            "local variable assignment in the body of `class {name}` is not supported \
             when {name} is built at runtime (the body would see the enclosing scope's \
             locals instead of its own)"
        )
        .into());
    }
    // The body runs as a BLOCK with the new class as `self`, so a statement
    // that only the static class path can emit (`include`, a visibility
    // directive, `alias`, a nested class) would reach codegen's "top-level-only
    // node in expression position" panic. Rewrite each into its runtime
    // spelling -- a self-send the runtime class receiver serves -- so the class
    // builds at runtime. See `transform_runtime_class_body`.
    let body = transform_runtime_class_body(hir, body)?;
    Ok(hir.push(HirNode::Block {
        params: Params::default(),
        body,
    }))
}

/// Rewrites the static-only nodes a lowered class body can hold into the
/// runtime self-sends a `Class.new { ... }`/`class_eval { ... }` block serves,
/// so a class with a DYNAMIC superclass (`class Tempfile < DelegateClass(File)`)
/// or a runtime reopen can carry the same bodies a statically-registered class
/// can. `def`/`ConstWrite`/`class << self`'s class-method `def`s pass through
/// untouched (codegen already emits those in block position). A nested class/
/// module is desugared to a runtime `Const = Class.new(Super) { body }` (its
/// body transformed the same way, recursively).
///
/// This is the COMPILE path: the rewritten sends resolve at runtime through the
/// class/module builtins (`include`/`alias_method`/visibility/...). A construct
/// with no runtime builtin still lowers (it becomes a self-send that raises
/// NoMethodError only if actually executed) -- acceptable for a runtime class
/// on an otherwise-unreachable path, and never worse than the previous hard
/// compile error.
fn transform_runtime_class_body(hir: &mut Hir, body: Vec<NodeId>) -> PResult<Vec<NodeId>> {
    // Classify without holding the `&hir[id]` borrow across the node-building
    // mutations below (each rewrite pushes fresh nodes).
    enum Rewrite {
        Mixin(&'static str, String),
        Alias(String, String),
        Visibility(&'static str, String),
        Undef(Vec<String>),
        ModuleFunction(String),
        Nested(String, Option<String>, Vec<NodeId>, bool),
        Cond(NodeId, Vec<NodeId>, Vec<NodeId>),
        Keep,
    }
    let mut out = Vec::with_capacity(body.len());
    for id in body {
        let rewrite = match &hir[id] {
            HirNode::Include(m) => Rewrite::Mixin("include", m.clone()),
            HirNode::Extend(m) => Rewrite::Mixin("extend", m.clone()),
            HirNode::Prepend(m) => Rewrite::Mixin("prepend", m.clone()),
            HirNode::AliasMethod {
                new_name, old_name, ..
            } => Rewrite::Alias(new_name.clone(), old_name.clone()),
            HirNode::MethodVisibility { name, visibility } => {
                Rewrite::Visibility(visibility_name(*visibility), name.clone())
            }
            HirNode::Undef(names) => Rewrite::Undef(names.clone()),
            HirNode::ModuleFunction(name) => Rewrite::ModuleFunction(name.clone()),
            HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } => Rewrite::Nested(name.clone(), superclass.clone(), body.clone(), *is_module),
            // A conditional guarding definitions (`if RUBY_VERSION < "3.2";
            // module PathAttr; ...; end`, tempfile): transform each branch the
            // same way and KEEP the runtime `if`, so the conditional still
            // decides at runtime which definitions execute.
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => Rewrite::Cond(*cond, then_body.clone(), else_body.clone()),
            _ => Rewrite::Keep,
        };
        let node = match rewrite {
            Rewrite::Mixin(method, m) => {
                let arg = class_ref(hir, &m);
                runtime_self_send(hir, method, vec![arg])
            }
            Rewrite::Alias(new_name, old_name) => {
                let args = vec![sym_lit(hir, new_name), sym_lit(hir, old_name)];
                runtime_self_send(hir, "alias_method", args)
            }
            Rewrite::Visibility(vis, name) => {
                let args = vec![sym_lit(hir, name)];
                runtime_self_send(hir, vis, args)
            }
            Rewrite::Undef(names) => {
                let args = names.into_iter().map(|n| sym_lit(hir, n)).collect();
                runtime_self_send(hir, "undef_method", args)
            }
            Rewrite::ModuleFunction(name) => {
                let args = vec![sym_lit(hir, name)];
                runtime_self_send(hir, "module_function", args)
            }
            Rewrite::Nested(name, superclass, inner, is_module) => {
                runtime_nested_class(hir, name, superclass, inner, is_module)?
            }
            Rewrite::Cond(cond, then_body, else_body) => {
                let then_body = transform_runtime_class_body(hir, then_body)?;
                let else_body = transform_runtime_class_body(hir, else_body)?;
                hir.push(HirNode::If {
                    cond,
                    then_body,
                    else_body,
                })
            }
            // Anything with no runtime spelling of its own passes through --
            // but a class-body DIRECTIVE reaching here means the two halves of
            // the table have drifted, and codegen would report it far from its
            // cause ("top-level-only node in expression position").
            Rewrite::Keep => {
                debug_assert!(
                    !hir[id].is_class_body_directive(),
                    "class-body directive with no runtime rewrite -- add it to \
                     `transform_runtime_class_body` alongside `is_class_body_directive`"
                );
                id
            }
        };
        out.push(node);
    }
    Ok(out)
}

/// A receiver-less (implicit-`self`) runtime call node -- the class body block's
/// `self` is the runtime class, so this dispatches to its class/module builtin.
fn runtime_self_send(hir: &mut Hir, name: &str, args: Vec<NodeId>) -> NodeId {
    hir.push(HirNode::Call {
        receiver: None,
        name: name.to_string(),
        args: args.into_iter().map(ArrayElem::Single).collect(),
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

fn class_ref(hir: &mut Hir, name: &str) -> NodeId {
    hir.push(HirNode::ClassRef(name.to_string()))
}

fn sym_lit(hir: &mut Hir, name: String) -> NodeId {
    hir.push(HirNode::SymbolLit(name))
}

fn visibility_name(v: Visibility) -> &'static str {
    match v {
        Visibility::Private => "private",
        Visibility::Protected => "protected",
        Visibility::Public => "public",
    }
}

/// A nested `class C < S; body; end` (or `module`) inside a runtime class body,
/// rebuilt as a runtime `C = Class.new(S) { body }` / `C = Module.new { body }`
/// const-assignment -- an ordinary expression that lives in block position. The
/// nested body is transformed the same way, so nesting composes.
fn runtime_nested_class(
    hir: &mut Hir,
    name: String,
    superclass: Option<String>,
    body: Vec<NodeId>,
    is_module: bool,
) -> PResult<NodeId> {
    let inner = transform_runtime_class_body(hir, body)?;
    let block = hir.push(HirNode::Block {
        params: Params::default(),
        body: inner,
    });
    let (builder, args) = if is_module {
        ("Module", Vec::new())
    } else {
        let parent = class_ref(hir, &superclass.unwrap_or_else(|| "Object".to_string()));
        ("Class", vec![ArrayElem::Single(parent)])
    };
    let builder_ref = hir.push(HirNode::ClassRef(builder.to_string()));
    let new_call = hir.push(HirNode::Call {
        receiver: Some(builder_ref),
        name: "new".to_string(),
        args,
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    });
    let path = crate::constpath::ConstPath::parse(&name);
    Ok(hir.push(HirNode::ConstWrite {
        scope: path.scope().map(str::to_string),
        name: path.base().to_string(),
        value: new_call,
    }))
}

/// `cref` is the class/module name this body OPENS, or `None` for a body that
/// opens no cref of its own -- a `class << obj` / `class << self` (CRuby walks
/// past a singleton cref) and a runtime class body, which is an ordinary block.
/// See `Hir::in_class_body`.
/// The holder module a `refine Target do ... end` puts its methods in.
/// Deliberately unspellable as a Ruby constant, so the holder claims no
/// name inside the refining module -- `M.constants` stays what the source
/// wrote -- and it is the same name CRuby prints for `M.refinements.first`.
/// A qualified target flattens (`Foo::Bar` -> `Foo.Bar`) so the name reads
/// as one leaf rather than a nested path.
pub(crate) fn refinement_holder_name(target: &str) -> String {
    format!("#refinement:{}", target.replace("::", "."))
}

/// `using M` in any position: `Some(node)` once the shape matched -- no
/// receiver, one bare constant argument. Anything else answers `None` and
/// falls through to an ordinary call, which is a clean rejection later if
/// nothing else defines `using`.
pub(crate) fn lower_using(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    name: &str,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<Option<Vec<NodeId>>> {
    if name != "using" || call.receiver().is_some() {
        return Ok(None);
    }
    let arg_list: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [arg] = arg_list.as_slice() else {
        return Ok(None);
    };
    if let Ok(module) = constant_path_name(arg) {
        hir.push_span(crate::lower::span_of(hir, node));
        let id = hir.push(HirNode::Using(module));
        hir.pop_span();
        return Ok(Some(vec![id]));
    }
    // `using Module.new { refine C do ... end }` -- irb's shape, and the only
    // way to activate a refinement over a class chosen where no constant
    // names the module. The anonymous module becomes an ordinary
    // compile-time module under an unwritable `#using:`-prefixed name (the
    // refinement-holder convention: claims no constant, `Module#name` stays
    // nil), so the whole existing rewrite -- holder registration, byte-range
    // activation, refined call sites -- applies unchanged.
    if let Some(mcall) = arg.as_call_node()
        && String::from_utf8_lossy(mcall.name().as_slice()) == "new"
        && mcall
            .receiver()
            .and_then(|r| {
                r.as_constant_read_node()
                    .map(|c| c.name().as_slice().to_vec())
            })
            .is_some_and(|n| n == b"Module")
        && let Some(block) = mcall.block().and_then(|b| b.as_block_node())
    {
        let span = crate::lower::span_of(hir, node);
        let anon = format!("#using:{}", span.start);
        let body = lower_class_body(result, hir, block.body(), None, Some(&anon))?;
        hir.push_span(span);
        let def = hir.push(HirNode::ClassDef {
            name: anon.clone(),
            superclass: None,
            body,
            is_module: true,
        });
        let using = hir.push(HirNode::Using(anon));
        hir.pop_span();
        return Ok(Some(vec![def, using]));
    }
    Ok(None)
}

pub(crate) fn lower_class_body(
    result: &ParseResult,
    hir: &mut Hir,
    body: Option<Node<'_>>,
    superclass: Option<&str>,
    cref: Option<&str>,
) -> PResult<Vec<NodeId>> {
    let stmts: Vec<Node<'_>> = match body {
        None => return Ok(Vec::new()),
        Some(n) => match n.as_statements_node() {
            Some(stmts) => stmts.body().iter().collect(),
            None => vec![n],
        },
    };
    // A `class T < FFI::Struct` turns its `layout` directive into
    // synthesized `[]`/`[]=`/`size`/`offset_of`/`pointer` methods over an
    // `FFI::MemoryPointer` ivar -- see `synthesize_ffi_struct`.
    let is_ffi_struct = superclass == Some("FFI::Struct");
    let mut out = Vec::new();
    // The DEFAULT visibility for every subsequent `def` in this class body,
    // switched by a bare `private`/`public`/`protected` (no arguments) --
    // see `lower_class_body_statement`'s docs.
    let mut visibility = Visibility::Public;
    let mut module_function = false;
    // A module that `extend FFI::Library` (the real `ffi` gem) turns its
    // `ffi_lib`/`attach_function` directives into synthesized wrapper class
    // methods over `extern "C"` symbols -- see `lower_ffi_directive`. A
    // NON-FFI statement in such a module still lowers normally (a module may
    // mix), so this only re-routes the recognized directives.
    let is_ffi = stmts.iter().any(is_extend_ffi_library);
    let mut ffi_lib: Option<String> = None;
    // `typedef :existing, :alias` names accumulated in source order, so a later
    // `attach_function` can name an alias the gem requires be declared first.
    let mut ffi_aliases: std::collections::HashMap<String, crate::hir::FfiType> =
        std::collections::HashMap::new();
    // This is the ONE place a `class`/`module` body's statements are lowered
    // (the runtime-class desugars route through here too), so it is also the
    // one place the cref chain deepens -- see `Hir::cvar_is_toplevel`.
    let mut lower_stmts = |hir: &mut Hir| {
        for stmt in &stmts {
            if is_ffi {
                if is_extend_ffi_library(stmt) {
                    continue; // `extend FFI::Library` is the marker, no output
                }
                if lower_ffi_directive(result, hir, stmt, &mut ffi_lib, &mut ffi_aliases, &mut out)?
                {
                    continue;
                }
            }
            if is_ffi_struct && let Some(fields) = as_ffi_layout(stmt)? {
                // Replace `layout ...` in place with the synthesized accessors,
                // so any user methods after it can still override them.
                let source = synthesize_ffi_struct(&fields)?;
                out.extend(parse_and_lower_into(hir, &source)?);
                continue;
            }
            lower_class_body_statement(
                result,
                hir,
                stmt,
                &mut visibility,
                &mut module_function,
                &mut out,
            )?;
        }
        PResult::Ok(())
    };
    match cref {
        Some(name) => hir.in_class_body(name, lower_stmts)?,
        None => lower_stmts(hir)?,
    }
    Ok(out)
}

/// `attr_reader :a, :b` -> a `DefMethod` getter per name (`body: [IvarRead]`).
/// `attr_writer :a, :b` -> a `DefMethod` setter per name (`name=`, one
/// required param, `body: [IvarWrite]`). `attr_accessor` emits both. Only
/// literal symbol arguments are recognized (matching `define_method`'s own
/// literal-name restriction elsewhere in this file); anything else falls
/// through to an ordinary `Call` (which real Ruby would resolve dynamically,
/// e.g. `attr_reader(*names)` -- unsupported, a clean rejection at
/// codegen if `attr_reader` itself isn't otherwise defined). Every
/// synthesized getter/setter gets the CURRENT default `visibility`, exactly
/// like an ordinary `def` would.
///
/// `private`/`public`/`protected` recognize three real Ruby forms, appending
/// nothing to `out` themselves (they're never a standalone HIR node): (1) a
/// bare call with no arguments switches the DEFAULT `visibility` for every
/// `def` for the REST of this class body; (2) `private def name; ... end`
/// (the `def`-as-sole-argument idiom) lowers the `def` normally through the
/// generic `lower_node` path, then retroactively overrides ITS OWN
/// visibility; (3) `private :name1, :name2, ...` retroactively overrides
/// the visibility of already-lowered method(s) of those names (searched in
/// `out`, everything lowered so far in this same class body -- real Ruby
/// requires the target already be defined earlier in the same body, so no
/// forward search is needed). Anything else (a dynamic/computed argument)
/// falls through to an ordinary `Call` -- a clean rejection at codegen time
/// if `private`/`public`/`protected` themselves aren't otherwise defined,
/// matching this function's own posture elsewhere.
fn lower_class_body_statement(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    visibility: &mut Visibility,
    module_function: &mut bool,
    out: &mut Vec<NodeId>,
) -> PResult<()> {
    // `alias new_name old_name` / `alias :new_name :old_name` (`AliasMethodNode`
    // -- a real Ruby KEYWORD, not a method call, so this is checked before the
    // `as_call_node()` cascade below). `old_name` must already be defined
    // EARLIER in this SAME class/module body (searched in `out`, exactly the
    // same "no forward search, no ancestor walk" restriction `private
    // :name1, :name2` already enforces above) -- aliasing an INHERITED
    // method is a clean rejection, a documented, narrow scope-cut. Resolved
    // entirely at LOWERING time: since the found `DefMethod`'s `params`/
    // `body`/`is_class_method`/`visibility` are all cheaply `Clone`-able,
    // the alias is just a second `DefMethod` node under a different name --
    // no new analyze-phase machinery, no shared-body indirection to keep in
    // sync with `super`/materialization.
    // `undef foo, bar` -- a keyword like `alias`, same target shape (prism
    // gives each name as a SymbolNode either way), so it reuses
    // `alias_target_name`. Recorded rather than resolved here: see
    // `HirNode::Undef` for why the inherited case rules out deleting a def.
    // A class-body `if`/`unless` guarding a `def`/`alias`/visibility directive
    // with a statically-literal predicate (`alias a b if true`) is folded at
    // definition time -- real Ruby runs these guards while the class body
    // executes, and an `alias`/`def` inside one has no ordinary value-`if`
    // lowering (they're class-body-only keywords). A dynamic predicate falls
    // through to the generic value-`if` path unchanged.
    if let Some(if_node) = node.as_if_node()
        && let Some(cond) = static_guard(&if_node.predicate())
    {
        let chosen = if cond {
            if_node.statements().map(|s| s.as_node())
        } else {
            if_node.subsequent()
        };
        return lower_class_body_selected(result, hir, chosen, visibility, module_function, out);
    }
    if let Some(unless_node) = node.as_unless_node()
        && let Some(cond) = static_guard(&unless_node.predicate())
    {
        let chosen = if !cond {
            unless_node.statements().map(|s| s.as_node())
        } else {
            unless_node.else_clause().map(|e| e.as_node())
        };
        return lower_class_body_selected(result, hir, chosen, visibility, module_function, out);
    }

    if let Some(undef) = node.as_undef_node() {
        let names = undef
            .names()
            .iter()
            .map(|n| alias_target_name(&n))
            .collect::<PResult<Vec<_>>>()?;
        out.push(hir.push(HirNode::Undef(names)));
        return Ok(());
    }

    if let Some(alias) = node.as_alias_method_node() {
        let new_name = alias_target_name(&alias.new_name())?;
        let old_name = alias_target_name(&alias.old_name())?;
        push_alias(hir, out, new_name, old_name);
        return Ok(());
    }

    // `class << self ... end` (`SingletonClassNode`) -- reopens the class's
    // OWN singleton class, the idiomatic way to define several class
    // methods at once without repeating `def self.` on each one. `class <<
    // obj` on any expression OTHER than a bare `self` is a per-instance
    // singleton class -- a materially bigger feature (a dynamically-
    // growable per-instance vtable) zeo doesn't support, matching
    // the plan's existing scope-cut on `define_singleton_method`; a clean
    // rejection, not silently ignored. The nested body is lowered through
    // the ORDINARY class-body path (so `attr_reader`/`private`/`alias`/
    // nested `def`s all work exactly as they would directly in the class
    // body), then each result is mapped onto the ENCLOSING class:
    //   - a `def`     -> retagged as a class method (`set_method_is_class_method`);
    //   - a constant  -> spliced onto the enclosing class. Real Ruby scopes a
    //     `class << self` constant to the SINGLETON class (so `C::NAME`
    //     NameErrors), but its only common use is lexical reference from the
    //     singleton's own methods -- which are now the enclosing class's class
    //     methods, and those resolve the enclosing class's constants (verified
    //     against the oracle). Documented divergence: external `C::NAME`
    //     resolves here where CRuby raises.
    //   - `include M` -> `extend M` on the enclosing class (M's instance
    //     methods become class methods either way -- same effect).
    //   - `prepend M`/`undef`/`private :m` -> their class-method halves
    //     (`ClassMethodPrepend`/`ClassMethodUndef`/`ClassMethodVisibility`);
    //   - any other call, and `extend M` -> rebound onto
    //     `self.singleton_class`, the receiver real Ruby runs them against.
    // A nested `class << self` stays a clean rejection.
    if let Some(singleton) = node.as_singleton_class_node() {
        // `class << HTTP` written INSIDE `class HTTP` IS `class << self` --
        // net/http spells its class-method aliases that way, and routing it
        // through the per-object desugar would install them on a runtime
        // singleton the compile-time tables never see. Same rule (and same
        // reason) as `def HTTP.version_1_2` -- see `lower::names_enclosing_class`.
        let is_self = singleton.expression().as_self_node().is_some()
            || crate::lower::names_enclosing_class(hir, &singleton.expression());
        if !is_self {
            // `class << obj` on a NON-`self` receiver: each `def` in
            // the body is a per-object singleton method (see
            // `desugar_singleton_class_defs`).
            out.extend(desugar_singleton_class_defs(result, hir, &singleton)?);
            return Ok(());
        }
        if opens_a_nested_singleton(singleton.body().as_ref()) {
            return Err(
                "a nested `class << self` isn't supported yet (zeo limitation) -- its \
                        body belongs to the singleton's own singleton, which zeo has no \
                        compile-time class for"
                    .to_string()
                    .into(),
            );
        }
        let inner = lower_class_body(result, hir, singleton.body(), None, None)?;
        let mut mapped = Vec::new();
        map_class_self_items(hir, &inner, &mut mapped)?;
        // A constant assigned here belongs to the SINGLETON class, not the
        // enclosing module (`M.const_defined?(:SC)` is false where
        // `M.singleton_class.const_defined?(:SC)` is true). The constants
        // move into a surrogate child definition under the reserved name
        // `#<Class:self>` -- no Ruby constant can collide with it -- which
        // `analyze::register_class` files as an ordinary module and the
        // runtime singleton mint answers for `M.singleton_class` (see
        // `zeo_rt::register_singleton_surrogate`). The `def`s beside them
        // are tagged: their lexical home is the singleton, so a bare `SC`
        // resolves against the surrogate and `Module.nesting` reports it.
        let (consts, rest): (Vec<NodeId>, Vec<NodeId>) = mapped
            .into_iter()
            .partition(|&n| matches!(hir[n], HirNode::ConstWrite { .. }));
        if !consts.is_empty() {
            out.push(hir.push(HirNode::ClassDef {
                name: "#<Class:self>".to_string(),
                superclass: None,
                body: consts,
                is_module: true,
            }));
            tag_singleton_body_defs(hir, &rest);
        }
        out.extend(rest);
        return Ok(());
    }

    if let Some(call) = node.as_call_node()
        && call.receiver().is_none()
    {
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
        if matches!(name.as_str(), "private" | "public" | "protected") {
            let new_vis = match name.as_str() {
                "private" => Visibility::Private,
                "protected" => Visibility::Protected,
                _ => Visibility::Public,
            };
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.is_empty() {
                *visibility = new_vis;
                return Ok(());
            }
            if arg_list.len() == 1 && arg_list[0].as_def_node().is_some() {
                let id = lower_node(result, hir, &arg_list[0])?;
                hir.set_method_visibility(id, new_vis);
                out.push(id);
                return Ok(());
            }
            if arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                for n in &arg_list {
                    let target = String::from_utf8_lossy(
                        n.as_symbol_node().expect("checked above").unescaped(),
                    )
                    .into_owned();
                    if let Some(&id) = out.iter().find(|&&id| {
                            matches!(&hir[id], HirNode::DefMethod { name: existing, .. } if *existing == target)
                        }) {
                            hir.set_method_visibility(id, new_vis);
                        } else {
                            // Re-declaring an INHERITED method's visibility (no
                            // local `def` to retag): recorded for codegen to
                            // apply after materialization. See
                            // `HirNode::MethodVisibility`.
                            out.push(hir.push(HirNode::MethodVisibility {
                                name: target,
                                visibility: new_vis,
                            }));
                        }
                }
                return Ok(());
            }
            // Falls through to the generic `Call` lowering below --
            // a dynamic/computed argument (e.g. `private(*names)`).
        }
        // `private_class_method :a, :b` / `private_class_method def self.x`
        // and their `public_` counterpart -- the class-method half of
        // `private`/`public` above, and lowered the same two ways: the
        // wrapped `def` is lowered on its own terms (so the running
        // visibility default and `module_function` mode reach it exactly as
        // a bare `def` does) and then retagged, while a name with no local
        // `def` becomes a deferred override. Unlike `private`, there is no
        // argument-less mode: Ruby has no running class-method default.
        if matches!(
            name.as_str(),
            "private_class_method" | "public_class_method"
        ) {
            let new_vis = if name == "private_class_method" {
                Visibility::Private
            } else {
                Visibility::Public
            };
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.len() == 1 && arg_list[0].as_def_node().is_some() {
                let before = out.len();
                lower_class_body_statement(
                    result,
                    hir,
                    &arg_list[0],
                    visibility,
                    module_function,
                    out,
                )?;
                for &id in &out[before..] {
                    hir.set_method_visibility(id, new_vis);
                }
                return Ok(());
            }
            if !arg_list.is_empty() && arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                for n in &arg_list {
                    let target = String::from_utf8_lossy(
                        n.as_symbol_node().expect("checked above").unescaped(),
                    )
                    .into_owned();
                    let local = out.iter().find(|&&id| {
                            matches!(&hir[id], HirNode::DefMethod { name: existing, is_class_method: true, .. } if *existing == target)
                        });
                    match local {
                        Some(&id) => hir.set_method_visibility(id, new_vis),
                        None => out.push(hir.push(HirNode::ClassMethodVisibility {
                            name: target,
                            visibility: new_vis,
                        })),
                    }
                }
                return Ok(());
            }
        }
        // `private_constant :A, :B` / `public_constant :A` -- a class-body
        // directive, not a call. The reference it must reject is a
        // qualified `M::A` from outside `M`, which zeo resolves at compile
        // time, so the names are carried to the compiler rather than left
        // for a runtime flag nothing could consult. A dynamic argument
        // falls through to the generic `Call` (still validated + recorded
        // at run time, just not enforced statically).
        if name == "private_constant" || name == "public_constant" {
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if !arg_list.is_empty() && arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                let names = arg_list
                    .iter()
                    .map(|n| {
                        String::from_utf8_lossy(
                            n.as_symbol_node().expect("checked above").unescaped(),
                        )
                        .into_owned()
                    })
                    .collect();
                out.push(hir.push(HirNode::ConstantVisibility {
                    names,
                    private: name == "private_constant",
                }));
                return Ok(());
            }
        }
        // `module_function` -- recognized in the same two forms as
        // `private`/`public`/`protected`: (1) a bare call switches a mode
        // so every subsequent `def` in this body becomes a MODULE method
        // (`Mod.name`); (2) `module_function :a, :b` retroactively
        // promotes already-defined method(s) of those names. Real Ruby
        // ALSO keeps a private instance copy for `include`-mixin; zeo
        // models only the module-method form (see
        // `module_function_namespace.rb`), so promotion is an in-place
        // retag to `is_class_method`, not an added copy -- which also
        // lets an uncalled module function be dead-code-eliminated
        // exactly like any other uncalled class method. A dynamic/
        // computed argument falls through to a generic `Call`.
        if name == "module_function" {
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.is_empty() {
                *module_function = true;
                return Ok(());
            }
            if arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                for n in &arg_list {
                    let target = String::from_utf8_lossy(
                        n.as_symbol_node().expect("checked above").unescaped(),
                    )
                    .into_owned();
                    match out.iter().rev().find(|&&id| {
                            matches!(&hir[id], HirNode::DefMethod { name: existing, is_class_method: false, .. } if *existing == target)
                        }) {
                            Some(&id) => {
                                promote_to_module_function(hir, id, out);
                            }
                            // Not defined here: it arrives through an
                            // `include`, so only `mro` can find it.
                            None => out.push(hir.push(HirNode::ModuleFunction(target))),
                        }
                }
                return Ok(());
            }
        }
        // `alias_method :new, :old` -- the method-call spelling of the
        // `alias` keyword, routed through the same `push_alias` (so an
        // inherited source defers to `mro::resolve_aliases`). Only two
        // literal symbol/string names; anything else falls through to a
        // generic `Call`.
        if name == "alias_method" {
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            if arg_list.len() == 2 {
                let names: Option<Vec<String>> = arg_list
                    .iter()
                    .map(|n| {
                        n.as_symbol_node()
                            .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                            .or_else(|| {
                                n.as_string_node()
                                    .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                            })
                    })
                    .collect();
                if let Some(names) = names {
                    push_alias(hir, out, names[0].clone(), names[1].clone());
                    return Ok(());
                }
            }
        }
        // `include Mod`/`extend Mod`/`prepend Mod` -- one or more bare
        // constant arguments, applied left-to-right (see `HirNode::
        // Include`'s docs for the multi-arg ordering rule). Anything
        // else (a non-constant argument, e.g. a computed module
        // expression) falls through to an ordinary `Call`, a clean
        // rejection at codegen time (zeo limitation: only a literal module
        // name is resolvable to a `ClassId` at compile time anyway).
        if matches!(name.as_str(), "include" | "extend" | "prepend")
            && let Some(args) = call.arguments()
        {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            // Only the all-constant form (`include Mod`) has a
            // compile-time module name. A non-constant argument
            // (`extend self`, a computed module expression) falls
            // through to a runtime `Call` -- e.g. `extend self` mixes a
            // module's own instance methods into its singleton at load.
            if let Ok(names) = arg_list
                .iter()
                .map(constant_path_name)
                .collect::<PResult<Vec<_>>>()
                && !names.is_empty()
            {
                // A single multi-arg `include`/`prepend` keeps its
                // arguments in SOURCE order in the ancestry
                // (`include A, B` -> [self, A, B]; `prepend A, B` ->
                // [A, B, self]). `analyze::mro` flattens the
                // registered mixin list with a uniform
                // "later-registered-is-closer" reversal -- correct
                // for SEPARATE statements (`include A; include B` ->
                // [self, B, A]) -- so one statement's args must be
                // registered in REVERSE to survive that reversal in
                // source order. `extend` uses a distinct singleton
                // path (`runtime_extend`) that already dispatches
                // first-arg-wins, so it keeps source order.
                let ordered: Vec<_> = if name == "extend" {
                    names
                } else {
                    names.into_iter().rev().collect()
                };
                // Stamped with the directive's OWN line, not the
                // enclosing class body's: an unresolvable target is
                // deferred to a runtime `NameError` raised from
                // right here (`analyze::defer_unresolved_directive`),
                // and a class-body span would put that frame on the
                // `class`/`module` line instead.
                hir.push_span(crate::lower::span_of(hir, node));
                out.extend(ordered.into_iter().map(|n| {
                    hir.push(match name.as_str() {
                        "include" => HirNode::Include(n),
                        "extend" => HirNode::Extend(n),
                        _ => HirNode::Prepend(n),
                    })
                }));
                hir.pop_span();
                return Ok(());
            }
        }
        // `refine Target do ... end` -- the block's `def`s become an
        // ordinary class body on a HOLDER module named for the target,
        // and a `Refine` marker records which class they refine. The
        // holder is a real `ClassDef` so every existing mechanism
        // (method registration, materialization, the module bridge that
        // emits a module's own methods as `RubyValue`-self functions)
        // carries it with no new machinery. Its name is unwritable as a
        // constant, so it claims no name inside the enclosing module.
        if name == "refine"
            && let (Some(args), Some(block)) = (call.arguments(), call.block())
        {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let (1, Some(block)) = (arg_list.len(), block.as_block_node()) {
                let target = constant_path_name(&arg_list[0])?;
                let holder = refinement_holder_name(&target);
                let body = lower_class_body(result, hir, block.body(), None, Some(&holder))?;
                hir.push_span(crate::lower::span_of(hir, node));
                out.push(hir.push(HirNode::ClassDef {
                    name: holder.clone(),
                    superclass: None,
                    body,
                    is_module: true,
                }));
                out.push(hir.push(HirNode::Refine { target, holder }));
                hir.pop_span();
                return Ok(());
            }
        }
        if let Some(nodes) = lower_using(result, hir, node, &name, &call)? {
            out.extend(nodes);
            return Ok(());
        }
        if matches!(
            name.as_str(),
            "attr" | "attr_reader" | "attr_writer" | "attr_accessor"
        ) && let Some(args) = call.arguments()
        {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if !arg_list.is_empty() && arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
                // The accessors this expands to are Ruby-visible
                // methods whose `source_location` is the
                // `attr_accessor` line -- an expansion that never
                // descends through `lower_node` has to stamp that
                // provenance itself.
                hir.push_span(crate::lower::span_of(hir, node));
                for n in &arg_list {
                    let ivar = String::from_utf8_lossy(
                        n.as_symbol_node().expect("checked above").unescaped(),
                    )
                    .into_owned();
                    // `attr :x` == `attr_reader :x` (the symbol form):
                    // getter for everything but attr_writer, setter only
                    // for attr_writer/attr_accessor.
                    if name != "attr_writer" {
                        let read = hir.push(HirNode::IvarRead(ivar.clone()));
                        let getter = hir.push(HirNode::DefMethod {
                            name: ivar.clone(),
                            params: Params::default(),
                            body: vec![read],
                            is_class_method: false,
                            visibility: *visibility,
                            is_def: true,
                        });
                        hir.attr_generated.insert(getter);
                        out.push(getter);
                    }
                    if matches!(name.as_str(), "attr_writer" | "attr_accessor") {
                        let param = "value".to_string();
                        let read_param = hir.push(HirNode::LocalRead(param.clone()));
                        let write = hir.push(HirNode::IvarWrite(ivar.clone(), read_param));
                        let setter = hir.push(HirNode::DefMethod {
                            name: format!("{ivar}="),
                            params: Params {
                                required: vec![param],
                                ..Params::default()
                            },
                            body: vec![write],
                            is_class_method: false,
                            visibility: *visibility,
                            is_def: true,
                        });
                        hir.attr_generated.insert(setter);
                        out.push(setter);
                    }
                }
                hir.pop_span();
                return Ok(());
            }
        }
    }
    // An ordinary `def` gets the CURRENT default visibility -- the generic
    // `lower_node` path (reached below) always sets `Public` (it has no
    // notion of a class body's running default; see its own docs), so this
    // corrects it retroactively when the current default isn't `Public`.
    if let Some(def) = node.as_def_node() {
        let id = lower_node(result, hir, node)?;
        // The running visibility default and `module_function` promotion apply
        // only to a bare `def name` (an instance method). A SINGLETON def
        // (`def self.name` / `def Recv.name`) defines a method on another
        // object, is unaffected by either in real Ruby, and doesn't lower to a
        // plain instance `DefMethod` -- so leave it exactly as lowered.
        if def.receiver().is_some() {
            out.push(id);
            return Ok(());
        }
        if *visibility != Visibility::Public {
            hir.set_method_visibility(id, *visibility);
        }
        // Under a bare `module_function`, every following `def` becomes a
        // MODULE method AND a PRIVATE instance method (real Ruby keeps both,
        // so an `include`d module's method is callable bare). The original
        // `def` stays as the private instance method; a class-method copy is
        // added alongside it.
        if *module_function && promote_to_module_function(hir, id, out) {
            return Ok(());
        }
        out.push(id);
        return Ok(());
    }
    out.push(lower_node(result, hir, node)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every class-body directive has to have a runtime spelling: a class built
    /// at runtime runs its body as an ordinary block, where the static-path node
    /// has no meaning. `HirNode::is_class_body_directive` is the shared table,
    /// and it is exhaustive, so the only way to grow the directive set without
    /// tripping this test is to also teach `transform_runtime_class_body` the
    /// rewrite -- which is the point.
    #[test]
    fn every_class_body_directive_has_a_runtime_rewrite() {
        let directives = [
            HirNode::Include("M".to_string()),
            HirNode::Extend("M".to_string()),
            HirNode::Prepend("M".to_string()),
            HirNode::Undef(vec!["m".to_string()]),
            HirNode::AliasMethod {
                new_name: "a".to_string(),
                old_name: "b".to_string(),
                is_class_method: false,
            },
            HirNode::MethodVisibility {
                name: "m".to_string(),
                visibility: Visibility::Private,
            },
            HirNode::ModuleFunction("m".to_string()),
        ];
        for node in directives {
            assert!(
                node.is_class_body_directive(),
                "this test only covers directives"
            );
            let mut hir = Hir::default();
            let id = hir.push(node);
            let out = transform_runtime_class_body(&mut hir, vec![id])
                .expect("a directive rewrites rather than erroring");
            assert!(
                !hir[out[0]].is_class_body_directive(),
                "class-body directive left unrewritten for a runtime class body"
            );
        }
    }
}
