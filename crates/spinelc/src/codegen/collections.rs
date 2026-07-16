//! Emits `Array`/`Hash`/`Range`/`String` literals -- split out of `expr.rs`
//! purely for size (mirrors `call.rs` holding all call-emission logic used
//! *from* `expr.rs`). Every fragment here still evaluates to a bare
//! `spinel_rt::RubyValue`, matching every other `emit_*` fragment's
//! contract.

use quote::quote;

use super::expr::{box_if_object_typed, emit_boxed_new, emit_expr};
use super::Ctx;
use crate::hir::{ArrayElem, HashPair, NodeId, RegexpFlags, StrPart};
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

/// `{ a: 1, b: 2 }` -- built via repeated `spinel_rt::hash_set` calls (not a
/// raw `Vec` literal) so a duplicate key keeps its *last* value, matching
/// real Ruby (see `spinel_rt::hash_new`'s docs).
pub fn emit_hash_lit(cx: &Ctx, pairs: &[HashPair]) -> TokenStream {
    let inserts = pairs.iter().map(|HashPair(k, v)| {
        let k_expr = emit_expr(cx, *k);
        let k_expr = box_if_object_typed(cx, *k, k_expr);
        let v_expr = emit_expr(cx, *v);
        let v_expr = box_if_object_typed(cx, *v, v_expr);
        quote! { __pairs.push((#k_expr, #v_expr)); }
    });
    quote! {
        spinel_rt::RubyValue::Hash({
            #[allow(unused_mut)]
            let mut __pairs: Vec<(spinel_rt::RubyValue, spinel_rt::RubyValue)> = Vec::new();
            #(#inserts)*
            spinel_rt::hash_new(__pairs)
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
