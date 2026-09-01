//! Program statics: the read-only string blob, the interned-symbol pool
//! (`zeo_syms` + `zeo_unit_init`), the `Str` tables, and the one
//! `zeo_program_desc` the emitted `main` hands `zeo_rt_main`.

use super::module::Emitter;
use super::names;
use crate::analyze::Analyzed;
use crate::codegen_error::{CResult, CodegenError};
use cranelift_codegen::ir::{self, InstBuilder, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use std::collections::HashMap;
use zeo_abi::abi::{
    self, ClassDesc, CmRow, ForeignRow, MetaRowC, ObjRow, ParamC, ProgramDesc, RegRow, SourceRow,
    Str, UnitRow, VisRow, VmRow,
};

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
pub(crate) fn define_syms(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    data.define_zeroinit((em.syms_len().max(1) * 4) as usize);
    data.set_align(4);
    em.module
        .define_data(em.syms_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::SYMS)))
}

/// Define the `zeo_callsites` array: one zeroed `CallSite` per emitted
/// inline cache. Zero bytes are NOT a valid slot (the runtime's `OnceLock`
/// is not zero-initialisable), so nothing may read one before
/// `zeo_unit_init` has written it -- which is why init runs before the
/// first statement rather than lazily per site.
pub(crate) fn define_callsites(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    data.define_zeroinit(em.callsites.len().max(1) * abi::CALLSITE_SIZE);
    data.set_align(8);
    em.module
        .define_data(em.callsites_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::CALLSITES)))
}

/// Define the `zeo_reopen_flags` array: one zeroed byte per builtin reopen.
/// Zero means "the reopen has not run yet", which is exactly what `.bss`
/// gives, so no `zeo_unit_init` row is needed.
pub(crate) fn define_reopen_flags(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    data.define_zeroinit(em.reopen_flags.len().max(1));
    data.set_align(1);
    em.module
        .define_data(em.reopen_flags_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::REOPEN_FLAGS)))
}

/// Define the `zeo_cm_sites` array -- the class-method caches. Same
/// zero-bytes-are-not-a-slot rule as [`define_callsites`].
pub(crate) fn define_cm_sites(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    data.define_zeroinit(em.cm_sites.max(1) * abi::CLASSMETHOD_SITE_SIZE);
    data.set_align(8);
    em.module
        .define_data(em.cm_sites_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::CM_SITES)))
}

/// Define the `zeo_const_sites` array -- the constant-read caches. Same
/// zero-bytes-are-not-a-slot rule as [`define_callsites`]; `zeo_unit_init`
/// constructs the whole array with ONE bulk call (a slot carries no
/// per-site constant).
pub(crate) fn define_const_sites(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    data.define_zeroinit(em.const_sites.max(1) * abi::CONST_SITE_SIZE);
    data.set_align(8);
    em.module
        .define_data(em.const_sites_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::CONST_SITES)))
}

/// Define the `zeo_new_sites` array -- the compiled-construction caches.
/// Same zero-bytes-are-not-a-slot and one-bulk-call rules as
/// [`define_const_sites`].
pub(crate) fn define_new_sites(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    data.define_zeroinit(em.new_sites.max(1) * abi::NEW_SITE_SIZE);
    data.set_align(8);
    em.module
        .define_data(em.new_sites_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::NEW_SITES)))
}

/// Define the `zeo_dyn_sites` array -- the dynamic-caller caches. Same
/// zero-bytes-are-not-a-slot and one-bulk-call rules as
/// [`define_const_sites`].
pub(crate) fn define_dyn_sites(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    data.define_zeroinit(em.dyn_sites.max(1) * abi::DYNCALLER_SITE_SIZE);
    data.set_align(8);
    em.module
        .define_data(em.dyn_sites_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::DYN_SITES)))
}

/// One block literal's compile-time constants, headed for one
/// `ProcShapeC` row (plus its `ParamC` rows) in `zeo_proc_shapes`.
/// `offset` is the shape's byte offset in the table, fixed at push time
/// (the rows interleave, so earlier shapes' row counts are already
/// known).
pub(crate) struct ProcShapeSpec {
    pub offset: usize,
    pub arity: i32,
    pub flags: u32,
    pub line: u32,
    pub file: String,
    pub outer: String,
    /// The frame label ruby gives this block -- see `ProcShapeC::label`.
    pub label: String,
    pub params: Vec<(u8, String)>,
}

/// Define the `zeo_proc_shapes` table from the specs `emit_proc_new`
/// collected: per spec, the `ProcShapeC` row then its `ParamC` rows,
/// with `params` self-relocated into the same table and every string
/// relocated into rodata.
pub(crate) fn define_proc_shapes(em: &mut Emitter) -> CResult<()> {
    use zeo_abi::abi::{PROC_SHAPE_SIZE, ParamC, ProcShapeC};
    let param_size = std::mem::size_of::<ParamC>();
    let mut bytes = vec![0u8; em.proc_shapes_len.max(1)];
    // Interning may grow rodata; collect every offset first.
    struct Interned {
        file: u32,
        outer: u32,
        label: u32,
        params: Vec<u32>,
    }
    let interned: Vec<Interned> = {
        let specs = std::mem::take(&mut em.proc_shapes);
        let rows = specs
            .iter()
            .map(|s| Interned {
                file: em.intern_rodata(s.file.as_bytes()),
                outer: em.intern_rodata(s.outer.as_bytes()),
                label: em.intern_rodata(s.label.as_bytes()),
                params: s
                    .params
                    .iter()
                    .map(|(_, n)| em.intern_rodata(n.as_bytes()))
                    .collect(),
            })
            .collect();
        em.proc_shapes = specs;
        rows
    };
    let mut data = DataDescription::new();
    for spec in &em.proc_shapes {
        let base = spec.offset;
        let scalar = |bytes: &mut [u8], field: usize, width: usize, v: u64| {
            bytes[base + field..base + field + width].copy_from_slice(&v.to_le_bytes()[..width]);
        };
        scalar(
            &mut bytes,
            std::mem::offset_of!(ProcShapeC, arity),
            4,
            spec.arity as u32 as u64,
        );
        scalar(
            &mut bytes,
            std::mem::offset_of!(ProcShapeC, flags),
            4,
            u64::from(spec.flags),
        );
        scalar(
            &mut bytes,
            std::mem::offset_of!(ProcShapeC, line),
            4,
            u64::from(spec.line),
        );
        scalar(
            &mut bytes,
            std::mem::offset_of!(ProcShapeC, n_params),
            4,
            spec.params.len() as u64,
        );
        let str_len = |bytes: &mut [u8], field: usize, len: usize| {
            let at = base + field + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(len as u64).to_le_bytes());
        };
        str_len(
            &mut bytes,
            std::mem::offset_of!(ProcShapeC, file),
            spec.file.len(),
        );
        str_len(
            &mut bytes,
            std::mem::offset_of!(ProcShapeC, outer),
            spec.outer.len(),
        );
        str_len(
            &mut bytes,
            std::mem::offset_of!(ProcShapeC, label),
            spec.label.len(),
        );
        for (i, (kind, name)) in spec.params.iter().enumerate() {
            let row = base + PROC_SHAPE_SIZE + i * param_size;
            bytes[row + std::mem::offset_of!(ParamC, kind)] = *kind;
            let at = row + std::mem::offset_of!(ParamC, name) + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(name.len() as u64).to_le_bytes());
        }
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    let self_gv = em.module.declare_data_in_data(em.proc_shapes_id, &mut data);
    for (spec, row) in em.proc_shapes.iter().zip(&interned) {
        let base = spec.offset;
        if !spec.params.is_empty() {
            let at = (base + std::mem::offset_of!(ProcShapeC, params)) as u32;
            data.write_data_addr(at, self_gv, (base + PROC_SHAPE_SIZE) as i64);
        }
        let str_ptr = |data: &mut DataDescription, field: usize, off: u32| {
            let at = (base + field + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rodata_gv, i64::from(off));
        };
        str_ptr(&mut data, std::mem::offset_of!(ProcShapeC, file), row.file);
        str_ptr(
            &mut data,
            std::mem::offset_of!(ProcShapeC, outer),
            row.outer,
        );
        str_ptr(
            &mut data,
            std::mem::offset_of!(ProcShapeC, label),
            row.label,
        );
        for (i, off) in row.params.iter().enumerate() {
            let at = (base
                + PROC_SHAPE_SIZE
                + i * param_size
                + std::mem::offset_of!(ParamC, name)
                + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rodata_gv, i64::from(*off));
        }
    }
    em.module
        .define_data(em.proc_shapes_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::PROC_SHAPES)))
}

