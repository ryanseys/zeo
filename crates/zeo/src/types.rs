//! `TyKind` mirrors the predecessor's `types.h` -- deliberately trimmed
//! (no `Array`/`Hash`/`Float`/`Bignum` variants yet). See the plan's stated
//! scope-cut: only `Int` gets a real unboxed native representation for now
//! (literal arithmetic); everything else uses `RubyValue` (`Poly`)
//! uniformly. `Object` monomorphization (knowing a receiver's concrete
//! class) is what drives the static-vs-dynamic dispatch decision in
//! `codegen`, even though the *value* itself stays boxed either way.

use crate::compiler::FMap;
use crate::compiler::{ClassId, Compiler};
use crate::hir::{HirNode, NodeId};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TyKind {
    Int,
    Float,
    Symbol,
    Str,
    Array,
    Hash,
    Range,
    Object(ClassId),
    /// A first-class class/module VALUE: `x = Widget` -- the
    /// payload is the class the value REFERS to (its own class is
    /// `Class`/`Module`). What keeps `x.new(...)`/`x.some_class_method`
    /// statically resolvable and `.class` results statically foldable.
    ClassObj(ClassId),
    /// A real `Proc` -- only ever seeded for a named `&block`
    /// parameter (see `analyze::register_class`'s seeding, mirroring how a
    /// named `*rest`/`**kwrest` param seeds `Array`/`Hash`); nothing else
    /// infers this today.
    Proc,
    /// A real, `regex`-crate-backed `Regexp` -- see
    /// `hir::HirNode::RegexpLit`'s docs.
    Regexp,
    /// A `Fiber` handle -- only ever produced by `Fiber.new`
    /// (see the inference arm below).
    Fiber,
    /// `Thread`/`Mutex`/`Queue` and `Ractor` -- same
    /// only-from-`.new` inference shape as `Fiber`.
    Thread,
    Mutex,
    Queue,
    Ractor,
    /// The result of a successful `Regexp#match`/`String#match`. Nothing in
    /// `infer_type_with_locals` itself seeds this; a local holding a match
    /// result stays `Poly`
    /// (a documented, narrower-than-`New`/literal-driven inference scope-cut,
    /// matching this module's existing "no dataflow through arbitrary method
    /// calls" posture).
    MatchData,
    Poly,
}

/// Numeric binary operators whose result stays `Int` when both operands are
/// statically `Int` -- what lets the local-type tracker
/// (`analyze::locals`) propagate `Int`-ness
/// through a chain like `z = x + y`. `**` is
/// deliberately ABSENT since the numeric tower landed:
/// `2 ** -2` is a Rational -- an Int-Int `**` result types `Poly`.
/// (Overflow itself is fine: a Bignum result is still `TyKind::Int`, one
/// Ruby class with two payloads.)
pub const INT_RESULT_BINARY_OPS: &[&str] = &["+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>"];

/// Numeric binary operators whose result stays `Float` when the operand
/// pair is Float-Float or mixed Int/Float (Ruby's numeric-tower promotion
/// widens the `Int` side) -- the exact operand shapes whose
/// success value is always a `Float`:
/// `%` and `**` RAISE on their edge cases (zero modulus, negative base to
/// a fractional power -- the latter a documented divergence from CRuby's
/// Complex promotion) rather than returning another class, so `**` CAN be
/// here even though the Int-Int table excludes it. `<=>` is absent
/// (`NaN <=> x` is nil) and the comparisons return booleans, which this
/// value-result table doesn't type.
pub const FLOAT_RESULT_BINARY_OPS: &[&str] = &["+", "-", "*", "/", "%", "**"];

/// An empty locals map, for callers that have no per-scope local-type
/// context available (or don't need it) -- see `infer_type`.
fn no_locals() -> FMap<String, TyKind> {
    FMap::default()
}

