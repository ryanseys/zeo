//! The Kernel FUNCTION family: `emit_universal_implicit_form` (the
//! universal implicit-self forms every method-body context shares -- the
//! Kernel functions below, `proc { }`, `at_exit`, `__method__`/`__callee__`,
//! and `method(:name)`) and `emit_kernel_function` itself (the print
//! family, conversions, rand/srand, throw, sleep, exit/abort).

use quote::{format_ident, quote};

use crate::codegen::Ctx;
use crate::codegen::expr::{box_if_object_typed, emit_expr};
use crate::hir::{KwArg, NodeId};
use proc_macro2::TokenStream;

/// The universal implicit-self forms every method-body context shares:
/// the Kernel functions below, `proc { }`, and `__method__` --
/// consulted after sibling method resolution (a user override wins,
/// real Ruby's rule) from both the ordinary implicit-self path and the
/// value-backed (builtin-reopen / top-level) method-body path.
pub(super) fn emit_universal_implicit_form(
    cx: &Ctx,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> Option<TokenStream> {
    if let Some(tokens) = emit_kernel_function(cx, name, args, kwargs, block, block_arg) {
        return Some(tokens);
    }
    // `proc { ... }` -- Kernel#proc: the literal block AS a Proc value
    // (`lambda { ... }` desugars in parse to `HirNode::Lambda` already;
    // `proc`'s non-lambda semantics are exactly `emit_proc_value`'s).
    if name == "proc" && args.is_empty() && kwargs.is_empty() {
        if let Some(b) = block {
            return Some(super::procs::emit_proc_value(cx, b));
        }
    }
    // `at_exit { ... }` -- registers the handler (run in reverse order
    // at process exit; see `zeo_rt::exec::run_at_exit`), answering
    // the Proc, CRuby's return value.
    if name == "at_exit" && args.is_empty() && kwargs.is_empty() {
        if let Some(b) = block {
            let p = super::procs::emit_proc_value(cx, b);
            return Some(quote! {
                {
                    let __h = #p;
                    zeo_rt::at_exit_register(__h.clone());
                    __h
                }
            });
        }
    }
    // `__method__`/`__callee__` -- the enclosing method's name as a Symbol,
    // `nil` at the top level (a compile-time constant here: codegen always
    // knows which method body it's emitting). The two differ only under an
    // alias (`__callee__` reports the called-as name); we don't track
    // aliases, so they coincide.
    if (name == "__method__" || name == "__callee__")
        && args.is_empty()
        && kwargs.is_empty()
        && block.is_none()
    {
        return Some(match &cx.current_method {
            Some(m) => {
                let sym = super::super::pooled_sym(m);
                quote! { zeo_rt::RubyValue::Symbol(#sym) }
            }
            None => quote! { zeo_rt::RubyValue::Nil },
        });
    }
    // `method(:name)` -- a bound Method object on the implicit self,
    // dispatched through the Kernel row (see `builtins::method`).
    if name == "method" && args.len() == 1 && kwargs.is_empty() && block.is_none() {
        if let Some(recv) = super::boxed_implicit_self(cx) {
            let __bx = cx.box_id;
            let arg = {
                let e = emit_expr(cx, args[0]);
                box_if_object_typed(cx, args[0], e)
            };
            let method_sym = super::super::pooled_sym("method");
            return Some(quote! {
                zeo_rt::send_value_in(#__bx, &#recv, #method_sym, &[#arg], None)?
            });
        }
    }
    None
}
/// The Kernel FUNCTIONS: the print family (multi-arg),
/// conversions, rand/srand, throw, sleep, exit/abort -- consulted after
/// sibling method resolution (a user `def puts`/`def Integer` wins,
/// real Ruby's rule). Capitalized-name conversion calls WITH arguments
/// parse as ordinary CallNodes, so there's no ClassRef ambiguity.
/// `None` when the name (or call shape) isn't a Kernel function.
fn emit_kernel_function(
    cx: &Ctx,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> Option<TokenStream> {
    if block.is_some() || block_arg.is_some() {
        return None;
    }
    // The format family accepts keyword references (`format("%<x>d", x: 1)`);
    // its keywords become a trailing options Hash the sprintf engine reads.
    // Every other Kernel function here takes no keywords, so bail for them.
    let is_format_family = matches!(name, "format" | "sprintf" | "printf");
    if !kwargs.is_empty() && !is_format_family {
        return None;
    }
    let plain_fn: Option<&str> = None;
    // Fallible functions (`?`); the conversions require >= 1 arg
    // (a bare `Integer` parses as a ClassRef, never reaches here).
    // The print family is fallible since it routes through `$stdout`/
    // `$stderr` (a duck-typed redirect target's `write` can raise).
    let fallible_fn = match name {
        "puts" => Some("kernel_puts"),
        "p" => Some("kernel_p"),
        "pp" => Some("kernel_pp"),
        "print" => Some("kernel_print"),
        "warn" => Some("kernel_warn"),
        "Integer" if !args.is_empty() => Some("kernel_integer"),
        "Float" if !args.is_empty() => Some("kernel_float"),
        "Rational" if !args.is_empty() => Some("kernel_rational"),
        "Complex" if !args.is_empty() => Some("kernel_complex"),
        "String" if !args.is_empty() => Some("kernel_string"),
        "Array" if !args.is_empty() => Some("kernel_array"),
        "Hash" if !args.is_empty() => Some("kernel_hash"),
        "format" | "sprintf" => Some("kernel_format"),
        "printf" => Some("kernel_printf"),
        "rand" => Some("kernel_rand"),
        "srand" => Some("kernel_srand"),
        "throw" if !args.is_empty() => Some("kernel_throw"),
        "sleep" => Some("kernel_sleep"),
        _ => None,
    };
    // `exit`/`abort` RAISE a rescuable `SystemExit` (CRuby's semantics):
    // they unwind through `ensure` and can be caught, so they emit a
    // `return Err(..)` rather than diverging.
    let raise_fn = match name {
        "exit" => Some("kernel_exit"),
        "abort" => Some("kernel_abort"),
        _ => None,
    };
    // `exit!` is CRuby's uncatchable immediate exit -- genuinely diverging.
    let never_fn = match name {
        "exit!" => Some("kernel_exit_bang"),
        _ => None,
    };
    if plain_fn.is_none() && fallible_fn.is_none() && never_fn.is_none() && raise_fn.is_none() {
        return None;
    }
    let mut arg_exprs: Vec<TokenStream> = args
        .iter()
        .map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        })
        .collect();
    // Format-family keywords ride along as one trailing Hash (the G2 ABI),
    // which the sprintf engine consults for `%<name>`/`%{name}` references.
    if !kwargs.is_empty() {
        let inserts = crate::codegen::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
        arg_exprs.push(quote! {
            {
                let __kw = zeo_rt::hash_new(vec![]);
                #inserts
                zeo_rt::RubyValue::Hash(__kw)
            }
        });
    }
    if let Some(f) = plain_fn {
        let func = format_ident!("{f}");
        return Some(quote! { zeo_rt::#func(&[#(#arg_exprs),*]) });
    }
    if let Some(f) = fallible_fn {
        let func = format_ident!("{f}");
        return Some(quote! { zeo_rt::#func(&[#(#arg_exprs),*])? });
    }
    if let Some(f) = raise_fn {
        let func = format_ident!("{f}");
        return Some(quote! { return Err(zeo_rt::#func(&[#(#arg_exprs),*])) });
    }
    let func = format_ident!("{}", never_fn.expect("one of the four sets matched"));
    Some(quote! { zeo_rt::#func(&[#(#arg_exprs),*]) })
}
