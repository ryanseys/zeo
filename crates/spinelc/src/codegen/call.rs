//! `emit_call`'s dispatch decision -- see "Two dispatch paths" in the plan:
//! Path 1 (static direct call, or a call rewritten away entirely, e.g.
//! `super` inlining) whenever `Compiler::method_in_chain` resolves the
//! target from the receiver's statically-known class; Path 2
//! (`spinel_rt::send`) only when it can't. Every fragment produced here
//! evaluates to a bare `spinel_rt::RubyValue` -- any call into a
//! `Result`-returning method has `?` applied right here, at the call site,
//! not left to the caller (see `expr.rs`'s module docs).

use quote::{format_ident, quote};

use super::expr::{box_if_object_typed, emit_expr, emit_symbol_expr, infer, infer_any_class, infer_class};
use super::ident::safe_ident;
use super::Ctx;
use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HirNode, KeywordParam, KwArg, NodeId, Params, Visibility};
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
const INT_BINARY_OPS: &[(&str, &str, IntOpKind)] = &[
    ("+", "int_add", IntOpKind::Value),
    ("-", "int_sub", IntOpKind::Value),
    ("*", "int_mul", IntOpKind::Value),
    ("/", "int_div", IntOpKind::DivMod),
    ("%", "int_mod", IntOpKind::DivMod),
    // `**` is FALLIBLE since the bignum migration: a negative exponent is
    // a Rational result, `0 ** -n` raises ZeroDivisionError.
    ("**", "int_pow", IntOpKind::Fallible),
    ("&", "int_band", IntOpKind::Value),
    ("|", "int_bor", IntOpKind::Value),
    ("^", "int_bxor", IntOpKind::Value),
    // Shifts are fallible too: a beyond-u32 width raises RangeError.
    ("<<", "int_shl", IntOpKind::Fallible),
    (">>", "int_shr", IntOpKind::Fallible),
    ("==", "int_eq", IntOpKind::Bool),
    ("!=", "int_neq", IntOpKind::Bool),
    ("<", "int_lt", IntOpKind::Bool),
    (">", "int_gt", IntOpKind::Bool),
    ("<=", "int_le", IntOpKind::Bool),
    (">=", "int_ge", IntOpKind::Bool),
    ("<=>", "int_cmp", IntOpKind::Cmp),
];

/// How an `INT_BINARY_OPS` row's runtime fn shapes into an emitted
/// expression (Phase 17.1's bignum migration: the `int_*` family takes
/// `&RubyValue` pairs -- an `Int`-typed value may carry either payload --
/// and arithmetic returns `RubyValue` directly, promoting on overflow).
#[derive(Clone, Copy, PartialEq)]
enum IntOpKind {
    /// `fn(&RubyValue, &RubyValue) -> RubyValue` -- infallible arithmetic.
    Value,
    /// `fn(..) -> Result<RubyValue, Signal>` -- emitted with `?`.
    Fallible,
    /// Like `Value`, but wrapped in the ZeroDivisionError guard.
    DivMod,
    /// `fn(..) -> bool` -- wrapped in `RubyValue::Bool`.
    Bool,
    /// `fn(..) -> i64` (`<=>`, never nil for Int pairs) -- wrapped in
    /// `RubyValue::Int`.
    Cmp,
}

const INT_UNARY_OPS: &[(&str, &str)] = &[
    ("-@", "int_neg"),
    ("+@", "int_pos"),
    ("~", "int_bnot"),
];

/// Wraps `spinel_rt::int_div`/`int_mod`'s call with a zero-divisor check,
/// raising a real, catchable `ZeroDivisionError` instead of letting the
/// division/modulo itself hard-panic the whole process -- real Ruby's own
/// behavior (unlike `Float`, where division by zero is `Infinity`/`NaN`/
/// `NaN`, not an error at all -- `float_div`'s own IEEE semantics already
/// give that for free, no check needed there). Found as a real,
/// previously-undetected gap via this session's own testing: `1 / 0`
/// crashed the entire generated binary with a raw Rust panic (`spinel_rt::
/// int_div`'s own internal `/` panicking) rather than raising something a
/// `rescue ZeroDivisionError` could ever catch. A no-op passthrough for
/// every other `rt_fn` (every non-`/`/`%` operator).
fn emit_int_div_or_mod_checked(
    cx: &Ctx,
    rt_fn: &str,
    recv_expr: TokenStream,
    arg_expr: TokenStream,
) -> TokenStream {
    let func = format_ident!("{rt_fn}");
    let err = super::expr::emit_boxed_new(
        cx,
        "ZeroDivisionError",
        vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new("divided by 0".to_string())) }],
    );
    quote! {
        {
            let __divisor = #arg_expr;
            if spinel_rt::int_is_zero(&__divisor) {
                return Err(spinel_rt::Signal::Raise(#err));
            }
            spinel_rt::#func(&(#recv_expr), &__divisor)
        }
    }
}

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
/// A `FrozenError` for a mutation attempt on a frozen `class_name` receiver,
/// with CRuby's exact message shape (`can't modify frozen Array: [1, 2, 3]`
/// -- `error.c:4221`, the class name static since every guarded site is
/// type-gated, the receiver's `inspect` computed at runtime). Constructed by
/// codegen, not inside the `spinel_rt` mutators, for the same reason as
/// `array_set`'s `IndexError` contract: only codegen can build an exception
/// object (see `emit_boxed_new`'s docs). `recv_value` must be a
/// `RubyValue`-typed expression valid at the emission site (the guarded
/// blocks bind `__recv` first and pass a rewrapped clone here).
/// A `RactorError` (the flat stand-in for `Ractor::Error` -- nested class
/// names don't exist yet) whose message comes from a runtime `__msg: String`
/// in scope at the emission site (boundary-crossing rejections are computed
/// at runtime, unlike `FiberError`'s fixed strings).
fn emit_ractor_error(cx: &Ctx) -> TokenStream {
    super::expr::emit_boxed_new(
        cx,
        "RactorError",
        vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(__msg)) }],
    )
}

