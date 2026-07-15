//! The minimal analyze pass: walks the top-level `Hir::Program` statements
//! once (no fixpoint loop -- see the plan's stated scope-cut) and registers
//! every `ClassDef`/`DefMethod` into a `Compiler`, mirroring spinel's
//! `walk_scope`/`register_locals`/`resolve_parents` (a tiny slice of them).
//! Structured as a single pass function rather than spinel's 128-iteration
//! fixpoint loop because none of the spike's 7 examples need mutual
//! recursion between inference results -- but the shape (one function that
//! walks the whole program and mutates a `Compiler`) is exactly what a real
//! fixpoint would wrap in `for iter in 0..128 { ... }` later.

mod locals;
mod mro;

use crate::compiler::{ClassId, Compiler, Scope, OBJECT_CLASS};
use crate::hir::{ArrayElem, Hir, HirNode, NodeId, Params, Pattern, PatternArm, StrPart, Visibility};
use crate::types::TyKind;
use std::collections::HashMap;

pub struct Analyzed {
    pub compiler: Compiler,
    /// Top-level statements that aren't class definitions -- the body of
    /// generated `fn main()`.
    pub main_statements: Vec<NodeId>,
    /// The same per-local `TyKind` tracking `Scope::local_types` does for a
    /// method body, but for `main_statements` -- there's no `Scope` for the
    /// top level to hang this off of.
    pub main_local_types: HashMap<String, TyKind>,
}

pub fn analyze(hir: Hir, root: NodeId) -> Result<Analyzed, String> {
    let mut compiler = Compiler::new(hir);
    let HirNode::Program(statements) = &compiler.hir[root] else {
        return Err("expected a Program root".to_string());
    };
    let statements = statements.clone();
    let prelude_len = compiler.hir.prelude_len;

    let mut main_statements = Vec::new();
    for (idx, stmt) in statements.into_iter().enumerate() {
        if let HirNode::ClassDef {
            name,
            superclass,
            body,
            is_module,
        } = &compiler.hir[stmt]
        {
            let name = name.clone();
            let superclass = superclass.clone();
            let body = body.clone();
            let is_module = *is_module;
            let before = compiler.classes.len();
            register_class(&mut compiler, name, superclass, is_module, &body, &[])?;
            // Exception-prelude classes are BOOTSTRAP: the "defined before
            // any user program runs" set every `Ruby::Box` sees (see
            // `Compiler::resolve_class`'s fallback and `Hir::prelude_len`).
            if idx < prelude_len {
                for c in &mut compiler.classes[before..] {
                    c.is_bootstrap = true;
                }
            }
        } else {
            main_statements.push(stmt);
        }
    }

    // Ancestor linearization + method/class-method materialization + class
    // variable ownership -- must run AFTER every `ClassDef` above has been
    // registered, since `include`/`extend`/`prepend`/`< Super` targets must
    // already exist (same "defined earlier in the file" rule `superclass`
    // resolution already enforces). See `mro`'s module docs.
    mro::materialize(&mut compiler, &main_statements)?;

    let main_local_types = locals::infer_locals(&compiler, None, &main_statements);

    Ok(Analyzed {
        compiler,
        main_statements,
        main_local_types,
    })
}

