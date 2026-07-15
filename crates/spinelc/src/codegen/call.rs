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
    recv_i64: TokenStream,
    arg_i64: TokenStream,
) -> TokenStream {
    if rt_fn != "int_div" && rt_fn != "int_mod" {
        let func = format_ident!("{rt_fn}");
        return quote! { spinel_rt::#func(#recv_i64, #arg_i64) };
    }
    let func = format_ident!("{rt_fn}");
    let err = super::expr::emit_boxed_new(
        cx,
        "ZeroDivisionError",
        vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new("divided by 0".to_string())) }],
    );
    quote! {
        {
            let __divisor = #arg_i64;
            if __divisor == 0 {
                return Err(spinel_rt::Signal::Raise(#err));
            }
            spinel_rt::#func(#recv_i64, __divisor)
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
            quote! { spinel_rt::hash_get(&(#recv_expr).as_hash_unchecked(), &(#key)) }
        }
        (TyKind::Hash, "[]=", 2) => {
            let key = emit_expr(cx, args[0]);
            let val = emit_expr(cx, args[1]);
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
        (TyKind::Str, "[]", 1) => {
            let idx = emit_expr(cx, args[0]);
            quote! { spinel_rt::string_get(&(#recv_expr).as_str_unchecked(), (#idx).as_int_unchecked()) }
        }
        (TyKind::Str, "[]=", 2) => {
            let idx = emit_expr(cx, args[0]);
            let val = emit_expr(cx, args[1]);
            let frozen_error =
                emit_frozen_error(cx, "String", quote! { spinel_rt::RubyValue::Str(__recv.clone()) });
            quote! {
                {
                    let __recv = (#recv_expr).as_str_unchecked();
                    let __idx = (#idx).as_int_unchecked();
                    let __val = #val;
                    if __recv.is_frozen() {
                        return Err(spinel_rt::Signal::Raise(#frozen_error));
                    }
                    spinel_rt::string_set(&__recv, __idx, &__val)
                }
            }
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
        Some(quote! { let #var = (#e).as_str_unchecked(); let #var = #var.lock(); })
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
        let pattern_is_str = infer(cx, args[0]) == TyKind::Str;
        let is_regexp_shaped_method = matches!(name, "match" | "match?" | "=~" | "!~" | "scan" | "split" | "sub" | "gsub");

        if is_regexp_shaped_method && pattern_is_str {
            panic!(
                "a String pattern argument to String#{name} isn't supported yet (spike scope) -- pass a Regexp literal instead"
            );
        }

        if pattern_is_regexp {
            let re_expr = emit_expr(cx, args[0]);
            let haystack_guard = quote! { let __h = (#recv_expr).as_str_unchecked(); let __h = __h.lock(); };
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
                        { #haystack_guard spinel_rt::regexp_split(&(#re_expr).as_regexp_unchecked(), &__h) }
                    });
                }
                ("sub", 2) if infer(cx, args[1]) == TyKind::Str => {
                    let repl_expr = emit_expr(cx, args[1]);
                    return Some(quote! {
                        { #haystack_guard let __r = (#repl_expr).as_str_unchecked(); let __r = __r.lock();
                          spinel_rt::regexp_sub(&(#re_expr).as_regexp_unchecked(), &__h, &__r) }
                    });
                }
                ("gsub", 2) if infer(cx, args[1]) == TyKind::Str => {
                    let repl_expr = emit_expr(cx, args[1]);
                    return Some(quote! {
                        { #haystack_guard let __r = (#repl_expr).as_str_unchecked(); let __r = __r.lock();
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
        .resolve_class(class_name)
        .unwrap_or_else(|| panic!("unknown class `{class_name}`"));
    if cx.compiler.class(cid).is_builtin {
        panic!(
            "`{class_name}.new` isn't supported yet -- built-in types are constructed via their own literal syntax, not `.new` (spike scope)"
        );
    }
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
    let ctor = quote! { std::sync::Arc::new(#class_ident { #fields }) };

    match cx.compiler.method_in_chain(cid, "initialize") {
        Some((_, sid)) => {
            let params = &cx.compiler.scope(sid).params;
            // Bind `initialize`'s REQUIRED + OPTIONAL parameters the way a
            // Path 1 call does (Phase 14.4, closing the "no
            // optional-argument Some-wrapping smarts" gap that previously
            // made `Set.new` vs `Set.new(arr)` on one `initialize(items =
            // nil)` a rustc arity error): required args 1:1, each optional
            // slot `Some(expr)` when provided else `None` (the callee's own
            // prologue lazily evaluates the default). Splat/post/keyword
            // params on `initialize` remain out of scope, matching this
            // function's original posture.
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
    let defining_captures = super::captures::collect_escaping_captures(
        cx.compiler,
        &defining_scope.body,
        &defining_scope.params,
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
        // A no-receiver call from WITHIN another CLASS method's own body
        // (`current_class` is `None` there -- no concrete `self` receiver
        // exists, see `codegen::mod::emit_class_method_fn`'s docs) to a
        // SIBLING class method on the same class/module (`def self.a;
        // b; end` calling `def self.b`, or the equivalent inside `class <<
        // self`) -- resolved the same way `ClassName.foo(...)` is
        // (`Compiler::class_method_in_chain`), dispatched as a direct
        // associated-function call (`Target::b(...)`), since a class method
        // has no dynamic dispatch table to fall back through either (see
        // the plan's Part 6 scope-cut on first-class `Class`/`Module`
        // values). A prior version of this function had no such branch at
        // all, meaning `class << self` blocks whose methods called each
        // other implicitly (the common, idiomatic reason to write several
        // class methods together) always panicked -- found via this
        // phase's own testing, fixed here rather than left as a silent gap
        // in a feature this same session just shipped.
        if cx.current_class.is_none() {
            if let Some(defining) = cx.defining_class {
                if cx.compiler.class_method_in_chain(defining, name).is_some() {
                    return emit_class_method_call_on(cx, defining, name, args, kwargs);
                }
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
                            Some(__v) => __v,
                            None => return Err(spinel_rt::Signal::Raise(#root_error)),
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
                    let block_caps = super::captures::block_captures(cx.compiler, params, body);
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

    if let HirNode::ClassRef(target_name) = &cx.compiler.hir[recv_id] {
        if cx.resolve_class(target_name).is_some() {
            if safe || block.is_some() || block_arg.is_some() {
                panic!("safe-navigation or a block on a class-method call isn't supported yet (spike scope)");
            }
            return emit_class_method_call(cx, target_name, name, args, kwargs);
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
            // Same "only an ACTUALLY-registered class/module" guard as
            // `emit_call`'s own `ClassRef` interception -- see its docs.
            if let HirNode::ClassRef(target_name) = &cx.compiler.hir[recv_id] {
                if cx.resolve_class(target_name).is_some() {
                    panic!("a splat argument on a class-method call isn't supported yet (spike scope)");
                }
            }
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
        None => {
            // `puts`/other no-receiver builtins aren't reachable through
            // `spinel_rt::send` at all (they have no `ClassRegistry` entry) --
            // a clean rejection here beats generating code that only fails
            // at RUNTIME with a confusing "no such method".
            let Some(cid) = cx.current_class.filter(|&cid| cx.compiler.method_in_chain(cid, name).is_some()) else {
                panic!("unsupported implicit-self splat call `{name}` (spike scope, or no such method is defined on the current class)");
            };
            let slf = &cx.self_ident;
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#slf.clone())) }
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
            spinel_rt::catch_break(spinel_rt::send_value(&#recv_obj_expr, #name_expr, &__args, #block_value))?
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
        .resolve_class(target_name)
        .unwrap_or_else(|| panic!("unknown class/module `{target_name}`"));
    emit_class_method_call_on(cx, target, name, args, kwargs)
}

/// The shared core behind `emit_class_method_call` (`ClassName.foo(...)`,
/// `target` resolved from a literal constant name) AND an implicit-self call
/// made FROM WITHIN another class method's own body (`emit_call`'s
/// no-receiver branch, `target` already known as `cx.defining_class` --
/// no name to look up at all). Same "plain required parameters only, no
/// keyword args, no block" restriction either way (see
/// `codegen::mod::emit_class_method_fn`'s matching rejection).
fn emit_class_method_call_on(
    cx: &Ctx,
    target: crate::compiler::ClassId,
    name: &str,
    args: &[NodeId],
    kwargs: &[HashPair],
) -> TokenStream {
    let target_name = &cx.compiler.class(target).name;
    // A `native_func` module function (Phase 14.3): a direct call into the
    // backing Rust crate's free function -- `spinelc_base64::encode64(arg)?`
    // -- linked only when the declaring package was `require`d (see
    // `Hir::native_deps`). Same Path-1-only posture as every other module
    // function; only the ARITY is checked here (the native fn itself
    // runtime-checks its `RubyValue` argument kinds).
    {
        let ci = cx.compiler.class(target);
        if let Some(&(_, arity)) = ci.native_methods.iter().find(|(n, _)| n == name) {
            let crate_path = ci
                .native_crate
                .as_ref()
                .expect("validated in analyze::register_class");
            if !kwargs.is_empty() || args.len() != arity {
                panic!(
                    "wrong number of arguments for native `{target_name}.{name}`: expected {arity}, got {} (keyword arguments unsupported)",
                    args.len()
                );
            }
            let crate_ident = safe_ident(crate_path);
            let method_ident = safe_ident(name);
            let arg_exprs = args.iter().map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            });
            return quote! { #crate_ident::#method_ident(#(#arg_exprs),*)? };
        }
    }
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
                spinel_rt::send_value(&__safe_recv, #name_expr, &[#(#arg_exprs),*], None)?
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
                .resolve_class(target_name)
                .unwrap_or_else(|| panic!("unknown class/module `{target_name}`"));
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
        let class_id_expr = match infer_any_class(cx, recv_id) {
            Some(cid) => {
                let id = cid.0;
                // Same "evaluate the receiver for its side effects even
                // though the class id itself is compile-time-known" reasoning
                // as `is_a?`/`kind_of?` above.
                quote! { { let _ = #recv_expr; spinel_rt::ClassId(#id) } }
            }
            None => quote! { (#recv_expr).class_id() },
        };
        return quote! {
            spinel_rt::RubyValue::Bool(spinel_rt::responds_to(#class_id_expr, #sym_expr))
        };
    }

    // `.nil?` -- universal, same override-respecting shape as
    // `freeze`/`frozen?` below (surfaced as a real need by Phase 13.5's
    // queue-sentinel idiom, `break if q.pop.nil?`, on a Poly receiver). A
    // statically-known Object receiver is never nil (only `RubyValue::Nil`
    // is), but its receiver expression still evaluates for side effects.
    if no_kwargs && name == "nil?" && args.is_empty() {
        let user_defined = matches!(
            infer(cx, recv_id),
            TyKind::Object(cid) if cx.compiler.method_in_chain(cid, name).is_some()
        );
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
        let user_defined = matches!(
            infer(cx, recv_id),
            TyKind::Object(cid) if cx.compiler.method_in_chain(cid, name).is_some()
        );
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
                        let __r = spinel_rt::catch_break(__blk(&[]));
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
    // statically known `Int` (see `INT_BINARY_OPS`'s docs above).
    if no_kwargs && args.len() == 1 {
        if let Some(&(_, rt_fn, result_ty)) =
            INT_BINARY_OPS.iter().find(|(op, _, _)| *op == name)
        {
            let recv_ty = infer(cx, recv_id);
            let arg_ty = infer(cx, args[0]);
            if recv_ty == TyKind::Int && arg_ty == TyKind::Int {
                let arg_expr = emit_expr(cx, args[0]);
                let wrapper = format_ident!("{result_ty}");
                let call = emit_int_div_or_mod_checked(
                    cx,
                    rt_fn,
                    quote! { (#recv_expr).as_int_unchecked() },
                    quote! { (#arg_expr).as_int_unchecked() },
                );
                return quote! { spinel_rt::RubyValue::#wrapper(#call) };
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
        let block_value = emit_block_option(cx, block, block_arg);
        // `catch_break` applied unconditionally on this fully-dynamic path
        // (unlike Path 1's `needs_block`-gated version): `send`'s target
        // isn't statically known here, so there's no way to tell in advance
        // whether it might invoke a block -- the match is a cheap no-op
        // when no `Signal::Break` was actually raised.
        return quote! {
            spinel_rt::catch_break(spinel_rt::send_value(&#recv_obj_expr, #name_expr, &[#(#rest_args),*], #block_value))?
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
                    let wrapper = format_ident!("{result_ty}");
                    let call = emit_int_div_or_mod_checked(cx, rt_fn, quote! { *__r }, quote! { *__a });
                    quote! {
                        (spinel_rt::RubyValue::Int(__r), spinel_rt::RubyValue::Int(__a)) => {
                            spinel_rt::RubyValue::#wrapper(#call)
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
                // Non-numeric operands fall through to DYNAMIC dispatch
                // (Phase 14.4): a user class's own operator method (`def
                // <<`), or `send_value`'s builtin table (`Array#<<`,
                // `String#+`, universal `==`) -- replacing the old
                // unconditional panic.
                return quote! {
                    match (&(#recv_expr), &(#arg_expr)) {
                        #int_arm
                        #float_arm
                        #cmp_arm
                        (__dyn_recv, __dyn_arg) => spinel_rt::catch_break(spinel_rt::send_value(
                            __dyn_recv,
                            spinel_rt::Symbol::intern(#name),
                            &[(*__dyn_arg).clone()],
                            None,
                        ))?,
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
                        __dyn_recv => spinel_rt::catch_break(spinel_rt::send_value(
                            __dyn_recv,
                            spinel_rt::Symbol::intern(#name),
                            &[],
                            None,
                        ))?,
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
    ) {
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
            spinel_rt::catch_break(spinel_rt::send_value(
                &(#recv_expr),
                #name_expr,
                &[#(#arg_exprs),*],
                #block_value,
            ))?
        };
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
        {
            if !kwargs.is_empty() {
                panic!(
                    "dynamic dispatch of `{name}` with keyword arguments isn't supported yet (spike scope): call it directly instead"
                );
            }
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            let name_expr = quote! { spinel_rt::Symbol::intern(#name) };
            let arg_exprs = args.iter().map(|&a| {
                let e = emit_expr(cx, a);
                box_if_object_typed(cx, a, e)
            });
            let block_value = emit_block_option(cx, block, block_arg);
            return quote! {
                spinel_rt::catch_break(spinel_rt::send_value(
                    &spinel_rt::RubyValue::Object(#class_ident::new_handle(#recv_expr)),
                    #name_expr,
                    &[#(#arg_exprs),*],
                    #block_value,
                ))?
            };
        }
    }

    panic!("unsupported call `{name}` (spike scope, or receiver's class isn't statically known)");
}