/// `zeo_unit_init`: intern every symbol name into `zeo_syms`, then hand
/// every `zeo_callsites` slot its caller class and initialise every
/// `zeo_cm_sites`, `zeo_const_sites` and `zeo_new_sites` slot. `None`
/// when the program has none of them.
///
/// Every array goes through ONE bulk capi call over a rodata table --
/// the old per-symbol/per-site unrolled bodies were the largest cold
/// text in small programs (~4 instructions per symbol).
pub(crate) fn define_unit_init(
    em: &mut Emitter,
    extra_inits: &[String],
) -> CResult<Option<FuncId>> {
    if em.syms.is_empty()
        && em.callsites.is_empty()
        && em.cm_sites == 0
        && em.const_sites == 0
        && em.new_sites == 0
        && em.dyn_sites == 0
        && extra_inits.is_empty()
    {
        return Ok(None);
    }
    // A package's initializer is EXPORTED under its prefix: the host's own
    // unit_init calls it, so its local site tables fill at the same moment
    // the host's do.
    let (init_sym, linkage) = match &em.pkg {
        Some(pkg) => (format!("{}_unit_init", pkg.prefix()), Linkage::Export),
        None => (names::UNIT_INIT.to_string(), Linkage::Local),
    };
    let sig = em.module.make_signature();
    let func_id = em
        .module
        .declare_function(&init_sym, linkage, &sig)
        .map_err(|e| CodegenError::internal(format!("declaring {init_sym}: {e}")))?;
    // A merged package's exported initializer, called after this object's
    // own tables fill.
    let extra_ids: Vec<FuncId> = extra_inits
        .iter()
        .map(|name| {
            let esig = em.module.make_signature();
            em.module
                .declare_function(name, Linkage::Import, &esig)
                .map_err(|e| CodegenError::internal(format!("declaring {name}: {e}")))
        })
        .collect::<CResult<_>>()?;

    // Interning may grow rodata, so collect (offset, len) rows first.
    let rows: Vec<(u32, usize)> = {
        let names: Vec<String> = em.syms_names().to_vec();
        names
            .iter()
            .map(|n| (em.intern_rodata(n.as_bytes()), n.len()))
            .collect()
    };
    // The `Str` table `zeo_rt_syms_init` walks: len as a scalar, ptr
    // relocated into rodata per row.
    let sym_rows_id = if rows.is_empty() {
        None
    } else {
        let id = em
            .module
            .declare_data(names::SYM_ROWS, Linkage::Local, false, false)
            .map_err(|e| CodegenError::internal(format!("declaring {}: {e}", names::SYM_ROWS)))?;
        let str_size = std::mem::size_of::<Str>();
        let mut bytes = vec![0u8; rows.len() * str_size];
        for (i, (_, len)) in rows.iter().enumerate() {
            let at = i * str_size + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(*len as u64).to_le_bytes());
        }
        let mut data = DataDescription::new();
        data.define(bytes.into_boxed_slice());
        data.set_align(8);
        let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
        for (i, (off, _)) in rows.iter().enumerate() {
            let at = (i * str_size + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rodata_gv, i64::from(*off));
        }
        em.module
            .define_data(id, &data)
            .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::SYM_ROWS)))?;
        Some(id)
    };
    let n_syms = rows.len();

    let callers: Vec<u32> = em.callsites.clone();
    // The caller-class blob `zeo_rt_callsites_init` reads alongside the
    // `.bss` sites.
    let callers_id = if callers.is_empty() {
        None
    } else if let Some(pkg) = &em.pkg {
        // The blob carries CLASS IDS, and only the HOST knows their final
        // values: the manifest ships the locals, and the host defines this
        // symbol with the band it assigned (`pkg::merge_rows`).
        let id = em
            .module
            .declare_data(
                &format!("{}_callers", pkg.prefix()),
                Linkage::Import,
                false,
                false,
            )
            .map_err(|e| CodegenError::internal(format!("declaring the callers import: {e}")))?;
        Some(id)
    } else {
        let id = em
            .module
            .declare_data(names::CALLSITE_CALLERS, Linkage::Local, false, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", names::CALLSITE_CALLERS))
            })?;
        let mut bytes = Vec::with_capacity(callers.len() * 4);
        for c in &callers {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let mut data = DataDescription::new();
        data.define(bytes.into_boxed_slice());
        data.set_align(4);
        em.module.define_data(id, &data).map_err(|e| {
            CodegenError::internal(format!("defining {}: {e}", names::CALLSITE_CALLERS))
        })?;
        Some(id)
    };

    let f_syms_init = em.import("zeo_rt_syms_init");
    let f_site_init = em.import("zeo_rt_callsites_init");
    let f_cm_init = em.import("zeo_rt_cm_sites_init");
    let n_cm = em.cm_sites;
    let f_const_init = em.import("zeo_rt_const_sites_init");
    let n_const = em.const_sites;
    let f_new_init = em.import("zeo_rt_class_new_sites_init");
    let n_new = em.new_sites;
    let f_dyn_init = em.import("zeo_rt_dyncaller_sites_init");
    let n_dyn = em.dyn_sites;

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 2), sig);
    let syms_init = em.module.declare_func_in_func(f_syms_init, &mut func);
    let site_init = em.module.declare_func_in_func(f_site_init, &mut func);
    let sym_rows_gv = sym_rows_id.map(|id| em.module.declare_data_in_func(id, &mut func));
    let callers_gv = callers_id.map(|id| em.module.declare_data_in_func(id, &mut func));
    let syms_gv = em.module.declare_data_in_func(em.syms_id, &mut func);
    let sites_gv = em.module.declare_data_in_func(em.callsites_id, &mut func);
    let cm_gv = em.module.declare_data_in_func(em.cm_sites_id, &mut func);
    let cm_init = em.module.declare_func_in_func(f_cm_init, &mut func);
    let const_gv = em.module.declare_data_in_func(em.const_sites_id, &mut func);
    let const_init = em.module.declare_func_in_func(f_const_init, &mut func);
    let new_gv = em.module.declare_data_in_func(em.new_sites_id, &mut func);
    let new_init = em.module.declare_func_in_func(f_new_init, &mut func);
    let dyn_gv = em.module.declare_data_in_func(em.dyn_sites_id, &mut func);
    let dyn_init = em.module.declare_func_in_func(f_dyn_init, &mut func);
    let extra_refs: Vec<ir::FuncRef> = extra_ids
        .iter()
        .map(|&id| em.module.declare_func_in_func(id, &mut func))
        .collect();
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fbc);
    let entry = b.create_block();
    b.switch_to_block(entry);
    if let Some(rows_gv) = sym_rows_gv {
        let rows_v = b.ins().symbol_value(em.ptr, rows_gv);
        let n_v = b.ins().iconst(em.ptr, n_syms as i64);
        let syms = b.ins().symbol_value(em.ptr, syms_gv);
        b.ins().call(syms_init, &[rows_v, n_v, syms]);
    }
    if let Some(callers_gv) = callers_gv {
        let sites = b.ins().symbol_value(em.ptr, sites_gv);
        let callers_v = b.ins().symbol_value(em.ptr, callers_gv);
        let n_v = b.ins().iconst(em.ptr, callers.len() as i64);
        b.ins().call(site_init, &[sites, callers_v, n_v]);
    }
    if n_cm > 0 {
        let cm = b.ins().symbol_value(em.ptr, cm_gv);
        let n_v = b.ins().iconst(em.ptr, n_cm as i64);
        b.ins().call(cm_init, &[cm, n_v]);
    }
    if n_const > 0 {
        let base = b.ins().symbol_value(em.ptr, const_gv);
        let n_v = b.ins().iconst(em.ptr, n_const as i64);
        b.ins().call(const_init, &[base, n_v]);
    }
    if n_new > 0 {
        let base = b.ins().symbol_value(em.ptr, new_gv);
        let n_v = b.ins().iconst(em.ptr, n_new as i64);
        b.ins().call(new_init, &[base, n_v]);
    }
    if n_dyn > 0 {
        let base = b.ins().symbol_value(em.ptr, dyn_gv);
        let n_v = b.ins().iconst(em.ptr, n_dyn as i64);
        b.ins().call(dyn_init, &[base, n_v]);
    }
    for extra in extra_refs {
        b.ins().call(extra, &[]);
    }
    b.ins().return_(&[]);
    b.seal_all_blocks();
    b.finalize(cfg);

    em.record_clif(&init_sym, &func);
    em.define(func_id, func, &init_sym, false)?;
    Ok(Some(func_id))
}

/// One `VmRow` (a value-channel method) to serialize.
pub(crate) struct VmRowSpec {
    pub class: u32,
    pub box_id: u32,
    pub name: String,
    pub f: FuncId,
}

/// One `VisRow` (a visibility stamp) to serialize.
pub(crate) struct VisRowSpec {
    pub class: u32,
    pub name: String,
    pub verb: u8,
}

/// One row's cells for [`define_rows`]: where each column sits in the row
/// struct and what goes there.
struct RowCells {
    /// `Str` columns as `(field offset, contents)`; the driver interns the
    /// contents into rodata, writes `Str::len`, and relocates `Str::ptr`.
    strs: Vec<(usize, String)>,
    /// Function-pointer columns as `(field offset, function)`.
    funcs: Vec<(usize, FuncId)>,
    /// Little-endian scalar columns as `(field offset, byte width, value)`.
    scalars: Vec<(usize, usize, u64)>,
}

