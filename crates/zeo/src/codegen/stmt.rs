//! Emits a sequence of Rust statements from an HIR body, ending in a tail
//! expression -- Ruby's own "last expression is the implicit return"
//! translates directly to a Rust block's tail-expression rule.
//!
//! `wrap_ok` distinguishes the two shapes a body can be used as: a whole
//! method/closure body (`wrap_ok: true`), whose block becomes the literal
//! body of a `-> Result<RubyValue, Signal>` function and so needs its tail
//! wrapped in `Ok(...)`; or a body spliced in as a plain `RubyValue`-typed
//! *value* inside some other expression (`wrap_ok: false` -- `if`/ternary/
//! `case` arms, loop bodies, rescue arms), which must stay a bare
//! `RubyValue` so it composes like any other `emit_expr` fragment.

use quote::quote;

use super::Ctx;
use super::expr::emit_expr;
use crate::hir::{HirNode, NodeId};
use proc_macro2::TokenStream;

pub fn emit_body(cx: &Ctx, body: &[NodeId], wrap_ok: bool) -> TokenStream {
    if body.is_empty() {
        return tail_nil(wrap_ok);
    }
    let last = body.len() - 1;
    let mut prev_line = None;
    let stmts = body
        .iter()
        .enumerate()
        .map(|(i, &stmt)| {
            let tokens = emit_statement(cx, stmt, i == last, wrap_ok);
            stamp_line(cx, stmt, &mut prev_line, tokens, i == last)
        })
        .collect::<Vec<_>>();
    quote! { #(#stmts)* }
}

/// Prepend a `zeo_rt::set_line` stamp when this statement's source line
/// differs from the previous statement's -- what keeps the current
/// backtrace frame's line tracking execution (CRuby's per-frame PC line,
/// at statement granularity). A TAIL expression keeps its value by
/// wrapping in a block. Synthetic statements stamp nothing.
fn stamp_line(
    cx: &Ctx,
    stmt: NodeId,
    prev_line: &mut Option<u32>,
    tokens: TokenStream,
    is_tail: bool,
) -> TokenStream {
    let Some((_, line)) = crate::codegen::source_location(cx.compiler, stmt) else {
        return tokens;
    };
    if *prev_line == Some(line) {
        return tokens;
    }
    *prev_line = Some(line);
    if is_tail {
        quote! { { zeo_rt::set_line(#line); #tokens } }
    } else {
        quote! { zeo_rt::set_line(#line); #tokens }
    }
}

/// `emit_body(.., false)` whose VALUE is always a boxed `RubyValue` -- for
/// alternative bodies that must agree on one Rust type (if/ternary/case/
/// pattern arms; `infer` types those expressions `Poly`, so consumers
/// always expect `RubyValue`). Only an Object-typed tail expression needs
/// the boxing; every other tail already produces `RubyValue`.
pub fn emit_body_boxed(cx: &Ctx, body: &[NodeId]) -> TokenStream {
    if body.is_empty() {
        return tail_nil(false);
    }
    let (init, last) = body.split_at(body.len() - 1);
    let mut prev_line = None;
    let init_stmts = init
        .iter()
        .map(|&s| {
            let tokens = emit_statement(cx, s, false, false);
            stamp_line(cx, s, &mut prev_line, tokens, false)
        })
        .collect::<Vec<_>>();
    let tail_id = last[0];
    let tail = emit_statement(cx, tail_id, true, false);
    // A tail assignment's own arm already boxed its value to `RubyValue`
    // (see `emit_statement`); every other tail still needs Object-typed
    // boxing here.
    let tail = match &cx.compiler.hir[tail_id] {
        HirNode::LocalWrite(..) | HirNode::MultiWrite { .. } => tail,
        _ => super::expr::box_if_object_typed(cx, tail_id, tail),
    };
    let tail = stamp_line(cx, tail_id, &mut prev_line, tail, true);
    quote! { #(#init_stmts)* #tail }
}

fn tail_nil(wrap_ok: bool) -> TokenStream {
    if wrap_ok {
        quote! { Ok(zeo_rt::RubyValue::Nil) }
    } else {
        quote! { zeo_rt::RubyValue::Nil }
    }
}

/// One statement (or the tail expression) of a body. `LocalWrite`/
/// `MultiWrite` need special handling: they compile to a plain Rust
/// reassignment (`x = v;`, not `let x = v;` -- see `codegen::hoisting`'s
/// docs for why a fresh `let` here would silently fail to persist mutations
/// across loop iterations), whose Rust type is `()`, not `RubyValue`. In
/// tail position that has no value of its own to return, so a trailing nil
/// follows it (see `emit_expr`'s `LocalWrite`/`MultiWrite` arms for the
/// sub-expression case, which does return the assigned value, matching
/// Ruby's real assignment-as-expression semantics).
fn emit_statement(cx: &Ctx, stmt: NodeId, is_tail: bool, wrap_ok: bool) -> TokenStream {
    if let HirNode::ClassDef { .. } = &cx.compiler.hir[stmt] {
        // A class/module definition SITE: its body statements run right
        // here, in document order (real Ruby executes a class body where
        // it appears, re-running each reopen) -- see
        // `Compiler::class_body_sites`. Dispatch-table registration stays
        // hoisted in `fn main()`; only the body's execution moves. In tail
        // position the expression value is `nil` (real Ruby returns the
        // body's last value -- a documented narrow divergence).
        // A marker with NO registered site is a `class`/`module` the
        // analyze walk never reached (e.g. inside a top-level `begin`) --
        // the same catalogued gap that used to die in `emit_expr` as "a
        // top-level-only node in expression position"; keep it loud rather
        // than silently skipping the definition.
        let site_body = cx
            .compiler
            .class_body_sites
            .iter()
            .find(|s| s.def_node == Some(stmt))
            .map(|s| crate::codegen::emit_class_body_site(cx.compiler, s))
            .unwrap_or_else(|| {
                panic!(
                    "`class`/`module` in a position the analyze walk doesn't register \
                     (e.g. inside a top-level `begin`) isn't supported yet (zeo limitation)"
                )
            });
        return if is_tail {
            let nil = tail_nil(wrap_ok);
            quote! { #site_body #nil }
        } else {
            quote! { #site_body }
        };
    }
    if matches!(
        &cx.compiler.hir[stmt],
        HirNode::LocalWrite(..) | HirNode::MultiWrite { .. }
    ) {
        // In TAIL position an assignment evaluates to the assigned value,
        // exactly like Ruby's real "assignment-as-expression" semantics
        // (`def inc(v); v += 1; end` returns the incremented value): reuse
        // `emit_expr`'s write-then-read form, boxed to `RubyValue`. As a
        // NON-tail statement it compiles to a plain Rust reassignment
        // (`x = v;`, not a value-returning block) so the mutation persists
        // across loop iterations -- see `codegen::hoisting`'s docs.
        if is_tail {
            // Box the write-then-read value to `RubyValue`. For a `LocalWrite`
            // this keys on the LOCAL's storage (a `nil | Foo` union local
            // already reads back as `RubyValue`, so must not be re-boxed --
            // see `box_tail_local_write`); a `MultiWrite` yields its RHS array,
            // already a `RubyValue`.
            let value = match &cx.compiler.hir[stmt] {
                HirNode::LocalWrite(name, _) => {
                    super::expr::box_tail_local_write(cx, name, emit_expr(cx, stmt))
                }
                _ => emit_expr(cx, stmt),
            };
            return if wrap_ok {
                quote! { Ok(#value) }
            } else {
                value
            };
        }
        if let HirNode::LocalWrite(name, value) = &cx.compiler.hir[stmt] {
            let v = emit_expr(cx, *value);
            // Box an `Object`-typed RHS when `name`'s OWN storage disagrees
            // (Tier 0 fix #2) -- see `emit_expr::box_for_local_storage`'s docs.
            let v = super::expr::box_for_local_storage(cx, name, *value, v);
            // `emit_local_write` picks the right shape (plain reassignment,
            // shadowing `let`, or a `RefCell` store) for whichever storage
            // class `name` has -- see `codegen::hoisting::LocalStorage`'s docs.
            let write = super::hoisting::emit_local_write(cx, name, v);
            quote! { #write }
        } else {
            let HirNode::MultiWrite { targets, value } = &cx.compiler.hir[stmt] else {
                unreachable!()
            };
            // Same reasoning as `LocalWrite` above: every target that's a
            // plain `Local` reassigns the SAME already-hoisted identifier as
            // before, so wrapping the destructuring scratch locals
            // (`__elems`/`__before`/`__splat`/`__after`) in their own nested
            // block (see `codegen::loops::emit_multi_target_group`) doesn't
            // affect their visibility to LATER statements in this body at all.
            let write = super::loops::emit_multi_write(cx, targets, *value);
            quote! { #write }
        }
    } else {
        let e = emit_expr(cx, stmt);
        if is_tail {
            // `raise`/a bare `return`/`retry`/`break`/`next`/`redo` all
            // compile to a literal diverging Rust statement -- `return ...;`
            // (`codegen::expr::emit_raise`/`HirNode::Return`'s docs,
            // `codegen::exceptions::emit_retry`), or a labeled `break`/
            // `continue`/`return Err(Signal::..)` (`codegen::loops`). Their
            // type (`!`) already unifies with anything, so wrapping in
            // `Ok(...)` here would build an `Ok(break ...)`/`Ok(return ...)`
            // that can never actually construct its `Ok` (the jump always
            // exits first). Harmless in principle (`!` coerces fine either
            // way) but `rustc` flags the `Ok(...)` call itself as unreachable
            // -- skip the wrap for exactly these diverging shapes rather than
            // accept the warning. (A tail `break`/`next`/`redo` reaches here
            // once its clause is inside a `begin`'s closure -- see
            // `codegen::exceptions`.)
            if wrap_ok && !is_diverging_tail(&cx.compiler.hir[stmt]) {
                // Box a bare tail `New`/`SelfRef`/`Shadowed`-local-read into
                // `RubyValue::Object` before wrapping -- this whole body's
                // enclosing function returns `Result<RubyValue, Signal>`,
                // but those three shapes emit an unboxed `Arc<Concrete>`
                // (see `codegen::expr::box_for_tail_return`'s docs).
                let e = super::expr::box_for_tail_return(cx, stmt, e);
                quote! { Ok(#e) }
            } else {
                e
            }
        } else {
            quote! { #e; }
        }
    }
}

fn is_diverging_tail(node: &HirNode) -> bool {
    matches!(
        node,
        HirNode::Raise(..)
            | HirNode::Return(_)
            | HirNode::Retry
            | HirNode::Break(_)
            | HirNode::Next(_)
            | HirNode::Redo
    )
}
