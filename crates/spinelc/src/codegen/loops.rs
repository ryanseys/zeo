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
use crate::hir::{MultiTarget, MultiTargetGroup, NodeId};
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
    let cond_expr = {
        let e = emit_expr(&loop_cx, cond);
        super::expr::box_if_object_typed(&loop_cx, cond, e)
    };
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
pub fn emit_for(cx: &Ctx, target: &MultiTarget, iterable: NodeId, body: &[NodeId]) -> TokenStream {
    let outer = fresh_label(cx, "for");
    let redo = fresh_label(cx, "for_body");
    let iterable_ty = infer(cx, iterable);
    // Only a `Range`'s element type is provably `Int`, and only when `target`
    // is a single plain local -- an `Array`'s elements aren't tracked
    // individually, nor is a destructured `for a, b in ...`'s, so those stay
    // `Poly` (see `analyze::locals`' identical seeding for the same reason).
    let elem_ty = match (target, iterable_ty) {
        (MultiTarget::Local(_), TyKind::Range) => TyKind::Int,
        _ => TyKind::Poly,
    };
    let loop_cx = cx.in_loop(redo.clone(), outer.clone());
    let loop_cx = match target {
        MultiTarget::Local(name) => loop_cx.with_for_var(name, elem_ty),
        _ => loop_cx,
    };
    let inner = emit_redo_wrapped_body(&loop_cx, body, &redo);
    let iter_expr = emit_expr(cx, iterable);
    // Routed through `emit_target_write` (not a hardcoded reassignment)
    // because a target name might be captured by an escaping block somewhere
    // in `body` -- if so, `hoisting`'s prelude declares it as an
    // `Arc<parking_lot::Mutex<RubyValue>>`, and a bare `var_ident = ...`
    // reassignment would be a Rust type error against that, not just a
    // semantic gap.
    let bind_array = emit_target_write(cx, target, quote! { __iter[__idx].clone() });
    let bind_range = emit_target_write(cx, target, quote! { spinel_rt::RubyValue::Int(__i) });

    // `for` evaluates to the collection it iterated (CRuby: `for x in c; end`
    // returns `c`, the same object), unless the body `break`s with a value.
    // `__coll` holds that collection once; each arm derives its iteration
    // state from it and breaks with `__coll.clone()` on natural completion.
    match iterable_ty {
        TyKind::Array => quote! {
            {
                let __coll = #iter_expr;
                let __iter = __coll.as_array_unchecked().lock().clone();
                let mut __idx: usize = 0;
                #outer: loop {
                    if __idx >= __iter.len() { break #outer __coll.clone(); }
                    #bind_array
                    #inner
                    __idx += 1;
                }
            }
        },
        TyKind::Range => quote! {
            {
                let __coll = #iter_expr;
                let __exclusive = __coll.range_exclude_end();
                let __end = __coll.range_last().as_int_unchecked();
                let mut __i = __coll.range_first().as_int_unchecked();
                #outer: loop {
                    let __in_range = if __exclusive { __i < __end } else { __i <= __end };
                    if !__in_range { break #outer __coll.clone(); }
                    #bind_range
                    #inner
                    __i += 1;
                }
            }
        },
        // `for k, v in hash` / `for pair in hash`: iterate the pairs as
        // `[k, v]` arrays and reuse the Array arm's element-write, which the
        // shared `emit_target_write` destructures for a nested target and
        // binds whole for a single one -- CRuby's `Hash#each` shape.
        TyKind::Hash => quote! {
            {
                let __coll = #iter_expr;
                let __iter: Vec<spinel_rt::RubyValue> = __coll
                    .as_hash_unchecked()
                    .lock()
                    .values()
                    .map(|(__k, __v)| {
                        spinel_rt::RubyValue::Array(spinel_rt::array_new(vec![__k.clone(), __v.clone()]))
                    })
                    .collect();
                let mut __idx: usize = 0;
                #outer: loop {
                    if __idx >= __iter.len() { break #outer __coll.clone(); }
                    #bind_array
                    #inner
                    __idx += 1;
                }
            }
        },
        // ANY other iterable -- a user class with `each`, a Poly local, an
        // Enumerator. This is the GENERAL case, not a fallback: real Ruby's
        // `for` performs no type dispatch whatsoever. `compile_iter`
        // (compile.c:8548) emits `ADD_SEND_WITH_BLOCK(..., idEach, ...)`, so
        // `for x in obj` simply *is* `obj.each { |x| ... }`, and an iterable
        // that answers no `each` raises NoMethodError at RUNTIME. The
        // Array/Range/Hash arms above are fast paths over this, which is why
        // an unrecognized type must reach here rather than fail the compile.
        //
        // Elements are collected up front so the body keeps the labelled
        // `break`/`next` shape the other arms rely on -- the same snapshot
        // semantics the Array and Hash arms already have.
        _ => {
            let coll = super::expr::box_if_object_typed(cx, iterable, iter_expr);
            quote! {
                {
                    let __coll = #coll;
                    let __iter = spinel_rt::each_values(&__coll)?;
                    let mut __idx: usize = 0;
                    #outer: loop {
                        if __idx >= __iter.len() { break #outer __coll.clone(); }
                        #bind_array
                        #inner
                        __idx += 1;
                    }
                }
            }
        }
    }
}

