//! `emit_safe_call` -- `&.` safe navigation, which always dispatches
//! through the runtime `ClassRegistry`/`send` path (Path 2), regardless of
//! whether the receiver's class is statically known.

use quote::quote;

use crate::codegen::Ctx;
use crate::codegen::expr::{box_if_object_typed, emit_expr, infer_class};
use crate::hir::NodeId;
use proc_macro2::TokenStream;

/// `&.` always dispatches through the runtime `ClassRegistry`/`send` path
/// (Path 2), regardless of whether the receiver's class is statically
/// known -- unlike ordinary calls, which prefer a direct Path 1 call. The
/// receiver's *concrete Rust type* differs depending on that: an unboxed
/// class struct (e.g. `Box`, with no `.is_nil()`/`Clone`) when the class is
/// known and constructed via `New`, or an already-boxed `RubyValue` when
/// it's dynamically typed. Checking "is it nil" needs one uniform runtime
/// representation either way, so a statically-known-class receiver gets
/// boxed into `RubyValue::Object` here (an otherwise-avoidable `Arc`
/// allocation this specific call site pays for `&.`'s uniformity) before the
/// same nil-check-then-`send` logic runs regardless of which case it was.
pub(super) fn emit_safe_call(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> TokenStream {
    let __bx = cx.box_id;
    let boxed_recv = match infer_class(cx, recv_id) {
        Some(cid) => {
            let class_ident = crate::codegen::ident::class_ident(cx.compiler, cid);
            let recv_expr = emit_expr(cx, recv_id);
            quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
        }
        None => emit_expr(cx, recv_id),
    };
    let name_expr = quote! { zeo_rt::Symbol::intern(#name) };
    let arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    // `recv&.name(args) { block }` -- the block rides the same `send_value_in`
    // block slot a plain dynamic dispatch uses (nil short-circuits before it is
    // ever entered, matching Ruby).
    let block_value = super::emit_block_option(cx, block, block_arg);
    quote! {
        {
            let __safe_recv = #boxed_recv;
            if __safe_recv.is_nil() {
                zeo_rt::RubyValue::Nil
            } else {
                // `send_value` handles Object AND builtin receivers uniformly,
                // so the old non-Object panic is gone.
                zeo_rt::send_value_in(#__bx, &__safe_recv, #name_expr, &[#(#arg_exprs),*], #block_value)?
            }
        }
    }
}
