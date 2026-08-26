//! Trampolines: the `ValueFn`-shaped entry a dispatch row carries for a
//! compiled method. A PLAIN signature (required positionals only) keeps
//! the lean shape -- arity check, block release, direct call with argv
//! pointers. Everything else goes through `zeo_rt_bind_params`: the
//! runtime routes argv into a stack slot array from the method's `.rodata`
//! `ParamDescC` (kwargs peel, arity shapes, keyword errors -- one
//! implementation, the runtime's), and the trampoline passes per-slot
//! pointers (null = absent optional; its default runs in the body).

use super::module::Emitter;
use crate::codegen_error::{CResult, CodegenError};
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
    /// Required positionals plus REQUIRED keywords and nothing else, as
    /// the keyword names in declared (= slot) order. A call site whose
    /// keys are literal and cover this set exactly can fill the slots
    /// itself and go direct -- no Hash, no dynamic send, no binder. Any
    /// other keyword shape (an optional keyword, a `**rest`, `**nil`)
    /// keeps the binder, whose job is deciding what is absent.
    pub kw_direct: Option<Vec<String>>,
}

pub(crate) fn layout_of(p: &Params) -> CResult<Layout> {
    let rest_named = matches!(p.rest, Some(Some(_)));
    let kwrest_named = matches!(p.keyword_rest, Some(Some(_)));
    let n_slots = p.required.len()
        + p.optional.len()
        + usize::from(rest_named)
        + p.post.len()
        + p.keywords.len()
        + usize::from(kwrest_named);
    if n_slots > 64 {
        return Err(CodegenError::internal(format!(
            "a method with {n_slots} parameter slots overflows the 64-bit presence bitmap"
        )));
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
    let positional_only = p.optional.is_empty()
        && p.rest.is_none()
        && p.post.is_empty()
        && p.keyword_rest.is_none()
        && !p.no_keywords;
    let plain = positional_only && p.keywords.is_empty();
    let all_required = p
        .keywords
        .iter()
        .all(|k| matches!(k, KeywordParam::Required(_)));
    let kw_direct = (positional_only && !p.keywords.is_empty() && all_required).then(|| {
        p.keywords
            .iter()
            .map(|k| match k {
                KeywordParam::Required(n) => n.clone(),
                KeywordParam::Optional(n, _) => n.clone(),
            })
            .collect()
    });
    Ok(Layout {
        n_slots,
        optional_mask: mask,
        plain,
        kw_direct,
    })
}

/// What a `.rodata` `ParamDescC` serialization needs (`statics::
/// define_param_desc`): the shape, the name for binder error text, and
/// the callee frame binder raises run under.
pub(crate) struct ParamDescSpec<'a> {
    pub params: &'a Params,
    pub name: &'a str,
    pub file: Option<&'a str>,
    pub label: &'a str,
    pub line: u32,
    pub end_line: u32,
}

/// The static auto-splat decision: a single
/// ambiguous leading param never auto-splats; anything with real
/// positional structure does (non-lambda blocks only).
pub(crate) fn auto_splats(params: &Params) -> bool {
    let lead = params.required.len();
    let opt = params.optional.len();
    let post = params.post.len();
    let ambiguous_param0 = lead == 1 && opt == 0 && post == 0 && params.rest.is_none();
    !ambiguous_param0 && (lead + post > 0 || opt > 1)
}

/// `Proc#arity`'s value for a block/lambda of this
/// shape (a trailing comma's rest is not signature).
pub(crate) fn proc_arity(params: &Params, is_lambda: bool) -> i32 {
    let lead = params.required.len() as i32;
    let opt = params.optional.len() as i32;
    let post = params.post.len() as i32;
    let has_rest = params.rest.is_some() && !params.implicit_rest;
    let has_kw = !params.keywords.is_empty();
    let has_kwrest = params.keyword_rest.is_some();
    let any_required_kw = params
        .keywords
        .iter()
        .any(|k| matches!(k, KeywordParam::Required(_)));
    let min = lead + post + i32::from(any_required_kw);
    let max = (!has_rest).then(|| lead + opt + post + i32::from(has_kw || has_kwrest));
    let positive = match max {
        Some(max) if is_lambda => min == max,
        Some(_) => true,
        None => false,
    };
    if positive { min } else { -min - 1 }
}

