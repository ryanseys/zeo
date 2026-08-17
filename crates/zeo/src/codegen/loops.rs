//! Codegen for loop constructs (`while`/`until`/`loop`/`for`) and
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

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use quote::quote;

use super::Ctx;
use super::expr::{emit_expr, infer};
use crate::hir::{HirNode, MultiTarget, MultiTargetGroup, NodeId};
use crate::types::TyKind;
use proc_macro2::TokenStream;
use syn::Lifetime;

/// Whether `for x in <this>` may count with native integers: a range LITERAL
/// whose begin is provably `Int` and whose end is `Int` or ABSENT (an endless
/// range still yields Ints -- it just never stops on its own).
///
/// Deliberately syntactic: only a literal exposes its endpoints to inference.
/// A range reaching the loop through a variable answers `false` and takes the
/// runtime-dispatched arm, which decides the same question from the value.
/// The two must agree on what "countable" means, because this predicate also
/// decides whether the loop variable is typed `Int`.
fn counts_as_ints(cx: &Ctx, iterable: NodeId) -> bool {
    match cx.compiler.hir[iterable] {
        HirNode::RangeLit {
            start: Some(s),
            end,
            ..
        } => {
            matches!(infer(cx, s), TyKind::Int)
                && end.is_none_or(|e| matches!(infer(cx, e), TyKind::Int))
        }
        _ => false,
    }
}

/// A fresh, function-body-unique loop label -- see `Ctx::label_counter`'s
/// docs for why a shared `Cell` beats threading a counter through every
/// `emit_*` function's return value.
pub(super) fn fresh_label(cx: &Ctx, tag: &str) -> Lifetime {
    let n = cx.label_counter.get();
    cx.label_counter.set(n + 1);
    // The tag is often a Ruby method name, and Ruby method names carry
    // characters a Rust lifetime cannot (`all?`, `map!`). The counter is what
    // makes the label unique; the tag only makes the emitted loop readable, so
    // folding those characters to `_` costs nothing. Without it, fusing
    // `Array#all?` made `syn::Lifetime::new` panic mid-compile.
    let tag: String = tag
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    Lifetime::new(&format!("'zeo_{tag}_{n}"), proc_macro2::Span::call_site())
}

/// Wraps `body` in the redo-supporting inner loop -- shared by every
/// construct below (and by `.times`'s block inlining in `codegen::call`).
/// `loop_cx` must already carry this loop's own `(redo_label, outer_label)`
/// pair (see `Ctx::loop_labels`).
/// `emit_redo_wrapped_body`'s VALUE twin, for the value-consuming iterator
/// splices (`map`/`select`/`sum`/...): the inner redo loop is an EXPRESSION
/// evaluating to the iteration's value -- the body's tail on a normal pass,
/// or whatever a value-mode `next` broke out with (`emit_next`'s
/// `next_yields_value` arm). `redo` still re-enters the label, so the value
/// that finally surfaces is the re-run's, exactly once per element.
pub(super) fn emit_redo_wrapped_body_value(
    loop_cx: &Ctx,
    body: &[NodeId],
    redo_label: &Lifetime,
) -> TokenStream {
    let body_val = super::stmt::emit_body(loop_cx, body, false);
    // Parenthesized: a brace-block value after a labeled `break` trips
    // rustc's confusability lint (`break 'l { .. }` reads like an unlabeled
    // break of a labeled block).
    quote! {
        #redo_label: loop {
            zeo_rt::check_ints()?;
            break #redo_label ({ #body_val });
        }
    }
}

pub(super) fn emit_redo_wrapped_body(
    loop_cx: &Ctx,
    body: &[NodeId],
    redo_label: &Lifetime,
) -> TokenStream {
    let body_val = super::stmt::emit_body_discard(loop_cx, body);
    // `body_val` is a whole sequence of statements ending in a tail
    // expression, not one expression -- wrapped in its own `{ }` block (then
    // used as a statement) so it discards as a unit. A bare `#body_val;`
    // would instead splice a `;` after only the FIRST fragment, leaving the
    // tail as its own separate statement -- and when that tail happens to
    // be something like a plain `RubyValue::Nil` with no visible side
    // effect, `clippy`'s `path_statements` lint flags it.
    // The interruption checkpoint at every back-edge: one relaxed load per
    // iteration (see `zeo_rt::check_ints`), which is what makes a busy
    // `while`/`loop`/`for`/`.times` body killable by `Thread#kill`/`#raise`.
    // Placed inside the redo loop so a `redo`-repeated iteration checks too.
    quote! {
        #redo_label: loop {
            zeo_rt::check_ints()?;
            { #body_val };
            break #redo_label;
        }
    }
}

