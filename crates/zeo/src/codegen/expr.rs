//! Emits a `TokenStream` for every HIR node except `LocalWrite` used as a
//! plain body statement (see `stmt.rs`). Every fragment produced here
//! evaluates to a bare `zeo_rt::RubyValue` -- any fallible sub-call (a
//! method dispatch that returns `Result<RubyValue, Signal>`) already has `?`
//! applied internally, so callers can always splice an `emit_expr` result
//! wherever a plain `RubyValue`-typed expression is expected, with no
//! wrapping of their own.

use quote::quote;

use super::Ctx;
use super::call::emit_call;
use super::collections::{
    emit_array_lit, emit_flip_flop, emit_hash_lit, emit_range_lit, emit_string_lit,
};
use super::ident::safe_ident;
use super::loops::{emit_break, emit_for, emit_loop, emit_next, emit_redo, emit_while};
use crate::compiler::ClassId;
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
    {
        if name == var {
            return *ty;
        }
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
        MATCH_DATA_CLASS, MODULE_CLASS, MUTEX_CLASS, PROC_CLASS, QUEUE_CLASS, RACTOR_CLASS,
        RANGE_CLASS, REGEXP_CLASS, STRING_CLASS, SYMBOL_CLASS, THREAD_CLASS,
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
        TyKind::Queue => Some(QUEUE_CLASS),
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
        return quote! { zeo_rt::Symbol::intern(#s) };
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
        return quote! {
            if __blk.is_some() {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("yield".to_string()))
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
    // its public surface. `New`/`SuperCall`/etc. keep the static "method".
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
        let name = name.as_str();
        // `defined?` consults `respond_to_missing?` too (a method_missing
        // method answers "method"), but SWALLOWS a raise from it (returning
        // nil) -- `unwrap_or(false)`, not `?`, keeps that exception-safety.
        return quote! {
            if zeo_rt::responds_to_or_missing(&#recv, zeo_rt::Symbol::intern(#name), #include_all).unwrap_or(false) {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("method".to_string()))
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
    if let HirNode::QualifiedConstRead(scope, name) = &cx.compiler.hir[id] {
        if super::constfold::const_form_resolves(cx, id) != Some(true) {
            if let Some(scope_id) = cx.resolve_class(scope) {
                let scope_id = scope_id.0;
                let name = name.as_str();
                return quote! {
                    if zeo_rt::const_defined_in(zeo_rt::ClassId(#scope_id), #name) {
                        zeo_rt::RubyValue::Str(zeo_rt::string_new("constant".to_string()))
                    } else {
                        zeo_rt::RubyValue::Nil
                    }
                };
            }
        }
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
        return quote! {
            if zeo_rt::ivar_defined(&#recv, #name) {
                zeo_rt::RubyValue::Str(zeo_rt::string_new("instance-variable".to_string()))
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
        HirNode::ClassVarRead(_) => Some("class variable"),
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
        | HirNode::MultiWrite { .. } => Some("assignment"),
        // Handled by the early returns at the top of this function.
        HirNode::Call { .. }
        | HirNode::Yield(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::ArrayLit(_)
        | HirNode::HashLit(_)
        | HirNode::IvarRead(_) => {
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
        | HirNode::Undef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ModuleFunction(_) => None,
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
                        // truthy".
                        quote! {
                            (#v_expr).as_array_unchecked().lock().iter()
                                .any(|__c| __c.truthy())
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
                body_cx.runtime_super_params = Some(std::rc::Rc::new(params.clone()));
            }
            super::call::emit_lambda_value(&body_cx, params, body, *method_body)
        }
        HirNode::SymbolLit(s) => {
            quote! { zeo_rt::RubyValue::Symbol(zeo_rt::Symbol::intern(#s)) }
        }
        HirNode::NilLit => quote! { zeo_rt::RubyValue::Nil },
        HirNode::BoolLit(b) => quote! { zeo_rt::RubyValue::Bool(#b) },
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
                return quote! { zeo_rt::ivar_get_dyn(&#slf, #key) };
            }
            // `self` is a CLASS object (a class-method body, or a class body
            // itself): `@x` is that class object's own ivar, which lives in
            // its own runtime table. A direct table hit, no `ivar_get_dyn`
            // match to walk.
            if let Some(cid) = cx.class_self {
                let id = cid.0;
                let key = ident.to_string();
                return quote! { zeo_rt::class_ivar_get(#id, #key) };
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
            // The `MutexGuard` from `.lock()` is bound to an explicit `__g`
            // local, INSIDE its own block, rather than written as a single
            // bare `#slf.#ident.lock().clone()` expression: Rust's
            // temporary-lifetime rule keeps an UNNAMED `.lock()` guard alive
            // until the end of the ENCLOSING STATEMENT, not just this one
            // sub-expression, so referencing the SAME ivar TWICE within one
            // statement (any expression reading an ivar more than once, e.g.
            // `@x * @x`, not just the already-guarded read-modify-WRITE
            // shape Part 9 audited) silently deadlocks a non-reentrant
            // `parking_lot::Mutex` -- confirmed via a minimal, standalone
            // repro BEFORE this fix, and confirmed this exact `{ let __g =
            // ...; __g.clone() }` shape (not just wrapping in a bare `{ }`
            // block, which does NOT change the guard's drop timing -- also
            // confirmed empirically) resolves it.
            quote! { { let __g = #slf.#ident.lock(); __g.clone() } }
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
            // `parking_lot::Mutex` (non-reentrant, Part 9), a `#v` expression
            // that itself reads this same ivar would otherwise call `.lock()`
            // while the write guard below is already held, hanging forever
            // instead of RefCell's old clean "already borrowed" panic.
            let write = emit_ivar_write_stmt(cx, name, quote! { __v.clone() });
            quote! { { let __v = #v; #write __v } }
        }
        HirNode::ClassVarRead(name) => {
            let owner = cvar_owner_id(cx, name);
            quote! { zeo_rt::cvar_get(#owner, #name) }
        }
        HirNode::ClassVarWrite(name, value) => {
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: cvar storage is likewise always
            // `RubyValue` (`zeo_rt::cvar_set`'s own signature), never an
            // unboxed concrete-class slot.
            let v = box_if_object_typed(cx, *value, v);
            let write = emit_cvar_write_stmt(cx, name, quote! { __v.clone() });
            quote! { { let __v = #v; #write __v } }
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
                quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) }
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
            let id = qualified_const_class(cx, scope, name)
                .expect("guarded above")
                .0;
            quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)) }
        }
        HirNode::New {
            class_name,
            args,
            kwargs,
            block,
        } => super::call::emit_new(cx, class_name, args, kwargs, *block),
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
        HirNode::Break(v) => emit_break(cx, *v),
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
            cx.compiler.hir.vcall_nodes.contains(&id),
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
            quote! { { let __v = #v; zeo_rt::global_assign(#bx, #name, __v.clone())?; __v } }
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
                let err = uninitialized_constant_error(cx, scope.as_deref().unwrap_or(name));
                return quote! { return Err(zeo_rt::Signal::Raise(#err)) };
            }
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: constant storage is likewise always
            // `RubyValue` (`zeo_rt::const_set`'s own signature).
            let v = box_if_object_typed(cx, *value, v);
            let write = emit_const_write_stmt(cx, scope.as_deref(), name, quote! { __v.clone() });
            quote! { { let __v = #v; #write __v } }
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
            let mut overlay = std::collections::HashMap::new();
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
            let b = super::stmt::emit_body(cx, body, false);
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
            let invoke = quote! {
                match __blk.as_ref() {
                    Some(__b) => __b.as_proc_unchecked(),
                    None => return Err(zeo_rt::raise_no_block_yield()),
                }
            };
            // No splat: the arguments are a fixed-length list, so they go
            // straight into a borrowed slice literal with no Vec allocated.
            if !args.iter().any(|a| matches!(a, ArrayElem::Splat(_))) {
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
            let pushes = args.iter().map(|a| match a {
                ArrayElem::Single(n) => {
                    let e = box_if_object_typed(cx, *n, emit_expr(cx, *n));
                    quote! { __args.push(#e); }
                }
                ArrayElem::Splat(n) => {
                    let e = emit_expr(cx, *n);
                    quote! { __args.extend((#e).as_array_unchecked().lock().iter().cloned()); }
                }
            });
            quote! {
                {
                    let mut __args: Vec<zeo_rt::RubyValue> = Vec::new();
                    #(#pushes)*
                    (#invoke).call(&__args)?
                }
            }
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
            ..
        } => {
            // A `def` in value position installs on: the block's DYNAMIC self
            // when inside one (`Class.new { def g; end }`), otherwise the
            // ENCLOSING class -- Object at top level, so `p(def foo; end)`
            // defines foo as a private method of Object and returns :foo,
            // matching CRuby (a plain object doesn't respond to define_method).
            let self_val = if cx.self_is_dynamic {
                super::call::boxed_implicit_self(cx)
                    .expect("a dynamic-self `def` in expression position must have a boxed self")
            } else {
                let cid = cx
                    .class_self
                    .or(cx.current_class)
                    .unwrap_or(crate::compiler::OBJECT_CLASS)
                    .0;
                quote! { zeo_rt::RubyValue::Class(zeo_rt::ClassId(#cid)) }
            };
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
            body_cx.runtime_super_params = Some(std::rc::Rc::new(params.clone()));
            let proc = super::call::emit_proc_or_lambda_value(&body_cx, params, body, true, true);
            if *is_class_method {
                // `def self.x` always installs a singleton method on `self`.
                quote! {
                    zeo_rt::send_value(
                        &#self_val,
                        zeo_rt::Symbol::intern("define_singleton_method"),
                        &[zeo_rt::RubyValue::Symbol(zeo_rt::Symbol::intern(#name)), #proc],
                        None,
                    )?
                }
            } else if *is_def {
                // A real `def` installs on the runtime default definee: an
                // instance method on a Class/Module self, a singleton method on
                // any other (`instance_exec { def m; end }`).
                quote! {
                    zeo_rt::define_in_default_definee(
                        &#self_val,
                        zeo_rt::Symbol::intern(#name),
                        #proc,
                    )?
                }
            } else {
                // A literal `define_method(:m){...}` call: an ordinary
                // Module#define_method dispatch, which raises NoMethodError when
                // self isn't a Module/Class (`instance_exec { define_method... }`).
                quote! {
                    zeo_rt::send_value(
                        &#self_val,
                        zeo_rt::Symbol::intern("define_method"),
                        &[zeo_rt::RubyValue::Symbol(zeo_rt::Symbol::intern(#name)), #proc],
                        None,
                    )?
                }
            }
        }
        HirNode::Program(_)
        | HirNode::ClassDef { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::Undef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ModuleFunction(_) => {
            let loc = crate::codegen::source_location(cx.compiler, id);
            crate::codegen::unsupported(format!(
                "a definition-level construct used as a VALUE isn't supported yet (zeo limitation): {loc:?}"
            ))
        }
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
/// call-shape dispatch (translated to `emit_new`-style construction -- see
/// the plan's Part 6): `raise SomeError` (bare class ref) defaults the
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
        // `raise SomeError` / `raise SomeError, "msg"` constructs via
        // `SomeError.new(...)`, running any custom `initialize` (defaults and
        // `super` chain included), exactly like CRuby's `exc.exception` path.
        let args = match explicit_msg {
            Some(msg_id) => vec![emit_expr(cx, msg_id)],
            None => vec![],
        };
        return emit_boxed_new(cx, class_name, args);
    }
    if let Some(msg_id) = explicit_msg {
        // `raise <expr>, message` where `<expr>` didn't resolve to a class
        // above. A constant-SHAPED operand that failed to resolve is an
        // undefined constant: CRuby evaluates it -- and raises `uninitialized
        // constant` -- before the message is ever consulted, so lowering it as
        // an ordinary const read yields the correctly-scoped runtime NameError
        // (and compiles cleanly in a dead/rescued branch). A genuinely
        // COMPUTED class operand (a variable, a call) stays unsupported.
        if const_path_of(cx, node).is_some() {
            return emit_expr(cx, node);
        }
        // A genuinely COMPUTED class/exception operand (`raise klass, msg`):
        // coerce at runtime like `Kernel#raise` (`klass.exception(msg)`).
        let class_expr = emit_expr(cx, node);
        let msg_expr = emit_expr(cx, msg_id);
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
                quote! { zeo_rt::coerce_raise_arg(#boxed) }
            }
        }
        // A Poly (or non-exception) operand is coerced at runtime: an
        // Exception raises itself, a String becomes a `RuntimeError`, and
        // everything else is CRuby's `TypeError: exception class/object
        // expected`.
        _ => {
            let expr = emit_expr(cx, node);
            quote! { zeo_rt::coerce_raise_arg(#expr) }
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
        let ctor = super::call::emit_new_with_arg_tokens(cx, class_name, arg_exprs);
        return quote! { zeo_rt::stamp_backtrace(#ctor) };
    }
    let class_ident = super::ident::class_ident(cx.compiler, cid);
    let ctor = super::call::emit_new_with_arg_tokens(cx, class_name, arg_exprs);
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
/// `LocalWrite` arm) to `RubyValue`. Unlike `box_if_object_typed`, this keys
/// on the TARGET LOCAL's own storage, not the write's RHS type: a `nil | Foo`
/// union local stores as a plain `RubyValue` slot (`LocalStorage::Hoisted`)
/// even when THIS write's RHS is a concrete `Foo` -- so its read-back is
/// already a `RubyValue` and must NOT be re-boxed. Only a `Shadowed`
/// (`TyKind::Object`) local reads back as an unboxed `Arc<Concrete>` needing
/// the `RubyValue::Object` wrap.
pub(super) fn box_tail_local_write(cx: &Ctx, name: &str, value: TokenStream) -> TokenStream {
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
    // owner a bare constant resolves to (see `const_owner_id`, and the
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
    cx.compiler
        .class(defining)
        .cvar_owners
        .get(name)
        .copied()
        .unwrap_or(defining)
        .0
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
        return quote! { zeo_rt::ivar_set_dyn(&#slf, #key, #value)?; };
    }
    // `self` is a CLASS object -- see the matching arm in `IvarRead`. The
    // frozen-class guard lives inside `class_ivar_set` itself (a frozen
    // class raises `can't modify frozen Class: Foo`), hence the `?`.
    if let Some(cid) = cx.class_self {
        let id = cid.0;
        let key = ident.to_string();
        return quote! { zeo_rt::class_ivar_set(#id, #key, #value)?; };
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
    // The message interpolates the receiver's real `#<Class:0xaddr @ivar=...>`
    // inspect (`default_object_repr`), built ONLY on the raise path (the
    // `Clone::clone(&#slf)` is a cheap `Arc` bump, never taken on a normal write).
    // `RubyObject::is_frozen` is UFCS-qualified: generated programs never
    // `use` the trait by name. The one atomic load this adds to every ivar
    // write (including inside `initialize`, where it's always false) is
    // negligible.
    let current = cx
        .current_class
        .expect("ivar write outside a class context");
    let class_name = cx.compiler.class(current).name.clone();
    let prefix = format!("can't modify frozen {class_name}: ");
    // Boxed via the concrete `new_handle` (not a bare
    // `RubyValue::Object(Clone::clone(..))`): the constructor's expected
    // `Arc<dyn RubyObject>` would drive UFCS-clone inference BACKWARDS into
    // the generic and defeat the unsize coercion; `new_handle`'s concrete
    // `Arc<Self>` parameter anchors it.
    let class_ident = super::ident::class_ident(cx.compiler, current);
    let frozen_error = emit_boxed_new(
        cx,
        "FrozenError",
        vec![quote! {
            zeo_rt::RubyValue::Str(zeo_rt::string_new(format!(
                "{}{}",
                #prefix,
                zeo_rt::RubyValue::Object(#class_ident::new_handle(Clone::clone(&#slf)))
                    .inspect_string()
            )))
        }],
    );
    quote! {
        if zeo_rt::RubyObject::is_frozen(&*#slf) {
            return Err(zeo_rt::Signal::Raise(#frozen_error));
        }
        *#slf.#ident.lock() = #value;
    }
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
pub(super) fn const_owner_id(cx: &Ctx, scope: Option<&str>, name: &str) -> u32 {
    const_owner_id_opt(cx, scope, name).unwrap_or_else(|| {
        // Reached only when the caller guarantees a resolvable scope (the sole
        // caller passes an already-registered class's own fq name). An eager
        // or dead-branch reference must use `const_owner_id_opt` and raise a
        // runtime `NameError` instead -- see `emit_const_read`/
        // `emit_const_write_stmt`.
        panic!(
            "internal error: unknown class/module `{}` in const_owner_id",
            scope.unwrap_or(name)
        )
    })
}

/// Fallible companion to [`const_owner_id`]: returns `None` when an explicit
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
        None => cx.defining_class.unwrap_or_else(|| {
            if cx.box_id != 0 {
                cx.compiler
                    .box_surrogate(cx.box_id)
                    .expect("analyze registers a surrogate for every allocated box")
            } else {
                crate::compiler::OBJECT_CLASS
            }
        }),
    };
    Some(
        cx.compiler
            .class(owner_class)
            .const_owners
            .get(name)
            .copied()
            .unwrap_or(owner_class)
            .0,
    )
}

/// A constant READ -- `scope: None` for a bare `NAME` (see `const_owner_id`'s
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
        .or_else(|| {
            (scope == "Object")
                .then(|| cx.resolve_class(name))
                .flatten()
        })
}

pub(super) fn emit_const_read(cx: &Ctx, scope: Option<&str>, name: &str) -> TokenStream {
    // An explicit `Scope::NAME` whose scope class isn't registered is a
    // `NameError` on the missing SCOPE (`uninitialized constant OpenSSL`),
    // deferred to runtime so a dead/guarded branch still compiles.
    let Some(owner) = const_owner_id_opt(cx, scope, name) else {
        // A single-segment scope that isn't a compile-time class may still be a
        // RUNTIME constant holding a class (`Line = Struct.new(...)` /
        // `Data.define`), so resolve the path at runtime: read the scope
        // constant, then the leaf on the class it names. `uninitialized
        // constant Line::FLAGS` (leaf missing) vs `uninitialized constant Line`
        // (scope missing) then matches CRuby.
        if let Some(s) = scope.filter(|s| !s.contains("::")) {
            let scope_owner = const_owner_id_opt(cx, None, s).unwrap_or(0);
            let qualified = format!("{s}::{name}");
            // A missing SCOPE head is reported the way a missing bare constant
            // is -- qualified by the enclosing cref (`Outer::Wrap::Deep` for a
            // `Deep::Missing` written inside `module Outer; module Wrap`), which
            // is what `qualified` below does for the resolved case.
            let missing_scope = match cx.defining_class {
                Some(d) => format!("{}::{s}", cx.compiler.fq_name(d)),
                None => s.to_string(),
            };
            return quote! {
                match zeo_rt::const_get(#scope_owner, #s) {
                    Some(zeo_rt::RubyValue::Class(__cid)) => match zeo_rt::const_get_scoped(__cid.0, #name) {
                        Some(__v) => __v,
                        None => return Err(zeo_rt::Signal::Raise(zeo_rt::stamp_backtrace(zeo_rt::make_name_error(
                            format!("uninitialized constant {}", #qualified),
                            #name,
                            zeo_rt::RubyValue::Class(__cid),
                        )))),
                    },
                    Some(__other) => return Err(zeo_rt::raise_error(
                        "TypeError",
                        format!("{} is not a class/module", __other.inspect_string()),
                    )),
                    None => return Err(zeo_rt::Signal::Raise(zeo_rt::stamp_backtrace(zeo_rt::make_name_error(
                        format!("uninitialized constant {}", #missing_scope),
                        #s,
                        zeo_rt::RubyValue::Nil,
                    )))),
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
    // The `NameError` message mirrors real Ruby's: an explicit path prints
    // as written (`uninitialized constant Store::MISSING`); a bare miss
    // inside a class/module body is qualified by the cref head's
    // fully-qualified name (`uninitialized constant Store::Cart::DEFAULT`
    // -- oracle-verified); a bare top-level miss stays bare.
    let qualified = match (scope, cx.defining_class) {
        (Some(s), _) => format!("{s}::{name}"),
        (None, Some(d)) => format!("{}::{name}", cx.compiler.fq_name(d)),
        (None, None) => name.to_string(),
    };
    // The raised `NameError` carries `#name` (the missing leaf as a Symbol) and
    // `#receiver` (the class the lookup ran against -- `Object` at top level, the
    // enclosing module for a nested miss), matching CRuby.
    // Object is where Ruby's bare-name lookup ends, and the only constants the
    // compile-time owner map can't already have placed are the ones the runtime
    // installs (`RUBY_RELEASE_DATE` and friends) -- which is exactly why a bare
    // read inside a nested module needs this tail. An explicit `Scope::NAME`
    // gets no such fallback; CRuby doesn't give it one either.
    let lookup = if scope.is_none() && owner != crate::compiler::OBJECT_CLASS.0 {
        quote! { zeo_rt::const_get(#owner, #name).or_else(|| zeo_rt::const_get(0u32, #name)) }
    } else {
        quote! { zeo_rt::const_get(#owner, #name) }
    };
    quote! {
        match #lookup {
            Some(__v) => __v,
            None => return Err(zeo_rt::Signal::Raise(zeo_rt::stamp_backtrace(zeo_rt::make_name_error(
                format!("uninitialized constant {}", #qualified),
                #name,
                zeo_rt::RubyValue::Class(zeo_rt::ClassId(#owner)),
            )))),
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
    quote! { zeo_rt::const_set(#owner, #name, #value); }
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
        Some(lib) => quote! { #[link(name = #lib)] },
        None => quote! {},
    };
    // A variadic function's argument list is shaped at runtime, so it goes
    // through libffi rather than a fixed `extern "C"` signature.
    if let Some(rest_id) = call.variadic {
        return emit_ffi_variadic(cx, call, rest_id, &link);
    }

    let sym = quote::format_ident!("{}", call.symbol);
    let mut extern_params = Vec::new();
    let mut bindings = Vec::new();
    let mut call_idents = Vec::new();
    let mut has_callback = false;
    for (i, (arg_id, ty)) in call.args.iter().enumerate() {
        let pname = quote::format_ident!("__ffi_arg{}", i);
        let cty = ffi_c_type(ty);
        extern_params.push(quote! { #pname: #cty });
        let val = emit_expr(cx, *arg_id);
        bindings.push(ffi_marshal_in(ty, &pname, val));
        call_idents.push(quote! { #pname });
        has_callback |= matches!(ty, crate::hir::FfiType::Callback(..));
    }
    let ret_cty = ffi_c_type(&call.ret);
    let wrap = ffi_wrap_ret(&call.ret);
    // An exception raised inside a callback can't unwind through C; it was
    // stashed and is re-raised here, after the C function returns.
    let cb_check = if has_callback {
        quote! { zeo_rt::ffi::take_callback_error()?; }
    } else {
        quote! {}
    };
    quote! {
        {
            #link
            extern "C" {
                fn #sym(#(#extern_params),*) -> #ret_cty;
            }
            #(#bindings)*
            let __ffi_ret = unsafe { #sym(#(#call_idents),*) };
            #cb_check
            #wrap
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
fn ffi_kind_tokens(ty: &crate::hir::FfiType) -> TokenStream {
    use crate::hir::FfiType::*;
    let variant = match ty {
        Void => quote! { Void },
        Int(8) => quote! { I8 },
        Int(16) => quote! { I16 },
        Int(64) => quote! { I64 },
        Uint(8) => quote! { U8 },
        Uint(16) => quote! { U16 },
        Uint(32) => quote! { U32 },
        Uint(64) => quote! { U64 },
        Float(32) => quote! { F32 },
        Float(64) => quote! { F64 },
        Bool => quote! { Bool },
        Str => quote! { Str },
        Pointer | Callback(..) => quote! { Pointer },
        // Enums are C `int`; `Int(32)` and any residual width default the same.
        Enum(_) | Int(_) => quote! { I32 },
        Uint(_) | Float(_) => quote! { I32 },
    };
    quote! { zeo_rt::ffi::FfiKind::#variant }
}

/// The Rust type mirroring one C ABI type (LP64: `i32`==C `int`, `i64`==C
/// `long`, `u64`==`size_t`). `Void` is `()` -- only valid as a return.
fn ffi_c_type(ty: &crate::hir::FfiType) -> TokenStream {
    use crate::hir::FfiType::*;
    match ty {
        Void => quote! { () },
        Int(w) => {
            let t = quote::format_ident!("i{}", w);
            quote! { #t }
        }
        Uint(w) => {
            let t = quote::format_ident!("u{}", w);
            quote! { #t }
        }
        Float(w) => {
            let t = quote::format_ident!("f{}", w);
            quote! { #t }
        }
        Bool => quote! { bool },
        Str => quote! { *const ::std::os::raw::c_char },
        Pointer => quote! { *mut ::std::os::raw::c_void },
        // An enum's underlying C type is `int`, the gem's default.
        Enum(_) => quote! { ::std::os::raw::c_int },
        // A callback is a C function pointer -- passed as an opaque address.
        Callback(..) => quote! { *const ::std::os::raw::c_void },
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
        Callback(..) => quote! { compile_error!("an FFI callback is not a valid return type") },
    }
}