/// The shared spine of the simple row tables: `Ok(None)` when `rows` is
/// empty; declare `sym`; intern every string column into rodata in row
/// order; write the scalar columns and every `Str::len` into one
/// `size * rows.len()` blob; align 8; then relocate per row -- string
/// `ptr`s first, function pointers second -- and define. The per-row
/// reloc order is what every hand-rolled builder emitted, so converting
/// one changes no bytes.
fn define_rows<R>(
    em: &mut Emitter,
    sym: &str,
    size: usize,
    rows: &[R],
    cells: impl Fn(&R) -> RowCells,
) -> CResult<Option<DataId>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let id = em
        .module
        .declare_data(sym, Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring {sym}: {e}")))?;
    let cells: Vec<RowCells> = rows.iter().map(cells).collect();
    let interned: Vec<Vec<u32>> = cells
        .iter()
        .map(|c| {
            c.strs
                .iter()
                .map(|(_, s)| em.intern_rodata(s.as_bytes()))
                .collect()
        })
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, c) in cells.iter().enumerate() {
        let base = i * size;
        for &(field, width, v) in &c.scalars {
            bytes[base + field..base + field + width].copy_from_slice(&v.to_le_bytes()[..width]);
        }
        for (field, s) in &c.strs {
            let at = base + field + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(s.len() as u64).to_le_bytes());
        }
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, c) in cells.iter().enumerate() {
        let base = i * size;
        for ((field, _), off) in c.strs.iter().zip(&interned[i]) {
            let at = (base + field + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rodata_gv, i64::from(*off));
        }
        for &(field, f) in &c.funcs {
            let f_ref = em.module.declare_func_in_data(f, &mut data);
            data.write_function_addr((base + field) as u32, f_ref);
        }
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {sym}: {e}")))?;
    Ok(Some(id))
}

/// The `zeo_vm_rows` table: `VmRow` structs with name/function relocs.
fn define_vm_rows(em: &mut Emitter, rows: &[VmRowSpec]) -> CResult<Option<DataId>> {
    define_rows(em, "zeo_vm_rows", std::mem::size_of::<VmRow>(), rows, |r| {
        RowCells {
            strs: vec![(std::mem::offset_of!(VmRow, name), r.name.clone())],
            funcs: vec![(std::mem::offset_of!(VmRow, f), r.f)],
            scalars: vec![
                (std::mem::offset_of!(VmRow, class), 4, u64::from(r.class)),
                (std::mem::offset_of!(VmRow, box_id), 4, u64::from(r.box_id)),
                (std::mem::offset_of!(VmRow, flags), 4, 0),
            ],
        }
    })
}

/// A method's `.rodata` `ParamDescC` (+ its keyword rows): what the bound
/// trampoline hands `zeo_rt_bind_params`. Anonymous data objects; every
/// string lives in the rodata blob.
pub(crate) fn define_param_desc(
    em: &mut Emitter,
    spec: &super::params::ParamDescSpec<'_>,
) -> CResult<DataId> {
    use zeo_abi::abi::{KwParamC, PARAM_STAR_ANON, PARAM_STAR_NAMED, PARAM_STAR_NONE, ParamDescC};
    let p = spec.params;
    let put_u32 = |bytes: &mut [u8], at: usize, v: u32| {
        bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    };
    let put_usize = |bytes: &mut [u8], at: usize, v: usize| {
        bytes[at..at + 8].copy_from_slice(&(v as u64).to_le_bytes());
    };
    let str_len_at = |bytes: &mut [u8], field: usize, s: &str| {
        let at = field + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(s.len() as u64).to_le_bytes());
    };

    let kws_id = if p.keywords.is_empty() {
        None
    } else {
        let size = std::mem::size_of::<KwParamC>();
        let mut bytes = vec![0u8; size * p.keywords.len()];
        let mut offs = Vec::with_capacity(p.keywords.len());
        for (i, kw) in p.keywords.iter().enumerate() {
            let base = i * size;
            let (name, required) = match kw {
                crate::hir::KeywordParam::Required(n) => (n, 1u8),
                crate::hir::KeywordParam::Optional(n, _) => (n, 0u8),
            };
            offs.push(em.intern_rodata(name.as_bytes()));
            str_len_at(
                &mut bytes,
                base + std::mem::offset_of!(KwParamC, name),
                name,
            );
            bytes[base + std::mem::offset_of!(KwParamC, required)] = required;
        }
        let mut data = DataDescription::new();
        data.define(bytes.into_boxed_slice());
        data.set_align(8);
        let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
        for (i, off) in offs.iter().enumerate() {
            let at = (i * size
                + std::mem::offset_of!(KwParamC, name)
                + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rodata_gv, i64::from(*off));
        }
        let id = em
            .module
            .declare_anonymous_data(false, false)
            .map_err(|e| CodegenError::internal(format!("declaring a kw table: {e}")))?;
        em.module
            .define_data(id, &data)
            .map_err(|e| CodegenError::internal(format!("defining a kw table: {e}")))?;
        Some(id)
    };

    let star = |r: &Option<Option<String>>| match r {
        None => PARAM_STAR_NONE,
        Some(None) => PARAM_STAR_ANON,
        Some(Some(_)) => PARAM_STAR_NAMED,
    };
    let file = spec.file.unwrap_or("");
    let mut bytes = vec![0u8; std::mem::size_of::<ParamDescC>()];
    put_u32(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, nreq),
        p.required.len() as u32,
    );
    put_u32(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, nopt),
        p.optional.len() as u32,
    );
    put_u32(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, npost),
        p.post.len() as u32,
    );
    bytes[std::mem::offset_of!(ParamDescC, rest)] = star(&p.rest);
    bytes[std::mem::offset_of!(ParamDescC, kwrest)] = star(&p.keyword_rest);
    bytes[std::mem::offset_of!(ParamDescC, no_keywords)] = u8::from(p.no_keywords);
    bytes[std::mem::offset_of!(ParamDescC, implicit_rest)] = u8::from(p.implicit_rest);
    put_usize(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, n_kws),
        p.keywords.len(),
    );
    str_len_at(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, name),
        spec.name,
    );
    str_len_at(&mut bytes, std::mem::offset_of!(ParamDescC, file), file);
    str_len_at(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, label),
        spec.label,
    );
    put_u32(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, line),
        spec.line,
    );
    put_u32(
        &mut bytes,
        std::mem::offset_of!(ParamDescC, end_line),
        spec.end_line,
    );

    let name_off = em.intern_rodata(spec.name.as_bytes());
    let file_off = em.intern_rodata(file.as_bytes());
    let label_off = em.intern_rodata(spec.label.as_bytes());
    let mut data = DataDescription::new();
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    let str_ptr = |data: &mut DataDescription, field: usize, off: u32| {
        data.write_data_addr(
            (field + std::mem::offset_of!(Str, ptr)) as u32,
            rodata_gv,
            i64::from(off),
        );
    };
    str_ptr(&mut data, std::mem::offset_of!(ParamDescC, name), name_off);
    str_ptr(&mut data, std::mem::offset_of!(ParamDescC, file), file_off);
    str_ptr(
        &mut data,
        std::mem::offset_of!(ParamDescC, label),
        label_off,
    );
    if let Some(kid) = kws_id {
        let gv = em.module.declare_data_in_data(kid, &mut data);
        data.write_data_addr(std::mem::offset_of!(ParamDescC, kws) as u32, gv, 0);
    }
    let id = em
        .module
        .declare_anonymous_data(false, false)
        .map_err(|e| CodegenError::internal(format!("declaring a ParamDesc: {e}")))?;
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining a ParamDesc: {e}")))?;
    Ok(id)
}

/// One class-method dispatch row (`CmRow`): `def self.x`'s trampoline on
/// the class-method channel.
pub(crate) struct CmRowSpec {
    pub class: u32,
    /// The box the `def self.x` was written in; 0 is main.
    pub box_id: u32,
    pub name: String,
    pub f: FuncId,
}

/// One `RegRow` -- the non-method registrar calls, in program order. The
/// only kind so far marks a class's OWN class-method names; `a` carries
/// the name, every other field is zero.
pub(crate) struct RegRowSpec {
    pub kind: u8,
    pub class: u32,
    pub a: String,
    /// The alias kinds' OLD name; empty for every other kind.
    pub b: String,
    /// A super-target row's trampoline; None for the mark kinds.
    pub f: Option<FuncId>,
    /// `REG_EXTENDS`' module ids; empty for every other kind.
    pub ids: Vec<u32>,
    /// `REG_REGISTER_BUILTIN`'s is-a-module bit; 0 for every other kind.
    pub flag: u8,
}

/// One `ObjRow` (an object-channel method on a compiled class).
pub(crate) struct ObjRowSpec {
    pub class: u32,
    pub name: String,
    pub f: FuncId,
}

/// The `zeo_vm_foreign` table: rows a builtin reopen INHERITED, marked
/// so a `super` walk skips them at that position.
fn define_foreign_rows(em: &mut Emitter, rows: &[(u32, String)]) -> CResult<Option<DataId>> {
    define_rows(
        em,
        "zeo_vm_foreign",
        std::mem::size_of::<ForeignRow>(),
        rows,
        |(class, name)| RowCells {
            strs: vec![(std::mem::offset_of!(ForeignRow, name), name.clone())],
            funcs: vec![],
            scalars: vec![(
                std::mem::offset_of!(ForeignRow, class),
                4,
                u64::from(*class),
            )],
        },
    )
}

/// The `zeo_obj_rows` table (same shape as `zeo_vm_rows`, minus box/flags).
fn define_obj_rows(em: &mut Emitter, rows: &[ObjRowSpec]) -> CResult<Option<DataId>> {
    define_rows(
        em,
        "zeo_obj_rows",
        std::mem::size_of::<ObjRow>(),
        rows,
        |r| RowCells {
            strs: vec![(std::mem::offset_of!(ObjRow, name), r.name.clone())],
            funcs: vec![(std::mem::offset_of!(ObjRow, f), r.f)],
            scalars: vec![(std::mem::offset_of!(ObjRow, class), 4, u64::from(r.class))],
        },
    )
}

