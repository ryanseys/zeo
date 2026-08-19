//! Escaping blocks: the `BlockFn` body, the cell plumbing, `proc_new`, and
//! the block-passing send with its catch_break landing. The slice's block
//! shape: at most one required parameter (nil-filled/extra-dropped, the
//! non-lambda rule; auto-splat and multi-param binding land at M1-2), no
//! `return`-from-proc, no bare `yield` inside a block.

use super::ctx::{Fx, Local, VALUE_SIZE};
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::analyze::{captures, class_query};
use crate::hir::{ArrayElem, HirNode, NodeId};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::Module;
use zeo_abi::abi::SignalKind;

/// The names an escaping block at `block` captures from the enclosing
/// scope, in deterministic order.
fn captured_names(fx: &Fx, block: NodeId) -> Result<Vec<String>, String> {
    let HirNode::Block { params, body } = &fx.an.compiler.hir[block] else {
        return fx.unsupported(block, "a non-literal block");
    };
    let caps = captures::block_captures(
        &fx.an.compiler,
        params,
        body,
        class_query::SelfClass::new(fx.method_class, None),
    );
    let mut names: Vec<String> = caps
        .locals
        .iter()
        .filter(|n| fx.locals.contains_key(*n))
        .cloned()
        .collect();
    names.sort();
    for n in &names {
        if fx.shadowed.contains(n) {
            return fx.unsupported(
                block,
                "a block capturing a fused-loop parameter (the shadow dies with the loop)",
            );
        }
        if !matches!(fx.locals.get(n), Some(Local::Cell { .. })) {
            panic!("ICE: captured local `{n}` was hoisted as a plain slot");
        }
    }
    Ok(names)
}

/// Build the block's `RProc` into a fresh slot; the caller passes it as
/// the moved-in `blk`.
pub(crate) fn build_proc(
    fx: &mut Fx,
    site: NodeId,
    block: NodeId,
) -> Result<(ir::StackSlot, Vec<String>), String> {
    let names = captured_names(fx, block)?;
    let arity = {
        let HirNode::Block { params, .. } = &fx.an.compiler.hir[block] else {
            unreachable!("checked in captured_names");
        };
        params.required.len()
    };
    let f_id = define_block_fn(fx, site, block, &names)?;
    let f_ref = fx.em.module.declare_func_in_func(f_id, fx.b.func);
    let ptr_ty = fx.em.ptr;
    let f_addr = fx.b.ins().func_addr(ptr_ty, f_ref);

    // The cell-pointer array the runtime copies (retaining each).
    let cells_slot = (!names.is_empty()).then(|| {
        fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            names.len() as u32 * 8,
            3,
        ))
    });
    for (i, name) in names.iter().enumerate() {
        let Some(&Local::Cell { ss, .. }) = fx.locals.get(name) else {
            unreachable!("checked in captured_names");
        };
        let ptr = fx.cell_ptr(ss);
        let dst = fx.slot_addr(cells_slot.expect("names non-empty"), (i * 8) as i32);
        fx.b.ins().store(MemFlagsData::trusted(), ptr, dst, 0);
    }
    let cells_ptr = match cells_slot {
        Some(ss) => fx.slot_addr(ss, 0),
        None => fx.b.ins().iconst(ptr_ty, 0),
    };
    let n_cells = fx.b.ins().iconst(ptr_ty, names.len() as i64);
    let self_ptr = fx.self_ptr.expect("self_ptr is set in the prologue");
    let null = fx.b.ins().iconst(ptr_ty, 0);
    let arity_v = fx.b.ins().iconst(types::I32, arity as i64);
    let flags = fx.b.ins().iconst(types::I32, 0);
    let proc_ss = fx.temp_slot();
    let proc_addr = fx.slot_addr(proc_ss, 0);
    fx.call(
        "zeo_rt_proc_new",
        &[
            f_addr, cells_ptr, n_cells, self_ptr, null, null, arity_v, flags, proc_addr,
        ],
    );
    // The proc is owned until a send/call consumes it (moved-in blk).
    fx.owned_created += 1;
    Ok((proc_ss, names))
}

