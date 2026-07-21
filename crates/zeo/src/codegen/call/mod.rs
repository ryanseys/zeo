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

// Re-exports so every pre-existing `crate::codegen::call::<name>` path used
// by sibling `codegen` modules keeps resolving after this file split --
// each name still lives at its original address as far as any caller
// outside this module tree is concerned.
pub use new::{emit_new, emit_new_with_arg_tokens};
pub use procs::emit_lambda_value;
pub(crate) use procs::emit_proc_or_lambda_value;
pub use super_calls::emit_super_inline;

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

/// Either inline-splice shape -- what the escaping-block scans
/// (`captures`/`hoisting`/`exceptions`) ask: a block NOT matching one of
/// these becomes a real, heap-allocated `Proc`.
pub fn is_inline_block_fast_path(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> bool {
    is_times_fast_path(compiler, receiver, name, kwargs_empty)
        || is_range_each_fast_path(compiler, receiver, name, kwargs_empty)
}

/// The one native counted-loop splice behind both `n.times { }` and
/// `(a..b).each { }`: iterate `__i` from `start` to `stop`
/// (`inclusive` decides `>` vs `>=`), binding the block's first required
/// param as a fresh `Int` each iteration, and evaluate to `result` (times:
/// the receiver Int; range each: the receiver range). The body is spliced
/// into this Rust scope -- no closure or `Proc` object is ever allocated --
/// sharing `codegen::loops`' redo-wrapping so `break`/`next`/`redo` work
/// exactly as in `while`/`until`/`for`.
#[allow(clippy::too_many_arguments)]
fn emit_counted_block_splice(
    cx: &Ctx,
    block_id: NodeId,
    start: i64,
    stop: i64,
    inclusive: bool,
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
    // `nil` (the case the old nested-capture guard rejected outright).
    let nested_captured: std::collections::HashSet<String> =
        super::captures::collect_escaping_captures(cx.compiler, body, params, cx.current_class)
            .locals;
    let mut loop_cx = cx.in_loop(redo.clone(), outer.clone());
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
    let inner = super::loops::emit_redo_wrapped_body(&loop_cx, body, &redo);
    let done = if inclusive {
        quote! { __i > #stop }
    } else {
        quote! { __i >= #stop }
    };
    quote! {
        {
            let mut __i: i64 = #start;
            #outer: loop {
                if #done { break #outer #result; }
                #bind
                #(#block_locals)*
                #inner
                __i += 1;
            }
        }
    }
}
/// Finishes a dynamic-dispatch (`send_value`) emission: `catch_break`
/// wraps the call ONLY when the call site itself carries a block -- a
/// `Signal::Break` can only ever target a block attached to THIS call, so
/// on a blockless call any arriving Break belongs to an OUTER block and
/// must keep propagating. (The unconditional wrap this replaced silently
/// ate a consumer's iteration-terminating break as it crossed a
/// `Yielder#<<` call inside an `Enumerator.new` generator -- turning
/// `infinite_enum.take(3)` into a hang. Phase 17.2.)
fn wrap_dynamic_result(has_block: bool, call: TokenStream) -> TokenStream {
    if has_block {
        quote! { zeo_rt::catch_break(#call)? }
    } else {
        quote! { (#call)? }
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
        let slf = &cx.self_ident;
        return Some(quote! { (#slf).clone() });
    }
    if let Some(cid) = cx.current_class {
        let slf = &cx.self_ident;
        return Some(if cx.compiler.value_backed(cid) {
            quote! { (#slf.clone()) }
        } else {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#slf.clone())) }
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
    Some(quote! {
        zeo_rt::RubyValue::Hash(zeo_rt::hash_new(vec![#(#pairs),*]))
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

    // `binding.local_variable_get(:name)` -- with a literal symbol naming an
    // in-scope local, this is the one Binding operation with a fully STATIC
    // answer (the value of that local), so it lowers to a direct read. It is
    // also the only way to read a reserved-word parameter (`def f(then:)` ->
    // `binding.local_variable_get(:then)`). The receiver must be a bare
    // `binding` call; a stored Binding (`b = binding; b.local_variable_get`)
    // is out of scope (it would need a captured-scope object).
    if name == "local_variable_get" && args.len() == 1 {
        if let Some(rid) = receiver {
            if let HirNode::Call {
                receiver: None,
                name: bname,
                args: bargs,
                ..
            } = &cx.compiler.hir[rid]
            {
                if bname == "binding" && bargs.is_empty() {
                    if let HirNode::SymbolLit(local) = &cx.compiler.hir[args[0]] {
                        return super::hoisting::emit_local_read(cx, local);
                    }
                }
            }
        }
    }

    // Implicit self / no receiver. `&.` is meaningless without a receiver,
    // so `safe` is irrelevant here.
    let Some(recv_id) = receiver else {
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
                zeo_rt::RubyValue::Bool(zeo_rt::responds_to_value(&#recv, #sym_expr, #include_all))
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
            if cx.compiler.value_backed(cid) {
                let slf = &cx.self_ident;
                if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
                    let scope = cx.compiler.scope(sid);
                    let mod_ident = super::ident::class_ident(cx.compiler, cid);
                    let method_ident = safe_ident(name);
                    return super::params::emit_call_args_to(
                        cx,
                        &super::params::Callee::FreeFn {
                            path: quote! { #mod_ident::#method_ident },
                            recv: quote! { #slf.clone() },
                        },
                        name,
                        &scope.params,
                        args,
                        kwargs,
                        block,
                        block_arg,
                        scope.needs_block_param(),
                    );
                }
                // The universal Kernel forms resolve here too -- `puts`/
                // `proc { }`/`__method__` inside a reopened builtin's (or a
                // top-level) method body are Kernel calls, not methods of
                // the receiver, and the dynamic fallback below would miss
                // them at runtime.
                if let Some(tokens) =
                    kernel::emit_universal_implicit_form(cx, name, args, kwargs, block, block_arg)
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
                let dyn_call = quote! {
                    zeo_rt::send_value_in(#__bx,
                        &#slf,
                        zeo_rt::Symbol::intern(#name),
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
            if let Some((_, sid)) = cx
                .compiler
                .method_in_chain(cid, name)
                .filter(|_| !cx.self_is_dynamic)
            {
                let scope = cx.compiler.scope(sid);
                let slf = &cx.self_ident;
                let recv_expr = quote! { #slf.clone() };
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
                );
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
        if cx.current_class.is_none() {
            if let Some(defining) = cx.class_self.or(cx.defining_class) {
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
                    let ctor =
                        new::emit_new(cx, &cx.compiler.fq_name(defining), args, kwargs, None);
                    // A native-backed class (D3) has no struct to `new_handle`
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
        if cx.current_class.is_none() {
            if let Some((_, sid)) = cx
                .compiler
                .method_in_chain(crate::compiler::OBJECT_CLASS, name)
            {
                let scope = cx.compiler.scope(sid);
                let mod_ident =
                    super::ident::class_ident(cx.compiler, crate::compiler::OBJECT_CLASS);
                let method_ident = safe_ident(name);
                // `boxed_implicit_self` IS this rule ("self here, boxed"),
                // including the case this used to get wrong: inside an
                // escaping block it answers the block's own receiver, so a
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
                );
            }
        }
        // The Kernel FUNCTIONS: the print family (multi-arg
        // now), conversions, rand/srand, throw, sleep, exit/abort --
        // checked AFTER sibling method resolution (a user `def puts`/`def
        // Integer` wins, real Ruby's rule; the old intercept-first
        // ordering was a latent bug this stage fixed). Capitalized-name
        // conversion calls WITH arguments parse as ordinary CallNodes, so
        // there's no ClassRef ambiguity.
        if let Some(tokens) =
            kernel::emit_universal_implicit_form(cx, name, args, kwargs, block, block_arg)
        {
            return tokens;
        }
        // `catch(:tag) { ... }` -- the one Kernel function that takes its
        // block as a first-class value.
        if name == "catch" && args.len() == 1 && kwargs.is_empty() {
            if let Some(b) = block {
                let tag = emit_expr(cx, args[0]);
                let blk = procs::emit_proc_value(cx, b);
                return quote! { zeo_rt::kernel_catch(#tag, #blk)? };
            }
        }
        // `to_enum(:meth, *args)` / `enum_for` on the implicit self (Phase
        // 17.2): routed through dynamic dispatch, whose Kernel row builds
        // the Enumerator over the boxed receiver -- what the Struct
        // template's `return to_enum(:each) unless block_given?` compiles
        // to.
        if (name == "to_enum" || name == "enum_for")
            && kwargs.is_empty()
            && block.is_none()
            && block_arg.is_none()
        {
            if let Some(cid) = cx.current_class {
                let slf = &cx.self_ident;
                // A reopened builtin's (or `Object`'s) `self` is already a
                // boxed `RubyValue`.
                let boxed = if cx.compiler.value_backed(cid) {
                    quote! { (#slf.clone()) }
                } else {
                    let class_ident = super::ident::class_ident(cx.compiler, cid);
                    quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#slf.clone())) }
                };
                let arg_exprs: Vec<TokenStream> = args
                    .iter()
                    .map(|&a| {
                        let e = emit_expr(cx, a);
                        box_if_object_typed(cx, a, e)
                    })
                    .collect();
                return quote! {
                    zeo_rt::send_value_in(#__bx,
                        &#boxed,
                        zeo_rt::Symbol::intern(#name),
                        &[#(#arg_exprs),*],
                        None,
                    )?
                };
            }
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
        return quote! {
            zeo_rt::send_value_in(#__bx,
                &#recv,
                zeo_rt::Symbol::intern(#name),
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
    // Integer constant, not a class) must NOT take this branch -- found as
    // a real, previously-undetected bug via this session's own testing: it
    // unconditionally treated ANY `ClassRef` receiver as a class-method
    // call, so compound assignment (`+=`/`-=`/etc., every operator except
    // `||=`) on a non-class constant panicked with a confusing "unknown
    // class/module" error instead of reading its actual value.
    // The concurrency builtins' constructors and `Fiber.yield` (Phases
    // 13.3/13.5) -- intercepted ahead of the generic class-method branch
    // below (which would reject the block / find no such class method). A
    // `Fiber.new`/`Thread.new` block becomes an ordinary escaping `Proc`
    // via `emit_proc_value` -- the same capture machinery every other
    // escaping block uses, so captured locals/`self` compose for free. All
    // FiberError construction happens here, not in `zeo_rt::fiber_*`
    // (the `array_set`->`IndexError` division of labor; messages verbatim
    // from CRuby `cont.c`).
    if let HirNode::ClassRef(target_name) = &cx.compiler.hir[recv_id] {
        if !safe && kwargs.is_empty() {
            match (target_name.as_str(), name) {
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
                    let Some(block_id) = block else {
                        if block_arg.is_some() {
                            panic!(
                                "`Fiber.new` requires a literal block (spike scope -- `&proc` conversion isn't wired here yet)"
                            );
                        }
                        return raise::emit_missing_block_raise(cx, "Fiber");
                    };
                    let proc = procs::emit_proc_value(cx, block_id);
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
                    let Some(block_id) = block else {
                        if block_arg.is_some() {
                            panic!(
                                "`Thread.new` requires a literal block (spike scope -- `&proc` conversion isn't wired here yet)"
                            );
                        }
                        return raise::emit_missing_block_raise(cx, "Thread");
                    };
                    let proc = procs::emit_proc_value(cx, block_id);
                    let arg_exprs: Vec<TokenStream> = args
                        .iter()
                        .map(|&a| {
                            let e = emit_expr(cx, a);
                            super::expr::box_if_object_typed(cx, a, e)
                        })
                        .collect();
                    return quote! { zeo_rt::thread_new(#proc, vec![#(#arg_exprs),*]) };
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
                // `Ractor.new(*args) { |*params| }` -- block
                // ISOLATION is enforced HERE, at compile time (the capture
                // set is statically known), strictly earlier than CRuby's
                // own Proc-creation-time `Ractor::IsolationError`. Args
                // cross the boundary at runtime (shareable-by-reference or
                // deep-copied; a rejection raises `RactorError`).
                ("Ractor", "new") => {
                    let Some(block_id) = block else {
                        if block_arg.is_some() {
                            panic!("`Ractor.new` requires a literal block (spike scope)");
                        }
                        return raise::emit_missing_block_raise(cx, "Ractor");
                    };
                    let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
                        panic!(
                            "internal error: a Block node should only be reached via the Call that invokes it"
                        );
                    };
                    let block_caps = super::captures::block_captures(
                        cx.compiler,
                        params,
                        body,
                        cx.current_class,
                    );
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
                    let assigned_here: std::collections::HashSet<&String> =
                        assigned_here.iter().collect();
                    if let Some(outer) = block_caps
                        .locals
                        .iter()
                        .filter(|n| cx.captured_locals.contains(*n) || !assigned_here.contains(n))
                        .min()
                    {
                        panic!(
                            "can not isolate a Proc because it accesses outer variables ({outer})"
                        );
                    }
                    if block_caps.self_captured {
                        panic!(
                            "can not isolate a Proc because it accesses instance variables of the enclosing object"
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
                    let ractor_error = raise::emit_ractor_error(cx);
                    return quote! {
                        match zeo_rt::ractor_new(#proc, vec![#(#arg_exprs),*]) {
                            Ok(__r) => __r,
                            Err(__msg) => return Err(zeo_rt::Signal::Raise(#ractor_error)),
                        }
                    };
                }
                ("Ractor", "receive") if args.is_empty() && block.is_none() => {
                    return quote! { zeo_rt::ractor_receive() };
                }
                ("Ractor", "make_shareable") if args.len() == 1 && block.is_none() => {
                    let v = emit_expr(cx, args[0]);
                    let v = super::expr::box_if_object_typed(cx, args[0], v);
                    let ractor_error = raise::emit_ractor_error(cx);
                    return quote! {
                        match zeo_rt::make_shareable(&(#v)) {
                            Ok(__v) => __v,
                            Err(__msg) => return Err(zeo_rt::Signal::Raise(#ractor_error)),
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
    }

    if let Some(target_path) = super::expr::const_path_of(cx, recv_id) {
        if let Some(target) = cx.resolve_class(&target_path) {
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
            // behavior, replacing the old compile-time rejection.
            let is_static = cx.compiler.class_method_in_chain(target, name).is_some();
            if is_static {
                if safe {
                    panic!(
                        "safe-navigation on a class-method call isn't supported yet (spike scope)"
                    );
                }
                return reflect::emit_class_method_call_on(
                    cx, target, name, args, kwargs, block, block_arg,
                );
            }
        }
    }

    // A receiver STATICALLY TYPED as a class value (`x = Widget;
    // x.new(...)` / `x.some_class_method` -- Phase 16.1): same Path 1
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
            if name == "new"
                && !cx.compiler.class(target).is_module
                && !cx.compiler.class(target).is_builtin
            {
                let recv_expr = emit_expr(cx, recv_id);
                let ctor = new::emit_new(cx, &cx.compiler.fq_name(target), args, kwargs, None);
                return quote! { { let _ = #recv_expr; #ctor } };
            }
            if cx.compiler.class_method_in_chain(target, name).is_some() {
                let recv_expr = emit_expr(cx, recv_id);
                let call = reflect::emit_class_method_call_on(
                    cx, target, name, args, kwargs, block, block_arg,
                );
                return quote! { { let _ = #recv_expr; #call } };
            }
        }
    }

    if safe {
        if !kwargs.is_empty() {
            panic!(
                "keyword arguments on a safe-navigation (`&.`) call aren't supported yet (spike scope)"
            );
        }
        if block.is_some() || block_arg.is_some() {
            panic!("a block on a safe-navigation (`&.`) call isn't supported yet (spike scope)");
        }
        return path2::emit_safe_call(cx, recv_id, name, args);
    }

    let recv_expr = emit_expr(cx, recv_id);
    dispatch(
        cx, recv_id, name, args, kwargs, block, block_arg, &recv_expr, false,
    )
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
    if let Some(cid) = infer_class(cx, recv_id) {
        if cx.compiler.is_blank_slate(cid)
            && cx.compiler.method_in_chain(cid, name).is_none()
            && !crate::compiler::is_basic_object_method(name)
        {
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
    if no_kwargs && name == "!" && args.is_empty() {
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
    if no_kwargs && (name == "is_a?" || name == "kind_of?") && args.len() == 1 {
        if let Some(target_name) = super::expr::const_path_of(cx, args[0]) {
            // A top-level anchor `::Name` carries the scope "Object" (the
            // root); its name is an ordinary top-level class, so fall back to
            // resolving the tail when `Object::Name` doesn't resolve directly.
            let resolved = cx.resolve_class(&target_name).or_else(|| {
                target_name
                    .strip_prefix("Object::")
                    .and_then(|t| cx.resolve_class(t))
            });
            let Some(target) = resolved else {
                // A constant bound to a RUNTIME class (`Foo = Class.new`, #97
                // F4): resolve it at runtime and ancestry-check its id.
                let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
                return quote! {
                    {
                        let __rtc = zeo_rt::const_get(0, #target_name).ok_or_else(|| {
                            zeo_rt::raise_error("NameError", format!("uninitialized constant {}", #target_name))
                        })?;
                        match __rtc {
                            zeo_rt::RubyValue::Class(__tid) => zeo_rt::RubyValue::Bool(
                                zeo_rt::is_a((#recv_boxed).class_id(), __tid)),
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
            return match infer_any_class(cx, recv_id) {
                Some(recv_class) => {
                    let result = cx.compiler.class(recv_class).ancestors.contains(&target);
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
                    zeo_rt::RubyValue::Bool(zeo_rt::is_a(
                        (#recv_expr).class_id(),
                        zeo_rt::ClassId(#target_id),
                    ))
                },
            };
        }
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
    {
        if let TyKind::Object(cid) = infer(cx, recv_id) {
            if cx.compiler.method_in_chain(cid, name).is_none() {
                let recv_boxed = super::expr::box_if_object_typed(cx, recv_id, recv_expr.clone());
                let arg = emit_expr(cx, args[0]);
                let arg = super::expr::box_if_object_typed(cx, args[0], arg);
                let negate = name == "!=";
                return quote! {
                    zeo_rt::RubyValue::Bool(zeo_rt::rb_eq_checked(&(#recv_boxed), &(#arg))? != #negate)
                };
            }
        }
    }

    // `instance_of?` against a literal class/module constant
    // -- EXACT class identity, not ancestry (`w.instance_of?(Object)` is
    // false for a Widget); same static-fold-else-runtime shape as
    // `is_a?`/`kind_of?` above. A non-constant argument falls through to
    // the dynamic path (`send`/`send_value`'s Class-argument arms).
    if no_kwargs && name == "instance_of?" && args.len() == 1 {
        if let Some(target_name) = super::expr::const_path_of(cx, args[0]) {
            // A top-level anchor `::Name` carries the scope "Object" (the
            // root); its name is an ordinary top-level class, so fall back to
            // resolving the tail when `Object::Name` doesn't resolve directly.
            let resolved = cx.resolve_class(&target_name).or_else(|| {
                target_name
                    .strip_prefix("Object::")
                    .and_then(|t| cx.resolve_class(t))
            });
            let Some(target) = resolved else {
                // A constant bound to a RUNTIME class (`Foo = Class.new`, #97
                // F4): resolve it at runtime and check EXACT class identity.
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
                // Two builtins can't be constant-folded here. `Queue` may be a
                // subclass (`SizedQueue`, which types as `Queue` but carries its
                // own class id). `MatchData` is nilable: `str.match(re)` types as
                // MatchData but returns `nil` on no match, so `.class` must be
                // read at runtime (`"x".match(/z/).class == NilClass`).
                Some(cid) if cid != zeo_abi::QUEUE_CLASS && cid != zeo_abi::MATCH_DATA_CLASS => {
                    let id = cid.0;
                    quote! { { let _ = #recv_expr; zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) } }
                }
                _ => quote! { zeo_rt::RubyValue::Class((#recv_expr).class_id()) },
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
    if no_kwargs && name == "respond_to?" && (args.len() == 1 || args.len() == 2) {
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
        // (#97 F3) -- keyed by object identity, so a class-id-only probe can't
        // see it. The singleton fast path (`is_live()`) means an ordinary
        // program pays only one predictable atomic here.
        let boxed_recv = super::expr::box_if_object_typed(cx, recv_id, recv_expr.clone());
        return quote! {
            zeo_rt::RubyValue::Bool(zeo_rt::responds_to_value(&#boxed_recv, #sym_expr, #include_all))
        };
    }

    // `.nil?` -- universal, same override-respecting shape as
    // `freeze`/`frozen?` below (surfaced as a real need by Phase 13.5's
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
                        quote! { (#recv_expr).freeze_value() }
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
                _ => quote! { (#recv_expr).dup_value(#copy_frozen) },
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
            let double =
                raise::emit_fiber_error(cx, "attempt to resume a resumed fiber (double resume)");
            let cross = raise::emit_fiber_error(cx, "fiber called across threads");
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
                    zeo_rt::FiberResume::DoubleResume => {
                        return Err(zeo_rt::Signal::Raise(#double))
                    }
                    zeo_rt::FiberResume::CrossThread => {
                        return Err(zeo_rt::Signal::Raise(#cross))
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
            let double =
                raise::emit_fiber_error(cx, "attempt to resume a resumed fiber (double resume)");
            let cross = raise::emit_fiber_error(cx, "fiber called across threads");
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
                    zeo_rt::FiberResume::DoubleResume => {
                        return Err(zeo_rt::Signal::Raise(#double))
                    }
                    zeo_rt::FiberResume::CrossThread => {
                        return Err(zeo_rt::Signal::Raise(#cross))
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
    // `zeo_rt::thread_outcome` (a real may yield point); an `Err` is the
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

    // `Queue`: `pop` blocks coroutine-yieldingly; a closed
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
    if no_kwargs && infer(cx, recv_id) == TyKind::Ractor && block.is_none() && block_arg.is_none() {
        let ractor_error = raise::emit_ractor_error(cx);
        match (name, args.len()) {
            ("send", 1) => {
                let v = emit_expr(cx, args[0]);
                let v = super::expr::box_if_object_typed(cx, args[0], v);
                return quote! {
                    {
                        let __r = (#recv_expr).as_ractor_unchecked();
                        match zeo_rt::ractor_send(&__r, &(#v)) {
                            Ok(()) => zeo_rt::RubyValue::Ractor(__r),
                            Err(__msg) => return Err(zeo_rt::Signal::Raise(#ractor_error)),
                        }
                    }
                };
            }
            ("value", 0) => {
                return quote! {
                    match zeo_rt::ractor_outcome(&(#recv_expr).as_ractor_unchecked()) {
                        Ok(__v) => __v,
                        Err(__sig) => return Err(__sig),
                    }
                };
            }
            ("join", 0) => {
                return quote! {
                    {
                        let __r = (#recv_expr).as_ractor_unchecked();
                        match zeo_rt::ractor_outcome(&__r) {
                            Ok(_) => zeo_rt::RubyValue::Ractor(__r),
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
    if no_kwargs && args.len() == 1 {
        if let Some(&(_, rt_fn, kind)) = ops::INT_BINARY_OPS.iter().find(|(op, _, _)| *op == name) {
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
    }

    // Native `Int` unary operators (`-@`/`+@`/`~`), same eligibility rule
    // (all three return `RubyValue` -- negation can promote `-i64::MIN`).
    if no_kwargs && args.is_empty() {
        if let Some(&(_, rt_fn)) = ops::INT_UNARY_OPS.iter().find(|(op, _)| *op == name) {
            if infer(cx, recv_id) == TyKind::Int {
                let func = format_ident!("{rt_fn}");
                return quote! { zeo_rt::#func(&(#recv_expr)) };
            }
        }
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
    if no_kwargs && args.is_empty() {
        if let Some(&(_, rt_fn)) = ops::FLOAT_UNARY_OPS.iter().find(|(op, _)| *op == name) {
            if infer(cx, recv_id) == TyKind::Float {
                let func = format_ident!("{rt_fn}");
                return quote! {
                    zeo_rt::RubyValue::Float(zeo_rt::#func((#recv_expr).as_float_unchecked()))
                };
            }
        }
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
    if let Some(cid) = infer_any_class(cx, recv_id) {
        if cx.compiler.class(cid).is_builtin {
            if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
                let scope = cx.compiler.scope(sid);
                if !bypass_visibility {
                    if let Some(err) = visibility::enforce_visibility(cx, recv_id, scope, name) {
                        return err;
                    }
                }
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
                );
            }
        }
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
    {
        if let HirNode::IntegerLit(n) = &cx.compiler.hir[recv_id] {
            let n = *n;
            // `Integer#times` evaluates to its receiver (MRI), not nil --
            // matters in expression position (`x = 5.times {}`).
            return emit_counted_block_splice(
                cx,
                block_id,
                0,
                n,
                false,
                "times",
                quote! { zeo_rt::RubyValue::Int(#n) },
            );
        }
    }

    // `(a..b).each { |i| }` on a LITERAL Int-bounded range: the same native
    // counted loop `.times` fuses to (the generic path allocates a real
    // Proc and dynamic-dispatches every yield -- bm_range_each spent 65s
    // there). `.each` answers the receiver range. `i64::MAX`-inclusive is
    // excluded: the `__i += 1` after the final iteration would overflow.
    if let Some(block_id) =
        block.filter(|_| is_range_each_fast_path(cx.compiler, Some(recv_id), name, no_kwargs))
    {
        if let HirNode::RangeLit {
            start: Some(s),
            end: Some(e),
            exclusive,
        } = &cx.compiler.hir[recv_id]
        {
            if let (HirNode::IntegerLit(s), HirNode::IntegerLit(e)) =
                (&cx.compiler.hir[*s], &cx.compiler.hir[*e])
            {
                let (s, e, exclusive) = (*s, *e, *exclusive);
                if exclusive || e < i64::MAX {
                    return emit_counted_block_splice(
                        cx,
                        block_id,
                        s,
                        e,
                        !exclusive,
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
            }
        }
    }

    let recv_class = infer_class(cx, recv_id);

    // `send`/`public_send`: try literal-name static resolution first
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
    if (name == "send" || name == "public_send") && !args.is_empty() {
        if let HirNode::SymbolLit(target) = &cx.compiler.hir[args[0]] {
            let target = target.clone();
            if let Some(cid) = recv_class {
                if let Some((_, sid)) = cx.compiler.method_in_chain(cid, &target) {
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
    if let Some(cid) = recv_class {
        if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
            let scope = cx.compiler.scope(sid);
            if !bypass_visibility {
                if let Some(err) = visibility::enforce_visibility(cx, recv_id, scope, name) {
                    return err;
                }
            }
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
            );
        }
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
    // `raise`/`rescue` failure in this spike) is a faithful, not a lesser,
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
                // One inline Int-Int fast arm (the hot `def add(a, b); a +
                // b; end` case); EVERY other operand shape -- Float pairs,
                // mixed promotion, Bignum/Rational/Complex lanes, user
                // operator methods, builtin rows -- resolves through
                // `send_value`'s MRO walk, whose Integer/Float operator
                // rows drive the same one tower matrix. The
                // old hand-inlined Float/mixed arms are gone: they
                // duplicated the promotion rules and knew nothing of the
                // new lanes.
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
                return quote! {
                    match (&(#recv_expr), &(#arg_expr)) {
                        #int_arm
                        (__dyn_recv, __dyn_arg) => zeo_rt::send_value_in(#__bx,
                            __dyn_recv,
                            zeo_rt::Symbol::intern(#name),
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
                // Same shape as the binary fallback: inline Int fast arm,
                // everything else through the MRO walk's unary rows.
                let int_arm = int_entry.map(|&(_, rt_fn)| {
                    let func = format_ident!("{rt_fn}");
                    quote! {
                        __r @ zeo_rt::RubyValue::Int(_) => zeo_rt::#func(__r),
                    }
                });
                return quote! {
                    match &(#recv_expr) {
                        #int_arm
                        __dyn_recv => zeo_rt::send_value_in(#__bx,
                            __dyn_recv,
                            zeo_rt::Symbol::intern(#name),
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
    // one of those falls through to `send_value`'s DYNAMIC dispatch (Phase
    // 14.4): its builtin method table handles the supported operations
    // (`[1,2,3].each { ... }` works now), and anything else raises a real,
    // rescuable `NoMethodError` at runtime with the builtin class's actual
    // name -- Ruby's own behavior, replacing the old compile-time
    // "unsupported call" panic for statically-collection-typed receivers.
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
            // Since Phase 17.1's MRO-walking method tables, EVERY value
            // kind falls through -- `5.itself`/`"a".between?(...)` resolve
            // Kernel/Comparable rows down the receiver's real ancestor
            // chain at runtime, and a genuinely unknown name raises real
            // Ruby's NoMethodError instead of the old compile-time panic.
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
        let name_expr = quote! { zeo_rt::Symbol::intern(#name) };
        let arg_exprs = args.iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        // Keyword args ride as one trailing Hash (the G2 convention).
        let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
        let block_value = emit_block_option(cx, block, block_arg);
        let dyn_call = quote! {
            zeo_rt::send_value_in(#__bx,
                &(#recv_expr),
                #name_expr,
                &[#(#arg_exprs,)* #(#kw_hash,)*],
                #block_value,
            )
        };
        return wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
    }

    // A statically-known class that `include Enumerable` (the RUST-backed
    // builtin module, Phase 14.4 rev.2) with no own/materialized definition
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
    if let Some(cid) = recv_class {
        if cx
            .compiler
            .class(cid)
            .ancestors
            .contains(&crate::compiler::ENUMERABLE_CLASS)
            || cx
                .compiler
                .class(cid)
                .ancestors
                .contains(&crate::compiler::COMPARABLE_CLASS)
        {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            let name_expr = quote! { zeo_rt::Symbol::intern(#name) };
            let arg_exprs = args.iter().map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            });
            // Keyword args ride as one trailing Hash (the G2 convention).
            let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
            let block_value = emit_block_option(cx, block, block_arg);
            return quote! {
                zeo_rt::catch_break(zeo_rt::send_value_in(#__bx,
                    &zeo_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)),
                    #name_expr,
                    &[#(#arg_exprs,)* #(#kw_hash,)*],
                    #block_value,
                ))?
            };
        }
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
    quote! {
        zeo_rt::catch_break(zeo_rt::send_value_in(#__bx,
            &#recv_boxed,
            #name_expr,
            &[#(#arg_exprs,)* #(#kw_hash,)*],
            #block_value,
        ))?
    }
}
