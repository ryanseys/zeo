//! Hoists every local variable assigned anywhere in a method/top-level
//! scope to a single, mutable, nil-defaulted declaration at the very top of
//! that scope -- exactly matching Ruby's own real local-variable semantics
//! (a name is bound for the rest of its enclosing scope the moment it's
//! syntactically assigned ANYWHERE within it, defaulting to `nil` if that
//! particular assignment never actually executes -- e.g. `while false; x =
//! 1; end; puts x` prints `nil`, not a `NameError`).
//!
//! This isn't fidelity for its own sake: without it, a native Rust `loop {
//! }` body's `let x = ...;` re-declares `x` FRESH every iteration --
//! shadowing (then immediately discarding, at the end of that iteration)
//! whatever the previous iteration computed -- so a `while i < 10; i += 1;
//! end`-style loop's own exit condition would never observe the mutation
//! and would loop forever. Hoisting `x` ONCE, before the loop, and having
//! every `LocalWrite`/`MultiWrite`/`For` target compile to a plain
//! REASSIGNMENT (`x = ...;`, not `let x = ...;`) instead, is what lets a
//! mutation survive across iterations (see `stmt.rs`/`codegen::loops` for
//! the reassignment side of this fix).
//!
//! A `Block`'s own declared parameters are the one thing NOT collected here
//! -- a block genuinely introduces its own local scope for its formal
//! parameters in real Ruby (`3.times { |i| ... }`'s `i` never leaks out,
//! unlike `for`'s index variable), so those stay ordinary, freshly-scoped
//! `let` bindings at their one `codegen::call`/`codegen::loops` emission
//! site, untouched by this pass. A method's own parameters are similarly
//! left alone (bound via the Rust function signature, not this prelude) --
//! reassigning a parameter itself is a narrower, pre-existing gap this
//! doesn't attempt to fix (nothing exercises it yet).

use super::ident::safe_ident;
use super::stmt::emit_body;
use super::Ctx;
use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use crate::types::TyKind;
use proc_macro2::TokenStream;
use quote::quote;

/// The three ways a local/parameter name's storage can be emitted -- see
/// each variant's docs. Replaces a 2-way `is_hoisted` bool now that Phase 6
/// adds a real, escaping capture case (see `codegen::captures`); every read/
/// write site should go through `emit_local_read`/`emit_local_write` below
/// rather than matching this directly, so the 3-way logic lives in ONE
/// place instead of being re-derived at each of the (now 4+) sites a local
/// is read or written.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum LocalStorage {
    /// A concrete class-instance local (`TyKind::Object`, e.g. `x =
    /// SomeClass.new`) -- Rust already infers this as an unboxed
    /// `Arc<ConcreteClass>` (not `RubyValue`) directly from its own
    /// assignment expression, and `ruby_class!`'s whole "Path 1 static
    /// dispatch on a concrete Rust type" design depends on that unboxing --
    /// uniformly hoisting every local as `RubyValue` would break it
    /// (confirmed the hard way: it broke safe-navigation's
    /// `Box.new(...)`-assigned-to-a-local case). Takes priority over
    /// `Captured` even if some escaping block also references this name:
    /// object *identity* (and therefore ivar mutation through it) already
    /// flows correctly via the shared `Arc` with no cell-wrapping needed --
    /// only *reassigning the local's own binding* from inside a closure
    /// wouldn't propagate back out, the SAME pre-existing, narrow,
    /// documented gap as reassigning an object-typed local inside a native
    /// loop (nothing exercises either case).
    Shadowed,
    /// A plain hoisted `let mut ...: RubyValue = Nil;` declaration at the
    /// top of the scope, reassigned in place -- the common case.
    Hoisted,
    /// Captured by some escaping block (see `codegen::captures`) -- an
    /// `Arc<parking_lot::Mutex<RubyValue>>` cell (Part 9: `Arc`/`Mutex`, not
    /// `Rc`/`RefCell`, so a captured local is genuinely `Send + Sync` if a
    /// `Thread`/`Ractor` ever needs it to be), declared once at the top of
    /// the scope and shared (via `Arc::clone`) with every escaping closure
    /// that references this name, so a mutation from inside the closure is
    /// visible to the enclosing scope and vice versa (real Ruby closure
    /// semantics, not a snapshot).
    Captured,
}

