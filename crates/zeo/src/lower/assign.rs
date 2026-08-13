//! The `Storage`/target machinery behind compound assignment (`+=`/`||=`/
//! `&&=`) and multi-assignment (`a, b = ...`) lowering: the shared
//! read-then-write desugar over every lvalue kind, the evaluate-once
//! receiver/index binding for `obj.attr op= rhs`/`arr[i] op= rhs`, and the
//! generalized multi-assignment target shape. Split out of `parse/mod.rs`.

use super::consts::constant_path_name;
use super::{PResult, cvar_read, cvar_write, lower_node};
use crate::hir::{ArrayElem, Hir, HirNode, LastMatch, NodeId};
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
    /// `expr::NAME op= v` -- the scope is already bound to the hidden local
    /// `tmp` by a preceding statement, so the read half and the write half
    /// consult the SAME evaluation of it (`registry_for(key)::CACHE ||= {}`
    /// must call `registry_for` once). See `bind_scope_once`.
    DynConst {
        tmp: String,
        name: String,
    },
}

impl Storage {
    fn read(&self, hir: &mut Hir) -> NodeId {
        match self {
            Storage::Local(n) => hir.push(HirNode::LocalRead(n.clone())),
            Storage::Ivar(n) => hir.push(HirNode::IvarRead(n.clone())),
            Storage::ClassVar(n) => super::cvar_read(hir, n.clone()),
            Storage::Global(n) => hir.push(HirNode::GlobalRead(n.clone())),
            Storage::Const { scope: None, name } => hir.push(HirNode::ClassRef(name.clone())),
            Storage::Const {
                scope: Some(scope),
                name,
            } => hir.push(HirNode::QualifiedConstRead(scope.clone(), name.clone())),
            Storage::DynConst { tmp, name } => {
                let scope = hir.push(HirNode::LocalRead(tmp.clone()));
                hir.push(HirNode::DynConstRead {
                    scope,
                    name: name.clone(),
                    lenient: false,
                })
            }
        }
    }

    fn write(&self, hir: &mut Hir, value: NodeId) -> NodeId {
        match self {
            Storage::Local(n) => hir.push(HirNode::LocalWrite(n.clone(), value)),
            Storage::Ivar(n) => hir.push(HirNode::IvarWrite(n.clone(), value)),
            Storage::ClassVar(n) => super::cvar_write(hir, n.clone(), value),
            Storage::Global(n) => hir.push(HirNode::GlobalWrite(n.clone(), value)),
            Storage::Const { scope, name } => hir.push(HirNode::ConstWrite {
                scope: scope.clone(),
                name: name.clone(),
                value,
            }),
            Storage::DynConst { tmp, name } => {
                let scope = hir.push(HirNode::LocalRead(tmp.clone()));
                hir.push(HirNode::DynConstWrite {
                    scope,
                    name: name.clone(),
                    value,
                })
            }
        }
    }
}