/// Everything a trampoline definition needs beyond its `FuncId`s.
pub(crate) struct TrampSpec<'a> {
    pub tramp: FuncId,
    pub body: FuncId,
    pub params: &'a Params,
    pub has_blk: bool,
    /// This row's byte in `zeo_reopen_flags`, for a compile-time reopen of a
    /// BUILTIN. While it reads zero the `class Foo ... end` is still ahead,
    /// and the trampoline forwards to the row this one replaced.
    ///
    /// Guarded HERE and not in the body: the trampoline holds the call's own
    /// argv and block in the shape a native row takes, and it runs before the
    /// frame is pushed -- so the raise for a name that does not exist yet
    /// lands at the CALL, exactly where ruby's does.
    pub reopen_flag: Option<u32>,
    /// Whether [`Self::reopen_flag`] belongs to a LAZY UNIT's reopen. Such a
    /// byte can stay zero for the whole program (the unit is never
    /// required), so at zero the guard forwards to the native row only when
    /// there IS one and otherwise runs the body.
    pub reopen_unit: bool,
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

/// The callee frame a trampoline's own raises run under. A trampoline
/// raises BEFORE the body pushes anything -- a wrong-arity call, a frozen
/// receiver on an accessor -- and CRuby attributes both to the callee, so
/// the frame is pushed around the raise.
struct CalleeFrame {
    push: ir::FuncRef,
    pop: ir::FuncRef,
    rodata: cranelift_codegen::ir::GlobalValue,
    file: (u32, usize),
    label: (u32, usize),
    line: u32,
    end_line: u32,
    ptr: types::Type,
}

impl CalleeFrame {
    fn declare(
        em: &mut Emitter,
        func: &mut ir::Function,
        file: Option<&str>,
        label: &str,
        line: u32,
        end_line: u32,
    ) -> Self {
        let file = file.unwrap_or("");
        let file = (em.intern_rodata(file.as_bytes()), file.len());
        let label = (em.intern_rodata(label.as_bytes()), label.len());
        let push_id = em.import("zeo_rt_frame_push");
        let push = em.module.declare_func_in_func(push_id, func);
        let pop_id = em.import("zeo_rt_frame_pop");
        let pop = em.module.declare_func_in_func(pop_id, func);
        let rodata = em.module.declare_data_in_func(em.rodata_id, func);
        CalleeFrame {
            push,
            pop,
            rodata,
            file,
            label,
            line,
            end_line,
            ptr: em.ptr,
        }
    }

    /// Push, run `raise`, pop, and answer the status it returned.
    fn around(
        &self,
        b: &mut FunctionBuilder,
        raise: impl FnOnce(&mut FunctionBuilder) -> ir::Value,
    ) -> ir::Value {
        let base = b.ins().symbol_value(self.ptr, self.rodata);
        let at = |b: &mut FunctionBuilder, off: u32| {
            if off == 0 {
                base
            } else {
                b.ins().iadd_imm_u(base, i64::from(off))
            }
        };
        let file_ptr = at(b, self.file.0);
        let file_n = b.ins().iconst(self.ptr, self.file.1 as i64);
        let label_ptr = at(b, self.label.0);
        let label_n = b.ins().iconst(self.ptr, self.label.1 as i64);
        let line = b.ins().iconst(types::I32, i64::from(self.line));
        let end = b.ins().iconst(types::I32, i64::from(self.end_line));
        b.ins().call(
            self.push,
            &[file_ptr, file_n, label_ptr, label_n, line, end],
        );
        let status = raise(b);
        b.ins().call(self.pop, &[]);
        status
    }
}

