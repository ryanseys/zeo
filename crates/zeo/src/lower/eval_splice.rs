//! Literal-eval recognition: the compile-time-constant-string argument
//! check shared by `require`/`autoload`/`eval`, the `Ruby::Box#eval`/
//! `Ruby::Box.new` recognizers, and the "no top-level `class`/`def` inside
//! an eval'd body" guard. Split out of `parse/mod.rs`.

use super::consts::constant_path_name;
use super::{PResult, lower_node, parse_and_lower_into};
use crate::hir::{ArrayElem, Hir, HirNode, NodeId, StrPart};
use ruby_prism::{CallNode, Node, ParseResult};

/// Lower a `box.eval(arg)` call to its final HIR node, always a
/// [`HirNode::BoxScope`] carrying the box's static id.
///
/// A single string-LITERAL argument keeps the zero-cost AOT path: the source
/// is parsed and INLINED into the scope's body at compile time, so it needs
/// no runtime parser and -- at STATEMENT position (`allow_defs`) -- may even
/// define classes, exactly like real Ruby's `Box#eval` top-level iseq.
///
/// Every other source shape -- a non-literal expression, or a computed
/// string -- lowers to a receiver-less `eval` [`HirNode::Call`] wrapped in
/// the same `BoxScope`. Codegen threads the enclosing `box_id` into that
/// call so it evaluates through the runtime `eval` entry in the box's dimension,
/// mirroring how receiver-less `Kernel#eval` already falls through to the VM
/// for a non-literal source. `class`/`def` in a dynamic body are handled by
/// the VM at runtime, so `allow_defs` gates only the literal path.
pub fn lower_box_eval(
    hir: &mut Hir,
    result: &ruby_prism::ParseResult,
    call: &ruby_prism::CallNode<'_>,
    box_id: u32,
    allow_defs: bool,
) -> PResult<NodeId> {
    if call.block().is_some() {
        return Err("`Ruby::Box#eval` doesn't take a block".to_string().into());
    }
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    if args.len() != 1 {
        return Err(
            "`Ruby::Box#eval` is only supported with exactly one argument (zeo limitation)"
                .to_string()
                .into(),
        );
    }
    // Literal source: parse + inline at compile time (the AOT path).
    if let Some(src) = args[0]
        .as_string_node()
        .map(|sn| String::from_utf8_lossy(sn.unescaped()).into_owned())
    {
        let body = parse_and_lower_into(hir, &src).map_err(|e| {
            crate::lower_error::LowerError::unsupported(format!("Ruby::Box#eval: {e}"))
        })?;
        if !allow_defs {
            reject_top_level_defs(hir, &body)?;
        }
        return Ok(hir.push(HirNode::BoxScope { box_id, body }));
    }
    // Non-literal source: evaluate through the runtime `eval` entry in the box's
    // dimension. A receiver-less `eval` Call inside the `BoxScope` picks up
    // `box_id` from codegen's box context (see `codegen::call`'s eval
    // special-case); `self` is the ambient main object, consistent with the
    // literal path and with CRuby's `box.eval("self")` returning `main`.
    let source = lower_node(result, hir, &args[0])?;
    let eval_call = hir.push(HirNode::Call {
        receiver: None,
        name: "eval".to_string(),
        args: vec![ArrayElem::Single(source)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    });
    Ok(hir.push(HirNode::BoxScope {
        box_id,
        body: vec![eval_call],
    }))
}

/// Whether `node` is exactly `Ruby::Box.new` (no args, no block) -- the
/// only allocation shape supported, recognized by the loader at
/// top-level `box = Ruby::Box.new` statements.
pub fn is_ruby_box_new(node: &Node<'_>) -> bool {
    let Some(call) = node.as_call_node() else {
        return false;
    };
    if call.name().as_slice() != b"new" || call.block().is_some() {
        return false;
    }
    if call
        .arguments()
        .is_some_and(|a| a.arguments().iter().next().is_some())
    {
        return false;
    }
    call.receiver()
        .is_some_and(|r| constant_path_name(&r).is_ok_and(|n| n == "Ruby::Box"))
}

/// If `id` is a `StringLit` HIR node with no interpolation, its concatenated
/// literal text -- the exact structural check `eval`'s literal-splice path
/// The single string-literal argument of a `require`-shaped call, or `None`
/// when the call has any other argument shape (no arguments, several, or one
/// that isn't a compile-time-constant string).
///
/// Lowering the argument through the ordinary path is deliberate -- it picks
/// up prism's adjacent-literal folding for free -- and the throwaway node left
/// behind on a `None` return is harmless append-only arena bookkeeping.
pub(crate) fn single_literal_string_arg(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
) -> PResult<Option<String>> {
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [arg] = args.as_slice() else {
        return Ok(None);
    };
    // A splat/forwarding argument (`require(*names)`, `require(...)`) is one
    // argument syntactically but has no compile-time text -- and a bare
    // `SplatNode` doesn't lower as an expression, so it must be answered
    // (not lowered) here. The call stays live and the runtime
    // `Kernel#require` raises the catchable LoadError; every corpus hit
    // sits inside a `rescue LoadError` optional-dependency helper.
    if arg.as_splat_node().is_some() || arg.as_forwarding_arguments_node().is_some() {
        return Ok(None);
    }
    let id = lower_node(result, hir, arg)?;
    Ok(literal_string_text(hir, id))
}

/// needs (a `StringLit` is compile-time-constant iff every `StrPart` is
/// `Lit`, never `Interp`). Reusable for any future "must be a literal"
/// construct.
pub fn literal_string_text(hir: &Hir, id: NodeId) -> Option<String> {
    let HirNode::StringLit(parts) = &hir[id] else {
        return None;
    };
    let mut out = String::new();
    for p in parts {
        match p {
            StrPart::Lit(s) => out.push_str(s),
            // A raw-byte (non-UTF-8) or interpolated segment can't fold to a
            // static UTF-8 string (e.g. a `require` path).
            StrPart::Bytes(_) | StrPart::Interp(_) => return None,
        }
    }
    Some(out)
}

/// `analyze::register_class`'s registration walk only ever scans the
/// LITERAL top level of `Program`'s (or a class body's) own statement list
/// for `ClassDef`/`DefMethod` -- an `Eval`'d body's top-level statements are
/// nested inside its `Eval(body)` node, which that walk never unwraps. A
/// top-level `class`/`def` inside an eval'd literal would otherwise flow
/// straight to `codegen::expr::emit_expr`'s "unexpected top-level-only node
/// in expression position" panic -- this rejects that case with a clean
/// compile error instead of letting zeo itself panic (zeo limitation: the
/// same gap already exists today for any non-eval code that nests a
/// `class`/`def` inside e.g. an `if`, so this isn't a new hole, just a new
/// way to trigger an old one).
pub(crate) fn reject_top_level_defs(hir: &Hir, body: &[NodeId]) -> PResult<()> {
    for &id in body {
        if matches!(hir[id], HirNode::DefMethod { .. }) {
            return Err(
                "`eval` containing a top-level `def` isn't supported yet (zeo limitation)"
                    .to_string()
                    .into(),
            );
        }
    }
    Ok(())
}

/// Whether an eval'd literal's own top level holds a `class`/`module`.
///
/// The same hole as [`reject_top_level_defs`], for the other keyword -- but
/// only inside a METHOD body. The top-level registration walk descends an
/// `Eval` splice, so a `class` written in a top-level eval registers; one
/// written in an eval inside a `def` reached codegen with no site and died
/// as "a position the analyze walk doesn't register". A `Ruby::Box#eval`
/// splices into a `BoxScope`, which the walk also descends, so only the
/// plain-`eval` inline path asks this -- and its answer sends the snippet to
/// the runtime `eval` entry.
pub(crate) fn defines_a_class(hir: &Hir, body: &[NodeId]) -> bool {
    body.iter()
        .any(|&id| matches!(hir[id], HirNode::ClassDef { .. }))
}