fn define_cm_rows(em: &mut Emitter, rows: &[CmRowSpec]) -> CResult<Option<DataId>> {
    define_rows(em, "zeo_cm_rows", std::mem::size_of::<CmRow>(), rows, |r| {
        RowCells {
            strs: vec![(std::mem::offset_of!(CmRow, name), r.name.clone())],
            funcs: vec![(std::mem::offset_of!(CmRow, f), r.f)],
            scalars: vec![
                (std::mem::offset_of!(CmRow, class), 4, u64::from(r.class)),
                (std::mem::offset_of!(CmRow, box_id), 4, u64::from(r.box_id)),
            ],
        }
    })
}

/// The `UnitRow` table: one row per SPELLING a `require` can use for a
/// compiled-in load-path file (the load-path-relative feature name and the
/// absolute path), both pointing at the same unit function.
fn define_unit_rows(em: &mut Emitter, rows: &[(String, FuncId)]) -> CResult<Option<DataId>> {
    define_rows(
        em,
        "zeo_unit_rows",
        std::mem::size_of::<UnitRow>(),
        rows,
        |(name, f)| RowCells {
            strs: vec![(std::mem::offset_of!(UnitRow, feature), name.clone())],
            funcs: vec![(std::mem::offset_of!(UnitRow, f), *f)],
            scalars: vec![],
        },
    )
}

/// The `SourceRow` table: `--embed-sources`' pack, one row per file.
fn define_source_rows(em: &mut Emitter, rows: &[(String, String)]) -> CResult<Option<DataId>> {
    define_rows(
        em,
        "zeo_source_pack",
        std::mem::size_of::<SourceRow>(),
        rows,
        |(path, text)| RowCells {
            strs: vec![
                (std::mem::offset_of!(SourceRow, path), path.clone()),
                (std::mem::offset_of!(SourceRow, text), text.clone()),
            ],
            funcs: vec![],
            scalars: vec![],
        },
    )
}