/// Whether a REOPENED builtin class overrides `name` for a
/// receiver of static type `recv_ty` -- the guard every builtin-receiver
/// result-narrowing arm below must consult: `class String; def length;
/// "long"; end` makes the old `length -> Int` narrowing unsound (the
/// override's result is whatever it returns, so the call types `Poly`).
/// Free for un-reopened builtins: their materialized method table is empty.
pub(crate) fn builtin_override(
    compiler: &Compiler,
    recv_ty: TyKind,
    box_id: u32,
    name: &str,
) -> bool {
    use crate::compiler::{
        ARRAY_CLASS, FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS, MATCH_DATA_CLASS, PROC_CLASS,
        RANGE_CLASS, REGEXP_CLASS, STRING_CLASS, SYMBOL_CLASS,
    };
    let cid = match recv_ty {
        TyKind::Int => INTEGER_CLASS,
        TyKind::Float => FLOAT_CLASS,
        TyKind::Str => STRING_CLASS,
        TyKind::Symbol => SYMBOL_CLASS,
        TyKind::Array => ARRAY_CLASS,
        TyKind::Hash => HASH_CLASS,
        TyKind::Range => RANGE_CLASS,
        TyKind::Proc => PROC_CLASS,
        TyKind::Regexp => REGEXP_CLASS,
        TyKind::MatchData => MATCH_DATA_CLASS,
        _ => return false,
    };
    // Both tables: `methods` (materialized -- complete, but only AFTER
    // `mro::materialize` runs) and `own_methods` (populated during
    // registration, so per-scope local-type inference -- which runs at
    // `register_method` time -- already sees a reopen defined earlier in
    // the file; the same defined-earlier posture the rest of the one-pass
    // analyze has).
    if compiler.method_in_chain(cid, name).is_some()
        || compiler
            .class(cid)
            .own_methods
            .iter()
            .any(|&sid| compiler.scope(sid).name == name)
    {
        return true;
    }
    // Inside a box, that box's OVERLAY patches widen static
    // narrowing exactly like a root reopen would -- the patch is invisible
    // to every other box, so only the referencing code's own box checks.
    box_id != 0
        && compiler.classes.iter().any(|c| {
            c.builtin_overlay == Some(cid)
                && c.box_id == box_id
                && (c
                    .own_methods
                    .iter()
                    .any(|&sid| compiler.scope(sid).name == name)
                    || c.methods.iter().any(|e| compiler.names.str(e.name) == name))
        })
}

/// Context-free type inference: given a node, what's its static type,
/// ignoring any local variable bindings in scope? Mirrors `infer_type`/
/// `infer_uncached` (`analyze_infer.c:4771`/`4225`), simplified: no
/// memoization cache (`c->ntype[id]`) because nothing here is expensive or
/// mutually recursive yet. Prefer `infer_type_with_locals` wherever a
/// per-scope local-type map is available (almost everywhere in `codegen` and
/// `analyze::locals`) -- this thin wrapper exists only for the few call
/// sites (e.g. resolving a `New` receiver's class) that don't need one.
pub fn infer_type(compiler: &Compiler, id: NodeId) -> TyKind {
    infer_type_with_locals(compiler, None, 0, &no_locals(), id)
}

