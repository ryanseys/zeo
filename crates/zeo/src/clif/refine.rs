//! Calls routed through the refinements active at them.
//!
//! A refined site gives up its static dispatch: whether the refinement
//! applies depends on the receiver's RUNTIME class, so the whole call goes
//! through one runtime entry that tries the refined bodies and then falls
//! back to an ordinary send. That is a real cost, paid only where a `using`
//! and a refined name actually meet -- which is a handful of sites in the
//! rare program that refines at all.

use crate::codegen_error::CResult;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};

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
pub(crate) fn refined_call(fx: &mut Fx, site: NodeId) -> CResult<Option<Operand>> {
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
    let reflect_slots = fx.an.compiler.eval_activations_at(site);
    if !(active.is_empty() && reflect_slots.is_empty())
        && let Some(entry) = reflect_entry(&name, &args, &kwargs, block, block_arg)
    {
        let set = if reflect_slots.is_empty() {
            Cands::Static(&active)
        } else {
            Cands::Slots(&reflect_slots)
        };
        return lower_reflect(fx, site, receiver, entry, &args, block, set).map(Some);
    }
    // In a SNIPPET the candidates are a run-time question: the module a
    // `using` names is a run-time constant, and what it refines lives in
    // the running program's registry. The site carries the ACTIVATION
    // SLOTS the `using`s covering it filled instead.
    let slots = fx.an.compiler.eval_activations_at(site);
    let cands = candidates(fx, site, &name);
    if cands.is_empty() && slots.is_empty() {
        return Ok(None);
    }
    let set = if slots.is_empty() {
        Cands::Static(&cands)
    } else {
        Cands::Slots(&slots)
    };
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
    if !safe {
        return lower(
            fx, site, recv_ptr, explicit, &name, &args, &kwargs, block, block_arg, set,
        )
        .map(Some);
    }
    // `recv&.m` where a `using` covers `m`: the nil test is the only
    // difference. A nil receiver answers nil and evaluates neither the
    // arguments nor the block, so the whole refined call sits in the
    // non-nil branch -- the shape `expr`'s ordinary safe-navigation arm
    // uses.
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let b_nil = fx.b.create_block();
    let b_call = fx.b.create_block();
    let join = fx.b.create_block();
    let tv = fx.b.ins().load(
        types::I8,
        MemFlagsData::trusted(),
        recv_ptr,
        zeo_abi::abi::TAG_OFFSET as i32,
    );
    let is_nil = fx.b.ins().icmp_imm_u(IntCC::Equal, tv, 0);
    fx.b.ins().brif(is_nil, b_nil, &[], b_call, &[]);
    fx.b.switch_to_block(b_nil);
    ownership::write_move_into(fx, &Operand::Nil, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(b_call);
    let res = lower(
        fx, site, recv_ptr, explicit, &name, &args, &kwargs, block, block_arg, set,
    )?;
    ownership::write_move_into(fx, &res, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Some(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    }))
}

#[allow(
    clippy::too_many_arguments,
    reason = "one refined-call entry: the call's whole written shape plus the candidate set the site's `using` decided"
)]
fn lower(
    fx: &mut Fx,
    site: NodeId,
    recv_ptr: cranelift_codegen::ir::Value,
    explicit: bool,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[KwArg],
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
    cands: Cands<'_>,
) -> CResult<Operand> {
    // A splat's element count is a run-time number, so those arguments go
    // over as an Array and the runtime flattens them. Everything else stays
    // on the flat argv, which needs no allocation.
    let splatted = args.iter().any(|a| matches!(a, ArrayElem::Splat(_)));
    let argv = if splatted {
        super::call::build_array(fx, args)?
    } else {
        super::call::build_argv(fx, site, args)?
    };
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
    let (ids_ptr, n_ids) = cands.table(fx);
    let sym = fx.sym_id(name);
    let zero_box = fx.box_v();
    let blk_ptr = blk.unwrap_or_else(|| fx.b.ins().iconst(fx.em.ptr, 0));
    let explicit_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(explicit)));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = if splatted {
        // `ruby2_keywords`: a marked forwarder's splat keeps a trailing
        // hash's keyword mark, as on every other args-Array send.
        let unmark = fx.b.ins().iconst(types::I8, i64::from(!fx.ruby2_keywords));
        fx.call(
            cands.args_entry(),
            &[
                zero_box, recv_ptr, sym, argv, unmark, kw, blk_ptr, ids_ptr, n_ids, explicit_v, out,
            ],
        )
    } else {
        let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
        fx.call(
            cands.entry(),
            &[
                zero_box, recv_ptr, sym, argv, argc, kw, blk_ptr, ids_ptr, n_ids, explicit_v, out,
            ],
        )
    }
    .expect("a refined send returns a status");
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

/// Which refinements a site asks about: the set a whole-program compile
/// resolved, or -- in a snippet -- the activation slots a `using` fills
/// when it runs.
#[derive(Clone, Copy)]
enum Cands<'a> {
    Static(&'a [(ClassId, ClassId, bool)]),
    Slots(&'a [u32]),
}

impl Cands<'_> {
    fn entry(self) -> &'static str {
        match self {
            Cands::Static(_) => "zeo_rt_refined_send_in",
            Cands::Slots(_) => "zeo_rt_eval_refined_send",
        }
    }

    /// [`Cands::entry`] for a call whose arguments are a runtime-built Array.
    fn args_entry(self) -> &'static str {
        match self {
            Cands::Static(_) => "zeo_rt_refined_send_args_in",
            Cands::Slots(_) => "zeo_rt_eval_refined_send_args",
        }
    }

    fn reflect_entry(self) -> &'static str {
        match self {
            Cands::Static(_) => "zeo_rt_reflect_dispatch_in",
            Cands::Slots(_) => "zeo_rt_eval_reflect_dispatch",
        }
    }

    fn table(self, fx: &mut Fx) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        match self {
            Cands::Static(cands) => candidate_table(fx, cands),
            Cands::Slots(slots) => {
                let base = fx.using_base;
                let bytes: Vec<u8> = slots
                    .iter()
                    .flat_map(|s| (base + s).to_le_bytes())
                    .collect();
                let off = fx.em.intern_rodata_aligned(&bytes, 4);
                let ptr = fx.rod(off);
                let n = fx.b.ins().iconst(fx.em.ptr, slots.len() as i64);
                (ptr, n)
            }
        }
    }
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
    active: Cands<'_>,
) -> CResult<Operand> {
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
    let (ids_ptr, n_ids) = active.table(fx);
    let zero_box = fx.box_v();
    let entry_v = fx.b.ins().iconst(types::I8, i64::from(entry));
    let argc = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let blk_ptr = blk.unwrap_or_else(|| fx.b.ins().iconst(fx.em.ptr, 0));
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status(
        active.reflect_entry(),
        &[
            zero_box, recv_ptr, entry_v, argv, argc, blk_ptr, ids_ptr, n_ids, out,
        ],
    );
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
