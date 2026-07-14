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

/// Whether `name` gets a hoisted `let mut ... = Nil;` declaration (and, in
/// turn, whether its `LocalWrite`/`MultiWrite` sites compile to a plain
/// reassignment rather than a `let` -- see `stmt.rs`/`codegen::loops`) --
/// everything EXCEPT a concrete class-instance local (`TyKind::Object`,
/// e.g. `x = SomeClass.new`). Rust already infers THAT as an unboxed
/// `Rc<ConcreteClass>` (not `RubyValue`) directly from its own assignment
/// expression, and `ruby_class!`'s whole "Path 1 static dispatch on a
/// concrete Rust type" design depends on that unboxing -- uniformly
/// hoisting every local as `RubyValue` would break it (confirmed the hard
/// way: it broke safe-navigation's `Box.new(...)`-assigned-to-a-local
/// case). Known, narrow, pre-existing gap this doesn't fix: reassigning an
/// object-typed local INSIDE a loop still relies on shadowing and won't
/// persist across iterations -- nothing exercises that yet.
pub fn is_hoisted(cx: &Ctx, name: &str) -> bool {
    !matches!(cx.local_types.get(name), Some(TyKind::Object(_)))
}

/// Mirrors `analyze::collect_ivars`'s traversal shape (recursing into every
/// sub-expression position a `LocalWrite` could appear in), but collects
/// local-variable names instead of ivar names, and additionally descends
/// into loop bodies and `MultiWrite`/`For` targets.
fn collect_locals(compiler: &Compiler, id: NodeId, out: &mut Vec<String>) {
    match &compiler.hir[id] {
        HirNode::LocalWrite(name, value) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
            collect_locals(compiler, *value, out);
        }
        HirNode::IvarWrite(_, value) => collect_locals(compiler, *value, out),
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
        HirNode::Break(v) | HirNode::Next(v) => {
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
            args,
            block,
            ..
        } => {
            if let Some(r) = receiver {
                collect_locals(compiler, *r, out);
            }
            for &a in args {
                collect_locals(compiler, a, out);
            }
            if let Some(b) = block {
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
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
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
    let mut names = Vec::new();
    for &n in body {
        collect_locals(cx.compiler, n, &mut names);
    }
    let decls = names.iter().filter(|n| is_hoisted(cx, n)).map(|n| {
        let ident = safe_ident(n);
        // `#[allow(unused_assignments)]`: the `Nil` default is frequently
        // overwritten before ever being read (e.g. a local's very first
        // assignment happens unconditionally right after this) -- that's
        // the intended, Ruby-faithful shape, not a mistake to warn about.
        quote! {
            #[allow(unused_assignments)]
            let mut #ident: spinel_rt::RubyValue = spinel_rt::RubyValue::Nil;
        }
    });
    let inner = emit_body(cx, body, wrap_ok);
    quote! { #(#decls)* #inner }
}
