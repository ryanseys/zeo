//! Trampolines: the `ValueFn`-shaped entry a dispatch row carries for a
//! compiled method -- arity check, block release (the callee consumes the
//! moved-in block; the M0 slice's methods take none), then the direct
//! call with per-argument pointers. The full `bind_params` machinery
//! (optionals, splats, kwargs) lands at M1-1.

use super::emit::Emitter;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, UserFuncName, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Module};

/// The `ValueFn` C signature.
pub(crate) fn value_fn_sig(em: &Emitter) -> ir::Signature {
    let mut sig = em.module.make_signature();
    for _ in 0..4 {
        sig.params.push(AbiParam::new(em.ptr));
    }
    // (recv, argv, argc, blk, out) -- argc is the middle param.
    sig.params.insert(2, AbiParam::new(em.ptr));
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// The direct-body C signature: `(self, p1..pn, out) -> i32`.
pub(crate) fn body_sig(em: &Emitter, arity: usize) -> ir::Signature {
    let mut sig = em.module.make_signature();
    for _ in 0..(arity + 2) {
        sig.params.push(AbiParam::new(em.ptr));
    }
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// Define the trampoline for a required-params-only method.
pub(crate) fn define_trampoline(
    em: &mut Emitter,
    tramp: FuncId,
    body: FuncId,
    arity: usize,
    fn_index: u32,
) -> Result<(), String> {
    let sig = value_fn_sig(em);
    let mut func = ir::Function::with_name_signature(UserFuncName::user(1, fn_index), sig);
    let body_ref = em.module.declare_func_in_func(body, &mut func);
    let release_id = em.import("zeo_rt_release");
    let release = em.module.declare_func_in_func(release_id, &mut func);
    let wrong_id = em.import("zeo_rt_wrong_arity");
    let wrong = em.module.declare_func_in_func(wrong_id, &mut func);

    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fbc);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let recv = b.block_params(entry)[0];
    let argv = b.block_params(entry)[1];
    let argc = b.block_params(entry)[2];
    let blk = b.block_params(entry)[3];
    let out = b.block_params(entry)[4];

    // The block was MOVED in; a method that declares none consumes it by
    // releasing (Ruby: an unused block is simply ignored).
    let has_blk = b.ins().icmp_imm_u(IntCC::NotEqual, blk, 0);
    let do_release = b.create_block();
    let arity_check = b.create_block();
    b.ins().brif(has_blk, do_release, &[], arity_check, &[]);
    b.switch_to_block(do_release);
    b.ins().call(release, &[blk]);
    b.ins().jump(arity_check, &[]);

    b.switch_to_block(arity_check);
    let ok = b.create_block();
    let bad = b.create_block();
    let right = b.ins().icmp_imm_u(IntCC::Equal, argc, arity as i64);
    b.ins().brif(right, ok, &[], bad, &[]);

    b.switch_to_block(bad);
    let n = b.ins().iconst(em.ptr, arity as i64);
    let call = b.ins().call(wrong, &[argc, n, n]);
    let status = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[status]);

    b.switch_to_block(ok);
    let mut args = Vec::with_capacity(arity + 2);
    args.push(recv);
    for i in 0..arity {
        let p = if i == 0 {
            argv
        } else {
            b.ins()
                .iadd_imm_u(argv, i64::from(i as u32 * super::ctx::VALUE_SIZE))
        };
        args.push(p);
    }
    args.push(out);
    let call = b.ins().call(body_ref, &args);
    let status = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[status]);

    b.seal_all_blocks();
    b.finalize(cfg);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(tramp, &mut ctx)
        .map_err(|e| format!("compiling a trampoline: {e}"))
}

