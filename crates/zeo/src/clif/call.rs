//! Call lowering: direct compiled->compiled calls (a receiverless call
//! naming a known compiled method with matching arity) and the uncached
//! dynamic sends. Inline caches (`CallSite` slots) are a later milestone;
//! every dynamic send here is the uncached entry -- correct, then fast.

use super::ctx::{Fx, VALUE_SIZE};
use super::expr::lower_expr;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::hir::{ArrayElem, NodeId};
use cranelift_codegen::ir::{InstBuilder, StackSlotData, StackSlotKind, types};
use cranelift_module::Module;

/// Evaluate plain positional args to borrowable pointers (owned temps hand
/// their value to the pool -- alive until frame pop / loop latch).
fn arg_ptrs(
    fx: &mut Fx,
    site: NodeId,
    args: &[ArrayElem],
) -> Result<Vec<cranelift_codegen::ir::Value>, String> {
    let mut ptrs = Vec::with_capacity(args.len());
    for arg in args {
        let ArrayElem::Single(id) = arg else {
            return fx.unsupported(site, "a splat argument");
        };
        let op = lower_expr(fx, *id)?;
        let p = ownership::borrow_ptr(fx, &op);
        if op.owned() {
            ownership::pool_owned(fx, p, op.tag());
        }
        ptrs.push(p);
    }
    Ok(ptrs)
}

/// A direct call to compiled method `name`: `(self, p1..pn, out)` through
/// the status protocol. `self_ptr` is the caller's own borrowed self.
pub(crate) fn direct_call(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> Result<Operand, String> {
    let ptrs = arg_ptrs(fx, site, args)?;
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let func_id = fx.em.methods[name].body;
    let fref = fx.em.module.declare_func_in_func(func_id, fx.b.func);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let mut call_args = Vec::with_capacity(ptrs.len() + 2);
    call_args.push(self_ptr);
    call_args.extend(ptrs);
    call_args.push(out);
    let inst = fx.b.ins().call(fref, &call_args);
    let status = fx.b.func.dfg.inst_results(inst)[0];
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// An explicit-receiver dynamic send through the uncached entry (the
/// visibility barrier's `caller` is `Object` -- the only lexical class the
/// slice compiles).
pub(crate) fn dynamic_send(
    fx: &mut Fx,
    site: NodeId,
    recv: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> Result<Operand, String> {
    let recv_op = lower_expr(fx, recv)?;
    dynamic_send_value(fx, site, recv_op, name, args)
}

/// `dynamic_send` on an already-lowered receiver (the `New` lowering hands
/// a Class value here).
pub(crate) fn dynamic_send_value(
    fx: &mut Fx,
    site: NodeId,
    recv_op: Operand,
    name: &str,
    args: &[ArrayElem],
) -> Result<Operand, String> {
    let recv_ptr = ownership::borrow_ptr(fx, &recv_op);
    if recv_op.owned() {
        ownership::pool_owned(fx, recv_ptr, recv_op.tag());
    }
    // A CONTIGUOUS argv array (the borrowed-copy shape `lower_puts` uses).
    let argc = args.len();
    let argv = (argc > 0).then(|| {
        fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            argc as u32 * VALUE_SIZE,
            3,
        ))
    });
    for (i, arg) in args.iter().enumerate() {
        let ArrayElem::Single(id) = arg else {
            return fx.unsupported(site, "a splat argument");
        };
        let op = lower_expr(fx, *id)?;
        if op.owned() {
            let tag = op.tag();
            let addr = ownership::addr_of(fx, &op);
            ownership::pool_owned(fx, addr, tag);
        }
        let argv = argv.expect("argc > 0 here");
        let dst = fx.slot_addr(argv, (i as u32 * VALUE_SIZE) as i32);
        ownership::write_borrow(fx, &op, dst);
    }
    let argv_ptr = match argv {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let sym = fx.sym_id(name);
    let zero_box = fx.b.ins().iconst(types::I32, 0);
    let argc_v = fx.b.ins().iconst(fx.em.ptr, argc as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let caller = fx.b.ins().iconst(types::I32, 0); // Object
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_send_value_explicit_in",
            &[zero_box, recv_ptr, sym, argv_ptr, argc_v, null, caller, out],
        )
        .expect("send returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}
