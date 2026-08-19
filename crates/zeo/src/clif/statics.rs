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
use zeo_abi::abi::{
    self, ClassDesc, CmRow, ForeignRow, ObjRow, ProgramDesc, RegRow, Str, VisRow, VmRow,
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

    em.record_clif(names::UNIT_INIT, &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("compiling {}: {e}", names::UNIT_INIT))?;
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

/// The `zeo_vm_rows` table: `VmRow` structs with name/function relocs.
fn define_vm_rows(em: &mut Emitter, rows: &[VmRowSpec]) -> Result<Option<DataId>, String> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<VmRow>();
    let id = em
        .module
        .declare_data("zeo_vm_rows", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_vm_rows: {e}"))?;
    let interned: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.name.as_bytes()))
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        let put_u32 = |bytes: &mut [u8], at: usize, v: u32| {
            bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
        };
        put_u32(
            &mut bytes,
            base + std::mem::offset_of!(VmRow, class),
            row.class,
        );
        put_u32(
            &mut bytes,
            base + std::mem::offset_of!(VmRow, box_id),
            row.box_id,
        );
        put_u32(&mut bytes, base + std::mem::offset_of!(VmRow, flags), 0);
        let at = base + std::mem::offset_of!(VmRow, name) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(row.name.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        let name_at =
            (base + std::mem::offset_of!(VmRow, name) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(name_at, rodata_gv, i64::from(interned[i]));
        let f_ref = em.module.declare_func_in_data(row.f, &mut data);
        data.write_function_addr((base + std::mem::offset_of!(VmRow, f)) as u32, f_ref);
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining zeo_vm_rows: {e}"))?;
    Ok(Some(id))
}

/// A method's `.rodata` `ParamDescC` (+ its keyword rows): what the bound
/// trampoline hands `zeo_rt_bind_params`. Anonymous data objects; every
/// string lives in the rodata blob.
pub(crate) fn define_param_desc(
    em: &mut Emitter,
    spec: &super::params::ParamDescSpec<'_>,
) -> Result<DataId, String> {
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
            .map_err(|e| format!("declaring a kw table: {e}"))?;
        em.module
            .define_data(id, &data)
            .map_err(|e| format!("defining a kw table: {e}"))?;
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
        .map_err(|e| format!("declaring a ParamDesc: {e}"))?;
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining a ParamDesc: {e}"))?;
    Ok(id)
}

/// One class-method dispatch row (`CmRow`): `def self.x`'s trampoline on
/// the class-method channel.
pub(crate) struct CmRowSpec {
    pub class: u32,
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
    /// A super-target row's trampoline; None for the mark kinds.
    pub f: Option<FuncId>,
}

/// One `ObjRow` (an object-channel method on a compiled class).
pub(crate) struct ObjRowSpec {
    pub class: u32,
    pub name: String,
    pub f: FuncId,
}

/// The `zeo_vm_foreign` table: rows a builtin reopen INHERITED, marked
/// so a `super` walk skips them at that position.
fn define_foreign_rows(em: &mut Emitter, rows: &[(u32, String)]) -> Result<Option<DataId>, String> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<ForeignRow>();
    let id = em
        .module
        .declare_data("zeo_vm_foreign", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_vm_foreign: {e}"))?;
    let interned: Vec<u32> = rows
        .iter()
        .map(|(_, name)| em.intern_rodata(name.as_bytes()))
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, (class, name)) in rows.iter().enumerate() {
        let base = i * size;
        bytes[base + std::mem::offset_of!(ForeignRow, class)
            ..base + std::mem::offset_of!(ForeignRow, class) + 4]
            .copy_from_slice(&class.to_le_bytes());
        let at = base + std::mem::offset_of!(ForeignRow, name) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(name.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, &off) in interned.iter().enumerate() {
        let base = i * size;
        let at =
            (base + std::mem::offset_of!(ForeignRow, name) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(at, rodata_gv, i64::from(off));
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining zeo_vm_foreign: {e}"))?;
    Ok(Some(id))
}

/// The `zeo_obj_rows` table (same shape as `zeo_vm_rows`, minus box/flags).
fn define_obj_rows(em: &mut Emitter, rows: &[ObjRowSpec]) -> Result<Option<DataId>, String> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<ObjRow>();
    let id = em
        .module
        .declare_data("zeo_obj_rows", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_obj_rows: {e}"))?;
    let interned: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.name.as_bytes()))
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        bytes[base + std::mem::offset_of!(ObjRow, class)
            ..base + std::mem::offset_of!(ObjRow, class) + 4]
            .copy_from_slice(&row.class.to_le_bytes());
        let at = base + std::mem::offset_of!(ObjRow, name) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(row.name.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        let name_at =
            (base + std::mem::offset_of!(ObjRow, name) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(name_at, rodata_gv, i64::from(interned[i]));
        let f_ref = em.module.declare_func_in_data(row.f, &mut data);
        data.write_function_addr((base + std::mem::offset_of!(ObjRow, f)) as u32, f_ref);
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining zeo_obj_rows: {e}"))?;
    Ok(Some(id))
}

fn define_cm_rows(em: &mut Emitter, rows: &[CmRowSpec]) -> Result<Option<DataId>, String> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<CmRow>();
    let id = em
        .module
        .declare_data("zeo_cm_rows", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_cm_rows: {e}"))?;
    let interned: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.name.as_bytes()))
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        bytes[base + std::mem::offset_of!(CmRow, class)
            ..base + std::mem::offset_of!(CmRow, class) + 4]
            .copy_from_slice(&row.class.to_le_bytes());
        let at = base + std::mem::offset_of!(CmRow, name) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(row.name.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        let name_at =
            (base + std::mem::offset_of!(CmRow, name) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(name_at, rodata_gv, i64::from(interned[i]));
        let f_ref = em.module.declare_func_in_data(row.f, &mut data);
        data.write_function_addr((base + std::mem::offset_of!(CmRow, f)) as u32, f_ref);
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining zeo_cm_rows: {e}"))?;
    Ok(Some(id))
}

fn define_reg_rows(em: &mut Emitter, rows: &[RegRowSpec]) -> Result<Option<DataId>, String> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<RegRow>();
    let id = em
        .module
        .declare_data("zeo_reg_rows", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_reg_rows: {e}"))?;
    let interned: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.a.as_bytes()))
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        bytes[base + std::mem::offset_of!(RegRow, kind)] = row.kind;
        bytes[base + std::mem::offset_of!(RegRow, class)
            ..base + std::mem::offset_of!(RegRow, class) + 4]
            .copy_from_slice(&row.class.to_le_bytes());
        let at = base + std::mem::offset_of!(RegRow, a) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(row.a.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, &off) in interned.iter().enumerate() {
        let base = i * size;
        let a_at = (base + std::mem::offset_of!(RegRow, a) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(a_at, rodata_gv, i64::from(off));
    }
    for (i, row) in rows.iter().enumerate() {
        let Some(f) = row.f else { continue };
        let f_ref = em.module.declare_func_in_data(f, &mut data);
        let base = i * size;
        data.write_function_addr((base + std::mem::offset_of!(RegRow, f)) as u32, f_ref);
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining zeo_reg_rows: {e}"))?;
    Ok(Some(id))
}

/// The `zeo_classes` table plus its two auxiliary arrays: the linearized
/// ancestor ids and the ivar-name `Str` entries every `ClassDesc` points
/// into.
fn define_classes(
    em: &mut Emitter,
    classes: &[super::classes::ClassSpec],
) -> Result<Option<DataId>, String> {
    if classes.is_empty() {
        return Ok(None);
    }
    // Ancestor ids, one shared u32 array.
    let anc_id = em
        .module
        .declare_data("zeo_class_ancestors", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_class_ancestors: {e}"))?;
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
        .map_err(|e| format!("defining zeo_class_ancestors: {e}"))?;

    // Ivar names, one shared Str array.
    let str_size = std::mem::size_of::<Str>();
    let n_ivars: usize = classes.iter().map(|c| c.ivars.len()).sum();
    let ivars_id = (n_ivars > 0)
        .then(|| {
            em.module
                .declare_data("zeo_class_ivars", Linkage::Local, false, false)
                .map_err(|e| format!("declaring zeo_class_ivars: {e}"))
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
            .map_err(|e| format!("defining zeo_class_ivars: {e}"))?;
    } else {
        ivar_offsets.resize(classes.len(), 0);
    }

    let size = std::mem::size_of::<ClassDesc>();
    let id = em
        .module
        .declare_data("zeo_classes", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_classes: {e}"))?;
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
        bytes[base + std::mem::offset_of!(ClassDesc, hidden)
            ..base + std::mem::offset_of!(ClassDesc, hidden) + 2]
            .copy_from_slice(&c.hidden.to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    let anc_gv = em.module.declare_data_in_data(anc_id, &mut data);
    let ivars_gv = ivars_id.map(|iv| em.module.declare_data_in_data(iv, &mut data));
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
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining zeo_classes: {e}"))?;
    Ok(Some(id))
}

/// The `zeo_vis_rows` table.
fn define_vis_rows(em: &mut Emitter, rows: &[VisRowSpec]) -> Result<Option<DataId>, String> {
    if rows.is_empty() {
        return Ok(None);
    }
    let size = std::mem::size_of::<VisRow>();
    let id = em
        .module
        .declare_data("zeo_vis_rows", Linkage::Local, false, false)
        .map_err(|e| format!("declaring zeo_vis_rows: {e}"))?;
    let interned: Vec<u32> = rows
        .iter()
        .map(|r| em.intern_rodata(r.name.as_bytes()))
        .collect();
    let mut data = DataDescription::new();
    let mut bytes = vec![0u8; size * rows.len()];
    for (i, row) in rows.iter().enumerate() {
        let base = i * size;
        bytes[base + std::mem::offset_of!(VisRow, class)
            ..base + std::mem::offset_of!(VisRow, class) + 4]
            .copy_from_slice(&row.class.to_le_bytes());
        bytes[base + std::mem::offset_of!(VisRow, verb)] = row.verb;
        let at = base + std::mem::offset_of!(VisRow, name) + std::mem::offset_of!(Str, len);
        bytes[at..at + 8].copy_from_slice(&(row.name.len() as u64).to_le_bytes());
    }
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let rodata_gv = em.module.declare_data_in_data(em.rodata_id, &mut data);
    for (i, _row) in rows.iter().enumerate() {
        let base = i * size;
        let name_at =
            (base + std::mem::offset_of!(VisRow, name) + std::mem::offset_of!(Str, ptr)) as u32;
        data.write_data_addr(name_at, rodata_gv, i64::from(interned[i]));
    }
    em.module
        .define_data(id, &data)
        .map_err(|e| format!("defining zeo_vis_rows: {e}"))?;
    Ok(Some(id))
}

/// `zeo_program_desc` + the `Str` tables: the loaded-features seed (the
/// same list the rustc backend emits) and the parse warnings.
#[allow(
    clippy::too_many_arguments,
    reason = "the desc is the one table of tables; every parameter is one of its sections"
)]
pub(crate) fn define_desc(
    em: &mut Emitter,
    analyzed: &Analyzed,
    toplevel: FuncId,
    unit_init: Option<FuncId>,
    vm_rows: &[VmRowSpec],
    vis_rows: &[VisRowSpec],
    classes: &[super::classes::ClassSpec],
    obj_rows: &[ObjRowSpec],
    cm_rows: &[CmRowSpec],
    reg_rows: &[RegRowSpec],
    foreign_rows: &[(u32, String)],
) -> Result<DataId, String> {
    let vm_table = define_vm_rows(em, vm_rows)?;
    let vis_table = define_vis_rows(em, vis_rows)?;
    let class_table = define_classes(em, classes)?;
    let obj_table = define_obj_rows(em, obj_rows)?;
    let cm_table = define_cm_rows(em, cm_rows)?;
    let reg_table = define_reg_rows(em, reg_rows)?;
    let foreign_table = define_foreign_rows(em, foreign_rows)?;
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
    if let Some(rt) = reg_table {
        let gv = em.module.declare_data_in_data(rt, &mut desc);
        desc.write_data_addr(std::mem::offset_of!(ProgramDesc, reg_rows) as u32, gv, 0);
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
    data.set_align(8);
    em.module
        .define_data(em.rodata_id, &data)
        .map_err(|e| format!("defining {}: {e}", names::RODATA))
}
