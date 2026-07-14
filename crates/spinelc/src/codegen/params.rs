//! Path 1 (static) parameter binding: the callee's Rust fn signature/
//! prologue (`emit_signature_params`/`emit_prologue`) and the caller's
//! argument-list construction (`emit_call_args`) for a method/block's
//! `Params`. Path 2 (dynamic `send`) binding lives in `dispatch.rs` --
//! spinelc-authored trampoline bodies, not this module (see its docs).
//!
//! Each `Param` kind maps to one concrete Rust parameter type: `required`/
//! `post` -> `RubyValue` (unchanged from before Params existed); `optional`/
//! keyword-optional -> `Option<RubyValue>`, shadowed in the callee's own
//! prologue by a `let mut` binding of the SAME Ruby name that unwraps it,
//! evaluating the default expression LAZILY (only when the caller passed
//! `None`) -- this preserves Ruby's own lazy-default-evaluation semantics
//! and lets a later body reassignment of that name go through the ordinary
//! hoisting-style plain-reassignment path (see `codegen::hoisting`'s docs);
//! named `rest`/`keyword_rest` -> a raw `Vec<RubyValue>`/`Vec<(Symbol,
//! RubyValue)>` Rust parameter, shadowed the same way into a real
//! `RubyValue::Array`/`RubyValue::Hash`. An ANONYMOUS `rest`/`keyword_rest`
//! (a bare `*`/`**` with no name) gets no Rust parameter at all -- nothing in
//! the Ruby body can ever reference it, so the caller simply evaluates (for
//! side effects) and discards those values instead of collecting them.

use quote::{format_ident, quote};

use super::expr::emit_expr;
use super::ident::safe_ident;
use super::Ctx;
use crate::hir::{HashPair, HirNode, KeywordParam, NodeId, Params};
use proc_macro2::TokenStream;

