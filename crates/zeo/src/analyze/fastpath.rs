//! The inline-iterator fast-path predicates: which call shapes fuse to a
//! native loop (no `Proc` ever allocated). Pure `Compiler`+HIR questions --
//! the capture scan, the hoisting scan, and the emitter's dispatch all ask
//! the SAME predicate, so it lives here, backend-neutrally, rather than in
//! any one consumer.

#![warn(
    clippy::wildcard_enum_match_arm,
    reason = "swept: this module's matches are exhaustive. Re-enabled because a\n    parent module's file-level allow is INHERITED by its submodules"
)]

use crate::compiler::Compiler;
use crate::hir::{HirNode, NodeId};

/// Whether a call shape is the `.times` fast path (inline splice, no real
/// `Proc` ever allocated) -- the exact condition `codegen`'s dispatch
/// checks, factored out so `analyze::captures`' escaping-block scan can
/// ask the identical question: any block NOT matching this shape becomes a
/// real, heap-allocated `Proc` (see that module's docs -- there is no
/// separate "escape analysis" beyond this one check, since `.times` is the
/// only inline fast path that exists).
pub fn is_times_fast_path(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> bool {
    kwargs_empty
        && name == "times"
        && !compiler.times_literal_suppressed
        && receiver.is_some_and(|r| matches!(compiler.hir[r], HirNode::IntegerLit(_)))
}

/// `.times`'s sibling: `(1..9).each { }` on a LITERAL range whose bounds are
/// both Int literals -- the other call shape that fuses to a native counted
/// loop (no `Proc` allocated). Beginless/endless ranges keep the generic
/// path (an endless `each` never terminates by counting up).
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: only a literal range with two literal Int bounds fuses; every \
              other receiver shape keeps the generic dispatch path by definition"
)]
pub fn is_range_each_fast_path(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> bool {
    kwargs_empty
        && name == "each"
        && !compiler.range_each_literal_suppressed
        && receiver.is_some_and(|r| match compiler.hir[r] {
            HirNode::RangeLit {
                start: Some(s),
                end: Some(e),
                ..
            } => {
                matches!(compiler.hir[s], HirNode::IntegerLit(_))
                    && matches!(compiler.hir[e], HirNode::IntegerLit(_))
            }
            _ => false,
        })
}

/// Either inline-splice shape -- what the CAPTURE scans ask: a block NOT
/// matching one of these becomes a real, heap-allocated `Proc`.
/// Deliberately syntactic-only (typed `inline_iter_sites` nominations are
/// NOT consulted): a typed site's dynamic-fallback arm still builds a real
/// proc, so its shared outer locals must keep escaping-style cell captures,
/// which the inline arm routes through just as correctly.
pub fn is_inline_block_fast_path(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
) -> bool {
    is_times_fast_path(compiler, receiver, name, kwargs_empty)
        || is_range_each_fast_path(compiler, receiver, name, kwargs_empty)
}

/// The DESCEND decision for the hoisting/exception scans: any spliced block
/// body -- literal shape or typed-site nomination -- shares the enclosing
/// Rust scope, so its assigned locals hoist there and its loop jumps compile
/// against labels in that scope. Wider than `is_inline_block_fast_path`
/// (see its docs for why the capture scans keep the narrow answer).
pub fn is_spliced_block_body(
    compiler: &Compiler,
    receiver: Option<NodeId>,
    name: &str,
    kwargs_empty: bool,
    block: NodeId,
) -> bool {
    is_inline_block_fast_path(compiler, receiver, name, kwargs_empty)
        || compiler.inline_iter_sites.contains_key(&block)
}
