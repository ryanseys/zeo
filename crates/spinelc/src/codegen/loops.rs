//! Codegen for Phase 4's loop constructs (`while`/`until`/`loop`/`for`) and
//! their `break`/`next`/`redo` control flow, plus `a, b = ...` multi-assign.
//! See `hir.rs`'s docs for why `until` is folded into `While` (a `negate`
//! flag, exactly like `unless` folds into `If`) and why `loop do..end` isn't
//! its own `ruby-prism` node (a lowering-time call-shape desugar, mirroring
//! `define_method`).
//!
//! Every construct compiles to a labeled native Rust `loop { }` -- never a
//! bare Rust `while`/`for`, even for the simplest case -- so `break value`
//! always has an expression-position target to jump to (Rust's own `while`/
//! `for` are never expressions; only `loop { }` can carry a value out via
//! `break`). `redo` is the one construct with no direct Rust equivalent
//! (`continue` always re-tests/advances): each loop is actually TWO nested
//! native loops, an outer one that re-tests the condition/advances and an
//! inner "run the body exactly once, unless told otherwise" one -- `next`/
//! `break` `continue`/`break` the OUTER label, `redo` `continue`s the INNER
//! one, landing back at the top of the body with no re-test at all. This
//! same machinery also backs the pre-existing `.times` block-inlining
//! special case (see `codegen::call`), so `break`/`next`/`redo` work there
//! too, for free.

use quote::quote;

use super::expr::{emit_expr, infer};
use super::stmt::emit_body;
use super::Ctx;
use crate::hir::NodeId;
use crate::types::TyKind;
use proc_macro2::TokenStream;
use syn::Lifetime;

/// A fresh, function-body-unique loop label -- see `Ctx::label_counter`'s
/// docs for why a shared `Cell` beats threading a counter through every
/// `emit_*` function's return value.
pub(super) fn fresh_label(cx: &Ctx, tag: &str) -> Lifetime {
    let n = cx.label_counter.get();
    cx.label_counter.set(n + 1);
    Lifetime::new(&format!("'spinel_{tag}_{n}"), proc_macro2::Span::call_site())
}

/// Wraps `body` in the redo-supporting inner loop -- shared by every
/// construct below (and by `.times`'s block inlining in `codegen::call`).
/// `loop_cx` must already carry this loop's own `(redo_label, outer_label)`
/// pair (see `Ctx::loop_labels`).
pub(super) fn emit_redo_wrapped_body(loop_cx: &Ctx, body: &[NodeId], redo_label: &Lifetime) -> TokenStream {
    let body_val = emit_body(loop_cx, body, false);
    // `body_val` is a whole sequence of statements ending in a tail
    // expression, not one expression -- wrapped in its own `{ }` block (then
    // used as a statement) so it discards as a unit. A bare `#body_val;`
    // would instead splice a `;` after only the FIRST fragment, leaving the
    // tail as its own separate statement -- and when that tail happens to
    // be something like a plain `RubyValue::Nil` with no visible side
    // effect, `clippy`'s `path_statements` lint flags it.
    quote! {
        #redo_label: loop {
            { #body_val };
            break #redo_label;
        }
    }
}