/// The callee's extra Rust fn parameters (after `&self`), one per `Params`
/// entry that gets a real Rust parameter (i.e. every kind except an
/// anonymous `rest`/`keyword_rest` -- see the module's docs).
pub fn emit_signature_params(params: &Params) -> TokenStream {
    let required = params.required.iter().map(|name| {
        let ident = safe_ident(name);
        quote! { , #ident: spinel_rt::RubyValue }
    });
    let optional = params.optional.iter().map(|(name, _)| {
        let ident = safe_ident(name);
        quote! { , #ident: Option<spinel_rt::RubyValue> }
    });
    let rest = params.rest.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! { , #ident: Vec<spinel_rt::RubyValue> }
    });
    let post = params.post.iter().map(|name| {
        let ident = safe_ident(name);
        quote! { , #ident: spinel_rt::RubyValue }
    });
    let keywords = params.keywords.iter().map(|kw| match kw {
        KeywordParam::Required(name) => {
            let ident = safe_ident(name);
            quote! { , #ident: spinel_rt::RubyValue }
        }
        KeywordParam::Optional(name, _) => {
            let ident = safe_ident(name);
            quote! { , #ident: Option<spinel_rt::RubyValue> }
        }
    });
    let keyword_rest = params.keyword_rest.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! { , #ident: Vec<(spinel_rt::Symbol, spinel_rt::RubyValue)> }
    });
    quote! { #(#required)* #(#optional)* #(#rest)* #(#post)* #(#keywords)* #(#keyword_rest)* }
}

/// The callee's own prologue: shadows every `Option<RubyValue>`/raw
/// collection Rust parameter with a `let mut` binding of the same Ruby name
/// holding a real `RubyValue` -- see the module's docs. Emitted in `Params`'
/// declared order so a later default expression can reference an earlier
/// parameter, matching Ruby's own rule that a default may only depend on
/// parameters declared before it.
pub fn emit_prologue(cx: &Ctx, params: &Params) -> TokenStream {
    let optional = params.optional.iter().map(|(name, default)| {
        emit_lazy_default_shadow(cx, name, *default)
    });
    let rest = params.rest.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! {
            #[allow(unused_mut)]
            let mut #ident: spinel_rt::RubyValue =
                spinel_rt::RubyValue::Array(spinel_rt::array_new(#ident));
        }
    });
    let keywords = params.keywords.iter().map(|kw| match kw {
        KeywordParam::Required(_) => quote! {},
        KeywordParam::Optional(name, default) => emit_lazy_default_shadow(cx, name, *default),
    });
    let keyword_rest = params.keyword_rest.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! {
            #[allow(unused_mut)]
            let mut #ident: spinel_rt::RubyValue = spinel_rt::RubyValue::Hash(spinel_rt::hash_new(
                #ident.into_iter().map(|(k, v)| (spinel_rt::RubyValue::Symbol(k), v)).collect()
            ));
        }
    });
    quote! { #(#optional)* #(#rest)* #(#keywords)* #(#keyword_rest)* }
}

fn emit_lazy_default_shadow(cx: &Ctx, name: &str, default: NodeId) -> TokenStream {
    let ident = safe_ident(name);
    let default_expr = emit_expr(cx, default);
    quote! {
        #[allow(unused_mut)]
        let mut #ident: spinel_rt::RubyValue = #ident.unwrap_or_else(|| #default_expr);
    }
}

/// The Path 1 call site's argument list for a call into a method/block
/// declaring `params`, given the call's positional `args` and keyword
/// `kwargs` (both in exact source order). Returns a full block expression
/// (`{ temporaries...; #recv_expr.method(...)? }`) rather than a plain
/// comma list: every argument expression -- positional AND keyword, in the
/// order actually WRITTEN at the call site -- is evaluated into a temporary
/// FIRST, because Ruby evaluates keyword argument values in written order,
/// which can differ from the callee's declared keyword order (`foo(y: b, x:
/// a)` evaluates `b` before `a`, even though the final call binds `x` to
/// `a`'s value and `y` to `b`'s) -- the temporaries let the final call
/// reorder freely without disturbing evaluation order or side effects.
///
/// `method_name` is only used for error messages. Arity/keyword-name
/// mismatches are clean compile-time panics (spinelc has full static
/// knowledge of both sides at a Path 1 call site, so this catches what real
/// Ruby would only raise `ArgumentError` for at runtime -- strictly better,
/// not a new restriction).
pub fn emit_call_args(
    cx: &Ctx,
    recv_expr: &TokenStream,
    method_name: &str,
    params: &Params,
    args: &[NodeId],
    kwargs: &[HashPair],
) -> TokenStream {
    // Fast path: a plain required-only callee with no call-site kwargs --
    // by far the common case, and everything Path 1 ever supported before
    // `Params` existed. Emit the exact same simple shape as before (no
    // temporaries, no block, no explicit arity check -- Rust's own
    // fixed-arity call already catches a mismatch) rather than paying for
    // the general machinery's temporaries/clones on every call.
    if params.optional.is_empty()
        && params.rest.is_none()
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && kwargs.is_empty()
    {
        let method_ident = safe_ident(method_name);
        let arg_exprs = args.iter().map(|&a| emit_expr(cx, a));
        return quote! { (#recv_expr).#method_ident(#(#arg_exprs),*)? };
    }

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let has_rest = params.rest.is_some();
    let min_positional = nreq + npost;

    if args.len() < min_positional {
        panic!(
            "too few arguments for `{method_name}` (spike scope): expected at least {min_positional}, got {}",
            args.len()
        );
    }
    let extra = args.len() - min_positional;
    if !has_rest && extra > nopt {
        panic!(
            "too many arguments for `{method_name}` (spike scope): expected at most {}, got {}",
            nreq + nopt + npost,
            args.len()
        );
    }
    let opt_bound = extra.min(nopt);
    let rest_count = extra - opt_bound;

    // Every positional arg gets a temporary, in source order, regardless of
    // which bucket (required/optional/rest/post) it ends up routed to.
    let pos_temps: Vec<syn::Ident> = (0..args.len()).map(|i| format_ident!("__a{i}")).collect();
    let pos_lets = args.iter().zip(&pos_temps).map(|(&a, t)| {
        let e = emit_expr(cx, a);
        quote! { let #t = #e; }
    });

    // Every kwarg value gets a temporary too, in source order. The key must
    // be a literal symbol (guaranteed by the call-site lowering's
    // `is_symbol_keys` check -- see `parse/mod.rs::lower_call_args`).
    let kw_temps: Vec<syn::Ident> = (0..kwargs.len()).map(|i| format_ident!("__kw{i}")).collect();
    let kw_lets = kwargs.iter().zip(&kw_temps).map(|(pair, t)| {
        let e = emit_expr(cx, pair.1);
        quote! { let #t = #e; }
    });
    let kw_names: Vec<String> = kwargs
        .iter()
        .map(|pair| match &cx.compiler.hir[pair.0] {
            HirNode::SymbolLit(s) => s.clone(),
            _ => panic!("`{method_name}`: keyword argument names must be literal symbols (spike scope)"),
        })
        .collect();
    let mut kw_used = vec![false; kwargs.len()];
    let find_kw = |name: &str, used: &mut Vec<bool>| -> Option<syn::Ident> {
        kw_names.iter().position(|n| n == name).map(|i| {
            used[i] = true;
            kw_temps[i].clone()
        })
    };

    let required_args = pos_temps[..nreq].iter().map(|t| quote! { #t.clone() });
    let optional_args = (0..nopt).map(|i| {
        if i < opt_bound {
            let t = &pos_temps[nreq + i];
            quote! { Some(#t.clone()) }
        } else {
            quote! { None }
        }
    });
    // NOTE: a trailing comma after the `vec![...]` (not a leading one before
    // it) -- this can be the very FIRST argument in the final call (e.g. a
    // method that's only `*rest`, with no required/optional before it), and
    // a leading `,` there would leave a dangling comma with nothing before
    // it (`method(, vec![...])`), invalid Rust. A trailing comma is always
    // safe regardless of position, matching `#(#required_args,)*`'s own
    // per-item-terminated style above.
    let rest_arg = params
        .rest
        .as_ref()
        .and_then(|r| r.as_ref())
        .map(|_| {
            let elems = pos_temps[nreq + opt_bound..nreq + opt_bound + rest_count]
                .iter()
                .map(|t| quote! { #t.clone() });
            quote! { vec![#(#elems),*], }
        })
        .unwrap_or_default();
    let post_args = pos_temps[nreq + opt_bound + rest_count..]
        .iter()
        .map(|t| quote! { #t.clone() });

    // Collected eagerly (not left as a lazy iterator): the closure above
    // mutably borrows `kw_used`, and `keyword_rest_arg`/the unmatched-
    // keyword check below both need to read it afterward.
    let keyword_args: Vec<TokenStream> = params
        .keywords
        .iter()
        .map(|kw| match kw {
            KeywordParam::Required(name) => {
                let t = find_kw(name, &mut kw_used).unwrap_or_else(|| {
                    panic!("missing required keyword argument `{name}:` for `{method_name}`")
                });
                quote! { #t.clone() }
            }
            KeywordParam::Optional(name, _) => match find_kw(name, &mut kw_used) {
                Some(t) => quote! { Some(#t.clone()) },
                None => quote! { None },
            },
        })
        .collect();
    let keyword_rest_arg = params
        .keyword_rest
        .as_ref()
        .and_then(|kr| kr.as_ref())
        .map(|_| {
            let leftover: Vec<TokenStream> = (0..kwargs.len())
                .filter(|&i| !kw_used[i])
                .map(|i| {
                    let name = &kw_names[i];
                    let t = &kw_temps[i];
                    quote! { (spinel_rt::Symbol::intern(#name), #t.clone()) }
                })
                .collect();
            quote! { vec![#(#leftover),*], }
        })
        .unwrap_or_default();
    // Any kwarg left unmatched with nowhere to go (no keyword_rest at all)
    // is an unknown-keyword error -- checked after the loop above so every
    // declared keyword gets a chance to claim its match first.
    if params.keyword_rest.is_none() {
        if let Some(i) = kw_used.iter().position(|&used| !used) {
            panic!(
                "unknown keyword argument `{}:` for `{method_name}`",
                kw_names[i]
            );
        }
    }

    let method_ident = safe_ident(method_name);
    quote! {
        {
            #(#pos_lets)*
            #(#kw_lets)*
            (#recv_expr).#method_ident(
                #(#required_args,)* #(#optional_args,)* #rest_arg #(#post_args,)* #(#keyword_args,)* #keyword_rest_arg
            )?
        }
    }
}

/// The Path 2 (dynamic `send`/`method_missing`) trampoline body for a
/// method declaring `params` -- a full closure expression spliced into the
/// `dispatch { name => expr }` section of a `ruby_class!` invocation (see
/// `spinel_rt::ruby_class!`'s docs for why the macro no longer DERIVES this
/// from the method's own Rust signature: an exact-length slice-pattern
/// match can't express "these two are optional" the way ordinary Rust
/// control flow can, so spinelc authors this once, here, using the same
/// `Params` info the Path 1 caller above already has).
///
/// Keyword parameters are NOT bound dynamically (documented spike scope-
/// cut, not an oversight): Path 2's calling convention is a bare positional
/// `&[RubyValue]` slice with no name information at all, unlike Path 1's
/// call sites, which always know the callee's declared keyword names
/// statically. A method declaring any keyword parameter still dispatches
/// fine via an ordinary direct (Path 1) call -- it simply isn't reachable
/// through `send`/`method_missing`/a non-literal `public_send`, mirroring
/// this project's existing "the method is still callable explicitly"
/// posture for other Path-2-only gaps.
pub fn emit_dynamic_trampoline(
    class_ident: &proc_macro2::Ident,
    method_name: &str,
    params: &Params,
) -> TokenStream {
    let method_ident = safe_ident(method_name);

    if !params.keywords.is_empty() || params.keyword_rest.is_some() {
        return quote! {
            |_recv: &spinel_rt::RObj, _args: &[spinel_rt::RubyValue]| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
                panic!(
                    "dynamic dispatch to `{}` isn't supported yet (spike scope): it declares keyword parameters, which `send`/`method_missing` don't bind yet -- call it directly instead",
                    #method_name
                )
            }
        };
    }

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let has_rest = params.rest.is_some();
    let min_lit = nreq + npost;
    // `args.len() < 0` is always false (a useless-comparison lint) when a
    // method has no required/post params -- each bound is built as an
    // `Option`, so a missing bound doesn't leave a dangling `||`, and the
    // whole check is skipped when NEITHER bound applies (no required/post
    // params and an unbounded rest).
    let min_cond = (min_lit > 0).then(|| quote! { args.len() < #min_lit });
    let max_cond = (!has_rest).then(|| {
        let max_lit = nreq + nopt + npost;
        quote! { args.len() > #max_lit }
    });
    let arity_check = match (min_cond, max_cond) {
        (None, None) => quote! {},
        (Some(a), None) => quote! { if #a { panic!("wrong number of arguments for {}", #method_name); } },
        (None, Some(b)) => quote! { if #b { panic!("wrong number of arguments for {}", #method_name); } },
        (Some(a), Some(b)) => {
            quote! { if #a || #b { panic!("wrong number of arguments for {}", #method_name); } }
        }
    };

    let required_args = (0..nreq).map(|i| quote! { args[#i].clone() });
    let optional_args = (0..nopt).map(|i| {
        let idx = nreq + i;
        quote! { if #i < __opt_bound { Some(args[#idx].clone()) } else { None } }
    });
    // Trailing comma (not leading) -- safe regardless of position, same
    // reasoning as `emit_call_args`'s `rest_arg`.
    let rest_arg = params.rest.iter().flatten().map(|_| {
        quote! { args[(#nreq + __opt_bound)..(args.len() - #npost)].to_vec(), }
    });
    let post_args = (0..npost).map(|i| quote! { args[args.len() - #npost + #i].clone() });

    quote! {
        |recv: &spinel_rt::RObj, args: &[spinel_rt::RubyValue]| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
            let this = recv.as_any().downcast_ref::<#class_ident>()
                .expect("class_id guarantees this downcast");
            #arity_check
            let __opt_bound = (args.len() - #min_lit).min(#nopt);
            #class_ident::#method_ident(
                this,
                #(#required_args,)* #(#optional_args,)* #(#rest_arg)* #(#post_args,)*
            )
        }
    }
}
