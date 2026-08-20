//! Calls routed through the refinements active at them.
//!
//! A refined site gives up its static dispatch: whether the refinement
//! applies depends on the receiver's RUNTIME class, so the whole call goes
//! through one runtime entry that tries the refined bodies and then falls
//! back to an ordinary send. That is a real cost, paid only where a `using`
//! and a refined name actually meet -- which is a handful of sites in the
//! rare program that refines at all.

use cranelift_codegen::ir::{InstBuilder, types};

use crate::compiler::ClassId;
use crate::hir::{ArrayElem, HirNode, KwArg, NodeId};

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;

/// The `(target, holder, singleton)` triples this site must hand the
/// runtime: the refinements a `using` activates over the site's span,
/// narrowed to those that actually define `name`. Empty for every other
/// site, which is nearly all of them.
pub(crate) fn candidates(fx: &Fx, site: NodeId, name: &str) -> Vec<(ClassId, ClassId, bool)> {
    fx.an
        .compiler
        .refinements_active_at(site)
        .into_iter()
        .filter(|&(_, holder, _)| fx.an.compiler.refinement_defines(holder, name))
        .collect()
}

/// The refined lowering for one `HirNode::Call`, or `None` when no
/// refinement covers it.
pub(crate) fn refined_call(fx: &mut Fx, site: NodeId) -> Result<Option<Operand>, String> {
    let HirNode::Call {
        receiver,
        name,
        args,
        kwargs,
        block,
        block_arg,
        safe,
    } = &fx.an.compiler.hir[site]
    else {
        return Ok(None);
    };
    let (receiver, name, args, kwargs, block, block_arg, safe) = (
        *receiver,
        name.clone(),
        args.clone(),
        kwargs.clone(),
        *block,
        *block_arg,
        *safe,
    );
    // The three shapes that name a method at RUNTIME. Real ruby honours a
    // refinement through every one of them (only `Module#instance_methods`
    // stays blind), so at a site any `using` covers they hand the WHOLE
    // active set to the runtime -- which name is being asked about is not a
    // compile-time fact, and neither is whether the receiver's own chain
    // shadows the entry.
    let active = fx.an.compiler.refinements_active_at(site);
    if !active.is_empty()
        && let Some(entry) = reflect_entry(&name, &args, &kwargs, block, block_arg)
    {
        return lower_reflect(fx, site, receiver, entry, &args, block, &active).map(Some);
    }
    let cands = candidates(fx, site, &name);
    if cands.is_empty() {
        return Ok(None);
    }
    // The refined entry takes one flat argument list, so a splat -- whose
    // element count only the run time knows -- has no place to go yet.
    if args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
        || kwargs.iter().any(|k| matches!(k, KwArg::DoubleSplat(_)))
    {
        return fx.unsupported(site, "a splat at a refined call").map(Some);
    }
    if safe {
        return fx
            .unsupported(site, "a refined call through `&.`")
            .map(Some);
    }
    lower(
        fx, site, receiver, &name, &args, &kwargs, block, block_arg, &cands,
    )
    .map(Some)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one refined-call entry: the call's whole written shape plus the candidate set the site's `using` decided"
)]
fn lower(
    fx: &mut Fx,
    site: NodeId,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    cands: &[(ClassId, ClassId, bool)],
) -> Result<Operand, String> {
    // An EXPLICIT receiver runs the visibility check the refined entry
    // makes: a `private def` inside a `refine` block refuses one.
    let explicit = receiver.is_some_and(|r| !matches!(fx.an.compiler.hir[r], HirNode::SelfRef));
    let recv_ptr = match receiver {
        Some(r) => {
            let op = super::expr::lower_expr(fx, r)?;
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.self_ptr.expect("self_ptr is set in the prologue"),
    };
    let argv = super::call::build_argv(fx, site, args)?;
    let kw = if kwargs.is_empty() {
        fx.b.ins().iconst(fx.em.ptr, 0)
    } else {
        super::call::build_hash(fx, kwargs)?
    };
    let blk = match (block, block_arg) {
        (None, None) => None,
        (Some(b), None) => Some(super::blocks::literal_block_ptr(fx, site, b)?),
        (None, Some(ba)) => Some(super::blocks::block_arg_ptr(fx, ba)?),
        (Some(_), Some(_)) => {
            return fx.unsupported(site, "a literal block beside a `&` block argument");
        }
    };
    let (ids_ptr, n_ids) = candidate_table(fx, cands);
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let blk_ptr = blk.unwrap_or_else(|| fx.b.ins().iconst(fx.em.ptr, 0));
    let explicit_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(explicit)));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_refined_send_in",
            &[
                zero_box, recv_ptr, sym, argv, argc, kw, blk_ptr, ids_ptr, n_ids, explicit_v, out,
            ],
        )
        .expect("refined_send_in returns a status");
    if blk.is_some() {
        super::blocks::catch_break(fx, status, out);
    } else {
        fx.fallible(status);
    }
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The candidate set as one 4-aligned `.rodata` `u32` triple array.
fn candidate_table(
    fx: &mut Fx,
    cands: &[(ClassId, ClassId, bool)],
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let mut bytes = Vec::with_capacity(cands.len() * 12);
    for &(target, holder, singleton) in cands {
        bytes.extend_from_slice(&target.0.to_le_bytes());
        bytes.extend_from_slice(&holder.0.to_le_bytes());
        bytes.extend_from_slice(&u32::from(singleton).to_le_bytes());
    }
    let off = fx.em.intern_rodata_aligned(&bytes, 4);
    let ptr = fx.rod(off);
    let n = fx.b.ins().iconst(fx.em.ptr, cands.len() as i64);
    (ptr, n)
}

