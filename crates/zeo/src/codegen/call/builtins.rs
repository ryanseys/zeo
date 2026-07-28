//! The built-in-type fast paths `dispatch` consults before falling through
//! to ordinary Path 1/Path 2 method dispatch: `Array`/`Hash`/`Str`/`Range`'s
//! minimal collection method set (`try_collection_dispatch`), `Regexp`
//! (`try_regexp_dispatch`), and `Proc`/lambda call syntax
//! (`try_proc_dispatch`). Each returns `None` (falls through) for any
//! receiver whose static type isn't the one it handles, so a user class's
//! own method of the same name is completely unaffected.

use quote::{format_ident, quote};

use crate::codegen::Ctx;
use crate::codegen::expr::{box_if_object_typed, emit_expr, infer};
use crate::hir::NodeId;
use crate::types::TyKind;
use proc_macro2::TokenStream;

/// `Array`/`Hash`/`Str`/`Range`'s built-in method fast path (see
/// `zeo_rt::collections`'s module docs for the original deliberate
/// scope-cut this grew from). `a[i]`/`a[i] = v` are ordinary `CallNode`s
/// named `"[]"`/`"[]="` at the `ruby-prism` level (just like the numeric
/// operators), so this is dispatch-table generalization, not a new HIR
/// shape. Returns `None` (falls through to ordinary Path 1/Path 2 dispatch)
/// for any receiver whose static type isn't one of these four, so a user
/// class's own `def []` is completely unaffected.
pub(super) fn try_collection_dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    let ty = infer(cx, recv_id);
    // A compile-time reopen of the builtin method anywhere in its chain wins
    // at every call site -- fall through to ordinary dispatch (the same
    // check inference's own narrowing rules run).
    if matches!(
        ty,
        TyKind::Array | TyKind::Hash | TyKind::Str | TyKind::Range
    ) && crate::types::builtin_override(cx.compiler, ty, cx.box_id, name)
    {
        return None;
    }
    let tokens = match (ty, name, args.len()) {
        // Only for a statically-Int index -- Range/other index shapes
        // fall through to the dynamic rows.
        (TyKind::Array, "[]", 1) if infer(cx, args[0]) == TyKind::Int => {
            let idx = emit_expr(cx, args[0]);
            quote! { zeo_rt::array_get(&(#recv_expr).as_array_unchecked(), (#idx).as_int_unchecked()) }
        }
        // Only for a statically-Int index -- a Range index is a SPLICE with
        // different semantics (to_ary coercion), served by the dynamic row.
        (TyKind::Array, "[]=", 2) if infer(cx, args[0]) == TyKind::Int => {
            let idx = emit_expr(cx, args[0]);
            let val = emit_expr(cx, args[1]);
            // Boxed if Object-typed, same as Hash's `[]=` value below: the
            // stored element must be a real `RubyValue`.
            let val = crate::codegen::expr::box_if_object_typed(cx, args[1], val);
            // A negative index still out of range after counting from the
            // end raises a real `IndexError` -- constructed
            // here, not inside `zeo_rt::array_set` itself, since only
            // codegen has the class registry needed to build one (see
            // `emit_boxed_new`'s docs).
            // `; minimum: -N` where N is the array's length -- CRuby's full
            // message (`rb_ary_store`'s "index %ld too small for array;
            // minimum: %ld"). The minimum is the NUMBER `-len`, not a literal
            // `-` before the length: an empty array's minimum is `0`, and
            // `-0` would be wrong. `__recv`/`__idx` are both in scope at the
            // `None =>` arm this splices into.
            let index_error = crate::codegen::expr::emit_boxed_new(
                cx,
                "IndexError",
                vec![quote! {
                    zeo_rt::RubyValue::Str(zeo_rt::string_new(
                        format!(
                            "index {} too small for array; minimum: {}",
                            __idx,
                            -(zeo_rt::array_len(&__recv) as i64)
                        )
                    ))
                }],
            );
            let frozen_error = super::raise::emit_frozen_error(
                cx,
                "Array",
                quote! { zeo_rt::RubyValue::Array(__recv.clone()) },
            );
            quote! {
                {
                    // Receiver and arguments evaluate FIRST, then the frozen
                    // check, then the actual store -- real Ruby's own order
                    // (`FrozenError` raises from inside `[]=`, after argument
                    // evaluation but before any work: CRuby's
                    // `rb_ary_modify_check` at the top of the mutator).
                    let __recv = (#recv_expr).as_array_unchecked();
                    let __idx = (#idx).as_int_unchecked();
                    let __val = #val;
                    if __recv.is_frozen() {
                        return Err(zeo_rt::Signal::Raise(#frozen_error));
                    }
                    match zeo_rt::array_set(&__recv, __idx, __val) {
                        Some(__v) => __v,
                        None => return Err(zeo_rt::Signal::Raise(#index_error)),
                    }
                }
            }
        }
        (TyKind::Array, "length" | "size", 0) => {
            quote! { zeo_rt::RubyValue::Int(zeo_rt::array_len(&(#recv_expr).as_array_unchecked())) }
        }
        // The bare single-value/no-arg Array mutators and probes -- the
        // shapes 12M-send list benchmarks live on. Multi-arg `push`, count
        // forms of `pop`/`shift` (a DIFFERENT return type), and everything
        // else stay on the dynamic rows. The `*_checked` cores run the same
        // frozen-check-then-mutate the builtin defs do.
        (TyKind::Array, "push" | "append" | "<<", 1) => {
            let val = emit_expr(cx, args[0]);
            let val = box_if_object_typed(cx, args[0], val);
            quote! { zeo_rt::array_push_checked(&(#recv_expr).as_array_unchecked(), #val)? }
        }
        (TyKind::Array, "pop", 0) => {
            quote! { zeo_rt::array_pop_checked(&(#recv_expr).as_array_unchecked())? }
        }
        (TyKind::Array, "shift", 0) => {
            quote! { zeo_rt::array_shift_checked(&(#recv_expr).as_array_unchecked())? }
        }
        (TyKind::Array, "empty?", 0) => {
            quote! { zeo_rt::RubyValue::Bool(zeo_rt::array_len(&(#recv_expr).as_array_unchecked()) == 0) }
        }
        (TyKind::Array, "first", 0) => {
            quote! { zeo_rt::array_get(&(#recv_expr).as_array_unchecked(), 0) }
        }
        (TyKind::Array, "last", 0) => {
            quote! { zeo_rt::array_get(&(#recv_expr).as_array_unchecked(), -1) }
        }
        (TyKind::Hash, "[]", 1) => {
            let key = emit_expr(cx, args[0]);
            // Boxed if Object-typed: an object KEY reaches the
            // `HashKey` projection (which now dispatches a user `hash`).
            let key = crate::codegen::expr::box_if_object_typed(cx, args[0], key);
            quote! { zeo_rt::hash_index(&(#recv_expr).as_hash_unchecked(), &(#key))? }
        }
        (TyKind::Hash, "[]=", 2) => {
            let key = emit_expr(cx, args[0]);
            let key = crate::codegen::expr::box_if_object_typed(cx, args[0], key);
            let val = emit_expr(cx, args[1]);
            let val = crate::codegen::expr::box_if_object_typed(cx, args[1], val);
            let frozen_error = super::raise::emit_frozen_error(
                cx,
                "Hash",
                quote! { zeo_rt::RubyValue::Hash(__recv.clone()) },
            );
            // The frozen check runs BEFORE `hash_set` ever hashes the key --
            // the same ordering CRuby guarantees (`rb_hash_modify` is
            // `rb_hash_aset`'s first statement, ahead of any `st_update`).
            quote! {
                {
                    let __recv = (#recv_expr).as_hash_unchecked();
                    let __key = #key;
                    let __val = #val;
                    if __recv.is_frozen() {
                        return Err(zeo_rt::Signal::Raise(#frozen_error));
                    }
                    zeo_rt::hash_set(&__recv, __key, __val)
                }
            }
        }
        (TyKind::Hash, "length" | "size", 0) => {
            quote! { zeo_rt::RubyValue::Int(zeo_rt::hash_len(&(#recv_expr).as_hash_unchecked())) }
        }
        (TyKind::Hash, "empty?", 0) => {
            quote! { zeo_rt::RubyValue::Bool(zeo_rt::hash_len(&(#recv_expr).as_hash_unchecked()) == 0) }
        }
        (TyKind::Str, "[]", 1) if infer(cx, args[0]) == TyKind::Int => {
            let idx = emit_expr(cx, args[0]);
            quote! { zeo_rt::string_get(&(#recv_expr).as_str_unchecked(), (#idx).as_int_unchecked()) }
        }
        (TyKind::Str, "length" | "size", 0) => {
            quote! { zeo_rt::RubyValue::Int(zeo_rt::string_len(&(#recv_expr).as_str_unchecked())) }
        }
        (TyKind::Str, "empty?", 0) => {
            quote! { zeo_rt::RubyValue::Bool(zeo_rt::string_len(&(#recv_expr).as_str_unchecked()) == 0) }
        }
        (TyKind::Range, "first", 0) => quote! { (#recv_expr).range_first() },
        (TyKind::Range, "last", 0) => quote! { (#recv_expr).range_last() },
        (TyKind::Range, "exclude_end?", 0) => {
            quote! { zeo_rt::RubyValue::Bool((#recv_expr).range_exclude_end()) }
        }
        _ => return None,
    };
    Some(tokens)
}
/// `Regexp`/`MatchData` built-in methods, and `String`'s methods that take a
/// `Regexp` pattern argument -- mirrors `try_collection_dispatch`'s
/// shape (a `None` return falls through to ordinary Path 1/Path 2 dispatch),
/// kept as its own function since `gsub`/`sub`'s block form needs the call
/// site's own `block`, which `try_collection_dispatch` was never threaded to
/// receive.
///
/// Scope-cut, checked at `zeo` CODEGEN time (a clear, immediate panic,
/// not embedded runtime code -- same posture as e.g.
/// `codegen::captures::walk`'s "block escaping from inside another escaping
/// block" rejection): a `String` pattern ARGUMENT to `match`/`match?`/`=~`/
/// `!~`/`scan`/`split`/`sub`/`gsub` (as opposed to a `Regexp` argument, this
/// function's primary scope) isn't supported. Real Ruby's own behavior here
/// is a genuinely asymmetric special case worth naming, not an oversight:
/// `match`/`match?`/`=~`/`scan` compile a `String` pattern INTO a `Regexp`
/// first, while `split`/`sub`/`gsub`'s `String` pattern is matched as a
/// LITERAL substring (regex metacharacters NOT interpreted) -- reproducing
/// both halves of that distinction is a separate, plain-`String`-methods
/// feature, not part of this Regexp phase.
pub(super) fn try_regexp_dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    block: Option<NodeId>,
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    let ty = infer(cx, recv_id);

    // A borrowed `&str` view of a statically-`Str`-typed argument, bound to
    // `ident` INSIDE the returned prelude -- never held across a nested call
    // that could lock the SAME `RStr` again (the read-modify-write
    // self-deadlock family `hoisting::emit_local_write`'s docs describe).
    // Returns `None` (not a panic) when the argument isn't statically `Str`
    // -- e.g. a `Poly`-typed argument -- so callers can cleanly decline via
    // `?` and fall through to ordinary dispatch, same posture as every other
    // arm here.
    let str_guard = |arg_id: NodeId, ident: &str| -> Option<TokenStream> {
        if infer(cx, arg_id) != TyKind::Str {
            return None;
        }
        let e = emit_expr(cx, arg_id);
        let var = format_ident!("{ident}");
        // Lock, then take the UTF-8 view -- the regexp engine consumes a
        // `&str`, and a `Cow<str>` derefs to one. For valid UTF-8 (every
        // string, until non-UTF-8 encodings enter the picture) this is the
        // exact bytes.
        Some(quote! {
            let #var = (#e).as_str_unchecked();
            let #var = #var.lock();
            let #var = #var.to_utf8_lossy();
        })
    };

    if ty == TyKind::Regexp {
        match (name, args.len()) {
            ("source", 0) => {
                return Some(quote! { zeo_rt::regexp_source(&(#recv_expr).as_regexp_unchecked()) });
            }
            ("to_s", 0) => {
                return Some(quote! { zeo_rt::regexp_to_s(&(#recv_expr).as_regexp_unchecked()) });
            }
            ("inspect", 0) => {
                return Some(
                    quote! { zeo_rt::regexp_inspect(&(#recv_expr).as_regexp_unchecked()) },
                );
            }
            // `===` is safe against ANY subject shape (real Ruby: `Regexp#===`
            // is `false`, not an error, for a non-String) -- routed through
            // `RubyValue::rb_case_eq` rather than requiring a statically
            // `Str`-typed argument like the other arms here, since it's the
            // one Regexp method real Ruby itself designed to be called with
            // an arbitrary-shaped subject (`case/when` dispatch).
            ("===", 1) => {
                let arg_expr = emit_expr(cx, args[0]);
                return Some(
                    quote! { zeo_rt::RubyValue::Bool((#recv_expr).rb_case_eq(&(#arg_expr))) },
                );
            }
            ("=~", 1) => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard zeo_rt::regexp_match_index(&(#recv_expr).as_regexp_unchecked(), &__h) }
                });
            }
            // `~ rxp` matches the pattern against `$_` (the last input line),
            // returning the match position or nil and setting `$~`. A non-
            // String `$_` clears the match and answers nil.
            ("~", 0) => {
                let bx = cx.box_id;
                return Some(quote! {
                    {
                        match zeo_rt::global_get(#bx, "$_") {
                            zeo_rt::RubyValue::Str(__s) => {
                                let __g = __s.lock();
                                let __h = __g.to_utf8_lossy();
                                zeo_rt::regexp_match_index(&(#recv_expr).as_regexp_unchecked(), &__h)
                            }
                            _ => {
                                zeo_rt::set_last_match(None);
                                zeo_rt::RubyValue::Nil
                            }
                        }
                    }
                });
            }
            ("!~", 1) => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard zeo_rt::RubyValue::Bool(!zeo_rt::regexp_is_match(&(#recv_expr).as_regexp_unchecked(), &__h)) }
                });
            }
            ("match", 1) if block.is_none() => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard zeo_rt::regexp_match(&(#recv_expr).as_regexp_unchecked(), &__h) }
                });
            }
            ("match?", 1) => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard zeo_rt::RubyValue::Bool(zeo_rt::regexp_is_match(&(#recv_expr).as_regexp_unchecked(), &__h)) }
                });
            }
            _ => {}
        }
    }

    if ty == TyKind::Str && !args.is_empty() {
        let pattern_is_regexp = infer(cx, args[0]) == TyKind::Regexp;
        // A non-Regexp pattern (String, or dynamically typed) falls
        // through to `send_value`, whose String rows handle String
        // patterns for split/sub/gsub/match/match?.

        if pattern_is_regexp {
            let re_expr = emit_expr(cx, args[0]);
            let haystack_guard = quote! {
                let __h = (#recv_expr).as_str_unchecked();
                let __h = __h.lock();
                let __h = __h.to_utf8_lossy();
            };
            match (name, args.len()) {
                ("=~", 1) => {
                    return Some(quote! {
                        { #haystack_guard zeo_rt::regexp_match_index(&(#re_expr).as_regexp_unchecked(), &__h) }
                    });
                }
                ("!~", 1) => {
                    return Some(quote! {
                        { #haystack_guard zeo_rt::RubyValue::Bool(!zeo_rt::regexp_is_match(&(#re_expr).as_regexp_unchecked(), &__h)) }
                    });
                }
                ("match", 1) if block.is_none() => {
                    return Some(quote! {
                        { #haystack_guard zeo_rt::regexp_match(&(#re_expr).as_regexp_unchecked(), &__h) }
                    });
                }
                ("match?", 1) => {
                    return Some(quote! {
                        { #haystack_guard zeo_rt::RubyValue::Bool(zeo_rt::regexp_is_match(&(#re_expr).as_regexp_unchecked(), &__h)) }
                    });
                }
                ("scan", 1) => {
                    // The block form yields each match and returns the RECEIVER
                    // (not the array of matches). The haystack is materialized
                    // to an owned String so the receiver's lock is released
                    // before the user block runs (it may touch the receiver).
                    if let Some(block_id) = block {
                        let blk_expr = super::procs::emit_proc_value(cx, block_id);
                        return Some(quote! {
                            {
                                let __recv = (#recv_expr).as_str_unchecked();
                                let __hs = { let __g = __recv.lock(); __g.to_utf8_lossy().into_owned() };
                                zeo_rt::regexp_scan_block(&(#re_expr).as_regexp_unchecked(), &__hs, &(#blk_expr).as_proc_unchecked())?;
                                zeo_rt::RubyValue::Str(__recv)
                            }
                        });
                    }
                    return Some(quote! {
                        { #haystack_guard zeo_rt::regexp_scan(&(#re_expr).as_regexp_unchecked(), &__h) }
                    });
                }
                ("split", 1) => {
                    return Some(quote! {
                        { #haystack_guard zeo_rt::regexp_split(&(#re_expr).as_regexp_unchecked(), &__h, 0) }
                    });
                }
                ("sub", 2) if infer(cx, args[1]) == TyKind::Str => {
                    let repl_expr = emit_expr(cx, args[1]);
                    return Some(quote! {
                        { #haystack_guard let __r = (#repl_expr).as_str_unchecked(); let __r = __r.lock(); let __r = __r.to_utf8_lossy();
                          zeo_rt::regexp_sub(&(#re_expr).as_regexp_unchecked(), &__h, &__r)? }
                    });
                }
                ("gsub", 2) if infer(cx, args[1]) == TyKind::Str => {
                    let repl_expr = emit_expr(cx, args[1]);
                    return Some(quote! {
                        { #haystack_guard let __r = (#repl_expr).as_str_unchecked(); let __r = __r.lock(); let __r = __r.to_utf8_lossy();
                          zeo_rt::regexp_gsub(&(#re_expr).as_regexp_unchecked(), &__h, &__r)? }
                    });
                }
                ("sub", 1) => {
                    let block_id = block?;
                    let blk_expr = super::procs::emit_proc_value(cx, block_id);
                    return Some(quote! {
                        { #haystack_guard
                          zeo_rt::regexp_sub_block(&(#re_expr).as_regexp_unchecked(), &__h, &(#blk_expr).as_proc_unchecked())? }
                    });
                }
                ("gsub", 1) => {
                    let block_id = block?;
                    let blk_expr = super::procs::emit_proc_value(cx, block_id);
                    return Some(quote! {
                        { #haystack_guard
                          zeo_rt::regexp_gsub_block(&(#re_expr).as_regexp_unchecked(), &__h, &(#blk_expr).as_proc_unchecked())? }
                    });
                }
                _ => {}
            }
        }
    }

    if ty == TyKind::MatchData {
        match (name, args.len()) {
            ("[]", 1) => {
                let arg_expr = emit_expr(cx, args[0]);
                return Some(quote! {
                    zeo_rt::matchdata_get(&(#recv_expr).as_matchdata_unchecked(), &(#arg_expr))?
                });
            }
            ("pre_match", 0) => {
                return Some(
                    quote! { zeo_rt::matchdata_pre_match(&(#recv_expr).as_matchdata_unchecked()) },
                );
            }
            ("post_match", 0) => {
                return Some(
                    quote! { zeo_rt::matchdata_post_match(&(#recv_expr).as_matchdata_unchecked()) },
                );
            }
            ("to_a", 0) => {
                return Some(
                    quote! { zeo_rt::matchdata_to_a(&(#recv_expr).as_matchdata_unchecked()) },
                );
            }
            ("captures", 0) => {
                return Some(
                    quote! { zeo_rt::matchdata_captures(&(#recv_expr).as_matchdata_unchecked()) },
                );
            }
            ("named_captures", 0) => {
                return Some(
                    quote! { zeo_rt::matchdata_named_captures(&(#recv_expr).as_matchdata_unchecked()) },
                );
            }
            ("string", 0) => {
                return Some(
                    quote! { zeo_rt::matchdata_string(&(#recv_expr).as_matchdata_unchecked()) },
                );
            }
            ("to_s", 0) => {
                return Some(
                    quote! { zeo_rt::matchdata_to_s(&(#recv_expr).as_matchdata_unchecked()) },
                );
            }
            _ => {}
        }
    }

    None
}
/// `blk.call(args)` / `blk.(args)` / `blk[args]` on a statically
/// `TyKind::Proc` receiver -- dispatches directly to the underlying
/// closure, mirroring `try_collection_dispatch`'s shape. Only ever reached
/// for a NAMED `&block` parameter (the only way a local/param is currently
/// inferred `Proc` -- see `analyze::register_class`'s seeding, mirroring
/// how a named `*rest`/`**kwrest` seeds `Array`/`Hash`); nothing else infers
/// this type yet, so a user class's own `def call` is unaffected.
pub(super) fn try_proc_dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    block: Option<NodeId>,
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    if infer(cx, recv_id) != TyKind::Proc || !matches!(name, "call" | "()" | "[]") {
        return None;
    }
    let arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    // A literal block passed to `#call` rides into the proc's own `&block`
    // param (`->(&b) { b.call }.call { ... }`); without one, `None` -- exactly
    // the plain-`#call` behavior.
    let block_expr = match block {
        Some(b) => {
            let p = super::procs::emit_proc_value(cx, b);
            quote! { Some(#p) }
        }
        None => quote! { None },
    };
    Some(
        quote! { ((#recv_expr).as_proc_unchecked()).call_with_block(&[#(#arg_exprs),*], #block_expr)? },
    )
}