/// Registers one `class`/`module` definition (or REOPENING -- Phase 15.2)
/// into the `Compiler`, recursively descending nested `ClassDef`s (Phase
/// 15.3). `cref` is the ENCLOSING lexical chain (outermost first,
/// `Compiler::cref_of`'s order) -- empty at the top level -- used to
/// resolve the qualified-form prefix, the superclass, and
/// include/extend/prepend targets, exactly as real Ruby resolves each of
/// those in the scope ENCLOSING the definition.
///
/// `name` may be a qualified path (`"Store::Item"` -- the `class
/// Store::Item ... end` form): the prefix must already resolve (real
/// Ruby's own NameError posture), the leaf registers under it as
/// namespace parent, and the class is marked `qualified_def` so its cref
/// is just itself (see `ClassInfo::qualified_def`'s docs). A leading `::`
/// anchors the definition at the top level from any nesting depth.
fn register_class(
    compiler: &mut Compiler,
    name: String,
    superclass: Option<String>,
    is_module: bool,
    body: &[NodeId],
    cref: &[ClassId],
) -> Result<(), String> {
    let (lexical_parent, leaf, qualified_def) = match name.strip_prefix("::") {
        Some(rest) if !rest.contains("::") => (None, rest.to_string(), true),
        _ => match name.rsplit_once("::") {
            Some((prefix, leaf)) => {
                let parent = compiler.resolve_class(prefix, cref, 0).ok_or_else(|| {
                    format!(
                        "unknown class/module `{prefix}` in `{name}` (must be defined earlier in the file)"
                    )
                })?;
                (Some(parent), leaf.to_string(), true)
            }
            None => (cref.last().copied(), name.clone(), false),
        },
    };
    // A built-in placeholder (`Integer`/`Array`/etc. -- see
    // `spinel_abi::BUILTINS`) has no generated Rust struct to reopen or
    // add methods onto; reopening one is Phase 16.3's job. Reject cleanly
    // here rather than let this surface later as a confusing
    // generated-`rustc`-compile failure or silently-wrong dispatch. Only a
    // TOP-LEVEL name can collide: `module Store; class String; end; end`
    // defines a fresh, unrelated `Store::String` (real Ruby's rule), which
    // then lexically shadows the builtin inside `Store` -- also real
    // Ruby's rule, falling out of `resolve_class`'s scope walk.
    let existing = compiler.class_in_scope(lexical_parent, &leaf, 0);
    if let Some(cid) = existing {
        if compiler.class(cid).is_builtin {
            return Err(format!(
                "reopening/redefining the built-in class `{name}` isn't supported yet (spike scope)"
            ));
        }
    }
    let class_id = match existing {
        // REOPENING (Phase 15.2, replacing the old silent-no-op duplicate
        // registration): a second `class Foo`/`module Foo` MERGES into the
        // existing `ClassInfo` -- the body loop below appends
        // includes/body-statements and registers methods with real Ruby's
        // last-`def`-wins rule (see the method arm). Guards mirror CRuby's
        // own (all oracle-verified): the definition KIND must match
        // (`TypeError: Foo is not a module`), and a superclass clause, if
        // written at all, must resolve to the original parent
        // (`TypeError: superclass mismatch for class Foo`).
        Some(cid) => {
            if compiler.class(cid).is_module != is_module {
                return Err(format!(
                    "{name} is not a {}",
                    if is_module { "module" } else { "class" }
                ));
            }
            if let Some(s) = &superclass {
                let want = compiler.resolve_class(s, cref, 0).ok_or_else(|| {
                    format!("unknown superclass `{s}` (must be defined earlier in the file)")
                })?;
                if compiler.class(cid).parent != Some(want) {
                    return Err(format!("superclass mismatch for class {name}"));
                }
            }
            cid
        }
        None => {
            let parent = if is_module {
                None
            } else {
                Some(match &superclass {
                    None => OBJECT_CLASS,
                    // Resolved in the ENCLOSING scope (`cref`, not the
                    // class being opened): real Ruby evaluates the
                    // superclass expression before the new class exists.
                    Some(s) => {
                        let cid = compiler.resolve_class(s, cref, 0).ok_or_else(|| {
                            format!("unknown superclass `{s}` (must be defined earlier in the file)")
                        })?;
                        if compiler.class(cid).is_builtin {
                            return Err(format!(
                                "subclassing the built-in type `{s}` isn't supported yet (spike scope, no generated Rust struct exists for it)"
                            ));
                        }
                        cid
                    }
                })
            };
            let cid = compiler.add_class(leaf, parent, is_module);
            compiler.classes[cid.0 as usize].lexical_parent = lexical_parent;
            compiler.classes[cid.0 as usize].qualified_def = qualified_def;
            cid
        }
    };
    // The chain this class's OWN body resolves names against -- what nested
    // definitions and include/extend/prepend targets see. Derived from the
    // registered class (not `cref` + push) so a qualified-def class
    // correctly contributes a cut chain.
    let child_cref = compiler.cref_of(Some(class_id));

    for &stmt in body {
        match &compiler.hir[stmt] {
            HirNode::DefMethod {
                name,
                params,
                body,
                is_class_method,
                visibility,
            } => {
                let (name, params, body, is_class_method, visibility) = (
                    name.clone(),
                    params.clone(),
                    body.clone(),
                    *is_class_method,
                    *visibility,
                );
                let sid = register_method(compiler, class_id, class_id, name, params, body, visibility)?;
                add_own_method(compiler, class_id, sid, is_class_method);
            }
            // A nested `class`/`module` definition (Phase 15.3) --
            // registered recursively under this class's own cref; the
            // `ClassDef` node itself never lands in `class_body_stmts`
            // (nested classes are ordinary `ClassInfo`s emitted from the
            // global class list, not statements to re-execute).
            HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } => {
                let (name, superclass, body, is_module) =
                    (name.clone(), superclass.clone(), body.clone(), *is_module);
                register_class(compiler, name, superclass, is_module, &body, &child_cref)?;
            }
            HirNode::Include(m) => {
                let target = resolve_module_target(compiler, m, &child_cref)?;
                compiler.classes[class_id.0 as usize].includes.push(target);
            }
            HirNode::Extend(m) => {
                let target = resolve_module_target(compiler, m, &child_cref)?;
                compiler.classes[class_id.0 as usize].extends.push(target);
            }
            HirNode::Prepend(m) => {
                let target = resolve_module_target(compiler, m, &child_cref)?;
                compiler.classes[class_id.0 as usize].prepends.push(target);
            }
            HirNode::ClassVarWrite(..) | HirNode::ConstWrite { .. } => {
                compiler.classes[class_id.0 as usize]
                    .class_body_stmts
                    .push(stmt);
            }
            // The native-package DSL (Phase 14.3) -- consumed here into
            // `ClassInfo` bookkeeping, never emitted as code.
            HirNode::NativeCrate(crate_path) => {
                compiler.classes[class_id.0 as usize].native_crate = Some(crate_path.clone());
            }
            HirNode::NativeFunc { name, arity } => {
                compiler.classes[class_id.0 as usize]
                    .native_methods
                    .push((name.clone(), *arity));
            }
            _ => {}
        }
    }
    {
        let ci = &compiler.classes[class_id.0 as usize];
        if !ci.native_methods.is_empty() && ci.native_crate.is_none() {
            return Err(format!(
                "module `{}` declares `native_func`s but no `native_crate` naming the backing Rust crate",
                ci.name
            ));
        }
    }
    Ok(())
}

