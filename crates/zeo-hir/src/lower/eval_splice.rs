//! Literal-eval recognition: the compile-time-constant-string argument
//! check shared by `require`/`autoload`/`eval`, the `Ruby::Box#eval`/
//! `Ruby::Box.new` recognizers, and the "no top-level `class`/`def` inside
//! an eval'd body" guard. Split out of `parse/mod.rs`.

use super::consts::constant_path_name;
use super::{PResult, lower_node, parse_and_lower_into};
use crate::hir::{Hir, HirNode, NodeId, StrPart};
use ruby_prism::{CallNode, Node, ParseResult};

/// `box.eval("literal")`'s body splice -- shared by the loader's
/// STATEMENT-position recognizer (class definitions allowed: real Ruby's
/// `Box#eval` compiles a top-level iseq) and `lower_node`'s
/// expression-position one (which additionally rejects defs, same as root
/// `eval`).
pub fn lower_box_eval_body(
    hir: &mut Hir,
    _result: &ruby_prism::ParseResult,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<Vec<NodeId>> {
    if call.block().is_some() {
        return Err("`Ruby::Box#eval` doesn't take a block".to_string().into());
    }
    let args: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    if args.len() != 1 {
        return Err(
            "`Ruby::Box#eval` is only supported with exactly one string-literal argument (zeo limitation)"
                .to_string().into(),
        );
    }
    let Some(src) = args[0]
        .as_string_node()
        .map(|sn| String::from_utf8_lossy(sn.unescaped()).into_owned())
    else {
        return Err(
            "`Ruby::Box#eval` with a non-literal argument isn't supported (zeo limitation) -- the source must be a plain string literal, resolvable at compile time"
                .to_string().into(),
        );
    };
    parse_and_lower_into(hir, &src)
        .map_err(|e| crate::lower_error::LowerError::unsupported(format!("Ruby::Box#eval: {e}")))
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
        if matches!(
            hir[id],
            HirNode::ClassDef { .. } | HirNode::DefMethod { .. }
        ) {
            return Err(
                "`eval` containing a top-level `class`/`def` isn't supported yet (zeo limitation)"
                    .to_string()
                    .into(),
            );
        }
    }
    Ok(())
}
