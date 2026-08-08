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
/// The visibility read here is the ENTRY's, not the definition's: ruby lets a
/// subclass re-scope a method it inherited (`private :inherited_method`)
/// without redefining it, and reading the definition would report the
/// ancestor's visibility and wave the call through.
/// The two receivers ruby runs no visibility check against: a literal `self`
/// (private is reachable through one since 2.7), and a receiver zeo
/// SYNTHESIZED for a call ruby writes with no receiver at all -- see
/// [`Hir::is_implicit_self_receiver`](crate::hir::Hir::is_implicit_self_receiver).
fn runs_no_check(cx: &Ctx, recv_id: NodeId) -> bool {
    matches!(cx.compiler.hir[recv_id], HirNode::SelfRef)
        || cx.compiler.hir.is_implicit_self_receiver(recv_id)
}

pub(super) fn enforce_visibility(
    cx: &Ctx,
    recv_id: NodeId,
    entry: &crate::compiler::MethodEntry,
    method_name: &str,
) -> Option<TokenStream> {
    match entry.visibility {
        Visibility::Public => {}
        Visibility::Private => {
            if !runs_no_check(cx, recv_id) {
                return Some(emit_visibility_error(cx, recv_id, "private", method_name));
            }
        }
        Visibility::Protected => {
            let related = cx
                .ask_opt(crate::codegen::class_query::ClassQuery::ProtectedRelated(
                    entry.owner,
                ))
                .is_some_and(|a| a.yes());
            if !related {
                return Some(emit_visibility_error(cx, recv_id, "protected", method_name));
            }
        }
    }
    None
}
/// Whether a Path 1 site must give up its devirtualized emission and fall
/// through to Path 2: a `protected` target under a dynamic `self` (a re-homed
/// block, a `Class.new`-body method) has no compile-time caller class to
/// check against, so only the runtime barrier -- asked with this site's
/// [`Caller::Runtime`] class -- can decide it.
pub(super) fn defers_to_runtime(cx: &Ctx, visibility: Visibility, bypass: bool) -> bool {
    !bypass && cx.self_is_dynamic && matches!(visibility, Visibility::Protected)
}

/// The caller class a Path 2 site carries -- the same question
/// [`enforce_visibility`] answers at compile time, for the sites where the
/// receiver's class is only known at run time.
pub(super) enum Caller {
    /// A per-site compile-time constant -- cacheable in the site's
    /// `CallSite`.
    Static(u32),
    /// A dynamic-`self` context (a re-homed block, a runtime method body):
    /// the caller class is whatever the runtime `self`'s class turns out to
    /// be, so the site must ask per call -- and must NOT fill a `CallSite`,
    /// whose vetting is per-site-constant.
    Runtime(TokenStream),
}

impl Caller {
    /// A statement binding the caller class to `__caller_class` BEFORE the
    /// call expression -- the runtime form reads `self`, which a block
    /// argument's `move` closure may consume later in the same expression
    /// (E0382 otherwise). Empty for the static form; pair with
    /// [`Caller::bound`].
    pub(super) fn bind(&self) -> TokenStream {
        match self {
            Caller::Static(_) => TokenStream::new(),
            Caller::Runtime(t) => quote! { let __caller_class: u32 = #t; },
        }
    }

    /// The argument matching [`Caller::bind`].
    pub(super) fn bound(&self) -> TokenStream {
        match self {
            Caller::Static(c) => quote! { #c },
            Caller::Runtime(_) => quote! { __caller_class },
        }
    }
}

/// `zeo_rt::FCALL` (`u32::MAX`) wherever ruby runs no check at all: an implicit
/// receiver, a literal `self` receiver (private is reachable that way since
/// 2.7), and `send`/`__send__`. Otherwise the class whose body the call sits
/// in, which is what decides whether a `protected` target is in reach -- read
/// off the runtime `self` where that class isn't a compile-time fact
/// (`Ctx::self_is_dynamic`: `instance_eval`/`instance_exec` re-homed blocks
/// and `Class.new`-body method bodies).
pub(super) fn caller_class(cx: &Ctx, recv_id: NodeId, bypass: bool) -> Caller {
    if bypass || runs_no_check(cx, recv_id) {
        return Caller::Static(u32::MAX);
    }
    if cx.self_is_dynamic {
        let s = &cx.self_ident;
        return Caller::Runtime(quote! { (#s).class_id().0 });
    }
    // Outside any class body `self` is `main`, an ordinary `Object` -- and
    // `Object` is what a protected check has to compare against there.
    Caller::Static(cx.current_class.map_or(0, |c| c.0))
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
    if runs_no_check(cx, recv_id) {
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
