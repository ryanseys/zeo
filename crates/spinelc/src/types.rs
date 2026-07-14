//! `TyKind` mirrors spinel's `types.h` -- deliberately trimmed for the spike
//! (no `Array`/`Hash`/`Float`/`Bignum` variants yet). See the plan's stated
//! scope-cut: only `Int` gets a real unboxed native representation for now
//! (literal arithmetic); everything else uses `RubyValue` (`Poly`)
//! uniformly. `Object` monomorphization (knowing a receiver's concrete
//! class) is what drives the static-vs-dynamic dispatch decision in
//! `codegen`, even though the *value* itself stays boxed either way.

use crate::compiler::{ClassId, Compiler};
use crate::hir::{HirNode, NodeId};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TyKind {
    Int,
    Symbol,
    Object(ClassId),
    Poly,
}

/// Mirrors `infer_type`/`infer_uncached` (`analyze_infer.c:4771`/`4225`):
/// given a node, what's its static type? Spinel's version is a giant
/// memoizing `if`/`else if` chain over ~40 concrete kinds, recursing through
/// locals/ivars until a whole-program fixpoint stabilizes it. This is the
/// spike-scope slice: no memoization cache (`c->ntype[id]`) because nothing
/// here is expensive or mutually recursive yet, and only `New` resolves to a
/// concrete `Object(ClassId)` -- everything else is `Poly` (the scope-cut
/// stated in the plan: real ivar/param/return monomorphization is Phase 1).
pub fn infer_type(compiler: &Compiler, id: NodeId) -> TyKind {
    match &compiler.hir[id] {
        HirNode::IntegerLit(_) => TyKind::Int,
        HirNode::SymbolLit(_) => TyKind::Symbol,
        HirNode::New { class_name, .. } => match compiler.class_by_name(class_name) {
            Some(cid) => TyKind::Object(cid),
            None => TyKind::Poly,
        },
        _ => TyKind::Poly,
    }
}
