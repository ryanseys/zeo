//! Object emission: ISA/flags, the `ObjectModule`, the capi import cache,
//! and the per-program orchestration -- prologue/epilogue of the compiled
//! `<main>`, the emitted C `main`, and the statics (see `statics`).

use super::capi_names::{self, CTy};
use super::ctx::{Fx, VALUE_SIZE};
use super::{names, statics, stmt, verify};
use crate::analyze::Analyzed;
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;

/// Lower `analyzed` to one object file's bytes.
pub fn compile(analyzed: &Analyzed) -> Result<Vec<u8>, String> {
    let mut em = Emitter::new()?;
    let toplevel = define_toplevel(&mut em, analyzed)?;
    let unit_init = statics::define_unit_init(&mut em)?;
    statics::define_syms(&mut em)?;
    let desc = statics::define_desc(&mut em, analyzed, toplevel, unit_init)?;
    define_main(&mut em, desc)?;
    statics::define_rodata(&mut em)?;
    let product = em.module.finish();
    product
        .emit()
        .map_err(|e| format!("emitting the object file: {e}"))
}

/// Program-wide emission state: the module, the rodata blob, the symbol
/// pool, and the capi import cache.
pub(crate) struct Emitter {
    pub module: ObjectModule,
    pub ptr: ir::Type,
    pub rodata_id: DataId,
    pub syms_id: DataId,
    pub syms: statics::SymPool,
    rodata: Vec<u8>,
    rodata_offsets: HashMap<Vec<u8>, u32>,
    imports: HashMap<&'static str, FuncId>,
}

impl Emitter {
    fn new() -> Result<Emitter, String> {
        let mut flags = settings::builder();
        let set = |flags: &mut settings::Builder, k: &str, v: &str| {
            flags
                .set(k, v)
                .map_err(|e| format!("cranelift setting {k}={v}: {e}"))
        };
        set(&mut flags, "opt_level", "speed")?;
        set(&mut flags, "is_pic", "true")?;
        set(&mut flags, "preserve_frame_pointers", "true")?;
        set(&mut flags, "enable_probestack", "false")?;
        set(&mut flags, "unwind_info", "true")?;
        set(&mut flags, "libcall_call_conv", "isa_default")?;
        let verify = cfg!(debug_assertions) || std::env::var_os("ZEO_CLIF_VERIFY").is_some();
        set(
            &mut flags,
            "enable_verifier",
            if verify { "true" } else { "false" },
        )?;
        let isa = cranelift_native::builder()
            .map_err(|e| format!("cranelift has no backend for this host: {e}"))?
            .finish(settings::Flags::new(flags))
            .map_err(|e| format!("building the target ISA: {e}"))?;
        if isa.triple().endianness() != Ok(target_lexicon::Endianness::Little) {
            return Err("the clif backend only serializes little-endian tables".to_string());
        }
        let elf = isa.triple().binary_format == target_lexicon::BinaryFormat::Elf;
        let mut builder =
            ObjectBuilder::new(isa, "zeo-p0", cranelift_module::default_libcall_names())
                .map_err(|e| format!("cranelift object builder: {e}"))?;
        builder.per_function_section(true);
        // `.eh_frame` is free on ELF; Mach-O emission panics in
        // cranelift-object 0.134 and is not load-bearing (decision 13).
        builder.unwind_info(elf);
        let mut module = ObjectModule::new(builder);
        let ptr = module.target_config().pointer_type();
        let rodata_id = module
            .declare_data(names::RODATA, Linkage::Local, false, false)
            .map_err(|e| format!("declaring {}: {e}", names::RODATA))?;
        let syms_id = module
            .declare_data(names::SYMS, Linkage::Local, true, false)
            .map_err(|e| format!("declaring {}: {e}", names::SYMS))?;
        Ok(Emitter {
            module,
            ptr,
            rodata_id,
            syms_id,
            syms: statics::SymPool::default(),
            rodata: Vec::new(),
            rodata_offsets: HashMap::new(),
            imports: HashMap::new(),
        })
    }

    /// `bytes`' offset in the rodata blob (deduplicated).
    pub(crate) fn intern_rodata(&mut self, bytes: &[u8]) -> u32 {
        if let Some(&off) = self.rodata_offsets.get(bytes) {
            return off;
        }
        let off = u32::try_from(self.rodata.len()).expect("rodata under 4GB");
        self.rodata.extend_from_slice(bytes);
        self.rodata_offsets.insert(bytes.to_vec(), off);
        off
    }

    pub(crate) fn take_rodata(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.rodata)
    }

    pub(crate) fn syms_len(&self) -> u32 {
        self.syms_names().len() as u32
    }

    pub(crate) fn syms_names(&self) -> &[String] {
        self.syms.names()
    }

    fn ctype(&self, t: CTy) -> ir::Type {
        match t {
            CTy::Ptr | CTy::Usize => self.ptr,
            CTy::I32 | CTy::U32 => types::I32,
            CTy::U8 => types::I8,
        }
    }

    fn abi_param(&self, t: CTy) -> AbiParam {
        match t {
            // A sub-32-bit C ARGUMENT is the caller's to zero-extend (the
            // Apple arm64 rule; a no-op elsewhere).
            CTy::U8 => AbiParam::new(types::I8).uext(),
            CTy::Ptr | CTy::Usize | CTy::I32 | CTy::U32 => AbiParam::new(self.ctype(t)),
        }
    }

    /// The import `FuncId` for capi symbol `name` (declared once).
    pub(crate) fn import(&mut self, name: &'static str) -> FuncId {
        if let Some(&id) = self.imports.get(name) {
            return id;
        }
        let row = capi_names::sig(name);
        let mut sig = self.module.make_signature();
        sig.params
            .extend(row.params.iter().map(|&t| self.abi_param(t)));
        if let Some(ret) = row.ret {
            // Returns carry no extension claim -- only the low bits are
            // read, at the value's own width.
            sig.returns.push(AbiParam::new(self.ctype(ret)));
        }
        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .unwrap_or_else(|e| panic!("declaring capi import {name}: {e}"));
        self.imports.insert(name, id);
        id
    }
}

