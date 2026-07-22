//! `if`/`unless` chain lowering and the class-body static-`if`/`unless`
//! fold (`static_bool`), `begin`/`rescue`/`ensure`, and the shared
//! `return`/`break`/`next` optional-value helper. Split out of
//! `parse/mod.rs`.

use super::consts::constant_path_name;
use super::{PResult, lower_array_elem, lower_body, lower_node};
use crate::hir::{Hir, HirNode, NodeId, RescueClause};
use ruby_prism::{Node, ParseResult};

/// `if`/`elsif`/`elsif`.../`else` is one `IfNode` per level, chained through
/// `subsequent()`: `None` (no further clauses), another `IfNode` (an
/// `elsif`), or an `ElseNode` (the final `else`). Recursing here builds the
/// same nesting `HirNode::If`'s `else_body` already expects -- an `elsif`
/// becomes a single-statement `else_body` containing the nested `If`.
pub(crate) fn lower_if_chain(
    result: &ParseResult,
    hir: &mut Hir,
    predicate: &Node<'_>,
    then_stmts: Option<ruby_prism::StatementsNode<'_>>,
    subsequent: Option<Node<'_>>,
) -> PResult<NodeId> {
    let cond = lower_node(result, hir, predicate)?;
    let then_body = lower_body(result, hir, then_stmts.map(|s| s.as_node()))?;
    let else_body = match subsequent {
        None => Vec::new(),
        Some(n) => {
            if let Some(elsif) = n.as_if_node() {
                vec![lower_if_chain(
                    result,
                    hir,
                    &elsif.predicate(),
                    elsif.statements(),
                    elsif.subsequent(),
                )?]
            } else if let Some(else_node) = n.as_else_node() {
                lower_body(result, hir, else_node.statements().map(|s| s.as_node()))?
            } else {
                return Err("expected `elsif` or `else` after `if` (zeo limitation)"
                    .to_string()
                    .into());
            }
        }
    };
    Ok(hir.push(HirNode::If {
        cond,
        then_body,
        else_body,
    }))
}

/// A statically-known boolean value for a class-body `if`/`unless` guard --
/// only the literal forms real programs use to compile-time-select a `def`/
/// `alias` (`... if true`, `... unless false`, `... if (true)`). `nil` counts
/// as false (Ruby's own truthiness). Anything else (a method call, a
/// constant, a comparison) is `None`, leaving the `if` to lower as an
/// ordinary runtime conditional.
pub(crate) fn static_bool(node: &Node<'_>) -> Option<bool> {
    if node.as_true_node().is_some() {
        return Some(true);
    }
    if node.as_false_node().is_some() || node.as_nil_node().is_some() {
        return Some(false);
    }
    if let Some(paren) = node.as_parentheses_node() {
        let stmts = paren.body()?.as_statements_node()?;
        let body: Vec<_> = stmts.body().iter().collect();
        if let [only] = body.as_slice() {
            return static_bool(only);
        }
    }
    None
}

/// `begin body rescue R1 rescue R2 ... else ... ensure ... end` -- `rescue`
/// clauses arrive as a singly-linked chain (`RescueNode::subsequent()`), not
/// a list, mirroring `if`/`elsif`'s own `subsequent()` chaining. `exceptions()`
/// entries are expected to be plain constants (`rescue Foo, Bar => e`) --
/// anything else (a splatted exception list, `rescue *errs`) falls through
/// to `constant_name`'s existing "expected a plain constant name" rejection,
/// same posture as `superclass`/`include`/`extend`/`prepend` resolution
/// elsewhere in this file. `reference()` (the `=> e` binding) is always a
/// plain local-variable target in real Ruby's own grammar for this position.
pub(crate) fn lower_begin(
    result: &ParseResult,
    hir: &mut Hir,
    begin: &ruby_prism::BeginNode<'_>,
) -> PResult<NodeId> {
    let body = lower_body(result, hir, begin.statements().map(|s| s.as_node()))?;

    let mut rescues = Vec::new();
    let mut next = begin.rescue_clause();
    while let Some(r) = next {
        let classes = r
            .exceptions()
            .iter()
            .map(|n| constant_path_name(&n))
            .collect::<PResult<Vec<_>>>()?;
        let binding = match r.reference() {
            None => None,
            Some(n) => Some(local_target_name(&n)?),
        };
        let rescue_body = lower_body(result, hir, r.statements().map(|s| s.as_node()))?;
        rescues.push(RescueClause {
            classes,
            binding,
            body: rescue_body,
        });
        next = r.subsequent();
    }

    let else_body = match begin.else_clause() {
        None => None,
        Some(e) => Some(lower_body(
            result,
            hir,
            e.statements().map(|s| s.as_node()),
        )?),
    };
    let ensure_body = match begin.ensure_clause() {
        None => None,
        Some(e) => Some(lower_body(
            result,
            hir,
            e.statements().map(|s| s.as_node()),
        )?),
    };

    Ok(hir.push(HirNode::Begin {
        body,
        rescues,
        else_body,
        ensure_body,
    }))
}

/// A `return`/`break`/`next`'s optional value.
///
/// More than one value (`return 1, 2`) builds an implicit ARRAY -- the same
/// array a `[1, 2]` literal would, splats included, which is why this
/// delegates to `lower_array_elem` rather than re-deriving the shape. Real
/// Ruby, oracle-verified:
///
/// ```text
/// def two = (return 1, 2)      # => [1, 2]
/// def m(a) = (return 1, *a)    # m([2, 3]) => [1, 2, 3]
/// [1].each { break 1, 2 }      # => [1, 2]
/// ```
///
/// A SINGLE splat is an array too, and that is the case a plain
/// "len == 1 ? lower it : error" rule gets wrong: `return *a` with `a ==
/// [1]` is `[1]`, not `1` -- the splat expands into a fresh array rather
/// than passing its operand through. So one argument only takes the
/// scalar path when it isn't a splat.
pub(crate) fn lower_single_optional_argument(
    result: &ParseResult,
    hir: &mut Hir,
    args: Option<ruby_prism::ArgumentsNode<'_>>,
    _keyword: &str,
) -> PResult<Option<NodeId>> {
    let Some(args) = args else { return Ok(None) };
    let list: Vec<_> = args.arguments().iter().collect();
    match list.as_slice() {
        [] => Ok(None),
        [only] if only.as_splat_node().is_none() => Ok(Some(lower_node(result, hir, only)?)),
        _ => {
            let elems = list
                .iter()
                .map(|n| lower_array_elem(result, hir, n))
                .collect::<PResult<Vec<_>>>()?;
            Ok(Some(hir.push(HirNode::ArrayLit(elems))))
        }
    }
}

/// A `rescue ... => e` binding -- always a plain local-variable target in
/// real Ruby's own grammar for this one position (unlike a general
/// multi-assignment target, which additionally allows ivars/cvars/globals/
/// constants/`obj.attr`/`arr[i]`/nested groups -- see `lower_multi_target`).
fn local_target_name(node: &Node<'_>) -> PResult<String> {
    let target = node
        .as_local_variable_target_node()
        .ok_or("`rescue => name` only supports a plain local variable binding (zeo limitation)")?;
    Ok(String::from_utf8_lossy(target.name().as_slice()).into_owned())
}
