//! Object emission: ISA/flags, the `ObjectModule`, the capi import cache,
//! and the per-program orchestration -- prologue/epilogue of the compiled
//! `<main>`, the emitted C `main`, and the statics (see `statics`).

use super::capi_names::{self, CTy};
use super::ctx::Fx;
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
    compile_inner(analyzed, false).map(|(bytes, _)| bytes)
}

/// `compile` plus the per-function CLIF text (`--emit-clif`, snapshots).
pub fn compile_with_clif(analyzed: &Analyzed) -> Result<(Vec<u8>, String), String> {
    compile_inner(analyzed, true).map(|(bytes, text)| (bytes, text.expect("collected")))
}

fn compile_inner(
    analyzed: &Analyzed,
    collect_clif: bool,
) -> Result<(Vec<u8>, Option<String>), String> {
    let mut em = Emitter::new()?;
    em.clif_text = collect_clif.then(String::new);
    let defs = collect_methods(&mut em, analyzed)?;
    let collected = super::classes::collect_classes(&mut em, analyzed)?;
    let (class_specs, obj_methods, class_vis) =
        (collected.classes, collected.methods, collected.vis);
    for def in &defs {
        let func = em.methods[&def.name].body;
        let spec = BodyFnSpec {
            func,
            owner: zeo_abi::ClassId(0),
            owner_name: "Object",
            name: &def.name,
            params: &def.params,
            hir_params: &def.hir_params,
            body: &def.body,
            node: def.node,
            has_blk: def.has_blk,
        };
        define_method_body(&mut em, analyzed, &spec)?;
    }
    for m in &obj_methods {
        if let Some(func) = m.body_fn {
            let spec = BodyFnSpec {
                func,
                owner: m.owner,
                owner_name: &m.owner_name,
                name: &m.name,
                params: &m.params,
                hir_params: &m.hir_params,
                body: &m.body,
                node: m.node,
                has_blk: m.has_blk,
            };
            define_method_body(&mut em, analyzed, &spec)?;
        }
    }
    for def in &defs {
        let decl = &em.methods[&def.name];
        let (tramp, body, arity, has_blk) = (decl.tramp, decl.body, decl.arity, decl.has_blk);
        let idx = em.next_fn_index();
        super::params::define_trampoline(&mut em, tramp, body, arity, has_blk, idx)?;
    }
    for m in &obj_methods {
        let idx = em.next_fn_index();
        match (m.accessor, m.body_fn) {
            (Some((slot, kind)), None) => {
                super::params::define_accessor(&mut em, m.tramp, slot, kind, idx)?;
            }
            (None, Some(body)) => {
                super::params::define_trampoline(&mut em, m.tramp, body, m.arity, m.has_blk, idx)?;
            }
            (Some(_), Some(_)) | (None, None) => {
                unreachable!("collect_classes declares exactly one of accessor/body")
            }
        }
    }
    let toplevel = define_toplevel(&mut em, analyzed)?;
    let unit_init = statics::define_unit_init(&mut em)?;
    statics::define_syms(&mut em)?;
    let vm_rows: Vec<statics::VmRowSpec> = defs
        .iter()
        .map(|d| statics::VmRowSpec {
            class: 0,
            box_id: 0,
            name: d.name.clone(),
            f: em.methods[&d.name].tramp,
        })
        .collect();
    let vis_rows: Vec<statics::VisRowSpec> = defs
        .iter()
        .filter_map(|d| {
            let verb = match d.visibility {
                crate::hir::Visibility::Private => 0,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => return None,
            };
            Some(statics::VisRowSpec {
                class: 0,
                name: d.name.clone(),
                verb,
            })
        })
        .collect();
    let mut vis_rows = vis_rows;
    vis_rows.extend(class_vis);
    let obj_rows: Vec<statics::ObjRowSpec> = obj_methods
        .iter()
        .map(|m| statics::ObjRowSpec {
            class: m.owner.0,
            name: m.name.clone(),
            f: m.tramp,
        })
        .collect();
    let desc = statics::define_desc(
        &mut em,
        analyzed,
        toplevel,
        unit_init,
        &vm_rows,
        &vis_rows,
        &class_specs,
        &obj_rows,
    )?;
    define_main(&mut em, desc)?;
    statics::define_rodata(&mut em)?;
    let clif = em.clif_text.take();
    let product = em.module.finish();
    let bytes = product
        .emit()
        .map_err(|e| format!("emitting the object file: {e}"))?;
    Ok((bytes, clif))
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
    /// Compiled methods by Ruby name -- what a receiverless call resolves
    /// against for the direct path.
    pub methods: HashMap<String, MethodDecl>,
    fn_index: u32,
    /// When `Some`, every finished function's CLIF renders here (before
    /// machine compilation -- the target-independent IR).
    pub clif_text: Option<String>,
}

