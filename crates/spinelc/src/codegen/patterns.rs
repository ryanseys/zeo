//! `case/in` pattern matching -- the one DRY core (`emit_pattern_match`) used
//! identically by every `case/in` arm, `expr in pattern`, and `expr =>
//! pattern` (see `hir.rs`'s `Pattern`/`PatternArm` docs for the tree shape).
//!
//! Every pattern kind compiles to a single Rust `bool`-typed expression,
//! built as a labeled BLOCK EXPRESSION (`'label: { ...; break 'label false;
//! ...; true }`) rather than a pure `&&`/`||` chain -- a plain boolean chain
//! can't express "check the arity, THEN check each element, THEN bind the
//! rest" cleanly once a pattern needs several sequential steps (array/hash/
//! find patterns), whereas a labeled block gives an early-exit "abort this
//! pattern, try the next arm" escape hatch that composes with ordinary `if`/
//! `&&`/`||` everywhere else. A pattern that BINDS (`Bind`/`Capture`, and
//! every named rest/shorthand slot) does so via `codegen::hoisting::emit_local_write`
//! -- exactly the same already-hoisted mutable-local storage `if`/`case`
//! branch locals and `MultiWrite` targets use, so a matched binding survives
//! precisely as long as real Ruby's does (the rest of the enclosing method
//! scope), no new storage class needed.

use quote::{format_ident, quote};

use super::expr::{emit_expr, infer};
use super::ident::safe_ident;
use super::loops::fresh_label;
use super::Ctx;
use crate::hir::{HashPatternRest, NodeId, Pattern, PatternArm};
use crate::types::TyKind;
use proc_macro2::TokenStream;
use std::collections::HashMap;

/// A fresh, function-body-unique plain identifier -- the non-lifetime
/// counterpart to `loops::fresh_label`, sharing the SAME `Ctx::label_counter`
/// (harmless: it only needs to be unique within one generated function body,
/// not to avoid colliding with loop label numbering specifically).
fn fresh_temp(cx: &Ctx, tag: &str) -> proc_macro2::Ident {
    let n = cx.label_counter.get();
    cx.label_counter.set(n + 1);
    format_ident!("__pat_{}_{}", tag, n)
}