/// Real (if still one-pass, non-fixpoint) type inference: given a node and
/// the local-variable type map built so far for its enclosing scope (see
/// `analyze::locals::infer_locals`), what's its static type? This is what
/// lets `x + y` resolve to native `Int` arithmetic when `x`/`y` are locals
/// previously assigned an `Int`-typed value, not just literal-on-literal.
pub fn infer_type_with_locals(
    compiler: &Compiler,
    defining: Option<crate::compiler::ClassId>,
    box_id: u32,
    locals: &FMap<String, TyKind>,
    id: NodeId,
) -> TyKind {
    match &compiler.hir[id] {
        HirNode::IntegerLit(_) => TyKind::Int,
        // A bignum literal is still an Integer -- one Ruby class, two
        // payloads (`TyKind::Int` means exactly that).
        // Rational/imaginary literals type `Poly`: no static fast paths
        // exist for those kinds (deliberate -- they're rare), so they
        // dispatch through the runtime tower.
        HirNode::BigIntegerLit { .. } => TyKind::Int,
        HirNode::RationalLit { .. } | HirNode::ImaginaryLit(_) => TyKind::Poly,
        HirNode::FloatLit(_) => TyKind::Float,
        HirNode::Lambda { .. } => TyKind::Proc,
        HirNode::SymbolLit(_) => TyKind::Symbol,
        HirNode::StringLit(_) => TyKind::Str,
        HirNode::RegexpLit(..) => TyKind::Regexp,
        HirNode::ArrayLit(_) => TyKind::Array,
        HirNode::HashLit(_) => TyKind::Hash,
        HirNode::RangeLit { .. } => TyKind::Range,
        // A box-scoped splice types as its last statement, RESOLVED IN THE
        // BOX -- what keeps `w = box.eval("Widget.new")`
        // statically typed as the box's own Widget.
        HirNode::BoxScope { box_id: bx, body } => body.last().map_or(TyKind::Poly, |&s| {
            infer_type_with_locals(compiler, defining, *bx, locals, s)
        }),
        HirNode::BoxHandle(_) => TyKind::Poly,
        // Resolved against the referencing method's own lexical chain
        // -- `defining` is the AOT def->cref: a bare `Item.new`
        // inside `module Store` types as the nested `Store::Item`.
        HirNode::New { class_name, .. } => {
            match compiler.resolve_class(class_name, &compiler.cref_of(defining), box_id) {
                // `TyKind::Object(cid)` means "an instance with a compiled
                // layout", so it is only correct when such a layout actually
                // exists. The kinds with none all stay Poly (a plain
                // `RubyValue`), matching what `.new` lowers
                // to for each:
                //   - a MODULE: `M.new` routes to the dynamic path (which
                //     raises NoMethodError);
                //   - `Object.new`: the sentinel idiom, a boxed
                //     `zeo_rt::Object`;
                //   - a BUILT-IN (`Time.new`): answered by the
                //     runtime's own class-method table, as a `RubyValue`.
                //   - a BOOTSTRAP exception class: a `raise
                //     ArgumentError.new(...)` is a
                //     boxed `RubyValue` built by `construct_by_class_id`.
                //   - a class with its OWN `def self.new`: the call routes to
                //     that class method, which answers a `RubyValue` and is
                //     free to return anything at all (rubygems'
                //     `Gem::Package::TarWriter.new` answers nil in its block
                //     form).
                //   - a program where a runtime site may (re)define
                //     `initialize`/`new` (a lazily-loaded unit's class, a
                //     computed `define_method`): `.new` dispatches
                //     dynamically for the same reason every other call site
                //     does (`may_be_patched_at_runtime`), and the result is
                //     a `RubyValue`.
                //   - a RUNTIME-CONDITIONAL class: `.new` always takes
                //     the dynamic arm (concealment check + overlay rows), so
                //     the value is a `RubyValue` here too.
                Some(cid)
                    if compiler.has_generated_struct(cid)
                        && !compiler.class(cid).runtime_conditional
                        && compiler.class_method_in_chain(cid, "new").is_none()
                        && !compiler.may_be_patched_at_runtime("initialize")
                        && !compiler.may_be_patched_at_runtime("new") =>
                {
                    TyKind::Object(cid)
                }
                _ => TyKind::Poly,
            }
        }
        HirNode::LocalRead(name) => locals.get(name).copied().unwrap_or(TyKind::Poly),
        // A bare/qualified constant that names a class or module is a
        // first-class Class value; one that doesn't stays an
        // ordinary value constant (type unknown -> Poly). A
        // runtime-conditional class stays Poly: the read is a runtime
        // question (NameError while concealed), so nothing may fold on it.
        HirNode::ClassRef(name) => {
            match compiler.resolve_class(name, &compiler.cref_of(defining), box_id) {
                Some(cid) if !compiler.class(cid).runtime_conditional => TyKind::ClassObj(cid),
                _ => TyKind::Poly,
            }
        }
        HirNode::QualifiedConstRead(scope, name) => {
            let path = format!("{scope}::{name}");
            match compiler.resolve_class(&path, &compiler.cref_of(defining), box_id) {
                Some(cid) if !compiler.class(cid).runtime_conditional => TyKind::ClassObj(cid),
                _ => TyKind::Poly,
            }
        }
        // `Fiber.new { }` / `Thread.new { }` / `Mutex.new` / `Queue.new` --
        // the only constructors of these values. (All four are BUILTIN
        // classes kept OUT of `HirNode::New` by parse -- see the `.new`
        // lowering's exclusion list -- so this never collides with the
        // ordinary user-class `New` arm above.)
        // A `.new` carrying a `*args` splat, a `**h` double-splat, or a
        // forwarded `&block` argument can't take the static construction path
        // (see `parse`'s `New` lowering; a `&block` stays a `Call`, not a
        // `New`) -- it dispatches dynamically through `send_value`, which
        // answers a `RubyValue`, so it must type as `Poly`, not the concrete
        // class. Typing it `Object(cid)` made a chained method call take the
        // static struct-method path (`.run(..)`) against a boxed `RubyValue`,
        // emitting invalid Rust.
        HirNode::Call {
            receiver: Some(_),
            name,
            args,
            kwargs,
            block_arg,
            ..
        } if name == "new"
            && (args
                .iter()
                .any(|a| matches!(a, crate::hir::ArrayElem::Splat(_)))
                || kwargs
                    .iter()
                    .any(|k| matches!(k, crate::hir::KwArg::DoubleSplat(_)))
                || block_arg.is_some()) =>
        {
            TyKind::Poly
        }
        HirNode::Call {
            receiver: Some(recv),
            name,
            ..
        } if name == "new" => match &compiler.hir[*recv] {
            // The collection constructors type as what they build --
            // `a = Array.new(10000, 0)` must leave `a` statically `Array`
            // (element access compiles to the inline fast path instead of
            // 32M dynamic sends in a hot loop; found via bm_loops_times
            // running 100x slower than C). A literal block changes nothing
            // (`Array.new(3) { ... }` is still an Array); the splat/&proc
            // shapes are already sent to Poly by the guard above. A user
            // REOPEN of a builtin still wins at the call site: dispatch's
            // reopened-builtin check runs before the typed fast paths.
            HirNode::ClassRef(n) if n == "Array" => TyKind::Array,
            HirNode::ClassRef(n) if n == "Hash" => TyKind::Hash,
            HirNode::ClassRef(n) if n == "String" => TyKind::Str,
            HirNode::ClassRef(n) if n == "Fiber" => TyKind::Fiber,
            HirNode::ClassRef(n) if n == "Thread" => TyKind::Thread,
            HirNode::ClassRef(n) if n == "Mutex" => TyKind::Mutex,
            HirNode::ClassRef(n) if n == "Queue" => TyKind::Queue,
            // A `SizedQueue` value IS a `Queue` (bounded) -- it shares the
            // whole `Queue` method surface, so it types as `Queue` for static
            // dispatch; its distinct class identity lives in the runtime
            // payload (`queue_is_sized`), not the static type.
            HirNode::ClassRef(n) if n == "SizedQueue" => TyKind::Queue,
            HirNode::ClassRef(n) if n == "Ractor" => TyKind::Ractor,
            // `x.new(...)` through a class-value-typed receiver
            // constructs exactly what a literal `Widget.new(...)`
            // does -- and must TYPE the same way, since codegen's ClassObj
            // interception emits the same unboxed `Arc<Concrete>`
            // construction -- including the `HirNode::New` arm's
            // runtime-patch gate above, under which both emit a dynamic
            // `RubyValue` send instead.
            _ => match infer_type_with_locals(compiler, defining, box_id, locals, *recv) {
                TyKind::ClassObj(cid)
                    if compiler.has_generated_struct(cid)
                        && !compiler.class(cid).runtime_conditional
                        && compiler.class_method_in_chain(cid, "new").is_none()
                        && !compiler.may_be_patched_at_runtime("initialize")
                        && !compiler.may_be_patched_at_runtime("new") =>
                {
                    TyKind::Object(cid)
                }
                _ => TyKind::Poly,
            },
        },
        // An assignment's own VALUE is the local's binding read back, so it is
        // unboxed only when the local itself is (`LocalStorage::Shadowed`, i.e.
        // the whole-scope type is that same class). A local widened to `Poly`
        // -- by a second assignment of another class, or by sitting in a
        // `begin` -- reads back as a `RubyValue`, and claiming `Object` here
        // would have the caller box an already-boxed value.
        HirNode::LocalWrite(name, value) => {
            let value_ty = infer_type_with_locals(compiler, defining, box_id, locals, *value);
            match (value_ty, locals.get(name)) {
                (TyKind::Object(cid), Some(&TyKind::Object(slot))) if cid == slot => value_ty,
                (TyKind::Object(_), _) => TyKind::Poly,
                _ => value_ty,
            }
        }
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if matches!(args.as_slice(), [crate::hir::ArrayElem::Single(_)])
            && (INT_RESULT_BINARY_OPS.contains(&name.as_str())
                || FLOAT_RESULT_BINARY_OPS.contains(&name.as_str())) =>
        {
            let crate::hir::ArrayElem::Single(arg) = args[0] else {
                unreachable!("guarded above")
            };
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            let arg_ty = infer_type_with_locals(compiler, defining, box_id, locals, arg);
            if recv_ty == TyKind::Int
                && arg_ty == TyKind::Int
                && INT_RESULT_BINARY_OPS.contains(&name.as_str())
            {
                TyKind::Int
            } else if matches!(
                (recv_ty, arg_ty),
                (TyKind::Float, TyKind::Float)
                    | (TyKind::Float, TyKind::Int)
                    | (TyKind::Int, TyKind::Float)
            ) && FLOAT_RESULT_BINARY_OPS.contains(&name.as_str())
            {
                TyKind::Float
            } else {
                TyKind::Poly
            }
        }
        // Unary `-@`/`+@` on a statically-`Float` receiver stays `Float`.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && (name == "-@" || name == "+@") => {
            match infer_type_with_locals(compiler, defining, box_id, locals, *recv) {
                TyKind::Float => TyKind::Float,
                _ => TyKind::Poly,
            }
        }
        // `.class` on a receiver whose class is statically known is a
        // statically-known Class value -- mirrors codegen's
        // `.class` fold; the Object arm respects a user-defined `class`
        // override by NOT narrowing (same guard the emission site has).
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && name == "class" => {
            use crate::compiler::{
                ARRAY_CLASS, CLASS_CLASS, FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS, MODULE_CLASS,
                PROC_CLASS, RANGE_CLASS, REGEXP_CLASS, STRING_CLASS, SYMBOL_CLASS,
            };
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            // A reopened builtin's `class` override defeats the fold --
            // see `builtin_override`'s docs.
            if builtin_override(compiler, recv_ty, box_id, "class") {
                return TyKind::Poly;
            }
            match recv_ty {
                // Only for a class nothing inherits from: `self.class` inside a
                // shared inherited body answers the RECEIVER's class, which is
                // any descendant, so folding it to the base silently resolved
                // `self.class::Handler` / `self.class.const_get(:H)` against
                // the wrong namespace. Subclassed receivers keep the dynamic
                // read, which reports the real class.
                TyKind::Object(cid)
                    if compiler.method_in_chain(cid, "class").is_none()
                        && !compiler.has_subclass(cid) =>
                {
                    TyKind::ClassObj(cid)
                }
                TyKind::Int => TyKind::ClassObj(INTEGER_CLASS),
                TyKind::Float => TyKind::ClassObj(FLOAT_CLASS),
                TyKind::Str => TyKind::ClassObj(STRING_CLASS),
                TyKind::Symbol => TyKind::ClassObj(SYMBOL_CLASS),
                TyKind::Array => TyKind::ClassObj(ARRAY_CLASS),
                TyKind::Hash => TyKind::ClassObj(HASH_CLASS),
                TyKind::Range => TyKind::ClassObj(RANGE_CLASS),
                TyKind::Proc => TyKind::ClassObj(PROC_CLASS),
                TyKind::Regexp => TyKind::ClassObj(REGEXP_CLASS),
                TyKind::ClassObj(cid) => TyKind::ClassObj(if compiler.class(cid).is_module {
                    MODULE_CLASS
                } else {
                    CLASS_CLASS
                }),
                _ => TyKind::Poly,
            }
        }
        // `.length`/`.size` on any of the built-in collection types always
        // returns an `Int` (`zeo_rt::{array,hash,string}_len` all return
        // `i64`) -- needed so e.g. `i < arr.length` still types both
        // sides `Int`, not just literal-on-literal comparisons.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && (name == "length" || name == "size") => {
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            // A reopened builtin's override may return anything -- see
            // `builtin_override`'s docs.
            if builtin_override(compiler, recv_ty, box_id, name) {
                return TyKind::Poly;
            }
            match recv_ty {
                TyKind::Array | TyKind::Hash | TyKind::Str => TyKind::Int,
                _ => TyKind::Poly,
            }
        }
        // `.freeze` returns SELF, so a frozen collection keeps its static
        // type (`A = [1, 2].freeze; A[0]` still takes the static `Array`
        // fast path instead of the Poly-dispatch fallback -- the single most
        // common real-world freeze idiom, a frozen constant). Scoped to the
        // built-in collection types only: an `Object` receiver's class might
        // define its own `freeze` returning something else, and builtins
        // can't override it, so this narrowing is only provably sound here.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && name == "freeze" => {
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            if builtin_override(compiler, recv_ty, box_id, name) {
                return TyKind::Poly;
            }
            match recv_ty {
                t @ (TyKind::Str | TyKind::Array | TyKind::Hash | TyKind::Range) => t,
                _ => TyKind::Poly,
            }
        }
        // `.dup`/`.clone` on a built-in collection returns a fresh value of
        // the SAME kind (`RubyValue::dup_value` is per-variant), so the copy
        // keeps the receiver's static type and its later accesses stay on
        // the static fast paths -- same soundness reasoning (and same
        // Object-receiver exclusion) as the `freeze` arm above.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && (name == "dup" || name == "clone") => {
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            if builtin_override(compiler, recv_ty, box_id, name) {
                return TyKind::Poly;
            }
            match recv_ty {
                t @ (TyKind::Str | TyKind::Array | TyKind::Hash | TyKind::Range) => t,
                _ => TyKind::Poly,
            }
        }
        // `.to_a` on a built-in collection answers an Array (`Array#to_a` is
        // identity, `Range`/`Hash` walk into a fresh one) -- same soundness
        // posture (and same override exclusion) as the `dup` arm above.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && name == "to_a" => {
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            if builtin_override(compiler, recv_ty, box_id, name) {
                return TyKind::Poly;
            }
            match recv_ty {
                TyKind::Array | TyKind::Hash | TyKind::Range => TyKind::Array,
                _ => TyKind::Poly,
            }
        }
        // `Regexp#match`/`String#match` (either receiver/argument order)
        // always returns a `MatchData` (or `nil`, which doesn't change a
        // LOCAL's own static type -- a subsequent read still sees
        // `MatchData`, matching this codebase's existing "narrow the
        // ASSIGNED type, not a full nilable-union" posture for every other
        // seeded local type). Without this, a local holding a match result
        // stays `Poly`, and every `MatchData` accessor called on it later
        // would incorrectly route through the `Poly`-dispatch-to-`send`
        // fallback (which assumes an `Object` receiver) instead of this
        // module's own static `TyKind::MatchData` dispatch.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if name == "match" && matches!(args.as_slice(), [crate::hir::ArrayElem::Single(_)]) => {
            let crate::hir::ArrayElem::Single(arg) = args[0] else {
                unreachable!("guarded above")
            };
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            let arg_ty = infer_type_with_locals(compiler, defining, box_id, locals, arg);
            if builtin_override(compiler, recv_ty, box_id, name) {
                return TyKind::Poly;
            }
            match (recv_ty, arg_ty) {
                (TyKind::Regexp, TyKind::Str) | (TyKind::Str, TyKind::Regexp) => TyKind::MatchData,
                _ => TyKind::Poly,
            }
        }
        // `MatchData#to_a`/`#captures` -> `Array`, `#named_captures` ->
        // `Hash` -- same motivating need as the `match` arm just above (a
        // local holding one of these results must resolve past `Poly` for
        // its own later `[]`/`length` accesses to take the static fast
        // path instead of the `Poly`-dispatch-to-`send` fallback, which
        // assumes an `Object` receiver).
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && matches!(name.as_str(), "to_a" | "captures" | "named_captures") => {
            let recv_ty = infer_type_with_locals(compiler, defining, box_id, locals, *recv);
            if builtin_override(compiler, recv_ty, box_id, name) {
                return TyKind::Poly;
            }
            match recv_ty {
                TyKind::MatchData if name == "named_captures" => TyKind::Hash,
                TyKind::MatchData => TyKind::Array,
                _ => TyKind::Poly,
            }
        }
        _ => TyKind::Poly,
    }
}
