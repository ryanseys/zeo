//! Trampolines: the `ValueFn`-shaped entry a dispatch row carries for a
//! compiled method. A PLAIN signature (required positionals only) keeps
//! the lean shape -- arity check, block release, direct call with argv
//! pointers. Everything else goes through `zeo_rt_bind_params`: the
//! runtime routes argv into a stack slot array from the method's `.rodata`
//! `ParamDescC` (kwargs peel, arity shapes, keyword errors -- one
//! implementation, the runtime's), and the trampoline passes per-slot
//! pointers (null = absent optional; its default runs in the body).

use super::emit::Emitter;
use crate::hir::{KeywordParam, Params};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, StackSlotData, StackSlotKind, UserFuncName, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Module};

/// The signature-slot facts a `hir::Params` implies -- shared by the body
/// signature, the trampoline, and the direct-call gate. Slot order:
/// required, optional, rest (when named), post, keywords (declared
/// order), kwrest (when named); the block channel is separate.
pub(crate) struct Layout {
    pub n_slots: usize,
    /// Bit `s` set = slot `s` is an optional binding (its body pointer may
    /// be null; the prologue runs the default).
    pub optional_mask: u64,
    /// Required positionals only (and no `**nil`): the lean trampoline
    /// suffices and a count-matched call site may go direct.
    pub plain: bool,
}

pub(crate) fn layout_of(p: &Params) -> Result<Layout, String> {
    let rest_named = matches!(p.rest, Some(Some(_)));
    let kwrest_named = matches!(p.keyword_rest, Some(Some(_)));
    let n_slots = p.required.len()
        + p.optional.len()
        + usize::from(rest_named)
        + p.post.len()
        + p.keywords.len()
        + usize::from(kwrest_named);
    if n_slots > 64 {
        return Err(format!(
            "a method with {n_slots} parameter slots overflows the 64-bit presence bitmap"
        ));
    }
    let mut mask = 0u64;
    let mut s = p.required.len();
    for _ in &p.optional {
        mask |= 1 << s;
        s += 1;
    }
    s += usize::from(rest_named) + p.post.len();
    for kw in &p.keywords {
        if matches!(kw, KeywordParam::Optional(..)) {
            mask |= 1 << s;
        }
        s += 1;
    }
    let plain = p.optional.is_empty()
        && p.rest.is_none()
        && p.post.is_empty()
        && p.keywords.is_empty()
        && p.keyword_rest.is_none()
        && !p.no_keywords;
    Ok(Layout {
        n_slots,
        optional_mask: mask,
        plain,
    })
}

/// Everything a trampoline definition needs beyond its `FuncId`s.
pub(crate) struct TrampSpec<'a> {
    pub tramp: FuncId,
    pub body: FuncId,
    pub params: &'a Params,
    pub has_blk: bool,
    /// The Ruby method name (binder error text).
    pub name: &'a str,
    /// The callee frame the binder's raises run under.
    pub file: Option<&'a str>,
    pub label: &'a str,
    pub line: u32,
    pub end_line: u32,
}

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

