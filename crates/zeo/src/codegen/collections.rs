//! Emits `Array`/`Hash`/`Range`/`String` literals -- split out of `expr.rs`
//! purely for size (mirrors `call.rs` holding all call-emission logic used
//! *from* `expr.rs`). Every fragment here still evaluates to a bare
//! `zeo_rt::RubyValue`, matching every other `emit_*` fragment's
//! contract.

use quote::{format_ident, quote};

use super::Ctx;
use super::expr::{box_if_object_typed, emit_boxed_new, emit_expr};
use crate::hir::{ArrayElem, KwArg, NodeId, RegexpFlags, StrPart};
use proc_macro2::TokenStream;

/// `[1, 2, *rest]` -- a plain element is pushed directly; a `*splat`
/// element's runtime value is flattened in via
/// `zeo_rt::array_splat_into` (see its docs for why this can't be a
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
            quote! { zeo_rt::array_splat_into(&mut __arr, &(#v))?; }
        }
    });
    quote! {
        zeo_rt::RubyValue::Array({
            #[allow(unused_mut)]
            let mut __arr: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#pushes)*
            zeo_rt::array_new(__arr)
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
            quote! { zeo_rt::hash_set(&#hash_ident, #k_expr, #v_expr); }
        }
        // `**obj` converts through `to_hash` like CRuby, so a non-Hash
        // converter object splats (and a non-converter raises a real
        // TypeError); boxed first, since the coercion takes a `RubyValue`.
        KwArg::DoubleSplat(n) => {
            let e = box_if_object_typed(cx, *n, emit_expr(cx, *n));
            quote! {
                for (__k, __v) in zeo_rt::to_hash_coerce(&(#e))?.lock().values().cloned().collect::<Vec<_>>() {
                    zeo_rt::hash_set(&#hash_ident, __k, __v);
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
            let KwArg::Pair(k, v) = kw else {
                unreachable!("guarded all-Pair above")
            };
            let k_expr = box_if_object_typed(cx, *k, emit_expr(cx, *k));
            let v_expr = box_if_object_typed(cx, *v, emit_expr(cx, *v));
            quote! { __pairs.push((#k_expr, #v_expr)); }
        });
        return quote! {
            zeo_rt::RubyValue::Hash({
                #[allow(unused_mut)]
                let mut __pairs: Vec<(zeo_rt::RubyValue, zeo_rt::RubyValue)> = Vec::new();
                #(#inserts)*
                zeo_rt::hash_new(__pairs)
            })
        };
    }
    let inserts = emit_kwarg_inserts(cx, kwargs, &quote! { __h });
    quote! {
        zeo_rt::RubyValue::Hash({
            let __h = zeo_rt::hash_new(vec![]);
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
    quote! { zeo_rt::RubyValue::Range(#start_expr, #end_expr, #exclusive) }
}

/// A (possibly-interpolated) string literal -- a single `Lit` part (the
/// common, no-`#{}` case) skips the `String`-builder scaffolding entirely.
/// An interpolated `#{expr}` part is stringified via `to_display_string`
/// (mirrors real Ruby: interpolation calls `to_s`, not `inspect`).
pub fn emit_string_lit(cx: &Ctx, parts: &[StrPart]) -> TokenStream {
    // A `# encoding:` magic comment tags EVERY literal in the file with that
    // encoding (byte-built); otherwise a raw-byte segment forces ASCII-8BIT,
    // and a purely-UTF-8 literal keeps the readable String path.
    let script_enc = cx.compiler.hir.script_encoding.as_deref();
    if let Some(name) = script_enc {
        let enc = format_ident!("{name}");
        let pieces = parts.iter().map(|p| string_lit_bytes_piece(cx, p));
        return quote! {
            zeo_rt::RubyValue::Str(zeo_rt::string_from_bytes({
                #[allow(unused_mut)]
                let mut __b: Vec<u8> = Vec::new();
                #(#pieces)*
                __b
            }, zeo_rt::encoding::#enc))
        };
    }
    if let [StrPart::Lit(s)] = parts {
        // `# frozen_string_literal: true`: a non-interpolated literal is its
        // interned, frozen twin (equal literals share one object, and
        // mutation raises). Interpolated literals below stay mutable.
        if cx.compiler.hir.frozen_string_literal {
            return quote! {
                zeo_rt::RubyValue::Str(zeo_rt::intern_frozen(
                    zeo_rt::encoding::StrBuf::from_utf8(#s.to_string()),
                ))
            };
        }
        return quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new(#s.to_string())) };
    }
    if let [StrPart::Bytes(b)] = parts {
        let bytes = byte_literals(b);
        return quote! {
            zeo_rt::RubyValue::Str(zeo_rt::string_from_bytes(
                vec![#(#bytes),*],
                zeo_rt::encoding::UTF_8,
            ))
        };
    }
    // Any raw-byte segment makes the WHOLE literal byte-built, tagged with the
    // SOURCE encoding (UTF-8 by default -- an invalid `\xNN` literal stays
    // UTF-8-and-invalid, matching CRuby; the `# encoding:` magic comment case
    // is handled above). A purely-UTF-8 literal keeps the readable String path.
    if parts.iter().any(|p| matches!(p, StrPart::Bytes(_))) {
        let pieces = parts.iter().map(|p| string_lit_bytes_piece(cx, p));
        return quote! {
            zeo_rt::RubyValue::Str(zeo_rt::string_from_bytes({
                #[allow(unused_mut)]
                let mut __b: Vec<u8> = Vec::new();
                #(#pieces)*
                __b
            }, zeo_rt::encoding::UTF_8))
        };
    }
    let pieces = parts.iter().map(|p| match p {
        StrPart::Lit(s) => quote! { __s.push_str(#s); },
        StrPart::Bytes(_) => unreachable!("byte segments took the byte-built path above"),
        StrPart::Interp(n) => {
            let e = emit_expr(cx, *n);
            // Boxed if Object-typed: interpolation reaches
            // `try_display_string`, which dispatches a user-defined `to_s`
            // -- and a RAISING one propagates via `?` (catchable at the
            // interpolation site, CRuby's rule).
            let e = super::expr::box_if_object_typed(cx, *n, e);
            quote! { __s.push_str(&(#e).try_display_string()?); }
        }
    });
    quote! {
        zeo_rt::RubyValue::Str(zeo_rt::string_new({
            #[allow(unused_mut)]
            let mut __s = String::new();
            #(#pieces)*
            __s
        }))
    }
}

/// The `u8` token literals for a raw-byte string segment.
fn byte_literals(bytes: &[u8]) -> Vec<TokenStream> {
    bytes.iter().map(|b| quote! { #b }).collect()
}

/// One string-literal segment appended to the `__b: Vec<u8>` byte builder --
/// shared by the raw-byte and magic-comment-encoding literal paths.
fn string_lit_bytes_piece(cx: &Ctx, part: &StrPart) -> TokenStream {
    match part {
        StrPart::Lit(s) => quote! { __b.extend_from_slice(#s.as_bytes()); },
        StrPart::Bytes(b) => {
            let bytes = byte_literals(b);
            quote! { __b.extend_from_slice(&[#(#bytes),*]); }
        }
        StrPart::Interp(n) => {
            let e = super::expr::box_if_object_typed(cx, *n, emit_expr(cx, *n));
            quote! { __b.extend_from_slice((#e).try_display_string()?.as_bytes()); }
        }
    }
}

/// `/pattern/flags` / `%r{pattern}flags`, possibly interpolated -- the
/// pattern text is assembled exactly like `emit_string_lit`'s general
/// (interpolated) case, then compiled at the construction site via
/// `zeo_rt::regexp_new`. A compile failure raises a real, catchable
/// `RegexpError` -- checked EVERY time this literal is reached, even for a
/// non-interpolated pattern that could in principle be validated once at
/// `zeo` compile time instead (a documented, narrower-timing
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
            // A regexp source is UTF-8; a stray raw-byte segment is rendered
            // lossily (non-UTF-8 patterns aren't otherwise modeled).
            StrPart::Bytes(b) => {
                let s = String::from_utf8_lossy(b).into_owned();
                quote! { __pat.push_str(#s); }
            }
            StrPart::Interp(n) => {
                // Boxed if Object-typed, same as string interpolation: the
                // display protocol dispatches a user `to_s` and needs a
                // `RubyValue` receiver.
                let e = box_if_object_typed(cx, *n, emit_expr(cx, *n));
                quote! { __pat.push_str(&(#e).try_display_string()?); }
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
        vec![quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new(__err)) }],
    );
    quote! {
        match zeo_rt::regexp_new(&(#pattern_expr), #ignore_case, #extended, #multiline) {
            // A regexp LITERAL is frozen at birth (real Ruby since 3.0,
            // interpolated ones included); `Regexp.new`/`.union` stay
            // unfrozen by not passing through this emission.
            Ok(__re) => {
                __re.set_frozen();
                zeo_rt::RubyValue::Regexp(__re)
            }
            Err(__err) => return Err(zeo_rt::Signal::Raise(#regexp_error)),
        }
    }
}
