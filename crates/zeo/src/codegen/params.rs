//! Path 1 (static) parameter binding: the callee's Rust fn signature/
//! prologue (`emit_signature_params`/`emit_prologue`) and the caller's
//! argument-list construction (`emit_call_args`) for a method/block's
//! `Params`. Path 2 (dynamic `send`) binding lives in `dispatch.rs` --
//! zeo-authored trampoline bodies, not this module (see its docs).
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

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use quote::{format_ident, quote};

use super::Ctx;
use super::expr::{box_if_object_typed, emit_expr};
use super::ident::safe_ident;
use crate::compiler::FSet;
use crate::compiler::{AccessorKind, AccessorShape};
use crate::hir::{HirNode, KeywordParam, KwArg, NodeId, Params};
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

/// The arguments a forwarding wrapper passes on: exactly the names
/// `emit_signature_params` declared, comma-PREFIXED to follow a receiver.
pub fn emit_forward_args(params: &Params, needs_block: bool) -> TokenStream {
    let items = signature_param_idents(params, needs_block);
    quote! { #(, #items)* }
}

/// `emit_signature_params` for a RECEIVERLESS free function (a class
/// method/module function) -- the same items, comma-SEPARATED rather than
/// comma-prefixed (nothing precedes them in the signature).
pub fn emit_signature_params_free(params: &Params, needs_block: bool) -> TokenStream {
    let items = signature_param_items(params, needs_block);
    quote! { #(#items),* }
}

/// Whether rendered Rust mentions `name` as a whole identifier.
///
/// Not `split_whitespace`: `proc_macro2` renders a delimited group with no
/// space inside it, so `__n.saturating_sub(__min)` yields the word `(__min)`
/// and a whole-word compare silently misses the reference -- which emitted
/// `let __extra = __n.saturating_sub(__min);` with no `__min` to read.
fn mentions_ident(text: &str, name: &str) -> bool {
    let boundary = |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    text.match_indices(name).any(|(i, _)| {
        boundary(text[..i].chars().next_back()) && boundary(text[i + name.len()..].chars().next())
    })
}

/// `__opt_bound` for a trampoline, emitted only where the argument list it
/// feeds actually reads it -- a method with no optional and no rest parameter
/// never does, and there are thousands of those.
fn opt_bound_let(consumers: &TokenStream, min_lit: usize, nopt: usize) -> TokenStream {
    if !mentions_ident(&consumers.to_string(), "__opt_bound") {
        return quote! {};
    }
    quote! { let __opt_bound = (args.len() - #min_lit).min(#nopt); }
}

fn signature_param_items(params: &Params, needs_block: bool) -> Vec<TokenStream> {
    signature_param_pairs(params, needs_block)
        .into_iter()
        .map(|(ident, ty)| quote! { #ident: #ty })
        .collect()
}

/// The parameter NAMES `signature_param_items` puts in the signature, in the
/// same order -- what a forwarding wrapper hands straight through.
fn signature_param_idents(params: &Params, needs_block: bool) -> Vec<proc_macro2::Ident> {
    signature_param_pairs(params, needs_block)
        .into_iter()
        .map(|(ident, _)| ident)
        .collect()
}

/// The one place the signature's order and spelling are decided, so a name
/// list and a typed list can never drift apart.
fn signature_param_pairs(
    params: &Params,
    needs_block: bool,
) -> Vec<(proc_macro2::Ident, TokenStream)> {
    let mut items = Vec::new();
    for name in &params.required {
        let ident = safe_ident(name);
        items.push((ident, quote! { zeo_rt::RubyValue }));
    }
    for (name, _) in &params.optional {
        let ident = safe_ident(name);
        items.push((ident, quote! { Option<zeo_rt::RubyValue> }));
    }
    if let Some(Some(name)) = &params.rest {
        let ident = safe_ident(name);
        items.push((ident, quote! { Vec<zeo_rt::RubyValue> }));
    }
    for name in &params.post {
        let ident = safe_ident(name);
        items.push((ident, quote! { zeo_rt::RubyValue }));
    }
    for kw in &params.keywords {
        match kw {
            KeywordParam::Required(name) => {
                let ident = safe_ident(name);
                items.push((ident, quote! { zeo_rt::RubyValue }));
            }
            KeywordParam::Optional(name, _) => {
                let ident = safe_ident(name);
                items.push((ident, quote! { Option<zeo_rt::RubyValue> }));
            }
        }
    }
    if let Some(Some(name)) = &params.keyword_rest {
        let ident = safe_ident(name);
        // `(RubyValue, RubyValue)`, not `(Symbol, RubyValue)`: a `**kwrest` hash
        // can hold non-symbol keys (`method HELP_MAPPINGS => :help`).
        items.push((
            ident,
            quote! { Vec<(zeo_rt::RubyValue, zeo_rt::RubyValue)> },
        ));
    }
    if needs_block {
        items.push((format_ident!("__blk"), quote! { Option<zeo_rt::RubyValue> }));
    }
    items
}

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

/// The callee's own prologue. Shadows every `Option<RubyValue>` or raw
/// collection Rust parameter with a `let mut` binding of the same Ruby name,
/// holding a real `RubyValue`.
///
/// Emitted in `Params`' declared order, so a later default expression can
/// reference an earlier parameter. That matches Ruby's rule that a default
/// may depend only on parameters declared before it.
///
/// A named `&blk` shadows `__blk` into an ordinary `RubyValue` local, `Nil`
/// when unyielded. That matches real Ruby, and differs from every other
/// kind's None-becomes-a-default rule.
///
/// Finally, any of this method's own parameter names that an escaping block
/// captures get one more shadow, into `Arc<parking_lot::Mutex<RubyValue>>`.
/// Captured method PARAMETERS need this as much as captured plain locals do,
/// because `codegen::hoisting`'s prelude only sees names `collect_locals`
/// finds, and those never include a method's own params.
pub fn emit_prologue(cx: &Ctx, params: &Params, body: &[NodeId]) -> TokenStream {
    // Destructured names are excluded from capture wraps: a wrap turns a
    // by-value Rust PARAMETER into its capture cell, and a destructured name
    // has no parameter to turn -- only its `__destr_<i>` slot does.
    // `emit_destructures` already declares each one directly at its storage
    // class (cell included), so wrapping here would wrap the cell in a cell.
    let destructured: FSet<String> = params.destructured_names().into_iter().collect();
    // ASSIGNED names are excluded too: the hoist prelude that follows this
    // prologue (`emit_hoisted_body_with_extra_roots`, same collection roots)
    // declares every assigned local itself, and for a captured PARAM its
    // `Captured if is_param` arm already seeds the cell from the parameter
    // value -- wrapping here as well re-wrapped the cell in a second cell
    // (a real E0308: `Mutex::new(message)` fed an `Arc<Mutex<..>>`, from the
    // timeout gem's captured `message ||= ...`). Ownership is split exactly:
    // this prologue wraps captured-but-never-assigned params, the prelude
    // owns everything assigned.
    let mut assigned: Vec<String> = Vec::new();
    for &n in body {
        super::hoisting::collect_locals(cx.compiler, n, &mut assigned);
    }
    for id in params.default_ids() {
        super::hoisting::collect_locals(cx.compiler, id, &mut assigned);
    }
    let wrap_if_captured = |name: &str| -> Option<TokenStream> {
        (cx.captured_locals.contains(name)
            && !destructured.contains(name)
            && !assigned.iter().any(|a| a == name))
        .then(|| {
            let ident = safe_ident(name);
            quote! {
                let #ident: std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                    std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(#ident));
            }
        })
    };
    // Emitted in PARAMETER ORDER, each binding immediately followed by its
    // capture wrap (deterministic output for free): a later parameter's
    // DEFAULT expression may read an earlier captured parameter (`def
    // fill_breakable(sep = ' ', width = sep.length)` with `sep` captured by
    // a block, prettyprint's shape), and codegen reads a captured local
    // through its cell -- so the cell must exist by the time that default
    // evaluates, not at the end of the prologue. Required (and post) params
    // are bound by the Rust signature itself, so only their wraps emit,
    // first.
    // Ahead of every binding: the locals the defaults below themselves assign.
    let mut pieces: Vec<TokenStream> = vec![super::hoisting::emit_param_default_decls(
        cx,
        &params.default_ids(),
        &params.bound_names(),
    )];
    for name in params.required.iter().chain(&params.post) {
        pieces.extend(wrap_if_captured(name));
    }
    for (name, default) in &params.optional {
        pieces.push(emit_lazy_default_shadow(cx, name, *default));
        pieces.extend(wrap_if_captured(name));
    }
    if let Some(Some(name)) = &params.rest {
        let ident = safe_ident(name);
        pieces.push(quote! {
            #[allow(unused_mut)]
            let mut #ident: zeo_rt::RubyValue =
                zeo_rt::RubyValue::Array(zeo_rt::array_new(#ident));
        });
        pieces.extend(wrap_if_captured(name));
    }
    for kw in &params.keywords {
        match kw {
            KeywordParam::Required(name) => pieces.extend(wrap_if_captured(name)),
            KeywordParam::Optional(name, default) => {
                pieces.push(emit_lazy_default_shadow(cx, name, *default));
                pieces.extend(wrap_if_captured(name));
            }
        }
    }
    if let Some(Some(name)) = &params.keyword_rest {
        let ident = safe_ident(name);
        pieces.push(quote! {
            #[allow(unused_mut)]
            let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Hash(zeo_rt::hash_new(
                #ident.into_iter().collect()
            ));
        });
        pieces.extend(wrap_if_captured(name));
    }
    // `params.block` being `Some(Some(name))` already implies `needs_block`
    // (see `Scope::needs_block_param`), so no extra gate is needed here.
    if let Some(Some(name)) = &params.block {
        let ident = safe_ident(name);
        pieces.push(quote! {
            #[allow(unused_mut)]
            let mut #ident: zeo_rt::RubyValue = __blk.clone().unwrap_or(zeo_rt::RubyValue::Nil);
        });
        pieces.extend(wrap_if_captured(name));
    }
    // Destructures split the `__destr_<i>` slots bound by the signature; the
    // destructured NAMES are declared by `emit_destructures` itself at their
    // own storage class (cell included), never wrapped above.
    let destructures = emit_destructures(cx, params);
    quote! { #(#pieces)* #destructures }
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
        let mut #ident: zeo_rt::RubyValue = match #ident {
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
/// mismatches are clean compile-time panics (zeo has full static
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
    callee_frame: TokenStream,
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
        callee_frame,
    )
}

/// How `emit_call_args_to` spells the actual invocation once the argument
/// list is built -- ordinary Path-1 method syntax, or a builtin-reopen FREE
/// FUNCTION, whose receiver `RubyValue` is passed as the
/// generated `__self` first argument instead of a method receiver.
pub enum Callee {
    Method(TokenStream),
    FreeFn {
        path: TokenStream,
        recv: TokenStream,
    },
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

/// A best-effort label for a NON-symbol keyword key in an "unknown keyword"
/// error (the rare "named keywords, no `**kwrest`, a non-symbol key" path). Real
/// Ruby prints the key's `inspect` (`unknown keyword: "s"`); a string literal
/// reproduces that exactly, other shapes fall back to a placeholder.
fn kw_key_label(cx: &Ctx, key: NodeId) -> String {
    match &cx.compiler.hir[key] {
        HirNode::StringLit(parts) => match parts.as_slice() {
            [crate::hir::StrPart::Lit(s)] => format!("{s:?}"),
            _ => "(expr)".to_string(),
        },
        HirNode::ClassRef(name) => name.clone(),
        _ => "(expr)".to_string(),
    }
}

/// `emit_call_args`, generalized over the invocation shape (see `Callee`).
///
/// `callee_frame` is the CALLEE's backtrace-frame guard
/// (`codegen::scope_frame_guard` tokens, possibly empty): a statically
/// detected arity/keyword mismatch raises with that frame pushed, so the
/// backtrace's innermost line is the callee's `def` -- CRuby raises these
/// inside the callee, oracle-verified. The happy path never pushes it.
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
    callee_frame: TokenStream,
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
    // `**nil` is precisely the declaration that this conversion must not
    // happen, so it never applies there -- the refusal below reports the
    // keywords instead.
    let kw_as_positional = params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && !params.no_keywords
        && !kwargs.is_empty();
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
    let kw_temps: Vec<syn::Ident> = (0..kwargs.len())
        .map(|i| format_ident!("__kw{i}"))
        .collect();
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
    // `Some(name)` for a literal-symbol key (`k: v` / `:k => v`), which can
    // bind a named keyword parameter; `None` for a NON-symbol key (`CONST => v`,
    // `"s" => v`), which -- like real Ruby -- can only land in `**kwrest` (a
    // keyword parameter name is always a symbol). `kw_as_positional` above
    // already handled the no-keyword/no-kwrest case, so a `None` reaching the
    // binding below means a keyword-declaring callee.
    let kw_names: Vec<Option<String>> = kwargs
        .iter()
        .map(|kw| {
            let KwArg::Pair(k, _) = kw else {
                unreachable!("a `**` double-splat routes to emit_splat_call, never Path 1")
            };
            match &cx.compiler.hir[*k] {
                HirNode::SymbolLit(s) => Some(s.clone()),
                _ => None,
            }
        })
        .collect();

    // A statically-detectable argument-shape error: evaluate the arg
    // temporaries (caller's frame -- CRuby's order), then raise under the
    // CALLEE's frame -- see the comment above `pos_temps` and the
    // `callee_frame` doc above.
    // `?`-propagated rather than `return`ed: a call whose arguments are wrong
    // is still an EXPRESSION, and it can sit anywhere one does -- including as
    // a receiver (`ask(a, b, k: 1) =~ ...` in a class that narrowed `ask`'s
    // arity). A `return` there types the block as `!`, which is not what the
    // surrounding position wants; `?` types it as the `RubyValue` every other
    // expression produces, and unwinds identically.
    let raise_argument_error = |msg: String| {
        let frame = &callee_frame;
        quote! {
            {
                #(#pos_lets)*
                #(#kw_lets)*
                #frame
                Err::<zeo_rt::RubyValue, zeo_rt::Signal>(
                    zeo_rt::raise_error("ArgumentError", #msg.to_string()),
                )?
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

    // `**nil` refuses keywords BEFORE the arity check, so `nokw(1, 2, 3, b: 4)`
    // reports the keywords rather than the count (CRuby `vm_args.c:848`).
    if params.no_keywords && !kwargs.is_empty() {
        return raise_argument_error("no keywords accepted".to_string());
    }
    if n_pos < min_positional || (!has_rest && n_pos > nreq + nopt + npost) {
        return raise_argument_error(format!(
            "wrong number of arguments (given {}, expected {})",
            n_pos,
            expected_shape()
        ));
    }
    for kw in &params.keywords {
        if let KeywordParam::Required(name) = kw
            && !kw_names.iter().any(|n| n.as_deref() == Some(name.as_str()))
        {
            return raise_argument_error(format!("missing keyword: :{name}"));
        }
    }
    if params.keyword_rest.is_none() {
        let declared = |n: &str| {
            params.keywords.iter().any(|kw| match kw {
                KeywordParam::Required(k) | KeywordParam::Optional(k, _) => k == n,
            })
        };
        // With no `**kwrest`, a symbol key must name a declared keyword, and a
        // non-symbol key can't bind at all (real Ruby: `unknown keyword: "s"`).
        for (i, name) in kw_names.iter().enumerate() {
            match name {
                Some(n) if declared(n) => {}
                Some(n) => return raise_argument_error(format!("unknown keyword: :{n}")),
                None => {
                    let KwArg::Pair(k, _) = &kwargs[i] else {
                        unreachable!()
                    };
                    return raise_argument_error(format!(
                        "unknown keyword: {}",
                        kw_key_label(cx, *k)
                    ));
                }
            }
        }
    }

    let extra = n_pos - min_positional;
    let opt_bound = extra.min(nopt);
    let rest_count = extra - opt_bound;

    let mut kw_used = vec![false; kwargs.len()];
    let find_kw = |name: &str, used: &mut Vec<bool>| -> Option<syn::Ident> {
        kw_names
            .iter()
            .position(|n| n.as_deref() == Some(name))
            .map(|i| {
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
                    let t = &kw_temps[i];
                    // A `**kwrest` hash keeps its keys as full values -- a symbol
                    // key stays a `:sym`, but a non-symbol key (`HELP_MAPPINGS =>
                    // :help`, an options-hash idiom) keeps its real object, so the
                    // pair type is `(RubyValue, RubyValue)`. The non-symbol key is
                    // evaluated here in call position, AFTER the value temporaries
                    // -- a value/key order difference only observable for a
                    // side-effecting key (in practice always a constant/literal).
                    let key = match &kw_names[i] {
                        Some(name) => {
                            let sym = super::pooled_sym(name);
                            quote! { zeo_rt::RubyValue::Symbol(#sym) }
                        }
                        None => {
                            let KwArg::Pair(k, _) = &kwargs[i] else {
                                unreachable!()
                            };
                            box_if_object_typed(cx, *k, emit_expr(cx, *k))
                        }
                    };
                    quote! { (#key, #t.clone()) }
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
        quote! { zeo_rt::catch_break(#call_expr)? }
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
/// `zeo_rt::ruby_class!`'s docs for why the macro does NOT DERIVE this
/// from the method's own Rust signature: an exact-length slice-pattern
/// match can't express "these two are optional" the way ordinary Rust
/// control flow can, so zeo authors this once, here, using the same
/// `Params` info the Path 1 caller above already has).
///
/// Keyword parameters are NOT bound dynamically (documented zeo scope-
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
/// Hash off `args` (via `zeo_rt::bind_dynamic_kwargs`, shadowing `args`
/// with the remaining positionals) plus the keyword/kwrest argument tokens
/// in DECLARED order (the Rust signature's order). Empty for a
/// keywordless callee.
fn dynamic_kwargs_binding(
    method_name: &str,
    params: &Params,
    callee_frame: &TokenStream,
) -> (Option<TokenStream>, Vec<TokenStream>) {
    if params.keywords.is_empty() && params.keyword_rest.is_none() {
        // `**nil` refuses a kw-marked trailing Hash (the caller wrote
        // keywords) BEFORE the arity check -- the dynamic twin of
        // `emit_call_args_to`'s literal-kwargs refusal. The raise runs with
        // the callee's frame pushed, same as the binder below.
        if params.no_keywords {
            let preamble = quote! {
                {
                    #callee_frame
                    zeo_rt::reject_marked_kwargs(args)?;
                }
            };
            return (Some(preamble), Vec::new());
        }
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
    // The binder's missing/unknown-keyword raises happen with the CALLEE's
    // frame pushed (block-scoped, so the guard pops before the real body --
    // which pushes its own -- runs). CRuby attributes these to the def line.
    let preamble = quote! {
        #[allow(unused_mut, unused_variables)]
        let (args, __kw_req, __kw_opt, mut __kw_rest) = {
            #callee_frame
            zeo_rt::bind_dynamic_kwargs(
                #method_name,
                args,
                &[#(#req_names),*],
                &[#(#opt_names),*],
                #has_kwrest,
            )?
        };
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
/// CRuby's exact shapes (oracle-verified against ruby 4.0.6): `N` fixed,
/// `N..M` bounded range, `N+` when a rest param leaves the upper bound open.
/// Both trampolines build their argument bindings identically below, so the
/// `Option`-built bounds keep a missing lower/upper bound from emitting a
/// dangling `||` (an `args.len() < 0` useless-comparison lint).
fn emit_arity_check(params: &Params, callee_frame: &TokenStream) -> TokenStream {
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
    // The raise runs with the CALLEE's frame pushed -- CRuby attributes a
    // wrong-argument-count error to the callee's def line.
    quote! {
        if #cond {
            #callee_frame
            return Err(zeo_rt::raise_error(
                "ArgumentError",
                format!("wrong number of arguments (given {}, expected {})", args.len(), #expected),
            ));
        }
    }
}

/// Whether a signature is PLAIN -- required positionals only (a `&block`
/// param is fine) -- and so compresses to a `zeo_rt::zeo_tramp!` invocation
/// instead of the long-form closure. The macro's expansion is
/// token-for-token the long form for this shape.
fn plain_signature(params: &Params) -> bool {
    params.optional.is_empty()
        && params.rest.is_none()
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        // `**nil` needs the long form's kwargs preamble (the marked-hash
        // refusal) even though it binds nothing.
        && !params.no_keywords
}

/// The frame guard as a `zeo_tramp!` argument: the same `let __frame = ...`
/// statement, minus the trailing semicolon a macro `stmt` fragment must not
/// carry. `None` (no argument at all) for a def with no location.
fn tramp_frame_arg(callee_frame: &TokenStream) -> Option<TokenStream> {
    if callee_frame.is_empty() {
        return None;
    }
    let mut tts: Vec<proc_macro2::TokenTree> = callee_frame.clone().into_iter().collect();
    if matches!(tts.last(), Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == ';') {
        tts.pop();
    }
    let stmt: TokenStream = tts.into_iter().collect();
    Some(quote! { , #stmt })
}

/// The Path 2 entry for a method whose whole body is one ivar access: the
/// field, reached directly, instead of a downcast-and-call into the generated
/// inherent method. See `zeo_tramp!`'s `rd`/`wr` heads for what that saves and
/// why the runtime half is the half that matters.
///
/// A HAND-WRITTEN accessor still threads `callee_frame` through, so an arity
/// error and a frozen-receiver raise stay attributed to its `def` line exactly
/// as before; only the SUCCEEDING call loses its frame, which nothing but
/// `TracePoint` can see (`Hir::uses_call_tracing` gates that).
///
/// An `attr_*`-synthesized one drops the frame outright, matching CRuby: its
/// accessors are iseq-less and appear in no backtrace at all, so today's
/// `bt.rb:2:in 'T#a='` line above the caller is a divergence this removes.
pub fn emit_accessor_trampoline(
    class_ident: &proc_macro2::Ident,
    shape: &AccessorShape,
    slot: usize,
    callee_frame: &TokenStream,
) -> TokenStream {
    let head = format_ident!(
        "{}",
        match shape.kind {
            AccessorKind::Reader => "rd",
            AccessorKind::Writer => "wr",
        }
    );
    let frame_arg = (!shape.attr_generated)
        .then(|| tramp_frame_arg(callee_frame))
        .flatten();
    quote! { zeo_rt::zeo_tramp!(#head #class_ident, #slot #frame_arg) }
}

pub fn emit_dynamic_trampoline(
    class_ident: &proc_macro2::Ident,
    method_name: &str,
    params: &Params,
    needs_block: bool,
    callee_frame: &TokenStream,
) -> TokenStream {
    let method_ident = safe_ident(method_name);
    if plain_signature(params) {
        let head = format_ident!("{}", if needs_block { "instb" } else { "inst" });
        let n = params.required.len();
        let ix = 0..n;
        let frame_arg = tramp_frame_arg(callee_frame);
        return quote! {
            zeo_rt::zeo_tramp!(#head #class_ident, #method_ident, #n, [#(#ix),*] #frame_arg)
        };
    }
    let (kw_preamble, kw_args) = dynamic_kwargs_binding(method_name, params, callee_frame);

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let min_lit = nreq + npost;
    let arity_check = emit_arity_check(params, callee_frame);

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
    let blk_ident = if needs_block {
        format_ident!("blk")
    } else {
        format_ident!("_blk")
    };
    let block_arg = needs_block.then(|| quote! { blk, });
    // The argument list is built first so `opt_bound_let` can see whether it
    // reads `__opt_bound` -- a method with no optional and no rest parameter
    // never does.
    let call_args = quote! { #(#required_args,)* #(#optional_args,)* #(#rest_arg)* #(#post_args,)* #(#kw_args,)* #block_arg };
    let opt_bound = opt_bound_let(&call_args, min_lit, nopt);

    quote! {
        |recv: &zeo_rt::RObj, args: &[zeo_rt::RubyValue], #blk_ident: Option<zeo_rt::RubyValue>| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
            let this = zeo_rt::downcast_robj::<#class_ident>(recv)
                .expect("class_id guarantees this downcast");
            #kw_preamble
            #arity_check
            #opt_bound
            #class_ident::#method_ident(
                this,
                #call_args
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

/// The Path 2 trampoline for a BUILTIN-REOPEN method or a user
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
    callee_frame: &TokenStream,
) -> TokenStream {
    if plain_signature(params) {
        let head = format_ident!(
            "{}{}",
            if recv_mode == RecvMode::Pass {
                "pass"
            } else {
                "drop"
            },
            if needs_block { "b" } else { "" }
        );
        let n = params.required.len();
        let ix = 0..n;
        let frame_arg = tramp_frame_arg(callee_frame);
        return quote! {
            zeo_rt::zeo_tramp!(#head #fn_path, #n, [#(#ix),*] #frame_arg)
        };
    }
    let (kw_preamble, kw_args) = dynamic_kwargs_binding(method_name, params, callee_frame);

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let min_lit = nreq + npost;
    let arity_check = emit_arity_check(params, callee_frame);

    let required_args = (0..nreq).map(|i| quote! { args[#i].clone() });
    let optional_args = (0..nopt).map(|i| {
        let idx = nreq + i;
        quote! { if #i < __opt_bound { Some(args[#idx].clone()) } else { None } }
    });
    let rest_arg = params.rest.iter().flatten().map(|_| {
        quote! { args[(#nreq + __opt_bound)..(args.len() - #npost)].to_vec(), }
    });
    let post_args = (0..npost).map(|i| quote! { args[args.len() - #npost + #i].clone() });
    let blk_ident = if needs_block {
        format_ident!("blk")
    } else {
        format_ident!("_blk")
    };
    let block_arg = needs_block.then(|| quote! { blk, });
    // The argument list is built first so `opt_bound_let` can see whether it
    // reads `__opt_bound` -- a method with no optional and no rest parameter
    // never does.
    let call_args = quote! { #(#required_args,)* #(#optional_args,)* #(#rest_arg)* #(#post_args,)* #(#kw_args,)* #block_arg };
    let opt_bound = opt_bound_let(&call_args, min_lit, nopt);
    let (recv_ident, recv_arg) = match recv_mode {
        RecvMode::Pass => (format_ident!("recv"), Some(quote! { recv.clone(), })),
        RecvMode::Drop => (format_ident!("_recv"), None),
    };

    quote! {
        |#recv_ident: &zeo_rt::RubyValue, args: &[zeo_rt::RubyValue], #blk_ident: Option<zeo_rt::RubyValue>| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
            #kw_preamble
            #arity_check
            #opt_bound
            #fn_path(
                #recv_arg
                #call_args
            )
        }
    }
}

/// The `MethodFn` trampoline for a native-exception reopen/subclass method:
/// identical arity/keyword handling to `emit_value_trampoline`, but the
/// receiver is a `&RObj` (the flat `methods` table `define_method` registers
/// into) rather than a `&RubyValue`, so it BOXES the object into the
/// `RubyValue::Object` self the `emit_builtin_method_fn`-shaped body takes.
/// Registered via `ClassRegistry::define_method`; a non-capturing closure
/// coerces to `MethodFn` -> `MethodImpl::Static`.
pub fn emit_exc_trampoline(
    fn_path: &TokenStream,
    method_name: &str,
    params: &Params,
    needs_block: bool,
    callee_frame: &TokenStream,
) -> TokenStream {
    let (kw_preamble, kw_args) = dynamic_kwargs_binding(method_name, params, callee_frame);

    let nreq = params.required.len();
    let nopt = params.optional.len();
    let npost = params.post.len();
    let min_lit = nreq + npost;
    let arity_check = emit_arity_check(params, callee_frame);

    let required_args = (0..nreq).map(|i| quote! { args[#i].clone() });
    let optional_args = (0..nopt).map(|i| {
        let idx = nreq + i;
        quote! { if #i < __opt_bound { Some(args[#idx].clone()) } else { None } }
    });
    let rest_arg = params.rest.iter().flatten().map(|_| {
        quote! { args[(#nreq + __opt_bound)..(args.len() - #npost)].to_vec(), }
    });
    let post_args = (0..npost).map(|i| quote! { args[args.len() - #npost + #i].clone() });
    let blk_ident = if needs_block {
        format_ident!("blk")
    } else {
        format_ident!("_blk")
    };
    let block_arg = needs_block.then(|| quote! { blk, });
    // The argument list is built first so `opt_bound_let` can see whether it
    // reads `__opt_bound` -- a method with no optional and no rest parameter
    // never does.
    let call_args = quote! { #(#required_args,)* #(#optional_args,)* #(#rest_arg)* #(#post_args,)* #(#kw_args,)* #block_arg };
    let opt_bound = opt_bound_let(&call_args, min_lit, nopt);

    quote! {
        |recv: &zeo_rt::RObj, args: &[zeo_rt::RubyValue], #blk_ident: Option<zeo_rt::RubyValue>| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
            let __self = zeo_rt::RubyValue::Object(recv.clone());
            #kw_preamble
            #arity_check
            #opt_bound
            #fn_path(
                __self,
                #call_args
            )
        }
    }
}

/// `Proc#arity` for a block/lambda with these parameters -- CRuby's
/// `rb_proc_arity`/`rb_iseq_min_max_arity` (proc.c), evaluated at compile
/// time and baked into the constructed `RProc` (a Rust closure can't
/// answer this about itself; see `zeo_rt::ProcData`).
///
/// ```text
/// min = required + post + (1 if any REQUIRED keyword)
/// max = UNLIMITED if *rest, else required + optional + post + (1 if any keyword/**kwrest)
/// arity = min             when (lambda ? min == max : max != UNLIMITED)
///       = -min - 1        otherwise
/// ```
///
/// Verified against ruby 4.0.6 across 24 signatures -- including the two
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
    if positive { min } else { -min - 1 }
}

/// `Proc#parameters` metadata: emits `vec![zeo_rt::ProcParamMeta::new(...)]`
/// in CRuby's order and kinds. Kinds are stored CANONICALLY (lambda-style): a
/// plain required positional is `:req` regardless of the receiver's lambda-ness.
/// The runtime remaps leading requireds to `:opt` for a proc-view report (see
/// `builtins::rproc::parameters`), which also lets `#parameters(lambda:)` force
/// either view. A parenthesized destructuring slot (`__destr_<i>`) reports no
/// name; `is_lambda` does not affect the stored kinds.
pub(super) fn proc_parameters(params: &Params, _is_lambda: bool) -> Option<TokenStream> {
    fn mk(kind: &str, name: Option<&str>) -> TokenStream {
        let name_tok = match name {
            Some(n) => quote! { Some(#n) },
            None => quote! { None },
        };
        quote! { zeo_rt::ProcParamMeta { kind: #kind, name: #name_tok } }
    }
    // A parenthesized destructuring slot (`__destr_<i>`) reports no name.
    fn visible(n: &str) -> Option<&str> {
        (!n.starts_with("__destr")).then_some(n)
    }
    let mut items: Vec<TokenStream> = Vec::new();
    for r in &params.required {
        items.push(mk("req", visible(r)));
    }
    for (o, _) in &params.optional {
        items.push(mk("opt", visible(o)));
    }
    if let Some(rest) = &params.rest {
        // An anonymous `*` (absent, or the parser's `__anon_rest` synthetic)
        // reports the name `:*` (CRuby 4.0).
        items.push(mk("rest", Some(anon_name(rest.as_deref(), "*"))));
    }
    for p in &params.post {
        items.push(mk("req", visible(p)));
    }
    for kw in &params.keywords {
        match kw {
            KeywordParam::Required(n) => items.push(mk("keyreq", Some(n))),
            KeywordParam::Optional(n, _) => items.push(mk("key", Some(n))),
        }
    }
    if let Some(kwrest) = &params.keyword_rest {
        items.push(mk("keyrest", Some(anon_name(kwrest.as_deref(), "**"))));
    }
    if let Some(blk) = &params.block {
        items.push(mk("block", Some(anon_name(blk.as_deref(), "&"))));
    }
    if items.is_empty() {
        // A no-param signature is `ProcData`'s default -- skip the call.
        return None;
    }
    let key = items
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(";");
    Some(super::pooled_proc_params(key, &items))
}

/// The reported name for a `rest`/`keyword_rest`/`block` slot: its declared
/// name, or `sigil` (`*`/`**`/`&`) when it is anonymous (absent, or a parser
/// `__anon_*` synthetic).
fn anon_name<'a>(name: Option<&'a str>, sigil: &'a str) -> &'a str {
    match name {
        Some(n) if !n.starts_with("__anon") => n,
        _ => sigil,
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
/// read off `ruby 4.0.6` via `def m(x); yield x; end; m([1,2]) { |...| }`:
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
/// Path 1's call sites, which know `args.len()` at zeo-compile-time).
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
/// `zeo_rt::block_auto_splat` at invocation time. The split of a
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

    // `__positional` is a SLICE of the closure's own arguments, not a copy of
    // them. It used to be `#args_ident.to_vec()`, unconditionally: a heap
    // allocation plus a clone of every argument on every invocation of every
    // non-fused block, to serve bindings that only ever `.get(i).cloned()`
    // out of it. Both shapes below sit entirely inside the borrow, and the
    // auto-splat -- the one step that can produce different elements -- owns
    // its result only when a coercion actually applied (`block_auto_splat`
    // answers a `Cow`). Everything downstream reads through `Deref`, so the
    // binding builders are untouched by the distinction.
    let positional_and_kw_source = if has_keywords {
        quote! {
            let (__positional, __kw_source):
                (&[zeo_rt::RubyValue], Option<zeo_rt::RubyValue>) =
                match #args_ident.last() {
                    Some(__v @ zeo_rt::RubyValue::Hash(_)) => {
                        (&#args_ident[..#args_ident.len() - 1], Some(__v.clone()))
                    }
                    _ => (#args_ident, None),
                };
        }
    } else {
        quote! {
            let __positional: &[zeo_rt::RubyValue] = #args_ident;
        }
    };
    let auto_splat = (!is_lambda && auto_splats(params)).then(|| {
        quote! {
            let __positional = zeo_rt::block_auto_splat(__positional)?;
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
            let mut #ident: zeo_rt::RubyValue = __positional.get(#i).cloned().unwrap_or(zeo_rt::RubyValue::Nil);
        }
    });
    let optional_lets = params
        .optional
        .iter()
        .enumerate()
        .map(|(i, (name, default))| {
            let ident = safe_ident(name);
            let default_expr = emit_expr(cx, *default);
            quote! {
                #[allow(unused_mut)]
                let mut #ident: zeo_rt::RubyValue = if #i < __opt_bound {
                    __positional.get(#nreq + #i).cloned().unwrap_or(zeo_rt::RubyValue::Nil)
                } else {
                    #default_expr
                };
            }
        });
    let rest_let = params.rest.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! {
            // `mut` for the same reason every other parameter binding here
            // carries it: a Ruby parameter is an ordinary reassignable local,
            // whatever slot it arrived in.
            #[allow(unused_mut)]
            let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Array(zeo_rt::array_new(
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
            let mut #ident: zeo_rt::RubyValue = {
                let __idx = #nreq + __opt_bound + __rest_count + #i;
                __positional.get(__idx).cloned().unwrap_or(zeo_rt::RubyValue::Nil)
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
            let name_sym = super::pooled_sym(name);
            quote! {
                #[allow(unused_mut)]
                let mut #ident: zeo_rt::RubyValue = match &__kw_source {
                    Some(zeo_rt::RubyValue::Hash(__h))
                        if zeo_rt::hash_has_key(
                            __h,
                            &zeo_rt::RubyValue::Symbol(#name_sym),
                        ) =>
                    {
                        zeo_rt::hash_get(
                            __h,
                            &zeo_rt::RubyValue::Symbol(#name_sym),
                        )
                    }
                    _ => {
                        return Err(zeo_rt::raise_error(
                            "ArgumentError",
                            format!("missing keyword: :{}", #name),
                        ))
                    }
                };
            }
        }
        KeywordParam::Optional(name, default) => {
            let ident = safe_ident(name);
            let name_sym = super::pooled_sym(name);
            let default_expr = emit_expr(cx, *default);
            quote! {
                // `mut`: a keyword parameter is reassignable like any other
                // (tempfile's `def initialize(..., mode: 0, ...)` then does
                // `mode |= File::RDWR`).
                #[allow(unused_mut)]
                let mut #ident: zeo_rt::RubyValue = match &__kw_source {
                    Some(zeo_rt::RubyValue::Hash(__h)) => {
                        let __v = zeo_rt::hash_get(__h, &zeo_rt::RubyValue::Symbol(#name_sym));
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
            #[allow(unused_mut)]
            let mut #ident: zeo_rt::RubyValue = match &__kw_source {
                Some(zeo_rt::RubyValue::Hash(__h)) => {
                    let __declared: &[&str] = &[#(#kw_names),*];
                    // The locked map yields `(&HashKey, &(key, value))` --
                    // the ORIGINAL key/value pair is the stored tuple, the
                    // `HashKey` projection is only the map's own index.
                    zeo_rt::RubyValue::Hash(zeo_rt::hash_new(
                        __h.lock()
                            .iter()
                            .map(|(_, __kv)| __kv.clone())
                            .filter(|(__k, _)| match __k {
                                zeo_rt::RubyValue::Symbol(__s) => !__declared.contains(&__s.name().as_str()),
                                _ => true,
                            })
                            .collect()
                    ))
                }
                _ => zeo_rt::RubyValue::Hash(zeo_rt::hash_new(Vec::new())),
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
            let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Nil;
        }
    });

    // A `&block` parameter (`->(&b) { ... }` / `def obj.m(&b)`) binds from the
    // call-site block `__blk` (the closure's third parameter -- see
    // `emit_proc_or_lambda_value`), so `b.call`/`b.nil?` reflect the block the
    // proc/method was actually called with. Declaring a `&block` param forces
    // the block-carrying closure shape (`needs_blk_param`), so `__blk` is
    // always in scope here -- for an ordinary proc/lambda too, not just a
    // method body: `->(&b) { b.call(9) }.call { ... }` now threads the block
    // through. A blockless call binds nil (`b.nil?` true).
    let block_source = quote! { __blk.clone().unwrap_or(zeo_rt::RubyValue::Nil) };
    let block_let = params.block.iter().flatten().map(|name| {
        let ident = safe_ident(name);
        quote! {
            #[allow(unused_variables, unused_mut)]
            let mut #ident: zeo_rt::RubyValue = #block_source;
        }
    });

    let bindings = quote! {
        #(#required_lets)*
        #(#optional_lets)*
        #(#rest_let)*
        #(#post_lets)*
        #(#keyword_lets)*
        #(#keyword_rest_let)*
        #destructures
        #(#block_local_lets)*
        #(#block_let)*
    };
    // The arity math, emitted only where something reads it.
    //
    // All five used to be emitted unconditionally, under
    // `#[allow(unused_variables)]` -- five `let`s and their attributes, about
    // 250 bytes of Rust in EVERY method body. Most methods declare no optional
    // and no rest parameter, so most of it was dead: 13,012 sites across
    // activemodel's generated Rust, and it multiplies through every shared and
    // module body.
    //
    // Which are live is decided by looking at the emitted bindings rather than
    // by re-deriving it from `nopt`/`has_rest`/`npost`: the consumers are
    // spread over six builders above, and a rule restated here would drift
    // from them silently -- into a `cannot find value __extra`, or back into
    // dead code nobody notices. Walked in reverse declaration order so a name
    // one `let` needs is still seen: `__rest_count` reads `__opt_bound` and
    // `__extra`, `__extra` reads `__min` and `__n`.
    let arity_math = {
        let mut used = bindings.to_string();
        let mut lets: Vec<TokenStream> = Vec::new();
        for (name, decl) in [
            (
                "__rest_count",
                quote! { let __rest_count = if #has_rest { __extra.saturating_sub(__opt_bound) } else { 0 }; },
            ),
            (
                "__opt_bound",
                quote! { let __opt_bound = __extra.min(#nopt); },
            ),
            (
                "__extra",
                quote! { let __extra = __n.saturating_sub(__min); },
            ),
            ("__min", quote! { let __min = #nreq + #npost; }),
            ("__n", quote! { let __n = __positional.len(); }),
        ] {
            if mentions_ident(&used, name) {
                used.push_str(&decl.to_string());
                lets.push(decl);
            }
        }
        lets.reverse();
        quote! { #(#lets)* }
    };
    quote! {
        #positional_and_kw_source
        #auto_splat
        #arity_math
        #bindings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::FMap;
    use crate::hir::{Hir, HirNode};

    /// A throwaway default-value expression -- `proc_arity`/`auto_splats`
    /// only ever count parameters, never look at what a default evaluates
    /// to, so one shared `nil` node serves every optional slot.
    fn nil_default(hir: &mut Hir) -> crate::hir::NodeId {
        hir.push(HirNode::NilLit)
    }

    /// Builds a `Params` from a compact spec: `req`, `opt`, `post` counts,
    /// plus rest/kwrest flags and keyword kinds.
    #[derive(Default)]
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

    /// Every case here was READ OFF ruby 4.0.6 (`p ->(...) {}.arity` /
    /// `p proc { |...| }.arity`), not derived from our own implementation --
    /// this table IS the specification. See `proc_arity`'s docs for the
    /// formula it encodes.
    #[test]
    fn proc_arity_matches_the_ruby_oracle_for_every_signature_shape() {
        let hir = &mut Hir::default();
        // (spec, is_lambda, expected) -- the Ruby source each row mirrors
        // is named in the comment.
        let cases: Vec<(Spec, bool, i32, &str)> = vec![
            (
                Spec {
                    ..Default::default()
                },
                false,
                0,
                "proc {}",
            ),
            (
                Spec {
                    req: 1,
                    ..Default::default()
                },
                false,
                1,
                "proc { |a| }",
            ),
            (
                Spec {
                    req: 2,
                    ..Default::default()
                },
                false,
                2,
                "proc { |a, b| }",
            ),
            (
                Spec {
                    req: 2,
                    ..Default::default()
                },
                true,
                2,
                "->(a, b) {}",
            ),
            (
                Spec {
                    ..Default::default()
                },
                true,
                0,
                "->() {}",
            ),
            (
                Spec {
                    block: true,
                    ..Default::default()
                },
                true,
                0,
                "->(&b) {}",
            ),
            // An optional/rest param makes a LAMBDA negative...
            (
                Spec {
                    req: 1,
                    opt: 1,
                    ..Default::default()
                },
                true,
                -2,
                "->(a, b = 1) {}",
            ),
            (
                Spec {
                    req: 1,
                    opt: 2,
                    ..Default::default()
                },
                true,
                -2,
                "lambda { |a, b = 1, c = 2| }",
            ),
            // ...but a plain proc reports its MINIMUM instead (max is still
            // bounded, so no negation) -- the shape that makes the
            // lambda/proc distinction visible.
            (
                Spec {
                    req: 1,
                    opt: 1,
                    ..Default::default()
                },
                false,
                1,
                "proc { |a, b = 1| }",
            ),
            // A rest param is unbounded: negative for proc AND lambda.
            (
                Spec {
                    req: 1,
                    rest: true,
                    ..Default::default()
                },
                false,
                -2,
                "proc { |a, *b| }",
            ),
            (
                Spec {
                    rest: true,
                    ..Default::default()
                },
                false,
                -1,
                "proc { |*a| }",
            ),
            (
                Spec {
                    req: 1,
                    rest: true,
                    ..Default::default()
                },
                true,
                -2,
                "->(a, *b) {}",
            ),
            (
                Spec {
                    req: 2,
                    rest: true,
                    ..Default::default()
                },
                false,
                -3,
                "proc { |a, b, *c| }",
            ),
            // Post params are required: they count toward the minimum.
            (
                Spec {
                    req: 1,
                    rest: true,
                    post: 1,
                    ..Default::default()
                },
                true,
                -3,
                "->(a, *b, c) {}",
            ),
            (
                Spec {
                    req: 1,
                    rest: true,
                    post: 2,
                    ..Default::default()
                },
                true,
                -4,
                "->(a, *b, c, d) {}",
            ),
            // A REQUIRED keyword adds exactly one mandatory slot, however
            // many there are -- and keeps the count positive.
            (
                Spec {
                    req: 1,
                    kw_required: 1,
                    ..Default::default()
                },
                true,
                2,
                "->(a, b:) {}",
            ),
            (
                Spec {
                    kw_required: 1,
                    ..Default::default()
                },
                true,
                1,
                "->(b:) {}",
            ),
            (
                Spec {
                    req: 1,
                    kw_required: 2,
                    ..Default::default()
                },
                true,
                2,
                "->(a, b:, c:) {}",
            ),
            (
                Spec {
                    req: 1,
                    kw_required: 1,
                    kw_optional: 1,
                    ..Default::default()
                },
                true,
                2,
                "->(a, b:, c: 1) {}",
            ),
            (
                Spec {
                    req: 1,
                    kw_required: 1,
                    kwrest: true,
                    ..Default::default()
                },
                true,
                2,
                "->(a, e:, **g) {}",
            ),
            (
                Spec {
                    req: 1,
                    kw_required: 1,
                    ..Default::default()
                },
                false,
                2,
                "proc { |a, b:| }",
            ),
            // An OPTIONAL keyword / **kwrest alone widens the maximum, so a
            // lambda goes negative while a proc reports its minimum.
            (
                Spec {
                    req: 1,
                    kw_optional: 1,
                    ..Default::default()
                },
                true,
                -2,
                "->(a, b: 1) {}",
            ),
            (
                Spec {
                    req: 1,
                    kwrest: true,
                    ..Default::default()
                },
                true,
                -2,
                "->(a, **k) {}",
            ),
            (
                Spec {
                    req: 1,
                    kw_optional: 1,
                    ..Default::default()
                },
                false,
                1,
                "proc { |a, b: 1| }",
            ),
            (
                Spec {
                    req: 1,
                    kwrest: true,
                    ..Default::default()
                },
                false,
                1,
                "proc { |a, **k| }",
            ),
            // Everything at once.
            (
                Spec {
                    req: 1,
                    opt: 1,
                    rest: true,
                    post: 1,
                    kw_required: 1,
                    kw_optional: 1,
                    kwrest: true,
                    block: true,
                },
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
            (
                Spec {
                    ..Default::default()
                },
                false,
                "{ }",
            ),
            (
                Spec {
                    rest: true,
                    ..Default::default()
                },
                false,
                "{ |*a| }",
            ),
            (
                Spec {
                    rest: true,
                    kwrest: true,
                    ..Default::default()
                },
                false,
                "{ |*a, **k| }",
            ),
            (
                Spec {
                    kw_required: 1,
                    ..Default::default()
                },
                false,
                "{ |a:| }",
            ),
            // `ambiguous_param0`: exactly one lead and nothing else
            // positional. Keywords do NOT disturb the exemption.
            (
                Spec {
                    req: 1,
                    ..Default::default()
                },
                false,
                "{ |a| }",
            ),
            (
                Spec {
                    req: 1,
                    kwrest: true,
                    ..Default::default()
                },
                false,
                "{ |a, **k| }",
            ),
            (
                Spec {
                    req: 1,
                    kw_required: 1,
                    ..Default::default()
                },
                false,
                "{ |a, b:| }",
            ),
            // A LONE optional is the `|a|` case by another spelling -- one
            // slot, no lead, so nothing forces a spread.
            (
                Spec {
                    opt: 1,
                    ..Default::default()
                },
                false,
                "{ |a = 9| }",
            ),
            // The subtle one: one optional plus a rest still has no lead and
            // only one optional, so it does NOT splat -- unlike `|a, *b|`
            // just below, which is identical but for the lead. This pair is
            // why the rule can't be "count the positional slots".
            (
                Spec {
                    opt: 1,
                    rest: true,
                    ..Default::default()
                },
                false,
                "{ |a = 5, *b| }",
            ),
            // lead + post > 0 -> splats.
            (
                Spec {
                    req: 2,
                    ..Default::default()
                },
                true,
                "{ |a, b| }",
            ),
            (
                Spec {
                    req: 2,
                    kwrest: true,
                    ..Default::default()
                },
                true,
                "{ |a, b, **k| }",
            ),
            (
                Spec {
                    req: 1,
                    rest: true,
                    ..Default::default()
                },
                true,
                "{ |a, *b| }",
            ),
            (
                Spec {
                    req: 1,
                    opt: 1,
                    ..Default::default()
                },
                true,
                "{ |a, b = 5| }",
            ),
            (
                Spec {
                    req: 1,
                    rest: true,
                    post: 1,
                    ..Default::default()
                },
                true,
                "{ |a, *b, c| }",
            ),
            (
                Spec {
                    req: 1,
                    post: 1,
                    ..Default::default()
                },
                true,
                "{ |a, (b)| }-shaped post",
            ),
            (
                Spec {
                    rest: true,
                    post: 1,
                    ..Default::default()
                },
                true,
                "{ |*a, b| }",
            ),
            (
                Spec {
                    rest: true,
                    post: 2,
                    ..Default::default()
                },
                true,
                "{ |*a, b, c| }",
            ),
            (
                Spec {
                    opt: 1,
                    rest: true,
                    post: 1,
                    ..Default::default()
                },
                true,
                "{ |a = 5, *b, c| }",
            ),
            // opt > 1 -> splats, with no lead at all.
            (
                Spec {
                    opt: 2,
                    ..Default::default()
                },
                true,
                "{ |a = 5, b = 4| }",
            ),
            (
                Spec {
                    opt: 2,
                    kwrest: true,
                    ..Default::default()
                },
                true,
                "{ |a = 1, b = 2, **k| }",
            ),
            (
                Spec {
                    opt: 2,
                    rest: true,
                    ..Default::default()
                },
                true,
                "{ |a = 5, b = 4, *c| }",
            ),
            (
                Spec {
                    opt: 3,
                    ..Default::default()
                },
                true,
                "{ |a = 1, b = 2, c = 3| }",
            ),
            // A trailing comma (`|a, |`) lowers to an anonymous rest, which
            // breaks the exemption exactly as `*b` does.
            (
                Spec {
                    req: 1,
                    rest: true,
                    ..Default::default()
                },
                true,
                "{ |a, | }",
            ),
        ];
        for (spec, expected, source) in cases {
            let p = params(hir, spec);
            assert_eq!(
                auto_splats(&p),
                expected,
                "auto-splat of a block `{source}`"
            );
        }
    }

    /// A lambda's parameters never auto-splat, whatever their shape (real
    /// Ruby: a lambda is strict, `->(a, b) {}.call([1, 2])` is an
    /// ArgumentError, not a destructure) -- enforced at the CALLER, which
    /// consults `is_lambda` before ever asking `auto_splats`.
    #[test]
    fn auto_splats_is_only_consulted_for_non_lambdas() {
        let hir = &mut Hir::default();
        let p = params(
            hir,
            Spec {
                req: 2,
                ..Default::default()
            },
        );
        // The shape itself says "splat"...
        assert!(auto_splats(&p));
        // ...and `emit_proc_param_bindings` gates on `!is_lambda`, so the
        // emitted lambda body carries no auto-splat call at all.
        let cx_free = emit_proc_param_bindings_probe(&p, true);
        assert!(
            !cx_free.contains("block_auto_splat"),
            "lambda body: {cx_free}"
        );
        let proc_body = emit_proc_param_bindings_probe(&p, false);
        assert!(
            proc_body.contains("block_auto_splat"),
            "proc body: {proc_body}"
        );
    }

    /// `emit_proc_param_bindings` needs a `Ctx` (for default-value
    /// expressions); this probe builds the minimal one a param list with no
    /// defaults requires.
    fn emit_proc_param_bindings_probe(params: &Params, is_lambda: bool) -> String {
        let compiler = crate::compiler::Compiler::new(Hir::default());
        let label_counter = std::cell::Cell::new(0u32);
        let empty = FSet::default();
        let cx = Ctx {
            compiler: &compiler,
            box_id: 0,
            current_class: None,
            defining_class: None,
            class_self: None,
            current_method: None,
            current_method_origin: None,
            local_types: std::borrow::Cow::Owned(FMap::default()),
            label_counter: &label_counter,
            loop_labels: None,
            next_yields_value: false,
            for_var_override: None,
            captured_locals: std::borrow::Cow::Borrowed(&empty),
            binding_names: None,
            in_eval_splice: false,
            self_ident: quote::format_ident!("__self"),
            in_real_proc: false,
            self_is_dynamic: false,
            self_slots: false,
            shared_body: false,
            trace: None,
            runtime_super_params: None,
            defined_by_define_method: false,
            lexical_frame_label: None,
            block_depth: 0,
            has_blk_binding: false,
        };
        emit_proc_param_bindings(&cx, params, &format_ident!("__args"), is_lambda).to_string()
    }
}