/// The direct-body C signature: `(self, p1..pn, [blk,] out) -> i32`. The
/// blk slot exists only when the scope uses a block (`yield`/
/// `block_given?`); it is MOVED in (null = none) and the body releases it.
pub(crate) fn body_sig(em: &Emitter, arity: usize, has_blk: bool) -> ir::Signature {
    let mut sig = em.module.make_signature();
    let n = arity + 2 + usize::from(has_blk);
    for _ in 0..n {
        sig.params.push(AbiParam::new(em.ptr));
    }
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// Define a method's trampoline: the lean shape for a plain signature,
/// the `bind_params` shape for everything else.
pub(crate) fn define_trampoline(
    em: &mut Emitter,
    spec: &TrampSpec<'_>,
    fn_index: u32,
) -> Result<(), String> {
    let layout = layout_of(spec.params)?;
    if layout.plain {
        define_plain_trampoline(
            em,
            spec.tramp,
            spec.body,
            spec.params.required.len(),
            spec.has_blk,
            fn_index,
        )
    } else {
        define_bound_trampoline(em, spec, &layout, fn_index)
    }
}

/// The general trampoline: bind argv into a stack slot array through the
/// runtime, pass per-slot pointers (null = absent optional), release the
/// owned slots after the body call.
fn define_bound_trampoline(
    em: &mut Emitter,
    spec: &TrampSpec<'_>,
    layout: &Layout,
    fn_index: u32,
) -> Result<(), String> {
    let desc_id = super::statics::define_param_desc(em, spec)?;
    let sig = value_fn_sig(em);
    let mut func = ir::Function::with_name_signature(UserFuncName::user(1, fn_index), sig);
    let body_ref = em.module.declare_func_in_func(spec.body, &mut func);
    let release_id = em.import("zeo_rt_release");
    let release = em.module.declare_func_in_func(release_id, &mut func);
    let bind_id = em.import("zeo_rt_bind_params");
    let bind = em.module.declare_func_in_func(bind_id, &mut func);
    let desc_gv = em.module.declare_data_in_func(desc_id, &mut func);

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

    let n = layout.n_slots;
    let slots_ss = (n > 0).then(|| {
        b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            n as u32 * super::ctx::VALUE_SIZE,
            3,
        ))
    });
    let present_ss =
        b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let slots_ptr = match slots_ss {
        Some(ss) => b.ins().stack_addr(em.ptr, ss, 0),
        None => b.ins().iconst(em.ptr, 0),
    };
    let present_ptr = b.ins().stack_addr(em.ptr, present_ss, 0);
    let desc_ptr = b.ins().symbol_value(em.ptr, desc_gv);
    let call = b
        .ins()
        .call(bind, &[desc_ptr, argv, argc, slots_ptr, present_ptr]);
    let status = b.func.dfg.inst_results(call)[0];

    // A moved-in block the body will not consume (either because binding
    // failed or because the body has no blk slot) is released here.
    let release_blk = |b: &mut FunctionBuilder| {
        let got = b.ins().icmp_imm_u(IntCC::NotEqual, blk, 0);
        let rel = b.create_block();
        let cont = b.create_block();
        b.ins().brif(got, rel, &[], cont, &[]);
        b.switch_to_block(rel);
        b.ins().call(release, &[blk]);
        b.ins().jump(cont, &[]);
        b.switch_to_block(cont);
    };

    let ok = b.create_block();
    let bad = b.create_block();
    b.ins().brif(status, bad, &[], ok, &[]);
    b.switch_to_block(bad);
    // Binder contract: on error every slot is Nil -- only the block needs
    // releasing.
    release_blk(&mut b);
    let one = b.ins().iconst(types::I32, 1);
    b.ins().return_(&[one]);

    b.switch_to_block(ok);
    if !spec.has_blk {
        release_blk(&mut b);
    }
    let present = b
        .ins()
        .load(types::I64, ir::MemFlagsData::trusted(), present_ptr, 0);
    let zero_ptr = b.ins().iconst(em.ptr, 0);
    let mut args = Vec::with_capacity(n + 3);
    args.push(recv);
    let mut slot_addrs = Vec::with_capacity(n);
    for s in 0..n {
        let ss = slots_ss.expect("n > 0 when slots exist");
        let addr = b.ins().stack_addr(em.ptr, ss, s as i32 * 24);
        slot_addrs.push(addr);
        if layout.optional_mask & (1 << s) != 0 {
            let bit = b.ins().band_imm_u(present, (1u64 << s) as i64);
            let got = b.ins().icmp_imm_u(IntCC::NotEqual, bit, 0);
            args.push(b.ins().select(got, addr, zero_ptr));
        } else {
            args.push(addr);
        }
    }
    if spec.has_blk {
        args.push(blk);
    }
    args.push(out);
    let call = b.ins().call(body_ref, &args);
    let status = b.func.dfg.inst_results(call)[0];
    // The slots hold the binder's owned copies (the body took its own);
    // absent/nil slots release as no-ops.
    for addr in slot_addrs {
        b.ins().call(release, &[addr]);
    }
    b.ins().return_(&[status]);

    b.seal_all_blocks();
    b.finalize(cfg);
    em.record_clif("trampoline", &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(spec.tramp, &mut ctx)
        .map_err(|e| format!("compiling a trampoline: {e}"))
}

/// The lean trampoline for a required-params-only method.
fn define_plain_trampoline(
    em: &mut Emitter,
    tramp: FuncId,
    body: FuncId,
    arity: usize,
    has_blk: bool,
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

    // The block was MOVED in. A body with a blk slot receives it whole;
    // one without consumes it by releasing (Ruby: an unused block is
    // simply ignored).
    let arity_check = b.create_block();
    if has_blk {
        b.ins().jump(arity_check, &[]);
    } else {
        let got_blk = b.ins().icmp_imm_u(IntCC::NotEqual, blk, 0);
        let do_release = b.create_block();
        b.ins().brif(got_blk, do_release, &[], arity_check, &[]);
        b.switch_to_block(do_release);
        b.ins().call(release, &[blk]);
        b.ins().jump(arity_check, &[]);
    }

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
    let mut args = Vec::with_capacity(arity + 3);
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
    if has_blk {
        args.push(blk);
    }
    args.push(out);
    let call = b.ins().call(body_ref, &args);
    let status = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[status]);

    b.seal_all_blocks();
    b.finalize(cfg);
    em.record_clif("trampoline", &func);
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
    em.record_clif("trampoline", &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(tramp, &mut ctx)
        .map_err(|e| format!("compiling an accessor: {e}"))
}
