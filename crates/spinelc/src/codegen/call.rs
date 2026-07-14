//! `emit_call`'s dispatch decision -- see "Two dispatch paths" in the plan:
//! Path 1 (static direct call, or a call rewritten away entirely, e.g.
//! `super` inlining) whenever `Compiler::method_in_chain` resolves the
//! target from the receiver's statically-known class; Path 2
//! (`spinel_rt::send`) only when it can't. Every fragment produced here
//! evaluates to a bare `spinel_rt::RubyValue` -- any call into a
//! `Result`-returning method has `?` applied right here, at the call site,
//! not left to the caller (see `expr.rs`'s module docs).

use quote::{format_ident, quote};

use super::expr::{box_if_object_typed, emit_expr, emit_symbol_expr, infer, infer_class};
use super::ident::safe_ident;
use super::Ctx;
use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HashPair, HirNode, KeywordParam, NodeId, Params, Visibility};
use crate::types::TyKind;
use proc_macro2::TokenStream;

/// Whether a call shape is the `.times` fast path (inline splice, no real
/// `Proc` ever allocated) -- the exact condition `dispatch` already checks
/// for below, factored out so `codegen::captures`' escaping-block scan can
/// ask the identical question: any block NOT matching this shape becomes a
/// real, heap-allocated `Proc` (see that module's docs -- there is no
/// separate "escape analysis" beyond this one check, since `.times` is the
/// only inline fast path that exists).
pub fn is_times_fast_path(compiler: &Compiler, receiver: Option<NodeId>, name: &str, kwargs_empty: bool) -> bool {
    kwargs_empty
        && name == "times"
        && receiver.is_some_and(|r| matches!(compiler.hir[r], HirNode::IntegerLit(_)))
}

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

/// Same shape as `INT_BINARY_OPS`, minus the bitwise/shift operators (real
/// Ruby's `Float` has none of those) -- `<=>` is deliberately NOT here (see
/// its own dedicated check in `dispatch`): a `Float` comparison against
/// `NaN` returns `nil`, not an `Int`, which this table's uniform
/// "op -> one wrapper variant" shape can't express.
const FLOAT_BINARY_OPS: &[(&str, &str, &str)] = &[
    ("+", "float_add", "Float"),
    ("-", "float_sub", "Float"),
    ("*", "float_mul", "Float"),
    ("/", "float_div", "Float"),
    ("%", "float_mod", "Float"),
    ("**", "float_pow", "Float"),
    ("==", "float_eq", "Bool"),
    ("!=", "float_neq", "Bool"),
    ("<", "float_lt", "Bool"),
    (">", "float_gt", "Bool"),
    ("<=", "float_le", "Bool"),
    (">=", "float_ge", "Bool"),
];

const FLOAT_UNARY_OPS: &[(&str, &str)] = &[("-@", "float_neg"), ("+@", "float_pos")];

