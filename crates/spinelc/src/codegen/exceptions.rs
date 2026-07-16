//! `begin`/`rescue`/`else`/`ensure`/`retry` (Phase 9). See the plan's Part 8
//! for the full design rationale -- translated from spinel's own
//! `setjmp`/`longjmp`-based C implementation into ordinary `Result`/`?`-based
//! propagation, since Rust's own exhaustive `match` and straight-line
//! execution already give "`ensure` always runs, exactly once, after a
//! `retry`-driven re-attempt has settled" for free, with no goto-funnel or
//! deferred-return-flag bookkeeping needed the way spinel's C codegen does.
//!
//! **The closure boundary, and why `retry`/`break`/`next`/`redo`/`return`
//! all change shape inside one.** `body`, each `rescue` clause's own body,
//! and `else_body` are all captured via an immediately-invoked, NON-`move`
//! closure -- `(|| -> Result<RubyValue, Signal> { ... })()` -- exactly the
//! same shape `codegen::mod::emit_class`'s own per-method `Signal::Return`
//! catch already uses (not a real, storable, escaping `Proc`): since it's
//! invoked in the very same statement it's defined in, it never needs
//! `move`/an explicit capture list, and can read AND WRITE any hoisted local
//! or `self`/ivar in its enclosing scope exactly as if the code were written
//! inline. This closure boundary is unavoidable -- it's the only way to
//! inspect a sub-expression's `Result` (to test a raised exception against
//! `rescue` clauses) without immediately propagating it via `?` -- but it
//! means a literal Rust `return`/labeled `break`/`continue` lexically inside
//! it can no longer reach the enclosing method/loop directly (a closure is
//! its own `fn`-like boundary for both). So, while emitting these three
//! bodies: `return` raises `Signal::Return` instead of a literal `return`
//! (caught by `codegen::mod`'s per-method wrapping, extended to trigger on
//! any `Begin` node -- see `codegen::captures::body_contains_begin`);
//! `break`/`next`/`redo` raise the matching `Signal` instead of a literal
//! labeled jump (the exact mechanism `codegen::loops` already uses once a
//! REAL escaping `Proc`'s own closure boundary is in play); and `retry`
//! ALWAYS raises `Signal::Retry` (see `emit_retry`'s docs), caught right
//! here by the retry loop below.
//!
//! **The one gap this doesn't solve**: a `break`/`next`/`redo` lexically
//! inside `body`/a `rescue` clause/`else`, intended to target a native loop
//! LEXICALLY OUTSIDE the `begin` (e.g. `while cond; begin; break; rescue;
//! end; end`), has nowhere correct to go -- the native loop machinery
//! (`codegen::loops`) relies entirely on literal Rust `break`/`continue` and
//! never inspects a `Result` for a bubbled-up `Signal::Break`/`Next`/`Redo`,
//! and teaching it to do so would mean wrapping every loop body in the same
//! kind of closure this module uses, undoing the zero-cost literal-label
//! design Phase 4 deliberately chose. Rather than silently miscompiling
//! this (the signal would `?`-propagate past the loop and out of the whole
//! method, wrong), `reject_unsupported_loop_crossing` detects it and panics
//! with a clear message -- a narrow, real, DOCUMENTED scope-cut, not an
//! oversight. A loop written INSIDE the `begin` itself is completely
//! unaffected (its own labels are established inside the very same closure,
//! so a `break`/`next`/`redo` targeting IT works via ordinary literal jumps,
//! no signal involved at all).

use quote::quote;

use super::loops::fresh_label;
use super::Ctx;
use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HirNode, NodeId, RescueClause, StrPart};
use proc_macro2::TokenStream;

/// `retry` -- always raises `Signal::Retry`, unconditionally, never a
/// literal Rust `continue`: it must cross the same closure boundary
/// `emit_begin`'s `body`/`rescue`-clause bodies are wrapped in (see the
/// module's docs), exactly like `break`/`next`/`redo` do once a real
/// escaping `Proc`'s closure boundary is in play (`codegen::loops`). Caught
/// by the nearest enclosing `begin`'s own retry loop. A `retry` used
/// outside any `rescue` clause -- a real, if rare, Ruby `SyntaxError` this
/// spike doesn't separately re-validate at lowering time -- simply
/// propagates as an uncaught `Signal`, matching this project's existing
/// posture for other mis-scoped constructs it doesn't statically re-check
/// (e.g. a bare `break` outside any loop is instead a clean codegen panic --
/// `retry` differs only because, unlike `break`, there's no equivalent
/// "was there ever a possible target at all" check cheap enough to do here).
pub fn emit_retry() -> TokenStream {
    quote! { return Err(spinel_rt::Signal::Retry) }
}

