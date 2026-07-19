//! Emits a sequence of Rust statements from an HIR body, ending in a tail
//! expression -- Ruby's own "last expression is the implicit return"
//! translates directly to a Rust block's tail-expression rule.
//!
//! `wrap_ok` distinguishes the two shapes a body can be used as: a whole
//! method/closure body (`wrap_ok: true`), whose block becomes the literal
//! body of a `-> Result<RubyValue, Signal>` function and so needs its tail
//! wrapped in `Ok(...)`; or a body spliced in as a plain `RubyValue`-typed
//! *value* inside some other expression (`wrap_ok: false` -- today only
//! `super`-inlining, later also `if`/`case` arms), which must stay a bare
//! `RubyValue` so it composes like any other `emit_expr` fragment.

use quote::quote;

use super::expr::emit_expr;
use super::Ctx;
use crate::hir::{HirNode, NodeId};
use proc_macro2::TokenStream;

pub fn emit_body(cx: &Ctx, body: &[NodeId], wrap_ok: bool) -> TokenStream {
    if body.is_empty() {
        return tail_nil(wrap_ok);
    }
    let last = body.len() - 1;
    let stmts = body
        .iter()
        .enumerate()
        .map(|(i, &stmt)| emit_statement(cx, stmt, i == last, wrap_ok));
    quote! { #(#stmts)* }
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
    let init_stmts = init.iter().map(|&s| emit_statement(cx, s, false, false));
    let tail_id = last[0];
    let tail = emit_statement(cx, tail_id, true, false);
    // A tail assignment appends its own trailing-nil value (already a
    // `RubyValue`, and the tokens aren't a single boxable expression).
    let tail = match &cx.compiler.hir[tail_id] {
        HirNode::LocalWrite(..) | HirNode::MultiWrite { .. } => tail,
        _ => super::expr::box_if_object_typed(cx, tail_id, tail),
    };
    quote! { #(#init_stmts)* #tail }
}

fn tail_nil(wrap_ok: bool) -> TokenStream {
    if wrap_ok {
        quote! { Ok(spinel_rt::RubyValue::Nil) }
    } else {
        quote! { spinel_rt::RubyValue::Nil }
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
    if let HirNode::LocalWrite(name, value) = &cx.compiler.hir[stmt] {
        let v = emit_expr(cx, *value);
        // Box an `Object`-typed RHS when `name`'s OWN storage disagrees (Tier
        // 0 fix #2) -- see `emit_expr::box_for_local_storage`'s docs.
        let v = super::expr::box_for_local_storage(cx, name, *value, v);
        // `emit_local_write` picks the right shape (plain reassignment,
        // shadowing `let`, or a `RefCell` store) for whichever storage
        // class `name` has -- see `codegen::hoisting::LocalStorage`'s docs.
        let write = super::hoisting::emit_local_write(cx, name, v);
        if is_tail {
            let nil = tail_nil(wrap_ok);
            quote! { #write #nil }
        } else {
            quote! { #write }
        }
    } else if let HirNode::MultiWrite { targets, value } = &cx.compiler.hir[stmt] {
        // Same reasoning as `LocalWrite` above: every target that's a plain
        // `Local` reassigns the SAME already-hoisted identifier as before,
        // so wrapping the destructuring scratch locals (`__elems`/`__before`/
        // `__splat`/`__after`) in their own nested block (see
        // `codegen::loops::emit_multi_target_group`) doesn't affect their
        // visibility to LATER statements in this same body at all.
        let write = super::loops::emit_multi_write(cx, targets, *value);
        if is_tail {
            let nil = tail_nil(wrap_ok);
            quote! { #write #nil }
        } else {
            quote! { #write }
        }
    } else {
        let e = emit_expr(cx, stmt);
        if is_tail {
            // `raise`/a bare `return`/`retry` already compile to a literal
            // Rust `return ...;` (see `codegen::expr::emit_raise`/`HirNode::
            // Return`'s docs, `codegen::exceptions::emit_retry`) -- a
            // diverging expression whose type (`!`) already unifies with
            // anything, so wrapping it in `Ok(...)` here would build an
            // `Ok(return ...)` that can never actually construct its `Ok`
            // (the `return` always exits first). Harmless in principle
            // (`!` coerces fine either way) but `rustc` flags the `Ok(...)`
            // call itself as unreachable -- skip the wrap for exactly these
            // three diverging shapes rather than accept the warning.
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
    matches!(node, HirNode::Raise(..) | HirNode::Return(_) | HirNode::Retry)
}
