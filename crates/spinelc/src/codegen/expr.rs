//! Emits a `TokenStream` for every HIR node except `LocalWrite` used as a
//! plain body statement (see `stmt.rs`). Every fragment produced here
//! evaluates to a bare `spinel_rt::RubyValue` -- any fallible sub-call (a
//! method dispatch that returns `Result<RubyValue, Signal>`) already has `?`
//! applied internally, so callers can always splice an `emit_expr` result
//! wherever a plain `RubyValue`-typed expression is expected, with no
//! wrapping of their own.

use quote::quote;

use super::call::emit_call;
use super::collections::{emit_array_lit, emit_hash_lit, emit_range_lit, emit_string_lit};
use super::ident::safe_ident;
use super::loops::{emit_break, emit_for, emit_loop, emit_next, emit_redo, emit_while};
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
    // A `for`-loop's own index variable overrides the enclosing scope's flat
    // (position-insensitive) local-type map for reads of that exact name --
    // see `Ctx::for_var_override`'s docs for why the map alone can't express
    // this.
    if let (HirNode::LocalRead(name), Some((var, ty))) = (&cx.compiler.hir[id], &cx.for_var_override) {
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
        if let Some(cid) = cx.current_class {
            // Inside a reopened BUILTIN class's method (Phase 16.3), `self`
            // is the receiver VALUE (the free function's `__self:
            // RubyValue` parameter), so it types as the builtin's own
            // static kind (`TyKind::Str` inside `class String`) -- NOT
            // `Object(cid)`, whose repr is an unboxed `Arc<Concrete>` no
            // builtin has. That keeps `self.length`/`self + other` on the
            // same static fast paths any other builtin-typed receiver gets.
            if cx.compiler.class(cid).is_builtin {
                return builtin_self_ty(cid);
            }
            return TyKind::Object(cid);
        }
    }
    infer_type_with_locals(cx.compiler, cx.defining_class, &cx.local_types, id)
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

