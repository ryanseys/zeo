//! `CallNode` argument/kwarg lowering helpers, block lowering (`lower_block`,
//! block parameters), and the call-argument splat/forwarding machinery. Split
//! out of `parse/mod.rs`.

use super::defs::lower_params;
use super::{PResult, lower_array_elem, lower_body, lower_kwargs};
use crate::hir::{ArrayElem, Hir, HirNode, KwArg, NodeId, Params};
use ruby_prism::{Node, ParseResult};

/// Shared by `lower_block` and lambda lowering (`-> (x) { }`/`lambda { }`):
/// both a `BlockNode` and a `LambdaNode` expose their own `.parameters()` as
/// the identical `Option<Node>` shape (a `BlockParametersNode`, or the
/// `_1`/`it` sugar nodes -- confirmed via `Prism.parse` directly, not just
/// inferred from the bindings).
pub(crate) fn lower_block_like_params(
    result: &ParseResult,
    hir: &mut Hir,
    params: Option<Node<'_>>,
) -> PResult<Params> {
    match params {
        None => Ok(Params::default()),
        // `_1`/`_2`/... -- `NumberedParametersNode { maximum }` reports the
        // highest `_N` referenced in the body; synthesize that many plain
        // required params (pure lowering-time sugar, no new HIR).
        Some(p) if p.as_numbered_parameters_node().is_some() => {
            let n = p.as_numbered_parameters_node().unwrap().maximum();
            Ok(Params {
                required: (1..=n).map(|i| format!("_{i}")).collect(),
                ..Params::default()
            })
        }
        // `it` -- `ItParametersNode` carries no fields (the body just
        // references bare `it`); synthesize a single required param.
        Some(p) if p.as_it_parameters_node().is_some() => Ok(Params {
            required: vec!["it".to_string()],
            ..Params::default()
        }),
        Some(p) => {
            let bp = p
                .as_block_parameters_node()
                .ok_or("unsupported block parameter form (zeo limitation)")?;
            let mut params = lower_params(result, hir, bp.parameters())?;
            // `|x; sum|`'s block-locals -- prism keeps them on the
            // `BlockParametersNode` itself (`locals()`), not in the
            // `ParametersNode` `lower_params` handles, precisely because
            // they are not parameters. See `Params::block_locals`' docs.
            params.block_locals = bp
                .locals()
                .iter()
                .filter_map(|l| l.as_block_local_variable_node())
                .map(|l| String::from_utf8_lossy(l.name().as_slice()).into_owned())
                .collect();
            Ok(params)
        }
    }
}

pub(crate) fn lower_block(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    let block = node
        .as_block_node()
        .ok_or("expected a block (`{ }` or `do..end`)")?;
    let mut params = lower_block_like_params(result, hir, block.parameters())?;
    // Prism's block-scope local table minus the names this block binds as
    // parameters (and explicit `;`-block-locals) yields exactly the IMPLICIT
    // block-locals -- names first-assigned inside the body, which Ruby resets
    // to nil per invocation. See `Params::implicit_block_locals`.
    let bound: std::collections::HashSet<String> = params.bound_names().into_iter().collect();
    params.implicit_block_locals = block
        .locals()
        .iter()
        .map(|c| String::from_utf8_lossy(c.as_slice()).into_owned())
        .filter(|n| !bound.contains(n))
        .collect();
    let body = lower_body(result, hir, block.body())?;
    Ok(hir.push(HirNode::Block { params, body }))
}

/// Splits a call's raw argument list into (positional `ArrayElem`s, an
/// ordered `KwArg` list, an optional forwarded block). A trailing
/// `KeywordHashNode` (`foo(x: 1, **h)`) is the only prism shape recognized as
/// keyword arguments (lowered via `lower_kwargs`); every other entry lowers as
/// a positional argument via `lower_array_elem`. The returned `Option<NodeId>`
/// is a `...`-forwarded block (`&__fwd_blk`); a call's own literal block lives
/// outside this function.
pub(crate) fn lower_call_args(
    result: &ParseResult,
    hir: &mut Hir,
    arguments: Option<ruby_prism::ArgumentsNode<'_>>,
) -> PResult<(Vec<ArrayElem>, Vec<KwArg>, Option<NodeId>)> {
    let Some(arguments) = arguments else {
        return Ok((Vec::new(), Vec::new(), None));
    };
    let mut list: Vec<_> = arguments.arguments().iter().collect();
    // `n(...)` inside `def m(...)` -- a `ForwardingArgumentsNode` in the
    // list. Expands to the three internal params `lower_params`
    // synthesized: `*__fwd_rest, **__fwd_kw, &__fwd_blk` (the block half
    // returned separately -- a call's block slot lives outside this
    // function). `**__fwd_kw` enters the ordered `kwargs` list as a trailing
    // double-splat.
    if let Some(pos) = list
        .iter()
        .position(|n| n.as_forwarding_arguments_node().is_some())
    {
        list.remove(pos);
        let fwd_kw = hir.push(HirNode::LocalRead("__fwd_kw".to_string()));
        let fwd_block = Some(hir.push(HirNode::LocalRead("__fwd_blk".to_string())));
        // The rest-splat slots in positionally where `...` was written.
        let rest_read = hir.push(HirNode::LocalRead("__fwd_rest".to_string()));
        let mut args = Vec::new();
        for (i, n) in list.iter().enumerate() {
            if i == pos {
                args.push(ArrayElem::Splat(rest_read));
            }
            args.push(lower_array_elem_or_anon(result, hir, n)?);
        }
        if pos >= list.len() {
            args.push(ArrayElem::Splat(rest_read));
        }
        return Ok((args, vec![KwArg::DoubleSplat(fwd_kw)], fwd_block));
    }
    let kwargs = match list.last().and_then(|n| n.as_keyword_hash_node()) {
        Some(kw) => {
            list.pop();
            let elements: Vec<Node<'_>> = kw.elements().iter().collect();
            lower_kwargs(result, hir, &elements)?
        }
        None => Vec::new(),
    };
    let args = list
        .iter()
        .map(|n| lower_array_elem_or_anon(result, hir, n))
        .collect::<PResult<Vec<_>>>()?;
    Ok((args, kwargs, None))
}

/// `lower_array_elem`, plus the CALL-argument-only anonymous `*` forwarding
/// form (`n(*)` inside `def m(*)`) -- an array literal's own bare `*` stays
/// rejected in `lower_array_elem` itself.
fn lower_array_elem_or_anon(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<ArrayElem> {
    if let Some(splat) = node.as_splat_node() {
        if splat.expression().is_none() {
            return Ok(ArrayElem::Splat(
                hir.push(HirNode::LocalRead("__anon_rest".to_string())),
            ));
        }
    }
    lower_array_elem(result, hir, node)
}
