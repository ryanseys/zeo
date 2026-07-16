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

use crate::compiler::{ClassId, Compiler};
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use crate::types::{infer_type_with_locals, TyKind};
use std::collections::{HashMap, HashSet};

pub fn infer_locals(compiler: &Compiler, defining: Option<ClassId>, box_id: u32, body: &[NodeId]) -> HashMap<String, TyKind> {
    let mut locals = HashMap::new();
    for &stmt in body {
        track_node(compiler, defining, box_id, &mut locals, stmt);
    }
    locals
}

/// Additionally walks one more root (a parameter's default-value expression,
/// via `Params::default_ids`) into an already-built locals map -- see
/// `hir::Params`'s docs: a default can reference/assign a
/// local exactly like an ordinary body statement can, and `infer_locals`
/// alone never sees it (defaults aren't part of `body`).
pub fn track_extra(compiler: &Compiler, defining: Option<ClassId>, box_id: u32, locals: &mut HashMap<String, TyKind>, id: NodeId) {
    track_node(compiler, defining, box_id, locals, id);
}

/// Recurses into every sub-expression position a `LocalWrite` could appear
/// in (mirrors `analyze::collect_ivars`'s traversal shape exactly), so an
/// assignment nested inside a call's receiver/args/block -- not just a
/// bare top-level statement -- still updates the map before later
/// statements read it.
fn track_node(compiler: &Compiler, defining: Option<ClassId>, box_id: u32, locals: &mut HashMap<String, TyKind>, id: NodeId) {
    match &compiler.hir[id] {
        // A lambda's own body is a fresh, independent scope for local-
        // variable TYPE tracking purposes -- same treatment a non-`.times`
        // escaping block already gets from this function's `Call` arm
        // (simply never recursed into).
        HirNode::Lambda { .. } => {}
        HirNode::LocalWrite(name, value) => {
            track_node(compiler, defining, box_id, locals, *value);
            let ty = infer_type_with_locals(compiler, defining, box_id, locals, *value);
            // JOINED with any earlier assignment's type, not overwritten:
            // codegen reads this map flow-INsensitively (one entry per
            // local for the whole scope), so a local reassigned to a
            // DIFFERENT type -- two different concrete classes, an Int
            // then a String -- can only soundly be `Poly` everywhere
            // (the overwrite miscompiled reads occurring BEFORE the later
            // assignment, e.g. boxing an `Arc<P>` with class Q's ident).
            let joined = match locals.get(name) {
                Some(prev) if *prev != ty => TyKind::Poly,
                _ => ty,
            };
            locals.insert(name.clone(), joined);
        }
        HirNode::IvarWrite(_, value) | HirNode::ClassVarWrite(_, value) => {
            track_node(compiler, defining, box_id, locals, *value)
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            track_node(compiler, defining, box_id, locals, *l);
            track_node(compiler, defining, box_id, locals, *r);
        }
        HirNode::Defined(v) => track_node(compiler, defining, box_id, locals, *v),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            track_node(compiler, defining, box_id, locals, *cond);
            *locals = join_branches(compiler, defining, box_id, locals, &[then_body, else_body]);
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            if let Some(s) = subject {
                track_node(compiler, defining, box_id, locals, *s);
            }
            let mut branches: Vec<&[NodeId]> = Vec::with_capacity(arms.len() + 1);
            for (values, body) in arms {
                for &v in values {
                    track_node(compiler, defining, box_id, locals, v);
                }
                branches.push(body);
            }
            branches.push(else_body);
            *locals = join_branches(compiler, defining, box_id, locals, &branches);
        }
        HirNode::New { args, .. } => {
            for &a in args {
                track_node(compiler, defining, box_id, locals, a);
            }
        }
        HirNode::SuperCall { args, block, .. } => {
            for &a in args {
                track_node(compiler, defining, box_id, locals, a);
            }
            if let Some(b) = block {
                track_node(compiler, defining, box_id, locals, *b);
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                track_node(compiler, defining, box_id, locals, *n);
            }
        }
        HirNode::HashLit(pairs) => {
            for pair in pairs {
                track_node(compiler, defining, box_id, locals, pair.0);
                track_node(compiler, defining, box_id, locals, pair.1);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
            if let Some(s) = start {
                track_node(compiler, defining, box_id, locals, *s);
            }
            if let Some(e) = end {
                track_node(compiler, defining, box_id, locals, *e);
            }
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    track_node(compiler, defining, box_id, locals, *n);
                }
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
                track_node(compiler, defining, box_id, locals, *r);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                track_node(compiler, defining, box_id, locals, *n);
            }
            for pair in kwargs {
                track_node(compiler, defining, box_id, locals, pair.0);
                track_node(compiler, defining, box_id, locals, pair.1);
            }
            if let Some(s) = kwargs_splat {
                track_node(compiler, defining, box_id, locals, *s);
            }
            if let Some(b) = block {
                track_node(compiler, defining, box_id, locals, *b);
            }
            if let Some(b) = block_arg {
                track_node(compiler, defining, box_id, locals, *b);
            }
        }
        HirNode::Block { body, .. } => {
            for &n in body {
                track_node(compiler, defining, box_id, locals, n);
            }
        }
        HirNode::While { cond, body, .. } => {
            track_node(compiler, defining, box_id, locals, *cond);
            *locals = join_loop(compiler, defining, box_id, locals, body, None);
        }
        HirNode::Loop { body } => {
            *locals = join_loop(compiler, defining, box_id, locals, body, None);
        }
        HirNode::For { target, iterable, body } => {
            track_node(compiler, defining, box_id, locals, *iterable);
            // A `for`-in-`Range` variable is provably always `Int` (the only
            // element type `Range` iteration supports -- see
            // `codegen::loops::emit_for`) when the target is a single plain
            // local; an `Array`'s element type isn't tracked per-element (nor
            // is a destructured `for a, b in ...`'s), so those widen to
            // `Poly`.
            let elem_ty = match (target, infer_type_with_locals(compiler, defining, box_id, locals, *iterable)) {
                (crate::hir::MultiTarget::Local(_), TyKind::Range) => TyKind::Int,
                _ => TyKind::Poly,
            };
            *locals = join_loop(compiler, defining, box_id, locals, body, Some((target, elem_ty)));
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                track_node(compiler, defining, box_id, locals, *v);
            }
        }
        HirNode::Redo | HirNode::BlockGiven | HirNode::SelfRef => {}
        HirNode::MultiWrite { targets, value } => {
            track_node(compiler, defining, box_id, locals, *value);
            targets.for_each_node(&mut |n| track_node(compiler, defining, box_id, locals, n));
            // Destructured targets' element types aren't tracked precisely
            // (spike scope) -- an arbitrary Array's element types are
            // unknown -- so each local-like target widens to `Poly`, same as
            // any other Array-`[]` read.
            targets.for_each_local_name(&mut |n| {
                locals.insert(n.to_string(), TyKind::Poly);
            });
        }
        HirNode::GlobalWrite(_, value) => track_node(compiler, defining, box_id, locals, *value),
        HirNode::ConstWrite { value, .. } => track_node(compiler, defining, box_id, locals, *value),
        HirNode::Seq(body) => {
            for &n in body {
                track_node(compiler, defining, box_id, locals, n);
            }
        }
        HirNode::BoxScope { box_id: bx, body } => {
            for &s in body.clone().iter() {
                track_node(compiler, defining, *bx, locals, s);
            }
        }
        HirNode::Eval(body) => {
            for &n in body {
                track_node(compiler, defining, box_id, locals, n);
            }
        }
        HirNode::Yield(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                track_node(compiler, defining, box_id, locals, *n);
            }
        }
        HirNode::Raise(args) => {
            for &a in args {
                track_node(compiler, defining, box_id, locals, a);
            }
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            track_node(compiler, defining, box_id, locals, *subject);
            let mut branch_locals: Vec<HashMap<String, TyKind>> = Vec::new();
            for arm in arms {
                let mut b = locals.clone();
                // A pattern-bound name is always `Poly` from this static
                // tracker's point of view -- narrowing to a builtin type
                // after a class-guard match is a `codegen`-only concept
                // (see `codegen::patterns::collect_narrowing`), scoped to
                // just that one arm's own emitted body, not this longer-
                // lived `Scope::local_types` map.
                arm.pattern.for_each_bound_name(&mut |n| {
                    b.entry(n.to_string()).or_insert(TyKind::Poly);
                });
                if let Some((g, _)) = arm.guard {
                    track_node(compiler, defining, box_id, &mut b, g);
                }
                for &n in &arm.body {
                    track_node(compiler, defining, box_id, &mut b, n);
                }
                branch_locals.push(b);
            }
            if let Some(body) = else_body {
                let mut b = locals.clone();
                for &n in body {
                    track_node(compiler, defining, box_id, &mut b, n);
                }
                branch_locals.push(b);
            }
            // No explicit `else` means an unmatched subject RAISES
            // (`NoMatchingPatternError`) rather than falling through to
            // `nil` the way a value-matching `CaseWhen` would -- so, unlike
            // `join_branches`, there's no "nothing happened" continuation to
            // merge in; only the arms' (and any explicit else's) own
            // branches participate. An empty `branch_locals` (a
            // `case/in` with no `in` clauses at all -- not valid Ruby, but
            // guarded against rather than silently wiping every known local)
            // leaves `locals` untouched.
            if !branch_locals.is_empty() {
                *locals = merge_locals(branch_locals);
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            track_node(compiler, defining, box_id, locals, *subject);
            let mut matched = locals.clone();
            pattern.for_each_bound_name(&mut |n| {
                matched.insert(n.to_string(), TyKind::Poly);
            });
            *locals = merge_locals(vec![matched, locals.clone()]);
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            // `before` (body may raise partway through, so a rescue clause
            // conservatively forks from BEFORE any of body's own assignments
            // -- safer than crediting a partial run, and `merge_locals`'
            // disagreement-widens-to-`Poly` rule already makes this sound
            // either way) vs. the success path (body ran to completion,
            // optionally further refined by an `else`).
            let before = locals.clone();
            for &n in body {
                track_node(compiler, defining, box_id, locals, n);
            }
            let mut success = locals.clone();
            if let Some(b) = else_body {
                for &n in b {
                    track_node(compiler, defining, box_id, &mut success, n);
                }
            }
            let mut branches = vec![success];
            for r in rescues {
                let mut b = before.clone();
                if let Some(name) = &r.binding {
                    b.entry(name.clone()).or_insert(TyKind::Poly);
                }
                for &n in &r.body {
                    track_node(compiler, defining, box_id, &mut b, n);
                }
                branches.push(b);
            }
            *locals = merge_locals(branches);
            // `ensure` always runs, unconditionally, AFTER every other path
            // has settled -- not one more branch to merge, a deterministic
            // continuation of whichever state the merge above produced.
            if let Some(b) = ensure_body {
                for &n in b {
                    track_node(compiler, defining, box_id, locals, n);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        // An imaginary literal's inner node is itself a numeric
        // literal by syntax -- a leaf for this walk's purposes.
        | HirNode::ImaginaryLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoxHandle(_)
        | HirNode::BoolLit(_)
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. }
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
    defining: Option<ClassId>,
    box_id: u32,
    before: &HashMap<String, TyKind>,
    branches: &[&[NodeId]],
) -> HashMap<String, TyKind> {
    let branch_locals: Vec<HashMap<String, TyKind>> = branches
        .iter()
        .map(|&body| {
            let mut b = before.clone();
            for &n in body {
                track_node(compiler, defining, box_id, &mut b, n);
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
    defining: Option<ClassId>,
    box_id: u32,
    before: &HashMap<String, TyKind>,
    body: &[NodeId],
    seed: Option<(&crate::hir::MultiTarget, TyKind)>,
) -> HashMap<String, TyKind> {
    let mut ran = before.clone();
    if let Some((target, elem_ty)) = seed {
        target.for_each_node(&mut |n| track_node(compiler, defining, box_id, &mut ran, n));
        target.for_each_local_name(&mut |n| {
            ran.insert(n.to_string(), elem_ty);
        });
    }
    for &n in body {
        track_node(compiler, defining, box_id, &mut ran, n);
    }
    merge_locals(vec![ran, before.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the real parse+analyze pipeline and reports the TOP-LEVEL
    /// scope's inferred type for one local.
    fn ty_of(source: &str, local: &str) -> TyKind {
        let (hir, root) = crate::parse::parse_and_lower(source).expect("parses");
        let analyzed = crate::analyze::analyze(hir, root).expect("analyzes");
        analyzed
            .main_local_types
            .get(local)
            .copied()
            .unwrap_or_else(|| panic!("no local named `{local}`"))
    }

    #[test]
    fn a_local_assigned_once_keeps_that_type() {
        assert_eq!(ty_of("x = 1", "x"), TyKind::Int);
        assert_eq!(ty_of("s = \"a\"", "s"), TyKind::Str);
        assert_eq!(ty_of("a = [1]", "a"), TyKind::Array);
    }

    #[test]
    fn a_local_reassigned_to_the_same_type_keeps_it() {
        assert_eq!(ty_of("x = 1\nx = 2", "x"), TyKind::Int);
    }

    /// `local_types` is read FLOW-INSENSITIVELY by codegen (one entry per
    /// local for the whole scope), so a local assigned two DIFFERENT types
    /// can only soundly be `Poly` everywhere. Overwriting instead
    /// miscompiled every read before the second assignment -- the corpus
    /// symptom was an `Arc<P>` boxed with class Q's identifier, an E0308 on
    /// the whole generated program.
    #[test]
    fn a_local_reassigned_to_a_different_type_widens_to_poly() {
        assert_eq!(ty_of("x = 1\nx = \"s\"", "x"), TyKind::Poly);
        assert_eq!(ty_of("x = \"s\"\nx = 1", "x"), TyKind::Poly);
        assert_eq!(ty_of("x = [1]\nx = {a: 1}", "x"), TyKind::Poly);
    }

    /// Two different CLASSES are different types for this purpose -- the
    /// exact shape that broke: each has its own generated Rust struct, so a
    /// single `Object(cid)` entry can't describe both.
    #[test]
    fn a_local_reassigned_to_a_different_class_widens_to_poly() {
        let source = "class A; end\nclass B; end\nx = A.new\nx = B.new";
        assert_eq!(ty_of(source, "x"), TyKind::Poly);
    }

    #[test]
    fn a_local_reassigned_to_the_same_class_keeps_that_class() {
        let source = "class A; end\nx = A.new\nx = A.new";
        assert!(matches!(ty_of(source, "x"), TyKind::Object(_)));
    }

    /// Widening is one-way: once Poly, a later same-type assignment doesn't
    /// narrow it back (the whole-scope entry must stay sound for the reads
    /// between the two assignments).
    #[test]
    fn widening_to_poly_is_not_undone_by_a_later_assignment() {
        assert_eq!(ty_of("x = 1\nx = \"s\"\nx = 2", "x"), TyKind::Poly);
    }

    /// Pre-existing behavior this must not disturb: a local assigned
    /// different types in different BRANCHES already widened via
    /// `join_branches`.
    #[test]
    fn branch_disagreement_still_widens_to_poly() {
        assert_eq!(ty_of("if true\n  x = 1\nelse\n  x = \"s\"\nend", "x"), TyKind::Poly);
    }

    #[test]
    fn branch_agreement_still_keeps_the_type() {
        assert_eq!(ty_of("if true\n  x = 1\nelse\n  x = 2\nend", "x"), TyKind::Int);
    }

    /// An assignment nested inside a call's arguments still registers.
    #[test]
    fn a_nested_assignment_is_tracked() {
        assert_eq!(ty_of("puts(y = 1)", "y"), TyKind::Int);
    }
}
