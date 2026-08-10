//! `emit_call`'s dispatch decision -- see "Two dispatch paths" in the plan:
//! Path 1 (static direct call, or a call rewritten away entirely, e.g.
//! `super` inlining) whenever `Compiler::method_in_chain` resolves the
//! target from the receiver's statically-known class; Path 2
//! (`zeo_rt::send`) only when it can't. Every fragment produced here
//! evaluates to a bare `zeo_rt::RubyValue` -- any call into a
//! `Result`-returning method has `?` applied right here, at the call site,
//! not left to the caller (see `expr.rs`'s module docs).

mod builtins;
mod kernel;
mod new;
mod ops;
mod path2;
mod procs;
mod raise;
mod reflect;
mod splat;
mod super_calls;
mod visibility;

use quote::{format_ident, quote};

use super::Ctx;
use super::expr::{
    box_if_object_typed, emit_expr, emit_symbol_expr, infer, infer_any_class, infer_class,
};
use super::ident::safe_ident;
use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HirNode, KwArg, NodeId, Visibility};
use crate::types::TyKind;
use proc_macro2::TokenStream;

/// An inline-spliced block (`n.times { |i| ... }` and friends) inside a
/// `binding` scope. Such a block shares the enclosing Rust scope, so its own
/// parameter and block-local names are ordinary per-iteration `let`s that no
/// Binding could share -- unless they join `nested_captured`, which is the set
/// the splice sites already cell-wrap for a nested escaping block's sake.
/// Answers the name list a `binding` inside the spliced body emits: this
/// block's own names first, then the enclosing scope's, minus any it shadows.
/// `None` (and no promotion) outside a `binding` scope.
fn inline_block_binding_names(
    cx: &Ctx,
    params: &crate::hir::Params,
    nested_captured: &mut std::collections::HashSet<String>,
) -> Option<std::rc::Rc<Vec<String>>> {
    let outer = cx.binding_names.as_ref()?;
    let own: Vec<String> = params
        .required
        .iter()
        .chain(&params.block_locals)
        .cloned()
        .collect();
    nested_captured.extend(own.iter().cloned());
    let mut names = own.clone();
    names.extend(outer.iter().filter(|n| !own.contains(n)).cloned());
    Some(std::rc::Rc::new(names))
}

/// The eval VM call every `Kernel#eval` site funnels through, given the
/// already-emitted Binding of the calling scope and the call's
/// `(src[, binding[, file[, line]]])` arguments. The absent trailing ones are
/// `nil`, which is what tells the runtime to use `scope`.
/// Whether the class this body was WRITTEN in defines `name` itself, which
/// makes a Kernel function of that name shadowed: real Ruby resolves the
/// method, never the Kernel one.
///
/// The sibling branches apply that rule directly whenever they can see a
/// concrete receiver. Inside a BLOCK they cannot -- yet the block's self is
/// still the enclosing method's -- so `[x].map { |v| pp(v) }` written in a
/// class with its own `pp` folded to `Kernel#pp` and PRINTED instead of calling
/// the method. Declining the fold leaves the call to the dynamic tail, which
/// resolves it exactly as an explicit `self.pp(v)` would. `class_self` rules out
/// a class method's body, where `self` is the class and an instance method of
/// the same name does not apply. Both the RECEIVER's class and the one the body
/// was written in are asked: a module's own `methods` list is emptied once
/// materialization has copied it into the including class, so only the receiver
/// still knows, while a reopened builtin keeps its methods where it wrote them.
fn kernel_name_shadowed(cx: &Ctx, name: &str) -> bool {
    if cx.class_self.is_some() {
        return false;
    }
    // The RECEIVER's class is the per-class half, and is asked as a recorded
    // query; the class the body was WRITTEN in is the same for every member of
    // a sharing group, so it is read directly. Both are asked because a
    // module's own `methods` list is emptied once materialization has copied it
    // into the including class -- only the receiver still knows -- while a
    // reopened builtin keeps its methods where it wrote them.
    let shadowed_by_receiver = cx
        .ask_opt(super::class_query::ClassQuery::ShadowsKernel(
            name.to_string(),
        ))
        .is_some_and(|a| a.yes());
    shadowed_by_receiver
        || cx.defining_class.is_some_and(|owner| {
            owner != crate::compiler::OBJECT_CLASS
                && cx.compiler.method_in_chain(owner, name).is_some()
        })
}