/// The `zeo_abi::abi::REFLECT_*` entry a written name and argument shape
/// asks for, or `None` when the site is an ordinary call.
fn reflect_entry(
    name: &str,
    args: &[ArrayElem],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> Option<u8> {
    use zeo_abi::abi::{REFLECT_METHOD, REFLECT_PUBLIC_SEND, REFLECT_RESPOND_TO, REFLECT_SEND};
    if !kwargs.is_empty()
        || block_arg.is_some()
        || args.is_empty()
        || args.iter().any(|a| matches!(a, ArrayElem::Splat(_)))
    {
        return None;
    }
    let no_block = block.is_none();
    Some(match name {
        "send" | "__send__" => REFLECT_SEND,
        "public_send" => REFLECT_PUBLIC_SEND,
        "respond_to?" if args.len() <= 2 && no_block => REFLECT_RESPOND_TO,
        "method" if args.len() == 1 && no_block => REFLECT_METHOD,
        _ => return None,
    })
}

fn lower_reflect(
    fx: &mut Fx,
    site: NodeId,
    receiver: Option<NodeId>,
    entry: u8,
    args: &[ArrayElem],
    block: Option<NodeId>,
    active: &[(ClassId, ClassId, bool)],
) -> Result<Operand, String> {
    let recv_ptr = match receiver {
        Some(r) => {
            let op = super::expr::lower_expr(fx, r)?;
            let p = ownership::borrow_ptr(fx, &op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.self_ptr.expect("self_ptr is set in the prologue"),
    };
    let argv = super::call::build_argv(fx, site, args)?;
    let blk = match block {
        Some(b) => Some(super::blocks::literal_block_ptr(fx, site, b)?),
        None => None,
    };
    let (ids_ptr, n_ids) = candidate_table(fx, active);
    let zero_box = fx.box_v();
    let entry_v = fx.b.ins().iconst(types::I8, i64::from(entry));
    let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let blk_ptr = blk.unwrap_or_else(|| fx.b.ins().iconst(fx.em.ptr, 0));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_reflect_dispatch_in",
            &[
                zero_box, recv_ptr, entry_v, argv, argc, blk_ptr, ids_ptr, n_ids, out,
            ],
        )
        .expect("reflect_dispatch_in returns a status");
    if blk.is_some() {
        super::blocks::catch_break(fx, status, out);
    } else {
        fx.fallible(status);
    }
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}
