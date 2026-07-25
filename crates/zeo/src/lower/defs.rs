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
    let inner = lower_class_body(result, hir, singleton.body(), None)?;
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
fn map_class_self_items(hir: &mut Hir, ids: &[NodeId], out: &mut Vec<NodeId>) -> PResult<()> {
    // Classify without holding the `&hir[id]` borrow across the mutations below.
    enum Item {
        Method,
        ClassAlias,
        Passthrough,
        Extend(String),
        Cond(NodeId, Vec<NodeId>, Vec<NodeId>),
        Skip,
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
            HirNode::Include(m) => Item::Extend(m.clone()),
            // A conditional guarding class-method defs (erb/compiler.rb's
            // `class << self; if defined?(Ractor); def register_scanner ...`):
            // map each branch the same way and KEEP the runtime `if`, so the
            // branch that executes at load time defines the class method.
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => Item::Cond(*cond, then_body.clone(), else_body.clone()),
            // A visibility directive (`public :a`) or a runtime call
            // (`public(*METHODS)`) inside `class << self` would run at load
            // in the enclosing MODULE's context, not the singleton's -- so
            // `public(*METHODS)` over names the singleton doesn't yet carry
            // would wrongly raise. It is a documented best-effort NO-OP
            // (fileutils' Verbose/NoWrite/DryRun are load-time convenience
            // wrappers; the singleton-visibility nuance is bundler-irrelevant).
            HirNode::MethodVisibility { .. } | HirNode::Call { .. } => Item::Skip,
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
            Item::Skip => {}
            Item::Reject => {
                return Err(
                    "unsupported statement in `class << self` (zeo limitation) -- only `def`s, constants, `include`, visibility directives, conditionals, and `attr_*`/`alias` are handled here; `extend`/`prepend`/ivars/a nested `class << self` aren't supported yet".to_string().into(),
                );
            }
        }
    }
    Ok(())
}

/// The Ruby version zeo targets -- kept in lockstep with `zeo_rt::bootstrap`'s
/// `VERSION` (the value of the runtime `RUBY_VERSION` constant). Mirrors the
/// loader's hardcoded `RUBY_ENGINE`: zeo compiles to one fixed target, so a
/// `RUBY_VERSION`-gated definition is statically decidable.
const TARGET_RUBY_VERSION: &str = "4.0.5";

