//! `private`/`protected` enforcement for an explicit-receiver Path 1 call:
//! `enforce_visibility` (the check, mirroring real Ruby's actual rules) and
//! `emit_visibility_error` (the raised `NoMethodError`'s construction).

use quote::quote;

use crate::codegen::Ctx;
use crate::codegen::expr::infer_any_class;
use crate::hir::{HirNode, NodeId, Visibility};
use proc_macro2::TokenStream;

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
pub(super) fn enforce_visibility(
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
            let owner = scope
                .class
                .expect("a materialized method always has an owner class");
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
fn emit_visibility_error(cx: &Ctx, recv_id: NodeId, kind: &str, method_name: &str) -> TokenStream {
    let describe = match infer_any_class(cx, recv_id) {
        Some(cid) => format!("an instance of {}", cx.compiler.class(cid).name),
        None => "an instance of Object".to_string(),
    };
    let msg = format!("{kind} method '{method_name}' called for {describe}");
    quote! {
        return Err(zeo_rt::raise_error("NoMethodError", #msg.to_string()))
    }
}
