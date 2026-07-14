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

use crate::compiler::{Compiler, Scope, OBJECT_CLASS};
use crate::hir::{ArrayElem, Hir, HirNode, NodeId, StrPart};
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
        } = &compiler.hir[stmt]
        {
            let name = name.clone();
            let superclass = superclass.clone();
            let body = body.clone();
            register_class(&mut compiler, name, superclass, &body)?;
        } else {
            main_statements.push(stmt);
        }
    }

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
    body: &[NodeId],
) -> Result<(), String> {
    let parent = match &superclass {
        None => OBJECT_CLASS,
        Some(s) => compiler.class_by_name(s).ok_or_else(|| {
            format!("unknown superclass `{s}` (must be defined earlier in the file)")
        })?,
    };
    let class_id = compiler.add_class(name, parent);

    let mut ivars: Vec<String> = Vec::new();
    for &stmt in body {
        if let HirNode::DefMethod { name, params, body } = &compiler.hir[stmt] {
            let (name, params, body) = (name.clone(), params.clone(), body.clone());
            for &n in &body {
                collect_ivars(&compiler.hir, n, &mut ivars);
            }
            let local_types = locals::infer_locals(compiler, &body);
            compiler.add_scope(Scope {
                name,
                class: Some(class_id),
                params,
                body,
                local_types,
            });
        }
    }
    compiler.classes[class_id.0 as usize].ivars = ivars;
    Ok(())
}

/// Recursively scans a method body for `@ivar` reads/writes so the class's
/// `ruby_class!` invocation knows which fields to declare. Mirrors spinel's
/// ivar-registration passes, minus the whole-program fixpoint (a single
/// bottom-up scan is enough here because ivar *names* -- unlike ivar
/// *types* -- don't depend on inference, only on which `@name` tokens
/// appear).
fn collect_ivars(hir: &Hir, id: NodeId, out: &mut Vec<String>) {
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
            block,
            ..
        } => {
            if let Some(r) = receiver {
                collect_ivars(hir, *r, out);
            }
            for &a in args {
                collect_ivars(hir, a, out);
            }
            if let Some(b) = block {
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
        HirNode::Break(v) | HirNode::Next(v) => {
            if let Some(v) = v {
                collect_ivars(hir, *v, out);
            }
        }
        HirNode::Redo => {}
        HirNode::MultiWrite { value, .. } => collect_ivars(hir, *value, out),
        HirNode::Eval(body) => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::LocalRead(_) => {}
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => {}
    }
}