/// One compiled method's declaration facts.
pub(crate) struct MethodDecl {
    pub body: FuncId,
    pub tramp: FuncId,
    pub arity: usize,
    pub has_blk: bool,
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
            methods: HashMap::new(),
            fn_index: 0,
            clif_text: None,
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

    /// Like `intern_rodata`, but the offset is `align`-aligned (id arrays
    /// the runtime reads as `&[u32]`). Not deduplicated -- alignment is
    /// part of the identity and these tables are tiny.
    pub(crate) fn intern_rodata_aligned(&mut self, bytes: &[u8], align: usize) -> u32 {
        let pad = (align - (self.rodata.len() % align)) % align;
        self.rodata.extend(std::iter::repeat_n(0u8, pad));
        let off = u32::try_from(self.rodata.len()).expect("rodata under 4GB");
        self.rodata.extend_from_slice(bytes);
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

    /// Record `func`'s CLIF when `--emit-clif` asked for it, under its
    /// exported symbol name.
    pub(crate) fn record_clif(&mut self, name: &str, func: &ir::Function) {
        if let Some(text) = &mut self.clif_text {
            use std::fmt::Write as _;
            let _ = writeln!(text, ";; {name}\n{}", func.display());
        }
    }

    /// A fresh `UserFuncName` index (cosmetic; must be unique per module).
    pub(crate) fn next_fn_index(&mut self) -> u32 {
        self.fn_index += 1;
        self.fn_index
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

/// One eligible top-level `def`'s facts (from `Compiler.classes[0]` --
/// analyze hoists method scopes out of `main_statements`).
pub(crate) struct DefSpec {
    pub name: String,
    params: Vec<String>,
    hir_params: crate::hir::Params,
    body: Vec<crate::hir::NodeId>,
    visibility: crate::hir::Visibility,
    node: Option<crate::hir::NodeId>,
    has_blk: bool,
}

/// Collect and DECLARE every top-level `def` the slice can compile
/// (required positional params only; unconditional; no aliases or
/// accessors). Prelude-native rows are the runtime's own, never emitted.
fn collect_methods(em: &mut Emitter, analyzed: &Analyzed) -> Result<Vec<DefSpec>, String> {
    let compiler = &analyzed.compiler;
    let mut out = Vec::new();
    for entry in &compiler.classes[0].methods {
        let scope = compiler.scope(entry.def);
        if scope.native_default {
            continue;
        }
        let name = compiler.names.str(entry.name).to_string();
        let at = scope
            .def_node
            .and_then(|n| crate::codegen::source_location(compiler, n))
            .map(|(f, l)| format!(" ({f}:{l})"))
            .unwrap_or_default();
        let refuse = |what: &str| {
            Err(format!(
                "--backend aot is an M0 vertical slice: cannot lower {what} yet{at}"
            ))
        };
        if scope.runtime_conditional {
            return refuse("a conditionally-defined method");
        }
        if scope.alias_of.is_some() {
            return refuse("an alias");
        }
        if scope.accessor.is_some() {
            return refuse("an attr_* accessor");
        }
        let params = &scope.params;
        if !(params.destructures.is_empty()
            && params.optional.is_empty()
            && params.rest.is_none()
            && !params.implicit_rest
            && params.post.is_empty()
            && params.keywords.is_empty()
            && params.keyword_rest.is_none()
            && params.block.is_none()
            && params.block_locals.is_empty()
            && params.implicit_block_locals.is_empty())
        {
            return refuse("a def with non-required parameters");
        }
        let arity = params.required.len();
        let has_blk = scope.needs_block_param();
        let body_sig = super::params::body_sig(em, arity, has_blk);
        let body_id = em
            .module
            .declare_function(
                &names::method_symbol("Object", &name),
                Linkage::Local,
                &body_sig,
            )
            .map_err(|e| format!("declaring {name}: {e}"))?;
        let tramp_sig = super::params::value_fn_sig(em);
        let tramp_id = em
            .module
            .declare_function(
                &names::trampoline_symbol("Object", &name),
                Linkage::Local,
                &tramp_sig,
            )
            .map_err(|e| format!("declaring {name}'s trampoline: {e}"))?;
        em.methods.insert(
            name.clone(),
            MethodDecl {
                body: body_id,
                tramp: tramp_id,
                arity,
                has_blk,
            },
        );
        out.push(DefSpec {
            name,
            params: params.required.clone(),
            hir_params: params.clone(),
            body: scope.body.clone(),
            visibility: scope.visibility,
            node: scope.def_node,
            has_blk,
        });
    }
    Ok(out)
}

/// What `define_method_body` compiles: any owner's ordinary method.
pub(crate) struct BodyFnSpec<'a> {
    pub func: FuncId,
    pub owner: zeo_abi::ClassId,
    pub owner_name: &'a str,
    pub name: &'a str,
    pub params: &'a [String],
    pub hir_params: &'a crate::hir::Params,
    pub body: &'a [crate::hir::NodeId],
    pub node: Option<crate::hir::NodeId>,
    pub has_blk: bool,
}

/// One compiled method body: `(self, p1..pn, out) -> i32`. Params are
/// copied into owned slots (the M0 rule -- borrow-through is a perf-pass
/// lever); the tail value moves into `out`; `return` jumps to the shared
/// ok-exit.
fn define_method_body(
    em: &mut Emitter,
    analyzed: &Analyzed,
    def: &BodyFnSpec<'_>,
) -> Result<(), String> {
    let sig = super::params::body_sig(em, def.params.len(), def.has_blk);
    let idx = em.next_fn_index();
    let label = format!("{}#{}", def.owner_name, def.name);
    let (line, end_line) = match def.node {
        Some(node) => (
            crate::codegen::source_location(&analyzed.compiler, node).map_or(0, |(_, l)| l),
            crate::codegen::source_end_line(&analyzed.compiler, node),
        ),
        None => (0, 0),
    };
    let file = analyzed.compiler.hir.files.first().map(|f| f.name.clone());

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, idx), sig);
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
    let entry = fx.b.current_block().expect("entry is current");
    let entry_params: Vec<ir::Value> = fx.b.block_params(entry).to_vec();
    let self_ptr = entry_params[0];
    let out_ptr = *entry_params.last().expect("out is the last param");
    let blk_ptr = def.has_blk.then(|| entry_params[entry_params.len() - 2]);
    fx.self_ptr = Some(self_ptr);
    fx.method_class = Some(def.owner);
    fx.frame_label = label.clone();
    fx.blk_ptr = blk_ptr;
    let ret_ok = fx.b.create_block();
    fx.ret = Some((out_ptr, ret_ok));

