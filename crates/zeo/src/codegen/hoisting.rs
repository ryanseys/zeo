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
//! site, untouched by this pass. A method's own parameters are bound via
//! the Rust function signature/`params::emit_prologue`, not this prelude
//! -- but a REASSIGNED parameter is rebound here from that existing value
//! rather than nil-shadowed (see
//! `emit_hoisted_body_with_extra_roots`'s `param_names` docs).

use super::Ctx;
use super::ident::safe_ident;
use super::stmt::emit_body;
use crate::compiler::FSet;
use crate::hir::NodeId;
use crate::types::TyKind;
use proc_macro2::TokenStream;
use quote::quote;

/// The three ways a local/parameter name's storage can be emitted -- see
/// each variant's docs. A 3-way enum, not a bool, because a real
/// escaping capture case exists (see `codegen::captures`); every read/
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
    /// `Arc<parking_lot::Mutex<RubyValue>>` cell (`Arc`/`Mutex`, not
    /// `Rc`/`RefCell`, so a captured local is genuinely `Send + Sync`),
    /// declared once at the top of
    /// the scope and shared (via `Arc::clone`) with every escaping closure
    /// that references this name, so a mutation from inside the closure is
    /// visible to the enclosing scope and vice versa (real Ruby closure
    /// semantics, not a snapshot).
    Captured,
}

