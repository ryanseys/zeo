//! Emits a `TokenStream` for every HIR node except `LocalWrite` used as a
//! plain body statement (see `stmt.rs`). Every fragment produced here
//! evaluates to a bare `zeo_rt::RubyValue` -- any fallible sub-call (a
//! method dispatch that returns `Result<RubyValue, Signal>`) already has `?`
//! applied internally, so callers can always splice an `emit_expr` result
//! wherever a plain `RubyValue`-typed expression is expected, with no
//! wrapping of their own.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use quote::quote;

use super::Ctx;
use super::call::emit_call;
use super::collections::{
    emit_array_lit, emit_flip_flop, emit_hash_lit, emit_range_lit, emit_string_lit,
};
use super::ident::safe_ident;
use super::loops::{emit_break, emit_for, emit_loop, emit_next, emit_redo, emit_while};
use crate::compiler::ClassId;
use crate::compiler::FMap;
use crate::hir::{ArrayElem, HirNode, NodeId};
use crate::types::{TyKind, infer_type_with_locals};
use proc_macro2::TokenStream;

/// A node's static type, given the enclosing scope's local-type context --
/// the one place `codegen` should call into `types::infer_type_with_locals`,
/// so every dispatch decision (receiver class, numeric-operator eligibility)
/// sees the same local-aware inference.
pub fn infer(cx: &Ctx, id: NodeId) -> TyKind {
    // A `for`-loop's own index variable overrides the enclosing scope's flat
    // (position-insensitive) local-type map for reads of that exact name --
    // see `Ctx::for_var_override`'s docs for why the map alone can't express
    // this.
    if let (HirNode::LocalRead(name), Some((var, ty))) =
        (&cx.compiler.hir[id], &cx.for_var_override)
        && name == var
    {
        return *ty;
    }
    // `self`'s static type is the CURRENT method's own receiver class --
    // `types::infer_type_with_locals` has no notion of "current class" at
    // all (it's a context-free per-node classifier), so this is handled here
    // instead, the same way `for_var_override` is. Lets `self.foo(...)`/
    // implicit-self dispatch resolve Path 1 exactly like any other
    // statically-known-class receiver.
    if matches!(cx.compiler.hir[id], HirNode::SelfRef) {
        // Inside an escaping block, `self` is a `RubyValue` parameter whose
        // class isn't knowable at compile time -- `instance_exec` can run the
        // block under any receiver at all. `Poly` is exactly that ("dispatch
        // dynamically"), and it's what keeps `self.foo` inside a block from
        // compiling to a Path 1 direct call on a class the receiver may not
        // even be.
        if cx.self_is_dynamic {
            return TyKind::Poly;
        }
        if let Some(cid) = cx.current_class {
            // Inside a reopened BUILTIN class's method, `self`
            // is the receiver VALUE (the free function's `__self:
            // RubyValue` parameter), so it types as the builtin's own
            // static kind (`TyKind::Str` inside `class String`) -- NOT
            // `Object(cid)`, whose repr is an unboxed `Arc<Concrete>` no
            // builtin has. That keeps `self.length`/`self + other` on the
            // same static fast paths any other builtin-typed receiver gets.
            // (`Object` -- top-level defs -- types as `Poly`: its `__self`
            // can be any value at all once dispatch reaches the MRO tail.)
            if cx.compiler.value_backed(cid) {
                return builtin_self_ty(cid);
            }
            return TyKind::Object(cid);
        }
    }
    infer_type_with_locals(
        cx.compiler,
        cx.defining_class,
        cx.box_id,
        &cx.local_types,
        id,
    )
}

/// The static type of `self` inside a reopened BUILTIN class's methods --
/// the inverse of `infer_any_class`'s TyKind->ClassId mapping below, for
/// exactly the value kinds that HAVE a static TyKind. The rest (`NilClass`/
/// `TrueClass`/`FalseClass`) stay `Poly`: their `__self` is still a real
/// `RubyValue` at runtime, just dispatched dynamically.
pub(super) fn builtin_self_ty(cid: ClassId) -> TyKind {
    use crate::compiler::{
        ARRAY_CLASS, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS, MATCH_DATA_CLASS,
        MUTEX_CLASS, PROC_CLASS, QUEUE_CLASS, RACTOR_CLASS, RANGE_CLASS, REGEXP_CLASS,
        STRING_CLASS, SYMBOL_CLASS, THREAD_CLASS,
    };
    match cid {
        INTEGER_CLASS => TyKind::Int,
        FLOAT_CLASS => TyKind::Float,
        STRING_CLASS => TyKind::Str,
        SYMBOL_CLASS => TyKind::Symbol,
        ARRAY_CLASS => TyKind::Array,
        HASH_CLASS => TyKind::Hash,
        RANGE_CLASS => TyKind::Range,
        PROC_CLASS => TyKind::Proc,
        REGEXP_CLASS => TyKind::Regexp,
        MATCH_DATA_CLASS => TyKind::MatchData,
        FIBER_CLASS => TyKind::Fiber,
        THREAD_CLASS => TyKind::Thread,
        MUTEX_CLASS => TyKind::Mutex,
        QUEUE_CLASS => TyKind::Queue,
        RACTOR_CLASS => TyKind::Ractor,
        _ => TyKind::Poly,
    }
}

/// The receiver's statically-known class, if any -- the entire input to the
/// Path 1 / Path 2 dispatch decision.
pub fn infer_class(cx: &Ctx, id: NodeId) -> Option<ClassId> {
    match infer(cx, id) {
        TyKind::Object(cid) => Some(cid),
        _ => None,
    }
}

/// Like `infer_class`, but ALSO resolves a statically-known BUILT-IN
/// primitive type (`Int`/`Float`/`Str`/`Symbol`/`Array`/`Hash`/`Range`/
/// `Proc`) to its reserved `ClassId` (see `compiler::BUILTIN_CLASSES`).
/// Deliberately kept separate from `infer_class` itself: that function's
/// `Some(cid)` result is used all over `codegen::call` to mean "there's a
/// real generated Rust struct here, safe to call `#class_ident::new_handle`/
/// dispatch a static method on it" -- which is FALSE for a built-in type (no
/// struct exists for `Integer`/`Array`/etc., see `codegen::mod`'s
/// builtin-skipping filters). This helper is for the narrower set of call
/// sites that only need the class's IDENTITY for an ancestry/registry check
/// (`is_a?`/`kind_of?`/`respond_to?`), not a constructible struct.
pub fn infer_any_class(cx: &Ctx, id: NodeId) -> Option<ClassId> {
    use crate::compiler::{
        ARRAY_CLASS, CLASS_CLASS, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS,
        MATCH_DATA_CLASS, MODULE_CLASS, MUTEX_CLASS, PROC_CLASS, RACTOR_CLASS, RANGE_CLASS,
        REGEXP_CLASS, STRING_CLASS, SYMBOL_CLASS, THREAD_CLASS,
    };
    match infer(cx, id) {
        TyKind::Object(cid) => Some(cid),
        TyKind::Int => Some(INTEGER_CLASS),
        TyKind::Float => Some(FLOAT_CLASS),
        TyKind::Str => Some(STRING_CLASS),
        TyKind::Symbol => Some(SYMBOL_CLASS),
        TyKind::Array => Some(ARRAY_CLASS),
        TyKind::Hash => Some(HASH_CLASS),
        TyKind::Range => Some(RANGE_CLASS),
        TyKind::Proc => Some(PROC_CLASS),
        TyKind::Regexp => Some(REGEXP_CLASS),
        TyKind::MatchData => Some(MATCH_DATA_CLASS),
        TyKind::Fiber => Some(FIBER_CLASS),
        TyKind::Thread => Some(THREAD_CLASS),
        TyKind::Mutex => Some(MUTEX_CLASS),
        // `TyKind::Queue` covers BOTH `Queue` and `SizedQueue` (one runtime
        // representation, told apart per value -- see `types.rs`'s ClassRef
        // arms), so a queue-typed receiver's class identity is not a static
        // fact: `is_a?`/`instance_of?`/`respond_to?` must ask at run time.
        TyKind::Queue => None,
        TyKind::Ractor => Some(RACTOR_CLASS),
        // A class VALUE's own class is `Class` (or `Module` for a module
        // value) -- what `Widget.is_a?(Class)`/`.respond_to?` consult.
        TyKind::ClassObj(cid) => Some(if cx.compiler.class(cid).is_module {
            MODULE_CLASS
        } else {
            CLASS_CLASS
        }),
        TyKind::Poly => None,
    }
}

