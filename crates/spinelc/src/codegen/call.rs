//! `emit_call`'s dispatch decision -- see "Two dispatch paths" in the plan:
//! Path 1 (static direct call, or a call rewritten away entirely, e.g.
//! `super` inlining) whenever `Compiler::method_in_chain` resolves the
//! target from the receiver's statically-known class; Path 2
//! (`spinel_rt::send`) only when it can't. Every fragment produced here
//! evaluates to a bare `spinel_rt::RubyValue` -- any call into a
//! `Result`-returning method has `?` applied right here, at the call site,
//! not left to the caller (see `expr.rs`'s module docs).

use quote::{format_ident, quote};

use super::expr::{emit_expr, emit_symbol_expr, infer, infer_class};
use super::ident::safe_ident;
use super::stmt::emit_body;
use super::Ctx;
use crate::hir::{HirNode, NodeId};
use crate::types::TyKind;
use proc_macro2::TokenStream;

/// Binary operators with a native `spinel_rt::int_*` implementation, and
/// which `spinel_rt::RubyValue` variant wraps their result. Every one of
/// these is a plain `CallNode` at the `ruby-prism` level (`a + b` and
/// `a.foo(b)` are the same node shape, just a different `name()`) -- so this
/// table is the entire generalization of the pre-Phase-1 spike's single
/// hardcoded literal-`+`-on-`IntegerLit` fast path: any operand pair
/// statically known `Int` (not just literals -- see
/// `analyze::locals`/`types::infer_type_with_locals`) routes through here;
/// anything else falls through to ordinary Path 1/Path 2 method dispatch
/// below, which is what makes a user class's own `def <=>`/`def +` etc.
/// dispatch correctly instead of needing special-casing here.
const INT_BINARY_OPS: &[(&str, &str, &str)] = &[
    ("+", "int_add", "Int"),
    ("-", "int_sub", "Int"),
    ("*", "int_mul", "Int"),
    ("/", "int_div", "Int"),
    ("%", "int_mod", "Int"),
    ("**", "int_pow", "Int"),
    ("&", "int_band", "Int"),
    ("|", "int_bor", "Int"),
    ("^", "int_bxor", "Int"),
    ("<<", "int_shl", "Int"),
    (">>", "int_shr", "Int"),
    ("==", "int_eq", "Bool"),
    ("!=", "int_neq", "Bool"),
    ("<", "int_lt", "Bool"),
    (">", "int_gt", "Bool"),
    ("<=", "int_le", "Bool"),
    (">=", "int_ge", "Bool"),
    ("<=>", "int_cmp", "Int"),
];

const INT_UNARY_OPS: &[(&str, &str)] = &[
    ("-@", "int_neg"),
    ("+@", "int_pos"),
    ("~", "int_bnot"),
];