/// An `attr_reader`/`attr_writer` trampoline -- the slot access IS the
/// method (no body fn; the rustc backend's `zeo_tramp!(rd/wr)` heads).
pub(crate) fn define_accessor(
    em: &mut Emitter,
    tramp: FuncId,
    slot: usize,
    kind: crate::compiler::AccessorKind,
    fn_index: u32,
) -> Result<(), String> {
    let sig = value_fn_sig(em);
    let mut func = ir::Function::with_name_signature(UserFuncName::user(1, fn_index), sig);
    let release_id = em.import("zeo_rt_release");
    let release = em.module.declare_func_in_func(release_id, &mut func);
    let wrong_id = em.import("zeo_rt_wrong_arity");
    let wrong = em.module.declare_func_in_func(wrong_id, &mut func);
    let get_id = em.import("zeo_rt_ivar_get_slot");
    let get = em.module.declare_func_in_func(get_id, &mut func);
    let (frozen, set, retain) = match kind {
        crate::compiler::AccessorKind::Writer => {
            let frozen_id = em.import("zeo_rt_frozen_check");
            let frozen = em.module.declare_func_in_func(frozen_id, &mut func);
            let set_id = em.import("zeo_rt_ivar_set_slot");
            let set = em.module.declare_func_in_func(set_id, &mut func);
            let retain_id = em.import("zeo_rt_retain");
            let retain = em.module.declare_func_in_func(retain_id, &mut func);
            (Some(frozen), Some(set), Some(retain))
        }
        crate::compiler::AccessorKind::Reader => (None, None, None),
    };

    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fbc);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let recv = b.block_params(entry)[0];
    let argv = b.block_params(entry)[1];
    let argc = b.block_params(entry)[2];
    let blk = b.block_params(entry)[3];
    let out = b.block_params(entry)[4];

    let has_blk = b.ins().icmp_imm_u(IntCC::NotEqual, blk, 0);
    let do_release = b.create_block();
    let arity_check = b.create_block();
    b.ins().brif(has_blk, do_release, &[], arity_check, &[]);
    b.switch_to_block(do_release);
    b.ins().call(release, &[blk]);
    b.ins().jump(arity_check, &[]);

    b.switch_to_block(arity_check);
    let want = match kind {
        crate::compiler::AccessorKind::Reader => 0,
        crate::compiler::AccessorKind::Writer => 1,
    };
    let ok = b.create_block();
    let bad = b.create_block();
    let right = b.ins().icmp_imm_u(IntCC::Equal, argc, want);
    b.ins().brif(right, ok, &[], bad, &[]);
    b.switch_to_block(bad);
    let n = b.ins().iconst(em.ptr, want);
    let call = b.ins().call(wrong, &[argc, n, n]);
    let status = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[status]);

    b.switch_to_block(ok);
    let slot_v = b.ins().iconst(em.ptr, slot as i64);
    match kind {
        crate::compiler::AccessorKind::Reader => {
            b.ins().call(get, &[recv, slot_v, out]);
            let zero = b.ins().iconst(types::I32, 0);
            b.ins().return_(&[zero]);
        }
        crate::compiler::AccessorKind::Writer => {
            // Frozen first (the rustc writer's order); then TWO fresh
            // references off the caller's borrowed argv[0]: one moves into
            // the slot, one is the returned value.
            let frozen = frozen.expect("writer imports frozen_check");
            let set = set.expect("writer imports ivar_set_slot");
            let retain = retain.expect("writer imports retain");
            let call = b.ins().call(frozen, &[recv]);
            let status = b.func.dfg.inst_results(call)[0];
            let go = b.create_block();
            let sig_out = b.create_block();
            b.ins().brif(status, sig_out, &[], go, &[]);
            b.switch_to_block(sig_out);
            let one = b.ins().iconst(types::I32, 1);
            b.ins().return_(&[one]);
            b.switch_to_block(go);
            b.ins().call(retain, &[argv]);
            b.ins().call(retain, &[argv]);
            // out <- a bit-copy (owns one of the two retains).
            let fl = cranelift_codegen::ir::MemFlagsData::trusted();
            for off in [0, 8, 16] {
                let w = b.ins().load(types::I64, fl, argv, off);
                b.ins().store(fl, w, out, off);
            }
            // The slot consumes the other (ivar_set_slot MOVES from its
            // pointer; the caller's own reference stays untouched because
            // of the double retain).
            let call = b.ins().call(set, &[recv, slot_v, argv]);
            let status = b.func.dfg.inst_results(call)[0];
            b.ins().return_(&[status]);
        }
    }

    b.seal_all_blocks();
    b.finalize(cfg);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(tramp, &mut ctx)
        .map_err(|e| format!("compiling an accessor: {e}"))
}