/// The scope half of a DYNAMIC constant-path target (`expr::NAME op= rhs`):
/// lowers `parent` and binds it to a hidden local, so the read half and the
/// write half consult ONE evaluation of it -- `registry_for(key)::CACHE ||= {}`
/// calls `registry_for` once, exactly as `bind_call_target_once` does for
/// `obj.attr op= rhs`. Returns `(bind statement, target)`; the caller
/// sequences the bind ahead of whatever its operator builds.
pub(crate) fn bind_dynamic_const_scope(
    result: &ParseResult,
    hir: &mut Hir,
    parent: &Node<'_>,
    name: String,
) -> PResult<(NodeId, Storage)> {
    let scope = lower_node(result, hir, parent)?;
    let tmp = hir.gensym("__cscope");
    let bind = hir.push(HirNode::LocalWrite(tmp.clone(), scope));
    Ok((bind, Storage::DynConst { tmp, name }))
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
        Storage::DynConst { tmp, name } => {
            let scope = hir.push(HirNode::LocalRead(tmp.clone()));
            hir.push(HirNode::DynConstRead {
                scope,
                name: name.clone(),
                lenient: true,
            })
        }
        // `@@x ||= v` gets the same leniency (an unassigned `@@x` reads nil
        // and the write defines it -- the memoization idiom); `+=`/`&&=`
        // keep the raising read. See `NodeFlag::LENIENT_CVAR_READ`.
        Storage::ClassVar(_) => {
            let read = target.read(hir);
            hir.set_flag(read, crate::hir::NodeFlag::LENIENT_CVAR_READ);
            read
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

/// Push a setter call reached from ASSIGNMENT SYNTAX, so the expression
/// evaluates to the right-hand side rather than to whatever the setter
/// answered. CRuby compiles `recv.x = v` to `setn` + `send` + `pop`
/// (`compile_attrasgn`), which leaves the value on the stack across the
/// call -- so a setter that answers `self` for chaining, or `false`, cannot
/// change what an assignment using it means, and `a = b[0] = 1` gives every
/// target the same value.
///
/// The right-hand side is captured IN PLACE, in the argument slot it
/// already occupies, so the receiver and the index arguments still evaluate
/// before it; only the read-back moves after the call.
///
/// Assignment syntax alone comes here. `s.send(:[]=, k, v)` and the
/// explicit `s.[]=(k, v)` are ordinary calls that keep the setter's own
/// return -- which is why this is applied per lowering site rather than
/// keyed off the setter's name.
pub(crate) fn push_assignment_call(hir: &mut Hir, mut call: HirNode) -> NodeId {
    let HirNode::Call { args, .. } = &call else {
        unreachable!("an assignment target lowers to a setter Call")
    };
    // No positional argument means no value to hand back -- unreachable from
    // assignment syntax, but the shape is not this function's to enforce.
    let Some(ArrayElem::Single(value)) = args.last() else {
        return hir.push(call);
    };
    let value = *value;
    let tmp = hir.gensym("__asgn");
    let write = hir.push(HirNode::LocalWrite(tmp.clone(), value));
    if let HirNode::Call { args, .. } = &mut call
        && let Some(slot) = args.last_mut()
    {
        *slot = ArrayElem::Single(write);
    }
    let send = hir.push(call);
    let read = hir.push(HirNode::LocalRead(tmp));
    hir.push(HirNode::Seq(vec![send, read]))
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
    push_assignment_call(
        hir,
        HirNode::Call {
            receiver: Some(write_recv),
            name: write_name.to_string(),
            args: vec![ArrayElem::Single(value)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        },
    )
}

/// One evaluate-once index binding of `arr[...] op= rhs` -- the hidden
/// local's name, and whether the source spelled it as a SPLAT (`self[*mask]
/// += x`, where the bound value is the ARRAY and both `[]`/`[]=` re-splat
/// it).
pub(crate) struct IdxTmp {
    name: String,
    splat: bool,
}

impl IdxTmp {
    fn read(&self, hir: &mut Hir) -> ArrayElem {
        let read = hir.push(HirNode::LocalRead(self.name.clone()));
        match self.splat {
            true => ArrayElem::Splat(read),
            false => ArrayElem::Single(read),
        }
    }
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
) -> PResult<(Vec<NodeId>, NodeId, String, Vec<IdxTmp>)> {
    let recv_expr = lower_node(result, hir, receiver)?;
    let recv_tmp = hir.gensym("__recv");
    let mut binds = vec![hir.push(HirNode::LocalWrite(recv_tmp.clone(), recv_expr))];
    // EVERY index gets its own binding, for the same reason the receiver does:
    // `h[i(), j()] += 1` must call each index expression exactly once, not once
    // per `[]`/`[]=` call. A splat index binds its INNER expression (the
    // array) and re-splats it on both calls.
    let mut idx_tmps = Vec::new();
    for index_node in index_args.arguments().iter() {
        let (splat, idx_expr) = match index_node.as_splat_node() {
            Some(s) => {
                let inner = s
                    .expression()
                    .ok_or("a bare `*` has no index expression to bind")?;
                (true, lower_node(result, hir, &inner)?)
            }
            None => (false, lower_node(result, hir, &index_node)?),
        };
        let idx_tmp = hir.gensym("__idx");
        binds.push(hir.push(HirNode::LocalWrite(idx_tmp.clone(), idx_expr)));
        idx_tmps.push(IdxTmp {
            name: idx_tmp,
            splat,
        });
    }
    let read_recv = hir.push(HirNode::LocalRead(recv_tmp.clone()));
    let args = idx_tmps.iter().map(|t| t.read(hir)).collect();
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
    idx_tmps: &[IdxTmp],
    value: NodeId,
) -> NodeId {
    let write_recv = hir.push(HirNode::LocalRead(recv_tmp.to_string()));
    let mut args: Vec<ArrayElem> = idx_tmps.iter().map(|t| t.read(hir)).collect();
    args.push(ArrayElem::Single(value));
    push_assignment_call(
        hir,
        HirNode::Call {
            receiver: Some(write_recv),
            name: "[]=".to_string(),
            args,
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        },
    )
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
        if hir.cvar_is_toplevel() {
            // `@@a, @@b = 1, 2` written outside any class body. Borrowing the
            // `Call` target shape (rather than growing a variant every walker
            // would have to learn) reproduces Ruby's ordering for free: the
            // slot's value is distributed into `tmp_name` first, and only then
            // does this target's "write" -- the raise -- run, so the targets to
            // its LEFT have already been assigned.
            return Ok(MultiTarget::Call {
                write_call: super::cvar_toplevel_raise(hir),
                tmp_name: hir.gensym("__mval"),
            });
        }
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
        if arg_list.is_empty() {
            return Err("`arr[] = ...` as a multi-assignment target needs an index".into());
        }
        // ANY index count: `[]=` is an ordinary method, so `grid[x, y], b =
        // ...` is a three-argument `[]=` -- the same rule `index_arguments`
        // states for compound assignment. A splat index rides along as the
        // splat it is.
        let mut args: Vec<ArrayElem> = Vec::new();
        for a in &arg_list {
            args.push(match a.as_splat_node() {
                Some(s) => {
                    let inner = s
                        .expression()
                        .ok_or("a bare `*` has no index expression to bind")?;
                    ArrayElem::Splat(lower_node(result, hir, &inner)?)
                }
                None => ArrayElem::Single(lower_node(result, hir, a)?),
            });
        }
        let tmp_name = hir.gensym("__mval");
        let tmp_read = hir.push(HirNode::LocalRead(tmp_name.clone()));
        args.push(ArrayElem::Single(tmp_read));
        let write_call = hir.push(HirNode::Call {
            receiver: Some(receiver),
            name: "[]=".to_string(),
            args,
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
        // A bare trailing comma with no splat -- `a, = rhs`, `a, b, = rhs` --
        // is prism's `ImplicitRestNode`: an anonymous rest that takes the
        // `before` elements and discards the tail. Identical meaning to a
        // written `a, *_ = rhs` / `a, * = rhs`, so it lowers to the same
        // anonymous-splat `Some(None)`.
        Some(n) if n.as_implicit_rest_node().is_some() => Some(None),
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

/// The assignment family of [`super::lower_node_inner`]'s recognizer chain:
/// variable reads/writes for every storage kind (local, `it`, ivar, cvar,
/// gvar, the `$1`/`` $` `` match references), the compound-assignment forms
/// (`+=`/`||=`/`&&=`) over each of them plus `obj.attr`/`arr[i]` targets,
/// and multi-assignment. `Ok(None)` = not this family's node.
pub(crate) fn try_lower(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<Option<NodeId>> {
    if let Some(lvr) = node.as_local_variable_read_node() {
        let name = String::from_utf8_lossy(lvr.name().as_slice()).into_owned();
        return Ok(Some(hir.push(HirNode::LocalRead(name))));
    }

    // A bare `it` inside a block body (implicit-parameter sugar, distinct
    // from an ordinary local read at the `ruby-prism` level) -- matches the
    // synthesized `it` required-param name `lower_block` binds for
    // `ItParametersNode` blocks.
    if node.as_it_local_variable_read_node().is_some() {
        return Ok(Some(hir.push(HirNode::LocalRead("it".to_string()))));
    }

    if let Some(lvw) = node.as_local_variable_write_node() {
        let name = String::from_utf8_lossy(lvw.name().as_slice()).into_owned();
        let value = lower_node(result, hir, &lvw.value())?;
        return Ok(Some(hir.push(HirNode::LocalWrite(name, value))));
    }

    if let Some(ivr) = node.as_instance_variable_read_node() {
        let name = String::from_utf8_lossy(ivr.name().as_slice()).into_owned();
        return Ok(Some(hir.push(HirNode::IvarRead(
            name.trim_start_matches('@').to_string(),
        ))));
    }

    if let Some(ivw) = node.as_instance_variable_write_node() {
        let name = String::from_utf8_lossy(ivw.name().as_slice()).into_owned();
        let value = lower_node(result, hir, &ivw.value())?;
        return Ok(Some(hir.push(HirNode::IvarWrite(
            name.trim_start_matches('@').to_string(),
            value,
        ))));
    }

    // `x += 1` / `@x += 1` / `@@x += 1` / `$x += 1` / `X += 1` -- desugars to
    // a plain read-operator-write, e.g. `x = x + 1`, reusing the existing
    // `*Read`/`*Write` + operator `Call` dispatch infrastructure entirely (no
    // new HIR node needed for the operator form itself, exactly like
    // `unless`/ternary reuse `If`). `||=`/`&&=` desugar to `Or`/`And` over the
    // same read/write pair (`a ||= b` is `a || (a = b)`, NOT `a = a || b` --
    // the RHS/assignment must not even be EVALUATED when `a` is already
    // truthy, which `HirNode::Or`'s existing short-circuit codegen already
    // gives for free). See `Storage`'s docs for why every one of these five
    // storage kinds shares this one desugar instead of five near-identical
    // repetitions.
    if let Some(op) = node.as_local_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_compound_op_write(
            hir,
            Storage::Local(name),
            op_name,
            rhs,
        )));
    }
    if let Some(op) = node.as_local_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_and_write(hir, Storage::Local(name), rhs)));
    }
    if let Some(op) = node.as_local_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_or_write(hir, Storage::Local(name), rhs)));
    }
    if let Some(op) = node.as_instance_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_compound_op_write(
            hir,
            Storage::Ivar(name),
            op_name,
            rhs,
        )));
    }
    if let Some(op) = node.as_instance_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_and_write(hir, Storage::Ivar(name), rhs)));
    }
    if let Some(op) = node.as_instance_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_or_write(hir, Storage::Ivar(name), rhs)));
    }
    if let Some(op) = node.as_class_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_compound_op_write(
            hir,
            Storage::ClassVar(name),
            op_name,
            rhs,
        )));
    }
    if let Some(op) = node.as_class_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_and_write(hir, Storage::ClassVar(name), rhs)));
    }
    if let Some(op) = node.as_class_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_or_write(hir, Storage::ClassVar(name), rhs)));
    }
    if let Some(op) = node.as_global_variable_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_compound_op_write(
            hir,
            Storage::Global(name),
            op_name,
            rhs,
        )));
    }
    if let Some(op) = node.as_global_variable_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_and_write(hir, Storage::Global(name), rhs)));
    }
    if let Some(op) = node.as_global_variable_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_or_write(hir, Storage::Global(name), rhs)));
    }
    if let Some(nref) = node.as_numbered_reference_read_node() {
        return Ok(Some(hir.push(HirNode::LastMatchRef(LastMatch::Group(
            nref.number() as usize,
        )))));
    }
    // `` $` ``, `$&`, `$'` -- one node kind for all three, told apart by
    // name. (`$~` itself arrives as an ordinary global read, handled below.)
    if let Some(bref) = node.as_back_reference_read_node() {
        let name = String::from_utf8_lossy(bref.name().as_slice()).into_owned();
        let which = match name.as_str() {
            "$&" => LastMatch::Group(0),
            "$`" => LastMatch::Pre,
            "$'" => LastMatch::Post,
            "$+" => LastMatch::LastGroup,
            other => {
                return Err(format!(
                    "the `{other}` back-reference global isn't supported yet (zeo limitation)"
                )
                .into());
            }
        };
        return Ok(Some(hir.push(HirNode::LastMatchRef(which))));
    }
    if let Some(gvr) = node.as_global_variable_read_node() {
        let name = String::from_utf8_lossy(gvr.name().as_slice()).into_owned();
        // `$~` reads the last-match slot, not the `$foo` table -- see
        // `HirNode::LastMatchRef`.
        if name == "$~" {
            return Ok(Some(hir.push(HirNode::LastMatchRef(LastMatch::Data))));
        }
        return Ok(Some(hir.push(HirNode::GlobalRead(name))));
    }
    if let Some(gvw) = node.as_global_variable_write_node() {
        let name = String::from_utf8_lossy(gvw.name().as_slice()).into_owned();
        let value = lower_node(result, hir, &gvw.value())?;
        return Ok(Some(hir.push(HirNode::GlobalWrite(name, value))));
    }
    if let Some(op) = node.as_call_operator_write_node() {
        let recv = op
            .receiver()
            .ok_or("`+=` on a method call with no receiver isn't supported (zeo limitation)")?;
        let read_name = String::from_utf8_lossy(op.read_name().as_slice()).into_owned();
        let write_name = String::from_utf8_lossy(op.write_name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (bind, read_call, tmp) = bind_call_target_once(result, hir, &recv, &read_name)?;
        let combined = hir.push(HirNode::Call {
            receiver: Some(read_call),
            name: op_name,
            args: vec![ArrayElem::Single(rhs)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        });
        let write_call = build_call_target_write(hir, &tmp, &write_name, combined);
        return Ok(Some(hir.push(HirNode::Seq(vec![bind, write_call]))));
    }
    if let Some(op) = node.as_call_and_write_node() {
        let recv = op
            .receiver()
            .ok_or("`&&=` on a method call with no receiver isn't supported (zeo limitation)")?;
        let read_name = String::from_utf8_lossy(op.read_name().as_slice()).into_owned();
        let write_name = String::from_utf8_lossy(op.write_name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (bind, read_call, tmp) = bind_call_target_once(result, hir, &recv, &read_name)?;
        let write_call = build_call_target_write(hir, &tmp, &write_name, rhs);
        let and_node = hir.push(HirNode::And(read_call, write_call));
        return Ok(Some(hir.push(HirNode::Seq(vec![bind, and_node]))));
    }
    if let Some(op) = node.as_call_or_write_node() {
        let recv = op
            .receiver()
            .ok_or("`||=` on a method call with no receiver isn't supported (zeo limitation)")?;
        let read_name = String::from_utf8_lossy(op.read_name().as_slice()).into_owned();
        let write_name = String::from_utf8_lossy(op.write_name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (bind, read_call, tmp) = bind_call_target_once(result, hir, &recv, &read_name)?;
        let write_call = build_call_target_write(hir, &tmp, &write_name, rhs);
        let or_node = hir.push(HirNode::Or(read_call, write_call));
        return Ok(Some(hir.push(HirNode::Seq(vec![bind, or_node]))));
    }

    // `arr[i] += rhs` / `arr[i] ||= rhs` / `arr[i] &&= rhs` -- same
    // evaluate-once reasoning as the `obj.attr` forms above, extended to
    // BOTH the receiver and the (single) index argument (`arr[compute_idx()]
    // += 1` must call `compute_idx()` exactly once too). See
    // `bind_index_target_once`'s docs.
    if let Some(op) = node.as_index_operator_write_node() {
        let recv = op.receiver().ok_or(
            "`+=` on an indexing expression with no receiver isn't supported (zeo limitation)",
        )?;
        let idx = index_arguments(op.arguments())?;
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmps) =
            bind_index_target_once(result, hir, &recv, &idx)?;
        let combined = hir.push(HirNode::Call {
            receiver: Some(read_call),
            name: op_name,
            args: vec![ArrayElem::Single(rhs)],
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        });
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmps, combined);
        let mut stmts = binds;
        stmts.push(write_call);
        return Ok(Some(hir.push(HirNode::Seq(stmts))));
    }
    if let Some(op) = node.as_index_and_write_node() {
        let recv = op.receiver().ok_or(
            "`&&=` on an indexing expression with no receiver isn't supported (zeo limitation)",
        )?;
        let idx = index_arguments(op.arguments())?;
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmps) =
            bind_index_target_once(result, hir, &recv, &idx)?;
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmps, rhs);
        let and_node = hir.push(HirNode::And(read_call, write_call));
        let mut stmts = binds;
        stmts.push(and_node);
        return Ok(Some(hir.push(HirNode::Seq(stmts))));
    }
    if let Some(op) = node.as_index_or_write_node() {
        let recv = op.receiver().ok_or(
            "`||=` on an indexing expression with no receiver isn't supported (zeo limitation)",
        )?;
        let idx = index_arguments(op.arguments())?;
        let rhs = lower_node(result, hir, &op.value())?;
        let (binds, read_call, recv_tmp, idx_tmps) =
            bind_index_target_once(result, hir, &recv, &idx)?;
        let write_call = build_index_target_write(hir, &recv_tmp, &idx_tmps, rhs);
        let or_node = hir.push(HirNode::Or(read_call, write_call));
        let mut stmts = binds;
        stmts.push(or_node);
        return Ok(Some(hir.push(HirNode::Seq(stmts))));
    }

    if let Some(cvar) = node.as_class_variable_read_node() {
        let name = String::from_utf8_lossy(cvar.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        return Ok(Some(cvar_read(hir, name)));
    }

    if let Some(cvar) = node.as_class_variable_write_node() {
        let name = String::from_utf8_lossy(cvar.name().as_slice())
            .trim_start_matches('@')
            .to_string();
        let value = lower_node(result, hir, &cvar.value())?;
        return Ok(Some(cvar_write(hir, name, value)));
    }

    // `a, b = 1, 2` / `a, *b, c = arr` / `(a, b), @x, $y, Z, obj.attr, arr[i]
    // = ...` -- see `MultiTargetGroup`/`lower_multi_target`'s docs for the
    // full generalized target shape.
    if let Some(mw) = node.as_multi_write_node() {
        let targets = lower_multi_target_group(result, hir, mw.lefts(), mw.rest(), mw.rights())?;
        let value = lower_node(result, hir, &mw.value())?;
        return Ok(Some(hir.push(HirNode::MultiWrite { targets, value })));
    }

    Ok(None)
}
