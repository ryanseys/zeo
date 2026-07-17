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

use super::expr::{box_if_object_typed, emit_expr};
use super::ident::safe_ident;
use super::Ctx;
use crate::hir::{KwArg, HirNode, KeywordParam, NodeId, Params};
use proc_macro2::TokenStream;

/// The callee's extra Rust fn parameters (after `&self`), one per `Params`
/// entry that gets a real Rust parameter (i.e. every kind except an
/// anonymous `rest`/`keyword_rest` -- see the module's docs). `needs_block`
/// (see `compiler::Scope::needs_block_param`) appends the implicit trailing
/// `__blk: Option<RubyValue>` slot every method using `&block`/bare
/// `yield`/`block_given?` gets.
pub fn emit_signature_params(params: &Params, needs_block: bool) -> TokenStream {
    let items = signature_param_items(params, needs_block);
    quote! { #(, #items)* }
}

/// `emit_signature_params` for a RECEIVERLESS free function (a class
/// method/module function) -- the same items, comma-SEPARATED rather than
/// comma-prefixed (nothing precedes them in the signature).
pub fn emit_signature_params_free(params: &Params, needs_block: bool) -> TokenStream {
    let items = signature_param_items(params, needs_block);
    quote! { #(#items),* }
}

fn signature_param_items(params: &Params, needs_block: bool) -> Vec<TokenStream> {
    let mut items = Vec::new();
    for name in &params.required {
        let ident = safe_ident(name);
        items.push(quote! { #ident: spinel_rt::RubyValue });
    }
    for (name, _) in &params.optional {
        let ident = safe_ident(name);
        items.push(quote! { #ident: Option<spinel_rt::RubyValue> });
    }
    if let Some(Some(name)) = &params.rest {
        let ident = safe_ident(name);
        items.push(quote! { #ident: Vec<spinel_rt::RubyValue> });
    }
    for name in &params.post {
        let ident = safe_ident(name);
        items.push(quote! { #ident: spinel_rt::RubyValue });
    }
    for kw in &params.keywords {
        match kw {
            KeywordParam::Required(name) => {
                let ident = safe_ident(name);
                items.push(quote! { #ident: spinel_rt::RubyValue });
            }
            KeywordParam::Optional(name, _) => {
                let ident = safe_ident(name);
                items.push(quote! { #ident: Option<spinel_rt::RubyValue> });
            }
        }
    }
    if let Some(Some(name)) = &params.keyword_rest {
        let ident = safe_ident(name);
        items.push(quote! { #ident: Vec<(spinel_rt::Symbol, spinel_rt::RubyValue)> });
    }
    if needs_block {
        items.push(quote! { __blk: Option<spinel_rt::RubyValue> });
    }
    items
}

/// The callee's own prologue: shadows every `Option<RubyValue>`/raw
/// collection Rust parameter with a `let mut` binding of the same Ruby name
/// holding a real `RubyValue` -- see the module's docs. Emitted in `Params`'
/// declared order so a later default expression can reference an earlier
/// parameter, matching Ruby's own rule that a default may only depend on
/// parameters declared before it. A named `&blk` shadows `__blk` into an
/// ordinary `RubyValue` local (`Nil` when unyielded, matching real Ruby --
/// NOT absent, unlike every other kind's `None`-becomes-a-default rule).
/// Finally, ANY of this method's own parameter names that some escaping
/// block inside its body captures (see `codegen::captures`) get one more
/// wrapping shadow into `Arc<parking_lot::Mutex<RubyValue>>` -- captured
/// method PARAMETERS need this too, not just captured plain locals, since
/// `codegen::hoisting`'s prelude only ever sees names `collect_locals` finds
/// (which never includes a method's own params -- they're bound via the
/// Rust fn signature, not that prelude).
/// The parenthesized-destructuring params (`|a, (b, c)|`), replayed as the
/// ordinary multi-assignments they are: each slot was bound under an internal
/// name by the normal positional rules, and this splits that value into the
/// nested names. Emitted by BOTH param-binding sites (methods' `emit_prologue`
/// and blocks' `emit_proc_param_bindings`) immediately after the slots are
/// bound and before the body runs. See `hir::Params::destructures`.
///
/// Empty for the overwhelming majority of params lists, so this costs
/// nothing when unused.
///
/// The nested names are DECLARED here as well as assigned. They are
/// parameters (`Params::bound_names` lists them), and `bound_names` is
/// precisely hoisting's exclusion set -- a method's ordinary params need no
/// declaration because the Rust fn signature binds them, but a destructured
/// name has no signature slot of its own (only the `__destr_<i>` slot does),
/// so nothing else would ever declare it. Declaration goes through
/// `local_storage` rather than always emitting `let mut`: a destructured name
/// captured by an escaping block needs the `Arc<Mutex<..>>` cell every other
/// captured local gets, and `emit_local_write` will emit a cell-write for it.
fn emit_destructures(cx: &Ctx, params: &Params) -> TokenStream {
    if params.destructures.is_empty() {
        return quote! {};
    }
    let decls = params
        .destructured_names()
        .into_iter()
        .map(|n| super::hoisting::emit_local_decl(cx, &n))
        .collect::<Vec<_>>();
    let writes = params
        .destructures
        .iter()
        .map(|(read, group)| super::loops::emit_multi_write(cx, group, *read));
    quote! { #(#decls)* #(#writes)* }
}

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
    // `params.block` being `Some(Some(name))` already implies `needs_block`
    // (see `Scope::needs_block_param`), so no extra gate is needed here --
    // `.iter().flatten()` alone (same idiom as `rest`/`keyword_rest` above)
    // naturally yields nothing for `None`/an anonymous `&`.
    let block = params.block.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! {
            #[allow(unused_mut)]
            let mut #ident: spinel_rt::RubyValue = __blk.clone().unwrap_or(spinel_rt::RubyValue::Nil);
        }
    });
    // Sorted for deterministic codegen output -- a `HashSet`'s iteration
    // order is otherwise arbitrary, and these `let`s are independent (order
    // among themselves doesn't affect behavior), but reproducible output is
    // still worth having.
    let own_names = super::captures::own_param_names(params);
    // Destructured names are excluded: this wrap turns a by-value Rust
    // PARAMETER into its capture cell, and a destructured name has no
    // parameter to turn -- only its `__destr_<i>` slot does.
    // `emit_destructures` already declares each one directly at its storage
    // class (cell included), so wrapping here would wrap the cell in a cell.
    let destructured: std::collections::HashSet<String> =
        params.destructured_names().into_iter().collect();
    let mut captured_params: Vec<&String> = own_names
        .iter()
        .filter(|name| cx.captured_locals.contains(*name) && !destructured.contains(*name))
        .collect();
    captured_params.sort();
    let captured_param_wraps = captured_params.into_iter().map(|name| {
        let ident = safe_ident(name);
        quote! {
            let #ident: std::sync::Arc<spinel_rt::parking_lot::Mutex<spinel_rt::RubyValue>> =
                std::sync::Arc::new(spinel_rt::parking_lot::Mutex::new(#ident));
        }
    });
    // Destructures split the slots bound above, so they come after those --
    // but BEFORE `captured_param_wraps`, which reads each captured param's
    // current value into its cell. A `__destr_<i>` slot wrapped before being
    // split would leave the split reading a cell as if it were a value.
    // (The destructured NAMES aren't affected: `emit_destructures` declares
    // each one at its own storage class, cell included.)
    let destructures = emit_destructures(cx, params);
    quote! { #(#optional)* #(#rest)* #(#keywords)* #(#keyword_rest)* #(#block)* #destructures #(#captured_param_wraps)* }
}

fn emit_lazy_default_shadow(cx: &Ctx, name: &str, default: NodeId) -> TokenStream {
    let ident = safe_ident(name);
    let default_expr = {
        let e = emit_expr(cx, default);
        // A default may itself be Object-typed (`def m(o = Widget.new)`).
        super::expr::box_if_object_typed(cx, default, e)
    };
    // `match`, not `unwrap_or_else(|| ...)`: a fallible default expression
    // (`def m(a = some_call)`) contains a `?`, which must propagate through
    // the ENCLOSING method body, not a helper closure that returns a plain
    // `RubyValue` (a real rustc E0277 found by the conformance corpus).
    quote! {
        #[allow(unused_mut)]
        let mut #ident: spinel_rt::RubyValue = match #ident {
            Some(__v) => __v,
            None => #default_expr,
        };
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
// Every one of these is a genuinely distinct piece of the callee's shape
// (receiver expr/name/params/call-site args-kwargs-block-block_arg/whether
// a block channel exists at all) -- same reasoning as `codegen::call`'s
// `emit_call`/`dispatch`.
#[allow(clippy::too_many_arguments)]
pub fn emit_call_args(
    cx: &Ctx,
    recv_expr: &TokenStream,
    method_name: &str,
    params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    needs_block: bool,
) -> TokenStream {
    emit_call_args_to(
        cx,
        &Callee::Method(recv_expr.clone()),
        method_name,
        params,
        args,
        kwargs,
        block,
        block_arg,
        needs_block,
    )
}

/// How `emit_call_args_to` spells the actual invocation once the argument
/// list is built -- ordinary Path-1 method syntax, or a builtin-reopen FREE
/// FUNCTION (Phase 16.3), whose receiver `RubyValue` is passed as the
/// generated `__self` first argument instead of a method receiver.
pub enum Callee {
    Method(TokenStream),
    FreeFn { path: TokenStream, recv: TokenStream },
    /// A receiverless free function -- a class method/module function
    /// (`Widget::create(...)`), which has no `self`/`__self` parameter.
    Bare(TokenStream),
}

impl Callee {
    fn invoke(&self, method_name: &str, arg_list: TokenStream) -> TokenStream {
        let method_ident = safe_ident(method_name);
        match self {
            Callee::Method(recv) => quote! { (#recv).#method_ident(#arg_list) },
            // No parens around `#recv` (a call ARGUMENT position needs
            // none -- rustc lints them as unnecessary); the trailing comma
            // after it is always valid call syntax, whether `arg_list` is
            // empty or not.
            Callee::FreeFn { path, recv } => quote! { #path(#recv, #arg_list) },
            Callee::Bare(path) => quote! { #path(#arg_list) },
        }
    }
}

/// `emit_call_args`, generalized over the invocation shape (see `Callee`).
#[allow(clippy::too_many_arguments)]
pub fn emit_call_args_to(
    cx: &Ctx,
    callee: &Callee,
    method_name: &str,
    params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    needs_block: bool,
) -> TokenStream {
    // Fast path: a plain required-only callee called with the RIGHT number
    // of arguments, no call-site kwargs and no block channel -- by far the
    // common case, and everything Path 1 ever supported before `Params`
    // existed. Emit the exact same simple shape as before (no temporaries,
    // no explicit arity check). A wrong-arity call deliberately FALLS
    // THROUGH to the general machinery below, whose count check raises a
    // runtime ArgumentError -- CRuby's behavior (dead wrong-arity code
    // stays silent); letting Rust's own fixed-arity call catch it would be
    // a rustc error on the whole generated program instead.
    if params.optional.is_empty()
        && params.rest.is_none()
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && kwargs.is_empty()
        && !needs_block
        && args.len() == params.required.len()
    {
        // Boxed via `box_if_object_typed`: the callee's own Rust parameter
        // type is always plain `RubyValue` (an ordinary parameter is never
        // inferred `TyKind::Object` -- see that function's docs), but an
        // Object-typed ARGUMENT expression (a `New`/`Shadowed`-local-read/
        // `SelfRef`) emits a bare, unboxed `Arc<Concrete>` -- a real `rustc`
        // type mismatch at the call site otherwise, confirmed by direct
        // reproduction (passing an object instance as a plain argument).
        let arg_exprs = args.iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        let call = callee.invoke(method_name, quote! { #(#arg_exprs),* });
        return quote! { #call? };
    }

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let has_rest = params.rest.is_some();

    // Ruby's keywords-to-positional-hash conversion: when the callee
    // declares NO keyword parameters AND no `**kwrest`, trailing keywords at
    // the call site are not keywords at all -- they become one positional
    // Hash. That is what makes the classic options-hash idiom work, and it
    // is still true in Ruby 3+ (oracle-verified):
    //
    //     def m(opts = {}) = opts
    //     m(a: 1, b: 2)   # => {a: 1, b: 2}, bound to `opts`
    //
    // Without this the keyword-binding path below rejects them as unknown
    // keywords -- `m(a: 1)` raised `unknown keyword: :a` for a method that
    // has none to be unknown. A callee that DOES declare keywords keeps the
    // real check (`k(x: 1, zz: 2)` is still `unknown keyword: :zz`).
    let kw_as_positional =
        params.keywords.is_empty() && params.keyword_rest.is_none() && !kwargs.is_empty();
    let (kwargs, kw_hash_arg) = if kw_as_positional {
        (&[][..], Some(super::collections::emit_hash_lit(cx, kwargs)))
    } else {
        (kwargs, None)
    };
    // The converted hash counts as an ordinary positional from here on --
    // for arity checking as much as for binding.
    let n_pos = args.len() + usize::from(kw_hash_arg.is_some());
    let min_positional = nreq + npost;

    // Every positional arg gets a temporary, in source order, regardless of
    // which bucket (required/optional/rest/post) it ends up routed to.
    // Built BEFORE the arity/keyword checks below: a definitely-wrong call
    // still evaluates its arguments (for side effects) and then raises a
    // runtime ArgumentError, CRuby's exact behavior -- never a compile
    // panic (`rescue ArgumentError` around a bad call is a corpus idiom).
    let pos_temps: Vec<syn::Ident> = (0..n_pos).map(|i| format_ident!("__a{i}")).collect();
    let pos_lets: Vec<TokenStream> = args
        .iter()
        .zip(&pos_temps)
        .map(|(&a, t)| {
            let e = emit_expr(cx, a);
            let e = box_if_object_typed(cx, a, e);
            quote! { let #t = #e; }
        })
        // The converted hash is built last, matching its source position as
        // the trailing argument.
        .chain(kw_hash_arg.into_iter().map(|h| {
            let t = &pos_temps[args.len()];
            quote! { let #t = #h; }
        }))
        .collect();

    // Every kwarg value gets a temporary too, in source order. The key must
    // be a literal symbol (guaranteed by the call-site lowering's
    // `is_symbol_keys` check -- see `parse/mod.rs::lower_call_args`).
    // Post-routing, every kwarg here is a literal `Pair` -- `emit_call`'s
    // guard already sent any `**h` `DoubleSplat` to `emit_splat_call`.
    let kw_temps: Vec<syn::Ident> = (0..kwargs.len()).map(|i| format_ident!("__kw{i}")).collect();
    let kw_lets: Vec<TokenStream> = kwargs
        .iter()
        .zip(&kw_temps)
        .map(|(kw, t)| {
            let KwArg::Pair(_, v) = kw else {
                unreachable!("a `**` double-splat routes to emit_splat_call, never Path 1")
            };
            let e = emit_expr(cx, *v);
            let e = box_if_object_typed(cx, *v, e);
            quote! { let #t = #e; }
        })
        .collect();
    let kw_names: Vec<String> = kwargs
        .iter()
        .map(|kw| {
            let KwArg::Pair(k, _) = kw else {
                unreachable!("a `**` double-splat routes to emit_splat_call, never Path 1")
            };
            match &cx.compiler.hir[*k] {
                HirNode::SymbolLit(s) => s.clone(),
                _ => panic!("`{method_name}`: keyword argument names must be literal symbols (spike scope)"),
            }
        })
        .collect();

    // A statically-detectable argument-shape error: evaluate the arg
    // temporaries, then raise -- see the comment above `pos_temps`.
    let raise_argument_error = |msg: String| {
        quote! {
            {
                #(#pos_lets)*
                #(#kw_lets)*
                return Err(spinel_rt::raise_error("ArgumentError", #msg.to_string()));
            }
        }
    };
    let expected_shape = || {
        if has_rest {
            format!("{min_positional}+")
        } else if nopt > 0 {
            format!("{min_positional}..{}", nreq + nopt + npost)
        } else {
            format!("{min_positional}")
        }
    };

    if n_pos < min_positional || (!has_rest && n_pos > nreq + nopt + npost) {
        return raise_argument_error(format!(
            "wrong number of arguments (given {}, expected {})",
            n_pos,
            expected_shape()
        ));
    }
    for kw in &params.keywords {
        if let KeywordParam::Required(name) = kw {
            if !kw_names.iter().any(|n| n == name) {
                return raise_argument_error(format!("missing keyword: :{name}"));
            }
        }
    }
    if params.keyword_rest.is_none() {
        let declared = |n: &String| {
            params.keywords.iter().any(|kw| match kw {
                KeywordParam::Required(k) | KeywordParam::Optional(k, _) => k == n,
            })
        };
        if let Some(unknown) = kw_names.iter().find(|n| !declared(n)) {
            return raise_argument_error(format!("unknown keyword: :{unknown}"));
        }
    }

    let extra = n_pos - min_positional;
    let opt_bound = extra.min(nopt);
    let rest_count = extra - opt_bound;

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
                let t = find_kw(name, &mut kw_used)
                    .expect("missing required keywords already raised above");
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
    // Unknown keywords already raised above (before `find_kw` ran); with a
    // keyword_rest, every unmatched kwarg lands in `keyword_rest_arg`.

    // The implicit trailing block argument (see `compiler::Scope::needs_block_param`).
    // If the callee doesn't accept a block at all (`!needs_block`), nothing
    // is built even if one was written at this call site -- an unused block
    // is simply never invoked, matching real Ruby (it's always legal to
    // pass a block a method ignores).
    let block_arg_value = if needs_block {
        let v = super::call::emit_block_option(cx, block, block_arg);
        quote! { #v, }
    } else {
        TokenStream::new()
    };

    let call_expr = callee.invoke(
        method_name,
        quote! {
            #(#required_args,)* #(#optional_args,)* #rest_arg #(#post_args,)* #(#keyword_args,)* #keyword_rest_arg #block_arg_value
        },
    );
    // `catch_break` only matters when the callee might actually invoke a
    // block (see its docs) -- gated on `needs_block` rather than applied
    // unconditionally, purely to avoid the extra match on every ordinary
    // call.
    let dispatched = if needs_block {
        quote! { spinel_rt::catch_break(#call_expr)? }
    } else {
        quote! { #call_expr? }
    };
    quote! {
        {
            #(#pos_lets)*
            #(#kw_lets)*
            #dispatched
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
/// The G2 trailing-kwargs-hash convention's callee side: for a
/// keyword-declaring `params`, a preamble that splits/binds the trailing
/// Hash off `args` (via `spinel_rt::bind_dynamic_kwargs`, shadowing `args`
/// with the remaining positionals) plus the keyword/kwrest argument tokens
/// in DECLARED order (the Rust signature's order). Empty for a
/// keywordless callee.
fn dynamic_kwargs_binding(
    method_name: &str,
    params: &Params,
) -> (Option<TokenStream>, Vec<TokenStream>) {
    if params.keywords.is_empty() && params.keyword_rest.is_none() {
        return (None, Vec::new());
    }
    let req_names: Vec<&str> = params
        .keywords
        .iter()
        .filter_map(|kw| match kw {
            KeywordParam::Required(n) => Some(n.as_str()),
            KeywordParam::Optional(..) => None,
        })
        .collect();
    let opt_names: Vec<&str> = params
        .keywords
        .iter()
        .filter_map(|kw| match kw {
            KeywordParam::Optional(n, _) => Some(n.as_str()),
            KeywordParam::Required(_) => None,
        })
        .collect();
    let has_kwrest = params.keyword_rest.is_some();
    let preamble = quote! {
        #[allow(unused_mut, unused_variables)]
        let (args, __kw_req, __kw_opt, mut __kw_rest) = spinel_rt::bind_dynamic_kwargs(
            #method_name,
            args,
            &[#(#req_names),*],
            &[#(#opt_names),*],
            #has_kwrest,
        )?;
    };
    // Keyword arguments in DECLARED order (matching
    // `emit_signature_params`), picking from the binder's required/optional
    // vectors with independent counters.
    let (mut ri, mut oi) = (0usize, 0usize);
    let mut kw_args: Vec<TokenStream> = params
        .keywords
        .iter()
        .map(|kw| match kw {
            KeywordParam::Required(_) => {
                let i = ri;
                ri += 1;
                quote! { __kw_req[#i].clone() }
            }
            KeywordParam::Optional(..) => {
                let i = oi;
                oi += 1;
                quote! { __kw_opt[#i].clone() }
            }
        })
        .collect();
    if has_kwrest {
        kw_args.push(quote! { std::mem::take(&mut __kw_rest) });
    }
    (Some(preamble), kw_args)
}

/// The rescuable-`ArgumentError` arity guard shared by both dynamic-dispatch
/// trampolines (`emit_dynamic_trampoline`/`emit_value_trampoline`). Ruby
/// resolves arity at RUNTIME and the error is rescuable, so this emits a
/// `return Err(raise_error("ArgumentError", …))` -- NOT a `panic!`, which
/// would abort the process where a `rescue ArgumentError` should catch it.
///
/// Emits nothing when the method accepts any count (no required/post params
/// and an unbounded rest). The compile-time `expected` description matches
/// CRuby's exact shapes (oracle-verified against ruby 4.0.5): `N` fixed,
/// `N..M` bounded range, `N+` when a rest param leaves the upper bound open.
/// Both trampolines build their argument bindings identically below, so the
/// `Option`-built bounds keep a missing lower/upper bound from emitting a
/// dangling `||` (an `args.len() < 0` useless-comparison lint).
fn emit_arity_check(params: &Params) -> TokenStream {
    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let has_rest = params.rest.is_some();
    let min_lit = nreq + npost;
    let min_cond = (min_lit > 0).then(|| quote! { args.len() < #min_lit });
    let max_cond = (!has_rest).then(|| {
        let max_lit = nreq + nopt + npost;
        quote! { args.len() > #max_lit }
    });
    let cond = match (min_cond, max_cond) {
        (None, None) => return quote! {},
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (Some(a), Some(b)) => quote! { #a || #b },
    };
    let expected = if has_rest {
        format!("{min_lit}+")
    } else if nopt == 0 {
        format!("{min_lit}")
    } else {
        format!("{min_lit}..{}", nreq + nopt + npost)
    };
    quote! {
        if #cond {
            return Err(spinel_rt::raise_error(
                "ArgumentError",
                format!("wrong number of arguments (given {}, expected {})", args.len(), #expected),
            ));
        }
    }
}

pub fn emit_dynamic_trampoline(
    class_ident: &proc_macro2::Ident,
    method_name: &str,
    params: &Params,
    needs_block: bool,
) -> TokenStream {
    let method_ident = safe_ident(method_name);
    let (kw_preamble, kw_args) = dynamic_kwargs_binding(method_name, params);

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let min_lit = nreq + npost;
    let arity_check = emit_arity_check(params);

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
    // Trailing comma, same reasoning as `rest_arg` -- safe regardless of
    // position (and here, whether anything precedes it at all). The
    // closure's own `blk` parameter is prefixed `_` when unused (not just
    // when `needs_block` is false but ALSO named `_blk` there), matching
    // every other unused-parameter convention in this codebase and avoiding
    // a spurious unused-variable warning in the generated program.
    let blk_ident = if needs_block { format_ident!("blk") } else { format_ident!("_blk") };
    let block_arg = needs_block.then(|| quote! { blk, });

    quote! {
        |recv: &spinel_rt::RObj, args: &[spinel_rt::RubyValue], #blk_ident: Option<spinel_rt::RubyValue>| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
            let this = spinel_rt::downcast_robj::<#class_ident>(recv)
                .expect("class_id guarantees this downcast");
            #kw_preamble
            #arity_check
            let __opt_bound = (args.len() - #min_lit).min(#nopt);
            #class_ident::#method_ident(
                this,
                #(#required_args,)* #(#optional_args,)* #(#rest_arg)* #(#post_args,)* #(#kw_args,)* #block_arg
            )
        }
    }
}

/// Whether a `ValueMethodFn` trampoline forwards its receiver into the
/// free function it wraps -- the one structural difference between the two
/// kinds of free function generated code produces.
#[derive(Clone, Copy, PartialEq)]
pub enum RecvMode {
    /// A BUILTIN-REOPEN instance method: the receiver is the builtin
    /// `RubyValue` itself, cloned into the function's `__self` first
    /// parameter (`emit_signature_params`' free shape).
    Pass,
    /// A CLASS method (`def self.x`): the receiver is the class object, and
    /// the emitted function takes no receiver parameter at all
    /// (`emit_signature_params_free`) -- which class it belongs to is
    /// already baked into the function's own path and its `Ctx::class_self`.
    /// Forwarding `recv` here would be an arity mismatch on the generated
    /// program.
    Drop,
}

/// The Path 2 trampoline for a BUILTIN-REOPEN method (Phase 16.3) or a user
/// CLASS method -- the `ValueMethodFn`-shaped counterpart of
/// `emit_dynamic_trampoline` above (same arity checking, same
/// keyword-parameter scope-cut), minus the `RObj` downcast. See `RecvMode`
/// for the receiver difference. Registered from generated `main()` via
/// `ClassRegistry::define_value_method`/`define_class_method`.
pub fn emit_value_trampoline(
    fn_path: &TokenStream,
    method_name: &str,
    params: &Params,
    needs_block: bool,
    recv_mode: RecvMode,
) -> TokenStream {
    let (kw_preamble, kw_args) = dynamic_kwargs_binding(method_name, params);

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let min_lit = nreq + npost;
    let arity_check = emit_arity_check(params);

    let required_args = (0..nreq).map(|i| quote! { args[#i].clone() });
    let optional_args = (0..nopt).map(|i| {
        let idx = nreq + i;
        quote! { if #i < __opt_bound { Some(args[#idx].clone()) } else { None } }
    });
    let rest_arg = params.rest.iter().flatten().map(|_| {
        quote! { args[(#nreq + __opt_bound)..(args.len() - #npost)].to_vec(), }
    });
    let post_args = (0..npost).map(|i| quote! { args[args.len() - #npost + #i].clone() });
    let blk_ident = if needs_block { format_ident!("blk") } else { format_ident!("_blk") };
    let block_arg = needs_block.then(|| quote! { blk, });
    let (recv_ident, recv_arg) = match recv_mode {
        RecvMode::Pass => (format_ident!("recv"), Some(quote! { recv.clone(), })),
        RecvMode::Drop => (format_ident!("_recv"), None),
    };

    quote! {
        |#recv_ident: &spinel_rt::RubyValue, args: &[spinel_rt::RubyValue], #blk_ident: Option<spinel_rt::RubyValue>| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
            #kw_preamble
            #arity_check
            #[allow(unused_variables)]
            let __opt_bound = (args.len() - #min_lit).min(#nopt);
            #fn_path(
                #recv_arg
                #(#required_args,)* #(#optional_args,)* #(#rest_arg)* #(#post_args,)* #(#kw_args,)* #block_arg
            )
        }
    }
}

/// `Proc#arity` for a block/lambda with these parameters -- CRuby's
/// `rb_proc_arity`/`rb_iseq_min_max_arity` (proc.c), evaluated at compile
/// time and baked into the constructed `RProc` (a Rust closure can't
/// answer this about itself; see `spinel_rt::ProcData`).
///
/// ```text
/// min = required + post + (1 if any REQUIRED keyword)
/// max = UNLIMITED if *rest, else required + optional + post + (1 if any keyword/**kwrest)
/// arity = min             when (lambda ? min == max : max != UNLIMITED)
///       = -min - 1        otherwise
/// ```
///
/// Verified against ruby 4.0.5 across 24 signatures -- including the two
/// shapes that make the lambda/proc distinction visible:
/// `->(a, b = 1) {}.arity == -2` but `proc { |a, b = 1| }.arity == 1`.
pub(super) fn proc_arity(params: &Params, is_lambda: bool) -> i32 {
    let lead = params.required.len() as i32;
    let opt = params.optional.len() as i32;
    let post = params.post.len() as i32;
    let has_rest = params.rest.is_some();
    let has_kw = !params.keywords.is_empty();
    let has_kwrest = params.keyword_rest.is_some();
    let any_required_kw = params
        .keywords
        .iter()
        .any(|k| matches!(k, KeywordParam::Required(_)));

    let min = lead + post + i32::from(any_required_kw);
    let max = (!has_rest).then(|| lead + opt + post + i32::from(has_kw || has_kwrest));
    let positive = match max {
        Some(max) if is_lambda => min == max,
        Some(_) => true,
        None => false,
    };
    if positive {
        min
    } else {
        -min - 1
    }
}

/// Whether a (non-lambda) block with these parameters auto-splats a lone
/// Array argument across its positional slots.
///
/// Two independent triggers, either of which splats, unless the
/// `ambiguous_param0` exemption applies:
///   - `lead + post > 0` -- the block demands at least one positional by
///     position, so a lone Array is spread to feed it;
///   - `opt > 1` -- more than one optional slot, even with no required ones.
///
/// `ambiguous_param0` is the `{ |a| }` exemption: exactly one lead param and
/// nothing else positional. `|a|` must receive the array WHOLE (it is the
/// `each { |pair| }` idiom), so it never splats. Keyword params don't disturb
/// it (`|a, **k|` is still exempt), but a rest or post does (`|a, *b|` splats).
///
/// Oracle-derived, not recalled from CRuby's source -- every row below was
/// read off `ruby 4.0.5` via `def m(x); yield x; end; m([1,2]) { |...| }`:
///
/// ```text
/// |a|             -> no    ambiguous_param0
/// |a, **k|        -> no    ambiguous_param0 (keywords don't disturb it)
/// |a, b: 9|       -> no    ambiguous_param0
/// |a = 9|         -> no    lead+post == 0, opt == 1
/// |*a|            -> no    lead+post == 0, opt == 0
/// |*a, **k|       -> no
/// |a = 5, *b|     -> no    lead+post == 0, opt == 1  (the subtle one)
/// |a, b|          -> yes   lead == 2
/// |a, *b|         -> yes   lead == 1, rest breaks the exemption
/// |a, *b, **k|    -> yes
/// |a, *b, c|      -> yes
/// |a = 5, b = 4|  -> yes   opt > 1, with NO lead at all
/// |a = 1, b = 2, **k| -> yes
/// |a = 5, b = 4, *c|  -> yes   opt > 1
/// |a = 5, *b, c|  -> yes   post > 0
/// |*a, b|         -> yes   post > 0
/// |*a, b, c|      -> yes
/// |a, |           -> yes   trailing comma == an anonymous rest, which
///                          breaks the exemption exactly like `*b` does
/// ```
///
/// The pair `|a, *b|` (splats) vs `|a = 5, *b|` (does not) is what rules out
/// the tempting "count the positional slots" formulation: both have one
/// nameable slot plus a rest, and they disagree.
fn auto_splats(params: &Params) -> bool {
    let lead = params.required.len();
    let opt = params.optional.len();
    let post = params.post.len();
    let ambiguous_param0 = lead == 1 && opt == 0 && post == 0 && params.rest.is_none();
    !ambiguous_param0 && (lead + post > 0 || opt > 1)
}

/// A real escaping block's OWN parameter binding, given its `Params` and the
/// closure's own runtime `&[RubyValue]` argument slice (`args_ident`) --
/// used by `codegen::call`'s Proc-construction site (decision 4: a block
/// gets FULL `Params` support, not just plain required params). Unlike
/// `emit_dynamic_trampoline`'s STRICT arity checking (correct for a real
/// method call), this binds LENIENTLY: missing positional values become
/// `Nil`, extra ones are silently dropped -- real Ruby's own block-arity
/// leniency, not a method call's. All arithmetic here is RUNTIME (the
/// closure's argument count isn't known until it's actually invoked, unlike
/// Path 1's call sites, which know `args.len()` at spinelc-compile-time).
///
/// Keyword params on a block are a real, if rare, Ruby feature -- bound by
/// treating the LAST yielded positional value as the keyword source when
/// it's a `Hash` (mirroring real Ruby's own auto-conversion of a trailing
/// Hash argument into block keywords). Missing keywords (required or
/// optional) fall back to `Nil`/the default, never a panic -- and "does the
/// key exist" is approximated as "is its value non-nil" (`hash_get` has no
/// separate presence check), a documented imprecision, same posture as this
/// codebase's other narrower-than-real-Ruby approximations.
///
/// AUTO-SPLAT (`is_lambda: false` only -- see `auto_splats`): decided here,
/// statically, from the block's own parameter shape; performed by
/// `spinel_rt::block_auto_splat` at invocation time. The split of a
/// trailing Hash into `__kw_source` happens BEFORE it, matching CRuby: a
/// block `|a, b, **k|` yielded one `[1, {x: 9}]` array binds `b = {x: 9}`
/// (Ruby 3 has no implicit hash-to-keywords conversion), not `k = {x: 9}`.
pub fn emit_proc_param_bindings(
    cx: &Ctx,
    params: &Params,
    args_ident: &proc_macro2::Ident,
    is_lambda: bool,
) -> TokenStream {
    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let has_rest = params.rest.is_some();
    let has_keywords = !params.keywords.is_empty() || params.keyword_rest.is_some();

    let positional_and_kw_source = if has_keywords {
        quote! {
            let (__positional, __kw_source): (Vec<spinel_rt::RubyValue>, Option<spinel_rt::RubyValue>) =
                match #args_ident.last() {
                    Some(__v @ spinel_rt::RubyValue::Hash(_)) => {
                        (#args_ident[..#args_ident.len() - 1].to_vec(), Some(__v.clone()))
                    }
                    _ => (#args_ident.to_vec(), None),
                };
        }
    } else {
        quote! {
            let __positional: Vec<spinel_rt::RubyValue> = #args_ident.to_vec();
        }
    };
    let auto_splat = (!is_lambda && auto_splats(params)).then(|| {
        quote! {
            let __positional = spinel_rt::block_auto_splat(__positional)?;
        }
    });

    let required_lets = params.required.iter().enumerate().map(|(i, name)| {
        let ident = safe_ident(name);
        quote! {
            // `allow(unused_variables)`: a block legitimately declares a
            // param its body never reads (`each { |x| n += 1 }` counting
            // elements) -- rustc's lint isn't a Ruby-visible concern.
            // `mut`: a Ruby block param is an ordinary reassignable local
            // (`upto(3) { |n| n = n + 1 }`).
            #[allow(unused_variables, unused_mut)]
            let mut #ident: spinel_rt::RubyValue = __positional.get(#i).cloned().unwrap_or(spinel_rt::RubyValue::Nil);
        }
    });
    let optional_lets = params.optional.iter().enumerate().map(|(i, (name, default))| {
        let ident = safe_ident(name);
        let default_expr = emit_expr(cx, *default);
        quote! {
            #[allow(unused_mut)]
            let mut #ident: spinel_rt::RubyValue = if #i < __opt_bound {
                __positional.get(#nreq + #i).cloned().unwrap_or(spinel_rt::RubyValue::Nil)
            } else {
                #default_expr
            };
        }
    });
    let rest_let = params.rest.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! {
            let #ident: spinel_rt::RubyValue = spinel_rt::RubyValue::Array(spinel_rt::array_new(
                (#nreq + __opt_bound..#nreq + __opt_bound + __rest_count)
                    .filter_map(|__i| __positional.get(__i).cloned())
                    .collect()
            ));
        }
    });
    // Post params start where the lead/optional/rest slots stopped consuming,
    // NOT at `__n - npost`. The two agree whenever there are enough arguments
    // to reach the posts, but underfull they don't: `m([1, 2]) { |a, *b, c, d| }`
    // is `a=1, b=[], c=2, d=nil` (oracle-verified) -- the posts fill
    // left-to-right from what's left and nil-pad the tail. Anchoring from the
    // end instead wrapped back over the lead's own argument and bound `c=1`.
    let post_lets = params.post.iter().enumerate().map(|(i, name)| {
        let ident = safe_ident(name);
        quote! {
            #[allow(unused_mut)]
            let mut #ident: spinel_rt::RubyValue = {
                let __idx = #nreq + __opt_bound + __rest_count + #i;
                __positional.get(__idx).cloned().unwrap_or(spinel_rt::RubyValue::Nil)
            };
        }
    });
    let kw_names: Vec<String> = params
        .keywords
        .iter()
        .map(|kw| match kw {
            KeywordParam::Required(n) | KeywordParam::Optional(n, _) => n.clone(),
        })
        .collect();
    let keyword_lets = params.keywords.iter().map(|kw| match kw {
        // A required keyword is bound by PRESENCE (`hash_has_key`), so a
        // supplied `k: nil` still binds nil; its absence is CRuby's
        // `ArgumentError: missing keyword: :k`, enforced for lambdas and
        // ordinary procs alike.
        KeywordParam::Required(name) => {
            let ident = safe_ident(name);
            quote! {
                let #ident: spinel_rt::RubyValue = match &__kw_source {
                    Some(spinel_rt::RubyValue::Hash(__h))
                        if spinel_rt::hash_has_key(
                            __h,
                            &spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#name)),
                        ) =>
                    {
                        spinel_rt::hash_get(
                            __h,
                            &spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#name)),
                        )
                    }
                    _ => {
                        return Err(spinel_rt::raise_error(
                            "ArgumentError",
                            format!("missing keyword: :{}", #name),
                        ))
                    }
                };
            }
        }
        KeywordParam::Optional(name, default) => {
            let ident = safe_ident(name);
            let default_expr = emit_expr(cx, *default);
            quote! {
                let #ident: spinel_rt::RubyValue = match &__kw_source {
                    Some(spinel_rt::RubyValue::Hash(__h)) => {
                        let __v = spinel_rt::hash_get(__h, &spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#name)));
                        if __v.is_nil() { #default_expr } else { __v }
                    }
                    _ => #default_expr,
                };
            }
        }
    });
    let keyword_rest_let = params.keyword_rest.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! {
            let #ident: spinel_rt::RubyValue = match &__kw_source {
                Some(spinel_rt::RubyValue::Hash(__h)) => {
                    let __declared: &[&str] = &[#(#kw_names),*];
                    // The locked map yields `(&HashKey, &(key, value))` --
                    // the ORIGINAL key/value pair is the stored tuple, the
                    // `HashKey` projection is only the map's own index.
                    spinel_rt::RubyValue::Hash(spinel_rt::hash_new(
                        __h.lock()
                            .iter()
                            .map(|(_, __kv)| __kv.clone())
                            .filter(|(__k, _)| match __k {
                                spinel_rt::RubyValue::Symbol(__s) => !__declared.contains(&__s.name().as_str()),
                                _ => true,
                            })
                            .collect()
                    ))
                }
                _ => spinel_rt::RubyValue::Hash(spinel_rt::hash_new(Vec::new())),
            };
        }
    });

    // Parenthesized destructuring params, split after their slots are bound.
    let destructures = emit_destructures(cx, params);

    // Block-locals (`|x; sum|`): a fresh `nil` binding per invocation,
    // shadowing any enclosing local of the same name. (Shadowing a same-named
    // PARAM is not a case to handle -- `|sum; sum|` is a SyntaxError in real
    // Ruby, "duplicated argument name", so it never reaches codegen.)
    //
    // Being re-declared here, inside the per-call prologue, IS the
    // reset-to-nil-every-invocation semantics: `[1,2,3].each { |x; total|
    // total = (total || 0) + x }` must never accumulate. Hoisting them out
    // would silently turn that into a running sum.
    let block_local_lets = params.block_locals.iter().map(|name| {
        let ident = safe_ident(name);
        quote! {
            #[allow(unused_variables, unused_mut)]
            let mut #ident: spinel_rt::RubyValue = spinel_rt::RubyValue::Nil;
        }
    });

    // `__opt_bound`/`__rest_count` are computed even when there's no
    // rest/optional param at all -- harmless dead-ish locals the compiler
    // won't warn about here since they're always at least read by the
    // arity math below (and, when genuinely unused because neither optional
    // nor rest is declared, `#[allow(unused_variables)]` covers it).
    quote! {
        #positional_and_kw_source
        #auto_splat
        #[allow(unused_variables)]
        let __n = __positional.len();
        #[allow(unused_variables)]
        let __min = #nreq + #npost;
        #[allow(unused_variables)]
        let __extra = __n.saturating_sub(__min);
        #[allow(unused_variables)]
        let __opt_bound = __extra.min(#nopt);
        #[allow(unused_variables)]
        let __rest_count = if #has_rest { __extra.saturating_sub(__opt_bound) } else { 0 };
        #(#required_lets)*
        #(#optional_lets)*
        #(#rest_let)*
        #(#post_lets)*
        #(#keyword_lets)*
        #(#keyword_rest_let)*
        #destructures
        #(#block_local_lets)*
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::{Hir, HirNode};

    /// A throwaway default-value expression -- `proc_arity`/`auto_splats`
    /// only ever count parameters, never look at what a default evaluates
    /// to, so one shared `nil` node serves every optional slot.
    fn nil_default(hir: &mut Hir) -> crate::hir::NodeId {
        hir.push(HirNode::NilLit)
    }

    /// Builds a `Params` from a compact spec: `req`, `opt`, `post` counts,
    /// plus rest/kwrest flags and keyword kinds.
    struct Spec {
        req: usize,
        opt: usize,
        post: usize,
        rest: bool,
        kw_required: usize,
        kw_optional: usize,
        kwrest: bool,
        block: bool,
    }

    impl Default for Spec {
        fn default() -> Self {
            Spec {
                req: 0,
                opt: 0,
                post: 0,
                rest: false,
                kw_required: 0,
                kw_optional: 0,
                kwrest: false,
                block: false,
            }
        }
    }

    fn params(hir: &mut Hir, spec: Spec) -> Params {
        let mut p = Params::default();
        for i in 0..spec.req {
            p.required.push(format!("r{i}"));
        }
        for i in 0..spec.opt {
            let d = nil_default(hir);
            p.optional.push((format!("o{i}"), d));
        }
        if spec.rest {
            p.rest = Some(Some("rest".to_string()));
        }
        for i in 0..spec.post {
            p.post.push(format!("p{i}"));
        }
        for i in 0..spec.kw_required {
            p.keywords.push(KeywordParam::Required(format!("kr{i}")));
        }
        for i in 0..spec.kw_optional {
            let d = nil_default(hir);
            p.keywords.push(KeywordParam::Optional(format!("ko{i}"), d));
        }
        if spec.kwrest {
            p.keyword_rest = Some(Some("kwrest".to_string()));
        }
        if spec.block {
            p.block = Some(Some("blk".to_string()));
        }
        p
    }

    /// Every case here was READ OFF ruby 4.0.5 (`p ->(...) {}.arity` /
    /// `p proc { |...| }.arity`), not derived from our own implementation --
    /// this table IS the specification. See `proc_arity`'s docs for the
    /// formula it encodes.
    #[test]
    fn proc_arity_matches_the_ruby_oracle_for_every_signature_shape() {
        let hir = &mut Hir::default();
        // (spec, is_lambda, expected) -- the Ruby source each row mirrors
        // is named in the comment.
        let cases: Vec<(Spec, bool, i32, &str)> = vec![
            (Spec { ..Default::default() }, false, 0, "proc {}"),
            (Spec { req: 1, ..Default::default() }, false, 1, "proc { |a| }"),
            (Spec { req: 2, ..Default::default() }, false, 2, "proc { |a, b| }"),
            (Spec { req: 2, ..Default::default() }, true, 2, "->(a, b) {}"),
            (Spec { ..Default::default() }, true, 0, "->() {}"),
            (Spec { block: true, ..Default::default() }, true, 0, "->(&b) {}"),
            // An optional/rest param makes a LAMBDA negative...
            (Spec { req: 1, opt: 1, ..Default::default() }, true, -2, "->(a, b = 1) {}"),
            (Spec { req: 1, opt: 2, ..Default::default() }, true, -2, "lambda { |a, b = 1, c = 2| }"),
            // ...but a plain proc reports its MINIMUM instead (max is still
            // bounded, so no negation) -- the shape that makes the
            // lambda/proc distinction visible.
            (Spec { req: 1, opt: 1, ..Default::default() }, false, 1, "proc { |a, b = 1| }"),
            // A rest param is unbounded: negative for proc AND lambda.
            (Spec { req: 1, rest: true, ..Default::default() }, false, -2, "proc { |a, *b| }"),
            (Spec { rest: true, ..Default::default() }, false, -1, "proc { |*a| }"),
            (Spec { req: 1, rest: true, ..Default::default() }, true, -2, "->(a, *b) {}"),
            (Spec { req: 2, rest: true, ..Default::default() }, false, -3, "proc { |a, b, *c| }"),
            // Post params are required: they count toward the minimum.
            (Spec { req: 1, rest: true, post: 1, ..Default::default() }, true, -3, "->(a, *b, c) {}"),
            (Spec { req: 1, rest: true, post: 2, ..Default::default() }, true, -4, "->(a, *b, c, d) {}"),
            // A REQUIRED keyword adds exactly one mandatory slot, however
            // many there are -- and keeps the count positive.
            (Spec { req: 1, kw_required: 1, ..Default::default() }, true, 2, "->(a, b:) {}"),
            (Spec { kw_required: 1, ..Default::default() }, true, 1, "->(b:) {}"),
            (Spec { req: 1, kw_required: 2, ..Default::default() }, true, 2, "->(a, b:, c:) {}"),
            (Spec { req: 1, kw_required: 1, kw_optional: 1, ..Default::default() }, true, 2, "->(a, b:, c: 1) {}"),
            (Spec { req: 1, kw_required: 1, kwrest: true, ..Default::default() }, true, 2, "->(a, e:, **g) {}"),
            (Spec { req: 1, kw_required: 1, ..Default::default() }, false, 2, "proc { |a, b:| }"),
            // An OPTIONAL keyword / **kwrest alone widens the maximum, so a
            // lambda goes negative while a proc reports its minimum.
            (Spec { req: 1, kw_optional: 1, ..Default::default() }, true, -2, "->(a, b: 1) {}"),
            (Spec { req: 1, kwrest: true, ..Default::default() }, true, -2, "->(a, **k) {}"),
            (Spec { req: 1, kw_optional: 1, ..Default::default() }, false, 1, "proc { |a, b: 1| }"),
            (Spec { req: 1, kwrest: true, ..Default::default() }, false, 1, "proc { |a, **k| }"),
            // Everything at once.
            (
                Spec { req: 1, opt: 1, rest: true, post: 1, kw_required: 1, kw_optional: 1, kwrest: true, block: true },
                true,
                -4,
                "->(a, b = 1, *c, d, e:, f: 2, **g, &h) {}",
            ),
        ];
        for (spec, is_lambda, expected, source) in cases {
            let p = params(hir, spec);
            assert_eq!(
                proc_arity(&p, is_lambda),
                expected,
                "arity of `{source}` (is_lambda: {is_lambda})"
            );
        }
    }

    /// Also oracle-read: `def m(a); yield a; end; m([1, 2]) { |...| }` and
    /// checking whether the array arrived splatted. See `auto_splats`.
    #[test]
    fn auto_splats_matches_the_ruby_oracle_for_every_block_param_shape() {
        let hir = &mut Hir::default();
        let cases: Vec<(Spec, bool, &str)> = vec![
            // Nothing positional to spread into.
            (Spec { ..Default::default() }, false, "{ }"),
            (Spec { rest: true, ..Default::default() }, false, "{ |*a| }"),
            (Spec { rest: true, kwrest: true, ..Default::default() }, false, "{ |*a, **k| }"),
            (Spec { kw_required: 1, ..Default::default() }, false, "{ |a:| }"),
            // `ambiguous_param0`: exactly one lead and nothing else
            // positional. Keywords do NOT disturb the exemption.
            (Spec { req: 1, ..Default::default() }, false, "{ |a| }"),
            (Spec { req: 1, kwrest: true, ..Default::default() }, false, "{ |a, **k| }"),
            (Spec { req: 1, kw_required: 1, ..Default::default() }, false, "{ |a, b:| }"),
            // A LONE optional is the `|a|` case by another spelling -- one
            // slot, no lead, so nothing forces a spread.
            (Spec { opt: 1, ..Default::default() }, false, "{ |a = 9| }"),
            // The subtle one: one optional plus a rest still has no lead and
            // only one optional, so it does NOT splat -- unlike `|a, *b|`
            // just below, which is identical but for the lead. This pair is
            // why the rule can't be "count the positional slots".
            (Spec { opt: 1, rest: true, ..Default::default() }, false, "{ |a = 5, *b| }"),
            // lead + post > 0 -> splats.
            (Spec { req: 2, ..Default::default() }, true, "{ |a, b| }"),
            (Spec { req: 2, kwrest: true, ..Default::default() }, true, "{ |a, b, **k| }"),
            (Spec { req: 1, rest: true, ..Default::default() }, true, "{ |a, *b| }"),
            (Spec { req: 1, opt: 1, ..Default::default() }, true, "{ |a, b = 5| }"),
            (Spec { req: 1, rest: true, post: 1, ..Default::default() }, true, "{ |a, *b, c| }"),
            (Spec { req: 1, post: 1, ..Default::default() }, true, "{ |a, (b)| }-shaped post"),
            (Spec { rest: true, post: 1, ..Default::default() }, true, "{ |*a, b| }"),
            (Spec { rest: true, post: 2, ..Default::default() }, true, "{ |*a, b, c| }"),
            (Spec { opt: 1, rest: true, post: 1, ..Default::default() }, true, "{ |a = 5, *b, c| }"),
            // opt > 1 -> splats, with no lead at all.
            (Spec { opt: 2, ..Default::default() }, true, "{ |a = 5, b = 4| }"),
            (Spec { opt: 2, kwrest: true, ..Default::default() }, true, "{ |a = 1, b = 2, **k| }"),
            (Spec { opt: 2, rest: true, ..Default::default() }, true, "{ |a = 5, b = 4, *c| }"),
            (Spec { opt: 3, ..Default::default() }, true, "{ |a = 1, b = 2, c = 3| }"),
            // A trailing comma (`|a, |`) lowers to an anonymous rest, which
            // breaks the exemption exactly as `*b` does.
            (Spec { req: 1, rest: true, ..Default::default() }, true, "{ |a, | }"),
        ];
        for (spec, expected, source) in cases {
            let p = params(hir, spec);
            assert_eq!(auto_splats(&p), expected, "auto-splat of a block `{source}`");
        }
    }

    /// A lambda's parameters never auto-splat, whatever their shape (real
    /// Ruby: a lambda is strict, `->(a, b) {}.call([1, 2])` is an
    /// ArgumentError, not a destructure) -- enforced at the CALLER, which
    /// consults `is_lambda` before ever asking `auto_splats`.
    #[test]
    fn auto_splats_is_only_consulted_for_non_lambdas() {
        let hir = &mut Hir::default();
        let p = params(hir, Spec { req: 2, ..Default::default() });
        // The shape itself says "splat"...
        assert!(auto_splats(&p));
        // ...and `emit_proc_param_bindings` gates on `!is_lambda`, so the
        // emitted lambda body carries no auto-splat call at all.
        let cx_free = emit_proc_param_bindings_probe(&p, true);
        assert!(!cx_free.contains("block_auto_splat"), "lambda body: {cx_free}");
        let proc_body = emit_proc_param_bindings_probe(&p, false);
        assert!(proc_body.contains("block_auto_splat"), "proc body: {proc_body}");
    }

    /// `emit_proc_param_bindings` needs a `Ctx` (for default-value
    /// expressions); this probe builds the minimal one a param list with no
    /// defaults requires.
    fn emit_proc_param_bindings_probe(params: &Params, is_lambda: bool) -> String {
        let compiler = crate::compiler::Compiler::new(Hir::default());
        let label_counter = std::cell::Cell::new(0u32);
        let empty = std::collections::HashSet::new();
        let cx = Ctx {
            compiler: &compiler,
            box_id: 0,
            current_class: None,
            defining_class: None,
            class_self: None,
            current_method: None,
            local_types: std::borrow::Cow::Owned(std::collections::HashMap::new()),
            label_counter: &label_counter,
            loop_labels: None,
            for_var_override: None,
            captured_locals: std::borrow::Cow::Borrowed(&empty),
            self_ident: quote::format_ident!("__self"),
            in_real_proc: false,
            self_is_dynamic: false,
        };
        emit_proc_param_bindings(&cx, params, &format_ident!("__args"), is_lambda).to_string()
    }
}