/// `while cond ... end` / `until cond ... end` -- see `hir.rs`'s `While`
/// docs for the `negate` flag. The exit test runs at the TOP of the outer
/// loop (before the body), matching Ruby: a `while false` body never
/// executes at all.
pub fn emit_while(cx: &Ctx, cond: NodeId, body: &[NodeId], negate: bool) -> TokenStream {
    let outer = fresh_label(cx, "while");
    let redo = fresh_label(cx, "while_body");
    let loop_cx = cx.in_loop(redo.clone(), outer.clone());
    let cond_expr = emit_expr(&loop_cx, cond);
    let inner = emit_redo_wrapped_body(&loop_cx, body, &redo);
    let test = if negate {
        quote! { !(#cond_expr).truthy() }
    } else {
        quote! { (#cond_expr).truthy() }
    };
    quote! {
        #outer: loop {
            if !(#test) { break #outer spinel_rt::RubyValue::Nil; }
            #inner
        }
    }
}

/// `loop do ... end` -- see `hir.rs`'s `Loop` docs. No exit test of its own:
/// only `break` (or, once Phase 9 exists, an uncaught `raise`) ever ends it.
pub fn emit_loop(cx: &Ctx, body: &[NodeId]) -> TokenStream {
    let outer = fresh_label(cx, "loop");
    let redo = fresh_label(cx, "loop_body");
    let loop_cx = cx.in_loop(redo.clone(), outer.clone());
    let inner = emit_redo_wrapped_body(&loop_cx, body, &redo);
    quote! {
        #outer: loop {
            #inner
        }
    }
}

/// `for var in iterable ... end` -- see `hir.rs`'s `For` docs. The iterable
/// is evaluated exactly once (bound to `__iter`), before the loop starts, in
/// the OUTER (pre-loop) label scope -- matching Ruby's own single evaluation
/// and consistent with how `While`'s condition/`For`'s bounds are the only
/// things that run outside the redo-wrapped body. Only `Array`/`Range` are
/// supported (a clean codegen-time panic otherwise, spike scope, same
/// posture as `emit_call`'s own final panic). `var` is a plain reassignment,
/// not a `let` -- it's already declared `mut` in the enclosing scope's
/// hoisting prelude (see `codegen::hoisting`), since (unlike a block's own
/// params) Ruby's `for` doesn't introduce a new scope for its index
/// variable.
pub fn emit_for(cx: &Ctx, var: &str, iterable: NodeId, body: &[NodeId]) -> TokenStream {
    let outer = fresh_label(cx, "for");
    let redo = fresh_label(cx, "for_body");
    let iterable_ty = infer(cx, iterable);
    // Only a `Range`'s element type is provably `Int` -- an `Array`'s
    // elements aren't tracked individually, so `var` stays `Poly` there (see
    // `analyze::locals`' identical seeding for the same reason).
    let elem_ty = match iterable_ty {
        TyKind::Range => TyKind::Int,
        _ => TyKind::Poly,
    };
    let loop_cx = cx.in_loop(redo.clone(), outer.clone()).with_for_var(var, elem_ty);
    let inner = emit_redo_wrapped_body(&loop_cx, body, &redo);
    let iter_expr = emit_expr(cx, iterable);
    // Routed through `emit_local_write` (not a hardcoded reassignment)
    // because `var` might be captured by an escaping block somewhere in
    // `body` -- if so, `hoisting`'s prelude declares it as an
    // `Rc<RefCell<RubyValue>>`, and a bare `var_ident = ...` reassignment
    // would be a Rust type error against that, not just a semantic gap.
    let bind_array = super::hoisting::emit_local_write(cx, var, quote! { __iter[__idx].clone() });
    let bind_range = super::hoisting::emit_local_write(cx, var, quote! { spinel_rt::RubyValue::Int(__i) });

    match iterable_ty {
        TyKind::Array => quote! {
            {
                let __iter = (#iter_expr).as_array_unchecked().borrow().clone();
                let mut __idx: usize = 0;
                #outer: loop {
                    if __idx >= __iter.len() { break #outer spinel_rt::RubyValue::Nil; }
                    #bind_array
                    #inner
                    __idx += 1;
                }
            }
        },
        TyKind::Range => quote! {
            {
                let __iter = #iter_expr;
                let __exclusive = __iter.range_exclude_end();
                let __end = __iter.range_last().as_int_unchecked();
                let mut __i = __iter.range_first().as_int_unchecked();
                #outer: loop {
                    let __in_range = if __exclusive { __i < __end } else { __i <= __end };
                    if !__in_range { break #outer spinel_rt::RubyValue::Nil; }
                    #bind_range
                    #inner
                    __i += 1;
                }
            }
        },
        other => panic!("`for` requires an Array or Range iterable (spike scope), got {other:?}"),
    }
}

/// `break` / `break value` -- see `hir.rs`'s `Break` docs.
pub fn emit_break(cx: &Ctx, value: Option<NodeId>) -> TokenStream {
    let (_, outer) = cx
        .loop_labels
        .clone()
        .unwrap_or_else(|| panic!("`break` outside a supported loop construct (spike scope)"));
    let value_expr = match value {
        Some(v) => emit_expr(cx, v),
        None => quote! { spinel_rt::RubyValue::Nil },
    };
    quote! { break #outer #value_expr }
}

/// `next` / `next value` -- see `hir.rs`'s `Next` docs. The value (if any)
/// is still evaluated for its side effects before jumping, matching Ruby's
/// evaluation order, even though a native loop has nowhere meaningful to
/// send the result.
pub fn emit_next(cx: &Ctx, value: Option<NodeId>) -> TokenStream {
    let (_, outer) = cx
        .loop_labels
        .clone()
        .unwrap_or_else(|| panic!("`next` outside a supported loop construct (spike scope)"));
    match value {
        None => quote! { continue #outer },
        Some(v) => {
            let e = emit_expr(cx, v);
            quote! { { let _ = #e; continue #outer } }
        }
    }
}

/// `redo` -- see `hir.rs`'s `Redo` docs.
pub fn emit_redo(cx: &Ctx) -> TokenStream {
    let (redo, _) = cx
        .loop_labels
        .clone()
        .unwrap_or_else(|| panic!("`redo` outside a supported loop construct (spike scope)"));
    quote! { continue #redo }
}

/// The assignments produced by `a, b = 1, 2` / `a, *b, c = arr` -- shared
/// between `stmt.rs` (the primary, statement-position case) and `expr.rs`'s
/// minimal sub-expression fallback. Each target is a plain reassignment,
/// not a `let` -- every target name is already declared `mut` in the
/// enclosing scope's hoisting prelude (see `codegen::hoisting`'s docs for
/// why a fresh `let` here would silently fail to persist across loop
/// iterations); only `__elems`/`__before`/`__splat`/`__after` are genuine
/// fresh, transient scratch locals, unrelated to that concern. The
/// right-hand side must be statically `Array`-typed -- a clean codegen-time
/// panic otherwise, since there's no other dispatch to fall back to (spike
/// scope, same posture as `emit_call`'s final panic). See
/// `spinel_rt::multi_assign`'s docs for the destructuring rules this
/// implements.
pub fn emit_multi_write_lets(
    cx: &Ctx,
    before: &[String],
    splat: &Option<String>,
    after: &[String],
    value: NodeId,
) -> TokenStream {
    if infer(cx, value) != TyKind::Array {
        panic!(
            "multi-assignment requires an Array-typed right-hand side (spike scope), got {:?}",
            infer(cx, value)
        );
    }
    let value_expr = emit_expr(cx, value);
    let n_before = before.len();
    let n_after = after.len();
    let has_splat = splat.is_some();

    let bind_before = before.iter().enumerate().map(|(i, name)| {
        super::hoisting::emit_local_write(cx, name, quote! { __before[#i].clone() })
    });
    let bind_after = after.iter().enumerate().map(|(i, name)| {
        super::hoisting::emit_local_write(cx, name, quote! { __after[#i].clone() })
    });
    let bind_splat = splat.as_ref().map(|name| {
        super::hoisting::emit_local_write(
            cx,
            name,
            quote! { spinel_rt::RubyValue::Array(spinel_rt::array_new(__splat)) },
        )
    });

    quote! {
        let __elems = (#value_expr).as_array_unchecked().borrow().clone();
        let (__before, __splat, __after) = spinel_rt::multi_assign(&__elems, #n_before, #has_splat, #n_after);
        #(#bind_before)*
        #bind_splat
        #(#bind_after)*
    }
}