pub fn emit_new(cx: &Ctx, class_name: &str, args: &[NodeId]) -> TokenStream {
    let cid = cx
        .compiler
        .class_by_name(class_name)
        .unwrap_or_else(|| panic!("unknown class `{class_name}`"));
    let ci = cx.compiler.class(cid);
    let class_ident = safe_ident(class_name);

    let fields = ci.ivars.iter().map(|iv| {
        let f = safe_ident(iv);
        quote! { #f: std::cell::RefCell::new(spinel_rt::RubyValue::Nil), }
    });
    // Wrapped in `Rc` immediately, not just at `new_handle` time: a local
    // holding this needs to be `Rc::clone()`-able on every re-read
    // (`codegen::expr`'s `LocalRead` -- see `ruby_class!`'s `new_handle` docs
    // for why the bare struct can't just derive `Clone` instead). `Rc<T>`
    // derefs transparently, so `.initialize(...)` and Path 1's
    // `(recv_expr).method(...)` both still work unchanged.
    let ctor = quote! { std::rc::Rc::new(#class_ident { #(#fields)* }) };

    match cx.compiler.method_in_chain(cid, "initialize") {
        Some(_) => {
            let arg_exprs = args.iter().map(|&a| emit_expr(cx, a));
            quote! { { let __obj = #ctor; __obj.initialize(#(#arg_exprs),*)?; __obj } }
        }
        None => ctor,
    }
}

/// `super` always resolves against the *static* superclass -- never through
/// the dynamic dispatch table (mirrors `emit_super`, `codegen.c:3262`). The
/// spike implements it via statement inlining rather than a cross-type
/// function call: subclasses in this object model are distinct Rust structs
/// with no shared layout (unlike spinel's C "common initial sequence"
/// trick), so `Animal::speak(self: &Dog)` wouldn't type-check. Splicing the
/// resolved parent method's body directly into the call site sidesteps that
/// entirely -- and is a real spinel mechanism too, just used there as an
/// optimization (`emit_super_inline`, reached when the parent yields) rather
/// than the default.
pub fn emit_super_inline(cx: &Ctx) -> TokenStream {
    let cid = cx.current_class.expect("`super` outside a method");
    let mname = cx
        .current_method
        .as_deref()
        .expect("`super` outside a method");
    let parent = cx.compiler.class(cid).parent.unwrap_or_else(|| {
        panic!(
            "`super` in {}, which has no superclass",
            cx.compiler.class(cid).name
        )
    });
    let (defining_class, sid) = cx
        .compiler
        .method_in_chain(parent, mname)
        .unwrap_or_else(|| {
            panic!(
                "`super`: no `{mname}` found above {}",
                cx.compiler.class(cid).name
            )
        });

    let defining_scope = cx.compiler.scope(sid);
    let body = defining_scope.body.clone();
    let inline_cx = Ctx {
        compiler: cx.compiler,
        current_class: Some(defining_class),
        current_method: Some(mname.to_string()),
        local_types: &defining_scope.local_types,
    };
    let inlined = emit_body(&inline_cx, &body, false);
    quote! { { #inlined } }
}

pub fn emit_call(
    cx: &Ctx,
    receiver: Option<NodeId>,
    name: &str,
    args: &[NodeId],
    block: Option<NodeId>,
    safe: bool,
) -> TokenStream {
    // Implicit self / no receiver: the spike's only builtin is `puts`.
    // `&.` is meaningless without a receiver, so `safe` is irrelevant here.
    let Some(recv_id) = receiver else {
        if name == "puts" && args.len() == 1 {
            let arg = emit_expr(cx, args[0]);
            return quote! { { spinel_rt::puts(#arg); spinel_rt::RubyValue::Nil } };
        }
        panic!("unsupported implicit-self call `{name}` (spike scope)");
    };

    if safe {
        return emit_safe_call(cx, recv_id, name, args);
    }

    let recv_expr = emit_expr(cx, recv_id);
    dispatch(cx, recv_id, name, args, block, &recv_expr)
}

/// `&.` always dispatches through the runtime `ClassRegistry`/`send` path
/// (Path 2), regardless of whether the receiver's class is statically
/// known -- unlike ordinary calls, which prefer a direct Path 1 call. The
/// receiver's *concrete Rust type* differs depending on that: an unboxed
/// class struct (e.g. `Box`, with no `.is_nil()`/`Clone`) when the class is
/// known and constructed via `New`, or an already-boxed `RubyValue` when
/// it's dynamically typed. Checking "is it nil" needs one uniform runtime
/// representation either way, so a statically-known-class receiver gets
/// boxed into `RubyValue::Object` here (an otherwise-avoidable `Rc`
/// allocation this specific call site pays for `&.`'s uniformity) before the
/// same nil-check-then-`send` logic runs regardless of which case it was.
fn emit_safe_call(cx: &Ctx, recv_id: NodeId, name: &str, args: &[NodeId]) -> TokenStream {
    let boxed_recv = match infer_class(cx, recv_id) {
        Some(cid) => {
            let class_ident = safe_ident(&cx.compiler.class(cid).name);
            let recv_expr = emit_expr(cx, recv_id);
            quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
        }
        None => emit_expr(cx, recv_id),
    };
    let name_expr = quote! { spinel_rt::Symbol::intern(#name) };
    let arg_exprs = args.iter().map(|&a| emit_expr(cx, a));
    quote! {
        {
            let __safe_recv = #boxed_recv;
            if __safe_recv.is_nil() {
                spinel_rt::RubyValue::Nil
            } else {
                match &__safe_recv {
                    spinel_rt::RubyValue::Object(__robj) => {
                        spinel_rt::send(__robj, #name_expr, &[#(#arg_exprs),*])?
                    }
                    _ => panic!(
                        "safe navigation on a non-Object receiver isn't supported yet (spike scope)"
                    ),
                }
            }
        }
    }
}

/// The actual dispatch decision (see the module's "Two dispatch paths"
/// docs), given an already-computed `recv_expr` for the receiver's runtime
/// value -- factored out of `emit_call` so `&.`'s nil-guard can wrap this
/// without the receiver expression being evaluated twice.
fn dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    block: Option<NodeId>,
    recv_expr: &TokenStream,
) -> TokenStream {
    // `!`/`not` -- Ruby truthiness on ANY value, not an `Int`-specific
    // operator (`!0`, `!""`, `!nil` are all valid and not equivalent),
    // so this is handled separately from the numeric tables below.
    if name == "!" && args.is_empty() {
        return quote! { spinel_rt::RubyValue::Bool(!(#recv_expr).truthy()) };
    }

    // Native `Int` arithmetic/comparison/bitwise ops: both operands must be
    // statically known `Int` (see `INT_BINARY_OPS`'s docs above).
    if args.len() == 1 {
        if let Some(&(_, rt_fn, result_ty)) =
            INT_BINARY_OPS.iter().find(|(op, _, _)| *op == name)
        {
            let recv_ty = infer(cx, recv_id);
            let arg_ty = infer(cx, args[0]);
            if recv_ty == TyKind::Int && arg_ty == TyKind::Int {
                let arg_expr = emit_expr(cx, args[0]);
                let func = format_ident!("{rt_fn}");
                let wrapper = format_ident!("{result_ty}");
                return quote! {
                    spinel_rt::RubyValue::#wrapper(spinel_rt::#func(
                        (#recv_expr).as_int_unchecked(),
                        (#arg_expr).as_int_unchecked(),
                    ))
                };
            }
        }
    }

    // Native `Int` unary operators (`-@`/`+@`/`~`), same eligibility rule.
    if args.is_empty() {
        if let Some(&(_, rt_fn)) = INT_UNARY_OPS.iter().find(|(op, _)| *op == name) {
            if infer(cx, recv_id) == TyKind::Int {
                let func = format_ident!("{rt_fn}");
                return quote! {
                    spinel_rt::RubyValue::Int(spinel_rt::#func((#recv_expr).as_int_unchecked()))
                };
            }
        }
    }

    // Known-shape block inlining (mirrors `emit_block_value_into`/`.times`,
    // `codegen_iter.c:1281`): the block body is spliced into a native Rust
    // `for` loop -- no closure or Proc object is allocated.
    if name == "times" {
        if let HirNode::IntegerLit(n) = &cx.compiler.hir[recv_id] {
            let n = *n;
            let block_id = block.unwrap_or_else(|| panic!("`times` requires a block"));
            let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
                panic!("`times`'s argument must be a block");
            };
            let bind = params.first().map(|p| {
                let ident = safe_ident(p);
                quote! { let #ident = spinel_rt::RubyValue::Int(__i); }
            });
            let inner = emit_body(cx, body, false);
            return quote! {
                { for __i in 0..#n { #bind #inner; } spinel_rt::RubyValue::Nil }
            };
        }
    }

    let recv_class = infer_class(cx, recv_id);

    // `send`/`public_send`: try literal-name static resolution first
    // (mirrors spinel's `desugar_public_send_recv`) -- rewritten to a direct
    // call (Path 1) when the class is known AND the name resolves. Falls to
    // Path 2 (the genuinely new part -- see the plan's Object Model
    // section) when the receiver's class isn't known, the name isn't a
    // literal, or the literal name doesn't resolve anywhere in the chain
    // (which is exactly the `method_missing` trigger condition).
    if (name == "send" || name == "public_send") && !args.is_empty() {
        if let HirNode::SymbolLit(target) = &cx.compiler.hir[args[0]] {
            let target = target.clone();
            if let Some(cid) = recv_class {
                if cx.compiler.method_in_chain(cid, &target).is_some() {
                    return dispatch(cx, recv_id, &target, &args[1..], block, recv_expr);
                }
            }
        }
        let cid = recv_class.unwrap_or_else(|| {
            panic!("dynamic `send` on a receiver of unknown static class (spike scope)")
        });
        let class_ident = safe_ident(&cx.compiler.class(cid).name);
        let name_expr = emit_symbol_expr(cx, args[0]);
        let rest_args = args[1..].iter().map(|&a| emit_expr(cx, a));
        return quote! {
            spinel_rt::send(&#class_ident::new_handle(#recv_expr), #name_expr, &[#(#rest_args),*])?
        };
    }

    // Ordinary call with a statically known receiver class: direct call
    // (Path 1). This is the common case -- `method_in_chain` mirrors
    // `comp_method_in_chain` exactly (compiler.c:404).
    if let Some(cid) = recv_class {
        if cx.compiler.method_in_chain(cid, name).is_some() {
            let method_ident = safe_ident(name);
            let arg_exprs = args.iter().map(|&a| emit_expr(cx, a));
            return quote! { (#recv_expr).#method_ident(#(#arg_exprs),*)? };
        }
    }

    panic!("unsupported call `{name}` (spike scope, or receiver's class isn't statically known)");
}
