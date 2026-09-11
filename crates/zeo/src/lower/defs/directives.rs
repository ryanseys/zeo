//! The receiverless class-body DIRECTIVES `lower_class_body_statement`
//! dispatches by name: `private`/`public`/`protected`,
//! `private_class_method`/`public_class_method`, `private_constant`/
//! `public_constant`, `module_function`, `alias_method`,
//! `include`/`extend`/`prepend`, `refine`, `using`, and the `attr` family.
//! A handler answers `Ok(true)` when it consumed the statement and
//! `Ok(false)` for a shape it does not take (a dynamic/non-literal
//! argument), which falls through to the generic statement lowering.

use super::{
    lower_class_body, lower_class_body_statement, lower_using, promote_to_module_function,
    push_alias, refine_target, refinement_holder_name,
};
use crate::hir::{Hir, HirNode, NodeId, Params, Visibility};
use crate::lower::consts::constant_path_name;
use crate::lower::{PResult, lower_node};
use ruby_prism::{CallNode, Node, ParseResult};

pub(super) fn visibility(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    call: &CallNode<'_>,
    name: &str,
    visibility: &mut Visibility,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    if matches!(name, "private" | "public" | "protected") {
        let new_vis = match name {
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
            // In a `class << self` body the cursor must ALSO reach a
            // runtime `define_method` after it -- keep the directive as
            // a node, which the singleton mapping rebinds onto
            // `self.singleton_class` (a runtime send that stores the
            // surrogate's body default; the mapping appends a reset at
            // body end). Compiled `def`s beside it keep the
            // compile-time stamp above either way.
            if hir.is_in_singleton_body() {
                out.push(lower_node(result, hir, node)?);
            }
            return Ok(true);
        }
        if arg_list.len() == 1 && arg_list[0].as_def_node().is_some() {
            let id = lower_node(result, hir, &arg_list[0])?;
            hir.set_method_visibility(id, new_vis);
            out.push(id);
            return Ok(true);
        }
        if arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
            for n in &arg_list {
                let target =
                    String::from_utf8_lossy(n.as_symbol_node().expect("checked above").unescaped())
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
            return Ok(true);
        }
        // Falls through to the generic `Call` lowering below --
        // a dynamic/computed argument (e.g. `private(*names)`).
    }
    Ok(false)
}

