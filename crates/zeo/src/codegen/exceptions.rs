//! `begin`/`rescue`/`else`/`ensure`/`retry`. See the plan's Part 8
//! for the full design rationale -- translated from zeo's own
//! `setjmp`/`longjmp`-based C implementation into ordinary `Result`/`?`-based
//! propagation, since Rust's own exhaustive `match` and straight-line
//! execution already give "`ensure` always runs, exactly once, after a
//! `retry`-driven re-attempt has settled" for free, with no goto-funnel or
//! deferred-return-flag bookkeeping needed the way zeo's C codegen does.
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
//! it cannot reach the enclosing method/loop directly (a closure is
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
//! **A `break`/`next`/`redo` targeting a native loop OUTSIDE the `begin`**
//! (e.g. `while cond; begin; break; rescue; end; end`) needs no loop-body
//! closure at all -- the key observation is that the `begin` EXPRESSION is
//! itself spliced INLINE into the loop body (only its sub-bodies are
//! closures), so the point where the `begin` yields its value is ordinary
//! inline Rust where the enclosing loop's own literal labels are still in
//! scope. So instead of `?`-propagating the bubbled `Signal::Break`/`Next`/
//! `Redo` out of the whole method (which would skip the loop, wrong), the
//! `begin`'s final settling MATCHES on it and translates it into the exact
//! literal `break`/`continue` the loop machinery already uses -- reaching
//! `cx.loop_labels`, which is always the innermost enclosing native loop,
//! exactly the target a bare (label-less) Ruby `break`/`next`/`redo` means.
//! The loop body keeps its zero-cost literal-label shape untouched. This
//! composes through NESTED `begin`s: an inner `begin` (whose own
//! `cx.loop_labels` was cleared to `None` by the outer `begin`'s closure)
//! `?`-propagates the signal up to the outer `begin`, which -- being inline
//! in the loop -- performs the translation. `loop_crossing_target` decides
//! when this applies (a bubbling jump present AND a native loop lexically
//! encloses the `begin`); `node_contains_bubbling_loop_jump` descends
//! THROUGH nested `begin`s (they re-raise, they don't absorb) but stops at
//! any construct that establishes its OWN loop label or closure boundary. A
//! loop written INSIDE the `begin` itself is unaffected either way (its own
//! labels are established inside the very same closure, so a jump targeting
//! IT works via ordinary literal jumps, no signal involved at all).

use quote::quote;

use super::Ctx;
use super::loops::fresh_label;
use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HirNode, NodeId, RescueClause, StrPart};
use proc_macro2::TokenStream;

