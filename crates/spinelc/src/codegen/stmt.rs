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
    } else if let HirNode::MultiWrite {
        before,
        splat,
        after,
        value,
    } = &cx.compiler.hir[stmt]
    {
        // Same reasoning as `LocalWrite` above: each target's `let` must be
        // a plain top-level Rust statement (not nested in a sub-block) so
        // later statements in this same body can see it.
        let lets = super::loops::emit_multi_write_lets(cx, before, splat, after, *value);
        if is_tail {
            let nil = tail_nil(wrap_ok);
            quote! { #lets #nil }
        } else {
            quote! { #lets }
        }
    } else {
        let e = emit_expr(cx, stmt);
        if is_tail {
            if wrap_ok {
                quote! { Ok(#e) }
            } else {
                e
            }
        } else {
            quote! { #e; }
        }
    }
}
