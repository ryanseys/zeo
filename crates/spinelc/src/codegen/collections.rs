//! Emits `Array`/`Hash`/`Range`/`String` literals -- split out of `expr.rs`
//! purely for size (mirrors `call.rs` holding all call-emission logic used
//! *from* `expr.rs`). Every fragment here still evaluates to a bare
//! `spinel_rt::RubyValue`, matching every other `emit_*` fragment's
//! contract.

use quote::quote;

use super::expr::{box_if_object_typed, emit_boxed_new, emit_expr};
use super::Ctx;
use crate::hir::{ArrayElem, KwArg, NodeId, RegexpFlags, StrPart};
use proc_macro2::TokenStream;

/// `[1, 2, *rest]` -- a plain element is pushed directly; a `*splat`
/// element's runtime value is flattened in via
/// `spinel_rt::array_splat_into` (see its docs for why this can't be a
/// simple `.extend()` call site -- the splatted value's own `RubyValue`
/// variant isn't known until `emit_expr` runs).
pub fn emit_array_lit(cx: &Ctx, elems: &[ArrayElem]) -> TokenStream {
    let pushes = elems.iter().map(|e| match e {
        ArrayElem::Single(n) => {
            let v = emit_expr(cx, *n);
            let v = box_if_object_typed(cx, *n, v);
            quote! { __arr.push(#v); }
        }
        ArrayElem::Splat(n) => {
            let v = emit_expr(cx, *n);
            // Boxed like a plain element: splatting a non-Array is ordinary
            // Ruby (`[*obj]` consults `obj.to_a`), so the operand can be an
            // Object-typed expression emitting a bare `Arc<Concrete>` --
            // which `array_splat_into`'s `&RubyValue` won't take. It never
            // came up while only Arrays could be splatted.
            let v = box_if_object_typed(cx, *n, v);
            quote! { spinel_rt::array_splat_into(&mut __arr, &(#v))?; }
        }
    });
    quote! {
        spinel_rt::RubyValue::Array({
            #[allow(unused_mut)]
            let mut __arr: Vec<spinel_rt::RubyValue> = Vec::new();
            #(#pushes)*
            spinel_rt::array_new(__arr)
        })
    }
}

/// Emits the in-order statements that populate an already-created runtime
/// hash (bound to `hash_ident`) from a `KwArg` list: a `Pair` sets one key; a
/// `DoubleSplat` merges every entry of a `to_hash`-coerced value AT ITS
/// POSITION. `hash_set` preserves insertion order and overwrites on a
/// duplicate key, so this reproduces Ruby's left-to-right, last-key-wins
/// merge for `f(**a, c: 1, **b)`. Shared by `emit_hash_lit`, `call`'s
/// `emit_splat_call`, and `yield`'s trailing hash so the three can't drift.
pub fn emit_kwarg_inserts(cx: &Ctx, kwargs: &[KwArg], hash_ident: &TokenStream) -> TokenStream {
    let stmts = kwargs.iter().map(|kw| match kw {
        KwArg::Pair(k, v) => {
            let k_expr = box_if_object_typed(cx, *k, emit_expr(cx, *k));
            let v_expr = box_if_object_typed(cx, *v, emit_expr(cx, *v));
            quote! { spinel_rt::hash_set(&#hash_ident, #k_expr, #v_expr); }
        }
        // `**obj` converts through `to_hash` like CRuby, so a non-Hash
        // converter object splats (and a non-converter raises a real
        // TypeError); boxed first, since the coercion takes a `RubyValue`.
        KwArg::DoubleSplat(n) => {
            let e = box_if_object_typed(cx, *n, emit_expr(cx, *n));
            quote! {
                for (__k, __v) in spinel_rt::to_hash_coerce(&(#e))?.lock().values().cloned().collect::<Vec<_>>() {
                    spinel_rt::hash_set(&#hash_ident, __k, __v);
                }
            }
        }
    });
    quote! { #(#stmts)* }
}

/// `{ a: 1, b: 2, **other }` -- an all-`Pair` literal takes the one-shot
/// `hash_new(Vec<(k,v)>)` fast path (unchanged, so the common case emits
/// exactly what it always did); a `**` splat forces incremental building via
/// `emit_kwarg_inserts` so each merge lands at its source position. NO
/// empty-suppression: a literal `{**h}` with an empty `h` is legitimately
/// `{}` (unlike a call/yield arg's trailing hash, which drops an empty one).
pub fn emit_hash_lit(cx: &Ctx, kwargs: &[KwArg]) -> TokenStream {
    if kwargs.iter().all(|kw| matches!(kw, KwArg::Pair(..))) {
        let inserts = kwargs.iter().map(|kw| {
            let KwArg::Pair(k, v) = kw else { unreachable!("guarded all-Pair above") };
            let k_expr = box_if_object_typed(cx, *k, emit_expr(cx, *k));
            let v_expr = box_if_object_typed(cx, *v, emit_expr(cx, *v));
            quote! { __pairs.push((#k_expr, #v_expr)); }
        });
        return quote! {
            spinel_rt::RubyValue::Hash({
                #[allow(unused_mut)]
                let mut __pairs: Vec<(spinel_rt::RubyValue, spinel_rt::RubyValue)> = Vec::new();
                #(#inserts)*
                spinel_rt::hash_new(__pairs)
            })
        };
    }
    let inserts = emit_kwarg_inserts(cx, kwargs, &quote! { __h });
    quote! {
        spinel_rt::RubyValue::Hash({
            let __h = spinel_rt::hash_new(vec![]);
            #inserts
            __h
        })
    }
}

/// `a..b` / `a...b` -- either endpoint may be absent (see `HirNode::RangeLit`).
pub fn emit_range_lit(
    cx: &Ctx,
    start: Option<NodeId>,
    end: Option<NodeId>,
    exclusive: bool,
) -> TokenStream {
    // Endpoints are boxed if Object-typed: they land in
    // `RubyValue::Range`'s `Box<RubyValue>` payload.
    let start_expr = match start {
        Some(n) => {
            let e = emit_expr(cx, n);
            let e = box_if_object_typed(cx, n, e);
            quote! { Some(Box::new(#e)) }
        }
        None => quote! { None },
    };
    let end_expr = match end {
        Some(n) => {
            let e = emit_expr(cx, n);
            let e = box_if_object_typed(cx, n, e);
            quote! { Some(Box::new(#e)) }
        }
        None => quote! { None },
    };
    quote! { spinel_rt::RubyValue::Range(#start_expr, #end_expr, #exclusive) }
}

/// A (possibly-interpolated) string literal -- a single `Lit` part (the
/// common, no-`#{}` case) skips the `String`-builder scaffolding entirely.
/// An interpolated `#{expr}` part is stringified via `to_display_string`
/// (mirrors real Ruby: interpolation calls `to_s`, not `inspect`).
pub fn emit_string_lit(cx: &Ctx, parts: &[StrPart]) -> TokenStream {
    if let [StrPart::Lit(s)] = parts {
        return quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(#s.to_string())) };
    }
    let pieces = parts.iter().map(|p| match p {
        StrPart::Lit(s) => quote! { __s.push_str(#s); },
        StrPart::Interp(n) => {
            let e = emit_expr(cx, *n);
            // Boxed if Object-typed (Phase 16.2): interpolation reaches
            // `to_display_string`, which dispatches a user-defined `to_s`.
            let e = super::expr::box_if_object_typed(cx, *n, e);
            quote! { __s.push_str(&(#e).to_display_string()); }
        }
    });
    quote! {
        spinel_rt::RubyValue::Str(spinel_rt::string_new({
            #[allow(unused_mut)]
            let mut __s = String::new();
            #(#pieces)*
            __s
        }))
    }
}

/// `/pattern/flags` / `%r{pattern}flags`, possibly interpolated -- the
/// pattern text is assembled exactly like `emit_string_lit`'s general
/// (interpolated) case, then compiled at the construction site via
/// `spinel_rt::regexp_new`. A compile failure raises a real, catchable
/// `RegexpError` -- checked EVERY time this literal is reached, even for a
/// non-interpolated pattern that could in principle be validated once at
/// `spinelc` compile time instead (a documented, narrower-timing
/// approximation of real Ruby's own parse-time `SyntaxError` for a static
/// pattern -- see `hir::HirNode::RegexpLit`'s docs; not silent wrongness,
/// since an invalid pattern is still caught, just one step later than real
/// Ruby catches it).
pub fn emit_regexp_lit(cx: &Ctx, parts: &[StrPart], flags: RegexpFlags) -> TokenStream {
    let pattern_expr = if let [StrPart::Lit(s)] = parts {
        quote! { #s.to_string() }
    } else {
        let pieces = parts.iter().map(|p| match p {
            StrPart::Lit(s) => quote! { __pat.push_str(#s); },
            StrPart::Interp(n) => {
                let e = emit_expr(cx, *n);
                quote! { __pat.push_str(&(#e).to_display_string()); }
            }
        });
        quote! {
            {
                #[allow(unused_mut)]
                let mut __pat = String::new();
                #(#pieces)*
                __pat
            }
        }
    };
    let ignore_case = flags.ignore_case;
    let extended = flags.extended;
    let multiline = flags.multiline;
    let regexp_error = emit_boxed_new(
        cx,
        "RegexpError",
        vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(__err)) }],
    );
    quote! {
        match spinel_rt::regexp_new(&(#pattern_expr), #ignore_case, #extended, #multiline) {
            Ok(__re) => spinel_rt::RubyValue::Regexp(__re),
            Err(__err) => return Err(spinel_rt::Signal::Raise(#regexp_error)),
        }
    }
}