    // Recursion guard BEFORE the frame exists: a failure returns without
    // pops (mirrors the rustc prologue's `stack_check()?` position).
    let status = fx
        .call("zeo_rt_stack_check", &[])
        .expect("stack_check returns a status");
    let early = fx.b.create_block();
    let cont = fx.b.create_block();
    fx.b.ins().brif(status, early, &[], cont, &[]);
    fx.b.switch_to_block(early);
    let one = fx.b.ins().iconst(types::I32, 1);
    fx.b.ins().return_(&[one]);
    fx.b.switch_to_block(cont);

    // What escaping blocks capture becomes a cell instead of a slot.
    let captured = crate::analyze::captures::collect_escaping_captures(
        &analyzed.compiler,
        def.body,
        def.hir_params,
        crate::analyze::class_query::SelfClass::new(Some(def.owner), None),
    )
    .locals;
    // Params: owned copies in slots (retained when heap); a captured param
    // escapes straight into its cell.
    for (i, name) in def.params.iter().enumerate() {
        let src = super::operand::Operand::Ptr {
            addr: entry_params[i + 1],
            owned: false,
            tag: super::operand::TagInfo::Unknown,
        };
        if captured.contains(name) {
            let seed = fx.temp_slot();
            let seed_addr = fx.slot_addr(seed, 0);
            super::ownership::write_move_into(&mut fx, &src, seed_addr);
            init_cell_local(&mut fx, name.clone(), Some(seed_addr));
        } else {
            let ss = fx.new_value_slot();
            let dst = fx.slot_addr(ss, 0);
            super::ownership::write_move_into(&mut fx, &src, dst);
            fx.locals.insert(name.clone(), super::ctx::Local::Slot(ss));
        }
    }
    // The body's other locals, nil-initialized (cells when captured).
    let mut locals = crate::analyze::local_storage::Locals::default();
    for &stmt in def.body {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, stmt, &mut locals);
    }
    for name in locals.names().to_vec() {
        if fx.locals.contains_key(&name) {
            continue;
        }
        if captured.contains(&name) {
            init_cell_local(&mut fx, name, None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals.insert(name, super::ctx::Local::Slot(ss));
        }
    }

    if let Some(file) = &file {
        let off = fx.em.intern_rodata(file.as_bytes());
        let label_off = fx.em.intern_rodata(label.as_bytes());
        let file_ptr = fx.rod(off);
        let file_len = fx.b.ins().iconst(fx.em.ptr, file.len() as i64);
        let label_ptr = fx.rod(label_off);
        let label_len = fx.b.ins().iconst(fx.em.ptr, label.len() as i64);
        let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
        let end_v = fx.b.ins().iconst(types::I32, i64::from(end_line));
        fx.call(
            "zeo_rt_frame_push",
            &[file_ptr, file_len, label_ptr, label_len, line_v, end_v],
        );
    }
    let status = fx
        .call("zeo_rt_check_ints", &[])
        .expect("check_ints returns a status");
    fx.fallible(status);

    super::stmt::lower_value_body_into(&mut fx, def.body, out_ptr)?;
    fx.b.ins().jump(ret_ok, &[]);

    let has_frame = file.is_some();
    let epilogue = |fx: &mut Fx, status: i64| {
        release_locals(fx);
        if let Some(blk) = blk_ptr {
            // The body owns the moved-in block; a null slot releases as a
            // no-op inside the runtime? No -- guard it.
            let got =
                fx.b.ins()
                    .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, blk, 0);
            let rel = fx.b.create_block();
            let cont = fx.b.create_block();
            fx.b.ins().brif(got, rel, &[], cont, &[]);
            fx.b.switch_to_block(rel);
            fx.call("zeo_rt_release", &[blk]);
            fx.b.ins().jump(cont, &[]);
            fx.b.switch_to_block(cont);
        }
        if has_frame {
            fx.call("zeo_rt_frame_pop", &[]);
        }
        let code = fx.b.ins().iconst(types::I32, status);
        fx.b.ins().return_(&[code]);
    };
    fx.b.switch_to_block(ret_ok);
    epilogue(&mut fx, 0);
    let land = fx.land;
    fx.b.switch_to_block(land);
    epilogue(&mut fx, 1);

    verify::check(&fx, &label);
    let Fx { mut b, .. } = fx;
    b.seal_all_blocks();
    b.finalize(cfg);

    em.record_clif(&label, &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(def.func, &mut ctx)
        .map_err(|e| format!("compiling {label}: {e}"))?;
    Ok(())
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

    fx.frame_label = "<main>".to_string();
    // Hoisted locals: an owned slot each, or a cell when an escaping block
    // captures the name.
    let captured = crate::analyze::captures::collect_escaping_captures(
        &analyzed.compiler,
        &analyzed.main_statements,
        &crate::hir::Params::default(),
        crate::analyze::class_query::SelfClass::new(None, None),
    )
    .locals;
    for name in locals.names().to_vec() {
        if captured.contains(&name) {
            init_cell_local(&mut fx, name, None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals.insert(name, super::ctx::Local::Slot(ss));
        }
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

    // The toplevel's `self`: one pooled `main` handle, borrowed by every
    // receiverless direct call.
    let self_ss = fx.temp_slot();
    let self_addr = fx.slot_addr(self_ss, 0);
    fx.call("zeo_rt_main_object", &[self_addr]);
    fx.owned_created += 1;
    super::ownership::pool_owned(
        &mut fx,
        self_addr,
        super::operand::TagInfo::Known(zeo_abi::abi::ValueTag::Object as u8),
    );
    fx.self_ptr = Some(self_addr);

    // Defs registered through the row tables run nothing in statement
    // position (the rustc backend's shape: registration precedes the body).
    let runnable: Vec<crate::hir::NodeId> = analyzed
        .main_statements
        .iter()
        .copied()
        .filter(|&s| {
            // Defs and (statement-free, collect_classes-verified) class
            // definitions registered through the tables run nothing here.
            !matches!(
                analyzed.compiler.hir[s],
                crate::hir::HirNode::DefMethod { .. } | crate::hir::HirNode::ClassDef { .. }
            )
        })
        .collect();
    stmt::lower_stmts(&mut fx, &runnable)?;

    // Normal exit: release the locals, pop the frame (drains the pool),
    // hand back Nil.
    let epilogue = |fx: &mut Fx, status: i64| {
        release_locals(fx);
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

    em.record_clif(names::TOPLEVEL, &func);
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

    em.record_clif("main", &func);
    let mut ctx = em.module.make_context();
    ctx.func = func;
    em.module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("compiling main: {e}"))?;
    Ok(func_id)
}

/// A fresh CAPTURED local: an owned cell (seeded from `seed`'s moved
/// value, nil when `None`) whose pointer lives in an 8-byte slot.
fn init_cell_local(fx: &mut Fx, name: String, seed: Option<ir::Value>) {
    let init = match seed {
        Some(p) => p,
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let cellp = fx
        .call("zeo_rt_cell_new", &[init])
        .expect("cell_new returns the cell");
    let ss =
        fx.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let dst = fx.slot_addr(ss, 0);
    fx.b.ins().store(MemFlagsData::trusted(), cellp, dst, 0);
    fx.locals
        .insert(name, super::ctx::Local::Cell { ss, owned: true });
}

/// Release every local: slots drop their value, owned cells drop their
/// reference (the proc's copies keep the cell alive).
fn release_locals(fx: &mut Fx) {
    let locals: Vec<super::ctx::Local> = fx.locals.values().copied().collect();
    for l in locals {
        match l {
            super::ctx::Local::Slot(ss) => {
                let addr = fx.slot_addr(ss, 0);
                fx.call("zeo_rt_release", &[addr]);
            }
            super::ctx::Local::Cell { ss, owned: true } => {
                let ptr = fx.cell_ptr(ss);
                fx.call("zeo_rt_cell_release", &[ptr]);
            }
            super::ctx::Local::Cell { owned: false, .. } => {}
        }
    }
}
