//! The `Storage`/target machinery behind compound assignment (`+=`/`||=`/
//! `&&=`) and multi-assignment (`a, b = ...`) lowering: the shared
//! read-then-write desugar over every lvalue kind, the evaluate-once
//! receiver/index binding for `obj.attr op= rhs`/`arr[i] op= rhs`, and the
//! generalized multi-assignment target shape. Split out of `parse/mod.rs`.

use super::consts::constant_path_name;
use super::{PResult, lower_node};
use crate::hir::{ArrayElem, Hir, HirNode, NodeId};
use ruby_prism::{Node, ParseResult};

/// The lvalue "storage kind" a compound-assignment (`+=`)/`||=`/`&&=`
/// operator can target -- factors their identical read-then-write desugar
/// (see the call sites in `lower_node` above) into one place instead of
/// five near-identical repetitions, one per underlying `HirNode` read/write
/// pair.
pub(crate) enum Storage {
    Local(String),
    Ivar(String),
    ClassVar(String),
    Global(String),
    /// `scope: None` = a bare, lexically-resolved name; `scope:
    /// Some(class_name)` = an explicit `Foo::NAME` -- see
    /// `HirNode::ConstWrite`'s docs.
    Const {
        scope: Option<String>,
        name: String,
    },
}

impl Storage {
    fn read(&self, hir: &mut Hir) -> NodeId {
        match self {
            Storage::Local(n) => hir.push(HirNode::LocalRead(n.clone())),
            Storage::Ivar(n) => hir.push(HirNode::IvarRead(n.clone())),
            Storage::ClassVar(n) => hir.push(HirNode::ClassVarRead(n.clone())),
            Storage::Global(n) => hir.push(HirNode::GlobalRead(n.clone())),
            Storage::Const { scope: None, name } => hir.push(HirNode::ClassRef(name.clone())),
            Storage::Const {
                scope: Some(scope),
                name,
            } => hir.push(HirNode::QualifiedConstRead(scope.clone(), name.clone())),
        }
    }

    fn write(&self, hir: &mut Hir, value: NodeId) -> NodeId {
        match self {
            Storage::Local(n) => hir.push(HirNode::LocalWrite(n.clone(), value)),
            Storage::Ivar(n) => hir.push(HirNode::IvarWrite(n.clone(), value)),
            Storage::ClassVar(n) => hir.push(HirNode::ClassVarWrite(n.clone(), value)),
            Storage::Global(n) => hir.push(HirNode::GlobalWrite(n.clone(), value)),
            Storage::Const { scope, name } => hir.push(HirNode::ConstWrite {
                scope: scope.clone(),
                name: name.clone(),
                value,
            }),
        }
    }
}

/// `target op= rhs` -- e.g. `x += 1`, desugared to `x = x + 1` (evaluating
/// `rhs` unconditionally, unlike `||=`/`&&=` below).
pub(crate) fn lower_compound_op_write(
    hir: &mut Hir,
    target: Storage,
    op: String,
    rhs: NodeId,
) -> NodeId {
    let read = target.read(hir);
    let call = hir.push(HirNode::Call {
        receiver: Some(read),
        name: op,
        args: vec![ArrayElem::Single(rhs)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    });
    target.write(hir, call)
}

/// `target ||= rhs` -- `target || (target = rhs)`, NOT `target = target ||
/// rhs`: `rhs` (and the write itself) must only be evaluated when `target`
/// is falsy, which `HirNode::Or`'s existing short-circuit codegen gives for
/// free.
///
/// A `Const` target is a genuine, narrow exception to reusing `Storage::read`
/// as-is (confirmed against real Ruby, not assumed): `CONST ||= v` on a
/// constant that was NEVER assigned quietly defines it, treating "never
/// assigned" as equivalent to a falsy read -- unlike an ordinary constant
/// read (`Storage::read`'s `ClassRef`/`QualifiedConstRead`), which always
/// raises `NameError` for that case, and unlike `CONST += v`/`CONST &&= v`
/// on the same undefined constant, which still DO raise (Ruby doesn't
/// extend this leniency to any other compound-assignment operator on a
/// constant). So only THIS function substitutes the lenient
/// `HirNode::ConstReadOrNil` for a `Const` target's read half --
/// `lower_and_write`/`lower_compound_op_write` deliberately keep using
/// `Storage::read` unchanged.
pub(crate) fn lower_or_write(hir: &mut Hir, target: Storage, rhs: NodeId) -> NodeId {
    let read = match &target {
        Storage::Const { scope, name } => {
            hir.push(HirNode::ConstReadOrNil(scope.clone(), name.clone()))
        }
        _ => target.read(hir),
    };
    let write = target.write(hir, rhs);
    hir.push(HirNode::Or(read, write))
}

/// `target &&= rhs` -- `target && (target = rhs)`; see `lower_or_write`'s
/// docs for the same "don't evaluate/write unless needed" reasoning.
pub(crate) fn lower_and_write(hir: &mut Hir, target: Storage, rhs: NodeId) -> NodeId {
    let read = target.read(hir);
    let write = target.write(hir, rhs);
    hir.push(HirNode::And(read, write))
}

/// At least one index argument. The COUNT is unrestricted: `[]`/`[]=` are
/// ordinary methods, so `h[a, b] += 1` is just a two-argument `[]` paired with
/// a three-argument `[]=`, and `Array#[]=` genuinely takes a `(start, length,
/// value)` form. Only the empty case (`h[] += 1`) is rejected, since `[]` needs
/// something to index by.
pub(crate) fn index_arguments(
    result_args: Option<ruby_prism::ArgumentsNode<'_>>,
) -> PResult<ruby_prism::ArgumentsNode<'_>> {
    let args =
        result_args.ok_or("`[]`-style compound assignment requires at least one index argument")?;
    if args.arguments().iter().count() == 0 {
        return Err(
            "`[]`-style compound assignment requires at least one index argument"
                .to_string()
                .into(),
        );
    }
    Ok(args)
}

