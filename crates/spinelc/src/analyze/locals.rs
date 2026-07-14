//! Forward, single-pass local-variable type tracking -- explicitly NOT the
//! whole-program fixpoint deferred in the plan's stated scope-cut (see
//! `docs/PORTING_ANALYSIS.md` roadmap item 2). Walks a method/top-level body
//! in source order, refining each local's `TyKind` as it's assigned, so
//! `codegen` can resolve `x + y` to native `Int` arithmetic when `x`/`y` are
//! locals previously assigned an `Int`-typed value -- not just
//! literal-on-literal operands, which is all the pre-Phase-1 spike handled.
//!
//! Branches (`if`/`case`, as of Phase 2) are handled by `join_branches`: each
//! branch is walked against its own clone of the pre-branch map, and a local
//! keeps its type after the branch only if every branch's exit state agrees
//! on it (including a branch that never touches it at all, which "agrees"
//! with the pre-branch type by definition) -- any disagreement widens to
//! `Poly`, since which branch actually ran isn't known at compile time.
//! Loops (Phase 4) will reuse the same join, treating "loop body ran zero
//! times" as one more branch to agree with.

use crate::compiler::Compiler;
use crate::hir::{HirNode, NodeId};
use crate::types::{infer_type_with_locals, TyKind};
use std::collections::{HashMap, HashSet};

pub fn infer_locals(compiler: &Compiler, body: &[NodeId]) -> HashMap<String, TyKind> {
    let mut locals = HashMap::new();
    for &stmt in body {
        track_node(compiler, &mut locals, stmt);
    }
    locals
}

/// Recurses into every sub-expression position a `LocalWrite` could appear
/// in (mirrors `analyze::collect_ivars`'s traversal shape exactly), so an
/// assignment nested inside a call's receiver/args/block -- not just a
/// bare top-level statement -- still updates the map before later
/// statements read it.
fn track_node(compiler: &Compiler, locals: &mut HashMap<String, TyKind>, id: NodeId) {
    match &compiler.hir[id] {
        HirNode::LocalWrite(name, value) => {
            track_node(compiler, locals, *value);
            let ty = infer_type_with_locals(compiler, locals, *value);
            locals.insert(name.clone(), ty);
        }
        HirNode::IvarWrite(_, value) => track_node(compiler, locals, *value),
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            track_node(compiler, locals, *l);
            track_node(compiler, locals, *r);
        }
        HirNode::Defined(v) => track_node(compiler, locals, *v),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            track_node(compiler, locals, *cond);
            *locals = join_branches(compiler, locals, &[then_body, else_body]);
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            if let Some(s) = subject {
                track_node(compiler, locals, *s);
            }
            let mut branches: Vec<&[NodeId]> = Vec::with_capacity(arms.len() + 1);
            for (values, body) in arms {
                for &v in values {
                    track_node(compiler, locals, v);
                }
                branches.push(body);
            }
            branches.push(else_body);
            *locals = join_branches(compiler, locals, &branches);
        }
        HirNode::New { args, .. } | HirNode::SuperCall { args } => {
            for &a in args {
                track_node(compiler, locals, a);
            }
        }
        HirNode::Call {
            receiver,
            args,
            block,
            ..
        } => {
            if let Some(r) = receiver {
                track_node(compiler, locals, *r);
            }
            for &a in args {
                track_node(compiler, locals, a);
            }
            if let Some(b) = block {
                track_node(compiler, locals, *b);
            }
        }
        HirNode::Block { body, .. } => {
            for &n in body {
                track_node(compiler, locals, n);
            }
        }
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => {}
    }
}

/// Walks each of `branches` independently against its own clone of `before`,
/// then merges: a local keeps its type only if every branch's resulting map
/// agrees on it (a branch that never assigns it still "votes" with its
/// cloned-from-`before` type, so an untouched local survives the join
/// unchanged); any disagreement -- including a branch introducing a
/// brand-new binding the others don't have -- widens to `Poly`, since only
/// one branch actually runs at runtime and the compiler can't know which.
fn join_branches(
    compiler: &Compiler,
    before: &HashMap<String, TyKind>,
    branches: &[&[NodeId]],
) -> HashMap<String, TyKind> {
    let branch_locals: Vec<HashMap<String, TyKind>> = branches
        .iter()
        .map(|&body| {
            let mut b = before.clone();
            for &n in body {
                track_node(compiler, &mut b, n);
            }
            b
        })
        .collect();

    let keys: HashSet<&String> = branch_locals.iter().flat_map(HashMap::keys).collect();

    keys.into_iter()
        .map(|key| {
            let first = branch_locals[0].get(key).copied();
            let agrees = branch_locals.iter().all(|b| b.get(key).copied() == first);
            (key.clone(), if agrees { first.unwrap_or(TyKind::Poly) } else { TyKind::Poly })
        })
        .collect()
}
