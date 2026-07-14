//! Emits a `TokenStream` for every HIR node except `LocalWrite` used as a
//! plain body statement (see `stmt.rs`). Every fragment produced here
//! evaluates to a bare `spinel_rt::RubyValue` -- any fallible sub-call (a
//! method dispatch that returns `Result<RubyValue, Signal>`) already has `?`
//! applied internally, so callers can always splice an `emit_expr` result
//! wherever a plain `RubyValue`-typed expression is expected, with no
//! wrapping of their own.

use quote::quote;

use super::call::emit_call;
use super::ident::safe_ident;
use super::Ctx;
use crate::compiler::ClassId;
use crate::hir::{HirNode, NodeId};
use crate::types::{infer_type_with_locals, TyKind};
use proc_macro2::TokenStream;

/// A node's static type, given the enclosing scope's local-type context --
/// the one place `codegen` should call into `types::infer_type_with_locals`,
/// so every dispatch decision (receiver class, numeric-operator eligibility)
/// sees the same local-aware inference.
pub fn infer(cx: &Ctx, id: NodeId) -> TyKind {
    infer_type_with_locals(cx.compiler, cx.local_types, id)
}

/// The receiver's statically-known class, if any -- the entire input to the
/// Path 1 / Path 2 dispatch decision.
pub fn infer_class(cx: &Ctx, id: NodeId) -> Option<ClassId> {
    match infer(cx, id) {
        TyKind::Object(cid) => Some(cid),
        _ => None,
    }
}