/// The block body as a `BlockFn`: env cells become (unowned) cell locals,
/// params bind nil-filled/extra-dropped, `next` is the ok-exit, `break`
/// arms the Break signal.
fn define_block_fn(
    fx: &mut Fx,
    site: NodeId,
    block: NodeId,
    captured: &[String],
) -> Result<cranelift_module::FuncId, String> {
    let HirNode::Block { params, body } = &fx.an.compiler.hir[block] else {
        unreachable!("checked in captured_names");
    };
    if !(params.destructures.is_empty()
        && params.optional.is_empty()
        && params.rest.is_none()
        && !params.implicit_rest
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && params.block.is_none()
        && params.block_locals.is_empty()
        && params.implicit_block_locals.is_empty()
        && params.required.len() <= 1)
    {
        return fx.unsupported(site, "this block's parameter shape");
    }
    let block_params = params.required.clone();
    let body = body.clone();
    let label = format!("block in {}", fx.frame_label);
    let (line, file) = {
        let loc = fx.location(site);
        (
            loc.map_or(0, |(_, l)| l),
            fx.an.compiler.hir.files.first().map(|f| f.name.clone()),
        )
    };

    // A fresh function; the enclosing Fx lends its Emitter.
    let em = &mut *fx.em;
    let an = fx.an;
    let method_class = fx.method_class;

    let mut sig = em.module.make_signature();
    for _ in 0..6 {
        sig.params.push(AbiParam::new(em.ptr));
    }
    sig.returns.push(AbiParam::new(types::I32));
    let idx = em.next_fn_index();
    let f_id = em
        .module
        .declare_function(
            &format!("zeo_blk_{idx}"),
            cranelift_module::Linkage::Local,
            &sig,
        )
        .map_err(|e| format!("declaring a block fn: {e}"))?;

    let mut func = ir::Function::with_name_signature(UserFuncName::user(2, idx), sig);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let b = FunctionBuilder::new(&mut func, &mut fbc);
    let mut bfx = Fx::new(em, an, b, |em, b| {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let rodata_gv = em.module.declare_data_in_func(em.rodata_id, b.func);
        let syms_gv = em.module.declare_data_in_func(em.syms_id, b.func);
        let rodata = b.ins().symbol_value(em.ptr, rodata_gv);
        let syms = b.ins().symbol_value(em.ptr, syms_gv);
        (rodata, syms)
    });
    let entry = bfx.b.current_block().expect("entry is current");
    let ep: Vec<ir::Value> = bfx.b.block_params(entry).to_vec();
    let (env, self_p, argv, argc, _blk, out) = (ep[0], ep[1], ep[2], ep[3], ep[4], ep[5]);
    bfx.self_ptr = Some(self_p);
    bfx.method_class = method_class;
    bfx.frame_label = label.clone();

    // Env cells -> unowned cell locals.
    let fl = MemFlagsData::trusted();
    let cells_off = std::mem::offset_of!(zeo_rt::capi::ProcEnv, cells) as i32;
    let ptr_ty = bfx.em.ptr;
    let cells_base = bfx.b.ins().load(ptr_ty, fl, env, cells_off);
    for (i, name) in captured.iter().enumerate() {
        let cellp = bfx.b.ins().load(ptr_ty, fl, cells_base, (i * 8) as i32);
        let ss =
            bfx.b
                .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        let dst = bfx.slot_addr(ss, 0);
        bfx.b.ins().store(fl, cellp, dst, 0);
        bfx.locals
            .insert(name.clone(), Local::Cell { ss, owned: false });
    }

    // Params: nil-fill the missing, drop the extra (non-lambda binding).
    for (i, name) in block_params.iter().enumerate() {
        let ss = bfx.new_value_slot();
        let have = bfx
            .b
            .ins()
            .icmp_imm_u(IntCC::UnsignedGreaterThan, argc, i as i64);
        let bind = bfx.b.create_block();
        let cont = bfx.b.create_block();
        bfx.b.ins().brif(have, bind, &[], cont, &[]);
        bfx.b.switch_to_block(bind);
        let src = if i == 0 {
            argv
        } else {
            bfx.b.ins().iadd_imm_u(argv, (i as u32 * VALUE_SIZE) as i64)
        };
        let op = Operand::Ptr {
            addr: src,
            owned: false,
            tag: TagInfo::Unknown,
        };
        let dst = bfx.slot_addr(ss, 0);
        ownership::write_assign(&mut bfx, &op, dst);
        bfx.b.ins().jump(cont, &[]);
        bfx.b.switch_to_block(cont);
        if !captured.contains(name) {
            bfx.locals.insert(name.clone(), Local::Slot(ss));
        } else {
            // A captured own-param would need a cell binding per call;
            // outside the slice.
            return bfx.unsupported(site, "a block parameter captured by a nested block");
        }
    }
    // Other block-body locals.
    let mut locals = crate::analyze::local_storage::Locals::default();
    for &stmt in &body {
        crate::analyze::local_storage::collect_locals(&an.compiler, stmt, &mut locals);
    }
    for name in locals.names().to_vec() {
        if bfx.locals.contains_key(&name) {
            continue;
        }
        let ss = bfx.new_value_slot();
        bfx.locals.insert(name, Local::Slot(ss));
    }

    if let Some(file) = &file {
        let off = bfx.em.intern_rodata(file.as_bytes());
        let label_off = bfx.em.intern_rodata(label.as_bytes());
        let file_ptr = bfx.rod(off);
        let file_len = bfx.b.ins().iconst(ptr_ty, file.len() as i64);
        let label_ptr = bfx.rod(label_off);
        let label_len = bfx.b.ins().iconst(ptr_ty, label.len() as i64);
        let line_v = bfx.b.ins().iconst(types::I32, i64::from(line));
        bfx.call(
            "zeo_rt_frame_push",
            &[file_ptr, file_len, label_ptr, label_len, line_v, line_v],
        );
    }
    let status = bfx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints returns a status");
    bfx.fallible(status);

    let ret_ok = bfx.b.create_block();
    bfx.block_next = Some((out, ret_ok));
    super::stmt::lower_value_body_into(&mut bfx, &body, out)?;
    bfx.b.ins().jump(ret_ok, &[]);

    let has_frame = file.is_some();
    let epilogue = |bfx: &mut Fx, status: i64| {
        let local_slots: Vec<Local> = bfx.locals.values().copied().collect();
        for l in local_slots {
            match l {
                Local::Slot(ss) => {
                    let addr = bfx.slot_addr(ss, 0);
                    bfx.call("zeo_rt_release", &[addr]);
                }
                Local::Cell { owned: false, .. } => {}
                Local::Cell { ss, owned: true } => {
                    let ptr = bfx.cell_ptr(ss);
                    bfx.call("zeo_rt_cell_release", &[ptr]);
                }
            }
        }
        if has_frame {
            bfx.call("zeo_rt_frame_pop", &[]);
        }
        let code = bfx.b.ins().iconst(types::I32, status);
        bfx.b.ins().return_(&[code]);
    };
    bfx.b.switch_to_block(ret_ok);
    epilogue(&mut bfx, 0);
    let land = bfx.land;
    bfx.b.switch_to_block(land);
    epilogue(&mut bfx, 1);

    super::verify::check(&bfx, &label);
    let Fx { mut b, .. } = bfx;
    b.seal_all_blocks();
    b.finalize(cfg);
    em.record_clif(&label, &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(f_id, &mut ctx)
        .map_err(|e| format!("compiling {label}: {e}"))?;
    Ok(f_id)
}

/// A dynamic send carrying a literal block: build the proc, pass it moved,
/// and catch a Break -- the Break value IS the send's value (rustc's
/// `catch_break`).
pub(crate) fn block_send(
    fx: &mut Fx,
    site: NodeId,
    recv: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    block: NodeId,
) -> Result<Operand, String> {
    let (proc_ss, _names) = build_proc(fx, site, block)?;
    let recv_ptr = match recv {
        Some(r) => {
            let recv_op = super::expr::lower_expr(fx, r)?;
            let p = ownership::borrow_ptr(fx, &recv_op);
            if recv_op.owned() {
                ownership::pool_owned(fx, p, recv_op.tag());
            }
            (p, true)
        }
        None => (fx.self_ptr.expect("self_ptr is set in the prologue"), false),
    };
    let argv_ptr = super::call::build_argv(fx, site, args)?;
    let sym = fx.sym_id(name);
    let zero_box = fx.b.ins().iconst(types::I32, 0);
    let argc_v = fx.b.ins().iconst(fx.em.ptr, args.len() as i64);
    let blk_ptr = fx.slot_addr(proc_ss, 0);
    // The callee consumes the moved-in proc, error path included.
    fx.owned_consumed += 1;
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = if recv_ptr.1 {
        let caller = fx.b.ins().iconst(types::I32, 0);
        fx.call(
            "zeo_rt_send_value_explicit_in",
            &[
                zero_box, recv_ptr.0, sym, argv_ptr, argc_v, blk_ptr, caller, out,
            ],
        )
        .expect("send returns a status")
    } else {
        fx.call(
            "zeo_rt_send_value_in",
            &[zero_box, recv_ptr.0, sym, argv_ptr, argc_v, blk_ptr, out],
        )
        .expect("send returns a status")
    };
    // catch_break: a Break's value is the send's value; everything else
    // propagates.
    let ok = fx.b.create_block();
    let signalled = fx.b.create_block();
    fx.b.ins().brif(status, signalled, &[], ok, &[]);
    fx.b.switch_to_block(signalled);
    let kind = fx.call("zeo_rt_signal_kind", &[]).expect("kind answers");
    let is_break =
        fx.b.ins()
            .icmp_imm_u(IntCC::Equal, kind, i64::from(SignalKind::Break as u8));
    let take = fx.b.create_block();
    fx.b.ins().brif(is_break, take, &[], fx.land, &[]);
    fx.b.switch_to_block(take);
    fx.call("zeo_rt_signal_take", &[out]);
    fx.b.ins().jump(ok, &[]);
    fx.b.switch_to_block(ok);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}
