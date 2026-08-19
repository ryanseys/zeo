//! Object emission: ISA/flags, the `ObjectModule`, and (until the real
//! lowering modules land at M0-10+) the hello-slice lowering itself -- a
//! top level of literal-string `puts` statements, the program tables, and
//! the C `main` that tail-calls `zeo_rt_main`.

use crate::analyze::Analyzed;
use crate::clif::capi_names::{self, CTy};
use crate::clif::names;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;
use zeo_abi::abi::{self, ProgramDesc, Str};

const VALUE_SIZE: u32 = abi::VALUE_SIZE as u32;
/// `EncodingId(1)` = UTF-8, the encoding of every plain source literal.
const ENC_UTF8: i64 = 1;

/// Lower `analyzed` to one object file's bytes.
pub fn compile(analyzed: &Analyzed) -> Result<Vec<u8>, String> {
    let mut em = Emitter::new(analyzed)?;
    let toplevel = em.define_toplevel()?;
    let desc = em.define_desc(toplevel)?;
    em.define_main(desc)?;
    em.define_rodata()?;
    let product = em.module.finish();
    product
        .emit()
        .map_err(|e| format!("emitting the object file: {e}"))
}

struct Emitter<'a> {
    analyzed: &'a Analyzed,
    module: ObjectModule,
    ptr: ir::Type,
    rodata_id: DataId,
    rodata: Vec<u8>,
    rodata_offsets: HashMap<Vec<u8>, u32>,
    imports: HashMap<&'static str, FuncId>,
}

impl<'a> Emitter<'a> {
    fn new(analyzed: &'a Analyzed) -> Result<Emitter<'a>, String> {
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
        Ok(Emitter {
            analyzed,
            module,
            ptr,
            rodata_id,
            rodata: Vec::new(),
            rodata_offsets: HashMap::new(),
            imports: HashMap::new(),
        })
    }

    /// `bytes`' offset in the rodata blob (deduplicated).
    fn intern(&mut self, bytes: &[u8]) -> u32 {
        if let Some(&off) = self.rodata_offsets.get(bytes) {
            return off;
        }
        let off = u32::try_from(self.rodata.len()).expect("rodata under 4GB");
        self.rodata.extend_from_slice(bytes);
        self.rodata_offsets.insert(bytes.to_vec(), off);
        off
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
            // A sub-32-bit C argument is the CALLER's to zero-extend
            // (the Apple arm64 rule; a no-op elsewhere).
            CTy::U8 => AbiParam::new(types::I8).uext(),
            CTy::Ptr | CTy::Usize | CTy::I32 | CTy::U32 => AbiParam::new(self.ctype(t)),
        }
    }

    /// The import `FuncId` for capi symbol `name` (declared once).
    fn import(&mut self, name: &'static str) -> FuncId {
        if let Some(&id) = self.imports.get(name) {
            return id;
        }
        let row = capi_names::sig(name);
        let mut sig = self.module.make_signature();
        sig.params
            .extend(row.params.iter().map(|&t| self.abi_param(t)));
        if let Some(ret) = row.ret {
            sig.returns.push(self.abi_param(ret));
        }
        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .unwrap_or_else(|e| panic!("declaring capi import {name}: {e}"));
        self.imports.insert(name, id);
        id
    }

    /// The compiled `<main>` body, `UnitFn`-shaped: frame, `check_ints`,
    /// the statements, `Nil` out. The hello slice lowers literal-string
    /// `puts` only; anything else refuses with its source location.
    fn define_toplevel(&mut self) -> Result<FuncId, String> {
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(self.ptr));
        sig.returns.push(AbiParam::new(types::I32));
        let func_id = self
            .module
            .declare_function(names::TOPLEVEL, Linkage::Local, &sig)
            .map_err(|e| format!("declaring {}: {e}", names::TOPLEVEL))?;

        // Pre-resolve everything that needs `&mut self` before the builder
        // borrows the function.
        let statements: Vec<PutsStmt> = self.lower_statements()?;
        let frame = self
            .analyzed
            .compiler
            .hir
            .files
            .first()
            .map(|f| f.name.clone())
            .map(|name| {
                let off = self.intern(name.as_bytes());
                (off, name.len(), self.intern(b"<main>"))
            });
        let max_args = statements.iter().map(|s| s.args.len()).max().unwrap_or(0);
        let f_frame_push = self.import("zeo_rt_frame_push");
        let f_frame_pop = self.import("zeo_rt_frame_pop");
        let f_check_ints = self.import("zeo_rt_check_ints");
        let f_set_line = self.import("zeo_rt_set_line");
        let f_str_new = self.import("zeo_rt_str_new");
        let f_pool_push = self.import("zeo_rt_pool_push");
        let f_kernel_puts = self.import("zeo_rt_kernel_puts");

