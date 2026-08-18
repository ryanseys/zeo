//! Emits a sequence of Rust statements from an HIR body, ending in a tail
//! expression -- Ruby's own "last expression is the implicit return"
//! translates directly to a Rust block's tail-expression rule.
//!
//! `wrap_ok` distinguishes the two shapes a body can be used as: a whole
//! method/closure body (`wrap_ok: true`), whose block becomes the literal
//! body of a `-> Result<RubyValue, Signal>` function and so needs its tail
//! wrapped in `Ok(...)`; or a body spliced in as a plain `RubyValue`-typed
//! *value* inside some other expression (`wrap_ok: false` -- `if`/ternary/
//! `case` arms, loop bodies, rescue arms), which must stay a bare
//! `RubyValue` so it composes like any other `emit_expr` fragment.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use quote::quote;

use super::Ctx;
use super::expr::emit_expr;
use crate::hir::{HirNode, NodeId};
use proc_macro2::TokenStream;

pub fn emit_body(cx: &Ctx, body: &[NodeId], wrap_ok: bool) -> TokenStream {
    if body.is_empty() {
        return tail_nil(wrap_ok);
    }
    let mut prev_line = None;
    // A program with no `class << self` anywhere never groups -- and this is
    // asked once per emitted statement of every body in the compile, so the
    // whole-table test comes first.
    if cx.compiler.hir.singleton_frame_stmts.is_empty() {
        return emit_body_plain(cx, body, &mut prev_line, true, wrap_ok);
    }
    let mut out: Vec<TokenStream> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        // A run of statements one `class << self` body contributed to this
        // one runs under a frame of its own -- see `emit_singleton_frame`.
        // Every other body has no entry in the table and takes the plain
        // path unchanged.
        let origin = cx.compiler.hir.singleton_frame_stmts.get(&body[i]).copied();
        let Some(origin) = origin else {
            let tokens = emit_statement(cx, body[i], i + 1 == body.len(), wrap_ok);
            out.push(stamp_line(
                cx,
                body[i],
                &mut prev_line,
                tokens,
                i + 1 == body.len(),
            ));
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < body.len() && cx.compiler.hir.singleton_frame_stmts.get(&body[j]) == Some(&origin)
        {
            j += 1;
        }
        out.push(emit_singleton_frame(
            cx,
            origin,
            &body[i..j],
            &mut prev_line,
            j == body.len(),
            wrap_ok,
        ));
        i = j;
    }
    quote! { #(#out)* }
}

/// A `class << self` body's own backtrace frame, around the statements the
/// singleton mapping spliced into the ENCLOSING class body.
///
/// Two positions have to be right, not one. The frame itself is labelled
/// `singleton class` and tracks the group's own lines; the ENCLOSING frame
/// is left reading the `class << self` KEYWORD's line, which is what ruby
/// reports for it -- so the head stamps that line before the guard rather
/// than letting the first grouped statement stamp its own into the frame
/// underneath. `lexical_frame_label` carries the label down so a block
/// written here is `block in singleton class`.
fn emit_singleton_frame<'a>(
    cx: &Ctx<'a>,
    origin: NodeId,
    group: &[NodeId],
    prev_line: &mut Option<(&'a str, u32)>,
    is_tail: bool,
    wrap_ok: bool,
) -> TokenStream {
    let Some((file, line)) = crate::codegen::source_location(cx.compiler, origin) else {
        return emit_body_plain(cx, group, prev_line, is_tail, wrap_ok);
    };
    let head = match prev_line.as_ref() {
        Some((f, l)) if *f == file && *l == line => TokenStream::new(),
        _ => quote! { zeo_rt::set_line(#line); },
    };
    *prev_line = Some((file, line));
    let end_line = crate::codegen::source_end_line(cx.compiler, origin);
    let pooled = crate::codegen::pooled_file(file);
    let mut inner_cx = cx.clone();
    inner_cx.lexical_frame_label = Some("singleton class".to_string());
    // The group's own line tracking starts fresh inside the new frame.
    let mut inner_prev = None;
    let body = emit_body_plain(&inner_cx, group, &mut inner_prev, is_tail, false);
    let framed = quote! {
        {
            let __frame = zeo_rt::FrameGuard::push(#pooled, "singleton class", #line, #end_line);
            #body
        }
    };
    // A group holding the body's TAIL supplies its value, so the frame block
    // is an expression there and a statement everywhere else.
    match (is_tail, wrap_ok) {
        (false, _) => quote! { #head #framed; },
        (true, false) => quote! { #head #framed },
        (true, true) => quote! { #head Ok(#framed) },
    }
}

/// `emit_body`'s statement loop, without the singleton grouping -- so a group
/// can reuse it without re-detecting itself.
fn emit_body_plain<'a>(
    cx: &Ctx<'a>,
    body: &[NodeId],
    prev_line: &mut Option<(&'a str, u32)>,
    tail_is_value: bool,
    wrap_ok: bool,
) -> TokenStream {
    if body.is_empty() {
        return tail_nil(wrap_ok);
    }
    let last = body.len() - 1;
    let stmts = body
        .iter()
        .enumerate()
        .map(|(i, &stmt)| {
            let is_tail = tail_is_value && i == last;
            let tokens = emit_statement(cx, stmt, is_tail, wrap_ok);
            stamp_line(cx, stmt, prev_line, tokens, is_tail)
        })
        .collect::<Vec<_>>();
    quote! { #(#stmts)* }
}

/// Prepend a `zeo_rt::set_line` stamp when this statement's source location
/// differs from the previous statement's -- what keeps the current
/// backtrace frame's line tracking execution (CRuby's per-frame PC line,
/// at statement granularity). A TAIL expression keeps its value by
/// wrapping in a block. Synthetic statements stamp nothing.
///
/// Under coverage (the program required `coverage`), each stamp also emits
/// a `cov_line` hit beside it and records the line as coverable; and a
/// mid-stream FILE change -- which only the top-level walk ever has, at the
/// point where a spliced `require`'s statements begin -- emits the
/// `cov_file_loaded` mark that makes the file reportable iff measurement is
/// set up when its top level runs (CRuby's own inclusion rule; the entry
/// file never marks, so it is never reported, also CRuby's rule).
fn stamp_line<'a>(
    cx: &Ctx<'a>,
    stmt: NodeId,
    prev_line: &mut Option<(&'a str, u32)>,
    tokens: TokenStream,
    is_tail: bool,
) -> TokenStream {
    // A body with no frame of its own has nowhere to stamp: the write would
    // land in the CALLER's frame and never be restored, so an unrelated raise
    // later in the caller reported this body's line. See `Ctx::frameless`.
    if cx.frameless {
        return tokens;
    }
    let Some((file, line)) = crate::codegen::source_location(cx.compiler, stmt) else {
        return tokens;
    };
    if prev_line
        .as_ref()
        .is_some_and(|(f, l)| *f == file && *l == line)
    {
        return tokens;
    }
    let mut cov = TokenStream::new();
    if crate::codegen::coverage_active() {
        let entering_spliced_file = prev_line.as_ref().is_some_and(|(f, _)| *f != file)
            && cx
                .compiler
                .hir
                .files
                .first()
                .is_none_or(|f0| f0.name != file);
        let pooled = crate::codegen::pooled_file(file);
        if entering_spliced_file {
            cov.extend(quote! { zeo_rt::cov_file_loaded(#pooled); });
        }
        crate::codegen::coverage_record_stmt(file, line);
        cov.extend(quote! { zeo_rt::cov_line(#pooled, #line); });
    }
    *prev_line = Some((file, line));
    if is_tail {
        quote! { { zeo_rt::set_line(#line); #cov #tokens } }
    } else {
        quote! { zeo_rt::set_line(#line); #cov #tokens }
    }
}

/// `emit_body` for a body whose VALUE is discarded (loop bodies): every
/// statement -- the last included -- emits in statement position, so a tail
/// assignment skips its read-back and a pure tail vanishes, instead of
/// leaving an unused `Clone::clone(&x)` behind for rustc to warn about.
pub fn emit_body_discard(cx: &Ctx, body: &[NodeId]) -> TokenStream {
    let mut prev_line = None;
    let stmts = body
        .iter()
        .map(|&stmt| {
            let tokens = emit_statement(cx, stmt, false, false);
            stamp_line(cx, stmt, &mut prev_line, tokens, false)
        })
        .collect::<Vec<_>>();
    quote! { #(#stmts)* }
}

/// `emit_body(.., false)` whose VALUE is always a boxed `RubyValue` -- for
/// alternative bodies that must agree on one Rust type (if/ternary/case/
/// pattern arms; `infer` types those expressions `Poly`, so consumers
/// always expect `RubyValue`). Only an Object-typed tail expression needs
/// the boxing; every other tail already produces `RubyValue`.
pub fn emit_body_boxed(cx: &Ctx, body: &[NodeId]) -> TokenStream {
    if body.is_empty() {
        return tail_nil(false);
    }
    let (init, last) = body.split_at(body.len() - 1);
    let mut prev_line = None;
    let init_stmts = init
        .iter()
        .map(|&s| {
            let tokens = emit_statement(cx, s, false, false);
            stamp_line(cx, s, &mut prev_line, tokens, false)
        })
        .collect::<Vec<_>>();
    let tail_id = last[0];
    let tail = emit_statement(cx, tail_id, true, false);
    // A tail assignment's own arm already boxed its value to `RubyValue`
    // (see `emit_statement`); every other tail still needs Object-typed
    // boxing here.
    let tail = match &cx.compiler.hir[tail_id] {
        HirNode::LocalWrite(..) | HirNode::MultiWrite { .. } => tail,
        _ => super::expr::box_if_object_typed(cx, tail_id, tail),
    };
    let tail = stamp_line(cx, tail_id, &mut prev_line, tail, true);
    quote! { #(#init_stmts)* #tail }
}

/// A statement-position `if`: no value, arms in discard mode. Mirrors
/// `expr::emit_if`'s dead-branch elision (a provably-false `defined?` guard's
/// branch is never emitted) without its arm boxing -- there is nothing to box.
fn emit_if_statement(
    cx: &Ctx,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
) -> TokenStream {
    match super::constfold::static_cond(cx, cond) {
        Some(true) => return emit_body_discard(cx, then_body),
        Some(false) => return emit_body_discard(cx, else_body),
        None => {}
    }
    let cond_expr = super::expr::box_if_object_typed(cx, cond, emit_expr(cx, cond));
    let then_stmts = emit_body_discard(cx, then_body);
    if else_body.is_empty() {
        return quote! { if (#cond_expr).truthy() { #then_stmts } };
    }
    let else_stmts = emit_body_discard(cx, else_body);
    quote! {
        if (#cond_expr).truthy() { #then_stmts } else { #else_stmts }
    }
}

fn tail_nil(wrap_ok: bool) -> TokenStream {
    if wrap_ok {
        quote! { Ok(zeo_rt::RubyValue::Nil) }
    } else {
        quote! { zeo_rt::RubyValue::Nil }
    }
}

/// One statement (or the tail expression) of a body. `LocalWrite`/
/// `MultiWrite` need special handling: they compile to a plain Rust
/// reassignment (`x = v;`, not `let x = v;` -- see `codegen::hoisting`'s
/// docs for why a fresh `let` here would silently fail to persist mutations
/// across loop iterations), whose Rust type is `()`, not `RubyValue`. In
/// tail position that has no value of its own to return, so a trailing nil
/// follows it (see `emit_expr`'s `LocalWrite`/`MultiWrite` arms for the
/// sub-expression case, which does return the assigned value, matching
/// Ruby's real assignment-as-expression semantics).
/// A `Seq` whose last element reads back a COMPILER-introduced local, and
/// so carries a value only an expression position would use -- the shape
/// `lower::assign::push_assignment_call` builds. Answers the statements
/// worth emitting without it.
fn statement_without_value_readback(cx: &Ctx, stmt: NodeId) -> Option<Vec<NodeId>> {
    let HirNode::Seq(body) = &cx.compiler.hir[stmt] else {
        return None;
    };
    let (&last, rest) = body.split_last()?;
    match &cx.compiler.hir[last] {
        HirNode::LocalRead(name) if crate::hir::is_internal_local(name) => Some(rest.to_vec()),
        // A receiver-binding desugar nests the setter's own Seq as its tail
        // (`obj.attr += 1` -> Seq([recv bind, Seq([write call, read])])), so
        // the droppable read sits one level down. Without the recursion the
        // read-back survived in a discarded position -- `Clone::clone` is
        // `#[must_use]`, so rustc warned on the generated program.
        HirNode::Seq(_) => {
            let inner = statement_without_value_readback(cx, last)?;
            Some(rest.iter().copied().chain(inner).collect())
        }
        _ => None,
    }
}

fn emit_statement(cx: &Ctx, stmt: NodeId, is_tail: bool, wrap_ok: bool) -> TokenStream {
    if let HirNode::ClassDef { .. } = &cx.compiler.hir[stmt] {
        // A class/module definition SITE: its body statements run right
        // here, in document order (real Ruby executes a class body where
        // it appears, re-running each reopen) -- see
        // `Compiler::class_body_sites`. Dispatch-table registration stays
        // hoisted in `fn main()`; only the body's execution moves.
        //
        // In tail position the definition IS the enclosing scope's value, and
        // ruby's is the body's own last value -- so the site keeps it there
        // and drops it everywhere else.
        return if is_tail {
            let value = crate::codegen::emit_class_def_marker(
                cx,
                stmt,
                crate::codegen::BodyValue::KeepOrNil,
            );
            match wrap_ok {
                true => quote! { Ok(#value) },
                false => value,
            }
        } else {
            crate::codegen::emit_class_def_marker(cx, stmt, crate::codegen::BodyValue::Discard)
        };
    }
    if matches!(&cx.compiler.hir[stmt], HirNode::Refine { .. })
        && cx.compiler.refinement_marker_registered(stmt)
    {
        // A `refine` marker analyze already recorded runs nothing where it was
        // written (see `emit_expr`'s arm). As a non-tail statement it emits
        // literally nothing, so rustc gets no bare `RubyValue::Nil` path
        // statement to warn about.
        return if is_tail {
            tail_nil(wrap_ok)
        } else {
            quote! {}
        };
    }
    if let HirNode::MethodVisibility { name, visibility } = &cx.compiler.hir[stmt] {
        // Only a reopen's re-mark of the class's OWN method joins a site's
        // statements (see analyze's `MethodVisibility` arm) -- applied here,
        // at its document position: ruby's program-order visibility.
        let cid = cx
            .defining_class
            .expect("MethodVisibility sits in a class body")
            .0;
        let vis = match visibility {
            crate::hir::Visibility::Public => quote! { Public },
            crate::hir::Visibility::Private => quote! { Private },
            crate::hir::Visibility::Protected => quote! { Protected },
        };
        let apply = quote! {
            zeo_rt::runtime_set_visibility(
                zeo_rt::ClassId(#cid),
                &[zeo_rt::RubyValue::Symbol(zeo_rt::Symbol::intern(#name))],
                zeo_rt::MethodVisibility::#vis,
            )?;
        };
        return if is_tail {
            let nil = tail_nil(wrap_ok);
            quote! { #apply #nil }
        } else {
            apply
        };
    }
    if let HirNode::ClassMethodVisibility { name, visibility } = &cx.compiler.hir[stmt] {
        // A unit body's `private_class_method :x` applies where it stands (see
        // analyze's `ClassMethodVisibility` arm: only unit walks route here;
        // eager bodies keep the start-of-program override row).
        let cid = cx
            .defining_class
            .expect("ClassMethodVisibility sits in a class body")
            .0;
        let private = matches!(visibility, crate::hir::Visibility::Private);
        let apply = quote! {
            zeo_rt::runtime_class_method_visibility(
                zeo_rt::ClassId(#cid),
                &[zeo_rt::RubyValue::Symbol(zeo_rt::Symbol::intern(#name))],
                #private,
            )?;
        };
        return if is_tail {
            let nil = tail_nil(wrap_ok);
            quote! { #apply #nil }
        } else {
            apply
        };
    }
    if let HirNode::MethodRedefine { class, name, scope } = &cx.compiler.hir[stmt] {
        // A redefinition applied at its document position -- see
        // `analyze::redefs`. The install replaces the overlay body; the
        // definition's own `method_added` report is a separate `DefHook`
        // spliced right after this statement.
        let id = *class;
        let tramp = crate::codegen::redef_trampoline(
            cx.compiler,
            crate::compiler::ClassId(*class),
            crate::compiler::ScopeId(*scope),
        );
        let apply = quote! {
            zeo_rt::runtime_replace_method(
                zeo_rt::ClassId(#id),
                zeo_rt::Symbol::intern(#name),
                #tramp,
            );
        };
        return if is_tail {
            let nil = tail_nil(wrap_ok);
            quote! { #apply #nil }
        } else {
            apply
        };
    }
    if matches!(
        &cx.compiler.hir[stmt],
        HirNode::LocalWrite(..) | HirNode::MultiWrite { .. }
    ) {
        // In TAIL position an assignment evaluates to the assigned value,
        // exactly like Ruby's real "assignment-as-expression" semantics
        // (`def inc(v); v += 1; end` returns the incremented value): reuse
        // `emit_expr`'s write-then-read form, boxed to `RubyValue`. As a
        // NON-tail statement it compiles to a plain Rust reassignment
        // (`x = v;`, not a value-returning block) so the mutation persists
        // across loop iterations -- see `codegen::hoisting`'s docs.
        if is_tail {
            // Box the write-then-read value to `RubyValue`. For a `LocalWrite`
            // this keys on the LOCAL's storage (a `nil | Foo` union local
            // already reads back as `RubyValue`, so must not be re-boxed --
            // see `box_tail_local_write`); a `MultiWrite` yields its RHS array,
            // already a `RubyValue`.
            let value = match &cx.compiler.hir[stmt] {
                HirNode::LocalWrite(name, _) => {
                    super::expr::box_tail_local_write(cx, name, emit_expr(cx, stmt))
                }
                _ => emit_expr(cx, stmt),
            };
            return if wrap_ok {
                quote! { Ok(#value) }
            } else {
                value
            };
        }
        if let HirNode::LocalWrite(name, value) = &cx.compiler.hir[stmt] {
            let v = emit_expr(cx, *value);
            // Box an `Object`-typed RHS when `name`'s OWN storage disagrees
            // See `emit_expr::box_for_local_storage`'s docs.
            let v = super::expr::box_for_local_storage(cx, name, *value, v);
            // `emit_local_write` picks the right shape (plain reassignment,
            // shadowing `let`, or a `RefCell` store) for whichever storage
            // class `name` has -- see `codegen::hoisting::LocalStorage`'s docs.
            let write = super::hoisting::emit_local_write(cx, name, v);
            quote! { #write }
        } else {
            let HirNode::MultiWrite { targets, value } = &cx.compiler.hir[stmt] else {
                unreachable!()
            };
            // Same reasoning as `LocalWrite` above: every target that's a
            // plain `Local` reassigns the SAME already-hoisted identifier as
            // before, so wrapping the destructuring scratch locals
            // (`__elems`/`__before`/`__splat`/`__after`) in their own nested
            // block (see `codegen::loops::emit_multi_target_group`) doesn't
            // affect their visibility to LATER statements in this body at all.
            let write = super::loops::emit_multi_write(cx, targets, *value);
            quote! { #write }
        }
    } else {
        // A statement-position assignment needs no read-back: Ruby's
        // "assignment evaluates to its value" matters only in expression
        // position, and the dropped `Clone::clone(&x)` tail the expression
        // form appends was pure refcount noise (and a rustc warning) in
        // every loop body.
        if !is_tail {
            if let HirNode::LocalWrite(name, value) = &cx.compiler.hir[stmt] {
                let v = emit_expr(cx, *value);
                let v = super::expr::box_for_local_storage(cx, name, *value, v);
                let write = super::hoisting::emit_local_write(cx, name, quote! { __v });
                // Unannotated: `box_for_local_storage` leaves a `Shadowed`
                // local's RHS as the bare `Arc<Concrete>` its slot holds.
                return quote! { { let __v = #v; #write } };
            }
            // The same reasoning one step out: a setter call reached from
            // assignment syntax reads its captured value back only so the
            // EXPRESSION has ruby's value (`lower::assign::
            // push_assignment_call`). Nothing consumes it here.
            if let Some(rest) = statement_without_value_readback(cx, stmt) {
                let stmts = rest
                    .iter()
                    .map(|&n| emit_statement(cx, n, false, false))
                    .collect::<Vec<_>>();
                return quote! { #(#stmts)* };
            }
            // A statement-position `if` discards its value, so its ARMS emit in
            // discard mode too -- through the expression form, an arm tail
            // built by `push_assignment_call` kept its read-back and left an
            // unused `Clone::clone(&__asgn..)` for rustc's `unused_must_use`
            // (the shape feature-unit bodies hit at rspec scale).
            if let HirNode::If {
                cond,
                then_body,
                else_body,
            } = &cx.compiler.hir[stmt]
            {
                let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
                return emit_if_statement(cx, cond, &then_body, &else_body);
            }
        }
        let e = emit_expr(cx, stmt);
        if is_tail {
            // `raise`/a bare `return`/`retry`/`break`/`next`/`redo` all
            // compile to a literal diverging Rust statement -- `return ...;`
            // (`codegen::expr::emit_raise`/`HirNode::Return`'s docs,
            // `codegen::exceptions::emit_retry`), or a labeled `break`/
            // `continue`/`return Err(Signal::..)` (`codegen::loops`). Their
            // type (`!`) already unifies with anything, so wrapping in
            // `Ok(...)` here would build an `Ok(break ...)`/`Ok(return ...)`
            // that can never actually construct its `Ok` (the jump always
            // exits first). Harmless in principle (`!` coerces fine either
            // way) but `rustc` flags the `Ok(...)` call itself as unreachable
            // -- skip the wrap for exactly these diverging shapes rather than
            // accept the warning. (A tail `break`/`next`/`redo` reaches here
            // once its clause is inside a `begin`'s closure -- see
            // `codegen::exceptions`.)
            if wrap_ok && !is_diverging_tail(&cx.compiler.hir[stmt]) {
                // Box a bare tail `New`/`SelfRef`/`Shadowed`-local-read into
                // `RubyValue::Object` before wrapping -- this whole body's
                // enclosing function returns `Result<RubyValue, Signal>`,
                // but those three shapes emit an unboxed `Arc<Concrete>`
                // (see `codegen::expr::box_for_tail_return`'s docs).
                let e = super::expr::box_for_tail_return(cx, stmt, e);
                quote! { Ok(#e) }
            } else {
                e
            }
        } else if is_pure_statement(cx, &cx.compiler.hir[stmt]) {
            // A statement-position expression with no effect (a bare local
            // read, a literal, `self`) emits nothing: the old
            // `Clone::clone(&x);` statements did real refcount work and drew
            // rustc's unused-`clone` warnings into every generated program.
            TokenStream::new()
        } else {
            quote! { #e; }
        }
    }
}

/// Statement-position shapes Ruby evaluates purely for their (absent) side
/// effects -- safe to emit nothing for. Deliberately minimal: anything with
/// interpolation, a call, or a fallible read stays emitted.
fn is_pure_statement(cx: &Ctx, node: &HirNode) -> bool {
    // `include M` compiles to `M.included(self)` -- and to NOTHING when the
    // module defines no such hook, which is almost always. See
    // `expr::mixin_hook_runs`.
    if matches!(
        node,
        HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::ClassMethodPrepend(_)
    ) {
        return !super::expr::mixin_hook_runs(cx, node);
    }
    matches!(
        node,
        HirNode::IntegerLit(_)
            | HirNode::FloatLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::NilLit
            | HirNode::BoolLit(_)
            | HirNode::LocalRead(_)
            | HirNode::SelfRef
    )
}

fn is_diverging_tail(node: &HirNode) -> bool {
    matches!(
        node,
        HirNode::Raise(..)
            | HirNode::Return(_)
            | HirNode::Retry
            | HirNode::Break(_)
            | HirNode::Next(_)
            | HirNode::Redo
    )
}
