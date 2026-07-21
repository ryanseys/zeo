//! Error-emission helpers shared across this module's call sites --
//! `RactorError`/`FiberError`/`FrozenError` construction and the
//! `LocalJumpError` a missing block raises -- factored out of `dispatch`/
//! `emit_call` so each guarded call site stays a one-line `emit_*_error(cx,
//! ...)` rather than repeating the `emit_boxed_new` plumbing inline.

use quote::quote;

use crate::codegen::Ctx;
use proc_macro2::TokenStream;

/// A `RactorError` (the flat stand-in for `Ractor::Error` -- nested class
/// names don't exist yet) whose message comes from a runtime `__msg: String`
/// in scope at the emission site (boundary-crossing rejections are computed
/// at runtime, unlike `FiberError`'s fixed strings).
pub(super) fn emit_ractor_error(cx: &Ctx) -> TokenStream {
    crate::codegen::expr::emit_boxed_new(
        cx,
        "RactorError",
        vec![quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new(__msg)) }],
    )
}
/// A boxed `class_name` exception carrying `msg` -- what a codegen site emits
/// when real Ruby RAISES where a compile-time check would otherwise reject.
pub(super) fn emit_simple_error(cx: &Ctx, class_name: &str, msg: &str) -> TokenStream {
    crate::codegen::expr::emit_boxed_new(
        cx,
        class_name,
        vec![quote! {
            zeo_rt::RubyValue::Str(zeo_rt::string_new(#msg.to_string()))
        }],
    )
}
/// A `FiberError` with a fixed message -- CRuby's own wording, passed
/// verbatim from the dispatch sites.
pub(super) fn emit_fiber_error(cx: &Ctx, msg: &str) -> TokenStream {
    emit_simple_error(cx, "FiberError", msg)
}
/// `Thread.new` / `Fiber.new` / `Ractor.new` written with NO block at all.
/// Every one of them raises at runtime in real Ruby rather than being a static
/// error, and each is rescuable -- so emit the raise instead of rejecting the
/// program. Messages verbatim: `thread.c:1034`, `ractor.rb:231`, and Fiber's
/// via the Proc creation it performs.
///
/// This is only for the genuinely blockless form; `&proc` conversion (a real
/// block argument that just isn't a literal) stays a compile-time gap, since
/// silently raising "no block" for it would be wrong.
pub(super) fn emit_missing_block_raise(cx: &Ctx, target: &str) -> TokenStream {
    let (class_name, msg) = match target {
        "Thread" => ("ThreadError", "must be called with a block"),
        "Fiber" => (
            "ArgumentError",
            "tried to create Proc object without a block",
        ),
        _ => ("ArgumentError", "must be called with a block"),
    };
    let err = emit_simple_error(cx, class_name, msg);
    // `?`-propagate rather than `return`: a block body wraps its tail
    // expression in `Ok(...)`, and a bare `return` inside that wrapper
    // generates an `unreachable call` warning in the emitted crate. The `?`
    // form carries the signal out just as well and reads as ordinary Rust.
    quote! { Err(zeo_rt::Signal::Raise(#err))? }
}
/// A `FrozenError` for a mutation attempt on a frozen `class_name` receiver,
/// with CRuby's exact message shape (`can't modify frozen Array: [1, 2, 3]`
/// -- `error.c:4221`, the class name static since every guarded site is
/// type-gated, the receiver's `inspect` computed at runtime). Constructed by
/// codegen, not inside the `zeo_rt` mutators, for the same reason as
/// `array_set`'s `IndexError` contract: only codegen can build an exception
/// object (see `emit_boxed_new`'s docs). `recv_value` must be a
/// `RubyValue`-typed expression valid at the emission site (the guarded
/// blocks bind `__recv` first and pass a rewrapped clone here).
pub(super) fn emit_frozen_error(
    cx: &Ctx,
    class_name: &str,
    recv_value: TokenStream,
) -> TokenStream {
    crate::codegen::expr::emit_boxed_new(
        cx,
        "FrozenError",
        vec![quote! {
            zeo_rt::RubyValue::Str(zeo_rt::string_new(format!(
                "can't modify frozen {}: {}",
                #class_name,
                (#recv_value).inspect_string()
            )))
        }],
    )
}