pub fn local_storage(cx: &Ctx, name: &str) -> LocalStorage {
    let object_typed = matches!(cx.local_types.get(name), Some(TyKind::Object(_)));
    // In a `binding` scope the cell wins even over `Shadowed`: a Binding hands
    // its locals out BY REFERENCE, and an unboxed `Arc<Concrete>` has no slot
    // to share (see `Ctx::binding_names`).
    if cx.captured_locals.contains(name) && (!object_typed || cx.binding_names.is_some()) {
        LocalStorage::Captured
    } else if object_typed {
        LocalStorage::Shadowed
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
        // The `MutexGuard` is bound to an explicit `__g` local inside its
        // own block, not written as a bare `#ident.lock().clone()` -- see
        // `codegen::expr`'s `IvarRead` arm for the full explanation: an
        // UNNAMED `.lock()` temporary's
        // scope extends to the end of the ENCLOSING STATEMENT, so reading
        // the SAME captured local twice in one expression (e.g. `total *
        // total`) would otherwise deadlock a non-reentrant
        // `parking_lot::Mutex` -- confirmed empirically that a bare `{ }`
        // wrapper alone does NOT change the guard's drop timing; only
        // binding it to a named local inside the block does.
        LocalStorage::Captured => quote! { { let __g = #ident.lock(); __g.clone() } },
        // UFCS through the `Clone` trait, not `.clone()` method syntax: a
        // `Shadowed` local can be an `Arc<Concrete>` of a user class that
        // defines a Ruby `clone` method, which becomes an INHERENT method on
        // the generated struct and would win the method-syntax resolution
        // over the `Arc` refcount bump (see `codegen::expr`'s `SelfRef` arm).
        LocalStorage::Hoisted | LocalStorage::Shadowed => quote! { Clone::clone(&#ident) },
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
        // its own statement, because `*#ident.lock() = #value;` evaluates
        // the LHS place expression (calling `lock()`,
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

// The collection half (`Locals`, `collect_locals`) lives in
// `analyze::local_storage` -- pure HIR analysis, shared with the capture
// scan; re-exported so `super::hoisting::<name>` paths keep resolving.
pub(super) use crate::analyze::local_storage::{Locals, collect_locals};

/// The hoisting-aware replacement for `stmt::emit_body`, used ONLY at the
/// top of a genuinely fresh Ruby local-variable scope: a method body, the
/// top-level `main` body, or a `super`-inlined parent method body. NOT used
/// at nested `emit_body` calls (an `if`/`case`/loop body shares its
/// enclosing scope's locals -- re-hoisting there would just reintroduce the
/// same shadowing bug this exists to fix, one level down).
pub fn emit_hoisted_body(cx: &Ctx, body: &[NodeId], wrap_ok: bool) -> TokenStream {
    emit_hoisted_body_with_extra_roots(cx, body, &[], &[], wrap_ok)
}

/// [`emit_hoisted_body`] with `preamble` spliced in AFTER the declarations and
/// before the first statement. The one caller is `TOPLEVEL_BINDING`, which has
/// to name the top-level frame's cells (so it can't come earlier) and has to
/// exist before any statement runs (so it can't come later).
pub fn emit_hoisted_body_after_decls(
    cx: &Ctx,
    body: &[NodeId],
    preamble: TokenStream,
    wrap_ok: bool,
) -> TokenStream {
    let decls = emit_hoisted_decls(cx, body, &[], &[]);
    let inner = emit_body(cx, body, wrap_ok);
    quote! { #decls #preamble #inner }
}

/// The locals a parameter DEFAULT assigns -- `def m(a = (flag = true; nil))`,
/// where Ruby scopes `flag` to the whole method and leaves it nil when the
/// default doesn't run. The prelude below is too late to declare these: the
/// default is emitted earlier, by `params::emit_prologue`. Parameters are
/// excluded because only that prologue can bind them (an optional or keyword
/// parameter is still an `Option<..>` until it unwraps it).
fn param_default_locals(cx: &Ctx, default_ids: &[NodeId], param_names: &[String]) -> Vec<String> {
    let mut names = Locals::default();
    for &id in default_ids {
        collect_locals(cx.compiler, id, &mut names);
    }
    let mut names = names.into_names();
    names.retain(|n| !param_names.iter().any(|p| p == n));
    names
}

/// Declarations for `param_default_locals`, to emit immediately BEFORE
/// `params::emit_prologue`.
pub fn emit_param_default_decls(
    cx: &Ctx,
    default_ids: &[NodeId],
    param_names: &[String],
) -> TokenStream {
    let decls = param_default_locals(cx, default_ids, param_names)
        .into_iter()
        .map(|name| emit_local_decl(cx, &name));
    quote! { #(#decls)* }
}

/// Same as `emit_hoisted_body`, but additionally scans `extra_roots` (a
/// method's own `Params::default_ids()` -- see `hir::Params`'s docs) for
/// names to declare in the hoisting prelude. A default's own
/// codegen still happens separately, inside `codegen::params::emit_prologue`
/// -- `extra_roots` here only feeds the NAME-COLLECTION pass, so a local a
/// default assigns (e.g. `def f(x: (y = 1; y))`) gets hoisted correctly too.
///
/// `param_names` (the method's own `Params::bound_names()`) marks names
/// that are ALREADY BOUND when this prelude runs -- via the Rust function
/// signature, `params::emit_prologue`, or a `super` splice's argument
/// bindings. An assigned name that's also a parameter is REBOUND from that
/// existing value (`let mut x = x;` / a cell seeded with `x`) instead of
/// nil-defaulted -- the nil default would SHADOW the parameter, making
/// `def f(name); name = name.upcase; ...` read `nil` (retiring the module
/// docs' old "reassigning a parameter" gap). A
/// never-assigned parameter isn't collected at all and keeps its plain
/// signature binding.
pub fn emit_hoisted_body_with_extra_roots(
    cx: &Ctx,
    body: &[NodeId],
    extra_roots: &[NodeId],
    param_names: &[String],
    wrap_ok: bool,
) -> TokenStream {
    let decls = emit_hoisted_decls(cx, body, extra_roots, param_names);
    let inner = emit_body(cx, body, wrap_ok);
    quote! { #decls #inner }
}

/// The declaration half of [`emit_hoisted_body_with_extra_roots`] on its own,
/// so a caller can splice something between the prelude and the statements.
fn emit_hoisted_decls(
    cx: &Ctx,
    body: &[NodeId],
    extra_roots: &[NodeId],
    param_names: &[String],
) -> TokenStream {
    let mut names = Locals::default();
    for &n in body {
        collect_locals(cx.compiler, n, &mut names);
    }
    for &n in extra_roots {
        collect_locals(cx.compiler, n, &mut names);
    }
    let predeclared = param_default_locals(cx, extra_roots, param_names);
    let decls = names.names().iter().filter_map(|n| {
        let ident = safe_ident(n);
        let is_param = param_names.iter().any(|p| p == n);
        if predeclared.contains(n) {
            return None;
        }
        match local_storage(cx, n) {
            // `#[allow(unused_assignments)]`: the `Nil` default is
            // frequently overwritten before ever being read (e.g. a local's
            // very first assignment happens unconditionally right after
            // this) -- that's the intended, Ruby-faithful shape, not a
            // mistake to warn about.
            LocalStorage::Hoisted if is_param => Some(quote! {
                #[allow(unused_assignments)]
                let mut #ident: zeo_rt::RubyValue = #ident;
            }),
            LocalStorage::Hoisted => Some(quote! {
                #[allow(unused_assignments)]
                let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Nil;
            }),
            LocalStorage::Captured if is_param => Some(quote! {
                let #ident: std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                    std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(#ident));
            }),
            LocalStorage::Captured => Some(quote! {
                let #ident: std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                    std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(zeo_rt::RubyValue::Nil));
            }),
            LocalStorage::Shadowed => None,
        }
    });
    quote! { #(#decls)* }
}

/// ONE local's declaration, per its storage class -- the same rule
/// `emit_hoisted_body`'s prelude applies, for callers that need to declare a
/// name outside that prelude. Today: destructuring parameters
/// (`codegen::params::emit_destructures`), whose nested names are params (so
/// hoisting deliberately skips them) but have no Rust signature slot to be
/// bound by either.
///
/// `Shadowed` emits nothing: that storage class means "the `let` comes from
/// the assignment itself", and `emit_local_write` duly emits one.
pub(super) fn emit_local_decl(cx: &Ctx, name: &str) -> TokenStream {
    let ident = safe_ident(name);
    match local_storage(cx, name) {
        LocalStorage::Hoisted => quote! {
            #[allow(unused_assignments, unused_mut)]
            let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Nil;
        },
        LocalStorage::Captured => quote! {
            let #ident: std::sync::Arc<zeo_rt::parking_lot::Mutex<zeo_rt::RubyValue>> =
                std::sync::Arc::new(zeo_rt::parking_lot::Mutex::new(zeo_rt::RubyValue::Nil));
        },
        LocalStorage::Shadowed => quote! {},
    }
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
pub fn emit_proc_own_locals_prelude(cx: &Ctx, names: &FSet<String>) -> TokenStream {
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort();
    let decls = sorted.iter().filter_map(|n| {
        let ident = safe_ident(n);
        match local_storage(cx, n) {
            LocalStorage::Hoisted => Some(quote! {
                #[allow(unused_assignments)]
                let mut #ident: zeo_rt::RubyValue = zeo_rt::RubyValue::Nil;
            }),
            LocalStorage::Captured => unreachable!(
                "an own-only name is by definition not in captured_locals (see call.rs's split)"
            ),
            LocalStorage::Shadowed => None,
        }
    });
    quote! { #(#decls)* }
}