/// `while cond ... end` / `until cond ... end` -- see `hir.rs`'s `While`
/// docs for the `negate` flag. The exit test runs at the TOP of the outer
/// loop (before the body), matching Ruby: a `while false` body never
/// executes at all.
pub fn emit_while(
    cx: &Ctx,
    cond: NodeId,
    body: &[NodeId],
    negate: bool,
    post: bool,
) -> TokenStream {
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
    if post {
        // `begin ... end while cond`: POST-test -- the body runs once before
        // the first check. The test sits at the TOP but is skipped on the
        // first pass, so `next` (a `continue #outer`) still re-evaluates the
        // condition, while `redo` (the inner label) re-runs the body only.
        quote! {
            {
                let mut __post_first = true;
                #outer: loop {
                    if !__post_first && !(#test) { break #outer zeo_rt::RubyValue::Nil; }
                    __post_first = false;
                    #inner
                }
            }
        }
    } else {
        quote! {
            #outer: loop {
                if !(#test) { break #outer zeo_rt::RubyValue::Nil; }
                #inner
            }
        }
    }
}

/// `loop do ... end` -- see `hir.rs`'s `Loop` docs. No exit test of its own:
/// only `break` (or an uncaught `raise`) ever ends it.
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
/// supported (a clean codegen-time panic otherwise, zeo limitation, same
/// posture as `emit_call`'s own final panic). `var` is a plain reassignment,
/// not a `let` -- it's already declared `mut` in the enclosing scope's
/// hoisting prelude (see `codegen::hoisting`), since (unlike a block's own
/// params) Ruby's `for` doesn't introduce a new scope for its index
/// variable.
pub fn emit_for(cx: &Ctx, target: &MultiTarget, iterable: NodeId, body: &[NodeId]) -> TokenStream {
    let outer = fresh_label(cx, "for");
    let redo = fresh_label(cx, "for_body");
    let iterable_ty = infer(cx, iterable);
    let counted = counts_as_ints(cx, iterable);
    // A `Range`'s element type is provably `Int` only when its ENDPOINTS are,
    // and only when `target` is a single plain local -- an `Array`'s elements
    // aren't tracked individually, nor is a destructured `for a, b in ...`'s,
    // so those stay `Poly` (see `analyze::locals`' identical seeding for the
    // same reason).
    //
    // `("a".."c")` is a Range too, and yields Strings. Typing its loop
    // variable `Int` on the strength of the receiver's class alone bound a
    // String into an Int slot, and the counted lowering below then read its
    // endpoints with `as_int_unchecked` and PANICKED the program -- on a
    // perfectly ordinary ruby loop.
    let elem_ty = match (target, iterable_ty) {
        (MultiTarget::Local(_), TyKind::Range) if counted => TyKind::Int,
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
    let bind_array = emit_target_write(cx, target, quote! { __iter[__idx].clone() }, None);
    let bind_elem = emit_target_write(cx, target, quote! { __elem }, None);
    let bind_range = emit_target_write(cx, target, quote! { zeo_rt::RubyValue::Int(__i) }, None);

    // `for` evaluates to the collection it iterated (CRuby: `for x in c; end`
    // returns `c`, the same object), unless the body `break`s with a value.
    // `__coll` holds that collection once; each arm derives its iteration
    // state from it and breaks with `__coll.clone()` on natural completion.
    match iterable_ty {
        // The LIVE array, not a snapshot: CRuby's `for` re-reads the length
        // every step and the element at the current index, so an append
        // during iteration is visited, a `pop` shortens the walk, and a
        // rewritten upcoming slot is seen (all oracle-verified). One lock
        // round-trip per step, released before the body runs -- the body may
        // mutate the receiver, and the payload Mutex is not reentrant.
        TyKind::Array => quote! {
            {
                let __coll = #iter_expr;
                let mut __idx: usize = 0;
                // The step is at the TOP so `next` (a `continue #outer`) still
                // advances -- a bottom step is skipped by `next`, spinning
                // forever. `redo` continues the INNER label and never reaches here.
                let mut __first = true;
                #outer: loop {
                    if !__first { __idx += 1; }
                    __first = false;
                    let __elem = {
                        let __g = __coll.as_array_ref().lock();
                        match __g.get(__idx) {
                            Some(__e) => __e.clone(),
                            None => break #outer __coll.clone(),
                        }
                    };
                    #bind_elem
                    #inner
                }
            }
        },
        // A statically Int-bounded range literal: count natively, no dispatch.
        // An ENDLESS one counts too -- it simply has no upper test, so only
        // `break` ends it, exactly as ruby's does. Reading that absent end as
        // an Int (`as_int_unchecked` on the `nil`) used to panic before the
        // first iteration.
        TyKind::Range if counted => quote! {
            {
                let __coll = #iter_expr;
                let __exclusive = __coll.range_exclude_end();
                let __end_v = __coll.range_last();
                let __endless = matches!(__end_v, zeo_rt::RubyValue::Nil);
                let __end = if __endless { 0 } else { __end_v.as_int_unchecked() };
                let mut __i = __coll.range_first().as_int_unchecked();
                // Top-of-loop step so `next` advances (see the Array arm).
                let mut __first = true;
                #outer: loop {
                    if !__first { __i += 1; }
                    __first = false;
                    let __in_range = __endless
                        || if __exclusive { __i < __end } else { __i <= __end };
                    if !__in_range { break #outer __coll.clone(); }
                    #bind_range
                    #inner
                }
            }
        },
        // `for k, v in hash` / `for pair in hash`: iterate the pairs
        // snapshot -- CRuby's `Hash#each` shape, and the same snapshot rule
        // zeo's own `Hash#each` runs under.
        TyKind::Hash => {
            // The two-plain-target spelling binds the pair halves DIRECTLY;
            // boxing a fresh `[k, v]` Array per pair existed only to reuse
            // the generic destructure, an allocation per entry.
            if let MultiTarget::Nested(g) = target
                && g.splat.is_none()
                && g.after.is_empty()
                && g.before.len() == 2
            {
                let bind_k =
                    emit_target_write(cx, &g.before[0], quote! { __iter[__idx].0.clone() }, None);
                let bind_v =
                    emit_target_write(cx, &g.before[1], quote! { __iter[__idx].1.clone() }, None);
                return quote! {
                    {
                        let __coll = #iter_expr;
                        let __iter = zeo_rt::hash_pairs_snapshot(__coll.as_hash_ref());
                        let mut __idx: usize = 0;
                        // Top-of-loop step so `next` advances (see the Array arm).
                        let mut __first = true;
                        #outer: loop {
                            if !__first { __idx += 1; }
                            __first = false;
                            if __idx >= __iter.len() { break #outer __coll.clone(); }
                            #bind_k
                            #bind_v
                            #inner
                        }
                    }
                };
            }
            quote! {
                {
                    let __coll = #iter_expr;
                    let __iter: Vec<zeo_rt::RubyValue> = __coll
                        .as_hash_ref()
                        .lock()
                        .values()
                        .map(|(__k, __v)| {
                            zeo_rt::RubyValue::Array(zeo_rt::array_new(vec![__k.clone(), __v.clone()]))
                        })
                        .collect();
                    let mut __idx: usize = 0;
                    // Top-of-loop step so `next` advances (see the Array arm).
                    let mut __first = true;
                    #outer: loop {
                        if !__first { __idx += 1; }
                        __first = false;
                        if __idx >= __iter.len() { break #outer __coll.clone(); }
                        #bind_array
                        #inner
                    }
                }
            }
        }
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
                    // A RANGE reaches here whenever it was not statically
                    // known to be one -- through a Poly local, a method
                    // return, a rescue-widened variable. Collecting it is
                    // wrong twice over: an ENDLESS range never finishes, and
                    // `as_int_unchecked` on the absent end used to panic. So
                    // the value is asked, and a countable range counts.
                    //
                    // Everything else keeps the collect: `for` is `each`, and
                    // the snapshot is what lets the body keep the labelled
                    // `break`/`next`/`redo` shape the other arms rely on.
                    let __is_range = matches!(__coll, zeo_rt::RubyValue::Range(_));
                    let __begin = if __is_range { __coll.range_first() } else { zeo_rt::RubyValue::Nil };
                    let __end_v = if __is_range { __coll.range_last() } else { zeo_rt::RubyValue::Nil };
                    let __endless = __is_range && matches!(__end_v, zeo_rt::RubyValue::Nil);
                    let __counted = __is_range
                        && matches!(__begin, zeo_rt::RubyValue::Int(_))
                        && (__endless || matches!(__end_v, zeo_rt::RubyValue::Int(_)));
                    let __exclusive = __is_range && __coll.range_exclude_end();
                    let __end = if __counted && !__endless { __end_v.as_int_unchecked() } else { 0 };
                    let mut __i = if __counted { __begin.as_int_unchecked() } else { 0 };
                    // `each_values` raises for a beginless or Float range
                    // exactly where ruby's `Range#each` does, so this `?` is
                    // ruby's own TypeError.
                    let __iter: Vec<zeo_rt::RubyValue> =
                        if __counted { Vec::new() } else { zeo_rt::each_values(&__coll)? };
                    let mut __idx: usize = 0;
                    // Top-of-loop step so `next` advances (see the Array arm).
                    let mut __first = true;
                    #outer: loop {
                        if !__first {
                            if __counted { __i += 1; } else { __idx += 1; }
                        }
                        __first = false;
                        let __elem = if __counted {
                            let __in_range = __endless
                                || if __exclusive { __i < __end } else { __i <= __end };
                            if !__in_range { break #outer __coll.clone(); }
                            zeo_rt::RubyValue::Int(__i)
                        } else {
                            match __iter.get(__idx) {
                                Some(__e) => __e.clone(),
                                None => break #outer __coll.clone(),
                            }
                        };
                        #bind_elem
                        #inner
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
pub fn emit_break(cx: &Ctx, id: NodeId, value: Option<NodeId>) -> TokenStream {
    let value_expr = match value {
        Some(v) => emit_expr(cx, v),
        None => quote! { zeo_rt::RubyValue::Nil },
    };
    match &cx.loop_labels {
        Some((_, outer)) => quote! { break #outer #value_expr },
        None if cx.in_real_proc => quote! { return Err(zeo_rt::Signal::Break(#value_expr)) },
        // A `break` at the top of a `define_method` body RETURNS from the
        // method -- `define_method(:go) { break :early }` answers `:early`
        // (oracle-verified). The block became the method, so there is no
        // yielding call left to break out of, and CRuby's own rule for a
        // block-turned-method is the lambda one. A real `def` never reaches
        // here: CRuby rejects a bare `break` in a method body outright.
        // rake's `define_method(:execute) { ... break if @failure ... }`
        // (cxxproject) is the corpus shape.
        None if cx.defined_by_define_method => quote! { return Ok(#value_expr) },
        None => super::unsupported_at(
            cx.compiler,
            id,
            "`break` outside a supported loop construct (zeo limitation)",
        ),
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
        // A value-consuming splice's `next v`: v IS this iteration's value,
        // so break the inner redo loop with it -- control lands on the
        // splice's own collect step (see `emit_redo_wrapped_body_value`).
        Some((redo, _)) if cx.next_yields_value => {
            let value_expr = match value {
                Some(v) => emit_expr(cx, v),
                None => quote! { zeo_rt::RubyValue::Nil },
            };
            // Parenthesized for the same brace-value lint the wrapper's own
            // break guards against.
            quote! { break #redo (#value_expr) }
        }
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
                None => quote! { zeo_rt::RubyValue::Nil },
            };
            quote! { return Err(zeo_rt::Signal::Next(#value_expr)) }
        }
        None => super::unsupported("`next` outside a supported loop construct (zeo limitation)"),
    }
}

/// `redo` -- see `hir.rs`'s `Redo` docs. Inside a real `Proc` with no
/// enclosing native loop, raises `Signal::Redo` -- caught (and acted on) by
/// the closure's own redo-wrapper loop, same as `Next`.
pub fn emit_redo(cx: &Ctx) -> TokenStream {
    match &cx.loop_labels {
        Some((redo, _)) => quote! { continue #redo },
        None if cx.in_real_proc => quote! { return Err(zeo_rt::Signal::Redo) },
        None => super::unsupported("`redo` outside a supported loop construct (zeo limitation)"),
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
/// `at` is the assignment's own node, where there is one: a constant target
/// records where it was written (`Module#const_source_location`).
pub fn emit_target_write(
    cx: &Ctx,
    target: &MultiTarget,
    value: TokenStream,
    at: Option<NodeId>,
) -> TokenStream {
    match target {
        MultiTarget::Local(name) => super::hoisting::emit_local_write(cx, name, value),
        MultiTarget::Ivar(name) => super::expr::emit_ivar_write_stmt(cx, name, value),
        MultiTarget::ClassVar(name) => super::expr::emit_cvar_write_stmt(cx, name, value),
        MultiTarget::Global(name) => {
            let bx = cx.box_id;
            quote! { zeo_rt::global_assign(#bx, #name, #value)?; }
        }
        MultiTarget::Const(name) => super::expr::emit_const_write_stmt(cx, None, name, value, at),
        MultiTarget::ScopedConst { scope, name } => {
            super::expr::emit_const_write_stmt(cx, Some(scope), name, value, at)
        }
        MultiTarget::Call {
            write_call,
            tmp_name,
        } => {
            let bind = super::hoisting::emit_local_write(cx, tmp_name, value);
            let call = emit_expr(cx, *write_call);
            quote! { #bind let _ = #call; }
        }
        MultiTarget::Nested(group) => emit_multi_target_group(cx, group, value, at),
    }
}

/// Destructures `value_expr` (an Array-typed Rust expression) against
/// `group`'s before/splat/after shape via `zeo_rt::multi_assign`, writing
/// each target through `emit_target_write` -- the recursive core both
/// `HirNode::MultiWrite` and a nested `MultiTarget::Nested` group reduce to.
/// Only `__elems`/`__before`/`__splat`/`__after` are genuine fresh,
/// transient scratch locals; every actual target write goes through
/// `emit_target_write`'s own storage-appropriate rules. See
/// `zeo_rt::multi_assign`'s docs for the destructuring rules this
/// implements.
pub fn emit_multi_target_group(
    cx: &Ctx,
    group: &crate::hir::MultiTargetGroup,
    value_expr: TokenStream,
    at: Option<NodeId>,
) -> TokenStream {
    let n_before = group.before.len();
    let n_after = group.after.len();
    let has_splat = group.splat.is_some();

    let bind_before = group
        .before
        .iter()
        .enumerate()
        .map(|(i, t)| emit_target_write(cx, t, quote! { __before[#i].clone() }, at));
    let bind_after = group
        .after
        .iter()
        .enumerate()
        .map(|(i, t)| emit_target_write(cx, t, quote! { __after[#i].clone() }, at));
    let bind_splat = match &group.splat {
        Some(Some(t)) => Some(emit_target_write(
            cx,
            t,
            quote! { zeo_rt::RubyValue::Array(zeo_rt::array_new(__splat)) },
            at,
        )),
        _ => None,
    };

    quote! {
        {
            let __elems = (#value_expr).as_array_ref().lock().to_vec();
            let (__before, __splat, __after) = zeo_rt::multi_assign(&__elems, #n_before, #has_splat, #n_after);
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
            zeo_rt::RubyValue::Array(zeo_rt::array_new(
                zeo_rt::block_auto_splat(&[#value_expr])?.into_owned()
            ))
        }
    };
    emit_multi_target_group(cx, targets, value_expr, Some(value))
}

/// Sub-expression form of a multiple assignment: `result = (x, y = rhs)` or
/// `(a, b = pair)[0]`. Same destructuring as `emit_multi_write`, but the whole
/// expression YIELDS the raw right-hand side verbatim -- CRuby's rule, verified
/// against ruby 4.0.6: `(a, b = 5)` -> `5`, `(a, b = 1, 2)` -> `[1, 2]`,
/// `(a, b = arr)` -> `arr` (the array itself, not a fresh destructured copy).
/// The RHS is evaluated exactly once and is already a `RubyValue` in both
/// branches -- the `Array` branch calls `as_array_unchecked` on it, the scalar
/// branch feeds it to `block_auto_splat`, which takes a `&[RubyValue]`.
pub fn emit_multi_write_value(cx: &Ctx, targets: &MultiTargetGroup, value: NodeId) -> TokenStream {
    let value_expr = {
        let e = emit_expr(cx, value);
        super::expr::box_if_object_typed(cx, value, e)
    };
    let coerced = if infer(cx, value) == TyKind::Array {
        quote! { __rhs.clone() }
    } else {
        quote! {
            zeo_rt::RubyValue::Array(zeo_rt::array_new(
                zeo_rt::block_auto_splat(&[__rhs.clone()])?.into_owned()
            ))
        }
    };
    let group = emit_multi_target_group(cx, targets, coerced, Some(value));
    quote! {
        {
            let __rhs = #value_expr;
            #group
            __rhs
        }
    }
}
