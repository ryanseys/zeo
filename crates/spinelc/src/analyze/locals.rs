//! Forward, single-pass local-variable type tracking -- explicitly NOT the
//! whole-program fixpoint deferred in the plan's stated scope-cut (see
//! `docs/PORTING_ANALYSIS.md` roadmap item 2). Walks a method/top-level body
//! in source order, refining each local's `TyKind` as it's assigned, so
//! `codegen` can resolve `x + y` to native `Int` arithmetic when `x`/`y` are
//! locals previously assigned an `Int`-typed value -- not just
//! literal-on-literal operands, which is all the pre-Phase-1 spike handled.
//!
//! Scope-cut: no control flow exists yet in the phase this lands (Phase 1),
//! so a straight-line sequential walk is sound -- reassigning a local simply
//! overwrites its prior entry. Once branches/loops exist (Phase 2/4), a
//! write inside a conditional/loop body needs to *widen* to `Poly` on
//! disagreement with the type entering the branch, rather than
//! unconditionally overwriting; that join-at-merge-point rule is deferred to
//! those phases, structured so it can wrap this same walk rather than
//! replace it.

use crate::compiler::Compiler;
use crate::hir::{HirNode, NodeId};
use crate::types::{infer_type_with_locals, TyKind};
use std::collections::HashMap;

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
