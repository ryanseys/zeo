//! `Proc`/lambda VALUE construction -- `emit_proc_value` (a literal block
//! AS a `Proc`, e.g. `Kernel#proc`/`at_exit`), `emit_lambda_value` (the
//! `->(...) { }`/`lambda { }` STRICT-arity variant), their shared
//! `emit_proc_or_lambda_value` machinery (captures, the redo-wrapper loop),
//! and `emit_lambda_arity_check`.

use quote::{format_ident, quote};

use crate::codegen::Ctx;
use crate::codegen::ident::safe_ident;
use crate::hir::{HirNode, NodeId, Params};
use proc_macro2::TokenStream;

/// Builds a real, escaping `zeo_rt::RubyValue::Proc` value from a literal
/// block (`HirNode::Block`) at a call site whose callee ISN'T the `.times`
/// inline fast path (see `is_times_fast_path`'s docs -- that's the entire
/// "escape decision", no separate dataflow analysis exists). A plain Rust
/// closure (`move |args| { ... }`), not a hand-rolled env struct/trait --
/// Rust's own closure capture already builds exactly the environment one
/// would otherwise hand-generate (see `zeo_rt::rproc`'s docs).
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
        panic!("internal error: expected a Block node at block_id")
    };
    emit_proc_or_lambda_value(cx, params, body, false, false)
}
/// `-> (x) { ... }` / `lambda { ... }` (`HirNode::Lambda`) -- see that
/// variant's docs. Shares its ENTIRE construction with `emit_proc_value`
/// (captures, redo-wrapper loop) via `emit_proc_or_lambda_value`, differing
/// only in the two places real Ruby's own lambda semantics require: strict
/// arity (`is_lambda: true` gates a runtime `ArgumentError` check
/// `emit_proc_or_lambda_value` inserts) and folding `Signal::Return`/`Break`
/// into a normal `Ok` return instead of letting them propagate.
pub fn emit_lambda_value(
    cx: &Ctx,
    params: &Params,
    body: &[NodeId],
    method_body: bool,
) -> TokenStream {
    emit_proc_or_lambda_value(cx, params, body, true, method_body)
}
pub(crate) fn emit_proc_or_lambda_value(
    cx: &Ctx,
    params: &Params,
    body: &[NodeId],
    is_lambda: bool,
    method_body: bool,
) -> TokenStream {
    let block_caps =
        crate::codegen::captures::block_captures(cx.compiler, params, body, cx.current_class);

    let mut genuine: Vec<&String> = block_caps
        .locals
        .iter()
        .filter(|n| cx.captured_locals.contains(*n))
        .collect();
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

    // BARE block use (`yield`/`block_given?`) in this body (nested blocks
    // included) targets the LEXICALLY enclosing METHOD's block, so an ordinary
    // block/lambda clones that method's `__blk` in from the parent scope.
    // Composes through nesting (the transitive scan makes each enclosing
    // closure capture `__blk` first), and the enclosing method is guaranteed to
    // HAVE one (same scan sets its `uses_bare_block`). A METHOD-BODY lambda
    // never clones a lexical `__blk` -- its block is the call-site block, taken
    // as the closure's own third parameter below (CRuby's `invoke_bmethod`
    // specval vs the captured env, `vm.c:1786`).
    let bare_block_use = crate::analyze::scan_bare_block_use_body(&cx.compiler.hir, body);

    // An ordinary proc/lambda that declares its OWN `&block` parameter
    // (`->(&b) { b.call }`) RECEIVES the call-site block as the closure's third
    // parameter (`__blk`), so a block handed to its `#call` reaches `&b`. This
    // is distinct from bare `yield`/`block_given?`, which CAPTURES the enclosing
    // method's block instead (`blk_clone`, below) -- so a proc that takes its
    // own block param must NOT also clone in the lexical `__blk` (the param owns
    // that name here).
    let takes_own_block = params.block.is_some();
    // ... and only where a lexical `__blk` EXISTS (`cx.has_blk_binding`).
    // A top-level/class-body block can still scan as a bare block use (a
    // bare `super` forwards the caller's block, and `super` there emits
    // CRuby's runtime "super called outside of method" raise) -- there is
    // no `__blk` binding to clone at that level.
    let blk_clone = (bare_block_use && !method_body && !takes_own_block && cx.has_blk_binding)
        .then(|| quote! { let __blk = __blk.clone(); });

    // Whether a METHOD-BODY closure must name its call-site block parameter
    // `__blk` (vs `_`): needed if the body uses bare block OR declares a `&blk`
    // param, whose binding reads `__blk` (`params::emit_proc_param_bindings`).
    // A `&blk` param is a declaration, not a body use, so the bare scan misses
    // it. (Irrelevant to an ordinary block/lambda, which takes no block param.)
    let needs_blk_param = bare_block_use || params.block.is_some();

    // A block that mentions `self` (an ivar, a bare `self`, an implicit-self
    // call) does NOT capture it: it takes it as the closure's first
    // parameter, and the value below is only the DEFAULT -- the lexical self
    // that ordinary `#call`/`yield` runs under. `instance_exec` passes a
    // different one. `boxed_implicit_self` is exactly the "self here, as a
    // RubyValue" rule this needs, so it isn't re-derived. A method-body lambda
    // always takes a self parameter (the runtime install rebinds it per call),
    // so it needs the default even when the body itself never mentions `self`.
    let needs_self = block_caps.self_captured;
    // `__self_default` is the closure's stored `self_val`; every shape that
    // takes a `&self` closure parameter needs it -- a method-body lambda, a
    // self-capturing block, AND an ordinary proc/lambda promoted to the
    // block-carrying `with_self_and_block` shape because it declares a `&block`
    // param (`needs_blk_param`), even when its body never reads `self`.
    let self_default = (needs_self || method_body || takes_own_block).then(|| {
        let boxed = super::boxed_implicit_self(cx).expect("boxed_implicit_self is total");
        quote! { let __self_default = #boxed; }
    });

    // The block's OWN parameter names shadow the enclosing scope's metadata
    // for them -- see `Ctx::in_proc`.
    // Names THIS block binds (its own params or own locals) that a NESTED
    // escaping block captures -- e.g. the `m` in
    // `each { |m| define_method(m) { m } }`. They must become shared
    // `Arc<Mutex<RubyValue>>` cells so the inner closure can `Arc::clone` them,
    // exactly like a method promotes its OWN captured params (see
    // `params::emit_prologue`'s `captured_param_wraps`). Without this the inner
    // block would fresh-declare the name and read `nil`.
    let own_params = crate::codegen::captures::own_param_names(params);
    let nested_captured: std::collections::HashSet<String> =
        crate::codegen::captures::collect_escaping_captures(
            cx.compiler,
            body,
            params,
            cx.current_class,
        )
        .locals;
    // A name that is both this block's own local AND captured by a nested
    // block is cell-declared below (`nested_local_decls`) and lives in
    // `proc_cx.captured_locals`; it must therefore leave `own_only`, or the
    // own-locals prelude would ALSO fresh-declare it -- a second binding that
    // shadows the shared cell (the inner closure then reads `nil`), which the
    // prelude's own-only invariant (`hoisting.rs`) forbids outright.
    own_only.retain(|n| !nested_captured.contains(n));

    // Nested-Proc guard: names shared with the enclosing METHOD are
    // `Captured` cells, and a name a NESTED escaping block
    // captures from this one just became a cell too (`nested_captured`,
    // removed from `own_only` above) -- both compose through any nesting
    // depth. A remaining `own_only` name that IS assigned somewhere in this
    // subtree is at worst a deeper block's own fresh local (bm_ao_render's
    // `vf`, assigned two `.times` levels down: the fresh `let` declared
    // here is dead, and the assigning block re-declares its own -- the same
    // emission this shape already gets when the enclosing scope is a
    // method). Only a name NOBODY under this block assigns is left: a read
    // of the enclosing BLOCK's own plain per-invocation `let`, not a cell,
    // which a `move` closure can't share correctly (fresh-declaring it
    // would silently read `nil` where real Ruby sees the outer block's
    // value). Reject that narrow case cleanly; everything else nests fine.
    if cx.in_real_proc {
        if let Some(outer_block_local) = own_only
            .iter()
            .find(|n| !block_caps.assigned.contains(n.as_str()))
        {
            return crate::codegen::unsupported(format!(
                "a nested escaping block capturing its enclosing BLOCK's own local `{outer_block_local}` isn't supported yet (zeo limitation) -- move it to the enclosing method/top level, which makes it a shared Captured cell"
            ));
        }
    }

    let mut proc_cx = cx.in_proc(needs_self, &own_params);
    // Whether THIS closure's body has a `__blk` in scope, for nested blocks'
    // own forwarding captures: a method-body closure names its call-site
    // block param `__blk` when `needs_blk_param`; an ordinary closure has
    // one exactly when it cloned the lexical one in (or receives it for its
    // own `&block` param).
    proc_cx.has_blk_binding = if method_body {
        needs_blk_param
    } else {
        blk_clone.is_some() || takes_own_block
    };
    if !nested_captured.is_empty() {
        proc_cx
            .captured_locals
            .to_mut()
            .extend(nested_captured.iter().cloned());
    }
    // Cell-wrap this block's own PARAMS that a nested block captures (after the
    // plain param binding reads its value).
    //
    // DESTRUCTURED names are excluded, for the reason `params::emit_prologue`
    // excludes them on the method path: `emit_destructures` (inside
    // `param_bindings` below) declares each one itself via `emit_local_decl`,
    // which already yields a CELL when a nested block captures it -- so
    // wrapping here too wrapped the cell in a second cell. Ownership is split
    // exactly: this site wraps the params the Rust closure signature binds,
    // `emit_destructures` owns the ones it binds itself. `pp`'s
    // `seplist { |(member, value)| ... group { value } }` is the shape that
    // found it.
    //
    // Note this is NOT the method path's other exclusion: an ASSIGNED name
    // still needs wrapping here, because `emit_proc_param_bindings` always
    // emits a plain `let mut`, never a cell.
    let destructured: std::collections::HashSet<String> =
        params.destructured_names().into_iter().collect();
    let mut nested_param_names: Vec<&String> = own_params
        .iter()
        .filter(|n| nested_captured.contains(*n) && !destructured.contains(*n))
        .collect();
    nested_param_names.sort();
    let nested_param_wraps = nested_param_names.into_iter().map(|name| {
        let ident = safe_ident(name);
        quote! {
            let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(#ident));
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
            let #ident: ::std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                ::std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(zeo_rt::RubyValue::Nil));
        }
    });
    let own_locals_prelude =
        crate::codegen::hoisting::emit_proc_own_locals_prelude(&proc_cx, &own_only);
    let arity_check =
        is_lambda.then(|| emit_lambda_arity_check(cx, params, &format_ident!("__args")));
    let param_bindings = crate::codegen::params::emit_proc_param_bindings(
        &proc_cx,
        params,
        &format_ident!("__args"),
        is_lambda,
    );
    // NOT `hoisting::emit_hoisted_body` -- that would re-collect EVERY name
    // this block references (including the genuine captures above) and
    // declare them AGAIN, shadowing the shared `Arc::clone`s just captured
    // with brand-new empty cells. `own_locals_prelude` already handles the
    // one case that genuinely needs a fresh declaration.
    let body_tokens = crate::codegen::stmt::emit_body(&proc_cx, body, true);
    // The block's own backtrace frame, pushed per invocation: CRuby labels
    // blocks LEXICALLY -- `block in Class#m`, `block (2 levels) in ...` for
    // nesting -- with the block's own source file/line regardless of where
    // the proc is later called. Fully span-less bodies (prelude) push
    // nothing; the first LOCATED statement keeps the predicate aligned with
    // `stamp_line` (see `scope_frame_guard`).
    let frame_guard = match body
        .iter()
        .find_map(|&n| crate::codegen::source_location(cx.compiler, n))
    {
        Some((file, line)) => {
            let base = crate::codegen::enclosing_frame_label(cx);
            let label = match proc_cx.block_depth {
                1 => format!("block in {base}"),
                n => format!("block ({n} levels) in {base}"),
            };
            quote! { let __frame = zeo_rt::FrameGuard::push(#file, #label, #line); }
        }
        None => quote! {},
    };
    let redo_label = crate::codegen::loops::fresh_label(cx, "proc_redo");

    // A lambda folds `Return`/`Break` into a normal `Ok` return (it's a
    // self-contained closure boundary, like a method -- see
    // `hir::HirNode::Lambda`'s docs); an ordinary Proc lets them propagate
    // via `other => break #redo_label other` (caught, respectively, at the
    // call site that attached the block and the lexically enclosing
    // method's own boundary).
    let terminal_arm = if is_lambda {
        quote! {
            Err(zeo_rt::Signal::Return(__v)) | Err(zeo_rt::Signal::Break(__v)) => break #redo_label Ok(__v),
            other => break #redo_label other,
        }
    } else {
        quote! { other => break #redo_label other, }
    };

    // `Proc#arity`/`#lambda?`/`#curry` read these -- a Rust closure can't
    // answer them about itself (see `zeo_rt::ProcData`).
    let arity = crate::codegen::params::proc_arity(params, is_lambda);
    // `Proc#parameters` metadata, attached to the constructed proc.
    let proc_params = crate::codegen::params::proc_parameters(params, is_lambda);
    // Three shapes:
    // - `with_self_and_block` whenever the body must see a CALL-SITE block --
    //   a METHOD-BODY lambda (the runtime install rebinds the receiver per
    //   call), OR an ordinary proc/lambda that declares its own `&block` param
    //   or uses bare block (`->(&b) { b.call }.call { ... }`): the block rides
    //   in as `__blk`, the closure's third parameter, where the body's
    //   `yield`/`block_given?`/`&blk` codegen finds it. Each slot is named `_`
    //   when unused, to avoid an unused-binding warning.
    // - `with_self` when the body mentions `self` (so `instance_exec` can
    //   rebind it), and `with_meta` when it never does (nothing to rebind).
    let (ctor, closure_params, default_arg) = if method_body || takes_own_block {
        let self_p = if needs_self || method_body {
            quote! { __self: &zeo_rt::RubyValue }
        } else {
            quote! { _: &zeo_rt::RubyValue }
        };
        // Name the block slot `__blk` when the body reads it: a method body via
        // `needs_blk_param` (bare `yield` or a `&blk` param), a non-method proc
        // via its own `&block` param (`takes_own_block`). `_` otherwise.
        let blk_p = if needs_blk_param || takes_own_block {
            quote! { __blk: Option<zeo_rt::RubyValue> }
        } else {
            quote! { _: Option<zeo_rt::RubyValue> }
        };
        (
            quote! { zeo_rt::RProc::with_self_and_block },
            quote! { #self_p, __args: &[zeo_rt::RubyValue], #blk_p },
            quote! { __self_default, },
        )
    } else if needs_self {
        (
            quote! { zeo_rt::RProc::with_self },
            quote! { __self: &zeo_rt::RubyValue, __args: &[zeo_rt::RubyValue] },
            quote! { __self_default, },
        )
    } else {
        (
            quote! { zeo_rt::RProc::with_meta },
            quote! { __args: &[zeo_rt::RubyValue] },
            quote! {},
        )
    };
    quote! {
        {
            #(#capture_clones)*
            #blk_clone
            #self_default
            zeo_rt::RubyValue::Proc(#ctor(move |#closure_params| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
                #frame_guard
                let __out = #redo_label: loop {
                    let __result: Result<zeo_rt::RubyValue, zeo_rt::Signal> = (|| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
                        #arity_check
                        #own_locals_prelude
                        #param_bindings
                        #(#nested_param_wraps)*
                        #(#nested_local_decls)*
                        #body_tokens
                    })();
                    match __result {
                        Err(zeo_rt::Signal::Redo) => continue #redo_label,
                        Err(zeo_rt::Signal::Next(__v)) => break #redo_label Ok(__v),
                        #terminal_arm
                    }
                };
                // Per-invocation interruption checkpoint, on the normal EXIT
                // (not entry): iterator-driven loops (`arr.each { ... }`)
                // still hit it once per element, but a freshly started
                // Thread body -- also a proc -- always reaches its own first
                // statements (and any `begin`) before a queued kill/raise
                // can land, matching the running-target delivery real Ruby's
                // `Thread.new ...; Thread.pass; t.raise` idiom relies on.
                if __out.is_ok() { zeo_rt::check_ints()?; }
                __out
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
fn emit_lambda_arity_check(
    cx: &Ctx,
    params: &Params,
    args_ident: &proc_macro2::Ident,
) -> TokenStream {
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
            let __argc = if matches!(#args_ident.last(), Some(zeo_rt::RubyValue::Hash(_))) {
                #args_ident.len() - 1
            } else {
                #args_ident.len()
            };
        }
    } else {
        quote! { let __argc = #args_ident.len(); }
    };
    let err = crate::codegen::expr::emit_boxed_new(
        cx,
        "ArgumentError",
        vec![quote! {
            zeo_rt::RubyValue::Str(zeo_rt::string_new(format!(
                "wrong number of arguments (given {}, expected {})",
                __argc, #min_lit
            )))
        }],
    );
    quote! {
        #argc
        if #cond {
            return Err(zeo_rt::Signal::Raise(#err));
        }
    }
}