/// Files one registered method `Scope` under its class's own-method list --
/// REPLACING any earlier same-name entry rather than appending a shadowed
/// duplicate: real Ruby's last-`def`-wins rule (oracle-verified), which
/// applies identically to a redefinition within one class body and to one
/// arriving via reopening (`method_in_chain` resolves the FIRST name match,
/// so append-only registration would silently keep dispatching the OLD
/// body). Instance and class methods are separate namespaces, hence the
/// separate lists.
fn add_own_method(compiler: &mut Compiler, class_id: ClassId, sid: crate::compiler::ScopeId, is_class_method: bool) {
    let mname = compiler.scope(sid).name.clone();
    let ci = &compiler.classes[class_id.0 as usize];
    let list = if is_class_method { &ci.own_class_methods } else { &ci.own_methods };
    let replaced = list
        .iter()
        .position(|&s| compiler.scope(s).name == mname);
    let ci = &mut compiler.classes[class_id.0 as usize];
    let list = if is_class_method { &mut ci.own_class_methods } else { &mut ci.own_methods };
    match replaced {
        Some(i) => list[i] = sid,
        None => list.push(sid),
    }
}

fn resolve_module_target(compiler: &Compiler, name: &str, cref: &[ClassId]) -> Result<ClassId, String> {
    compiler
        .resolve_class(name, cref, 0)
        .ok_or_else(|| format!("unknown module `{name}` (must be defined earlier in the file)"))
}

/// Registers one method BODY (a class/module's own literal `def`, or a
/// winning ancestor's body being materialized onto a descendant -- see
/// `mro::materialize_methods`/`materialize_class_methods`) as a fresh
/// `Scope`, running the full per-method analysis pipeline (local-type
/// inference, named-`*rest`/`**kwrest`/`&block`-param type seeding,
/// bare-`yield`/`block_given?` scanning) that used to live directly inline
/// in `register_class` before materialization needed to reuse it too.
/// `owner` is whichever class/module this Scope is filed under (and, for a
/// materialized method, whose concrete struct it'll be generated into);
/// `defining_class` is whichever class/module's HIR body `params`/`body`
/// actually came from -- equal to `owner` for an ordinary own-body method,
/// an ancestor otherwise (see `compiler::Scope::defining_class`'s docs).
fn register_method(
    compiler: &mut Compiler,
    owner: ClassId,
    defining_class: ClassId,
    name: String,
    params: Params,
    body: Vec<NodeId>,
    visibility: Visibility,
) -> Result<crate::compiler::ScopeId, String> {
    let mut local_types = locals::infer_locals(compiler, Some(defining_class), &body);
    for id in params.default_ids() {
        locals::track_extra(compiler, Some(defining_class), &mut local_types, id);
    }
    if let Some(Some(n)) = &params.rest {
        local_types.entry(n.clone()).or_insert(TyKind::Array);
    }
    if let Some(Some(n)) = &params.keyword_rest {
        local_types.entry(n.clone()).or_insert(TyKind::Hash);
    }
    if let Some(Some(n)) = &params.block {
        local_types.entry(n.clone()).or_insert(TyKind::Proc);
    }
    let mut uses_bare_block = false;
    for &n in &body {
        if scan_bare_block_use(&compiler.hir, n)? {
            uses_bare_block = true;
        }
    }
    Ok(compiler.push_scope(Scope {
        name,
        class: Some(owner),
        defining_class,
        params,
        body,
        local_types,
        uses_bare_block,
        visibility,
    }))
}