fn emit_eval_in_scope(cx: &Ctx, scope: TokenStream, args: &[NodeId]) -> TokenStream {
    let mut arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    let src = arg_exprs.next().expect("eval always has a source argument");
    let rest: Vec<TokenStream> = arg_exprs
        .chain(std::iter::repeat_with(|| quote! { zeo_rt::RubyValue::Nil }))
        .take(3)
        .collect();
    quote! { zeo_rt::eval_value_in_scope(#src, #scope, #(#rest),*)? }
}

/// `Kernel#binding` -- this scope, as a value. `Ctx::binding_names` holds the
/// names (in `local_variables` order), each of which
/// `captures::binding_scope_names` already promoted to cell storage, so the
/// call site just clones the `Arc`s into the runtime object; the compiled code
/// keeps reading and writing the very same cells.
///
/// A name whose storage ISN'T a cell at this exact position is skipped: an
/// inline-spliced block's own parameter (`x = 1; 3.times { |x| binding }`)
/// shadows the enclosing cell with a plain per-iteration `let`, and it is the
/// shadow that is in scope here. Such a block's own parameters are absent from
/// the Binding rather than wrongly bound -- see `docs/COMPATIBILITY.md`.
pub(crate) fn emit_binding(cx: &Ctx, id: NodeId) -> TokenStream {
    let (file, line) = super::source_location(cx.compiler, id).unwrap_or(("(eval)", 0));
    emit_binding_value(cx, file, line)
}

/// [`emit_binding`] with the source location supplied -- `TOPLEVEL_BINDING`,
/// whose `source_location` CRuby reports as `["<main>", 0]`.
pub(crate) fn emit_binding_value(cx: &Ctx, file: &str, line: u32) -> TokenStream {
    let recv = boxed_implicit_self(cx).expect("every context has an implicit self");
    emit_binding_with_self(cx, recv, file, line)
}

/// [`emit_binding_value`] with `self` supplied rather than taken from the
/// context -- `obj.send(:eval, src)`, where CRuby reads the LOCALS from the
/// caller's frame but binds `self` to the receiver.
pub(crate) fn emit_binding_with_self(
    cx: &Ctx,
    recv: TokenStream,
    file: &str,
    line: u32,
) -> TokenStream {
    let box_id = cx.box_id;
    // `u32::MAX`, not `0`: `ClassId(0)` is `Object`, a cref a top-level
    // binding must not claim (see `zeo_rt::binding_new`).
    let cref = cx.defining_class.map_or(u32::MAX, |c| c.0);
    let entries = cx
        .binding_names
        .iter()
        .flat_map(|names| names.iter())
        .filter(|n| {
            super::hoisting::local_storage(cx, n) == super::hoisting::LocalStorage::Captured
        })
        .map(|n| {
            let ident = safe_ident(n);
            quote! { (#n, ::std::sync::Arc::clone(&#ident)) }
        });
    quote! {
        zeo_rt::binding_new(#recv, vec![#(#entries),*], #file, #line, #box_id, #cref)
    }
}

// Re-exports so every pre-existing `crate::codegen::call::<name>` path used
// by sibling `codegen` modules keeps resolving after this file split --
// each name still lives at its original address as far as any caller
// outside this module tree is concerned.
pub use new::{emit_new, emit_new_with_arg_tokens, emit_private_new_error};
pub use procs::emit_lambda_value;
pub(crate) use procs::emit_proc_or_lambda_value;
pub use super_calls::emit_super;

/// Whether a call shape is the `.times` fast path (inline splice, no real
/// `Proc` ever allocated) -- the exact condition `dispatch` already checks
/// for below, factored out so `codegen::captures`' escaping-block scan can
/// ask the identical question: any block NOT matching this shape becomes a
/// real, heap-allocated `Proc` (see that module's docs -- there is no
/// separate "escape analysis" beyond this one check, since `.times` is the
/// only inline fast path that exists).
pub fn is_times_fast_path(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> bool {
    kwargs_empty
        && name == "times"
        && !compiler.times_literal_suppressed
        && receiver.is_some_and(|r| matches!(compiler.hir[r], HirNode::IntegerLit(_)))
}

/// `.times`'s sibling: `(1..9).each { }` on a LITERAL range whose bounds are
/// both Int literals -- the other call shape that fuses to a native counted
/// loop (no `Proc` allocated). Beginless/endless ranges keep the generic
/// path (an endless `each` never terminates by counting up).
pub fn is_range_each_fast_path(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> bool {
    kwargs_empty
        && name == "each"
        && !compiler.range_each_literal_suppressed
        && receiver.is_some_and(|r| match compiler.hir[r] {
            HirNode::RangeLit {
                start: Some(s),
                end: Some(e),
                ..
            } => {
                matches!(compiler.hir[s], HirNode::IntegerLit(_))
                    && matches!(compiler.hir[e], HirNode::IntegerLit(_))
            }
            _ => false,
        })
}

/// Either inline-splice shape -- what the CAPTURE scans ask: a block NOT
/// matching one of these becomes a real, heap-allocated `Proc`.
/// Deliberately syntactic-only (typed `inline_iter_sites` nominations are
/// NOT consulted): a typed site's dynamic-fallback arm still builds a real
/// proc, so its shared outer locals must keep escaping-style cell captures,
/// which the inline arm routes through just as correctly.
pub fn is_inline_block_fast_path(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> bool {
    is_times_fast_path(compiler, receiver, name, kwargs_empty)
        || is_range_each_fast_path(compiler, receiver, name, kwargs_empty)
}

/// The DESCEND decision for the hoisting/exception scans: any spliced block
/// body -- literal shape or typed-site nomination -- shares the enclosing
/// Rust scope, so its assigned locals hoist there and its loop jumps compile
/// against labels in that scope. Wider than `is_inline_block_fast_path`
/// (see its docs for why the capture scans keep the narrow answer).
pub fn is_spliced_block_body(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
    block: NodeId,
) -> bool {
    is_inline_block_fast_path(compiler, receiver, name, kwargs_empty)
        || compiler.inline_iter_sites.contains_key(&block)
}

/// The one native counted-loop splice behind `n.times { }`,
/// `n.upto/downto/step(..) { }` and `(a..b).each { }`: iterate `__i` from
/// `start`, advancing by `step` (an `i64` expression; the add is
/// overflow-CHECKED, so a walk that would leave `i64` just stops -- exactly
/// where CRuby's next value exceeds every `i64` limit) until `done` (a bool
/// expression over `__i`), binding the block's first required param as a
/// fresh `Int` each iteration, and evaluate to `result` (times/upto/...: the
/// receiver Int; range each: the receiver range). The body is spliced into
/// this Rust scope -- no closure or `Proc` object is ever allocated --
/// sharing `codegen::loops`' redo-wrapping so `break`/`next`/`redo` work
/// exactly as in `while`/`until`/`for`.
#[allow(clippy::too_many_arguments)]
fn emit_counted_block_splice(
    cx: &Ctx,
    block_id: NodeId,
    start: TokenStream,
    done: TokenStream,
    step: TokenStream,
    label_stem: &str,
    result: TokenStream,
) -> TokenStream {
    let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
        panic!("the inline splice's argument must be a block");
    };
    let outer = super::loops::fresh_label(cx, label_stem);
    let redo = super::loops::fresh_label(cx, &format!("{label_stem}_body"));
    // This inlined block's OWN param/block-locals that a NESTED
    // escaping block captures (e.g.
    // `arr.each { 3.times { |i| store << ->{ i } } }`). Since the
    // spliced body shares this Rust scope rather than being a
    // closure, its param would otherwise be a plain per-iteration
    // `let` that a `move` closure can't share -- so cell-wrap exactly
    // those names into `Arc<Mutex<RubyValue>>` (fresh per iteration,
    // matching Ruby's per-iteration block-param binding), register
    // them as `captured_locals` so the body's own reads/writes route
    // through the cell, and let the nested closure `Arc::clone` them
    // -- the identical treatment a real block param gets via
    // `emit_proc_or_lambda_value`'s `nested_param_wraps`. Without
    // this, the nested block would fresh-declare the name and read
    // `nil`.
    let mut nested_captured: std::collections::HashSet<String> =
        super::captures::collect_escaping_captures(cx.compiler, body, params, cx.self_class())
            .locals;
    let splice_binding = inline_block_binding_names(cx, params, &mut nested_captured);
    let mut loop_cx = cx.in_loop(redo.clone(), outer.clone());
    loop_cx.binding_names = splice_binding;
    // This block's OWN param/block-local names shadow a same-named OUTER
    // captured local (`rescue => e` cell-hoisted in the enclosing scope,
    // then `arr.each { |e| ... }` spliced here): the spliced binding is a
    // plain per-iteration `let`, so body reads must not route through the
    // outer cell -- `in_proc`'s exact rule. `nested_captured` re-adds any
    // of them a nested escaping block really captures.
    if params
        .required
        .iter()
        .chain(&params.block_locals)
        .any(|n| loop_cx.captured_locals.contains(n))
    {
        loop_cx
            .captured_locals
            .to_mut()
            .retain(|n| !params.required.contains(n) && !params.block_locals.contains(n));
    }
    if !nested_captured.is_empty() {
        loop_cx
            .captured_locals
            .to_mut()
            .extend(nested_captured.iter().cloned());
    }
    // The counter param IS an `Int` by construction (bound as
    // `RubyValue::Int(__i)` below) -- tell inference so body reads
    // take the typed fast paths: an untyped index in `a[i] = ...`
    // forced 32M dynamic sends in bm_loops_times (~100x slower than
    // C). A cell-wrapped (nested-captured) param stays untyped, its
    // reads route through the Arc<Mutex> cell; and either way any
    // stale OUTER type under the same name must not leak in. Same
    // for block-locals, which rebind as plain `RubyValue` nil.
    if let Some(p) = params.required.first() {
        if nested_captured.contains(p) {
            loop_cx.local_types.to_mut().remove(p);
        } else {
            loop_cx
                .local_types
                .to_mut()
                .insert(p.clone(), crate::types::TyKind::Int);
        }
    }
    for name in &params.block_locals {
        loop_cx.local_types.to_mut().remove(name);
    }
    let bind = params.required.first().map(|p| {
        let ident = safe_ident(p);
        // `mut`: a block param is an ordinary reassignable local.
        let plain = quote! { #[allow(unused_mut)] let mut #ident = zeo_rt::RubyValue::Int(__i); };
        if nested_captured.contains(p) {
            quote! {
                #plain
                let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                    ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(#ident));
            }
        } else {
            plain
        }
    });
    // `3.times { |i; n| ... }` -- block-locals get a fresh `nil` per
    // iteration here, exactly as `emit_proc_param_bindings` does for
    // a real Proc. Inside the loop, not outside: the reset-every-
    // invocation semantics is the whole point of the declaration. A
    // block-local a nested escaping block captures is cell-wrapped for
    // the same reason as the param above.
    let block_locals = params.block_locals.iter().map(|name| {
        let ident = safe_ident(name);
        if nested_captured.contains(name) {
            quote! {
                let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                    ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(zeo_rt::RubyValue::Nil));
            }
        } else {
            quote! {
                #[allow(unused_variables, unused_mut)]
                let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Nil;
            }
        }
    });
    // Implicit block-locals (a name first-assigned inside the body) are fresh
    // on every invocation in Ruby. Since this splice shares the enclosing Rust
    // scope, those names were hoisted to a single enclosing `let` -- reset them
    // to nil at the top of each iteration so a conditional first-assignment
    // (`x = v if cond`) does not carry into the next. A name captured from the
    // enclosing scope (`sum` in `sum = 0; n.times { sum += i }`) is NOT in this
    // set (prism scopes it to the enclosing method), so it still accumulates.
    // A block-local a NESTED escaping block captures is cell-wrapped
    // (`Arc<Mutex>`) rather than a plain local, so it is excluded here -- its
    // freshness is the nested-capture machinery's job, not a scalar reset.
    let implicit_resets = params
        .implicit_block_locals
        .iter()
        .filter(|name| !nested_captured.contains(*name))
        .map(|name| {
            // Through the storage-aware writer: a name some OTHER nested
            // block (per the method-level capture analysis) cell-wrapped is
            // an `Arc<Mutex>` here, not a plain local.
            super::hoisting::emit_local_write(&loop_cx, name, quote! { zeo_rt::RubyValue::Nil })
        });
    let implicit_resets = quote! { #[allow(unused_assignments)] { #(#implicit_resets)* } };
    let inner = super::loops::emit_redo_wrapped_body(&loop_cx, body, &redo);
    quote! {
        {
            let mut __i: i64 = #start;
            // Step at the TOP so `next` (a `continue #outer`) still advances --
            // a bottom step is skipped by `next`, spinning forever. `redo`
            // continues the INNER label and never reaches here.
            let mut __first = true;
            #outer: loop {
                if !__first {
                    __i = match __i.checked_add(#step) {
                        Some(__v) => __v,
                        None => break #outer #result,
                    };
                }
                __first = false;
                if #done { break #outer #result; }
                #bind
                #(#block_locals)*
                #implicit_resets
                #inner
            }
        }
    }
}
/// How an inlined Array-iterator splice CONSUMES each iteration.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrayIterMode {
    /// `each`/`each_with_index`: body value discarded; evaluates to the
    /// receiver handle.
    Each { with_index: bool },
    /// `map`: block values collect into the result array.
    Map,
    /// `select` (`keep: true`) / `reject`: the block value is the predicate,
    /// the ORIGINAL element is what collects -- a param reassignment inside
    /// the body must not change what lands in the result (CRuby).
    Filter { keep: bool },
    /// `sum` (block form): block values accumulate through `zeo_rt::SumAcc`,
    /// the runtime `sum`'s own numeric ladder.
    Sum,
}

/// `emit_counted_block_splice`'s Array twin: iterate a LIVE view of the
/// receiver array -- lock per element, never across the body, the runtime
/// `Array#each` rule (Enumerable's `map`/`select`/`sum` drive `each`, so
/// they share it) -- binding the block's first param from the fetched
/// element (and, for `each_with_index`, the second from the index). The
/// value-consuming modes wrap the body in the VALUE redo loop and set
/// `next_yields_value`, so `next v` surfaces `v` as that iteration's value
/// (see `loops::emit_redo_wrapped_body_value`); `break v` overrides the
/// whole splice's value as usual. Expects the receiver bound as
/// `__iter_arr` by the caller's match arm.
fn emit_array_iter_splice(
    cx: &Ctx,
    block_id: NodeId,
    mode: ArrayIterMode,
    label_stem: &str,
) -> TokenStream {
    let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
        panic!("the inline splice's argument must be a block");
    };
    let with_index = mode == ArrayIterMode::Each { with_index: true };
    let outer = super::loops::fresh_label(cx, label_stem);
    let redo = super::loops::fresh_label(cx, &format!("{label_stem}_body"));
    let mut nested_captured: std::collections::HashSet<String> =
        super::captures::collect_escaping_captures(cx.compiler, body, params, cx.self_class())
            .locals;
    let splice_binding = inline_block_binding_names(cx, params, &mut nested_captured);
    let mut loop_cx = cx.in_loop(redo.clone(), outer.clone());
    loop_cx.binding_names = splice_binding;
    loop_cx.next_yields_value = !matches!(mode, ArrayIterMode::Each { .. });
    // This block's OWN param/block-local names shadow a same-named OUTER
    // captured local (`rescue => e` cell-hoisted in the enclosing scope,
    // then `arr.each { |e| ... }` spliced here): the spliced binding is a
    // plain per-iteration `let`, so body reads must not route through the
    // outer cell -- `in_proc`'s exact rule. `nested_captured` re-adds any
    // of them a nested escaping block really captures.
    if params
        .required
        .iter()
        .chain(&params.block_locals)
        .any(|n| loop_cx.captured_locals.contains(n))
    {
        loop_cx
            .captured_locals
            .to_mut()
            .retain(|n| !params.required.contains(n) && !params.block_locals.contains(n));
    }
    if !nested_captured.is_empty() {
        loop_cx
            .captured_locals
            .to_mut()
            .extend(nested_captured.iter().cloned());
    }
    // The element param is untyped (drop any stale OUTER type under the same
    // name); the index param is `Int` by construction, unless cell-wrapped.
    if let Some(p) = params.required.first() {
        loop_cx.local_types.to_mut().remove(p);
    }
    if with_index && let Some(p) = params.required.get(1) {
        if nested_captured.contains(p) {
            loop_cx.local_types.to_mut().remove(p);
        } else {
            loop_cx
                .local_types
                .to_mut()
                .insert(p.clone(), crate::types::TyKind::Int);
        }
    }
    for name in &params.block_locals {
        loop_cx.local_types.to_mut().remove(name);
    }
    let cell_wrap = |ident: &proc_macro2::Ident| {
        quote! {
            let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(#ident));
        }
    };
    let bind_elem = params.required.first().map(|p| {
        let ident = safe_ident(p);
        let plain = quote! { #[allow(unused_mut)] let mut #ident = __iter_e; };
        if nested_captured.contains(p) {
            let wrap = cell_wrap(&ident);
            quote! { #plain #wrap }
        } else {
            plain
        }
    });
    let bind_idx = (with_index && params.required.len() >= 2).then(|| {
        let ident = safe_ident(&params.required[1]);
        let plain =
            quote! { #[allow(unused_mut)] let mut #ident = zeo_rt::RubyValue::Int(__iter_i as i64); };
        if nested_captured.contains(&params.required[1]) {
            let wrap = cell_wrap(&ident);
            quote! { #plain #wrap }
        } else {
            plain
        }
    });
    let block_locals = params.block_locals.iter().map(|name| {
        let ident = safe_ident(name);
        if nested_captured.contains(name) {
            quote! {
                let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                    ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(zeo_rt::RubyValue::Nil));
            }
        } else {
            quote! {
                #[allow(unused_variables, unused_mut)]
                let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Nil;
            }
        }
    });
    let implicit_resets = params
        .implicit_block_locals
        .iter()
        .filter(|name| !nested_captured.contains(*name))
        .map(|name| {
            // Storage-aware: see the counted splice's identical note.
            super::hoisting::emit_local_write(&loop_cx, name, quote! { zeo_rt::RubyValue::Nil })
        });
    let implicit_resets = quote! { #[allow(unused_assignments)] { #(#implicit_resets)* } };
    // The per-mode pieces: accumulator setup, the exhaustion value, an
    // element keep-alive for the filter modes (the RESULT collects the
    // pristine element even if the body reassigns its param), and the
    // body-value consumer. `Each` discards the body value and answers the
    // receiver; everything else runs the body in VALUE mode.
    let setup = match mode {
        ArrayIterMode::Each { .. } => quote! {},
        ArrayIterMode::Map | ArrayIterMode::Filter { .. } => {
            quote! { let mut __iter_out: Vec<zeo_rt::RubyValue> = Vec::new(); }
        }
        ArrayIterMode::Sum => {
            quote! { let mut __iter_acc = zeo_rt::SumAcc::new(zeo_rt::RubyValue::Int(0)); }
        }
    };
    let result = match mode {
        ArrayIterMode::Each { .. } => quote! { zeo_rt::RubyValue::Array(__iter_arr.clone()) },
        ArrayIterMode::Map | ArrayIterMode::Filter { .. } => {
            quote! { zeo_rt::RubyValue::Array(zeo_rt::array_new(__iter_out)) }
        }
        ArrayIterMode::Sum => quote! { __iter_acc.finish() },
    };
    let keep_orig = matches!(mode, ArrayIterMode::Filter { .. })
        .then(|| quote! { let __iter_orig = __iter_e.clone(); });
    let (inner, consume) = match mode {
        ArrayIterMode::Each { .. } => (
            super::loops::emit_redo_wrapped_body(&loop_cx, body, &redo),
            quote! {},
        ),
        ArrayIterMode::Map => (
            super::loops::emit_redo_wrapped_body_value(&loop_cx, body, &redo),
            quote! { __iter_out.push(__iter_y); },
        ),
        ArrayIterMode::Filter { keep } => (
            super::loops::emit_redo_wrapped_body_value(&loop_cx, body, &redo),
            quote! { if __iter_y.truthy() == #keep { __iter_out.push(__iter_orig); } },
        ),
        ArrayIterMode::Sum => (
            super::loops::emit_redo_wrapped_body_value(&loop_cx, body, &redo),
            quote! { __iter_acc = __iter_acc.add(__iter_y)?; },
        ),
    };
    let inner = if matches!(mode, ArrayIterMode::Each { .. }) {
        inner
    } else {
        quote! { let __iter_y = #inner; #consume }
    };
    quote! {
        {
            let mut __iter_i: usize = 0;
            #setup
            #outer: loop {
                #[allow(unused_variables)]
                let __iter_e = {
                    let __g = __iter_arr.lock();
                    match __g.get(__iter_i) {
                        Some(__v) => __v.clone(),
                        None => break #outer #result,
                    }
                };
                #keep_orig
                #bind_elem
                #bind_idx
                #(#block_locals)*
                #implicit_resets
                // Advance BEFORE the body so `next` (in value mode: the
                // inner break) still steps; `redo` re-enters the inner
                // label with the same bound element.
                __iter_i += 1;
                #inner
            }
        }
    }
}

/// The Hash twin: walk the same pairs SNAPSHOT the runtime `Hash#each`
/// takes (one lock acquisition, then lock-free iteration -- the block may
/// mutate the receiver), binding the block's params per CRuby's tuple rule:
/// `|k, v|` auto-splats the pair, a single param receives it whole as a
/// two-element Array. Evaluates to the receiver handle, and holds the same
/// synthetic `Hash#each` C-frame the runtime method shows in a
/// block-raised backtrace. Expects the receiver bound as `__iter_h` by the
/// caller's match arm.
fn emit_hash_each_splice(cx: &Ctx, block_id: NodeId, label_stem: &str) -> TokenStream {
    let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
        panic!("the inline splice's argument must be a block");
    };
    let outer = super::loops::fresh_label(cx, label_stem);
    let redo = super::loops::fresh_label(cx, &format!("{label_stem}_body"));
    let mut nested_captured: std::collections::HashSet<String> =
        super::captures::collect_escaping_captures(cx.compiler, body, params, cx.self_class())
            .locals;
    let splice_binding = inline_block_binding_names(cx, params, &mut nested_captured);
    let mut loop_cx = cx.in_loop(redo.clone(), outer.clone());
    loop_cx.binding_names = splice_binding;
    // This block's OWN param/block-local names shadow a same-named OUTER
    // captured local (`rescue => e` cell-hoisted in the enclosing scope,
    // then `arr.each { |e| ... }` spliced here): the spliced binding is a
    // plain per-iteration `let`, so body reads must not route through the
    // outer cell -- `in_proc`'s exact rule. `nested_captured` re-adds any
    // of them a nested escaping block really captures.
    if params
        .required
        .iter()
        .chain(&params.block_locals)
        .any(|n| loop_cx.captured_locals.contains(n))
    {
        loop_cx
            .captured_locals
            .to_mut()
            .retain(|n| !params.required.contains(n) && !params.block_locals.contains(n));
    }
    if !nested_captured.is_empty() {
        loop_cx
            .captured_locals
            .to_mut()
            .extend(nested_captured.iter().cloned());
    }
    for p in &params.required {
        loop_cx.local_types.to_mut().remove(p);
    }
    for name in &params.block_locals {
        loop_cx.local_types.to_mut().remove(name);
    }
    let cell_wrap = |ident: &proc_macro2::Ident| {
        quote! {
            let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(#ident));
        }
    };
    let bind_one = |p: &String, value: TokenStream| {
        let ident = safe_ident(p);
        let plain = quote! { #[allow(unused_mut)] let mut #ident = #value; };
        if nested_captured.contains(p) {
            let wrap = cell_wrap(&ident);
            quote! { #plain #wrap }
        } else {
            plain
        }
    };
    let binds = match params.required.len() {
        0 => quote! {},
        // One param takes the pair WHOLE (the runtime's `yield_tuple` rule).
        1 => bind_one(
            &params.required[0],
            quote! {
                zeo_rt::RubyValue::Array(zeo_rt::array_new(vec![__iter_k, __iter_v]))
            },
        ),
        _ => {
            let k = bind_one(&params.required[0], quote! { __iter_k });
            let v = bind_one(&params.required[1], quote! { __iter_v });
            quote! { #k #v }
        }
    };
    let block_locals = params.block_locals.iter().map(|name| {
        let ident = safe_ident(name);
        if nested_captured.contains(name) {
            quote! {
                let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                    ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(zeo_rt::RubyValue::Nil));
            }
        } else {
            quote! {
                #[allow(unused_variables, unused_mut)]
                let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Nil;
            }
        }
    });
    let implicit_resets = params
        .implicit_block_locals
        .iter()
        .filter(|name| !nested_captured.contains(*name))
        .map(|name| {
            // Storage-aware: see the counted splice's identical note.
            super::hoisting::emit_local_write(&loop_cx, name, quote! { zeo_rt::RubyValue::Nil })
        });
    let implicit_resets = quote! { #[allow(unused_assignments)] { #(#implicit_resets)* } };
    let inner = super::loops::emit_redo_wrapped_body(&loop_cx, body, &redo);
    quote! {
        {
            let __iter_frame = zeo_rt::synthetic_c_frame("Hash#each");
            // The inlined loop marks the hash under iteration exactly as the
            // runtime row does, so inserting a new key raises mid-`each`.
            let __iter_guard = zeo_rt::hash_iter_guard(&__iter_h);
            let __iter_pairs = zeo_rt::hash_pairs_snapshot(&__iter_h);
            let mut __iter_i: usize = 0;
            #outer: loop {
                if __iter_i >= __iter_pairs.len() {
                    break #outer zeo_rt::RubyValue::Hash(__iter_h.clone());
                }
                #[allow(unused_variables)]
                let (__iter_k, __iter_v) = __iter_pairs[__iter_i].clone();
                #binds
                #(#block_locals)*
                #implicit_resets
                __iter_i += 1;
                #inner
            }
        }
    }
}

/// A typed-receiver iterator site (`Compiler::inline_iter_sites`): emit the
/// native loop under a match arm that PROVES the receiver's (and any
/// argument's) runtime shape, with the ordinary dynamic dispatch as the
/// other arm -- an Int-typed local holding a post-overflow BigInt, a
/// shadowing block param, or a Float `step` limit just takes the fallback.
/// Every native arm is additionally gated on `zeo_rt::iter_inline_ok`: once
/// anything is defined at runtime (a reopen of the builtin, a per-object
/// singleton) or inside a box, only real dispatch can answer, and the site
/// keeps working through the fallback arm. The escaping-block scans treated
/// this site as escaping (cell captures), which both arms share correctly.
fn emit_typed_iter_inline(
    cx: &Ctx,
    kind: crate::compiler::InlineIterKind,
    recv_id: NodeId,
    args: &[NodeId],
    block_id: NodeId,
    name: &str,
    block_arg: Option<NodeId>,
) -> TokenStream {
    use crate::compiler::InlineIterKind as K;
    let recv = emit_expr(cx, recv_id);
    let __bx = cx.box_id;
    let name_sym = super::pooled_sym(name);
    let block_value = emit_block_option(cx, Some(block_id), block_arg);
    let fallback = |fb_args: TokenStream| {
        wrap_dynamic_result(
            true,
            quote! {
                zeo_rt::send_value_in(#__bx, &__iter_other, #name_sym, #fb_args, #block_value)
            },
        )
    };
    // The receiver Int evaluates to itself (MRI: `times`/`upto`/`downto`/
    // `step` answer their receiver).
    let int_result = quote! { zeo_rt::RubyValue::Int(__iter_a) };
    match kind {
        K::TimesInt => {
            let splice = emit_counted_block_splice(
                cx,
                block_id,
                quote! { 0i64 },
                quote! { __i >= __iter_a },
                quote! { 1i64 },
                "times",
                int_result,
            );
            let fallback = fallback(quote! { &[] });
            quote! {
                match #recv {
                    zeo_rt::RubyValue::Int(__iter_a) if zeo_rt::iter_inline_ok_for(#__bx, zeo_rt::INTEGER_CLASS) => #splice,
                    __iter_other => #fallback,
                }
            }
        }
        // A one-arg `step` IS `upto`: limit inclusive, stride 1.
        K::UptoInt | K::DowntoInt | K::StepInt if args.len() == 1 => {
            let limit = emit_expr(cx, args[0]);
            let (done, step) = if kind == K::DowntoInt {
                (quote! { __i < __iter_b }, quote! { -1i64 })
            } else {
                (quote! { __i > __iter_b }, quote! { 1i64 })
            };
            let splice = emit_counted_block_splice(
                cx,
                block_id,
                quote! { __iter_a },
                done,
                step,
                name,
                int_result,
            );
            let fallback = fallback(quote! { &[__iter_lim] });
            quote! {
                match (#recv, #limit) {
                    (zeo_rt::RubyValue::Int(__iter_a), zeo_rt::RubyValue::Int(__iter_b))
                        if zeo_rt::iter_inline_ok_for(#__bx, zeo_rt::INTEGER_CLASS) => #splice,
                    (__iter_other, __iter_lim) => #fallback,
                }
            }
        }
        K::StepInt => {
            let limit = emit_expr(cx, args[0]);
            let by = emit_expr(cx, args[1]);
            // The stride's sign picks the walk's direction; zero strides
            // (an ArgumentError) fall back to the runtime's own check.
            let splice = emit_counted_block_splice(
                cx,
                block_id,
                quote! { __iter_a },
                quote! { (__iter_s > 0 && __i > __iter_b) || (__iter_s < 0 && __i < __iter_b) },
                quote! { __iter_s },
                name,
                int_result,
            );
            let fallback = fallback(quote! { &[__iter_lim, __iter_stp] });
            quote! {
                match (#recv, #limit, #by) {
                    (
                        zeo_rt::RubyValue::Int(__iter_a),
                        zeo_rt::RubyValue::Int(__iter_b),
                        zeo_rt::RubyValue::Int(__iter_s),
                    ) if __iter_s != 0 && zeo_rt::iter_inline_ok_for(#__bx, zeo_rt::INTEGER_CLASS) => #splice,
                    (__iter_other, __iter_lim, __iter_stp) => #fallback,
                }
            }
        }
        K::UptoInt | K::DowntoInt => unreachable!("mark pass nominates these with one argument"),
        K::RangeEachInt => {
            let splice = emit_counted_block_splice(
                cx,
                block_id,
                quote! { __iter_a },
                quote! { if __iter_x { __i >= __iter_b } else { __i > __iter_b } },
                quote! { 1i64 },
                "range_each",
                quote! {
                    zeo_rt::RubyValue::Range(
                        Some(Box::new(zeo_rt::RubyValue::Int(__iter_a))),
                        Some(Box::new(zeo_rt::RubyValue::Int(__iter_b))),
                        __iter_x,
                    )
                },
            );
            let fallback = fallback(quote! { &[] });
            quote! {
                match #recv {
                    zeo_rt::RubyValue::Range(Some(__iter_bs), Some(__iter_be), __iter_x)
                        if zeo_rt::iter_inline_ok_for(#__bx, zeo_rt::RANGE_CLASS)
                            && matches!(__iter_bs.as_ref(), zeo_rt::RubyValue::Int(_))
                            && matches!(__iter_be.as_ref(), zeo_rt::RubyValue::Int(_)) =>
                    {
                        let (zeo_rt::RubyValue::Int(__iter_a), zeo_rt::RubyValue::Int(__iter_b)) =
                            (*__iter_bs, *__iter_be)
                        else {
                            unreachable!()
                        };
                        #splice
                    }
                    __iter_other => #fallback,
                }
            }
        }
        K::ArrayEach
        | K::ArrayEachWithIndex
        | K::ArrayMap
        | K::ArraySelect
        | K::ArrayReject
        | K::ArraySum => {
            let mode = match kind {
                K::ArrayEach => ArrayIterMode::Each { with_index: false },
                K::ArrayEachWithIndex => ArrayIterMode::Each { with_index: true },
                K::ArrayMap => ArrayIterMode::Map,
                K::ArraySelect => ArrayIterMode::Filter { keep: true },
                K::ArrayReject => ArrayIterMode::Filter { keep: false },
                _ => ArrayIterMode::Sum,
            };
            let splice = emit_array_iter_splice(cx, block_id, mode, name);
            let fallback = fallback(quote! { &[] });
            quote! {
                match #recv {
                    zeo_rt::RubyValue::Array(__iter_arr) if zeo_rt::iter_inline_ok_for(#__bx, zeo_rt::ARRAY_CLASS) => #splice,
                    __iter_other => #fallback,
                }
            }
        }
        K::HashEach => {
            let splice = emit_hash_each_splice(cx, block_id, name);
            let fallback = fallback(quote! { &[] });
            quote! {
                match #recv {
                    zeo_rt::RubyValue::Hash(__iter_h) if zeo_rt::iter_inline_ok_for(#__bx, zeo_rt::HASH_CLASS) => #splice,
                    __iter_other => #fallback,
                }
            }
        }
    }
}

/// The operand identifier when `id` reads a PLAIN hoisted local
/// (`LocalStorage::Hoisted`) -- the one case the dynamic-operator match can
/// borrow the place directly instead of cloning into a borrowed temporary.
/// Sound because the match arms only ever READ through the borrow (the
/// fallback send clones the argument itself), and nothing can write the
/// local while it is held: any closure that could reassign it would have
/// forced `Captured` (cell) storage, which -- like an Object-typed
/// (`Shadowed`, unboxed `Arc<Concrete>`) local or a `for`-var override --
/// stays on the clone path.
fn borrowable_operand(cx: &Ctx, id: NodeId) -> Option<proc_macro2::Ident> {
    let HirNode::LocalRead(name) = &cx.compiler.hir[id] else {
        return None;
    };
    if cx.for_var_override.as_ref().is_some_and(|(n, _)| n == name) {
        return None;
    }
    (super::hoisting::local_storage(cx, name) == super::hoisting::LocalStorage::Hoisted)
        .then(|| safe_ident(name))
}

/// Finishes a dynamic-dispatch (`send_value`) emission: `catch_break`
/// wraps the call ONLY when the call site itself carries a block -- a
/// `Signal::Break` can only ever target a block attached to THIS call, so
/// on a blockless call any arriving Break belongs to an OUTER block and
/// must keep propagating. (The unconditional wrap this replaced silently
/// ate a consumer's iteration-terminating break as it crossed a
/// `Yielder#<<` call inside an `Enumerator.new` generator -- turning
/// `infinite_enum.take(3)` into a hang.)
fn wrap_dynamic_result(has_block: bool, call: TokenStream) -> TokenStream {
    if has_block {
        quote! { zeo_rt::catch_break(#call)? }
    } else {
        quote! { (#call)? }
    }
}

/// Which `send` a `recv.send(...)` site resolves to.
///
/// CRuby has no `send` intrinsic: `send` is an ordinary method on `Kernel`,
/// so a class defining its own -- `BasicSocket#send` writing bytes,
/// `Ractor#send` passing a message, any `def send` of your own -- simply wins
/// the lookup by sitting earlier in the MRO, and only Kernel's reinterprets
/// the first argument as a method name. Resolve first, reinterpret second.
#[derive(PartialEq, Eq, Clone, Copy)]
enum SendTarget {
    /// Provably Kernel's: reinterpret `args[0]` here, at compile time.
    Kernel,
    /// Provably shadowed: emit an ordinary call named `send`.
    Shadowed,
    /// The receiver's class isn't statically known, so the MRO question goes
    /// to the runtime (`zeo_rt::send_dispatch_in`).
    Unknown,
}

/// [`SendTarget`] for one call site: walk the receiver's ancestors and see who
/// gets to `name` first. Both halves of the chain matter -- a user `def send`
/// and a builtin's own row -- so this consults the user method table AND the
/// generated builtin surface at each position.
fn resolve_send(cx: &Ctx, recv_id: NodeId, name: &str) -> SendTarget {
    let Some(cid) = super::expr::infer_any_class(cx, recv_id) else {
        return SendTarget::Unknown;
    };
    let defines = |anc: crate::compiler::ClassId| {
        cx.compiler
            .class(anc)
            .own_methods
            .iter()
            .any(|&sid| cx.compiler.scope(sid).name == name)
            || crate::builtin_surface::provides_instance_method(anc, name)
    };
    match cx
        .compiler
        .class(cid)
        .ancestors
        .iter()
        .copied()
        .find(|&a| defines(a))
    {
        // Nobody closer than Kernel/BasicObject: the reinterpreting one.
        Some(crate::compiler::KERNEL_CLASS | crate::compiler::BASIC_OBJECT_CLASS) | None => {
            SendTarget::Kernel
        }
        Some(_) => SendTarget::Shadowed,
    }
}
/// The value `self` has in the context currently being emitted, as a boxed
/// `RubyValue` ready for runtime dispatch: the concrete receiver inside an
/// instance method, the CLASS OBJECT inside a class method or class body
/// (both have no `self: Arc<Self>` binding), and the shared `main` object
/// at the top level. Every implicit-self dynamic-dispatch site routes
/// through this rather than re-deriving the rule.
pub(crate) fn boxed_implicit_self(cx: &Ctx) -> Option<TokenStream> {
    // Already a `RubyValue`, and the ONLY correct answer inside an escaping
    // block: `instance_exec` may have rebound the receiver, so an implicit-
    // self call there (`obj.instance_exec { helper }`) must dispatch on the
    // block's actual runtime self, not on whatever `self` meant lexically.
    if cx.self_is_dynamic {
        // Concrete UFCS: a dynamic self can be bound as `&RubyValue`, where
        // generic `Clone::clone` would clone the reference -- see
        // `codegen::expr`'s `SelfRef` arm for the full note.
        let slf = &cx.self_ident;
        return Some(quote! { zeo_rt::RubyValue::clone(&#slf) });
    }
    if let Some(cid) = cx.current_class {
        let slf = &cx.self_ident;
        return Some(if cx.compiler.value_backed(cid) {
            quote! { zeo_rt::RubyValue::clone(&#slf) }
        } else {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(Clone::clone(&#slf))) }
        });
    }
    // A class method/class body: self is the class object. `class_self`
    // (the receiver), not `defining_class` (the lexical origin) -- they
    // differ for an inherited or `extend`ed class method, and it is the
    // receiver that `self` means. Falls back to `defining_class` only for a
    // context that somehow has one without the other, which shouldn't
    // arise; keeping the old answer there is strictly safer than panicking.
    if let Some(cid) = cx.class_self.or(cx.defining_class) {
        let id = cid.0;
        return Some(quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) });
    }
    Some(quote! { zeo_rt::main_object() })
}
/// The implicit block argument for a call site, as an `Option<RubyValue>`
/// expression -- `Some(proc)` from a literal block (`emit_proc_value`) or a
/// forwarded `&existing_proc`, `None` when neither is present. Shared by
/// Path 1 (`codegen::params::emit_call_args`, gated on `needs_block`) and
/// Path 2 (`send`/`public_send` below, built unconditionally since the
/// dynamic target's own needs aren't known statically).
/// The G2 trailing-kwargs-hash convention's CALLER side: a dynamic call
/// site's keyword arguments as one `RubyValue::Hash` expression, appended
/// as the last element of the `send` argument slice. The receiving
/// trampoline (`params::dynamic_kwargs_binding`) pops and binds it when
/// the callee declares keywords; a keywordless callee sees it as an
/// ordinary trailing Hash (real Ruby's own pre-3.0-flavored collapse --
/// the documented no-`ruby2_keywords` approximation).
pub(super) fn emit_kwargs_trailing_hash(cx: &Ctx, kwargs: &[KwArg]) -> Option<TokenStream> {
    if kwargs.is_empty() {
        return None;
    }
    // Only reached on the Path-1 / non-splat route, where `emit_call`'s
    // routing guard has already sent any `**h` to `emit_splat_call` -- so
    // every element here is a literal `Pair`.
    let pairs = kwargs.iter().map(|kw| {
        let KwArg::Pair(k, v) = kw else {
            unreachable!("a `**` double-splat routes to emit_splat_call, never here")
        };
        let ke = emit_expr(cx, *k);
        let ve = {
            let e = emit_expr(cx, *v);
            box_if_object_typed(cx, *v, e)
        };
        quote! { (#ke, #ve) }
    });
    // kw-marked: the callee side reads the mark wherever keyword-vs-
    // positional-Hash changes behavior (`**nil`'s refusal, Struct member
    // binding) -- CRuby's `rb_keyword_given_p` carried on the value.
    Some(quote! {
        {
            let __kw = zeo_rt::hash_new(vec![#(#pairs),*]);
            zeo_rt::hash_mark_kwargs(&__kw);
            zeo_rt::RubyValue::Hash(__kw)
        }
    })
}
pub(super) fn emit_block_option(
    cx: &Ctx,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> TokenStream {
    match (block, block_arg) {
        (Some(b), None) => {
            let v = procs::emit_proc_value(cx, b);
            quote! { Some(#v) }
        }
        (None, Some(e)) => {
            let v = emit_expr(cx, e);
            // Boxed if Object-typed: `&obj` duck-types through `to_proc`,
            // and the converter takes a real `RubyValue`.
            let v = box_if_object_typed(cx, e, v);
            // `&expr` converts like real Ruby: Proc passes, Symbol becomes
            // `Symbol#to_proc` (`map(&:to_s)`), nil means no block.
            quote! { zeo_rt::block_arg_to_proc(#v)? }
        }
        (None, None) => quote! { None },
        (Some(_), Some(_)) => {
            panic!("a call can't pass both a literal block and a block-forwarding argument")
        }
    }
}
// Every one of these is a genuinely distinct piece of a call site's syntax
// (receiver/name/positional args/kwargs/literal block/forwarded block/safe-
// nav), not incidental duplication a struct would meaningfully collapse --
// bundling them would just move the same count behind one more layer.
#[allow(clippy::too_many_arguments)]
pub fn emit_call(
    cx: &Ctx,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    safe: bool,
    is_vcall: bool,
) -> TokenStream {
    let __bx = cx.box_id;
    // A call-site `*expr`/`**h` splat can't take any of the arity-checked
    // static paths below (the flattened argument COUNT isn't known until
    // runtime) -- see `emit_splat_call`'s docs for the always-dynamic
    // fallback this routes to instead. This is THE routing linchpin: any
    // `**h` `DoubleSplat` (or positional `*expr`) goes dynamic, so every
    // Path-1 consumer below only ever sees pure `Pair` kwargs. The common
    // case is unaffected: `args` unwraps to a plain `Vec<NodeId>` and every
    // fast path runs exactly as before.
    if kwargs.iter().any(|k| matches!(k, KwArg::DoubleSplat(_)))
        || args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
    {
        return splat::emit_splat_call(cx, receiver, name, args, kwargs, block, block_arg, safe);
    }
    let args: Vec<NodeId> = args
        .iter()
        .map(|a| match a {
            ArrayElem::Single(n) => *n,
            ArrayElem::Splat(_) => unreachable!("checked above"),
        })
        .collect();
    let args = &args[..];

    // `send(:eval, src, ...)` -- the reflective spelling, with a receiver or
    // without. CRuby routes it to the same private `Kernel#eval`, which reads
    // its LOCALS from the caller's frame either way and takes `self` from the
    // receiver, so it lowers exactly like a direct `eval` but with the
    // receiver supplying the Binding's `self`. Only `send`/`__send__`:
    // `public_send(:eval, ...)` is `NoMethodError` in CRuby, `eval` being
    // private, and is left on the ordinary dispatch path.
    if super::captures::is_sent_eval(cx.compiler, name, args)
        && kwargs.is_empty()
        && block.is_none()
        && block_arg.is_none()
    {
        let slf = match receiver {
            Some(r) => {
                let e = emit_expr(cx, r);
                box_if_object_typed(cx, r, e)
            }
            None => boxed_implicit_self(cx).expect("every context has an implicit self"),
        };
        let scope = emit_binding_with_self(cx, slf, "(eval)", 0);
        return emit_eval_in_scope(cx, scope, &args[1..]);
    }

    // A bare name inside an AOT-spliced `eval("literal")` that IS one of the
    // enclosing scope's locals. prism parsed the snippet on its own, so it
    // could only hand the name over as a vcall; Ruby resolves it as the local,
    // whose declaration the hoisting prelude has already emitted. Restricted
    // to the splice: outside one, a name prism called a vcall genuinely isn't
    // a local, because it would have parsed as a read if it were.
    if cx.in_eval_splice
        && is_vcall
        && receiver.is_none()
        && args.is_empty()
        && kwargs.is_empty()
        && block.is_none()
        && block_arg.is_none()
        && cx
            .binding_names
            .iter()
            .flat_map(|names| names.iter())
            .any(|n| n == name)
    {
        return super::hoisting::emit_local_read(cx, name);
    }

    // `binding.local_variable_get(:name)` -- with a literal symbol naming an
    // in-scope local, this is the one Binding operation with a fully STATIC
    // answer (the value of that local), so it lowers to a direct read,
    // materializing no Binding and deoptimizing no scope. It is also the only
    // way to read a reserved-word parameter (`def f(then:)` ->
    // `binding.local_variable_get(:then)`). The receiver must be a bare
    // `binding` call; a stored Binding (`b = binding; b.local_variable_get`)
    // goes through the real object.
    if name == "local_variable_get"
        && args.len() == 1
        && let Some(rid) = receiver
        && let HirNode::Call {
            receiver: None,
            name: bname,
            args: bargs,
            ..
        } = &cx.compiler.hir[rid]
        && bname == "binding"
        && bargs.is_empty()
        && let HirNode::SymbolLit(local) = &cx.compiler.hir[args[0]]
    {
        return super::hoisting::emit_local_read(cx, local);
    }

    // Implicit self / no receiver. `&.` is meaningless without a receiver,
    // so `safe` is irrelevant here.
    let Some(recv_id) = receiver else {
        // A bare name inside a `refine` block. Real Ruby activates a
        // refinement inside its own block, and `self` there is an instance
        // of the refined class -- so a sibling refined method is reachable
        // by name, while the holder itself is on nobody's ancestry and the
        // ordinary implicit-self walk would never find it.
        if let Some(holder) = cx.defining_class {
            let candidates: Vec<_> = cx
                .compiler
                .refinements_beside(holder)
                .into_iter()
                .filter(|&(_, h, _)| cx.compiler.refinement_defines(h, name))
                .collect();
            if !candidates.is_empty() {
                let recv = boxed_implicit_self(cx).expect("every context has an implicit self");
                let mut arg_exprs: Vec<TokenStream> = args
                    .iter()
                    .map(|&a| box_if_object_typed(cx, a, emit_expr(cx, a)))
                    .collect();
                arg_exprs.extend(emit_kwargs_trailing_hash(cx, kwargs));
                let blk = emit_block_option(cx, block, block_arg);
                let name_sym = super::pooled_sym(name);
                let pairs = candidates.iter().map(|&(target, holder, singleton)| {
                    let (target, holder) = (target.0, holder.0);
                    quote! { (zeo_rt::ClassId(#target), zeo_rt::ClassId(#holder), #singleton) }
                });
                return wrap_dynamic_result(
                    block.is_some() || block_arg.is_some(),
                    quote! {
                        zeo_rt::refined_send_in(
                            #__bx, &#recv, #name_sym, &[#(#arg_exprs),*], #blk,
                            &[#(#pairs),*],
                        )
                    },
                );
            }
        }
        // Receiver-less `eval(src[, binding[, file[, line]]])`: route straight
        // to the runtime eval VM, carrying THIS scope -- CRuby evaluates a bare
        // `eval` (or one given a `nil` binding) in the caller's own frame, so
        // the call site materializes a Binding of itself and hands it over,
        // which is what lets the source read and write the caller's locals. An
        // explicit binding argument wins over it. `emit_binding_value` carries
        // `cx.box_id` too, 0 at top level (an ordinary `Kernel#eval`) and the
        // box's id inside a `BoxScope` (`box.eval(dynamic_source)`), so this one
        // path serves both. The single string-LITERAL form never reaches here:
        // it lowered to `HirNode::Eval` (an AOT inline splice) at lower time.
        // A class that defines its OWN `eval` shadows `Kernel#eval` for
        // receiverless calls in its instance methods (ruby's ordinary method
        // resolution) -- skip the VM route and resolve the sibling instead.
        if name == "eval"
            && (1..=4).contains(&args.len())
            && kwargs.is_empty()
            && block.is_none()
            && block_arg.is_none()
            && !(cx.class_self.is_none()
                && cx
                    .ask_opt(super::class_query::ClassQuery::InChain("eval".to_string()))
                    .is_some_and(|a| a.yes()))
        {
            let scope = emit_binding_value(cx, "(eval)", 0);
            return emit_eval_in_scope(cx, scope, args);
        }
        // `public_send` on the IMPLICIT self still enforces visibility: real
        // Ruby checks the RESOLVED method entry's visibility with a
        // `CALL_PUBLIC` scope (`rb_method_call_status`, vm_eval.c:837), which
        // has nothing to do with whether the call site wrote a receiver. So a
        // receiverless `public_send(:private_one)` raises just like
        // `obj.public_send(:private_one)` -- route it through the same gated
        // runtime entry rather than letting it resolve as a sibling call.
        // Plain `send`/`__send__` stay on their existing path: they are
        // deliberately visibility-blind, so nothing needs intercepting.
        if name == "public_send" && !args.is_empty() {
            let recv = boxed_implicit_self(cx).expect("every context has an implicit self");
            let name_expr = emit_symbol_expr(cx, args[0]);
            let rest_args = args[1..].iter().map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            });
            let block_value = emit_block_option(cx, block, block_arg);
            let call = quote! {
                zeo_rt::send_value_public_in(#__bx, &#recv, #name_expr, &[#(#rest_args),*], #block_value)
            };
            return wrap_dynamic_result(block.is_some() || block_arg.is_some(), call);
        }
        // `respond_to?` on the IMPLICIT self routes through the same runtime
        // reflection the explicit-receiver fast path uses, boxing self via
        // `boxed_implicit_self` -- the class value inside a `def self.x`, the
        // instance inside an instance method. Without this, a class method's
        // implicit `respond_to?(:sibling_class_method)` fell through to
        // instance-method resolution and answered `false` where `self.respond_
        // to?` answered `true`.
        if name == "respond_to?" && kwargs.is_empty() && (args.len() == 1 || args.len() == 2) {
            let recv = boxed_implicit_self(cx).expect("every context has an implicit self");
            let sym_expr = emit_symbol_expr(cx, args[0]);
            let include_all = match args.get(1) {
                Some(&a) => {
                    let e = emit_expr(cx, a);
                    quote! { (#e).truthy() }
                }
                None => quote! { false },
            };
            return quote! {
                zeo_rt::RubyValue::Bool(zeo_rt::responds_to_or_missing(&#recv, #sym_expr, #include_all)?)
            };
        }
        // A no-receiver call to a sibling method on the CURRENT class (`foo(x)`
        // inside a method body, calling another method on the same object) --
        // composes directly onto the existing `self: Arc<Self>` receiver:
        // `self.clone()` (a cheap `Arc` refcount bump) IS the receiver
        // expression, and the rest is exactly Path 1 dispatch, reusing
        // `emit_call_args` the same way an ordinary explicit-receiver call
        // does (`dispatch`, below). Mirrors that function's own posture for a
        // statically-known class with no matching method: a clean compile-
        // time panic, not a dynamic `method_missing` fallback (the class is
        // known, so an undefined method here is provably an error).
        if let Some(cid) = cx.current_class {
            // Inside a REOPENED builtin's method, `self` is the
            // `__self: RubyValue` parameter: an implicit-self call to a
            // sibling reopen method is a direct free-function call, and any
            // OTHER name (`length` inside `Array#under_limit?` -- a native
            // builtin method, oracle-verified as implicit-self-reachable)
            // dispatches dynamically on `__self` through `send_value`, whose
            // value-methods-then-curated-tables order resolves it exactly
            // like an explicit `self.length` would.
            if cx.ask(super::class_query::ClassQuery::ValueBacked).yes() {
                let slf = &cx.self_ident;
                if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
                    let scope = cx.compiler.scope(sid);
                    let mod_ident = super::ident::class_ident(cx.compiler, cid);
                    let method_ident = safe_ident(name);
                    return super::params::emit_call_args_to(
                        cx,
                        &super::params::Callee::FreeFn {
                            path: quote! { #mod_ident::#method_ident },
                            recv: quote! { Clone::clone(&#slf) },
                        },
                        name,
                        &scope.params,
                        args,
                        kwargs,
                        block,
                        block_arg,
                        scope.needs_block_param(),
                        crate::codegen::scope_frame_guard(cx.compiler, scope, false),
                    );
                }
                // The universal Kernel forms resolve here too -- `puts`/
                // `proc { }`/`__method__` inside a reopened builtin's (or a
                // top-level) method body are Kernel calls, not methods of
                // the receiver, and the dynamic fallback below would miss
                // them at runtime.
                if !kernel_name_shadowed(cx, name)
                    && let Some(tokens) = kernel::emit_universal_implicit_form(
                        cx, name, args, kwargs, block, block_arg,
                    )
                {
                    return tokens;
                }
                let arg_exprs = args.iter().map(|&a| {
                    let e = emit_expr(cx, a);
                    box_if_object_typed(cx, a, e)
                });
                // Keyword args ride as one trailing Hash (the G2
                // convention) -- the callee's trampoline binds it.
                let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
                let block_value = emit_block_option(cx, block, block_arg);
                let name_sym = super::pooled_sym(name);
                let dyn_call = quote! {
                    zeo_rt::send_value_in(#__bx,
                        &#slf,
                        #name_sym,
                        &[#(#arg_exprs,)* #(#kw_hash,)*],
                        #block_value,
                    )
                };
                return wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
            }
            // The Path 1 direct call needs `self` to BE an `Arc<Concrete>` of
            // this class -- true in a method body, false inside an escaping
            // block, whose self is a `RubyValue` parameter that
            // `instance_exec` may have pointed at another class entirely.
            // There, fall through to the dynamic dispatch below: the sibling
            // method is then resolved against the receiver actually passed,
            // which is the whole point of rebinding.
            //
            // ...and the same two runtime-overlay questions the EXPLICIT-
            // receiver Path 1 asks (see the `recv_class.filter` further down).
            // A receiverless call is a send to `self` like any other, and
            // `self` may carry a per-object singleton method that shadows the
            // class's: `f.define_singleton_method(:close) { }` was honoured by
            // `f.close` and by `self.close`, and not by the bare `close`
            // written in the method next door.
            if let Some((_, sid)) = cx
                .compiler
                .method_in_chain(cid, name)
                .filter(|_| !cx.self_is_dynamic)
                .filter(|_| {
                    !cx.compiler.may_be_undefined_at_runtime(cid, name)
                        && !cx.compiler.may_be_patched_at_runtime(name)
                })
            {
                let scope = cx.compiler.scope(sid);
                let slf = &cx.self_ident;
                let recv_expr = quote! { Clone::clone(&#slf) };
                return super::params::emit_call_args(
                    cx,
                    &recv_expr,
                    name,
                    &scope.params,
                    args,
                    kwargs,
                    block,
                    block_arg,
                    scope.needs_block_param(),
                    crate::codegen::scope_frame_guard(cx.compiler, scope, false),
                );
            }
            // A name that resolves only as an alias of an inherited BUILTIN
            // (`alias_method :raise!, :raise` -- no user `Scope` to call, see
            // `ClassInfo::builtin_aliases`). The `raise`/`fail` family is
            // parse-special (an aliased spelling lowers as an ordinary
            // `Call`, never a `HirNode::Raise`), so it substitutes HERE into
            // exactly what the source spelling emits -- `return Err(...)`
            // with the same static class checks and `cause:` handling. Every
            // OTHER builtin alias (`dup!` -> `dup`) needs no static form:
            // the dynamic fallback below reaches the registry's alias row
            // (`register_alias`), which rewrites the name and re-dispatches.
            if !cx.self_is_dynamic
                && block.is_none()
                && block_arg.is_none()
                && let Some(target) = cx.compiler.builtin_alias_target(cid, name)
                && (target == "raise" || target == "fail")
            {
                let cause = match kwargs {
                    [] => Some(crate::hir::RaiseCause::Absent),
                    [KwArg::Pair(k, v)]
                        if matches!(&cx.compiler.hir[*k],
                                    HirNode::SymbolLit(s) if s == "cause") =>
                    {
                        Some(crate::hir::RaiseCause::Explicit(*v))
                    }
                    // Any other keyword is `raise`'s ArgumentError --
                    // let the dynamic path report it at runtime.
                    _ => None,
                };
                if let Some(cause) = cause
                    && args.len() <= 3
                {
                    return super::expr::emit_raise(cx, args, &cause);
                }
            }
        }
        // A no-receiver call from WITHIN another CLASS method's own body
        // (`current_class` is `None` there -- no concrete `self` receiver
        // exists, see `codegen::mod::emit_class_method_fn`'s docs) to a
        // SIBLING class method on the same class/module (`def self.a;
        // b; end` calling `def self.b`, or the equivalent inside `class <<
        // self`) -- resolved the same way `ClassName.foo(...)` is
        // (`Compiler::class_method_in_chain`), dispatched as a direct
        // associated-function call (`Target::b(...)`). A prior version of
        // this function had no such branch at all, meaning `class << self`
        // blocks whose methods called each other implicitly (the common,
        // idiomatic reason to write several class methods together) always
        // panicked.
        //
        // Resolved against `class_self` (the RECEIVER class), NOT
        // `defining_class` (where the body was written): an implicit-self
        // call is a send to `self`, and in a class method `self` is the
        // class it was CALLED on. The two differ exactly when the method is
        // inherited or `extend`ed in, and using the lexical one there was
        // silently wrong in both directions -- oracle-verified:
        //   - `class Base; def self.create; new; end; end; Sub.create`
        //     built a Base, not a Sub;
        //   - `module H; def helped; name; end; end; class Ext; extend H;
        //     end; Ext.helped` answered "Helper", not "Ext".
        // Neither raised; both just quietly produced the wrong object.
        if cx.current_class.is_none()
            && let Some(defining) = cx.class_self.or(cx.defining_class)
        {
            if cx.compiler.class_method_in_chain(defining, name).is_some() {
                return reflect::emit_class_method_call_on(
                    cx, defining, name, args, kwargs, block, block_arg,
                );
            }
            // A bare `new` inside a class method (`def self.create;
            // new; end`) constructs the class itself -- `self` there IS
            // the class, so `new` resolves like `Self.new` (real
            // Ruby's rule; checked after the sibling lookup so a user
            // `def self.new` override wins).
            if name == "new"
                && !cx.compiler.class(defining).is_module
                && !cx.compiler.class(defining).is_builtin
                && kwargs.is_empty()
                && block.is_none()
                && block_arg.is_none()
            {
                // Boxed: this Call node infers as `Poly` (only a
                // literal `HirNode::New` infers `Object(cid)`), so the
                // expression must be a `RubyValue`.
                let ctor = new::emit_new(cx, &cx.compiler.fq_name(defining), args, kwargs, None);
                // A native-backed class has no struct to `new_handle`
                // -- `emit_new` already yields a fully-boxed `RubyValue`
                // built by the runtime.
                if cx.compiler.is_native_backed(defining) {
                    return ctor;
                }
                let class_ident = super::ident::class_ident(cx.compiler, defining);
                return quote! {
                    zeo_rt::RubyValue::Object(#class_ident::new_handle(#ctor))
                };
            }
        }
        // A TOP-LEVEL-defined method -- a private instance method on
        // `Object`, real Ruby's rule. Reachable via implicit self from the
        // top level (receiver: the runtime `main` object) and from a class
        // method's body (receiver: the class value -- a class object is
        // itself an Object instance, so Object's methods are genuinely in
        // its chain). Inside ordinary INSTANCE methods this branch never
        // fires: `mro::materialize` spread the same method into the class
        // itself, so the `current_class` branch above already resolved it
        // (with `@ivar`s correctly landing on that class's own struct).
        // Checked BEFORE the Kernel functions below so a top-level
        // `def puts` overrides the built-in, same as a sibling method would.
        if cx.current_class.is_none()
            && let Some((_, sid)) = cx
                .compiler
                .method_in_chain(crate::compiler::OBJECT_CLASS, name)
        {
            let scope = cx.compiler.scope(sid);
            let mod_ident = super::ident::class_ident(cx.compiler, crate::compiler::OBJECT_CLASS);
            let method_ident = safe_ident(name);
            // `boxed_implicit_self` IS this rule ("self here, boxed"),
            // including the subtle case where, inside an escaping
            // block, it answers the block's own receiver, so a
            // top-level def called from an `instance_exec`'d block runs
            // against the rebound self rather than always `main`.
            let recv = boxed_implicit_self(cx).expect("boxed_implicit_self is total");
            return super::params::emit_call_args_to(
                cx,
                &super::params::Callee::FreeFn {
                    path: quote! { #mod_ident::#method_ident },
                    recv,
                },
                name,
                &scope.params,
                args,
                kwargs,
                block,
                block_arg,
                scope.needs_block_param(),
                crate::codegen::scope_frame_guard(cx.compiler, scope, false),
            );
        }
        // The Kernel FUNCTIONS: the print family (multi-arg
        // now), conversions, rand/srand, throw, sleep, exit/abort --
        // checked AFTER sibling method resolution (a user `def puts`/`def
        // Integer` wins, real Ruby's rule). Capitalized-name
        // conversion calls WITH arguments parse as ordinary CallNodes, so
        // there's no ClassRef ambiguity.
        // ...unless the enclosing class defines the name itself -- see
        // [`kernel_name_shadowed`].
        if !kernel_name_shadowed(cx, name)
            && let Some(tokens) =
                kernel::emit_universal_implicit_form(cx, name, args, kwargs, block, block_arg)
        {
            return tokens;
        }
        // `catch(:tag) { ... }` -- the one Kernel function that takes its
        // block as a first-class value.
        if name == "catch"
            && args.len() == 1
            && kwargs.is_empty()
            && let Some(b) = block
        {
            let tag = emit_expr(cx, args[0]);
            let blk = procs::emit_proc_value(cx, b);
            return quote! { zeo_rt::kernel_catch(#tag, #blk)? };
        }
        // `to_enum(:meth, *args)` / `enum_for` on the implicit self:
        // routed through dynamic dispatch, whose Kernel row builds
        // the Enumerator over the boxed receiver -- what the Struct
        // template's `return to_enum(:each) unless block_given?` compiles
        // to.
        if (name == "to_enum" || name == "enum_for")
            && kwargs.is_empty()
            && block.is_none()
            && block_arg.is_none()
            && cx.current_class.is_some()
        {
            // `boxed_implicit_self` rather than a local box: it checks
            // `self_is_dynamic` FIRST, which is what a method emitted
            // into an `__own_` bridge container needs -- there `self`
            // arrives as a `RubyValue` parameter even though
            // `current_class` names an ordinary struct-backed class, and
            // boxing it again is a type error. prism's `Pattern#scan`
            // (`return to_enum(:scan, root) unless block_given?`) is one
            // of forty-odd such methods irb pulls in.
            let boxed = boxed_implicit_self(cx).expect("a method context has an implicit self");
            let arg_exprs: Vec<TokenStream> = args
                .iter()
                .map(|&a| {
                    let e = emit_expr(cx, a);
                    box_if_object_typed(cx, a, e)
                })
                .collect();
            let name_sym = super::pooled_sym(name);
            return quote! {
                zeo_rt::send_value_in(#__bx,
                    &#boxed,
                    #name_sym,
                    &[#(#arg_exprs),*],
                    None,
                )?
            };
        }
        // Nothing static matched: sibling methods, top-level defs, the
        // Kernel functions and the `proc`/`at_exit`/`__method__`/`method`
        // forms have all been tried. Rather than rejecting at compile time,
        // dispatch on the implicit receiver through the runtime -- which is
        // both what real Ruby does and strictly more faithful than a
        // panic: it resolves the Kernel/Object methods that have no static
        // form here (`send`, `respond_to?`, `instance_variable_get`,
        // `freeze`, `tap`, ...), and a genuinely undefined name raises
        // NoMethodError at the moment the call runs -- so, as in CRuby,
        // an unreachable bad call stays silent.
        let recv = boxed_implicit_self(cx).expect("every context has an implicit self");
        let mut arg_exprs: Vec<TokenStream> = args
            .iter()
            .map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            })
            .collect();
        // Keyword arguments ride the G2 trailing-Hash convention.
        arg_exprs.extend(emit_kwargs_trailing_hash(cx, kwargs));
        let blk = emit_block_option(cx, block, block_arg);
        // A bareword VCALL that misses must raise NameError, not NoMethodError
        // (it could have been a local). is_vcall implies no args and no block.
        let name_sym = super::pooled_sym(name);
        if is_vcall {
            return quote! {
                zeo_rt::send_value_vcall_in(#__bx, &#recv, #name_sym)?
            };
        }
        return quote! {
            zeo_rt::send_value_in(#__bx,
                &#recv,
                #name_sym,
                &[#(#arg_exprs),*],
                #blk,
            )?
        };
    };

    // `ClassName.foo(...)` / `ModuleName.foo(...)` -- a call on the
    // class/module itself, not an instance (see `HirNode::ClassRef`'s
    // docs). Never a `RubyValue`, so this must be intercepted before the
    // ordinary `emit_expr(cx, recv_id)` receiver-evaluation path below ever
    // sees it (mirrors `HirNode::Block`'s "only reached via the Call that
    // invokes it" pattern).
    //
    // ONLY when `target_name` is an ACTUALLY-REGISTERED class/module --
    // `ClassRef` is also how an ORDINARY bare constant read lowers (see its
    // own docs: "used as a plain VALUE, ... an ordinary lexically-scoped
    // constant READ" when the name isn't a class), so `MAX.+(1)` (the
    // `MAX += 1` compound-assignment desugar, where `MAX` is a plain
    // Integer constant, not a class) must NOT take this branch. Treating
    // ANY `ClassRef` receiver as a class-method call would make compound
    // assignment (`+=`/`-=`/etc., every operator except `||=`) on a
    // non-class constant panic with a confusing "unknown class/module"
    // error instead of reading its actual value.
    // The concurrency builtins' constructors and `Fiber.yield` --
    // intercepted ahead of the generic class-method branch
    // below (which would reject the block / find no such class method). A
    // `Fiber.new`/`Thread.new` block becomes an ordinary escaping `Proc`
    // via `emit_proc_value` -- the same capture machinery every other
    // escaping block uses, so captured locals/`self` compose for free. All
    // FiberError construction happens here, not in `zeo_rt::fiber_*`
    // (the `array_set`->`IndexError` division of labor; messages verbatim
    // from CRuby `cont.c`).
    // The receiver names a class either bare (`Proc`) or as an absolute
    // top-level path (`::Proc`, which lowers to a QualifiedConstRead against
    // Object) -- both feed the builtin `.new`/etc. special cases below.
    let class_target: Option<&str> = match &cx.compiler.hir[recv_id] {
        HirNode::ClassRef(n) => Some(n.as_str()),
        HirNode::QualifiedConstRead(scope, n) if scope == "Object" => Some(n.as_str()),
        _ => None,
    };
    // A refined name on the CLASS itself, at a site some `using` covers --
    // `refine Range.singleton_class` makes `Range.from(...)` a refined call,
    // and `refine Time.class()` (= `refine Class`) refines every class
    // receiver. Checked before every class-method special case and the static
    // resolution below, which would otherwise devirtualize straight past the
    // runtime match (the same reason `dispatch` asks first). The receiver
    // emits as a first-class `RubyValue::Class`, so the ordinary refined
    // entry point serves unchanged, and a candidate that doesn't match at
    // runtime falls back to an ordinary (dynamic) class-method send.
    if class_target.is_some() {
        let recv_expr = emit_expr(cx, recv_id);
        if let Some(tokens) =
            emit_refined_call(cx, recv_id, name, args, kwargs, block, block_arg, &recv_expr)
        {
            return tokens;
        }
    }
    // `Ractor.new(*args, name: ...) { |*params| }` -- handled AHEAD of the
    // kwargs-empty class-target block so the `name:` keyword reaches it.
    // Block ISOLATION is enforced HERE, at compile time (the capture set is
    // statically known), strictly earlier than CRuby's own
    // Proc-creation-time `Ractor::IsolationError`. Args cross the boundary
    // at runtime (shareable-by-reference or deep-copied; a rejection raises
    // the same typed error a `#send` would).
    if class_target == Some("Ractor") && name == "new" && !safe {
        let mut name_kwarg: Option<TokenStream> = None;
        for kw in kwargs {
            match kw {
                KwArg::Pair(k, v) if matches!(&cx.compiler.hir[*k], HirNode::SymbolLit(s) if s == "name") =>
                {
                    let e = emit_expr(cx, *v);
                    name_kwarg = Some(super::expr::box_if_object_typed(cx, *v, e));
                }
                _ => {
                    return crate::codegen::unsupported(
                        "`Ractor.new` supports only the `name:` keyword (zeo limitation)",
                    );
                }
            }
        }
        let Some(block_id) = block else {
            if block_arg.is_some() {
                return crate::codegen::unsupported(
                    "`Ractor.new` requires a literal block (zeo limitation)",
                );
            }
            return raise::emit_missing_block_raise(cx, "Ractor");
        };
        let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
            panic!(
                "internal error: a Block node should only be reached via the Call that invokes it"
            );
        };
        let block_caps =
            super::captures::block_captures(cx.compiler, params, body, cx.self_class());
        // `block_captures` reports every referenced non-param
        // name, INCLUDING the block's own locals (`msg =
        // Ractor.receive` -- found the hard way). An outer-scope
        // access is a name that's either a genuine shared
        // capture (in `cx.captured_locals`) or one this block
        // never assigns itself (an enclosing param/block-local).
        let mut assigned_here = Vec::new();
        for &n in body {
            super::hoisting::collect_locals(cx.compiler, n, &mut assigned_here);
        }
        let assigned_here: std::collections::HashSet<&String> = assigned_here.iter().collect();
        if let Some(outer) = block_caps
            .locals
            .iter()
            .filter(|n| cx.captured_locals.contains(*n) || !assigned_here.contains(n))
            .min()
        {
            return crate::codegen::unsupported(format!(
                "can not isolate a Proc because it accesses outer variables ({outer})"
            ));
        }
        if block_caps.self_captured {
            return crate::codegen::unsupported(
                "can not isolate a Proc because it accesses instance variables of the enclosing object",
            );
        }
        let proc = procs::emit_proc_value(cx, block_id);
        let arg_exprs: Vec<TokenStream> = args
            .iter()
            .map(|&a| {
                let e = emit_expr(cx, a);
                super::expr::box_if_object_typed(cx, a, e)
            })
            .collect();
        // The `name:` kwarg's runtime value (nil = unnamed, like CRuby; a
        // non-String raises TypeError inside `ractor_new`).
        let name_tok = match name_kwarg {
            Some(e) => quote! {
                match (#e) {
                    zeo_rt::RubyValue::Nil => None,
                    __n => Some(__n),
                }
            },
            None => quote! { None },
        };
        // The call site, for `#inspect`'s `#<Ractor:#2 file.rb:4 ...>` slot.
        let loc_tok = match crate::codegen::source_location(cx.compiler, block_id) {
            Some((file, line)) => quote! { Some(format!("{}:{}", #file, #line)) },
            None => quote! { None },
        };
        return quote! {
            match zeo_rt::ractor_new(#proc, vec![#(#arg_exprs),*], #name_tok, #loc_tok) {
                Ok(__r) => __r,
                Err(__sig) => return Err(__sig),
            }
        };
    }

    if let Some(target_name) = class_target
        && !safe
        && kwargs.is_empty()
    {
        match (target_name, name) {
            // `Proc.new { ... }` IS its block (CRuby: `proc_new` just
            // wraps the given block) -- the same value `proc { ... }`
            // builds, so it routes to the same emitter and carries the
            // same arity/lambda? metadata. Blockless `Proc.new` is an
            // ArgumentError in real Ruby; it falls through to the
            // builtin-`.new` rejection below rather than miscompiling.
            ("Proc", "new") if block.is_some() && args.is_empty() => {
                return procs::emit_proc_value(cx, block.expect("checked is_some"));
            }
            ("Fiber", "new") => {
                // `&proc` is the same conversion every other call site's
                // block argument goes through (the Thread.new arm below is
                // the model); `&nil` is "no block", which raises exactly as
                // a missing literal one does.
                let proc = match (block, block_arg) {
                    (Some(block_id), _) => procs::emit_proc_value(cx, block_id),
                    (None, Some(arg)) => {
                        let v = emit_expr(cx, arg);
                        let v = box_if_object_typed(cx, arg, v);
                        let missing = raise::emit_simple_error(
                            cx,
                            "ArgumentError",
                            "tried to create Proc object without a block",
                        );
                        quote! {
                            match zeo_rt::block_arg_to_proc(#v)? {
                                Some(__p) => __p,
                                None => Err(zeo_rt::Signal::Raise(#missing))?,
                            }
                        }
                    }
                    (None, None) => return raise::emit_missing_block_raise(cx, "Fiber"),
                };
                return quote! { zeo_rt::fiber_new(#proc) };
            }
            ("Fiber", "yield") if block.is_none() && block_arg.is_none() => {
                let arg_exprs: Vec<TokenStream> = args
                    .iter()
                    .map(|&a| {
                        let e = emit_expr(cx, a);
                        super::expr::box_if_object_typed(cx, a, e)
                    })
                    .collect();
                let root_error =
                    raise::emit_fiber_error(cx, "attempt to yield on a not resumed fiber");
                return quote! {
                    match zeo_rt::fiber_yield(vec![#(#arg_exprs),*]) {
                        zeo_rt::FiberYield::Value(__v) => __v,
                        // `Fiber#raise` injected an exception at this yield.
                        zeo_rt::FiberYield::Raise(__e) => return Err(zeo_rt::Signal::Raise(__e)),
                        zeo_rt::FiberYield::Root => return Err(zeo_rt::Signal::Raise(#root_error)),
                    }
                };
            }
            // `Thread.new(*args) { |*params| }` -- constructor args pass
            // through to the block's params, matching CRuby.
            ("Thread", "new") => {
                // `&proc` is the same conversion every other call site's
                // block argument goes through -- drb's
                // `Thread.new(&method(:main_loop))` needs it -- so a
                // missing block is the only remaining error.
                let proc = match (block, block_arg) {
                    (Some(block_id), _) => procs::emit_proc_value(cx, block_id),
                    (None, Some(arg)) => {
                        let v = emit_expr(cx, arg);
                        let v = box_if_object_typed(cx, arg, v);
                        // `&nil` is "no block", which `Thread.new` rejects
                        // exactly as a missing literal one does.
                        let missing = raise::emit_simple_error(
                            cx,
                            "ThreadError",
                            "must be called with a block",
                        );
                        quote! {
                            match zeo_rt::block_arg_to_proc(#v)? {
                                Some(__p) => __p,
                                None => Err(zeo_rt::Signal::Raise(#missing))?,
                            }
                        }
                    }
                    (None, None) => return raise::emit_missing_block_raise(cx, "Thread"),
                };
                let arg_exprs: Vec<TokenStream> = args
                    .iter()
                    .map(|&a| {
                        let e = emit_expr(cx, a);
                        super::expr::box_if_object_typed(cx, a, e)
                    })
                    .collect();
                return quote! { zeo_rt::thread_new(#proc, vec![#(#arg_exprs),*]) };
            }
            // `Module.nesting` -- the lexical class/module chain at THIS
            // call site, innermost first. It is compile-time knowledge and
            // nothing else: a builtin row runs with no view of its
            // caller's lexical scope, so folding here is the only way to
            // answer anything but `[]`. `cref_chain` is outermost-first.
            ("Module", "nesting") if args.is_empty() && block.is_none() => {
                let ids = cx.cref_chain().iter().rev().map(|c| {
                    let id = c.0;
                    quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) }
                });
                return quote! {
                    zeo_rt::RubyValue::Array(zeo_rt::array_new(vec![#(#ids),*]))
                };
            }
            ("Mutex", "new") if args.is_empty() && block.is_none() => {
                return quote! { zeo_rt::mutex_new() };
            }
            ("Queue", "new") if args.is_empty() && block.is_none() => {
                return quote! { zeo_rt::queue_new() };
            }
            ("SizedQueue", "new") if args.len() == 1 && block.is_none() => {
                let n = emit_expr(cx, args[0]);
                return quote! {
                    zeo_rt::sized_queue_new((#n).as_int_unchecked())
                };
            }
            // `Ractor.new` is handled ahead of this kwargs-empty block (the
            // `name:` keyword must reach it).
            ("Ractor", "receive") if args.is_empty() && block.is_none() => {
                // Default-port receive on the CURRENT ractor -- the main
                // ractor's included (it blocks, fed by workers' sends).
                return quote! {
                    match zeo_rt::ractor_receive() {
                        Ok(__v) => __v,
                        Err(__sig) => return Err(__sig),
                    }
                };
            }
            ("Ractor", "make_shareable") if args.len() == 1 && block.is_none() => {
                let v = emit_expr(cx, args[0]);
                let v = super::expr::box_if_object_typed(cx, args[0], v);
                return quote! {
                    match zeo_rt::make_shareable_value(&(#v)) {
                        Ok(__v) => __v,
                        Err(__sig) => return Err(__sig),
                    }
                };
            }
            ("Ractor", "shareable?") if args.len() == 1 && block.is_none() => {
                let v = emit_expr(cx, args[0]);
                let v = super::expr::box_if_object_typed(cx, args[0], v);
                return quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::shareable(&(#v)))
                };
            }
            _ => {}
        }
    }

    if let Some(target_path) = super::expr::const_path_of(cx, recv_id)
        && let Some(target) = cx.resolve_class(&target_path)
    {
        // `const_get`/`const_defined?` with a literal name fold against
        // the compile-time registry (a literal-constant receiver has no
        // side effects to preserve).
        if let Some(folded) =
            reflect::try_const_reflection(cx, target, name, args, kwargs, block, block_arg)
        {
            return folded;
        }
        // Path 1 only when the class actually DEFINES a matching class
        // method. Anything else falls through to the generic dynamic path
        // with the receiver as a first-class Class VALUE:
        // `Widget == Widget`, `Widget.name`, `Widget.ancestors` resolve
        // in `send_value`'s Class arm, and a genuinely unknown method is a
        // real runtime NoMethodError ("for class Widget") -- real Ruby's
        // behavior.
        // Checked BEFORE the resolution below, because
        // `private_class_method :new` marks a name no body defines.
        if let Some(err) = visibility::enforce_class_method_visibility(cx, recv_id, target, name) {
            return err;
        }
        let is_static = cx.compiler.class_method_in_chain(target, name).is_some()
            && !cx.compiler.may_be_patched_at_runtime(name);
        if is_static {
            if safe {
                return crate::codegen::unsupported(
                    "safe-navigation on a class-method call isn't supported yet (zeo limitation)",
                );
            }
            return reflect::emit_class_method_call_on(
                cx, target, name, args, kwargs, block, block_arg,
            );
        }
    }

    // A receiver STATICALLY TYPED as a class value (`x = Widget;
    // x.new(...)` / `x.some_class_method`): same Path 1
    // dispatch a literal `Widget.` receiver gets, via the tracked
    // `TyKind::ClassObj`. The receiver expression is still evaluated for
    // side effects (a `let _ =` binding, like `is_a?`'s fold); anything
    // not statically resolvable falls through to the dynamic path
    // (`send_value`'s Class arm, including the registry constructor).
    if let TyKind::ClassObj(target) = infer(cx, recv_id) {
        // `x.class.const_get(:N)` / `.const_defined?(:N)` fold too, evaluating
        // the receiver expression for its side effects first.
        if let Some(folded) =
            reflect::try_const_reflection(cx, target, name, args, kwargs, block, block_arg)
        {
            let recv_expr = emit_expr(cx, recv_id);
            return quote! { { let _ = #recv_expr; #folded } };
        }
        if !safe && kwargs.is_empty() && block.is_none() && block_arg.is_none() {
            if let Some(err) =
                visibility::enforce_class_method_visibility(cx, recv_id, target, name)
            {
                return err;
            }
            if name == "new"
                && !cx.compiler.class(target).is_module
                && !cx.compiler.class(target).is_builtin
            {
                let recv_expr = emit_expr(cx, recv_id);
                let ctor = new::emit_new(cx, &cx.compiler.fq_name(target), args, kwargs, None);
                return quote! { { let _ = #recv_expr; #ctor } };
            }
            if cx.compiler.class_method_in_chain(target, name).is_some()
                && !cx.compiler.may_be_patched_at_runtime(name)
            {
                let recv_expr = emit_expr(cx, recv_id);
                let call = reflect::emit_class_method_call_on(
                    cx, target, name, args, kwargs, block, block_arg,
                );
                return quote! { { let _ = #recv_expr; #call } };
            }
        }
    }

    if safe {
        return path2::emit_safe_call(cx, recv_id, name, args, kwargs, block, block_arg);
    }

    let recv_expr = emit_expr(cx, recv_id);
    dispatch(
        cx, recv_id, name, args, kwargs, block, block_arg, &recv_expr, false,
    )
}
/// A call routed through the refinements active at it, or `None` -- the
/// answer for every site no `using` covers, and for every name the covering
/// refinements do not define, which is all but a handful of sites in the
/// rare program that refines at all.
///
/// A refined site gives up its static dispatch: whether the refinement
/// applies depends on the receiver's RUNTIME class, so the whole call goes
/// through one runtime entry point that tries the refined bodies and then
/// falls back to an ordinary send. That is a real cost, paid only where a
/// `using` and a refined name actually meet.
///
/// The lexical question is asked of the RECEIVER's span. A `using` cannot
/// be written between a receiver and the method name it carries, so the two
/// always sit on the same side of every activation.
#[allow(clippy::too_many_arguments)]
fn emit_refined_call(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    let active = cx.compiler.refinements_active_at(recv_id);
    if active.is_empty() {
        return None;
    }
    if let Some(tokens) =
        emit_refined_reflection(cx, recv_id, name, args, kwargs, block, recv_expr, &active)
    {
        return Some(tokens);
    }
    let candidates: Vec<_> = active
        .into_iter()
        .filter(|&(_, holder, _)| cx.compiler.refinement_defines(holder, name))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let __bx = cx.box_id;
    let recv = box_if_object_typed(cx, recv_id, recv_expr.clone());
    let mut arg_exprs: Vec<_> = args
        .iter()
        .map(|&a| box_if_object_typed(cx, a, emit_expr(cx, a)))
        .collect();
    arg_exprs.extend(emit_kwargs_trailing_hash(cx, kwargs));
    let blk = emit_block_option(cx, block, block_arg);
    let name_sym = super::pooled_sym(name);
    let pairs = candidates.iter().map(|&(target, holder, singleton)| {
        let (target, holder) = (target.0, holder.0);
        quote! { (zeo_rt::ClassId(#target), zeo_rt::ClassId(#holder), #singleton) }
    });
    Some(wrap_dynamic_result(
        block.is_some() || block_arg.is_some(),
        quote! {
            zeo_rt::refined_send_in(
                #__bx,
                &#recv,
                #name_sym,
                &[#(#arg_exprs),*],
                #blk,
                &[#(#pairs),*],
            )
        },
    ))
}

/// The three shapes that name a method at RUNTIME -- `send`, `respond_to?`
/// and `method`. Real Ruby honours a refinement through every one of them
/// (only `Module#instance_methods` stays blind to it), so at a site any
/// `using` covers they hand the WHOLE active set to the runtime: which
/// name is being asked about is not a compile-time fact here.
#[allow(clippy::too_many_arguments)]
fn emit_refined_reflection(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    recv_expr: &TokenStream,
    active: &[(crate::compiler::ClassId, crate::compiler::ClassId, bool)],
) -> Option<TokenStream> {
    if !kwargs.is_empty() || args.is_empty() {
        return None;
    }
    // Only Kernel's own `send`/`method`/`respond_to?` reinterprets its
    // first argument as a method name. A receiver whose class shadows one
    // of them -- or whose class isn't statically known, so the question
    // belongs to the runtime -- keeps its ordinary path.
    if resolve_send(cx, recv_id, name) != SendTarget::Kernel {
        return None;
    }
    let __bx = cx.box_id;
    let recv = box_if_object_typed(cx, recv_id, recv_expr.clone());
    let name_expr = emit_symbol_expr(cx, args[0]);
    let pairs: Vec<_> = active
        .iter()
        .map(|&(target, holder, singleton)| {
            let (target, holder) = (target.0, holder.0);
            quote! { (zeo_rt::ClassId(#target), zeo_rt::ClassId(#holder), #singleton) }
        })
        .collect();
    match name {
        "send" | "__send__" | "public_send" => {
            let rest = args[1..]
                .iter()
                .map(|&a| box_if_object_typed(cx, a, emit_expr(cx, a)));
            let blk = emit_block_option(cx, block, None);
            let public = name == "public_send";
            Some(wrap_dynamic_result(
                block.is_some(),
                quote! {
                    zeo_rt::refined_send_dynamic(
                        #__bx, &#recv, #name_expr, &[#(#rest),*], #blk,
                        &[#(#pairs),*], #public,
                    )
                },
            ))
        }
        "respond_to?" if args.len() <= 2 && block.is_none() => {
            let include_all = match args.get(1) {
                Some(&a) => {
                    let e = emit_expr(cx, a);
                    quote! { (#e).truthy() }
                }
                None => quote! { false },
            };
            Some(quote! {
                zeo_rt::RubyValue::Bool(zeo_rt::refined_responds_to(
                    &#recv, #name_expr, #include_all, &[#(#pairs),*],
                )?)
            })
        }
        "method" if args.len() == 1 && block.is_none() => Some(quote! {
            zeo_rt::refined_method(&#recv, #name_expr, &[#(#pairs),*])?
        }),
        _ => None,
    }
}

/// Kernel's reflection entries -- `send`/`public_send`, `respond_to?` and
/// `method` -- on a receiver whose class is NOT statically known. Two
/// questions are open at such a site and neither is decidable here: which of
/// the entries the receiver's chain actually resolves to (any class may
/// define its own), and whether a refinement active at the site answers the
/// name it was handed. One runtime entry point asks both.
///
/// The refinement set is read here rather than left to `emit_refined_call`,
/// which asks the same question of a KNOWN receiver: this site has to be
/// taken whether or not a `using` covers it, since the shadow question stands
/// on its own.
#[allow(clippy::too_many_arguments)]
fn emit_reflect_dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    let no_block = block.is_none() && block_arg.is_none();
    let entry = match name {
        "send" if !args.is_empty() => quote! { zeo_rt::Reflect::Send },
        "public_send" if !args.is_empty() => quote! { zeo_rt::Reflect::PublicSend },
        "respond_to?" if (1..=2).contains(&args.len()) && no_block && kwargs.is_empty() => {
            quote! { zeo_rt::Reflect::RespondTo }
        }
        "method" if args.len() == 1 && no_block && kwargs.is_empty() => {
            quote! { zeo_rt::Reflect::Method }
        }
        _ => return None,
    };
    if resolve_send(cx, recv_id, name) != SendTarget::Unknown {
        return None;
    }
    let __bx = cx.box_id;
    let recv = box_if_object_typed(cx, recv_id, recv_expr.clone());
    let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
    let all_args = args
        .iter()
        .map(|&a| box_if_object_typed(cx, a, emit_expr(cx, a)));
    let blk = emit_block_option(cx, block, block_arg);
    let pairs = cx
        .compiler
        .refinements_active_at(recv_id)
        .into_iter()
        .map(|(target, holder, singleton)| {
            let (target, holder) = (target.0, holder.0);
            quote! { (zeo_rt::ClassId(#target), zeo_rt::ClassId(#holder), #singleton) }
        });
    Some(wrap_dynamic_result(
        !no_block,
        quote! {
            zeo_rt::reflect_dispatch_in(
                #__bx,
                &#recv,
                #entry,
                &[#(#all_args,)* #(#kw_hash,)*],
                #blk,
                &[#(#pairs),*],
            )
        },
    ))
}

/// The actual dispatch decision (see the module's "Two dispatch paths"
/// docs), given an already-computed `recv_expr` for the receiver's runtime
/// value -- factored out of `emit_call` so `&.`'s nil-guard can wrap this
/// without the receiver expression being evaluated twice. `bypass_visibility`
/// is `true` only for the recursive call `send`/`public_send`'s own static-
/// resolution retry below makes (both already resolved their own visibility
/// rule -- `send` always bypasses, `public_send` already validated `Public`
/// before recursing) -- `false` for every ordinary explicit-receiver call,
/// which gets `enforce_visibility`'s real check.
// `block_arg` is only threaded through the `send`/`public_send` static-
// resolution retry below for now -- real Proc construction (which will
// genuinely consume it) lands later in this same phase.
#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    recv_expr: &TokenStream,
    bypass_visibility: bool,
) -> TokenStream {
    let __bx = cx.box_id;

    // A REFINED name at a site some `using` covers. Checked before every
    // fast path below, since a refinement may well override one of them
    // (`String#size` is both a refinable name and an inlined length read),
    // and before the blank-slate guard, which asks a question about the
    // target class the refinement deliberately never touched.
    if let Some(tokens) =
        emit_refined_call(cx, recv_id, name, args, kwargs, block, block_arg, recv_expr)
    {
        return tokens;
    }

    // A reflection entry on a receiver whose class only the runtime knows.
    // Ahead of every fold below, which would answer for Kernel's without
    // first asking whether Kernel's is the one this chain resolves to.
    if let Some(tokens) =
        emit_reflect_dispatch(cx, recv_id, name, args, kwargs, block, block_arg, recv_expr)
    {
        return tokens;
    }

    // BLANK SLATE (a `BasicObject` subclass): the Object/Kernel surface does
    // not exist on this receiver, so `class`/`inspect`/`respond_to?`/`dup`
    // and friends must raise NoMethodError rather than being answered.
    //
    // Every universal fast path below folds its answer from static type info
    // WITHOUT walking the ancestor chain, so each would happily reply for a
    // receiver that has no such method. Guarding once here -- before any of
    // them -- is both the smaller change and the faithful one: in CRuby the
    // blank slate is not a special case anywhere, just the consequence of
    // Kernel sitting BELOW the subclass's root in the chain
    // (`class.c:1853`), and one check placed at the top of dispatch says
    // exactly that.
    //
    // A method the user actually defined still resolves normally, as do
    // BasicObject's own (`==`, `equal?`, `!`, `__send__`, `instance_eval`,
    // ...), which reach their builtin table through the ordinary MRO walk.
    if let Some(cid) = infer_class(cx, recv_id)
        && cx.compiler.is_blank_slate(cid)
        && cx.compiler.method_in_chain(cid, name).is_none()
        && !crate::compiler::is_basic_object_method(name)
    {
        // The name is provably absent from the chain, so this IS a miss --
        // and a miss is `method_missing`'s whole job. A blank slate is
        // where that matters most: `BasicObject` has seven methods, so
        // catching the rest is what the class is FOR, and `respond_to?` is
        // itself one of the names that must arrive at the hook rather than
        // being answered universally.
        //
        // Dispatched here rather than left to the runtime because falling
        // through would reach the universal fast paths below, which answer
        // `class`/`inspect`/`respond_to?` from static type info without
        // ever walking the chain that does not contain them.
        if cx.compiler.method_in_chain(cid, "method_missing").is_some() {
            let mm = super::pooled_sym("method_missing");
            let name_sym = super::pooled_sym(name);
            let arg_exprs = args.iter().map(|&a| {
                let e = crate::codegen::expr::emit_expr(cx, a);
                crate::codegen::expr::box_if_object_typed(cx, a, e)
            });
            let blk = emit_block_option(cx, block, block_arg);
            // A statically-typed receiver is a bare `Arc<Concrete>`; the
            // dynamic channel takes a `RubyValue`.
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            return quote! {
                zeo_rt::send_value_in(#__bx,
                    &zeo_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)),
                    #mm,
                    &[zeo_rt::RubyValue::Symbol(#name_sym), #(#arg_exprs),*],
                    #blk,
                )?
            };
        }
        let describe = format!("an instance of {}", cx.compiler.class(cid).name);
        let msg = format!("undefined method '{name}' for {describe}");
        // Typed `?`-propagation rather than a bare `return`: this
        // expression can appear as the RECEIVER of a further call
        // (`a.dup.own`), where codegen takes a reference to it -- and
        // `&!` does not coerce to `&RubyValue`, so a diverging `return`
        // fails to type-check there. Naming the type keeps it usable in
        // every position while still carrying the raise outward.
        return quote! {
            Err::<zeo_rt::RubyValue, zeo_rt::Signal>(
                zeo_rt::raise_error("NoMethodError", #msg.to_string()),
            )?
        };
    }

    /// Whether `!recv` may still fold to a truthiness test. A program where
    /// nobody defines `!` folds everywhere; one that does still folds at a
    /// site whose receiver class is known not to define it.
    fn bang_folds(cx: &Ctx, recv_id: NodeId) -> bool {
        if cx.compiler.may_be_patched_at_runtime("!") {
            return false;
        }
        if !cx.compiler.defines_bang() {
            return true;
        }
        infer_class(cx, recv_id).is_some_and(|cid| cx.compiler.method_in_chain(cid, "!").is_none())
    }

    // Every fast path below (operators, collection `[]`/`length`, `.times`)
    // is a fixed, positional-only shape that has nowhere to put a keyword
    // argument -- gated on `kwargs.is_empty()` so a call that actually
    // passes one (a vanishingly rare shape for these, e.g. `a.+(x: 1)`
    // written with explicit dot-call syntax) falls through to the general
    // Path 1/Path 2 dispatch below instead of silently discarding it.
    let no_kwargs = kwargs.is_empty();

    // `!`/`not` -- Ruby truthiness on ANY value, not an `Int`-specific
    // operator (`!0`, `!""`, `!nil` are all valid and not equivalent),
    // so this is handled separately from the numeric tables below.
    // Declined where a user body could answer instead: see
    // `Compiler::defines_bang`. A plain truthiness test (`x ? a : b`) is
    // not this node and keeps folding, which is ruby's rule too.
    if no_kwargs && name == "!" && args.is_empty() && bang_folds(cx, recv_id) {
        let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
        return quote! { zeo_rt::RubyValue::Bool(!(#recv_boxed).truthy()) };
    }

    // `is_a?`/`kind_of?` against a literal class/module constant -- a real
    // ancestry check against the SAME linearized `ancestors` list `super`
    // consults (see `analyze::mro`), not zeo's own two-tier dispatch/
    // reflection split (confirmed to diverge on a module-of-module
    // diamond). Constant-folds to a literal `true`/`false` when the
    // receiver's class is statically known (the common Path 1 case);
    // otherwise falls back to a runtime `zeo_rt::is_a` check against the
    // receiver's actual runtime `class_id()`.
    if no_kwargs
        && (name == "is_a?" || name == "kind_of?")
        && args.len() == 1
        && let Some(target_name) = super::expr::const_path_of(cx, args[0])
    {
        // A top-level anchor `::Name` carries the scope "Object" (the
        // root); its name is an ordinary top-level class, so fall back to
        // resolving the tail when `Object::Name` doesn't resolve directly.
        let resolved = cx.resolve_class(&target_name).or_else(|| {
            target_name
                .strip_prefix("Object::")
                .and_then(|t| cx.resolve_class(t))
        });
        let Some(target) = resolved else {
            // A constant bound to a RUNTIME class (`Foo = Class.new`), or
            // one ALIASING a compiled class (`ALIAS = Base`): read the
            // constant -- through the owner its own PATH names, so a
            // qualified `M::ALIAS` is looked up under `M` -- and
            // ancestry-check the id it holds.
            let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
            let (scope, leaf) = super::expr::split_const_path(&target_name);
            let read = super::expr::emit_const_read(cx, scope, leaf);
            return quote! {
                {
                    let __rtc = #read;
                    match __rtc {
                        zeo_rt::RubyValue::Class(__tid) => zeo_rt::RubyValue::Bool(
                            zeo_rt::is_a_value(&(#recv_boxed), __tid)),
                        _ => return Err(zeo_rt::raise_error("TypeError", "class or module required".to_string())),
                    }
                }
            };
        };
        let target_id = target.0;
        // `infer_any_class` (not `infer_class`): a statically-known
        // BUILT-IN-typed receiver (e.g. `TyKind::Int`) must also
        // constant-fold here, not fall through to the runtime branch
        // below, which assumes `#recv_expr` is an actual `RubyValue` it
        // can call `.class_id()` on at runtime -- true either way now
        // (see the universal `RubyValue::class_id`), but the static
        // fold is strictly cheaper and matches every other statically-
        // known-class case in this function.
        let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
        return match infer_any_class(cx, recv_id) {
            Some(recv_class) => {
                let result = cx.compiler.class(recv_class).ancestors.contains(&target);
                // A statically-FALSE verdict against a MODULE is not final:
                // `obj.extend(M)` files M on one object's singleton and
                // leaves its class alone, so the compile-time ancestry
                // cannot see it. Only that one case falls back, and
                // `value_extends` answers it from a relaxed load in a
                // program that never extends anything. A class target
                // needs no fallback -- `extend` refuses a Class.
                if !result && cx.compiler.class(target).is_module {
                    return quote! {
                        zeo_rt::RubyValue::Bool(zeo_rt::value_extends(
                            &(#recv_boxed),
                            zeo_rt::ClassId(#target_id),
                        ))
                    };
                }
                // The `true`/`false` verdict is fully compile-time-known
                // here, but `recv_expr` itself must still be EVALUATED --
                // it may be an arbitrary expression with side effects
                // (`log_and_get(x).is_a?(Integer)`), and real Ruby always
                // evaluates a method call's receiver regardless of what
                // the call itself does with it. `let _ = ...;` forces
                // that evaluation without actually using the (statically
                // already-known) value, and as a side benefit keeps a
                // receiver-only-ever-used-via-`is_a?` local from
                // generating a spurious "value assigned but never read"
                // warning in the GENERATED program.
                quote! { { let _ = #recv_expr; zeo_rt::RubyValue::Bool(#result) } }
            }
            None => quote! {
                zeo_rt::RubyValue::Bool(zeo_rt::is_a_value(
                    &(#recv_boxed),
                    zeo_rt::ClassId(#target_id),
                ))
            },
        };
    }

    // `==`/`!=` on an OBJECT receiver with no matching user definition
    //: real Ruby's `Object#==` default (reference identity)
    // and its derived `!=`, via `rb_eq` on boxed operands -- which itself
    // dispatches a user `==` when one exists, so `a != b` correctly
    // negates a user-defined `==` even when no `!=` was written.
    // Receivers with a matching own definition fall through to ordinary
    // Path 1 dispatch below.
    if no_kwargs
        && (name == "==" || name == "!=")
        && args.len() == 1
        && block.is_none()
        && block_arg.is_none()
        && let TyKind::Object(cid) = infer(cx, recv_id)
        && cx.compiler.method_in_chain(cid, name).is_none()
    {
        let recv_boxed = super::expr::box_if_object_typed(cx, recv_id, recv_expr.clone());
        let arg = emit_expr(cx, args[0]);
        let arg = super::expr::box_if_object_typed(cx, args[0], arg);
        let negate = name == "!=";
        return quote! {
            zeo_rt::RubyValue::Bool(zeo_rt::rb_eq_checked(&(#recv_boxed), &(#arg))? != #negate)
        };
    }

    // `instance_of?` against a literal class/module constant
    // -- EXACT class identity, not ancestry (`w.instance_of?(Object)` is
    // false for a Widget); same static-fold-else-runtime shape as
    // `is_a?`/`kind_of?` above. A non-constant argument falls through to
    // the dynamic path (`send`/`send_value`'s Class-argument arms).
    if no_kwargs
        && name == "instance_of?"
        && args.len() == 1
        && let Some(target_name) = super::expr::const_path_of(cx, args[0])
    {
        // A top-level anchor `::Name` carries the scope "Object" (the
        // root); its name is an ordinary top-level class, so fall back to
        // resolving the tail when `Object::Name` doesn't resolve directly.
        let resolved = cx.resolve_class(&target_name).or_else(|| {
            target_name
                .strip_prefix("Object::")
                .and_then(|t| cx.resolve_class(t))
        });
        let Some(target) = resolved else {
            // A constant bound to a RUNTIME class (`Foo = Class.new`):
            // resolve it at runtime and check EXACT class identity.
            let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
            return quote! {
                {
                    let __rtc = zeo_rt::const_get(0, #target_name).ok_or_else(|| {
                        zeo_rt::raise_error("NameError", format!("uninitialized constant {}", #target_name))
                    })?;
                    match __rtc {
                        zeo_rt::RubyValue::Class(__tid) => zeo_rt::RubyValue::Bool(
                            (#recv_boxed).class_id() == __tid),
                        _ => return Err(zeo_rt::raise_error("TypeError", "class or module required".to_string())),
                    }
                }
            };
        };
        let target_id = target.0;
        return match infer_any_class(cx, recv_id) {
            Some(recv_class) => {
                let result = recv_class == target;
                quote! { { let _ = #recv_expr; zeo_rt::RubyValue::Bool(#result) } }
            }
            None => quote! {
                zeo_rt::RubyValue::Bool(
                    (#recv_expr).class_id() == zeo_rt::ClassId(#target_id),
                )
            },
        };
    }

    // `.class` -- universal, same override-respecting shape
    // as `freeze`/`dup` below (`class` is an ordinary overridable method
    // in real Ruby). Statically-known receivers fold to a Class literal
    // (still evaluating the receiver for side effects); Poly receivers ask
    // the value at runtime.
    if no_kwargs && name == "class" && args.is_empty() && block.is_none() && block_arg.is_none() {
        // `infer_any_class`, not just `TyKind::Object`: a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => reflect::any_builtin_overrides(cx, name),
        };
        if !user_defined {
            return match infer_any_class(cx, recv_id) {
                // Three builtins can't be constant-folded here. `Queue` may be a
                // subclass (`SizedQueue`, which types as `Queue` but carries its
                // own class id). `MatchData` is nilable: `str.match(re)` types as
                // MatchData but returns `nil` on no match, so `.class` must be
                // read at runtime (`"x".match(/z/).class == NilClass`).
                // `Proc` joins them ONLY in a program that defines a
                // `class P < Proc`: such an instance is still a
                // `RubyValue::Proc` and types as `Proc`, but carries its own
                // class id -- so folding would answer `Proc` for it. A program
                // with no such subclass keeps the fold, which is nearly all of
                // them.
                Some(cid)
                    if cid != zeo_abi::QUEUE_CLASS
                        && cid != zeo_abi::MATCH_DATA_CLASS
                        && !(cid == zeo_abi::PROC_CLASS && cx.compiler.has_proc_subclass()) =>
                {
                    let id = cid.0;
                    quote! { { let _ = #recv_expr; zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) } }
                }
                // `value_class` (not a bare `class_id()`) so a Struct/Data
                // member literally named `class` shadows Kernel#class -- the
                // runtime-minted accessor is invisible to this fold.
                _ => quote! { zeo_rt::value_class(&(#recv_expr)) },
            };
        }
    }

    // `respond_to?(:name)` -- a flat probe on the receiver's own already-
    // materialized method table (`zeo_rt::responds_to`; see its docs for
    // why no ancestor walk is needed, mirroring `send`'s own dispatch).
    // Works uniformly whether the receiver's class is statically known
    // (Object) or only known at runtime (Poly) -- unlike `is_a?` above,
    // there's no compile-time constant-fold here (a name could still resolve
    // differently at runtime for a `define_method`-extended class), so this
    // always calls into the registry.
    if no_kwargs
        && name == "respond_to?"
        && (args.len() == 1 || args.len() == 2)
        // A class carrying its OWN `respond_to?` answers with whatever it
        // likes, so the fold belongs behind the same chain question `send`
        // asks. The unknown-receiver half of it left above.
        && resolve_send(cx, recv_id, name) == SendTarget::Kernel
    {
        let sym_expr = emit_symbol_expr(cx, args[0]);
        // The optional second argument (`include_all`) opts private methods
        // back in -- absent means false, CRuby's default.
        let include_all = match args.get(1) {
            Some(&a) => {
                let e = emit_expr(cx, a);
                quote! { (#e).truthy() }
            }
            None => quote! { false },
        };
        // Box the receiver to a `RubyValue` and route through
        // `responds_to_value`, which also honors a per-object singleton method
        // keyed by object identity, so a class-id-only probe can't
        // see it. The singleton fast path (`is_live()`) means an ordinary
        // program pays only one predictable atomic here.
        let boxed_recv = super::expr::box_if_object_typed(cx, recv_id, recv_expr.clone());
        return quote! {
            zeo_rt::RubyValue::Bool(zeo_rt::responds_to_or_missing(&#boxed_recv, #sym_expr, #include_all)?)
        };
    }

    // `.nil?` -- universal, same override-respecting shape as
    // `freeze`/`frozen?` below (surfaced as a real need by the
    // queue-sentinel idiom, `break if q.pop.nil?`, on a Poly receiver). A
    // statically-known Object receiver is never nil (only `RubyValue::Nil`
    // is), but its receiver expression still evaluates for side effects.
    if no_kwargs && name == "nil?" && args.is_empty() {
        // `infer_any_class`, not just `TyKind::Object`: a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => reflect::any_builtin_overrides(cx, name),
        };
        if !user_defined {
            return match infer(cx, recv_id) {
                TyKind::Object(_) => {
                    quote! { { let _ = #recv_expr; zeo_rt::RubyValue::Bool(false) } }
                }
                _ => quote! { zeo_rt::RubyValue::Bool((#recv_expr).is_nil()) },
            };
        }
    }

    // `.freeze`/`.frozen?` -- universal `Kernel` methods, dispatched over
    // every receiver representation. A user class's OWN
    // `def freeze`/`def frozen?` override wins, matching real Ruby (they're
    // ordinary overridable `Kernel` methods) -- checked via the receiver's
    // materialized method table, falling through to ordinary Path 1
    // dispatch when one exists. Two receiver shapes: a statically-known
    // Object receiver is a bare `Arc<Concrete>` (flag reached via the
    // `RubyObject` trait, UFCS-qualified since generated programs don't
    // import the trait by name); everything else -- builtins and Poly -- is
    // already a `RubyValue`, handled by its own universal
    // `freeze_value`/`is_frozen` methods (see their docs for the
    // always-frozen-immediates / flagless-`Proc` tiering).
    // The ENV singleton OVERRIDES `dup`/`clone`/`freeze` to raise (you must
    // copy it via `ENV.to_h`), which only the runtime dispatch path -- its
    // identity check in `send_in` -- honors. A direct `ENV.<m>` must therefore
    // skip the universal value fast paths below and fall through to `send`.
    let recv_is_env =
        matches!(&cx.compiler.hir[recv_id], crate::hir::HirNode::ClassRef(n) if n == "ENV");

    if no_kwargs && (name == "freeze" || name == "frozen?") && args.is_empty() && !recv_is_env {
        // `infer_any_class`, not just `TyKind::Object`: a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => reflect::any_builtin_overrides(cx, name),
        };
        if !user_defined {
            return match infer(cx, recv_id) {
                TyKind::Object(cid) => {
                    let class_ident = super::ident::class_ident(cx.compiler, cid);
                    if name == "freeze" {
                        // Returns self, boxed -- `freeze`'s result is
                        // Poly-typed downstream (see `types.rs`), so the
                        // uniform `RubyValue` representation is the right
                        // one, exactly like `emit_boxed_new`'s.
                        quote! {
                            {
                                let __r = #recv_expr;
                                zeo_rt::RubyObject::set_frozen(&*__r);
                                zeo_rt::RubyValue::Object(#class_ident::new_handle(__r))
                            }
                        }
                    } else {
                        quote! {
                            zeo_rt::RubyValue::Bool(zeo_rt::RubyObject::is_frozen(&*(#recv_expr)))
                        }
                    }
                }
                _ => {
                    if name == "freeze" {
                        // Fallible: Queue/SizedQueue refuse to freeze
                        // (TypeError) -- see `freeze_value`'s docs.
                        quote! { (#recv_expr).freeze_value()? }
                    } else {
                        quote! { zeo_rt::RubyValue::Bool((#recv_expr).is_frozen()) }
                    }
                }
            };
        }
    }

    // `.instance_variable_get(:@x)` / `.instance_variable_set(:@x, v)` /
    // `.instance_variables` -- universal reflection over any receiver's named
    // ivars (an `Object`'s slots, or a class object's own ivars via
    // `civars`). A user override wins, the same fall-through the other
    // universal arms use. The receiver is boxed to a uniform `RubyValue` so
    // one runtime helper serves every representation.
    if no_kwargs
        && block.is_none()
        && block_arg.is_none()
        && matches!(
            (name, args.len()),
            ("instance_variable_get", 1) | ("instance_variable_set", 2) | ("instance_variables", 0)
        )
    {
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => reflect::any_builtin_overrides(cx, name),
        };
        if !user_defined {
            let boxed = match infer_class(cx, recv_id) {
                Some(cid) => {
                    let class_ident = super::ident::class_ident(cx.compiler, cid);
                    quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
                }
                None => quote! { (#recv_expr) },
            };
            return match name {
                "instance_variable_get" => {
                    let a = emit_expr(cx, args[0]);
                    quote! { zeo_rt::instance_variable_get(&#boxed, &(#a))? }
                }
                "instance_variable_set" => {
                    let a = emit_expr(cx, args[0]);
                    let v = emit_expr(cx, args[1]);
                    let v = box_if_object_typed(cx, args[1], v);
                    quote! { zeo_rt::instance_variable_set(&#boxed, &(#a), #v)? }
                }
                _ => quote! { zeo_rt::instance_variables(&#boxed) },
            };
        }
    }

    // `.dup`/`.clone` -- universal `Kernel` methods, same
    // override-respecting shape as `freeze`/`frozen?` above (they're
    // ordinary overridable `Kernel` methods in real Ruby). The single
    // semantic difference between the two -- `clone` copies the frozen
    // flag, `dup` doesn't -- is the `copy_frozen` flag threaded to
    // `RubyObject::dup_object` (statically-known Object receiver, a bare
    // `Arc<Concrete>`) or `RubyValue::dup_value` (builtins and Poly).
    // `clone(freeze: false)` keyword form: not supported (kwargs fall
    // through to the ordinary rejection paths).
    if no_kwargs && (name == "dup" || name == "clone") && args.is_empty() && !recv_is_env {
        // `infer_any_class`, not just `TyKind::Object`: a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        //
        // Also fall through when the class has a USER `initialize_copy`
        // (defining class != Object's default no-op): `dup`/`clone` must
        // run that hook, which only the runtime `Kernel#dup`/`#clone` path
        // does. Object's own default hook changes nothing, so a class without
        // an override keeps the unboxed fast path.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => {
                cx.compiler.method_in_chain(cid, name).is_some()
                    || matches!(
                        cx.compiler.method_in_chain(cid, "initialize_copy"),
                        Some((defining, _)) if defining != crate::compiler::OBJECT_CLASS
                    )
            }
            None => reflect::any_builtin_overrides(cx, name),
        };
        if !user_defined {
            let copy_frozen = name == "clone";
            // `clone` carries the singleton class (methods + extended
            // modules); `dup` drops it -- the same split the Kernel rows
            // make, which this fast path must not lose.
            if name == "clone" {
                return match infer(cx, recv_id) {
                    TyKind::Object(_) => quote! {
                        {
                            let __orig: zeo_rt::RObj = (#recv_expr).clone();
                            let __copy = zeo_rt::RubyObject::dup_object(&*__orig, true);
                            zeo_rt::copy_value_singletons(
                                &zeo_rt::RubyValue::Object(__orig),
                                &zeo_rt::RubyValue::Object(__copy.clone()),
                            );
                            zeo_rt::RubyValue::Object(__copy)
                        }
                    },
                    _ => quote! {
                        {
                            let __orig = #recv_expr;
                            let __copy = __orig.dup_value(true)?;
                            zeo_rt::copy_value_singletons(&__orig, &__copy);
                            __copy
                        }
                    },
                };
            }
            return match infer(cx, recv_id) {
                TyKind::Object(_) => {
                    // Boxed result, like `freeze`'s -- the copy's static
                    // class is knowable, but `dup` results flow into
                    // Poly-typed positions downstream (see `types.rs`).
                    quote! {
                        zeo_rt::RubyValue::Object(
                            zeo_rt::RubyObject::dup_object(&*(#recv_expr), #copy_frozen),
                        )
                    }
                }
                _ => quote! { (#recv_expr).dup_value(#copy_frozen)? },
            };
        }
    }

    // `Fiber#resume` / `Fiber#alive?` on a statically-known Fiber receiver
    //. `resume`'s error outcomes each become their own
    // CRuby-verbatim `FiberError`; an uncaught Ruby signal from inside the
    // fiber's body re-raises HERE, at the resumer -- exactly CRuby's
    // `cont.c:2914` behavior. A Poly-typed receiver falls through to the
    // generic Poly-`send` fallback below (which can't reach a Fiber -- the
    // same documented builtin-receiver `send` gap every other builtin has).
    if no_kwargs && infer(cx, recv_id) == TyKind::Fiber && block.is_none() && block_arg.is_none() {
        if name == "resume" {
            let arg_exprs: Vec<TokenStream> = args
                .iter()
                .map(|&a| {
                    let e = emit_expr(cx, a);
                    super::expr::box_if_object_typed(cx, a, e)
                })
                .collect();
            let dead = raise::emit_fiber_error(cx, "attempt to resume a terminated fiber");
            let uninit = raise::emit_fiber_error(cx, "uninitialized fiber");
            let double =
                raise::emit_fiber_error(cx, "attempt to resume a resumed fiber (double resume)");
            let cross = raise::emit_fiber_error(cx, "fiber called across threads");
            let unborn = raise::emit_fiber_error(cx, "cannot raise exception on unborn fiber");
            return quote! {
                match zeo_rt::fiber_resume(
                    &(#recv_expr).as_fiber_unchecked(),
                    vec![#(#arg_exprs),*],
                ) {
                    zeo_rt::FiberResume::Value(__v) => __v,
                    zeo_rt::FiberResume::RubyError(__sig) => return Err(__sig),
                    zeo_rt::FiberResume::Dead => {
                        return Err(zeo_rt::Signal::Raise(#dead))
                    }
                    zeo_rt::FiberResume::Uninitialized => {
                        return Err(zeo_rt::Signal::Raise(#uninit))
                    }
                    zeo_rt::FiberResume::DoubleResume => {
                        return Err(zeo_rt::Signal::Raise(#double))
                    }
                    zeo_rt::FiberResume::CrossThread => {
                        return Err(zeo_rt::Signal::Raise(#cross))
                    }
                    // `#raise`'s outcome alone; a resume/transfer never
                    // answers it -- exhaustiveness only.
                    zeo_rt::FiberResume::Unborn => {
                        return Err(zeo_rt::Signal::Raise(#unborn))
                    }
                }
            };
        }
        if name == "transfer" {
            // Symmetric transfer -- same outcome shape as `resume` (a transfer
            // BACK to root yields the value here; the error variants are the
            // same CRuby-verbatim FiberErrors).
            let arg_exprs: Vec<TokenStream> = args
                .iter()
                .map(|&a| {
                    let e = emit_expr(cx, a);
                    super::expr::box_if_object_typed(cx, a, e)
                })
                .collect();
            let dead = raise::emit_fiber_error(cx, "attempt to resume a terminated fiber");
            let uninit = raise::emit_fiber_error(cx, "uninitialized fiber");
            let double =
                raise::emit_fiber_error(cx, "attempt to resume a resumed fiber (double resume)");
            let cross = raise::emit_fiber_error(cx, "fiber called across threads");
            let unborn = raise::emit_fiber_error(cx, "cannot raise exception on unborn fiber");
            return quote! {
                match zeo_rt::fiber_transfer(
                    &(#recv_expr).as_fiber_unchecked(),
                    vec![#(#arg_exprs),*],
                ) {
                    zeo_rt::FiberResume::Value(__v) => __v,
                    zeo_rt::FiberResume::RubyError(__sig) => return Err(__sig),
                    zeo_rt::FiberResume::Dead => {
                        return Err(zeo_rt::Signal::Raise(#dead))
                    }
                    zeo_rt::FiberResume::Uninitialized => {
                        return Err(zeo_rt::Signal::Raise(#uninit))
                    }
                    zeo_rt::FiberResume::DoubleResume => {
                        return Err(zeo_rt::Signal::Raise(#double))
                    }
                    zeo_rt::FiberResume::CrossThread => {
                        return Err(zeo_rt::Signal::Raise(#cross))
                    }
                    // `#raise`'s outcome alone; a resume/transfer never
                    // answers it -- exhaustiveness only.
                    zeo_rt::FiberResume::Unborn => {
                        return Err(zeo_rt::Signal::Raise(#unborn))
                    }
                }
            };
        }
        if name == "alive?" && args.is_empty() {
            return quote! {
                zeo_rt::RubyValue::Bool(zeo_rt::fiber_alive(
                    &(#recv_expr).as_fiber_unchecked(),
                ))
            };
        }
    }

    // `Thread#join`/`#value`: both wait via
    // `zeo_rt::thread_outcome` (blocks until the thread finishes); an `Err` is the
    // thread's own uncaught signal, re-raised HERE in the joiner -- CRuby's
    // stored-exception semantics (`thread.c:1195`). `join` returns the
    // THREAD itself, `value` the block's result.
    if no_kwargs && infer(cx, recv_id) == TyKind::Thread && args.is_empty() && block.is_none() {
        if name == "join" {
            return quote! {
                {
                    let __t = (#recv_expr).as_thread_unchecked();
                    match zeo_rt::thread_outcome(&__t) {
                        Ok(_) => zeo_rt::RubyValue::Thread(__t),
                        Err(__sig) => return Err(__sig),
                    }
                }
            };
        }
        if name == "value" {
            return quote! {
                match zeo_rt::thread_outcome(&(#recv_expr).as_thread_unchecked()) {
                    Ok(__v) => __v,
                    Err(__sig) => return Err(__sig),
                }
            };
        }
    }

    // Ruby `Mutex` -- CRuby-verbatim ThreadError messages come
    // back from the runtime (`Err(&str)`), boxed into real exceptions here.
    // `lock`/`unlock` both return self, matching CRuby.
    if no_kwargs && infer(cx, recv_id) == TyKind::Mutex {
        let thread_error = super::expr::emit_boxed_new(
            cx,
            "ThreadError",
            vec![quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new(__msg.to_string())) }],
        );
        match (name, args.len(), block) {
            ("lock", 0, None) => {
                return quote! {
                    {
                        let __m = (#recv_expr).as_mutex_unchecked();
                        match zeo_rt::mutex_lock(&__m) {
                            Ok(()) => zeo_rt::RubyValue::Mutex(__m),
                            Err(__msg) => return Err(zeo_rt::Signal::Raise(#thread_error)),
                        }
                    }
                };
            }
            ("unlock", 0, None) => {
                return quote! {
                    {
                        let __m = (#recv_expr).as_mutex_unchecked();
                        match zeo_rt::mutex_unlock(&__m) {
                            Ok(()) => zeo_rt::RubyValue::Mutex(__m),
                            Err(__msg) => return Err(zeo_rt::Signal::Raise(#thread_error)),
                        }
                    }
                };
            }
            ("locked?", 0, None) => {
                return quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::mutex_locked(&(#recv_expr).as_mutex_unchecked()))
                };
            }
            ("owned?", 0, None) => {
                return quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::mutex_owned(&(#recv_expr).as_mutex_unchecked()))
                };
            }
            // `try_lock` -- acquire without blocking; true iff it was free.
            ("try_lock", 0, None) => {
                return quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::mutex_try_lock(&(#recv_expr).as_mutex_unchecked()))
                };
            }
            // `synchronize { }`: lock, run the block (an ordinary escaping
            // Proc), ALWAYS unlock -- including on a signal (an exception/
            // `break` inside the block must release the lock on its way
            // out), then re-propagate. `catch_break` first: `break` inside
            // `synchronize` exits it with the break's value, real Ruby
            // behavior.
            ("synchronize", 0, Some(block_id)) => {
                let proc = procs::emit_proc_value(cx, block_id);
                let lock_err = super::expr::emit_boxed_new(
                    cx,
                    "ThreadError",
                    vec![quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new(__msg.to_string())) }],
                );
                return quote! {
                    {
                        let __m = (#recv_expr).as_mutex_unchecked();
                        if let Err(__msg) = zeo_rt::mutex_lock(&__m) {
                            return Err(zeo_rt::Signal::Raise(#lock_err));
                        }
                        let __blk = (#proc).as_proc_unchecked();
                        let __r = zeo_rt::catch_break(__blk.call(&[]));
                        let _ = zeo_rt::mutex_unlock(&__m);
                        match __r {
                            Ok(__v) => __v,
                            Err(__sig) => return Err(__sig),
                        }
                    }
                };
            }
            _ => {}
        }
    }

    // `Queue`: `pop` blocks the calling thread; a closed
    // empty queue pops nil; push to a closed queue raises ClosedQueueError
    // -- all CRuby `thread_sync.c` semantics, verified in the plan addendum.
    if no_kwargs && infer(cx, recv_id) == TyKind::Queue && block.is_none() {
        match (name, args.len()) {
            ("push" | "<<" | "enq", 1) => {
                let v = emit_expr(cx, args[0]);
                let v = super::expr::box_if_object_typed(cx, args[0], v);
                let closed_err = super::expr::emit_boxed_new(
                    cx,
                    "ClosedQueueError",
                    vec![
                        quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new("queue closed".to_string())) },
                    ],
                );
                return quote! {
                    {
                        let __q = (#recv_expr).as_queue_unchecked();
                        let __v = #v;
                        match zeo_rt::queue_push(&__q, __v) {
                            Ok(()) => zeo_rt::RubyValue::Queue(__q),
                            Err(_) => return Err(zeo_rt::Signal::Raise(#closed_err)),
                        }
                    }
                };
            }
            ("pop" | "shift" | "deq", 0) => {
                // `queue_pop` is an interruption checkpoint (`Thread#kill`/
                // `#raise` delivery), so it returns `Result` -- propagate with
                // `?`, exactly like any other fallible builtin call.
                return quote! { zeo_rt::queue_pop(&(#recv_expr).as_queue_unchecked())? };
            }
            ("close", 0) => {
                return quote! {
                    {
                        let __q = (#recv_expr).as_queue_unchecked();
                        zeo_rt::queue_close(&__q);
                        zeo_rt::RubyValue::Queue(__q)
                    }
                };
            }
            ("closed?", 0) => {
                return quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::queue_closed(&(#recv_expr).as_queue_unchecked()))
                };
            }
            ("length" | "size", 0) => {
                return quote! {
                    zeo_rt::RubyValue::Int(zeo_rt::queue_len(&(#recv_expr).as_queue_unchecked()))
                };
            }
            ("empty?", 0) => {
                return quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::queue_len(&(#recv_expr).as_queue_unchecked()) == 0)
                };
            }
            // `SizedQueue#max` -- the bound (nil on an unbounded `Queue`).
            ("max", 0) => {
                return quote! {
                    match zeo_rt::queue_max(&(#recv_expr).as_queue_unchecked()) {
                        Some(__n) => zeo_rt::RubyValue::Int(__n),
                        None => zeo_rt::RubyValue::Nil,
                    }
                };
            }
            _ => {}
        }
    }

    // `Ractor` instance methods. NOTE: on a Ractor receiver,
    // `send` is the MESSAGE-passing method (as in real Ruby, where
    // `Ractor#send` shadows `Object#send`) -- this arm must stay ahead of
    // the generic dynamic-dispatch `send` handling further down.
    // `value`/`join` mirror Thread's (an uncaught signal re-raises in the
    // caller; join returns the Ractor itself).
    if infer(cx, recv_id) == TyKind::Ractor && block.is_none() && block_arg.is_none() {
        // `send`/`<<` alone also takes the `move:` kwarg (`send(obj,
        // move: true)` runs the real move traversal); any other kwarg shape
        // falls through like every other arm.
        if let ("send" | "<<", 1) = (name, args.len())
            && let Some(move_kw) = ractor_move_kwarg(cx, kwargs)
        {
            let v = emit_expr(cx, args[0]);
            let v = super::expr::box_if_object_typed(cx, args[0], v);
            let mv = match move_kw {
                Some(e) => quote! { (#e).truthy() },
                None => quote! { false },
            };
            // Typed `Signal` errors straight from the runtime
            // (`Ractor::ClosedError`/`Ractor::Error`/`Ractor::MovedError`)
            // -- no re-wrap.
            return quote! {
                {
                    let __r = (#recv_expr).as_ractor_unchecked();
                    match zeo_rt::ractor_send_mode(&__r, &(#v), #mv) {
                        Ok(()) => zeo_rt::RubyValue::Ractor(__r),
                        Err(__sig) => return Err(__sig),
                    }
                }
            };
        }
    }
    if no_kwargs && infer(cx, recv_id) == TyKind::Ractor && block.is_none() && block_arg.is_none() {
        match (name, args.len()) {
            ("value", 0) => {
                return quote! {
                    match zeo_rt::ractor_value(&(#recv_expr).as_ractor_unchecked()) {
                        Ok(__v) => __v,
                        Err(__sig) => return Err(__sig),
                    }
                };
            }
            ("join", 0) => {
                return quote! {
                    {
                        let __r = (#recv_expr).as_ractor_unchecked();
                        match zeo_rt::ractor_join(&__r) {
                            Ok(()) => zeo_rt::RubyValue::Ractor(__r),
                            Err(__sig) => return Err(__sig),
                        }
                    }
                };
            }
            _ => {}
        }
    }

    // Native `Int` arithmetic/comparison/bitwise ops: both operands must be
    // statically known `Int` (see `INT_BINARY_OPS`'s docs above). The
    // operands stay boxed `&RubyValue`s since the bignum migration -- the
    // `int_*` family's inline small-small fast half keeps the hot path
    // cheap, and overflow promotes instead of panicking.
    if no_kwargs
        && args.len() == 1
        && let Some(&(_, rt_fn, kind)) = ops::INT_BINARY_OPS.iter().find(|(op, _, _)| *op == name)
    {
        let recv_ty = infer(cx, recv_id);
        let arg_ty = infer(cx, args[0]);
        if recv_ty == TyKind::Int && arg_ty == TyKind::Int {
            let arg_expr = emit_expr(cx, args[0]);
            let func = format_ident!("{rt_fn}");
            return match kind {
                ops::IntOpKind::Value => {
                    quote! { zeo_rt::#func(&(#recv_expr), &(#arg_expr)) }
                }
                ops::IntOpKind::Fallible => {
                    quote! { zeo_rt::#func(&(#recv_expr), &(#arg_expr))? }
                }
                ops::IntOpKind::DivMod => {
                    ops::emit_int_div_or_mod_checked(cx, rt_fn, recv_expr.clone(), arg_expr)
                }
                ops::IntOpKind::Bool => quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::#func(&(#recv_expr), &(#arg_expr)))
                },
                ops::IntOpKind::Cmp => quote! {
                    zeo_rt::RubyValue::Int(zeo_rt::#func(&(#recv_expr), &(#arg_expr)))
                },
            };
        }
    }

    // Native `Int` unary operators (`-@`/`+@`/`~`), same eligibility rule
    // (all three return `RubyValue` -- negation can promote `-i64::MIN`).
    if no_kwargs
        && args.is_empty()
        && let Some(&(_, rt_fn)) = ops::INT_UNARY_OPS.iter().find(|(op, _)| *op == name)
        && infer(cx, recv_id) == TyKind::Int
    {
        let func = format_ident!("{rt_fn}");
        return quote! { zeo_rt::#func(&(#recv_expr)) };
    }

    // Native `Float` arithmetic/comparison, INCLUDING mixed `Int`/`Float`
    // operands (Ruby's own numeric-tower promotion: `1 + 2.0` promotes the
    // `Int` side to `f64` before operating, same as `Float`-`Float`).
    // Reached only when the Int-Int fast path above didn't match (an
    // Int-Int pair already returned), so this only ever needs to check "is
    // at least one side Float, and is the other Int or Float".
    if no_kwargs && args.len() == 1 {
        let recv_ty = infer(cx, recv_id);
        let arg_ty = infer(cx, args[0]);
        let is_float_op = matches!(
            (recv_ty, arg_ty),
            (TyKind::Float, TyKind::Float)
                | (TyKind::Float, TyKind::Int)
                | (TyKind::Int, TyKind::Float)
        );
        if is_float_op {
            let arg_expr = emit_expr(cx, args[0]);
            let recv_f = match recv_ty {
                TyKind::Float => quote! { (#recv_expr).as_float_unchecked() },
                _ => quote! { zeo_rt::num_to_f64_unchecked(&(#recv_expr)) },
            };
            let arg_f = match arg_ty {
                TyKind::Float => quote! { (#arg_expr).as_float_unchecked() },
                _ => quote! { zeo_rt::num_to_f64_unchecked(&(#arg_expr)) },
            };
            // `<=>` isn't in `FLOAT_BINARY_OPS` (see its docs) -- a `NaN`
            // comparison returns `nil`, not an `Int`.
            if name == "<=>" {
                return quote! {
                    match zeo_rt::float_cmp(#recv_f, #arg_f) {
                        Some(__n) => zeo_rt::RubyValue::Int(__n),
                        None => zeo_rt::RubyValue::Nil,
                    }
                };
            }
            // `%` needs a fallible form: `x % 0` raises ZeroDivisionError,
            // where every other Float op (incl. `/`, whose zero divisor is
            // Infinity) is total -- so it can't ride the uniform table below.
            if name == "%" {
                return quote! {
                    zeo_rt::float_mod_checked(#recv_f, #arg_f)?
                };
            }
            // `**` needs a fallible form too: a negative base to a fractional
            // power has no real result (zeo raises Math::DomainError; CRuby
            // promotes to Complex -- a documented divergence).
            if name == "**" {
                return quote! {
                    zeo_rt::float_pow_checked(#recv_f, #arg_f)?
                };
            }
            if let Some(&(_, rt_fn, result_ty)) =
                ops::FLOAT_BINARY_OPS.iter().find(|(op, _, _)| *op == name)
            {
                let func = format_ident!("{rt_fn}");
                let wrapper = format_ident!("{result_ty}");
                return quote! {
                    zeo_rt::RubyValue::#wrapper(zeo_rt::#func(#recv_f, #arg_f))
                };
            }
        }
    }

    // Native `Float` unary operators (`-@`/`+@` -- no `~`, real Ruby's
    // `Float` has none), same eligibility rule.
    if no_kwargs
        && args.is_empty()
        && let Some(&(_, rt_fn)) = ops::FLOAT_UNARY_OPS.iter().find(|(op, _)| *op == name)
        && infer(cx, recv_id) == TyKind::Float
    {
        let func = format_ident!("{rt_fn}");
        return quote! {
            zeo_rt::RubyValue::Float(zeo_rt::#func((#recv_expr).as_float_unchecked()))
        };
    }

    // A REOPENED builtin's method on a statically-typed builtin receiver
    //: a direct call to the generated free function (`__bm_
    // String::length(recv, ...)`), checked BEFORE the collection/Proc/
    // Regexp fast paths below because a user redefinition must OVERRIDE the
    // native behavior -- real Ruby's rule, oracle-verified (`class String;
    // def length; 42; end` wins at every call site). The receiver
    // expression is already a boxed `RubyValue` for every builtin TyKind
    // (only `Object` receivers are unboxed `Arc<Concrete>`s, and those
    // never reach this arm).
    if let Some(cid) = infer_any_class(cx, recv_id)
        && cx.compiler.class(cid).is_builtin
        && let Some(entry) = cx.compiler.lookup_method(cid, name).copied()
        && !visibility::defers_to_runtime(cx, entry.visibility, bypass_visibility)
    {
        if !bypass_visibility
            && let Some(err) = visibility::enforce_visibility(cx, recv_id, &entry, name)
        {
            return err;
        }
        let scope = cx.compiler.scope(entry.def);
        let mod_ident = super::ident::class_ident(cx.compiler, cid);
        let method_ident = safe_ident(name);
        return super::params::emit_call_args_to(
            cx,
            &super::params::Callee::FreeFn {
                path: quote! { #mod_ident::#method_ident },
                recv: recv_expr.clone(),
            },
            name,
            &scope.params,
            args,
            kwargs,
            block,
            block_arg,
            scope.needs_block_param(),
            crate::codegen::scope_frame_guard(cx.compiler, scope, false),
        );
    }

    if no_kwargs {
        if let Some(tokens) = builtins::try_collection_dispatch(cx, recv_id, name, args, recv_expr)
        {
            return tokens;
        }
        if let Some(tokens) = builtins::try_proc_dispatch(cx, recv_id, name, args, block, recv_expr)
        {
            return tokens;
        }
        if let Some(tokens) =
            builtins::try_regexp_dispatch(cx, recv_id, name, args, block, recv_expr)
        {
            return tokens;
        }
    }

    // Known-shape block inlining (mirrors `emit_block_value_into`/`.times`,
    // `codegen_iter.c:1281`): the block body is spliced into a native,
    // labeled Rust loop -- no closure or Proc object is allocated. Shares
    // `codegen::loops`' redo-wrapping machinery with `while`/`until`/`loop`/
    // `for`, so `break`/`next`/`redo` inside a `.times` block work exactly
    // the same way.
    // Blockless `5.times` falls through to the dynamic row, which answers
    // an Enumerator -- the inline splice below is only for the
    // block form.
    if let Some(block_id) =
        block.filter(|_| is_times_fast_path(cx.compiler, Some(recv_id), name, no_kwargs))
        && let HirNode::IntegerLit(n) = &cx.compiler.hir[recv_id]
    {
        let n = *n;
        // `Integer#times` evaluates to its receiver (MRI), not nil --
        // matters in expression position (`x = 5.times {}`).
        return emit_counted_block_splice(
            cx,
            block_id,
            quote! { 0i64 },
            quote! { __i >= #n },
            quote! { 1i64 },
            "times",
            quote! { zeo_rt::RubyValue::Int(#n) },
        );
    }

    // `(a..b).each { |i| }` on a LITERAL Int-bounded range: the same native
    // counted loop `.times` fuses to (the generic path allocates a real
    // Proc and dynamic-dispatches every yield -- bm_range_each spent 65s
    // there). `.each` answers the receiver range.
    if let Some(block_id) =
        block.filter(|_| is_range_each_fast_path(cx.compiler, Some(recv_id), name, no_kwargs))
        && let HirNode::RangeLit {
            start: Some(s),
            end: Some(e),
            exclusive,
        } = &cx.compiler.hir[recv_id]
        && let (HirNode::IntegerLit(s), HirNode::IntegerLit(e)) =
            (&cx.compiler.hir[*s], &cx.compiler.hir[*e])
    {
        let (s, e, exclusive) = (*s, *e, *exclusive);
        let done = if exclusive {
            quote! { __i >= #e }
        } else {
            quote! { __i > #e }
        };
        return emit_counted_block_splice(
            cx,
            block_id,
            quote! { #s },
            done,
            quote! { 1i64 },
            "range_each",
            quote! {
                zeo_rt::RubyValue::Range(
                    Some(Box::new(zeo_rt::RubyValue::Int(#s))),
                    Some(Box::new(zeo_rt::RubyValue::Int(#e))),
                    #exclusive,
                )
            },
        );
    }

    // Typed-receiver iterator fusion (`Compiler::inline_iter_sites`): the
    // literal shapes above never nominate (their receivers aren't locals),
    // so ordering is free. See `emit_typed_iter_inline` for the guard rule.
    if let Some(block_id) = block
        && let Some(&kind) = cx.compiler.inline_iter_sites.get(&block_id)
    {
        return emit_typed_iter_inline(cx, kind, recv_id, args, block_id, name, block_arg);
    }

    let recv_class = infer_class(cx, recv_id);

    // `send`/`public_send`: which one does this receiver's chain resolve to?
    // (See `resolve_send`.) Then, for Kernel's:
    // try literal-name static resolution first
    // (mirrors zeo's `desugar_public_send_recv`) -- rewritten to a direct
    // call (Path 1) when the class is known AND the name resolves. Falls to
    // Path 2 (the genuinely new part -- see the plan's Object Model
    // section) when the receiver's class isn't known, the name isn't a
    // literal, or the literal name doesn't resolve anywhere in the chain
    // (which is exactly the `method_missing` trigger condition). Path 2's
    // calling convention has no keyword-argument channel (see
    // `codegen::params::emit_dynamic_trampoline`'s docs) -- `kwargs` is
    // simply dropped on that fallback path, matching the same documented
    // scope-cut as a method declaring keyword params being unreachable via
    // `send` at all.
    // An UNKNOWN receiver has already left through `emit_reflect_dispatch`
    // above, which asks the same question of all four reflection entries.
    let send_resolves = if name == "send" || name == "public_send" {
        resolve_send(cx, recv_id, name)
    } else {
        SendTarget::Shadowed
    };
    if send_resolves == SendTarget::Kernel && !args.is_empty() {
        if let HirNode::SymbolLit(target) = &cx.compiler.hir[args[0]] {
            let target = target.clone();
            if let Some(cid) = recv_class
                && let Some((_, sid)) = cx.compiler.method_in_chain(cid, &target)
            {
                // `public_send` -- unlike `send` -- only ever calls
                // `Public` methods, with NO self-receiver/protected-
                // relatedness relaxation at all (stricter than an
                // ordinary explicit-receiver call, matching real Ruby).
                // Checked HERE (not via `enforce_visibility`, whose
                // rules are deliberately looser) before recursing.
                // `public_send` -- unlike `send` -- only ever calls
                // `Public` methods, with NO self-receiver/protected-
                // relatedness relaxation at all (stricter than an
                // ordinary explicit-receiver call, matching real Ruby).
                //
                // A non-public target is NOT a compile error, though:
                // real Ruby resolves visibility at CALL time and raises a
                // rescuable NoMethodError. So decline the Path-1
                // shortcut and fall through to the dynamic path, whose
                // `send_value_public_in` performs exactly that check --
                // keeping the rule in ONE place rather than duplicating
                // the message here.
                let public_ok = name != "public_send"
                    || cx.compiler.scope(sid).visibility == Visibility::Public;
                if public_ok {
                    return dispatch(
                        cx,
                        recv_id,
                        &target,
                        &args[1..],
                        kwargs,
                        block,
                        block_arg,
                        recv_expr,
                        true,
                    );
                }
            }
        }
        // Keyword args ride as one trailing Hash (the G2 convention) --
        // the callee's trampoline pops and binds it.
        let kw_hash = emit_kwargs_trailing_hash(cx, kwargs);
        // A statically-known class needs boxing into an `RObj` handle first
        // (`recv_expr` is an unboxed `Arc<Concrete>` there); a `Poly` receiver
        // is ALREADY a `RubyValue::Object(...)` at runtime (e.g. a `rescue`
        // clause's exception binding -- see `codegen::exceptions`'s docs for
        // why that's never narrowed to a concrete class), so it just needs
        // unwrapping, not a fabricated `new_handle` call (which would need a
        // compile-time class name we don't have here).
        let recv_obj_expr = match recv_class {
            Some(cid) => {
                let class_ident = super::ident::class_ident(cx.compiler, cid);
                quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
            }
            // Any non-Object receiver (a builtin collection, or genuinely
            // Poly) dispatches through `send_value`'s builtin table --
            // `[1,2].send(:length)` works now, not just Object receivers.
            None => quote! { (#recv_expr) },
        };
        let name_expr = emit_symbol_expr(cx, args[0]);
        let rest_args = args[1..].iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        let kw_hash = kw_hash.into_iter();
        let block_value = emit_block_option(cx, block, block_arg);
        // `public_send` routes through the visibility-gated entry point; plain
        // `send`/`__send__` stay deliberately visibility-blind.
        let send_fn = if name == "public_send" {
            quote! { zeo_rt::send_value_public_in }
        } else {
            quote! { zeo_rt::send_value_in }
        };
        let dyn_call = quote! {
            #send_fn(#__bx, &#recv_obj_expr, #name_expr, &[#(#rest_args,)* #(#kw_hash,)*], #block_value)
        };
        return wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
    }

    // Ordinary call with a statically known receiver class: direct call
    // (Path 1). This is the common case -- `method_in_chain` mirrors
    // `comp_method_in_chain` exactly (compiler.c:404).
    // ...unless a guarded `undef` in the chain may have retracted the name by
    // the time this runs, or a runtime site may have replaced its body or
    // re-marked its visibility -- either way only the dynamic path can see the
    // overlay. See `ClassInfo::runtime_undefs` and `Compiler::runtime_patches`.
    if let Some(cid) = recv_class.filter(|&c| {
        !cx.compiler.may_be_undefined_at_runtime(c, name)
            && !cx.compiler.may_be_patched_at_runtime(name)
    }) && let Some(entry) = cx.compiler.lookup_method(cid, name).copied()
        && !visibility::defers_to_runtime(cx, entry.visibility, bypass_visibility)
    {
        let sid = entry.def;
        let scope = cx.compiler.scope(sid);
        if !bypass_visibility
            && let Some(err) = visibility::enforce_visibility(cx, recv_id, &entry, name)
        {
            return err;
        }
        if let Some(inlined) =
            emit_inline_accessor(cx, cid, scope, recv_expr, args, kwargs, block, block_arg)
        {
            return inlined;
        }
        // An `attr_*` accessor is iseq-less in CRuby and appears in no
        // backtrace, so a wrong-arity call to one -- the only thing that
        // reaches this line for an accessor -- must report the caller's
        // frame alone (oracle-verified against a hand-written `def x=(v)`,
        // which does get its own).
        let frame = match cx.compiler.accessor_shape(cid, scope) {
            Some(a) if a.attr_generated => quote! {},
            _ => crate::codegen::scope_frame_guard(cx.compiler, scope, false),
        };
        return super::params::emit_call_args(
            cx,
            recv_expr,
            name,
            &scope.params,
            args,
            kwargs,
            block,
            block_arg,
            scope.needs_block_param(),
            frame,
        );
    }

    // Runtime-checked fallback for a built-in `Int` operator whose
    // operand(s) couldn't be statically proven `Int`/`Float` -- most
    // commonly an ordinary method PARAMETER, which is always `Poly`
    // (zeo never infers a param's type from its call sites; see
    // `Scope::params`'s docs), regardless of what's actually passed at
    // runtime. Without this, `def add(a, b); a + b; end` -- arithmetic on
    // the plainest possible method parameters -- can never work, which
    // would make `Params` barely usable. This is deliberately narrow: a
    // runtime type check for exactly the same built-in `Int`/`Float` op
    // tables above (INCLUDING the same `Int`/`Float` mixed-promotion rule
    // the static fast path uses), not a general dynamic multi-method
    // dispatch system (which would also need to resolve a runtime
    // String/Array/user-`Object`'s own `+`/`<=>` -- a separably-scoped, much
    // larger feature). `recv_class.is_none()` only (a known Object class's
    // own operator overload, if any, already took priority above); real
    // Ruby can't catch a type mismatch here statically either, so a clear
    // runtime panic (not a raised exception, matching every other pre-
    // `raise`/`rescue` failure in this runtime) is a faithful, not a lesser,
    // translation for any OTHER operand shape -- STRICTLY better than
    // today's alternative of `dispatch` itself never reaching a fallback and
    // panicking zeo at compile time instead.
    if no_kwargs && recv_class.is_none() {
        if args.len() == 1 {
            let int_entry = ops::INT_BINARY_OPS.iter().find(|(op, _, _)| *op == name);
            let is_numeric_op = int_entry.is_some()
                || ops::FLOAT_BINARY_OPS.iter().any(|(op, _, _)| *op == name)
                || name == "<=>";
            if is_numeric_op {
                // An Object-typed argument is an unboxed `Arc<Concrete>` --
                // box it so the match scrutinee (and the `send_value`
                // fallback's `(*__dyn_arg).clone()`) is a `RubyValue`
                // (`junk << Trash.new(j)` on a Poly receiver hits this).
                let arg_expr = {
                    let e = emit_expr(cx, args[0]);
                    box_if_object_typed(cx, args[0], e)
                };
                let recv_operand = match borrowable_operand(cx, recv_id) {
                    Some(ident) => quote! { &#ident },
                    None => quote! { &(#recv_expr) },
                };
                let arg_operand = match borrowable_operand(cx, args[0]) {
                    Some(ident) => quote! { &#ident },
                    None => quote! { &(#arg_expr) },
                };
                // One inline Int-Int fast arm (the hot `def add(a, b); a +
                // b; end` case), plus Float-Float/mixed arms for the TOTAL
                // float operators below; every other operand shape --
                // Bignum/Rational/Complex lanes, user operator methods,
                // builtin rows -- resolves through `send_value`'s MRO walk,
                // whose Integer/Float operator rows drive the same one
                // tower matrix.
                let int_arm = int_entry.map(|&(_, rt_fn, kind)| {
                    let func = format_ident!("{rt_fn}");
                    let call = match kind {
                        ops::IntOpKind::Value => quote! { zeo_rt::#func(__r, __a) },
                        ops::IntOpKind::Fallible => quote! { zeo_rt::#func(__r, __a)? },
                        ops::IntOpKind::DivMod => ops::emit_int_div_or_mod_checked(
                            cx,
                            rt_fn,
                            quote! { __r },
                            quote! { (*__a).clone() },
                        ),
                        ops::IntOpKind::Bool => quote! {
                            zeo_rt::RubyValue::Bool(zeo_rt::#func(__r, __a))
                        },
                        ops::IntOpKind::Cmp => quote! {
                            zeo_rt::RubyValue::Int(zeo_rt::#func(__r, __a))
                        },
                    };
                    quote! {
                        (
                            __r @ zeo_rt::RubyValue::Int(_),
                            __a @ zeo_rt::RubyValue::Int(_),
                        ) => #call,
                    }
                });
                // Float-Float and mixed Int/Float arms, exactly the tower's
                // own `Flo` lane (`num_to_f64_unchecked` promotes a small
                // `Int` with the same `as f64`): the TOTAL operators only.
                // `%`/`**` (edge cases raise) and `<=>` (`NaN` is nil) keep
                // the MRO rows; a `BigInt` operand falls through too. The
                // comparisons are `float_lt`-family IEEE semantics -- what
                // CRuby's own `Float#<` answers for `NaN` (false, never the
                // `Comparable` fallback's ArgumentError).
                let float_arms = ops::FLOAT_BINARY_OPS
                    .iter()
                    .find(|&&(op, _, _)| op == name && op != "%" && op != "**")
                    .map(|&(_, rt_fn, result_ty)| {
                        let func = format_ident!("{rt_fn}");
                        let wrapper = format_ident!("{result_ty}");
                        quote! {
                            (
                                zeo_rt::RubyValue::Float(__r),
                                zeo_rt::RubyValue::Float(__a),
                            ) => zeo_rt::RubyValue::#wrapper(zeo_rt::#func(*__r, *__a)),
                            (
                                zeo_rt::RubyValue::Float(__r),
                                zeo_rt::RubyValue::Int(__a),
                            ) => zeo_rt::RubyValue::#wrapper(zeo_rt::#func(*__r, *__a as f64)),
                            (
                                zeo_rt::RubyValue::Int(__r),
                                zeo_rt::RubyValue::Float(__a),
                            ) => zeo_rt::RubyValue::#wrapper(zeo_rt::#func(*__r as f64, *__a)),
                        }
                    });
                let name_sym = super::pooled_sym(name);
                return quote! {
                    match (#recv_operand, #arg_operand) {
                        #int_arm
                        #float_arms
                        (__dyn_recv, __dyn_arg) => zeo_rt::send_value_in(#__bx,
                            __dyn_recv,
                            #name_sym,
                            &[(*__dyn_arg).clone()],
                            None,
                        )?,
                    }
                };
            }
        }
        if args.is_empty() {
            let int_entry = ops::INT_UNARY_OPS.iter().find(|(op, _)| *op == name);
            let float_entry = ops::FLOAT_UNARY_OPS.iter().find(|(op, _)| *op == name);
            if int_entry.is_some() || float_entry.is_some() {
                // Same shape as the binary fallback: inline Int and Float
                // fast arms, everything else through the MRO walk's unary
                // rows.
                let int_arm = int_entry.map(|&(_, rt_fn)| {
                    let func = format_ident!("{rt_fn}");
                    quote! {
                        __r @ zeo_rt::RubyValue::Int(_) => zeo_rt::#func(__r),
                    }
                });
                let float_arm = float_entry.map(|&(_, rt_fn)| {
                    let func = format_ident!("{rt_fn}");
                    quote! {
                        zeo_rt::RubyValue::Float(__r) => {
                            zeo_rt::RubyValue::Float(zeo_rt::#func(*__r))
                        }
                    }
                });
                let recv_operand = match borrowable_operand(cx, recv_id) {
                    Some(ident) => quote! { &#ident },
                    None => quote! { &(#recv_expr) },
                };
                let name_sym = super::pooled_sym(name);
                return quote! {
                    match #recv_operand {
                        #int_arm
                        #float_arm
                        __dyn_recv => zeo_rt::send_value_in(#__bx,
                            __dyn_recv,
                            #name_sym,
                            &[],
                            None,
                        )?,
                    }
                };
            }
        }
    }

    // Last resort for a receiver that's dynamically typed with no more
    // specific static shape at all (`TyKind::Poly` -- e.g. a `rescue`
    // clause's exception binding, or an ordinary method parameter) and
    // nothing above matched: dispatch dynamically (Path 2) against whatever
    // `class_id()` the runtime value ACTUALLY carries, exactly the same
    // `zeo_rt::send` call `send`/`public_send`'s own Path-2 fallback
    // above already makes, minus needing a literal-symbol method name
    // (ordinary dot-call syntax always has one, statically, at the call
    // site). This is what makes calling an ordinary method on a `rescue`'s
    // exception binding work (`e.message`, `e.to_s`) -- `e` is never
    // narrowed to a concrete class (see
    // `codegen::exceptions::emit_rescue_chain`'s docs for why that would be
    // unsound), so it stays exactly this kind of receiver. Deliberately
    // narrower than plain `recv_class.is_none()`: an `Array`/`Hash`/`Range`/
    // `Str`/`Proc`-typed receiver ALSO has no `recv_class` (that's only ever
    // `Some` for `TyKind::Object`), but calling an unimplemented method on
    // one of those falls through to `send_value`'s DYNAMIC dispatch:
    // its builtin method table handles the supported operations
    // (`[1,2,3].each { ... }` works now), and anything else raises a real,
    // rescuable `NoMethodError` at runtime with the builtin class's actual
    // name -- Ruby's own behavior.
    // The kinds listed are exactly the ones whose static repr is already a
    // boxed `RubyValue` (see `types.rs`'s module docs: only `Int` -- and
    // `Object`, as `Arc<Concrete>` -- get unboxed native representations,
    // so those two MUST NOT route through a `&RubyValue` call). `kwargs`
    // has no Path 2 channel at all -- raise the same clear error the
    // `send`/`public_send` case above does, rather than silently dropping
    // it and dispatching without it.
    if matches!(
        infer(cx, recv_id),
        TyKind::Poly
            | TyKind::Str
            | TyKind::Array
            | TyKind::Hash
            | TyKind::Range
            | TyKind::Regexp
            | TyKind::MatchData
            // With MRO-walking method tables, EVERY value
            // kind falls through -- `5.itself`/`"a".between?(...)` resolve
            // Kernel/Comparable rows down the receiver's real ancestor
            // chain at runtime, and a genuinely unknown name raises real
            // Ruby's NoMethodError.
            | TyKind::Int
            | TyKind::Float
            | TyKind::Symbol
            | TyKind::Proc
            | TyKind::Fiber
            | TyKind::Thread
            | TyKind::Mutex
            | TyKind::Queue
            | TyKind::Ractor
            // A class VALUE receiver whose method isn't a
            // statically-defined class method (that case returned above):
            // `send_value`'s Class arm handles the reflection set
            // (`name`/`ancestors`/`==`/the registry constructor), and an
            // unknown name is a real runtime NoMethodError "for class X".
            | TyKind::ClassObj(_)
    ) {
        let name_expr = super::pooled_sym(name);
        let arg_exprs = args.iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        // Keyword args ride as one trailing Hash (the G2 convention).
        let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
        let block_value = emit_block_option(cx, block, block_arg);
        // THE Path-2 site: a `Poly` receiver -- an untyped local, a method
        // parameter, an ancestor's return -- is what `bm_rbtree`, `bm_splay`
        // and `bm_linked_list` actually hold, and a third of their time was
        // resolving the same class to the same method on every call. Per-site
        // inline cache; see `zeo_rt::CallSite`. A SHARED body gets none -- see
        // `Ctx::shared_body`.
        let caller = visibility::caller_class(cx, recv_id, bypass_visibility);
        // A runtime caller class (re-homed `self`) can't fill a per-site
        // cache -- its vetting varies per call -- so it takes the uncached
        // entry, like a shared body.
        let dyn_call = match caller {
            visibility::Caller::Static(c) if !cx.shared_body => {
                let site = crate::codegen::pooled_call_site(c);
                quote! {
                    zeo_rt::send_value_cached(#site, #__bx,
                        &(#recv_expr),
                        #name_expr,
                        &[#(#arg_exprs,)* #(#kw_hash,)*],
                        #block_value,
                    )
                }
            }
            caller => {
                let (bind, arg) = (caller.bind(), caller.bound());
                quote! {
                    {
                        #bind
                        zeo_rt::send_value_explicit_in(#__bx,
                            &(#recv_expr),
                            #name_expr,
                            &[#(#arg_exprs,)* #(#kw_hash,)*],
                            #block_value,
                            #arg,
                        )
                    }
                }
            }
        };
        return wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
    }

    // A statically-known class that `include Enumerable` (the RUST-backed
    // builtin module) with no own/materialized definition
    // for this name: dispatch dynamically -- `send`'s Enumerable fallback
    // reaches `zeo_rt::enumerable`, which drives this receiver's own
    // `each`. Deliberately NO compile-time list of Enumerable method names
    // here: the runtime match in `zeo_rt::enumerable::enumerable_send`
    // is the single source of truth, and a name it doesn't recognize
    // raises a real, rescuable `NoMethodError` at runtime -- exactly real
    // Ruby's behavior, and the same compile-time-strictness-for-runtime-
    // faithfulness trade this phase already made for builtin receivers
    // (see the widened dynamic fallback above). The cost is that a TYPO'd
    // method on an Enumerable-including class surfaces at runtime instead
    // of compile time -- scoped to exactly the classes that opted into an
    // open-ended mixin.
    if let Some(cid) = recv_class
        && (cx
            .compiler
            .class(cid)
            .ancestors
            .contains(&crate::compiler::ENUMERABLE_CLASS)
            || cx
                .compiler
                .class(cid)
                .ancestors
                .contains(&crate::compiler::COMPARABLE_CLASS))
    {
        let class_ident = super::ident::class_ident(cx.compiler, cid);
        let name_expr = super::pooled_sym(name);
        let arg_exprs = args.iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        // Keyword args ride as one trailing Hash (the G2 convention).
        let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
        let block_value = emit_block_option(cx, block, block_arg);
        let caller = visibility::caller_class(cx, recv_id, bypass_visibility);
        let (bind, caller_arg) = (caller.bind(), caller.bound());
        return quote! {
            {
                #bind
                zeo_rt::catch_break(zeo_rt::send_value_explicit_in(#__bx,
                    &zeo_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)),
                    #name_expr,
                    &[#(#arg_exprs,)* #(#kw_hash,)*],
                    #block_value,
                    #caller_arg,
                ))?
            }
        };
    }

    // No static form matched. Dispatch through the runtime rather than
    // rejecting: the receiver's class may still provide the method via an
    // ancestor the static tables don't mirror (every user object inherits
    // Kernel/Object -- `method(:x)`, `tap`, `frozen?`,
    // `instance_variable_get`, ...), and a name that genuinely resolves
    // nowhere raises NoMethodError at the moment the call runs, which is
    // what real Ruby does anyway.
    let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
    let name_expr = quote! { zeo_rt::Symbol::intern(#name) };
    let arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    // Keyword args ride as one trailing Hash (the G2 convention).
    let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
    let block_value = emit_block_option(cx, block, block_arg);
    // The general dynamic dispatch, and the one the object-graph benchmarks
    // spend a third of their time in -- so it carries a per-site inline cache
    // (`zeo_rt::CallSite`). Every other dynamic entry point stays uncached.
    match visibility::caller_class(cx, recv_id, bypass_visibility) {
        visibility::Caller::Static(c) => {
            let site = crate::codegen::pooled_call_site(c);
            quote! {
                zeo_rt::catch_break(zeo_rt::send_value_cached(#site, #__bx,
                    &#recv_boxed,
                    #name_expr,
                    &[#(#arg_exprs,)* #(#kw_hash,)*],
                    #block_value,
                ))?
            }
        }
        caller => {
            let (bind, caller_arg) = (caller.bind(), caller.bound());
            quote! {
                {
                    #bind
                    zeo_rt::catch_break(zeo_rt::send_value_explicit_in(#__bx,
                        &#recv_boxed,
                        #name_expr,
                        &[#(#arg_exprs,)* #(#kw_hash,)*],
                        #block_value,
                        #caller_arg,
                    ))?
                }
            }
        }
    }
}

/// The `Ractor#send` arm's kwarg split: `Some(None)` for a kwarg-less call,
/// `Some(Some(expr))` when the ONLY kwarg is a literal-keyed `move:` (its
/// emitted, boxed value), and `None` -- fall through to ordinary dispatch --
/// for any other kwarg shape (a double-splat, an unknown keyword).
fn ractor_move_kwarg(cx: &Ctx, kwargs: &[KwArg]) -> Option<Option<TokenStream>> {
    match kwargs {
        [] => Some(None),
        [KwArg::Pair(k, v)] if matches!(&cx.compiler.hir[*k], HirNode::SymbolLit(s) if s == "move") =>
        {
            let e = emit_expr(cx, *v);
            Some(Some(super::expr::box_if_object_typed(cx, *v, e)))
        }
        _ => None,
    }
}

/// A Path-1 call to an accessor, replaced by the field access itself: no
/// call, no frame, no `check_ints`, no wide `Result` through memory.
///
/// `recv_expr` at a Path-1 site with a statically-known `Object` receiver is
/// already the unboxed `Arc<Concrete>` the generated struct lives behind
/// (that is what makes `emit_call_args`' `(#recv).#method(..)` typecheck), and
/// `Compiler::accessor_shape` has established the field exists on `cid`'s own
/// struct -- so `(#recv).#field` is valid for exactly the same reason.
///
/// `None` -- keep the ordinary call -- for anything the field access could not
/// reproduce: a block or block-pass, call-site keywords, or an argument count
/// the accessor does not take (which must still raise `ArgumentError` at
/// runtime, so it falls through to the arity machinery).
///
/// Runtime redefinition is not a hazard here because it is not one at any
/// Path-1 site: zeo already binds these statically, and a
/// `define_method`-after-the-fact does not displace them (tracked as
/// `tests/gaps/issue_runtime_redefine_accessor.rb`). Inlining preserves that
/// exactly; it does not widen it.
#[allow(clippy::too_many_arguments)] // mirrors `emit_call_args`' own parameter list
fn emit_inline_accessor(
    cx: &Ctx,
    cid: crate::compiler::ClassId,
    scope: &crate::compiler::Scope,
    recv_expr: &TokenStream,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> Option<TokenStream> {
    use crate::compiler::AccessorKind;
    if !kwargs.is_empty() || block.is_some() || block_arg.is_some() {
        return None;
    }
    let shape = cx.compiler.accessor_shape(cid, scope)?;
    let slot = super::expr::ivar_slot(cx, cid, &shape.ivar)?;
    // The husk guard: an inlined accessor bypasses dispatch, so a program
    // that names `Ractor` (and can therefore `send(obj, move: true)` the
    // receiver away) checks the class word here -- ahead of the frozen
    // check, since `Ractor::MovedError` beats `FrozenError`. Everything
    // else emits the bare field access it always did.
    let moved_guard = if cx.compiler.uses_ractor() {
        quote! { zeo_rt::check_not_moved_obj(&**__r)?; }
    } else {
        quote! {}
    };
    match (shape.kind, args) {
        // `&#recv_expr`, not `#recv_expr`: a receiver expression is often a
        // temporary (`Clone::clone(&x)`), and borrowing it in a `let` extends
        // it to the end of THIS block, where taking the field off the
        // temporary directly drops it at the end of the `let` statement.
        (AccessorKind::Reader, []) => Some(quote! {
            {
                let __r = &#recv_expr;
                #moved_guard
                __r.__ivars.get(#slot)
            }
        }),
        (AccessorKind::Writer, [arg]) => {
            let class_ident = crate::codegen::ident::class_ident(cx.compiler, cid);
            let v = super::expr::emit_expr(cx, *arg);
            // Boxed for the same reason `emit_expr`'s own `IvarWrite` arm boxes:
            // the slot is `RubyValue`, but an Object-typed RHS emits a bare
            // `Arc<Concrete>`.
            let v = super::expr::box_if_object_typed(cx, *arg, v);
            // Receiver bound before the argument: Ruby evaluates left to right.
            Some(quote! {
                {
                    let __r = &#recv_expr;
                    let __v: zeo_rt::RubyValue = #v;
                    #moved_guard
                    if zeo_rt::RubyObject::is_frozen(&**__r) {
                        Err::<(), zeo_rt::Signal>(zeo_rt::ivar_frozen_error(
                            #class_ident::new_handle(Clone::clone(__r)),
                        ))?;
                    }
                    __r.__ivars.set(#slot, __v.clone());
                    __v
                }
            })
        }
        _ => None,
    }
}