/// Finishes a dynamic-dispatch (`send_value`) emission: `catch_break`
/// wraps the call ONLY when the call site itself carries a block -- a
/// `Signal::Break` can only ever target a block attached to THIS call, so
/// on a blockless call any arriving Break belongs to an OUTER block and
/// must keep propagating. (The unconditional wrap this replaced silently
/// ate a consumer's iteration-terminating break as it crossed a
/// `Yielder#<<` call inside an `Enumerator.new` generator -- turning
/// `infinite_enum.take(3)` into a hang. Phase 17.2.)
fn wrap_dynamic_result(has_block: bool, call: TokenStream) -> TokenStream {
    if has_block {
        quote! { spinel_rt::catch_break(#call)? }
    } else {
        quote! { (#call)? }
    }
}

/// A `FiberError` with a fixed message -- CRuby's own wording, passed
/// verbatim from the dispatch sites (Phase 13.3).
fn emit_fiber_error(cx: &Ctx, msg: &str) -> TokenStream {
    super::expr::emit_boxed_new(
        cx,
        "FiberError",
        vec![quote! {
            spinel_rt::RubyValue::Str(spinel_rt::string_new(#msg.to_string()))
        }],
    )
}

fn emit_frozen_error(cx: &Ctx, class_name: &str, recv_value: TokenStream) -> TokenStream {
    super::expr::emit_boxed_new(
        cx,
        "FrozenError",
        vec![quote! {
            spinel_rt::RubyValue::Str(spinel_rt::string_new(format!(
                "can't modify frozen {}: {}",
                #class_name,
                (#recv_value).inspect_string()
            )))
        }],
    )
}

fn try_collection_dispatch(
    cx: &Ctx,
    recv_id: NodeId,
    name: &str,
    args: &[NodeId],
    recv_expr: &TokenStream,
) -> Option<TokenStream> {
    let ty = infer(cx, recv_id);
    let tokens = match (ty, name, args.len()) {
        // Only for a statically-Int index -- Range/other index shapes
        // fall through to the dynamic rows (Phase 17.1).
        (TyKind::Array, "[]", 1) if infer(cx, args[0]) == TyKind::Int => {
            let idx = emit_expr(cx, args[0]);
            quote! { spinel_rt::array_get(&(#recv_expr).as_array_unchecked(), (#idx).as_int_unchecked()) }
        }
        // Only for a statically-Int index -- a Range index is a SPLICE with
        // different semantics (to_ary coercion), served by the dynamic row.
        (TyKind::Array, "[]=", 2) if infer(cx, args[0]) == TyKind::Int => {
            let idx = emit_expr(cx, args[0]);
            let val = emit_expr(cx, args[1]);
            // Boxed if Object-typed, same as Hash's `[]=` value below: the
            // stored element must be a real `RubyValue`.
            let val = super::expr::box_if_object_typed(cx, args[1], val);
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
            let frozen_error =
                emit_frozen_error(cx, "Array", quote! { spinel_rt::RubyValue::Array(__recv.clone()) });
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
                        return Err(spinel_rt::Signal::Raise(#frozen_error));
                    }
                    match spinel_rt::array_set(&__recv, __idx, __val) {
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
            // Boxed if Object-typed (Phase 16.2): an object KEY reaches the
            // `HashKey` projection (which now dispatches a user `hash`).
            let key = super::expr::box_if_object_typed(cx, args[0], key);
            quote! { spinel_rt::hash_index(&(#recv_expr).as_hash_unchecked(), &(#key))? }
        }
        (TyKind::Hash, "[]=", 2) => {
            let key = emit_expr(cx, args[0]);
            let key = super::expr::box_if_object_typed(cx, args[0], key);
            let val = emit_expr(cx, args[1]);
            let val = super::expr::box_if_object_typed(cx, args[1], val);
            let frozen_error =
                emit_frozen_error(cx, "Hash", quote! { spinel_rt::RubyValue::Hash(__recv.clone()) });
            // The frozen check runs BEFORE `hash_set` ever hashes the key --
            // the same ordering CRuby guarantees (`rb_hash_modify` is
            // `rb_hash_aset`'s first statement, ahead of any `st_update`).
            quote! {
                {
                    let __recv = (#recv_expr).as_hash_unchecked();
                    let __key = #key;
                    let __val = #val;
                    if __recv.is_frozen() {
                        return Err(spinel_rt::Signal::Raise(#frozen_error));
                    }
                    spinel_rt::hash_set(&__recv, __key, __val)
                }
            }
        }
        (TyKind::Hash, "length" | "size", 0) => {
            quote! { spinel_rt::RubyValue::Int(spinel_rt::hash_len(&(#recv_expr).as_hash_unchecked())) }
        }
        (TyKind::Str, "[]", 1) if infer(cx, args[0]) == TyKind::Int => {
            let idx = emit_expr(cx, args[0]);
            quote! { spinel_rt::string_get(&(#recv_expr).as_str_unchecked(), (#idx).as_int_unchecked()) }
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

/// `Regexp`/`MatchData` built-in methods, and `String`'s methods that take a
/// `Regexp` pattern argument (Phase 12.7) -- mirrors `try_collection_dispatch`'s
/// shape (a `None` return falls through to ordinary Path 1/Path 2 dispatch),
/// kept as its own function since `gsub`/`sub`'s block form needs the call
/// site's own `block`, which `try_collection_dispatch` was never threaded to
/// receive.
///
/// Scope-cut, checked at `spinelc` CODEGEN time (a clear, immediate panic,
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
fn try_regexp_dispatch(
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
            ("source", 0) => return Some(quote! { spinel_rt::regexp_source(&(#recv_expr).as_regexp_unchecked()) }),
            ("to_s", 0) => return Some(quote! { spinel_rt::regexp_to_s(&(#recv_expr).as_regexp_unchecked()) }),
            ("inspect", 0) => return Some(quote! { spinel_rt::regexp_inspect(&(#recv_expr).as_regexp_unchecked()) }),
            // `===` is safe against ANY subject shape (real Ruby: `Regexp#===`
            // is `false`, not an error, for a non-String) -- routed through
            // `RubyValue::rb_case_eq` rather than requiring a statically
            // `Str`-typed argument like the other arms here, since it's the
            // one Regexp method real Ruby itself designed to be called with
            // an arbitrary-shaped subject (`case/when` dispatch).
            ("===", 1) => {
                let arg_expr = emit_expr(cx, args[0]);
                return Some(quote! { spinel_rt::RubyValue::Bool((#recv_expr).rb_case_eq(&(#arg_expr))) });
            }
            ("=~", 1) => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard spinel_rt::regexp_match_index(&(#recv_expr).as_regexp_unchecked(), &__h) }
                });
            }
            // `~ rxp` matches the pattern against `$_` (the last input line),
            // returning the match position or nil and setting `$~`. A non-
            // String `$_` clears the match and answers nil.
            ("~", 0) => {
                let bx = cx.box_id;
                return Some(quote! {
                    {
                        match spinel_rt::global_get(#bx, "$_") {
                            spinel_rt::RubyValue::Str(__s) => {
                                let __g = __s.lock();
                                let __h = __g.to_utf8_lossy();
                                spinel_rt::regexp_match_index(&(#recv_expr).as_regexp_unchecked(), &__h)
                            }
                            _ => {
                                spinel_rt::set_last_match(None);
                                spinel_rt::RubyValue::Nil
                            }
                        }
                    }
                });
            }
            ("!~", 1) => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard spinel_rt::RubyValue::Bool(!spinel_rt::regexp_is_match(&(#recv_expr).as_regexp_unchecked(), &__h)) }
                });
            }
            ("match", 1) => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard spinel_rt::regexp_match(&(#recv_expr).as_regexp_unchecked(), &__h) }
                });
            }
            ("match?", 1) => {
                let guard = str_guard(args[0], "__h")?;
                return Some(quote! {
                    { #guard spinel_rt::RubyValue::Bool(spinel_rt::regexp_is_match(&(#recv_expr).as_regexp_unchecked(), &__h)) }
                });
            }
            _ => {}
        }
    }

    if ty == TyKind::Str && !args.is_empty() {
        let pattern_is_regexp = infer(cx, args[0]) == TyKind::Regexp;
        // A non-Regexp pattern (String, or dynamically typed) falls
        // through to `send_value`, whose String rows handle String
        // patterns for split/sub/gsub/match/match? (Phase 17.1 -- the old
        // compile-time "pass a Regexp literal instead" rejection retired).

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
                        { #haystack_guard spinel_rt::regexp_match_index(&(#re_expr).as_regexp_unchecked(), &__h) }
                    });
                }
                ("!~", 1) => {
                    return Some(quote! {
                        { #haystack_guard spinel_rt::RubyValue::Bool(!spinel_rt::regexp_is_match(&(#re_expr).as_regexp_unchecked(), &__h)) }
                    });
                }
                ("match", 1) => {
                    return Some(quote! {
                        { #haystack_guard spinel_rt::regexp_match(&(#re_expr).as_regexp_unchecked(), &__h) }
                    });
                }
                ("match?", 1) => {
                    return Some(quote! {
                        { #haystack_guard spinel_rt::RubyValue::Bool(spinel_rt::regexp_is_match(&(#re_expr).as_regexp_unchecked(), &__h)) }
                    });
                }
                ("scan", 1) => {
                    return Some(quote! {
                        { #haystack_guard spinel_rt::regexp_scan(&(#re_expr).as_regexp_unchecked(), &__h) }
                    });
                }
                ("split", 1) => {
                    return Some(quote! {
                        { #haystack_guard spinel_rt::regexp_split(&(#re_expr).as_regexp_unchecked(), &__h, 0) }
                    });
                }
                ("sub", 2) if infer(cx, args[1]) == TyKind::Str => {
                    let repl_expr = emit_expr(cx, args[1]);
                    return Some(quote! {
                        { #haystack_guard let __r = (#repl_expr).as_str_unchecked(); let __r = __r.lock(); let __r = __r.to_utf8_lossy();
                          spinel_rt::regexp_sub(&(#re_expr).as_regexp_unchecked(), &__h, &__r) }
                    });
                }
                ("gsub", 2) if infer(cx, args[1]) == TyKind::Str => {
                    let repl_expr = emit_expr(cx, args[1]);
                    return Some(quote! {
                        { #haystack_guard let __r = (#repl_expr).as_str_unchecked(); let __r = __r.lock(); let __r = __r.to_utf8_lossy();
                          spinel_rt::regexp_gsub(&(#re_expr).as_regexp_unchecked(), &__h, &__r) }
                    });
                }
                ("sub", 1) => {
                    let block_id = block?;
                    let blk_expr = emit_proc_value(cx, block_id);
                    return Some(quote! {
                        { #haystack_guard
                          spinel_rt::regexp_sub_block(&(#re_expr).as_regexp_unchecked(), &__h, &(#blk_expr).as_proc_unchecked())? }
                    });
                }
                ("gsub", 1) => {
                    let block_id = block?;
                    let blk_expr = emit_proc_value(cx, block_id);
                    return Some(quote! {
                        { #haystack_guard
                          spinel_rt::regexp_gsub_block(&(#re_expr).as_regexp_unchecked(), &__h, &(#blk_expr).as_proc_unchecked())? }
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
                    spinel_rt::matchdata_get(&(#recv_expr).as_matchdata_unchecked(), &(#arg_expr))
                });
            }
            ("pre_match", 0) => {
                return Some(quote! { spinel_rt::matchdata_pre_match(&(#recv_expr).as_matchdata_unchecked()) })
            }
            ("post_match", 0) => {
                return Some(quote! { spinel_rt::matchdata_post_match(&(#recv_expr).as_matchdata_unchecked()) })
            }
            ("to_a", 0) => return Some(quote! { spinel_rt::matchdata_to_a(&(#recv_expr).as_matchdata_unchecked()) }),
            ("captures", 0) => {
                return Some(quote! { spinel_rt::matchdata_captures(&(#recv_expr).as_matchdata_unchecked()) })
            }
            ("named_captures", 0) => {
                return Some(quote! { spinel_rt::matchdata_named_captures(&(#recv_expr).as_matchdata_unchecked()) })
            }
            ("string", 0) => return Some(quote! { spinel_rt::matchdata_string(&(#recv_expr).as_matchdata_unchecked()) }),
            ("to_s", 0) => return Some(quote! { spinel_rt::matchdata_to_s(&(#recv_expr).as_matchdata_unchecked()) }),
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
    Some(quote! { ((#recv_expr).as_proc_unchecked()).call(&[#(#arg_exprs),*])? })
}

pub fn emit_new(
    cx: &Ctx,
    class_name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
) -> TokenStream {
    // A USER class with a real `initialize`: bind its arguments through the
    // SAME `emit_call_args_to` machinery every other call site uses, so
    // `initialize` gets the full `Params` surface (splat/post/keyword/block)
    // rather than the required+optional-only subset `.new` used to bind by
    // hand -- `def initialize(*values)` was a compile-time rejection.
    //
    // Only this path can: the others have no generated `initialize` with a
    // `Params` to bind against (a builtin/module `.new` dispatches
    // dynamically; `Object.new`'s copy is a free function in a container).
    // They keep the token path below, which is also the one `raise`'s
    // synthetic-argument caller needs.
    let Some(cid) = cx.resolve_class(class_name) else {
        // Not a compile-time class -- a constant bound to a RUNTIME class
        // (`Foo = Class.new`, #97 F4). Read the constant at runtime and
        // dispatch `.new` dynamically; a truly-undefined constant raises
        // NameError via `const_get`, matching Ruby. Positional args + block
        // are threaded; kwargs on a runtime-class `.new` are a fast-follow.
        let arg_exprs = args.iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        let block_expr = match block {
            Some(b) => {
                let p = emit_proc_value(cx, b);
                quote! { Some(#p) }
            }
            None => quote! { None },
        };
        return quote! {
            {
                let __rtclass = spinel_rt::const_get(0, #class_name).ok_or_else(|| {
                    spinel_rt::raise_error("NameError", format!("uninitialized constant {}", #class_name))
                })?;
                spinel_rt::send_value(
                    &__rtclass,
                    spinel_rt::Symbol::intern("new"),
                    &[#(#arg_exprs),*],
                    #block_expr,
                )?
            }
        };
    };
    let ci = cx.compiler.class(cid);
    if cx.compiler.has_generated_struct(cid) {
        if let Some((_, sid)) = cx.compiler.method_in_chain(cid, "initialize") {
            let scope = cx.compiler.scope(sid);
            let ctor = emit_ctor_struct(cx, cid);
            // `initialize` takes `self: Arc<Self>` BY VALUE (see
            // `ruby_class!`'s docs), so it would move `__obj` -- clone the
            // handle (a refcount bump) to keep `__obj` returnable.
            //
            // A literal block passed to `.new` is forwarded to `initialize`
            // (so `yield`/`block_given?` inside it see it); `needs_block`
            // keeps the callee's block slot lined up either way.
            let init = super::params::emit_call_args_to(
                cx,
                &super::params::Callee::Method(quote! { __obj.clone() }),
                "initialize",
                &scope.params,
                args,
                kwargs,
                block,
                None,
                scope.needs_block_param() || block.is_some(),
            );
            return quote! { { let __obj = #ctor; #init; __obj } };
        }
    }
    // Boxed via `box_if_object_typed`: `initialize`'s own Rust parameters
    // are always plain `RubyValue` (see that function's docs) -- an
    // Object-typed constructor ARGUMENT (e.g. passing one class instance
    // into another's constructor) otherwise emits a bare, unboxed
    // `Arc<Concrete>`, a real `rustc` type mismatch confirmed by direct
    // reproduction.
    let mut arg_exprs: Vec<TokenStream> = args
        .iter()
        .map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        })
        .collect();
    // A builtin/module `.new` dispatches dynamically through `send_value_in`,
    // and an EXCEPTION-BACKED subclass's `.new` runs its `initialize` through
    // the runtime (`construct_by_class_id` -> the `emit_exc_trampoline`
    // trampoline) -- both carry keywords as one trailing Hash (the G2
    // convention), so `AError.new(msg, code: 9)`'s options reach the row.
    // (A struct-backed user class binds keywords through `emit_call_args_to`
    // above, not here.)
    if !kwargs.is_empty()
        && (ci.is_builtin || ci.is_module || cx.compiler.is_native_backed(cid))
    {
        let inserts = super::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
        arg_exprs.push(quote! {
            {
                let __kw = spinel_rt::hash_new(vec![]);
                #inserts
                spinel_rt::RubyValue::Hash(__kw)
            }
        });
    }
    emit_new_with_arg_tokens(cx, class_name, arg_exprs)
}

/// The bare `Arc<Concrete>` struct literal for one generated class -- every
/// ivar `Nil`, unfrozen. Shared by `emit_new`'s general-binder path and
/// `emit_new_with_arg_tokens`' hand-bound one, which must construct the
/// identical object.
fn emit_ctor_struct(cx: &Ctx, cid: crate::compiler::ClassId) -> TokenStream {
    let ci = cx.compiler.class(cid);
    let class_ident = super::ident::class_ident(cx.compiler, cid);
    let fields = ci.ivars.iter().map(|iv| {
        let f = safe_ident(iv);
        quote! { #f: spinel_rt::parking_lot::Mutex::new(spinel_rt::RubyValue::Nil), }
    });
    // Every object starts unfrozen -- `.freeze`'s per-object flag (see
    // `ruby_class!`'s `__frozen` field docs).
    let fields = quote! { __frozen: std::sync::atomic::AtomicBool::new(false), #(#fields)* };
    // Wrapped in `Arc` immediately, not just at `new_handle` time: a local
    // holding this needs to be `Arc::clone()`-able on every re-read
    // (`codegen::expr`'s `LocalRead` -- see `ruby_class!`'s `new_handle` docs
    // for why the bare struct can't just derive `Clone` instead). `Arc<T>`
    // derefs transparently, so Path 1's `(recv_expr).method(...)` calls still
    // work unchanged against a `self: Arc<Self>`-shaped method. `Arc` (not
    // `Rc`, Part 9): every generated struct is genuinely `Send + Sync`.
    quote! { std::sync::Arc::new(#class_ident { #fields }) }
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
    let __bx = cx.box_id;
    let cid = cx
        .resolve_class(class_name)
        .unwrap_or_else(|| panic!("unknown class `{class_name}`"));
    // A BUILT-IN's `.new` -- no generated struct exists to construct, but
    // that doesn't make the call an error: `Time.new(...)` is ordinary Ruby,
    // answered by the runtime's own class-method table. Route it dynamically
    // (exactly as the module case just below does) and let the runtime
    // decide: a builtin with a `new` row constructs, and one without raises
    // real Ruby's NoMethodError at the moment the call runs. This used to be
    // a compile-time panic ("built-in types are constructed via their own
    // literal syntax"), which was never true of `Time`/`File`/`Dir` and made
    // an unreachable `Time.new` fail the whole compile.
    // An IMMEDIATE-builtin subclass (`class MyInt < Integer`, D3) is
    // registry-only -- no constructor -- so its `.new` must dispatch
    // dynamically too, landing on `Class#new`'s NoMethodError arm (CRuby's
    // exact "undefined method 'new' for class MyInt").
    if cx.compiler.class(cid).is_builtin
        || cx.compiler.class(cid).is_module
        || cx.compiler.is_immediate_subclass(cid)
    {
        let id = cid.0;
        return quote! {
            spinel_rt::send_value_in(#__bx,
                &spinel_rt::RubyValue::Class(spinel_rt::ClassId(#id)),
                spinel_rt::Symbol::intern("new"),
                &[#(#arg_exprs),*],
                None,
            )?
        };
    }
    // A NATIVE-BACKED class (D3) -- an exception subclass (`RubyException`) or a
    // value-builtin subclass (`ValueSubclass`, `class Stack < Array`) -- has no
    // generated struct, so construct it through the runtime by id. Returns a
    // boxed `RubyValue` matching its `Poly` static type, running the registered
    // constructor (`exception_construct`/`value_subclass_construct`) which seeds
    // any payload and runs `initialize` just as the struct literal's inline
    // `.initialize(...)?` did.
    if cx.compiler.is_native_backed(cid) {
        let id = cid.0;
        return quote! {
            spinel_rt::construct_by_class_id(spinel_rt::ClassId(#id), &[#(#arg_exprs),*], None)?
        };
    }
    // `Object.new` -- a bare sentinel instance of the runtime root
    // (`spinel_rt::Object`), boxed: no generated struct exists (Object's
    // container holds top-level defs as free functions), and its static
    // type is `Poly` (see `types.rs`'s New/ClassObj exclusions), so the
    // whole expression is a plain `RubyValue`. Each call makes a fresh
    // `Arc` -- distinct identity, the sentinel idiom's whole point. A
    // user-defined `initialize` (top-level `def initialize` / `class
    // Object` reopen) is honored through its `__bm_Object` copy.
    if cid == crate::compiler::OBJECT_CLASS {
        let ctor = quote! {
            spinel_rt::RubyValue::Object(std::sync::Arc::new(spinel_rt::Object::default()))
        };
        return match cx.compiler.method_in_chain(cid, "initialize") {
            Some((_, sid)) => {
                let final_args = bind_new_args(cx, sid, class_name, arg_exprs);
                let needs_block = cx.compiler.scope(sid).needs_block_param();
                let block_slot = needs_block.then(|| quote! { , None });
                let mod_ident = super::ident::class_ident(cx.compiler, cid);
                quote! {
                    {
                        let __obj = #ctor;
                        #mod_ident::initialize(__obj.clone() #(, #final_args)* #block_slot)?;
                        __obj
                    }
                }
            }
            None if !arg_exprs.is_empty() => {
                // A raise, not a panic -- see the matching arm for user
                // classes below.
                let n = arg_exprs.len();
                let msg = format!("wrong number of arguments (given {n}, expected 0)");
                quote! {
                    {
                        #(let _ = #arg_exprs;)*
                        return Err(spinel_rt::raise_error("ArgumentError", #msg.to_string()));
                    }
                }
            }
            None => ctor,
        };
    }
    let ctor = emit_ctor_struct(cx, cid);

    match cx.compiler.method_in_chain(cid, "initialize") {
        Some((_, sid)) => {
            let mut final_args = bind_new_args(cx, sid, class_name, arg_exprs);
            // An `initialize` that uses `yield`/`&blk` still gets its block
            // slot (always `None` -- `.new` doesn't forward a block yet, a
            // narrower, pre-existing gap).
            if cx.compiler.scope(sid).needs_block_param() {
                final_args.push(quote! { None });
            }
            // `initialize` takes `self: Arc<Self>` BY VALUE now (see
            // `ruby_class!`'s docs), so calling it on `__obj` directly would
            // move it -- clone the `Arc` handle first (a cheap refcount bump,
            // not a deep copy) so `__obj` is still available to return.
            quote! { { let __obj = #ctor; __obj.clone().initialize(#(#final_args),*)?; __obj } }
        }
        // No user `initialize`, so the inherited `Object#initialize` takes
        // none -- passing any is an ArgumentError, not something to drop on
        // the floor. `Bag.new(1, 2)` on an `initialize`-less class silently
        // ignored its arguments and constructed happily.
        //
        // Raised at RUNTIME (after evaluating the arguments for their side
        // effects), not a compile panic: real Ruby resolves this arity at
        // runtime and the error is rescuable. Message shape oracle-verified
        // -- CRuby names no method in it.
        None if !arg_exprs.is_empty() => {
            let n = arg_exprs.len();
            let msg = format!("wrong number of arguments (given {n}, expected 0)");
            quote! {
                {
                    #(let _ = #arg_exprs;)*
                    return Err(spinel_rt::raise_error("ArgumentError", #msg.to_string()));
                }
            }
        }
        None => ctor,
    }
}

/// Binds `initialize`'s REQUIRED + OPTIONAL parameters the way a Path 1
/// call does (Phase 14.4): required args 1:1, each optional slot
/// `Some(expr)` when provided else `None` (the callee's own prologue lazily
/// evaluates the default). Splat/post/keyword params on `initialize` remain
/// out of scope, matching `emit_new_with_arg_tokens`'s original posture.
fn bind_new_args(
    cx: &Ctx,
    sid: crate::compiler::ScopeId,
    class_name: &str,
    arg_exprs: Vec<TokenStream>,
) -> Vec<TokenStream> {
    let params = &cx.compiler.scope(sid).params;
    let nreq = params.required.len();
    let nopt = params.optional.len();
    if params.rest.is_some()
        || !params.post.is_empty()
        || !params.keywords.is_empty()
        || params.keyword_rest.is_some()
    {
        panic!(
            "`{class_name}.new`: an `initialize` with splat/post/keyword parameters isn't supported yet (spike scope)"
        );
    }
    if arg_exprs.len() < nreq || arg_exprs.len() > nreq + nopt {
        panic!(
            "wrong number of arguments for `{class_name}.new` (given {}, expected {})",
            arg_exprs.len(),
            if nopt == 0 {
                nreq.to_string()
            } else {
                format!("{nreq}..{}", nreq + nopt)
            }
        );
    }
    let mut final_args: Vec<TokenStream> = Vec::with_capacity(nreq + nopt);
    let mut provided = arg_exprs.into_iter();
    for _ in 0..nreq {
        let e = provided.next().expect("bounds checked above");
        final_args.push(e);
    }
    for _ in 0..nopt {
        final_args.push(match provided.next() {
            Some(e) => quote! { Some(#e) },
            None => quote! { None },
        });
    }
    final_args
}

/// The value `self` has in the context currently being emitted, as a boxed
/// `RubyValue` ready for runtime dispatch: the concrete receiver inside an
/// instance method, the CLASS OBJECT inside a class method or class body
/// (both have no `self: Arc<Self>` binding), and the shared `main` object
/// at the top level. Every implicit-self dynamic-dispatch site routes
/// through this rather than re-deriving the rule.
pub(crate) fn boxed_implicit_self(cx: &Ctx) -> Option<TokenStream> {
    // Already a `RubyValue`, and the ONLY correct answer inside an escaping
    // block: `instance_exec` may have rebound the receiver, so an implicit-
    // self call there (`obj.instance_exec { helper }`) must dispatch on the
    // block's actual runtime self, not on whatever `self` meant lexically.
    if cx.self_is_dynamic {
        let slf = &cx.self_ident;
        return Some(quote! { (#slf).clone() });
    }
    if let Some(cid) = cx.current_class {
        let slf = &cx.self_ident;
        return Some(if cx.compiler.value_backed(cid) {
            quote! { (#slf.clone()) }
        } else {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#slf.clone())) }
        });
    }
    // A class method/class body: self is the class object. `class_self`
    // (the receiver), not `defining_class` (the lexical origin) -- they
    // differ for an inherited or `extend`ed class method, and it is the
    // receiver that `self` means. Falls back to `defining_class` only for a
    // context that somehow has one without the other, which shouldn't
    // arise; keeping the old answer there is strictly safer than panicking.
    if let Some(cid) = cx.class_self.or(cx.defining_class) {
        let id = cid.0;
        return Some(quote! { spinel_rt::RubyValue::Class(spinel_rt::ClassId(#id)) });
    }
    Some(quote! { spinel_rt::main_object() })
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
pub fn emit_super_inline(
    cx: &Ctx,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
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

    // The method CURRENTLY executing (whose lexical body this `super` call
    // sits inside) -- needed for bare `super`'s forwarding case (in the splice
    // AND the runtime-dispatch branches below). Guaranteed to exist: this
    // `super` is inside `mname`'s own body on `defining_class`.
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

    // `super` into an inherited VALUE builtin (D3): a `class Stack < Array`
    // method whose `super` finds NO user definition above targets the native
    // `Array` method -- `super` in `initialize` re-seats the payload, any other
    // runs the builtin against it. There is no HIR to splice (the builtin has no
    // `own_methods`), so dispatch through the runtime. Only reached when no user
    // ancestor overrides `mname`; a user parent still splices.
    if found.is_none() && cx.compiler.is_value_subclass(receiver_class) {
        return emit_value_super(cx, mname, &current_params, args, kwargs, zsuper, block);
    }

    let (new_defining_class, sid) = found.unwrap_or_else(|| {
        panic!(
            "`super`: no `{mname}` found above {}",
            cx.compiler.class(defining_class).name
        )
    });

    // `super` into a NATIVE exception method (D3): the resolved parent is a
    // pristine `BUILTIN_EXCEPTIONS_RB` body (`native_default`) whose real
    // behavior lives in a `spinel-rt` fn, not the retained HIR -- splicing that
    // HIR would set a visible `@message` ivar and miss the hidden message slot.
    // Dispatch through the runtime instead, resuming the MRO walk after
    // `defining_class` (`native_default` is only ever set on bootstrap-exception
    // scopes, so this can't fire for an ordinary struct class). A user
    // exception parent's own method (`!native_default`) still splices below --
    // its HIR body runs correctly against the dynamic-self receiver.
    if cx.compiler.scope(sid).native_default {
        return emit_super_native(cx, mname, &current_params, args, kwargs, zsuper, block);
    }

    let defining_scope = cx.compiler.scope(sid);
    let body = defining_scope.body.clone();
    // `loop_labels`/`label_counter` carry over from `cx` unchanged: the
    // parent method's body is spliced in at this call site, so a `break`
    // inside it must still target whatever loop lexically encloses the
    // `super` call, exactly as if that code were written there directly (see
    // `Ctx::loop_labels`'s docs).
    let defining_captures = super::captures::collect_escaping_captures(
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
        // UNCHANGED across the splice -- `self` is still the SAME concrete
        // receiver instance throughout a chain of nested `super` calls.
        current_class: Some(receiver_class),
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
    };
    // Binds the parent's OWN parameter names fresh, before splicing its body
    // in -- previously this relied on the parent's params
    // happening to share names with the calling method's own, since the
    // spliced body just referenced its param names directly with nothing
    // ever binding them. See `emit_super_arg_bindings`'s docs.
    let bindings = emit_super_arg_bindings(cx, &inline_cx, &defining_scope.params, &current_params, args, kwargs, zsuper);
    // A literal block at the `super` site becomes the spliced body's
    // `__blk` (its `yield` runs this block) -- built as a real Proc in the
    // CALLING scope (`cx`: captures resolve against the child method's own
    // locals), shadowing the child's `__blk` only inside the splice braces.
    // With no literal block, real Ruby forwards the current method's block
    // -- which the splice sees for free, `__blk` already being in scope.
    let blk_binding = block.map(|b| {
        let proc_value = emit_proc_value(cx, b);
        quote! { let __blk: Option<spinel_rt::RubyValue> = Some(#proc_value); }
    });
    // A fresh hoisting prelude of its own: the parent method's local
    // variables are a genuinely separate Ruby scope from the calling
    // (sub)method's, even though inlining splices their statements into the
    // same Rust expression position (see `hoisting`'s docs).
    // The parent's own params count as ALREADY BOUND for the hoisting
    // prelude (bound just above by `bindings`), so a parent body that
    // reassigns one rebinds the forwarded value instead of nil-shadowing
    // it -- same rule as an ordinary method's own prelude.
    let inlined = super::hoisting::emit_hoisted_body_with_extra_roots(
        &inline_cx,
        &body,
        &defining_scope.params.default_ids(),
        &defining_scope.params.bound_names(),
        false,
    );
    quote! { { #bindings #blk_binding #inlined } }
}

/// `super` from an exception-backed method into a NATIVE default parent
/// (`native_default`), dispatched through `spinel_rt::send_super_from` rather
/// than spliced (see `emit_super_inline`'s native-branch comment for why).
/// Builds the forwarded argument slice -- explicit `super(a, b)` args, or, for
/// bare `super`, the current method's own positional parameters (required /
/// optional / splatted `*rest` / post) -- then resumes the receiver's MRO walk
/// after `defining_class`. Keyword arguments append as one trailing Hash (the
/// runtime's G2 convention); the native `Exception` methods take none, so this
/// only matters for a user parent reached transitively, which the runtime walk
/// resolves correctly.
fn emit_super_native(
    cx: &Ctx,
    mname: &str,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    let self_ident = &cx.self_ident;
    let def_id = cx.defining_class.expect("`super` outside a method").0;
    let (pushes, block_expr) =
        emit_runtime_super_args(cx, current_params, args, kwargs, zsuper, block);
    quote! {
        {
            let mut __super_args: Vec<spinel_rt::RubyValue> = Vec::new();
            #(#pushes)*
            spinel_rt::send_super_from(
                &#self_ident,
                spinel_rt::ClassId(#def_id),
                spinel_rt::Symbol::intern(#mname),
                &__super_args,
                #block_expr,
            )?
        }
    }
}

/// `super` from a value-builtin subclass method into the inherited builtin
/// (D3): `spinel_rt::value_super` re-seats the payload for `initialize`, else
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
            let mut __super_args: Vec<spinel_rt::RubyValue> = Vec::new();
            #(#pushes)*
            spinel_rt::value_super(
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
            let e = super::expr::box_if_object_typed(cx, a, e);
            pushes.push(quote! { __super_args.push(#e); });
        }
        if !kwargs.is_empty() {
            let inserts = super::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
            pushes.push(quote! {
                {
                    let __kw = spinel_rt::hash_new(vec![]);
                    #inserts
                    __super_args.push(spinel_rt::RubyValue::Hash(__kw));
                }
            });
        }
    }
    let block_expr = match block {
        Some(b) => {
            let proc_value = emit_proc_value(cx, b);
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
        let temps: Vec<syn::Ident> =
            (0..src.len()).map(|i| format_ident!("__super_fwd{i}")).collect();
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
        let opt_bound = (src.len() - nreq - parent_params.post.len()).min(parent_params.optional.len());
        for (i, name) in parent_params.required.iter().enumerate() {
            let dst = safe_ident(name);
            let t = &temps[i];
            lets.push(quote! { let #dst: spinel_rt::RubyValue = #t; });
        }
        for (i, (name, default)) in parent_params.optional.iter().enumerate() {
            let dst = safe_ident(name);
            if i < opt_bound {
                let t = &temps[nreq + i];
                lets.push(quote! { let #dst: spinel_rt::RubyValue = #t; });
            } else {
                let default_expr = super::expr::emit_expr(inline_cx, *default);
                lets.push(quote! { let #dst: spinel_rt::RubyValue = #default_expr; });
            }
        }
        for (i, name) in parent_params.post.iter().enumerate() {
            let dst = safe_ident(name);
            let t = &temps[nreq + opt_bound + i];
            lets.push(quote! { let #dst: spinel_rt::RubyValue = #t; });
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
                _ => panic!("`super`: keyword argument names must be literal symbols (spike scope)"),
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
                            let val = super::expr::box_if_object_typed(cx, *v, emit_expr(cx, *v));
                            Some(quote! {
                                (spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#name)), #val)
                            })
                        }
                        _ => None,
                    },
                    KwArg::DoubleSplat(_) => None,
                })
                .collect();
            let dst = safe_ident(rest_name);
            quote! {
                let #dst: spinel_rt::RubyValue =
                    spinel_rt::RubyValue::Hash(spinel_rt::hash_new(vec![#(#pairs),*]));
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
            let names = missing.iter().map(|n| format!(":{n}")).collect::<Vec<_>>().join(", ");
            format!("missing keywords: {names}")
        };
        return quote! { return Err(spinel_rt::raise_error("ArgumentError", #msg.to_string())); };
    }

    let lets = parent_params.keywords.iter().map(|kw| {
        let (name, default) = match kw {
            KeywordParam::Required(name) => (name, None),
            KeywordParam::Optional(name, d) => (name, Some(d)),
        };
        let dst = safe_ident(name);
        match provided.get(name) {
            Some(&v) => {
                let e = super::expr::box_if_object_typed(cx, v, emit_expr(cx, v));
                quote! { let #dst: spinel_rt::RubyValue = #e; }
            }
            None => {
                let default_expr = emit_expr(inline_cx, *default.expect("required-missing handled above"));
                quote! { let #dst: spinel_rt::RubyValue = #default_expr; }
            }
        }
    });
    let lets: Vec<TokenStream> = lets.collect();
    quote! { #(#lets)* #kwrest_let }
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

pub(crate) fn emit_proc_or_lambda_value(cx: &Ctx, params: &Params, body: &[NodeId], is_lambda: bool) -> TokenStream {
    let block_caps = super::captures::block_captures(cx.compiler, params, body, cx.current_class);

    let mut genuine: Vec<&String> = block_caps.locals.iter().filter(|n| cx.captured_locals.contains(*n)).collect();
    genuine.sort();
    let capture_clones = genuine.iter().map(|name| {
        let ident = safe_ident(name);
        quote! { let #ident = ::std::sync::Arc::clone(&#ident); }
    });
    let mut own_only: std::collections::HashSet<String> = block_caps
        .locals
        .iter()
        .filter(|n| !cx.captured_locals.contains(*n))
        .cloned()
        .collect();

    // Nested-Proc guard (Phase 13.5, replacing the old blanket "no block
    // escaping inside another escaping block" rejection): names shared with
    // the enclosing METHOD are `Captured` cells and compose through any
    // nesting depth, but an `own_only` name that this INNER block never
    // assigns itself can only be the enclosing BLOCK's own local -- a plain
    // per-invocation `let`, not a cell, which a `move` closure can't share
    // correctly (fresh-declaring it here would silently read `nil` where
    // real Ruby sees the outer block's value). Reject that narrow case
    // cleanly; everything else nests fine.
    if cx.in_real_proc && !own_only.is_empty() {
        let mut assigned_here = Vec::new();
        for &n in body {
            super::hoisting::collect_locals(cx.compiler, n, &mut assigned_here);
        }
        let assigned_here: std::collections::HashSet<&String> = assigned_here.iter().collect();
        if let Some(outer_block_local) = own_only.iter().find(|n| !assigned_here.contains(n)) {
            panic!(
                "a nested escaping block capturing its enclosing BLOCK's own local `{outer_block_local}` isn't supported yet (spike scope) -- move it to the enclosing method/top level, which makes it a shared Captured cell"
            );
        }
    }

    // A `yield`/`block_given?` anywhere in this block's body (nested blocks
    // included) targets the enclosing METHOD's implicit block -- clone its
    // `__blk: Option<RubyValue>` into the closure so the spliced yield
    // codegen (`expr.rs`'s `HirNode::Yield`) finds it. Composes through
    // nesting: an outer closure whose inner block yields captures `__blk`
    // itself first (the scan is transitive), so each closure clones from
    // its lexical parent. The enclosing method is guaranteed to HAVE
    // `__blk`: the same transitive scan sets its `uses_bare_block`
    // (`analyze::scan_bare_block_use`).
    let blk_clone = crate::analyze::scan_bare_block_use_body(&cx.compiler.hir, body)
        .then(|| quote! { let __blk = __blk.clone(); });

    // A block that mentions `self` (an ivar, a bare `self`, an implicit-self
    // call) does NOT capture it: it takes it as the closure's first
    // parameter, and the value below is only the DEFAULT -- the lexical self
    // that ordinary `#call`/`yield` runs under. `instance_exec` passes a
    // different one. `boxed_implicit_self` is exactly the "self here, as a
    // RubyValue" rule this needs, so it isn't re-derived.
    let needs_self = block_caps.self_captured;
    let self_default = needs_self.then(|| {
        let boxed = boxed_implicit_self(cx).expect("boxed_implicit_self is total");
        quote! { let __self_default = #boxed; }
    });

    // The block's OWN parameter names shadow the enclosing scope's metadata
    // for them -- see `Ctx::in_proc`.
    // Names THIS block binds (its own params or own locals) that a NESTED
    // escaping block captures (#97 F2b) -- e.g. the `m` in
    // `each { |m| define_method(m) { m } }`. They must become shared
    // `Arc<Mutex<RubyValue>>` cells so the inner closure can `Arc::clone` them,
    // exactly like a method promotes its OWN captured params (see
    // `params::emit_prologue`'s `captured_param_wraps`). Without this the inner
    // block would fresh-declare the name and read `nil` -- the case the panic
    // just below used to reject outright.
    let own_params = super::captures::own_param_names(params);
    let nested_captured: std::collections::HashSet<String> =
        super::captures::collect_escaping_captures(cx.compiler, body, params, cx.current_class)
            .locals;
    // A name that is both this block's own local AND captured by a nested
    // block is cell-declared below (`nested_local_decls`) and lives in
    // `proc_cx.captured_locals`; it must therefore leave `own_only`, or the
    // own-locals prelude would ALSO fresh-declare it -- a second binding that
    // shadows the shared cell (the inner closure then reads `nil`), which the
    // prelude's own-only invariant (`hoisting.rs`) forbids outright.
    own_only.retain(|n| !nested_captured.contains(n));
    let mut proc_cx = cx.in_proc(needs_self, &own_params);
    if !nested_captured.is_empty() {
        proc_cx.captured_locals.to_mut().extend(nested_captured.iter().cloned());
    }
    // Cell-wrap this block's own PARAMS that a nested block captures (after the
    // plain param binding reads its value).
    let mut nested_param_names: Vec<&String> =
        own_params.iter().filter(|n| nested_captured.contains(*n)).collect();
    nested_param_names.sort();
    let nested_param_wraps = nested_param_names.into_iter().map(|name| {
        let ident = safe_ident(name);
        quote! {
            let #ident: ::std::sync::Arc<spinel_rt::parking_lot::Mutex<spinel_rt::RubyValue>> =
                ::std::sync::Arc::new(spinel_rt::parking_lot::Mutex::new(#ident));
        }
    });
    // Declare cells for this block's own LOCALS (not params, not already an
    // enclosing-scope cell) that a nested block captures.
    let mut nested_local_names: Vec<&String> = nested_captured
        .iter()
        .filter(|n| !own_params.contains(*n) && !cx.captured_locals.contains(*n))
        .collect();
    nested_local_names.sort();
    let nested_local_decls = nested_local_names.into_iter().map(|name| {
        let ident = safe_ident(name);
        quote! {
            let #ident: ::std::sync::Arc<spinel_rt::parking_lot::Mutex<spinel_rt::RubyValue>> =
                ::std::sync::Arc::new(spinel_rt::parking_lot::Mutex::new(spinel_rt::RubyValue::Nil));
        }
    });
    let own_locals_prelude = super::hoisting::emit_proc_own_locals_prelude(&proc_cx, &own_only);
    let arity_check = is_lambda.then(|| emit_lambda_arity_check(cx, params, &format_ident!("__args")));
    let param_bindings =
        super::params::emit_proc_param_bindings(&proc_cx, params, &format_ident!("__args"), is_lambda);
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

    // `Proc#arity`/`#lambda?`/`#curry` read these -- a Rust closure can't
    // answer them about itself (see `spinel_rt::ProcData`).
    let arity = super::params::proc_arity(params, is_lambda);
    // `Proc#parameters` metadata, attached to the constructed proc.
    let proc_params = super::params::proc_parameters(params, is_lambda);
    // Two shapes, differing only in whether the body needs a receiver:
    // `with_self` takes one as a parameter (so `instance_exec` can rebind
    // it); `with_meta` is for a body that never mentions `self` and so has
    // nothing to rebind.
    let (ctor, self_param) = if needs_self {
        (
            quote! { spinel_rt::RProc::with_self },
            quote! { __self: &spinel_rt::RubyValue, },
        )
    } else {
        (quote! { spinel_rt::RProc::with_meta }, quote! {})
    };
    let default_arg = needs_self.then(|| quote! { __self_default, });
    quote! {
        {
            #(#capture_clones)*
            #blk_clone
            #self_default
            spinel_rt::RubyValue::Proc(#ctor(move |#self_param __args: &[spinel_rt::RubyValue]| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
                #redo_label: loop {
                    let __result: Result<spinel_rt::RubyValue, spinel_rt::Signal> = (|| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
                        #arity_check
                        #own_locals_prelude
                        #param_bindings
                        #(#nested_param_wraps)*
                        #(#nested_local_decls)*
                        #body_tokens
                    })();
                    match __result {
                        Err(spinel_rt::Signal::Redo) => continue #redo_label,
                        Err(spinel_rt::Signal::Next(__v)) => break #redo_label Ok(__v),
                        #terminal_arm
                    }
                }
            }, #default_arg #arity, #is_lambda).with_home().with_params(#proc_params))
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
    let has_keywords = !params.keywords.is_empty() || params.keyword_rest.is_some();
    let min_lit = nreq + npost;
    let min_cond = (min_lit > 0).then(|| quote! { __argc < #min_lit });
    let max_cond = (!has_rest).then(|| {
        let max_lit = nreq + nopt + npost;
        quote! { __argc > #max_lit }
    });
    let cond = match (min_cond, max_cond) {
        (None, None) => return TokenStream::new(),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (Some(a), Some(b)) => quote! { #a || #b },
    };
    // A lambda that declares keyword params consumes a trailing kwargs Hash
    // as keywords, not as a positional argument, so it must not count toward
    // positional arity (mirrors `emit_proc_param_bindings`' kw-source split).
    let argc = if has_keywords {
        quote! {
            let __argc = if matches!(#args_ident.last(), Some(spinel_rt::RubyValue::Hash(_))) {
                #args_ident.len() - 1
            } else {
                #args_ident.len()
            };
        }
    } else {
        quote! { let __argc = #args_ident.len(); }
    };
    let err = super::expr::emit_boxed_new(
        cx,
        "ArgumentError",
        vec![quote! {
            spinel_rt::RubyValue::Str(spinel_rt::string_new(format!(
                "wrong number of arguments (given {}, expected {})",
                __argc, #min_lit
            )))
        }],
    );
    quote! {
        #argc
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
/// The G2 trailing-kwargs-hash convention's CALLER side: a dynamic call
/// site's keyword arguments as one `RubyValue::Hash` expression, appended
/// as the last element of the `send` argument slice. The receiving
/// trampoline (`params::dynamic_kwargs_binding`) pops and binds it when
/// the callee declares keywords; a keywordless callee sees it as an
/// ordinary trailing Hash (real Ruby's own pre-3.0-flavored collapse --
/// the documented no-`ruby2_keywords` approximation).
pub(super) fn emit_kwargs_trailing_hash(cx: &Ctx, kwargs: &[KwArg]) -> Option<TokenStream> {
    if kwargs.is_empty() {
        return None;
    }
    // Only reached on the Path-1 / non-splat route, where `emit_call`'s
    // routing guard has already sent any `**h` to `emit_splat_call` -- so
    // every element here is a literal `Pair`.
    let pairs = kwargs.iter().map(|kw| {
        let KwArg::Pair(k, v) = kw else {
            unreachable!("a `**` double-splat routes to emit_splat_call, never here")
        };
        let ke = emit_expr(cx, *k);
        let ve = {
            let e = emit_expr(cx, *v);
            box_if_object_typed(cx, *v, e)
        };
        quote! { (#ke, #ve) }
    });
    Some(quote! {
        spinel_rt::RubyValue::Hash(spinel_rt::hash_new(vec![#(#pairs),*]))
    })
}

pub(super) fn emit_block_option(cx: &Ctx, block: Option<NodeId>, block_arg: Option<NodeId>) -> TokenStream {
    match (block, block_arg) {
        (Some(b), None) => {
            let v = emit_proc_value(cx, b);
            quote! { Some(#v) }
        }
        (None, Some(e)) => {
            let v = emit_expr(cx, e);
            // Boxed if Object-typed: `&obj` duck-types through `to_proc`,
            // and the converter takes a real `RubyValue`.
            let v = box_if_object_typed(cx, e, v);
            // `&expr` converts like real Ruby: Proc passes, Symbol becomes
            // `Symbol#to_proc` (`map(&:to_s)`), nil means no block.
            quote! { spinel_rt::block_arg_to_proc(#v)? }
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
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    safe: bool,
) -> TokenStream {
    let __bx = cx.box_id;
    // A call-site `*expr`/`**h` splat can't take any of the arity-checked
    // static paths below (the flattened argument COUNT isn't known until
    // runtime) -- see `emit_splat_call`'s docs for the always-dynamic
    // fallback this routes to instead. This is THE routing linchpin: any
    // `**h` `DoubleSplat` (or positional `*expr`) goes dynamic, so every
    // Path-1 consumer below only ever sees pure `Pair` kwargs. The common
    // case is unaffected: `args` unwraps to a plain `Vec<NodeId>` and every
    // fast path runs exactly as before.
    if kwargs.iter().any(|k| matches!(k, KwArg::DoubleSplat(_)))
        || args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
    {
        return emit_splat_call(cx, receiver, name, args, kwargs, block, block_arg, safe);
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
            // Inside a REOPENED builtin's method (Phase 16.3), `self` is the
            // `__self: RubyValue` parameter: an implicit-self call to a
            // sibling reopen method is a direct free-function call, and any
            // OTHER name (`length` inside `Array#under_limit?` -- a native
            // builtin method, oracle-verified as implicit-self-reachable)
            // dispatches dynamically on `__self` through `send_value`, whose
            // value-methods-then-curated-tables order resolves it exactly
            // like an explicit `self.length` would.
            if cx.compiler.value_backed(cid) {
                let slf = &cx.self_ident;
                if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
                    let scope = cx.compiler.scope(sid);
                    let mod_ident = super::ident::class_ident(cx.compiler, cid);
                    let method_ident = safe_ident(name);
                    return super::params::emit_call_args_to(
                        cx,
                        &super::params::Callee::FreeFn {
                            path: quote! { #mod_ident::#method_ident },
                            recv: quote! { #slf.clone() },
                        },
                        name,
                        &scope.params,
                        args,
                        kwargs,
                        block,
                        block_arg,
                        scope.needs_block_param(),
                    );
                }
                // The universal Kernel forms resolve here too -- `puts`/
                // `proc { }`/`__method__` inside a reopened builtin's (or a
                // top-level) method body are Kernel calls, not methods of
                // the receiver, and the dynamic fallback below would miss
                // them at runtime.
                if let Some(tokens) =
                    emit_universal_implicit_form(cx, name, args, kwargs, block, block_arg)
                {
                    return tokens;
                }
                let arg_exprs = args.iter().map(|&a| {
                    let e = emit_expr(cx, a);
                    box_if_object_typed(cx, a, e)
                });
                // Keyword args ride as one trailing Hash (the G2
                // convention) -- the callee's trampoline binds it.
                let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
                let block_value = emit_block_option(cx, block, block_arg);
                let dyn_call = quote! {
                    spinel_rt::send_value_in(#__bx,
                        &#slf,
                        spinel_rt::Symbol::intern(#name),
                        &[#(#arg_exprs,)* #(#kw_hash,)*],
                        #block_value,
                    )
                };
                return wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
            }
            // The Path 1 direct call needs `self` to BE an `Arc<Concrete>` of
            // this class -- true in a method body, false inside an escaping
            // block, whose self is a `RubyValue` parameter that
            // `instance_exec` may have pointed at another class entirely.
            // There, fall through to the dynamic dispatch below: the sibling
            // method is then resolved against the receiver actually passed,
            // which is the whole point of rebinding.
            if let Some((_, sid)) = cx
                .compiler
                .method_in_chain(cid, name)
                .filter(|_| !cx.self_is_dynamic)
            {
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
        // A no-receiver call from WITHIN another CLASS method's own body
        // (`current_class` is `None` there -- no concrete `self` receiver
        // exists, see `codegen::mod::emit_class_method_fn`'s docs) to a
        // SIBLING class method on the same class/module (`def self.a;
        // b; end` calling `def self.b`, or the equivalent inside `class <<
        // self`) -- resolved the same way `ClassName.foo(...)` is
        // (`Compiler::class_method_in_chain`), dispatched as a direct
        // associated-function call (`Target::b(...)`). A prior version of
        // this function had no such branch at all, meaning `class << self`
        // blocks whose methods called each other implicitly (the common,
        // idiomatic reason to write several class methods together) always
        // panicked.
        //
        // Resolved against `class_self` (the RECEIVER class), NOT
        // `defining_class` (where the body was written): an implicit-self
        // call is a send to `self`, and in a class method `self` is the
        // class it was CALLED on. The two differ exactly when the method is
        // inherited or `extend`ed in, and using the lexical one there was
        // silently wrong in both directions -- oracle-verified:
        //   - `class Base; def self.create; new; end; end; Sub.create`
        //     built a Base, not a Sub;
        //   - `module H; def helped; name; end; end; class Ext; extend H;
        //     end; Ext.helped` answered "Helper", not "Ext".
        // Neither raised; both just quietly produced the wrong object.
        if cx.current_class.is_none() {
            if let Some(defining) = cx.class_self.or(cx.defining_class) {
                if cx.compiler.class_method_in_chain(defining, name).is_some() {
                    return emit_class_method_call_on(cx, defining, name, args, kwargs, block, block_arg);
                }
                // A bare `new` inside a class method (`def self.create;
                // new; end`) constructs the class itself -- `self` there IS
                // the class, so `new` resolves like `Self.new` (real
                // Ruby's rule; checked after the sibling lookup so a user
                // `def self.new` override wins).
                if name == "new"
                    && !cx.compiler.class(defining).is_module
                    && !cx.compiler.class(defining).is_builtin
                    && kwargs.is_empty()
                    && block.is_none()
                    && block_arg.is_none()
                {
                    // Boxed: this Call node infers as `Poly` (only a
                    // literal `HirNode::New` infers `Object(cid)`), so the
                    // expression must be a `RubyValue`.
                    let ctor = emit_new(cx, &cx.compiler.fq_name(defining), args, kwargs, None);
                    // A native-backed class (D3) has no struct to `new_handle`
                    // -- `emit_new` already yields a fully-boxed `RubyValue`
                    // built by the runtime.
                    if cx.compiler.is_native_backed(defining) {
                        return ctor;
                    }
                    let class_ident = super::ident::class_ident(cx.compiler, defining);
                    return quote! {
                        spinel_rt::RubyValue::Object(#class_ident::new_handle(#ctor))
                    };
                }
            }
        }
        // A TOP-LEVEL-defined method -- a private instance method on
        // `Object`, real Ruby's rule. Reachable via implicit self from the
        // top level (receiver: the runtime `main` object) and from a class
        // method's body (receiver: the class value -- a class object is
        // itself an Object instance, so Object's methods are genuinely in
        // its chain). Inside ordinary INSTANCE methods this branch never
        // fires: `mro::materialize` spread the same method into the class
        // itself, so the `current_class` branch above already resolved it
        // (with `@ivar`s correctly landing on that class's own struct).
        // Checked BEFORE the Kernel functions below so a top-level
        // `def puts` overrides the built-in, same as a sibling method would.
        if cx.current_class.is_none() {
            if let Some((_, sid)) =
                cx.compiler.method_in_chain(crate::compiler::OBJECT_CLASS, name)
            {
                let scope = cx.compiler.scope(sid);
                let mod_ident =
                    super::ident::class_ident(cx.compiler, crate::compiler::OBJECT_CLASS);
                let method_ident = safe_ident(name);
                // `boxed_implicit_self` IS this rule ("self here, boxed"),
                // including the case this used to get wrong: inside an
                // escaping block it answers the block's own receiver, so a
                // top-level def called from an `instance_exec`'d block runs
                // against the rebound self rather than always `main`.
                let recv = boxed_implicit_self(cx).expect("boxed_implicit_self is total");
                return super::params::emit_call_args_to(
                    cx,
                    &super::params::Callee::FreeFn {
                        path: quote! { #mod_ident::#method_ident },
                        recv,
                    },
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
        // The Kernel FUNCTIONS (Phase 17.1): the print family (multi-arg
        // now), conversions, rand/srand, throw, sleep, exit/abort --
        // checked AFTER sibling method resolution (a user `def puts`/`def
        // Integer` wins, real Ruby's rule; the old intercept-first
        // ordering was a latent bug this stage fixed). Capitalized-name
        // conversion calls WITH arguments parse as ordinary CallNodes, so
        // there's no ClassRef ambiguity.
        if let Some(tokens) =
            emit_universal_implicit_form(cx, name, args, kwargs, block, block_arg)
        {
            return tokens;
        }
        // `catch(:tag) { ... }` -- the one Kernel function that takes its
        // block as a first-class value.
        if name == "catch" && args.len() == 1 && kwargs.is_empty() {
            if let Some(b) = block {
                let tag = emit_expr(cx, args[0]);
                let blk = emit_proc_value(cx, b);
                return quote! { spinel_rt::kernel_catch(#tag, #blk)? };
            }
        }
        // `to_enum(:meth, *args)` / `enum_for` on the implicit self (Phase
        // 17.2): routed through dynamic dispatch, whose Kernel row builds
        // the Enumerator over the boxed receiver -- what the Struct
        // template's `return to_enum(:each) unless block_given?` compiles
        // to.
        if (name == "to_enum" || name == "enum_for")
            && kwargs.is_empty()
            && block.is_none()
            && block_arg.is_none()
        {
            if let Some(cid) = cx.current_class {
                let slf = &cx.self_ident;
                // A reopened builtin's (or `Object`'s) `self` is already a
                // boxed `RubyValue` (Phase 16.3).
                let boxed = if cx.compiler.value_backed(cid) {
                    quote! { (#slf.clone()) }
                } else {
                    let class_ident = super::ident::class_ident(cx.compiler, cid);
                    quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#slf.clone())) }
                };
                let arg_exprs: Vec<TokenStream> = args
                    .iter()
                    .map(|&a| {
                        let e = emit_expr(cx, a);
                        box_if_object_typed(cx, a, e)
                    })
                    .collect();
                return quote! {
                    spinel_rt::send_value_in(#__bx, 
                        &#boxed,
                        spinel_rt::Symbol::intern(#name),
                        &[#(#arg_exprs),*],
                        None,
                    )?
                };
            }
        }
        // Nothing static matched: sibling methods, top-level defs, the
        // Kernel functions and the `proc`/`at_exit`/`__method__`/`method`
        // forms have all been tried. Rather than rejecting at compile time,
        // dispatch on the implicit receiver through the runtime -- which is
        // both what real Ruby does and strictly more faithful than a
        // panic: it resolves the Kernel/Object methods that have no static
        // form here (`send`, `respond_to?`, `instance_variable_get`,
        // `freeze`, `tap`, ...), and a genuinely undefined name raises
        // NoMethodError at the moment the call runs -- so, as in CRuby,
        // an unreachable bad call stays silent.
        let recv = boxed_implicit_self(cx).expect("every context has an implicit self");
        let mut arg_exprs: Vec<TokenStream> = args
            .iter()
            .map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            })
            .collect();
        // Keyword arguments ride the G2 trailing-Hash convention.
        arg_exprs.extend(emit_kwargs_trailing_hash(cx, kwargs));
        let blk = emit_block_option(cx, block, block_arg);
        return quote! {
            spinel_rt::send_value_in(#__bx,
                &#recv,
                spinel_rt::Symbol::intern(#name),
                &[#(#arg_exprs),*],
                #blk,
            )?
        };
    };

    /// The universal implicit-self forms every method-body context shares:
    /// the Kernel functions below, `proc { }`, and `__method__` --
    /// consulted after sibling method resolution (a user override wins,
    /// real Ruby's rule) from both the ordinary implicit-self path and the
    /// value-backed (builtin-reopen / top-level) method-body path.
    fn emit_universal_implicit_form(
        cx: &Ctx,
        name: &str,
        args: &[NodeId],
        kwargs: &[KwArg],
        block: Option<NodeId>,
        block_arg: Option<NodeId>,
    ) -> Option<TokenStream> {
        if let Some(tokens) = emit_kernel_function(cx, name, args, kwargs, block, block_arg) {
            return Some(tokens);
        }
        // `proc { ... }` -- Kernel#proc: the literal block AS a Proc value
        // (`lambda { ... }` desugars in parse to `HirNode::Lambda` already;
        // `proc`'s non-lambda semantics are exactly `emit_proc_value`'s).
        if name == "proc" && args.is_empty() && kwargs.is_empty() {
            if let Some(b) = block {
                return Some(emit_proc_value(cx, b));
            }
        }
        // `at_exit { ... }` -- registers the handler (run in reverse order
        // at process exit; see `spinel_rt::exec::run_at_exit`), answering
        // the Proc, CRuby's return value.
        if name == "at_exit" && args.is_empty() && kwargs.is_empty() {
            if let Some(b) = block {
                let p = emit_proc_value(cx, b);
                return Some(quote! {
                    {
                        let __h = #p;
                        spinel_rt::at_exit_register(__h.clone());
                        __h
                    }
                });
            }
        }
        // `__method__`/`__callee__` -- the enclosing method's name as a Symbol,
        // `nil` at the top level (a compile-time constant here: codegen always
        // knows which method body it's emitting). The two differ only under an
        // alias (`__callee__` reports the called-as name); we don't track
        // aliases, so they coincide.
        if (name == "__method__" || name == "__callee__")
            && args.is_empty()
            && kwargs.is_empty()
            && block.is_none()
        {
            return Some(match &cx.current_method {
                Some(m) => quote! { spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#m)) },
                None => quote! { spinel_rt::RubyValue::Nil },
            });
        }
        // `method(:name)` -- a bound Method object on the implicit self,
        // dispatched through the Kernel row (see `builtins::method_obj`).
        if name == "method" && args.len() == 1 && kwargs.is_empty() && block.is_none() {
            if let Some(recv) = boxed_implicit_self(cx) {
                let __bx = cx.box_id;
                let arg = {
                    let e = emit_expr(cx, args[0]);
                    box_if_object_typed(cx, args[0], e)
                };
                return Some(quote! {
                    spinel_rt::send_value_in(#__bx, &#recv, spinel_rt::Symbol::intern("method"), &[#arg], None)?
                });
            }
        }
        None
    }

    /// The implicit `self` as a boxed `RubyValue` expression, in every
    /// context that has one: an ordinary instance method (handle-boxed), a
    /// value-backed (builtin-reopen / Object) method (`__self` as-is), a
    /// class-method body (the class value), or the top level (the `main`
    /// object).

    /// The Kernel FUNCTIONS (Phase 17.1): the print family (multi-arg),
    /// conversions, rand/srand, throw, sleep, exit/abort -- consulted after
    /// sibling method resolution (a user `def puts`/`def Integer` wins,
    /// real Ruby's rule). Capitalized-name conversion calls WITH arguments
    /// parse as ordinary CallNodes, so there's no ClassRef ambiguity.
    /// `None` when the name (or call shape) isn't a Kernel function.
    fn emit_kernel_function(
        cx: &Ctx,
        name: &str,
        args: &[NodeId],
        kwargs: &[KwArg],
        block: Option<NodeId>,
        block_arg: Option<NodeId>,
    ) -> Option<TokenStream> {
        if block.is_some() || block_arg.is_some() {
            return None;
        }
        // The format family accepts keyword references (`format("%<x>d", x: 1)`);
        // its keywords become a trailing options Hash the sprintf engine reads.
        // Every other Kernel function here takes no keywords, so bail for them.
        let is_format_family = matches!(name, "format" | "sprintf" | "printf");
        if !kwargs.is_empty() && !is_format_family {
            return None;
        }
        let plain_fn: Option<&str> = None;
        // Fallible functions (`?`); the conversions require >= 1 arg
        // (a bare `Integer` parses as a ClassRef, never reaches here).
        // The print family is fallible since it routes through `$stdout`/
        // `$stderr` (a duck-typed redirect target's `write` can raise).
        let fallible_fn = match name {
            "puts" => Some("kernel_puts"),
            "p" => Some("kernel_p"),
            "pp" => Some("kernel_pp"),
            "print" => Some("kernel_print"),
            "warn" => Some("kernel_warn"),
            "Integer" if !args.is_empty() => Some("kernel_integer"),
            "Float" if !args.is_empty() => Some("kernel_float"),
            "Rational" if !args.is_empty() => Some("kernel_rational"),
            "Complex" if !args.is_empty() => Some("kernel_complex"),
            "String" if !args.is_empty() => Some("kernel_string"),
            "Array" if !args.is_empty() => Some("kernel_array"),
            "Hash" if !args.is_empty() => Some("kernel_hash"),
            "format" | "sprintf" => Some("kernel_format"),
            "printf" => Some("kernel_printf"),
            "rand" => Some("kernel_rand"),
            "srand" => Some("kernel_srand"),
            "throw" if !args.is_empty() => Some("kernel_throw"),
            "sleep" => Some("kernel_sleep"),
            _ => None,
        };
        let never_fn = match name {
            "exit" => Some("kernel_exit"),
            "abort" => Some("kernel_abort"),
            _ => None,
        };
        if plain_fn.is_none() && fallible_fn.is_none() && never_fn.is_none() {
            return None;
        }
        let mut arg_exprs: Vec<TokenStream> = args
            .iter()
            .map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            })
            .collect();
        // Format-family keywords ride along as one trailing Hash (the G2 ABI),
        // which the sprintf engine consults for `%<name>`/`%{name}` references.
        if !kwargs.is_empty() {
            let inserts = super::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
            arg_exprs.push(quote! {
                {
                    let __kw = spinel_rt::hash_new(vec![]);
                    #inserts
                    spinel_rt::RubyValue::Hash(__kw)
                }
            });
        }
        if let Some(f) = plain_fn {
            let func = format_ident!("{f}");
            return Some(quote! { spinel_rt::#func(&[#(#arg_exprs),*]) });
        }
        if let Some(f) = fallible_fn {
            let func = format_ident!("{f}");
            return Some(quote! { spinel_rt::#func(&[#(#arg_exprs),*])? });
        }
        let func = format_ident!("{}", never_fn.expect("one of the three sets matched"));
        Some(quote! { spinel_rt::#func(&[#(#arg_exprs),*]) })
    }

    // `ClassName.foo(...)` / `ModuleName.foo(...)` -- a call on the
    // class/module itself, not an instance (see `HirNode::ClassRef`'s
    // docs). Never a `RubyValue`, so this must be intercepted before the
    // ordinary `emit_expr(cx, recv_id)` receiver-evaluation path below ever
    // sees it (mirrors `HirNode::Block`'s "only reached via the Call that
    // invokes it" pattern).
    //
    // ONLY when `target_name` is an ACTUALLY-REGISTERED class/module --
    // `ClassRef` is also how an ORDINARY bare constant read lowers (see its
    // own docs: "used as a plain VALUE, ... an ordinary lexically-scoped
    // constant READ" when the name isn't a class), so `MAX.+(1)` (the
    // `MAX += 1` compound-assignment desugar, where `MAX` is a plain
    // Integer constant, not a class) must NOT take this branch -- found as
    // a real, previously-undetected bug via this session's own testing: it
    // unconditionally treated ANY `ClassRef` receiver as a class-method
    // call, so compound assignment (`+=`/`-=`/etc., every operator except
    // `||=`) on a non-class constant panicked with a confusing "unknown
    // class/module" error instead of reading its actual value.
    // The concurrency builtins' constructors and `Fiber.yield` (Phases
    // 13.3/13.5) -- intercepted ahead of the generic class-method branch
    // below (which would reject the block / find no such class method). A
    // `Fiber.new`/`Thread.new` block becomes an ordinary escaping `Proc`
    // via `emit_proc_value` -- the same capture machinery every other
    // escaping block uses, so captured locals/`self` compose for free. All
    // FiberError construction happens here, not in `spinel_rt::fiber_*`
    // (the `array_set`->`IndexError` division of labor; messages verbatim
    // from CRuby `cont.c`).
    if let HirNode::ClassRef(target_name) = &cx.compiler.hir[recv_id] {
        if !safe && kwargs.is_empty() {
            match (target_name.as_str(), name) {
                // `Proc.new { ... }` IS its block (CRuby: `proc_new` just
                // wraps the given block) -- the same value `proc { ... }`
                // builds, so it routes to the same emitter and carries the
                // same arity/lambda? metadata. Blockless `Proc.new` is an
                // ArgumentError in real Ruby; it falls through to the
                // builtin-`.new` rejection below rather than miscompiling.
                ("Proc", "new") if block.is_some() && args.is_empty() => {
                    return emit_proc_value(cx, block.expect("checked is_some"));
                }
                ("Fiber", "new") => {
                    let Some(block_id) = block else {
                        panic!("`Fiber.new` requires a literal block (spike scope -- `&proc` conversion isn't wired here yet)");
                    };
                    let proc = emit_proc_value(cx, block_id);
                    return quote! { spinel_rt::fiber_new(#proc) };
                }
                ("Fiber", "yield") if block.is_none() && block_arg.is_none() => {
                    let arg_exprs: Vec<TokenStream> = args
                        .iter()
                        .map(|&a| {
                            let e = emit_expr(cx, a);
                            super::expr::box_if_object_typed(cx, a, e)
                        })
                        .collect();
                    let root_error =
                        emit_fiber_error(cx, "attempt to yield on a not resumed fiber");
                    return quote! {
                        match spinel_rt::fiber_yield(vec![#(#arg_exprs),*]) {
                            spinel_rt::FiberYield::Value(__v) => __v,
                            // `Fiber#raise` injected an exception at this yield.
                            spinel_rt::FiberYield::Raise(__e) => return Err(spinel_rt::Signal::Raise(__e)),
                            spinel_rt::FiberYield::Root => return Err(spinel_rt::Signal::Raise(#root_error)),
                        }
                    };
                }
                // `Thread.new(*args) { |*params| }` -- constructor args pass
                // through to the block's params, matching CRuby.
                ("Thread", "new") => {
                    let Some(block_id) = block else {
                        panic!("`Thread.new` requires a literal block (spike scope -- `&proc` conversion isn't wired here yet)");
                    };
                    let proc = emit_proc_value(cx, block_id);
                    let arg_exprs: Vec<TokenStream> = args
                        .iter()
                        .map(|&a| {
                            let e = emit_expr(cx, a);
                            super::expr::box_if_object_typed(cx, a, e)
                        })
                        .collect();
                    return quote! { spinel_rt::thread_new(#proc, vec![#(#arg_exprs),*]) };
                }
                ("Mutex", "new") if args.is_empty() && block.is_none() => {
                    return quote! { spinel_rt::mutex_new() };
                }
                ("Queue", "new") if args.is_empty() && block.is_none() => {
                    return quote! { spinel_rt::queue_new() };
                }
                ("SizedQueue", "new") if args.len() == 1 && block.is_none() => {
                    let n = emit_expr(cx, args[0]);
                    return quote! {
                        spinel_rt::sized_queue_new((#n).as_int_unchecked())
                    };
                }
                // `Ractor.new(*args) { |*params| }` (Phase 13.8) -- block
                // ISOLATION is enforced HERE, at compile time (the capture
                // set is statically known), strictly earlier than CRuby's
                // own Proc-creation-time `Ractor::IsolationError`. Args
                // cross the boundary at runtime (shareable-by-reference or
                // deep-copied; a rejection raises `RactorError`).
                ("Ractor", "new") => {
                    let Some(block_id) = block else {
                        panic!("`Ractor.new` requires a literal block (spike scope)");
                    };
                    let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
                        panic!("a Block should only be reached via the Call that invokes it");
                    };
                    let block_caps = super::captures::block_captures(cx.compiler, params, body, cx.current_class);
                    // `block_captures` reports every referenced non-param
                    // name, INCLUDING the block's own locals (`msg =
                    // Ractor.receive` -- found the hard way). An outer-scope
                    // access is a name that's either a genuine shared
                    // capture (in `cx.captured_locals`) or one this block
                    // never assigns itself (an enclosing param/block-local).
                    let mut assigned_here = Vec::new();
                    for &n in body {
                        super::hoisting::collect_locals(cx.compiler, n, &mut assigned_here);
                    }
                    let assigned_here: std::collections::HashSet<&String> =
                        assigned_here.iter().collect();
                    if let Some(outer) = block_caps
                        .locals
                        .iter()
                        .filter(|n| cx.captured_locals.contains(*n) || !assigned_here.contains(n))
                        .min()
                    {
                        panic!("can not isolate a Proc because it accesses outer variables ({outer})");
                    }
                    if block_caps.self_captured {
                        panic!("can not isolate a Proc because it accesses instance variables of the enclosing object");
                    }
                    let proc = emit_proc_value(cx, block_id);
                    let arg_exprs: Vec<TokenStream> = args
                        .iter()
                        .map(|&a| {
                            let e = emit_expr(cx, a);
                            super::expr::box_if_object_typed(cx, a, e)
                        })
                        .collect();
                    let ractor_error = emit_ractor_error(cx);
                    return quote! {
                        match spinel_rt::ractor_new(#proc, vec![#(#arg_exprs),*]) {
                            Ok(__r) => __r,
                            Err(__msg) => return Err(spinel_rt::Signal::Raise(#ractor_error)),
                        }
                    };
                }
                ("Ractor", "receive") if args.is_empty() && block.is_none() => {
                    return quote! { spinel_rt::ractor_receive() };
                }
                ("Ractor", "make_shareable") if args.len() == 1 && block.is_none() => {
                    let v = emit_expr(cx, args[0]);
                    let v = super::expr::box_if_object_typed(cx, args[0], v);
                    let ractor_error = emit_ractor_error(cx);
                    return quote! {
                        match spinel_rt::make_shareable(&(#v)) {
                            Ok(__v) => __v,
                            Err(__msg) => return Err(spinel_rt::Signal::Raise(#ractor_error)),
                        }
                    };
                }
                ("Ractor", "shareable?") if args.len() == 1 && block.is_none() => {
                    let v = emit_expr(cx, args[0]);
                    let v = super::expr::box_if_object_typed(cx, args[0], v);
                    return quote! {
                        spinel_rt::RubyValue::Bool(spinel_rt::shareable(&(#v)))
                    };
                }
                _ => {}
            }
        }
    }

    if let Some(target_path) = super::expr::const_path_of(cx, recv_id) {
        if let Some(target) = cx.resolve_class(&target_path) {
            // `const_get`/`const_defined?` with a literal name fold against
            // the compile-time registry (a literal-constant receiver has no
            // side effects to preserve).
            if let Some(folded) =
                try_const_reflection(cx, target, name, args, kwargs, block, block_arg)
            {
                return folded;
            }
            // Path 1 only when the class actually DEFINES a matching class
            // method. Anything else falls through to the generic dynamic path
            // with the receiver as a first-class Class VALUE (Phase 16.1):
            // `Widget == Widget`, `Widget.name`, `Widget.ancestors` resolve
            // in `send_value`'s Class arm, and a genuinely unknown method is a
            // real runtime NoMethodError ("for class Widget") -- real Ruby's
            // behavior, replacing the old compile-time rejection.
            let is_static = cx.compiler.class_method_in_chain(target, name).is_some();
            if is_static {
                if safe {
                    panic!("safe-navigation on a class-method call isn't supported yet (spike scope)");
                }
                return emit_class_method_call_on(cx, target, name, args, kwargs, block, block_arg);
            }
        }
    }

    // A receiver STATICALLY TYPED as a class value (`x = Widget;
    // x.new(...)` / `x.some_class_method` -- Phase 16.1): same Path 1
    // dispatch a literal `Widget.` receiver gets, via the tracked
    // `TyKind::ClassObj`. The receiver expression is still evaluated for
    // side effects (a `let _ =` binding, like `is_a?`'s fold); anything
    // not statically resolvable falls through to the dynamic path
    // (`send_value`'s Class arm, including the registry constructor).
    if let TyKind::ClassObj(target) = infer(cx, recv_id) {
        // `x.class.const_get(:N)` / `.const_defined?(:N)` fold too, evaluating
        // the receiver expression for its side effects first.
        if let Some(folded) = try_const_reflection(cx, target, name, args, kwargs, block, block_arg) {
            let recv_expr = emit_expr(cx, recv_id);
            return quote! { { let _ = #recv_expr; #folded } };
        }
        if !safe && kwargs.is_empty() && block.is_none() && block_arg.is_none() {
            if name == "new" && !cx.compiler.class(target).is_module && !cx.compiler.class(target).is_builtin {
                let recv_expr = emit_expr(cx, recv_id);
                let ctor = emit_new(cx, &cx.compiler.fq_name(target), args, kwargs, None);
                return quote! { { let _ = #recv_expr; #ctor } };
            }
            if cx.compiler.class_method_in_chain(target, name).is_some() {
                let recv_expr = emit_expr(cx, recv_id);
                let call = emit_class_method_call_on(cx, target, name, args, kwargs, block, block_arg);
                return quote! { { let _ = #recv_expr; #call } };
            }
        }
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
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    safe: bool,
) -> TokenStream {
    let __bx = cx.box_id;
    if safe {
        panic!("safe-navigation (`&.`) on a call with a splat argument isn't supported yet (spike scope)");
    }
    let recv_obj_expr = match receiver {
        Some(recv_id) => {
            // A class-VALUE receiver (`Point.new(*args)` / `Klass.foo(**h)`):
            // a literal class constant, or a local statically typed as a class
            // object, emits its `RubyValue::Class` handle directly (see
            // `emit_expr`'s `ClassRef` arm) so the runtime dispatches `new`/the
            // class method through `send_value`'s Class arm -- the same
            // registry-constructor path `HirNode::New` reaches, just with a
            // runtime-built argument vector. Same "only an ACTUALLY-registered
            // class/module" guard as `emit_call`'s constant-receiver
            // interception.
            let is_class_receiver = super::expr::const_path_of(cx, recv_id)
                .is_some_and(|p| cx.resolve_class(&p).is_some())
                || matches!(infer(cx, recv_id), TyKind::ClassObj(_));
            if is_class_receiver {
                emit_expr(cx, recv_id)
            } else {
            let recv_expr = emit_expr(cx, recv_id);
            match infer_class(cx, recv_id) {
                Some(cid) => {
                    let class_ident = super::ident::class_ident(cx.compiler, cid);
                    quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
                }
                None if infer(cx, recv_id) == TyKind::Poly => quote! { (#recv_expr) },
                None => panic!("a splat argument call on a receiver whose class isn't statically known (and isn't a `rescue` binding) isn't supported yet (spike scope)"),
            }
            }
        }
        None => {
            // `puts`/other no-receiver builtins aren't reachable through
            // `spinel_rt::send` at all (they have no `ClassRegistry` entry) --
            // a clean rejection here beats generating code that only fails
            // at RUNTIME with a confusing "no such method".
            // The implicit receiver for THIS context -- a concrete `self`
            // inside an instance method, the class object inside a class
            // method/body, the `main` object at the top level (which
            // carries Object's value methods, so top-level defs dispatch).
            // Kernel functions with no registry entry (`puts`) can't be
            // reached this way, but they're intercepted well before here.
            boxed_implicit_self(cx).expect("every context has an implicit self")
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
    // Keyword args (literal pairs INTERLEAVED with `**h` double-splats, in
    // source order) merge into ONE trailing Hash (the G2 convention), built
    // by the shared `emit_kwarg_inserts` -- so `f(**a, c: 1, **b)` gets Ruby's
    // exact left-to-right, last-key-wins order (which the old two-phase
    // "literals then splats" build got wrong for a splat written before a
    // pair, and couldn't represent for two splats at all).
    let kw_push = (!kwargs.is_empty()).then(|| {
        let inserts = super::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
        quote! {
            let __kw = spinel_rt::hash_new(vec![]);
            #inserts
            // Only when non-empty: a `**h` whose hash is empty AT RUNTIME
            // contributes NOTHING -- `def c(h) = foo(1, **h); c({})` passes
            // just `1`, with no trailing hash (oracle-verified; pushing it
            // unconditionally silently handed the callee an extra `{}`
            // argument). About a RUNTIME-empty hash, not the literal `**{}` a
            // parser could fold away, so the guard belongs here, not lowering.
            // A literal keyword (`k: 1`) can never produce an empty hash, so
            // for a pairs-only list this check is simply never false.
            if !__kw.lock().is_empty() {
                __args.push(spinel_rt::RubyValue::Hash(__kw));
            }
        }
    });
    let block_value = emit_block_option(cx, block, block_arg);
    let dyn_call =
        quote! { spinel_rt::send_value_in(#__bx, &#recv_obj_expr, #name_expr, &__args, #block_value) };
    let dyn_call = wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
    quote! {
        {
            let mut __args: Vec<spinel_rt::RubyValue> = Vec::new();
            #(#arg_pushes)*
            #kw_push
            #dyn_call
        }
    }
}

/// Whether ANY reopened builtin defines `name` (Phase 16.3) -- the
/// Poly-receiver side of the universal arms' override check in `dispatch`:
/// a Poly value might turn out AT RUNTIME to be an instance of a reopened
/// builtin, so a universal method it overrides anywhere must route
/// dynamically (where `send_value`'s value-method-first probe decides per
/// actual class). Free for programs with no reopens: every builtin's
/// materialized method table is empty.
fn any_builtin_overrides(cx: &Ctx, name: &str) -> bool {
    cx.compiler.classes.iter().any(|c| {
        c.is_builtin
            && c.methods
                .iter()
                .any(|&sid| cx.compiler.scope(sid).name == name)
    })
}

/// The shared core behind `ClassName.foo(...)` (`emit_call`'s
/// constant-receiver interception, `target` resolved from the literal
/// path) AND an implicit-self call
/// made FROM WITHIN another class method's own body (`emit_call`'s
/// no-receiver branch, `target` already known as `cx.defining_class` --
/// no name to look up at all). Same "plain required parameters only, no
/// keyword args, no block" restriction either way (see
/// `codegen::mod::emit_class_method_fn`'s matching rejection).
/// Extracts the compile-time string of a literal Symbol (`:Name`) or
/// single-segment String (`"Name"`) argument -- the only forms the constant-
/// reflection fold recognizes; a computed name falls through to runtime.
fn literal_name_arg(cx: &Ctx, id: NodeId) -> Option<String> {
    match &cx.compiler.hir[id] {
        HirNode::SymbolLit(s) => Some(s.clone()),
        HirNode::StringLit(parts) => match parts.as_slice() {
            [crate::hir::StrPart::Lit(s)] => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// A well-formed constant name (`/\A\p{Upper}\w*\z/`): a leading uppercase
/// letter, then identifier characters. Anything else (`lower`, `_Foo`, `@x`,
/// `1A`, `A B`, `""`) is a `NameError "wrong constant name ..."`.
fn is_valid_const_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_uppercase() => chars.all(|c| c.is_alphanumeric() || c == '_'),
        _ => false,
    }
}

/// The class/module `target::cname` names, if any -- a nested definition in
/// `target`'s own namespace, or (const lookup inherits) a top-level class,
/// which lives on `Object` and so is visible from every receiver.
fn class_const_in(
    cx: &Ctx,
    target: crate::compiler::ClassId,
    cname: &str,
) -> Option<crate::compiler::ClassId> {
    if let Some(c) = cx.resolve_class(&format!("{}::{cname}", cx.compiler.fq_name(target))) {
        return Some(c);
    }
    cx.compiler.resolve_class(cname, &[], cx.box_id)
}

/// Whether a VALUE constant named `cname` is defined on `target` or any
/// ancestor (constant lookup inherits, up through `Object`) -- read from the
/// compile-time `const_owners` registry `resolve_consts` populates.
fn value_const_defined_in(cx: &Ctx, target: crate::compiler::ClassId, cname: &str) -> bool {
    let mut chain = cx.compiler.class(target).ancestors.clone();
    if !chain.contains(&crate::compiler::OBJECT_CLASS) {
        chain.push(crate::compiler::OBJECT_CLASS);
    }
    chain
        .iter()
        .any(|&anc| cx.compiler.class(anc).const_owners.contains_key(cname))
}

/// Compile-time fold of `Klass.const_get(:NAME)` / `Klass.const_defined?(:NAME)`
/// on a statically-known class/module `target` with a LITERAL name -- the
/// same flat constant/class registry a constant READ resolves against.
/// Returns `None` (fall through to ordinary dynamic dispatch) for a computed
/// name, extra args, a block, or any other method.
fn try_const_reflection(
    cx: &Ctx,
    target: crate::compiler::ClassId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> Option<TokenStream> {
    if (name != "const_get" && name != "const_defined?")
        || !kwargs.is_empty()
        || block.is_some()
        || block_arg.is_some()
    {
        return None;
    }
    let [arg] = args else { return None };
    let cname = literal_name_arg(cx, *arg)?;

    if !is_valid_const_name(&cname) {
        let msg = format!("wrong constant name {cname}");
        let err = super::expr::emit_boxed_new(
            cx,
            "NameError",
            vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(#msg.to_string())) }],
        );
        return Some(quote! { return Err(spinel_rt::Signal::Raise(#err)) });
    }

    let as_class = class_const_in(cx, target, &cname);
    if name == "const_defined?" {
        let defined = as_class.is_some() || value_const_defined_in(cx, target, &cname);
        return Some(quote! { spinel_rt::RubyValue::Bool(#defined) });
    }
    // const_get: a class-name constant answers the Class value directly; a
    // value constant reads through the runtime store (which also holds the
    // builtin-seeded ones, e.g. `Float::INFINITY`), raising a receiver-
    // qualified NameError on a genuine miss.
    if let Some(c) = as_class {
        let id = c.0;
        return Some(quote! { spinel_rt::RubyValue::Class(spinel_rt::ClassId(#id)) });
    }
    let owner = super::expr::const_owner_id(cx, Some(&cx.compiler.fq_name(target)), &cname);
    let qualified = if target == crate::compiler::OBJECT_CLASS {
        cname.clone()
    } else {
        format!("{}::{cname}", cx.compiler.fq_name(target))
    };
    let err = super::expr::emit_boxed_new(
        cx,
        "NameError",
        vec![quote! {
            spinel_rt::RubyValue::Str(spinel_rt::string_new(format!("uninitialized constant {}", #qualified)))
        }],
    );
    Some(quote! {
        match spinel_rt::const_get(#owner, #cname) {
            Some(__v) => __v,
            None => return Err(spinel_rt::Signal::Raise(#err)),
        }
    })
}

fn emit_class_method_call_on(
    cx: &Ctx,
    target: crate::compiler::ClassId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> TokenStream {
    let target_name = &cx.compiler.class(target).name;
    let Some((_, sid)) = cx.compiler.class_method_in_chain(target, name) else {
        panic!(
            "unsupported call `{target_name}.{name}` (spike scope, or no such class method is defined)"
        );
    };
    let scope = cx.compiler.scope(sid);
    let target_ident = super::ident::class_ident(cx.compiler, target);
    let method_ident = super::ident::class_method_ident(name);
    // Full `Params` support (P1): the same binding machinery an instance
    // call gets, through the receiverless `Callee::Bare` shape.
    super::params::emit_call_args_to(
        cx,
        &super::params::Callee::Bare(quote! { #target_ident::#method_ident }),
        name,
        &scope.params,
        args,
        kwargs,
        block,
        block_arg,
        scope.needs_block_param(),
    )
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
    let __bx = cx.box_id;
    let boxed_recv = match infer_class(cx, recv_id) {
        Some(cid) => {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
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
                // `&.` doesn't accept a block yet (spike scope,
                // unrelated to Phase 6 -- narrower than real Ruby, matches
                // this call's existing kwargs restriction). `send_value`
                // (Phase 14.4) handles Object AND builtin receivers
                // uniformly, so the old non-Object panic is gone.
                spinel_rt::send_value_in(#__bx, &__safe_recv, #name_expr, &[#(#arg_exprs),*], None)?
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
fn enforce_visibility(
    cx: &Ctx,
    recv_id: NodeId,
    scope: &crate::compiler::Scope,
    method_name: &str,
) -> Option<TokenStream> {
    match scope.visibility {
        Visibility::Public => {}
        Visibility::Private => {
            if !matches!(cx.compiler.hir[recv_id], HirNode::SelfRef) {
                return Some(emit_visibility_error(cx, recv_id, "private", method_name));
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
                return Some(emit_visibility_error(cx, recv_id, "protected", method_name));
            }
        }
    }
    None
}

/// A visibility violation is RUNTIME behavior in Ruby, not a syntax error:
/// `obj.priv_method` raises `NoMethodError` when it actually runs, so an
/// unreachable bad call stays silent and a `rescue NoMethodError` around a
/// deliberate one works. Message shape verbatim from CRuby
/// ("private method 'x' called for an instance of Foo").
fn emit_visibility_error(
    cx: &Ctx,
    recv_id: NodeId,
    kind: &str,
    method_name: &str,
) -> TokenStream {
    let describe = match infer_any_class(cx, recv_id) {
        Some(cid) => format!("an instance of {}", cx.compiler.class(cid).name),
        None => "an instance of Object".to_string(),
    };
    let msg = format!("{kind} method '{method_name}' called for {describe}");
    quote! {
        return Err(spinel_rt::raise_error("NoMethodError", #msg.to_string()))
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
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    recv_expr: &TokenStream,
    bypass_visibility: bool,
) -> TokenStream {
    let __bx = cx.box_id;
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
        let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
        return quote! { spinel_rt::RubyValue::Bool(!(#recv_boxed).truthy()) };
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
        if let Some(target_name) = super::expr::const_path_of(cx, args[0]) {
            let Some(target) = cx.resolve_class(&target_name) else {
                // A constant bound to a RUNTIME class (`Foo = Class.new`, #97
                // F4): resolve it at runtime and ancestry-check its id.
                let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
                return quote! {
                    {
                        let __rtc = spinel_rt::const_get(0, #target_name).ok_or_else(|| {
                            spinel_rt::raise_error("NameError", format!("uninitialized constant {}", #target_name))
                        })?;
                        match __rtc {
                            spinel_rt::RubyValue::Class(__tid) => spinel_rt::RubyValue::Bool(
                                spinel_rt::is_a((#recv_boxed).class_id(), __tid)),
                            _ => return Err(spinel_rt::raise_error("TypeError", "class or module required".to_string())),
                        }
                    }
                };
            };
            let target_id = target.0;
            // `infer_any_class` (not `infer_class`): a statically-known
            // BUILT-IN-typed receiver (e.g. `TyKind::Int`) must also
            // constant-fold here, not fall through to the runtime branch
            // below, which assumes `#recv_expr` is an actual `RubyValue` it
            // can call `.class_id()` on at runtime -- true either way now
            // (see the universal `RubyValue::class_id`), but the static
            // fold is strictly cheaper and matches every other statically-
            // known-class case in this function.
            return match infer_any_class(cx, recv_id) {
                Some(recv_class) => {
                    let result = cx.compiler.class(recv_class).ancestors.contains(&target);
                    // The `true`/`false` verdict is fully compile-time-known
                    // here, but `recv_expr` itself must still be EVALUATED --
                    // it may be an arbitrary expression with side effects
                    // (`log_and_get(x).is_a?(Integer)`), and real Ruby always
                    // evaluates a method call's receiver regardless of what
                    // the call itself does with it. `let _ = ...;` forces
                    // that evaluation without actually using the (statically
                    // already-known) value, and as a side benefit keeps a
                    // receiver-only-ever-used-via-`is_a?` local from
                    // generating a spurious "value assigned but never read"
                    // warning in the GENERATED program.
                    quote! { { let _ = #recv_expr; spinel_rt::RubyValue::Bool(#result) } }
                }
                None => quote! {
                    spinel_rt::RubyValue::Bool(spinel_rt::is_a(
                        (#recv_expr).class_id(),
                        spinel_rt::ClassId(#target_id),
                    ))
                },
            };
        }
    }

    // `==`/`!=` on an OBJECT receiver with no matching user definition
    // (Phase 16.2): real Ruby's `Object#==` default (reference identity)
    // and its derived `!=`, via `rb_eq` on boxed operands -- which itself
    // dispatches a user `==` when one exists, so `a != b` correctly
    // negates a user-defined `==` even when no `!=` was written.
    // Receivers with a matching own definition fall through to ordinary
    // Path 1 dispatch below.
    if no_kwargs && (name == "==" || name == "!=") && args.len() == 1 && block.is_none() && block_arg.is_none() {
        if let TyKind::Object(cid) = infer(cx, recv_id) {
            if cx.compiler.method_in_chain(cid, name).is_none() {
                let recv_boxed =
                    super::expr::box_if_object_typed(cx, recv_id, recv_expr.clone());
                let arg = emit_expr(cx, args[0]);
                let arg = super::expr::box_if_object_typed(cx, args[0], arg);
                let negate = name == "!=";
                return quote! {
                    spinel_rt::RubyValue::Bool(spinel_rt::rb_eq_checked(&(#recv_boxed), &(#arg))? != #negate)
                };
            }
        }
    }

    // `instance_of?` against a literal class/module constant (Phase 16.1)
    // -- EXACT class identity, not ancestry (`w.instance_of?(Object)` is
    // false for a Widget); same static-fold-else-runtime shape as
    // `is_a?`/`kind_of?` above. A non-constant argument falls through to
    // the dynamic path (`send`/`send_value`'s Class-argument arms).
    if no_kwargs && name == "instance_of?" && args.len() == 1 {
        if let Some(target_name) = super::expr::const_path_of(cx, args[0]) {
            let Some(target) = cx.resolve_class(&target_name) else {
                // A constant bound to a RUNTIME class (`Foo = Class.new`, #97
                // F4): resolve it at runtime and check EXACT class identity.
                let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
                return quote! {
                    {
                        let __rtc = spinel_rt::const_get(0, #target_name).ok_or_else(|| {
                            spinel_rt::raise_error("NameError", format!("uninitialized constant {}", #target_name))
                        })?;
                        match __rtc {
                            spinel_rt::RubyValue::Class(__tid) => spinel_rt::RubyValue::Bool(
                                (#recv_boxed).class_id() == __tid),
                            _ => return Err(spinel_rt::raise_error("TypeError", "class or module required".to_string())),
                        }
                    }
                };
            };
            let target_id = target.0;
            return match infer_any_class(cx, recv_id) {
                Some(recv_class) => {
                    let result = recv_class == target;
                    quote! { { let _ = #recv_expr; spinel_rt::RubyValue::Bool(#result) } }
                }
                None => quote! {
                    spinel_rt::RubyValue::Bool(
                        (#recv_expr).class_id() == spinel_rt::ClassId(#target_id),
                    )
                },
            };
        }
    }

    // `.class` -- universal (Phase 16.1), same override-respecting shape
    // as `freeze`/`dup` below (`class` is an ordinary overridable method
    // in real Ruby). Statically-known receivers fold to a Class literal
    // (still evaluating the receiver for side effects); Poly receivers ask
    // the value at runtime.
    if no_kwargs && name == "class" && args.is_empty() && block.is_none() && block_arg.is_none() {
        // `infer_any_class`, not just `TyKind::Object` (Phase 16.3): a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => any_builtin_overrides(cx, name),
        };
        if !user_defined {
            return match infer_any_class(cx, recv_id) {
                // `Queue` is the one builtin whose runtime value may be a
                // subclass (`SizedQueue`, which types as `Queue` but carries
                // its own class id in the payload), so its `.class` is read
                // at runtime rather than constant-folded.
                Some(cid) if cid != spinel_abi::QUEUE_CLASS => {
                    let id = cid.0;
                    quote! { { let _ = #recv_expr; spinel_rt::RubyValue::Class(spinel_rt::ClassId(#id)) } }
                }
                _ => quote! { spinel_rt::RubyValue::Class((#recv_expr).class_id()) },
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
    if no_kwargs && name == "respond_to?" && (args.len() == 1 || args.len() == 2) {
        let sym_expr = emit_symbol_expr(cx, args[0]);
        // The optional second argument (`include_all`) opts private methods
        // back in -- absent means false, CRuby's default.
        let include_all = match args.get(1) {
            Some(&a) => {
                let e = emit_expr(cx, a);
                quote! { (#e).truthy() }
            }
            None => quote! { false },
        };
        // Box the receiver to a `RubyValue` and route through
        // `responds_to_value`, which also honors a per-object singleton method
        // (#97 F3) -- keyed by object identity, so a class-id-only probe can't
        // see it. The singleton fast path (`is_live()`) means an ordinary
        // program pays only one predictable atomic here.
        let boxed_recv = super::expr::box_if_object_typed(cx, recv_id, recv_expr.clone());
        return quote! {
            spinel_rt::RubyValue::Bool(spinel_rt::responds_to_value(&#boxed_recv, #sym_expr, #include_all))
        };
    }

    // `.nil?` -- universal, same override-respecting shape as
    // `freeze`/`frozen?` below (surfaced as a real need by Phase 13.5's
    // queue-sentinel idiom, `break if q.pop.nil?`, on a Poly receiver). A
    // statically-known Object receiver is never nil (only `RubyValue::Nil`
    // is), but its receiver expression still evaluates for side effects.
    if no_kwargs && name == "nil?" && args.is_empty() {
        // `infer_any_class`, not just `TyKind::Object` (Phase 16.3): a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => any_builtin_overrides(cx, name),
        };
        if !user_defined {
            return match infer(cx, recv_id) {
                TyKind::Object(_) => {
                    quote! { { let _ = #recv_expr; spinel_rt::RubyValue::Bool(false) } }
                }
                _ => quote! { spinel_rt::RubyValue::Bool((#recv_expr).is_nil()) },
            };
        }
    }

    // `.freeze`/`.frozen?` -- universal `Kernel` methods, dispatched over
    // every receiver representation (Phase 13.1). A user class's OWN
    // `def freeze`/`def frozen?` override wins, matching real Ruby (they're
    // ordinary overridable `Kernel` methods) -- checked via the receiver's
    // materialized method table, falling through to ordinary Path 1
    // dispatch when one exists. Two receiver shapes: a statically-known
    // Object receiver is a bare `Arc<Concrete>` (flag reached via the
    // `RubyObject` trait, UFCS-qualified since generated programs don't
    // import the trait by name); everything else -- builtins and Poly -- is
    // already a `RubyValue`, handled by its own universal
    // `freeze_value`/`is_frozen` methods (see their docs for the
    // always-frozen-immediates / flagless-`Proc` tiering).
    if no_kwargs && (name == "freeze" || name == "frozen?") && args.is_empty() {
        // `infer_any_class`, not just `TyKind::Object` (Phase 16.3): a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => any_builtin_overrides(cx, name),
        };
        if !user_defined {
            return match infer(cx, recv_id) {
                TyKind::Object(cid) => {
                    let class_ident = super::ident::class_ident(cx.compiler, cid);
                    if name == "freeze" {
                        // Returns self, boxed -- `freeze`'s result is
                        // Poly-typed downstream (see `types.rs`), so the
                        // uniform `RubyValue` representation is the right
                        // one, exactly like `emit_boxed_new`'s.
                        quote! {
                            {
                                let __r = #recv_expr;
                                spinel_rt::RubyObject::set_frozen(&*__r);
                                spinel_rt::RubyValue::Object(#class_ident::new_handle(__r))
                            }
                        }
                    } else {
                        quote! {
                            spinel_rt::RubyValue::Bool(spinel_rt::RubyObject::is_frozen(&*(#recv_expr)))
                        }
                    }
                }
                _ => {
                    if name == "freeze" {
                        quote! { (#recv_expr).freeze_value() }
                    } else {
                        quote! { spinel_rt::RubyValue::Bool((#recv_expr).is_frozen()) }
                    }
                }
            };
        }
    }

    // `.instance_variable_get(:@x)` / `.instance_variable_set(:@x, v)` /
    // `.instance_variables` -- universal reflection over any receiver's named
    // ivars (an `Object`'s slots, or a class object's own ivars via
    // `civars`). A user override wins, the same fall-through the other
    // universal arms use. The receiver is boxed to a uniform `RubyValue` so
    // one runtime helper serves every representation.
    if no_kwargs
        && block.is_none()
        && block_arg.is_none()
        && matches!(
            (name, args.len()),
            ("instance_variable_get", 1) | ("instance_variable_set", 2) | ("instance_variables", 0)
        )
    {
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => cx.compiler.method_in_chain(cid, name).is_some(),
            None => any_builtin_overrides(cx, name),
        };
        if !user_defined {
            let boxed = match infer_class(cx, recv_id) {
                Some(cid) => {
                    let class_ident = super::ident::class_ident(cx.compiler, cid);
                    quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
                }
                None => quote! { (#recv_expr) },
            };
            return match name {
                "instance_variable_get" => {
                    let a = emit_expr(cx, args[0]);
                    quote! { spinel_rt::instance_variable_get(&#boxed, &(#a))? }
                }
                "instance_variable_set" => {
                    let a = emit_expr(cx, args[0]);
                    let v = emit_expr(cx, args[1]);
                    let v = box_if_object_typed(cx, args[1], v);
                    quote! { spinel_rt::instance_variable_set(&#boxed, &(#a), #v)? }
                }
                _ => quote! { spinel_rt::instance_variables(&#boxed) },
            };
        }
    }

    // `.dup`/`.clone` -- universal `Kernel` methods (Phase 15.2), same
    // override-respecting shape as `freeze`/`frozen?` above (they're
    // ordinary overridable `Kernel` methods in real Ruby). The single
    // semantic difference between the two -- `clone` copies the frozen
    // flag, `dup` doesn't -- is the `copy_frozen` flag threaded to
    // `RubyObject::dup_object` (statically-known Object receiver, a bare
    // `Arc<Concrete>`) or `RubyValue::dup_value` (builtins and Poly).
    // `clone(freeze: false)` keyword form: not supported (kwargs fall
    // through to the ordinary rejection paths).
    if no_kwargs && (name == "dup" || name == "clone") && args.is_empty() {
        // `infer_any_class`, not just `TyKind::Object` (Phase 16.3): a
        // REOPENED builtin's override of this universal method must also
        // fall through -- to the builtin free-function arm further down --
        // instead of taking the universal fast path (a user `String#dup`
        // beats `Kernel#dup`, oracle-verified). A Poly receiver falls
        // through whenever ANY builtin reopen defines this name: only the
        // runtime value knows its class, so the decision defers to
        // `send_value`'s own value-method-first probe.
        //
        // Also fall through when the class has a USER `initialize_copy`
        // (defining class != Object's default no-op): `dup`/`clone` must
        // run that hook, which only the runtime `Kernel#dup`/`#clone` path
        // does. Object's own default hook changes nothing, so a class without
        // an override keeps the unboxed fast path.
        let user_defined = match infer_any_class(cx, recv_id) {
            Some(cid) => {
                cx.compiler.method_in_chain(cid, name).is_some()
                    || matches!(
                        cx.compiler.method_in_chain(cid, "initialize_copy"),
                        Some((defining, _)) if defining != crate::compiler::OBJECT_CLASS
                    )
            }
            None => any_builtin_overrides(cx, name),
        };
        if !user_defined {
            let copy_frozen = name == "clone";
            return match infer(cx, recv_id) {
                TyKind::Object(_) => {
                    // Boxed result, like `freeze`'s -- the copy's static
                    // class is knowable, but `dup` results flow into
                    // Poly-typed positions downstream (see `types.rs`).
                    quote! {
                        spinel_rt::RubyValue::Object(
                            spinel_rt::RubyObject::dup_object(&*(#recv_expr), #copy_frozen),
                        )
                    }
                }
                _ => quote! { (#recv_expr).dup_value(#copy_frozen) },
            };
        }
    }

    // `Fiber#resume` / `Fiber#alive?` on a statically-known Fiber receiver
    // (Phase 13.3). `resume`'s error outcomes each become their own
    // CRuby-verbatim `FiberError`; an uncaught Ruby signal from inside the
    // fiber's body re-raises HERE, at the resumer -- exactly CRuby's
    // `cont.c:2914` behavior. A Poly-typed receiver falls through to the
    // generic Poly-`send` fallback below (which can't reach a Fiber -- the
    // same documented builtin-receiver `send` gap every other builtin has).
    if no_kwargs && infer(cx, recv_id) == TyKind::Fiber && block.is_none() && block_arg.is_none() {
        if name == "resume" {
            let arg_exprs: Vec<TokenStream> = args
                .iter()
                .map(|&a| {
                    let e = emit_expr(cx, a);
                    super::expr::box_if_object_typed(cx, a, e)
                })
                .collect();
            let dead = emit_fiber_error(cx, "attempt to resume a terminated fiber");
            let double = emit_fiber_error(cx, "attempt to resume the current fiber (double resume)");
            let cross = emit_fiber_error(cx, "fiber called across threads");
            return quote! {
                match spinel_rt::fiber_resume(
                    &(#recv_expr).as_fiber_unchecked(),
                    vec![#(#arg_exprs),*],
                ) {
                    spinel_rt::FiberResume::Value(__v) => __v,
                    spinel_rt::FiberResume::RubyError(__sig) => return Err(__sig),
                    spinel_rt::FiberResume::Dead => {
                        return Err(spinel_rt::Signal::Raise(#dead))
                    }
                    spinel_rt::FiberResume::DoubleResume => {
                        return Err(spinel_rt::Signal::Raise(#double))
                    }
                    spinel_rt::FiberResume::CrossThread => {
                        return Err(spinel_rt::Signal::Raise(#cross))
                    }
                }
            };
        }
        if name == "alive?" && args.is_empty() {
            return quote! {
                spinel_rt::RubyValue::Bool(spinel_rt::fiber_alive(
                    &(#recv_expr).as_fiber_unchecked(),
                ))
            };
        }
    }

    // `Thread#join`/`#value` (Phase 13.5): both wait via
    // `spinel_rt::thread_outcome` (a real may yield point); an `Err` is the
    // thread's own uncaught signal, re-raised HERE in the joiner -- CRuby's
    // stored-exception semantics (`thread.c:1195`). `join` returns the
    // THREAD itself, `value` the block's result.
    if no_kwargs && infer(cx, recv_id) == TyKind::Thread && args.is_empty() && block.is_none() {
        if name == "join" {
            return quote! {
                {
                    let __t = (#recv_expr).as_thread_unchecked();
                    match spinel_rt::thread_outcome(&__t) {
                        Ok(_) => spinel_rt::RubyValue::Thread(__t),
                        Err(__sig) => return Err(__sig),
                    }
                }
            };
        }
        if name == "value" {
            return quote! {
                match spinel_rt::thread_outcome(&(#recv_expr).as_thread_unchecked()) {
                    Ok(__v) => __v,
                    Err(__sig) => return Err(__sig),
                }
            };
        }
    }

    // Ruby `Mutex` (Phase 13.5) -- CRuby-verbatim ThreadError messages come
    // back from the runtime (`Err(&str)`), boxed into real exceptions here.
    // `lock`/`unlock` both return self, matching CRuby.
    if no_kwargs && infer(cx, recv_id) == TyKind::Mutex {
        let thread_error = super::expr::emit_boxed_new(
            cx,
            "ThreadError",
            vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(__msg.to_string())) }],
        );
        match (name, args.len(), block) {
            ("lock", 0, None) => {
                return quote! {
                    {
                        let __m = (#recv_expr).as_mutex_unchecked();
                        match spinel_rt::mutex_lock(&__m) {
                            Ok(()) => spinel_rt::RubyValue::Mutex(__m),
                            Err(__msg) => return Err(spinel_rt::Signal::Raise(#thread_error)),
                        }
                    }
                };
            }
            ("unlock", 0, None) => {
                return quote! {
                    {
                        let __m = (#recv_expr).as_mutex_unchecked();
                        match spinel_rt::mutex_unlock(&__m) {
                            Ok(()) => spinel_rt::RubyValue::Mutex(__m),
                            Err(__msg) => return Err(spinel_rt::Signal::Raise(#thread_error)),
                        }
                    }
                };
            }
            ("locked?", 0, None) => {
                return quote! {
                    spinel_rt::RubyValue::Bool(spinel_rt::mutex_locked(&(#recv_expr).as_mutex_unchecked()))
                };
            }
            ("owned?", 0, None) => {
                return quote! {
                    spinel_rt::RubyValue::Bool(spinel_rt::mutex_owned(&(#recv_expr).as_mutex_unchecked()))
                };
            }
            // `try_lock` -- acquire without blocking; true iff it was free.
            ("try_lock", 0, None) => {
                return quote! {
                    spinel_rt::RubyValue::Bool(spinel_rt::mutex_try_lock(&(#recv_expr).as_mutex_unchecked()))
                };
            }
            // `synchronize { }`: lock, run the block (an ordinary escaping
            // Proc), ALWAYS unlock -- including on a signal (an exception/
            // `break` inside the block must release the lock on its way
            // out), then re-propagate. `catch_break` first: `break` inside
            // `synchronize` exits it with the break's value, real Ruby
            // behavior.
            ("synchronize", 0, Some(block_id)) => {
                let proc = emit_proc_value(cx, block_id);
                let lock_err = super::expr::emit_boxed_new(
                    cx,
                    "ThreadError",
                    vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(__msg.to_string())) }],
                );
                return quote! {
                    {
                        let __m = (#recv_expr).as_mutex_unchecked();
                        if let Err(__msg) = spinel_rt::mutex_lock(&__m) {
                            return Err(spinel_rt::Signal::Raise(#lock_err));
                        }
                        let __blk = (#proc).as_proc_unchecked();
                        let __r = spinel_rt::catch_break(__blk.call(&[]));
                        let _ = spinel_rt::mutex_unlock(&__m);
                        match __r {
                            Ok(__v) => __v,
                            Err(__sig) => return Err(__sig),
                        }
                    }
                };
            }
            _ => {}
        }
    }

    // `Queue` (Phase 13.5): `pop` blocks coroutine-yieldingly; a closed
    // empty queue pops nil; push to a closed queue raises ClosedQueueError
    // -- all CRuby `thread_sync.c` semantics, verified in the plan addendum.
    if no_kwargs && infer(cx, recv_id) == TyKind::Queue && block.is_none() {
        match (name, args.len()) {
            ("push" | "<<" | "enq", 1) => {
                let v = emit_expr(cx, args[0]);
                let v = super::expr::box_if_object_typed(cx, args[0], v);
                let closed_err = super::expr::emit_boxed_new(
                    cx,
                    "ClosedQueueError",
                    vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new("queue closed".to_string())) }],
                );
                return quote! {
                    {
                        let __q = (#recv_expr).as_queue_unchecked();
                        let __v = #v;
                        match spinel_rt::queue_push(&__q, __v) {
                            Ok(()) => spinel_rt::RubyValue::Queue(__q),
                            Err(_) => return Err(spinel_rt::Signal::Raise(#closed_err)),
                        }
                    }
                };
            }
            ("pop" | "shift" | "deq", 0) => {
                return quote! { spinel_rt::queue_pop(&(#recv_expr).as_queue_unchecked()) };
            }
            ("close", 0) => {
                return quote! {
                    {
                        let __q = (#recv_expr).as_queue_unchecked();
                        spinel_rt::queue_close(&__q);
                        spinel_rt::RubyValue::Queue(__q)
                    }
                };
            }
            ("closed?", 0) => {
                return quote! {
                    spinel_rt::RubyValue::Bool(spinel_rt::queue_closed(&(#recv_expr).as_queue_unchecked()))
                };
            }
            ("length" | "size", 0) => {
                return quote! {
                    spinel_rt::RubyValue::Int(spinel_rt::queue_len(&(#recv_expr).as_queue_unchecked()))
                };
            }
            ("empty?", 0) => {
                return quote! {
                    spinel_rt::RubyValue::Bool(spinel_rt::queue_len(&(#recv_expr).as_queue_unchecked()) == 0)
                };
            }
            // `SizedQueue#max` -- the bound (nil on an unbounded `Queue`).
            ("max", 0) => {
                return quote! {
                    match spinel_rt::queue_max(&(#recv_expr).as_queue_unchecked()) {
                        Some(__n) => spinel_rt::RubyValue::Int(__n),
                        None => spinel_rt::RubyValue::Nil,
                    }
                };
            }
            _ => {}
        }
    }

    // `Ractor` instance methods (Phase 13.8). NOTE: on a Ractor receiver,
    // `send` is the MESSAGE-passing method (as in real Ruby, where
    // `Ractor#send` shadows `Object#send`) -- this arm must stay ahead of
    // the generic dynamic-dispatch `send` handling further down.
    // `value`/`join` mirror Thread's (an uncaught signal re-raises in the
    // caller; join returns the Ractor itself).
    if no_kwargs && infer(cx, recv_id) == TyKind::Ractor && block.is_none() && block_arg.is_none() {
        let ractor_error = emit_ractor_error(cx);
        match (name, args.len()) {
            ("send", 1) => {
                let v = emit_expr(cx, args[0]);
                let v = super::expr::box_if_object_typed(cx, args[0], v);
                return quote! {
                    {
                        let __r = (#recv_expr).as_ractor_unchecked();
                        match spinel_rt::ractor_send(&__r, &(#v)) {
                            Ok(()) => spinel_rt::RubyValue::Ractor(__r),
                            Err(__msg) => return Err(spinel_rt::Signal::Raise(#ractor_error)),
                        }
                    }
                };
            }
            ("value", 0) => {
                return quote! {
                    match spinel_rt::ractor_outcome(&(#recv_expr).as_ractor_unchecked()) {
                        Ok(__v) => __v,
                        Err(__sig) => return Err(__sig),
                    }
                };
            }
            ("join", 0) => {
                return quote! {
                    {
                        let __r = (#recv_expr).as_ractor_unchecked();
                        match spinel_rt::ractor_outcome(&__r) {
                            Ok(_) => spinel_rt::RubyValue::Ractor(__r),
                            Err(__sig) => return Err(__sig),
                        }
                    }
                };
            }
            _ => {}
        }
    }

    // Native `Int` arithmetic/comparison/bitwise ops: both operands must be
    // statically known `Int` (see `INT_BINARY_OPS`'s docs above). The
    // operands stay boxed `&RubyValue`s since the bignum migration -- the
    // `int_*` family's inline small-small fast half keeps the hot path
    // cheap, and overflow promotes instead of panicking.
    if no_kwargs && args.len() == 1 {
        if let Some(&(_, rt_fn, kind)) = INT_BINARY_OPS.iter().find(|(op, _, _)| *op == name) {
            let recv_ty = infer(cx, recv_id);
            let arg_ty = infer(cx, args[0]);
            if recv_ty == TyKind::Int && arg_ty == TyKind::Int {
                let arg_expr = emit_expr(cx, args[0]);
                let func = format_ident!("{rt_fn}");
                return match kind {
                    IntOpKind::Value => {
                        quote! { spinel_rt::#func(&(#recv_expr), &(#arg_expr)) }
                    }
                    IntOpKind::Fallible => {
                        quote! { spinel_rt::#func(&(#recv_expr), &(#arg_expr))? }
                    }
                    IntOpKind::DivMod => {
                        emit_int_div_or_mod_checked(cx, rt_fn, recv_expr.clone(), arg_expr)
                    }
                    IntOpKind::Bool => quote! {
                        spinel_rt::RubyValue::Bool(spinel_rt::#func(&(#recv_expr), &(#arg_expr)))
                    },
                    IntOpKind::Cmp => quote! {
                        spinel_rt::RubyValue::Int(spinel_rt::#func(&(#recv_expr), &(#arg_expr)))
                    },
                };
            }
        }
    }

    // Native `Int` unary operators (`-@`/`+@`/`~`), same eligibility rule
    // (all three return `RubyValue` -- negation can promote `-i64::MIN`).
    if no_kwargs && args.is_empty() {
        if let Some(&(_, rt_fn)) = INT_UNARY_OPS.iter().find(|(op, _)| *op == name) {
            if infer(cx, recv_id) == TyKind::Int {
                let func = format_ident!("{rt_fn}");
                return quote! { spinel_rt::#func(&(#recv_expr)) };
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
                _ => quote! { spinel_rt::num_to_f64_unchecked(&(#recv_expr)) },
            };
            let arg_f = match arg_ty {
                TyKind::Float => quote! { (#arg_expr).as_float_unchecked() },
                _ => quote! { spinel_rt::num_to_f64_unchecked(&(#arg_expr)) },
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

    // A REOPENED builtin's method on a statically-typed builtin receiver
    // (Phase 16.3): a direct call to the generated free function (`__bm_
    // String::length(recv, ...)`), checked BEFORE the collection/Proc/
    // Regexp fast paths below because a user redefinition must OVERRIDE the
    // native behavior -- real Ruby's rule, oracle-verified (`class String;
    // def length; 42; end` wins at every call site). The receiver
    // expression is already a boxed `RubyValue` for every builtin TyKind
    // (only `Object` receivers are unboxed `Arc<Concrete>`s, and those
    // never reach this arm).
    if let Some(cid) = infer_any_class(cx, recv_id) {
        if cx.compiler.class(cid).is_builtin {
            if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
                let scope = cx.compiler.scope(sid);
                if !bypass_visibility {
                    if let Some(err) = enforce_visibility(cx, recv_id, scope, name) {
                        return err;
                    }
                }
                let mod_ident = super::ident::class_ident(cx.compiler, cid);
                let method_ident = safe_ident(name);
                return super::params::emit_call_args_to(
                    cx,
                    &super::params::Callee::FreeFn {
                        path: quote! { #mod_ident::#method_ident },
                        recv: recv_expr.clone(),
                    },
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
    }

    if no_kwargs {
        if let Some(tokens) = try_collection_dispatch(cx, recv_id, name, args, recv_expr) {
            return tokens;
        }
        if let Some(tokens) = try_proc_dispatch(cx, recv_id, name, args, recv_expr) {
            return tokens;
        }
        if let Some(tokens) = try_regexp_dispatch(cx, recv_id, name, args, block, recv_expr) {
            return tokens;
        }
    }

    // Known-shape block inlining (mirrors `emit_block_value_into`/`.times`,
    // `codegen_iter.c:1281`): the block body is spliced into a native,
    // labeled Rust loop -- no closure or Proc object is allocated. Shares
    // `codegen::loops`' redo-wrapping machinery with `while`/`until`/`loop`/
    // `for`, so `break`/`next`/`redo` inside a `.times` block work exactly
    // the same way.
    // Blockless `5.times` falls through to the dynamic row, which answers
    // an Enumerator (Phase 17.2) -- the inline splice below is only for the
    // block form.
    if let Some(block_id) = block.filter(|_| is_times_fast_path(cx.compiler, Some(recv_id), name, no_kwargs)) {
        if let HirNode::IntegerLit(n) = &cx.compiler.hir[recv_id] {
            let n = *n;
            let HirNode::Block { params, body } = &cx.compiler.hir[block_id] else {
                panic!("`times`'s argument must be a block");
            };
            let outer = super::loops::fresh_label(cx, "times");
            let redo = super::loops::fresh_label(cx, "times_body");
            let loop_cx = cx.in_loop(redo.clone(), outer.clone());
            let bind = params.required.first().map(|p| {
                let ident = safe_ident(p);
                // `mut`: a block param is an ordinary reassignable local.
                quote! { #[allow(unused_mut)] let mut #ident = spinel_rt::RubyValue::Int(__i); }
            });
            // `3.times { |i; n| ... }` -- block-locals get a fresh `nil` per
            // iteration here, exactly as `emit_proc_param_bindings` does for
            // a real Proc. Inside the loop, not outside: the reset-every-
            // invocation semantics is the whole point of the declaration.
            let block_locals = params.block_locals.iter().map(|name| {
                let ident = safe_ident(name);
                quote! {
                    #[allow(unused_variables, unused_mut)]
                    let mut #ident: spinel_rt::RubyValue = spinel_rt::RubyValue::Nil;
                }
            });
            let inner = super::loops::emit_redo_wrapped_body(&loop_cx, body, &redo);
            return quote! {
                {
                    let mut __i: i64 = 0;
                    #outer: loop {
                        if __i >= #n { break #outer spinel_rt::RubyValue::Nil; }
                        #bind
                        #(#block_locals)*
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
        // Keyword args ride as one trailing Hash (the G2 convention) --
        // the callee's trampoline pops and binds it.
        let kw_hash = emit_kwargs_trailing_hash(cx, kwargs);
        // A statically-known class needs boxing into an `RObj` handle first
        // (`recv_expr` is an unboxed `Arc<Concrete>` there); a `Poly` receiver
        // is ALREADY a `RubyValue::Object(...)` at runtime (e.g. a `rescue`
        // clause's exception binding -- see `codegen::exceptions`'s docs for
        // why that's never narrowed to a concrete class), so it just needs
        // unwrapping, not a fabricated `new_handle` call (which would need a
        // compile-time class name we don't have here).
        let recv_obj_expr = match recv_class {
            Some(cid) => {
                let class_ident = super::ident::class_ident(cx.compiler, cid);
                quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)) }
            }
            // Any non-Object receiver (a builtin collection, or genuinely
            // Poly) dispatches through `send_value`'s builtin table --
            // `[1,2].send(:length)` works now, not just Object receivers.
            None => quote! { (#recv_expr) },
        };
        let name_expr = emit_symbol_expr(cx, args[0]);
        let rest_args = args[1..].iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        let kw_hash = kw_hash.into_iter();
        let block_value = emit_block_option(cx, block, block_arg);
        let dyn_call = quote! {
            spinel_rt::send_value_in(#__bx, &#recv_obj_expr, #name_expr, &[#(#rest_args,)* #(#kw_hash,)*], #block_value)
        };
        return wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
    }

    // Ordinary call with a statically known receiver class: direct call
    // (Path 1). This is the common case -- `method_in_chain` mirrors
    // `comp_method_in_chain` exactly (compiler.c:404).
    if let Some(cid) = recv_class {
        if let Some((_, sid)) = cx.compiler.method_in_chain(cid, name) {
            let scope = cx.compiler.scope(sid);
            if !bypass_visibility {
                if let Some(err) = enforce_visibility(cx, recv_id, scope, name) {
                    return err;
                }
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
            let is_numeric_op = int_entry.is_some()
                || FLOAT_BINARY_OPS.iter().any(|(op, _, _)| *op == name)
                || name == "<=>";
            if is_numeric_op {
                // An Object-typed argument is an unboxed `Arc<Concrete>` --
                // box it so the match scrutinee (and the `send_value`
                // fallback's `(*__dyn_arg).clone()`) is a `RubyValue`
                // (`junk << Trash.new(j)` on a Poly receiver hits this).
                let arg_expr = {
                    let e = emit_expr(cx, args[0]);
                    box_if_object_typed(cx, args[0], e)
                };
                // One inline Int-Int fast arm (the hot `def add(a, b); a +
                // b; end` case); EVERY other operand shape -- Float pairs,
                // mixed promotion, Bignum/Rational/Complex lanes, user
                // operator methods, builtin rows -- resolves through
                // `send_value`'s MRO walk, whose Integer/Float operator
                // rows drive the same one tower matrix (Phase 17.1). The
                // old hand-inlined Float/mixed arms are gone: they
                // duplicated the promotion rules and knew nothing of the
                // new lanes.
                let int_arm = int_entry.map(|&(_, rt_fn, kind)| {
                    let func = format_ident!("{rt_fn}");
                    let call = match kind {
                        IntOpKind::Value => quote! { spinel_rt::#func(__r, __a) },
                        IntOpKind::Fallible => quote! { spinel_rt::#func(__r, __a)? },
                        IntOpKind::DivMod => emit_int_div_or_mod_checked(
                            cx,
                            rt_fn,
                            quote! { __r },
                            quote! { (*__a).clone() },
                        ),
                        IntOpKind::Bool => quote! {
                            spinel_rt::RubyValue::Bool(spinel_rt::#func(__r, __a))
                        },
                        IntOpKind::Cmp => quote! {
                            spinel_rt::RubyValue::Int(spinel_rt::#func(__r, __a))
                        },
                    };
                    quote! {
                        (
                            __r @ spinel_rt::RubyValue::Int(_),
                            __a @ spinel_rt::RubyValue::Int(_),
                        ) => #call,
                    }
                });
                return quote! {
                    match (&(#recv_expr), &(#arg_expr)) {
                        #int_arm
                        (__dyn_recv, __dyn_arg) => spinel_rt::send_value_in(#__bx, 
                            __dyn_recv,
                            spinel_rt::Symbol::intern(#name),
                            &[(*__dyn_arg).clone()],
                            None,
                        )?,
                    }
                };
            }
        }
        if args.is_empty() {
            let int_entry = INT_UNARY_OPS.iter().find(|(op, _)| *op == name);
            let float_entry = FLOAT_UNARY_OPS.iter().find(|(op, _)| *op == name);
            if int_entry.is_some() || float_entry.is_some() {
                // Same shape as the binary fallback: inline Int fast arm,
                // everything else through the MRO walk's unary rows.
                let int_arm = int_entry.map(|&(_, rt_fn)| {
                    let func = format_ident!("{rt_fn}");
                    quote! {
                        __r @ spinel_rt::RubyValue::Int(_) => spinel_rt::#func(__r),
                    }
                });
                return quote! {
                    match &(#recv_expr) {
                        #int_arm
                        __dyn_recv => spinel_rt::send_value_in(#__bx, 
                            __dyn_recv,
                            spinel_rt::Symbol::intern(#name),
                            &[],
                            None,
                        )?,
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
    // one of those falls through to `send_value`'s DYNAMIC dispatch (Phase
    // 14.4): its builtin method table handles the supported operations
    // (`[1,2,3].each { ... }` works now), and anything else raises a real,
    // rescuable `NoMethodError` at runtime with the builtin class's actual
    // name -- Ruby's own behavior, replacing the old compile-time
    // "unsupported call" panic for statically-collection-typed receivers.
    // The kinds listed are exactly the ones whose static repr is already a
    // boxed `RubyValue` (see `types.rs`'s module docs: only `Int` -- and
    // `Object`, as `Arc<Concrete>` -- get unboxed native representations,
    // so those two MUST NOT route through a `&RubyValue` call). `kwargs`
    // has no Path 2 channel at all -- raise the same clear error the
    // `send`/`public_send` case above does, rather than silently dropping
    // it and dispatching without it.
    if matches!(
        infer(cx, recv_id),
        TyKind::Poly
            | TyKind::Str
            | TyKind::Array
            | TyKind::Hash
            | TyKind::Range
            | TyKind::Regexp
            | TyKind::MatchData
            // Since Phase 17.1's MRO-walking method tables, EVERY value
            // kind falls through -- `5.itself`/`"a".between?(...)` resolve
            // Kernel/Comparable rows down the receiver's real ancestor
            // chain at runtime, and a genuinely unknown name raises real
            // Ruby's NoMethodError instead of the old compile-time panic.
            | TyKind::Int
            | TyKind::Float
            | TyKind::Symbol
            | TyKind::Proc
            | TyKind::Fiber
            | TyKind::Thread
            | TyKind::Mutex
            | TyKind::Queue
            | TyKind::Ractor
            // A class VALUE receiver (Phase 16.1) whose method isn't a
            // statically-defined class method (that case returned above):
            // `send_value`'s Class arm handles the reflection set
            // (`name`/`ancestors`/`==`/the registry constructor), and an
            // unknown name is a real runtime NoMethodError "for class X".
            | TyKind::ClassObj(_)
    ) {
        let name_expr = quote! { spinel_rt::Symbol::intern(#name) };
        let arg_exprs = args.iter().map(|&a| {
            let e = emit_expr(cx, a);
            box_if_object_typed(cx, a, e)
        });
        // Keyword args ride as one trailing Hash (the G2 convention).
        let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
        let block_value = emit_block_option(cx, block, block_arg);
        let dyn_call = quote! {
            spinel_rt::send_value_in(#__bx,
                &(#recv_expr),
                #name_expr,
                &[#(#arg_exprs,)* #(#kw_hash,)*],
                #block_value,
            )
        };
        return wrap_dynamic_result(block.is_some() || block_arg.is_some(), dyn_call);
    }

    // A statically-known class that `include Enumerable` (the RUST-backed
    // builtin module, Phase 14.4 rev.2) with no own/materialized definition
    // for this name: dispatch dynamically -- `send`'s Enumerable fallback
    // reaches `spinel_rt::enumerable`, which drives this receiver's own
    // `each`. Deliberately NO compile-time list of Enumerable method names
    // here: the runtime match in `spinel_rt::enumerable::enumerable_send`
    // is the single source of truth, and a name it doesn't recognize
    // raises a real, rescuable `NoMethodError` at runtime -- exactly real
    // Ruby's behavior, and the same compile-time-strictness-for-runtime-
    // faithfulness trade this phase already made for builtin receivers
    // (see the widened dynamic fallback above). The cost is that a TYPO'd
    // method on an Enumerable-including class surfaces at runtime instead
    // of compile time -- scoped to exactly the classes that opted into an
    // open-ended mixin.
    if let Some(cid) = recv_class {
        if cx
            .compiler
            .class(cid)
            .ancestors
            .contains(&crate::compiler::ENUMERABLE_CLASS)
            || cx
                .compiler
                .class(cid)
                .ancestors
                .contains(&crate::compiler::COMPARABLE_CLASS)
        {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            let name_expr = quote! { spinel_rt::Symbol::intern(#name) };
            let arg_exprs = args.iter().map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            });
            // Keyword args ride as one trailing Hash (the G2 convention).
            let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
            let block_value = emit_block_option(cx, block, block_arg);
            return quote! {
                spinel_rt::catch_break(spinel_rt::send_value_in(#__bx,
                    &spinel_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)),
                    #name_expr,
                    &[#(#arg_exprs,)* #(#kw_hash,)*],
                    #block_value,
                ))?
            };
        }
    }

    // No static form matched. Dispatch through the runtime rather than
    // rejecting: the receiver's class may still provide the method via an
    // ancestor the static tables don't mirror (every user object inherits
    // Kernel/Object -- `method(:x)`, `tap`, `frozen?`,
    // `instance_variable_get`, ...), and a name that genuinely resolves
    // nowhere raises NoMethodError at the moment the call runs, which is
    // what real Ruby does anyway.
    let recv_boxed = box_if_object_typed(cx, recv_id, recv_expr.clone());
    let name_expr = quote! { spinel_rt::Symbol::intern(#name) };
    let arg_exprs = args.iter().map(|&a| {
        let e = emit_expr(cx, a);
        box_if_object_typed(cx, a, e)
    });
    // Keyword args ride as one trailing Hash (the G2 convention).
    let kw_hash = emit_kwargs_trailing_hash(cx, kwargs).into_iter();
    let block_value = emit_block_option(cx, block, block_arg);
    quote! {
        spinel_rt::catch_break(spinel_rt::send_value_in(#__bx,
            &#recv_boxed,
            #name_expr,
            &[#(#arg_exprs,)* #(#kw_hash,)*],
            #block_value,
        ))?
    }
}
