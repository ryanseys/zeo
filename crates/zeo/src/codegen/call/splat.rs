//! `emit_splat_call` -- a call site carrying a `*arr` positional splat
//! and/or `**h` double-splat, always dispatched dynamically via
//! `zeo_rt::send` (see the function's own docs for why no static,
//! arity-checked calling convention can handle a runtime-variable argument
//! count).

use quote::quote;

use crate::codegen::Ctx;
use crate::codegen::expr::{box_if_object_typed, emit_expr, infer, infer_class};
use crate::hir::{ArrayElem, KwArg, NodeId};
use crate::types::TyKind;
use proc_macro2::TokenStream;

/// A call site carrying a `*arr` positional splat and/or `**h` double-splat
/// -- see `emit_call`'s docs for why this can never take a static, arity-
/// checked calling convention. ALWAYS dispatches dynamically via
/// `zeo_rt::send`, even when the receiver's class is statically known --
/// a real, documented, minor perf cost (not a correctness gap): call-site
/// splats are rare enough that duplicating Path 1's whole typed-parameter
/// machinery for a runtime-variable argument count isn't worthwhile.
/// Keyword arguments (literal `kwargs` or a `**h` double-splat) have no Path
/// 2 channel at all (matches the existing `send`/`public_send`
/// dynamic-dispatch restriction elsewhere in this file) -- a clean
/// rejection, not silently dropped.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_splat_call(
    cx: &Ctx,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    safe: bool,
) -> TokenStream {
    let __bx = cx.box_id;
    if safe {
        panic!(
            "safe-navigation (`&.`) on a call with a splat argument isn't supported yet (spike scope)"
        );
    }
    let recv_obj_expr = match receiver {
        Some(recv_id) => {
            // A class-VALUE receiver (`Point.new(*args)` / `Klass.foo(**h)`):
            // a literal class constant, or a local statically typed as a class
            // object, emits its `RubyValue::Class` handle directly (see
            // `emit_expr`'s `ClassRef` arm) so the runtime dispatches `new`/the
            // class method through `send_value`'s Class arm -- the same
            // registry-constructor path `HirNode::New` reaches, just with a
            // runtime-built argument vector. Same "only an ACTUALLY-registered
            // class/module" guard as `emit_call`'s constant-receiver
            // interception.
            let is_class_receiver = crate::codegen::expr::const_path_of(cx, recv_id)
                .is_some_and(|p| cx.resolve_class(&p).is_some())
                || matches!(infer(cx, recv_id), TyKind::ClassObj(_));
            if is_class_receiver {
                emit_expr(cx, recv_id)
            } else {
                let recv_expr = emit_expr(cx, recv_id);
                match infer_class(cx, recv_id) {
                    Some(cid) => {
                        let class_ident = crate::codegen::ident::class_ident(cx.compiler, cid);
                        quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
                    }
                    // A receiver with no user-class type -- `Poly`, or a builtin
                    // value type like `Proc`/`Array` (`pr.call(*args)`,
                    // `arr.push(*xs)`): pass the raw receiver so `send_value_in`
                    // dispatches through its runtime method table (`rproc::lookup`
                    // answers `call`/`[]`/`yield`, etc.), the same dynamic path a
                    // non-splat call on such a receiver already takes.
                    None => quote! { (#recv_expr) },
                }
            }
        }
        None => {
            // `puts`/other no-receiver builtins aren't reachable through
            // `zeo_rt::send` at all (they have no `ClassRegistry` entry) --
            // a clean rejection here beats generating code that only fails
            // at RUNTIME with a confusing "no such method".
            // The implicit receiver for THIS context -- a concrete `self`
            // inside an instance method, the class object inside a class
            // method/body, the `main` object at the top level (which
            // carries Object's value methods, so top-level defs dispatch).
            // Kernel functions with no registry entry (`puts`) can't be
            // reached this way, but they're intercepted well before here.
            super::boxed_implicit_self(cx).expect("every context has an implicit self")
        }
    };
    let name_expr = quote! { zeo_rt::Symbol::intern(#name) };
    let arg_pushes = args.iter().map(|a| match a {
        ArrayElem::Single(n) => {
            let e = emit_expr(cx, *n);
            let e = box_if_object_typed(cx, *n, e);
            quote! { __args.push(#e); }
        }
        ArrayElem::Splat(n) => {
            let e = emit_expr(cx, *n);
            quote! { __args.extend((#e).as_array_unchecked().lock().iter().cloned()); }
        }
    });
    // Keyword args (literal pairs INTERLEAVED with `**h` double-splats, in
    // source order) merge into ONE trailing Hash (the G2 convention), built
    // by the shared `emit_kwarg_inserts` -- so `f(**a, c: 1, **b)` gets Ruby's
    // exact left-to-right, last-key-wins order (which the old two-phase
    // "literals then splats" build got wrong for a splat written before a
    // pair, and couldn't represent for two splats at all).
    let kw_push = (!kwargs.is_empty()).then(|| {
        let inserts = crate::codegen::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
        quote! {
            let __kw = zeo_rt::hash_new(vec![]);
            #inserts
            // Only when non-empty: a `**h` whose hash is empty AT RUNTIME
            // contributes NOTHING -- `def c(h) = foo(1, **h); c({})` passes
            // just `1`, with no trailing hash (oracle-verified; pushing it
            // unconditionally silently handed the callee an extra `{}`
            // argument). About a RUNTIME-empty hash, not the literal `**{}` a
            // parser could fold away, so the guard belongs here, not lowering.
            // A literal keyword (`k: 1`) can never produce an empty hash, so
            // for a pairs-only list this check is simply never false.
            if !__kw.lock().is_empty() {
                __args.push(zeo_rt::RubyValue::Hash(__kw));
            }
        }
    });
    let block_value = super::emit_block_option(cx, block, block_arg);
    let dyn_call =
        quote! { zeo_rt::send_value_in(#__bx, &#recv_obj_expr, #name_expr, &__args, #block_value) };
    let dyn_call = super::wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
    quote! {
        {
            let mut __args: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#arg_pushes)*
            #kw_push
            #dyn_call
        }
    }
}