/// `Array`/`Hash`/`Str`/`Range`'s minimal built-in method set (Phase 3 --
/// see `spinel_rt::collections`'s module docs for the deliberate scope-cut:
/// `[]`/`[]=`/`length` only, no Enumerable). `a[i]`/`a[i] = v` are ordinary
/// `CallNode`s named `"[]"`/`"[]="` at the `ruby-prism` level (just like the
/// numeric operators above), so this is dispatch-table generalization, not a
/// new HIR shape -- mirroring the Phase 1 insight that operators were
/// already plain calls. Returns `None` (falls through to ordinary Path 1/
/// Path 2 dispatch below) for any receiver whose static type isn't one of
/// these four, so a user class's own `def []` is completely unaffected.
fn try_collection_dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    let ty = infer(cx, recv_id);
    let tokens = match (ty, name, args.len()) {
        (TyKind::Array, "[]", 1) => {
            let idx = emit_expr(cx, args[0]);
            quote! { spinel_rt::array_get(&(#recv_expr).as_array_unchecked(), (#idx).as_int_unchecked()) }
        }
        (TyKind::Array, "[]=", 2) => {
            let idx = emit_expr(cx, args[0]);
            let val = emit_expr(cx, args[1]);
            // A negative index still out of range after counting from the
            // end raises a real `IndexError` -- constructed
            // here, not inside `spinel_rt::array_set` itself, since only
            // codegen has the class registry needed to build one (see
            // `emit_boxed_new`'s docs).
            let index_error = super::expr::emit_boxed_new(
                cx,
                "IndexError",
                vec![quote! {
                    spinel_rt::RubyValue::Str(spinel_rt::string_new(
                        format!("index {__idx} too small for array")
                    ))
                }],
            );
            quote! {
                {
                    let __idx = (#idx).as_int_unchecked();
                    match spinel_rt::array_set(&(#recv_expr).as_array_unchecked(), __idx, #val) {
                        Some(__v) => __v,
                        None => return Err(spinel_rt::Signal::Raise(#index_error)),
                    }
                }
            }
        }
        (TyKind::Array, "length" | "size", 0) => {
            quote! { spinel_rt::RubyValue::Int(spinel_rt::array_len(&(#recv_expr).as_array_unchecked())) }
        }
        (TyKind::Hash, "[]", 1) => {
            let key = emit_expr(cx, args[0]);
            quote! { spinel_rt::hash_get(&(#recv_expr).as_hash_unchecked(), &(#key)) }
        }
        (TyKind::Hash, "[]=", 2) => {
            let key = emit_expr(cx, args[0]);
            let val = emit_expr(cx, args[1]);
            quote! { spinel_rt::hash_set(&(#recv_expr).as_hash_unchecked(), #key, #val) }
        }
        (TyKind::Hash, "length" | "size", 0) => {
            quote! { spinel_rt::RubyValue::Int(spinel_rt::hash_len(&(#recv_expr).as_hash_unchecked())) }
        }
        (TyKind::Str, "[]", 1) => {
            let idx = emit_expr(cx, args[0]);
            quote! { spinel_rt::string_get(&(#recv_expr).as_str_unchecked(), (#idx).as_int_unchecked()) }
        }
        (TyKind::Str, "[]=", 2) => {
            let idx = emit_expr(cx, args[0]);
            let val = emit_expr(cx, args[1]);
            quote! { spinel_rt::string_set(&(#recv_expr).as_str_unchecked(), (#idx).as_int_unchecked(), &(#val)) }
        }
        (TyKind::Str, "length" | "size", 0) => {
            quote! { spinel_rt::RubyValue::Int(spinel_rt::string_len(&(#recv_expr).as_str_unchecked())) }
        }
        (TyKind::Range, "first", 0) => quote! { (#recv_expr).range_first() },
        (TyKind::Range, "last", 0) => quote! { (#recv_expr).range_last() },
        (TyKind::Range, "exclude_end?", 0) => {
            quote! { spinel_rt::RubyValue::Bool((#recv_expr).range_exclude_end()) }
        }
        _ => return None,
    };
    Some(tokens)
}

/// `blk.call(args)` / `blk.(args)` / `blk[args]` on a statically
/// `TyKind::Proc` receiver -- dispatches directly to the underlying
/// closure, mirroring `try_collection_dispatch`'s shape. Only ever reached
/// for a NAMED `&block` parameter (the only way a local/param is currently
/// inferred `Proc` -- see `analyze::register_class`'s seeding, mirroring
/// how a named `*rest`/`**kwrest` seeds `Array`/`Hash`); nothing else infers
/// this type yet, so a user class's own `def call` is unaffected.
fn try_proc_dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    if infer(cx, recv_id) != TyKind::Proc || !matches!(name, "call" | "()" | "[]") {
        return None;
    }
    let arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    Some(quote! { ((#recv_expr).as_proc_unchecked())(&[#(#arg_exprs),*])? })
}

pub fn emit_new(cx: &Ctx, class_name: &str, args: &[NodeId]) -> TokenStream {
    // Boxed via `box_if_object_typed`: `initialize`'s own Rust parameters
    // are always plain `RubyValue` (see that function's docs) -- an
    // Object-typed constructor ARGUMENT (e.g. passing one class instance
    // into another's constructor) otherwise emits a bare, unboxed
    // `Arc<Concrete>`, a real `rustc` type mismatch confirmed by direct
    // reproduction.
    let arg_exprs = args
        .iter()
        .map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        })
        .collect();
    emit_new_with_arg_tokens(cx, class_name, arg_exprs)
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
    let cid = cx
        .compiler
        .class_by_name(class_name)
        .unwrap_or_else(|| panic!("unknown class `{class_name}`"));
    let ci = cx.compiler.class(cid);
    let class_ident = safe_ident(class_name);

    let fields = ci.ivars.iter().map(|iv| {
        let f = safe_ident(iv);
        quote! { #f: spinel_rt::parking_lot::Mutex::new(spinel_rt::RubyValue::Nil), }
    });
    // Wrapped in `Arc` immediately, not just at `new_handle` time: a local
    // holding this needs to be `Arc::clone()`-able on every re-read
    // (`codegen::expr`'s `LocalRead` -- see `ruby_class!`'s `new_handle` docs
    // for why the bare struct can't just derive `Clone` instead). `Arc<T>`
    // derefs transparently, so Path 1's `(recv_expr).method(...)` calls still
    // work unchanged against a `self: Arc<Self>`-shaped method. `Arc` (not
    // `Rc`, Part 9): every generated struct is genuinely `Send + Sync`.
    let ctor = quote! { std::sync::Arc::new(#class_ident { #(#fields)* }) };

    match cx.compiler.method_in_chain(cid, "initialize") {
        Some(_) => {
            // `initialize` takes `self: Arc<Self>` BY VALUE now (see
            // `ruby_class!`'s docs), so calling it on `__obj` directly would
            // move it -- clone the `Arc` handle first (a cheap refcount bump,
            // not a deep copy) so `__obj` is still available to return.
            quote! { { let __obj = #ctor; __obj.clone().initialize(#(#arg_exprs),*)?; __obj } }
        }
        None => ctor,
    }
}

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
/// Rust structs with no shared layout (unlike spinel's C "common initial
/// sequence" trick), so `Animal::speak(self: &Dog)` wouldn't type-check.
/// Splicing the resolved method's body directly into the call site
/// sidesteps that entirely -- and is a real spinel mechanism too, just used
/// there as an optimization (`emit_super_inline`, reached when the parent
/// yields) rather than the default.
pub fn emit_super_inline(cx: &Ctx, args: &[NodeId]) -> TokenStream {
    let receiver_class = cx.current_class.expect("`super` outside a method");
    let defining_class = cx.defining_class.expect("`super` outside a method");
    let mname = cx
        .current_method
        .as_deref()
        .expect("`super` outside a method");

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
        cx.compiler
            .class(anc)
            .own_methods
            .iter()
            .find(|&&s| cx.compiler.scope(s).name == mname)
            .map(|&sid| (anc, sid))
    });
    let (new_defining_class, sid) = found.unwrap_or_else(|| {
        panic!(
            "`super`: no `{mname}` found above {}",
            cx.compiler.class(defining_class).name
        )
    });

    // The method CURRENTLY executing (whose lexical body this `super` call
    // sits inside) -- needed only for bare `super`'s forwarding case below.
    // Guaranteed to exist: `defining_class` was either the receiver's own
    // class (an ordinary call into this function) or a previously-found
    // ancestor from an earlier `super` splice, and both cases only ever set
    // `defining_class` to a class that owns a `mname` method (that's exactly
    // how it was found).
    let current_sid = cx
        .compiler
        .class(defining_class)
        .own_methods
        .iter()
        .find(|&&s| cx.compiler.scope(s).name == mname)
        .copied()
        .unwrap_or_else(|| {
            panic!("internal error: `{mname}` not found in its own defining class's own_methods")
        });
    let current_params = cx.compiler.scope(current_sid).params.clone();

    let defining_scope = cx.compiler.scope(sid);
    let body = defining_scope.body.clone();
    // `loop_labels`/`label_counter` carry over from `cx` unchanged: the
    // parent method's body is spliced in at this call site, so a `break`
    // inside it must still target whatever loop lexically encloses the
    // `super` call, exactly as if that code were written there directly (see
    // `Ctx::loop_labels`'s docs).
    let defining_captures = super::captures::collect_escaping_captures(cx.compiler, &defining_scope.body);
    let inline_cx = Ctx {
        compiler: cx.compiler,
        // UNCHANGED across the splice -- `self` is still the SAME concrete
        // receiver instance throughout a chain of nested `super` calls.
        current_class: Some(receiver_class),
        defining_class: Some(new_defining_class),
        current_method: Some(mname.to_string()),
        local_types: std::borrow::Cow::Borrowed(&defining_scope.local_types),
        label_counter: cx.label_counter,
        loop_labels: cx.loop_labels.clone(),
        // NOT inherited -- see `Ctx::for_var_override`'s docs.
        for_var_override: None,
        captured_locals: &defining_captures.locals,
        // Both ARE inherited (unlike `for_var_override`): the inlined body
        // must keep referring to whichever `self` the CALLING method's own
        // body is already using, and `break`/`next`/`redo`/`return` inside
        // it must still behave per whatever Rust-function boundary actually
        // encloses this splice (a real Proc closure or not).
        self_ident: cx.self_ident.clone(),
        in_real_proc: cx.in_real_proc,
    };
    // Binds the parent's OWN parameter names fresh, before splicing its body
    // in -- previously this relied on the parent's params
    // happening to share names with the calling method's own, since the
    // spliced body just referenced its param names directly with nothing
    // ever binding them. See `emit_super_arg_bindings`'s docs.
    let bindings = emit_super_arg_bindings(cx, &inline_cx, &defining_scope.params, &current_params, args);
    // A fresh hoisting prelude of its own: the parent method's local
    // variables are a genuinely separate Ruby scope from the calling
    // (sub)method's, even though inlining splices their statements into the
    // same Rust expression position (see `hoisting`'s docs).
    let inlined = super::hoisting::emit_hoisted_body_with_extra_roots(
        &inline_cx,
        &body,
        &defining_scope.params.default_ids(),
        false,
    );
    quote! { { #bindings #inlined } }
}

/// Binds the parent method's (`parent_params`) own parameter names, right
/// before its body is spliced in -- either from EXPLICIT `super(expr, ...)`
/// arguments (evaluated in the CALLING scope, `cx`), or, for bare `super`
/// (forwarding), from the CURRENTLY-EXECUTING method's (`current_params`)
/// own already-bound parameter of the same position within each bucket
/// (required/optional/rest/post; keywords matched by NAME instead, since
/// position isn't meaningful there). `args.is_empty()` is treated as the
/// forwarding case -- this also (harmlessly) covers a literal `super()`,
/// which real Ruby treats as "no arguments at all" rather than forwarding;
/// `HirNode::SuperCall` doesn't distinguish the two shapes (see
/// `parse/mod.rs`'s lowering, a pre-existing, documented, narrow
/// simplification this fix doesn't change), so this collapses to the more
/// common (forwarding) case, same as before this fix.
fn emit_super_arg_bindings(
    cx: &Ctx,
    inline_cx: &Ctx,
    parent_params: &Params,
    current_params: &Params,
    args: &[NodeId],
) -> TokenStream {
    if args.is_empty() {
        return emit_super_forwarding_bindings(parent_params, current_params);
    }
    emit_super_explicit_bindings(cx, inline_cx, parent_params, args)
}

/// Bare `super`: bind each of the parent's own required/optional/rest/post
/// parameter names to the CURRENT method's own already-bound parameter of
/// the SAME POSITION within that bucket (i.e. current values, not a
/// positional re-evaluation of anything) -- a keyword param is matched by
/// NAME instead, since "position" isn't meaningful there. Any parent
/// parameter with no corresponding current one (arities/shapes genuinely
/// differ between parent and child) is simply left unbound, keeping its own
/// `nil`/lazy-default codegen (a narrow, documented approximation of Ruby's
/// full forwarding semantics -- most real overrides share a compatible
/// shape).
fn emit_super_forwarding_bindings(parent_params: &Params, current_params: &Params) -> TokenStream {
    let mut lets = Vec::new();
    for (i, name) in parent_params.required.iter().enumerate() {
        if let Some(src) = current_params.required.get(i) {
            let dst = safe_ident(name);
            let src_ident = safe_ident(src);
            lets.push(quote! { let #dst: spinel_rt::RubyValue = #src_ident.clone(); });
        }
    }
    for (i, (name, _)) in parent_params.optional.iter().enumerate() {
        if let Some((src, _)) = current_params.optional.get(i) {
            let dst = safe_ident(name);
            let src_ident = safe_ident(src);
            lets.push(quote! { let #dst: spinel_rt::RubyValue = #src_ident.clone(); });
        }
    }
    if let Some(Some(dst_name)) = &parent_params.rest {
        if let Some(Some(src_name)) = &current_params.rest {
            let dst = safe_ident(dst_name);
            let src_ident = safe_ident(src_name);
            lets.push(quote! { let #dst: spinel_rt::RubyValue = #src_ident.clone(); });
        }
    }
    for (i, name) in parent_params.post.iter().enumerate() {
        if let Some(src) = current_params.post.get(i) {
            let dst = safe_ident(name);
            let src_ident = safe_ident(src);
            lets.push(quote! { let #dst: spinel_rt::RubyValue = #src_ident.clone(); });
        }
    }
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
            // uniformity with the other buckets above.
            let dst = safe_ident(dst_name);
            lets.push(quote! { let #dst: spinel_rt::RubyValue = #dst.clone(); });
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

    let pos_temps: Vec<syn::Ident> = (0..args.len()).map(|i| format_ident!("__super_a{i}")).collect();
    let pos_lets = args.iter().zip(&pos_temps).map(|(&a, t)| {
        let e = emit_expr(cx, a);
        quote! { let #t = #e; }
    });

    let required_lets = parent_params.required.iter().enumerate().map(|(i, name)| {
        let dst = safe_ident(name);
        let src = &pos_temps[i];
        quote! { let #dst: spinel_rt::RubyValue = #src.clone(); }
    });
    let optional_lets = parent_params.optional.iter().enumerate().map(|(i, (name, default))| {
        let dst = safe_ident(name);
        if i < opt_bound {
            let src = &pos_temps[nreq + i];
            quote! { let #dst: spinel_rt::RubyValue = #src.clone(); }
        } else {
            let default_expr = emit_expr(inline_cx, *default);
            quote! { let #dst: spinel_rt::RubyValue = #default_expr; }
        }
    });
    let rest_let = parent_params.rest.as_ref().and_then(|r| r.as_ref()).map(|name| {
        let dst = safe_ident(name);
        let elems = pos_temps[nreq + opt_bound..nreq + opt_bound + rest_count]
            .iter()
            .map(|t| quote! { #t.clone() });
        quote! { let #dst: spinel_rt::RubyValue = spinel_rt::RubyValue::Array(spinel_rt::array_new(vec![#(#elems),*])); }
    });
    let post_lets = parent_params.post.iter().enumerate().map(|(i, name)| {
        let dst = safe_ident(name);
        let src = &pos_temps[nreq + opt_bound + rest_count + i];
        quote! { let #dst: spinel_rt::RubyValue = #src.clone(); }
    });

    quote! {
        #(#pos_lets)*
        #(#required_lets)*
        #(#optional_lets)*
        #rest_let
        #(#post_lets)*
    }
}

/// Builds a real, escaping `spinel_rt::RubyValue::Proc` value from a literal
/// block (`HirNode::Block`) at a call site whose callee ISN'T the `.times`
/// inline fast path (see `is_times_fast_path`'s docs -- that's the entire
/// "escape decision", no separate dataflow analysis exists). A plain Rust
/// closure (`move |args| { ... }`), not a hand-rolled env struct/trait --
/// Rust's own closure capture already builds exactly the environment one
/// would otherwise hand-generate (see `spinel_rt::rproc`'s docs).
///
/// Capture strategy: `codegen::captures::block_captures` finds every name
/// this SPECIFIC block references. Names ALSO in `cx.captured_locals` (i.e.
/// genuinely shared with code outside the block) get an `Arc::clone` into a
/// same-named local right before the closure, then `move`d in -- the
/// closure body's ordinary `emit_local_read`/`write` codegen (via
/// `cx.captured_locals`, unchanged inside the closure) transparently
/// resolves them to that shared cell. Names NOT in `cx.captured_locals` are
/// block-OWNED locals (fresh every invocation, confirmed against real Ruby
/// -- see `hoisting::emit_proc_own_locals_prelude`'s docs) and get their own
/// declaration INSIDE the closure instead. `self`/ivar references clone an
/// owned `Arc<Self>` handle the same way (`self_ident` cannot be `let`-bound
/// directly -- see `Ctx::in_proc`'s docs).
///
/// `redo`/`next` never escape the closure (caught by the wrapping labeled
/// loop below); `break`/`return` propagate via `?`/a raised `Signal`, caught
/// respectively at the call site that attached this block
/// (`emit_call_args`'s `catch_break`) and the lexically enclosing method's
/// own boundary (`codegen::mod`'s per-method wrapping).
pub fn emit_proc_value(cx: &Ctx, block_id: NodeId) -> TokenStream {
    let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
        panic!("expected a block")
    };
    emit_proc_or_lambda_value(cx, params, body, false)
}

/// `-> (x) { ... }` / `lambda { ... }` (`HirNode::Lambda`) -- see that
/// variant's docs. Shares its ENTIRE construction with `emit_proc_value`
/// (captures, redo-wrapper loop) via `emit_proc_or_lambda_value`, differing
/// only in the two places real Ruby's own lambda semantics require: strict
/// arity (`is_lambda: true` gates a runtime `ArgumentError` check
/// `emit_proc_or_lambda_value` inserts) and folding `Signal::Return`/`Break`
/// into a normal `Ok` return instead of letting them propagate.
pub fn emit_lambda_value(cx: &Ctx, params: &Params, body: &[NodeId]) -> TokenStream {
    emit_proc_or_lambda_value(cx, params, body, true)
}

fn emit_proc_or_lambda_value(cx: &Ctx, params: &Params, body: &[NodeId], is_lambda: bool) -> TokenStream {
    let block_caps = super::captures::block_captures(cx.compiler, params, body);

    let mut genuine: Vec<&String> = block_caps.locals.iter().filter(|n| cx.captured_locals.contains(*n)).collect();
    genuine.sort();
    let capture_clones = genuine.iter().map(|name| {
        let ident = safe_ident(name);
        quote! { let #ident = ::std::sync::Arc::clone(&#ident); }
    });
    let own_only: std::collections::HashSet<String> = block_caps
        .locals
        .iter()
        .filter(|n| !cx.captured_locals.contains(*n))
        .cloned()
        .collect();

    let needs_self = block_caps.self_captured;
    let self_clone = needs_self.then(|| {
        let slf = &cx.self_ident;
        quote! { let __self = ::std::sync::Arc::clone(&#slf); }
    });

    let proc_cx = cx.in_proc(needs_self);
    let own_locals_prelude = super::hoisting::emit_proc_own_locals_prelude(&proc_cx, &own_only);
    let arity_check = is_lambda.then(|| emit_lambda_arity_check(cx, params, &format_ident!("__args")));
    let param_bindings =
        super::params::emit_proc_param_bindings(&proc_cx, params, &format_ident!("__args"));
    // NOT `hoisting::emit_hoisted_body` -- that would re-collect EVERY name
    // this block references (including the genuine captures above) and
    // declare them AGAIN, shadowing the shared `Arc::clone`s just captured
    // with brand-new empty cells. `own_locals_prelude` already handles the
    // one case that genuinely needs a fresh declaration.
    let body_tokens = super::stmt::emit_body(&proc_cx, body, true);
    let redo_label = super::loops::fresh_label(cx, "proc_redo");

    // A lambda folds `Return`/`Break` into a normal `Ok` return (it's a
    // self-contained closure boundary, like a method -- see
    // `hir::HirNode::Lambda`'s docs); an ordinary Proc lets them propagate
    // via `other => break #redo_label other` (caught, respectively, at the
    // call site that attached the block and the lexically enclosing
    // method's own boundary).
    let terminal_arm = if is_lambda {
        quote! {
            Err(spinel_rt::Signal::Return(__v)) | Err(spinel_rt::Signal::Break(__v)) => break #redo_label Ok(__v),
            other => break #redo_label other,
        }
    } else {
        quote! { other => break #redo_label other, }
    };

    quote! {
        {
            #(#capture_clones)*
            #self_clone
            spinel_rt::RubyValue::Proc(::std::sync::Arc::new(move |__args: &[spinel_rt::RubyValue]| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
                #redo_label: loop {
                    let __result: Result<spinel_rt::RubyValue, spinel_rt::Signal> = (|| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
                        #arity_check
                        #own_locals_prelude
                        #param_bindings
                        #body_tokens
                    })();
                    match __result {
                        Err(spinel_rt::Signal::Redo) => continue #redo_label,
                        Err(spinel_rt::Signal::Next(__v)) => break #redo_label Ok(__v),
                        #terminal_arm
                    }
                }
            }))
        }
    }
}

/// A lambda's STRICT arity check (real Ruby: missing/extra positional
/// arguments raise `ArgumentError`, unlike an ordinary block's lenient nil-
/// fill/drop -- see `codegen::params::emit_proc_param_bindings`'s docs).
/// Emitted INSIDE the closure body, checked against the runtime `__args`
/// slice, mirroring `codegen::params::emit_dynamic_trampoline`'s arity-check
/// shape but raising a real, catchable exception instead of a bare panic
/// (a lambda's `ArgumentError` is ordinary Ruby-level control flow, fully
/// expected to be rescued).
fn emit_lambda_arity_check(cx: &Ctx, params: &Params, args_ident: &proc_macro2::Ident) -> TokenStream {
    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let has_rest = params.rest.is_some();
    let min_lit = nreq + npost;
    let min_cond = (min_lit > 0).then(|| quote! { #args_ident.len() < #min_lit });
    let max_cond = (!has_rest).then(|| {
        let max_lit = nreq + nopt + npost;
        quote! { #args_ident.len() > #max_lit }
    });
    let cond = match (min_cond, max_cond) {
        (None, None) => return TokenStream::new(),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (Some(a), Some(b)) => quote! { #a || #b },
    };
    let err = super::expr::emit_boxed_new(
        cx,
        "ArgumentError",
        vec![quote! {
            spinel_rt::RubyValue::Str(spinel_rt::string_new(format!(
                "wrong number of arguments (given {}, expected {})",
                #args_ident.len(), #min_lit
            )))
        }],
    );
    quote! {
        if #cond {
            return Err(spinel_rt::Signal::Raise(#err));
        }
    }
}

/// The implicit block argument for a call site, as an `Option<RubyValue>`
/// expression -- `Some(proc)` from a literal block (`emit_proc_value`) or a
/// forwarded `&existing_proc`, `None` when neither is present. Shared by
/// Path 1 (`codegen::params::emit_call_args`, gated on `needs_block`) and
/// Path 2 (`send`/`public_send` below, built unconditionally since the
/// dynamic target's own needs aren't known statically).
pub(super) fn emit_block_option(cx: &Ctx, block: Option<NodeId>, block_arg: Option<NodeId>) -> TokenStream {
    match (block, block_arg) {
        (Some(b), None) => {
            let v = emit_proc_value(cx, b);
            quote! { Some(#v) }
        }
        (None, Some(e)) => {
            let v = emit_expr(cx, e);
            quote! { Some(#v) }
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
    kwargs: &[HashPair],
    kwargs_splat: Option<NodeId>,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    safe: bool,
) -> TokenStream {
    // A call-site `*expr`/`**h` splat can't take any of the arity-checked
    // static paths below (the flattened argument COUNT isn't known until
    // runtime) -- see `emit_splat_call`'s docs for the always-dynamic
    // fallback this routes to instead. Every other call site (the
    // overwhelming common case) is completely unaffected: `args` unwraps
    // back to a plain `Vec<NodeId>` and every existing fast path below runs
    // exactly as it did before call-site splats existed.
    if kwargs_splat.is_some() || args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
        return emit_splat_call(cx, receiver, name, args, kwargs, kwargs_splat, block, block_arg, safe);
    }
    let args: Vec<NodeId> = args
        .iter()
        .map(|a| match a {
            ArrayElem::Single(n) => *n,
            ArrayElem::Splat(_) => unreachable!("checked above"),
        })
        .collect();
    let args = &args[..];

    // Implicit self / no receiver. `&.` is meaningless without a receiver,
    // so `safe` is irrelevant here.
    let Some(recv_id) = receiver else {
        if name == "puts" && args.len() == 1 && kwargs.is_empty() {
            let arg = emit_expr(cx, args[0]);
            return quote! { { spinel_rt::puts(#arg); spinel_rt::RubyValue::Nil } };
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
            if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
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
        panic!("unsupported implicit-self call `{name}` (spike scope, or no such method is defined on the current class)");
    };

    // `ClassName.foo(...)` / `ModuleName.foo(...)` -- a call on the
    // class/module itself, not an instance (see `HirNode::ClassRef`'s
    // docs). Never a `RubyValue`, so this must be intercepted before the
    // ordinary `emit_expr(cx, recv_id)` receiver-evaluation path below ever
    // sees it (mirrors `HirNode::Block`'s "only reached via the Call that
    // invokes it" pattern).
    if let HirNode::ClassRef(target_name) = &cx.compiler.hir[recv_id] {
        if safe || block.is_some() || block_arg.is_some() {
            panic!("safe-navigation or a block on a class-method call isn't supported yet (spike scope)");
        }
        return emit_class_method_call(cx, target_name, name, args, kwargs);
    }

    if safe {
        if !kwargs.is_empty() {
            panic!("keyword arguments on a safe-navigation (`&.`) call aren't supported yet (spike scope)");
        }
        if block.is_some() || block_arg.is_some() {
            panic!("a block on a safe-navigation (`&.`) call isn't supported yet (spike scope)");
        }
        return emit_safe_call(cx, recv_id, name, args);
    }

    let recv_expr = emit_expr(cx, recv_id);
    dispatch(cx, recv_id, name, args, kwargs, block, block_arg, &recv_expr, false)
}

/// A call site carrying a `*arr` positional splat and/or `**h` double-splat
/// -- see `emit_call`'s docs for why this can never take a static, arity-
/// checked calling convention. ALWAYS dispatches dynamically via
/// `spinel_rt::send`, even when the receiver's class is statically known --
/// a real, documented, minor perf cost (not a correctness gap): call-site
/// splats are rare enough that duplicating Path 1's whole typed-parameter
/// machinery for a runtime-variable argument count isn't worthwhile.
/// Keyword arguments (literal `kwargs` or a `**h` double-splat) have no Path
/// 2 channel at all (matches the existing `send`/`public_send`
/// dynamic-dispatch restriction elsewhere in this file) -- a clean
/// rejection, not silently dropped.
#[allow(clippy::too_many_arguments)]
fn emit_splat_call(
    cx: &Ctx,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[HashPair],
    kwargs_splat: Option<NodeId>,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    safe: bool,
) -> TokenStream {
    if !kwargs.is_empty() || kwargs_splat.is_some() {
        panic!("a call combining a `*`/`**` splat argument with keyword arguments isn't supported yet (spike scope)");
    }
    if safe {
        panic!("safe-navigation (`&.`) on a call with a splat argument isn't supported yet (spike scope)");
    }
    let recv_obj_expr = match receiver {
        Some(recv_id) => {
            if matches!(cx.compiler.hir[recv_id], HirNode::ClassRef(_)) {
                panic!("a splat argument on a class-method call isn't supported yet (spike scope)");
            }
            let recv_expr = emit_expr(cx, recv_id);
            match infer_class(cx, recv_id) {
                Some(cid) => {
                    let class_ident = safe_ident(&cx.compiler.class(cid).name);
                    quote! { #class_ident::new_handle(#recv_expr) }
                }
                None if infer(cx, recv_id) == TyKind::Poly => quote! { (#recv_expr).as_object_unchecked() },
                None => panic!("a splat argument call on a receiver whose class isn't statically known (and isn't a `rescue` binding) isn't supported yet (spike scope)"),
            }
        }
        None => {
            // `puts`/other no-receiver builtins aren't reachable through
            // `spinel_rt::send` at all (they have no `ClassRegistry` entry) --
            // a clean rejection here beats generating code that only fails
            // at RUNTIME with a confusing "no such method".
            let Some(cid) = cx.current_class.filter(|&cid| cx.compiler.method_in_chain(cid, name).is_some()) else {
                panic!("unsupported implicit-self splat call `{name}` (spike scope, or no such method is defined on the current class)");
            };
            let slf = &cx.self_ident;
            let class_ident = safe_ident(&cx.compiler.class(cid).name);
            quote! { #class_ident::new_handle(#slf.clone()) }
        }
    };
    let name_expr = quote! { spinel_rt::Symbol::intern(#name) };
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
    let block_value = emit_block_option(cx, block, block_arg);
    quote! {
        {
            let mut __args: Vec<spinel_rt::RubyValue> = Vec::new();
            #(#arg_pushes)*
            spinel_rt::catch_break(spinel_rt::send(&#recv_obj_expr, #name_expr, &__args, #block_value))?
        }
    }
}

/// `ClassName.foo(...)` -- looked up in the target's MRO-resolved
/// `class_methods` (see `Compiler::class_method_in_chain`) and called as a
/// plain Rust associated-function/free-function path (`Target::foo(...)`),
/// never through `send`/the dynamic dispatch table at all (no runtime
/// `Class`/`Module` value exists to dispatch through dynamically -- see the
/// plan's Part 6 scope-cut). Only plain required parameters are supported
/// (see `codegen::mod::emit_class_method_fn`'s matching rejection) -- kept
/// deliberately narrow since call-site binding for optional/rest/keyword
/// params would duplicate `params::emit_call_args`'s machinery for a
/// second, self-less calling convention (`Target::method(...)` instead of
/// `(recv).method(...)`) that class methods don't yet need.
fn emit_class_method_call(
    cx: &Ctx,
    target_name: &str,
    name: &str,
    args: &[NodeId],
    kwargs: &[HashPair],
) -> TokenStream {
    let target = cx
        .compiler
        .class_by_name(target_name)
        .unwrap_or_else(|| panic!("unknown class/module `{target_name}`"));
    let Some((_, sid)) = cx.compiler.class_method_in_chain(target, name) else {
        panic!(
            "unsupported call `{target_name}.{name}` (spike scope, or no such class method is defined)"
        );
    };
    let scope = cx.compiler.scope(sid);
    if !kwargs.is_empty()
        || !scope.params.optional.is_empty()
        || scope.params.rest.is_some()
        || !scope.params.post.is_empty()
        || !scope.params.keywords.is_empty()
        || scope.params.keyword_rest.is_some()
        || scope.needs_block_param()
    {
        panic!(
            "class method call `{target_name}.{name}` uses keyword arguments or a callee with optional/rest/post/keyword parameters or a block -- only plain required parameters are supported yet (spike scope)"
        );
    }
    if args.len() != scope.params.required.len() {
        panic!(
            "wrong number of arguments for `{target_name}.{name}` (spike scope): expected {}, got {}",
            scope.params.required.len(),
            args.len()
        );
    }
    let target_ident = safe_ident(target_name);
    let method_ident = safe_ident(name);
    let arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    quote! { #target_ident::#method_ident(#(#arg_exprs),*)? }
}

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
    let arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    quote! {
        {
            let __safe_recv = #boxed_recv;
            if __safe_recv.is_nil() {
                spinel_rt::RubyValue::Nil
            } else {
                match &__safe_recv {
                    spinel_rt::RubyValue::Object(__robj) => {
                        // `&.` doesn't accept a block yet (spike scope,
                        // unrelated to Phase 6 -- narrower than real Ruby,
                        // matches this call's existing kwargs restriction).
                        spinel_rt::send(__robj, #name_expr, &[#(#arg_exprs),*], None)?
                    }
                    _ => panic!(
                        "safe navigation on a non-Object receiver isn't supported yet (spike scope)"
                    ),
                }
            }
        }
    }
}

/// Enforces `scope`'s visibility for an explicit-receiver Path 1 call --
/// panics with a clear compile-time error if disallowed. Only ever called
/// when `bypass_visibility` is `false` (see `dispatch`'s docs): an implicit-
/// self call never reaches this at all (handled entirely separately in
/// `emit_call`'s own no-receiver branch), and `send` always bypasses it
/// (matching real Ruby). Mirrors real Ruby's actual rules: `private` allows
/// an EXPLICIT literal `self` receiver (Ruby 2.7+) but nothing else;
/// `protected` allows a call whose CALLING method's own receiver class is
/// ancestor-related (either direction) to the target method's owner class
/// -- e.g. `def ==(other); x == other.x; end` calling a `protected` `x` on
/// `other`, another instance of the same class. Path 2 (dynamic dispatch
/// against a receiver whose class isn't statically known) doesn't enforce
/// this at all yet -- a documented, narrow gap, matching this codebase's
/// existing posture on other Path-1-only guarantees (e.g. keyword args).
fn enforce_visibility(cx: &Ctx, recv_id: NodeId, scope: &crate::compiler::Scope, method_name: &str) {
    match scope.visibility {
        Visibility::Public => {}
        Visibility::Private => {
            if !matches!(cx.compiler.hir[recv_id], HirNode::SelfRef) {
                panic!(
                    "private method `{method_name}` called with an explicit receiver (spike scope: only a literal `self` receiver or no receiver at all is allowed, matching real Ruby)"
                );
            }
        }
        Visibility::Protected => {
            let owner = scope.class.expect("a materialized method always has an owner class");
            let related = cx.current_class.is_some_and(|caller_cid| {
                caller_cid == owner
                    || cx.compiler.class(caller_cid).ancestors.contains(&owner)
                    || cx.compiler.class(owner).ancestors.contains(&caller_cid)
            });
            if !related {
                panic!(
                    "protected method `{method_name}` called from outside a related class (spike scope)"
                );
            }
        }
    }
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
    kwargs: &[HashPair],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    recv_expr: &TokenStream,
    bypass_visibility: bool,
) -> TokenStream {
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
        return quote! { spinel_rt::RubyValue::Bool(!(#recv_expr).truthy()) };
    }

    // `is_a?`/`kind_of?` against a literal class/module constant -- a real
    // ancestry check against the SAME linearized `ancestors` list `super`
    // consults (see `analyze::mro`), not spinel's own two-tier dispatch/
    // reflection split (confirmed to diverge on a module-of-module
    // diamond). Constant-folds to a literal `true`/`false` when the
    // receiver's class is statically known (the common Path 1 case);
    // otherwise falls back to a runtime `spinel_rt::is_a` check against the
    // receiver's actual runtime `class_id()`.
    if no_kwargs && (name == "is_a?" || name == "kind_of?") && args.len() == 1 {
        if let HirNode::ClassRef(target_name) = &cx.compiler.hir[args[0]] {
            let target = cx
                .compiler
                .class_by_name(target_name)
                .unwrap_or_else(|| panic!("unknown class/module `{target_name}`"));
            return match infer_class(cx, recv_id) {
                Some(recv_class) => {
                    let result = cx.compiler.class(recv_class).ancestors.contains(&target);
                    quote! { spinel_rt::RubyValue::Bool(#result) }
                }
                None => {
                    let target_ident = safe_ident(&cx.compiler.class(target).name);
                    quote! {
                        spinel_rt::RubyValue::Bool(spinel_rt::is_a(
                            (#recv_expr).as_object_unchecked().class_id(),
                            #target_ident::CLASS_ID,
                        ))
                    }
                }
            };
        }
    }

    // `respond_to?(:name)` -- a flat probe on the receiver's own already-
    // materialized method table (`spinel_rt::responds_to`; see its docs for
    // why no ancestor walk is needed, mirroring `send`'s own dispatch).
    // Works uniformly whether the receiver's class is statically known
    // (Object) or only known at runtime (Poly) -- unlike `is_a?` above,
    // there's no compile-time constant-fold here (a name could still resolve
    // differently at runtime for a `define_method`-extended class), so this
    // always calls into the registry.
    if no_kwargs && name == "respond_to?" && args.len() == 1 {
        let sym_expr = emit_symbol_expr(cx, args[0]);
        let class_id_expr = match infer_class(cx, recv_id) {
            Some(cid) => {
                let class_ident = safe_ident(&cx.compiler.class(cid).name);
                quote! { #class_ident::CLASS_ID }
            }
            None => quote! { (#recv_expr).as_object_unchecked().class_id() },
        };
        return quote! {
            spinel_rt::RubyValue::Bool(spinel_rt::responds_to(#class_id_expr, #sym_expr))
        };
    }

    // Native `Int` arithmetic/comparison/bitwise ops: both operands must be
    // statically known `Int` (see `INT_BINARY_OPS`'s docs above).
    if no_kwargs && args.len() == 1 {
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
    if no_kwargs && args.is_empty() {
        if let Some(&(_, rt_fn)) = INT_UNARY_OPS.iter().find(|(op, _)| *op == name) {
            if infer(cx, recv_id) == TyKind::Int {
                let func = format_ident!("{rt_fn}");
                return quote! {
                    spinel_rt::RubyValue::Int(spinel_rt::#func((#recv_expr).as_int_unchecked()))
                };
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
            (TyKind::Float, TyKind::Float) | (TyKind::Float, TyKind::Int) | (TyKind::Int, TyKind::Float)
        );
        if is_float_op {
            let arg_expr = emit_expr(cx, args[0]);
            let recv_f = match recv_ty {
                TyKind::Float => quote! { (#recv_expr).as_float_unchecked() },
                _ => quote! { (#recv_expr).as_int_unchecked() as f64 },
            };
            let arg_f = match arg_ty {
                TyKind::Float => quote! { (#arg_expr).as_float_unchecked() },
                _ => quote! { (#arg_expr).as_int_unchecked() as f64 },
            };
            // `<=>` isn't in `FLOAT_BINARY_OPS` (see its docs) -- a `NaN`
            // comparison returns `nil`, not an `Int`.
            if name == "<=>" {
                return quote! {
                    match spinel_rt::float_cmp(#recv_f, #arg_f) {
                        Some(__n) => spinel_rt::RubyValue::Int(__n),
                        None => spinel_rt::RubyValue::Nil,
                    }
                };
            }
            if let Some(&(_, rt_fn, result_ty)) = FLOAT_BINARY_OPS.iter().find(|(op, _, _)| *op == name) {
                let func = format_ident!("{rt_fn}");
                let wrapper = format_ident!("{result_ty}");
                return quote! {
                    spinel_rt::RubyValue::#wrapper(spinel_rt::#func(#recv_f, #arg_f))
                };
            }
        }
    }

    // Native `Float` unary operators (`-@`/`+@` -- no `~`, real Ruby's
    // `Float` has none), same eligibility rule.
    if no_kwargs && args.is_empty() {
        if let Some(&(_, rt_fn)) = FLOAT_UNARY_OPS.iter().find(|(op, _)| *op == name) {
            if infer(cx, recv_id) == TyKind::Float {
                let func = format_ident!("{rt_fn}");
                return quote! {
                    spinel_rt::RubyValue::Float(spinel_rt::#func((#recv_expr).as_float_unchecked()))
                };
            }
        }
    }

    if no_kwargs {
        if let Some(tokens) = try_collection_dispatch(cx, recv_id, name, args, recv_expr) {
            return tokens;
        }
        if let Some(tokens) = try_proc_dispatch(cx, recv_id, name, args, recv_expr) {
            return tokens;
        }
    }

    // Known-shape block inlining (mirrors `emit_block_value_into`/`.times`,
    // `codegen_iter.c:1281`): the block body is spliced into a native,
    // labeled Rust loop -- no closure or Proc object is allocated. Shares
    // `codegen::loops`' redo-wrapping machinery with `while`/`until`/`loop`/
    // `for`, so `break`/`next`/`redo` inside a `.times` block work exactly
    // the same way.
    if is_times_fast_path(cx.compiler, Some(recv_id), name, no_kwargs) {
        if let HirNode::IntegerLit(n) = &cx.compiler.hir[recv_id] {
            let n = *n;
            let block_id = block.unwrap_or_else(|| panic!("`times` requires a block"));
            let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
                panic!("`times`'s argument must be a block");
            };
            let outer = super::loops::fresh_label(cx, "times");
            let redo = super::loops::fresh_label(cx, "times_body");
            let loop_cx = cx.in_loop(redo.clone(), outer.clone());
            let bind = params.required.first().map(|p| {
                let ident = safe_ident(p);
                quote! { let #ident = spinel_rt::RubyValue::Int(__i); }
            });
            let inner = super::loops::emit_redo_wrapped_body(&loop_cx, body, &redo);
            return quote! {
                {
                    let mut __i: i64 = 0;
                    #outer: loop {
                        if __i >= #n { break #outer spinel_rt::RubyValue::Nil; }
                        #bind
                        #inner
                        __i += 1;
                    }
                }
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
                    if name == "public_send" && cx.compiler.scope(sid).visibility != Visibility::Public {
                        panic!(
                            "`public_send` cannot call non-public method `{target}` (spike scope, matches real Ruby)"
                        );
                    }
                    return dispatch(cx, recv_id, &target, &args[1..], kwargs, block, block_arg, recv_expr, true);
                }
            }
        }
        // The truly dynamic fallback below has no keyword-argument channel
        // at all (Path 2's calling convention is a bare positional
        // `&[RubyValue]` slice) -- raise the SAME clear error a directly-
        // called method's own trampoline already gives for this
        // (`codegen::params::emit_dynamic_trampoline`), rather than silently
        // dropping `kwargs` and dispatching without them.
        if !kwargs.is_empty() {
            panic!(
                "dynamic dispatch of `{name}` with keyword arguments isn't supported yet (spike scope): call it directly instead"
            );
        }
        // A statically-known class needs boxing into an `RObj` handle first
        // (`recv_expr` is an unboxed `Arc<Concrete>` there); a `Poly` receiver
        // is ALREADY a `RubyValue::Object(...)` at runtime (e.g. a `rescue`
        // clause's exception binding -- see `codegen::exceptions`'s docs for
        // why that's never narrowed to a concrete class), so it just needs
        // unwrapping, not a fabricated `new_handle` call (which would need a
        // compile-time class name we don't have here).
        let recv_obj_expr = match recv_class {
            Some(cid) => {
                let class_ident = safe_ident(&cx.compiler.class(cid).name);
                quote! { #class_ident::new_handle(#recv_expr) }
            }
            None => quote! { (#recv_expr).as_object_unchecked() },
        };
        let name_expr = emit_symbol_expr(cx, args[0]);
        let rest_args = args[1..].iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        let block_value = emit_block_option(cx, block, block_arg);
        // `catch_break` applied unconditionally on this fully-dynamic path
        // (unlike Path 1's `needs_block`-gated version): `send`'s target
        // isn't statically known here, so there's no way to tell in advance
        // whether it might invoke a block -- the match is a cheap no-op
        // when no `Signal::Break` was actually raised.
        return quote! {
            spinel_rt::catch_break(spinel_rt::send(&#recv_obj_expr, #name_expr, &[#(#rest_args),*], #block_value))?
        };
    }

    // Ordinary call with a statically known receiver class: direct call
    // (Path 1). This is the common case -- `method_in_chain` mirrors
    // `comp_method_in_chain` exactly (compiler.c:404).
    if let Some(cid) = recv_class {
        if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
            let scope = cx.compiler.scope(sid);
            if !bypass_visibility {
                enforce_visibility(cx, recv_id, scope, name);
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
    // (spinelc never infers a param's type from its call sites; see
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
    // panicking spinelc at compile time instead.
    if no_kwargs && recv_class.is_none() {
        if args.len() == 1 {
            let int_entry = INT_BINARY_OPS.iter().find(|(op, _, _)| *op == name);
            let float_entry = FLOAT_BINARY_OPS.iter().find(|(op, _, _)| *op == name);
            if int_entry.is_some() || float_entry.is_some() || name == "<=>" {
                let arg_expr = emit_expr(cx, args[0]);
                let int_arm = int_entry.map(|&(_, rt_fn, result_ty)| {
                    let func = format_ident!("{rt_fn}");
                    let wrapper = format_ident!("{result_ty}");
                    quote! {
                        (spinel_rt::RubyValue::Int(__r), spinel_rt::RubyValue::Int(__a)) => {
                            spinel_rt::RubyValue::#wrapper(spinel_rt::#func(*__r, *__a))
                        }
                    }
                });
                // `Float`-`Float`/`Int`-`Float`/`Float`-`Int`, promoting any
                // `Int` side to `f64` first -- same rule as the static fast
                // path above, just runtime-checked instead of statically
                // proven.
                let float_arm = float_entry.map(|&(_, rt_fn, result_ty)| {
                    let func = format_ident!("{rt_fn}");
                    let wrapper = format_ident!("{result_ty}");
                    quote! {
                        (spinel_rt::RubyValue::Float(__r), spinel_rt::RubyValue::Float(__a)) => {
                            spinel_rt::RubyValue::#wrapper(spinel_rt::#func(*__r, *__a))
                        }
                        (spinel_rt::RubyValue::Int(__r), spinel_rt::RubyValue::Float(__a)) => {
                            spinel_rt::RubyValue::#wrapper(spinel_rt::#func(*__r as f64, *__a))
                        }
                        (spinel_rt::RubyValue::Float(__r), spinel_rt::RubyValue::Int(__a)) => {
                            spinel_rt::RubyValue::#wrapper(spinel_rt::#func(*__r, *__a as f64))
                        }
                    }
                });
                // `<=>` isn't in `FLOAT_BINARY_OPS` (see its docs) -- a
                // `NaN` comparison returns `nil`, not an `Int`.
                let cmp_arm = (name == "<=>").then(|| {
                    quote! {
                        (spinel_rt::RubyValue::Float(__r), spinel_rt::RubyValue::Float(__a)) => {
                            match spinel_rt::float_cmp(*__r, *__a) {
                                Some(__n) => spinel_rt::RubyValue::Int(__n),
                                None => spinel_rt::RubyValue::Nil,
                            }
                        }
                        (spinel_rt::RubyValue::Int(__r), spinel_rt::RubyValue::Float(__a)) => {
                            match spinel_rt::float_cmp(*__r as f64, *__a) {
                                Some(__n) => spinel_rt::RubyValue::Int(__n),
                                None => spinel_rt::RubyValue::Nil,
                            }
                        }
                        (spinel_rt::RubyValue::Float(__r), spinel_rt::RubyValue::Int(__a)) => {
                            match spinel_rt::float_cmp(*__r, *__a as f64) {
                                Some(__n) => spinel_rt::RubyValue::Int(__n),
                                None => spinel_rt::RubyValue::Nil,
                            }
                        }
                    }
                });
                return quote! {
                    match (&(#recv_expr), &(#arg_expr)) {
                        #int_arm
                        #float_arm
                        #cmp_arm
                        _ => panic!("`{}` isn't supported yet for non-Int/Float operands at runtime (spike scope)", #name),
                    }
                };
            }
        }
        if args.is_empty() {
            let int_entry = INT_UNARY_OPS.iter().find(|(op, _)| *op == name);
            let float_entry = FLOAT_UNARY_OPS.iter().find(|(op, _)| *op == name);
            if int_entry.is_some() || float_entry.is_some() {
                let int_arm = int_entry.map(|&(_, rt_fn)| {
                    let func = format_ident!("{rt_fn}");
                    quote! {
                        spinel_rt::RubyValue::Int(__r) => spinel_rt::RubyValue::Int(spinel_rt::#func(*__r)),
                    }
                });
                let float_arm = float_entry.map(|&(_, rt_fn)| {
                    let func = format_ident!("{rt_fn}");
                    quote! {
                        spinel_rt::RubyValue::Float(__r) => spinel_rt::RubyValue::Float(spinel_rt::#func(*__r)),
                    }
                });
                return quote! {
                    match &(#recv_expr) {
                        #int_arm
                        #float_arm
                        _ => panic!("`{}` isn't supported yet for non-Int/Float operands at runtime (spike scope)", #name),
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
    // `spinel_rt::send` call `send`/`public_send`'s own Path-2 fallback
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
    // one of those (e.g. `Array#each`, genuinely unsupported -- see the
    // plan's Phase 3 scope-cut on Enumerable) should still hit the ordinary
    // "unsupported call" panic below, not attempt `.as_object_unchecked()`
    // on a value that was never an `Object` in the first place (confirmed
    // the hard way: an earlier, broader version of this check based on
    // `recv_class.is_none()` alone turned a clean "unsupported call" panic
    // for `[1,2,3].each { ... }` into a confusing "expected an Object, got
    // 1\n2\n3" one instead). `kwargs` has no Path 2 channel at all -- raise
    // the same clear error the `send`/`public_send` case above does, rather
    // than silently dropping it and dispatching without it. Found necessary
    // as a direct, small extension while building Phase 9 --
    // before this, ANY method call on a Poly-typed value was an
    // unconditional panic, which would have made a rescued exception's own
    // `message`/`to_s` uncallable via ordinary syntax.
    if infer(cx, recv_id) == TyKind::Poly {
        if !kwargs.is_empty() {
            panic!(
                "dynamic dispatch of `{name}` with keyword arguments isn't supported yet (spike scope): call it directly instead"
            );
        }
        let name_expr = quote! { spinel_rt::Symbol::intern(#name) };
        let arg_exprs = args.iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        let block_value = emit_block_option(cx, block, block_arg);
        return quote! {
            spinel_rt::catch_break(spinel_rt::send(
                &(#recv_expr).as_object_unchecked(),
                #name_expr,
                &[#(#arg_exprs),*],
                #block_value,
            ))?
        };
    }

    panic!("unsupported call `{name}` (spike scope, or receiver's class isn't statically known)");
}
