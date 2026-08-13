//! Forward, single-pass local-variable type tracking -- not a whole-program
//! fixpoint. Walks a method/top-level body
//! in source order, refining each local's `TyKind` as it's assigned, so
//! `codegen` can resolve `x + y` to native `Int` arithmetic when `x`/`y` are
//! locals previously assigned an `Int`-typed value -- not just
//! literal-on-literal operands.
//!
//! Branches (`if`/`case`) are handled by `join_branches`: each
//! branch is walked against its own clone of the pre-branch map, and a local
//! keeps its type after the branch only if every branch's exit state agrees
//! on it (including a branch that never touches it at all, which "agrees"
//! with the pre-branch type by definition) -- any disagreement widens to
//! `Poly`, since which branch actually ran isn't known at compile time.
//! Loops will reuse the same join, treating "loop body ran zero
//! times" as one more branch to agree with.

#![warn(
    clippy::wildcard_enum_match_arm,
    reason = "swept: this module's HIR walks are exhaustive. Re-enabled because a\n    parent module's file-level allow is INHERITED by its submodules"
)]

use crate::compiler::{ClassId, Compiler};
use crate::compiler::{FMap, FSet};
use crate::hir::{ArrayElem, HirNode, NodeId};
use crate::types::{TyKind, infer_type_with_locals};
use std::collections::HashMap;

pub fn infer_locals(
    compiler: &Compiler,
    defining: Option<ClassId>,
    box_id: u32,
    body: &[NodeId],
) -> FMap<String, TyKind> {
    let mut locals = FMap::default();
    for &stmt in body {
        track_node(compiler, defining, box_id, &mut locals, stmt);
    }
    widen_nested_writes(compiler, body, &mut locals);
    locals
}

/// Widens to `Poly` every `Object`-typed local this scope assigns somewhere
/// OTHER than one of its own top-level statements.
///
/// `TyKind::Object` is what makes a local `LocalStorage::Shadowed`, whose
/// whole point is that the binding comes from a natural Rust `let` at the
/// assignment itself rather than from a hoisted declaration. That only works
/// when the `let` lands in the same Rust block as the reads. Write it inside
/// an `if` arm -- `if c; ws = W.new; else; ws = W.new; end; ws.show`, fifteen
/// lines of ordinary Ruby -- and the `let` is scoped to the arm, so every
/// read after the `if` names a binding that does not exist there (E0425).
/// Widening puts the local back on hoisted `RubyValue` storage, which is
/// declared once at the top of the scope and therefore always in scope. The
/// cost is dynamic dispatch on reads, the same trade `Params::bound_names`
/// takes for a rebound parameter.
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: only a top-level `x = <expr>` binds a name HERE. Every other\n    statement kind contributes exactly what `assigned_in` finds under it"
)]
fn widen_nested_writes(compiler: &Compiler, body: &[NodeId], locals: &mut FMap<String, TyKind>) {
    let mut nested = FSet::default();
    for &stmt in body {
        // A top-level `x = <expr>` binds `x` right here; anything `<expr>`
        // itself assigns is nested all the same. `x` joins them when its
        // value expression assigns at all, because a chained `a = (b =
        // W.new)` reads its value back THROUGH `b` -- so once `b` widens to
        // a boxed slot, so does what `a` receives.
        match &compiler.hir[stmt] {
            HirNode::LocalWrite(name, value) => {
                let inner = assigned_in(compiler, *value);
                if !inner.is_empty() {
                    nested.insert(name.clone());
                    nested.extend(inner);
                }
            }
            _ => nested.extend(assigned_in(compiler, stmt)),
        }
    }
    for name in nested {
        if matches!(locals.get(&name), Some(TyKind::Object(_))) {
            locals.insert(name, TyKind::Poly);
        }
    }
}

/// Additionally walks one more root (a parameter's default-value expression,
/// via `Params::default_ids`) into an already-built locals map -- see
/// `hir::Params`'s docs: a default can reference/assign a
/// local exactly like an ordinary body statement can, and `infer_locals`
/// alone never sees it (defaults aren't part of `body`).
pub fn track_extra(
    compiler: &Compiler,
    defining: Option<ClassId>,
    box_id: u32,
    locals: &mut FMap<String, TyKind>,
    id: NodeId,
) {
    track_node(compiler, defining, box_id, locals, id);
}

