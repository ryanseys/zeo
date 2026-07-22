//! `super` call emission -- named `super_calls`, not `super`, since `super`
//! is a keyword. Every `super` dispatches at RUNTIME against the
//! receiver's live ancestry (M5): `emit_runtime_super` routes to
//! `send_super_from`'s per-position MRO walk (instance methods) or
//! `send_super_class_from` (class methods); `emit_super_dynamic` reads the
//! defining class off the runtime method-frame stack for runtime-defined
//! bodies; `emit_value_super` bridges into an inherited value builtin's
//! payload; and the `extend M` singleton-chain shape resolves its target
//! at COMPILE time (sibling-extend order is compile-time knowledge) but
//! still dispatches the resolved row at runtime. The former ~500-line
//! inline HIR splice (and its argument-binding machinery) is gone -- one
//! dispatch mechanism, verified suite-equivalent before the switch.

use quote::quote;

use crate::codegen::Ctx;
use crate::codegen::expr::emit_expr;
use crate::codegen::ident::safe_ident;
use crate::hir::{KwArg, NodeId, Params};
use proc_macro2::TokenStream;

/// `super` always resolves against the receiver's REAL, full linearized
/// `ancestors` (mirrors CRuby's `vm_search_super_method`) -- searching
/// FORWARD (toward the root) from wherever the CURRENTLY-executing method
/// was actually defined (`cx.defining_class`), not from
/// `cx.current_class` (the receiver's own concrete type, which only
/// coincides with `defining_class` for an ordinary own-body method).
/// That is what makes a `super` chain crossing a `prepend`/`include`
/// boundary resolve correctly. The RESOLUTION happens at runtime
/// (`send_super_from`'s per-position walk over own-method tables --
/// generated structs are distinct Rust types, so cross-type dispatch goes
/// through dynamic-self rows, never a spliced body); this function's
/// compile-time knowledge only picks WHICH runtime channel to emit and
/// forwards the right arguments.
pub fn emit_super(
    cx: &Ctx,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    // A `super` inside a RUNTIME-defined method body (a `def`/`define_method`
    // installed in a `Class.new`/`Struct.new`/`Data.define` block) has no
    // compile-time defining class -- the class is minted at runtime. `emit_expr`
    // marks such a body with `runtime_super_params`; resolve through the runtime
    // method-frame stack (`zeo_rt::send_super_dynamic`) instead of splicing a
    // compile-time ancestor's HIR.
    if let Some(params) = cx.runtime_super_params.clone() {
        return emit_super_dynamic(cx, &params, args, kwargs, zsuper, block);
    }

    // A CLASS method (`def self.foo`) has no `self: Arc<Self>` receiver, so
    // `current_class` is deliberately None there and `class_self` carries the
    // receiver class instead (see `codegen::mod`'s Ctx construction).
    //
    // Real Ruby needs no special case at all: `def self.foo` is an ordinary
    // instance method on the SINGLETON class, and singleton classes form a
    // parallel chain -- `#<Class:C>.super == #<Class:C.superclass>`
    // (`make_metaclass`, class.c:1186) -- so one walk serves both. This
    // compiler keeps class methods in their own flattened table rather than
    // modeling singleton classes, so the walk is MIRRORED here instead of
    // shared: same ancestor chain, `own_class_methods` instead of
    // `own_methods`. (Documented divergence: the flattening copies bodies
    // down, so it cannot see a class method added at runtime.)
    let in_class_method = cx.current_class.is_none();
    // `super` with no enclosing method at all (top level, a top-level
    // block, a class body): real Ruby raises at RUNTIME, rescuable -- so
    // emit that raise (message verbatim, vm_insnhelper.c) instead of
    // crashing the compiler on the missing context.
    let (Some(receiver_class), Some(defining_class), Some(mname)) = (
        cx.current_class.or(cx.class_self),
        cx.defining_class,
        cx.current_method.as_deref(),
    ) else {
        let err =
            super::raise::emit_simple_error(cx, "NoMethodError", "super called outside of method");
        return quote! { Err(zeo_rt::Signal::Raise(#err))? };
    };
    // Which pool a `super` search consults, per the note above.
    let own_pool = |compiler: &crate::compiler::Compiler, anc: crate::compiler::ClassId| {
        let info = compiler.class(anc);
        if in_class_method {
            info.own_class_methods.clone()
        } else {
            info.own_methods.clone()
        }
    };

    let ancestors = &cx.compiler.class(receiver_class).ancestors;
    let pos = ancestors.iter().position(|&a| a == defining_class);

    // The method CURRENTLY executing (whose lexical body this `super` call
    // sits inside) -- needed for bare `super`'s forwarding case (in the splice
    // AND the runtime-dispatch branches below). Guaranteed to exist: this
    // `super` is inside `mname`'s own body on `defining_class`. Which pool
    // that body's scope lives in depends on HOW it became a class method:
    // an ordinary `def self.x` sits in `own_class_methods`, but in the
    // `extend M` shape (recognized by `defining_class` being absent from the
    // receiver's materialized ancestry -- M sits in the receiver's
    // SINGLETON-class chain, which the flattened class-method model doesn't
    // build), the emitted body is M's INSTANCE method, so that pool is
    // consulted first -- M may define BOTH (`def x` and `def self.x`, as
    // singleton's `SingletonClassProperties` does with `included`), and
    // picking the wrong scope forwards the wrong parameter names.
    let extend_shape = pos.is_none() && in_class_method;
    let scope_of = |pool: &[crate::compiler::ScopeId]| {
        pool.iter()
            .find(|&&s| cx.compiler.scope(s).name == mname)
            .copied()
    };
    let current_sid = if extend_shape {
        scope_of(&cx.compiler.class(defining_class).own_methods)
    } else {
        scope_of(&own_pool(cx.compiler, defining_class))
    };
    let Some(current_sid) = current_sid else {
        // The current scope missing from the expected pool is an internal
        // inconsistency, but only a BARE `super` actually needs it (its
        // param names drive zsuper forwarding) -- explicit-args `super`
        // still resolves correctly through the runtime walk, so degrade to
        // that instead of crashing the compiler on a shape the pool
        // selection didn't anticipate.
        if zsuper {
            panic!(
                "internal error: bare `super` in `{mname}` whose scope isn't in its \
                 defining class's own pool"
            )
        }
        return emit_runtime_super(cx, mname, &Params::default(), args, kwargs, false, block);
    };
    let current_params = cx.compiler.scope(current_sid).params.clone();

    // EVERY class-method `super` resolves against the SINGLETON-class
    // chain, which the compiler reconstructs exactly for compile-time
    // extends -- each non-module ancestor contributes its own class
    // methods and then its `extend`ed modules' instance methods, most
    // recently extended first (`make_metaclass`'s parallel chain). That
    // covers both shapes: a `def self.x`'s `super` sees the class's OWN
    // sibling extends first (Host extends A, B; `Host.who`'s super hits
    // B then A -- oracle-verified), and a `super` written IN an extended
    // module resumes after that module's chain entry. A miss (e.g.
    // `super` targeting a BUILTIN default like `Module#included`) falls
    // through to the runtime walk below. Instance methods keep the plain
    // MRO walk over own pools.
    let found = if in_class_method {
        extended_singleton_super(cx, receiver_class, defining_class, mname)
    } else {
        pos.and_then(|pos| {
            ancestors[pos + 1..].iter().find_map(|&anc| {
                own_pool(cx.compiler, anc)
                    .iter()
                    .find(|&&s| cx.compiler.scope(s).name == mname)
                    .map(|&sid| (anc, sid, false))
            })
        })
    };

    // `super` into an inherited VALUE builtin (D3): a `class Stack < Array`
    // method whose `super` finds NO user definition above targets the native
    // `Array` method -- `super` in `initialize` re-seats the payload, any other
    // runs the builtin against it. There is no HIR to splice (the builtin has no
    // `own_methods`), so dispatch through the runtime. Only reached when no user
    // ancestor overrides `mname`; a user parent still splices. (Not for the
    // extend shape: a class-method `super` never targets a value builtin's
    // INSTANCE method -- it keeps its runtime handoff below.)
    if found.is_none() && pos.is_some() && cx.compiler.is_value_subclass(receiver_class) {
        return emit_value_super(cx, mname, &current_params, args, kwargs, zsuper, block);
    }

    // No user definition above the defining class. Real Ruby has NO
    // definition-time check here at all -- `super` resolves at CALL time
    // against the receiver's live ancestry (`vm_search_super_method`,
    // vm_insnhelper.c:5041) and raises `NoMethodError: super: no superclass
    // method 'x' for ...` only if that walk comes up empty (vm_eval.c:993).
    // Rejecting at compile time is wrong twice over: it kills programs that
    // merely *mention* such a `super` in a rescued or dead branch (codegen
    // lowers every branch eagerly), and it cannot see methods a compile-time
    // scan misses -- an included module's, or one registered at runtime. Hand
    // off to the runtime walk, which finds those and raises correctly if not.
    let Some((new_defining_class, _sid, target_is_module_instance)) = found else {
        return emit_runtime_super(cx, mname, &current_params, args, kwargs, zsuper, block);
    };

    // The resolved target dispatches at RUNTIME -- the parent's HIR is
    // never spliced (M5): one mechanism, `send_super_from`'s per-position
    // MRO walk (or the class-method/singleton channels below), serves every
    // `super`. A CLASS-method target keeps its COMPILE-TIME singleton-chain
    // resolution -- sibling extends interleave in an order the runtime
    // registry doesn't record -- and dispatches the resolved target's
    // registered row directly (module-instance bridge or `def self.x` row,
    // `call_singleton_super_target` serves both).
    if in_class_method {
        let target_id = new_defining_class.0;
        let recv_id = receiver_class.0;
        let (pushes, block_expr) =
            emit_runtime_super_args(cx, &current_params, args, kwargs, zsuper, block);
        return quote! {
            {
                let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
                #(#pushes)*
                zeo_rt::call_singleton_super_target(
                    zeo_rt::ClassId(#target_id),
                    #target_is_module_instance,
                    zeo_rt::ClassId(#recv_id),
                    zeo_rt::Symbol::intern(#mname),
                    &__super_args,
                    #block_expr,
                )?
            }
        };
    }
    emit_runtime_super(cx, mname, &current_params, args, kwargs, zsuper, block)
}

/// `super` resolution for a method that reached the receiver as a CLASS
/// method via `extend M`: walks the receiver's SINGLETON-class chain as the
/// compiler knows it -- for each non-module ancestor (`include`d modules
/// never join a singleton chain), the ancestor's own `def self.x` pool and
/// then its `extend`ed modules' instance-method pools, most recently
/// extended first. Returns the first `mname` definition STRICTLY AFTER
/// `defining_class`'s own entry in that chain, `None` when the walk runs
/// dry (the caller then defers to the runtime walk, which owns builtin
/// defaults and runtime-defined methods).
fn extended_singleton_super(
    cx: &Ctx,
    receiver_class: crate::compiler::ClassId,
    defining_class: crate::compiler::ClassId,
    mname: &str,
) -> Option<(crate::compiler::ClassId, crate::compiler::ScopeId, bool)> {
    // `(class, instance_pool)`: a chain entry resolves `mname` against its
    // instance methods (an extended module) or its `def self.x` pool (a
    // class standing in for its own metaclass).
    let mut chain: Vec<(crate::compiler::ClassId, bool)> = Vec::new();
    for (i, &anc) in cx
        .compiler
        .class(receiver_class)
        .ancestors
        .iter()
        .enumerate()
    {
        let info = cx.compiler.class(anc);
        // The receiver itself heads the chain even when it IS a module
        // (`module Target; extend Props; end`); mixed-in modules deeper in
        // the MRO contribute nothing to the singleton chain.
        if i > 0 && info.is_module {
            continue;
        }
        chain.push((anc, false));
        for &m in info.extends.iter().rev() {
            chain.push((m, true));
        }
    }
    // The defining entry: the extended MODULE (`super` written in it,
    // instance pool) or the CLASS itself (`def self.x`'s own slot) --
    // module and class ids never collide, so the id alone identifies it.
    let dpos = chain.iter().position(|&(c, _)| c == defining_class)?;
    chain[dpos + 1..].iter().find_map(|&(anc, instance_pool)| {
        let info = cx.compiler.class(anc);
        let pool = if instance_pool {
            &info.own_methods
        } else {
            &info.own_class_methods
        };
        pool.iter()
            .find(|&&s| cx.compiler.scope(s).name == mname)
            .map(|&sid| (anc, sid, instance_pool))
    })
}

/// `super` dispatched through `zeo_rt::send_super_from` rather than spliced,
/// resuming the receiver's REAL ancestor walk after `defining_class`.
///
/// Two callers, one mechanism:
///  - a `super` into a NATIVE default parent (`native_default`), where there is
///    no HIR worth splicing (see `emit_super`'s native-branch comment);
///  - a `super` the compile-time scan could not resolve at all, which real Ruby
///    resolves at CALL time against the receiver's live ancestry.
///
/// Builds the forwarded argument slice -- explicit `super(a, b)` args, or, for
/// bare `super`, the current method's own positional parameters (required /
/// optional / splatted `*rest` / post) -- with keyword arguments appended as
/// one trailing Hash (the runtime's G2 convention).
fn emit_runtime_super(
    cx: &Ctx,
    mname: &str,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    let def_id = cx.defining_class.expect("`super` outside a method").0;
    let (pushes, block_expr) =
        emit_runtime_super_args(cx, current_params, args, kwargs, zsuper, block);
    // A CLASS-method `super` (`current_class` deliberately `None` there --
    // see `emit_super`) has a class-object receiver, which
    // `send_super_from`'s object channel can't take: dispatch through the
    // class-method channel, which walks the receiver class's singleton
    // chain as the runtime knows it.
    if cx.current_class.is_none() {
        let recv_id = cx.class_self.expect("`super` outside a method").0;
        return quote! {
            {
                let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
                #(#pushes)*
                zeo_rt::send_super_class_from(
                    zeo_rt::ClassId(#recv_id),
                    zeo_rt::ClassId(#def_id),
                    zeo_rt::Symbol::intern(#mname),
                    &__super_args,
                    #block_expr,
                )?
            }
        };
    }
    // `send_super_from` takes a boxed `RubyValue`. An exception-backed self is
    // already one, but a plain generated-struct self is an `Arc<Concrete>` --
    // so box through the same helper an implicit-self call uses rather than
    // passing `self_ident` raw.
    let self_val = super::boxed_implicit_self(cx).unwrap_or_else(|| {
        // Concrete UFCS -- same `&RubyValue`-binding caveat as
        // `boxed_implicit_self`'s dynamic arm.
        let slf = &cx.self_ident;
        quote! { zeo_rt::RubyValue::clone(&#slf) }
    });
    quote! {
        {
            let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#pushes)*
            zeo_rt::send_super_from(
                &#self_val,
                zeo_rt::ClassId(#def_id),
                zeo_rt::Symbol::intern(#mname),
                &__super_args,
                #block_expr,
            )?
        }
    }
}
/// `super` from inside a RUNTIME-defined method body (a `def`/`define_method`
/// in a `Class.new`/`Struct.new`/`Data.define` block), whose defining class is
/// unknown at compile time. Forwards args exactly like `emit_runtime_super` --
/// explicit `super(a, b)`, or the enclosing method's own params for a bare
/// `super` -- but dispatches through `zeo_rt::send_super_dynamic`, which
/// reads the class off the runtime method-frame stack (pushed when the method
/// was entered) rather than from a compile-time `defining_class`.
fn emit_super_dynamic(
    cx: &Ctx,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    let self_val = super::boxed_implicit_self(cx).unwrap_or_else(|| {
        // Concrete UFCS -- same `&RubyValue`-binding caveat as
        // `boxed_implicit_self`'s dynamic arm.
        let slf = &cx.self_ident;
        quote! { zeo_rt::RubyValue::clone(&#slf) }
    });
    let (pushes, block_expr) =
        emit_runtime_super_args(cx, current_params, args, kwargs, zsuper, block);
    quote! {
        {
            let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#pushes)*
            zeo_rt::send_super_dynamic(&#self_val, &__super_args, #block_expr)?
        }
    }
}
/// `super` from a value-builtin subclass method into the inherited builtin
/// (D3): `zeo_rt::value_super` re-seats the payload for `initialize`, else
/// runs the root builtin method (`Array#push` ...) against the payload and
/// re-wraps a self-return. No HIR to splice (the builtin has no `own_methods`).
/// Argument forwarding is shared with the exception path.
fn emit_value_super(
    cx: &Ctx,
    mname: &str,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> TokenStream {
    let self_ident = &cx.self_ident;
    let (pushes, block_expr) =
        emit_runtime_super_args(cx, current_params, args, kwargs, zsuper, block);
    quote! {
        {
            let mut __super_args: Vec<zeo_rt::RubyValue> = Vec::new();
            #(#pushes)*
            zeo_rt::value_super(
                &#self_ident,
                #mname,
                &__super_args,
                #block_expr,
            )?
        }
    }
}
/// Build the forwarded argument pushes + block expression for a RUNTIME-
/// dispatched `super` (`send_super_from`/`value_super`) -- explicit
/// `super(a, b)` args, or, for bare `super`, the current method's own positional
/// parameters (required / optional / splatted `*rest` / post). Keyword arguments
/// append as one trailing Hash (the G2 convention). A literal block forwards; a
/// bare `super` without one passes `None` (the native builtins take no block).
fn emit_runtime_super_args(
    cx: &Ctx,
    current_params: &Params,
    args: &[NodeId],
    kwargs: &[KwArg],
    zsuper: bool,
    block: Option<NodeId>,
) -> (Vec<TokenStream>, TokenStream) {
    let mut pushes: Vec<TokenStream> = Vec::new();
    if zsuper {
        for name in &current_params.required {
            let id = safe_ident(name);
            pushes.push(quote! { __super_args.push(#id.clone()); });
        }
        for (name, _) in &current_params.optional {
            let id = safe_ident(name);
            pushes.push(quote! { __super_args.push(#id.clone()); });
        }
        if let Some(Some(name)) = &current_params.rest {
            let id = safe_ident(name);
            // The `*rest` local is a `RubyValue::Array` post-prologue -- splat
            // its elements (the same idiom `ArrayElem::Splat` uses at call sites).
            pushes.push(quote! {
                __super_args.extend((#id).as_array_unchecked().lock().iter().cloned());
            });
        }
        for name in &current_params.post {
            let id = safe_ident(name);
            pushes.push(quote! { __super_args.push(#id.clone()); });
        }
        // Bare `super` forwards the current method's KEYWORD arguments too
        // (CRuby's zsuper takes everything) -- appended as one trailing
        // Hash, the same G2 convention the explicit path uses. A `**kwrest`
        // merges at its position (last, matching binding order).
        if !current_params.keywords.is_empty()
            || matches!(&current_params.keyword_rest, Some(Some(_)))
        {
            let mut kw_pushes: Vec<TokenStream> = Vec::new();
            for kw in &current_params.keywords {
                let key = match kw {
                    crate::hir::KeywordParam::Required(n)
                    | crate::hir::KeywordParam::Optional(n, _) => n,
                };
                let id = safe_ident(key);
                kw_pushes.push(quote! {
                    zeo_rt::hash_set(
                        &__kw,
                        zeo_rt::RubyValue::Symbol(zeo_rt::Symbol::intern(#key)),
                        #id.clone(),
                    );
                });
            }
            if let Some(Some(krest)) = &current_params.keyword_rest {
                let id = safe_ident(krest);
                kw_pushes.push(quote! {
                    for (__k, __v) in (#id).as_hash_unchecked().lock().values().cloned().collect::<Vec<_>>() {
                        zeo_rt::hash_set(&__kw, __k, __v);
                    }
                });
            }
            pushes.push(quote! {
                {
                    let __kw = zeo_rt::hash_new(vec![]);
                    #(#kw_pushes)*
                    // An empty `**kwrest` (and no keywords) forwards
                    // NOTHING -- a trailing empty Hash would bind as a
                    // positional in the parent.
                    if zeo_rt::hash_len(&__kw) > 0 {
                        __super_args.push(zeo_rt::RubyValue::Hash(__kw));
                    }
                }
            });
        }
    } else {
        for &a in args {
            let e = emit_expr(cx, a);
            let e = crate::codegen::expr::box_if_object_typed(cx, a, e);
            pushes.push(quote! { __super_args.push(#e); });
        }
        if !kwargs.is_empty() {
            let inserts =
                crate::codegen::collections::emit_kwarg_inserts(cx, kwargs, &quote! { __kw });
            pushes.push(quote! {
                {
                    let __kw = zeo_rt::hash_new(vec![]);
                    #inserts
                    __super_args.push(zeo_rt::RubyValue::Hash(__kw));
                }
            });
        }
    }
    let block_expr = match block {
        Some(b) => {
            let proc_value = super::procs::emit_proc_value(cx, b);
            quote! { Some(#proc_value) }
        }
        // No literal block: real Ruby forwards the CURRENT method's block
        // (the splice saw `__blk` in scope for free; the runtime dispatch
        // must pass it explicitly). Inside a real Proc the method's `__blk`
        // isn't in scope -- `None` keeps that shape compiling, matching the
        // splice's own reach.
        None if !cx.in_real_proc => quote! { __blk.clone() },
        None => quote! { None },
    };
    (pushes, block_expr)
}