        let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 0), sig);
        let frame_push = self.module.declare_func_in_func(f_frame_push, &mut func);
        let frame_pop = self.module.declare_func_in_func(f_frame_pop, &mut func);
        let check_ints = self.module.declare_func_in_func(f_check_ints, &mut func);
        let set_line = self.module.declare_func_in_func(f_set_line, &mut func);
        let str_new = self.module.declare_func_in_func(f_str_new, &mut func);
        let pool_push = self.module.declare_func_in_func(f_pool_push, &mut func);
        let kernel_puts = self.module.declare_func_in_func(f_kernel_puts, &mut func);
        let rodata_gv = self.module.declare_data_in_func(self.rodata_id, &mut func);

        let cfg = self.module.target_config();
        let mut fbc = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut func, &mut fbc);
        let entry = b.create_block();
        let land = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let out_ptr = b.block_params(entry)[0];

        let argv_slot = (max_args > 0).then(|| {
            b.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                max_args as u32 * VALUE_SIZE,
                3,
            ))
        });
        let ret_slot = b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            VALUE_SIZE,
            3,
        ));
        let ptr_ty = self.ptr;
        let rodata_base = b.ins().symbol_value(ptr_ty, rodata_gv);
        let rod_addr = |b: &mut FunctionBuilder, off: u32| {
            if off == 0 {
                rodata_base
            } else {
                b.ins().iadd_imm_u(rodata_base, i64::from(off))
            }
        };

        if let Some((file_off, file_len, main_off)) = frame {
            let file_ptr = rod_addr(&mut b, file_off);
            let file_len = b.ins().iconst(ptr_ty, file_len as i64);
            let label_ptr = rod_addr(&mut b, main_off);
            let label_len = b.ins().iconst(ptr_ty, "<main>".len() as i64);
            let zero = b.ins().iconst(types::I32, 0);
            b.ins().call(
                frame_push,
                &[file_ptr, file_len, label_ptr, label_len, zero, zero],
            );
        }
        let call = b.ins().call(check_ints, &[]);
        let status = b.func.dfg.inst_results(call)[0];
        let ok = b.create_block();
        b.ins().brif(status, land, &[], ok, &[]);
        b.switch_to_block(ok);

        let mut prev_line: Option<u32> = None;
        for stmt in &statements {
            if let Some(line) = stmt.line
                && prev_line != Some(line)
            {
                let line_v = b.ins().iconst(types::I32, i64::from(line));
                b.ins().call(set_line, &[line_v]);
                prev_line = Some(line);
            }
            let argv_slot = argv_slot.expect("a puts statement always has an argv slot");
            for (i, &(off, len)) in stmt.args.iter().enumerate() {
                let text_ptr = rod_addr(&mut b, off);
                let text_len = b.ins().iconst(ptr_ty, len as i64);
                let enc = b.ins().iconst(types::I8, ENC_UTF8);
                let dst = b
                    .ins()
                    .stack_addr(ptr_ty, argv_slot, (i as u32 * VALUE_SIZE) as i32);
                b.ins().call(str_new, &[text_ptr, text_len, enc, dst]);
                // Pooled at creation: the frame's pool owns the fresh
                // string from here (drained at frame pop, error path
                // included); the slot's bits stay a valid borrow.
                b.ins().call(pool_push, &[dst]);
            }
            let argv_ptr = b.ins().stack_addr(ptr_ty, argv_slot, 0);
            let argc = b.ins().iconst(ptr_ty, stmt.args.len() as i64);
            let out = b.ins().stack_addr(ptr_ty, ret_slot, 0);
            let call = b.ins().call(kernel_puts, &[argv_ptr, argc, out]);
            let status = b.func.dfg.inst_results(call)[0];
            let next = b.create_block();
            b.ins().brif(status, land, &[], next, &[]);
            b.switch_to_block(next);
            // `puts` answers nil -- an immediate; nothing to pool.
        }

        // Normal exit: pop the frame (drains the pool), hand back Nil.
        if frame.is_some() {
            b.ins().call(frame_pop, &[]);
        }
        let zero64 = b.ins().iconst(types::I64, 0);
        for off in [0, 8, 16] {
            b.ins().store(MemFlagsData::trusted(), zero64, out_ptr, off);
        }
        let ok_status = b.ins().iconst(types::I32, 0);
        b.ins().return_(&[ok_status]);

        // The one landing: pop (drains the pool), propagate the signal.
        b.switch_to_block(land);
        if frame.is_some() {
            b.ins().call(frame_pop, &[]);
        }
        let err_status = b.ins().iconst(types::I32, 1);
        b.ins().return_(&[err_status]);

        b.seal_all_blocks();
        b.finalize(cfg);

        let mut ctx = self.module.make_context();
        ctx.func = func;
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| format!("compiling {}: {e}", names::TOPLEVEL))?;
        Ok(func_id)
    }

    /// The M0-9 statement recognizer: literal-string `puts` only. Every
    /// other shape names its location and refuses -- the vertical slice
    /// must never half-lower a program.
    fn lower_statements(&mut self) -> Result<Vec<PutsStmt>, String> {
        let hir = &self.analyzed.compiler.hir;
        let mut out = Vec::new();
        for &stmt in &self.analyzed.main_statements {
            let unsupported = |what: &str| {
                let at = crate::codegen::source_location(&self.analyzed.compiler, stmt)
                    .map(|(f, l)| format!(" ({f}:{l})"))
                    .unwrap_or_default();
                Err(format!(
                    "--backend aot is an M0 vertical slice: cannot lower {what} yet{at}"
                ))
            };
            let HirNode::Call {
                receiver: None,
                name,
                args,
                kwargs,
                block: None,
                block_arg: None,
                safe: _,
            } = &hir[stmt]
            else {
                return unsupported("this statement");
            };
            if name != "puts" || !kwargs.is_empty() {
                return unsupported(&format!("a call to `{name}`"));
            }
            let mut texts = Vec::new();
            for arg in args {
                let ArrayElem::Single(id) = arg else {
                    return unsupported("a splat argument");
                };
                let Some(text) = self.literal_str(*id) else {
                    return unsupported("a non-literal argument");
                };
                let off = self.intern(text.as_bytes());
                texts.push((off, text.len()));
            }
            let line =
                crate::codegen::source_location(&self.analyzed.compiler, stmt).map(|(_, l)| l);
            out.push(PutsStmt { args: texts, line });
        }
        Ok(out)
    }

    /// `id` as a pure (non-interpolated) string literal's text.
    #[allow(
        clippy::wildcard_enum_match_arm,
        reason = "structural: a shape test -- every future node kind is unambiguously not a pure string literal"
    )]
    fn literal_str(&self, id: NodeId) -> Option<String> {
        match &self.analyzed.compiler.hir[id] {
            HirNode::StringLit(parts) => match parts.as_slice() {
                [] => Some(String::new()),
                [StrPart::Lit(s)] => Some(s.clone()),
                [StrPart::Bytes(_) | StrPart::Interp(_)] | [_, _, ..] => None,
            },
            _ => None,
        }
    }

    /// `zeo_program_desc` + the `Str` tables: the loaded-features seed (the
    /// same list the rustc backend emits) and the parse warnings.
    fn define_desc(&mut self, toplevel: FuncId) -> Result<DataId, String> {
        let hir = &self.analyzed.compiler.hir;
        let mut loaded: Vec<String> = hir
            .loaded_files
            .iter()
            .filter(|f| !f.is_unit)
            .map(|f| f.canonical.to_string_lossy().into_owned())
            .collect();
        let ambient_rbconfig = "<zeo-shim>/rbconfig.rb".to_string();
        if !loaded.contains(&ambient_rbconfig) {
            loaded.push(ambient_rbconfig);
        }
        let warnings: Vec<String> = hir.warnings.iter().map(ToString::to_string).collect();

        // One Str-array object: loaded features first, then warnings.
        let entries: Vec<(u32, usize)> = loaded
            .iter()
            .chain(warnings.iter())
            .map(|s| (self.intern(s.as_bytes()), s.len()))
            .collect();
        let str_size = std::mem::size_of::<Str>();
        let tables_id = self
            .module
            .declare_data(names::STR_TABLES, Linkage::Local, false, false)
            .map_err(|e| format!("declaring {}: {e}", names::STR_TABLES))?;
        let mut tables = DataDescription::new();
        let mut bytes = vec![0u8; str_size * entries.len()];
        for (i, &(_, len)) in entries.iter().enumerate() {
            let at = i * str_size + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(len as u64).to_le_bytes());
        }
        tables.define(bytes.into_boxed_slice());
        tables.set_align(8);
        let rodata_gv = self
            .module
            .declare_data_in_data(self.rodata_id, &mut tables);
        for (i, &(off, _)) in entries.iter().enumerate() {
            let at = (i * str_size + std::mem::offset_of!(Str, ptr)) as u32;
            tables.write_data_addr(at, rodata_gv, i64::from(off));
        }
        self.module
            .define_data(tables_id, &tables)
            .map_err(|e| format!("defining {}: {e}", names::STR_TABLES))?;

        let desc_id = self
            .module
            .declare_data(names::PROGRAM_DESC, Linkage::Local, false, false)
            .map_err(|e| format!("declaring {}: {e}", names::PROGRAM_DESC))?;
        let mut desc = DataDescription::new();
        let mut buf = vec![0u8; std::mem::size_of::<ProgramDesc>()];
        let put_u32 = |buf: &mut [u8], at: usize, v: u32| {
            buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
        };
        let put_u64 = |buf: &mut [u8], at: usize, v: u64| {
            buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
        };
        put_u32(
            &mut buf,
            std::mem::offset_of!(ProgramDesc, abi_version),
            abi::ABI_VERSION,
        );
        put_u64(
            &mut buf,
            std::mem::offset_of!(ProgramDesc, n_loaded),
            loaded.len() as u64,
        );
        put_u64(
            &mut buf,
            std::mem::offset_of!(ProgramDesc, n_warnings),
            warnings.len() as u64,
        );
        desc.define(buf.into_boxed_slice());
        desc.set_align(8);
        let tables_gv = self.module.declare_data_in_data(tables_id, &mut desc);
        if !loaded.is_empty() {
            desc.write_data_addr(
                std::mem::offset_of!(ProgramDesc, loaded_features) as u32,
                tables_gv,
                0,
            );
        }
        if !warnings.is_empty() {
            desc.write_data_addr(
                std::mem::offset_of!(ProgramDesc, parse_warnings) as u32,
                tables_gv,
                (loaded.len() * str_size) as i64,
            );
        }
        let toplevel_ref = self.module.declare_func_in_data(toplevel, &mut desc);
        desc.write_function_addr(
            std::mem::offset_of!(ProgramDesc, toplevel) as u32,
            toplevel_ref,
        );
        self.module
            .define_data(desc_id, &desc)
            .map_err(|e| format!("defining {}: {e}", names::PROGRAM_DESC))?;
        Ok(desc_id)
    }

    /// The exported C `main(argc, argv)`: tail-calls `zeo_rt_main` with the
    /// program description (corelib null until G8).
    fn define_main(&mut self, desc: DataId) -> Result<FuncId, String> {
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I32));
        sig.params.push(AbiParam::new(self.ptr));
        sig.returns.push(AbiParam::new(types::I32));
        let func_id = self
            .module
            .declare_function("main", Linkage::Export, &sig)
            .map_err(|e| format!("declaring main: {e}"))?;
        let f_rt_main = self.import("zeo_rt_main");

        let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 1), sig);
        let rt_main = self.module.declare_func_in_func(f_rt_main, &mut func);
        let desc_gv = self.module.declare_data_in_func(desc, &mut func);
        let cfg = self.module.target_config();
        let mut fbc = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut func, &mut fbc);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let argc = b.block_params(entry)[0];
        let argv = b.block_params(entry)[1];
        let desc_ptr = b.ins().symbol_value(self.ptr, desc_gv);
        let corelib = b.ins().iconst(self.ptr, 0);
        let call = b.ins().call(rt_main, &[argc, argv, desc_ptr, corelib]);
        let code = b.func.dfg.inst_results(call)[0];
        b.ins().return_(&[code]);
        b.seal_all_blocks();
        b.finalize(cfg);

        let mut ctx = self.module.make_context();
        ctx.func = func;
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| format!("compiling main: {e}"))?;
        Ok(func_id)
    }

    /// The accumulated read-only bytes, defined LAST (interning happens
    /// throughout emission).
    fn define_rodata(&mut self) -> Result<(), String> {
        let mut data = DataDescription::new();
        // An empty data object is illegal; the blob always exists because
        // the Str tables and frame strings point into it.
        let bytes = std::mem::take(&mut self.rodata);
        data.define(if bytes.is_empty() {
            Box::new([0u8])
        } else {
            bytes.into_boxed_slice()
        });
        data.set_align(1);
        self.module
            .define_data(self.rodata_id, &data)
            .map_err(|e| format!("defining {}: {e}", names::RODATA))
    }
}

/// One lowered `puts` statement: rodata `(offset, len)` per literal arg.
struct PutsStmt {
    args: Vec<(u32, usize)>,
    line: Option<u32>,
}
