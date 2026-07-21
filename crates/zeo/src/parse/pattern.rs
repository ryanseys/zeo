//! `case/in` pattern lowering (`lower_pattern`) and its helpers: guarded
//! `in` clauses, `|` alternation flattening, array/find/hash pattern rest
//! and key handling. Split out of `parse/mod.rs`.

use super::consts::constant_path_name;
use super::{PResult, lower_node};
use crate::hir::{HashPatternRest, Hir, NodeId, Pattern};
use ruby_prism::{Node, ParseResult};

/// An `in` clause's pattern slot -- either the bare pattern, or (for a
/// guarded arm) prism's own encoding of `PATTERN if/unless COND`: the
/// pattern wrapped in an `IfNode`/`UnlessNode` whose `predicate` is the
/// guard condition and whose single statement is the real pattern
/// (confirmed empirically against `Prism.parse` -- there is no separate
/// "guard" field on `InNode` itself). Returns `(pattern, guard)` where
/// `guard` is `(condition, is_unless)`, mirroring `HirNode::While`'s
/// `negate`-flag convention rather than a separate boolean-inverted shape.
pub(crate) fn lower_in_pattern_and_guard(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<(Pattern, Option<(NodeId, bool)>)> {
    if let Some(if_node) = node.as_if_node() {
        let cond = lower_node(result, hir, &if_node.predicate())?;
        let pattern = lower_single_wrapped_pattern(result, hir, if_node.statements(), "if")?;
        return Ok((pattern, Some((cond, false))));
    }
    if let Some(unless_node) = node.as_unless_node() {
        let cond = lower_node(result, hir, &unless_node.predicate())?;
        let pattern =
            lower_single_wrapped_pattern(result, hir, unless_node.statements(), "unless")?;
        return Ok((pattern, Some((cond, true))));
    }
    Ok((lower_pattern(result, hir, node)?, None))
}

fn lower_single_wrapped_pattern(
    result: &ParseResult,
    hir: &mut Hir,
    stmts: Option<ruby_prism::StatementsNode<'_>>,
    guard_kind: &str,
) -> PResult<Pattern> {
    let stmts = stmts.ok_or_else(|| {
        format!("expected a pattern inside an `{guard_kind}`-guarded `in` clause")
    })?;
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 {
        return Err(format!(
            "expected exactly one pattern inside an `{guard_kind}`-guarded `in` clause (spike scope)"
        ).into());
    }
    lower_pattern(result, hir, &body[0])
}

/// Lowers one `case/in`/`in pattern`/`=> pattern` PATTERN node (as opposed to
/// an ordinary expression -- see `Pattern`'s docs for why this is a separate
/// tree from `HirNode`). Dispatches on the pattern's own `ruby-prism` node
/// shape; anything not specifically recognized falls through to the general
/// `Value` case (an ordinary expression, matched via `rb_eq`), which is what
/// makes a bare literal (`in 1`, `in nil`, `in "x"`) work for free.
pub(crate) fn lower_pattern(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<Pattern> {
    if let Some(t) = node.as_local_variable_target_node() {
        let name = String::from_utf8_lossy(t.name().as_slice()).into_owned();
        return Ok(Pattern::Bind(name));
    }
    if let Some(cap) = node.as_capture_pattern_node() {
        let inner = lower_pattern(result, hir, &cap.value())?;
        let name = String::from_utf8_lossy(cap.target().name().as_slice()).into_owned();
        return Ok(Pattern::Capture(Box::new(inner), name));
    }
    if node.as_alternation_pattern_node().is_some() {
        let mut parts = Vec::new();
        flatten_alternation(result, hir, node, &mut parts)?;
        if parts.iter().any(pattern_may_bind) {
            return Err(
                "a pattern can't bind a variable inside a `|` alternation (spike scope, matches real Ruby)"
                    .to_string().into(),
            );
        }
        return Ok(Pattern::Or(parts));
    }
    if let Some(pin) = node.as_pinned_variable_node() {
        let expr = lower_node(result, hir, &pin.variable())?;
        return Ok(Pattern::Pin(expr));
    }
    if let Some(pin) = node.as_pinned_expression_node() {
        let expr = lower_node(result, hir, &pin.expression())?;
        return Ok(Pattern::Pin(expr));
    }
    if let Some(arr) = node.as_array_pattern_node() {
        let constant = arr.constant().map(|c| constant_path_name(&c)).transpose()?;
        let pre = arr
            .requireds()
            .iter()
            .map(|n| lower_pattern(result, hir, &n))
            .collect::<PResult<Vec<_>>>()?;
        let rest = arr
            .rest()
            .map(|n| array_or_find_rest_name(&n))
            .transpose()?;
        let post = arr
            .posts()
            .iter()
            .map(|n| lower_pattern(result, hir, &n))
            .collect::<PResult<Vec<_>>>()?;
        return Ok(Pattern::Array {
            constant,
            pre,
            rest,
            post,
        });
    }
    if let Some(find) = node.as_find_pattern_node() {
        let constant = find
            .constant()
            .map(|c| constant_path_name(&c))
            .transpose()?;
        // `left()` is already typed as `SplatNode` by `ruby-prism`; `right()`
        // (asymmetrically) comes back as a generic `Node` that must still be
        // cast -- confirmed against the actual generated bindings, not
        // assumed from the grammar's apparent symmetry.
        let pre_rest = splat_target_name(&find.left())?;
        let mid = find
            .requireds()
            .iter()
            .map(|n| lower_pattern(result, hir, &n))
            .collect::<PResult<Vec<_>>>()?;
        let post_rest = array_or_find_rest_name(&find.right())?;
        return Ok(Pattern::Find {
            constant,
            pre_rest,
            mid,
            post_rest,
        });
    }
    if let Some(hp) = node.as_hash_pattern_node() {
        let constant = hp.constant().map(|c| constant_path_name(&c)).transpose()?;
        let mut pairs = Vec::new();
        for el in hp.elements().iter() {
            let assoc = el
                .as_assoc_node()
                .ok_or("expected `key: pattern` inside a hash pattern (spike scope)")?;
            let key = hash_pattern_key_name(&assoc.key())?;
            // The `{key:}` shorthand -- prism synthesizes the value as an
            // `ImplicitNode` wrapping a `LocalVariableTargetNode` of the
            // same name (confirmed empirically against `Prism.parse`), so
            // `None` here means "bind a local named `key` directly", not
            // "no value at all".
            let value_pattern = if assoc.value().as_implicit_node().is_some() {
                None
            } else {
                Some(lower_pattern(result, hir, &assoc.value())?)
            };
            pairs.push((key, value_pattern));
        }
        let rest = hash_pattern_rest(hp.rest())?;
        return Ok(Pattern::Hash {
            constant,
            pairs,
            rest,
        });
    }
    if let Some(range) = node.as_range_node() {
        let start = match range.left() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        let end = match range.right() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        return Ok(Pattern::Range {
            start,
            end,
            exclusive: range.is_exclude_end(),
        });
    }
    // A bare constant with no capture (`in Integer`, `in SomeClass`, or a
    // qualified `in Store::Item` -- Phase 15.3) -- an `is_a?`-style check,
    // resolved (built-in tag vs. user-class ancestry) entirely in
    // `codegen::patterns::emit_class_check`.
    if let Some(c) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(c.name().as_slice()).into_owned();
        return Ok(Pattern::ClassCheck(name));
    }
    if node.as_constant_path_node().is_some() {
        return Ok(Pattern::ClassCheck(constant_path_name(node)?));
    }
    // Fallback: an ordinary expression (literal or otherwise), matched via
    // `rb_eq` -- see `Pattern::Value`'s docs. Lowering this through the
    // generic `lower_node` path is what makes `nil`/`true`/`false`/
    // `Integer`/`String`/`Symbol` literals, and even an arbitrary method
    // call, work as a pattern for free.
    let value = lower_node(result, hir, node)?;
    Ok(Pattern::Value(value))
}

/// Recursively flattens `P1 | P2 | ... | Pn` (parsed as left-associative
/// nested `AlternationPatternNode`s) into a flat list -- validation that no
/// alternative binds a variable happens at the call site in `lower_pattern`.
fn flatten_alternation(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    out: &mut Vec<Pattern>,
) -> PResult<()> {
    if let Some(alt) = node.as_alternation_pattern_node() {
        flatten_alternation(result, hir, &alt.left(), out)?;
        flatten_alternation(result, hir, &alt.right(), out)?;
    } else {
        out.push(lower_pattern(result, hir, node)?);
    }
    Ok(())
}

fn pattern_may_bind(p: &Pattern) -> bool {
    let mut found = false;
    p.for_each_bound_name(&mut |_| found = true);
    found
}

/// A generic `*`/`**` splat target's name, given the already-cast `SplatNode`
/// -- `None` for an anonymous `*`/`**` (discards its slice), `Some(name)` for
/// a named one. Shared by `Find`'s always-present `left`/`right` splats.
fn splat_target_name(splat: &ruby_prism::SplatNode<'_>) -> PResult<Option<String>> {
    match splat.expression() {
        None => Ok(None),
        Some(e) => {
            let t = e.as_local_variable_target_node().ok_or(
                "a pattern's `*` splat may only bind a plain local variable name (spike scope)",
            )?;
            Ok(Some(
                String::from_utf8_lossy(t.name().as_slice()).into_owned(),
            ))
        }
    }
}

/// `ArrayPatternNode::rest()`'s payload -- a generic `Node` that must itself
/// be a `SplatNode` (unlike `Find`'s `left`/`right`, which prism already
/// types as `SplatNode` directly).
fn array_or_find_rest_name(node: &Node<'_>) -> PResult<Option<String>> {
    // A trailing comma (`in [0, 1, ]`) is prism's `ImplicitRestNode`: an
    // anonymous "at least this many elements" rest that binds nothing --
    // exactly the `Some(None)` shape codegen already emits for a bare `*`.
    if node.as_implicit_rest_node().is_some() {
        return Ok(None);
    }
    let splat = node
        .as_splat_node()
        .ok_or("expected a `*name` splat in this array pattern (spike scope)")?;
    splat_target_name(&splat)
}

/// A hash pattern key -- only a literal `key:` symbol is supported (spike
/// scope, matching this project's existing symbol-key-only restriction on
/// hash pattern -- a computed/string key needs a distinct `AssocNode` shape
/// this doesn't lower).
fn hash_pattern_key_name(node: &Node<'_>) -> PResult<String> {
    let sym = node
        .as_symbol_node()
        .ok_or("only symbol keys (`key:`) are supported in a hash pattern (spike scope)")?;
    Ok(String::from_utf8_lossy(sym.unescaped()).into_owned())
}

/// `HashPatternNode::rest()`'s payload -- see `HashPatternRest`'s docs for
/// the three shapes (`**rest`/anonymous `**`, `**nil`, or absent entirely).
fn hash_pattern_rest(node: Option<Node<'_>>) -> PResult<HashPatternRest> {
    let Some(n) = node else {
        return Ok(HashPatternRest::None);
    };
    if n.as_no_keywords_parameter_node().is_some() {
        return Ok(HashPatternRest::NoMoreKeys);
    }
    let sp = n
        .as_assoc_splat_node()
        .ok_or("expected `**rest`/`**nil` in this hash pattern (spike scope)")?;
    match sp.value() {
        None => Ok(HashPatternRest::Rest(None)),
        Some(v) => {
            let t = v.as_local_variable_target_node().ok_or(
                "a hash pattern's `**` may only bind a plain local variable name (spike scope)",
            )?;
            Ok(HashPatternRest::Rest(Some(
                String::from_utf8_lossy(t.name().as_slice()).into_owned(),
            )))
        }
    }
}