/// Define a method's trampoline: the lean shape for a plain signature,
/// the `bind_params` shape for everything else.
pub(crate) fn define_trampoline(
    em: &mut Emitter,
    spec: &TrampSpec<'_>,
    fn_index: u32,
) -> CResult<()> {
    let layout = layout_of(spec.params)?;
    if layout.plain {
        define_plain_trampoline(em, spec, spec.params.required.len(), fn_index)
    } else {
        define_bound_trampoline(em, spec, &layout, fn_index)
    }
}

/// The reopen guard both trampolines open with: while the flag byte reads
/// zero the `class Foo ... end` has not been reached, so the call goes to the
/// row this one replaced and returns.
///
/// Emitted before anything else the trampoline does -- no frame, no arity
/// check, no binder -- so the argv and block arrive exactly as the caller
/// passed them and a raise lands at the CALL.
fn emit_reopen_guard(
    em: &mut Emitter,
    b: &mut FunctionBuilder<'_>,
    entry: ir::Block,
    spec: &TrampSpec<'_>,
) -> CResult<()> {
    let Some(idx) = spec.reopen_flag else {
        return Ok(());
    };
    let recv = b.block_params(entry)[0];
    let argv = b.block_params(entry)[1];
    let argc = b.block_params(entry)[2];
    let blk = b.block_params(entry)[3];
    let out = b.block_params(entry)[4];
    let flags_gv = em.module.declare_data_in_func(em.reopen_flags_id, b.func);
    let base = b.ins().symbol_value(em.ptr, flags_gv);
    let ready = b
        .ins()
        .load(types::I8, ir::MemFlagsData::trusted(), base, idx as i32);
    let installed = b.create_block();
    let deferred = b.create_block();
    b.ins().brif(ready, installed, &[], deferred, &[]);
    b.switch_to_block(deferred);
    let slot = em.syms.intern(spec.name);
    let syms_gv = em.module.declare_data_in_func(em.syms_id, b.func);
    let syms = b.ins().symbol_value(em.ptr, syms_gv);
    let sym_id = b.ins().load(
        types::I32,
        ir::MemFlagsData::trusted(),
        syms,
        (slot * 4) as i32,
    );
    // A UNIT's reopen: with no native row to forward to, the body IS the
    // only answer, and forwarding would raise `undefined method` for a name
    // the unit ADDS. Asked here, in the cold arm, and only for a unit.
    if spec.reopen_unit {
        let e_id = em.import("zeo_rt_native_row_exists");
        let exists_fn = em.module.declare_func_in_func(e_id, b.func);
        let call = b.ins().call(exists_fn, &[recv, sym_id]);
        let exists = b.func.dfg.inst_results(call)[0];
        let forward = b.create_block();
        b.ins().brif(exists, forward, &[], installed, &[]);
        b.switch_to_block(forward);
    }
    let f_id = em.import("zeo_rt_native_row_call");
    let native = em.module.declare_func_in_func(f_id, b.func);
    let call = b.ins().call(native, &[recv, sym_id, argv, argc, blk, out]);
    let status = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[status]);
    b.switch_to_block(installed);
    Ok(())
}

/// The general trampoline: bind argv into a stack slot array through the
/// runtime, pass per-slot pointers (null = absent optional), release the
/// owned slots after the body call.
fn define_bound_trampoline(
    em: &mut Emitter,
    spec: &TrampSpec<'_>,
    layout: &Layout,
    fn_index: u32,
) -> CResult<()> {
    let desc_id = super::statics::define_param_desc(
        em,
        &ParamDescSpec {
            params: spec.params,
            name: spec.name,
            file: spec.file,
            label: spec.label,
            line: spec.line,
            end_line: spec.end_line,
        },
    )?;
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
    emit_reopen_guard(em, &mut b, entry, spec)?;
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
        .map_err(|e| CodegenError::internal(format!("compiling a trampoline: {e}")))
}