/// A Rust expression of type `zeo_rt::Symbol` (not `RubyValue`) -- used
/// for `send`'s second argument. A literal `:sym` skips the
/// box-then-immediately-unwrap round trip; anything else goes through the
/// runtime coercion (Symbol or String, real Ruby's rule -- `x.send("name")`
/// is legal; a non-name value raises CRuby's TypeError shape).
pub fn emit_symbol_expr(cx: &Ctx, id: NodeId) -> TokenStream {
    if let HirNode::SymbolLit(s) = &cx.compiler.hir[id] {
        return super::pooled_sym(s);
    }
    let e = emit_expr(cx, id);
    quote! { zeo_rt::method_name_symbol(&(#e))? }
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
/// fully faithful needs real per-instance/per-callsite tracking zeo
/// doesn't have yet.
/// Globals CRuby predefines, so `defined?` always answers `"global-variable"`
/// for them regardless of assignment (`$!`, `$stdout`, `$0`, ...). The
/// match-data globals (`$~`, `$&`, `$1`, ...) are NOT here -- they lower to
/// `LastMatchRef` and are defined only when a match participated.
fn is_predefined_global(name: &str) -> bool {
    matches!(
        name,
        "$!" | "$@"
            | "$;"
            | "$,"
            | "$/"
            | "$\\"
            | "$."
            | "$<"
            | "$>"
            | "$_"
            | "$0"
            | "$*"
            | "$:"
            | "$\""
            | "$$"
            | "$?"
            | "$DEBUG"
            | "$VERBOSE"
            | "$FILENAME"
            | "$PROGRAM_NAME"
            | "$stdin"
            | "$stdout"
            | "$stderr"
            | "$LOAD_PATH"
            | "$LOADED_FEATURES"
    )
}

/// `defined?` of a literal composed of `nodes` (an array's elements, a hash's
/// keys+values): `"expression"` only when every node is itself defined, else
/// `nil`. An empty list is defined.
fn defined_all_or_nil(cx: &Ctx, nodes: &[NodeId]) -> TokenStream {
    let expr_str = quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new("expression".to_string())) };
    if nodes.is_empty() {
        return expr_str;
    }
    let checks = nodes.iter().map(|&n| {
        let d = emit_defined(cx, n);
        quote! { !(#d).is_nil() }
    });
    quote! { if #(#checks)&&* { #expr_str } else { zeo_rt::RubyValue::Nil } }
}

fn emit_defined(cx: &Ctx, id: NodeId) -> TokenStream {
    // `defined?(yield)` is decided at RUNTIME: it answers `"yield"` only when
    // the enclosing method actually received a block, else `nil` -- the same
    // `__blk.is_some()` test that backs `block_given?`. (The Yield node's
    // presence here also flags this body as a bare-block user, so `__blk` is
    // in scope.)
    if matches!(&cx.compiler.hir[id], HirNode::Yield(_)) {
        // Outside a method there is no `__blk` binding to test -- and no
        // block either, which is the answer. `defined?(yield)` is legal
        // anywhere (CRuby's `DEFINED_YIELD` reads the local EP's block
        // handler), including where a bare `yield` is a SyntaxError, so
        // this must answer rather than refuse: lowering it to the missing
        // binding made the generated Rust fail to compile.
        if !cx.has_blk_binding {
            return quote! { zeo_rt::RubyValue::Nil };
        }
        return quote! {
            if __blk.is_some() {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("yield".to_string()))
            } else {
                zeo_rt::RubyValue::Nil
            }
        };
    }
    // `defined?(super)` answers "super" only when a super target exists past
    // this method's own class on the receiver's chain, else nil -- the same
    // resolution the emitted `super` walks, probed instead of called. A
    // class-method context (`current_class.is_none()`, `super_calls`' own
    // test) keeps the static classification.
    if matches!(&cx.compiler.hir[id], HirNode::SuperCall { .. })
        && cx.current_class.is_some()
        && let (Some(dc), Some(m)) = (cx.defining_class, cx.current_method.as_ref())
    {
        let dcid = dc.0;
        let name_sym = super::pooled_sym(m);
        let recv =
            super::call::boxed_implicit_self(cx).expect("every context has an implicit self");
        return quote! {
            if zeo_rt::super_defined(&#recv, zeo_rt::ClassId(#dcid), #name_sym) {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("super".to_string()))
            } else {
                zeo_rt::RubyValue::Nil
            }
        };
    }
    // `defined?(a_method_call)` answers `"method"` only when the receiver
    // actually responds to it, else `nil` -- CRuby evaluates the receiver and
    // probes it (a bare undefined name like `defined?(missing_thing)` is a
    // no-receiver call and answers nil, not "method"). An implicit-self call
    // includes private methods (`defined?(puts)`); an explicit receiver checks
    // its public surface. `New`/etc. keep the static "method".
    if let HirNode::Call { receiver, name, .. } = &cx.compiler.hir[id] {
        let receiver = *receiver;
        let name = name.clone();
        let (recv, include_all) = match receiver {
            None => (
                super::call::boxed_implicit_self(cx).expect("every context has an implicit self"),
                quote! { true },
            ),
            Some(rid) => {
                let e = emit_expr(cx, rid);
                (box_if_object_typed(cx, rid, e), quote! { false })
            }
        };
        let name_sym = super::pooled_sym(&name);
        // `defined?` consults `respond_to_missing?` too (a method_missing
        // method answers "method"), but SWALLOWS a raise from it (returning
        // nil) -- `unwrap_or(false)`, not `?`, keeps that exception-safety.
        //
        // The RECEIVER gets the same protection, and needs it more: it is
        // really evaluated (`defined?(side.size)` does call `side`), so a
        // chain whose middle link is missing raises where the whole point
        // of the idiom is to answer without a rescue. CRuby installs a
        // catch entry over the entire expression with a push-nil handler
        // (`compile.c` `defined_expr`), which swallows every raise, not
        // just NoMethodError -- `defined?((1/0).to_s)` is nil too.
        // Arguments are never emitted here at all, matching ruby:
        // `defined?([].push(boom))` answers "method" without calling boom.
        return quote! {
            (|| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
                Ok(
                    if zeo_rt::responds_to_or_missing(&#recv, #name_sym, #include_all)
                        .unwrap_or(false)
                    {
                        zeo_rt::RubyValue::Str(zeo_rt::string_new("method".to_string()))
                    } else {
                        zeo_rt::RubyValue::Nil
                    },
                )
            })()
            .unwrap_or(zeo_rt::RubyValue::Nil)
        };
    }
    // `defined?` of a RUNTIME-CONDITIONAL class: registered, but whether the
    // constant exists is settled by its guarded body having run. Asked of the
    // concealment table, the same authority every other by-name path consults.
    if let Some(cid) = conditional_class_of_defined_form(cx, id) {
        let cls = cid.0;
        // ...and a `private_constant` is still nil to `defined?`, whichever
        // authority answers the existence half. Checked at run time so a later
        // `public_constant` restores it.
        let private = defined_form_private_check(cx, id);
        return quote! {
            if #private {
                zeo_rt::RubyValue::Nil
            } else if zeo_rt::class_revealed(zeo_rt::ClassId(#cls)) {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("constant".to_string()))
            } else {
                zeo_rt::RubyValue::Nil
            }
        };
    }
    // `defined?(Scope::NAME)` where the scope is a known class/module but the
    // name isn't statically there. Folding that to nil is wrong the moment
    // anything does `const_set` -- `uri/common.rb`'s `remove_const(:Parser) if
    // defined?(::URI::Parser)` guards a constant it creates itself, so the
    // answer has to change over the program's life. The scope is still
    // resolved at compile time; only the membership test is deferred.
    if let HirNode::QualifiedConstRead(scope, name) = &cx.compiler.hir[id]
        && super::constfold::const_form_resolves(cx, id) != Some(true)
        && let Some(scope_id) = cx.resolve_class(scope)
    {
        let scope_id = scope_id.0;
        let name = name.as_str();
        // A `private_constant` is nil to `defined?`, even though
        // `const_defined?` still answers true for it -- two different
        // questions, and this is the one the scope operator gates.
        // Checked at run time so a later `public_constant` restores it.
        return quote! {
            if zeo_rt::const_is_private(#scope_id, #name) {
                zeo_rt::RubyValue::Nil
            } else if zeo_rt::const_defined_in(zeo_rt::ClassId(#scope_id), #name) {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("constant".to_string()))
            } else {
                zeo_rt::RubyValue::Nil
            }
        };
    }
    // `defined?(obj::NAME)` -- the scope is a value, so only the run time can
    // answer. It is `"constant"` when the scope operator finds the name and nil
    // otherwise, including when the scope is not a module at all: `defined?`
    // swallows that TypeError rather than raising it (oracle-verified against
    // `defined?(1::Here)`).
    if let HirNode::DynConstRead { scope, name, .. } = &cx.compiler.hir[id] {
        let s = emit_expr(cx, *scope);
        let s = box_if_object_typed(cx, *scope, s);
        let name = name.as_str();
        // Same closure the receiver case above uses, for the same reason: the
        // scope really is evaluated, and CRuby's catch entry over the whole
        // expression turns a raise from it into nil rather than a propagated
        // error.
        return quote! {
            (|| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
                Ok(if zeo_rt::scope_const_defined(&#s, #name) {
                    zeo_rt::RubyValue::Str(zeo_rt::string_new("constant".to_string()))
                } else {
                    zeo_rt::RubyValue::Nil
                })
            })()
            .unwrap_or(zeo_rt::RubyValue::Nil)
        };
    }
    let global_var =
        quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new("global-variable".to_string())) };
    // `defined?($g)` is `"global-variable"` only if the global has been
    // assigned (a predefined special like `$!`/`$stdout` always is); a
    // never-written user global answers nil.
    if let HirNode::GlobalRead(name) = &cx.compiler.hir[id] {
        if is_predefined_global(name) {
            return global_var;
        }
        let bx = cx.box_id;
        return quote! {
            if zeo_rt::global_defined(#bx, #name) { #global_var } else { zeo_rt::RubyValue::Nil }
        };
    }
    // The match globals: `$~` (the MatchData holder) is always defined; a
    // capture (`$&`/`$1`/`$'`/`$+`) is defined only when it participated in
    // the current last match.
    if let HirNode::LastMatchRef(which) = &cx.compiler.hir[id] {
        use crate::hir::LastMatch;
        let present = match which {
            LastMatch::Data => return global_var,
            LastMatch::Group(n) => quote! { !zeo_rt::last_match_group(#n).is_nil() },
            LastMatch::Pre => quote! { !zeo_rt::last_match_pre().is_nil() },
            LastMatch::Post => quote! { !zeo_rt::last_match_post().is_nil() },
            LastMatch::LastGroup => quote! { !zeo_rt::last_match_last_group().is_nil() },
        };
        return quote! {
            if #present { #global_var } else { zeo_rt::RubyValue::Nil }
        };
    }
    // An array/hash literal answers "expression" only if EVERY element is
    // itself defined -- CRuby recurses (`defined?([Missing, Array])` is nil,
    // `defined?([1, Array])` is "expression"). An empty literal is defined.
    if let HirNode::ArrayLit(elems) = &cx.compiler.hir[id] {
        let nodes: Vec<NodeId> = elems
            .iter()
            .map(|e| match e {
                crate::hir::ArrayElem::Single(n) | crate::hir::ArrayElem::Splat(n) => *n,
            })
            .collect();
        return defined_all_or_nil(cx, &nodes);
    }
    if let HirNode::HashLit(entries) = &cx.compiler.hir[id] {
        let mut nodes = Vec::new();
        for e in entries {
            match e {
                crate::hir::KwArg::Pair(k, v) => nodes.extend([*k, *v]),
                crate::hir::KwArg::DoubleSplat(n) => nodes.push(*n),
            }
        }
        return defined_all_or_nil(cx, &nodes);
    }
    // `defined?(@iv)` is "instance-variable" only if the ivar is actually set
    // on self (a never-assigned `@iv` answers nil), checked at runtime.
    if let HirNode::IvarRead(name) = &cx.compiler.hir[id] {
        let recv =
            super::call::boxed_implicit_self(cx).expect("every context has an implicit self");
        let name = name.as_str();
        // Guarded like a read: `defined?(@zz)` inside a Ractor raises
        // `Ractor::IsolationError` in CRuby rather than answering nil.
        return quote! {
            if { let __r = #recv; zeo_rt::ivar_isolation_check(&__r)?; zeo_rt::ivar_defined(&__r, #name) } {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("instance-variable".to_string()))
            } else {
                zeo_rt::RubyValue::Nil
            }
        };
    }
    // `defined?(@@x)` is "class variable" only if the cvar is set AT THIS
    // MOMENT, checked at runtime like `defined?(@iv)` above -- a static
    // answer breaks every runs-once guard of the shape
    // `unless defined?(@@configured) ... @@configured = true` (rspec's
    // `ensure_example_groups_are_configured`, which wires the mock adapter
    // into ExampleGroup exactly once, on the FIRST `describe`).
    if let HirNode::ClassVarRead(name) = &cx.compiler.hir[id] {
        let owner = cvar_owner_id(cx, name);
        let name = name.as_str();
        return quote! {
            if zeo_rt::cvar_defined(#owner, #name) {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("class variable".to_string()))
            } else {
                zeo_rt::RubyValue::Nil
            }
        };
    }
    let classification: Option<&str> = match &cx.compiler.hir[id] {
        HirNode::LocalRead(name) => {
            if cx.local_types.contains_key(name) {
                Some("local-variable")
            } else {
                None
            }
        }
        // Handled by the runtime check above.
        HirNode::ClassVarRead(_) => unreachable!("defined?(@@x) returned early"),
        // A constant reference classifies as `"constant"` only when it
        // provably resolves at compile time; an unresolvable `Scope::NAME`/
        // bare-`NAME` answers `nil` (CRuby's `defined?` on a missing constant).
        HirNode::ClassRef(_) | HirNode::QualifiedConstRead(..) => {
            match super::constfold::const_form_resolves(cx, id) {
                Some(true) => Some("constant"),
                _ => None,
            }
        }
        // Same narrowing posture as `IvarRead` just above (classified
        // whenever it's syntactically a `$foo` read, without tracking
        // whether it was ever actually assigned -- a documented
        // approximation of real Ruby, which returns `nil` for a
        // never-assigned global specifically, unlike every OTHER
        // classification here).
        // Real Ruby: `defined?(self)` is always `"self"`, everywhere --
        // confirmed via `ruby -e 'puts defined?(self)'` -- unlike every other
        // classification here, this needs no further check at all.
        HirNode::SelfRef => Some("self"),
        HirNode::New { .. }
        | HirNode::SuperCall { .. }
        | HirNode::Ffi(_)
        | HirNode::Eval(_)
        // A box-scoped splice classifies like the eval it rode in on; a
        // handle literal is an ordinary expression value.
        | HirNode::BoxScope { .. }
        | HirNode::BoxHandle(_)
        | HirNode::BlockGiven
        | HirNode::Raise(..) => Some("method"),
        // The keyword literals answer their own name (`defined?(nil) == "nil"`),
        // like `defined?(self) == "self"`, not the generic "expression".
        HirNode::NilLit => Some("nil"),
        HirNode::BoolLit(true) => Some("true"),
        HirNode::BoolLit(false) => Some("false"),
        HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::StringLit(_)
        | HirNode::RegexpLit(..)
        | HirNode::RangeLit { .. }
        | HirNode::FlipFlop { .. }
        | HirNode::And(..)
        | HirNode::Or(..)
        | HirNode::Defined(_)
        | HirNode::If { .. }
        | HirNode::CaseWhen { .. }
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::Begin { .. }
        | HirNode::ConstReadOrNil(..)
        | HirNode::PreExec(_)
        | HirNode::Seq(_)
        | HirNode::While { .. }
        | HirNode::Loop { .. }
        | HirNode::For { .. }
        | HirNode::Lambda { .. } => Some("expression"),
        // Every assignment form answers "assignment" -- the argument is never
        // evaluated, so an undefined operand doesn't matter (`defined?(x = 2)`,
        // `defined?(@iv += 1)`, `defined?(a, b = 1, 2)`).
        HirNode::LocalWrite(..)
        | HirNode::IvarWrite(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::MultiWrite { .. } => Some("assignment"),
        // Handled by the early returns at the top of this function.
        HirNode::Call { .. }
        | HirNode::Yield(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::ArrayLit(_)
        | HirNode::HashLit(_)
        | HirNode::IvarRead(_)
        | HirNode::DynConstRead { .. } => {
            unreachable!("defined? runtime-checked nodes handled above")
        }
        HirNode::Break(_) | HirNode::Next(_) | HirNode::Redo | HirNode::Return(_) | HirNode::Retry => None,
        HirNode::AliasGlobal(..) => Some("expression"),
        HirNode::Block { .. }
        | HirNode::Program(_)
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::DefHook { .. }
        | HirNode::MethodRedefine { .. }
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::ConstantVisibility { .. } => None,
    };
    match classification {
        Some(s) => quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new(#s.to_string())) },
        None => quote! { zeo_rt::RubyValue::Nil },
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
    // A `defined?`-guarded branch whose constant provably can't exist folds
    // away at compile time: the dead branch is never EMITTED (its body may be
    // MRI-only/uncompilable code), matching CRuby's own reachability. The
    // condition itself is a pure guard, so dropping it elides no side effect.
    // A block wrapper (`{ ... }`) around the surviving branch: `emit_body_boxed`
    // returns a STATEMENT SEQUENCE ending in a tail expression, which is only
    // valid Rust in an expression slot once braced -- an `if` expression can sit
    // in a value position (`x = if COND ...`), and a multi-statement branch
    // would otherwise splice bare statements where an expression is expected.
    match super::constfold::static_cond(cx, cond) {
        Some(true) => {
            let body = super::stmt::emit_body_boxed(cx, then_body);
            return quote! { { #body } };
        }
        Some(false) => {
            let body = super::stmt::emit_body_boxed(cx, else_body);
            return quote! { { #body } };
        }
        None => {}
    }
    // Boxed: an Object-typed condition is an unboxed `Arc<Concrete>` with
    // no `truthy()` (always truthy in Ruby, but the boxing keeps one code
    // shape).
    let cond_expr = box_if_object_typed(cx, cond, emit_expr(cx, cond));
    // Boxed arms: the if-expression itself types as `Poly` (no `If` arm in
    // `types.rs`), so both arms must agree on `RubyValue` -- an
    // Object-typed arm tail would otherwise be an unboxed `Arc<Concrete>`
    // (a real rustc E0308 whenever the arms' classes differ, found by the
    // conformance corpus).
    let then_val = super::stmt::emit_body_boxed(cx, then_body);
    let else_val = super::stmt::emit_body_boxed(cx, else_body);
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
    arms: &[(Vec<ArrayElem>, Vec<NodeId>)],
    else_body: &[NodeId],
) -> TokenStream {
    let has_subject = subject.is_some();
    // Boxed arms, same reasoning as `emit_if` (the whole `case` types
    // `Poly`; every arm must agree on `RubyValue`).
    let mut chain = super::stmt::emit_body_boxed(cx, else_body);

    for (values, body) in arms.iter().rev() {
        let body_val = super::stmt::emit_body_boxed(cx, body);
        let mut check: Option<TokenStream> = None;
        for elem in values {
            let this_check = match elem {
                ArrayElem::Single(v) => {
                    let v_expr = box_if_object_typed(cx, *v, emit_expr(cx, *v));
                    if has_subject {
                        // `case_eq`, which DISPATCHES the candidate's `===`
                        // (see its docs): a user class defining `===` is the
                        // whole point of `case`, and the native ladder
                        // silently took the wrong branch for one whose `===`
                        // differs from its `==`.
                        quote! { zeo_rt::case_eq(&#v_expr, &__subject)? }
                    } else {
                        quote! { (#v_expr).truthy() }
                    }
                }
                // `when *candidates` -- the same test as a listed value,
                // over every element, with the count known only at runtime.
                // `any` short-circuits, so the elements after a hit are
                // never tested, matching the `||` chain the listed form
                // builds.
                ArrayElem::Splat(v) => {
                    let v_expr = emit_expr(cx, *v);
                    if has_subject {
                        quote! { zeo_rt::case_eq_any(&#v_expr, &__subject)? }
                    } else {
                        // A subject-less `case` tests truthiness, so a
                        // splatted candidate list is "is any of them
                        // truthy" -- coerced like every splat, not unwrapped
                        // as a bare Array.
                        quote! {
                            {
                                let mut __sp = Vec::new();
                                zeo_rt::array_splat_into(&mut __sp, &(#v_expr))?;
                                __sp.iter().any(|__c| __c.truthy())
                            }
                        }
                    }
                }
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
            // Boxed if Object-typed: `case w when Widget` --
            // an unboxed `Arc<Concrete>` subject can't feed `rb_case_eq`'s
            // `&RubyValue` (unexercised before class candidates existed:
            // they were a compile-time rejection).
            let subject_expr = box_if_object_typed(cx, s, subject_expr);
            quote! { { let __subject = #subject_expr; #chain } }
        }
        None => chain,
    }
}

pub fn emit_expr(cx: &Ctx, id: NodeId) -> TokenStream {
    match &cx.compiler.hir[id] {
        HirNode::IntegerLit(v) => quote! { zeo_rt::RubyValue::Int(#v) },
        // The bignum/rational/imaginary literals -- digits
        // baked as array literals, assembled by the runtime at the use
        // site (no compile-time bigint dependency, no string parsing).
        HirNode::BigIntegerLit { negative, digits } => {
            quote! { zeo_rt::int_from_u32_digits(#negative, &[#(#digits),*]) }
        }
        HirNode::RationalLit {
            negative,
            num_digits,
            den_digits,
        } => {
            quote! {
                zeo_rt::rational_from_digits(
                    #negative,
                    &[#(#num_digits),*],
                    &[#(#den_digits),*],
                )
            }
        }
        HirNode::ImaginaryLit(inner) => {
            let inner_expr = emit_expr(cx, *inner);
            quote! { zeo_rt::complex_from_literal(#inner_expr) }
        }
        // A non-finite literal (`1e400` overflows to Infinity at parse time,
        // and `Float::NAN` folds here too) cannot be emitted as a Rust float
        // TOKEN: `Literal::f64_suffixed` asserts `is_finite()` and panics
        // inside proc-macro2. Emit the corresponding `f64` constant PATH
        // instead -- same value, and it survives tokenization.
        HirNode::FloatLit(v) if !v.is_finite() => {
            let konst = if v.is_nan() {
                quote! { f64::NAN }
            } else if *v > 0.0 {
                quote! { f64::INFINITY }
            } else {
                quote! { f64::NEG_INFINITY }
            };
            quote! { zeo_rt::RubyValue::Float(#konst) }
        }
        HirNode::FloatLit(v) => quote! { zeo_rt::RubyValue::Float(#v) },
        HirNode::Lambda {
            params,
            body,
            method_body,
        } => {
            // A `method_body` lambda IS a runtime method body -- the two parse
            // sites that build one are the `def obj.m` and `class << obj`
            // desugars, both of which install through
            // `define_singleton_method`. So a `super` inside it must resolve
            // through the runtime method-frame stack, exactly as for a `def`
            // in expression position: there is no compile-time singleton class
            // to splice an ancestor chain against, and `emit_super`
            // would otherwise fall through to `defining_class` and panic with
            // "`super` outside a method".
            let mut body_cx = cx.clone();
            if *method_body {
                body_cx.runtime_super_params = Some(std::rc::Rc::new((**params).clone()));
            }
            super::call::emit_lambda_value(
                &body_cx,
                params,
                body,
                *method_body,
                super::source_location(cx.compiler, id).map(|(f, l)| (f.to_string(), l)),
            )
        }
        HirNode::SymbolLit(s) => {
            let sym = super::pooled_sym(s);
            quote! { zeo_rt::RubyValue::Symbol(#sym) }
        }
        HirNode::NilLit => quote! { zeo_rt::RubyValue::Nil },
        HirNode::BoolLit(b) => quote! { zeo_rt::RubyValue::Bool(#b) },
        // `private_constant :Hidden` as the last statement of a module body,
        // which is where rspec-openapi and three others put it. The
        // visibility itself is a compile-time fact with no emission at all;
        // what is left is the value ruby answers, which is the module
        // (oracle-checked). A body with no enclosing class is `main`'s, whose
        // constants live on Object.
        HirNode::ConstantVisibility { .. } => {
            let cid = cx
                .self_class()
                .cid
                .unwrap_or(crate::compiler::OBJECT_CLASS)
                .0;
            quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#cid)) }
        }
        HirNode::SelfRef => {
            // Inside an escaping block, `self` is the closure's own receiver
            // parameter -- checked FIRST, because the top-level arm below
            // would otherwise answer `main_object()` for a block written at
            // the top level and then `instance_eval`'d onto something else
            // (`cfg.instance_eval { self.port = 8080 }`), silently sending to
            // `main` instead of `cfg`.
            if cx.self_is_dynamic {
                // Concrete UFCS, not `.clone()` method syntax (see the note
                // at the bottom of this arm) and not generic `Clone::clone`
                // either: a dynamic self can be bound as `&RubyValue` in
                // some closure shapes, where the generic form would clone
                // the REFERENCE -- the concrete form deref-coerces both
                // `&RubyValue` and owned bindings to the right argument.
                let slf = &cx.self_ident;
                return quote! { zeo_rt::RubyValue::clone(&#slf) };
            }
            // Only meaningful inside an ordinary instance method body (see
            // `hir::HirNode::SelfRef`'s docs) -- a class method/module
            // function has no `self: Arc<Self>` receiver at all in its Rust
            // signature, so referencing `self_ident` there would emit a
            // reference to a Rust binding that doesn't exist. Rejected here,
            // at codegen time, rather than letting `rustc` fail on the
            // GENERATED program with a confusing "cannot find value `self`".
            if cx.current_class.is_none() {
                // At the true TOP LEVEL (not inside any method), `self` is
                // CRuby's `main` object -- a shared runtime `Object`
                // instance. Safe inside escaping blocks too: it's a global
                // lookup, not a captured binding.
                if cx.defining_class.is_none() {
                    return quote! { zeo_rt::main_object() };
                }
                // A class method/module function/class body: `self` is the
                // class object itself. The id is a compile-time constant --
                // an inherited class method is materialized per subclass, so
                // this copy's `class_self` is already the right receiver
                // (see `Ctx::class_self`'s docs).
                let cid = cx
                    .class_self
                    .expect("a body with a defining_class but no current_class is a class-level body, which always has a class self");
                let id = cid.0;
                return quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) };
            }
            // Unboxed `Arc<Concrete>` -- exactly what an Object-typed local
            // read (`LocalStorage::Shadowed`) already returns, and exactly
            // what a call receiver needs for Path 1 dispatch (`emit_call`'s
            // `recv_expr = emit_expr(cx, recv_id)`). `cx.self_ident` is the
            // capture-alias identifier while emitting a self-capturing
            // escaping block's own body (see `Ctx::self_ident`'s docs), the
            // literal `self` receiver parameter otherwise. UFCS through the
            // `Clone` trait, NOT `.clone()` method syntax: a user Ruby method
            // named `clone` becomes an INHERENT `fn clone(self: Arc<Self>)`
            // on the generated struct, which method syntax would resolve to
            // instead of the `Arc` refcount bump (inherent beats trait).
            let slf = &cx.self_ident;
            quote! { Clone::clone(&#slf) }
        }
        HirNode::LocalRead(name) => super::hoisting::emit_local_read(cx, name),
        HirNode::And(l, r) => {
            // Ruby's `&&`/`and` returns the operand itself, not a bool --
            // `false && anything` is `false`, but `1 && 2` is `2`, not
            // `true`. A literal Rust `&&` is bool-typed and can't express
            // this, so short-circuit via an explicit `if` on `.truthy()`.
            // Operands box first: an Object-typed operand is an unboxed
            // `Arc<Concrete>` with no `truthy()`, and the two arms must
            // agree on one `RubyValue` result type anyway.
            let lhs = box_if_object_typed(cx, *l, emit_expr(cx, *l));
            let rhs = box_if_object_typed(cx, *r, emit_expr(cx, *r));
            quote! {
                { let __lhs = #lhs; if __lhs.truthy() { #rhs } else { __lhs } }
            }
        }
        HirNode::Or(l, r) => {
            let lhs = box_if_object_typed(cx, *l, emit_expr(cx, *l));
            let rhs = box_if_object_typed(cx, *r, emit_expr(cx, *r));
            quote! {
                { let __lhs = #lhs; if __lhs.truthy() { __lhs } else { #rhs } }
            }
        }
        HirNode::Defined(v) => emit_defined(cx, *v),
        HirNode::ArrayLit(elems) => emit_array_lit(cx, elems),
        HirNode::HashLit(pairs) => emit_hash_lit(cx, pairs),
        HirNode::RangeLit {
            start,
            end,
            exclusive,
        } => emit_range_lit(cx, *start, *end, *exclusive),
        HirNode::FlipFlop {
            state,
            left,
            right,
            exclusive,
        } => emit_flip_flop(cx, *state, *left, *right, *exclusive),
        HirNode::StringLit(parts) => {
            emit_string_lit(cx, parts, super::collections::literal_file_frozen(cx, id))
        }
        HirNode::RegexpLit(parts, flags) => super::collections::emit_regexp_lit(cx, parts, *flags),
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
            // (not a body statement) -- handle it faithfully to Ruby's
            // "assignment evaluates to the assigned value" semantics rather
            // than silently dropping it. `__v` is a temporary so the value
            // expression is evaluated exactly once, then written and read
            // back via whichever storage class `name` has (see
            // `hoisting::LocalStorage`'s docs).
            let v = emit_expr(cx, *value);
            let v = box_for_local_storage(cx, name, *value, v);
            let write = super::hoisting::emit_local_write(cx, name, quote! { __v });
            let read = super::hoisting::emit_local_read(cx, name);
            // `__v` is deliberately UNANNOTATED: `box_for_local_storage`
            // leaves a `Shadowed` local's RHS as the bare `Arc<Concrete>` its
            // slot holds, so naming `RubyValue` here contradicted the very
            // value being stored.
            quote! { { let __v = #v; #write #read } }
        }
        HirNode::IvarRead(name) => {
            let ident = safe_ident(name);
            // A dynamically-typed self (an escaping block's receiver, which
            // `instance_exec` may have rebound) has no statically-known
            // struct to take a field from -- resolve the ivar by name at
            // runtime. The key is the MANGLED ident, matching what
            // `ruby_class!` keys its own `ivar_get_named` arms on
            // (`stringify!($ivar)` over the same `safe_ident` output).
            //
            // Checked BEFORE `class_self`, matching `boxed_implicit_self`.
            // `class_self` is a LEXICAL fact ("this code sits in class Foo's
            // body"); `self_is_dynamic` says what `self` actually IS at
            // runtime, and when they disagree the runtime one wins. A `def`
            // nested in a `class_eval`/`Class.new` block inherits the
            // enclosing `class_self` but runs under a dynamic self, so
            // checking `class_self` first sent an INSTANCE's `@v` to the
            // class's own ivar table (it read nil). Nothing is lost when
            // `self` really is the class: `ivar_get_dyn`'s `Class` arm
            // resolves to the very same `class_ivar_get` table.
            if cx.self_is_dynamic {
                let slf = &cx.self_ident;
                let key = ident.to_string();
                // `&#slf`, not `#slf`: a dynamic self is an OWNED `RubyValue`
                // in a reopened-builtin/`Object` free function (`__self:
                // RubyValue`) but already a `&RubyValue` inside a Proc
                // closure. Borrowing covers the first and deref-coerces
                // `&&RubyValue` back to `&RubyValue` for the second.
                // The `_isolated` forms carry CRuby's Ractor guard: a dynamic
                // self is the only receiver that can be a SHAREABLE object
                // read from a non-main ractor (`Ractor.new { @n }`, whose
                // isolated Proc runs with the ractor itself as `self`).
                return match dyn_ivar_slot(cx, name) {
                    Some(slot) => {
                        quote! { zeo_rt::ivar_slot_get_dyn_isolated(&#slf, #slot, #key)? }
                    }
                    None => quote! { zeo_rt::ivar_get_dyn_isolated(&#slf, #key)? },
                };
            }
            // `self` is a CLASS object (a class-method body, or a class body
            // itself): `@x` is that class object's own ivar, which lives in
            // its own runtime table. A direct table hit, no `ivar_get_dyn`
            // match to walk.
            if let Some(cid) = cx.class_self {
                return civar_site(cid, &ident.to_string(), |site| quote! { #site.get() });
            }
            // The TOP LEVEL: `self` is `main` -- see the matching arm in
            // `emit_ivar_write_stmt`.
            if cx.current_class.is_none() {
                let key = ident.to_string();
                return quote! { zeo_rt::ivar_get_dyn(&zeo_rt::main_object(), #key) };
            }
            // `cx.self_ident` is ordinarily the literal `self`, but becomes a
            // fresh capture-alias identifier while emitting an escaping
            // block's own body that captured `self` (see `Ctx::in_proc`'s
            // docs -- `let self = ...;` is illegal Rust, so the closure
            // clones into a DIFFERENT name).
            let slf = &cx.self_ident;
            // `IvarCell::get` hands back an OWNED value, never a guard, so
            // reading the same ivar twice in one statement (`@x * @x`) is just
            // two calls. The old per-ivar `Mutex` needed a `{ let __g = ...;
            // __g.clone() }` shape here, because an unnamed `.lock()`
            // temporary lives to the end of the enclosing STATEMENT and a
            // second read of the same non-reentrant lock hung forever.
            let current = cx
                .current_class
                .expect("an ivar on a typed self has a concrete class");
            match ivar_slot(cx, current, name) {
                Some(slot) => quote! { #slf.__ivars.get(#slot) },
                None => {
                    let class_ident = super::ident::class_ident(cx.compiler, current);
                    let key = ident.to_string();
                    quote! {
                        #slf.__ivars
                            .get_named(#class_ident::__IVAR_NAMES, #key)
                            .unwrap_or(zeo_rt::RubyValue::Nil)
                    }
                }
            }
        }
        HirNode::IvarWrite(name, value) => {
            let v = emit_expr(cx, *value);
            // Boxed unconditionally (unlike a LOCAL's `box_for_local_storage`,
            // which skips it for `Shadowed` storage): an ivar's own Rust field
            // is ALWAYS `parking_lot::Mutex<RubyValue>` (see `ruby_class!`'s
            // generated struct), never an unboxed concrete-class slot, so an
            // Object-typed RHS (`@other = SomeClass.new`) must always be
            // boxed here -- confirmed the hard way (a real `rustc` type
            // mismatch storing one object instance inside another's ivar).
            let v = box_if_object_typed(cx, *value, v);
            // RHS bound to `__v` BEFORE `.lock()` is ever called -- with
            // `parking_lot::Mutex` (non-reentrant), a `#v` expression
            // that itself reads this same ivar would otherwise call `.lock()`
            // while the write guard below is already held, hanging forever
            // instead of RefCell's old clean "already borrowed" panic.
            let write = emit_ivar_write_stmt(cx, name, quote! { __v.clone() });
            quote! { { let __v: zeo_rt::RubyValue = #v; #write __v } }
        }
        HirNode::ClassVarRead(name) => {
            let owner = cvar_owner_id(cx, name);
            // The `@@x ||= v` read half tolerates an unassigned cvar (nil);
            // every other read is ruby's NameError, not nil.
            if cx
                .compiler
                .hir
                .has_flag(id, crate::hir::NodeFlag::LENIENT_CVAR_READ)
            {
                quote! { zeo_rt::cvar_get(#owner, #name) }
            } else {
                quote! { zeo_rt::cvar_get_checked(#owner, #name)? }
            }
        }
        HirNode::ClassVarWrite(name, value) => {
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: cvar storage is likewise always
            // `RubyValue` (`zeo_rt::cvar_set`'s own signature), never an
            // unboxed concrete-class slot.
            let v = box_if_object_typed(cx, *value, v);
            let write = emit_cvar_write_stmt(cx, name, quote! { __v.clone() });
            quote! { { let __v: zeo_rt::RubyValue = #v; #write __v } }
        }
        // A bare constant name -- see `HirNode::ClassRef`'s docs on its dual
        // reuse (a call receiver is intercepted before this arm ever runs,
        // in `codegen::call::emit_call`/`emit_splat_call`). Used as a plain
        // VALUE, it's either a genuine class/module reference (no first-
        // class `Class`/`Module` runtime value exists -- see the plan's Part
        // 6 scope-cut -- a clean rejection) or, when `name` isn't actually a
        // registered class, an ordinary lexically-scoped constant READ.
        // A bare/qualified constant naming a class or module used as a
        // VALUE is a first-class `RubyValue::Class` handle (retiring the
        // long-standing "no first-class Class/Module value"
        // rejection); one that names no class stays an ordinary
        // lexically-scoped constant READ.
        HirNode::ClassRef(name) => match cx.resolve_class(name) {
            Some(cid) => {
                let id = cid.0;
                // A nested class is a constant of its parent, so a
                // `private_constant :Hidden` hides `M::Hidden` exactly as it
                // hides a value constant -- and this arm resolves the class
                // statically, never reaching `emit_const_read`'s own guard.
                let path = crate::constpath::ConstPath::parse(name);
                let guard = private_const_guard(cx, path.scope(), path.base());
                if cx.compiler.class(cid).runtime_conditional {
                    // Registered but not PROMISED: whether this constant
                    // exists is settled by the guarded body having run --
                    // `NameError` until `reveal_class` fires there.
                    let fq = cx.compiler.fq_name(cid);
                    let owner = path
                        .scope()
                        .and_then(|s| cx.resolve_class(s))
                        .map_or(0, |c| c.0);
                    quote! { { #guard zeo_rt::conditional_class_ref(zeo_rt::ClassId(#id), #fq, zeo_rt::ClassId(#owner))? } }
                } else {
                    quote! { { #guard zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) } }
                }
            }
            // Not a statically known class. A JOINED name (`NS::Item`, the
            // form `constant_path_name` produces) has to read as `Item`
            // inside `NS` -- a flat read of the whole spelling finds nothing,
            // which is what a namespaced runtime class (`class NS::Item <
            // Struct.new(:a)`) would otherwise hit.
            None => {
                let path = crate::constpath::ConstPath::parse(name);
                emit_const_read(cx, path.scope(), path.base())
            }
        },
        HirNode::QualifiedConstRead(scope, name)
            if qualified_const_class(cx, scope, name).is_some() =>
        {
            let cid = qualified_const_class(cx, scope, name).expect("guarded above");
            let id = cid.0;
            let guard = private_const_guard(cx, Some(scope), name);
            if cx.compiler.class(cid).runtime_conditional {
                // Same runtime-conditional rule as the `ClassRef` arm above.
                let fq = cx.compiler.fq_name(cid);
                let owner = cx.resolve_class(scope).map_or(0, |c| c.0);
                quote! { { #guard zeo_rt::conditional_class_ref(zeo_rt::ClassId(#id), #fq, zeo_rt::ClassId(#owner))? } }
            } else {
                quote! { { #guard zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) } }
            }
        }
        HirNode::New {
            class_name,
            args,
            kwargs,
            block,
        } => super::call::emit_private_new_error(cx, class_name).unwrap_or_else(|| {
            let guard = super::call::runtime_private_new_guard(cx, class_name);
            let ctor = super::call::emit_new(cx, class_name, args, kwargs, *block);
            match guard {
                Some(g) => quote! { { #g #ctor } },
                None => ctor,
            }
        }),
        HirNode::SuperCall {
            args,
            kwargs,
            zsuper,
            block,
            block_arg,
        } => super::call::emit_super(cx, args, kwargs, *zsuper, *block, *block_arg),
        HirNode::While {
            cond,
            body,
            negate,
            post,
        } => emit_while(cx, *cond, body, *negate, *post),
        HirNode::Loop { body } => emit_loop(cx, body),
        HirNode::For {
            target,
            iterable,
            body,
        } => emit_for(cx, target, *iterable, body),
        HirNode::Break(v) => emit_break(cx, id, *v),
        HirNode::Next(v) => emit_next(cx, *v),
        HirNode::Redo => emit_redo(cx),
        HirNode::MultiWrite { targets, value } => {
            // Sub-expression form (`r = (x, y = rhs)`, `(a, b = pair)[0]`) --
            // see `stmt.rs` for the primary, statement-position case, which is
            // what makes the assigned locals visible to LATER statements
            // (nested in its own block here, so that visibility doesn't
            // matter). Yields the raw RHS verbatim, as real Ruby does.
            super::loops::emit_multi_write_value(cx, targets, *value)
        }
        // `binding` is intercepted HERE rather than in `emit_call` because it
        // is the one call whose emission needs its own node: the Binding
        // carries the call site's file and line as its `source_location`.
        HirNode::Call {
            receiver: None,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: _,
        } if name == "binding"
            && args.is_empty()
            && kwargs.is_empty()
            && block.is_none()
            && block_arg.is_none() =>
        {
            super::call::emit_binding(cx, id)
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe,
        } => emit_call(
            cx,
            *receiver,
            name,
            args,
            kwargs,
            *block,
            *block_arg,
            *safe,
            cx.compiler.hir.has_flag(id, crate::hir::NodeFlag::VCALL),
        ),
        HirNode::Block { .. } => {
            panic!(
                "internal error: a Block node should only be reached via the Call that invokes it"
            )
        }
        HirNode::GlobalRead(name) if name == "$!" => {
            // `$!` is the exception currently being handled -- the SAME
            // runtime slot a bare `raise` re-raises (`current_exception`,
            // pushed around every `rescue` clause body by `emit_begin`), not
            // an ordinary `$foo` global-table entry (which is never set, so
            // the plain read below would always answer nil). `nil` outside
            // any rescue, exactly as CRuby's own `$!`.
            quote! { zeo_rt::current_exception().unwrap_or(zeo_rt::RubyValue::Nil) }
        }
        HirNode::GlobalRead(name) if name == "$?" => {
            // `$?` reads a dedicated thread-local slot (like `$!`), the one
            // `Kernel#system`/the backtick set from their child's wait status
            // -- not the ordinary `$foo` box table, which is never written for
            // it. `nil` until the first child runs, exactly as CRuby's own.
            quote! { zeo_rt::last_child_status() }
        }
        HirNode::GlobalRead(name) => {
            // Globals are per-box tables -- the statement's own
            // defining box picks the table, no fallback layer (the CRuby
            // box model, verified in the plan's research contract).
            let bx = cx.box_id;
            quote! { zeo_rt::global_get(#bx, #name) }
        }
        // `alias $new $old` -- registers the indirection and answers nil
        // (real Ruby: `alias` is an expression whose value is nil). Runs
        // where it is written, so a later re-alias replaces an earlier one,
        // as Ruby's does. Per-box, like the table it indirects into.
        HirNode::AliasGlobal(new_name, old_name) => {
            let bx = cx.box_id;
            quote! {
                {
                    zeo_rt::global_alias(#bx, #new_name, #old_name);
                    zeo_rt::RubyValue::Nil
                }
            }
        }
        // The last-match specials read a dedicated runtime slot, not the
        // `$foo` table -- see `HirNode::LastMatchRef` and
        // `zeo_rt::lastmatch`. Not box-scoped: a match's result belongs
        // to whoever ran it, and `$~` has no per-box table to live in.
        HirNode::LastMatchRef(which) => match which {
            crate::hir::LastMatch::Data => quote! { zeo_rt::last_match() },
            crate::hir::LastMatch::Group(n) => quote! { zeo_rt::last_match_group(#n) },
            crate::hir::LastMatch::Pre => quote! { zeo_rt::last_match_pre() },
            crate::hir::LastMatch::Post => quote! { zeo_rt::last_match_post() },
            crate::hir::LastMatch::LastGroup => quote! { zeo_rt::last_match_last_group() },
        },
        HirNode::GlobalWrite(name, value) => {
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: global storage is likewise always
            // `RubyValue` (`zeo_rt::global_set`'s own signature).
            let v = box_if_object_typed(cx, *value, v);
            let bx = cx.box_id;
            quote! { { let __v: zeo_rt::RubyValue = #v; zeo_rt::global_assign(#bx, #name, __v.clone())?; __v } }
        }
        HirNode::QualifiedConstRead(scope, name) => emit_const_read(cx, Some(scope), name),
        HirNode::ConstReadOrNil(scope, name) => {
            // The `defined?`/`X ||= ...` fallback: an unregistered scope class
            // means the constant is simply absent -> `nil`, never a panic.
            match const_owner_id_opt(cx, scope.as_deref(), name) {
                Some(owner) => {
                    quote! { zeo_rt::const_get(#owner, #name).unwrap_or(zeo_rt::RubyValue::Nil) }
                }
                None => quote! { zeo_rt::RubyValue::Nil },
            }
        }
        HirNode::ConstWrite { scope, name, value } => {
            // An explicit `Scope::NAME = ...` whose scope isn't a registered
            // class raises `NameError` on the scope BEFORE the RHS is evaluated
            // (`Nope::X = (puts 1; 5)` raises without printing -- verified
            // against ruby 4.0.6), so short-circuit without emitting `value`.
            // (A bare `NAME =` never takes this branch: `const_owner_id_opt`
            // falls back to `Object`/the box surrogate for `scope: None`.)
            if const_owner_id_opt(cx, scope.as_deref(), name).is_none() {
                // ... unless the scope is a name a RUNTIME constant may hold a
                // class for (`SecretKeys::Encryptor = ...` under
                // `SecretKeys = Class.new(...)`), which is the write mirror of
                // `emit_const_read`'s own runtime path. The scope still reads
                // first, so a genuinely unset one raises the same `NameError`
                // before the right-hand side runs.
                if let Some(s) = scope.as_deref() {
                    let (head, leaf) = split_const_path(s);
                    let sc = emit_const_read(cx, head.filter(|h| !h.is_empty()), leaf);
                    let v = emit_expr(cx, *value);
                    let v = box_if_object_typed(cx, *value, v);
                    return quote! {
                        {
                            let __cscope: zeo_rt::RubyValue = #sc;
                            let __v: zeo_rt::RubyValue = #v;
                            zeo_rt::scope_const_set(&__cscope, #name, __v)?
                        }
                    };
                }
                let err = uninitialized_constant_error(cx, scope.as_deref().unwrap_or(name));
                return quote! { return Err(zeo_rt::Signal::Raise(#err)) };
            }
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: constant storage is likewise always
            // `RubyValue` (`zeo_rt::const_set`'s own signature).
            let v = box_if_object_typed(cx, *value, v);
            let write =
                emit_const_write_stmt(cx, scope.as_deref(), name, quote! { __v.clone() }, Some(id));
            // Ruby announces the constant AFTER the write, so the hook body can
            // already read it -- and on every assignment, re-assignment
            // included (oracle-verified).
            let announce = const_owner_id_opt(cx, scope.as_deref(), name).map(|owner| {
                super::emit_const_added(
                    cx.compiler,
                    crate::compiler::ClassId(owner),
                    name,
                    Some(id),
                )
            });
            quote! { { let __v: zeo_rt::RubyValue = #v; #write #announce __v } }
        }
        // `expr::NAME` / `expr::NAME = v` -- the scope is a VALUE, so the whole
        // search happens at run time. `zeo_rt::scope_const_*` run the scope
        // operator's own search (not `const_get`'s, which reaches through
        // `Object` and past a `private_constant`, and which a class can
        // override), and reject a non-module scope with ruby's TypeError.
        HirNode::DynConstRead {
            scope,
            name,
            lenient,
        } => {
            let s = emit_expr(cx, *scope);
            let s = box_if_object_typed(cx, *scope, s);
            let f = if *lenient {
                quote! { scope_const_get_or_nil }
            } else {
                quote! { scope_const_get }
            };
            quote! { zeo_rt::#f(&#s, #name)? }
        }
        HirNode::DynConstWrite { scope, name, value } => {
            // Ruby evaluates the scope first and the value second, and does NOT
            // demand a module of the scope until both are in hand: `1::X =
            // (puts 2; 3)` prints before it raises (oracle-verified) -- unlike
            // the static `Scope::NAME = ...` just above, which raises on an
            // unresolvable scope without touching the right-hand side.
            let s = emit_expr(cx, *scope);
            let s = box_if_object_typed(cx, *scope, s);
            let v = emit_expr(cx, *value);
            let v = box_if_object_typed(cx, *value, v);
            quote! {
                {
                    let __cscope: zeo_rt::RubyValue = #s;
                    let __v: zeo_rt::RubyValue = #v;
                    zeo_rt::scope_const_set(&__cscope, #name, __v)?
                }
            }
        }
        // Brace-wrapped into a single Rust block EXPRESSION, not spliced as
        // bare statements: every `emit_expr` caller assumes one expression,
        // and a multi-statement `Seq`/`Eval` body in tail position otherwise
        // lands inside the tail's `Ok(...)` as `Ok(stmt; stmt; expr)` --
        // invalid Rust, found via `arr[i] += 1` as the last statement of a
        // method/`begin` body (a PRE-existing bug since these arms were
        // written, latent only because no test ever put one in tail
        // position). Safe to wrap: any local a `Seq`/`Eval` body assigns is
        // DECLARED in the enclosing scope's hoisting prelude (see
        // `codegen::hoisting`), so the braces hide nothing that outlives
        // this expression.
        //
        // `Seq` additionally emits under a positionally-exact type OVERLAY
        // for its own hidden temps (`__recvN`/`__idxN`, `hir.gensym`'d by
        // `parse`'s compound-assignment desugars): the scope-level
        // `local_types` map is a whole-scope MERGE, and a temp first
        // assigned inside a `begin` body merges against the rescue branches'
        // "never assigned" vote and widens to `Poly` -- sending `arr[i] +=
        // 1` inside any `begin` down the Poly-dispatch-to-`send` fallback (a
        // second PRE-existing bug, confirmed against the pre-freeze
        // checkout). Within a `Seq` the overlay is exact by construction:
        // its binds are straight-line and always precede every read, and the
        // gensym'd names are unreadable outside it. `Eval` deliberately gets
        // NO overlay -- its body is arbitrary user code where "the first
        // write's type" isn't a sound stand-in for a branch-merged one.
        HirNode::PreExec(body) | HirNode::Seq(body) => {
            let mut overlay = FMap::default();
            for &n in body {
                if let HirNode::LocalWrite(name, value) = &cx.compiler.hir[n] {
                    overlay.insert(name.clone(), infer(cx, *value));
                }
            }
            let narrowed = cx.with_narrowed_locals(overlay);
            // A `Seq`/`PreExec` expression ALWAYS infers `Poly` (see `infer`),
            // so every consumer expects a boxed `RubyValue` -- but a bare tail
            // `self`/`New`/`Shadowed`-local emits an unboxed `Arc<Concrete>`.
            // `emit_body_boxed` boxes exactly that tail, keeping the block's
            // Rust type consistent with its `Poly` inference. (`emit_body(..,
            // false)` left `(@x = 1; self)` producing `Arc<Self>`, an E0308 in
            // `Ok(...)` position -- reproduced via `def freeze = (..; self)`.)
            let b = super::stmt::emit_body_boxed(&narrowed, body);
            quote! { { #b } }
        }
        HirNode::Eval(body) => {
            let b = super::stmt::emit_body(&cx.in_eval_splice(), body, false);
            quote! { { #b } }
        }
        // A synthesized `attach_function` wrapper body: declare the C
        // symbol `extern "C"` (fn-locally, `#[link]`ed), marshal each argument,
        // call it, wrap the result. See `emit_ffi_call`.
        HirNode::Ffi(call) => emit_ffi_call(cx, call),
        // The `Eval` emit shape with the box switched: the body
        // resolves classes/constants/globals against `box_id` -- the AOT
        // loading-box context.
        HirNode::BoxScope { box_id, body } => {
            let box_cx = cx.in_box(*box_id);
            let b = super::stmt::emit_body(&box_cx, body, false);
            quote! { { #b } }
        }
        // The handle VALUE `box = Ruby::Box.new` binds: a Class of the
        // box's top-level surrogate (`p box` prints its registered
        // `#<Ruby::Box:N>` name; CRuby's own inspect carries a
        // `,user,optional` suffix -- documented divergence).
        HirNode::BoxHandle(box_id) => {
            let cid = cx
                .compiler
                .box_surrogate(*box_id)
                .expect("analyze registers a surrogate for every allocated box")
                .0;
            quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#cid)) }
        }
        HirNode::Return(v) => {
            // Boxed: `return Widget.new` leaves the method as a
            // `RubyValue` (methods return `Result<RubyValue, Signal>`).
            let value = match v {
                Some(id) => box_if_object_typed(cx, *id, emit_expr(cx, *id)),
                None => quote! { zeo_rt::RubyValue::Nil },
            };
            if cx.in_real_proc {
                // Inside a real escaping `Proc`'s own body, a literal Rust
                // `return` would only return from the CLOSURE, not the
                // lexically enclosing method -- wrong (real Ruby: `return`
                // inside a block always exits the enclosing method). Raise
                // `Signal::Return` instead, caught at the enclosing method's
                // own boundary (see `codegen::mod`'s per-method wrapping).
                quote! { return Err(zeo_rt::Signal::Return(#value)) }
            } else {
                // A literal Rust `return` -- valid in any expression
                // position (its type is `!`, which unifies with anything) --
                // correct here because this code is either the enclosing
                // method's own body directly, or a fast-inline-path block
                // (`.times`) spliced into that SAME method body.
                quote! { return Ok(#value) }
            }
        }
        HirNode::Yield(args) => {
            // `__blk` is the implicit trailing parameter every method that
            // uses `yield`/`block_given?`/`&block` gets (see
            // `codegen::params`'s `emit_signature_params`/`emit_prologue`) --
            // an `Option<RubyValue>`, `None` when the call passed no block.
            // A `yield` with no block is a RESCUABLE `LocalJumpError` at
            // runtime (CRuby's `no block given (yield)`), so raise it as a
            // Signal rather than panicking -- a `begin/rescue LocalJumpError`
            // around the call must catch it, and codegen lowers all branches
            // eagerly so an unreached `yield` must not abort the process.
            // Boxed: `yield self` (or any Object-typed value) crosses the
            // Proc boundary as a `RubyValue` slice element.
            // BORROWED, not cloned: `RProc::call` takes `&self`, and the
            // block lives in `__blk` for the whole yield. Cloning it cost an
            // `Arc` refcount pair per yield -- on the hottest path a
            // generated program has.
            let invoke = quote! {
                match __blk.as_ref() {
                    Some(__b) => __b.as_proc_ref(),
                    None => return Err(zeo_rt::raise_no_block_yield()),
                }
            };
            // A trailing hash the `yield` spelled as KEYWORDS (`yield(v, **h)`)
            // is dropped when `h` turns out empty, so its length is a runtime
            // question and the fixed-slice form below cannot carry it.
            let kw_tail = args.last().and_then(|a| match a {
                ArrayElem::Single(n)
                    if cx
                        .compiler
                        .hir
                        .has_flag(*n, crate::hir::NodeFlag::KWARGS_HASH) =>
                {
                    Some(*n)
                }
                _ => None,
            });
            // No splat: the arguments are a fixed-length list, so they go
            // straight into a borrowed slice literal with no Vec allocated.
            if kw_tail.is_none() && !args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
                let arg_exprs = args.iter().map(|a| {
                    let ArrayElem::Single(n) = a else {
                        unreachable!("just checked for splats")
                    };
                    box_if_object_typed(cx, *n, emit_expr(cx, *n))
                });
                return quote! { (#invoke).call(&[#(#arg_exprs),*])? };
            }
            // `yield(*a)` -- the length is only known at runtime, so build
            // the argument vector the same way `call::emit_splat_call` does.
            // The block then binds from it through its ordinary parameter
            // machinery (auto-splat, rest, nil-padding all included).
            let fixed = &args[..args.len() - usize::from(kw_tail.is_some())];
            let pushes = fixed.iter().map(|a| match a {
                ArrayElem::Single(n) => {
                    let e = box_if_object_typed(cx, *n, emit_expr(cx, *n));
                    quote! { __args.push(#e); }
                }
                ArrayElem::Splat(n) => {
                    let e = box_if_object_typed(cx, *n, emit_expr(cx, *n));
                    // Coerced like every splat (`to_a`/wrap/nil-drops), not
                    // unwrapped as a bare Array -- see `call::splat`.
                    quote! { zeo_rt::array_splat_into(&mut __args, &(#e))?; }
                }
            });
            // The same rule `emit_splat_call` applies on the call side: a `**h`
            // that is empty AT RUNTIME contributes nothing, so the block sees
            // one fewer argument rather than a stray `{}`.
            let kw_push = kw_tail.map(|n| {
                let e = emit_expr(cx, n);
                quote! {
                    let __kw = #e;
                    if !__kw.as_hash_ref().lock().is_empty() {
                        __args.push(__kw);
                    }
                }
            });
            quote! {
                {
                    let mut __args: Vec<zeo_rt::RubyValue> = Vec::new();
                    #(#pushes)*
                    #kw_push
                    (#invoke).call(&__args)?
                }
            }
        }
        // `block_given?` asks about the enclosing METHOD's block, which the
        // analyze walk has already given a `__blk` parameter. Where no such
        // binding exists -- the top level, a class body, or a block written
        // in either -- there is no method to ask about, so the answer is the
        // constant `false`.
        HirNode::BlockGiven if !cx.has_blk_binding => {
            quote! { zeo_rt::RubyValue::Bool(false) }
        }
        HirNode::BlockGiven => quote! { zeo_rt::RubyValue::Bool(__blk.is_some()) },
        HirNode::Raise(args, cause) => emit_raise(cx, args, cause),
        HirNode::CaseIn {
            subject,
            arms,
            else_body,
        } => super::patterns::emit_case_in(cx, *subject, arms, else_body),
        HirNode::MatchPredicate { subject, pattern } => {
            super::patterns::emit_match_predicate(cx, *subject, pattern)
        }
        HirNode::MatchRequired { subject, pattern } => {
            super::patterns::emit_match_required(cx, *subject, pattern)
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => super::exceptions::emit_begin(cx, body, rescues, else_body, ensure_body),
        HirNode::Retry => super::exceptions::emit_retry(),
        // A `def` / literal `define_method(:sym){...}` in EXPRESSION position --
        // i.e. inside a block, most notably a `Class.new { ... }` body.
        // It installs on the current runtime `self` (the anonymous class):
        //   self.define_method(:name, ->(params){ body })      (instance method)
        //   self.define_singleton_method(:name, ...)            (`def self.x`)
        // A lambda body gives method-like arity/`return`; the runtime rebinds
        // `self` to the receiver when the method runs. Only reachable with a
        // runtime (dynamic) self -- a block's -- since a class body / top-level
        // `def` is handled before ever reaching expression position.
        HirNode::DefMethod {
            name,
            params,
            body,
            is_class_method,
            is_def,
            visibility,
        } => {
            // Ruby resolves these two against DIFFERENT things, and conflating
            // them put `def self.x` on the wrong object.
            //
            // A plain `def` installs on the CREF's default definee -- the
            // enclosing class, Object at the top level, so `p(def foo; end)`
            // makes `foo` a private method of Object and returns `:foo`. A
            // block's cref is dynamic, and `define_in_default_definee` derives
            // it from the block's self (a Class/Module gets an instance
            // method, anything else a singleton one -- `instance_exec { def m;
            // end }`).
            //
            // `def self.x` and a literal `define_method` are ordinary SENDS TO
            // SELF, and self is not the definee outside a class body. Inside
            // `def foo`, ruby's `def self.bar` lands on the INSTANCE, and
            // `define_method` raises NoMethodError because an instance is no
            // Module; naming the enclosing class here answered both wrongly.
            // `boxed_implicit_self` is that "self here" rule and is total, so
            // it is not re-derived.
            // The CREF's own definee, which is where a plain `def` lands
            // unless an `*_eval` replaced it -- a question about where the
            // `def` was WRITTEN, so it is answered here rather than read off
            // a runtime self.
            // `defining_class`, not `class_self`: the cref is where the `def`
            // was WRITTEN. The two differ for an inherited class method (whose
            // copies share one cref) and for a `class << self` body, whose
            // cref is the SINGLETON -- `lexical_home`, which `defining_class`
            // already carries.
            let cref_definee = {
                let cid = cx.defining_class.unwrap_or(crate::compiler::OBJECT_CLASS).0;
                quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#cid)) }
            };
            let definee = if cx.self_is_dynamic {
                super::call::boxed_implicit_self(cx)
                    .expect("a dynamic-self `def` in expression position must have a boxed self")
            } else {
                cref_definee.clone()
            };
            let receiver =
                super::call::boxed_implicit_self(cx).expect("boxed_implicit_self is total");
            // The body becomes a method-body lambda: its `yield`/
            // `block_given?`/`&block` reach the block the installed method is
            // called with, threaded through `ProcData`'s call-site block slot
            // (see `HirNode::Lambda`'s `method_body`). It is a RUNTIME-defined
            // method: the class it lands on is minted at runtime, so a `super`
            // in its body resolves through the runtime method-frame stack, not
            // a compile-time ancestor splice. Marking `runtime_super_params` is
            // what routes it there; `emit_super` checks that marker
            // BEFORE reading `defining_class`, so a `def` nested inside a real
            // class's method (`class Foo; def m; Class.new { def g; super; end
            // }; end; end`) resolves `g`'s `super` at runtime without wrongly
            // splicing against Foo. The enclosing class context is otherwise
            // left intact so lexical constant resolution in the body still sees
            // the surrounding module nesting (a method-body lambda already runs
            // under a dynamic `self`, so it never statically dispatches against
            // that class).
            let mut body_cx = cx.clone();
            body_cx.runtime_super_params = Some(std::rc::Rc::new((**params).clone()));
            body_cx.defined_by_define_method = !is_def;
            // Ruby labels a `define_method` body's frame after where it was
            // WRITTEN (`block in <main>`), a `def`'s after the method it
            // creates -- captured before `current_method` changes the answer.
            body_cx.lexical_frame_label =
                (!is_def).then(|| crate::codegen::enclosing_frame_label(cx));
            // The body IS this method's body, however it is installed, so
            // `__method__`/`__callee__` name it rather than reporting whatever
            // encloses the `def` -- `nil` at the top level, which is what a
            // `def` written inside a block used to answer.
            body_cx.current_method = Some(name.clone());
            body_cx.current_method_origin = None;
            let proc = super::call::emit_proc_or_lambda_value(
                &body_cx,
                params,
                body,
                true,
                true,
                super::source_location(cx.compiler, id).map(|(f, l)| (f.to_string(), l)),
            );
            let name_sym = super::pooled_sym(name);
            if *is_class_method {
                // `def self.x` always installs a singleton method on `self`.
                let dsm = super::pooled_sym("define_singleton_method");
                quote! {
                    zeo_rt::send_value(
                        &#receiver,
                        #dsm,
                        &[zeo_rt::RubyValue::Symbol(#name_sym), #proc],
                        None,
                    )?
                }
            } else if *is_def {
                // A real `def` installs on the default definee: the CREF's,
                // unless an `*_eval`/`Class.new` on the stack replaced it --
                // which only the runtime can say, so both candidates go.
                //
                // Written at the TOP LEVEL it is a PRIVATE instance method of
                // `Object`, exactly as a bare top-level `def` is (which analyze
                // registers that way before ever reaching expression position).
                // The visibility belongs to the FRAME, and a block inherits its
                // frame's -- so a `def` in a top-level block is private too,
                // while one inside any method body is public.
                let top_level = cx.defining_class.is_none() && cx.current_method.is_none();
                // A class body's running visibility default and
                // `module_function` mode reach a `def` nested in a
                // RUNTIME-undecidable branch as a visibility stamped on the
                // node itself (`lower::defs::apply_body_defaults_in_branches`)
                // -- and the runtime install must carry it, because the
                // dynamic walk reads the overlay row it writes AHEAD of the
                // static table's private row ("a runtime-defined method with
                // no mark is public"). The definee is the class body's own
                // class, a compile-time fact at these sites.
                let vis_mark = (*visibility != crate::hir::Visibility::Public
                    && !cx.self_is_dynamic
                    && cx.defining_class.is_some())
                .then(|| {
                    let cid = cx.defining_class.expect("guarded above").0;
                    let v = match visibility {
                        crate::hir::Visibility::Private => quote! { Private },
                        crate::hir::Visibility::Protected => quote! { Protected },
                        crate::hir::Visibility::Public => unreachable!("guarded above"),
                    };
                    quote! {
                        zeo_rt::runtime_set_visibility(
                            zeo_rt::ClassId(#cid),
                            &[zeo_rt::RubyValue::Symbol(#name_sym)],
                            zeo_rt::MethodVisibility::#v,
                        )?;
                    }
                });
                quote! {
                    {
                        let __defd = zeo_rt::define_in_default_definee(
                            &#cref_definee,
                            &#definee,
                            #name_sym,
                            #proc,
                            #top_level,
                        )?;
                        #vis_mark
                        __defd
                    }
                }
            } else {
                // A literal `define_method(:m){...}` call: an ordinary
                // Module#define_method dispatch, which raises NoMethodError when
                // self isn't a Module/Class (`instance_exec { define_method... }`).
                let dm = super::pooled_sym("define_method");
                quote! {
                    zeo_rt::send_value(
                        &#receiver,
                        #dm,
                        &[zeo_rt::RubyValue::Symbol(#name_sym), #proc],
                        None,
                    )?
                }
            }
        }
        // A mixin's ANCESTRY edit happened at compile time; what is left to do
        // where it was written is to run the module's hook -- `M.included(C)`,
        // `M.extended(C)`, `M.prepended(C)` -- which is how the
        // extend-on-include idiom (ActiveSupport::Concern and every DSL after
        // it) gives the base its class-side methods. Module's own default hook
        // is a no-op, so nothing is emitted unless the module defines one.
        HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_) => {
            let (m, hook, primitive) = mixin_parts(&cx.compiler.hir[id]).expect("a mixin node");
            // The primitive FIRST when the module overrides it -- it is what
            // performs the mixin, and analyze suppressed the static edit on the
            // strength of it. The notification follows either way, exactly as
            // ruby fires `included` even when an override skipped the splice.
            let overridden = primitive.filter(|p| {
                cx.resolve_class(m)
                    .is_some_and(|mid| cx.compiler.overrides_mixin_primitive(mid, p))
            });
            let splice = overridden.map(|p| emit_mixin_hook(cx, m, p));
            let notify = emit_mixin_hook(cx, m, hook);
            match splice {
                Some(splice) => quote! { { #splice; #notify } },
                None => notify,
            }
        }
        // A definition report -- `Klass.method_added(:name)` and its five
        // siblings -- spliced back in at the position of a `def` the analyze
        // walk consumed. `analyze::def_hooks` already proved a hook body
        // answers, so this always emits a real send. `send_value`, not a
        // visibility-checked call: ruby reaches its hooks through an FCALL, so
        // a `private def self.method_added` still runs.
        HirNode::DefHook {
            class,
            hook,
            name,
            pending,
        } => {
            let (cid, sym, arg) = (*class, super::pooled_sym(hook), super::pooled_sym(name));
            let send = quote! {
                zeo_rt::send_value(
                    &zeo_rt::RubyValue::Class(zeo_rt::ClassId(#cid)),
                    #sym,
                    &[zeo_rt::RubyValue::Symbol(#arg)],
                    None,
                )
            };
            // The half-built view the hook body must see -- see
            // `HirNode::DefHook::pending`. Omitted entirely when nothing is
            // left in the future, so the last definition in a class pays
            // nothing for it.
            if pending.is_empty() {
                return quote! { #send? };
            }
            let names = pending.iter().map(|n| super::pooled_sym(n));
            quote! {
                zeo_rt::with_pending_defs(
                    zeo_rt::ClassId(#cid),
                    &[#(#names),*],
                    || #send,
                )?
            }
        }
        // `using M` is spent entirely at compile time: the activation it
        // records already decided which call sites route through the
        // refinement, so nothing is left to run where it was written.
        HirNode::Using(_) => quote! { zeo_rt::RubyValue::Nil },
        // A `refine` whose registration analyze already consumed is spent the
        // same way: the holder module owns the methods and the `Refinement`
        // row says what they refine, so the marker runs nothing where it was
        // written. Reached only when the marker sits inside a statement the
        // class-body walk kept whole -- power_assert's refinements under a
        // runtime `if`. An UNregistered marker falls through to the rejection
        // below rather than losing the refinement.
        HirNode::Refine { .. } if cx.compiler.refinement_marker_registered(id) => {
            quote! { zeo_rt::RubyValue::Nil }
        }
        // A `class`/`module` written where a value is read -- `__skip__ =
        // module M ... end` (elasticgraph), `class Keys ... end` as the last
        // statement of its enclosing body (jaeger-client), and the surrogate a
        // `class << self` body mints when that body ends a method (logging's
        // `def kill; class << self; undef :close; end; end`). Ruby's value is
        // the body's last statement, which the site already computes and the
        // statement form discards.
        HirNode::ClassDef { .. } => {
            crate::codegen::emit_class_def_marker(cx, id, crate::codegen::BodyValue::Keep)
        }
        HirNode::Program(_)
        | HirNode::Refine { .. }
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::MethodRedefine { .. } => {
            // Name the construct: each of these has its OWN Ruby value (`undef`
            // and `alias` answer nil, `private :a` answers `:a`), so the next
            // one to be wired needs to be told apart from its siblings in the
            // ledger.
            let kind = definition_kind(&cx.compiler.hir[id]);
            crate::codegen::unsupported_at(
                cx.compiler,
                id,
                format!(
                    "a definition-level construct used as a VALUE isn't supported yet \
                     (zeo limitation): {kind}"
                ),
            )
        }
    }
}

/// The Ruby spelling of a definition-level node, for the rejection above and
/// for the one `codegen::consumed_tail_value` raises over the same nodes.
pub(super) fn definition_kind(node: &HirNode) -> &'static str {
    match node {
        HirNode::Program(_) => "a program body",
        HirNode::ClassDef {
            is_module: true, ..
        } => "a module definition",
        HirNode::ClassDef { .. } => "a class definition",
        HirNode::DefMethod { .. } => "a method definition",
        HirNode::Refine { .. } => "refine",
        HirNode::Undef(_) => "undef",
        HirNode::ClassMethodUndef(_) => "undef on a singleton class",
        HirNode::AliasMethod { .. } => "alias",
        HirNode::MethodVisibility { .. } => "a visibility directive",
        HirNode::ClassMethodVisibility { .. } => "a class-method visibility directive",
        HirNode::ModuleFunction(_) => "module_function",
        HirNode::MethodRedefine { .. } => "a method redefinition",
        HirNode::ConstantVisibility { .. } => "a constant visibility directive",
        HirNode::Include(_) => "include",
        HirNode::Extend(_) => "extend",
        HirNode::Prepend(_) | HirNode::ClassMethodPrepend(_) => "prepend",
        HirNode::Using(_) => "using",
        HirNode::Call { name, .. } if name.starts_with("attr_") => "an attribute reader/writer",
        HirNode::Call { .. } => "a call the walk consumed",
        _ => "a construct with no value form",
    }
}

/// `raise`/`fail` -- see `HirNode::Raise`'s docs for the call-shape survey.
/// Always compiles to `return Err(Signal::Raise(exc))`: even though `raise`
/// syntactically sits in expression position, real Ruby's `raise` never
/// actually produces a value there either (control never returns to that
/// position), so a literal Rust `return` is exactly as faithful here as
/// `HirNode::Return`'s is in the non-Proc case.
///
/// Bare `raise` (zero args, re-raise) reads `zeo_rt::current_exception()`
/// -- the innermost `rescue` clause currently executing, if any (see
/// `zeo_rt::handling`'s docs) -- and re-raises it EXACTLY (same object,
/// same ivars/message, not a fresh copy), matching real Ruby's re-raise
/// semantics. Outside any `rescue` clause, real Ruby's bare `raise`
/// constructs a fresh `RuntimeError` with an EMPTY message instead of
/// erroring (confirmed via `ruby -e 'begin; raise; rescue => e; puts
/// "[#{e.message}]"; end'` -> `"[]"`) -- faithfully mirrored here via
/// `unwrap_or_else`, not a panic.
pub(super) fn emit_raise(cx: &Ctx, args: &[NodeId], cause: &crate::hir::RaiseCause) -> TokenStream {
    let exc = match args {
        [] => {
            // NOT `.unwrap_or_else(|| #fallback)` -- `#fallback` itself
            // contains a `?` (from constructing via `initialize`, which can
            // fail), and `?` can't cross into a closure that doesn't itself
            // return a `Result` (`unwrap_or_else`'s closure here must return
            // a bare `RubyValue`, to match `current_exception()`'s `Option`
            // payload). A `match` arm has no such closure boundary.
            let fallback = emit_boxed_new(
                cx,
                "RuntimeError",
                vec![quote! { zeo_rt::RubyValue::Str(zeo_rt::string_new(String::new())) }],
            );
            quote! {
                match zeo_rt::current_exception() {
                    Some(__v) => __v,
                    None => #fallback,
                }
            }
        }
        [one] => emit_raise_value(cx, *one, None),
        [class_arg, msg_arg] => emit_raise_value(cx, *class_arg, Some(*msg_arg)),
        // `raise Class, msg, backtrace` -- the third argument installs the
        // exception's CUSTOM backtrace (an Array of Strings, one String, or
        // nil for the real stack), which the raise-time stamp then leaves
        // alone (`attach_backtrace` only fills an empty slot).
        [class_arg, msg_arg, bt_arg] => {
            let exc = emit_raise_value(cx, *class_arg, Some(*msg_arg));
            let bt = emit_expr(cx, *bt_arg);
            quote! {
                {
                    let __exc = #exc;
                    zeo_rt::apply_custom_backtrace(&__exc, &(#bt))?;
                    __exc
                }
            }
        }
        _ => unreachable!("lowering rejects `raise`/`fail` with more than 3 arguments"),
    };
    // An OMITTED `cause:` chains automatically: `raise_with_cause` threads the
    // currently-handled exception (`$!`) into the raised exception's `cause`
    // slot, and is a no-op for a bare re-raise or a non-exception operand.
    //
    // An EXPLICIT `cause:` takes the other branch and never calls
    // `raise_with_cause` at all -- which is exactly what makes `cause: nil`
    // SUPPRESS chaining rather than request it, since nothing is left to fill
    // the slot from `$!`.
    match cause {
        crate::hir::RaiseCause::Absent => {
            quote! { return Err(zeo_rt::Signal::Raise(zeo_rt::raise_with_cause(#exc))) }
        }
        crate::hir::RaiseCause::Explicit(node) => {
            let cause_expr = emit_expr(cx, *node);
            let cause_expr = box_if_object_typed(cx, *node, cause_expr);
            quote! {
                {
                    let __exc = #exc;
                    zeo_rt::set_explicit_cause(&__exc, #cause_expr)?;
                    zeo_rt::attach_backtrace(&__exc);
                    return Err(zeo_rt::Signal::Raise(__exc));
                }
            }
        }
    }
}

/// Builds the actual `RubyValue` to raise, mirroring zeo's own `raise`
/// call-shape dispatch, translated to `emit_new`-style construction:
/// `raise SomeError` (bare class ref) defaults the
/// message to the class's own name, as a COMPILE-TIME string literal (no
/// runtime `self.class` reflection needed, since the raised class is
/// statically known at the call site in every shape handled here);
/// `raise SomeError, "msg"` uses the explicit message; `raise "msg"`
/// (a `Str`-typed expression, no class) implies `RuntimeError`; anything
/// else is assumed to already be a constructed exception value (`raise
/// SomeError.new(...)`, or a local variable holding one) and used directly,
/// boxing a statically-known-`Object`-typed expression into
/// `RubyValue::Object` the same way `emit_safe_call` already does for its
/// own uniform-representation needs.
/// The hook Ruby runs after a mixin's ancestry edit: `M.included(C)`,
/// `M.extended(C)`, `M.prepended(C)`. Module's own default is a no-op, so
/// nothing is emitted unless the module defines the hook. The call goes
/// through dynamic dispatch: the hook is reached by NAME, and its body
/// commonly edits `C` at runtime (`base.extend(self)`).
/// `(module, receiving class)` when `include M`/`extend M`/`prepend M` here
/// really has a hook to call, `None` when it has none -- which is the common
/// case, since `Module`'s own `included`/`extended`/`prepended` is a no-op.
///
/// Asked by the statement emitter as well: with no hook the mixin emits no code
/// at all, and a statement that produced a bare `RubyValue::Nil` instead drew a
/// "path statement drops value" warning out of every program that mixes in a
/// module.
pub(super) fn mixin_hook_runs(cx: &Ctx, node: &HirNode) -> bool {
    let Some((module, hook, primitive)) = mixin_parts(node) else {
        return false;
    };
    let Some(mid) = cx.resolve_class(module) else {
        return false;
    };
    cx.defining_class.is_some()
        && (cx.compiler.class_method_in_chain(mid, hook).is_some()
            // The PRIMITIVE too: when a module overrides it, analyze suppressed
            // the compile-time ancestry edit and this node is what performs the
            // mixin at all. Dropping it as pure would silently lose the module.
            || primitive.is_some_and(|p| cx.compiler.overrides_mixin_primitive(mid, p)))
}

/// `(module name, notification hook, mix-in primitive)` for the three mixin
/// nodes -- `include` is `append_features` then `included`, and its two
/// siblings follow the same shape (`eval.c`'s `rb_mod_include`).
fn mixin_parts(node: &HirNode) -> Option<(&String, &'static str, Option<&'static str>)> {
    match node {
        HirNode::Include(m) => Some((m, "included", Some("append_features"))),
        HirNode::Prepend(m) => Some((m, "prepended", Some("prepend_features"))),
        // The singleton form fires the same hooks, but with the SINGLETON
        // class as their argument -- which zeo has no compile-time class for.
        // `analyze` rejects a module that defines either, so reaching here at
        // all means both are Module's own no-op defaults and nothing is
        // emitted; the pair is named so that stays true if one is added later.
        HirNode::ClassMethodPrepend(m) => Some((m, "prepended", Some("prepend_features"))),
        HirNode::Extend(m) => Some((m, "extended", Some("extend_object"))),
        _ => None,
    }
}

fn emit_mixin_hook(cx: &Ctx, module: &str, hook: &str) -> TokenStream {
    let nil = quote! { zeo_rt::RubyValue::Nil };
    let (Some(mid), Some(target)) = (cx.resolve_class(module), cx.defining_class) else {
        return nil;
    };
    // The notification is skipped when nobody defines it (Module's default is a
    // no-op). A PRIMITIVE is never skipped: it is reached only when the module
    // overrides it, and it is what performs the mixin.
    if cx.compiler.class_method_in_chain(mid, hook).is_none()
        && !cx.compiler.overrides_mixin_primitive(mid, hook)
    {
        return nil;
    }
    let (m, t) = (mid.0, target.0);
    let sym = super::pooled_sym(hook);
    quote! {
        zeo_rt::send_value(
            &zeo_rt::RubyValue::Class(zeo_rt::ClassId(#m)),
            #sym,
            &[zeo_rt::RubyValue::Class(zeo_rt::ClassId(#t))],
            None,
        )?
    }
}

fn emit_raise_value(cx: &Ctx, node: NodeId, explicit_msg: Option<NodeId>) -> TokenStream {
    // A literal class reference -- bare (`raise NotFound`) or qualified
    // (`raise Store::Errors::NotFound`) -- when the path
    // actually resolves to a class; a constant-shaped node that DOESN'T
    // resolve falls through to the value cases below (a constant can
    // legitimately hold a pre-built exception).
    let class_path = const_path_of(cx, node).filter(|p| cx.resolve_class(p).is_some());
    if let Some(class_name) = &class_path {
        // Only an Exception subclass can be raised. A non-exception class
        // (`raise String`, `raise Object, "m"`) is CRuby's TypeError, NOT an
        // attempt to instantiate it -- the builtin has no generated struct to
        // `new_handle`, so this both fixes invalid codegen and matches Ruby.
        let cid = cx.resolve_class(class_name).unwrap();
        let exc = zeo_abi::EXCEPTION_CLASS.0;
        let is_exc = cid.0 == exc || cx.compiler.class(cid).ancestors.iter().any(|a| a.0 == exc);
        if !is_exc {
            return emit_boxed_new(
                cx,
                "TypeError",
                vec![quote! {
                    zeo_rt::RubyValue::Str(zeo_rt::string_new(
                        "exception class/object expected".to_string()
                    ))
                }],
            );
        }
        // A class with its OWN `self.exception` goes through the runtime
        // protocol instead: `rb_make_exception` calls `klass.exception(msg)`,
        // and a class that overrides it expects to build the exception itself.
        // Constructing with `new` skipped the override entirely.
        if cx
            .compiler
            .class(cid)
            .class_methods
            .iter()
            .any(|e| cx.compiler.scope(e.def).name == "exception")
        {
            let id = cid.0;
            let class_expr = quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) };
            return match explicit_msg {
                Some(msg_id) => {
                    let msg = box_if_object_typed(cx, msg_id, emit_expr(cx, msg_id));
                    quote! { zeo_rt::coerce_raise_arg_with_message(#class_expr, #msg)? }
                }
                None => quote! { zeo_rt::coerce_raise_arg(#class_expr)? },
            };
        }
        // `raise SomeError` / `raise SomeError, "msg"` constructs via
        // `SomeError.new(...)`, running any custom `initialize` (defaults and
        // `super` chain included), exactly like CRuby's `exc.exception` path.
        let args = match explicit_msg {
            // Boxed like any other argument: `raise Gem::DependencyResolutionError,
            // Gem::Resolver::Conflict.new(...)` passes an object-typed
            // expression, which `initialize` takes as a plain `RubyValue`.
            Some(msg_id) => vec![box_if_object_typed(cx, msg_id, emit_expr(cx, msg_id))],
            None => vec![],
        };
        return emit_boxed_new(cx, class_name, args);
    }
    if let Some(msg_id) = explicit_msg {
        // `raise <expr>, message` where `<expr>` didn't resolve to a class
        // above -- a variable, a call, or a constant-SHAPED operand that names
        // no compiled class (an alias, `Foo = Class.new`, or nothing at all).
        // All of them coerce at runtime like `Kernel#raise` does
        // (`klass.exception(msg)`), and the operand is evaluated FIRST, so an
        // undefined constant raises its own `uninitialized constant` before
        // the message is ever consulted -- Ruby's order.
        //
        // Boxed like the message: `raise obj, "msg"` where obj's static type
        // is a generated struct (a class overriding #exception) would
        // otherwise hand the raw Arc to a RubyValue parameter and fail rustc.
        let class_expr = box_if_object_typed(cx, node, emit_expr(cx, node));
        let msg_expr = box_if_object_typed(cx, msg_id, emit_expr(cx, msg_id));
        return quote! {
            zeo_rt::coerce_raise_arg_with_message(#class_expr, #msg_expr)?
        };
    }
    match infer(cx, node) {
        TyKind::Str => {
            let msg_expr = emit_expr(cx, node);
            emit_boxed_new(cx, "RuntimeError", vec![msg_expr])
        }
        TyKind::Object(cid) => {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            let expr = emit_expr(cx, node);
            let boxed = quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#expr)) };
            // A statically-known Exception subclass raises directly; a
            // non-exception object is coerced at runtime to CRuby's TypeError.
            // `Exception`'s id is fixed (`zeo-abi`), so no lookup is needed.
            let exc_cid = zeo_abi::EXCEPTION_CLASS.0;
            let is_exc = cx
                .compiler
                .class(cid)
                .ancestors
                .iter()
                .any(|a| a.0 == exc_cid);
            if is_exc {
                boxed
            } else {
                quote! { zeo_rt::coerce_raise_arg(#boxed)? }
            }
        }
        // A Poly (or non-exception) operand is coerced at runtime: an
        // Exception raises itself, a String becomes a `RuntimeError`, and
        // everything else is CRuby's `TypeError: exception class/object
        // expected`.
        _ => {
            let expr = emit_expr(cx, node);
            quote! { zeo_rt::coerce_raise_arg(#expr)? }
        }
    }
}

/// `emit_new_with_arg_tokens` returns a bare, unboxed `Arc<Concrete>` (the
/// same representation an ordinary `ClassName.new(...)` expression has --
/// see that function's docs); `Signal::Raise` needs a real `RubyValue`, so
/// this boxes it the same way `emit_safe_call` already does for its own
/// uniform-representation needs.
/// The class/module PATH a node names, when it has a constant-reference
/// SHAPE at all: a bare `ClassRef` or a qualified `Foo::Bar`
/// (`QualifiedConstRead`). Whether the path actually NAMES a
/// registered class (vs. an ordinary value constant) is the caller's
/// `resolve_class` check, same as the long-standing bare-`ClassRef` rule
/// (see `emit_call`'s class-method interception docs).
pub(super) fn const_path_of(cx: &Ctx, id: NodeId) -> Option<String> {
    match &cx.compiler.hir[id] {
        HirNode::ClassRef(n) => Some(n.clone()),
        HirNode::QualifiedConstRead(scope, n) => Some(format!("{scope}::{n}")),
        _ => None,
    }
}

pub(super) fn emit_boxed_new(
    cx: &Ctx,
    class_name: &str,
    arg_exprs: Vec<TokenStream>,
) -> TokenStream {
    let cid = cx
        .resolve_class(class_name)
        // Callers pass a class already known to resolve (a filtered user class,
        // or a literal builtin like `NameError`/`TypeError`); an undefined
        // constant in a user program raises a runtime NameError well before
        // reaching here (see `emit_const_read`).
        .unwrap_or_else(|| {
            panic!("internal error: unknown class `{class_name}` in emit_boxed_new")
        });
    // Every `emit_boxed_new` product is an exception codegen constructs AT
    // its raise site, so the backtrace stamp happens here once instead of
    // at each of the ~27 `Signal::Raise(#err)` emission sites
    // (attach-if-unset, so the user-`raise` path stamping again is a
    // harmless no-op). Cause chaining deliberately does NOT happen here --
    // it is `raise`'s own semantics (`raise_with_cause` at the raise
    // statement), and `cause: nil` suppression depends on that separation.
    //
    // A NATIVE-BACKED class (exception or value-builtin subclass) has no
    // generated struct to `new_handle` -- `emit_new_with_arg_tokens` already
    // returns a fully-boxed `RubyValue` built by the runtime.
    if cx.compiler.is_native_backed(cid) {
        let ctor = super::call::emit_new_with_arg_tokens(cx, class_name, arg_exprs, None);
        return quote! { zeo_rt::stamp_backtrace(#ctor) };
    }
    // A feature unit may redefine this class's `initialize` at LOAD time
    // (registered-but-not-promised), so a struct-backed exception built at a
    // raise site routes through the runtime exactly as `emit_new`'s own gate
    // does -- the compile-time class identity stays, only construction goes
    // dynamic. Without this the two raise-site callers bypassed the gate.
    if cx.compiler.may_be_patched_at_runtime("initialize")
        || cx.compiler.may_be_patched_at_runtime("new")
    {
        let id = cid.0;
        let new_sym = super::pooled_sym("new");
        return quote! {
            zeo_rt::stamp_backtrace({
                let __rtclass = zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id));
                zeo_rt::send_value(&__rtclass, #new_sym, &[#(#arg_exprs),*], None)?
            })
        };
    }
    let class_ident = super::ident::class_ident(cx.compiler, cid);
    let ctor = super::call::emit_new_with_arg_tokens(cx, class_name, arg_exprs, None);
    quote! { zeo_rt::stamp_backtrace(zeo_rt::RubyValue::Object(#class_ident::new_handle(#ctor))) }
}

/// The boxed `NameError: uninitialized constant <name>` value, for a constant
/// reference lowered in a branch that may be dead or rescued -- CRuby only
/// raises `uninitialized constant` if the branch actually runs, so an
/// unresolved reference defers to this runtime raise instead of a
/// compile-time panic (see `emit_const_read`, the rescue-clause and pattern
/// class checks, and `raise <undefined-const>, msg`). `name` is printed
/// verbatim, so callers pass whatever text CRuby's message shows in their
/// position (the missing head, or the full path).
pub(super) fn uninitialized_constant_error(cx: &Ctx, name: &str) -> TokenStream {
    let _ = cx;
    // `NameError#name` is the missing constant as a Symbol -- its leaf when a
    // path was passed (`Foo::Bar` -> `:Bar`), matching CRuby. The receiver (the
    // lexical `cref`) isn't known at this generic site, so it reads back as
    // `nil`; where it IS known -- an explicit `Klass.const_get` -- the fold in
    // `call.rs` supplies it precisely.
    let leaf = crate::constpath::ConstPath::parse(name).base();
    // Stamped here rather than at each of the five call sites: every one of
    // them raises the value immediately, and an unstamped `NameError` reaches
    // the top level with an EMPTY backtrace -- no `file:line:in '<module:X>'`
    // header, no `from` chain.
    quote! {
        zeo_rt::stamp_backtrace(zeo_rt::make_name_error(
            format!("uninitialized constant {}", #name),
            #leaf,
            zeo_rt::RubyValue::Nil,
        ))
    }
}

/// Wraps a raised exception VALUE so it fires in EXPRESSION (or boolean)
/// position: the `match` `return`s the error, and its unreachable `Ok` arm
/// yields `ok_ty`, so the whole thing type-unifies wherever a value of that
/// type is expected. `ok_ty` is the Rust type the surrounding position wants
/// -- `zeo_rt::RubyValue` for a value read, `bool` for a `rescue`/pattern
/// class check.
pub(super) fn raise_in_expr_position(err: TokenStream, ok_ty: TokenStream) -> TokenStream {
    quote! {
        match Err::<#ok_ty, zeo_rt::Signal>(zeo_rt::Signal::Raise(#err)) {
            Ok(__v) => __v,
            Err(__s) => return Err(__s),
        }
    }
}

/// Boxes `value` into `RubyValue::Object` if `id`'s own static type is
/// `TyKind::Object` -- which, by this codebase's construction, ALWAYS means
/// `value` is currently a bare, unboxed `Arc<Concrete>`. The only three
/// shapes `infer` ever classifies `TyKind::Object` are `HirNode::New`, a
/// `Shadowed`-storage local read, and a bare `HirNode::SelfRef` -- and all
/// three deliberately emit unboxed (so a cheap Path 1 call RECEIVER can
/// reuse the value directly, e.g. `emit_call`'s `recv_expr =
/// emit_expr(cx, recv_id)`; see `LocalStorage::Shadowed`'s docs). Anything
/// else (an ordinary method call, an ivar read, etc.) always infers `Poly`
/// and is ALREADY a real, boxed `RubyValue` -- boxing it again would be a
/// real type error, not just redundant, which is why this can't be a blind
/// "wrap everything" step; it's keyed on the type, which this codebase's
/// own invariant makes an exact proxy for "needs boxing".
pub(super) fn box_if_object_typed(cx: &Ctx, id: NodeId, value: TokenStream) -> TokenStream {
    match infer(cx, id) {
        TyKind::Object(cid) => {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#value)) }
        }
        _ => value,
    }
}

/// Boxes `value` (already-emitted codegen for a `LocalWrite`'s RHS) into
/// `RubyValue::Object` when needed to match the TARGET LOCAL's own storage.
/// `codegen::hoisting::local_storage` decides a local's storage from its
/// WHOLE-SCOPE merged type -- if some OTHER branch assigns this same local a
/// different class (`if cond; x = Foo.new; else; x = Bar.new; end`),
/// `analyze::locals::merge_locals`'s disagreement-widens-to-`Poly` rule
/// makes the local's storage `Hoisted` (a plain `RubyValue` slot) even
/// though THIS particular write's own RHS still emits a bare, unboxed
/// `Arc<Concrete>` (see `box_if_object_typed`'s docs) -- a real `rustc` type
/// mismatch in the generated program, confirmed by direct reproduction with
/// a plain `if`/`else`, no `rescue` needed. A `Shadowed` local (this write's
/// RHS is the ONLY class ever assigned to it) needs no boxing at all -- it
/// keeps its natural unboxed `Arc<Concrete>` type, exactly as
/// `LocalStorage::Shadowed`'s docs describe.
pub(super) fn box_for_local_storage(
    cx: &Ctx,
    name: &str,
    value_id: NodeId,
    value: TokenStream,
) -> TokenStream {
    if super::hoisting::local_storage(cx, name) == super::hoisting::LocalStorage::Shadowed {
        return value;
    }
    box_if_object_typed(cx, value_id, value)
}

/// Boxes a method/closure body's TAIL expression the same way, for the exact
/// same reason: the enclosing function always returns `Result<RubyValue,
/// Signal>`, but a bare tail `HirNode::New`/`SelfRef`/`Shadowed`-local-read
/// (e.g. `def identity; self; end`, or `def make; Foo.new; end`) emits an
/// unboxed `Arc<Concrete>` that can't go directly into `Ok(...)`. See
/// `codegen::stmt::emit_statement`'s only call site.
pub(super) fn box_for_tail_return(cx: &Ctx, id: NodeId, value: TokenStream) -> TokenStream {
    box_if_object_typed(cx, id, value)
}

/// Boxes a tail `LocalWrite`'s write-then-read value (see `emit_expr`'s
/// `LocalWrite` arm) to `RubyValue`. Only a `Shadowed` local reads back as an
/// unboxed `Arc<Concrete>` needing the `RubyValue::Object` wrap; `Hoisted`
/// and `Captured` slots both hold a real `RubyValue` already.
///
/// The STORAGE decides, not the type. A local can be typed `Object` and still
/// stored boxed -- assigned `Foo.new` on every path (so nothing widens it to
/// `Poly`) but read from inside a block, which makes it `Captured`. irb's
/// `ext/loader.rb` writes exactly that (`ws = WorkSpace.new(...)` in both
/// arms of an `if`, then `irb.suspend_workspace(ws) do ... end`), and keying
/// on the type alone boxed a `RubyValue` a second time.
pub(super) fn box_tail_local_write(cx: &Ctx, name: &str, value: TokenStream) -> TokenStream {
    if super::hoisting::local_storage(cx, name) != super::hoisting::LocalStorage::Shadowed {
        return value;
    }
    match cx.local_types.get(name) {
        Some(TyKind::Object(cid)) => {
            let class_ident = super::ident::class_ident(cx.compiler, *cid);
            quote! { zeo_rt::RubyValue::Object(#class_ident::new_handle(#value)) }
        }
        _ => value,
    }
}

/// The already-resolved OWNER class id for a `@@name` reference (see
/// `analyze::mro::resolve_cvars`) -- looked up via `cx.defining_class` (the
/// class/module whose HIR body this reference is LEXICALLY written in), not
/// `cx.current_class` (the concrete struct it's materialized onto for a
/// mixed-in/inherited method): cvar ownership is a property of where the
/// code was WRITTEN, exactly like a closure's lexical scope, not of which
/// concrete receiver ends up calling it.
fn cvar_owner_id(cx: &Ctx, name: &str) -> u32 {
    // A bare `@@x` written outside any class/module body is legal (if
    // unusual) Ruby; its storage lives on `Object` -- the same top-level
    // owner a bare constant resolves to (see `const_owner_id_opt`, and the
    // matching `Object`-seeding in `analyze::mro::resolve_cvars`).
    let defining = cx.defining_class.unwrap_or_else(|| {
        if cx.box_id != 0 {
            cx.compiler
                .box_surrogate(cx.box_id)
                .expect("analyze registers a surrogate for every allocated box")
        } else {
            crate::compiler::OBJECT_CLASS
        }
    });
    // Cvar lookup walks PAST singleton crefs (ruby's rule -- see
    // `Hir::cvar_is_toplevel`): a `class << self` surrogate lexical home
    // defers to the class itself, whose `cvar_owners` the resolver filled.
    let defining = if cx.compiler.is_singleton_surrogate(defining) {
        cx.compiler
            .class(defining)
            .lexical_parent
            .unwrap_or(defining)
    } else {
        defining
    };
    cx.compiler
        .class(defining)
        .cvar_owners
        .get(name)
        .copied()
        .unwrap_or(defining)
        .0
}

/// Wraps one class-level `@x` reference in a block holding its own `static`
/// [`zeo_rt::CivarSite`], which resolves the storage slot once and then holds
/// it. The global name-keyed table stays the source of truth -- reflection
/// (`Class#instance_variable_get`, `#instance_variables`) still reaches the
/// same slots -- but a read in a loop no longer takes a process-wide lock and
/// hashes a class id and a name to find storage it already found.
///
/// The `static` sits INSIDE the block, so two references to the same name in
/// one expression each get their own, and a nested reference (the read in
/// `@a = @b`) shadows rather than collides.
fn civar_site(
    class: ClassId,
    ivar: &str,
    body: impl FnOnce(&proc_macro2::Ident) -> TokenStream,
) -> TokenStream {
    // Upper-cased only to satisfy `non_upper_case_globals` -- the emission must
    // stay warning-free. Two names differing solely in case (`@a` and `@A` are
    // distinct ivars) cannot collide: every site gets its own block.
    //
    // Anything that is not alphanumeric becomes `_`, because the name arriving
    // here has already been through `safe_ident`: a keyword-named ivar (`@type`,
    // seahorse) comes in as the raw identifier `r#type`, and upper-casing that
    // gave `R#TYPE`, which is not an identifier at all -- `format_ident!`
    // panicked on it rather than reporting anything.
    let key: String = ivar
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    let site = quote::format_ident!("__CIV_{}_{}", class.0, key);
    let id = class.0;
    let inner = body(&site);
    quote! {
        {
            static #site: zeo_rt::CivarSite = zeo_rt::CivarSite::new(#id, #ivar);
            #inner
        }
    }
}

/// An ivar WRITE as a bare Rust STATEMENT (no read-back) -- factored out of
/// `emit_expr`'s `IvarWrite` arm so `codegen::loops::emit_target_write` (a
/// multi-assignment/`for`-loop ivar target) can reuse the identical
/// `self_ident`-aware write, instead of re-deriving it.
pub(super) fn emit_ivar_write_stmt(cx: &Ctx, name: &str, value: TokenStream) -> TokenStream {
    let ident = safe_ident(name);
    let slf = &cx.self_ident;
    // A dynamically-typed self: resolve by name at runtime (see `IvarRead`'s
    // arm). `ivar_set_dyn` carries the frozen check the static path emits
    // inline below -- it can compute the receiver's real class name for the
    // message, which is exactly what a statically-unknown self couldn't.
    //
    // Checked BEFORE `class_self`, and it MUST stay paired with `IvarRead`'s
    // matching order: when the two disagree, `@v += 1` in a `class_eval`-
    // nested `def` reads the instance but writes the class table, so the
    // increment silently evaporates.
    if cx.self_is_dynamic {
        let key = ident.to_string();
        // Borrowed for the same reason as `IvarRead`'s arm above.
        return match dyn_ivar_slot(cx, name) {
            Some(slot) => quote! { zeo_rt::ivar_slot_set_dyn(&#slf, #slot, #key, #value)?; },
            None => quote! { zeo_rt::ivar_set_dyn(&#slf, #key, #value)?; },
        };
    }
    // `self` is a CLASS object -- see the matching arm in `IvarRead`. The
    // frozen-class guard lives inside `class_ivar_set` itself (a frozen
    // class raises `can't modify frozen Class: Foo`), hence the `?`.
    if let Some(cid) = cx.class_self {
        let store = civar_site(cid, &ident.to_string(), |site| {
            quote! { #site.set(#value)?; }
        });
        return quote! { #store; };
    }
    // The TOP LEVEL: `self` is `main`, a runtime `Object` whose ivars are a
    // name-keyed map rather than struct fields (no compile-time class exists
    // whose ivar list codegen could have materialized). Same `ivar_set_dyn`
    // path a dynamic self takes -- see `dispatch::Object`'s docs.
    if cx.current_class.is_none() {
        let key = ident.to_string();
        return quote! { zeo_rt::ivar_set_dyn(&zeo_rt::main_object(), #key, #value)?; };
    }
    // The `.freeze` guard -- checked at the top of every ivar
    // write, mirroring CRuby's own `rb_check_frozen` in `vm_setivar_slowpath`.
    // `RubyObject::is_frozen` is UFCS-qualified: generated programs never
    // `use` the trait by name. The one atomic load this adds to every ivar
    // write (including inside `initialize`, where it's always false) is
    // negligible; the message is built by `ivar_frozen_error`, out of line, so
    // the write site carries a branch and a call rather than the
    // `format!`/`inspect`/`construct_by_class_id` this used to inline at every
    // one of them.
    //
    // Boxed via the concrete `new_handle` (not a bare
    // `RubyValue::Object(Clone::clone(..))`): the expected `Arc<dyn
    // RubyObject>` would drive UFCS-clone inference BACKWARDS into the generic
    // and defeat the unsize coercion; `new_handle`'s concrete `Arc<Self>`
    // parameter anchors it. The `Arc` bump only happens on the raise path.
    let current = cx
        .current_class
        .expect("ivar write outside a class context");
    let class_ident = super::ident::class_ident(cx.compiler, current);
    // Writing is what makes an ivar DEFINED -- the cell stamps it with an
    // assignment order the old per-slot `Option` had nowhere to record.
    let store = match ivar_slot(cx, current, name) {
        Some(slot) => quote! { #slf.__ivars.set(#slot, #value); },
        None => {
            let key = ident.to_string();
            quote! { #slf.__ivars.set_named(#class_ident::__IVAR_NAMES, #key, #value); }
        }
    };
    quote! {
        if zeo_rt::RubyObject::is_frozen(&*#slf) {
            return Err(zeo_rt::ivar_frozen_error(
                #class_ident::new_handle(Clone::clone(&#slf)),
            ));
        }
        #store
    }
}

/// The slot a DYNAMIC self's ivar occupies, for the one context where a
/// `RubyValue` receiver still has a known layout: a body `codegen::share`
/// emits once for a whole hierarchy, whose members all place the name at the
/// same index. `None` everywhere else, which keeps the by-name path.
fn dyn_ivar_slot(cx: &Ctx, name: &str) -> Option<usize> {
    if !cx.self_slots {
        return None;
    }
    cx.ask_opt(super::class_query::ClassQuery::IvarSlot(name.to_string()))?
        .slot()
}

/// Which slot of the receiver's `IvarCell` one ivar occupies -- its position in
/// the class's own declared list, which is the order `ruby_class!` lays the
/// slots out in. `None` for a name the class never declared, which the emitted
/// code must still reach through the cell's by-NAME entry so it lands in the
/// same storage `instance_variable_get` sees.
pub(super) fn ivar_slot(cx: &Ctx, class: ClassId, name: &str) -> Option<usize> {
    slot_of(cx.compiler, class, name)
}

/// [`ivar_slot`] without a `Ctx`, for the trampoline emitter.
///
/// The declared ivars occupy the leading slots and a compiled `Struct`'s
/// MEMBERS follow them, matching how `ruby_class!` lays the cell out -- which
/// is what keeps a member out of every by-name path while costing it nothing
/// to reach.
pub(super) fn slot_of(
    compiler: &crate::compiler::Compiler,
    class: ClassId,
    name: &str,
) -> Option<usize> {
    let info = compiler.class(class);
    info.ivars.iter().position(|iv| iv == name).or_else(|| {
        info.hidden_ivars
            .iter()
            .position(|iv| iv == name)
            .map(|i| info.ivars.len() + i)
    })
}

/// A cvar WRITE as a bare Rust STATEMENT -- see `emit_ivar_write_stmt`'s docs
/// for why this is factored out the same way.
pub(super) fn emit_cvar_write_stmt(cx: &Ctx, name: &str, value: TokenStream) -> TokenStream {
    let owner = cvar_owner_id(cx, name);
    // Fallible: a frozen OWNER class refuses the write -- see `cvar_set`.
    quote! { zeo_rt::cvar_set(#owner, #name, #value)?; }
}

/// The already-resolved OWNER class id for a bare (lexically-scoped)
/// constant reference (`scope: None`) or an explicit `Foo::NAME`
/// (`scope: Some("Foo")`) -- mirrors `cvar_owner_id`'s exact "resolved once
/// at analyze time" scheme (`analyze::mro::resolve_consts`), with one
/// deliberate divergence: a bare reference with NO enclosing class/module
/// body at all (ordinary top-level code, `cx.defining_class: None`) resolves
/// against `compiler::OBJECT_CLASS` directly instead of panicking the way
/// `cvar_owner_id` does -- real Ruby stores a top-level constant on `Object`
/// itself, and (unlike a bare `@@x`) a bare top-level `FOO` is completely
/// ordinary, valid Ruby. Never panics on a missing map entry either (unlike
/// `cvar_owner_id`): a constant genuinely never assigned anywhere reachable
/// still needs to resolve to SOME owner id so the runtime `const_get` lookup
/// can correctly report "unset" (raising `NameError` -- see `emit_const_read`'s
/// docs) as an ordinary RUNTIME outcome, not a zeo-compile-time panic --
/// an unset constant is legitimately-valid-but-erroring Ruby, not a
/// programming mistake in the compiler itself.
/// The owner id for a constant of a class the caller ALREADY resolved.
///
/// Infallible by construction, because it never re-derives the class: a
/// `ClassId` in hand is the answer, so there is nothing to look up and nothing
/// to get wrong. The name-taking [`const_owner_id_opt`] is for callers that
/// genuinely start from a name.
///
/// This exists because the alternative was tried and was BOTH a panic and a
/// miscompile. `try_const_reflection` held a resolved `ClassId`, rendered it
/// with `Compiler::fq_name` -- which yields an UNANCHORED `Foo::Bar` -- and
/// re-resolved that string against the emit site's cref chain. `resolve_class`
/// walks innermost-outward, so a nested constant that shadows the namespace
/// head captures the walk: appraisal2 has `class Appraisal` inside `module
/// Appraisal`, and from inside that module the head segment `Appraisal` found
/// the CLASS, which has no `Hooks`. That raised
/// `unknown class/module in const_owner_id`.
///
/// The panic was the lucky half. When the shadowing class also answers the
/// name, the round trip resolves to the WRONG owner and silently reads the
/// wrong constant -- see `tests/const_get_on_a_shadowed_namespace_head.rb`.
pub(super) fn const_owner_of(cx: &Ctx, owner_class: crate::compiler::ClassId, name: &str) -> u32 {
    cx.compiler
        .class(owner_class)
        .const_owners
        .get(name)
        .copied()
        .unwrap_or(owner_class)
        .0
}

/// The class a BARE constant belongs to at this emit site's top level: the
/// box's own surrogate inside a `Ruby::Box` (readable externally as
/// `box::CONST`, invisible to every other box), and the shared `Object` root
/// for the main program.
fn box_top_owner(cx: &Ctx) -> u32 {
    if cx.box_id == 0 {
        return crate::compiler::OBJECT_CLASS.0;
    }
    cx.compiler
        .box_surrogate(cx.box_id)
        .expect("analyze registers a surrogate for every allocated box")
        .0
}

/// The owner id for a constant named by a SCOPE STRING, resolved against this
/// emit site's cref chain. A caller that already holds the class wants
/// [`const_owner_of`] instead -- resolving is what can go wrong, so a caller
/// that need not resolve must not.
///
/// Returns `None` when an explicit
/// `Scope::NAME` names a scope class that isn't registered (e.g. a reference to
/// `OpenSSL::Digest` when `require "openssl"` didn't materialize the module).
/// Callers that lower EVERY branch eagerly (a `defined?` guard, a dead `if`
/// arm) use this to emit a runtime `NameError`/`nil` instead of a compile-time
/// panic -- CRuby only raises `uninitialized constant` if the branch actually
/// runs, so a never-executed reference must compile cleanly.
pub(super) fn const_owner_id_opt(cx: &Ctx, scope: Option<&str>, name: &str) -> Option<u32> {
    let owner_class = match scope {
        Some(class_name) => cx.resolve_class(class_name)?,
        // A bare constant at a BOX's top level is owned by the
        // box's surrogate -- readable externally as `box::CONST`, invisible
        // to other boxes; `Object` (the shared id-0 root) stays the owner
        // only for the root program.
        None => cx
            .defining_class
            .unwrap_or_else(|| crate::compiler::ClassId(box_top_owner(cx))),
    };
    Some(const_owner_of(cx, owner_class, name))
}

/// A constant READ -- `scope: None` for a bare `NAME` (see `const_owner_id_opt`'s
/// docs for the lexical-then-top-level resolution rule), `scope:
/// Some(class_name)` for an explicit `Foo::NAME`. An unset constant raises a
/// real `NameError` (unlike an ivar/cvar/global's "never assigned" -> `nil`
/// convention) -- matches actual Ruby, and is cheap here since the runtime
/// `const_get` already distinguishes "never set" (`None`) from "set to
/// `nil`" (`Some(RubyValue::Nil)`).
/// The class a `Scope::NAME` path names, if any: the nested `Scope::NAME`
/// directly, or -- since a top-level constant lives on `Object` and is visible
/// through any scope -- a top-level class of that name (so `::Integer`, lowered
/// as `Object::Integer`, resolves to the builtin `Integer` class rather than an
/// unset value constant).
fn qualified_const_class(cx: &Ctx, scope: &str, name: &str) -> Option<ClassId> {
    cx.resolve_class(&format!("{scope}::{name}"))
        // A top-level anchor `::Name` lowers with scope "Object" (the root),
        // where the name is an ordinary top-level class -- resolve it directly.
        // Restricted to the "Object" scope: an arbitrary `Scope::Name` must NOT
        // fall back to a same-named top-level class (`M::V` is M's own value
        // constant `V`, not the unrelated top-level module `V`).
        //
        // ANCHORED, not bare: `::Widget` written inside a `NS::Widget` that
        // shadows it must answer the TOP-LEVEL one, and a bare re-resolution
        // walks the lexical chain and answers the shadow -- silently, with no
        // error anywhere. That is the whole meaning of the `::` prefix.
        .or_else(|| {
            cx.resolve_class(&format!("::{name}"))
                .filter(|_| scope == "Object")
        })
}

/// Split a written constant PATH into the `(scope, leaf)` pair
/// [`emit_const_read`] takes: `"M::ALIAS"` -> `(Some("M"), "ALIAS")`.
pub(super) fn split_const_path(path: &str) -> (Option<&str>, &str) {
    match path.rsplit_once("::") {
        Some((scope, leaf)) => (Some(scope), leaf),
        None => (None, path),
    }
}

/// The guard a `Scope::NAME` naming a `private_constant` carries. `None` for a
/// public name, and for a BARE name -- `private_constant` rejects the scope
/// OPERATOR, not the reader: `M::S` is a NameError everywhere, even inside
/// `M`'s own body, while a bare `S` resolved through the cref is fine
/// (oracle-verified). So there is no cref comparison to make.
///
/// A guard rather than the raise itself, because a later
/// `M.public_constant :S` restores the name and only the runtime flag knows.
/// It costs one probe, and only where the compiler already saw a
/// `private_constant` -- every ordinary constant read is untouched.
/// The runtime-conditional class a `defined?(X)` / `defined?(A::B)` form
/// statically names, if any. Such a form must never fold to the literal
/// `"constant"` (or to nil): the answer is the concealment table's, the same
/// authority every other by-name path consults.
fn conditional_class_of_defined_form(cx: &Ctx, id: NodeId) -> Option<ClassId> {
    let cid = match &cx.compiler.hir[id] {
        HirNode::ClassRef(name) => cx.resolve_class(name)?,
        HirNode::QualifiedConstRead(scope, name) => qualified_const_class(cx, scope, name)?,
        _ => return None,
    };
    cx.compiler.class(cid).runtime_conditional.then_some(cid)
}

/// Whether the SCOPE OPERATOR in a `defined?` form names a private constant --
/// a runtime test, so a later `public_constant` restores it. `false` for a bare
/// reference, which the private rule does not gate.
fn defined_form_private_check(cx: &Ctx, id: NodeId) -> TokenStream {
    let (scope, base) = match &cx.compiler.hir[id] {
        HirNode::QualifiedConstRead(scope, name) => (scope.clone(), name.clone()),
        HirNode::ClassRef(name) => {
            let path = crate::constpath::ConstPath::parse(name);
            match path.scope() {
                Some(s) => (s.to_string(), path.base().to_string()),
                None => return quote! { false },
            }
        }
        _ => return quote! { false },
    };
    match cx.resolve_class(&scope) {
        Some(sid) => {
            let owner = sid.0;
            quote! { zeo_rt::const_is_private(#owner, #base) }
        }
        None => quote! { false },
    }
}

pub(super) fn private_const_guard(
    cx: &Ctx,
    scope: Option<&str>,
    name: &str,
) -> Option<TokenStream> {
    let scope = scope?;
    let owner = const_owner_id_opt(cx, Some(scope), name)?;
    if !cx
        .compiler
        .class(ClassId(owner))
        .private_constants
        .contains(name)
    {
        return None;
    }
    let path = format!("{}::{name}", cx.compiler.fq_name(ClassId(owner)));
    Some(quote! {
        if zeo_rt::const_is_private(#owner, #name) {
            return Err(zeo_rt::Signal::Raise(zeo_rt::stamp_backtrace(
                zeo_rt::make_name_error(
                    format!("private constant {} referenced", #path),
                    #name,
                    zeo_rt::RubyValue::Class(zeo_rt::ClassId(#owner)),
                ),
            )));
        }
    })
}

/// The constant name as the tail of a Rust identifier -- the per-site cache
/// slot's.
///
/// A constant read can arrive with a PATH for a name (`Dry::Types::Result::
/// Failure`, from a namespaced runtime class), and `::` is not an identifier
/// character: `format_ident!` PANICS on one rather than returning an error, so
/// a compile that should have produced a program produced a backtrace instead.
///
/// Two names that sanitize alike would collide -- as duplicate `static`s, which
/// rustc reports by name and line. That is a strictly better failure than a
/// panic, which reports nothing about the ruby that caused it.
fn const_slot(name: &str) -> String {
    name.to_uppercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub(super) fn emit_const_read(cx: &Ctx, scope: Option<&str>, name: &str) -> TokenStream {
    // An explicit `Scope::NAME` whose scope class isn't registered is a
    // `NameError` on the missing SCOPE (`uninitialized constant OpenSSL`),
    // deferred to runtime so a dead/guarded branch still compiles.
    let Some(owner) = const_owner_id_opt(cx, scope, name) else {
        // A scope that isn't a compile-time class may still be a RUNTIME
        // constant holding one (`Line = Struct.new(...)` / `Data.define` /
        // `class SecretKeys::Encryptor` under a computed superclass), so
        // resolve the path at runtime: read the scope, then the leaf on the
        // class it names. `uninitialized constant Line::FLAGS` (leaf missing)
        // vs `uninitialized constant Line` (scope missing) then matches CRuby.
        //
        // Recursive on the scope, so `K::C::P` works the same way `K::C` does:
        // the inner read is the one that raises for a missing HEAD, and it
        // reports that head exactly as a bare miss is reported.
        if let Some(s) = scope {
            // A TOP-ANCHORED scope (`::Tilt::Template`) splits with an empty
            // head; that is the anchor, not a namespace to look `Tilt` up in.
            let (head, leaf) = split_const_path(s);
            let head = head.filter(|h| !h.is_empty());
            let scope_expr = emit_const_read(cx, head, leaf);
            // `Object` is never NAMED as the scope: ruby reports
            // `Object::AlsoMissing` as "uninitialized constant AlsoMissing",
            // the same as a bare miss, since Object is where a top-level
            // constant lives anyway.
            let qualified = match s {
                "Object" => name.to_string(),
                _ => format!("{s}::{name}"),
            };
            return quote! {
                match #scope_expr {
                    zeo_rt::RubyValue::Class(__cid) => match zeo_rt::const_get_scoped(__cid.0, #name) {
                        Some(__v) => __v,
                        None => return Err(zeo_rt::Signal::Raise(zeo_rt::stamp_backtrace(zeo_rt::make_name_error(
                            format!("uninitialized constant {}", #qualified),
                            #name,
                            zeo_rt::RubyValue::Class(__cid),
                        )))),
                    },
                    __other => return Err(zeo_rt::raise_error(
                        "TypeError",
                        format!("{} is not a class/module", __other.inspect_string()),
                    )),
                }
            };
        }
        let missing = scope.unwrap_or(name);
        // Yields `RubyValue` on its unreachable `Ok` arm, so this type-checks
        // in every position a const read appears (incl. a borrowed argument
        // `&(...)`).
        return raise_in_expr_position(
            uninitialized_constant_error(cx, missing),
            quote! { zeo_rt::RubyValue },
        );
    };
    let private_guard = private_const_guard(cx, scope, name);
    // The `NameError` message mirrors real Ruby's: an explicit path prints
    // as written (`uninitialized constant Store::MISSING`); a bare miss
    // inside a class/module body is qualified by the cref head's
    // fully-qualified name (`uninitialized constant Store::Cart::DEFAULT`
    // -- oracle-verified); a bare top-level miss stays bare. A top-level `def`
    // has `Object` as its defining class, and `Object` is where a bare lookup
    // ENDS rather than a namespace it was qualified by, so it prints bare too.
    let qualified = match (scope, cx.defining_class) {
        // An EXPLICIT `Object::` scope prints bare for the same reason the
        // `defining_class == Object` case below does: Object is where a
        // lookup ENDS, not a namespace the name was qualified by, so ruby
        // reports `Object::AlsoMissing` as "uninitialized constant
        // AlsoMissing".
        (Some("Object"), _) => name.to_string(),
        (Some(s), _) => format!("{s}::{name}"),
        (None, Some(d)) if d != zeo_abi::OBJECT_CLASS => {
            format!("{}::{name}", cx.compiler.fq_name(d))
        }
        _ => name.to_string(),
    };
    // The raised `NameError` carries `#name` (the missing leaf as a Symbol) and
    // `#receiver` (the class the lookup ran against -- `Object` at top level, the
    // enclosing module for a nested miss), matching CRuby.
    // The top level is where Ruby's bare-name lookup ends, and the only
    // constants the compile-time owner map can't already have placed are the
    // ones the runtime installs (`RUBY_RELEASE_DATE` and friends) -- which is
    // exactly why a bare read inside a nested module needs this tail. An
    // explicit `Scope::NAME` gets no such fallback; CRuby doesn't give it one
    // either.
    //
    // Inside a BOX the top level is the box's own surrogate, and the tail past
    // it reaches only the MASTER constants -- the ones installed before the
    // main program ran. Falling straight through to `Object` would hand the box
    // main's own top-level constants, which a box (a copy of master) never
    // sees.
    //
    // An explicit scope also searches the SCOPE ITSELF rather than the owner
    // the compile-time map redirects to. That map answers where a name WOULD
    // resolve from, which for a class means `Object` (an ancestor of them all)
    // -- and the scope operator is the one lookup `Object`'s own constants are
    // out of reach of. The ancestry the redirect stood in for is walked at run
    // time instead, by `const_get_scoped`.
    let top = box_top_owner(cx);
    let scope_owner = scope.map(|s| {
        cx.resolve_class(s)
            .expect("const_owner_id_opt resolved this scope above")
            .0
    });
    let owner = scope_owner.unwrap_or(owner);
    let mut lookup = match scope_owner {
        Some(sid) => quote! { zeo_rt::const_get_scoped(#sid, #name) },
        None => quote! { zeo_rt::const_get(#owner, #name) },
    };
    if scope.is_none() {
        // Ruby searches every entry of `Module.nesting` OUTWARD before it
        // reaches the top, and this chain used to jump straight from the
        // innermost cref to it. Any bare name the compile-time owner map cannot
        // place -- which includes every constant on a class built at runtime --
        // was then looked for in two scopes out of the three or more Ruby
        // searches. jmespath reads `Token::BINDING_POWER` inside `class Parser`
        // in `module JMESPath`, where `Token` is a `Struct.new` subclass and so
        // has no compile-time owner: the lookup asked `JMESPath::Parser` and
        // `Object`, skipped `JMESPath` itself, and raised.
        //
        // `cref_parent`, not `lexical_parent`: the qualified form `class
        // Store::Item` does NOT see `Store`'s constants lexically, and only the
        // cref chain draws that distinction.
        let mut at = cx
            .compiler
            .class(crate::compiler::ClassId(owner))
            .cref_parent;
        while let Some(cid) = at {
            if cid.0 != owner && cid.0 != top {
                let enclosing = cid.0;
                lookup = quote! { #lookup.or_else(|| zeo_rt::const_get(#enclosing, #name)) };
            }
            at = cx.compiler.class(cid).cref_parent;
        }
        if owner != top {
            lookup = quote! { #lookup.or_else(|| zeo_rt::const_get(#top, #name)) };
        }
        if cx.box_id != 0 {
            lookup = quote! { #lookup.or_else(|| zeo_rt::const_get_master(#name)) };
        }
    }
    // The lookup behind a constant read is a global lock and at least two
    // hashes, and the same site asks the same question every time it runs, so
    // it caches what it found -- see `zeo_rt::ConstSite` for how the cache
    // knows when to stop trusting itself. Only a HIT is cached: a miss raises,
    // and the constant may well be defined by the time the site runs again.
    //
    // A miss on a scope whose chain defines a USER `const_missing` dispatches
    // the hook instead of the baked raise (CRuby's protocol); every other
    // site keeps the raise, whose as-written wording the default hook could
    // not reproduce.
    let site = quote::format_ident!("__CONST_{}_{}", owner, const_slot(name));
    let miss_owner = crate::compiler::ClassId(owner);
    let miss = if cx
        .compiler
        .class_method_in_chain(miss_owner, "const_missing")
        .is_some()
    {
        quote! { zeo_rt::const_miss(zeo_rt::ClassId(#owner), #name)? }
    } else {
        quote! {
            return Err(zeo_rt::const_miss_signal(
                zeo_rt::ClassId(#owner),
                #name,
                &format!("uninitialized constant {}", #qualified),
            ))
        }
    };
    quote! {
        {
            #private_guard
            static #site: zeo_rt::ConstSite = zeo_rt::ConstSite::new();
            match #site.get(|| #lookup) {
                Some(__v) => __v,
                None => #miss,
            }
        }
    }
}

/// A constant WRITE as a bare Rust STATEMENT -- see `emit_ivar_write_stmt`'s
/// docs for why this is factored out the same way (reused by
/// `codegen::loops::emit_target_write`'s `Const` multi-assignment target).
pub(super) fn emit_const_write_stmt(
    cx: &Ctx,
    scope: Option<&str>,
    name: &str,
    value: TokenStream,
    at: Option<crate::hir::NodeId>,
) -> TokenStream {
    // An explicit `Scope::NAME = ...` whose scope class isn't registered is a
    // `NameError` on the missing scope. CRuby resolves the scope BEFORE
    // evaluating the value (`Nope::X = (puts 1; 5)` raises without printing --
    // verified against ruby 4.0.6), so the value is deliberately NOT emitted
    // here. Deferred to runtime so a dead/guarded branch still compiles.
    let Some(owner) = const_owner_id_opt(cx, scope, name) else {
        let err = uninitialized_constant_error(cx, scope.unwrap_or(name));
        return quote! { return Err(zeo_rt::Signal::Raise(#err)); };
    };
    // `Module#const_source_location` reads back where the assignment was
    // written, so it travels with the write. A re-assignment restamps it, and
    // a span-less write (a synthetic one) simply records nothing.
    match at.and_then(|n| super::source_location(cx.compiler, n)) {
        Some((file, line)) => {
            let file = super::pooled_file(file);
            quote! { zeo_rt::const_set_at(#owner, #name, #value, #file, #line); }
        }
        None => quote! { zeo_rt::const_set(#owner, #name, #value); },
    }
}

// ---------------------------------------------------------------------------
// FFI: emit an `attach_function` wrapper body -- a self-contained block
// that declares the C symbol `extern "C"` (fn-locally, `#[link(name = ..)]`ed,
// so rustc links the library with no build-step change), marshals each Ruby
// argument to its C type, calls the symbol, and wraps the C result back into a
// `RubyValue`. The scalar type surface lives in `crate::hir::FfiType`.
// ---------------------------------------------------------------------------

fn emit_ffi_call(cx: &Ctx, call: &crate::hir::FfiCall) -> TokenStream {
    let link = match &call.lib {
        crate::hir::FfiLib::Static(lib) => quote! { #[link(name = #lib)] },
        crate::hir::FfiLib::None => quote! {},
        // Only a running process can open the library (a bundled `.so` path,
        // a versioned soname): resolve with dlopen/dlsym at the call site --
        // which is when and how CRuby's ffi gem binds every symbol -- and
        // call through libffi.
        crate::hir::FfiLib::Runtime(candidates) => {
            let sym = &call.symbol;
            let cands = candidates.iter();
            let addr = quote! { __FFI_SYM.get(&[#(#cands),*], #sym)? };
            return emit_ffi_runtime_call(cx, call, addr);
        }
        // The library was dlopened when the class body executed (`ffi_lib`
        // with runtime-only candidates); the handle sits in the slot.
        crate::hir::FfiLib::Deferred { slot } => {
            let sym = &call.symbol;
            let addr = quote! { __FFI_SYM.get_slot(#slot, #sym)? };
            return emit_ffi_runtime_call(cx, call, addr);
        }
    };
    // A variadic function's argument list is shaped at runtime, so it goes
    // through libffi rather than a fixed `extern "C"` signature.
    if let Some(rest_id) = call.variadic {
        return emit_ffi_variadic(cx, call, rest_id, &link);
    }

    let sym = quote::format_ident!("{}", call.symbol);
    let mut struct_defs = Vec::new();
    let mut extern_params = Vec::new();
    let mut bindings = Vec::new();
    let mut call_idents = Vec::new();
    let mut has_callback = false;
    for (i, (arg_id, ty)) in call.args.iter().enumerate() {
        let pname = quote::format_ident!("__ffi_arg{}", i);
        // A by-value struct: a `#[repr(C)]` mirror of the recorded layout,
        // read out of the ruby object's backing `MemoryPointer`. rustc owns
        // the ABI classification, which is why the mirror carries the REAL
        // field types -- a byte blob classifies differently on SysV/AArch64.
        if let crate::hir::FfiType::Struct(layout) = ty {
            let sid = quote::format_ident!("__FfiS{}", i);
            struct_defs.push(emit_repr_c_struct(&sid, layout));
            extern_params.push(quote! { #pname: #sid });
            let val = emit_expr(cx, *arg_id);
            bindings.push(quote! {
                let #pname: #sid = unsafe {
                    ::std::ptr::read_unaligned(zeo_rt::ffi::to_pointer(&#val)? as *const #sid)
                };
            });
            call_idents.push(quote! { #pname });
            continue;
        }
        let cty = ffi_c_type(ty);
        extern_params.push(quote! { #pname: #cty });
        let val = emit_expr(cx, *arg_id);
        bindings.push(ffi_marshal_in(ty, &pname, val));
        call_idents.push(quote! { #pname });
        has_callback |= matches!(ty, crate::hir::FfiType::Callback(..));
    }
    // A struct returned BY VALUE comes back as its `#[repr(C)]` mirror --
    // rustc owns the ABI classification, same as the argument direction
    // above. The bytes are copied into a fresh ruby-owned `MemoryPointer`;
    // the synthesized wrapper's `New` then views it through the struct's own
    // class (see `lower_attach_function`).
    let (ret_cty, wrap) = if let crate::hir::FfiType::Struct(layout) = &call.ret {
        let rid = quote::format_ident!("__FfiSRet");
        struct_defs.push(emit_repr_c_struct(&rid, layout));
        let size = proc_macro2::Literal::usize_unsuffixed(layout.size);
        (
            quote! { #rid },
            quote! {
                unsafe { zeo_rt::ffi::from_struct_ret(&__ffi_ret as *const #rid as *const u8, #size) }
            },
        )
    } else {
        (ffi_c_type(&call.ret), ffi_wrap_ret(&call.ret))
    };
    // An exception raised inside a callback can't unwind through C; it was
    // stashed and is re-raised here, after the C function returns.
    let cb_check = if has_callback {
        quote! { zeo_rt::ffi::take_callback_error()?; }
    } else {
        quote! {}
    };
    // `blocking: true` runs the C call with the GVL released, so a long call
    // does not stall other ruby threads. A no-op in the default parallel mode,
    // and the real handoff under `ZEO_GVL=1`.
    let invoke = if call.blocking {
        quote! { zeo_rt::gvl::without_gvl(|| unsafe { #sym(#(#call_idents),*) }) }
    } else {
        quote! { unsafe { #sym(#(#call_idents),*) } }
    };
    quote! {
        {
            #(#struct_defs)*
            #link
            extern "C" {
                fn #sym(#(#extern_params),*) -> #ret_cty;
            }
            #(#bindings)*
            let __ffi_ret = #invoke;
            #cb_check
            #wrap
        }
    }
}

/// The `#[repr(C)]` mirror of a recorded struct layout, with explicit
/// `__padN` fillers where the C layout has gaps, and compile-time asserts
/// that rustc placed every field where the accessor synthesis did -- a
/// mismatch is a build error, never silent corruption.
fn emit_repr_c_struct(
    ident: &proc_macro2::Ident,
    layout: &crate::hir::FfiStructLayout,
) -> TokenStream {
    let mut fields = Vec::new();
    let mut asserts = Vec::new();
    let mut nested = Vec::new();
    let mut cursor = 0usize;
    for (i, (name, ty, off)) in layout.fields.iter().enumerate() {
        if *off > cursor {
            let pad = quote::format_ident!("__pad{}", i);
            let width = proc_macro2::Literal::usize_unsuffixed(*off - cursor);
            fields.push(quote! { #pad: [u8; #width] });
        }
        let fname = quote::format_ident!("f{}", i);
        let fty = match ty {
            crate::hir::FfiType::Array(elem, count) => {
                let ecty = ffi_c_type(elem);
                let count = proc_macro2::Literal::usize_unsuffixed(*count);
                quote! { [#ecty; #count] }
            }
            // A nested struct stored BY VALUE mirrors recursively -- the ABI
            // needs the REAL field types at every depth (see the type's own
            // docs on SysV/AArch64 classification).
            crate::hir::FfiType::Struct(inner) => {
                let nid = quote::format_ident!("{}F{}", ident, i);
                nested.push(emit_repr_c_struct(&nid, inner));
                quote! { #nid }
            }
            other => ffi_c_type(other),
        };
        fields.push(quote! { #fname: #fty });
        let off_lit = proc_macro2::Literal::usize_unsuffixed(*off);
        let msg = format!("FFI layout drift on field `{name}`");
        asserts.push(quote! {
            assert!(::std::mem::offset_of!(#ident, #fname) == #off_lit, #msg);
        });
        cursor = *off + ffi_field_size(ty);
    }
    if layout.size > cursor {
        let width = proc_macro2::Literal::usize_unsuffixed(layout.size - cursor);
        fields.push(quote! { __tail: [u8; #width] });
    }
    let size = proc_macro2::Literal::usize_unsuffixed(layout.size);
    quote! {
        #(#nested)*
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct #ident { #(#fields),* }
        const _: () = {
            #(#asserts)*
            assert!(::std::mem::size_of::<#ident>() == #size, "FFI layout drift on struct size");
        };
    }
}

/// The `zeo_rt::ffi::FfiElem` descriptor of a by-value struct for the
/// runtime tier: the real field types in declaration order, inline arrays
/// expanded to N elements, nested structs recursing. No explicit padding --
/// libffi computes offsets from the same natural-alignment rules the
/// recorded layout's offsets came from, so the two always agree.
fn ffi_elem_slice(layout: &crate::hir::FfiStructLayout) -> TokenStream {
    let elems: Vec<TokenStream> = layout
        .fields
        .iter()
        .flat_map(|(_, ty, _)| ffi_elems_of(ty))
        .collect();
    quote! { &[#(#elems),*] }
}

fn ffi_elems_of(ty: &crate::hir::FfiType) -> Vec<TokenStream> {
    use crate::hir::FfiType;
    match ty {
        FfiType::Array(elem, count) => {
            let one = ffi_elems_of(elem);
            std::iter::repeat_with(|| one.clone())
                .take(*count)
                .flatten()
                .collect()
        }
        FfiType::Struct(inner) => {
            let slice = ffi_elem_slice(inner);
            vec![quote! { zeo_rt::ffi::FfiElem::Struct(#slice) }]
        }
        scalar => {
            let kind = ffi_kind_tokens(scalar);
            vec![quote! { zeo_rt::ffi::FfiElem::Scalar(#kind) }]
        }
    }
}

/// A layout field's byte width -- the shared `CScalar` table's widths, which
/// are also what `lower::ffi::ffi_field_accessor` computed the layout's
/// offsets from.
fn ffi_field_size(ty: &crate::hir::FfiType) -> usize {
    use crate::hir::FfiType::*;
    match ty {
        Array(elem, count) => ffi_field_size(elem) * count,
        Struct(l) => l.size,
        scalar => scalar
            .c_scalar()
            .expect("every non-aggregate FfiType has a C scalar")
            .size(),
    }
}

/// An `attach_function` whose library resolves at RUN time (`FfiLib::
/// Runtime` and `FfiLib::Deferred`): a per-site `FfiSymSite` resolves the
/// symbol once through `addr` (dlopen the candidates, or read a deferred
/// slot's handle), then the call goes through libffi (`call_fixed`) with
/// arguments marshaled to `VaVal`s. Slower than the `extern "C"` tier by one
/// indirect call and the marshal -- and the only tier that can open a
/// library the BUILD machine never saw.
fn emit_ffi_runtime_call(cx: &Ctx, call: &crate::hir::FfiCall, addr: TokenStream) -> TokenStream {
    use crate::hir::FfiType;
    if let Some(rest_id) = call.variadic {
        return crate::codegen::unsupported_at(
            cx.compiler,
            rest_id,
            "a variadic `attach_function` with a runtime-resolved `ffi_lib` isn't supported yet \
             (zeo limitation) -- name the library statically or drop `:varargs`",
        );
    }
    let mut pushes = Vec::new();
    let mut has_callback = false;
    for (i, (arg_id, ty)) in call.args.iter().enumerate() {
        let val = emit_expr(cx, *arg_id);
        pushes.push(match ty {
            // The symbol/int conversion the extern tier does inline.
            FfiType::Enum(members) => {
                let table = ffi_enum_members(members);
                quote! {
                    __vals.push(zeo_rt::ffi::marshal_fixed(
                        zeo_rt::ffi::FfiKind::I32,
                        &zeo_rt::RubyValue::Int(zeo_rt::ffi::enum_to_int(&(#val), #table)?),
                    )?);
                }
            }
            FfiType::EnumSlot(slot) => {
                let slot = proc_macro2::Literal::usize_unsuffixed(*slot);
                quote! {
                    __vals.push(zeo_rt::ffi::marshal_fixed(
                        zeo_rt::ffi::FfiKind::I32,
                        &zeo_rt::RubyValue::Int(
                            zeo_rt::ffi::enum_to_int_slot(#slot, &(#val))?,
                        ),
                    )?);
                }
            }
            FfiType::Callback(cb_args, cb_ret) => {
                has_callback = true;
                let handle = quote::format_ident!("__ffi_cb{}", i);
                let arg_kinds: Vec<TokenStream> = cb_args.iter().map(ffi_kind_tokens).collect();
                let ret_kind = ffi_kind_tokens(cb_ret);
                quote! {
                    let #handle = zeo_rt::ffi::make_callback(&(#val), &[#(#arg_kinds),*], #ret_kind)?;
                    __vals.push(zeo_rt::ffi::va_raw_pointer(#handle.code_ptr()));
                }
            }
            // A struct passed BY VALUE: its bytes stay in the ruby object's
            // own backing; libffi classifies the aggregate from the real
            // field types, same facts the extern tier's repr(C) mirror
            // carries. (A by-value RETURN on this tier is still rejected at
            // the declaration.)
            FfiType::Struct(layout) => {
                let elems = ffi_elem_slice(layout);
                quote! { __vals.push(zeo_rt::ffi::marshal_struct(#elems, &(#val))?); }
            }
            other => {
                let kind = ffi_kind_tokens(other);
                quote! { __vals.push(zeo_rt::ffi::marshal_fixed(#kind, &(#val))?); }
            }
        });
    }
    // `:strptr` calls through the plain Pointer kind; the pair wrap happens
    // in `post` below, over the already-wrapped `FFI::Pointer`.
    // A by-value struct return has no scalar kind at all -- it takes the
    // aggregate path below, so the kind is never asked for.
    let ret_kind = match &call.ret {
        FfiType::StrPtr => quote! { zeo_rt::ffi::FfiKind::Pointer },
        FfiType::Struct(_) => quote! {},
        other => ffi_kind_tokens(other),
    };
    let cb_check = if has_callback {
        quote! { zeo_rt::ffi::take_callback_error()?; }
    } else {
        quote! {}
    };
    // A struct returned BY VALUE takes libffi's aggregate return path: the
    // same field descriptor the argument direction builds, and a ruby-owned
    // buffer the wrapper then views through the struct's class.
    let call_expr = match &call.ret {
        FfiType::Struct(layout) => {
            let elems = ffi_elem_slice(layout);
            let size = proc_macro2::Literal::usize_unsuffixed(layout.size);
            quote! { zeo_rt::ffi::call_struct_ret(__addr, __vals, #elems, #size) }
        }
        _ => quote! { zeo_rt::ffi::call_fixed(__addr, __vals, #ret_kind) },
    };
    let invoke = if call.blocking {
        quote! { zeo_rt::gvl::without_gvl(|| unsafe { #call_expr })? }
    } else {
        quote! { unsafe { #call_expr }? }
    };
    // The extern tier's `int_to_enum`/`strptr` wraps, applied after the
    // generic wrap `call_fixed` already did.
    let post = match &call.ret {
        FfiType::Enum(members) => {
            let table = ffi_enum_members(members);
            quote! {
                match __ffi_ret {
                    zeo_rt::RubyValue::Int(__n) => zeo_rt::ffi::int_to_enum(__n, #table),
                    __other => __other,
                }
            }
        }
        FfiType::EnumSlot(slot) => {
            let slot = proc_macro2::Literal::usize_unsuffixed(*slot);
            quote! {
                match __ffi_ret {
                    zeo_rt::RubyValue::Int(__n) => zeo_rt::ffi::int_to_enum_slot(#slot, __n),
                    __other => __other,
                }
            }
        }
        FfiType::StrPtr => quote! { unsafe { zeo_rt::ffi::strptr_pair(__ffi_ret) } },
        _ => quote! { __ffi_ret },
    };
    quote! {
        {
            static __FFI_SYM: zeo_rt::ffi::FfiSymSite = zeo_rt::ffi::FfiSymSite::new();
            let __addr = #addr;
            let mut __vals: Vec<zeo_rt::ffi::VaVal> = Vec::new();
            #(#pushes)*
            let __ffi_ret = #invoke;
            #cb_check
            #post
        }
    }
}

/// A variadic `attach_function` call: resolve the C symbol's address, marshal
/// the fixed arguments plus the runtime `*rest` (type, value) pairs, and call
/// through libffi's variadic CIF.
fn emit_ffi_variadic(
    cx: &Ctx,
    call: &crate::hir::FfiCall,
    rest_id: crate::hir::NodeId,
    link: &TokenStream,
) -> TokenStream {
    let sym = quote::format_ident!("{}", call.symbol);
    let fixed_count = call.args.len();
    let fixed_pushes = call.args.iter().map(|(arg_id, ty)| {
        let kind = ffi_kind_tokens(ty);
        let val = emit_expr(cx, *arg_id);
        quote! { __ffi_vals.push(zeo_rt::ffi::va_fixed(#kind, &(#val))?); }
    });
    let rest_expr = emit_expr(cx, rest_id);
    let ret_kind = ffi_kind_tokens(&call.ret);
    quote! {
        {
            #link
            extern "C" {
                fn #sym();
            }
            let __ffi_fn: unsafe extern "C" fn() = #sym;
            let __ffi_addr = __ffi_fn as *const ::std::os::raw::c_void;
            let mut __ffi_vals: ::std::vec::Vec<zeo_rt::ffi::VaVal> = ::std::vec::Vec::new();
            #(#fixed_pushes)*
            __ffi_vals.extend(zeo_rt::ffi::va_parse_pairs(&(#rest_expr))?);
            unsafe { zeo_rt::ffi::call_variadic(__ffi_addr, #fixed_count, __ffi_vals, #ret_kind)? }
        }
    }
}

/// The `zeo_rt::ffi::FfiKind` variant for a scalar C type -- the runtime tag
/// codegen emits for variadic-argument marshaling and callback signatures.
/// `FfiKind` IS the shared `CScalar` (re-exported), so the variant name is
/// the scalar's own.
fn ffi_kind_tokens(ty: &crate::hir::FfiType) -> TokenStream {
    // A platform typedef's width is only knowable where the generated code
    // BUILDS (that is the whole point of the variant) -- so its kind is
    // computed there, from the target's own libc alias.
    if let crate::hir::FfiType::PlatformScalar(name) = ty {
        let id = quote::format_ident!("{}", name.as_str());
        return quote! {
            zeo_rt::ffi::platform_kind(
                ::core::mem::size_of::<zeo_rt::libc::#id>(),
                zeo_rt::libc::#id::MIN == 0,
            )
        };
    }
    // An inline array is a struct-layout field only (`as_ffi_layout` is the
    // sole producer), and `lower_attach_function` rejects a by-value struct
    // in every position that marshals through kinds (variadic, callback,
    // runtime lib) -- so a kind position always has a scalar.
    let scalar = ty
        .c_scalar()
        .expect("an inline array or by-value struct never reaches a kind position");
    let variant = quote::format_ident!("{}", scalar.name());
    quote! { zeo_rt::ffi::FfiKind::#variant }
}

/// The Rust type mirroring one C ABI type (LP64: `i32`==C `int`, `i64`==C
/// `long`, `u64`==`size_t`). `Void` is `()` -- only valid as a return.
fn ffi_c_type(ty: &crate::hir::FfiType) -> TokenStream {
    use crate::hir::FfiType::*;
    use zeo_abi::ffi::CScalar as S;
    match ty {
        // An enum's underlying C type is `int`, the gem's default -- true
        // whether or not the members were knowable at compile time.
        Enum(_) | EnumSlot(_) => quote! { ::std::os::raw::c_int },
        // A callback is a C function pointer -- passed as an opaque address
        // (`*const`, matching what `CallbackHandle::code_ptr` hands over).
        Callback(..) => quote! { *const ::std::os::raw::c_void },
        // The TARGET's own libc alias -- rustc supplies the real width where
        // the generated program builds. See `FfiType::PlatformScalar`.
        PlatformScalar(name) => {
            let id = quote::format_ident!("{}", name.as_str());
            quote! { zeo_rt::libc::#id }
        }
        // `:strptr` returns a `char *`; the wrap reads it twice (string AND
        // pointer). Argument position is rejected at the declaration.
        StrPtr => quote! { *const ::std::os::raw::c_char },
        // Confined to a struct layout -- see `FfiType::Array`.
        Array(..) => unreachable!("an inline array type never reaches a call site"),
        // `emit_ffi_call` intercepts a by-value struct before asking for a
        // scalar C type -- it emits the `#[repr(C)]` mirror instead.
        Struct(_) => unreachable!("a by-value struct is emitted as its repr(C) mirror"),
        scalar => match scalar.c_scalar().expect("matched the scalar arms") {
            S::Void => quote! { () },
            S::Bool => quote! { bool },
            S::Str => quote! { *const ::std::os::raw::c_char },
            S::Pointer => quote! { *mut ::std::os::raw::c_void },
            s => {
                let t = quote::format_ident!("{}", s.name().to_ascii_lowercase());
                quote! { #t }
            }
        },
    }
}

/// The `&[(name, value)]` member table literal for an enum's inline marshaling.
fn ffi_enum_members(members: &[(String, i64)]) -> TokenStream {
    let pairs = members.iter().map(|(name, val)| {
        let name = proc_macro2::Literal::string(name);
        let val = proc_macro2::Literal::i64_suffixed(*val);
        quote! { (#name, #val) }
    });
    quote! { &[ #(#pairs),* ] }
}

/// `let __ffi_argN: <cty> = <marshal the RubyValue `val`>;` -- a `:string` also
/// binds a `CString` owner that outlives the call (kept in the block scope).
fn ffi_marshal_in(
    ty: &crate::hir::FfiType,
    pname: &proc_macro2::Ident,
    val: TokenStream,
) -> TokenStream {
    use crate::hir::FfiType::*;
    let cty = ffi_c_type(ty);
    match ty {
        Int(_) | Uint(_) => quote! { let #pname: #cty = zeo_rt::ffi::to_i64(&#val)? as #cty; },
        Float(_) => quote! { let #pname: #cty = zeo_rt::ffi::to_f64(&#val)? as #cty; },
        Bool => quote! { let #pname: bool = zeo_rt::ffi::to_bool(&#val); },
        Str => {
            let owner = quote::format_ident!("{}_owner", pname);
            quote! {
                let #owner = zeo_rt::ffi::to_cstring(&#val)?;
                let #pname: #cty = #owner.as_ptr();
            }
        }
        Pointer => quote! { let #pname: #cty = zeo_rt::ffi::to_pointer(&#val)?; },
        Enum(members) => {
            let table = ffi_enum_members(members);
            quote! { let #pname: #cty = zeo_rt::ffi::enum_to_int(&#val, #table)? as #cty; }
        }
        // Same conversion, against the table the class body filled.
        EnumSlot(slot) => {
            let slot = proc_macro2::Literal::usize_unsuffixed(*slot);
            quote! { let #pname: #cty = zeo_rt::ffi::enum_to_int_slot(#slot, &#val)? as #cty; }
        }
        // Marshal a Ruby Proc into a libffi closure; its code pointer is the C
        // argument. The `_cb` handle (the live closure) is a block-local kept
        // alive across the call and dropped after it.
        Callback(arg_types, ret_type) => {
            let owner = quote::format_ident!("{}_cb", pname);
            let arg_kinds = arg_types.iter().map(ffi_kind_tokens);
            let ret_kind = ffi_kind_tokens(ret_type);
            quote! {
                let #owner = zeo_rt::ffi::make_callback(&#val, &[#(#arg_kinds),*], #ret_kind)?;
                let #pname: #cty = #owner.code_ptr();
            }
        }
        Void => quote! { compile_error!("`:void` is not a valid FFI argument type"); },
        // An integer under the target's own alias -- same shape as `Int`.
        PlatformScalar(_) => {
            quote! { let #pname: #cty = zeo_rt::ffi::to_i64(&#val)? as #cty; }
        }
        // Return-only; `lower_attach_function` rejects it as an argument.
        StrPtr => unreachable!("`:strptr` is rejected in argument position"),
        // Confined to a struct layout -- see `FfiType::Array`.
        Array(..) => unreachable!("an inline array type never reaches a call site"),
        // Intercepted by `emit_ffi_call` before marshaling -- see above.
        Struct(_) => unreachable!("a by-value struct is marshaled by its repr(C) mirror"),
        // Degraded to `Pointer` (plus the return's class wrap) during
        // lowering -- see `FfiType::StructRef`.
        StructRef(_) => unreachable!("a struct reference lowers to a pointer"),
    }
}

/// Wrap the C return value `__ffi_ret` back into a `RubyValue`.
fn ffi_wrap_ret(ty: &crate::hir::FfiType) -> TokenStream {
    use crate::hir::FfiType::*;
    match ty {
        Void => quote! { { let () = __ffi_ret; zeo_rt::RubyValue::Nil } },
        Int(_) | Uint(_) => quote! { zeo_rt::ffi::from_i64(__ffi_ret as i64) },
        Float(_) => quote! { zeo_rt::ffi::from_f64(__ffi_ret as f64) },
        Bool => quote! { zeo_rt::ffi::from_bool(__ffi_ret) },
        Str => quote! { unsafe { zeo_rt::ffi::from_cstr(__ffi_ret) } },
        Pointer => quote! { zeo_rt::ffi::from_pointer(__ffi_ret) },
        Enum(members) => {
            let table = ffi_enum_members(members);
            quote! { zeo_rt::ffi::int_to_enum(__ffi_ret as i64, #table) }
        }
        EnumSlot(slot) => {
            let slot = proc_macro2::Literal::usize_unsuffixed(*slot);
            quote! { zeo_rt::ffi::int_to_enum_slot(#slot, __ffi_ret as i64) }
        }
        Callback(..) => quote! { compile_error!("an FFI callback is not a valid return type") },
        // An integer whatever width the target gave the alias.
        PlatformScalar(_) => quote! { zeo_rt::ffi::from_i64(__ffi_ret as i64) },
        // The gem's `[String, Pointer]` pair: the decoded string AND the raw
        // pointer, so the caller can still free it.
        StrPtr => quote! {
            {
                let __s = unsafe { zeo_rt::ffi::from_cstr(__ffi_ret) };
                zeo_rt::RubyValue::Array(zeo_rt::array_new(vec![
                    __s,
                    zeo_rt::ffi::from_pointer(__ffi_ret as *const ::std::os::raw::c_void),
                ]))
            }
        },
        // Confined to a struct layout -- see `FfiType::Array`.
        Array(..) => unreachable!("an inline array type never reaches a call site"),
        // Intercepted by `emit_ffi_call` before wrapping -- see above.
        Struct(_) => unreachable!("a by-value struct return is wrapped by its repr(C) mirror"),
        // Degraded to `Pointer` (plus the class wrap) during lowering --
        // see `FfiType::StructRef`.
        StructRef(_) => unreachable!("a struct reference lowers to a pointer"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every constant a read can name has to come out of `const_slot` as a
    /// legal Rust identifier tail -- `format_ident!` panics on anything else,
    /// and a panicking compiler says nothing about the ruby that caused it.
    #[test]
    fn a_const_slot_is_always_an_identifier() {
        for name in [
            "VERSION",
            "Dry::Types::Result::Failure",
            "::Rooted",
            "With Space",
            "Ω",
        ] {
            let slot = const_slot(name);
            assert!(
                !slot.is_empty() && slot.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{name:?} -> {slot:?}"
            );
            // Proves it: this is the call that used to panic.
            let _ = quote::format_ident!("__CONST_1_{}", slot);
        }
        assert_eq!(const_slot("Dry::Types"), "DRY__TYPES");
    }
}