/// Scans a method's own control flow (NOT descending into a nested `Block`'s
/// body -- see below) for a bare `yield`/`block_given?`, returning whether
/// any was found. Mirrors `collect_ivars`'s traversal shape.
///
/// `yield`/`block_given?` lexically inside a block literal that's itself
/// passed to another call refers to a DIFFERENT enclosing method in real
/// Ruby (the block's own, not this one) -- a genuinely harder case (would
/// need per-block-site tracking of which method's implicit block it binds
/// to) that this spike doesn't attempt. Rather than silently miscompiling
/// that shape, `body_contains_yield_or_block_given` checks for it inside
/// every nested `Block` and rejects cleanly if found.
fn scan_bare_block_use(hir: &Hir, id: NodeId) -> Result<bool, String> {
    Ok(match &hir[id] {
        HirNode::Yield(_) | HirNode::BlockGiven => true,
        HirNode::IvarWrite(_, value)
        | HirNode::LocalWrite(_, value)
        | HirNode::ClassVarWrite(_, value) => scan_bare_block_use(hir, *value)?,
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            scan_bare_block_use(hir, *l)? || scan_bare_block_use(hir, *r)?
        }
        HirNode::Defined(v) => scan_bare_block_use(hir, *v)?,
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            scan_bare_block_use(hir, *cond)?
                || scan_bare_block_use_body(hir, then_body)?
                || scan_bare_block_use_body(hir, else_body)?
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            let mut found = false;
            if let Some(s) = subject {
                found |= scan_bare_block_use(hir, *s)?;
            }
            for (values, body) in arms {
                for &v in values {
                    found |= scan_bare_block_use(hir, v)?;
                }
                found |= scan_bare_block_use_body(hir, body)?;
            }
            found || scan_bare_block_use_body(hir, else_body)?
        }
        HirNode::While { cond, body, .. } => {
            scan_bare_block_use(hir, *cond)? || scan_bare_block_use_body(hir, body)?
        }
        HirNode::Loop { body } => scan_bare_block_use_body(hir, body)?,
        HirNode::For { target, iterable, body } => {
            let mut found = scan_bare_block_use(hir, *iterable)?;
            let mut ids = Vec::new();
            target.for_each_node(&mut |n| ids.push(n));
            for n in ids {
                found |= scan_bare_block_use(hir, n)?;
            }
            found || scan_bare_block_use_body(hir, body)?
        }
        HirNode::Call {
            receiver,
            args,
            kwargs,
            kwargs_splat,
            block,
            block_arg,
            ..
        } => {
            let mut found = false;
            if let Some(r) = receiver {
                found |= scan_bare_block_use(hir, *r)?;
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                found |= scan_bare_block_use(hir, *n)?;
            }
            for pair in kwargs {
                found |= scan_bare_block_use(hir, pair.0)?;
                found |= scan_bare_block_use(hir, pair.1)?;
            }
            if let Some(s) = kwargs_splat {
                found |= scan_bare_block_use(hir, *s)?;
            }
            if let Some(b) = block_arg {
                found |= scan_bare_block_use(hir, *b)?;
            }
            // NOT recursed into for `uses_bare_block` purposes -- see this
            // function's docs -- but checked for the rejection case.
            if let Some(b) = block {
                if let HirNode::Block { body, .. } = &hir[*b] {
                    if body_contains_yield_or_block_given(hir, body) {
                        return Err("`yield`/`block_given?` inside a nested block literal isn't supported yet (spike scope) -- it refers to a different enclosing method's block in real Ruby".to_string());
                    }
                }
            }
            found
        }
        HirNode::New { args, .. } | HirNode::SuperCall { args, .. } | HirNode::Raise(args) => {
            let mut found = false;
            for &a in args {
                found |= scan_bare_block_use(hir, a)?;
            }
            found
        }
        HirNode::ArrayLit(elems) => {
            let mut found = false;
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                found |= scan_bare_block_use(hir, *n)?;
            }
            found
        }
        HirNode::HashLit(pairs) => {
            let mut found = false;
            for pair in pairs {
                found |= scan_bare_block_use(hir, pair.0)?;
                found |= scan_bare_block_use(hir, pair.1)?;
            }
            found
        }
        HirNode::RangeLit { start, end, .. } => {
            let mut found = false;
            if let Some(s) = start {
                found |= scan_bare_block_use(hir, *s)?;
            }
            if let Some(e) = end {
                found |= scan_bare_block_use(hir, *e)?;
            }
            found
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            let mut found = false;
            for p in parts {
                if let StrPart::Interp(n) = p {
                    found |= scan_bare_block_use(hir, *n)?;
                }
            }
            found
        }
        HirNode::MultiWrite { targets, value } => {
            let mut found = scan_bare_block_use(hir, *value)?;
            let mut ids = Vec::new();
            targets.for_each_node(&mut |n| ids.push(n));
            for n in ids {
                found |= scan_bare_block_use(hir, n)?;
            }
            found
        }
        HirNode::GlobalWrite(_, value) => scan_bare_block_use(hir, *value)?,
        HirNode::ConstWrite { value, .. } => scan_bare_block_use(hir, *value)?,
        HirNode::Seq(body) | HirNode::Eval(body) => scan_bare_block_use_body(hir, body)?,
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => match v {
            Some(v) => scan_bare_block_use(hir, *v)?,
            None => false,
        },
        HirNode::CaseIn { subject, arms, else_body } => {
            let mut found = scan_bare_block_use(hir, *subject)?;
            for arm in arms {
                found |= scan_bare_block_use_pattern_arm(hir, arm)?;
            }
            if let Some(body) = else_body {
                found |= scan_bare_block_use_body(hir, body)?;
            }
            found
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            scan_bare_block_use(hir, *subject)? || scan_bare_block_use_pattern(hir, pattern)?
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let mut found = scan_bare_block_use_body(hir, body)?;
            for r in rescues {
                found |= scan_bare_block_use_body(hir, &r.body)?;
            }
            if let Some(b) = else_body {
                found |= scan_bare_block_use_body(hir, b)?;
            }
            if let Some(b) = ensure_body {
                found |= scan_bare_block_use_body(hir, b)?;
            }
            found
        }
        // A lambda is its own separate scope (same as an escaping block --
        // never recursed into for `uses_bare_block` purposes), but a bare
        // `yield`/`block_given?` lexically inside one still refers to a
        // DIFFERENT enclosing method's block in real Ruby, same as inside an
        // ordinary nested block literal -- rejected here for the same
        // reason `Call`'s own block-literal check above rejects that shape.
        HirNode::Lambda { body, .. } => {
            if body_contains_yield_or_block_given(hir, body) {
                return Err("`yield`/`block_given?` inside a lambda literal isn't supported yet (spike scope) -- it refers to a different enclosing method's block in real Ruby".to_string());
            }
            false
        }
        HirNode::Retry => false,
        HirNode::Redo
        | HirNode::Block { .. }
        | HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::SelfRef
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. }
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => false,
    })
}