/// `begin body rescue ... else ... ensure ... end` -- also the desugared
/// shape of an implicit method-body rescue and `expr rescue fallback` (see
/// `parse/mod.rs`'s recognizers, all of which produce this same HIR shape).
///
/// ```text
/// {
///     let __final: Result<RubyValue, Signal> = 'retry: loop {
///         let __body_result = (|| { body })();
///         let __outcome = match __body_result {
///             Ok(v)                       => success path (else, or just v),
///             Err(Signal::Raise(exc))     => rescue-clause chain, or re-raise,
///             other                       => other,               // Return/Break/Next/Redo pass through
///         };
///         match __outcome {
///             Err(Signal::Retry) => continue 'retry,
///             settled             => break 'retry settled,
///         }
///     };
///     ensure-statements;   // always run, exactly once, regardless of __final
///     __final?             // propagate whatever finally remains
/// }
/// ```
pub fn emit_begin(
    cx: &Ctx,
    body: &[NodeId],
    rescues: &[RescueClause],
    else_body: &Option<Vec<NodeId>>,
    ensure_body: &Option<Vec<NodeId>>,
) -> TokenStream {
    reject_unsupported_loop_crossing(cx, body, rescues, else_body);

    // See the module docs for why `loop_labels` is cleared and
    // `in_real_proc` is forced on: `body`/each rescue clause's
    // body/`else_body` are all about to be captured inside a fresh closure
    // boundary, so `return`/`break`/`next`/`redo` lexically inside them can
    // no longer compile to a literal Rust keyword.
    let closure_cx = Ctx {
        loop_labels: None,
        for_var_override: None,
        in_real_proc: true,
        ..cx.clone()
    };

    let body_tokens = super::stmt::emit_body(&closure_cx, body, true);

    // When there's no `else`, the body's own value IS the success value
    // (bound as `__v`, used directly); when there IS an `else`, the body's
    // value is discarded (bound as `_v`, matching real Ruby: an `else`
    // clause's own value replaces it) and `else_body` is evaluated in its
    // own nested closure (it, too, may raise/return/break/next).
    let (ok_bind, success_tokens) = match else_body {
        None => (quote! { __v }, quote! { Ok(__v) }),
        Some(stmts) => {
            let inner = super::stmt::emit_body(&closure_cx, stmts, true);
            (
                quote! { _v },
                quote! { (|| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> { #inner })() },
            )
        }
    };

    let rescue_chain = emit_rescue_chain(&closure_cx, rescues);
    let retry_label = fresh_label(cx, "retry");
    let ensure_tokens = ensure_body.as_ref().map(|stmts| {
        let e = super::stmt::emit_body(cx, stmts, false);
        quote! { { #e }; }
    });

    quote! {
        {
            let __final: Result<spinel_rt::RubyValue, spinel_rt::Signal> = #retry_label: loop {
                let __body_result: Result<spinel_rt::RubyValue, spinel_rt::Signal> =
                    (|| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> { #body_tokens })();
                let __outcome: Result<spinel_rt::RubyValue, spinel_rt::Signal> = match __body_result {
                    Ok(#ok_bind) => #success_tokens,
                    Err(spinel_rt::Signal::Raise(__exc)) => { #rescue_chain },
                    __other => __other,
                };
                match __outcome {
                    Err(spinel_rt::Signal::Retry) => continue #retry_label,
                    __settled => break #retry_label __settled,
                }
            };
            #ensure_tokens
            __final?
        }
    }
}

/// Builds the `rescue`-clause matching chain: `if <matches R1> { ... } else
/// if <matches R2> { ... } else { re-raise }`, tested top to bottom (first
/// matching clause wins, mirroring `RescueNode::subsequent()`'s own chained
/// shape). `__exc: RubyValue` is bound by `emit_begin`'s own `Err(Signal::
/// Raise(__exc))` match arm, in scope throughout. Each matching clause's own
/// body runs inside `spinel_rt::push_handling`/`pop_handling` (backing a
/// bare re-raise and, in the future, `$!`) -- see `spinel_rt::handling`'s
/// docs.
fn emit_rescue_chain(closure_cx: &Ctx, rescues: &[RescueClause]) -> TokenStream {
    let mut chain = quote! { Err(spinel_rt::Signal::Raise(__exc.clone())) };
    for r in rescues.iter().rev() {
        let cond = emit_rescue_match_cond(closure_cx, &r.classes);
        // NOT narrowed to a concrete `Arc<Class>` the way a pattern's
        // `Integer => n`/`case/in`'s class-guard capture is (see
        // `codegen::patterns::collect_narrowing`'s docs): unlike a builtin
        // primitive's runtime TAG check, dispatch here is Rust `downcast::
        // <T>()`-based, which only succeeds against the EXACT concrete
        // struct -- `rescue StandardError => e` must also match any raised
        // SUBCLASS instance (`ArgumentError`, etc.), which can never
        // downcast to the literal `StandardError` struct. `e` stays `Poly`
        // (`RubyValue::Object(...)`) -- calling an ordinary method on it
        // needs `.send(:name)` (Path 2), a real, documented scope-cut.
        let binding_write = r
            .binding
            .as_ref()
            .map(|name| super::hoisting::emit_local_write(closure_cx, name, quote! { __exc.clone() }));
        let body_tokens = super::stmt::emit_body(closure_cx, &r.body, true);
        let previous = chain;
        chain = quote! {
            if #cond {
                #binding_write
                spinel_rt::push_handling(__exc.clone());
                let __rescue_result: Result<spinel_rt::RubyValue, spinel_rt::Signal> =
                    (|| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> { #body_tokens })();
                spinel_rt::pop_handling();
                __rescue_result
            } else {
                #previous
            }
        };
    }
    chain
}

/// A `rescue` clause's own match condition against the raised value `__exc`
/// -- an `is_a?`-style ancestry check against the SAME linearized
/// `ancestors` list `super`/pattern-matching class-checks already consult
/// (`spinel_rt::is_a`), OR'd across every listed class (`rescue A, B => e`).
/// An empty `classes` list (a bare `rescue`) matches `StandardError` and its
/// descendants -- real Ruby's own default, deliberately narrower than
/// "catches literally anything" (a bare `rescue` does NOT catch a raised
/// `Exception`/`ScriptError` that isn't also a `StandardError`).
fn emit_rescue_match_cond(cx: &Ctx, classes: &[String]) -> TokenStream {
    let owned;
    let targets: &[String] = if classes.is_empty() {
        owned = vec!["StandardError".to_string()];
        &owned
    } else {
        classes
    };
    let checks = targets.iter().map(|name| {
        let cid = cx
            .resolve_class(name)
            .unwrap_or_else(|| panic!("unknown class `{name}` in a `rescue` clause (must be defined earlier in the file)"));
        // The raw baked id, not `#ident::CLASS_ID`: a rescue target may be
        // a MODULE (`rescue Alertable => e` -- real Ruby matches any
        // exception whose class includes it), which has no generated
        // struct to hang a const off.
        let id = cid.0;
        quote! { spinel_rt::is_a(__exc.as_object_unchecked().class_id(), spinel_rt::ClassId(#id)) }
    });
    quote! { #(#checks)||* }
}

/// See the module's top-level docs for why this is needed at all: a
/// `break`/`next`/`redo` lexically inside `body`/a `rescue` clause/`else`,
/// intended to reach a native loop OUTSIDE the `begin`, has no correct
/// codegen target once those bodies are wrapped in their own closure
/// boundary. Only checked when a native loop actually lexically encloses
/// this `begin` (`cx.loop_labels.is_some()`) -- otherwise there's no
/// possible conflicting target at all (either no loop encloses this code,
/// or the code is already inside a real Proc, whose own existing
/// `in_real_proc` signal-raising path already handles it correctly).
fn reject_unsupported_loop_crossing(
    cx: &Ctx,
    body: &[NodeId],
    rescues: &[RescueClause],
    else_body: &Option<Vec<NodeId>>,
) {
    if cx.loop_labels.is_none() {
        return;
    }
    let mut offending = body_contains_bare_loop_jump(cx.compiler, body);
    for r in rescues {
        offending |= body_contains_bare_loop_jump(cx.compiler, &r.body);
    }
    if let Some(b) = else_body {
        offending |= body_contains_bare_loop_jump(cx.compiler, b);
    }
    if offending {
        panic!(
            "`break`/`next`/`redo` inside a `begin`/`rescue`/`else` clause, targeting a loop OUTSIDE it, isn't supported yet (spike scope) -- a loop written INSIDE the `begin` itself is unaffected"
        );
    }
}

fn body_contains_bare_loop_jump(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter().any(|&n| node_contains_bare_loop_jump(compiler, n))
}

/// Stops descending at anything that establishes its OWN native-loop label
/// or its OWN closure boundary -- a `break`/`next`/`redo` lexically inside
/// one of those targets THAT construct, never whatever loop encloses the
/// `begin` we're scanning from (each such construct is independently correct
/// / independently re-checked on its own terms when it's emitted).
fn node_contains_bare_loop_jump(compiler: &Compiler, id: NodeId) -> bool {
    match &compiler.hir[id] {
        HirNode::Break(_) | HirNode::Next(_) | HirNode::Redo => true,
        HirNode::While { .. } | HirNode::Loop { .. } | HirNode::For { .. } | HirNode::Begin { .. } => false,
        HirNode::LocalWrite(_, v) | HirNode::IvarWrite(_, v) | HirNode::ClassVarWrite(_, v) | HirNode::Defined(v) => {
            node_contains_bare_loop_jump(compiler, *v)
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            node_contains_bare_loop_jump(compiler, *l) || node_contains_bare_loop_jump(compiler, *r)
        }
        HirNode::If { cond, then_body, else_body } => {
            node_contains_bare_loop_jump(compiler, *cond)
                || body_contains_bare_loop_jump(compiler, then_body)
                || body_contains_bare_loop_jump(compiler, else_body)
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            subject.is_some_and(|s| node_contains_bare_loop_jump(compiler, s))
                || arms.iter().any(|(values, body)| {
                    values.iter().any(|e| {
                        let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                        node_contains_bare_loop_jump(compiler, *v)
                    })
                        || body_contains_bare_loop_jump(compiler, body)
                })
                || body_contains_bare_loop_jump(compiler, else_body)
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            node_contains_bare_loop_jump(compiler, *subject)
                || arms.iter().any(|arm| body_contains_bare_loop_jump(compiler, &arm.body))
                || else_body.as_deref().is_some_and(|b| body_contains_bare_loop_jump(compiler, b))
        }
        HirNode::Call { receiver, name, args, kwargs, block, block_arg, .. } => {
            let block_jumps = block.is_some_and(|b| {
                let HirNode::Block { body, .. } = &compiler.hir[b] else {
                    panic!("a Block should only be reached via the Call that invokes it");
                };
                // `.times` shares THIS same Rust scope (an inline splice,
                // not a closure) -- its own label handles a break/next/redo
                // inside it, no conflict; a genuinely escaping block is
                // already its own separate Rust closure, independently
                // correct via its own `in_real_proc` handling.
                super::call::is_times_fast_path(compiler, *receiver, name, kwargs.is_empty())
                    && body_contains_bare_loop_jump(compiler, body)
            });
            block_jumps
                || receiver.is_some_and(|r| node_contains_bare_loop_jump(compiler, r))
                || args.iter().any(|a| {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    node_contains_bare_loop_jump(compiler, *n)
                })
                || kwargs
                    .iter()
                    .flat_map(|kw| kw.node_ids())
                    .any(|n| node_contains_bare_loop_jump(compiler, n))
                || block_arg.is_some_and(|b| node_contains_bare_loop_jump(compiler, b))
        }
        HirNode::MultiWrite { targets, value } => {
            let mut found = false;
            targets.for_each_node(&mut |n| found |= node_contains_bare_loop_jump(compiler, n));
            found || node_contains_bare_loop_jump(compiler, *value)
        }
        HirNode::GlobalWrite(_, value) => node_contains_bare_loop_jump(compiler, *value),
        HirNode::ConstWrite { value, .. } => node_contains_bare_loop_jump(compiler, *value),
        HirNode::PreExec(body) | HirNode::Seq(body) => body_contains_bare_loop_jump(compiler, body),
        HirNode::Yield(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_bare_loop_jump(compiler, *n)
        }),
        HirNode::Raise(args) => args.iter().any(|&a| node_contains_bare_loop_jump(compiler, a)),
        HirNode::New { args, .. } | HirNode::SuperCall { args, .. } => {
            args.iter().any(|&a| node_contains_bare_loop_jump(compiler, a))
        }
        HirNode::ArrayLit(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_bare_loop_jump(compiler, *n)
        }),
        HirNode::HashLit(pairs) => {
            pairs.iter().flat_map(|kw| kw.node_ids()).any(|n| node_contains_bare_loop_jump(compiler, n))
        }
        HirNode::RangeLit { start, end, .. } => {
            start.is_some_and(|s| node_contains_bare_loop_jump(compiler, s))
                || end.is_some_and(|e| node_contains_bare_loop_jump(compiler, e))
        }
        HirNode::StringLit(parts) => parts.iter().any(|p| match p {
            StrPart::Interp(n) => node_contains_bare_loop_jump(compiler, *n),
            StrPart::Lit(_) | StrPart::Bytes(_) => false,
        }),
        HirNode::Eval(body) | HirNode::BoxScope { body, .. } => body_contains_bare_loop_jump(compiler, body),
        HirNode::MatchPredicate { subject, .. } | HirNode::MatchRequired { subject, .. } => {
            node_contains_bare_loop_jump(compiler, *subject)
        }
        _ => false,
    }
}
