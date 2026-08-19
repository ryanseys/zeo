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
    block: Option<NodeId>,
) -> Result<Operand, String> {
    let decl_has_blk = fx.em.methods[name].has_blk;
    // A literal block on a method that never uses one is never invoked --
    // nothing to build (Ruby's own rule).
    let blk_ptr = match (block, decl_has_blk) {
        (Some(blk_node), true) => {
            let (proc_ss, _) = super::blocks::build_proc(fx, site, blk_node)?;
            // The callee consumes the moved-in proc.
            fx.owned_consumed += 1;
            Some(fx.slot_addr(proc_ss, 0))
        }
        (Some(_), false) => None,
        (None, true) => Some(fx.b.ins().iconst(fx.em.ptr, 0)),
        (None, false) => None,
    };
    let ptrs = arg_ptrs(fx, site, args)?;
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let func_id = fx.em.methods[name].body;
    let fref = fx.em.module.declare_func_in_func(func_id, fx.b.func);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let mut call_args = Vec::with_capacity(ptrs.len() + 3);
    call_args.push(self_ptr);
    call_args.extend(ptrs);
    if let Some(b) = blk_ptr {
        call_args.push(b);
    }
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

/// A receiverless dynamic send on the current `self` -- the implicit-call
/// mode (no visibility barrier: private methods answer).
pub(crate) fn implicit_send(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[ArrayElem],
) -> Result<Operand, String> {
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let argv_ptr = build_argv(fx, site, args)?;
    let sym = fx.sym_id(name);
    let zero_box = fx.b.ins().iconst(types::I32, 0);
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call(
            "zeo_rt_send_value_in",
            &[zero_box, self_ptr, sym, argv_ptr, argc_v, null, out],
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

/// A contiguous argv array of borrowed copies (owned temps hand their
/// value to the pool first). Null when empty.
pub(crate) fn build_argv(
    fx: &mut Fx,
    site: NodeId,
    args: &[ArrayElem],
) -> Result<cranelift_codegen::ir::Value, String> {
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
    Ok(match argv {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    })
}

/// A Hash from `KwArg` rows (a hash literal, or a call site's keyword
/// set), evaluated in written order -- key then value per pair, `**`
/// splats merged in place, later keys overwrite. The hash is pooled AT
/// CREATION (a `**` coercion can raise mid-build, and the error edge must
/// not strand an unpooled value), so the returned address is a BORROW --
/// alive until frame pop -- and later pairs mutate through the shared
/// handle.
pub(crate) fn build_hash(
    fx: &mut Fx,
    pairs: &[crate::hir::KwArg],
) -> Result<cranelift_codegen::ir::Value, String> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    fx.call("zeo_rt_hash_new", &[out]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, out, TagInfo::Known(zeo_abi::abi::ValueTag::Hash as u8));
    for kw in pairs {
        match kw {
            crate::hir::KwArg::Pair(k, v) => {
                let kop = lower_expr(fx, *k)?;
                let kptr = ownership::move_ptr(fx, &kop);
                let vop = lower_expr(fx, *v)?;
                let vptr = ownership::move_ptr(fx, &vop);
                fx.call("zeo_rt_hash_set", &[out, kptr, vptr]);
            }
            crate::hir::KwArg::DoubleSplat(e) => {
                let op = lower_expr(fx, *e)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx
                    .call("zeo_rt_kw_splat_into", &[out, p])
                    .expect("kw_splat_into returns a status");
                fx.fallible(status);
            }
        }
    }
    Ok(out)
}

/// An Array from `ArrayElem` rows (an array literal, or a splat-bearing
/// call site's arguments): singles pushed, splats expanded through the
/// runtime's `to_a` coercion (which can raise). Pooled at creation, same
/// reasoning as `build_hash` -- the returned address is a borrow.
pub(crate) fn build_array(
    fx: &mut Fx,
    args: &[ArrayElem],
) -> Result<cranelift_codegen::ir::Value, String> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let cap = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    fx.call("zeo_rt_array_new", &[cap, out]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, out, TagInfo::Known(zeo_abi::abi::ValueTag::Array as u8));
    for arg in args {
        match arg {
            ArrayElem::Single(id) => {
                let op = lower_expr(fx, *id)?;
                let p = ownership::move_ptr(fx, &op);
                fx.call("zeo_rt_array_push", &[out, p]);
            }
            ArrayElem::Splat(id) => {
                let op = lower_expr(fx, *id)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx
                    .call("zeo_rt_array_push_splat", &[out, p])
                    .expect("push_splat returns a status");
                fx.fallible(status);
            }
        }
    }
    Ok(out)
}

/// A splat-bearing dynamic send: args built as a runtime Array, keywords
/// (when present) as the kw Hash; the runtime entry unmarks the splat
/// tail (a splat-expanded hash is positional again -- `ruby2_keywords`
/// will pass 0 here when it lands) and appends the keywords.
pub(crate) fn splat_send(
    fx: &mut Fx,
    site: NodeId,
    recv: Option<Operand>,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
) -> Result<Operand, String> {
    let _ = site;
    let recv_ptr = match &recv {
        Some(op) => {
            let p = ownership::borrow_ptr(fx, op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.self_ptr.expect("self_ptr is set in the prologue"),
    };
    let args_ptr = build_array(fx, args)?;
    let kw_ptr = if kwargs.is_empty() {
        fx.b.ins().iconst(fx.em.ptr, 0)
    } else {
        build_hash(fx, kwargs)?
    };
    let sym = fx.sym_id(name);
    let zero_box = fx.b.ins().iconst(types::I32, 0);
    // `ruby2_keywords`' whole purpose: a marked forwarder's splat keeps a
    // trailing hash's keyword mark.
    let unmark = fx.b.ins().iconst(types::I8, i64::from(!fx.ruby2_keywords));
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = match recv {
        Some(_) => {
            let caller = fx.b.ins().iconst(types::I32, 0); // Object
            fx.call(
                "zeo_rt_send_value_explicit_args_in",
                &[
                    zero_box, recv_ptr, sym, args_ptr, unmark, kw_ptr, null, caller, out,
                ],
            )
        }
        None => fx.call(
            "zeo_rt_send_value_args_in",
            &[zero_box, recv_ptr, sym, args_ptr, unmark, kw_ptr, null, out],
        ),
    }
    .expect("splat sends return a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// A dynamic send WITH call-site keywords: positionals into argv, the
/// keyword Hash built here, the runtime's kw entry does the append-if-
/// non-empty. `recv` `None` = the implicit-self mode (private methods
/// answer); `Some` = the explicit entry behind the visibility barrier.
pub(crate) fn kw_send(
    fx: &mut Fx,
    site: NodeId,
    recv: Option<Operand>,
    name: &str,
    args: &[ArrayElem],
    kwargs: &[crate::hir::KwArg],
) -> Result<Operand, String> {
    let recv_ptr = match &recv {
        Some(op) => {
            let p = ownership::borrow_ptr(fx, op);
            if op.owned() {
                ownership::pool_owned(fx, p, op.tag());
            }
            p
        }
        None => fx.self_ptr.expect("self_ptr is set in the prologue"),
    };
    let argv_ptr = build_argv(fx, site, args)?;
    let kw_ptr = build_hash(fx, kwargs)?;
    let sym = fx.sym_id(name);
    let zero_box = fx.b.ins().iconst(types::I32, 0);
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = match recv {
        Some(_) => {
            let caller = fx.b.ins().iconst(types::I32, 0); // Object
            fx.call(
                "zeo_rt_send_value_explicit_kw_in",
                &[
                    zero_box, recv_ptr, sym, argv_ptr, argc_v, kw_ptr, null, caller, out,
                ],
            )
        }
        None => fx.call(
            "zeo_rt_send_value_kw_in",
            &[zero_box, recv_ptr, sym, argv_ptr, argc_v, kw_ptr, null, out],
        ),
    }
    .expect("kw sends return a status");
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
    let argv_ptr = build_argv(fx, site, args)?;
    let sym = fx.sym_id(name);
    let zero_box = fx.b.ins().iconst(types::I32, 0);
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
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