/// Binds `receiver` to a hidden local (a `HirNode::LocalWrite` statement)
/// exactly ONCE, then builds `tmp.read_name` against that same binding --
/// shared by every `obj.attr op= rhs`/`||=`/`&&=` desugar (see their own
/// call sites in `lower_node`): a receiver expression may have side effects
/// (`get_obj().attr += 1`), so re-lowering the SAME prism node twice (once
/// per read/write call) would silently double-evaluate it. Returns `(bind
/// statement, read call, hidden local's name)` -- the caller combines the
/// read call with `rhs` however its own operator requires (see
/// `build_call_target_write`'s docs for the matching write half).
pub(crate) fn bind_call_target_once(
    result: &ParseResult,
    hir: &mut Hir,
    receiver: &Node<'_>,
    read_name: &str,
) -> PResult<(NodeId, NodeId, String)> {
    let recv_expr = lower_node(result, hir, receiver)?;
    let tmp = hir.gensym("__recv");
    let bind = hir.push(HirNode::LocalWrite(tmp.clone(), recv_expr));
    let read_recv = hir.push(HirNode::LocalRead(tmp.clone()));
    let read_call = hir.push(HirNode::Call {
        receiver: Some(read_recv),
        name: read_name.to_string(),
        args: Vec::new(),
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    });
    Ok((bind, read_call, tmp))
}