/// A Rust expression of type `spinel_rt::Symbol` (not `RubyValue`) -- used
/// for `send`'s second argument. A literal `:sym` skips the
/// box-then-immediately-unwrap round trip; anything else goes through the
/// runtime coercion (Symbol or String, real Ruby's rule -- `x.send("name")`
/// is legal; a non-name value raises CRuby's TypeError shape).
pub fn emit_symbol_expr(cx: &Ctx, id: NodeId) -> TokenStream {
    if let HirNode::SymbolLit(s) = &cx.compiler.hir[id] {
        return quote! { spinel_rt::Symbol::intern(#s) };
    }
    let e = emit_expr(cx, id);
    quote! { spinel_rt::method_name_symbol(&(#e))? }
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
        HirNode::ClassVarRead(_) => Some("class variable"),
        HirNode::ClassRef(_) | HirNode::QualifiedConstRead(..) => Some("constant"),
        // Same narrowing posture as `IvarRead` just above (classified
        // whenever it's syntactically a `$foo` read, without tracking
        // whether it was ever actually assigned -- a documented
        // approximation of real Ruby, which returns `nil` for a
        // never-assigned global specifically, unlike every OTHER
        // classification here).
        HirNode::GlobalRead(_) => Some("global-variable"),
        // Real Ruby: `defined?(self)` is always `"self"`, everywhere --
        // confirmed via `ruby -e 'puts defined?(self)'` -- unlike every other
        // classification here, this needs no further check at all.
        HirNode::SelfRef => Some("self"),
        HirNode::New { .. }
        | HirNode::Call { .. }
        | HirNode::SuperCall { .. }
        | HirNode::Eval(_)
        | HirNode::BlockGiven
        | HirNode::Raise(_) => Some("method"),
        HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::StringLit(_)
        | HirNode::RegexpLit(..)
        | HirNode::ArrayLit(_)
        | HirNode::HashLit(_)
        | HirNode::RangeLit { .. }
        | HirNode::And(..)
        | HirNode::Or(..)
        | HirNode::Defined(_)
        | HirNode::If { .. }
        | HirNode::CaseWhen { .. }
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::Begin { .. }
        | HirNode::LocalWrite(..)
        | HirNode::IvarWrite(..)
        | HirNode::ClassVarWrite(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::ConstReadOrNil(..)
        | HirNode::Seq(_)
        | HirNode::While { .. }
        | HirNode::Loop { .. }
        | HirNode::For { .. }
        | HirNode::MultiWrite { .. }
        | HirNode::Lambda { .. }
        // Narrower than real CRuby, which returns the distinct string
        // "yield" here (only when a block was actually given) -- a
        // documented approximation, same posture as this function's other
        // narrowings (see the module docs above).
        | HirNode::Yield(_) => Some("expression"),
        HirNode::Break(_) | HirNode::Next(_) | HirNode::Redo | HirNode::Return(_) | HirNode::Retry => None,
        HirNode::Block { .. }
        | HirNode::Program(_)
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. } => None,
    };
    match classification {
        Some(s) => quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(#s.to_string())) },
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
                // `rb_case_eq`, not plain `rb_eq`: a strict superset that
                // additionally gives `when /regex/` real `Regexp#===`
                // matching against a `String` subject (see
                // `RubyValue::rb_case_eq`'s docs) -- every other value shape
                // this desugar already supported behaves identically either
                // way.
                quote! { (#v_expr).rb_case_eq(&__subject) }
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
            // Boxed if Object-typed (Phase 16.1): `case w when Widget` --
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
        HirNode::IntegerLit(v) => quote! { spinel_rt::RubyValue::Int(#v) },
        // The bignum/rational/imaginary literals (Phase 17.1) -- digits
        // baked as array literals, assembled by the runtime at the use
        // site (no compile-time bigint dependency, no string parsing).
        HirNode::BigIntegerLit { negative, digits } => {
            quote! { spinel_rt::int_from_u32_digits(#negative, &[#(#digits),*]) }
        }
        HirNode::RationalLit { negative, num_digits, den_digits } => {
            quote! {
                spinel_rt::rational_from_digits(
                    #negative,
                    &[#(#num_digits),*],
                    &[#(#den_digits),*],
                )
            }
        }
        HirNode::ImaginaryLit(inner) => {
            let inner_expr = emit_expr(cx, *inner);
            quote! { spinel_rt::complex_from_literal(#inner_expr) }
        }
        HirNode::FloatLit(v) => quote! { spinel_rt::RubyValue::Float(#v) },
        HirNode::Lambda { params, body } => super::call::emit_lambda_value(cx, params, body),
        HirNode::SymbolLit(s) => {
            quote! { spinel_rt::RubyValue::Symbol(spinel_rt::Symbol::intern(#s)) }
        }
        HirNode::NilLit => quote! { spinel_rt::RubyValue::Nil },
        HirNode::BoolLit(b) => quote! { spinel_rt::RubyValue::Bool(#b) },
        HirNode::SelfRef => {
            // Only meaningful inside an ordinary instance method body (see
            // `hir::HirNode::SelfRef`'s docs) -- a class method/module
            // function has no `self: Arc<Self>` receiver at all in its Rust
            // signature, so referencing `self_ident` there would emit a
            // reference to a Rust binding that doesn't exist. Rejected here,
            // at codegen time, rather than letting `rustc` fail on the
            // GENERATED program with a confusing "cannot find value `self`".
            if cx.current_class.is_none() {
                panic!("`self` isn't supported inside a class method/module function body yet (spike scope, no first-class Class/Module value exists)");
            }
            // Unboxed `Arc<Concrete>` -- exactly what an Object-typed local
            // read (`LocalStorage::Shadowed`) already returns, and exactly
            // what a call receiver needs for Path 1 dispatch (`emit_call`'s
            // `recv_expr = emit_expr(cx, recv_id)`). `cx.self_ident` is the
            // capture-alias identifier while emitting a self-capturing
            // escaping block's own body (see `Ctx::self_ident`'s docs), the
            // literal `self` receiver parameter otherwise.
            let slf = &cx.self_ident;
            quote! { #slf.clone() }
        }
        HirNode::LocalRead(name) => super::hoisting::emit_local_read(cx, name),
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
        HirNode::ArrayLit(elems) => emit_array_lit(cx, elems),
        HirNode::HashLit(pairs) => emit_hash_lit(cx, pairs),
        HirNode::RangeLit {
            start,
            end,
            exclusive,
        } => emit_range_lit(cx, *start, *end, *exclusive),
        HirNode::StringLit(parts) => emit_string_lit(cx, parts),
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
            // `cx.self_ident` is ordinarily the literal `self`, but becomes a
            // fresh capture-alias identifier while emitting an escaping
            // block's own body that captured `self` (see `Ctx::in_proc`'s
            // docs -- `let self = ...;` is illegal Rust, so the closure
            // clones into a DIFFERENT name).
            let slf = &cx.self_ident;
            // The `MutexGuard` from `.lock()` is bound to an explicit `__g`
            // local, INSIDE its own block, rather than written as a single
            // bare `#slf.#ident.lock().clone()` expression -- found the hard
            // way (this session's own testing, via `@x * @x`): Rust's
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
            quote! { spinel_rt::cvar_get(#owner, #name) }
        }
        HirNode::ClassVarWrite(name, value) => {
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: cvar storage is likewise always
            // `RubyValue` (`spinel_rt::cvar_set`'s own signature), never an
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
        // VALUE is a first-class `RubyValue::Class` handle (Phase 16.1,
        // retiring the long-standing "no first-class Class/Module value"
        // rejection); one that names no class stays an ordinary
        // lexically-scoped constant READ.
        HirNode::ClassRef(name) => match cx.resolve_class(name) {
            Some(cid) => {
                let id = cid.0;
                quote! { spinel_rt::RubyValue::Class(spinel_rt::ClassId(#id)) }
            }
            None => emit_const_read(cx, None, name),
        },
        HirNode::QualifiedConstRead(scope, name) if cx.resolve_class(&format!("{scope}::{name}")).is_some() => {
            let id = cx
                .resolve_class(&format!("{scope}::{name}"))
                .expect("guarded above")
                .0;
            quote! { spinel_rt::RubyValue::Class(spinel_rt::ClassId(#id)) }
        }
        HirNode::New { class_name, args } => super::call::emit_new(cx, class_name, args),
        HirNode::SuperCall { args, zsuper } => super::call::emit_super_inline(cx, args, *zsuper),
        HirNode::While { cond, body, negate } => emit_while(cx, *cond, body, *negate),
        HirNode::Loop { body } => emit_loop(cx, body),
        HirNode::For { target, iterable, body } => emit_for(cx, target, *iterable, body),
        HirNode::Break(v) => emit_break(cx, *v),
        HirNode::Next(v) => emit_next(cx, *v),
        HirNode::Redo => emit_redo(cx),
        HirNode::MultiWrite { targets, value } => {
            // Sub-expression fallback (rare) -- see `stmt.rs` for the
            // primary, statement-position case, which is what makes the
            // assigned locals visible to LATER statements. Nested in its own
            // block here, so that visibility doesn't matter; yields `nil`,
            // the same simplification `LocalWrite`'s own sub-expression
            // fallback already makes above (real Ruby returns the RHS array
            // here), not a new gap this introduces.
            let write = super::loops::emit_multi_write(cx, targets, *value);
            quote! { { #write spinel_rt::RubyValue::Nil } }
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            kwargs_splat,
            block,
            block_arg,
            safe,
        } => emit_call(cx, *receiver, name, args, kwargs, *kwargs_splat, *block, *block_arg, *safe),
        HirNode::Block { .. } => {
            panic!("a Block should only be reached via the Call that invokes it")
        }
        HirNode::GlobalRead(name) => {
            // Globals are per-box tables (Phase 18) -- the statement's own
            // defining box picks the table, no fallback layer (the CRuby
            // box model, verified in the plan's research contract).
            let bx = cx.box_id;
            quote! { spinel_rt::global_get(#bx, #name) }
        }
        HirNode::GlobalWrite(name, value) => {
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: global storage is likewise always
            // `RubyValue` (`spinel_rt::global_set`'s own signature).
            let v = box_if_object_typed(cx, *value, v);
            let bx = cx.box_id;
            quote! { { let __v = #v; spinel_rt::global_set(#bx, #name, __v.clone()); __v } }
        }
        HirNode::QualifiedConstRead(scope, name) => emit_const_read(cx, Some(scope), name),
        HirNode::ConstReadOrNil(scope, name) => {
            let owner = const_owner_id(cx, scope.as_deref(), name);
            quote! { spinel_rt::const_get(#owner, #name).unwrap_or(spinel_rt::RubyValue::Nil) }
        }
        HirNode::ConstWrite { scope, name, value } => {
            let v = emit_expr(cx, *value);
            // See `IvarWrite`'s docs: constant storage is likewise always
            // `RubyValue` (`spinel_rt::const_set`'s own signature).
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
        HirNode::Seq(body) => {
            let mut overlay = std::collections::HashMap::new();
            for &n in body {
                if let HirNode::LocalWrite(name, value) = &cx.compiler.hir[n] {
                    overlay.insert(name.clone(), infer(cx, *value));
                }
            }
            let narrowed = cx.with_narrowed_locals(overlay);
            let b = super::stmt::emit_body(&narrowed, body, false);
            quote! { { #b } }
        }
        HirNode::Eval(body) => {
            let b = super::stmt::emit_body(cx, body, false);
            quote! { { #b } }
        }
        HirNode::Return(v) => {
            let value = match v {
                Some(id) => emit_expr(cx, *id),
                None => quote! { spinel_rt::RubyValue::Nil },
            };
            if cx.in_real_proc {
                // Inside a real escaping `Proc`'s own body, a literal Rust
                // `return` would only return from the CLOSURE, not the
                // lexically enclosing method -- wrong (real Ruby: `return`
                // inside a block always exits the enclosing method). Raise
                // `Signal::Return` instead, caught at the enclosing method's
                // own boundary (see `codegen::mod`'s per-method wrapping).
                quote! { return Err(spinel_rt::Signal::Return(#value)) }
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
            // Panics with a clear "no block given" message otherwise,
            // mirroring real Ruby's `LocalJumpError`.
            let arg_exprs = args.iter().map(|&a| emit_expr(cx, a));
            quote! {
                (__blk.as_ref().expect("no block given (LocalJumpError)").as_proc_unchecked())(&[#(#arg_exprs),*])?
            }
        }
        HirNode::BlockGiven => quote! { spinel_rt::RubyValue::Bool(__blk.is_some()) },
        HirNode::Raise(args) => emit_raise(cx, args),
        HirNode::CaseIn { subject, arms, else_body } => {
            super::patterns::emit_case_in(cx, *subject, arms, else_body)
        }
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
        HirNode::Program(_)
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. } => {
            panic!("unexpected top-level-only node in expression position")
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
/// Bare `raise` (zero args, re-raise) reads `spinel_rt::current_exception()`
/// -- the innermost `rescue` clause currently executing, if any (see
/// `spinel_rt::handling`'s docs) -- and re-raises it EXACTLY (same object,
/// same ivars/message, not a fresh copy), matching real Ruby's re-raise
/// semantics. Outside any `rescue` clause, real Ruby's bare `raise`
/// constructs a fresh `RuntimeError` with an EMPTY message instead of
/// erroring (confirmed via `ruby -e 'begin; raise; rescue => e; puts
/// "[#{e.message}]"; end'` -> `"[]"`) -- faithfully mirrored here via
/// `unwrap_or_else`, not a panic.
fn emit_raise(cx: &Ctx, args: &[NodeId]) -> TokenStream {
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
                vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(String::new())) }],
            );
            quote! {
                match spinel_rt::current_exception() {
                    Some(__v) => __v,
                    None => #fallback,
                }
            }
        }
        [one] => emit_raise_value(cx, *one, None),
        [class_arg, msg_arg] => emit_raise_value(cx, *class_arg, Some(*msg_arg)),
        _ => unreachable!("lowering rejects `raise`/`fail` with more than 2 arguments"),
    };
    quote! { return Err(spinel_rt::Signal::Raise(#exc)) }
}

/// Builds the actual `RubyValue` to raise, mirroring spinel's own `raise`
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
    // (`raise Store::Errors::NotFound` -- Phase 15.3) -- when the path
    // actually resolves to a class; a constant-shaped node that DOESN'T
    // resolve falls through to the value cases below (a constant can
    // legitimately hold a pre-built exception).
    let class_path = const_path_of(cx, node).filter(|p| cx.resolve_class(p).is_some());
    if let Some(msg_id) = explicit_msg {
        let Some(class_name) = class_path else {
            panic!("`raise Class, message` requires a literal class name (spike scope)");
        };
        let msg_expr = emit_expr(cx, msg_id);
        return emit_boxed_new(cx, &class_name, vec![msg_expr]);
    }
    if let Some(class_name) = class_path {
        let default_msg =
            quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(#class_name.to_string())) };
        return emit_boxed_new(cx, &class_name, vec![default_msg]);
    }
    match infer(cx, node) {
        TyKind::Str => {
            let msg_expr = emit_expr(cx, node);
            emit_boxed_new(cx, "RuntimeError", vec![msg_expr])
        }
        TyKind::Object(cid) => {
            let class_ident = super::ident::class_ident(cx.compiler, cid);
            let expr = emit_expr(cx, node);
            quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#expr)) }
        }
        // Already dynamically typed (Poly) -- used directly, assuming it's
        // already a constructed exception value (e.g. a local variable
        // holding one). A non-exception Poly value raised this way is a
        // narrow, documented gap (real Ruby raises `TypeError: exception
        // class/object expected` here) -- the general runtime coercion
        // this needs is Phase 9 work, alongside `rescue`.
        _ => emit_expr(cx, node),
    }
}

/// `emit_new_with_arg_tokens` returns a bare, unboxed `Arc<Concrete>` (the
/// same representation an ordinary `ClassName.new(...)` expression has --
/// see that function's docs); `Signal::Raise` needs a real `RubyValue`, so
/// this boxes it the same way `emit_safe_call` already does for its own
/// uniform-representation needs.
/// The class/module PATH a node names, when it has a constant-reference
/// SHAPE at all: a bare `ClassRef` or a qualified `Foo::Bar`
/// (`QualifiedConstRead` -- Phase 15.3). Whether the path actually NAMES a
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

pub(super) fn emit_boxed_new(cx: &Ctx, class_name: &str, arg_exprs: Vec<TokenStream>) -> TokenStream {
    let cid = cx
        .resolve_class(class_name)
        .unwrap_or_else(|| panic!("unknown class `{class_name}`"));
    let class_ident = super::ident::class_ident(cx.compiler, cid);
    let ctor = super::call::emit_new_with_arg_tokens(cx, class_name, arg_exprs);
    quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#ctor)) }
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
            quote! { spinel_rt::RubyValue::Object(#class_ident::new_handle(#value)) }
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
pub(super) fn box_for_local_storage(cx: &Ctx, name: &str, value_id: NodeId, value: TokenStream) -> TokenStream {
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

/// The already-resolved OWNER class id for a `@@name` reference (see
/// `analyze::mro::resolve_cvars`) -- looked up via `cx.defining_class` (the
/// class/module whose HIR body this reference is LEXICALLY written in), not
/// `cx.current_class` (the concrete struct it's materialized onto for a
/// mixed-in/inherited method): cvar ownership is a property of where the
/// code was WRITTEN, exactly like a closure's lexical scope, not of which
/// concrete receiver ends up calling it.
fn cvar_owner_id(cx: &Ctx, name: &str) -> u32 {
    let defining = cx
        .defining_class
        .expect("`@@` class variable referenced outside any class/module body");
    cx.compiler
        .class(defining)
        .cvar_owners
        .get(name)
        .unwrap_or_else(|| panic!("internal error: `@@{name}` has no resolved owner (analyze::mro::resolve_cvars should have run)"))
        .0
}

/// An ivar WRITE as a bare Rust STATEMENT (no read-back) -- factored out of
/// `emit_expr`'s `IvarWrite` arm so `codegen::loops::emit_target_write` (a
/// multi-assignment/`for`-loop ivar target) can reuse the identical
/// `self_ident`-aware write, instead of re-deriving it.
pub(super) fn emit_ivar_write_stmt(cx: &Ctx, name: &str, value: TokenStream) -> TokenStream {
    let ident = safe_ident(name);
    let slf = &cx.self_ident;
    // The `.freeze` guard (Phase 13.1) -- checked at the top of every ivar
    // write, mirroring CRuby's own `rb_check_frozen` in `vm_setivar_slowpath`.
    // The message's `#<Class>` receiver rendering is a fully-static
    // approximation of real Ruby's `#<Class:0xaddr @ivar=...>` inspect (this
    // runtime has no per-object address/ivar reflection to interpolate) --
    // documented, same posture as `RubyValue::inspect_string`'s `Object` arm.
    // `RubyObject::is_frozen` is UFCS-qualified: generated programs never
    // `use` the trait by name. The one atomic load this adds to every ivar
    // write (including inside `initialize`, where it's always false) is
    // negligible; `FrozenError`'s construction only ever runs on the raise
    // path.
    let class_name = cx
        .current_class
        .map(|cid| cx.compiler.class(cid).name.clone())
        .expect("ivar write outside a class context");
    let msg = format!("can't modify frozen {class_name}: #<{class_name}>");
    let frozen_error = emit_boxed_new(
        cx,
        "FrozenError",
        vec![quote! { spinel_rt::RubyValue::Str(spinel_rt::string_new(#msg.to_string())) }],
    );
    quote! {
        if spinel_rt::RubyObject::is_frozen(&*#slf) {
            return Err(spinel_rt::Signal::Raise(#frozen_error));
        }
        *#slf.#ident.lock() = #value;
    }
}

/// A cvar WRITE as a bare Rust STATEMENT -- see `emit_ivar_write_stmt`'s docs
/// for why this is factored out the same way.
pub(super) fn emit_cvar_write_stmt(cx: &Ctx, name: &str, value: TokenStream) -> TokenStream {
    let owner = cvar_owner_id(cx, name);
    quote! { spinel_rt::cvar_set(#owner, #name, #value); }
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
/// docs) as an ordinary RUNTIME outcome, not a spinelc-compile-time panic --
/// an unset constant is legitimately-valid-but-erroring Ruby, not a
/// programming mistake in the compiler itself.
fn const_owner_id(cx: &Ctx, scope: Option<&str>, name: &str) -> u32 {
    let owner_class = match scope {
        Some(class_name) => cx
            .resolve_class(class_name)
            .unwrap_or_else(|| panic!("unknown class/module `{class_name}`")),
        None => cx.defining_class.unwrap_or(crate::compiler::OBJECT_CLASS),
    };
    cx.compiler
        .class(owner_class)
        .const_owners
        .get(name)
        .copied()
        .unwrap_or(owner_class)
        .0
}

/// A constant READ -- `scope: None` for a bare `NAME` (see `const_owner_id`'s
/// docs for the lexical-then-top-level resolution rule), `scope:
/// Some(class_name)` for an explicit `Foo::NAME`. An unset constant raises a
/// real `NameError` (unlike an ivar/cvar/global's "never assigned" -> `nil`
/// convention) -- matches actual Ruby, and is cheap here since the runtime
/// `const_get` already distinguishes "never set" (`None`) from "set to
/// `nil`" (`Some(RubyValue::Nil)`).
fn emit_const_read(cx: &Ctx, scope: Option<&str>, name: &str) -> TokenStream {
    let owner = const_owner_id(cx, scope, name);
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
    let err = emit_boxed_new(
        cx,
        "NameError",
        vec![quote! {
            spinel_rt::RubyValue::Str(spinel_rt::string_new(format!("uninitialized constant {}", #qualified)))
        }],
    );
    quote! {
        match spinel_rt::const_get(#owner, #name) {
            Some(__v) => __v,
            None => return Err(spinel_rt::Signal::Raise(#err)),
        }
    }
}

/// A constant WRITE as a bare Rust STATEMENT -- see `emit_ivar_write_stmt`'s
/// docs for why this is factored out the same way (reused by
/// `codegen::loops::emit_target_write`'s `Const` multi-assignment target).
pub(super) fn emit_const_write_stmt(cx: &Ctx, scope: Option<&str>, name: &str, value: TokenStream) -> TokenStream {
    let owner = const_owner_id(cx, scope, name);
    quote! { spinel_rt::const_set(#owner, #name, #value); }
}