/// `break` / `break value` -- see `hir.rs`'s `Break` docs. When there's no
/// enclosing NATIVE loop (`cx.loop_labels` is `None`) but this code is
/// inside a real escaping `Proc`'s own body (`cx.in_real_proc`), `break`
/// instead raises `Signal::Break` -- it must unwind all the way out of the
/// closure, back to whichever call site originally attached this block (see
/// `codegen::params::emit_call_args`'s `catch_break` wrapping), exactly
/// matching real Ruby: `arr.each { break }` makes the WHOLE `.each(...)`
/// call evaluate to the break value, not just the block invocation.
pub fn emit_break(cx: &Ctx, value: Option<NodeId>) -> TokenStream {
    let value_expr = match value {
        Some(v) => emit_expr(cx, v),
        None => quote! { spinel_rt::RubyValue::Nil },
    };
    match &cx.loop_labels {
        Some((_, outer)) => quote! { break #outer #value_expr },
        None if cx.in_real_proc => quote! { return Err(spinel_rt::Signal::Break(#value_expr)) },
        None => panic!("`break` outside a supported loop construct (spike scope)"),
    }
}

/// `next` / `next value` -- see `hir.rs`'s `Next` docs. The value (if any)
/// is still evaluated for its side effects before jumping, matching Ruby's
/// evaluation order, even though a native loop has nowhere meaningful to
/// send the result. Inside a real `Proc` with no enclosing native loop,
/// raises `Signal::Next` instead -- caught inside the closure's own
/// redo-wrapper loop (see `codegen::call`'s Proc-construction docs), never
/// escaping past `Proc::call` itself.
pub fn emit_next(cx: &Ctx, value: Option<NodeId>) -> TokenStream {
    match &cx.loop_labels {
        Some((_, outer)) => match value {
            None => quote! { continue #outer },
            Some(v) => {
                let e = emit_expr(cx, v);
                quote! { { let _ = #e; continue #outer } }
            }
        },
        None if cx.in_real_proc => {
            let value_expr = match value {
                Some(v) => emit_expr(cx, v),
                None => quote! { spinel_rt::RubyValue::Nil },
            };
            quote! { return Err(spinel_rt::Signal::Next(#value_expr)) }
        }
        None => panic!("`next` outside a supported loop construct (spike scope)"),
    }
}

/// `redo` -- see `hir.rs`'s `Redo` docs. Inside a real `Proc` with no
/// enclosing native loop, raises `Signal::Redo` -- caught (and acted on) by
/// the closure's own redo-wrapper loop, same as `Next`.
pub fn emit_redo(cx: &Ctx) -> TokenStream {
    match &cx.loop_labels {
        Some((redo, _)) => quote! { continue #redo },
        None if cx.in_real_proc => quote! { return Err(spinel_rt::Signal::Redo) },
        None => panic!("`redo` outside a supported loop construct (spike scope)"),
    }
}

/// Writes `value` (an already-emitted Rust expression) into `target` -- the
/// one place every multi-assignment/`for`-loop target kind's write
/// semantics live, shared by `emit_multi_target_group` (below) and
/// `emit_for`. `Local` reuses `hoisting::emit_local_write` (so a captured
/// local's write goes through the exact same guard-hygiene it always does);
/// `Ivar`/`ClassVar`/`Global`/`Const` are direct storage writes; `Call`
/// (`obj.attr = ...`/`arr[i] = ...`) binds `value` into the write's own
/// pre-built synthetic hidden local FIRST, then emits the write CALL itself
/// through the ordinary `emit_expr` path -- reusing the exact same
/// static/dynamic dispatch `codegen::call::dispatch` already provides for
/// every other call, with no bespoke attr/index-write codegen needed here
/// at all (see `hir::MultiTarget::Call`'s docs). `Nested` recurses into
/// `emit_multi_target_group`, further destructuring `value` itself.
pub fn emit_target_write(cx: &Ctx, target: &MultiTarget, value: TokenStream) -> TokenStream {
    match target {
        MultiTarget::Local(name) => super::hoisting::emit_local_write(cx, name, value),
        MultiTarget::Ivar(name) => super::expr::emit_ivar_write_stmt(cx, name, value),
        MultiTarget::ClassVar(name) => super::expr::emit_cvar_write_stmt(cx, name, value),
        MultiTarget::Global(name) => {
            let bx = cx.box_id;
            quote! { spinel_rt::global_set(#bx, #name, #value); }
        }
        MultiTarget::Const(name) => super::expr::emit_const_write_stmt(cx, None, name, value),
        MultiTarget::ScopedConst { scope, name } => {
            super::expr::emit_const_write_stmt(cx, Some(scope), name, value)
        }
        MultiTarget::Call { write_call, tmp_name } => {
            let bind = super::hoisting::emit_local_write(cx, tmp_name, value);
            let call = emit_expr(cx, *write_call);
            quote! { #bind let _ = #call; }
        }
        MultiTarget::Nested(group) => emit_multi_target_group(cx, group, value),
    }
}

/// Destructures `value_expr` (an Array-typed Rust expression) against
/// `group`'s before/splat/after shape via `spinel_rt::multi_assign`, writing
/// each target through `emit_target_write` -- the recursive core both
/// `HirNode::MultiWrite` and a nested `MultiTarget::Nested` group reduce to.
/// Only `__elems`/`__before`/`__splat`/`__after` are genuine fresh,
/// transient scratch locals; every actual target write goes through
/// `emit_target_write`'s own storage-appropriate rules. See
/// `spinel_rt::multi_assign`'s docs for the destructuring rules this
/// implements.
pub fn emit_multi_target_group(cx: &Ctx, group: &crate::hir::MultiTargetGroup, value_expr: TokenStream) -> TokenStream {
    let n_before = group.before.len();
    let n_after = group.after.len();
    let has_splat = group.splat.is_some();

    let bind_before = group
        .before
        .iter()
        .enumerate()
        .map(|(i, t)| emit_target_write(cx, t, quote! { __before[#i].clone() }));
    let bind_after = group
        .after
        .iter()
        .enumerate()
        .map(|(i, t)| emit_target_write(cx, t, quote! { __after[#i].clone() }));
    let bind_splat = match &group.splat {
        Some(Some(t)) => Some(emit_target_write(
            cx,
            t,
            quote! { spinel_rt::RubyValue::Array(spinel_rt::array_new(__splat)) },
        )),
        _ => None,
    };

    quote! {
        {
            let __elems = (#value_expr).as_array_unchecked().lock().clone();
            let (__before, __splat, __after) = spinel_rt::multi_assign(&__elems, #n_before, #has_splat, #n_after);
            #(#bind_before)*
            #bind_splat
            #(#bind_after)*
        }
    }
}

/// The top-level entry for `a, b = 1, 2` / `a, *b, c = arr`. A statically
/// `Array`-typed right-hand side destructures directly; anything else gets
/// real Ruby's implicit conversion at RUNTIME -- an Array passes through,
/// every other value destructures as the single-element `[value]` (the
/// `to_ary` rule for values with no `to_ary` of their own; a user-defined
/// `to_ary` isn't consulted, a documented approximation).
pub fn emit_multi_write(cx: &Ctx, targets: &MultiTargetGroup, value: NodeId) -> TokenStream {
    let value_expr = {
        let e = emit_expr(cx, value);
        super::expr::box_if_object_typed(cx, value, e)
    };
    let value_expr = if infer(cx, value) == TyKind::Array {
        value_expr
    } else {
        // A non-Array right-hand side destructures through `to_ary` when it
        // defines one (`x, y = pair_object`), and otherwise binds as a
        // single value with the remaining targets nil-filled (`c, d = 5` ->
        // `[5, nil]`) -- CRuby's `rb_check_array_type` rule, the same one
        // block auto-splat uses, so `block_auto_splat` is the shared
        // implementation (it answers the input unchanged when no coercion
        // applies, which is exactly the one-element case here).
        quote! {
            spinel_rt::RubyValue::Array(spinel_rt::array_new(
                spinel_rt::block_auto_splat(vec![#value_expr])?
            ))
        }
    };
    emit_multi_target_group(cx, targets, value_expr)
}