/// The lean trampoline for a required-params-only method.
fn define_plain_trampoline(
    em: &mut Emitter,
    spec: &TrampSpec<'_>,
    arity: usize,
    fn_index: u32,
) -> CResult<()> {
    let (tramp, body, has_blk) = (spec.tramp, spec.body, spec.has_blk);
    let sig = value_fn_sig(em);
    let mut func = ir::Function::with_name_signature(UserFuncName::user(1, fn_index), sig);
    let body_ref = em.module.declare_func_in_func(body, &mut func);
    let release_id = em.import("zeo_rt_release");
    let release = em.module.declare_func_in_func(release_id, &mut func);
    let wrong_id = em.import("zeo_rt_wrong_arity");
    let wrong = em.module.declare_func_in_func(wrong_id, &mut func);
    let frame = CalleeFrame::declare(
        em,
        &mut func,
        spec.file,
        spec.label,
        spec.line,
        spec.end_line,
    );

    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fbc);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    emit_reopen_guard(em, &mut b, entry, spec)?;
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
    let ptr = em.ptr;
    let status = frame.around(&mut b, |b| {
        let n = b.ins().iconst(ptr, arity as i64);
        let call = b.ins().call(wrong, &[argc, n, n]);
        b.func.dfg.inst_results(call)[0]
    });
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
        .map_err(|e| CodegenError::internal(format!("compiling a trampoline: {e}")))
}

/// An `attr_reader`/`attr_writer` trampoline -- the slot access IS the
/// method (no body fn).
#[allow(clippy::too_many_arguments)]
pub(crate) fn define_accessor(
    em: &mut Emitter,
    tramp: FuncId,
    slot: usize,
    kind: crate::compiler::AccessorKind,
    fn_index: u32,
    frame: Option<(Option<&str>, &str, u32, u32)>,
) -> CResult<()> {
    let sig = value_fn_sig(em);
    let mut func = ir::Function::with_name_signature(UserFuncName::user(1, fn_index), sig);
    // An `attr_*`-GENERATED accessor is iseq-less in CRuby: it appears in
    // no backtrace, and TracePoint sees no `:call` for it. Only a body
    // folded from a real `def` carries a frame here.
    let frame = frame.map(|(file, label, line, end_line)| {
        CalleeFrame::declare(em, &mut func, file, label, line, end_line)
    });
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
    let ptr = em.ptr;
    let raise_arity = |b: &mut FunctionBuilder| {
        let n = b.ins().iconst(ptr, want);
        let call = b.ins().call(wrong, &[argc, n, n]);
        b.func.dfg.inst_results(call)[0]
    };
    let status = match &frame {
        Some(frame) => frame.around(&mut b, raise_arity),
        None => raise_arity(&mut b),
    };
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
            // Frozen check first; then TWO fresh
            // references off the caller's borrowed argv[0]: one moves into
            // the slot, one is the returned value.
            let frozen = frozen.expect("writer imports frozen_check");
            let set = set.expect("writer imports ivar_set_slot");
            let retain = retain.expect("writer imports retain");
            let check_frozen = |b: &mut FunctionBuilder| {
                let call = b.ins().call(frozen, &[recv]);
                b.func.dfg.inst_results(call)[0]
            };
            let status = match &frame {
                Some(frame) => frame.around(&mut b, check_frozen),
                None => check_frozen(&mut b),
            };
            let go = b.create_block();
            let sig_out = b.create_block();
            b.ins().brif(status, sig_out, &[], go, &[]);
            b.switch_to_block(sig_out);
            let one = b.ins().iconst(types::I32, 1);
            b.ins().return_(&[one]);
            b.switch_to_block(go);
            super::ownership::retain_if_heap_raw(&mut b, argv, |b| {
                b.ins().call(retain, &[argv]);
                b.ins().call(retain, &[argv]);
            });
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
        .map_err(|e| CodegenError::internal(format!("compiling an accessor: {e}")))
}