/// `retry` -- always raises `Signal::Retry`, unconditionally, never a
/// literal Rust `continue`: it must cross the same closure boundary
/// `emit_begin`'s `body`/`rescue`-clause bodies are wrapped in (see the
/// module's docs), exactly like `break`/`next`/`redo` do once a real
/// escaping `Proc`'s closure boundary is in play (`codegen::loops`). Caught
/// by the nearest enclosing `begin`'s own retry loop. A `retry` used
/// outside any `rescue` clause -- a real, if rare, Ruby `SyntaxError` the
/// lowering doesn't separately re-validate -- simply
/// propagates as an uncaught `Signal`, matching this project's existing
/// posture for other mis-scoped constructs it doesn't statically re-check
/// (e.g. a bare `break` outside any loop is instead a clean codegen panic --
/// `retry` differs only because, unlike `break`, there's no equivalent
/// "was there ever a possible target at all" check cheap enough to do here).
pub fn emit_retry() -> TokenStream {
    quote! { return Err(zeo_rt::Signal::Retry) }
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
    // When a bare `break`/`next`/`redo` inside this `begin` targets a native
    // loop lexically OUTSIDE it, capture that loop's labels so the final
    // settling below can translate the bubbled `Signal` into a literal jump
    // (see the module docs). `None` -> ordinary `?`-propagation.
    let crossing = loop_crossing_target(cx, body, rescues, else_body);

    // See the module docs for why `loop_labels` is cleared and
    // `in_real_proc` is forced on: `body`/each rescue clause's
    // body/`else_body` are all about to be captured inside a fresh closure
    // boundary, so `return`/`break`/`next`/`redo` lexically inside them can
    // cannot compile to a literal Rust keyword.
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
                quote! { (|| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> { #inner })() },
            )
        }
    };

    let rescue_chain = emit_rescue_chain(&closure_cx, rescues);
    let retry_label = fresh_label(cx, "retry");
    let ensure_tokens = ensure_body.as_ref().map(|stmts| {
        let e = super::stmt::emit_body(cx, stmts, false);
        quote! { { #e }; }
    });

    // Settle `__final`. Normally `?`-propagate (any `Signal` unwinds past this
    // `begin`). But when a bare loop-jump inside this `begin` targets a native
    // loop OUTSIDE it, translate that bubbled `Signal` into the loop's literal
    // `break`/`continue` right here -- this `match` is spliced INLINE in the
    // loop body, so those labels are in scope (see the module docs). A
    // `Signal::Next`'s value is discarded, matching a native loop's own `next`.
    let settle = match &crossing {
        Some((redo_label, outer_label)) => quote! {
            match __final {
                Err(zeo_rt::Signal::Break(__bv)) => break #outer_label __bv,
                Err(zeo_rt::Signal::Next(_)) => continue #outer_label,
                Err(zeo_rt::Signal::Redo) => continue #redo_label,
                __other => __other?,
            }
        },
        None => quote! { __final? },
    };

    quote! {
        {
            let __final: Result<zeo_rt::RubyValue, zeo_rt::Signal> = #retry_label: loop {
                let __body_result: Result<zeo_rt::RubyValue, zeo_rt::Signal> =
                    (|| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> { #body_tokens })();
                let __outcome: Result<zeo_rt::RubyValue, zeo_rt::Signal> = match __body_result {
                    Ok(#ok_bind) => #success_tokens,
                    Err(zeo_rt::Signal::Raise(__exc)) => { #rescue_chain },
                    __other => __other,
                };
                match __outcome {
                    Err(zeo_rt::Signal::Retry) => continue #retry_label,
                    __settled => break #retry_label __settled,
                }
            };
            #ensure_tokens
            #settle
        }
    }
}

/// Builds the `rescue`-clause matching chain: `if <matches R1> { ... } else
/// if <matches R2> { ... } else { re-raise }`, tested top to bottom (first
/// matching clause wins, mirroring `RescueNode::subsequent()`'s own chained
/// shape). `__exc: RubyValue` is bound by `emit_begin`'s own `Err(Signal::
/// Raise(__exc))` match arm, in scope throughout. Each matching clause's own
/// body runs inside `zeo_rt::push_handling`/`pop_handling` (backing a
/// bare re-raise and, in the future, `$!`) -- see `zeo_rt::handling`'s
/// docs.
fn emit_rescue_chain(closure_cx: &Ctx, rescues: &[RescueClause]) -> TokenStream {
    let mut chain = quote! { Err(zeo_rt::Signal::Raise(__exc.clone())) };
    for r in rescues.iter().rev() {
        let cond = emit_rescue_match_cond(closure_cx, &r.classes, &r.splats);
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
        let binding_write = r.binding.as_ref().map(|name| {
            super::hoisting::emit_local_write(closure_cx, name, quote! { __exc.clone() })
        });
        let body_tokens = super::stmt::emit_body(closure_cx, &r.body, true);
        let previous = chain;
        chain = quote! {
            if #cond {
                #binding_write
                zeo_rt::push_handling(__exc.clone());
                let __rescue_result: Result<zeo_rt::RubyValue, zeo_rt::Signal> =
                    (|| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> { #body_tokens })();
                zeo_rt::pop_handling();
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
/// (`zeo_rt::is_a`), OR'd across every listed class (`rescue A, B => e`).
/// An empty `classes` list (a bare `rescue`) matches `StandardError` and its
/// descendants -- real Ruby's own default, deliberately narrower than
/// "catches literally anything" (a bare `rescue` does NOT catch a raised
/// `Exception`/`ScriptError` that isn't also a `StandardError`).
fn emit_rescue_match_cond(cx: &Ctx, classes: &[String], splats: &[NodeId]) -> TokenStream {
    // A bare `rescue` (no listed classes AND no splats) matches `StandardError`
    // and its descendants -- CRuby's default. A `rescue *errs` with no static
    // classes does NOT get this default: only the splat list matters.
    if classes.is_empty() && splats.is_empty() {
        let id = cx
            .resolve_class("StandardError")
            .expect("StandardError is a builtin")
            .0;
        return quote! {
            zeo_rt::is_a(__exc.as_object_unchecked().class_id(), zeo_rt::ClassId(#id))
        };
    }
    let static_checks = classes.iter().map(|name| {
        let Some(cid) = cx.resolve_class(name) else {
            // A `rescue UndefinedConst` names a constant that isn't a class
            // here. CRuby evaluates a rescue clause's class expression only
            // while MATCHING an actually-raised exception, and an undefined
            // constant there is a runtime NameError -- deferred to that point
            // (this `||`-joined check runs only when a rescue is being matched,
            // and short-circuits if an earlier listed class already matched),
            // so a `rescue` clause that never fires still compiles.
            let err = super::expr::uninitialized_constant_error(cx, name);
            return super::expr::raise_in_expr_position(err, quote! { bool });
        };
        // The raw baked id, not `#ident::CLASS_ID`: a rescue target may be
        // a MODULE (`rescue Alertable => e` -- real Ruby matches any
        // exception whose class includes it), which has no generated
        // struct to hang a const off.
        let id = cid.0;
        quote! { zeo_rt::is_a(__exc.as_object_unchecked().class_id(), zeo_rt::ClassId(#id)) }
    });
    // `rescue *errs => e`: evaluate the splat expression (an array of classes,
    // or a single class) and match `__exc` against it at runtime. Its own
    // error (a non-Module element, an undefined const while evaluating)
    // propagates like the undefined-static-const case above.
    let splat_checks = splats.iter().map(|&node| {
        let val = super::expr::emit_expr(cx, node);
        quote! {
            match zeo_rt::rescue_matches_any(&__exc, &(#val)) {
                Ok(__m) => __m,
                Err(__s) => return Err(__s),
            }
        }
    });
    let checks = static_checks.chain(splat_checks);
    quote! { #(#checks)||* }
}

/// The enclosing native loop's `(redo_label, outer_label)` when a bare
/// `break`/`next`/`redo` inside `body`/a `rescue` clause/`else` targets a
/// loop OUTSIDE this `begin` -- the labels `emit_begin`'s final settling
/// translates the bubbled `Signal` into (see the module's top-level docs).
/// `None` (ordinary `?`-propagation) when no native loop lexically encloses
/// this `begin` (`cx.loop_labels` is `None` -- either nothing encloses this
/// code, or it's already inside a real Proc, whose own `in_real_proc`
/// signal-raising path handles a `break`/`next`/`redo` correctly) or when no
/// such crossing jump is present at all. `ensure` is deliberately NOT scanned:
/// it's emitted with the ORIGINAL `cx` (loop labels intact), so a jump there
/// already compiles to a literal jump with no signal involved.
fn loop_crossing_target(
    cx: &Ctx,
    body: &[NodeId],
    rescues: &[RescueClause],
    else_body: &Option<Vec<NodeId>>,
) -> Option<(syn::Lifetime, syn::Lifetime)> {
    let (redo_label, outer_label) = cx.loop_labels.as_ref()?;
    let mut crosses = body_contains_bubbling_loop_jump(cx.compiler, body);
    for r in rescues {
        crosses |= body_contains_bubbling_loop_jump(cx.compiler, &r.body);
    }
    if let Some(b) = else_body {
        crosses |= body_contains_bubbling_loop_jump(cx.compiler, b);
    }
    crosses.then(|| (redo_label.clone(), outer_label.clone()))
}

fn body_contains_bubbling_loop_jump(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter()
        .any(|&n| node_contains_bubbling_loop_jump(compiler, n))
}

/// Whether `id` lexically contains a bare `break`/`next`/`redo` that would
/// BUBBLE (as a `Signal`) out to a native loop enclosing the `begin` we're
/// scanning from. Descends THROUGH a nested `Begin` -- a jump inside one
/// still re-raises past every `begin` to the loop, so it's a crossing jump
/// too. Stops at anything that establishes its OWN native-loop label
/// (`While`/`Loop`/`For`) or its OWN closure boundary (an escaping block):
/// a jump inside one of those targets THAT construct, and is independently
/// correct on its own terms when it's emitted.
fn node_contains_bubbling_loop_jump(compiler: &Compiler, id: NodeId) -> bool {
    match &compiler.hir[id] {
        HirNode::Break(_) | HirNode::Next(_) | HirNode::Redo => true,
        HirNode::While { .. } | HirNode::Loop { .. } | HirNode::For { .. } => false,
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            // A nested `begin` re-raises the jump rather than absorbing it, so
            // descend into every clause EXCEPT `ensure` (its jumps compile to
            // literal jumps directly, never a bubbling `Signal`).
            let _ = ensure_body;
            body_contains_bubbling_loop_jump(compiler, body)
                || rescues
                    .iter()
                    .any(|r| body_contains_bubbling_loop_jump(compiler, &r.body))
                || else_body
                    .as_deref()
                    .is_some_and(|b| body_contains_bubbling_loop_jump(compiler, b))
        }
        HirNode::LocalWrite(_, v)
        | HirNode::IvarWrite(_, v)
        | HirNode::ClassVarWrite(_, v)
        | HirNode::Defined(v) => node_contains_bubbling_loop_jump(compiler, *v),
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            node_contains_bubbling_loop_jump(compiler, *l)
                || node_contains_bubbling_loop_jump(compiler, *r)
        }
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            node_contains_bubbling_loop_jump(compiler, *cond)
                || body_contains_bubbling_loop_jump(compiler, then_body)
                || body_contains_bubbling_loop_jump(compiler, else_body)
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            subject.is_some_and(|s| node_contains_bubbling_loop_jump(compiler, s))
                || arms.iter().any(|(values, body)| {
                    values.iter().any(|e| {
                        let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                        node_contains_bubbling_loop_jump(compiler, *v)
                    }) || body_contains_bubbling_loop_jump(compiler, body)
                })
                || body_contains_bubbling_loop_jump(compiler, else_body)
        }
        HirNode::CaseIn {
            subject,
            arms,
            else_body,
        } => {
            node_contains_bubbling_loop_jump(compiler, *subject)
                || arms
                    .iter()
                    .any(|arm| body_contains_bubbling_loop_jump(compiler, &arm.body))
                || else_body
                    .as_deref()
                    .is_some_and(|b| body_contains_bubbling_loop_jump(compiler, b))
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            ..
        } => {
            let block_jumps = block.is_some_and(|b| {
                let HirNode::Block { body, .. } = &compiler.hir[b] else {
                    panic!("internal error: a Block node should only be reached via the Call that invokes it");
                };
                // `.times` shares THIS same Rust scope (an inline splice,
                // not a closure) -- its own label handles a break/next/redo
                // inside it, no conflict; a genuinely escaping block is
                // already its own separate Rust closure, independently
                // correct via its own `in_real_proc` handling.
                super::call::is_spliced_block_body(compiler, *receiver, name, kwargs.is_empty(), b)
                    && body_contains_bubbling_loop_jump(compiler, body)
            });
            block_jumps
                || receiver.is_some_and(|r| node_contains_bubbling_loop_jump(compiler, r))
                || args.iter().any(|a| {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    node_contains_bubbling_loop_jump(compiler, *n)
                })
                || kwargs
                    .iter()
                    .flat_map(|kw| kw.node_ids())
                    .any(|n| node_contains_bubbling_loop_jump(compiler, n))
                || block_arg.is_some_and(|b| node_contains_bubbling_loop_jump(compiler, b))
        }
        HirNode::MultiWrite { targets, value } => {
            let mut found = false;
            targets.for_each_node(&mut |n| found |= node_contains_bubbling_loop_jump(compiler, n));
            found || node_contains_bubbling_loop_jump(compiler, *value)
        }
        HirNode::GlobalWrite(_, value) => node_contains_bubbling_loop_jump(compiler, *value),
        HirNode::ConstWrite { value, .. } => node_contains_bubbling_loop_jump(compiler, *value),
        HirNode::PreExec(body) | HirNode::Seq(body) => {
            body_contains_bubbling_loop_jump(compiler, body)
        }
        HirNode::Yield(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_bubbling_loop_jump(compiler, *n)
        }),
        HirNode::Raise(args, _) => args
            .iter()
            .any(|&a| node_contains_bubbling_loop_jump(compiler, a)),
        HirNode::New { args, kwargs, .. } => {
            args.iter()
                .any(|&a| node_contains_bubbling_loop_jump(compiler, a))
                || kwargs
                    .iter()
                    .flat_map(|kw| kw.node_ids())
                    .any(|a| node_contains_bubbling_loop_jump(compiler, a))
        }
        HirNode::SuperCall { args, kwargs, block_arg, .. } => {
            args.iter()
                .any(|a| node_contains_bubbling_loop_jump(compiler, a.node_id()))
                || kwargs
                    .iter()
                    .flat_map(|kw| kw.node_ids())
                    .any(|a| node_contains_bubbling_loop_jump(compiler, a))
                || block_arg.is_some_and(|b| node_contains_bubbling_loop_jump(compiler, b))
        }
        HirNode::ArrayLit(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_bubbling_loop_jump(compiler, *n)
        }),
        HirNode::HashLit(pairs) => pairs
            .iter()
            .flat_map(|kw| kw.node_ids())
            .any(|n| node_contains_bubbling_loop_jump(compiler, n)),
        HirNode::RangeLit { start, end, .. } => {
            start.is_some_and(|s| node_contains_bubbling_loop_jump(compiler, s))
                || end.is_some_and(|e| node_contains_bubbling_loop_jump(compiler, e))
        }
        HirNode::StringLit(parts) => parts.iter().any(|p| match p {
            StrPart::Interp(n) => node_contains_bubbling_loop_jump(compiler, *n),
            StrPart::Lit(_) | StrPart::Bytes(_) => false,
        }),
        HirNode::Eval(body) | HirNode::BoxScope { body, .. } => {
            body_contains_bubbling_loop_jump(compiler, body)
        }
        HirNode::MatchPredicate { subject, .. } | HirNode::MatchRequired { subject, .. } => {
            node_contains_bubbling_loop_jump(compiler, *subject)
        }
        _ => false,
    }
}