pub(super) fn class_method_visibility(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    visibility: &mut Visibility,
    module_function: &mut bool,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    // `private_class_method :a, :b` / `private_class_method def self.x`
    // and their `public_` counterpart -- the class-method half of
    // `private`/`public` above, and lowered the same two ways: the
    // wrapped `def` is lowered on its own terms (so the running
    // visibility default and `module_function` mode reach it exactly as
    // a bare `def` does) and then retagged, while a name with no local
    // `def` becomes a deferred override. Unlike `private`, there is no
    // argument-less mode: Ruby has no running class-method default.
    if matches!(name, "private_class_method" | "public_class_method") {
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
            return Ok(true);
        }
        if !arg_list.is_empty() && arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
            for n in &arg_list {
                let target =
                    String::from_utf8_lossy(n.as_symbol_node().expect("checked above").unescaped())
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
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn constant_visibility(
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
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
                    String::from_utf8_lossy(n.as_symbol_node().expect("checked above").unescaped())
                        .into_owned()
                })
                .collect();
            out.push(hir.push(HirNode::ConstantVisibility {
                names,
                private: name == "private_constant",
            }));
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn module_function(
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    module_function: &mut bool,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
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
            return Ok(true);
        }
        if arg_list.iter().all(|n| n.as_symbol_node().is_some()) {
            for n in &arg_list {
                let target =
                    String::from_utf8_lossy(n.as_symbol_node().expect("checked above").unescaped())
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
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn alias_method(
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
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
                push_alias(hir, out, names[0].clone(), names[1].clone(), true);
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn mixin(
    hir: &mut Hir,
    node: &Node<'_>,
    call: &CallNode<'_>,
    name: &str,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    // `include Mod`/`extend Mod`/`prepend Mod` -- one or more bare
    // constant arguments, applied left-to-right (see `HirNode::
    // Include`'s docs for the multi-arg ordering rule). Anything
    // else (a non-constant argument, e.g. a computed module
    // expression) falls through to an ordinary `Call`, a clean
    // rejection at codegen time (zeo limitation: only a literal module
    // name is resolvable to a `ClassId` at compile time anyway).
    if matches!(name, "include" | "extend" | "prepend")
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
                hir.push(match name {
                    "include" => HirNode::Include(n),
                    "extend" => HirNode::Extend(n),
                    _ => HirNode::Prepend(n),
                })
            }));
            hir.pop_span();
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn refine(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    call: &CallNode<'_>,
    name: &str,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    // `refine Target do ... end` -- the block's `def`s become an
    // ordinary class body on a HOLDER module named for the target,
    // and a `Refine` marker records which class they refine. The
    // holder is a real `ClassDef` so every existing mechanism
    // (method registration, materialization, the module bridge that
    // emits a module's own methods as `RubyValue`-self functions)
    // carries it with no new machinery. Its name is unwritable as a
    // constant, so it claims no name inside the enclosing module.
    // A SNIPPET's `refine` is the run-time one: `Module#refine` mints
    // the holder, marks it and runs the block with the holder as self
    // and definee. The compile-time desugar below cannot serve one --
    // it registers a holder class the snippet's own compiler mints and
    // the running program has never heard of.
    if name == "refine"
        && !hir.mode.is_eval()
        && let (Some(args), Some(block)) = (call.arguments(), call.block())
    {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if let (1, Some(block)) = (arg_list.len(), block.as_block_node()) {
            // A refinement is resolved and installed at COMPILE time, so
            // its target has to be one zeo can name: a constant, a
            // constant's `.singleton_class` (aixm refines
            // `Range.singleton_class` -- class methods), or a constant's
            // `.class` (acpc_table_manager's `refine Time.class()`, which
            // IS `refine Class`). A genuinely computed one is a real
            // limitation rather than a syntax question, and saying so
            // beats "expected a constant name or path", which reads as
            // though the argument were misspelled.
            let (target, singleton) = refine_target(&arg_list[0]).ok_or(
                "`refine` needs a class or module zeo can name at compile time (a constant, \
                     or a constant's `.singleton_class`), not a computed one -- refinements are \
                     resolved and installed at compile time (zeo limitation)",
            )?;
            let holder = match singleton {
                false => refinement_holder_name(&target),
                // CRuby prints `#<refinement:#<Class:Range>>` for the
                // singleton form; the dot-joined spelling keeps the holder
                // apart from the same class's instance refinement.
                true => refinement_holder_name(&format!("#<Class:{target}>")),
            };
            let body = lower_class_body(result, hir, block.body(), None, Some(&holder))?;
            hir.push_span(crate::lower::span_of(hir, node));
            out.push(hir.push(HirNode::ClassDef {
                name: holder.clone(),
                superclass: None,
                body,
                is_module: true,
            }));
            out.push(hir.push(HirNode::Refine {
                target,
                holder,
                singleton,
            }));
            hir.pop_span();
            return Ok(true);
        }
    }
    Ok(false)
}

/// `using M` in a class/module body -- delegates to `lower_using`, which
/// wants the whole statement node.
pub(super) fn using(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    name: &str,
    call: &CallNode<'_>,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    if let Some(nodes) = lower_using(result, hir, node, name, call)? {
        out.extend(nodes);
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn attr(
    hir: &mut Hir,
    node: &Node<'_>,
    call: &CallNode<'_>,
    name: &str,
    visibility: Visibility,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    if matches!(
        name,
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
            // What the fold replaced, so analyze can put the call back when
            // the class `extend`ed a module that writes its own `attr_*`.
            let first = out.len();
            let macro_args: Vec<String> = arg_list
                .iter()
                .map(|n| {
                    String::from_utf8_lossy(n.as_symbol_node().expect("checked above").unescaped())
                        .into_owned()
                })
                .collect();
            for n in &arg_list {
                let ivar =
                    String::from_utf8_lossy(n.as_symbol_node().expect("checked above").unescaped())
                        .into_owned();
                // `attr :x` == `attr_reader :x` (the symbol form):
                // getter for everything but attr_writer, setter only
                // for attr_writer/attr_accessor.
                if name != "attr_writer" {
                    let read = hir.push(HirNode::IvarRead(ivar.clone()));
                    let getter = hir.push(HirNode::DefMethod {
                        name: ivar.clone(),
                        params: Box::default(),
                        body: vec![read],
                        is_class_method: false,
                        visibility,
                        is_def: true,
                    });
                    hir.set_flag(getter, crate::hir::NodeFlag::ATTR_GENERATED);
                    out.push(getter);
                }
                if matches!(name, "attr_writer" | "attr_accessor") {
                    // `__`-prefixed, so `param_entries` reports the slot
                    // ANONYMOUSLY -- ruby answers `[[:req]]` for an
                    // `attr_accessor` writer, naming nothing.
                    let param = "__value".to_string();
                    let read_param = hir.push(HirNode::LocalRead(param.clone()));
                    let write = hir.push(HirNode::IvarWrite(ivar.clone(), read_param));
                    let setter = hir.push(HirNode::DefMethod {
                        name: format!("{ivar}="),
                        params: Box::new(Params {
                            required: vec![param],
                            ..Params::default()
                        }),
                        body: vec![write],
                        is_class_method: false,
                        visibility,
                        is_def: true,
                    });
                    hir.set_flag(setter, crate::hir::NodeFlag::ATTR_GENERATED);
                    out.push(setter);
                }
            }
            hir.pop_span();
            for (i, &id) in out[first..].iter().enumerate() {
                let entry = (i == 0).then(|| (name.to_string(), macro_args.clone()));
                hir.attr_macro.insert(id, entry);
            }
            return Ok(true);
        }
    }
    Ok(false)
}
