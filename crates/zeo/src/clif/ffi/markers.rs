//! The class-body markers `lower::ffi` desugars a deferred directive into:
//! an `ffi_lib` whose candidates only the running class body can evaluate,
//! and an `enum` whose members it fills at run time.

use cranelift_codegen::ir::{self, InstBuilder, types};

use crate::diagnostics::clif::CResult;
use crate::hir::{ArrayElem, HirNode, NodeId};

use super::super::ctx::Fx;
use super::super::expr::lower_expr;
use super::super::operand::{Operand, TagInfo};
use super::super::ownership;
use super::build_argv;

/// Is `name` one of the markers `lower::ffi` desugars a deferred
/// directive into? The names are reserved: no ruby source spells them.
pub(crate) fn is_marker(name: &str) -> bool {
    matches!(
        name,
        "__zeo_ffi_lib" | "__zeo_ffi_enum" | "__zeo_ffi_enum_get" | "__zeo_ffi_enum_put"
    )
}

/// One marker call, lowered where it stands.
pub(crate) fn marker_call(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> CResult<Operand> {
    let shape = || format!("the `{name}` marker in this shape");
    let ids: Option<Vec<NodeId>> = args
        .iter()
        .map(|a| match a {
            ArrayElem::Single(id) => Some(*id),
            ArrayElem::Splat(_) => None,
        })
        .collect();
    // `lower::ffi` writes every marker as a leading integer slot plus
    // plain arguments -- anything else is an internal error, not a program.
    let Some(ids) = ids else {
        return fx.unsupported(site, &shape());
    };
    let Some((&slot_id, rest)) = ids.split_first() else {
        return fx.unsupported(site, &shape());
    };
    let HirNode::IntegerLit(slot) = fx.an.compiler.hir[slot_id] else {
        return fx.unsupported(site, &shape());
    };
    let slot = slot as usize;
    match name {
        "__zeo_ffi_lib" if rest.len().is_multiple_of(2) => lib_store(fx, slot, rest),
        "__zeo_ffi_enum" => enum_store(fx, slot, rest),
        "__zeo_ffi_enum_get" | "__zeo_ffi_enum_put" if rest.len() == 1 => {
            enum_field(fx, slot, rest[0], name.ends_with("put"))
        }
        _ => fx.unsupported(site, &shape()),
    }
}

/// `__zeo_ffi_lib(slot, splatted?, expr, ...)`: the candidate expressions
/// evaluate here, in class-body order, and the runtime dlopens every value
/// EAGERLY -- so an unopenable library raises `LoadError` at this exact
/// statement, as CRuby's `ffi_lib` does.
fn lib_store(fx: &mut Fx, slot: usize, pairs: &[NodeId]) -> CResult<Operand> {
    use ir::{MemFlagsData, StackSlotData, StackSlotKind};
    let n = pairs.len() / 2;
    let splats = fx.b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        n.max(1) as u32,
        0,
    ));
    let values: Vec<NodeId> = pairs.chunks_exact(2).map(|p| p[1]).collect();
    for (i, pair) in pairs.chunks_exact(2).enumerate() {
        let splatted = matches!(fx.an.compiler.hir[pair[0]], HirNode::IntegerLit(1));
        let v = fx.b.ins().iconst(types::I8, i64::from(u8::from(splatted)));
        let at = fx.slot_addr(splats, i as i32);
        fx.b.ins().store(MemFlagsData::trusted(), v, at, 0);
    }
    let argv = build_argv(fx, &values)?;
    let splats_ptr = fx.slot_addr(splats, 0);
    let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
    let n_v = fx.b.ins().iconst(fx.em.ptr, n as i64);
    call_out(fx, "zeo_rt_ffi_lib_store", &[slot_v, argv, splats_ptr, n_v])
}

/// `__zeo_ffi_enum(slot, member, ...)`: the member list evaluates here and
/// lands in the slot every signature lowered under it reads.
fn enum_store(fx: &mut Fx, slot: usize, members: &[NodeId]) -> CResult<Operand> {
    let argv = build_argv(fx, members)?;
    let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
    let n_v = fx.b.ins().iconst(fx.em.ptr, members.len() as i64);
    call_out(fx, "zeo_rt_ffi_enum_store", &[slot_v, argv, n_v])
}

/// A deferred enum STRUCT FIELD's read (`int` -> Symbol) or write.
fn enum_field(fx: &mut Fx, slot: usize, value: NodeId, put: bool) -> CResult<Operand> {
    let op = lower_expr(fx, value)?;
    let ptr = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        let tag = op.tag();
        ownership::pool_owned(fx, ptr, tag);
    }
    let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
    let put_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(put)));
    call_out(fx, "zeo_rt_ffi_enum_field", &[slot_v, put_v, ptr])
}

/// A fallible runtime call whose last argument is the `out` slot.
fn call_out(fx: &mut Fx, name: &'static str, args: &[ir::Value]) -> CResult<Operand> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let mut all = args.to_vec();
    all.push(out);
    let status = fx.call_status(name, &all);
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}