pub fn local_storage(cx: &Ctx, name: &str) -> LocalStorage {
    if matches!(cx.local_types.get(name), Some(TyKind::Object(_))) {
        LocalStorage::Shadowed
    } else if cx.captured_locals.contains(name) {
        LocalStorage::Captured
    } else {
        LocalStorage::Hoisted
    }
}

/// A local-variable READ, e.g. a bare `x` -- the one place that decides
/// between `x.clone()` (`Hoisted`/`Shadowed`) and `x.lock().clone()`
/// (`Captured`).
pub fn emit_local_read(cx: &Ctx, name: &str) -> TokenStream {
    let ident = safe_ident(name);
    match local_storage(cx, name) {
        LocalStorage::Captured => quote! { #ident.lock().clone() },
        LocalStorage::Hoisted | LocalStorage::Shadowed => quote! { #ident.clone() },
    }
}

/// A local-variable WRITE (assignment statement), given the already-emitted
/// value expression -- the one place that decides between a plain
/// reassignment (`Hoisted`), a fresh shadowing `let` (`Shadowed`), and a
/// `Mutex` store (`Captured`). Ends in `;` (a statement, not an expression)
/// -- callers needing the assigned value back (an assignment used as a
/// sub-expression) read it again afterward via `emit_local_read`.
pub fn emit_local_write(cx: &Ctx, name: &str, value: TokenStream) -> TokenStream {
    let ident = safe_ident(name);
    match local_storage(cx, name) {
        // `value` is bound to a temporary FIRST, then the store happens as
        // its own statement -- confirmed the hard way (originally against
        // `RefCell`, and still exactly as true against `parking_lot::Mutex`,
        // Part 9 -- if anything MORE important now): `*#ident.lock() =
        // #value;` evaluates the LHS place expression (calling `lock()`,
        // acquiring the guard) before evaluating `value`, so an RHS that
        // itself reads this SAME captured name (e.g. `total += n`, i.e.
        // `total = total + n`) would call `.lock()` again while the write
        // guard below is already held. `RefCell` turned this into a clean
        // "already borrowed" panic; a non-reentrant `Mutex` instead HANGS
        // FOREVER (no error at all) -- binding to a temp first avoids ever
        // holding a guard across a nested lock attempt.
        LocalStorage::Captured => quote! { { let __cap_v = #value; *#ident.lock() = __cap_v; } },
        LocalStorage::Hoisted => quote! { #ident = #value; },
        LocalStorage::Shadowed => quote! { let #ident = #value; },
    }
}

/// Mirrors `analyze::collect_ivars`'s traversal shape (recursing into every
/// sub-expression position a `LocalWrite` could appear in), but collects
/// local-variable names instead of ivar names, and additionally descends
/// into loop bodies and `MultiWrite`/`For` targets. `pub(super)`: also used
/// by `codegen::captures` to compute which names are genuinely shared with
/// an escaping block (as opposed to owned only by that block -- see the
/// `Call` arm's docs below).
pub(super) fn collect_locals(compiler: &Compiler, id: NodeId, out: &mut Vec<String>) {
    match &compiler.hir[id] {
        HirNode::LocalWrite(name, value) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
            collect_locals(compiler, *value, out);
        }
        HirNode::IvarWrite(_, value) | HirNode::ClassVarWrite(_, value) => {
            collect_locals(compiler, *value, out)
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            collect_locals(compiler, *l, out);
            collect_locals(compiler, *r, out);
        }
        HirNode::Defined(v) => collect_locals(compiler, *v, out),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_locals(compiler, *cond, out);
            for &n in then_body {
                collect_locals(compiler, n, out);
            }
            for &n in else_body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            if let Some(s) = subject {
                collect_locals(compiler, *s, out);
            }
            for (values, body) in arms {
                for &v in values {
                    collect_locals(compiler, v, out);
                }
                for &n in body {
                    collect_locals(compiler, n, out);
                }
            }
            for &n in else_body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::While { cond, body, .. } => {
            collect_locals(compiler, *cond, out);
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::Loop { body } => {
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::For { var, iterable, body } => {
            if !out.contains(var) {
                out.push(var.clone());
            }
            collect_locals(compiler, *iterable, out);
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                collect_locals(compiler, *v, out);
            }
        }
        HirNode::Redo => {}
        HirNode::MultiWrite {
            before,
            splat,
            after,
            value,
        } => {
            for name in before.iter().chain(splat.iter()).chain(after.iter()) {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            collect_locals(compiler, *value, out);
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
            if let Some(r) = receiver {
                collect_locals(compiler, *r, out);
            }
            for &a in args {
                collect_locals(compiler, a, out);
            }
            for pair in kwargs {
                collect_locals(compiler, pair.0, out);
                collect_locals(compiler, pair.1, out);
            }
            if let Some(b) = block {
                // An ESCAPING block (anything but the `.times` inline fast
                // path) is a genuinely separate Ruby scope now (Phase 6) --
                // a local first introduced INSIDE one is fresh per
                // invocation (confirmed against real Ruby: a Proc's own
                // internal local resets on every separate `.call()`, it
                // does NOT persist like a captured one), so it must NOT be
                // hoisted into THIS (enclosing) scope's prelude at all --
                // `codegen::captures::block_captures`/`emit_proc_own_locals_prelude`
                // give it its own fresh declaration INSIDE the closure
                // instead. Only a name genuinely shared with code outside
                // the block (which this same traversal will still find,
                // since it walks the rest of the method) ends up hoisted
                // here. `.times` stays inline (unchanged): its block is
                // spliced directly into whichever Rust scope encloses it,
                // so its locals still need to be part of THAT hoisting pass.
                if super::call::is_times_fast_path(compiler, *receiver, name, kwargs.is_empty()) {
                    collect_locals(compiler, *b, out);
                }
            }
            if let Some(b) = block_arg {
                collect_locals(compiler, *b, out);
            }
        }
        HirNode::New { args, .. } | HirNode::SuperCall { args } => {
            for &a in args {
                collect_locals(compiler, a, out);
            }
        }
        HirNode::Block { body, .. } => {
            // Params intentionally NOT collected -- see module docs.
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                collect_locals(compiler, *n, out);
            }
        }
        HirNode::HashLit(pairs) => {
            for pair in pairs {
                collect_locals(compiler, pair.0, out);
                collect_locals(compiler, pair.1, out);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
            if let Some(s) = start {
                collect_locals(compiler, *s, out);
            }
            if let Some(e) = end {
                collect_locals(compiler, *e, out);
            }
        }
        HirNode::StringLit(parts) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    collect_locals(compiler, *n, out);
                }
            }
        }
        HirNode::Eval(body) => {
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::Yield(args) | HirNode::Raise(args) => {
            for &a in args {
                collect_locals(compiler, a, out);
            }
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            collect_locals(compiler, *subject, out);
            for arm in arms {
                // A pattern's bound names leak into the enclosing METHOD
                // scope exactly like an `if`/`case` branch's locals do (no
                // new Ruby scope) -- collected here so `emit_hoisted_body`'s
                // prelude declares them, same as `MultiWrite`'s targets just
                // above.
                arm.pattern.for_each_bound_name(&mut |n| {
                    if !out.contains(&n.to_string()) {
                        out.push(n.to_string());
                    }
                });
                arm.pattern.for_each_node(&mut |n| collect_locals(compiler, n, out));
                if let Some((g, _)) = arm.guard {
                    collect_locals(compiler, g, out);
                }
                for &n in &arm.body {
                    collect_locals(compiler, n, out);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    collect_locals(compiler, n, out);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            collect_locals(compiler, *subject, out);
            pattern.for_each_bound_name(&mut |n| {
                if !out.contains(&n.to_string()) {
                    out.push(n.to_string());
                }
            });
            pattern.for_each_node(&mut |n| collect_locals(compiler, n, out));
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            for &n in body {
                collect_locals(compiler, n, out);
            }
            for r in rescues {
                // A rescue binding (`=> e`) leaks into the enclosing METHOD
                // scope exactly like a `case/in` pattern's bound names do --
                // same treatment as that arm just above.
                if let Some(name) = &r.binding {
                    if !out.contains(name) {
                        out.push(name.clone());
                    }
                }
                for &n in &r.body {
                    collect_locals(compiler, n, out);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    collect_locals(compiler, n, out);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    collect_locals(compiler, n, out);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::SelfRef
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::BlockGiven
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => {}
    }
}

/// The hoisting-aware replacement for `stmt::emit_body`, used ONLY at the
/// top of a genuinely fresh Ruby local-variable scope: a method body, the
/// top-level `main` body, or a `super`-inlined parent method body. NOT used
/// at nested `emit_body` calls (an `if`/`case`/loop body shares its
/// enclosing scope's locals -- re-hoisting there would just reintroduce the
/// same shadowing bug this exists to fix, one level down).
pub fn emit_hoisted_body(cx: &Ctx, body: &[NodeId], wrap_ok: bool) -> TokenStream {
    emit_hoisted_body_with_extra_roots(cx, body, &[], wrap_ok)
}

/// Same as `emit_hoisted_body`, but additionally scans `extra_roots` (a
/// method's own `Params::default_ids()` -- see `hir::Params`'s docs) for
/// names to declare in the hoisting prelude. A default's own
/// codegen still happens separately, inside `codegen::params::emit_prologue`
/// -- `extra_roots` here only feeds the NAME-COLLECTION pass, so a local a
/// default assigns (e.g. `def f(x: (y = 1; y))`) gets hoisted correctly too.
pub fn emit_hoisted_body_with_extra_roots(
    cx: &Ctx,
    body: &[NodeId],
    extra_roots: &[NodeId],
    wrap_ok: bool,
) -> TokenStream {
    let mut names = Vec::new();
    for &n in body {
        collect_locals(cx.compiler, n, &mut names);
    }
    for &n in extra_roots {
        collect_locals(cx.compiler, n, &mut names);
    }
    let decls = names.iter().filter_map(|n| {
        let ident = safe_ident(n);
        match local_storage(cx, n) {
            // `#[allow(unused_assignments)]`: the `Nil` default is
            // frequently overwritten before ever being read (e.g. a local's
            // very first assignment happens unconditionally right after
            // this) -- that's the intended, Ruby-faithful shape, not a
            // mistake to warn about.
            LocalStorage::Hoisted => Some(quote! {
                #[allow(unused_assignments)]
                let mut #ident: spinel_rt::RubyValue = spinel_rt::RubyValue::Nil;
            }),
            LocalStorage::Captured => Some(quote! {
                let #ident: std::sync::Arc<spinel_rt::parking_lot::Mutex<spinel_rt::RubyValue>> =
                    std::sync::Arc::new(spinel_rt::parking_lot::Mutex::new(spinel_rt::RubyValue::Nil));
            }),
            LocalStorage::Shadowed => None,
        }
    });
    let inner = emit_body(cx, body, wrap_ok);
    quote! { #(#decls)* #inner }
}

/// The escaping-block counterpart to `emit_hoisted_body`'s declaration
/// step -- used by `codegen::call`'s Proc-construction site for names an
/// escaping block references that AREN'T genuinely shared with its
/// enclosing scope (see `hoisting::collect_locals`'s `Call` arm and
/// `codegen::captures::block_captures`'s docs: these are fresh, block-owned
/// locals, confirmed against real Ruby to reset on every separate `.call()`
/// -- not `Captured` cells, and declared HERE, inside the closure, not in
/// the enclosing method's own prelude). Only `Hoisted`-storage names need an
/// explicit declaration at all: an Object-typed own-only name still just
/// gets its natural shadowing `let` at first assignment, same as anywhere
/// else in this codebase (see `LocalStorage::Shadowed`'s docs).
pub fn emit_proc_own_locals_prelude(cx: &Ctx, names: &std::collections::HashSet<String>) -> TokenStream {
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort();
    let decls = sorted.iter().filter_map(|n| {
        let ident = safe_ident(n);
        match local_storage(cx, n) {
            LocalStorage::Hoisted => Some(quote! {
                #[allow(unused_assignments)]
                let mut #ident: spinel_rt::RubyValue = spinel_rt::RubyValue::Nil;
            }),
            LocalStorage::Captured => unreachable!(
                "an own-only name is by definition not in captured_locals (see call.rs's split)"
            ),
            LocalStorage::Shadowed => None,
        }
    });
    quote! { #(#decls)* }
}