fn define_reg_rows(em: &mut Emitter, rows: &[RegRowSpec]) -> CResult<Option<DataId>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<RegRow>();
    let id = em
        .module
        .declare_data("zeo_reg_rows", Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring zeo_reg_rows: {e}")))?;
    let interned: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.a.as_bytes()))
        .collect();
    let interned_b: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.b.as_bytes()))
        .collect();
    // One shared u32 array holds every row's `ids` run (`REG_EXTENDS`'
    // module lists); each row points into it at its offset.
    let mut ids_bytes: Vec<u8> = Vec::new();
    let mut ids_offsets: Vec<usize> = Vec::with_capacity(rows.len());
    for row in rows {
        ids_offsets.push(ids_bytes.len());
        for id in &row.ids {
            ids_bytes.extend_from_slice(&id.to_le_bytes());
        }
    }
    let ids_id = if ids_bytes.is_empty() {
        None
    } else {
        let id = em
            .module
            .declare_data("zeo_reg_row_ids", Linkage::Local, false, false)
            .map_err(|e| CodegenError::internal(format!("declaring zeo_reg_row_ids: {e}")))?;
        let mut d = DataDescription::new();
        d.define(ids_bytes.into_boxed_slice());
        d.set_align(4);
        em.module
            .define_data(id, &d)
            .map_err(|e| CodegenError::internal(format!("defining zeo_reg_row_ids: {e}")))?;
        Some(id)
    };
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        bytes[base + std::mem::offset_of!(RegRow, kind)] = row.kind;
        bytes[base + std::mem::offset_of!(RegRow, flag)] = row.flag;
        bytes[base + std::mem::offset_of!(RegRow, class)
            ..base + std::mem::offset_of!(RegRow, class) + 4]
            .copy_from_slice(&row.class.to_le_bytes());
        let at = base + std::mem::offset_of!(RegRow, a) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(row.a.len() as u64).to_le_bytes());
        let bt = base + std::mem::offset_of!(RegRow, b) + std::mem::offset_of!(Str, len);
        bytes[bt..bt + 8].copy_from_slice(&(row.b.len() as u64).to_le_bytes());
        let n_at = base + std::mem::offset_of!(RegRow, n_ids);
        bytes[n_at..n_at + 8].copy_from_slice(&(row.ids.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    if let Some(ids_id) = ids_id {
        let ids_gv = em.module.declare_data_in_data(ids_id, &mut data);
        for (i, row) in rows.iter().enumerate() {
            if row.ids.is_empty() {
                continue;
            }
            let at = (i * size + std::mem::offset_of!(RegRow, ids)) as u32;
            data.write_data_addr(at, ids_gv, ids_offsets[i] as i64);
        }
    }
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, &off) in interned.iter().enumerate() {
        let base = i * size;
        let a_at = (base + std::mem::offset_of!(RegRow, a) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(a_at, rodata_gv, i64::from(off));
    }
    for (i, &off) in interned_b.iter().enumerate() {
        let base = i * size;
        let b_at = (base + std::mem::offset_of!(RegRow, b) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(b_at, rodata_gv, i64::from(off));
    }
    for (i, row) in rows.iter().enumerate() {
        let Some(f) = row.f else { continue };
        let f_ref = em.module.declare_func_in_data(f, &mut data);
        let base = i * size;
        data.write_function_addr((base + std::mem::offset_of!(RegRow, f)) as u32, f_ref);
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining zeo_reg_rows: {e}")))?;
    Ok(Some(id))
}

/// The `zeo_classes` table plus its two auxiliary arrays: the linearized
/// ancestor ids and the ivar-name `Str` entries every `ClassDesc` points
/// into.
fn define_classes(
    em: &mut Emitter,
    classes: &[super::classes::ClassSpec],
) -> CResult<Option<DataId>> {
    if classes.is_empty() {
        return Ok(None);
    }
    // Ancestor ids, one shared u32 array.
    let anc_id = em
        .module
        .declare_data("zeo_class_ancestors", Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring zeo_class_ancestors: {e}")))?;
    let mut anc_bytes = Vec::new();
    let mut anc_offsets = Vec::with_capacity(classes.len());
    for c in classes {
        anc_offsets.push(anc_bytes.len());
        for a in &c.ancestors {
            anc_bytes.extend_from_slice(&a.to_le_bytes());
        }
    }
    let mut anc = DataDescription::new();
    anc.define(anc_bytes.into_boxed_slice());
    anc.set_align(4);
    em.module
        .define_data(anc_id, &anc)
        .map_err(|e| CodegenError::internal(format!("defining zeo_class_ancestors: {e}")))?;

    // Ivar names, one shared Str array.
    let str_size = std::mem::size_of::<Str>();
    let n_ivars: usize = classes.iter().map(|c| c.ivars.len()).sum();
    let ivars_id = (n_ivars > 0)
        .then(|| {
            em.module
                .declare_data("zeo_class_ivars", Linkage::Local, false, false)
                .map_err(|e| CodegenError::internal(format!("declaring zeo_class_ivars: {e}")))
        })
        .transpose()?;
    let mut ivar_offsets = Vec::with_capacity(classes.len());
    if let Some(ivars_id) = ivars_id {
        let mut entries: Vec<(u32, usize)> = Vec::with_capacity(n_ivars);
        for c in classes {
            ivar_offsets.push(entries.len() * str_size);
            for iv in &c.ivars {
                entries.push((em.intern_rodata(iv.as_bytes()), iv.len()));
            }
        }
        let mut data = DataDescription::new();
        let mut bytes = vec![0u8; str_size * entries.len()];
        for (i, &(_, len)) in entries.iter().enumerate() {
            let at = i * str_size + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(len as u64).to_le_bytes());
        }
        data.define(bytes.into_boxed_slice());
        data.set_align(8);
        let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
        for (i, &(off, _)) in entries.iter().enumerate() {
            let at = (i * str_size + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rodata_gv, i64::from(off));
        }
        em.module
            .define_data(ivars_id, &data)
            .map_err(|e| CodegenError::internal(format!("defining zeo_class_ivars: {e}")))?;
    } else {
        ivar_offsets.resize(classes.len(), 0);
    }

    // A compiled Struct/Data's member names, one shared Str array.
    let n_members: usize = classes.iter().map(|c| c.members.len()).sum();
    let members_id = (n_members > 0)
        .then(|| {
            em.module
                .declare_data("zeo_class_members", Linkage::Local, false, false)
                .map_err(|e| CodegenError::internal(format!("declaring zeo_class_members: {e}")))
        })
        .transpose()?;
    let mut member_offsets = Vec::with_capacity(classes.len());
    if let Some(members_id) = members_id {
        let mut entries: Vec<(u32, usize)> = Vec::with_capacity(n_members);
        for c in classes {
            member_offsets.push(entries.len() * str_size);
            for m in &c.members {
                entries.push((em.intern_rodata(m.as_bytes()), m.len()));
            }
        }
        let mut data = DataDescription::new();
        let mut bytes = vec![0u8; str_size * entries.len()];
        for (i, &(_, len)) in entries.iter().enumerate() {
            let at = i * str_size + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(len as u64).to_le_bytes());
        }
        data.define(bytes.into_boxed_slice());
        data.set_align(8);
        let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
        for (i, &(off, _)) in entries.iter().enumerate() {
            let at = (i * str_size + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rodata_gv, i64::from(off));
        }
        em.module
            .define_data(members_id, &data)
            .map_err(|e| CodegenError::internal(format!("defining zeo_class_members: {e}")))?;
    } else {
        member_offsets.resize(classes.len(), 0);
    }

    let size = std::mem::size_of::<ClassDesc>();
    let id = em
        .module
        .declare_data("zeo_classes", Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring zeo_classes: {e}")))?;
    let name_offs: Vec<u32> = classes
        .iter()
        .map(|c| em.intern_rodata(c.name.as_bytes()))
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * classes.len()];
    let put_u64 = |bytes: &mut [u8], at: usize, v: u64| {
        bytes[at..at + 8].copy_from_slice(&v.to_le_bytes());
    };
    for (i, c) in classes.iter().enumerate() {
        let base = i * size;
        bytes[base + std::mem::offset_of!(ClassDesc, id)
            ..base + std::mem::offset_of!(ClassDesc, id) + 4]
            .copy_from_slice(&c.id.to_le_bytes());
        bytes[base + std::mem::offset_of!(ClassDesc, kind)] = c.kind;
        let at = base + std::mem::offset_of!(ClassDesc, name) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(c.name.len() as u64).to_le_bytes());
        put_u64(
            &mut bytes,
            base + std::mem::offset_of!(ClassDesc, n_ancestors),
            c.ancestors.len() as u64,
        );
        put_u64(
            &mut bytes,
            base + std::mem::offset_of!(ClassDesc, n_ivars),
            c.ivars.len() as u64,
        );
        put_u64(
            &mut bytes,
            base + std::mem::offset_of!(ClassDesc, n_members),
            c.members.len() as u64,
        );
        bytes[base + std::mem::offset_of!(ClassDesc, hidden)
            ..base + std::mem::offset_of!(ClassDesc, hidden) + 2]
            .copy_from_slice(&c.hidden.to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    let anc_gv = em.module.declare_data_in_data(anc_id, &mut data);
    let ivars_gv = ivars_id.map(|iv| em.module.declare_data_in_data(iv, &mut data));
    let members_gv = members_id.map(|m| em.module.declare_data_in_data(m, &mut data));
    for (i, c) in classes.iter().enumerate() {
        let base = i * size;
        let name_at =
            (base + std::mem::offset_of!(ClassDesc, name) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(name_at, rodata_gv, i64::from(name_offs[i]));
        data.write_data_addr(
            (base + std::mem::offset_of!(ClassDesc, ancestors)) as u32,
            anc_gv,
            anc_offsets[i] as i64,
        );
        if !c.ivars.is_empty() {
            let gv = ivars_gv.expect("ivar table exists when any class has ivars");
            data.write_data_addr(
                (base + std::mem::offset_of!(ClassDesc, ivar_names)) as u32,
                gv,
                ivar_offsets[i] as i64,
            );
        }
        if !c.members.is_empty() {
            let gv = members_gv.expect("member table exists when any class has members");
            data.write_data_addr(
                (base + std::mem::offset_of!(ClassDesc, members)) as u32,
                gv,
                member_offsets[i] as i64,
            );
        }
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining zeo_classes: {e}")))?;
    Ok(Some(id))
}

/// One method's reflection row: the signature, the `def` keyword's own
/// line, and the name an alias came from -- everything `#arity`,
/// `#parameters`, `#source_location` and `#inspect` read back.
pub(crate) struct MetaRowSpec {
    pub class: u32,
    pub singleton: bool,
    pub name: String,
    /// `(kind, name)` in ruby's own report order; an empty name is a bare
    /// `*` a C function would report.
    pub params: Vec<(u8, String)>,
    /// The `def`'s file and line; empty file = no source location.
    pub file: String,
    pub line: u32,
    /// The original name when this row is an alias; empty otherwise.
    pub aliased_from: String,
}

/// A reflection-row table + its one shared parameter array. `sym` names the
/// pair, so the redefinition-timeline rows can live in a table of their own.
fn define_meta_rows(em: &mut Emitter, rows: &[MetaRowSpec], sym: &str) -> CResult<Option<DataId>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<MetaRowC>();
    let psize = std::mem::size_of::<ParamC>();
    let id = em
        .module
        .declare_data(sym, Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring {sym}: {e}")))?;
    let names: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.name.as_bytes()))
        .collect();
    let files: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.file.as_bytes()))
        .collect();
    let aliases: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.aliased_from.as_bytes()))
        .collect();
    // One shared array holds every row's parameter run; each row points
    // into it at its own offset.
    let mut param_names: Vec<Vec<u32>> = Vec::with_capacity(rows.len());
    let mut param_offsets: Vec<usize> = Vec::with_capacity(rows.len());
    let mut n_params = 0usize;
    for r in rows {
        param_offsets.push(n_params * psize);
        n_params += r.params.len();
        param_names.push(
            r.params
                .iter()
                .map(|(_, n)| em.intern_rodata(n.as_bytes()))
                .collect(),
        );
    }
    let params_id = if n_params == 0 {
        None
    } else {
        let pid = em
            .module
            .declare_data(&format!("{sym}_params"), Linkage::Local, false, false)
            .map_err(|e| CodegenError::internal(format!("declaring {sym}_params: {e}")))?;
        let mut pd = DataDescription::new();
        let mut pbytes = vec![0u8; psize * n_params];
        let mut i = 0usize;
        for r in rows {
            for (kind, name) in &r.params {
                let base = i * psize;
                pbytes[base + std::mem::offset_of!(ParamC, kind)] = *kind;
                let at = base + std::mem::offset_of!(ParamC, name) + std::mem::offset_of!(Str, len);
                pbytes[at..at + 8].copy_from_slice(&(name.len() as u64).to_le_bytes());
                i += 1;
            }
        }
        pd.define(pbytes.into_boxed_slice());
        pd.set_align(8);
        let rod = em.module.declare_data_in_data(em.rodata_id, &mut pd);
        let mut i = 0usize;
        for offs in &param_names {
            for &off in offs {
                let at = (i * psize
                    + std::mem::offset_of!(ParamC, name)
                    + std::mem::offset_of!(Str, ptr)) as u32;
                pd.write_data_addr(at, rod, i64::from(off));
                i += 1;
            }
        }
        em.module
            .define_data(pid, &pd)
            .map_err(|e| CodegenError::internal(format!("defining zeo_meta_params: {e}")))?;
        Some(pid)
    };
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, r) in rows.iter().enumerate() {
        let base = i * size;
        let at = base + std::mem::offset_of!(MetaRowC, class);
        bytes[at..at + 4].copy_from_slice(&r.class.to_le_bytes());
        bytes[base + std::mem::offset_of!(MetaRowC, singleton)] = u8::from(r.singleton);
        let at = base + std::mem::offset_of!(MetaRowC, line);
        bytes[at..at + 4].copy_from_slice(&r.line.to_le_bytes());
        for (field, len) in [
            (std::mem::offset_of!(MetaRowC, name), r.name.len()),
            (std::mem::offset_of!(MetaRowC, file), r.file.len()),
            (
                std::mem::offset_of!(MetaRowC, aliased_from),
                r.aliased_from.len(),
            ),
        ] {
            let at = base + field + std::mem::offset_of!(Str, len);
            bytes[at..at + 8].copy_from_slice(&(len as u64).to_le_bytes());
        }
        let n_at = base + std::mem::offset_of!(MetaRowC, n_params);
        bytes[n_at..n_at + 8].copy_from_slice(&(r.params.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    if let Some(pid) = params_id {
        let pgv = em.module.declare_data_in_data(pid, &mut data);
        for (i, r) in rows.iter().enumerate() {
            if r.params.is_empty() {
                continue;
            }
            let at = (i * size + std::mem::offset_of!(MetaRowC, params)) as u32;
            data.write_data_addr(at, pgv, param_offsets[i] as i64);
        }
    }
    let rod = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for i in 0..rows.len() {
        let base = i * size;
        for (field, off) in [
            (std::mem::offset_of!(MetaRowC, name), names[i]),
            (std::mem::offset_of!(MetaRowC, file), files[i]),
            (std::mem::offset_of!(MetaRowC, aliased_from), aliases[i]),
        ] {
            let at = (base + field + std::mem::offset_of!(Str, ptr)) as u32;
            data.write_data_addr(at, rod, i64::from(off));
        }
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {sym}: {e}")))?;
    Ok(Some(id))
}

/// The `zeo_vis_rows` table.
fn define_vis_rows(em: &mut Emitter, rows: &[VisRowSpec]) -> CResult<Option<DataId>> {
    define_rows(
        em,
        "zeo_vis_rows",
        std::mem::size_of::<VisRow>(),
        rows,
        |r| RowCells {
            strs: vec![(std::mem::offset_of!(VisRow, name), r.name.clone())],
            funcs: vec![],
            scalars: vec![
                (std::mem::offset_of!(VisRow, class), 4, u64::from(r.class)),
                (std::mem::offset_of!(VisRow, verb), 1, u64::from(r.verb)),
            ],
        },
    )
}

/// The row tables `zeo_program_desc` points at, one slice per section.
#[derive(Clone, Copy)]
pub(crate) struct DescRows<'a> {
    pub vm: &'a [VmRowSpec],
    pub vis: &'a [VisRowSpec],
    pub classes: &'a [super::classes::ClassSpec],
    pub obj: &'a [ObjRowSpec],
    pub cm: &'a [CmRowSpec],
    pub reg: &'a [RegRowSpec],
    pub foreign: &'a [(u32, String)],
    pub meta: &'a [MetaRowSpec],
    pub redef_metas: &'a [MetaRowSpec],
    pub unit: &'a [(String, FuncId)],
}

/// Everything [`define_desc`] serializes besides the `Analyzed` program.
pub(crate) struct DescSpec<'a> {
    pub toplevel: FuncId,
    pub unit_init: Option<FuncId>,
    pub eval_install: bool,
    pub rows: DescRows<'a>,
}

/// `zeo_program_desc` + the `Str` tables: the loaded-features seed
/// and the parse warnings.
pub(crate) fn define_desc(
    em: &mut Emitter,
    analyzed: &Analyzed,
    spec: &DescSpec<'_>,
) -> CResult<DataId> {
    let &DescSpec {
        toplevel,
        unit_init,
        eval_install,
        rows,
    } = spec;
    let DescRows {
        vm: vm_rows,
        vis: vis_rows,
        classes,
        obj: obj_rows,
        cm: cm_rows,
        reg: reg_rows,
        foreign: foreign_rows,
        meta: meta_rows,
        redef_metas,
        unit: unit_rows,
    } = rows;
    let vm_table = define_vm_rows(em, vm_rows)?;
    let vis_table = define_vis_rows(em, vis_rows)?;
    let class_table = define_classes(em, classes)?;
    let obj_table = define_obj_rows(em, obj_rows)?;
    let cm_table = define_cm_rows(em, cm_rows)?;
    let reg_table = define_reg_rows(em, reg_rows)?;
    let foreign_table = define_foreign_rows(em, foreign_rows)?;
    let meta_table = define_meta_rows(em, meta_rows, "zeo_meta_rows")?;
    let redef_meta_table = define_meta_rows(em, redef_metas, "zeo_redef_metas")?;
    let class_table_ptrs = define_class_tables(em, analyzed)?;
    let unit_table = define_unit_rows(em, unit_rows)?;
    let source_table = define_source_rows(em, &analyzed.compiler.hir.loader.embedded_sources)?;
    let (cov_table, n_cov) = define_cov_rows(em, analyzed)?;
    let hir = &analyzed.compiler.hir;
    // Only what is loaded BEFORE the program's first line. Every feature the
    // program itself requires -- a spliced file and a statically linked
    // extension alike -- records itself at its own document position through
    // `HirNode::FeatureLoaded`, which is CRuby's `rb_provide_feature`. Seeding
    // them here instead made `$LOADED_FEATURES` name a library from line 1
    // however late the `require` was written.
    // What ruby 4.0 has loaded before the program's first line, whether or
    // not the program mentions it (`set.rb`, `thread.rb`, `monitor.rb`,
    // `rational.so`, `complex.so`, and rubygems' own `rbconfig` --
    // oracle-verified). Requiring one of these answers `false` and records
    // nothing, which is what `features::is_preloaded_at_boot` already told
    // the require-fold.
    let loaded: Vec<String> = crate::lower::features::PRELOADED_AT_BOOT
        .iter()
        .map(|f| format!("<zeo-builtin>/{f}.rb"))
        .collect();
    let warnings: Vec<String> = hir.warnings.iter().map(ToString::to_string).collect();
    // `$LOAD_PATH`: the `-I` roots, then the roots of every gem a require
    // actually activated. See `Loader::load_path`.
    let load_path = &hir.loader.search_roots;

    // One Str-array object: loaded features, then warnings, then `$LOAD_PATH`.
    let entries: Vec<(u32, usize)> = loaded
        .iter()
        .chain(warnings.iter())
        .chain(load_path.iter())
        .map(|s| (em.intern_rodata(s.as_bytes()), s.len()))
        .collect();
    let str_size = std::mem::size_of::<Str>();
    let tables_id = em
        .module
        .declare_data(names::STR_TABLES, Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring {}: {e}", names::STR_TABLES)))?;
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
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::STR_TABLES)))?;

    let desc_id = em
        .module
        .declare_data(names::PROGRAM_DESC, Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring {}: {e}", names::PROGRAM_DESC)))?;
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
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_load_path),
        load_path.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_load_path_search),
        hir.loader.search_root_count as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_vm_rows),
        vm_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_vm_foreign),
        foreign_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_vis_rows),
        vis_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_classes),
        classes.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_obj_rows),
        obj_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_cm_rows),
        cm_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_reg_rows),
        reg_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_meta_rows),
        meta_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_redef_metas),
        redef_metas.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_class_tables),
        needed_class_tables(analyzed).len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_units),
        unit_rows.len() as u64,
    );
    put_u64(
        &mut buf,
        std::mem::offset_of!(ProgramDesc, n_sources),
        analyzed.compiler.hir.loader.embedded_sources.len() as u64,
    );
    put_u64(&mut buf, std::mem::offset_of!(ProgramDesc, n_cov), n_cov);
    // `DATA` -- only a script with an `__END__` carries the path, so every
    // other program neither holds it nor opens anything at startup.
    let data_section = hir
        .data_section
        .as_ref()
        .map(|d| (em.intern_rodata(d.path.as_bytes()), d.path.len(), d.offset));
    if let Some((_, len, offset)) = data_section {
        put_u64(
            &mut buf,
            std::mem::offset_of!(ProgramDesc, data_section) + std::mem::offset_of!(Str, len),
            len as u64,
        );
        put_u64(
            &mut buf,
            std::mem::offset_of!(ProgramDesc, data_offset),
            offset,
        );
    }
    desc.define(buf.into_boxed_slice());
    desc.set_align(8);
    if let Some((off, _, _)) = data_section {
        let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut desc);
        desc.write_data_addr(
            (std::mem::offset_of!(ProgramDesc, data_section) + std::mem::offset_of!(Str, ptr))
                as u32,
            rodata_gv,
            i64::from(off),
        );
    }
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
    if !load_path.is_empty() {
        desc.write_data_addr(
            std::mem::offset_of!(ProgramDesc, load_path) as u32,
            tables_gv,
            ((loaded.len() + warnings.len()) * str_size) as i64,
        );
    }
    if let Some(cov) = cov_table {
        let gv = em.module.declare_data_in_data(cov, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, coverage) as u32, gv, 0);
    }
    if let Some(vm) = vm_table {
        let gv = em.module.declare_data_in_data(vm, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, vm_rows) as u32, gv, 0);
    }
    if let Some(f) = foreign_table {
        let gv = em.module.declare_data_in_data(f, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, vm_foreign) as u32, gv, 0);
    }
    if let Some(vis) = vis_table {
        let gv = em.module.declare_data_in_data(vis, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, vis_rows) as u32, gv, 0);
    }
    if let Some(ct) = class_table {
        let gv = em.module.declare_data_in_data(ct, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, classes) as u32, gv, 0);
    }
    if let Some(ot) = obj_table {
        let gv = em.module.declare_data_in_data(ot, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, obj_rows) as u32, gv, 0);
    }
    if let Some(ct) = cm_table {
        let gv = em.module.declare_data_in_data(ct, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, cm_rows) as u32, gv, 0);
    }
    if let Some(mt) = meta_table {
        let gv = em.module.declare_data_in_data(mt, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, meta_rows) as u32, gv, 0);
    }
    if let Some(mt) = redef_meta_table {
        let gv = em.module.declare_data_in_data(mt, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, redef_metas) as u32, gv, 0);
    }
    if let Some(ct) = class_table_ptrs {
        let gv = em.module.declare_data_in_data(ct, &mut desc);
        desc.write_data_addr(
            std::mem::offset_of!(ProgramDesc, class_tables) as u32,
            gv,
            0,
        );
    }
    if let Some(rt) = reg_table {
        let gv = em.module.declare_data_in_data(rt, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, reg_rows) as u32, gv, 0);
    }
    if let Some(ut) = unit_table {
        let gv = em.module.declare_data_in_data(ut, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, units) as u32, gv, 0);
    }
    if let Some(t) = source_table {
        let gv = em.module.declare_data_in_data(t, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, sources) as u32, gv, 0);
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
    // The compiler installs itself as the runtime's evaluator -- named
    // ONLY by a program that can `eval`, which is what lets `-dead_strip`
    // drop the whole compiler from every program that cannot (plan
    // decision 17).
    if eval_install {
        let sig = em.module.make_signature();
        let f = em
            .module
            .declare_function(names::EVAL_INSTALL, Linkage::Import, &sig)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", names::EVAL_INSTALL))
            })?;
        let f_ref = em.module.declare_func_in_data(f, &mut desc);
        desc.write_function_addr(
            std::mem::offset_of!(ProgramDesc, eval_install) as u32,
            f_ref,
        );
    }
    em.module
        .define_data(desc_id, &desc)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::PROGRAM_DESC)))?;
    Ok(desc_id)
}