/// `case subject; in PATTERN [if/unless GUARD] ... [else ...] end`. Arms are
/// tested top to bottom (first match wins), building a nested `if/else`
/// chain exactly like `codegen::expr::emit_case_when` -- NOT a native Rust
/// `match`, for the same reason: a pattern's class-check/destructure/guard
/// logic can't be expressed as Rust's own structural patterns generically.
pub fn emit_case_in(
    cx: &Ctx,
    subject: NodeId,
    arms: &[PatternArm],
    else_body: &Option<Vec<NodeId>>,
) -> TokenStream {
    let subject_expr = emit_expr(cx, subject);
    let subject_ty = infer(cx, subject);

    let mut chain = match else_body {
        Some(body) => super::stmt::emit_body(cx, body, false),
        None => emit_no_matching_pattern_raise(cx),
    };

    for arm in arms.iter().rev() {
        let arm_cx = cx.with_narrowed_locals(collect_narrowing(&arm.pattern));
        let cond = emit_pattern_match(&arm_cx, &arm.pattern, subject_ty, &quote! { __subject });
        let full_cond = match &arm.guard {
            None => cond,
            Some((g, is_unless)) => {
                let g_expr = emit_expr(&arm_cx, *g);
                let g_check = if *is_unless {
                    quote! { !(#g_expr).truthy() }
                } else {
                    quote! { (#g_expr).truthy() }
                };
                quote! { (#cond) && (#g_check) }
            }
        };
        let body_val = super::stmt::emit_body(&arm_cx, &arm.body, false);
        chain = quote! {
            if #full_cond { #body_val } else { #chain }
        };
    }

    quote! { { let __subject = #subject_expr; #chain } }
}

/// `expr in pattern` -- boolean one-liner, never raises. Any binding the
/// pattern makes on a successful match still leaks into the enclosing
/// method scope (matching real Ruby), exactly like a `case/in` arm's own
/// bindings do.
pub fn emit_match_predicate(cx: &Ctx, subject: NodeId, pattern: &Pattern) -> TokenStream {
    let subject_expr = emit_expr(cx, subject);
    let subject_ty = infer(cx, subject);
    let arm_cx = cx.with_narrowed_locals(collect_narrowing(pattern));
    let cond = emit_pattern_match(&arm_cx, pattern, subject_ty, &quote! { __subject });
    quote! { { let __subject = #subject_expr; spinel_rt::RubyValue::Bool(#cond) } }
}

/// `expr => pattern` -- raises `NoMatchingPatternError` on failure, `nil`
/// otherwise (real Ruby: this form's own value is never used for anything
/// but its binding/raising side effect).
pub fn emit_match_required(cx: &Ctx, subject: NodeId, pattern: &Pattern) -> TokenStream {
    let subject_expr = emit_expr(cx, subject);
    let subject_ty = infer(cx, subject);
    let arm_cx = cx.with_narrowed_locals(collect_narrowing(pattern));
    let cond = emit_pattern_match(&arm_cx, pattern, subject_ty, &quote! { __subject });
    let raise = emit_no_matching_pattern_raise(&arm_cx);
    quote! {
        {
            let __subject = #subject_expr;
            if !(#cond) { #raise }
            spinel_rt::RubyValue::Nil
        }
    }
}

fn emit_no_matching_pattern_raise(cx: &Ctx) -> TokenStream {
    let msg = quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new("no matching pattern".to_string())) };
    let boxed = super::expr::emit_boxed_new(cx, "NoMatchingPatternError", vec![msg]);
    quote! { return Err(spinel_rt::Signal::Raise(#boxed)); }
}

/// The DRY core: compiles `pattern` against an already-evaluated `scrutinee`
/// (a cheaply-re-evaluable Rust expression producing an OWNED `RubyValue`,
/// e.g. a bare identifier or `__arr[i].clone()` -- never something with a
/// one-shot side effect, since nested patterns freely reference it more than
/// once) into a single `bool`-typed Rust expression. Any binding a
/// sub-pattern makes always CLONES the scrutinee into the target local
/// (never moves it) -- deliberate: `Capture(Box::new(Bind(x)), y)` (`in x =>
/// y`, binding the SAME value to two names) would otherwise move `__subject`
/// into `x` and then fail to compile reading it again for `y`.
fn emit_pattern_match(cx: &Ctx, pattern: &Pattern, scrutinee_ty: TyKind, scrutinee: &TokenStream) -> TokenStream {
    match pattern {
        Pattern::Bind(name) => {
            let write = super::hoisting::emit_local_write(cx, name, quote! { (#scrutinee).clone() });
            quote! { { #write true } }
        }
        // Value patterns are matched via `RubyValue::rb_eq` -- the same
        // value-equality escape hatch `CaseWhen`'s value matching already
        // uses, not real Ruby's fully general `#===` protocol (see
        // `Pattern::Value`'s docs).
        Pattern::Value(node) => {
            let value_expr = emit_expr(cx, *node);
            quote! { (#value_expr).rb_eq(&(#scrutinee)) }
        }
        Pattern::Pin(node) => {
            let pin_expr = emit_expr(cx, *node);
            quote! { (#pin_expr).rb_eq(&(#scrutinee)) }
        }
        Pattern::ClassCheck(name) => emit_class_check(cx, name, scrutinee_ty, scrutinee),
        Pattern::Range { start, end, exclusive } => emit_range_pattern(cx, *start, *end, *exclusive, scrutinee),
        Pattern::Or(pats) => {
            let checks: Vec<TokenStream> = pats
                .iter()
                .map(|p| emit_pattern_match(cx, p, scrutinee_ty, scrutinee))
                .collect();
            checks.into_iter().fold(quote! { false }, |acc, next| quote! { (#acc) || (#next) })
        }
        Pattern::Capture(inner, name) => {
            let cond = emit_pattern_match(cx, inner, scrutinee_ty, scrutinee);
            let write = super::hoisting::emit_local_write(cx, name, quote! { (#scrutinee).clone() });
            quote! { (#cond) && { #write true } }
        }
        Pattern::Array { constant, pre, rest, post } => {
            emit_array_pattern(cx, constant, pre, rest, post, scrutinee_ty, scrutinee)
        }
        Pattern::Find { constant, pre_rest, mid, post_rest } => {
            emit_find_pattern(cx, constant, pre_rest, mid, post_rest, scrutinee_ty, scrutinee)
        }
        Pattern::Hash { constant, pairs, rest } => emit_hash_pattern(cx, constant, pairs, rest, scrutinee_ty, scrutinee),
    }
}

/// `1..10` / `..5` / `1..` as a pattern -- "does the scrutinee fall inside
/// this range", checked via `as_int_unchecked` (matching `codegen::loops`'
/// existing `for`-in-`Range` restriction to `Int`-valued ranges only, not a
/// general `Comparable`-based `#cover?`).
fn emit_range_pattern(
    cx: &Ctx,
    start: Option<NodeId>,
    end: Option<NodeId>,
    exclusive: bool,
    scrutinee: &TokenStream,
) -> TokenStream {
    let start_check = match start {
        Some(n) => {
            let e = emit_expr(cx, n);
            quote! { (#scrutinee).as_int_unchecked() >= (#e).as_int_unchecked() }
        }
        None => quote! { true },
    };
    let end_check = match end {
        Some(n) => {
            let e = emit_expr(cx, n);
            if exclusive {
                quote! { (#scrutinee).as_int_unchecked() < (#e).as_int_unchecked() }
            } else {
                quote! { (#scrutinee).as_int_unchecked() <= (#e).as_int_unchecked() }
            }
        }
        None => quote! { true },
    };
    quote! { (#start_check) && (#end_check) }
}

/// `in Integer` / `in SomeClass` (also used for an `Array`/`Hash`/`Find`
/// pattern's optional CONSTANT guard, e.g. `Point[x, y]`) -- resolves both
/// built-in primitive names and user-defined classes.
fn emit_class_check(cx: &Ctx, name: &str, scrutinee_ty: TyKind, scrutinee: &TokenStream) -> TokenStream {
    if let Some(check) = emit_builtin_class_check(name, scrutinee_ty, scrutinee) {
        return check;
    }
    let cid = cx
        .compiler
        .class_by_name(name)
        .unwrap_or_else(|| panic!("unknown class/module `{name}` used in a pattern"));
    match scrutinee_ty {
        // The receiver's class is already statically known -- constant-folds
        // to a literal `true`/`false` via the SAME linearized `ancestors`
        // list `is_a?`/`super` already consult (see `analyze::mro`), exactly
        // mirroring `codegen::call::dispatch`'s own `is_a?` handling.
        TyKind::Object(recv_cid) => {
            let result = cx.compiler.class(recv_cid).ancestors.contains(&cid);
            quote! { #result }
        }
        TyKind::Poly => {
            let class_ident = safe_ident(&cx.compiler.class(cid).name);
            quote! {
                spinel_rt::is_a((#scrutinee).as_object_unchecked().class_id(), #class_ident::CLASS_ID)
            }
        }
        // A statically-known BUILT-IN-typed scrutinee (Int/Str/Symbol/Array/
        // Hash/Range/Proc) can never be an instance of a user-defined class.
        _ => quote! { false },
    }
}

/// The built-in primitive names a pattern's `ClassCheck`/constant-guard
/// position can name -- `None` if `name` isn't one of these (the caller
/// falls back to user-class ancestry). `NilClass`/`TrueClass`/`FalseClass`
/// have no `TyKind` variant at all (see `types.rs`), so they're ALWAYS a
/// runtime tag check regardless of `scrutinee_ty`.
fn emit_builtin_class_check(name: &str, scrutinee_ty: TyKind, scrutinee: &TokenStream) -> Option<TokenStream> {
    let (tag_pattern, static_ty): (TokenStream, Option<TyKind>) = match name {
        "Integer" => (quote! { spinel_rt::RubyValue::Int(_) }, Some(TyKind::Int)),
        "String" => (quote! { spinel_rt::RubyValue::Str(_) }, Some(TyKind::Str)),
        "Symbol" => (quote! { spinel_rt::RubyValue::Symbol(_) }, Some(TyKind::Symbol)),
        "Array" => (quote! { spinel_rt::RubyValue::Array(_) }, Some(TyKind::Array)),
        "Hash" => (quote! { spinel_rt::RubyValue::Hash(_) }, Some(TyKind::Hash)),
        "Range" => (quote! { spinel_rt::RubyValue::Range(..) }, Some(TyKind::Range)),
        "Proc" => (quote! { spinel_rt::RubyValue::Proc(_) }, Some(TyKind::Proc)),
        "NilClass" => (quote! { spinel_rt::RubyValue::Nil }, None),
        "TrueClass" => (quote! { spinel_rt::RubyValue::Bool(true) }, None),
        "FalseClass" => (quote! { spinel_rt::RubyValue::Bool(false) }, None),
        _ => return None,
    };
    Some(match static_ty {
        Some(ty) if ty == scrutinee_ty => quote! { true },
        Some(_) if scrutinee_ty == TyKind::Poly => quote! { matches!(&(#scrutinee), #tag_pattern) },
        Some(_) => quote! { false },
        None => quote! { matches!(&(#scrutinee), #tag_pattern) },
    })
}

/// Every `name -> narrowed builtin TyKind` this pattern statically proves,
/// via a `Capture(Box::new(ClassCheck(builtin_name)), name)` shape (`in
/// Integer => n`) anywhere in the tree (including nested inside `Array`/
/// `Find`/`Hash` sub-patterns). Used to build the narrowed `Ctx` a matching
/// arm's own guard/body is emitted with, so `n + 1` inside that arm resolves
/// to native `Int` arithmetic instead of a runtime-Poly fallback.
///
/// Deliberately does NOT narrow to a user-defined class (`TyKind::Object`):
/// doing so would make `codegen::hoisting::local_storage` treat the bound
/// name as `Shadowed` (a fresh `let`, scoped to just this arm's own Rust
/// block) instead of `Hoisted` (a plain mutable reassignment) -- sound
/// WITHIN the arm's own body, but silently failing to persist the matched
/// value for anything read AFTER the whole `case/in` ends, exactly the
/// scenario a one-liner `expr in pattern; puts x` needs to work. Every
/// builtin `TyKind` this DOES narrow to is always `Hoisted` either way, so
/// this restriction costs nothing for the common, load-bearing case
/// (`Integer`/`String`/`Symbol`/`Array`/`Hash`/`Range`/`Proc`) while staying
/// provably sound for the one it skips.
fn collect_narrowing(pattern: &Pattern) -> HashMap<String, TyKind> {
    let mut out = HashMap::new();
    collect_narrowing_into(pattern, &mut out);
    out
}

fn collect_narrowing_into(pattern: &Pattern, out: &mut HashMap<String, TyKind>) {
    match pattern {
        Pattern::Capture(inner, name) => {
            if let Pattern::ClassCheck(class_name) = inner.as_ref() {
                if let Some(ty) = builtin_narrowed_type(class_name) {
                    out.insert(name.clone(), ty);
                }
            }
            collect_narrowing_into(inner, out);
        }
        Pattern::Or(pats) => {
            for p in pats {
                collect_narrowing_into(p, out);
            }
        }
        Pattern::Array { pre, rest, post, .. } => {
            for p in pre.iter().chain(post) {
                collect_narrowing_into(p, out);
            }
            // A named `*rest` capture is ALWAYS a freshly-built `Array` (see
            // `emit_array_pattern`'s `rest_bind`) -- narrowing it the same
            // way a `Capture(ClassCheck("Array"), _)` would lets `.length`/
            // `[]`/etc. on it take the native collection-dispatch fast path
            // instead of hitting the "receiver's class isn't statically
            // known" panic (`Array`, like every other narrowed type here, is
            // never `Shadowed` -- see this function's own docs).
            if let Some(Some(name)) = rest {
                out.insert(name.clone(), TyKind::Array);
            }
        }
        Pattern::Find { pre_rest, mid, post_rest, .. } => {
            if let Some(name) = pre_rest {
                out.insert(name.clone(), TyKind::Array);
            }
            for p in mid {
                collect_narrowing_into(p, out);
            }
            if let Some(name) = post_rest {
                out.insert(name.clone(), TyKind::Array);
            }
        }
        Pattern::Hash { pairs, rest, .. } => {
            for (_, p) in pairs {
                if let Some(p) = p {
                    collect_narrowing_into(p, out);
                }
            }
            // See the `Array` arm above -- a named `**rest` capture is
            // always a freshly-built `Hash`.
            if let HashPatternRest::Rest(Some(name)) = rest {
                out.insert(name.clone(), TyKind::Hash);
            }
        }
        Pattern::Bind(_) | Pattern::Value(_) | Pattern::Pin(_) | Pattern::ClassCheck(_) | Pattern::Range { .. } => {}
    }
}

fn builtin_narrowed_type(name: &str) -> Option<TyKind> {
    match name {
        "Integer" => Some(TyKind::Int),
        "String" => Some(TyKind::Str),
        "Symbol" => Some(TyKind::Symbol),
        "Array" => Some(TyKind::Array),
        "Hash" => Some(TyKind::Hash),
        "Range" => Some(TyKind::Range),
        "Proc" => Some(TyKind::Proc),
        _ => None,
    }
}

/// Resolves how to obtain a `Vec<spinel_rt::RubyValue>` for an `Array`/
/// `Find` pattern's scrutinee, binding it to `arr_ident` -- shared by both
/// (identical resolution rule). `None` means the pattern can PROVABLY never
/// match this scrutinee (a statically-known type that's neither `Array` nor
/// an `Object` with `#deconstruct`), so the caller should skip straight to
/// a compile-time-constant `false` with no runtime cost at all.
///
/// A `TyKind::Object` receiver's `#deconstruct` dispatch is resolved
/// STATICALLY (`Compiler::method_in_chain`, a compile-time fact): if the
/// class has one, call it directly (Path 1, safe); if not, this specific
/// class can NEVER satisfy an array pattern, so `None` here is not an
/// approximation -- it's provably correct. `TyKind::Poly`, by contrast,
/// genuinely doesn't know at compile time -- rather than attempting an
/// unsafe blind method call (this runtime has no `respond_to?` primitive to
/// guard it, unlike `is_a?`'s safe runtime fallback), it does the one check
/// that IS safe: is the runtime value ALREADY tagged `RubyValue::Array`? If
/// not, the pattern fails closed (`break #label false`) rather than crashing
/// -- a deliberate, narrower-than-real-Ruby scope-cut (a `Poly`-typed
/// receiver that happens to be an object with `#deconstruct` won't match),
/// not silent wrongness for the common case this DOES handle (nested
/// destructuring against a plain runtime Array, e.g. `[1, [2, 3]]`).
fn emit_array_binding(
    cx: &Ctx,
    label: &syn::Lifetime,
    arr_ident: &proc_macro2::Ident,
    scrutinee_ty: TyKind,
    scrutinee: &TokenStream,
) -> Option<TokenStream> {
    match scrutinee_ty {
        TyKind::Array => Some(quote! {
            let #arr_ident: Vec<spinel_rt::RubyValue> = (#scrutinee).as_array_unchecked().borrow().clone();
        }),
        TyKind::Object(cid) if cx.compiler.method_in_chain(cid, "deconstruct").is_some() => Some(quote! {
            let #arr_ident: Vec<spinel_rt::RubyValue> =
                (#scrutinee.clone()).deconstruct()?.as_array_unchecked().borrow().clone();
        }),
        TyKind::Poly => Some(quote! {
            let #arr_ident: Vec<spinel_rt::RubyValue> = match &(#scrutinee) {
                spinel_rt::RubyValue::Array(__rc) => __rc.borrow().clone(),
                _ => break #label false,
            };
        }),
        _ => None,
    }
}

/// See `emit_array_binding`'s docs -- the same rule, for `#deconstruct_keys`/
/// `RubyValue::Hash` instead of `#deconstruct`/`RubyValue::Array`. Binds an
/// `RHash` handle directly (not a copied `Vec`) so the existing `hash_get`/
/// `hash_has_key`/`hash_len` runtime helpers can be reused as-is.
fn emit_hash_binding(
    cx: &Ctx,
    label: &syn::Lifetime,
    h_ident: &proc_macro2::Ident,
    scrutinee_ty: TyKind,
    scrutinee: &TokenStream,
) -> Option<TokenStream> {
    match scrutinee_ty {
        TyKind::Hash => Some(quote! {
            let #h_ident: spinel_rt::RHash = (#scrutinee).as_hash_unchecked();
        }),
        TyKind::Object(cid) if cx.compiler.method_in_chain(cid, "deconstruct_keys").is_some() => Some(quote! {
            let #h_ident: spinel_rt::RHash =
                (#scrutinee.clone()).deconstruct_keys(spinel_rt::RubyValue::Nil)?.as_hash_unchecked();
        }),
        TyKind::Poly => Some(quote! {
            let #h_ident: spinel_rt::RHash = match &(#scrutinee) {
                spinel_rt::RubyValue::Hash(__rc) => __rc.clone(),
                _ => break #label false,
            };
        }),
        _ => None,
    }
}

/// `[pre.., *rest, post..]` -- see `Pattern::Array`'s docs.
#[allow(clippy::too_many_arguments)]
fn emit_array_pattern(
    cx: &Ctx,
    constant: &Option<String>,
    pre: &[Pattern],
    rest: &Option<Option<String>>,
    post: &[Pattern],
    scrutinee_ty: TyKind,
    scrutinee: &TokenStream,
) -> TokenStream {
    let label = fresh_label(cx, "pat_arr");
    let arr_ident = fresh_temp(cx, "arr");
    let Some(arr_binding) = emit_array_binding(cx, &label, &arr_ident, scrutinee_ty, scrutinee) else {
        return quote! { false };
    };

    let class_check = constant.as_ref().map(|name| {
        let check = emit_class_check(cx, name, scrutinee_ty, scrutinee);
        quote! { if !(#check) { break #label false; } }
    });

    let min_len = pre.len() + post.len();
    let arity_ok = if rest.is_some() {
        quote! { #arr_ident.len() >= #min_len }
    } else {
        quote! { #arr_ident.len() == #min_len }
    };

    let pre_checks = pre.iter().enumerate().map(|(i, p)| {
        let elem = quote! { #arr_ident[#i].clone() };
        let check = emit_pattern_match(cx, p, TyKind::Poly, &elem);
        quote! { if !(#check) { break #label false; } }
    });
    let post_len = post.len();
    let post_checks = post.iter().enumerate().map(|(i, p)| {
        let offset = post_len - i;
        let elem = quote! { #arr_ident[#arr_ident.len() - #offset].clone() };
        let check = emit_pattern_match(cx, p, TyKind::Poly, &elem);
        quote! { if !(#check) { break #label false; } }
    });
    let rest_bind = match rest {
        Some(Some(name)) => {
            let pre_len = pre.len();
            Some(super::hoisting::emit_local_write(
                cx,
                name,
                quote! {
                    spinel_rt::RubyValue::Array(spinel_rt::array_new(
                        #arr_ident[#pre_len..#arr_ident.len() - #post_len].to_vec()
                    ))
                },
            ))
        }
        _ => None,
    };

    quote! {
        #label: {
            #class_check
            #arr_binding
            if !(#arity_ok) { break #label false; }
            #(#pre_checks)*
            #(#post_checks)*
            #rest_bind
            true
        }
    }
}

/// `[*pre_rest, mid.., *post_rest]` -- unlike `Array`'s fixed pre/post
/// anchoring, `mid` must match SOME contiguous window of the scrutinee
/// array; this is inherently a runtime SEARCH (scanning left to right, first
/// window that matches wins), not a plain boolean chain -- built as a
/// labeled inner search loop that tries each starting offset in turn.
#[allow(clippy::too_many_arguments)]
fn emit_find_pattern(
    cx: &Ctx,
    constant: &Option<String>,
    pre_rest: &Option<String>,
    mid: &[Pattern],
    post_rest: &Option<String>,
    scrutinee_ty: TyKind,
    scrutinee: &TokenStream,
) -> TokenStream {
    let label = fresh_label(cx, "pat_find");
    let arr_ident = fresh_temp(cx, "find_arr");
    let Some(arr_binding) = emit_array_binding(cx, &label, &arr_ident, scrutinee_ty, scrutinee) else {
        return quote! { false };
    };

    let class_check = constant.as_ref().map(|name| {
        let check = emit_class_check(cx, name, scrutinee_ty, scrutinee);
        quote! { if !(#check) { break #label false; } }
    });

    let mid_len = mid.len();
    let start_ident = fresh_temp(cx, "find_start");
    let search_label = fresh_label(cx, "pat_find_search");
    let mid_checks: Vec<TokenStream> = mid
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let elem = quote! { #arr_ident[#start_ident + #i].clone() };
            emit_pattern_match(cx, p, TyKind::Poly, &elem)
        })
        .collect();
    let all_matched_expr = mid_checks
        .into_iter()
        .fold(quote! { true }, |acc, next| quote! { (#acc) && (#next) });

    let pre_bind = pre_rest.as_ref().map(|name| {
        super::hoisting::emit_local_write(
            cx,
            name,
            quote! { spinel_rt::RubyValue::Array(spinel_rt::array_new(#arr_ident[0..#start_ident].to_vec())) },
        )
    });
    let post_bind = post_rest.as_ref().map(|name| {
        super::hoisting::emit_local_write(
            cx,
            name,
            quote! {
                spinel_rt::RubyValue::Array(spinel_rt::array_new(#arr_ident[(#start_ident + #mid_len)..].to_vec()))
            },
        )
    });

    quote! {
        #label: {
            #class_check
            #arr_binding
            let __mid_len: usize = #mid_len;
            if #arr_ident.len() < __mid_len {
                break #label false;
            }
            let mut #start_ident: usize = 0;
            let __found: bool = #search_label: loop {
                if #start_ident > #arr_ident.len() - __mid_len {
                    break #search_label false;
                }
                if #all_matched_expr {
                    break #search_label true;
                }
                #start_ident += 1;
            };
            if !__found {
                break #label false;
            }
            #pre_bind
            #post_bind
            true
        }
    }
}

/// `{key: pattern, ..., **rest}` -- see `Pattern::Hash`/`HashPatternRest`'s
/// docs. Each key's PRESENCE is checked via `hash_has_key` (not just
/// `hash_get(...).is_nil()`, which can't distinguish a genuinely-absent key
/// from one whose value happens to be Ruby `nil`).
#[allow(clippy::too_many_arguments)]
fn emit_hash_pattern(
    cx: &Ctx,
    constant: &Option<String>,
    pairs: &[(String, Option<Pattern>)],
    rest: &HashPatternRest,
    scrutinee_ty: TyKind,
    scrutinee: &TokenStream,
) -> TokenStream {
    let label = fresh_label(cx, "pat_hash");
    let h_ident = fresh_temp(cx, "hash");
    let Some(h_binding) = emit_hash_binding(cx, &label, &h_ident, scrutinee_ty, scrutinee) else {
        return quote! { false };
    };

    let class_check = constant.as_ref().map(|name| {
        let check = emit_class_check(cx, name, scrutinee_ty, scrutinee);
        quote! { if !(#check) { break #label false; } }
    });

    let key_checks = pairs.iter().map(|(key, pat)| {
        let key_expr = quote! { spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#key)) };
        let has_key = quote! { spinel_rt::hash_has_key(&#h_ident, &(#key_expr)) };
        let value_expr = quote! { spinel_rt::hash_get(&#h_ident, &(#key_expr)) };
        let value_check = match pat {
            Some(p) => emit_pattern_match(cx, p, TyKind::Poly, &value_expr),
            // `{key:}` shorthand -- binds a local named `key` directly,
            // always "matching" once the key is present at all.
            None => {
                let write = super::hoisting::emit_local_write(cx, key, value_expr);
                quote! { { #write true } }
            }
        };
        quote! { if !(#has_key) || !(#value_check) { break #label false; } }
    });

    let no_more_keys_check = matches!(rest, HashPatternRest::NoMoreKeys).then(|| {
        let n = pairs.len();
        quote! { if spinel_rt::hash_len(&#h_ident) as usize != #n { break #label false; } }
    });

    let rest_bind = match rest {
        HashPatternRest::Rest(Some(name)) => {
            let key_lits: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
            Some(super::hoisting::emit_local_write(
                cx,
                name,
                quote! {
                    spinel_rt::RubyValue::Hash(spinel_rt::hash_except_keys(&#h_ident, &[#(#key_lits),*]))
                },
            ))
        }
        _ => None,
    };

    quote! {
        #label: {
            #class_check
            #h_binding
            #no_more_keys_check
            #(#key_checks)*
            #rest_bind
            true
        }
    }
}