/// Every `NodeId` embedded in `pattern` (see `Pattern::for_each_node`), OR'd
/// through `scan_bare_block_use` -- collected into a `Vec` first (rather than
/// scanned inside the `for_each_node` closure directly) since that closure
/// can't propagate this function's `Result`.
fn scan_bare_block_use_pattern(hir: &Hir, pattern: &Pattern) -> Result<bool, String> {
    let mut ids = Vec::new();
    pattern.for_each_node(&mut |n| ids.push(n));
    let mut found = false;
    for n in ids {
        found |= scan_bare_block_use(hir, n)?;
    }
    Ok(found)
}

fn scan_bare_block_use_pattern_arm(hir: &Hir, arm: &PatternArm) -> Result<bool, String> {
    let mut found = scan_bare_block_use_pattern(hir, &arm.pattern)?;
    if let Some((g, _)) = arm.guard {
        found |= scan_bare_block_use(hir, g)?;
    }
    found |= scan_bare_block_use_body(hir, &arm.body)?;
    Ok(found)
}

fn scan_bare_block_use_body(hir: &Hir, body: &[NodeId]) -> Result<bool, String> {
    let mut found = false;
    for &n in body {
        found |= scan_bare_block_use(hir, n)?;
    }
    Ok(found)
}

/// A plain, non-erroring OR-scan for `Yield`/`BlockGiven` anywhere inside
/// `body` (including further-nested blocks) -- used only by
/// `scan_bare_block_use`'s rejection check above; unlike that function, this
/// one deliberately DOES descend into nested blocks, since its only job is
/// "does this shape exist anywhere in here at all".
fn body_contains_yield_or_block_given(hir: &Hir, body: &[NodeId]) -> bool {
    body.iter().any(|&n| match &hir[n] {
        HirNode::Yield(_) | HirNode::BlockGiven => true,
        HirNode::If { then_body, else_body, .. } => {
            body_contains_yield_or_block_given(hir, then_body)
                || body_contains_yield_or_block_given(hir, else_body)
        }
        HirNode::CaseWhen { arms, else_body, .. } => {
            arms.iter().any(|(_, b)| body_contains_yield_or_block_given(hir, b))
                || body_contains_yield_or_block_given(hir, else_body)
        }
        HirNode::While { body, .. } | HirNode::Loop { body } | HirNode::For { body, .. } => {
            body_contains_yield_or_block_given(hir, body)
        }
        HirNode::CaseIn { arms, else_body, .. } => {
            arms.iter().any(|arm| body_contains_yield_or_block_given(hir, &arm.body))
                || else_body.as_deref().is_some_and(|b| body_contains_yield_or_block_given(hir, b))
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            body_contains_yield_or_block_given(hir, body)
                || rescues.iter().any(|r| body_contains_yield_or_block_given(hir, &r.body))
                || else_body.as_deref().is_some_and(|b| body_contains_yield_or_block_given(hir, b))
                || ensure_body.as_deref().is_some_and(|b| body_contains_yield_or_block_given(hir, b))
        }
        // A body statement is a `Call` node, never a bare `Block` directly
        // (see `codegen::expr`'s docs: "a Block should only be reached via
        // the Call that invokes it") -- so the only way back into a nested
        // block's body from here is through a `Call`'s own `block` field.
        HirNode::Call { block: Some(b), .. } => match &hir[*b] {
            HirNode::Block { body, .. } => body_contains_yield_or_block_given(hir, body),
            _ => false,
        },
        HirNode::Eval(body) => body_contains_yield_or_block_given(hir, body),
        _ => false,
    })
}

