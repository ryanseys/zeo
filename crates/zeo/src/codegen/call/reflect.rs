//! Compile-time constant/class reflection and class-method dispatch on a
//! statically-known target: `try_const_reflection` (`Klass.const_get(:N)`/
//! `const_defined?` with a literal symbol), `emit_class_method_call_on`
//! (`ClassName.foo(...)`), and their shared helpers
//! (`literal_name_arg`/`is_valid_const_name`/`any_builtin_overrides`).

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use quote::quote;

use crate::codegen::Ctx;
use crate::codegen::constfold::{ObjectReach, class_const_in, value_const_defined_in};
use crate::hir::{HirNode, KwArg, NodeId};
use proc_macro2::TokenStream;

/// Whether ANY reopened builtin defines `name` -- the
/// Poly-receiver side of the universal arms' override check in `dispatch`:
/// a Poly value might turn out AT RUNTIME to be an instance of a reopened
/// builtin, so a universal method it overrides anywhere must route
/// dynamically (where `send_value`'s value-method-first probe decides per
/// actual class). Free for programs with no reopens: every builtin's
/// materialized method table is empty.
pub(super) fn any_builtin_overrides(cx: &Ctx, name: &str) -> bool {
    let Some(id) = cx.compiler.names.get(name) else {
        return false;
    };
    cx.compiler
        .classes
        .iter()
        .any(|c| c.is_builtin && c.methods.iter().any(|e| e.name == id))
}
/// The shared core behind `ClassName.foo(...)` (`emit_call`'s
/// constant-receiver interception, `target` resolved from the literal
/// path) AND an implicit-self call
/// made FROM WITHIN another class method's own body (`emit_call`'s
/// no-receiver branch, `target` already known as `cx.defining_class` --
/// no name to look up at all). Same "plain required parameters only, no
/// keyword args, no block" restriction either way (see
/// `codegen::mod::emit_class_method_fn`'s matching rejection).
/// Extracts the compile-time string of a literal Symbol (`:Name`) or
/// single-segment String (`"Name"`) argument -- the only forms the constant-
/// reflection fold recognizes; a computed name falls through to runtime.
fn literal_name_arg(cx: &Ctx, id: NodeId) -> Option<String> {
    match &cx.compiler.hir[id] {
        HirNode::SymbolLit(s) => Some(s.clone()),
        HirNode::StringLit(parts) => match parts.as_slice() {
            [crate::hir::StrPart::Lit(s)] => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}
/// A well-formed constant name (`/\A\p{Upper}\w*\z/`): a leading uppercase
/// letter, then identifier characters. Anything else (`lower`, `_Foo`, `@x`,
/// `1A`, `A B`, `""`) is a `NameError "wrong constant name ..."`.
fn is_valid_const_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_uppercase() => chars.all(|c| c.is_alphanumeric() || c == '_'),
        _ => false,
    }
}
/// Compile-time fold of `Klass.const_get(:NAME)` / `Klass.const_defined?(:NAME)`
/// on a statically-known class/module `target` with a LITERAL name -- the
/// same flat constant/class registry a constant READ resolves against.
/// Returns `None` (fall through to ordinary dynamic dispatch) for a computed
/// name, extra args, a block, or any other method.
pub(super) fn try_const_reflection(
    cx: &Ctx,
    target: crate::compiler::ClassId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> Option<TokenStream> {
    if (name != "const_get" && name != "const_defined?")
        || !kwargs.is_empty()
        || block.is_some()
        || block_arg.is_some()
    {
        return None;
    }
    // Only the inheriting form folds -- this fold searches the ancestry. An
    // explicit `false` falls through to `rmodule.rs`, which restricts to the
    // receiver's own table.
    let arg = match args {
        [a] => a,
        [a, inherit] if matches!(cx.compiler.hir[*inherit], HirNode::BoolLit(true)) => a,
        _ => return None,
    };
    let cname = literal_name_arg(cx, *arg)?;

    // `const_get("A::B")` walks the path, which this flat fold cannot do --
    // hand it to the runtime row, where the walk lives.
    if cname.contains("::") {
        return None;
    }
    if !is_valid_const_name(&cname) {
        // Through `make_name_error`, not a bare construction: ruby's
        // `wrong constant name` carries `#name` and `#receiver` like any other
        // NameError, and a bare `NameError.new(msg)` leaves both nil. The
        // runtime row raises the same way for a name this fold cannot see.
        let msg = format!("wrong constant name {cname}");
        let id = target.0;
        return Some(quote! {
            return Err(zeo_rt::Signal::Raise(zeo_rt::stamp_backtrace(
                zeo_rt::make_name_error(
                    #msg.to_string(),
                    #cname,
                    zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)),
                ),
            )))
        });
    }

    // `const_get`/`const_defined?` reach through to `Object`, unlike the
    // scope operator -- `K.const_get(:Errno)` answers where `K::Errno` raises.
    let as_class = class_const_in(&cx.const_env(), target, &cname, ObjectReach::Included);
    // A runtime-conditional class is the runtime row's to answer: this fold
    // would bake an existence the concealment table decides.
    if as_class.is_some_and(|c| cx.compiler.class(c).runtime_conditional) {
        return None;
    }
    if name == "const_defined?" {
        // A BUILTIN's ext constants (`Socket::AF_INET6`, `Float::INFINITY`) are
        // seeded straight into `const_owners` by `analyze::seed_ext_const_owners`
        // rather than written as class-body statements, so
        // `value_const_defined_in`'s "an actual `NAME = ...` in the body" rule
        // cannot see them -- and the guard they gate
        // (`unless Socket.const_defined?(:AF_INET6)`) folded the wrong way.
        // Exact for a builtin: those entries are the seeded table itself.
        let seeded = cx.compiler.class(target).is_builtin
            && cx.compiler.class(target).const_owners.contains_key(&cname);
        let defined = as_class.is_some()
            || seeded
            || value_const_defined_in(&cx.const_env(), target, &cname, ObjectReach::Included);
        // Only a PROVEN yes folds. A no here means "no class body writes this
        // name", which is not the same as "no such constant": a top-level
        // assignment is no class body's statement, and a `const_set` can add one
        // at any time. Both are the runtime row's to answer.
        if !defined {
            return None;
        }
        return Some(quote! { zeo_rt::RubyValue::Bool(true) });
    }
    // const_get: a class-name constant answers the Class value directly; a
    // value constant reads through the runtime store (which also holds the
    // builtin-seeded ones, e.g. `Float::INFINITY`), raising a receiver-
    // qualified NameError on a genuine miss.
    if let Some(c) = as_class {
        let id = c.0;
        return Some(quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) });
    }
    // Straight from the resolved `ClassId`. Rendering it with `fq_name` and
    // re-resolving that string against this emit site's cref chain looks
    // equivalent and is not: `fq_name` drops the top-level anchor, and a
    // nested constant shadowing the namespace head then captures the walk.
    let owner = crate::codegen::expr::const_owner_of(cx, target, &cname);
    // A miss dispatches `const_missing` on the receiver (CRuby's protocol);
    // Module's default hook raises the same receiver-qualified NameError
    // this site used to bake, `#name`/`#receiver` included.
    let receiver_id = target.0;
    Some(quote! {
        match zeo_rt::const_get(#owner, #cname) {
            Some(__v) => __v,
            None => zeo_rt::const_miss(zeo_rt::ClassId(#receiver_id), #cname)?,
        }
    })
}
pub(super) fn emit_class_method_call_on(
    cx: &Ctx,
    target: crate::compiler::ClassId,
    name: &str,
    args: &[NodeId],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> TokenStream {
    let target_name = &cx.compiler.class(target).name;
    let Some((_, sid)) = cx.compiler.class_method_in_chain(target, name) else {
        return crate::codegen::unsupported(format!(
            "unsupported call `{target_name}.{name}` (a zeo gap, or no such class method is defined)"
        ));
    };
    let scope = cx.compiler.scope(sid);
    let target_ident = crate::codegen::ident::class_ident(cx.compiler, target);
    let method_ident = crate::codegen::ident::class_method_ident(name);
    // Full `Params` support: the same binding machinery an instance
    // call gets, through the receiverless `Callee::Bare` shape.
    crate::codegen::params::emit_call_args_to(
        cx,
        &crate::codegen::params::Callee::Bare(quote! { #target_ident::#method_ident }),
        name,
        &scope.params,
        args,
        kwargs,
        block,
        block_arg,
        scope.needs_block_param(),
        crate::codegen::scope_frame_guard(cx.compiler, scope, true),
    )
}