/// The compiled `<main>` body, `UnitFn`-shaped: hoisted nil-initialized
/// locals, the frame push, `check_ints`, the statements, then `Nil` out --
/// with the ONE landing block releasing the locals and popping the frame
/// (which drains the release pool) on the signal path.
fn define_toplevel(em: &mut Emitter, analyzed: &Analyzed) -> Result<FuncId, String> {
    let mut sig = em.module.make_signature();
    sig.params.push(AbiParam::new(em.ptr));
    sig.returns.push(AbiParam::new(types::I32));
    let func_id = em
        .module
        .declare_function(names::TOPLEVEL, Linkage::Local, &sig)
        .map_err(|e| format!("declaring {}: {e}", names::TOPLEVEL))?;

    let frame = analyzed.compiler.hir.files.first().map(|f| f.name.clone());
    let mut locals = crate::analyze::local_storage::Locals::default();
    for &stmt in &analyzed.main_statements {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, stmt, &mut locals);
    }

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 0), sig);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let b = FunctionBuilder::new(&mut func, &mut fbc);
    let mut fx = Fx::new(em, analyzed, b, |em, b| {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let rodata_gv = em.module.declare_data_in_func(em.rodata_id, b.func);
        let syms_gv = em.module.declare_data_in_func(em.syms_id, b.func);
        let rodata = b.ins().symbol_value(em.ptr, rodata_gv);
        let syms = b.ins().symbol_value(em.ptr, syms_gv);
        (rodata, syms)
    });
    let out_ptr = {
        let entry = fx.b.current_block().expect("entry is current");
        fx.b.block_params(entry)[0]
    };

    // Hoisted locals: one owned 24-byte slot each, zeroed (Nil).
    for name in locals.names().to_vec() {
        let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            VALUE_SIZE,
            3,
        ));
        let dst = fx.slot_addr(ss, 0);
        let z = fx.b.ins().iconst(types::I64, 0);
        for off in [0, 8, 16] {
            fx.b.ins().store(MemFlagsData::trusted(), z, dst, off);
        }
        fx.locals.insert(name, ss);
    }

    if let Some(file) = &frame {
        let off = fx.em.intern_rodata(file.as_bytes());
        let main_off = fx.em.intern_rodata(b"<main>");
        let file_ptr = fx.rod(off);
        let file_len = fx.b.ins().iconst(fx.em.ptr, file.len() as i64);
        let label_ptr = fx.rod(main_off);
        let label_len = fx.b.ins().iconst(fx.em.ptr, "<main>".len() as i64);
        let zero = fx.b.ins().iconst(types::I32, 0);
        fx.call(
            "zeo_rt_frame_push",
            &[file_ptr, file_len, label_ptr, label_len, zero, zero],
        );
    }
    let status = fx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints returns a status");
    fx.fallible(status);

    stmt::lower_stmts(&mut fx, &analyzed.main_statements)?;

    // Normal exit: release the locals, pop the frame (drains the pool),
    // hand back Nil.
    let epilogue = |fx: &mut Fx, status: i64| {
        let local_slots: Vec<_> = fx.locals.values().copied().collect();
        for ss in local_slots {
            let addr = fx.slot_addr(ss, 0);
            fx.call("zeo_rt_release", &[addr]);
        }
        if frame.is_some() {
            fx.call("zeo_rt_frame_pop", &[]);
        }
        let code = fx.b.ins().iconst(types::I32, status);
        fx.b.ins().return_(&[code]);
    };
    let z = fx.b.ins().iconst(types::I64, 0);
    for off in [0, 8, 16] {
        fx.b.ins().store(MemFlagsData::trusted(), z, out_ptr, off);
    }
    epilogue(&mut fx, 0);
    let land = fx.land;
    fx.b.switch_to_block(land);
    epilogue(&mut fx, 1);

    verify::check(&fx, names::TOPLEVEL);
    let Fx { mut b, .. } = fx;
    b.seal_all_blocks();
    b.finalize(cfg);

    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("compiling {}: {e}", names::TOPLEVEL))?;
    Ok(func_id)
}

/// The exported C `main(argc, argv)`: tail-calls `zeo_rt_main` with the
/// program description (corelib null until G8).
fn define_main(em: &mut Emitter, desc: DataId) -> Result<FuncId, String> {
    let mut sig = em.module.make_signature();
    sig.params.push(AbiParam::new(types::I32));
    sig.params.push(AbiParam::new(em.ptr));
    sig.returns.push(AbiParam::new(types::I32));
    let func_id = em
        .module
        .declare_function("main", Linkage::Export, &sig)
        .map_err(|e| format!("declaring main: {e}"))?;
    let f_rt_main = em.import("zeo_rt_main");

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 1), sig);
    let rt_main = em.module.declare_func_in_func(f_rt_main, &mut func);
    let desc_gv = em.module.declare_data_in_func(desc, &mut func);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fbc);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let argc = b.block_params(entry)[0];
    let argv = b.block_params(entry)[1];
    let desc_ptr = b.ins().symbol_value(em.ptr, desc_gv);
    let corelib = b.ins().iconst(em.ptr, 0);
    let call = b.ins().call(rt_main, &[argc, argv, desc_ptr, corelib]);
    let code = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[code]);
    b.seal_all_blocks();
    b.finalize(cfg);

    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("compiling main: {e}"))?;
    Ok(func_id)
}
