//! `super` call emission -- named `super_calls`, not `super`, since `super`
//! is a keyword. Covers both the compile-time-resolvable inline splice
//! (`emit_super_inline`, the common case: the parent method's body is
//! spliced directly at the call site) and the runtime fallback
//! (`emit_runtime_super`/`emit_super_dynamic`/`emit_value_super`, for a
//! runtime-defined method with no compile-time ancestor to splice), plus
//! every argument-binding helper (implicit forwarding vs. explicit
//! re-evaluation, positional vs. keyword) each of those needs.

use quote::{format_ident, quote};

use crate::codegen::Ctx;
use crate::codegen::expr::emit_expr;
use crate::codegen::ident::safe_ident;
use crate::hir::{HirNode, KeywordParam, KwArg, NodeId, Params};
use proc_macro2::TokenStream;

/// `super` always resolves against the receiver's REAL, full linearized
/// `ancestors` -- never through the dynamic dispatch table (mirrors
/// `emit_super`, `codegen.c:3262`) -- searching FORWARD (toward the root)
/// from wherever the CURRENTLY-executing method was actually defined
/// (`cx.defining_class`), not from `cx.current_class` (the receiver's own
/// concrete type, which only coincides with `defining_class` for an
/// ordinary own-body method). This is what makes a `super` chain that
/// crosses a `prepend`/`include` boundary -- module -> module -> parent
/// class, or any mix -- resolve correctly: in pure single inheritance,
/// "the defining class's own parent" and "the receiver's ancestors past
/// this point" are the same thing, but once a module can sit between two
/// classes in the ancestor list, they're not, so this must consult
/// `current_class`'s full `ancestors`, not `defining_class`'s own (a
/// module has no `.parent` at all).
///
/// The spike implements this via statement inlining rather than a
/// cross-type function call: subclasses in this object model are distinct
/// Rust structs with no shared layout (unlike zeo's C "common initial
/// sequence" trick), so `Animal::speak(self: &Dog)` wouldn't type-check.
/// Splicing the resolved method's body directly into the call site
/// sidesteps that entirely -- and is a real zeo mechanism too, just used
/// there as an optimization (`emit_super_inline`, reached when the parent
/// yields) rather than the default.
pub fn emit_super_inline(
    cx: &Ctx,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    // A `super` inside a RUNTIME-defined method body (a `def`/`define_method`
    // installed in a `Class.new`/`Struct.new`/`Data.define` block) has no
    // compile-time defining class -- the class is minted at runtime. `emit_expr`
    // marks such a body with `runtime_super_params`; resolve through the runtime
    // method-frame stack (`zeo_rt::send_super_dynamic`) instead of splicing a
    // compile-time ancestor's HIR.
    if let Some(params) = cx.runtime_super_params.clone() {
        return emit_super_dynamic(cx, &params, args, kwargs, zsuper, block);
    }

    // A CLASS method (`def self.foo`) has no `self: Arc<Self>` receiver, so
    // `current_class` is deliberately None there and `class_self` carries the
    // receiver class instead (see `codegen::mod`'s Ctx construction).
    //
    // Real Ruby needs no special case at all: `def self.foo` is an ordinary
    // instance method on the SINGLETON class, and singleton classes form a
    // parallel chain -- `#<Class:C>.super == #<Class:C.superclass>`
    // (`make_metaclass`, class.c:1186) -- so one walk serves both. This
    // compiler keeps class methods in their own flattened table rather than
    // modeling singleton classes, so the walk is MIRRORED here instead of
    // shared: same ancestor chain, `own_class_methods` instead of
    // `own_methods`. (Documented divergence: the flattening copies bodies
    // down, so it cannot see a class method added at runtime.)
    let in_class_method = cx.current_class.is_none();
    let receiver_class = cx
        .current_class
        .or(cx.class_self)
        .expect("`super` outside a method");
    let defining_class = cx.defining_class.expect("`super` outside a method");
    let mname = cx
        .current_method
        .as_deref()
        .expect("`super` outside a method");
    // Which pool a `super` search consults, per the note above.
    let own_pool = |compiler: &crate::compiler::Compiler, anc: crate::compiler::ClassId| {
        let info = compiler.class(anc);
        if in_class_method {
            info.own_class_methods.clone()
        } else {
            info.own_methods.clone()
        }
    };

    let ancestors = &cx.compiler.class(receiver_class).ancestors;
    let pos = ancestors
        .iter()
        .position(|&a| a == defining_class)
        .unwrap_or_else(|| {
            panic!(
                "internal error: {} not found in {}'s own ancestors",
                cx.compiler.class(defining_class).name,
                cx.compiler.class(receiver_class).name
            )
        });
    let found = ancestors[pos + 1..].iter().find_map(|&anc| {
        own_pool(cx.compiler, anc)
            .iter()
            .find(|&&s| cx.compiler.scope(s).name == mname)
            .map(|&sid| (anc, sid))
    });

    // The method CURRENTLY executing (whose lexical body this `super` call
    // sits inside) -- needed for bare `super`'s forwarding case (in the splice
    // AND the runtime-dispatch branches below). Guaranteed to exist: this
    // `super` is inside `mname`'s own body on `defining_class`.
    let current_sid = own_pool(cx.compiler, defining_class)
        .iter()
        .find(|&&s| cx.compiler.scope(s).name == mname)
        .copied()
        .unwrap_or_else(|| {
            panic!("internal error: `{mname}` not found in its own defining class's own methods")
        });
    let current_params = cx.compiler.scope(current_sid).params.clone();

    // `super` into an inherited VALUE builtin (D3): a `class Stack < Array`
    // method whose `super` finds NO user definition above targets the native
    // `Array` method -- `super` in `initialize` re-seats the payload, any other
    // runs the builtin against it. There is no HIR to splice (the builtin has no
    // `own_methods`), so dispatch through the runtime. Only reached when no user
    // ancestor overrides `mname`; a user parent still splices.
    if found.is_none() && cx.compiler.is_value_subclass(receiver_class) {
        return emit_value_super(cx, mname, &current_params, args, kwargs, zsuper, block);
    }

    // No user definition above the defining class. Real Ruby has NO
    // definition-time check here at all -- `super` resolves at CALL time
    // against the receiver's live ancestry (`vm_search_super_method`,
    // vm_insnhelper.c:5041) and raises `NoMethodError: super: no superclass
    // method 'x' for ...` only if that walk comes up empty (vm_eval.c:993).
    // Rejecting at compile time is wrong twice over: it kills programs that
    // merely *mention* such a `super` in a rescued or dead branch (codegen
    // lowers every branch eagerly), and it cannot see methods a compile-time
    // scan misses -- an included module's, or one registered at runtime. Hand
    // off to the runtime walk, which finds those and raises correctly if not.
    let Some((new_defining_class, sid)) = found else {
        return emit_runtime_super(cx, mname, &current_params, args, kwargs, zsuper, block);
    };

    // `super` into a NATIVE exception method (D3): the resolved parent is a
    // pristine `BUILTIN_EXCEPTIONS_RB` body (`native_default`) whose real
    // behavior lives in a `zeo-rt` fn, not the retained HIR -- splicing that
    // HIR would set a visible `@message` ivar and miss the hidden message slot.
    // Dispatch through the runtime instead, resuming the MRO walk after
    // `defining_class` (`native_default` is only ever set on bootstrap-exception
    // scopes, so this can't fire for an ordinary struct class). A user
    // exception parent's own method (`!native_default`) still splices below --
    // its HIR body runs correctly against the dynamic-self receiver.
    if cx.compiler.scope(sid).native_default {
        return emit_runtime_super(cx, mname, &current_params, args, kwargs, zsuper, block);
    }

    let defining_scope = cx.compiler.scope(sid);
    let body = defining_scope.body.clone();
    // `loop_labels`/`label_counter` carry over from `cx` unchanged: the
    // parent method's body is spliced in at this call site, so a `break`
    // inside it must still target whatever loop lexically encloses the
    // `super` call, exactly as if that code were written there directly (see
    // `Ctx::loop_labels`'s docs).
    let defining_captures = crate::codegen::captures::collect_escaping_captures(
        cx.compiler,
        &defining_scope.body,
        &defining_scope.params,
        Some(receiver_class),
    );
    let inline_cx = Ctx {
        compiler: cx.compiler,
        // The spliced parent body resolves names against ITS OWN defining
        // box (CRuby's def->box stamp), not the caller's.
        box_id: cx.compiler.class(new_defining_class).box_id,
        // UNCHANGED across the splice -- `self` is still the SAME receiver
        // throughout a chain of nested `super` calls. Propagated from `cx`
        // rather than set to `Some(receiver_class)`: for an INSTANCE method
        // the two are identical, but a CLASS method's `current_class` is
        // deliberately `None` (its receiver is the class object, carried in
        // `class_self`), and that `None` is exactly what tells a nested
        // `super` to search class methods rather than instance methods.
        // Forcing a value here made `Baz.base -> Bar.base -> Foo.base` look
        // for `base` among Bar's INSTANCE methods and fail.
        current_class: cx.current_class,
        defining_class: Some(new_defining_class),
        // Inherited for the same reason `current_class` is: a `super` splice
        // does not change WHAT `self` is, only which body is running. A
        // `super` inside a class method still has the class object as self.
        class_self: cx.class_self,
        current_method: Some(mname.to_string()),
        // Carried over with `self_ident` below: the splice keeps referring to
        // whichever `self` the CALLING method's body already uses, so how
        // that self is typed carries over with it.
        self_is_dynamic: cx.self_is_dynamic,
        local_types: std::borrow::Cow::Borrowed(&defining_scope.local_types),
        label_counter: cx.label_counter,
        loop_labels: cx.loop_labels.clone(),
        // NOT inherited -- see `Ctx::for_var_override`'s docs.
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&defining_captures.locals),
        // Both ARE inherited (unlike `for_var_override`): the inlined body
        // must keep referring to whichever `self` the CALLING method's own
        // body is already using, and `break`/`next`/`redo`/`return` inside
        // it must still behave per whatever Rust-function boundary actually
        // encloses this splice (a real Proc closure or not).
        self_ident: cx.self_ident.clone(),
        in_real_proc: cx.in_real_proc,
        // A compile-time `super` splice (this path only runs when
        // `defining_class` was known); a nested `super` in the spliced body
        // resolves through `defining_class` above, not the runtime frame.
        runtime_super_params: None,
    };
    // Binds the parent's OWN parameter names fresh, before splicing its body
    // in -- previously this relied on the parent's params
    // happening to share names with the calling method's own, since the
    // spliced body just referenced its param names directly with nothing
    // ever binding them. See `emit_super_arg_bindings`'s docs.
    let bindings = emit_super_arg_bindings(
        cx,
        &inline_cx,
        &defining_scope.params,
        &current_params,
        args,
        kwargs,
        zsuper,
    );
    // A literal block at the `super` site becomes the spliced body's
    // `__blk` (its `yield` runs this block) -- built as a real Proc in the
    // CALLING scope (`cx`: captures resolve against the child method's own
    // locals), shadowing the child's `__blk` only inside the splice braces.
    // With no literal block, real Ruby forwards the current method's block
    // -- which the splice sees for free, `__blk` already being in scope.
    let blk_binding = block.map(|b| {
        let proc_value = super::procs::emit_proc_value(cx, b);
        quote! { let __blk: Option<zeo_rt::RubyValue> = Some(#proc_value); }
    });
    // A fresh hoisting prelude of its own: the parent method's local
    // variables are a genuinely separate Ruby scope from the calling
    // (sub)method's, even though inlining splices their statements into the
    // same Rust expression position (see `hoisting`'s docs).
    // The parent's own params count as ALREADY BOUND for the hoisting
    // prelude (bound just above by `bindings`), so a parent body that
    // reassigns one rebinds the forwarded value instead of nil-shadowing
    // it -- same rule as an ordinary method's own prelude.
    let inlined = crate::codegen::hoisting::emit_hoisted_body_with_extra_roots(
        &inline_cx,
        &body,
        &defining_scope.params.default_ids(),
        &defining_scope.params.bound_names(),
        false,
    );
    quote! { { #bindings #blk_binding #inlined } }
}
/// `super` dispatched through `zeo_rt::send_super_from` rather than spliced,
/// resuming the receiver's REAL ancestor walk after `defining_class`.
///
/// Two callers, one mechanism:
///  - a `super` into a NATIVE default parent (`native_default`), where there is
///    no HIR worth splicing (see `emit_super_inline`'s native-branch comment);
///  - a `super` the compile-time scan could not resolve at all, which real Ruby
///    resolves at CALL time against the receiver's live ancestry.
///
/// Builds the forwarded argument slice -- explicit `super(a, b)` args, or, for
/// bare `super`, the current method's own positional parameters (required /
/// optional / splatted `*rest` / post) -- with keyword arguments appended as
/// one trailing Hash (the runtime's G2 convention).
fn emit_runtime_super(
    cx: &Ctx,
    mname: &str,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    // `send_super_from` takes a boxed `RubyValue`. An exception-backed self is
    // already one, but a plain generated-struct self is an `Arc<Concrete>` --
    // so box through the same helper an implicit-self call uses rather than
    // passing `self_ident` raw.
    let self_val = super::boxed_implicit_self(cx).unwrap_or_else(|| {
        let slf = &cx.self_ident;
        quote! { (#slf).clone() }
    });
    let def_id = cx.defining_class.expect("`super` outside a method").0;
    let (pushes, block_expr) =
        emit_runtime_super_args(cx, current_params, args, kwargs, zsuper, block);
    quote! {
        {
            let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#pushes)*
            zeo_rt::send_super_from(
                &#self_val,
                zeo_rt::ClassId(#def_id),
                zeo_rt::Symbol::intern(#mname),
                &__super_args,
                #block_expr,
            )?
        }
    }
}
/// `super` from inside a RUNTIME-defined method body (a `def`/`define_method`
/// in a `Class.new`/`Struct.new`/`Data.define` block), whose defining class is
/// unknown at compile time. Forwards args exactly like `emit_runtime_super` --
/// explicit `super(a, b)`, or the enclosing method's own params for a bare
/// `super` -- but dispatches through `zeo_rt::send_super_dynamic`, which
/// reads the class off the runtime method-frame stack (pushed when the method
/// was entered) rather than from a compile-time `defining_class`.
fn emit_super_dynamic(
    cx: &Ctx,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    let self_val = super::boxed_implicit_self(cx).unwrap_or_else(|| {
        let slf = &cx.self_ident;
        quote! { (#slf).clone() }
    });
    let (pushes, block_expr) =
        emit_runtime_super_args(cx, current_params, args, kwargs, zsuper, block);
    quote! {
        {
            let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#pushes)*
            zeo_rt::send_super_dynamic(&#self_val, &__super_args, #block_expr)?
        }
    }
}
/// `super` from a value-builtin subclass method into the inherited builtin
/// (D3): `zeo_rt::value_super` re-seats the payload for `initialize`, else
/// runs the root builtin method (`Array#push` ...) against the payload and
/// re-wraps a self-return. No HIR to splice (the builtin has no `own_methods`).
/// Argument forwarding is shared with the exception path.
fn emit_value_super(
    cx: &Ctx,
    mname: &str,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    let self_ident = &cx.self_ident;
    let (pushes, block_expr) =
        emit_runtime_super_args(cx, current_params, args, kwargs, zsuper, block);
    quote! {
        {
            let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#pushes)*
            zeo_rt::value_super(
                &#self_ident,
                #mname,
                &__super_args,
                #block_expr,
            )?
        }
    }
}
/// Build the forwarded argument pushes + block expression for a RUNTIME-
/// dispatched `super` (`send_super_from`/`value_super`) -- explicit
/// `super(a, b)` args, or, for bare `super`, the current method's own positional
/// parameters (required / optional / splatted `*rest` / post). Keyword arguments
/// append as one trailing Hash (the G2 convention). A literal block forwards; a
/// bare `super` without one passes `None` (the native builtins take no block).
fn emit_runtime_super_args(
    cx: &Ctx,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> (Vec<TokenStream>, TokenStream) {
    let mut pushes: Vec<TokenStream> = Vec::new();
    if zsuper {
        for name in &current_params.required {
            let id = safe_ident(name);
            pushes.push(quote! { __super_args.push(#id.clone()); });
        }
        for (name, _) in &current_params.optional {
            let id = safe_ident(name);
            pushes.push(quote! { __super_args.push(#id.clone()); });
        }
        if let Some(Some(name)) = &current_params.rest {
            let id = safe_ident(name);
            // The `*rest` local is a `RubyValue::Array` post-prologue -- splat
            // its elements (the same idiom `ArrayElem::Splat` uses at call sites).
            pushes.push(quote! {
                __super_args.extend((#id).as_array_unchecked().lock().iter().cloned());
            });
        }
        for name in &current_params.post {
            let id = safe_ident(name);
            pushes.push(quote! { __super_args.push(#id.clone()); });
        }
    } else {
        for &a in args {
            let e = emit_expr(cx, a);
            let e = crate::codegen::expr::box_if_object_typed(cx, a, e);
            pushes.push(quote! { __super_args.push(#e); });
        }
        if !kwargs.is_empty() {
            let inserts =
                crate::codegen::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
            pushes.push(quote! {
                {
                    let __kw = zeo_rt::hash_new(vec![]);
                    #inserts
                    __super_args.push(zeo_rt::RubyValue::Hash(__kw));
                }
            });
        }
    }
    let block_expr = match block {
        Some(b) => {
            let proc_value = super::procs::emit_proc_value(cx, b);
            quote! { Some(#proc_value) }
        }
        None => quote! { None },
    };
    (pushes, block_expr)
}
/// Binds the parent method's (`parent_params`) own parameter names, right
/// before its body is spliced in -- either from EXPLICIT `super(expr, ...)`
/// arguments (evaluated in the CALLING scope, `cx`), or, for bare `super`
/// (`zsuper`, forwarding), from the CURRENTLY-EXECUTING method's
/// (`current_params`) own already-bound parameter of the same position
/// within each bucket (required/optional/rest/post; keywords matched by
/// NAME instead, since position isn't meaningful there). A literal
/// `super()` (`zsuper: false`, `args` empty) takes the explicit path with
/// zero arguments -- real Ruby's "no arguments at all", so the parent's
/// optionals evaluate their own defaults instead of forwarding (the two
/// shapes mean opposite things; see `HirNode::SuperCall`'s docs).
fn emit_super_arg_bindings(
    cx: &Ctx,
    inline_cx: &Ctx,
    parent_params: &Params,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
) -> TokenStream {
    if zsuper {
        return emit_super_forwarding_bindings(inline_cx, parent_params, current_params);
    }
    emit_super_explicit_bindings(cx, inline_cx, parent_params, args, kwargs)
}
/// Bare `super`: forwards the CURRENT method's own already-bound positional
/// parameter VALUES, flattened in declaration order (required, optional,
/// post), to the parent's own positional slots -- the same bucket
/// arithmetic as an explicit `super(...)`, just with already-bound idents
/// instead of freshly evaluated argument expressions. This is what makes a
/// child `def initialize(m = "def"); super; end` feed its optional `m` into
/// a parent whose same-position parameter is REQUIRED (bucket-by-bucket
/// matching left the parent's name unbound there, and the spliced body then
/// referenced a name nothing had ever bound). A parent optional beyond what
/// the child supplies evaluates its own default (in the PARENT's scope,
/// `inline_cx`). Rest params fall back to whole-array aliasing when both
/// sides have one (dynamic length -- the static flattening can't cover it);
/// keyword params are matched by NAME, since "position" isn't meaningful
/// there.
fn emit_super_forwarding_bindings(
    inline_cx: &Ctx,
    parent_params: &Params,
    current_params: &Params,
) -> TokenStream {
    let mut lets = Vec::new();
    if current_params.rest.is_none() && parent_params.rest.is_none() {
        // Fully static shapes: flatten and bind positionally. Source values
        // are snapshotted into temporaries FIRST -- a parent param may share
        // a child param's name at a different position (`def m(a, b)` over
        // `def m(b, a)`), and sequential `let a = b; let b = a;` would read
        // the first rebinding.
        let src: Vec<syn::Ident> = current_params
            .required
            .iter()
            .chain(current_params.optional.iter().map(|(n, _)| n))
            .chain(current_params.post.iter())
            .map(|n| safe_ident(n))
            .collect();
        let temps: Vec<syn::Ident> = (0..src.len())
            .map(|i| format_ident!("__super_fwd{i}"))
            .collect();
        for (t, s) in temps.iter().zip(&src) {
            lets.push(quote! { let #t = #s.clone(); });
        }
        let nreq = parent_params.required.len();
        if src.len() < nreq + parent_params.post.len() {
            panic!(
                "bare `super`: child forwards {} positional(s) but the parent requires {} (spike scope)",
                src.len(),
                nreq + parent_params.post.len()
            );
        }
        let opt_bound =
            (src.len() - nreq - parent_params.post.len()).min(parent_params.optional.len());
        for (i, name) in parent_params.required.iter().enumerate() {
            let dst = safe_ident(name);
            let t = &temps[i];
            lets.push(quote! { let #dst: zeo_rt::RubyValue = #t; });
        }
        for (i, (name, default)) in parent_params.optional.iter().enumerate() {
            let dst = safe_ident(name);
            if i < opt_bound {
                let t = &temps[nreq + i];
                lets.push(quote! { let #dst: zeo_rt::RubyValue = #t; });
            } else {
                let default_expr = crate::codegen::expr::emit_expr(inline_cx, *default);
                lets.push(quote! { let #dst: zeo_rt::RubyValue = #default_expr; });
            }
        }
        for (i, name) in parent_params.post.iter().enumerate() {
            let dst = safe_ident(name);
            let t = &temps[nreq + opt_bound + i];
            lets.push(quote! { let #dst: zeo_rt::RubyValue = #t; });
        }
        let kw_lets = emit_super_forwarding_keyword_bindings(parent_params, current_params);
        return quote! { #(#lets)* #kw_lets };
    }
    // A rest param on either side makes the positional split dynamic --
    // keep the older same-bucket-same-position aliasing for those shapes.
    for (i, name) in parent_params.required.iter().enumerate() {
        if let Some(src) = current_params.required.get(i) {
            let dst = safe_ident(name);
            let src_ident = safe_ident(src);
            lets.push(quote! { let #dst: zeo_rt::RubyValue = #src_ident.clone(); });
        }
    }
    for (i, (name, _)) in parent_params.optional.iter().enumerate() {
        if let Some((src, _)) = current_params.optional.get(i) {
            let dst = safe_ident(name);
            let src_ident = safe_ident(src);
            lets.push(quote! { let #dst: zeo_rt::RubyValue = #src_ident.clone(); });
        }
    }
    if let Some(Some(dst_name)) = &parent_params.rest {
        if let Some(Some(src_name)) = &current_params.rest {
            let dst = safe_ident(dst_name);
            let src_ident = safe_ident(src_name);
            lets.push(quote! { let #dst: zeo_rt::RubyValue = #src_ident.clone(); });
        }
    }
    for (i, name) in parent_params.post.iter().enumerate() {
        if let Some(src) = current_params.post.get(i) {
            let dst = safe_ident(name);
            let src_ident = safe_ident(src);
            lets.push(quote! { let #dst: zeo_rt::RubyValue = #src_ident.clone(); });
        }
    }
    let kw_lets = emit_super_forwarding_keyword_bindings(parent_params, current_params);
    quote! { #(#lets)* #kw_lets }
}
fn emit_super_forwarding_keyword_bindings(
    parent_params: &Params,
    current_params: &Params,
) -> TokenStream {
    let mut lets = Vec::new();
    for kw in &parent_params.keywords {
        let dst_name = match kw {
            KeywordParam::Required(n) | KeywordParam::Optional(n, _) => n,
        };
        let has_match = current_params.keywords.iter().any(|k| match k {
            KeywordParam::Required(n) | KeywordParam::Optional(n, _) => n == dst_name,
        });
        if has_match {
            // Same name already bound in the current method's own scope --
            // re-binding it to itself is a harmless no-op, kept only for
            // uniformity with the positional buckets.
            let dst = safe_ident(dst_name);
            lets.push(quote! { let #dst: zeo_rt::RubyValue = #dst.clone(); });
        }
    }
    quote! { #(#lets)* }
}
/// Explicit `super(expr, ...)`: evaluates each argument expression in the
/// CALLING scope (`cx`), then binds the parent's own required/optional/rest/
/// post parameter names positionally -- the same bucket arithmetic
/// `codegen::params::emit_call_args` uses for an ordinary Path 1 call, minus
/// building an actual method call (the parent's body is spliced in instead).
/// A skipped optional's default expression is evaluated via `inline_cx` (the
/// PARENT's own scope), since a later default can reference an earlier
/// parent parameter by name -- matching `codegen::params::emit_prologue`'s
/// same lazy-evaluation contract. No keyword-argument channel exists for
/// this shape (`HirNode::SuperCall` carries positional `args` only -- see
/// its docs), matching every other Path-2-only/positional-only limitation
/// already documented elsewhere in this codebase.
fn emit_super_explicit_bindings(
    cx: &Ctx,
    inline_cx: &Ctx,
    parent_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
) -> TokenStream {
    let nreq = parent_params.required.len();
    let nopt = parent_params.optional.len();
    let npost = parent_params.post.len();
    let has_rest = parent_params.rest.is_some();
    let min_positional = nreq + npost;

    if args.len() < min_positional {
        panic!(
            "too few arguments for `super` (spike scope): expected at least {min_positional}, got {}",
            args.len()
        );
    }
    let extra = args.len() - min_positional;
    if !has_rest && extra > nopt {
        panic!(
            "too many arguments for `super` (spike scope): expected at most {}, got {}",
            nreq + nopt + npost,
            args.len()
        );
    }
    let opt_bound = extra.min(nopt);
    let rest_count = extra - opt_bound;

    let pos_temps: Vec<syn::Ident> = (0..args.len())
        .map(|i| format_ident!("__super_a{i}"))
        .collect();
    let pos_lets = args.iter().zip(&pos_temps).map(|(&a, t)| {
        let e = emit_expr(cx, a);
        quote! { let #t = #e; }
    });

    let required_lets = parent_params.required.iter().enumerate().map(|(i, name)| {
        let dst = safe_ident(name);
        let src = &pos_temps[i];
        quote! { let #dst: zeo_rt::RubyValue = #src.clone(); }
    });
    let optional_lets = parent_params
        .optional
        .iter()
        .enumerate()
        .map(|(i, (name, default))| {
            let dst = safe_ident(name);
            if i < opt_bound {
                let src = &pos_temps[nreq + i];
                quote! { let #dst: zeo_rt::RubyValue = #src.clone(); }
            } else {
                let default_expr = emit_expr(inline_cx, *default);
                quote! { let #dst: zeo_rt::RubyValue = #default_expr; }
            }
        });
    let rest_let = parent_params.rest.as_ref().and_then(|r| r.as_ref()).map(|name| {
        let dst = safe_ident(name);
        let elems = pos_temps[nreq + opt_bound..nreq + opt_bound + rest_count]
            .iter()
            .map(|t| quote! { #t.clone() });
        quote! { let #dst: zeo_rt::RubyValue = zeo_rt::RubyValue::Array(zeo_rt::array_new(vec![#(#elems),*])); }
    });
    let post_lets = parent_params.post.iter().enumerate().map(|(i, name)| {
        let dst = safe_ident(name);
        let src = &pos_temps[nreq + opt_bound + rest_count + i];
        quote! { let #dst: zeo_rt::RubyValue = #src.clone(); }
    });

    let keyword_lets = emit_super_explicit_keyword_bindings(cx, inline_cx, parent_params, kwargs);

    quote! {
        #(#pos_lets)*
        #(#required_lets)*
        #(#optional_lets)*
        #rest_let
        #(#post_lets)*
        #keyword_lets
    }
}
/// Binds the parent's keyword params from an explicit `super(x: .., y: ..)`,
/// matched by name. A required parent keyword the super omits raises
/// CRuby's `missing keyword(s)` `ArgumentError`; an omitted optional evaluates
/// its own default in the PARENT scope (`inline_cx`). `super(**h)` isn't
/// modelled yet.
fn emit_super_explicit_keyword_bindings(
    cx: &Ctx,
    inline_cx: &Ctx,
    parent_params: &Params,
    kwargs: &[KwArg],
) -> TokenStream {
    if parent_params.keywords.is_empty() && parent_params.keyword_rest.is_none() {
        return quote! {};
    }
    let mut provided: std::collections::HashMap<String, NodeId> = std::collections::HashMap::new();
    for kw in kwargs {
        match kw {
            KwArg::Pair(k, v) => match &cx.compiler.hir[*k] {
                HirNode::SymbolLit(name) => {
                    provided.insert(name.clone(), *v);
                }
                _ => {
                    panic!("`super`: keyword argument names must be literal symbols (spike scope)")
                }
            },
            KwArg::DoubleSplat(_) => {
                panic!("`super(**h)` (double-splat into super) isn't supported yet (spike scope)")
            }
        }
    }

    // A named `**kwrest` parent param collects every super keyword that isn't
    // bound to a declared keyword param -- the shape the `Data`/`Struct` base
    // `initialize(*args, **kwargs)` relies on for `super(x: .., y: ..)`.
    let kwrest_let = match &parent_params.keyword_rest {
        Some(Some(rest_name)) => {
            let named: std::collections::HashSet<&str> = parent_params
                .keywords
                .iter()
                .map(|kw| match kw {
                    KeywordParam::Required(n) | KeywordParam::Optional(n, _) => n.as_str(),
                })
                .collect();
            let pairs: Vec<TokenStream> = kwargs
                .iter()
                .filter_map(|kw| match kw {
                    KwArg::Pair(k, v) => match &cx.compiler.hir[*k] {
                        HirNode::SymbolLit(name) if !named.contains(name.as_str()) => {
                            let val = crate::codegen::expr::box_if_object_typed(
                                cx,
                                *v,
                                emit_expr(cx, *v),
                            );
                            Some(quote! {
                                (zeo_rt::RubyValue::Symbol(zeo_rt::Symbol::intern(#name)), #val)
                            })
                        }
                        _ => None,
                    },
                    KwArg::DoubleSplat(_) => None,
                })
                .collect();
            let dst = safe_ident(rest_name);
            quote! {
                let #dst: zeo_rt::RubyValue =
                    zeo_rt::RubyValue::Hash(zeo_rt::hash_new(vec![#(#pairs),*]));
            }
        }
        _ => quote! {},
    };

    let missing: Vec<String> = parent_params
        .keywords
        .iter()
        .filter_map(|kw| match kw {
            KeywordParam::Required(name) if !provided.contains_key(name) => Some(name.clone()),
            _ => None,
        })
        .collect();
    if !missing.is_empty() {
        let msg = if missing.len() == 1 {
            format!("missing keyword: :{}", missing[0])
        } else {
            let names = missing
                .iter()
                .map(|n| format!(":{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("missing keywords: {names}")
        };
        return quote! { return Err(zeo_rt::raise_error("ArgumentError", #msg.to_string())); };
    }

    let lets = parent_params.keywords.iter().map(|kw| {
        let (name, default) = match kw {
            KeywordParam::Required(name) => (name, None),
            KeywordParam::Optional(name, d) => (name, Some(d)),
        };
        let dst = safe_ident(name);
        match provided.get(name) {
            Some(&v) => {
                let e = crate::codegen::expr::box_if_object_typed(cx, v, emit_expr(cx, v));
                quote! { let #dst: zeo_rt::RubyValue = #e; }
            }
            None => {
                let default_expr =
                    emit_expr(inline_cx, *default.expect("required-missing handled above"));
                quote! { let #dst: zeo_rt::RubyValue = #default_expr; }
            }
        }
    });
    let lets: Vec<TokenStream> = lets.collect();
    quote! { #(#lets)* #kwrest_let }
}