/// A Rust expression of type `spinel_rt::Symbol` (not `RubyValue`) -- used
/// for `send`'s second argument. A literal `:sym` skips the
/// box-then-immediately-unwrap round trip.
pub fn emit_symbol_expr(cx: &Ctx, id: NodeId) -> TokenStream {
    if let HirNode::SymbolLit(s) = &cx.compiler.hir[id] {
        return quote! { spinel_rt::Symbol::intern(#s) };
    }
    let e = emit_expr(cx, id);
    quote! { (#e).as_symbol_unchecked() }
}

/// `defined?(expr)` -- a compile-time-resolvable classification of `expr`'s
/// *syntactic form*, not a runtime check (mirrors CRuby's own result
/// strings: `"expression"`/`"method"`/`"local-variable"`/
/// `"instance-variable"`/`nil`). Scope-cut, narrower than real Ruby: an
/// instance variable is classified `"instance-variable"` whenever it's
/// syntactically an `@ivar` read, without tracking whether that ivar was
/// ever actually assigned on this particular instance (CRuby returns `nil`
/// for a never-assigned ivar); a `Call`/`New`/`SuperCall` is always
/// classified `"method"` without checking it actually resolves. Both are
/// documented approximations, not silent wrongness -- getting `defined?`
/// fully faithful needs real per-instance/per-callsite tracking this spike
/// doesn't have yet.
fn emit_defined(cx: &Ctx, id: NodeId) -> TokenStream {
    let classification: Option<&str> = match &cx.compiler.hir[id] {
        HirNode::LocalRead(name) => {
            if cx.local_types.contains_key(name) {
                Some("local-variable")
            } else {
                None
            }
        }
        HirNode::IvarRead(_) => Some("instance-variable"),
        HirNode::New { .. } | HirNode::Call { .. } | HirNode::SuperCall { .. } => Some("method"),
        HirNode::IntegerLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::And(..)
        | HirNode::Or(..)
        | HirNode::Defined(_)
        | HirNode::If { .. }
        | HirNode::CaseWhen { .. }
        | HirNode::LocalWrite(..)
        | HirNode::IvarWrite(..) => Some("expression"),
        HirNode::Block { .. } | HirNode::Program(_) | HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => {
            None
        }
    };
    match classification {
        Some(s) => quote! { spinel_rt::RubyValue::Str(#s.to_string()) },
        None => quote! { spinel_rt::RubyValue::Nil },
    }
}

/// `if`/`unless`/`elsif`/ternary -- all normalized to `HirNode::If` at
/// lowering time. Ruby's implicit-last-expression-return and Rust's
/// `if`-as-expression are structurally identical, so this is a direct
/// translation: both branches are emitted as bare `RubyValue`-typed
/// *values* (`wrap_ok: false`, via `emit_body` -- this isn't a whole method
/// body, just a value nested inside the enclosing one), matching every
/// other `emit_expr` fragment's contract.
fn emit_if(cx: &Ctx, cond: NodeId, then_body: &[NodeId], else_body: &[NodeId]) -> TokenStream {
    let cond_expr = emit_expr(cx, cond);
    let then_val = super::stmt::emit_body(cx, then_body, false);
    let else_val = super::stmt::emit_body(cx, else_body, false);
    quote! {
        if (#cond_expr).truthy() { #then_val } else { #else_val }
    }
}

/// `case subject; when v1, v2 then ...; else ...; end`, desugared to a
/// nested `if`/`else` chain (arms tested top-to-bottom, first match wins --
/// NOT a native Rust `match`, since Ruby's `when` dispatches through
/// `===`/truthiness on values that Rust's structural pattern matching can't
/// express generically). With a `subject`, each value is compared via
/// `RubyValue::rb_eq` (value equality -- see its docs for why this bypasses
/// general `===`/`==` dispatch); with no subject, each value IS the boolean
/// condition being tested directly (`case; when a > b; ...; end` behaves
/// like a chained `if`/`elsif`). The subject (if any) is evaluated exactly
/// once into a temporary, since every arm may reference it.
fn emit_case_when(
    cx: &Ctx,
    subject: Option<NodeId>,
    arms: &[(Vec<NodeId>, Vec<NodeId>)],
    else_body: &[NodeId],
) -> TokenStream {
    let has_subject = subject.is_some();
    let mut chain = super::stmt::emit_body(cx, else_body, false);

    for (values, body) in arms.iter().rev() {
        let body_val = super::stmt::emit_body(cx, body, false);
        let mut check: Option<TokenStream> = None;
        for &v in values {
            let v_expr = emit_expr(cx, v);
            let this_check = if has_subject {
                quote! { (#v_expr).rb_eq(&__subject) }
            } else {
                quote! { (#v_expr).truthy() }
            };
            check = Some(match check {
                None => this_check,
                Some(prev) => quote! { (#prev) || (#this_check) },
            });
        }
        let check = check.expect("a `when` clause always has at least one value");
        chain = quote! {
            if #check { #body_val } else { #chain }
        };
    }

    match subject {
        Some(s) => {
            let subject_expr = emit_expr(cx, s);
            quote! { { let __subject = #subject_expr; #chain } }
        }
        None => chain,
    }
}

pub fn emit_expr(cx: &Ctx, id: NodeId) -> TokenStream {
    match &cx.compiler.hir[id] {
        HirNode::IntegerLit(v) => quote! { spinel_rt::RubyValue::Int(#v) },
        HirNode::SymbolLit(s) => {
            quote! { spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#s)) }
        }
        HirNode::LocalRead(name) => {
            let ident = safe_ident(name);
            quote! { #ident.clone() }
        }
        HirNode::And(l, r) => {
            // Ruby's `&&`/`and` returns the operand itself, not a bool --
            // `false && anything` is `false`, but `1 && 2` is `2`, not
            // `true`. A literal Rust `&&` is bool-typed and can't express
            // this, so short-circuit via an explicit `if` on `.truthy()`.
            let lhs = emit_expr(cx, *l);
            let rhs = emit_expr(cx, *r);
            quote! {
                { let __lhs = #lhs; if __lhs.truthy() { #rhs } else { __lhs } }
            }
        }
        HirNode::Or(l, r) => {
            let lhs = emit_expr(cx, *l);
            let rhs = emit_expr(cx, *r);
            quote! {
                { let __lhs = #lhs; if __lhs.truthy() { __lhs } else { #rhs } }
            }
        }
        HirNode::Defined(v) => emit_defined(cx, *v),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => emit_if(cx, *cond, then_body, else_body),
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => emit_case_when(cx, *subject, arms, else_body),
        HirNode::LocalWrite(name, value) => {
            // Only reachable when a LocalWrite is used as a sub-expression
            // (not a body statement) -- none of the 7 examples do this, but
            // handle it faithfully to Ruby's "assignment evaluates to the
            // assigned value" semantics rather than silently dropping it.
            let ident = safe_ident(name);
            let v = emit_expr(cx, *value);
            quote! { { let #ident = #v; #ident.clone() } }
        }
        HirNode::IvarRead(name) => {
            let ident = safe_ident(name);
            quote! { self.#ident.borrow().clone() }
        }
        HirNode::IvarWrite(name, value) => {
            let ident = safe_ident(name);
            let v = emit_expr(cx, *value);
            quote! { { let __v = #v; *self.#ident.borrow_mut() = __v.clone(); __v } }
        }
        HirNode::New { class_name, args } => super::call::emit_new(cx, class_name, args),
        HirNode::SuperCall { .. } => super::call::emit_super_inline(cx),
        HirNode::Call {
            receiver,
            name,
            args,
            block,
            safe,
        } => emit_call(cx, *receiver, name, args, *block, *safe),
        HirNode::Block { .. } => {
            panic!("a Block should only be reached via the Call that invokes it")
        }
        HirNode::Program(_) | HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => {
            panic!("unexpected top-level-only node in expression position")
        }
    }
}