/// Toplevel constants zeo's runtime ALWAYS defines, so `defined?(C)` is
/// statically true. Used to pick the live branch of a feature-probe like
/// erb/compiler.rb's `if defined?(Ractor)`. Extend as more `defined?`-gated
/// definitions surface in the require graph.
const ALWAYS_DEFINED_CONSTS: &[&str] = &["Ractor"];

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
        if let Some(call) = stmt.as_call_node() {
            if call.receiver().is_none() {
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
        ("Data", "define") | ("Struct", "new") | ("Class", "new")
    )
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
        out.push(hir.push(HirNode::DefMethod {
            name: new_name,
            params,
            body,
            is_class_method,
            visibility,
            is_def,
        }));
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

    let keyword_rest = match params.keyword_rest() {
        None => None,
        // Bare `...` forwarding (a `ForwardingParameterNode` in this slot)
        // -- desugared to `**__fwd_kw` here; `rest`/`block` above already
        // synthesized their `__fwd_*` halves.
        Some(n) if n.as_forwarding_parameter_node().is_some() => Some(Some("__fwd_kw".to_string())),
        Some(n) if n.as_no_keywords_parameter_node().is_some() => {
            // `**nil` -- explicit "no extra keywords accepted". Treated the
            // same as "no keyword_rest at all": real Ruby raises
            // `ArgumentError` for an unexpected kwarg only when `**nil` is
            // present, which needs exceptions to matter (zeo limitation).
            None
        }
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
    let body = lower_class_body(result, hir, body, None)?;
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
            Rewrite::Keep => id,
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

pub(crate) fn lower_class_body(
    result: &ParseResult,
    hir: &mut Hir,
    body: Option<Node<'_>>,
    superclass: Option<&str>,
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
    for stmt in &stmts {
        if is_ffi {
            if is_extend_ffi_library(stmt) {
                continue; // `extend FFI::Library` is the marker, no output
            }
            if lower_ffi_directive(result, hir, stmt, &mut ffi_lib, &mut ffi_aliases, &mut out)? {
                continue;
            }
        }
        if is_ffi_struct {
            if let Some(fields) = as_ffi_layout(stmt)? {
                // Replace `layout ...` in place with the synthesized accessors,
                // so any user methods after it can still override them.
                let source = synthesize_ffi_struct(&fields)?;
                out.extend(parse_and_lower_into(hir, &source)?);
                continue;
            }
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
    if let Some(if_node) = node.as_if_node() {
        if let Some(cond) = static_bool(&if_node.predicate()) {
            let chosen = if cond {
                if_node.statements().map(|s| s.as_node())
            } else {
                if_node.subsequent()
            };
            return lower_class_body_selected(
                result,
                hir,
                chosen,
                visibility,
                module_function,
                out,
            );
        }
    }
    if let Some(unless_node) = node.as_unless_node() {
        if let Some(cond) = static_bool(&unless_node.predicate()) {
            let chosen = if !cond {
                unless_node.statements().map(|s| s.as_node())
            } else {
                unless_node.else_clause().map(|e| e.as_node())
            };
            return lower_class_body_selected(
                result,
                hir,
                chosen,
                visibility,
                module_function,
                out,
            );
        }
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
    // `extend`/`prepend`/a nested `class << self` inside the singleton stay a
    // clean rejection: those act on the singleton's OWN singleton, which plain
    // enclosing-class retagging can't express (deferred).
    if let Some(singleton) = node.as_singleton_class_node() {
        if singleton.expression().as_self_node().is_none() {
            // `class << obj` on a NON-`self` receiver: each `def` in
            // the body is a per-object singleton method (see
            // `desugar_singleton_class_defs`).
            out.extend(desugar_singleton_class_defs(result, hir, &singleton)?);
            return Ok(());
        }
        let inner = lower_class_body(result, hir, singleton.body(), None)?;
        map_class_self_items(hir, &inner, out)?;
        return Ok(());
    }

    if let Some(call) = node.as_call_node() {
        if call.receiver().is_none() {
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
                        if let Some(&id) = out.iter().rev().find(|&&id| {
                            matches!(&hir[id], HirNode::DefMethod { name: existing, .. } if *existing == target)
                        }) {
                            hir.set_method_is_class_method(id);
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
                                    n.as_string_node().map(|s| {
                                        String::from_utf8_lossy(s.unescaped()).into_owned()
                                    })
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
            if matches!(name.as_str(), "include" | "extend" | "prepend") {
                if let Some(args) = call.arguments() {
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
                    {
                        if !names.is_empty() {
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
                            out.extend(ordered.into_iter().map(|n| {
                                hir.push(match name.as_str() {
                                    "include" => HirNode::Include(n),
                                    "extend" => HirNode::Extend(n),
                                    _ => HirNode::Prepend(n),
                                })
                            }));
                            return Ok(());
                        }
                    }
                }
            }
            if matches!(
                name.as_str(),
                "attr" | "attr_reader" | "attr_writer" | "attr_accessor"
            ) {
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if !arg_list.is_empty() && arg_list.iter().all(|n| n.as_symbol_node().is_some())
                    {
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
                                out.push(hir.push(HirNode::DefMethod {
                                    name: ivar.clone(),
                                    params: Params::default(),
                                    body: vec![read],
                                    is_class_method: false,
                                    visibility: *visibility,
                                    is_def: true,
                                }));
                            }
                            if matches!(name.as_str(), "attr_writer" | "attr_accessor") {
                                let param = "value".to_string();
                                let read_param = hir.push(HirNode::LocalRead(param.clone()));
                                let write = hir.push(HirNode::IvarWrite(ivar.clone(), read_param));
                                out.push(hir.push(HirNode::DefMethod {
                                    name: format!("{ivar}="),
                                    params: Params {
                                        required: vec![param],
                                        ..Params::default()
                                    },
                                    body: vec![write],
                                    is_class_method: false,
                                    visibility: *visibility,
                                    is_def: true,
                                }));
                            }
                        }
                        return Ok(());
                    }
                }
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
        if *module_function {
            if let HirNode::DefMethod {
                name, params, body, ..
            } = &hir[id]
            {
                let (name, params, body) = (name.clone(), params.clone(), body.clone());
                hir.set_method_visibility(id, Visibility::Private);
                out.push(id);
                out.push(hir.push(HirNode::DefMethod {
                    name,
                    params,
                    body,
                    is_class_method: true,
                    visibility: Visibility::Public,
                    is_def: true,
                }));
                return Ok(());
            }
        }
        out.push(id);
        return Ok(());
    }
    out.push(lower_node(result, hir, node)?);
    Ok(())
}