/// The accumulated read-only bytes, defined LAST (interning happens
/// throughout emission).
pub(crate) fn define_rodata(em: &mut Emitter) -> CResult<()> {
    let mut data = DataDescription::new();
    let bytes = em.take_rodata();
    data.define(if bytes.is_empty() {
        Box::new([0u8])
    } else {
        bytes.into_boxed_slice()
    });
    data.set_align(8);
    em.module
        .define_data(em.rodata_id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {}: {e}", names::RODATA)))
}

/// A `Str` array built on the STACK from `.rodata` bytes, as
/// `(ptr, count)` -- what a runtime call taking a name list reads. Used
/// where the list is per-SITE rather than per-program (the frozen-reopen
/// guard), so a static table would cost a relocation per site for nothing.
/// A stack array of the `*mut Cell` each named local holds -- the shape
/// `zeo_rt_binding_new` reads. Every name must be a `Local::Cell`; the
/// caller filters.
pub(crate) fn cell_array(fx: &mut super::ctx::Fx, names: &[&str]) -> cranelift_codegen::ir::Value {
    use cranelift_codegen::ir::{InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, types};
    let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (names.len().max(1) * 8) as u32,
        3,
    ));
    let fl = MemFlagsData::trusted();
    for (i, name) in names.iter().enumerate() {
        let Some(super::ctx::Local::Cell { ss: cell_ss, .. }) = fx.locals.get(*name).copied()
        else {
            unreachable!("cell_array is given only cell locals");
        };
        let at = fx.slot_addr(cell_ss, 0);
        let p = fx.b.ins().load(types::I64, fl, at, 0);
        let dst = fx.slot_addr(ss, (i * 8) as i32);
        fx.b.ins().store(fl, p, dst, 0);
    }
    fx.slot_addr(ss, 0)
}

