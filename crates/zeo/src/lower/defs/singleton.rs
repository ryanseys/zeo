//! The `class << self` desugar: singleton-class bodies retargeted onto
//! the enclosing class (the mentions_self rule), singleton item mapping,
//! and the self-site rewrites.

use super::*;

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
    // It DOES end any `class << self` run: a `class << self` in here names
    // obj's singleton, not the enclosing class's surrogate.
    let inner =
        hir.end_singleton_body(|hir| lower_class_body(result, hir, singleton.body(), None, None))?;
    desugar_singleton_items(result, hir, &recv_node, inner)
}

/// `class << self` written inside a run-time `eval`: the surrogate `ClassDef`
/// the emitter runs as one more `class_eval` against `self.singleton_class`
/// (`clif::eval::eval_class_def`). `None` for a body with no statements to
/// slice, which falls back on the desugar.
///
/// A snippet has no compile-time class to home a constant or a nested class
/// on -- that is what the whole-program path's surrogate is -- so the desugar
/// hoisted both onto the enclosing module and `M.constants` reported names
/// ruby does not. Handing the body back to the compiler as SOURCE, with the
/// singleton as its cref, answers every such question the way the class-body
/// path already answers it for `class Foo` in a snippet.
///
/// The lowered statements are never emitted; they exist so `eval_body_source`
/// can slice the body's own text out of the snippet by their spans.
pub(crate) fn eval_singleton_body(
    result: &ruby_prism::ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    singleton: &ruby_prism::SingletonClassNode<'_>,
) -> PResult<Option<NodeId>> {
    let body = lower_class_body(result, hir, singleton.body(), None, None)?;
    if body.is_empty() {
        return Ok(None);
    }
    hir.push_span(crate::lower::span_of(hir, node));
    let def = hir.push(HirNode::ClassDef {
        name: SINGLETON_SURROGATE.to_string(),
        superclass: None,
        body,
        is_module: false,
    });
    hir.pop_span();
    Ok(Some(def))
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
    #[allow(clippy::large_enum_variant)] // built once per definition; never stored in bulk
    enum Item {
        Def(String, Params, Vec<NodeId>, crate::hir::Visibility),
        /// `def self.x` in a `class << obj` body -- see its emission arm.
        MetaDef(String, Params, Vec<NodeId>),
        Const,
        Nested(String, Option<String>, Vec<NodeId>, bool),
        Cond(NodeId, Vec<NodeId>, Vec<NodeId>),
        Alias(String, String),
        Undef(Vec<String>),
        SingletonSelf,
        SelfSend,
        Passthrough,
        Mixin(&'static str, String),
        Guarded(Vec<NodeId>, Vec<crate::hir::RescueClause>),
        /// Runs unchanged once every `self` it names becomes
        /// `recv.singleton_class`.
        RetargetSelf,
        /// A statement naming an ivar ANYWHERE under it -- see
        /// `retarget_ivars_to_singleton`.
        SingletonIvars,
        /// Runs as the body of a `recv.singleton_class.class_eval`, which is
        /// where ruby runs it -- see the `SingletonBody` arm.
        SingletonBody,
        /// `private :m` / `protected :m` naming a per-object singleton method.
        Vis(String, crate::hir::Visibility),
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let item = match &hir[id] {
            HirNode::DefMethod {
                name,
                params,
                body,
                is_class_method: false,
                visibility,
                ..
            } => Item::Def(name.clone(), (**params).clone(), body.clone(), *visibility),
            // `def self.x` here defines on the singleton's own singleton --
            // `obj.singleton_class.x`. The `class << self` form takes the same
            // route; see its `MetaMethod`.
            HirNode::DefMethod {
                name,
                params,
                body,
                is_class_method: true,
                ..
            } => Item::MetaDef(name.clone(), (**params).clone(), body.clone()),
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
            // `private :m` NAMING a method -- the singleton half of
            // `MethodVisibility`, and exactly `recv.singleton_class.send
            // (:private, :m)`. A bare `private` leaves no node at all: the
            // lowering's running default already stamped every `def` after it,
            // which is what the `Def` arm above carries.
            HirNode::MethodVisibility { name, visibility } => Item::Vis(name.clone(), *visibility),
            // `alias new old` inside a singleton (`class << IPSocket; alias
            // getaddress_orig getaddress; ...`, ipaddr) aliases a method on the
            // object's singleton class -- rebind it to
            // `recv.singleton_class.alias_method(:new, :old)`, the same runtime
            // form the runtime-class-body transform emits for an `alias`.
            HirNode::AliasMethod {
                new_name, old_name, ..
            } => Item::Alias(new_name.clone(), old_name.clone()),
            // `undef :close` here retires the name for THIS ONE OBJECT --
            // logging's `def kill; class << self; undef :close; end; end`, and
            // the 98 corpus rows behind it. The `alias` arm's twin, and the
            // same runtime primitive: `recv.singleton_class.undef_method(:close)`.
            //
            // Not the `Passthrough` catch-all below (an `undef` names no
            // `self`): that emits a definition-level node where a value
            // belongs, and codegen refuses the whole program. A per-object
            // tombstone records the retirement, and every lookup consults it.
            HirNode::Undef(names) => Item::Undef(names.clone()),
            // `include M` / `prepend M` inside `class << obj` mixes M into the
            // OBJECT's singleton class -- which is CRuby's own definition of
            // `obj.extend(M)` (`rb_include_module(rb_singleton_class(obj), M)`),
            // so the runtime `recv.singleton_class.include(M)` this becomes is
            // the primitive rather than a paraphrase of it. rdoc's
            // `class << self; prepend Git` inside a method body, spreadsheet's
            // `include Compatibility` and treetop's all take this route.
            HirNode::Include(m) => Item::Mixin("include", m.clone()),
            HirNode::Prepend(m) => Item::Mixin("prepend", m.clone()),
            // `extend M` here reaches one level further out -- the singleton's
            // OWN singleton -- exactly as the `class << self` path's
            // `Item::ExtendSingleton` does. tins spells its `thread_local`
            // macro this way.
            HirNode::Extend(m) => Item::Mixin("extend", m.clone()),
            // `class << obj; self; end` -- the idiom that RETURNS the object's
            // singleton class (`self` inside the singleton body IS that class,
            // e.g. bundler's `def gem_class; class << Gem; self; end; end`).
            HirNode::SelfRef => Item::SingletonSelf,
            // A receiver-less call (`class << self; undef_method(:options)`,
            // optparse) runs with the singleton class as `self` in real Ruby --
            // rebind it onto `recv.singleton_class` so it targets the object's
            // singleton, not the enclosing method's self.
            HirNode::Call { receiver: None, .. } => Item::SelfSend,
            // An explicit `self` receiver names the same singleton class the
            // arm above reaches implicitly, so it retargets identically.
            // `class << Base; self.prepend(m)` (activerecord-jdbc-adapter, and
            // ten adapter gems behind it), `self.ancestors` (hirb),
            // `self.prepend(ClassMethods)` (the active_hash family).
            HirNode::Call {
                receiver: Some(r), ..
            } if matches!(hir[*r], HirNode::SelfRef) => Item::SelfSend,
            // `remove_method :now rescue nil` (tins) -- the rescue modifier,
            // which lowers to a `Begin` with one bare clause. Guarding a
            // definition-level statement this way is ordinary in a singleton
            // body, so map the guarded statements and keep the guard. An
            // `else`/`ensure` is not part of the modifier form and would need
            // its own decision about where its statements run, so it stays
            // rejected rather than silently flattened.
            HirNode::Begin {
                body,
                rescues,
                else_body: None,
                ensure_body: None,
            } => Item::Guarded(body.clone(), rescues.clone()),
            // An `@x` here names an ivar of the OBJECT'S SINGLETON CLASS,
            // which is a different object from the object -- the `class <<
            // self` mapping's twin, and for the same reason.
            _ if names_an_ivar(hir, id) => Item::SingletonIvars,
            // Anything that never consults `self` means the same thing wherever
            // it runs, so it runs unchanged at this position -- the rule the
            // `Guarded` rescue bodies below already apply, and the one the
            // `class << self` mapping applies to its own leftovers.
            // `m = Module.new do ... end` is the case that matters: the
            // module's `def`s bind `self` at CALL time, not here, which is why
            // `mentions_self` stops at a definition boundary.
            _ if !mentions_self(hir, id) => Item::Passthrough,
            // It consults `self`, but only by NAMING it -- no receiverless send
            // anywhere under it, so nothing needs a receiver rebound. The
            // `self` just has to EVALUATE to the object's singleton class, and
            // `recv.singleton_class` is that object. google_drive's
            // `class << obj; return self; end` is 39 corpus rows of exactly
            // this, and it keeps them off the runtime path below.
            _ if !has_implicit_self_send(hir, id) => Item::RetargetSelf,
            // It sends to an implicit `self` somewhere zeo cannot rewrite a
            // receiver -- inside a block, whose body runs with the singleton as
            // `self`. Run the statement where ruby runs it: as the body of a
            // `recv.singleton_class.class_eval`, whose `self` IS that class.
            //
            // The `class << self` form gets a compile-time class for this (see
            // `map_class_self_items`'s `SingletonBody`); a PER-OBJECT singleton
            // has none, so the escape is the runtime one. `class_eval` is the
            // primitive rather than a paraphrase: it is what ruby's own
            // `Module#class_eval` does with the receiver as `self`.
            _ => Item::SingletonBody,
        };
        match item {
            Item::Def(mname, params, body, visibility) => {
                let recv = lower_node(result, hir, recv_node)?;
                let marked = mname.clone();
                out.push(define_singleton_method_call(hir, recv, mname, params, body));
                // The body's running default (`private` on its own line, or a
                // `private def`) belongs to the row this just installed. The
                // install itself has no visibility parameter, so the mark
                // follows it as the call ruby spells the same thing with.
                if let Some(mark) =
                    singleton_visibility_mark(result, hir, recv_node, &marked, visibility)?
                {
                    out.push(mark);
                }
            }
            Item::Vis(mname, visibility) => {
                if let Some(mark) =
                    singleton_visibility_mark(result, hir, recv_node, &mname, visibility)?
                {
                    out.push(mark);
                }
            }
            // `def self.x` inside `class << obj` -- one level further up
            // again, on `obj.singleton_class`'s own singleton.
            Item::MetaDef(mname, params, body) => {
                let obj = lower_node(result, hir, recv_node)?;
                let recv = hir.push(HirNode::Call {
                    receiver: Some(obj),
                    name: "singleton_class".to_string(),
                    args: vec![],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                });
                out.push(define_singleton_method_call(hir, recv, mname, params, body));
            }
            Item::Const | Item::Passthrough => out.push(id),
            Item::Nested(name, superclass, body, is_module) => {
                out.push(runtime_nested_class(
                    hir,
                    name,
                    superclass,
                    body,
                    is_module,
                    NestedTarget::Lexical,
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
            Item::Mixin(verb, module_name) => {
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
                let module_ref = hir.push(HirNode::ClassRef(module_name));
                out.push(hir.push(HirNode::Call {
                    receiver: Some(singleton),
                    name: verb.to_string(),
                    args: vec![ArrayElem::Single(module_ref)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
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
            Item::Undef(names) => {
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
                // `undef a, b` names them all in one statement, and
                // `Module#undef_method` takes them all in one call.
                let syms = names
                    .into_iter()
                    .map(|n| ArrayElem::Single(hir.push(HirNode::SymbolLit(n))))
                    .collect();
                out.push(hir.push(HirNode::Call {
                    receiver: Some(singleton),
                    name: "undef_method".to_string(),
                    args: syms,
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
                let singleton = hir.push(singleton_class_of(recv));
                // Ruby runs this call with no receiver, or with a literal
                // `self`; neither takes a visibility check. The surrogate is
                // zeo's, so it must not add one -- fileutils' `public(*METHODS)`.
                hir.mark_implicit_self_receiver(singleton);
                if let HirNode::Call { receiver, .. } = &mut hir[id] {
                    *receiver = Some(singleton);
                }
                out.push(id);
            }
            Item::RetargetSelf => {
                // In place, so every parent's `NodeId` still points at the
                // right node -- the `self` BECOMES the `singleton_class` call.
                // The receiver is re-lowered per site, the same rule (and the
                // same side-effecting-receiver caveat) as every other arm here.
                for site in evaluated_self_sites(hir, id) {
                    let recv = lower_node(result, hir, recv_node)?;
                    hir[site] = singleton_class_of(recv);
                }
                out.push(id);
            }
            Item::SingletonIvars => {
                let mut err = None;
                retarget_ivars_to_singleton(hir, id, |hir| {
                    match lower_node(result, hir, recv_node) {
                        Ok(n) => n,
                        Err(e) => {
                            let n = hir.push(HirNode::SelfRef);
                            err = Some(e);
                            n
                        }
                    }
                });
                if let Some(e) = err {
                    return Err(e);
                }
                out.push(id);
            }
            Item::SingletonBody => {
                let recv = lower_node(result, hir, recv_node)?;
                let singleton = hir.push(singleton_class_of(recv));
                let block = hir.push(HirNode::Block {
                    params: Box::default(),
                    body: vec![id],
                });
                // The block runs under the RECEIVER's `self`. Lowering marks
                // the ones the SOURCE writes (see `NodeFlag::REHOMED_BLOCK`); a
                // synthesized one has to say so itself.
                hir.set_flag(block, crate::hir::NodeFlag::REHOMED_BLOCK);
                out.push(hir.push(HirNode::Call {
                    receiver: Some(singleton),
                    name: "class_eval".to_string(),
                    args: vec![],
                    kwargs: vec![],
                    block: Some(block),
                    block_arg: None,
                    safe: false,
                }));
            }
            Item::Guarded(body, rescues) => {
                let body = desugar_singleton_items(result, hir, recv_node, body)?;
                // The handler is ordinarily a VALUE -- `rescue nil` is the
                // whole idiom -- and a value is not one of the items this
                // mapper knows. Only a handler that consults `self` needs
                // mapping, since that `self` is the singleton class; anything
                // self-free means the same wherever it runs and passes through.
                let rescues = rescues
                    .into_iter()
                    .map(|r| {
                        let needs_mapping = r.body.iter().any(|&s| mentions_self(hir, s));
                        let body = if needs_mapping {
                            desugar_singleton_items(result, hir, recv_node, r.body)?
                        } else {
                            r.body
                        };
                        Ok(crate::hir::RescueClause { body, ..r })
                    })
                    .collect::<PResult<Vec<_>>>()?;
                out.push(hir.push(HirNode::Begin {
                    body,
                    rescues,
                    else_body: None,
                    ensure_body: None,
                }));
            }
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
///
/// The walk STOPS at a definition boundary. A `def`'s body binds `self` to
/// whatever receives the call, not to the `self` in scope where the `def` was
/// written, so what it consults says nothing about where the enclosing
/// statement can run. `m = Module.new do def x; connection; end end` is the
/// shape: `connection` is the module's business, and the assignment itself is
/// self-free. A block that is NOT a method body does capture the enclosing
/// `self`, and is still walked.
fn mentions_self(hir: &Hir, id: NodeId) -> bool {
    let mut stack = vec![id];
    while let Some(n) = stack.pop() {
        match &hir[n] {
            HirNode::SelfRef | HirNode::Call { receiver: None, .. } => return true,
            HirNode::DefMethod { .. }
            | HirNode::Lambda {
                method_body: true, ..
            } => continue,
            _ => {}
        }
        hir[n].for_each_child(&mut |child| stack.push(child));
    }
    false
}

/// Whether anything under `id` sends to an IMPLICIT self -- a receiverless
/// call. Those are the mentions that need `self` REBOUND rather than merely
/// evaluated, which for a call nested inside a block zeo cannot do.
///
/// Stops where `mentions_self` stops, and for the same reason.
fn has_implicit_self_send(hir: &Hir, id: NodeId) -> bool {
    let mut stack = vec![id];
    while let Some(n) = stack.pop() {
        match &hir[n] {
            HirNode::Call { receiver: None, .. } => return true,
            HirNode::DefMethod { .. }
            | HirNode::Lambda {
                method_body: true, ..
            } => continue,
            _ => {}
        }
        hir[n].for_each_child(&mut |child| stack.push(child));
    }
    false
}

/// Rewrites every `self` these statements EVALUATE into `self.singleton_class`
/// -- the object `self` actually denotes inside `class << self`.
///
/// This is the whole fix for a statement that consults `self` only by naming
/// it: `Mongoid.deprecate(self, :from_hash)` needs the singleton class as an
/// ARGUMENT, not as a receiver, so there is nothing to retarget and no
/// compile-time singleton class required -- 99 corpus rows behind that one
/// line. `self.default_params = {}` is the same shape through an attribute
/// assignment, which lowers to a `Seq` around a temporary and so never
/// matched the plain `self`-receiver arm.
///
/// Stops at a definition boundary for the reason `mentions_self` does: a
/// `self` inside a `def` is that method's future RECEIVER, not the singleton
/// class (oracle: `class << self; def who = self; end` makes `Foo.who` answer
/// `Foo`, not `#<Class:Foo>`).
fn retarget_self_to_singleton(hir: &mut Hir, id: NodeId) {
    // In place, so every parent's `NodeId` still points at the right node: the
    // `self` BECOMES the `singleton_class` call, over a fresh receiver.
    for site in evaluated_self_sites(hir, id) {
        let me = hir.push(HirNode::SelfRef);
        hir[site] = singleton_class_of(me);
    }
}

/// Every `self` under `id` that is EVALUATED here -- the sites
/// [`retarget_self_to_singleton`] and its `class << obj` counterpart rewrite.
/// Stops at a definition boundary for the reason `mentions_self` does.
fn evaluated_self_sites(hir: &Hir, id: NodeId) -> Vec<NodeId> {
    let mut stack = vec![id];
    let mut sites = Vec::new();
    while let Some(n) = stack.pop() {
        match &hir[n] {
            HirNode::SelfRef => {
                sites.push(n);
                continue;
            }
            HirNode::DefMethod { .. }
            | HirNode::Lambda {
                method_body: true, ..
            } => continue,
            _ => {}
        }
        hir[n].for_each_child(&mut |child| stack.push(child));
    }
    sites
}

/// Whether `id` reads or writes an ivar at THIS level -- the statements
/// [`retarget_ivars_to_singleton`] has to rewrite.
fn names_an_ivar(hir: &Hir, id: NodeId) -> bool {
    !evaluated_ivar_sites(hir, id).is_empty()
}

/// Rewrites every ivar under `id` onto `recv.singleton_class`, in place.
///
/// An `@x` named in a `class << X` body belongs to X's SINGLETON CLASS, which
/// is a different object from X -- so the `attr_accessor` written beside it
/// does NOT read what the body wrote (oracle-verified). Every ivar form is
/// mapped, not only the two bare statement forms: otherwise `@echo = @seen`
/// reads the wrong object and `@n += 1` raises on nil.
///
/// `build_recv` mints a fresh receiver per site, the same rule (and the same
/// side-effecting-receiver caveat) as every other rebinding here.
fn retarget_ivars_to_singleton(
    hir: &mut Hir,
    id: NodeId,
    mut build_recv: impl FnMut(&mut Hir) -> NodeId,
) {
    for site in evaluated_ivar_sites(hir, id) {
        let recv = build_recv(hir);
        let singleton = hir.push(singleton_class_of(recv));
        // `IvarRead`/`IvarWrite` carry the BARE name; the reflection methods
        // want the sigil.
        let (verb, name, args) = match &hir[site] {
            HirNode::IvarRead(name) => ("instance_variable_get", name.clone(), None),
            HirNode::IvarWrite(name, value) => {
                ("instance_variable_set", name.clone(), Some(*value))
            }
            _ => unreachable!("evaluated_ivar_sites yields only ivar nodes"),
        };
        let sym = hir.push(HirNode::SymbolLit(format!("@{name}")));
        let mut call_args = vec![ArrayElem::Single(sym)];
        call_args.extend(args.map(ArrayElem::Single));
        hir[site] = HirNode::Call {
            receiver: Some(singleton),
            name: verb.to_string(),
            args: call_args,
            kwargs: vec![],
            block: None,
            block_arg: None,
            safe: false,
        };
    }
}

/// Every ivar node under `id` that is READ OR WRITTEN here. Stops at a
/// definition boundary for the reason [`evaluated_self_sites`] does: a `def`
/// body's ivars belong to whatever `self` it runs against.
fn evaluated_ivar_sites(hir: &Hir, id: NodeId) -> Vec<NodeId> {
    let mut stack = vec![id];
    let mut sites = Vec::new();
    while let Some(n) = stack.pop() {
        match &hir[n] {
            HirNode::IvarRead(_) | HirNode::IvarWrite(..) => sites.push(n),
            HirNode::DefMethod { .. }
            | HirNode::Lambda {
                method_body: true, ..
            } => continue,
            _ => {}
        }
        hir[n].for_each_child(&mut |child| stack.push(child));
    }
    sites
}

/// `<recv>.singleton_class`, the node every singleton rebinding is built from.
fn singleton_class_of(recv: NodeId) -> HirNode {
    HirNode::Call {
        receiver: Some(recv),
        name: "singleton_class".to_string(),
        args: vec![],
        kwargs: vec![],
        block: None,
        block_arg: None,
        safe: false,
    }
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
/// `recv.define_singleton_method(:name, ->(params) { body })` -- how a `def`
/// written for a specific OBJECT reaches that object's singleton class, which
/// zeo has no compile-time namespace for.
/// `recv.singleton_class.send(:private, :name)` -- how ruby spells a
/// visibility mark on ONE object's row. `None` for `Public`, which is what an
/// unmarked install already is.
///
/// `send` and not a bare call: `Module#private` is itself private, so the
/// singleton class refuses an explicit-receiver call to it.
fn singleton_visibility_mark(
    result: &ruby_prism::ParseResult,
    hir: &mut Hir,
    recv_node: &Node<'_>,
    name: &str,
    visibility: crate::hir::Visibility,
) -> PResult<Option<NodeId>> {
    let verb = match visibility {
        crate::hir::Visibility::Public => return Ok(None),
        crate::hir::Visibility::Private => "private",
        crate::hir::Visibility::Protected => "protected",
    };
    let recv = lower_node(result, hir, recv_node)?;
    let singleton = hir.push(singleton_class_of(recv));
    let verb_sym = hir.push(HirNode::SymbolLit(verb.to_string()));
    let name_sym = hir.push(HirNode::SymbolLit(name.to_string()));
    Ok(Some(hir.push(HirNode::Call {
        receiver: Some(singleton),
        name: "send".to_string(),
        args: vec![ArrayElem::Single(verb_sym), ArrayElem::Single(name_sym)],
        kwargs: vec![],
        block: None,
        block_arg: None,
        safe: false,
    })))
}

fn define_singleton_method_call(
    hir: &mut Hir,
    recv: NodeId,
    name: String,
    params: Params,
    body: Vec<NodeId>,
) -> NodeId {
    let lambda = hir.push(HirNode::Lambda {
        params: Box::new(params),
        body,
        method_body: true,
    });
    // This lambda IS a `def`'s body, so ruby labels its frame after the
    // method rather than as a block. See `Hir::singleton_def_names`.
    hir.singleton_def_names.insert(lambda, name.clone());
    let sym = hir.push(HirNode::SymbolLit(name));
    hir.push(HirNode::Call {
        receiver: Some(recv),
        name: "define_singleton_method".to_string(),
        args: vec![ArrayElem::Single(sym), ArrayElem::Single(lambda)],
        kwargs: vec![],
        block: None,
        block_arg: None,
        safe: false,
    })
}

fn own_singleton_class(hir: &mut Hir) -> NodeId {
    let me = hir.push(HirNode::SelfRef);
    let recv = hir.push(HirNode::Call {
        receiver: Some(me),
        name: "singleton_class".to_string(),
        args: vec![],
        kwargs: vec![],
        block: None,
        block_arg: None,
        safe: false,
    });
    // Ruby writes these calls with NO receiver at all -- this one exists only
    // because zeo rebinds the statement rather than re-homing `self`. Record
    // it so the visibility checks keep treating the call as the FCALL it is;
    // otherwise a `private` singleton method called by its own body's DSL
    // (lita's `define_deprecated_class_method`) raises NoMethodError.
    hir.mark_implicit_self_receiver(recv);
    recv
}

pub(super) fn map_class_self_items(
    hir: &mut Hir,
    ids: &[NodeId],
    out: &mut Vec<NodeId>,
) -> PResult<()> {
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
        /// A statement naming an ivar ANYWHERE under it -- see
        /// `retarget_ivars_to_singleton`.
        SingletonIvars,
        /// A `def self.x` written in the singleton body -- a method of the
        /// singleton's OWN singleton. See the `DefMethod` arm below.
        MetaMethod(String, Params, Vec<NodeId>),
        SingletonSelf,
        SelfSend,
        /// Runs unchanged once every `self` it names is rewritten to
        /// `self.singleton_class` -- see `retarget_self_to_singleton`.
        RetargetSelf,
        /// Runs as a statement of the SINGLETON's own class body, which is
        /// where ruby runs it -- see the `SingletonBody` arm.
        SingletonBody,
        Cond(NodeId, Vec<NodeId>, Vec<NodeId>),
        Guarded(
            Vec<NodeId>,
            Vec<crate::hir::RescueClause>,
            Option<Vec<NodeId>>,
            Option<Vec<NodeId>>,
        ),
    }
    for &id in ids {
        let item = match &hir[id] {
            // A `def self.x` HERE is one level further up: it defines a method
            // on `#<Class:#<Class:K>>`, reached as `K.singleton_class.x`.
            // Retagging it like a plain `def` put it one level too LOW, which
            // answered `K.x` -- where ruby raises NoMethodError -- and left
            // `K.singleton_class.x` undefined. The singleton class is an
            // ordinary object at run time, so the def is a per-object
            // singleton method ON it, exactly as `class << obj` spells one.
            HirNode::DefMethod {
                name,
                params,
                body,
                is_class_method: true,
                ..
            } => Item::MetaMethod(name.clone(), (**params).clone(), body.clone()),
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
            // Prism::Visitor`). It passes through unchanged and the caller
            // wraps it in the surrogate reopen, exactly as it wraps a
            // constant -- `homes_on_the_singleton` names the pair. The
            // singleton methods beside it still reach it by bare name (the
            // surrogate is their lexical home), and `M::ColorizeVisitor`
            // raises, which is what ruby answers.
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
            _ if names_an_ivar(hir, id) => Item::SingletonIvars,
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
            // Backports.warned = {}`), so it runs unchanged at this position --
            // PROVIDED nothing under it reaches for `self` either. An argument
            // or a nested receiver can (`Foo.bar(baz)`, treetop's
            // `included_modules - Object.included_modules`), and passing one of
            // those through unchanged would retarget it at the enclosing class
            // and answer silently wrong.
            HirNode::Call { .. } if !mentions_self(hir, id) => Item::Passthrough,
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
            // It consults `self`, but only by NAMING it -- there is no
            // receiverless send anywhere under it, so nothing needs a receiver
            // rebound and no compile-time singleton class is required. The
            // `self` just has to evaluate to the right object, and
            // `self.singleton_class` in the enclosing class body IS that
            // object. `Mongoid.deprecate(self, :from_hash)` is 99 corpus rows
            // of exactly this.
            _ if !has_implicit_self_send(hir, id) => Item::RetargetSelf,
            // It sends to an implicit `self` somewhere zeo cannot rewrite a
            // receiver -- inside a block, whose body runs with the singleton
            // as `self`. Run the statement where ruby runs it instead.
            _ => Item::SingletonBody,
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
            Item::MetaMethod(mname, params, body) => {
                let recv = own_singleton_class(hir);
                out.push(define_singleton_method_call(hir, recv, mname, params, body));
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
            Item::SingletonIvars => {
                retarget_ivars_to_singleton(hir, id, |hir| hir.push(HirNode::SelfRef));
                out.push(id);
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
            Item::RetargetSelf => {
                retarget_self_to_singleton(hir, id);
                out.push(id);
            }
            // The statement becomes the body of a REOPEN of the singleton's
            // own class, spliced in AT ITS POSITION.
            //
            // This is CRuby's structure rather than a workaround for it:
            // `NODE_SCLASS` compiles to `NEW_CHILD_ISEQ(..., ISEQ_TYPE_CLASS)`,
            // a child iseq whose `self` is the singleton class object, so a
            // block created inside sees that `self` for free. A zeo class body
            // already runs with `self` bound to its own class, and
            // `zeo_rt::register_singleton_surrogate` already makes this
            // particular class BE `Foo.singleton_class` at run time -- so
            // `define_method` inside it installs an instance method of the
            // singleton, i.e. a class method of `Foo`, which is exactly what
            // ruby does.
            //
            // In place, one reopen per statement, rather than collected into
            // one body at the front: a singleton body's statements run in
            // SOURCE order, and `singleton_method_added` fires for a
            // `define_method`'d name exactly as for a `def` -- so the
            // interleaving is observable, not cosmetic. Oracle-pinned in
            // `test/gaps/a_singleton_body_statement_that_defines_methods_at_runtime.rb`.
            Item::SingletonBody => {
                // Carry the statement's own span onto the reopen: a class body
                // takes its backtrace frame from its definition node's
                // location, so a span-less one is emitted with NO frame at all
                // and the singleton's frame goes missing from every backtrace
                // raised inside it.
                let span = hir.span(id).unwrap_or(crate::hir::Span::SYNTH);
                hir.push_span(span);
                let def = hir.push(HirNode::ClassDef {
                    name: SINGLETON_SURROGATE.to_string(),
                    superclass: None,
                    body: vec![id],
                    is_module: false,
                });
                hir.pop_span();
                out.push(def);
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
        }
    }
    Ok(())
}

/// Marks every `DefMethod` in a constant-bearing `class << self` body
/// (recursing into kept conditional branches) -- see
/// `NodeFlag::SINGLETON_BODY_DEF`.
pub(super) fn tag_singleton_body_defs(hir: &mut Hir, ids: &[NodeId]) {
    for &id in ids {
        match &hir[id] {
            HirNode::DefMethod { .. } => {
                hir.set_flag(id, crate::hir::NodeFlag::SINGLETON_BODY_DEF);
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
pub(super) const TARGET_RUBY_VERSION: &str = "4.0.6";

/// Toplevel constants zeo's runtime ALWAYS defines, so `defined?(C)` is
/// statically true. Used to pick the live branch of a feature-probe like
/// erb/compiler.rb's `if defined?(Ractor)`. Extend as more `defined?`-gated
/// definitions surface in the require graph.
pub(super) const ALWAYS_DEFINED_CONSTS: &[&str] = &["Ractor"];
