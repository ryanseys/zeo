//! `TyKind` mirrors spinel's `types.h` -- deliberately trimmed for the spike
//! (no `Array`/`Hash`/`Float`/`Bignum` variants yet). See the plan's stated
//! scope-cut: only `Int` gets a real unboxed native representation for now
//! (literal arithmetic); everything else uses `RubyValue` (`Poly`)
//! uniformly. `Object` monomorphization (knowing a receiver's concrete
//! class) is what drives the static-vs-dynamic dispatch decision in
//! `codegen`, even though the *value* itself stays boxed either way.

use crate::compiler::{ClassId, Compiler};
use crate::hir::{HirNode, NodeId};
use std::collections::HashMap;

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
    /// A real `Proc` (Phase 6) -- only ever seeded for a named `&block`
    /// parameter (see `analyze::register_class`'s seeding, mirroring how a
    /// named `*rest`/`**kwrest` param seeds `Array`/`Hash`); nothing else
    /// infers this today.
    Proc,
    /// A real, `regex`-crate-backed `Regexp` (Phase 12.7) -- see
    /// `hir::HirNode::RegexpLit`'s docs.
    Regexp,
    /// A `Fiber` handle (Phase 13.3) -- only ever produced by `Fiber.new`
    /// (see the inference arm below), consumed by `codegen::call`'s
    /// `resume`/`alive?` dispatch.
    Fiber,
    /// `Thread`/`Mutex`/`Queue` (Phase 13.5) -- same only-from-`.new`
    /// inference shape as `Fiber`.
    Thread,
    Mutex,
    Queue,
    /// The result of a successful `Regexp#match`/`String#match` -- only ever
    /// produced by `codegen::call`'s own static dispatch (a `MatchData`
    /// value can't be constructed any other way), so nothing in
    /// `infer_type_with_locals` itself seeds this; a local holding a match
    /// result stays `Poly` unless the codegen call site narrows it directly
    /// (a documented, narrower-than-`New`/literal-driven inference scope-cut,
    /// matching this module's existing "no dataflow through arbitrary method
    /// calls" posture).
    MatchData,
    Poly,
}

/// Numeric binary operators whose result stays `Int` when both operands are
/// statically `Int` -- see `codegen::call`'s use of this same list to decide
/// the native-arithmetic fast path. Kept here (not codegen) because the
/// local-type tracker (`analyze::locals`) needs the identical list to
/// propagate `Int`-ness through a chain like `z = x + y`.
pub const INT_RESULT_BINARY_OPS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "&", "|", "^", "<<", ">>",
];

/// An empty locals map, for callers that have no per-scope local-type
/// context available (or don't need it) -- see `infer_type`.
fn no_locals() -> HashMap<String, TyKind> {
    HashMap::new()
}

/// Context-free type inference: given a node, what's its static type,
/// ignoring any local variable bindings in scope? Mirrors `infer_type`/
/// `infer_uncached` (`analyze_infer.c:4771`/`4225`) at spike scope: no
/// memoization cache (`c->ntype[id]`) because nothing here is expensive or
/// mutually recursive yet. Prefer `infer_type_with_locals` wherever a
/// per-scope local-type map is available (almost everywhere in `codegen` and
/// `analyze::locals`) -- this thin wrapper exists only for the few call
/// sites (e.g. resolving a `New` receiver's class) that don't need one.
pub fn infer_type(compiler: &Compiler, id: NodeId) -> TyKind {
    infer_type_with_locals(compiler, &no_locals(), id)
}

/// Real (if still one-pass, non-fixpoint) type inference: given a node and
/// the local-variable type map built so far for its enclosing scope (see
/// `analyze::locals::infer_locals`), what's its static type? This is what
/// lets `x + y` resolve to native `Int` arithmetic when `x`/`y` are locals
/// previously assigned an `Int`-typed value, not just literal-on-literal --
/// the Phase 1 generalization of the spike's original hardcoded
/// literal-`+`-on-`IntegerLit` fast path.
pub fn infer_type_with_locals(
    compiler: &Compiler,
    locals: &HashMap<String, TyKind>,
    id: NodeId,
) -> TyKind {
    match &compiler.hir[id] {
        HirNode::IntegerLit(_) => TyKind::Int,
        HirNode::FloatLit(_) => TyKind::Float,
        HirNode::Lambda { .. } => TyKind::Proc,
        HirNode::SymbolLit(_) => TyKind::Symbol,
        HirNode::StringLit(_) => TyKind::Str,
        HirNode::RegexpLit(..) => TyKind::Regexp,
        HirNode::ArrayLit(_) => TyKind::Array,
        HirNode::HashLit(_) => TyKind::Hash,
        HirNode::RangeLit { .. } => TyKind::Range,
        HirNode::New { class_name, .. } => match compiler.class_by_name(class_name) {
            Some(cid) => TyKind::Object(cid),
            None => TyKind::Poly,
        },
        HirNode::LocalRead(name) => locals.get(name).copied().unwrap_or(TyKind::Poly),
        // `Fiber.new { }` / `Thread.new { }` / `Mutex.new` / `Queue.new` --
        // the only constructors of these values. (All four are BUILTIN
        // classes kept OUT of `HirNode::New` by parse -- see the `.new`
        // lowering's exclusion list -- so this never collides with the
        // ordinary user-class `New` arm above.)
        HirNode::Call {
            receiver: Some(recv),
            name,
            ..
        } if name == "new" => match &compiler.hir[*recv] {
            HirNode::ClassRef(n) if n == "Fiber" => TyKind::Fiber,
            HirNode::ClassRef(n) if n == "Thread" => TyKind::Thread,
            HirNode::ClassRef(n) if n == "Mutex" => TyKind::Mutex,
            HirNode::ClassRef(n) if n == "Queue" => TyKind::Queue,
            _ => TyKind::Poly,
        },
        HirNode::LocalWrite(_, value) => infer_type_with_locals(compiler, locals, *value),
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if matches!(args.as_slice(), [crate::hir::ArrayElem::Single(_)])
            && INT_RESULT_BINARY_OPS.contains(&name.as_str()) =>
        {
            let crate::hir::ArrayElem::Single(arg) = args[0] else {
                unreachable!("guarded above")
            };
            let recv_ty = infer_type_with_locals(compiler, locals, *recv);
            let arg_ty = infer_type_with_locals(compiler, locals, arg);
            if recv_ty == TyKind::Int && arg_ty == TyKind::Int {
                TyKind::Int
            } else {
                TyKind::Poly
            }
        }
        // `.length`/`.size` on any of the built-in collection types always
        // returns an `Int` (`spinel_rt::{array,hash,string}_len` all return
        // `i64`) -- needed for e.g. `i < arr.length` to take the native
        // `Int` comparison fast path in `codegen::call`, not just literal-
        // on-literal comparisons.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } if args.is_empty() && (name == "length" || name == "size") => {
            match infer_type_with_locals(compiler, locals, *recv) {
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
            match infer_type_with_locals(compiler, locals, *recv) {
                t @ (TyKind::Str | TyKind::Array | TyKind::Hash | TyKind::Range) => t,
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
            let recv_ty = infer_type_with_locals(compiler, locals, *recv);
            let arg_ty = infer_type_with_locals(compiler, locals, arg);
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
            match infer_type_with_locals(compiler, locals, *recv) {
                TyKind::MatchData if name == "named_captures" => TyKind::Hash,
                TyKind::MatchData => TyKind::Array,
                _ => TyKind::Poly,
            }
        }
        _ => TyKind::Poly,
    }
}
