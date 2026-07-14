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
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
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
        HirNode::IvarWrite(_, value) | HirNode::ClassVarWrite(_, value) => {
            track_node(compiler, locals, *value)
        }
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
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                track_node(compiler, locals, *n);
            }
        }
        HirNode::HashLit(pairs) => {
            for pair in pairs {
                track_node(compiler, locals, pair.0);
                track_node(compiler, locals, pair.1);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
            if let Some(s) = start {
                track_node(compiler, locals, *s);
            }
            if let Some(e) = end {
                track_node(compiler, locals, *e);
            }
        }
        HirNode::StringLit(parts) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    track_node(compiler, locals, *n);
                }
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
                track_node(compiler, locals, *r);
            }
            for &a in args {
                track_node(compiler, locals, a);
            }
            for pair in kwargs {
                track_node(compiler, locals, pair.0);
                track_node(compiler, locals, pair.1);
            }
            if let Some(b) = block {
                track_node(compiler, locals, *b);
            }
            if let Some(b) = block_arg {
                track_node(compiler, locals, *b);
            }
        }
        HirNode::Block { body, .. } => {
            for &n in body {
                track_node(compiler, locals, n);
            }
        }
        HirNode::While { cond, body, .. } => {
            track_node(compiler, locals, *cond);
            *locals = join_loop(compiler, locals, body, None);
        }
        HirNode::Loop { body } => {
            *locals = join_loop(compiler, locals, body, None);
        }
        HirNode::For { var, iterable, body } => {
            track_node(compiler, locals, *iterable);
            // A `for`-in-`Range` variable is provably always `Int` (the only
            // element type `Range` iteration supports -- see
            // `codegen::loops::emit_for`); an `Array`'s element type isn't
            // tracked per-element, so it widens to `Poly`.
            let elem_ty = match infer_type_with_locals(compiler, locals, *iterable) {
                TyKind::Range => TyKind::Int,
                _ => TyKind::Poly,
            };
            *locals = join_loop(compiler, locals, body, Some((var, elem_ty)));
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                track_node(compiler, locals, *v);
            }
        }
        HirNode::Redo | HirNode::BlockGiven => {}
        HirNode::MultiWrite {
            before,
            splat,
            after,
            value,
        } => {
            track_node(compiler, locals, *value);
            // Destructured targets' element types aren't tracked precisely
            // (spike scope) -- an arbitrary Array's element types are
            // unknown -- so each one widens to `Poly`, same as any other
            // Array-`[]` read.
            for name in before.iter().chain(splat.iter()).chain(after.iter()) {
                locals.insert(name.clone(), TyKind::Poly);
            }
        }
        HirNode::Eval(body) => {
            for &n in body {
                track_node(compiler, locals, n);
            }
        }
        HirNode::Yield(args) => {
            for &a in args {
                track_node(compiler, locals, a);
            }
        }
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
    merge_locals(branch_locals)
}

/// A local keeps its type only if every one of `maps` agrees on it (a map
/// that never mentions it still "votes" with its own `.get`, so `None`
/// disagrees with `Some(_)` just as much as two different `Some`s would);
/// any disagreement widens to `Poly`. Shared by `join_branches` (`if`/`case`)
/// and `join_loop` below -- both boil down to "does every possible path
/// through this control-flow construct agree?".
fn merge_locals(maps: Vec<HashMap<String, TyKind>>) -> HashMap<String, TyKind> {
    let keys: HashSet<&String> = maps.iter().flat_map(HashMap::keys).collect();
    keys.into_iter()
        .map(|key| {
            let first = maps[0].get(key).copied();
            let agrees = maps.iter().all(|m| m.get(key).copied() == first);
            (key.clone(), if agrees { first.unwrap_or(TyKind::Poly) } else { TyKind::Poly })
        })
        .collect()
}

/// Local types after a loop: must agree with either "the body ran zero
/// times" (locals unchanged from `before`) or "the body ran" (walked once,
/// optionally seeding a per-iteration binding first -- e.g. `for`'s index
/// variable) -- the same disagreement-widens-to-`Poly` rule `join_branches`
/// uses for `if`/`case`, since the real iteration count isn't known at
/// compile time. Matches this module's stated plan for loops: "treating
/// 'loop body ran zero times' as one more branch to agree with".
fn join_loop(
    compiler: &Compiler,
    before: &HashMap<String, TyKind>,
    body: &[NodeId],
    seed: Option<(&str, TyKind)>,
) -> HashMap<String, TyKind> {
    let mut ran = before.clone();
    if let Some((name, ty)) = seed {
        ran.insert(name.to_string(), ty);
    }
    for &n in body {
        track_node(compiler, &mut ran, n);
    }
    merge_locals(vec![ran, before.clone()])
}
