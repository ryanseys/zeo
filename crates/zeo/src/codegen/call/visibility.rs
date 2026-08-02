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
/// against a receiver whose class isn't statically known) asks the same
/// question at RUN time instead -- see [`caller_class`], which is how the site
/// tells the runtime which of the two rules applies to it.
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
/// The caller class a Path 2 site records -- the same question
/// [`enforce_visibility`] answers at compile time, for the sites where the
/// receiver's class is only known at run time.
///
/// `zeo_rt::FCALL` (`u32::MAX`) wherever ruby runs no check at all: an implicit
/// receiver, a literal `self` receiver (private is reachable that way since
/// 2.7), and `send`/`__send__`. Otherwise the class whose body the call sits
/// in, which is what decides whether a `protected` target is in reach.
pub(super) fn caller_class(cx: &Ctx, recv_id: NodeId, bypass: bool) -> u32 {
    if bypass || matches!(cx.compiler.hir[recv_id], HirNode::SelfRef) {
        return u32::MAX;
    }
    // Outside any class body `self` is `main`, an ordinary `Object` -- and
    // `Object` is what a protected check has to compare against there.
    cx.current_class.map_or(0, |c| c.0)
}

/// The class-method counterpart, for an explicit `Target.name` receiver. Ruby
/// has no protected class method, so `private_class_method` is the whole rule,
/// and -- as with the instance half -- a literal `self` receiver is allowed.
/// The error names the receiver as a "class" or a "module", not as an instance.
pub(super) fn enforce_class_method_visibility(
    cx: &Ctx,
    recv_id: NodeId,
    target: crate::compiler::ClassId,
    method_name: &str,
) -> Option<TokenStream> {
    if !cx.compiler.class_method_is_private(target, method_name) {
        return None;
    }
    if matches!(cx.compiler.hir[recv_id], HirNode::SelfRef) {
        return None;
    }
    let info = cx.compiler.class(target);
    let kind = if info.is_module { "module" } else { "class" };
    let msg = format!(
        "private method '{method_name}' called for {kind} {}",
        cx.compiler.fq_name(target)
    );
    Some(quote! {
        return Err(zeo_rt::raise_error("NoMethodError", #msg.to_string()))
    })
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