pub(crate) fn str_array(
    fx: &mut super::ctx::Fx,
    names: &[&str],
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    use cranelift_codegen::ir::{InstBuilder, MemFlagsData, StackSlotData, StackSlotKind};
    let str_size = std::mem::size_of::<Str>();
    let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (names.len() * str_size) as u32,
        3,
    ));
    let fl = MemFlagsData::trusted();
    for (i, name) in names.iter().enumerate() {
        let (ptr, len) = super::expr::rodata_name(fx, name);
        let base = (i * str_size) as i32;
        let at = fx.slot_addr(ss, base + std::mem::offset_of!(Str, ptr) as i32);
        fx.b.ins().store(fl, ptr, at, 0);
        let at = fx.slot_addr(ss, base + std::mem::offset_of!(Str, len) as i32);
        fx.b.ins().store(fl, len, at, 0);
    }
    let base = fx.slot_addr(ss, 0);
    let n = fx.b.ins().iconst(fx.em.ptr, names.len() as i64);
    (base, n)
}

// ---------------------------------------------------------------------------
// FFI call descriptors
// ---------------------------------------------------------------------------

/// A pointer field inside a descriptor still to be filled in: where it
/// sits, and what it points at.
enum FfiReloc {
    /// A `.rodata` byte offset (a `Str`'s `ptr`).
    Rodata(usize, u32),
    /// Another anonymous data object (a member table, a sub-type array).
    Data(usize, DataId),
}

impl FfiReloc {
    fn shift(self, by: usize) -> FfiReloc {
        match self {
            FfiReloc::Rodata(at, off) => FfiReloc::Rodata(at + by, off),
            FfiReloc::Data(at, id) => FfiReloc::Data(at + by, id),
        }
    }
}

/// Apply a descriptor's pointer fields to a `DataDescription` already
/// holding its bytes.
fn write_ffi_relocs(em: &mut Emitter, data: &mut DataDescription, relocs: Vec<FfiReloc>) {
    if relocs.is_empty() {
        return;
    }
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, data);
    for r in relocs {
        match r {
            FfiReloc::Rodata(at, off) => {
                data.write_data_addr(at as u32, rodata_gv, i64::from(off));
            }
            FfiReloc::Data(at, id) => {
                let gv = em.module.declare_data_in_data(id, data);
                data.write_data_addr(at as u32, gv, 0);
            }
        }
    }
}

fn define_ffi_data(
    em: &mut Emitter,
    what: &str,
    bytes: Vec<u8>,
    relocs: Vec<FfiReloc>,
) -> CResult<DataId> {
    let mut data = DataDescription::new();
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    write_ffi_relocs(em, &mut data, relocs);
    let id = em
        .module
        .declare_anonymous_data(false, false)
        .map_err(|e| CodegenError::internal(format!("declaring {what}: {e}")))?;
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {what}: {e}")))?;
    Ok(id)
}

