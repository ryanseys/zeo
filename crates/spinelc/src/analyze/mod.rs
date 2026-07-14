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

    let mut main_statements = Vec::new();
    for stmt in statements {
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
            register_class(&mut compiler, name, superclass, is_module, &body)?;
        } else {
            main_statements.push(stmt);
        }
    }

    // Ancestor linearization + method/class-method materialization + class
    // variable ownership -- must run AFTER every `ClassDef` above has been
    // registered, since `include`/`extend`/`prepend`/`< Super` targets must
    // already exist (same "defined earlier in the file" rule `superclass`
    // resolution already enforces). See `mro`'s module docs.
    mro::materialize(&mut compiler)?;

    let main_local_types = locals::infer_locals(&compiler, &main_statements);

    Ok(Analyzed {
        compiler,
        main_statements,
        main_local_types,
    })
}

fn register_class(
    compiler: &mut Compiler,
    name: String,
    superclass: Option<String>,
    is_module: bool,
    body: &[NodeId],
) -> Result<(), String> {
    let parent = if is_module {
        None
    } else {
        Some(match &superclass {
            None => OBJECT_CLASS,
            Some(s) => compiler.class_by_name(s).ok_or_else(|| {
                format!("unknown superclass `{s}` (must be defined earlier in the file)")
            })?,
        })
    };
    let class_id = compiler.add_class(name, parent, is_module);

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
                if is_class_method {
                    compiler.classes[class_id.0 as usize]
                        .own_class_methods
                        .push(sid);
                } else {
                    compiler.classes[class_id.0 as usize].own_methods.push(sid);
                }
            }
            HirNode::Include(m) => {
                let target = resolve_module_target(compiler, m)?;
                compiler.classes[class_id.0 as usize].includes.push(target);
            }
            HirNode::Extend(m) => {
                let target = resolve_module_target(compiler, m)?;
                compiler.classes[class_id.0 as usize].extends.push(target);
            }
            HirNode::Prepend(m) => {
                let target = resolve_module_target(compiler, m)?;
                compiler.classes[class_id.0 as usize].prepends.push(target);
            }
            HirNode::ClassVarWrite(..) => {
                compiler.classes[class_id.0 as usize]
                    .class_body_stmts
                    .push(stmt);
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve_module_target(compiler: &Compiler, name: &str) -> Result<ClassId, String> {
    compiler
        .class_by_name(name)
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
    let mut local_types = locals::infer_locals(compiler, &body);
    for id in params.default_ids() {
        locals::track_extra(compiler, &mut local_types, id);
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
        HirNode::For { iterable, body, .. } => {
            scan_bare_block_use(hir, *iterable)? || scan_bare_block_use_body(hir, body)?
        }
        HirNode::Call {
            receiver,
            args,
            kwargs,
            block,
            block_arg,
            ..
        } => {
            let mut found = false;
            if let Some(r) = receiver {
                found |= scan_bare_block_use(hir, *r)?;
            }
            for &a in args {
                found |= scan_bare_block_use(hir, a)?;
            }
            for pair in kwargs {
                found |= scan_bare_block_use(hir, pair.0)?;
                found |= scan_bare_block_use(hir, pair.1)?;
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
        HirNode::New { args, .. } | HirNode::SuperCall { args } | HirNode::Raise(args) => {
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
        HirNode::StringLit(parts) => {
            let mut found = false;
            for p in parts {
                if let StrPart::Interp(n) = p {
                    found |= scan_bare_block_use(hir, *n)?;
                }
            }
            found
        }
        HirNode::MultiWrite { value, .. } => scan_bare_block_use(hir, *value)?,
        HirNode::Eval(body) => scan_bare_block_use_body(hir, body)?,
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
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
            block,
            block_arg,
            ..
        } => {
            if let Some(r) = receiver {
                collect_ivars(hir, *r, out);
            }
            for &a in args {
                collect_ivars(hir, a, out);
            }
            for pair in kwargs {
                collect_ivars(hir, pair.0, out);
                collect_ivars(hir, pair.1, out);
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
        HirNode::SuperCall { args } => {
            for &a in args {
                collect_ivars(hir, a, out);
            }
        }
        HirNode::Block { body, .. } => {
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
        HirNode::StringLit(parts) => {
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
        HirNode::For { iterable, body, .. } => {
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
        HirNode::MultiWrite { value, .. } => collect_ivars(hir, *value, out),
        HirNode::Eval(body) => {
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
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_) => {}
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => {}
    }
}