/// The write half of `bind_call_target_once` -- `tmp.write_name(value)`,
/// reading the SAME hidden receiver binding.
pub(crate) fn build_call_target_write(
    hir: &mut Hir,
    tmp: &str,
    write_name: &str,
    value: NodeId,
) -> NodeId {
    let write_recv = hir.push(HirNode::LocalRead(tmp.to_string()));
    hir.push(HirNode::Call {
        receiver: Some(write_recv),
        name: write_name.to_string(),
        args: vec![ArrayElem::Single(value)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

/// Same reasoning as `bind_call_target_once`, extended to BOTH the receiver
/// AND the (single) index argument of `arr[i] op= rhs`/`||=`/`&&=`
/// (`arr[compute_idx()] += 1` must call `compute_idx()` exactly once too,
/// not once per read/write `[]`/`[]=` call). Returns `(bind statements,
/// read call, receiver's hidden local name, index's hidden local name)`.
pub(crate) fn bind_index_target_once(
    result: &ParseResult,
    hir: &mut Hir,
    receiver: &Node<'_>,
    index_args: &ruby_prism::ArgumentsNode<'_>,
) -> PResult<(Vec<NodeId>, NodeId, String, Vec<String>)> {
    let recv_expr = lower_node(result, hir, receiver)?;
    let recv_tmp = hir.gensym("__recv");
    let mut binds = vec![hir.push(HirNode::LocalWrite(recv_tmp.clone(), recv_expr))];
    // EVERY index gets its own binding, for the same reason the receiver does:
    // `h[i(), j()] += 1` must call each index expression exactly once, not once
    // per `[]`/`[]=` call.
    let mut idx_tmps = Vec::new();
    for index_node in index_args.arguments().iter() {
        let idx_expr = lower_node(result, hir, &index_node)?;
        let idx_tmp = hir.gensym("__idx");
        binds.push(hir.push(HirNode::LocalWrite(idx_tmp.clone(), idx_expr)));
        idx_tmps.push(idx_tmp);
    }
    let read_recv = hir.push(HirNode::LocalRead(recv_tmp.clone()));
    let args = idx_tmps
        .iter()
        .map(|t| ArrayElem::Single(hir.push(HirNode::LocalRead(t.clone()))))
        .collect();
    let read_call = hir.push(HirNode::Call {
        receiver: Some(read_recv),
        name: "[]".to_string(),
        args,
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    });
    Ok((binds, read_call, recv_tmp, idx_tmps))
}

/// The write half of `bind_index_target_once` -- `recv_tmp[idx_tmps...] =
/// value`, reading the SAME hidden receiver/index bindings.
pub(crate) fn build_index_target_write(
    hir: &mut Hir,
    recv_tmp: &str,
    idx_tmps: &[String],
    value: NodeId,
) -> NodeId {
    let write_recv = hir.push(HirNode::LocalRead(recv_tmp.to_string()));
    let mut args: Vec<ArrayElem> = idx_tmps
        .iter()
        .map(|t| ArrayElem::Single(hir.push(HirNode::LocalRead(t.clone()))))
        .collect();
    args.push(ArrayElem::Single(value));
    hir.push(HirNode::Call {
        receiver: Some(write_recv),
        name: "[]=".to_string(),
        args,
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

/// One `MultiTarget` -- a `MultiWriteNode`/nested `MultiTargetNode`'s own
/// `lefts`/`rest`/`rights` entry, or a `for`-loop's `index()`. See
/// `MultiTarget`'s docs for the full generalized shape this now covers
/// (beyond the original plain-local-only restriction): local/ivar/cvar/
/// global/bare-constant/`obj.attr`/`arr[i]`/nested-group targets.
pub(crate) fn lower_multi_target(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<crate::hir::MultiTarget> {
    use crate::hir::MultiTarget;

    if let Some(t) = node.as_local_variable_target_node() {
        return Ok(MultiTarget::Local(
            String::from_utf8_lossy(t.name().as_slice()).into_owned(),
        ));
    }
    // The same group shape reached from a PARAMETER list (`|a, (b, c)|` --
    // see `required_param_slot`) names its leaves with parameter nodes rather
    // than target nodes: prism distinguishes the two contexts, but a
    // destructuring param binds a plain local exactly as an assignment target
    // does, so both spell the same `MultiTarget::Local`.
    if let Some(t) = node.as_required_parameter_node() {
        return Ok(MultiTarget::Local(
            String::from_utf8_lossy(t.name().as_slice()).into_owned(),
        ));
    }
    if let Some(t) = node.as_instance_variable_target_node() {
        let name = String::from_utf8_lossy(t.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        return Ok(MultiTarget::Ivar(name));
    }
    if let Some(t) = node.as_class_variable_target_node() {
        let name = String::from_utf8_lossy(t.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        return Ok(MultiTarget::ClassVar(name));
    }
    if let Some(t) = node.as_global_variable_target_node() {
        return Ok(MultiTarget::Global(
            String::from_utf8_lossy(t.name().as_slice()).into_owned(),
        ));
    }
    if let Some(t) = node.as_constant_target_node() {
        return Ok(MultiTarget::Const(
            String::from_utf8_lossy(t.name().as_slice()).into_owned(),
        ));
    }
    if let Some(t) = node.as_constant_path_target_node() {
        // Same parent-path + leaf split `ConstWrite`'s own `Foo::BAR = v`
        // lowering uses: the parent must be a static constant path.
        let parent = match t.parent() {
            Some(p) => constant_path_name(&p)?,
            None => String::new(), // `::BAR` -- top-level anchored
        };
        let name = String::from_utf8_lossy(
            t.name()
                .expect("a constant path target always has a name")
                .as_slice(),
        )
        .into_owned();
        return Ok(MultiTarget::ScopedConst {
            scope: parent,
            name,
        });
    }
    // `obj.attr, ... = ...` -- pre-builds the `attr=` write `Call` right now,
    // with a synthetic hidden local (`tmp_name`) standing in for "the value
    // this target receives" -- see `MultiTarget::Call`'s docs for why this
    // lets codegen reuse the ordinary static/dynamic dispatch machinery with
    // no bespoke attr-write codegen of its own.
    if let Some(t) = node.as_call_target_node() {
        let receiver = lower_node(result, hir, &t.receiver())?;
        // `CallTargetNode::name()` is ALREADY the setter name (`:x=`, not
        // `:x`) -- confirmed via `Prism.parse("b.x, b.y = ...")`; appending
        // another `=` here (a real bug, found via this session's own
        // testing) produced a double-equals method name (`x==`) that could
        // never resolve, silently breaking every multi-assignment into an
        // attr target (`b.x, b.y = b.y, b.x`) with a confusing "unsupported
        // call" panic instead of the correct swap.
        let setter_name = String::from_utf8_lossy(t.name().as_slice()).into_owned();
        let tmp_name = hir.gensym("__mval");
        let tmp_read = hir.push(HirNode::LocalRead(tmp_name.clone()));
        let write_call = hir.push(HirNode::Call {
            receiver: Some(receiver),
            name: setter_name,
            args: vec![ArrayElem::Single(tmp_read)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        });
        return Ok(MultiTarget::Call {
            write_call,
            tmp_name,
        });
    }
    // `arr[i], ... = ...` -- see `MultiTarget::Call`'s docs; same synthetic-
    // hidden-local trick, targeting `[]=` instead of `attr=`.
    if let Some(t) = node.as_index_target_node() {
        let receiver = lower_node(result, hir, &t.receiver())?;
        let arg_list: Vec<_> = t
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        if arg_list.len() != 1 {
            return Err("`arr[i] = ...` as a multi-assignment target only supports a single index argument (zeo limitation)".to_string().into());
        }
        let index = lower_node(result, hir, &arg_list[0])?;
        let tmp_name = hir.gensym("__mval");
        let tmp_read = hir.push(HirNode::LocalRead(tmp_name.clone()));
        let write_call = hir.push(HirNode::Call {
            receiver: Some(receiver),
            name: "[]=".to_string(),
            args: vec![ArrayElem::Single(index), ArrayElem::Single(tmp_read)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        });
        return Ok(MultiTarget::Call {
            write_call,
            tmp_name,
        });
    }
    // `(a, b), c = ...` -- a nested destructuring group; see
    // `lower_multi_target_group`'s docs.
    if let Some(t) = node.as_multi_target_node() {
        let group = lower_multi_target_group(result, hir, t.lefts(), t.rest(), t.rights())?;
        return Ok(MultiTarget::Nested(group));
    }
    Err(
        "unsupported multi-assignment/`for`-loop target shape (zeo limitation)"
            .to_string()
            .into(),
    )
}

/// The `before`/`splat`/`after` shape shared by `MultiWriteNode` and a
/// nested `MultiTargetNode` (both expose the identical `lefts()`/`rest()`/
/// `rights()` grammar) -- see `MultiTargetGroup`'s docs. An anonymous `*`
/// splat target (no name at all) is still a clean lowering error, unchanged
/// from the pre-existing plain-local-only restriction.
pub(crate) fn lower_multi_target_group(
    result: &ParseResult,
    hir: &mut Hir,
    lefts: ruby_prism::NodeList<'_>,
    rest: Option<Node<'_>>,
    rights: ruby_prism::NodeList<'_>,
) -> PResult<crate::hir::MultiTargetGroup> {
    let before = lefts
        .iter()
        .map(|n| lower_multi_target(result, hir, &n))
        .collect::<PResult<Vec<_>>>()?;
    let splat = match rest {
        None => None,
        // A PARAMETER-context group (`|(a, *r)|`) spells its splat as a
        // `RestParameterNode` where an assignment-context one uses a
        // `SplatNode` -- same meaning, and both allow the anonymous form
        // (`Some(None)`: absorbs and discards the middle slice).
        Some(n) if n.as_rest_parameter_node().is_some() => {
            let r = n.as_rest_parameter_node().unwrap();
            Some(r.name().map(|name| {
                Box::new(crate::hir::MultiTarget::Local(
                    String::from_utf8_lossy(name.as_slice()).into_owned(),
                ))
            }))
        }
        Some(n) => {
            let splat = n
                .as_splat_node()
                .ok_or("expected `*name` as a multi-assignment's splat target")?;
            match splat.expression() {
                // Anonymous `*` -- absorbs (and discards) the middle slice;
                // `MultiTargetGroup::splat`'s `Some(None)` shape models
                // exactly this.
                None => Some(None),
                Some(expr) => Some(Some(Box::new(lower_multi_target(result, hir, &expr)?))),
            }
        }
    };
    let after = rights
        .iter()
        .map(|n| lower_multi_target(result, hir, &n))
        .collect::<PResult<Vec<_>>>()?;
    Ok(crate::hir::MultiTargetGroup {
        before,
        splat,
        after,
    })
}
