//! `.new` construction: `emit_new` (the HIR-level `New` node, the common
//! case) and `emit_new_with_arg_tokens` (already-emitted argument
//! `TokenStream`s -- the narrower path `raise`'s codegen and a `**h`-splat
//! constructor call use), plus their shared `bind_new_args`/
//! `emit_ctor_struct` helpers.

use quote::quote;

use crate::codegen::Ctx;
use crate::codegen::expr::{box_if_object_typed, emit_expr};
use crate::codegen::ident::safe_ident;
use crate::hir::{KwArg, NodeId};
use proc_macro2::TokenStream;

pub fn emit_new(
    cx: &Ctx,
    class_name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
) -> TokenStream {
    // A USER class with a real `initialize`: bind its arguments through the
    // SAME `emit_call_args_to` machinery every other call site uses, so
    // `initialize` gets the full `Params` surface (splat/post/keyword/block),
    // including shapes like `def initialize(*values)`.
    //
    // Only this path can: the others have no generated `initialize` with a
    // `Params` to bind against (a builtin/module `.new` dispatches
    // dynamically; `Object.new`'s copy is a free function in a container).
    // They keep the token path below, which is also the one `raise`'s
    // synthetic-argument caller needs.
    let Some(cid) = cx.resolve_class(class_name) else {
        // Not a compile-time class -- a constant bound to a RUNTIME class
        // (`Foo = Class.new`, a native `Struct`/`Data` class -- Batch E). Read
        // the constant at runtime and dispatch `.new` dynamically; a
        // truly-undefined constant raises NameError via `const_get`, matching
        // Ruby. Positional args + block thread directly; keywords ride as one
        // trailing Hash (the G2 convention), which the runtime constructor
        // unpacks -- so `Point.new(x: 1, y: 2)` on a `Data.define` class binds.
        let mut arg_exprs: Vec<TokenStream> = args
            .iter()
            .map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            })
            .collect();
        if !kwargs.is_empty() {
            let inserts =
                crate::codegen::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
            arg_exprs.push(quote! {
                {
                    let __kw = zeo_rt::hash_new(vec![]);
                    #inserts
                    zeo_rt::RubyValue::Hash(__kw)
                }
            });
        }
        let block_expr = match block {
            Some(b) => {
                let p = super::procs::emit_proc_value(cx, b);
                quote! { Some(#p) }
            }
            None => quote! { None },
        };
        // A namespaced runtime class (`class NS::Item < Struct.new(:a)`) lives
        // as `Item` INSIDE `NS`, so the lookup needs that owner -- reading the
        // joined spelling under `Object` finds nothing.
        let path = crate::constpath::ConstPath::parse(class_name);
        let (owner_id, base_name) = match path.scope().and_then(|s| cx.resolve_class(s)) {
            Some(owner) => (owner.0, path.base().to_string()),
            None => (0, class_name.to_string()),
        };
        return quote! {
            {
                let __rtclass = zeo_rt::const_get(#owner_id, #base_name).ok_or_else(|| {
                    zeo_rt::raise_error("NameError", format!("uninitialized constant {}", #class_name))
                })?;
                zeo_rt::send_value(
                    &__rtclass,
                    zeo_rt::Symbol::intern("new"),
                    &[#(#arg_exprs),*],
                    #block_expr,
                )?
            }
        };
    };
    let ci = cx.compiler.class(cid);
    if cx.compiler.has_generated_struct(cid) {
        if let Some((_, sid)) = cx.compiler.method_in_chain(cid, "initialize") {
            let scope = cx.compiler.scope(sid);
            let ctor = emit_ctor_struct(cx, cid);
            // `initialize` takes `self: Arc<Self>` BY VALUE (see
            // `ruby_class!`'s docs), so it would move `__obj` -- clone the
            // handle (a refcount bump) to keep `__obj` returnable. UFCS, not
            // `.clone()` method syntax, which a user Ruby method named
            // `clone` (an inherent fn on the struct) would hijack.
            //
            // A literal block passed to `.new` is forwarded to `initialize`
            // (so `yield`/`block_given?` inside it see it); `needs_block`
            // keeps the callee's block slot lined up either way.
            let init = crate::codegen::params::emit_call_args_to(
                cx,
                &crate::codegen::params::Callee::Method(quote! { Clone::clone(&__obj) }),
                "initialize",
                &scope.params,
                args,
                kwargs,
                block,
                None,
                scope.needs_block_param() || block.is_some(),
                crate::codegen::scope_frame_guard(cx.compiler, scope, false),
            );
            return quote! { { let __obj = #ctor; #init; __obj } };
        }
    }
    // Boxed via `box_if_object_typed`: `initialize`'s own Rust parameters
    // are always plain `RubyValue` (see that function's docs) -- an
    // Object-typed constructor ARGUMENT (e.g. passing one class instance
    // into another's constructor) otherwise emits a bare, unboxed
    // `Arc<Concrete>`, a real `rustc` type mismatch confirmed by direct
    // reproduction.
    let mut arg_exprs: Vec<TokenStream> = args
        .iter()
        .map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        })
        .collect();
    // A builtin/module `.new` dispatches dynamically through `send_value_in`,
    // and an EXCEPTION-BACKED subclass's `.new` runs its `initialize` through
    // the runtime (`construct_by_class_id` -> the `emit_exc_trampoline`
    // trampoline) -- both carry keywords as one trailing Hash (the G2
    // convention), so `AError.new(msg, code: 9)`'s options reach the row.
    // (A struct-backed user class binds keywords through `emit_call_args_to`
    // above, not here.)
    if !kwargs.is_empty() && (ci.is_builtin || ci.is_module || cx.compiler.is_native_backed(cid)) {
        let inserts = crate::codegen::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
        arg_exprs.push(quote! {
            {
                let __kw = zeo_rt::hash_new(vec![]);
                #inserts
                zeo_rt::RubyValue::Hash(__kw)
            }
        });
    }
    emit_new_with_arg_tokens(cx, class_name, arg_exprs)
}
/// The bare `Arc<Concrete>` struct literal for one generated class -- every
/// ivar `Nil`, unfrozen. Shared by `emit_new`'s general-binder path and
/// `emit_new_with_arg_tokens`' hand-bound one, which must construct the
/// identical object.
fn emit_ctor_struct(cx: &Ctx, cid: crate::compiler::ClassId) -> TokenStream {
    let ci = cx.compiler.class(cid);
    let class_ident = crate::codegen::ident::class_ident(cx.compiler, cid);
    let fields = ci.ivars.iter().map(|iv| {
        let f = safe_ident(iv);
        quote! { #f: zeo_rt::parking_lot::Mutex::new(zeo_rt::RubyValue::Nil), }
    });
    // Every object starts unfrozen -- `.freeze`'s per-object flag (see
    // `ruby_class!`'s `__frozen` field docs).
    let fields = quote! {
        __frozen: std::sync::atomic::AtomicBool::new(false),
        __overflow: zeo_rt::parking_lot::Mutex::new(std::collections::HashMap::new()),
        #(#fields)*
    };
    // Wrapped in `Arc` immediately, not just at `new_handle` time: a local
    // holding this needs to be `Arc::clone()`-able on every re-read
    // (`codegen::expr`'s `LocalRead` -- see `ruby_class!`'s `new_handle` docs
    // for why the bare struct can't just derive `Clone` instead). `Arc<T>`
    // derefs transparently, so Path 1's `(recv_expr).method(...)` calls still
    // work unchanged against a `self: Arc<Self>`-shaped method. `Arc` (not
    // `Rc`, Part 9): every generated struct is genuinely `Send + Sync`.
    quote! { std::sync::Arc::new(#class_ident { #fields }) }
}
/// The actual construction logic behind `ClassName.new(...)`, factored out
/// to take already-built constructor-argument `TokenStream`s rather than
/// `NodeId`s -- reused by `raise`'s codegen (`codegen::expr`'s `Raise` arm),
/// which needs to construct an exception with a MESSAGE ARGUMENT that has
/// no corresponding HIR node at all (e.g. the class's own name, defaulted
/// as a compile-time string literal for a bare `raise SomeError`) --
/// codegen has no `&mut Hir` to synthesize one into.
pub fn emit_new_with_arg_tokens(
    cx: &Ctx,
    class_name: &str,
    arg_exprs: Vec<TokenStream>,
) -> TokenStream {
    let __bx = cx.box_id;
    let cid = cx
        .resolve_class(class_name)
        // An undefined `X.new` raises a runtime NameError via the const read
        // for `X` well before construction; reaching here with an unresolved
        // name is a compiler invariant violation, not a user error.
        .unwrap_or_else(|| {
            panic!("internal error: unknown class `{class_name}` in emit_new_with_arg_tokens")
        });
    // A BUILT-IN's `.new` -- no generated struct exists to construct, but
    // that doesn't make the call an error: `Time.new(...)` is ordinary Ruby,
    // answered by the runtime's own class-method table. Route it dynamically
    // (exactly as the module case just below does) and let the runtime
    // decide: a builtin with a `new` row constructs, and one without raises
    // real Ruby's NoMethodError at the moment the call runs.
    // An IMMEDIATE-builtin subclass (`class MyInt < Integer`) is
    // registry-only -- no constructor -- so its `.new` must dispatch
    // dynamically too, landing on `Class#new`'s NoMethodError arm (CRuby's
    // exact "undefined method 'new' for class MyInt").
    if cx.compiler.class(cid).is_builtin
        || cx.compiler.class(cid).is_module
        || cx.compiler.is_immediate_subclass(cid)
    {
        let id = cid.0;
        return quote! {
            zeo_rt::send_value_in(#__bx,
                &zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)),
                zeo_rt::Symbol::intern("new"),
                &[#(#arg_exprs),*],
                None,
            )?
        };
    }
    // A NATIVE-BACKED class -- an exception subclass (`RubyException`) or a
    // value-builtin subclass (`ValueSubclass`, `class Stack < Array`) -- has no
    // generated struct, so construct it through the runtime by id. Returns a
    // boxed `RubyValue` matching its `Poly` static type, running the registered
    // constructor (`exception_construct`/`value_subclass_construct`) which seeds
    // any payload and runs `initialize` just as the struct literal's inline
    // `.initialize(...)?` did.
    if cx.compiler.is_native_backed(cid) {
        let id = cid.0;
        return quote! {
            zeo_rt::construct_by_class_id(zeo_rt::ClassId(#id), &[#(#arg_exprs),*], None)?
        };
    }
    // `Object.new` -- a bare sentinel instance of the runtime root
    // (`zeo_rt::Object`), boxed: no generated struct exists (Object's
    // container holds top-level defs as free functions), and its static
    // type is `Poly` (see `types.rs`'s New/ClassObj exclusions), so the
    // whole expression is a plain `RubyValue`. Each call makes a fresh
    // `Arc` -- distinct identity, the sentinel idiom's whole point. A
    // user-defined `initialize` (top-level `def initialize` / `class
    // Object` reopen) is honored through its `__bm_Object` copy.
    if cid == crate::compiler::OBJECT_CLASS {
        let ctor = quote! {
            zeo_rt::RubyValue::Object(std::sync::Arc::new(zeo_rt::Object::default()))
        };
        return match cx.compiler.method_in_chain(cid, "initialize") {
            Some((_, sid)) => {
                let final_args = match bind_new_args(cx, sid, class_name, arg_exprs) {
                    Ok(a) => a,
                    Err(raise) => return raise,
                };
                let needs_block = cx.compiler.scope(sid).needs_block_param();
                let block_slot = needs_block.then(|| quote! { , None });
                let mod_ident = crate::codegen::ident::class_ident(cx.compiler, cid);
                quote! {
                    {
                        let __obj = #ctor;
                        #mod_ident::initialize(__obj.clone() #(, #final_args)* #block_slot)?;
                        __obj
                    }
                }
            }
            None if !arg_exprs.is_empty() => {
                // A raise, not a panic -- see the matching arm for user
                // classes below.
                let n = arg_exprs.len();
                let msg = format!("wrong number of arguments (given {n}, expected 0)");
                quote! {
                    {
                        #(let _ = #arg_exprs;)*
                        let __frame = zeo_rt::synthetic_c_frame("BasicObject#initialize");
                        return Err(zeo_rt::raise_error("ArgumentError", #msg.to_string()));
                    }
                }
            }
            None => ctor,
        };
    }
    let ctor = emit_ctor_struct(cx, cid);

    match cx.compiler.method_in_chain(cid, "initialize") {
        Some((_, sid)) => {
            let mut final_args = match bind_new_args(cx, sid, class_name, arg_exprs) {
                Ok(a) => a,
                Err(raise) => return raise,
            };
            // An `initialize` that uses `yield`/`&blk` still gets its block
            // slot (always `None` -- `.new` doesn't forward a block yet, a
            // narrower, pre-existing gap).
            if cx.compiler.scope(sid).needs_block_param() {
                final_args.push(quote! { None });
            }
            // `initialize` takes `self: Arc<Self>` BY VALUE now (see
            // `ruby_class!`'s docs), so calling it on `__obj` directly would
            // move it -- clone the `Arc` handle first (a cheap refcount bump,
            // not a deep copy) so `__obj` is still available to return. UFCS,
            // not `.clone()` method syntax, which a user Ruby method named
            // `clone` (an inherent fn on the struct) would hijack.
            quote! { { let __obj = #ctor; Clone::clone(&__obj).initialize(#(#final_args),*)?; __obj } }
        }
        // No user `initialize`, so the inherited `Object#initialize` takes
        // none -- passing any is an ArgumentError, not something to drop on
        // the floor. `Bag.new(1, 2)` on an `initialize`-less class silently
        // ignored its arguments and constructed happily.
        //
        // Raised at RUNTIME (after evaluating the arguments for their side
        // effects), not a compile panic: real Ruby resolves this arity at
        // runtime and the error is rescuable. Message shape oracle-verified
        // -- CRuby names no method in it.
        None if !arg_exprs.is_empty() => {
            let n = arg_exprs.len();
            let msg = format!("wrong number of arguments (given {n}, expected 0)");
            // The synthetic C frame: CRuby's innermost row here is
            // `'BasicObject#initialize'` at the CALLER's line.
            quote! {
                {
                    #(let _ = #arg_exprs;)*
                    let __frame = zeo_rt::synthetic_c_frame("BasicObject#initialize");
                    return Err(zeo_rt::raise_error("ArgumentError", #msg.to_string()));
                }
            }
        }
        None => ctor,
    }
}
/// Binds `initialize`'s REQUIRED + OPTIONAL parameters the way a Path 1
/// call does: required args 1:1, each optional slot
/// `Some(expr)` when provided else `None` (the callee's own prologue lazily
/// evaluates the default). Splat/post/keyword params on `initialize` remain
/// out of scope, matching `emit_new_with_arg_tokens`'s original posture.
///
/// A wrong argument COUNT is `Err(tokens)`: real Ruby raises a rescuable
/// runtime `ArgumentError` inside `'Class#initialize'` at its def line
/// (oracle-verified), so the caller returns those raise tokens instead of
/// the construction -- never a compile abort for reachable-or-not code.
fn bind_new_args(
    cx: &Ctx,
    sid: crate::compiler::ScopeId,
    class_name: &str,
    arg_exprs: Vec<TokenStream>,
) -> Result<Vec<TokenStream>, TokenStream> {
    let scope = cx.compiler.scope(sid);
    let params = &scope.params;
    let nreq = params.required.len();
    let nopt = params.optional.len();
    if params.rest.is_some()
        || !params.post.is_empty()
        || !params.keywords.is_empty()
        || params.keyword_rest.is_some()
    {
        return Err(crate::codegen::unsupported(format!(
            "`{class_name}.new`: an `initialize` with splat/post/keyword parameters isn't supported yet (zeo limitation)"
        )));
    }
    if arg_exprs.len() < nreq || arg_exprs.len() > nreq + nopt {
        let msg = format!(
            "wrong number of arguments (given {}, expected {})",
            arg_exprs.len(),
            if nopt == 0 {
                nreq.to_string()
            } else {
                format!("{nreq}..{}", nreq + nopt)
            }
        );
        let frame = crate::codegen::scope_frame_guard(cx.compiler, scope, false);
        return Err(quote! {
            {
                #(let _ = #arg_exprs;)*
                #frame
                return Err(zeo_rt::raise_error("ArgumentError", #msg.to_string()));
            }
        });
    }
    let mut final_args: Vec<TokenStream> = Vec::with_capacity(nreq + nopt);
    let mut provided = arg_exprs.into_iter();
    for _ in 0..nreq {
        let e = provided.next().expect("bounds checked above");
        final_args.push(e);
    }
    for _ in 0..nopt {
        final_args.push(match provided.next() {
            Some(e) => quote! { Some(#e) },
            None => quote! { None },
        });
    }
    Ok(final_args)
}