/// Every local name assigned anywhere under `root`, INCLUDING inside nested
/// blocks and `def`s -- an over-approximation on purpose: the only caller
/// uses it to widen types, where naming one local too many costs a little
/// dispatch speed and naming one too few is a miscompile.
fn assigned_in(compiler: &Compiler, root: NodeId) -> FSet<String> {
    let mut out = FSet::default();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let HirNode::LocalWrite(name, _) = &compiler.hir[id] {
            out.insert(name.clone());
        }
        if let HirNode::MultiWrite { targets, .. } = &compiler.hir[id] {
            targets.for_each_local_name(&mut |name| {
                out.insert(name.to_string());
            });
        }
        compiler.hir[id].for_each_child(&mut |c| stack.push(c));
    }
    out
}

/// Recurses into every sub-expression position a `LocalWrite` could appear
/// in (mirrors `analyze::collect_ivars`'s traversal shape exactly), so an
/// assignment nested inside a call's receiver/args/block -- not just a
/// bare top-level statement -- still updates the map before later
/// statements read it.
fn track_node(
    compiler: &Compiler,
    defining: Option<ClassId>,
    box_id: u32,
    locals: &mut FMap<String, TyKind>,
    id: NodeId,
) {
    match &compiler.hir[id] {
        // An `attach_function` wrapper body assigns no locals -- it
        // reads only its own params -- so there is nothing to track.
        HirNode::Ffi(_) => {}
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
        // PURE DESCENT: every child is tracked in this same scope and box, so
        // `for_each_child` expresses them all. Listed by variant rather than
        // behind a `_`, deliberately: this walk INFERS types, and a new
        // `HirNode` that binds or narrows a local must fail to compile here
        // rather than silently inherit plain descent.
        //
        // `Block` and `BoxScope` are absent on purpose -- the first walks only
        // its body (not its parameter defaults), the second re-homes its body
        // into a different box.
        HirNode::ClassVarWrite(..)
        | HirNode::IvarWrite(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::DynConstRead { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::Defined(_)
        | HirNode::And(..)
        | HirNode::Or(..)
        | HirNode::FlipFlop { .. }
        | HirNode::New { .. }
        | HirNode::SuperCall { .. }
        | HirNode::Call { .. }
        | HirNode::ArrayLit(_)
        | HirNode::HashLit(_)
        | HirNode::RangeLit { .. }
        | HirNode::StringLit(_)
        | HirNode::RegexpLit(..)
        | HirNode::Break(_)
        | HirNode::Next(_)
        | HirNode::Return(_)
        | HirNode::PreExec(_)
        | HirNode::Seq(_)
        | HirNode::Eval(_)
        | HirNode::Yield(_)
        | HirNode::Raise(..) => {
            compiler.hir[id].for_each_child(&mut |n| {
                track_node(compiler, defining, box_id, locals, n)
            });
        }
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
                for e in values {
                    let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                    track_node(compiler, defining, box_id, locals, *v);
                }
                branches.push(body);
            }
            branches.push(else_body);
            *locals = join_branches(compiler, defining, box_id, locals, &branches);
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
        HirNode::Redo | HirNode::BlockGiven | HirNode::SelfRef => {}
        HirNode::MultiWrite { targets, value } => {
            track_node(compiler, defining, box_id, locals, *value);
            targets.for_each_node(&mut |n| track_node(compiler, defining, box_id, locals, n));
            // Destructured targets' element types aren't tracked precisely
            // (zeo limitation) -- an arbitrary Array's element types are
            // unknown -- so each local-like target widens to `Poly`, same as
            // any other Array-`[]` read.
            targets.for_each_local_name(&mut |n| {
                locals.insert(n.to_string(), TyKind::Poly);
            });
        }
        HirNode::BoxScope { box_id: bx, body } => {
            for &s in body {
                track_node(compiler, defining, *bx, locals, s);
            }
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            track_node(compiler, defining, box_id, locals, *subject);
            let mut branch_locals: Vec<FMap<String, TyKind>> = Vec::new();
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
                    // OVERWRITES rather than fills in: inside the handler the
                    // slot holds the exception, whatever the name meant before.
                    // Leaving an earlier concrete type in place emitted the
                    // handler's `e.message` as a call on that type while the
                    // value was a runtime exception -- rustc caught it, but a
                    // name reused across a `rescue =>` is ordinary ruby.
                    b.insert(name.clone(), TyKind::Poly);
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
            // A `begin` body is emitted inside its OWN Rust closure (see
            // `codegen::exceptions::emit_begin`), while the rescue chain, the
            // `ensure`, and everything after the `begin` are emitted outside
            // it. An object-typed local takes its `let` from its own
            // assignment (`LocalStorage::Shadowed`), so a name first assigned
            // in there would be confined to that closure -- unreachable from
            // `ensure` (an E0425), or, if it was already bound outside,
            // silently shadowed so the outer read sees the stale value.
            // Widening to `Poly` puts it back in the hoisting prelude, where
            // one binding spans every clause. The cost is Path-1 dispatch on
            // those locals; the alternative is a wrong answer.
            for name in assigned_in(compiler, id) {
                locals.insert(name, TyKind::Poly);
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
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::ConstantVisibility { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::DefHook { .. }
        | HirNode::MethodRedefine { .. }
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
    before: &FMap<String, TyKind>,
    branches: &[&[NodeId]],
) -> FMap<String, TyKind> {
    let branch_locals: Vec<FMap<String, TyKind>> = branches
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
fn merge_locals(maps: Vec<FMap<String, TyKind>>) -> FMap<String, TyKind> {
    let keys: FSet<&String> = maps.iter().flat_map(HashMap::keys).collect();
    keys.into_iter()
        .map(|key| {
            let first = maps[0].get(key).copied();
            let agrees = maps.iter().all(|m| m.get(key).copied() == first);
            (
                key.clone(),
                if agrees {
                    first.unwrap_or(TyKind::Poly)
                } else {
                    TyKind::Poly
                },
            )
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
    before: &FMap<String, TyKind>,
    body: &[NodeId],
    seed: Option<(&crate::hir::MultiTarget, TyKind)>,
) -> FMap<String, TyKind> {
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
        assert_eq!(
            ty_of("if true\n  x = 1\nelse\n  x = \"s\"\nend", "x"),
            TyKind::Poly
        );
    }

    #[test]
    fn branch_agreement_still_keeps_the_type() {
        assert_eq!(
            ty_of("if true\n  x = 1\nelse\n  x = 2\nend", "x"),
            TyKind::Int
        );
    }

    /// An assignment nested inside a call's arguments still registers.
    #[test]
    fn a_nested_assignment_is_tracked() {
        assert_eq!(ty_of("puts(y = 1)", "y"), TyKind::Int);
    }

    /// A `begin` body compiles inside its own Rust closure while the rescue
    /// chain, the `ensure`, and everything after it compile outside it -- so an
    /// object-typed local, whose `let` comes from its own assignment, would be
    /// confined to that closure. `Poly` puts it in the hoisting prelude, where
    /// one binding spans every clause.
    #[test]
    fn a_local_assigned_inside_a_begin_widens_to_poly() {
        let src = "class A; end\nbegin\n  x = A.new\nensure\n  nil\nend\n";
        assert_eq!(ty_of(src, "x"), TyKind::Poly);
        let src = "class A; end\nbegin\n  raise 'x'\nrescue\n  y = A.new\nend\n";
        assert_eq!(ty_of(src, "y"), TyKind::Poly);
        let src = "class A; end\nbegin\n  a, b = A.new, A.new\nensure\n  nil\nend\n";
        assert_eq!(ty_of(src, "a"), TyKind::Poly);
        assert_eq!(ty_of(src, "b"), TyKind::Poly);
    }

    /// ...but a local assigned OUTSIDE the `begin` and merely read inside it
    /// keeps its type: its binding is already in the enclosing block.
    #[test]
    fn a_local_only_read_inside_a_begin_keeps_its_type() {
        let src = "class A; end\nz = A.new\nbegin\n  z\nensure\n  nil\nend\n";
        assert!(matches!(ty_of(src, "z"), TyKind::Object(_)));
    }
}
