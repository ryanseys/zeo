//! Program statics: the read-only string blob, the interned-symbol pool
//! (`zeo_syms` + `zeo_unit_init`), the `Str` tables, and the one
//! `zeo_program_desc` the emitted `main` hands `zeo_rt_main`.

use super::emit::Emitter;
use super::names;
use crate::analyze::Analyzed;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use std::collections::HashMap;
use zeo_abi::abi::{self, ProgramDesc, Str};

/// The program's symbol table: names in first-intern order; `zeo_unit_init`
/// interns each at startup into the `zeo_syms` `.bss` array, and emitted
/// code reads ids from there.
#[derive(Default)]
pub(crate) struct SymPool {
    names: Vec<String>,
    map: HashMap<String, u32>,
}

impl SymPool {
    /// The `zeo_syms` index for `name`.
    pub fn intern(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.map.get(name) {
            return i;
        }
        let i = u32::try_from(self.names.len()).expect("under 4G symbols");
        self.names.push(name.to_string());
        self.map.insert(name.to_string(), i);
        i
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }
}

/// Define the `zeo_syms` array (always present -- a zero-symbol program
/// gets a 4-byte placeholder so the eager `symbol_value` in every
/// prologue has something to name).
pub(crate) fn define_syms(em: &mut Emitter) -> Result<(), String> {
    let mut data = DataDescription::new();
    data.define_zeroinit((em.syms_len().max(1) * 4) as usize);
    data.set_align(4);
    em.module
        .define_data(em.syms_id, &data)
        .map_err(|e| format!("defining {}: {e}", names::SYMS))
}

/// `zeo_unit_init`: intern every symbol name into `zeo_syms`. `None` when
/// the program interned nothing.
pub(crate) fn define_unit_init(em: &mut Emitter) -> Result<Option<FuncId>, String> {
    if em.syms.is_empty() {
        return Ok(None);
    }
    let sig = em.module.make_signature();
    let func_id = em
        .module
        .declare_function(names::UNIT_INIT, Linkage::Local, &sig)
        .map_err(|e| format!("declaring {}: {e}", names::UNIT_INIT))?;
    let f_intern = em.import("zeo_rt_sym_intern");

    // Interning may grow rodata, so collect (offset, len) rows first.
    let rows: Vec<(u32, usize)> = {
        let names: Vec<String> = em.syms_names().to_vec();
        names
            .iter()
            .map(|n| (em.intern_rodata(n.as_bytes()), n.len()))
            .collect()
    };

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 2), sig);
    let intern = em.module.declare_func_in_func(f_intern, &mut func);
    let rodata_gv = em.module.declare_data_in_func(em.rodata_id, &mut func);
    let syms_gv = em.module.declare_data_in_func(em.syms_id, &mut func);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fbc);
    let entry = b.create_block();
    b.switch_to_block(entry);
    let rodata = b.ins().symbol_value(em.ptr, rodata_gv);
    let syms = b.ins().symbol_value(em.ptr, syms_gv);
    let fl = MemFlagsData::trusted();
    for (i, (off, len)) in rows.into_iter().enumerate() {
        let ptr = if off == 0 {
            rodata
        } else {
            b.ins().iadd_imm_u(rodata, i64::from(off))
        };
        let len_v = b.ins().iconst(em.ptr, len as i64);
        let call = b.ins().call(intern, &[ptr, len_v]);
        let id = b.func.dfg.inst_results(call)[0];
        b.ins().store(fl, id, syms, (i * 4) as i32);
    }
    b.ins().return_(&[]);
    b.seal_all_blocks();
    b.finalize(cfg);

    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("compiling {}: {e}", names::UNIT_INIT))?;
    Ok(Some(func_id))
}

/// `zeo_program_desc` + the `Str` tables: the loaded-features seed (the
/// same list the rustc backend emits) and the parse warnings.
pub(crate) fn define_desc(
    em: &mut Emitter,
    analyzed: &Analyzed,
    toplevel: FuncId,
    unit_init: Option<FuncId>,
) -> Result<DataId, String> {
    let hir = &analyzed.compiler.hir;
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
        .map(|s| (em.intern_rodata(s.as_bytes()), s.len()))
        .collect();
    let str_size = std::mem::size_of::<Str>();
    let tables_id = em
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
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut tables);
    for (i, &(off, _)) in entries.iter().enumerate() {
        let at = (i * str_size + std::mem::offset_of!(Str, ptr)) as u32;
        tables.write_data_addr(at, rodata_gv, i64::from(off));
    }
    em.module
        .define_data(tables_id, &tables)
        .map_err(|e| format!("defining {}: {e}", names::STR_TABLES))?;

    let desc_id = em
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
    let tables_gv = em.module.declare_data_in_data(tables_id, &mut desc);
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
    let toplevel_ref = em.module.declare_func_in_data(toplevel, &mut desc);
    desc.write_function_addr(
        std::mem::offset_of!(ProgramDesc, toplevel) as u32,
        toplevel_ref,
    );
    if let Some(init) = unit_init {
        let init_ref = em.module.declare_func_in_data(init, &mut desc);
        desc.write_function_addr(
            std::mem::offset_of!(ProgramDesc, unit_init) as u32,
            init_ref,
        );
    }
    em.module
        .define_data(desc_id, &desc)
        .map_err(|e| format!("defining {}: {e}", names::PROGRAM_DESC))?;
    Ok(desc_id)
}

/// The accumulated read-only bytes, defined LAST (interning happens
/// throughout emission).
pub(crate) fn define_rodata(em: &mut Emitter) -> Result<(), String> {
    let mut data = DataDescription::new();
    let bytes = em.take_rodata();
    data.define(if bytes.is_empty() {
        Box::new([0u8])
    } else {
        bytes.into_boxed_slice()
    });
    data.set_align(1);
    em.module
        .define_data(em.rodata_id, &data)
        .map_err(|e| format!("defining {}: {e}", names::RODATA))
}