/// One `FfiTypeC`'s bytes plus the pointer fields still to fill in.
fn ffi_type_bytes(em: &mut Emitter, ty: &super::ffi::TySpec) -> CResult<(Vec<u8>, Vec<FfiReloc>)> {
    use super::ffi::TySpec;
    use zeo_abi::abi::{
        FFI_TY_CALLBACK, FFI_TY_ENUM, FFI_TY_ENUM_SLOT, FFI_TY_SCALAR, FFI_TY_STRPTR,
        FFI_TY_STRUCT, FfiEnumMemberC, FfiTypeC,
    };
    let mut bytes = vec![0u8; std::mem::size_of::<FfiTypeC>()];
    let mut relocs = Vec::new();
    let put_usize = |bytes: &mut [u8], at: usize, v: usize| {
        bytes[at..at + 8].copy_from_slice(&(v as u64).to_le_bytes());
    };
    let (tag, scalar) = match ty {
        TySpec::Scalar(s) => (FFI_TY_SCALAR, s.code()),
        TySpec::Enum(_) => (FFI_TY_ENUM, zeo_abi::ffi::CScalar::I32.code()),
        TySpec::EnumSlot(_) => (FFI_TY_ENUM_SLOT, zeo_abi::ffi::CScalar::I32.code()),
        TySpec::Callback(_, ret) => (FFI_TY_CALLBACK, ret.code()),
        TySpec::Struct(..) => (FFI_TY_STRUCT, 0),
        TySpec::StrPtr => (FFI_TY_STRPTR, zeo_abi::ffi::CScalar::Pointer.code()),
    };
    bytes[std::mem::offset_of!(FfiTypeC, tag)] = tag;
    bytes[std::mem::offset_of!(FfiTypeC, scalar)] = scalar;
    match ty {
        TySpec::Enum(members) => {
            let size = std::mem::size_of::<FfiEnumMemberC>();
            let mut mbytes = vec![0u8; size * members.len()];
            let mut mrelocs = Vec::new();
            for (i, (name, value)) in members.iter().enumerate() {
                let base = i * size;
                let name_at = base + std::mem::offset_of!(FfiEnumMemberC, name);
                let off = em.intern_rodata(name.as_bytes());
                mrelocs.push(FfiReloc::Rodata(
                    name_at + std::mem::offset_of!(Str, ptr),
                    off,
                ));
                put_usize(
                    &mut mbytes,
                    name_at + std::mem::offset_of!(Str, len),
                    name.len(),
                );
                mbytes[base + std::mem::offset_of!(FfiEnumMemberC, value)
                    ..base + std::mem::offset_of!(FfiEnumMemberC, value) + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
            let id = define_ffi_data(em, "an FFI enum member table", mbytes, mrelocs)?;
            relocs.push(FfiReloc::Data(std::mem::offset_of!(FfiTypeC, members), id));
            put_usize(
                &mut bytes,
                std::mem::offset_of!(FfiTypeC, n_members),
                members.len(),
            );
        }
        TySpec::EnumSlot(slot) => {
            put_usize(&mut bytes, std::mem::offset_of!(FfiTypeC, slot), *slot);
        }
        TySpec::Callback(args, _) => {
            let subs: Vec<TySpec> = args.iter().map(|s| TySpec::Scalar(*s)).collect();
            if let Some(id) = ffi_type_array(em, &subs)? {
                relocs.push(FfiReloc::Data(std::mem::offset_of!(FfiTypeC, sub), id));
            }
            put_usize(
                &mut bytes,
                std::mem::offset_of!(FfiTypeC, n_sub),
                args.len(),
            );
        }
        TySpec::Struct(fields, size) => {
            if let Some(id) = ffi_type_array(em, fields)? {
                relocs.push(FfiReloc::Data(std::mem::offset_of!(FfiTypeC, sub), id));
            }
            put_usize(
                &mut bytes,
                std::mem::offset_of!(FfiTypeC, n_sub),
                fields.len(),
            );
            put_usize(&mut bytes, std::mem::offset_of!(FfiTypeC, size), *size);
        }
        TySpec::Scalar(_) | TySpec::StrPtr => {}
    }
    Ok((bytes, relocs))
}

/// An array of `FfiTypeC` as one anonymous data object; `None` when empty.
fn ffi_type_array(em: &mut Emitter, tys: &[super::ffi::TySpec]) -> CResult<Option<DataId>> {
    if tys.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<zeo_abi::abi::FfiTypeC>();
    let mut bytes = Vec::with_capacity(size * tys.len());
    let mut relocs = Vec::new();
    for (i, ty) in tys.iter().enumerate() {
        let (b, r) = ffi_type_bytes(em, ty)?;
        bytes.extend_from_slice(&b);
        relocs.extend(r.into_iter().map(|x| x.shift(i * size)));
    }
    define_ffi_data(em, "an FFI type table", bytes, relocs).map(Some)
}

/// One `attach_function` call site's `FfiCallC`.
pub(crate) fn define_ffi_call(em: &mut Emitter, spec: &super::ffi::CallSpec) -> CResult<DataId> {
    use zeo_abi::abi::FfiCallC;
    let args_id = ffi_type_array(em, &spec.args)?;
    let (ret_bytes, ret_relocs) = ffi_type_bytes(em, &spec.ret)?;
    let mut bytes = vec![0u8; std::mem::size_of::<FfiCallC>()];
    let ret_at = std::mem::offset_of!(FfiCallC, ret);
    bytes[ret_at..ret_at + ret_bytes.len()].copy_from_slice(&ret_bytes);
    bytes[std::mem::offset_of!(FfiCallC, n_args)..std::mem::offset_of!(FfiCallC, n_args) + 8]
        .copy_from_slice(&(spec.args.len() as u64).to_le_bytes());
    bytes[std::mem::offset_of!(FfiCallC, blocking)] = u8::from(spec.blocking);
    bytes[std::mem::offset_of!(FfiCallC, variadic)] = u8::from(spec.variadic);
    let mut relocs: Vec<FfiReloc> = ret_relocs.into_iter().map(|r| r.shift(ret_at)).collect();
    if let Some(id) = args_id {
        relocs.push(FfiReloc::Data(std::mem::offset_of!(FfiCallC, args), id));
    }
    define_ffi_data(em, "an FFI call descriptor", bytes, relocs)
}

/// The line-coverage table: one row per source file that has any coverable
/// line -- its total line count (the result array's length), the statement
/// lines stamped during emission, and the `def` lines no statement stream
/// ever passes. Empty (and so absent) in a program that never activated
/// coverage.
fn define_cov_rows(em: &mut Emitter, analyzed: &Analyzed) -> CResult<(Option<DataId>, u64)> {
    use zeo_abi::abi::CovFile;
    if !em.cov_active {
        return Ok((None, 0));
    }
    let mut files = cov_rows(em, analyzed);
    // Merged packages contribute their own rows (string-keyed by file, so
    // no id space to rebase). An artifact compiled WITHOUT the stamps can
    // never report its lines here, so it is refused -- the `package '..'`
    // spelling is what drops it to the source splice.
    let mut seen: crate::compiler::FSet<String> =
        files.iter().map(|(name, ..)| name.clone()).collect();
    for m in &analyzed.compiler.hir.pkg_merge {
        if !m.cov_active {
            return Err(CodegenError::unsupported(
                format!(
                    "this program measures coverage, but package '{}' was compiled without \
                     coverage stamps",
                    m.feature
                ),
                None,
            ));
        }
        for row in &m.cov {
            if !seen.insert(row.file.clone()) {
                continue;
            }
            files.push((row.file.clone(), row.total, row.stmt.clone(), row.def.clone()));
        }
    }
    if files.is_empty() {
        return Ok((None, 0));
    }
    let size = std::mem::size_of::<CovFile>();
    let mut bytes = vec![0u8; size * files.len()];
    let mut relocs: Vec<FfiReloc> = Vec::new();
    let mut line_ids: Vec<(usize, DataId)> = Vec::new();
    for (i, (name, total, stmt, def)) in files.iter().enumerate() {
        let base = i * size;
        let name_at = base + std::mem::offset_of!(CovFile, file);
        let off = em.intern_rodata(name.as_bytes());
        relocs.push(FfiReloc::Rodata(
            name_at + std::mem::offset_of!(Str, ptr),
            off,
        ));
        bytes[name_at + std::mem::offset_of!(Str, len)
            ..name_at + std::mem::offset_of!(Str, len) + 8]
            .copy_from_slice(&(name.len() as u64).to_le_bytes());
        let at = base + std::mem::offset_of!(CovFile, total);
        bytes[at..at + 4].copy_from_slice(&total.to_le_bytes());
        for (lines, ptr_field, n_field) in [
            (
                stmt,
                std::mem::offset_of!(CovFile, stmt_lines),
                std::mem::offset_of!(CovFile, n_stmt),
            ),
            (
                def,
                std::mem::offset_of!(CovFile, def_lines),
                std::mem::offset_of!(CovFile, n_def),
            ),
        ] {
            let at = base + n_field;
            bytes[at..at + 8].copy_from_slice(&(lines.len() as u64).to_le_bytes());
            if lines.is_empty() {
                continue;
            }
            let mut lb = Vec::with_capacity(lines.len() * 4);
            for l in lines {
                lb.extend_from_slice(&l.to_le_bytes());
            }
            let id = define_ffi_data(em, "a coverage line list", lb, Vec::new())?;
            line_ids.push((base + ptr_field, id));
        }
    }
    let mut data = DataDescription::new();
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    write_ffi_relocs(em, &mut data, relocs);
    for (at, id) in line_ids {
        let gv = em.module.declare_data_in_data(id, &mut data);
        data.write_data_addr(at as u32, gv, 0);
    }
    let id = em
        .module
        .declare_anonymous_data(false, false)
        .map_err(|e| CodegenError::internal(format!("declaring the coverage table: {e}")))?;
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining the coverage table: {e}")))?;
    Ok((Some(id), files.len() as u64))
}

/// The coverage rows THIS compile owns: one `(file, total, stmt lines,
/// def lines)` per source file with a coverable line. Drains the stamped
/// lines out of the emitter, so it runs once -- `define_cov_rows` for a
/// program, the manifest writer for a package build.
pub(super) fn cov_rows(
    em: &mut Emitter,
    analyzed: &Analyzed,
) -> Vec<(String, u32, Vec<u32>, Vec<u32>)> {
    let defs = crate::analyze::coverage::def_lines(&analyzed.compiler);
    let stmts = std::mem::take(&mut em.cov_lines);
    let mut seen = crate::compiler::FSet::default();
    analyzed
        .compiler
        .hir
        .files
        .iter()
        .filter(|f| seen.insert(f.name.clone()))
        .filter_map(|f| {
            let stmt: Vec<u32> = stmts.get(&f.name).into_iter().flatten().copied().collect();
            let def: Vec<u32> = defs.get(&f.name).into_iter().flatten().copied().collect();
            if stmt.is_empty() && def.is_empty() {
                return None;
            }
            let total = u32::try_from(f.source.lines().count()).unwrap_or(u32::MAX);
            Some((f.name.clone(), total, stmt, def))
        })
        .collect()
}

/// `zeo_class_tables`: one pointer per builtin class method table the program
/// can reach, in `CLASS_TABLE_SYMBOLS` order.
///
/// Each entry is a relocation against a `zeo_ctable_*` symbol the runtime
/// exports, so referencing one is what keeps that class's methods in the
/// binary -- and not referencing one is what lets them strip. Every table is
/// named today; narrowing the set is the size lever this exists for.
fn define_class_tables(em: &mut Emitter, analyzed: &Analyzed) -> CResult<Option<DataId>> {
    let symbols = needed_class_tables(analyzed);
    if symbols.is_empty() {
        return Ok(None);
    }
    let id = em
        .module
        .declare_data("zeo_class_tables", Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring zeo_class_tables: {e}")))?;
    let mut data = DataDescription::new();
    data.define(vec![0u8; symbols.len() * 8].into_boxed_slice());
    for (i, sym) in symbols.iter().enumerate() {
        let table = em
            .module
            .declare_data(sym, Linkage::Import, false, false)
            .map_err(|e| CodegenError::internal(format!("declaring {sym}: {e}")))?;
        let gv = em.module.declare_data_in_data(table, &mut data);
        data.write_data_addr((i * 8) as u32, gv, 0);
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining zeo_class_tables: {e}")))?;
    Ok(Some(id))
}

/// Which builtin class tables this program can reach.
///
/// The rule is the emitter's OWN registration gate, not a second opinion: a
/// require-gated extension registers per program (`classes.rs`), so a class
/// that gate excludes has no constant and no dispatch path. That is what
/// takes OpenSSL, socket, zlib, StringIO and FFI out of a program that never
/// asks for them.
///
/// An always-on class stays. A value of that kind can arrive without the
/// program naming it -- `1.to_s` needs String -- so nothing syntactic rules
/// one out, and narrowing those wants real reachability analysis.
///
/// A program that can compile code at RUN time gets everything: `eval` and an
/// unresolved `require` both reach the embedded compiler, which can name any
/// class at all.
pub(crate) fn needed_class_tables(analyzed: &Analyzed) -> Vec<&'static str> {
    // The symbol list is generated from the ext TREE; a feature-off
    // extension's table is absent from `libzeo.a`, so referencing its
    // symbol would fail the link (the dual-build switch).
    let all: Vec<&(zeo_abi::ClassId, &'static str)> = crate::builtin_surface::CLASS_TABLE_SYMBOLS
        .iter()
        .filter(|(id, _)| crate::lower::features::build_carries_class(*id))
        .collect();
    // The measurement hatch: drop the named tables so a link can price them.
    // See `debug_flags::dropped_tables` -- the miss is loud, not silent.
    let dropped = crate::debug_flags::dropped_tables();
    let keep = |sym: &&'static str| !dropped.iter().any(|d| d == *sym);
    // A program that LOADS a C extension keeps every carried table: the C
    // half reaches builtins the Ruby-side reachability walk cannot see
    // (zlib's stream guard is `rb_mutex_synchronize` -- `Thread::Mutex`
    // with no Ruby mention anywhere).
    let loads_cext = analyzed
        .compiler
        .hir
        .all_nodes()
        .iter()
        .any(|n| matches!(n, crate::hir::HirNode::CExtLoaded { .. }));
    if analyzed.compiler.compiles_at_runtime() || loads_cext {
        return all.iter().map(|(_, sym)| *sym).filter(keep).collect();
    }
    // A merged package's reachable set unions in: `-dead_strip` prunes any
    // table nothing NAMES, and the package's code reaches its tables
    // through this program's desc.
    let pkg_tables: std::collections::HashSet<&str> = analyzed
        .compiler
        .hir
        .pkg_merge
        .iter()
        .flat_map(|m| m.class_tables.iter().map(String::as_str))
        .collect();
    all.iter()
        .filter(|(id, sym)| {
            analyzed.compiler.builtin_is_reachable(*id) || pkg_tables.contains(sym)
        })
        .map(|(_, sym)| *sym)
        .filter(keep)
        .collect()
}