/// Recursively scans a method body for `@ivar` reads/writes so the class's
/// `ruby_class!` invocation knows which fields to declare. Mirrors spinel's
/// ivar-registration passes, minus the whole-program fixpoint (a single
/// bottom-up scan is enough here because ivar *names* -- unlike ivar
/// *types* -- don't depend on inference, only on which `@name` tokens
/// appear).
pub(crate) fn collect_ivars(hir: &Hir, id: NodeId, out: &mut Vec<String>) {
    match &hir[id] {
        HirNode::IvarRead(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        HirNode::IvarWrite(name, value) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
            collect_ivars(hir, *value, out);
        }
        HirNode::LocalWrite(_, value) => collect_ivars(hir, *value, out),
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            collect_ivars(hir, *l, out);
            collect_ivars(hir, *r, out);
        }
        HirNode::Defined(v) => collect_ivars(hir, *v, out),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_ivars(hir, *cond, out);
            for &n in then_body {
                collect_ivars(hir, n, out);
            }
            for &n in else_body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            if let Some(s) = subject {
                collect_ivars(hir, *s, out);
            }
            for (values, body) in arms {
                for &v in values {
                    collect_ivars(hir, v, out);
                }
                for &n in body {
                    collect_ivars(hir, n, out);
                }
            }
            for &n in else_body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Call {
            receiver,
            args,
            kwargs,
            kwargs_splat,
            block,
            block_arg,
            ..
        } => {
            if let Some(r) = receiver {
                collect_ivars(hir, *r, out);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                collect_ivars(hir, *n, out);
            }
            for pair in kwargs {
                collect_ivars(hir, pair.0, out);
                collect_ivars(hir, pair.1, out);
            }
            if let Some(s) = kwargs_splat {
                collect_ivars(hir, *s, out);
            }
            if let Some(b) = block {
                collect_ivars(hir, *b, out);
            }
            if let Some(b) = block_arg {
                collect_ivars(hir, *b, out);
            }
        }
        HirNode::New { args, .. } => {
            for &a in args {
                collect_ivars(hir, a, out);
            }
        }
        HirNode::SuperCall { args, .. } => {
            for &a in args {
                collect_ivars(hir, a, out);
            }
        }
        HirNode::Block { body, .. } | HirNode::Lambda { body, .. } => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                collect_ivars(hir, *n, out);
            }
        }
        HirNode::HashLit(pairs) => {
            for pair in pairs {
                collect_ivars(hir, pair.0, out);
                collect_ivars(hir, pair.1, out);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
            if let Some(s) = start {
                collect_ivars(hir, *s, out);
            }
            if let Some(e) = end {
                collect_ivars(hir, *e, out);
            }
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    collect_ivars(hir, *n, out);
                }
            }
        }
        HirNode::While { cond, body, .. } => {
            collect_ivars(hir, *cond, out);
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Loop { body } => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::For { target, iterable, body } => {
            target.for_each_node(&mut |n| collect_ivars(hir, n, out));
            collect_ivars(hir, *iterable, out);
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                collect_ivars(hir, *v, out);
            }
        }
        HirNode::Redo | HirNode::BlockGiven => {}
        HirNode::MultiWrite { targets, value } => {
            collect_ivars(hir, *value, out);
            targets.for_each_node(&mut |n| collect_ivars(hir, n, out));
        }
        HirNode::GlobalWrite(_, value) => collect_ivars(hir, *value, out),
        HirNode::ConstWrite { value, .. } => collect_ivars(hir, *value, out),
        HirNode::Seq(body) | HirNode::Eval(body) => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Yield(args) | HirNode::Raise(args) => {
            for &a in args {
                collect_ivars(hir, a, out);
            }
        }
        HirNode::ClassVarWrite(_, value) => collect_ivars(hir, *value, out),
        HirNode::CaseIn { subject, arms, else_body } => {
            collect_ivars(hir, *subject, out);
            for arm in arms {
                arm.pattern.for_each_node(&mut |n| collect_ivars(hir, n, out));
                if let Some((g, _)) = arm.guard {
                    collect_ivars(hir, g, out);
                }
                for &n in &arm.body {
                    collect_ivars(hir, n, out);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    collect_ivars(hir, n, out);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            collect_ivars(hir, *subject, out);
            pattern.for_each_node(&mut |n| collect_ivars(hir, n, out));
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
            for r in rescues {
                for &n in &r.body {
                    collect_ivars(hir, n, out);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    collect_ivars(hir, n, out);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    collect_ivars(hir, n, out);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::SelfRef
        | HirNode::LocalRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. } => {}
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn analyze_src(src: &str) -> Analyzed {
        let (hir, root) = crate::parse::parse_and_lower(src).expect("parse");
        analyze(hir, root).expect("analyze")
    }

    pub(super) fn analyze_err(src: &str) -> String {
        let (hir, root) = crate::parse::parse_and_lower(src).expect("parse");
        match analyze(hir, root) {
            Ok(_) => panic!("expected an analyze error"),
            Err(e) => e,
        }
    }

    pub(super) fn class_named(a: &Analyzed, name: &str) -> ClassId {
        a.compiler
            .resolve_class(name, &[], 0)
            .unwrap_or_else(|| panic!("class `{name}` not registered"))
    }

    /// Phase 15.2: reopening merges into ONE `ClassInfo` (the pre-15.2
    /// behavior pushed a shadowed duplicate whose members never dispatched).
    #[test]
    fn reopening_merges_into_the_existing_class() {
        let a = analyze_src(
            "class Foo\n  def a\n    1\n  end\nend\nclass Foo\n  def b\n    2\n  end\nend\n",
        );

        let dupes = a
            .compiler
            .classes
            .iter()
            .filter(|c| c.name == "Foo")
            .count();
        assert_eq!(dupes, 1, "one merged registration, not a shadowed pair");

        let ci = a.compiler.class(class_named(&a, "Foo"));
        let mut names: Vec<&str> = ci
            .own_methods
            .iter()
            .map(|&s| a.compiler.scope(s).name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["a", "b"]);
    }

    /// Real Ruby's last-`def`-wins rule (oracle-verified), across reopen
    /// AND within a single class body: the retained scope must be the
    /// LATER definition, distinguishable here by its parameter shape.
    #[test]
    fn redefinition_keeps_the_later_body() {
        for src in [
            // Across a reopen...
            "class Foo\n  def a\n    1\n  end\nend\nclass Foo\n  def a(x)\n    x\n  end\nend\n",
            // ...and within one body.
            "class Foo\n  def a\n    1\n  end\n  def a(x)\n    x\n  end\nend\n",
        ] {
            let a = analyze_src(src);
            let ci = a.compiler.class(class_named(&a, "Foo"));
            let sids: Vec<_> = ci
                .own_methods
                .iter()
                .filter(|&&s| a.compiler.scope(s).name == "a")
                .collect();
            assert_eq!(sids.len(), 1, "no shadowed duplicate entry");
            assert_eq!(
                a.compiler.scope(*sids[0]).params.required.len(),
                1,
                "the LATER def (the one taking a param) won"
            );
        }
    }

    /// Instance and class methods are separate namespaces: a class method
    /// must never replace a same-named instance method.
    #[test]
    fn class_and_instance_methods_do_not_replace_each_other() {
        let a = analyze_src(
            "class Foo\n  def a\n    1\n  end\n  def self.a\n    2\n  end\nend\n",
        );
        let ci = a.compiler.class(class_named(&a, "Foo"));
        assert_eq!(ci.own_methods.len(), 1);
        assert_eq!(ci.own_class_methods.len(), 1);
    }

    #[test]
    fn reopening_appends_includes() {
        let a = analyze_src(
            "module M1\nend\nmodule M2\nend\nclass Foo\n  include M1\nend\nclass Foo\n  include M2\nend\n",
        );
        let ci = a.compiler.class(class_named(&a, "Foo"));
        assert_eq!(ci.includes.len(), 2);
    }

    /// The reopen guards, each mirroring CRuby's own TypeError
    /// (oracle-verified messages).
    #[test]
    fn reopen_guards_mirror_ruby_type_errors() {
        assert!(analyze_err(
            "class Base\nend\nclass Other\nend\nclass Sub < Base\nend\nclass Sub < Other\nend\n"
        )
        .contains("superclass mismatch for class Sub"));

        assert!(analyze_err("class Foo\nend\nmodule Foo\nend\n").contains("Foo is not a module"));
        assert!(analyze_err("module Bar\nend\nclass Bar\nend\n").contains("Bar is not a class"));
    }

    /// A reopen may RESTATE the original superclass (real Ruby allows it).
    #[test]
    fn reopen_with_matching_superclass_is_allowed() {
        let a = analyze_src(
            "class Base\nend\nclass Sub < Base\nend\nclass Sub < Base\n  def ok\n    1\n  end\nend\n",
        );
        let ci = a.compiler.class(class_named(&a, "Sub"));
        assert_eq!(ci.own_methods.len(), 1);
    }

    /// Builtins stay un-reopenable until Phase 16.3 (loud, not silent).
    #[test]
    fn reopening_a_builtin_is_still_rejected() {
        assert!(analyze_err("class Integer\n  def double\n    self\n  end\nend\n")
            .contains("isn't supported yet (spike scope)"));
    }
}

#[cfg(test)]
mod namespacing_tests {
    use super::tests::{analyze_err, analyze_src, class_named};

    /// Phase 15.3: nested definitions register with `lexical_parent`,
    /// resolve scope-exactly, and display fully qualified.
    #[test]
    fn nested_definition_registers_under_its_namespace() {
        let a = analyze_src(
            "module Store\n  class Item\n    def price\n      1\n    end\n  end\nend\n",
        );
        let store = class_named(&a, "Store");
        let item = a
            .compiler
            .resolve_class("Store::Item", &[], 0)
            .expect("qualified path resolves");

        assert_eq!(a.compiler.class(item).lexical_parent, Some(store));
        assert_eq!(a.compiler.fq_name(item), "Store::Item");
        assert_eq!(
            a.compiler.resolve_class("Item", &[], 0),
            None,
            "a nested class is invisible at the top level by bare name"
        );
        assert_eq!(
            a.compiler.resolve_class("Item", &[store], 0),
            Some(item),
            "...but resolves lexically from inside its namespace"
        );
    }

    /// The two definition forms differ in CREF only: textual nesting sees
    /// the enclosing scope, the qualified form does not (oracle-verified
    /// NameError in real Ruby).
    #[test]
    fn qualified_definition_form_cuts_the_cref_chain() {
        let a = analyze_src(
            "module Store\n  class Inner\n  end\nend\nclass Store::Cart\nend\n",
        );
        let store = class_named(&a, "Store");
        let inner = a.compiler.resolve_class("Store::Inner", &[], 0).unwrap();
        let cart = a.compiler.resolve_class("Store::Cart", &[], 0).unwrap();

        assert!(!a.compiler.class(inner).qualified_def);
        assert_eq!(a.compiler.cref_of(Some(inner)), vec![store, inner]);

        assert!(a.compiler.class(cart).qualified_def);
        assert_eq!(
            a.compiler.cref_of(Some(cart)),
            vec![cart],
            "the qualified form's body does not see `Store` lexically"
        );
        assert_eq!(a.compiler.fq_name(cart), "Store::Cart", "naming still qualifies");
    }

    #[test]
    fn same_leaf_name_in_two_namespaces_stays_distinct() {
        let a = analyze_src(
            "module A1\n  class Widget\n  end\nend\nmodule B1\n  class Widget\n  end\nend\n",
        );
        let wa = a.compiler.resolve_class("A1::Widget", &[], 0).unwrap();
        let wb = a.compiler.resolve_class("B1::Widget", &[], 0).unwrap();
        assert_ne!(wa, wb);
    }

    /// Reopening composes with nesting (Phase 15.2 + 15.3): both the
    /// textual and the qualified reopen merge into the one registration.
    #[test]
    fn nested_class_reopens_through_both_forms() {
        let a = analyze_src(
            "module Store\n  class Item\n    def a\n      1\n    end\n  end\nend\nmodule Store\n  class Item\n    def b\n      2\n    end\n  end\nend\nclass Store::Item\n  def c\n    3\n  end\nend\n",
        );
        let item = a.compiler.resolve_class("Store::Item", &[], 0).unwrap();
        let count = a
            .compiler
            .classes
            .iter()
            .filter(|c| c.name == "Item")
            .count();
        assert_eq!(count, 1, "one merged registration across all three bodies");
        assert_eq!(a.compiler.class(item).own_methods.len(), 3);
    }

    #[test]
    fn qualified_definition_with_unknown_prefix_is_an_error() {
        assert!(analyze_err("class Nowhere::Item\nend\n")
            .contains("unknown class/module `Nowhere`"));
    }

    /// `module Store; class String; end; end` defines a fresh, unrelated
    /// nested class (real Ruby) -- it shadows the builtin lexically inside
    /// `Store` and nowhere else.
    #[test]
    fn a_nested_class_may_shadow_a_builtin_lexically() {
        let a = analyze_src("module Store\n  class String\n  end\nend\n");
        let store = class_named(&a, "Store");
        let nested = a.compiler.resolve_class("Store::String", &[], 0).unwrap();

        assert!(!a.compiler.class(nested).is_builtin);
        assert_eq!(
            a.compiler.resolve_class("String", &[], 0),
            Some(crate::compiler::STRING_CLASS),
            "top-level `String` is still the builtin"
        );
        assert_eq!(a.compiler.resolve_class("String", &[store], 0), Some(nested));
    }

    /// A leading `::` anchors a definition at the top level from any depth.
    #[test]
    fn top_anchored_definition_escapes_its_namespace() {
        let a = analyze_src("module M\n  class ::Escaped\n  end\nend\n");
        let escaped = class_named(&a, "Escaped");
        assert_eq!(a.compiler.class(escaped).lexical_parent, None);
        assert_eq!(a.compiler.fq_name(escaped), "Escaped");
    }
}

#[cfg(test)]
mod class_value_tests {
    use super::tests::analyze_src;
    use crate::compiler::{CLASS_CLASS, MODULE_CLASS, OBJECT_CLASS};
    use crate::types::TyKind;

    /// Phase 16.1: a local assigned a bare class name is statically typed
    /// as a class VALUE of that class -- what keeps `x.new`/class-method
    /// calls through the variable on Path 1.
    #[test]
    fn a_class_assigned_to_a_local_types_as_class_obj() {
        let a = analyze_src("class Widget\nend\nx = Widget\n");
        let widget = a.compiler.resolve_class("Widget", &[], 0).unwrap();
        assert_eq!(a.main_local_types.get("x"), Some(&TyKind::ClassObj(widget)));
    }

    /// `x.new` through the class-value-typed local types exactly like a
    /// literal `Widget.new` (both emit the same unboxed construction).
    #[test]
    fn new_through_a_class_value_types_as_the_instance() {
        let a = analyze_src("class Widget\nend\nx = Widget\ny = x.new\n");
        let widget = a.compiler.resolve_class("Widget", &[], 0).unwrap();
        assert_eq!(a.main_local_types.get("y"), Some(&TyKind::Object(widget)));
    }

    /// `Class < Module < Object` -- the special-cased parent seed, so
    /// `Widget.is_a?(Module)` answers true through the ordinary ancestry
    /// machinery.
    #[test]
    fn class_class_linearizes_under_module() {
        let a = analyze_src("");
        let ancestors = &a.compiler.class(CLASS_CLASS).ancestors;
        assert_eq!(ancestors, &vec![CLASS_CLASS, MODULE_CLASS, OBJECT_CLASS]);
    }
}
