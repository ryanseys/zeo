//! Code emission: ISA/flags, the module (an object file for AOT,
//! in-process code memory for JIT -- same lowering either way), the capi
//! import cache, and the per-program orchestration -- prologue/epilogue of
//! the compiled `<main>`, the emitted C `main`, and the statics (see
//! `statics`).

use super::capi_names::{self, CTy};
use super::ctx::Fx;
use super::{names, statics, stmt, verify};
use crate::analyze::Analyzed;
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{
    DataDescription, DataId, FuncId, Linkage, Module, ModuleDeclarations, ModuleReloc, ModuleResult,
};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;

/// The one module the emitter writes into. Both arms take the identical
/// lowering -- the enum (not two emitters) is what makes "JIT runs the
/// same code AOT links" a structural fact.
#[allow(
    clippy::large_enum_variant,
    reason = "one ClifModule exists per compile; boxing would buy nothing"
)]
pub(crate) enum ClifModule {
    Object(ObjectModule),
    Jit(JITModule),
}

impl ClifModule {
    fn as_dyn(&self) -> &dyn Module {
        match self {
            ClifModule::Object(m) => m,
            ClifModule::Jit(m) => m,
        }
    }

    fn as_dyn_mut(&mut self) -> &mut dyn Module {
        match self {
            ClifModule::Object(m) => m,
            ClifModule::Jit(m) => m,
        }
    }
}

impl Module for ClifModule {
    fn isa(&self) -> &dyn cranelift_codegen::isa::TargetIsa {
        self.as_dyn().isa()
    }

    fn declarations(&self) -> &ModuleDeclarations {
        self.as_dyn().declarations()
    }

    fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &ir::Signature,
    ) -> ModuleResult<FuncId> {
        self.as_dyn_mut().declare_function(name, linkage, signature)
    }

    fn declare_anonymous_function(&mut self, signature: &ir::Signature) -> ModuleResult<FuncId> {
        self.as_dyn_mut().declare_anonymous_function(signature)
    }

    fn declare_data(
        &mut self,
        name: &str,
        linkage: Linkage,
        writable: bool,
        tls: bool,
    ) -> ModuleResult<DataId> {
        self.as_dyn_mut().declare_data(name, linkage, writable, tls)
    }

    fn declare_anonymous_data(&mut self, writable: bool, tls: bool) -> ModuleResult<DataId> {
        self.as_dyn_mut().declare_anonymous_data(writable, tls)
    }

    fn define_function_with_control_plane(
        &mut self,
        func: FuncId,
        ctx: &mut cranelift_codegen::Context,
        ctrl_plane: &mut cranelift_codegen::control::ControlPlane,
    ) -> ModuleResult<()> {
        self.as_dyn_mut()
            .define_function_with_control_plane(func, ctx, ctrl_plane)
    }

    fn define_function_bytes(
        &mut self,
        func_id: FuncId,
        alignment: u64,
        bytes: &[u8],
        relocs: &[ModuleReloc],
    ) -> ModuleResult<()> {
        self.as_dyn_mut()
            .define_function_bytes(func_id, alignment, bytes, relocs)
    }

    fn define_data(&mut self, data_id: DataId, data: &DataDescription) -> ModuleResult<()> {
        self.as_dyn_mut().define_data(data_id, data)
    }
}

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
    let mut em = Emitter::new(false)?;
    em.clif_text = collect_clif.then(String::new);
    emit_program(&mut em, analyzed)?;
    let clif = em.clif_text.take();
    let ClifModule::Object(module) = em.module else {
        unreachable!("Emitter::new(false) builds an object module")
    };
    let product = module.finish();
    let bytes = product
        .emit()
        .map_err(|e| format!("emitting the object file: {e}"))?;
    Ok((bytes, clif))
}

/// A JIT-compiled program: finalized in-process code plus the emitted C
/// `main`'s address. The module OWNS the code memory -- it must outlive
/// every call into `main`.
pub struct Jitted {
    pub module: JITModule,
    pub main: *const u8,
}

/// Lower `analyzed` straight into executable memory (`--backend jit`).
pub fn compile_jit(analyzed: &Analyzed) -> Result<Jitted, String> {
    let mut em = Emitter::new(true)?;
    let main = emit_program(&mut em, analyzed)?;
    let ClifModule::Jit(mut module) = em.module else {
        unreachable!("Emitter::new(true) builds a JIT module")
    };
    module
        .finalize_definitions()
        .map_err(|e| format!("finalizing jitted code: {e}"))?;
    let main = module.get_finalized_function(main);
    Ok(Jitted { module, main })
}

/// The whole program into `em`'s module -- every function and data object,
/// mode-blind. Returns the emitted C `main`.
fn emit_program(em: &mut Emitter, analyzed: &Analyzed) -> Result<FuncId, String> {
    let defs = collect_methods(em, analyzed)?;
    let collected = super::classes::collect_classes(em, analyzed)?;
    let class_bodies = collect_class_bodies(em, analyzed)?;
    let (
        class_specs,
        obj_methods,
        mod_methods,
        cm_methods,
        own_cm,
        own_rows,
        class_vis,
        foreign,
        extends,
        sst,
        alias_rows,
        undef_rows,
        conceal,
        singleton_surrogates,
        private_consts,
        redefs,
        boot_redefs,
        set_ancestors,
        register_builtin,
    ) = (
        collected.classes,
        collected.methods,
        collected.module_methods,
        collected.class_methods,
        collected.own_cm,
        collected.own_rows,
        collected.vis,
        collected.foreign,
        collected.extends,
        collected.sst,
        collected.alias_rows,
        collected.undef_rows,
        collected.conceal,
        collected.singleton_surrogates,
        collected.private_consts,
        collected.redefs,
        collected.boot_redefs,
        collected.set_ancestors,
        collected.register_builtin,
    );
    // Populated BEFORE any body is defined: a class body's
    // `MethodRedefine` statement reads it while its own fn is built.
    for m in &redefs {
        em.redef_tramps.insert((m.owner.0, m.scope.0), m.tramp);
    }
    for def in &defs {
        let func = em.methods[&def.name].body;
        let spec = BodyFnSpec {
            func,
            owner: zeo_abi::ClassId(0),
            owner_name: "Object",
            name: &def.name,
            hir_params: &def.hir_params,
            body: &def.body,
            node: def.node,
            has_blk: def.has_blk,
            ruby2_keywords: def.ruby2_keywords,
            self_is_class: false,
            label_override: None,
            discard_value: false,
            dyn_ivars: false,
            defining_class: Some(zeo_abi::ClassId(0)),
            // A top-level `def` is never inside a `class << self`.
            lexical_home: None,
        };
        define_method_body(em, analyzed, &spec)?;
    }
    for m in &obj_methods {
        if let Some(func) = m.body_fn {
            let spec = BodyFnSpec {
                func,
                owner: m.owner,
                owner_name: &m.owner_name,
                name: &m.name,
                hir_params: &m.hir_params,
                body: &m.body,
                node: m.node,
                has_blk: m.has_blk,
                ruby2_keywords: m.ruby2_keywords,
                self_is_class: false,
                label_override: None,
                discard_value: false,
                dyn_ivars: m.dyn_ivars,
                defining_class: Some(m.defining_class),
                lexical_home: m.lexical_home,
            };
            define_method_body(em, analyzed, &spec)?;
        }
    }
    for m in &cm_methods {
        let spec = BodyFnSpec {
            func: m.body_fn,
            owner: m.owner,
            owner_name: &m.owner_name,
            name: &m.name,
            hir_params: &m.hir_params,
            body: &m.body,
            node: m.node,
            has_blk: m.has_blk,
            ruby2_keywords: m.ruby2_keywords,
            self_is_class: true,
            label_override: None,
            discard_value: false,
            dyn_ivars: false,
            defining_class: Some(m.defining_class),
            lexical_home: m.lexical_home,
        };
        define_method_body(em, analyzed, &spec)?;
    }
    for m in &redefs {
        let spec = BodyFnSpec {
            func: m.body_fn,
            owner: m.owner,
            owner_name: &m.owner_name,
            name: &m.name,
            hir_params: &m.hir_params,
            body: &m.body,
            node: m.node,
            has_blk: m.has_blk,
            ruby2_keywords: m.ruby2_keywords,
            self_is_class: false,
            label_override: None,
            discard_value: false,
            dyn_ivars: false,
            defining_class: Some(m.owner),
            lexical_home: None,
        };
        define_method_body(em, analyzed, &spec)?;
    }
    for m in &mod_methods {
        let spec = BodyFnSpec {
            func: m.body_fn,
            owner: m.owner,
            owner_name: &m.owner_name,
            name: &m.name,
            hir_params: &m.hir_params,
            body: &m.body,
            node: m.node,
            has_blk: m.has_blk,
            ruby2_keywords: m.ruby2_keywords,
            self_is_class: false,
            label_override: None,
            discard_value: false,
            dyn_ivars: m.dyn_ivars,
            defining_class: Some(m.defining_class),
            lexical_home: m.lexical_home,
        };
        define_method_body(em, analyzed, &spec)?;
    }
    let empty_params = crate::hir::Params::default();
    for cb in &class_bodies {
        let Some(func) = cb.call.func else { continue };
        let owner_name = analyzed.compiler.fq_name(cb.class);
        let spec = BodyFnSpec {
            func,
            owner: cb.class,
            owner_name: &owner_name,
            name: "",
            hir_params: &empty_params,
            body: &cb.stmts,
            node: cb.node,
            has_blk: false,
            ruby2_keywords: false,
            self_is_class: true,
            label_override: Some(cb.label.clone()),
            discard_value: cb.call.tail != BodyTail::Own,
            dyn_ivars: false,
            defining_class: None,
            // A class body's OWN cref is its class; the surrogate case is
            // carried by the `def`s inside it, not by the body fn.
            lexical_home: None,
        };
        define_method_body(em, analyzed, &spec)?;
    }
    for def in &defs {
        let decl = &em.methods[&def.name];
        let (tramp, body, has_blk) = (decl.tramp, decl.body, decl.has_blk);
        let idx = em.next_fn_index();
        let (file, label, line, end_line) =
            method_frame(analyzed, "Object", &def.name, def.node, false);
        let spec = super::params::TrampSpec {
            tramp,
            body,
            params: &def.hir_params,
            has_blk,
            name: &def.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    for m in &obj_methods {
        let idx = em.next_fn_index();
        match (m.accessor, m.body_fn) {
            (Some((slot, kind)), None) => {
                super::params::define_accessor(em, m.tramp, slot, kind, idx)?;
            }
            (None, Some(body)) => {
                let (file, label, line, end_line) =
                    method_frame(analyzed, &m.owner_name, &m.name, m.node, false);
                let spec = super::params::TrampSpec {
                    tramp: m.tramp,
                    body,
                    params: &m.hir_params,
                    has_blk: m.has_blk,
                    name: &m.name,
                    file: file.as_deref(),
                    label: &label,
                    line,
                    end_line,
                };
                super::params::define_trampoline(em, &spec, idx)?;
            }
            (Some(_), Some(_)) | (None, None) => {
                unreachable!("collect_classes declares exactly one of accessor/body")
            }
        }
    }
    for m in &redefs {
        let idx = em.next_fn_index();
        let (file, label, line, end_line) =
            method_frame(analyzed, &m.owner_name, &m.name, m.node, false);
        let spec = super::params::TrampSpec {
            tramp: m.tramp,
            body: m.body_fn,
            params: &m.hir_params,
            has_blk: m.has_blk,
            name: &m.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    for m in &mod_methods {
        let idx = em.next_fn_index();
        let (file, label, line, end_line) =
            method_frame(analyzed, &m.owner_name, &m.name, m.node, false);
        let spec = super::params::TrampSpec {
            tramp: m.tramp,
            body: m.body_fn,
            params: &m.hir_params,
            has_blk: m.has_blk,
            name: &m.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    for m in &cm_methods {
        let idx = em.next_fn_index();
        let (file, label, line, end_line) =
            method_frame(analyzed, &m.owner_name, &m.name, m.node, true);
        let spec = super::params::TrampSpec {
            tramp: m.tramp,
            body: m.body_fn,
            params: &m.hir_params,
            has_blk: m.has_blk,
            name: &m.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    let hoisted: Vec<ClassBodyCall> = class_bodies
        .iter()
        .filter(|cb| !cb.inline)
        .map(|cb| cb.call.clone())
        .collect();
    let toplevel = define_toplevel(em, analyzed, &hoisted)?;
    let unit_init = statics::define_unit_init(em)?;
    statics::define_syms(em)?;
    let mut vm_rows: Vec<statics::VmRowSpec> = defs
        .iter()
        .map(|d| statics::VmRowSpec {
            class: 0,
            box_id: 0,
            name: d.name.clone(),
            f: em.methods[&d.name].tramp,
        })
        .collect();
    vm_rows.extend(mod_methods.iter().map(|m| statics::VmRowSpec {
        class: m.owner.0,
        box_id: 0,
        name: m.name.clone(),
        f: m.tramp,
    }));
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
        .filter(|m| !m.super_target_only)
        .map(|m| statics::ObjRowSpec {
            class: m.owner.0,
            name: m.name.clone(),
            f: m.tramp,
        })
        .collect();
    let cm_rows: Vec<statics::CmRowSpec> = cm_methods
        .iter()
        .filter(|m| m.cm_row)
        .map(|m| statics::CmRowSpec {
            class: m.owner.0,
            name: m.name.clone(),
            f: m.tramp,
        })
        .collect();
    let mut reg_rows: Vec<statics::RegRowSpec> = own_rows
        .iter()
        .map(|(class, name)| statics::RegRowSpec {
            kind: zeo_abi::abi::REG_MARK_OWN_ROWS,
            class: *class,
            a: name.clone(),
            b: String::new(),
            f: None,
            ids: vec![],
            flag: 0,
        })
        .collect();
    reg_rows.extend(own_cm.iter().map(|(class, name)| statics::RegRowSpec {
        kind: zeo_abi::abi::REG_MARK_OWN_CLASS_METHOD_ROWS,
        class: *class,
        a: name.clone(),
        b: String::new(),
        f: None,
        ids: vec![],
        flag: 0,
    }));
    // Own-`super`-target rows: every OWN instance method's trampoline is
    // already receiver-generic, so it doubles as the class's per-position
    // contribution to a `super` walk (rustc emits dedicated dynamic-self
    // bridges for the same rows; here the trampoline IS one).
    reg_rows.extend(
        obj_methods
            .iter()
            .filter(|m| m.is_own)
            .map(|m| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_SUPER_TARGET_VALUE,
                class: m.owner.0,
                a: m.name.clone(),
                b: String::new(),
                f: Some(m.tramp),
                ids: vec![],
                flag: 0,
            }),
    );
    // `extend M` rows: the modules on each class's singleton chain.
    reg_rows.extend(extends.iter().map(|(class, mods)| statics::RegRowSpec {
        kind: zeo_abi::abi::REG_EXTENDS,
        class: *class,
        a: String::new(),
        b: String::new(),
        f: None,
        ids: mods.clone(),
        flag: 0,
    }));
    // Singleton-chain super-target rows, keyed `(class, module, name)`.
    reg_rows.extend(
        sst.iter()
            .map(|(class, module, name, tramp)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_SINGLETON_SUPER_TARGET,
                class: *class,
                a: name.clone(),
                b: String::new(),
                f: Some(*tramp),
                ids: vec![*module],
                flag: 0,
            }),
    );
    // Require-gated builtins register per program.
    reg_rows.extend(
        register_builtin
            .iter()
            .map(|(class, name, is_module, ancestors)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_REGISTER_BUILTIN,
                class: *class,
                a: name.clone(),
                b: String::new(),
                f: None,
                ids: ancestors.clone(),
                flag: u8::from(*is_module),
            }),
    );
    // A builtin reopen that changed the ancestry patches the chain.
    reg_rows.extend(
        set_ancestors
            .iter()
            .map(|(class, ancestors)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_SET_ANCESTORS,
                class: *class,
                a: String::new(),
                b: String::new(),
                f: None,
                ids: ancestors.clone(),
                flag: 0,
            }),
    );
    // Runtime-conditional classes start concealed.
    reg_rows.extend(conceal.iter().map(|class| statics::RegRowSpec {
        kind: zeo_abi::abi::REG_CONCEAL_CLASS,
        class: *class,
        a: String::new(),
        b: String::new(),
        f: None,
        ids: vec![],
        flag: 0,
    }));
    // The FIRST body of every observable redefinition timeline installs
    // before the first statement runs.
    reg_rows.extend(
        boot_redefs
            .iter()
            .map(|(class, name, sid)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_BOOT_REDEF,
                class: *class,
                a: name.clone(),
                b: String::new(),
                f: Some(em.redef_tramps[&(*class, sid.0)]),
                ids: vec![],
                flag: 0,
            }),
    );
    // `private_constant` marks.
    reg_rows.extend(
        private_consts
            .iter()
            .map(|(class, name)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_CONST_PRIVATE,
                class: *class,
                a: name.clone(),
                b: String::new(),
                f: None,
                ids: vec![],
                flag: 0,
            }),
    );
    // Singleton-class surrogates seed the runtime mint.
    reg_rows.extend(
        singleton_surrogates
            .iter()
            .map(|(surrogate, owner)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_SINGLETON_SURROGATE,
                class: *surrogate,
                a: String::new(),
                b: String::new(),
                f: None,
                ids: vec![*owner],
                flag: 0,
            }),
    );
    // `undef` marks.
    reg_rows.extend(undef_rows.iter().map(|(class, name)| statics::RegRowSpec {
        kind: zeo_abi::abi::REG_MARK_UNDEFINED,
        class: *class,
        a: name.clone(),
        b: String::new(),
        f: None,
        ids: vec![],
        flag: 0,
    }));
    // Builtin-source alias name-indirection rows.
    reg_rows.extend(
        alias_rows
            .iter()
            .map(|(class, new, old, is_class)| statics::RegRowSpec {
                kind: if *is_class {
                    zeo_abi::abi::REG_CLASS_ALIAS
                } else {
                    zeo_abi::abi::REG_ALIAS
                },
                class: *class,
                a: new.clone(),
                b: old.clone(),
                f: None,
                ids: vec![],
                flag: 0,
            }),
    );
    // Reflection rows: one per emitted method scope, in the order the
    // tables above register them.
    let mut meta_rows: Vec<statics::MetaRowSpec> = Vec::new();
    for d in &defs {
        meta_rows.push(meta_row(
            analyzed,
            0,
            false,
            &d.name,
            &d.hir_params,
            d.node,
            d.alias_of.as_deref(),
        ));
    }
    for m in &obj_methods {
        meta_rows.push(meta_row(
            analyzed,
            m.owner.0,
            false,
            &m.name,
            &m.hir_params,
            m.node,
            m.alias_of.as_deref(),
        ));
    }
    for m in &mod_methods {
        meta_rows.push(meta_row(
            analyzed,
            m.owner.0,
            false,
            &m.name,
            &m.hir_params,
            m.node,
            m.alias_of.as_deref(),
        ));
    }
    for m in cm_methods.iter().filter(|m| m.cm_row) {
        meta_rows.push(meta_row(
            analyzed,
            m.owner.0,
            true,
            &m.name,
            &m.hir_params,
            m.node,
            m.alias_of.as_deref(),
        ));
    }
    let desc = statics::define_desc(
        em,
        analyzed,
        toplevel,
        unit_init,
        &vm_rows,
        &vis_rows,
        &class_specs,
        &obj_rows,
        &cm_rows,
        &reg_rows,
        &foreign,
        &meta_rows,
    )?;
    let main = define_main(em, desc)?;
    statics::define_rodata(em)?;
    Ok(main)
}

/// Program-wide emission state: the module, the rodata blob, the symbol
/// pool, and the capi import cache.
pub(crate) struct Emitter {
    pub module: ClifModule,
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
    /// Class-body sites by their INLINE `ClassDef` marker: what a marker
    /// in statement position emits (a hoisted site's marker is absent --
    /// its body already ran in the toplevel prelude).
    pub class_bodies: HashMap<crate::hir::NodeId, ClassBodyCall>,
    /// `(class, scope)` -> the trampoline that installs that body, for the
    /// boot install and each positional `MethodRedefine`.
    pub redef_tramps: HashMap<(u32, u32), FuncId>,
    fn_index: u32,
    /// Regexp-literal site ids -- one cached frozen object per site
    /// (`zeo_rt_regexp_lit`), the rustc per-site `RegexpSite` twin.
    pub regexp_sites: u32,
    /// When `Some`, every finished function's CLIF renders here (before
    /// machine compilation -- the target-independent IR).
    pub clif_text: Option<String>,
}

/// One compiled method's declaration facts.
pub(crate) struct MethodDecl {
    pub body: FuncId,
    pub tramp: FuncId,
    /// Required-positional count (the direct-call gate compares it).
    pub arity: usize,
    /// Required positionals only -- a count-matched call site may go
    /// direct; any richer shape routes through the dynamic send and the
    /// bound trampoline.
    pub plain: bool,
    pub has_blk: bool,
}

impl Emitter {
    /// `jit`: emit into in-process code memory instead of an object file.
    /// The only lowering-visible difference is `is_pic` (`JITModule`
    /// requires non-PIC code); everything downstream is mode-blind.
    fn new(jit: bool) -> Result<Emitter, String> {
        let mut flags = settings::builder();
        let set = |flags: &mut settings::Builder, k: &str, v: &str| {
            flags
                .set(k, v)
                .map_err(|e| format!("cranelift setting {k}={v}: {e}"))
        };
        set(&mut flags, "opt_level", "speed")?;
        set(&mut flags, "is_pic", if jit { "false" } else { "true" })?;
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
        let mut module = if jit {
            // Imports resolve against the runtime linked into THIS process:
            // the capi table first (`zeo_rt_*` -- not exported, so dlsym
            // cannot find them), then cranelift's dlsym fallback (libcalls:
            // memcpy and friends from libc).
            let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
            builder.symbols(zeo_rt::capi::symbols::NAMES.iter().map(|&name| {
                let addr = zeo_rt::capi::symbols::addr(name)
                    .expect("every listed capi symbol has an address");
                (name, addr)
            }));
            ClifModule::Jit(JITModule::new(builder))
        } else {
            let elf = isa.triple().binary_format == target_lexicon::BinaryFormat::Elf;
            let mut builder =
                ObjectBuilder::new(isa, "zeo-p0", cranelift_module::default_libcall_names())
                    .map_err(|e| format!("cranelift object builder: {e}"))?;
            builder.per_function_section(true);
            // `.eh_frame` is free on ELF; Mach-O emission panics in
            // cranelift-object 0.134 and is not load-bearing (decision 13).
            builder.unwind_info(elf);
            ClifModule::Object(ObjectModule::new(builder))
        };
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
            class_bodies: HashMap::new(),
            redef_tramps: HashMap::new(),
            fn_index: 0,
            regexp_sites: 0,
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
            CTy::U8 | CTy::I8 => types::I8,
        }
    }

    fn abi_param(&self, t: CTy) -> AbiParam {
        match t {
            // A sub-32-bit C ARGUMENT is the caller's to zero-extend (the
            // Apple arm64 rule; a no-op elsewhere).
            CTy::U8 => AbiParam::new(types::I8).uext(),
            CTy::I8 => AbiParam::new(types::I8).sext(),
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
    hir_params: crate::hir::Params,
    ruby2_keywords: bool,
    body: Vec<crate::hir::NodeId>,
    visibility: crate::hir::Visibility,
    node: Option<crate::hir::NodeId>,
    has_blk: bool,
    alias_of: Option<String>,
}

/// The M1-1 parameter eligibility shared by top-level and class methods:
/// the full positional/keyword surface binds; destructures and the
/// block-only trailing-comma rest are still refusals.
pub(crate) fn check_params(p: &crate::hir::Params) -> Result<(), &'static str> {
    if p.implicit_rest {
        return Err("an implicit-rest parameter");
    }
    Ok(())
}

/// Collect and DECLARE every top-level `def` the backend can compile
/// (unconditional; no aliases or accessors). Prelude-native rows are the
/// runtime's own, never emitted.
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
        let refuse = |what: &str| Err(format!("the CLIF backend cannot lower {what} yet{at}"));
        if scope.runtime_conditional {
            return refuse("a conditionally-defined method");
        }
        let params = &scope.params;
        if let Err(what) = check_params(params) {
            return refuse(what);
        }
        let layout = super::params::layout_of(params)?;
        let has_blk = scope.needs_block_param();
        let body_sig = super::params::body_sig(em, layout.n_slots, has_blk);
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
                arity: params.required.len(),
                plain: layout.plain,
                has_blk,
            },
        );
        out.push(DefSpec {
            name,
            hir_params: params.clone(),
            ruby2_keywords: scope.ruby2_keywords,
            body: scope.body.clone(),
            visibility: scope.visibility,
            node: scope.def_node,
            has_blk,
            alias_of: scope.alias_of.clone(),
        });
    }
    Ok(out)
}

/// One method scope's reflection row: what ruby can ask back about a `def`
/// that its function pointer cannot answer -- the signature
/// (`#arity`/`#parameters`), the `def` keyword's own line
/// (`#source_location`, and `#inspect`'s tail), and the name an alias
/// copied from.
fn meta_row(
    analyzed: &Analyzed,
    class: u32,
    singleton: bool,
    name: &str,
    params: &crate::hir::Params,
    node: Option<crate::hir::NodeId>,
    alias_of: Option<&str>,
) -> statics::MetaRowSpec {
    let (file, line) = node
        .and_then(|n| crate::codegen::source_location(&analyzed.compiler, n))
        .map_or((String::new(), 0), |(f, l)| (f.to_string(), l));
    statics::MetaRowSpec {
        class,
        singleton,
        name: name.to_string(),
        params: param_entries(params, false),
        file,
        line,
        aliased_from: alias_of.unwrap_or_default().to_string(),
    }
}

/// A method's parameters as ruby reports them (`Method#parameters`): the
/// declared order, with an anonymous rest/keyrest/block named for its own
/// sigil -- ruby prints `def m(*)` as `[:rest, :*]`, and a `__`-prefixed
/// name IS anonymous (the internal one lowering gave a bare sigil).
pub(crate) fn param_entries(params: &crate::hir::Params, block: bool) -> Vec<(u8, String)> {
    use crate::hir::KeywordParam;
    use zeo_abi::abi;
    fn named(kind: u8, name: &str) -> (u8, String) {
        if name.starts_with("__") {
            (kind, String::new())
        } else {
            (kind, name.to_string())
        }
    }
    fn sigil(kind: u8, name: &Option<String>, s: &str) -> (u8, String) {
        match name {
            Some(n) if !n.starts_with("__") => (kind, n.clone()),
            _ => (kind, s.to_string()),
        }
    }
    let mut out = Vec::new();
    for r in &params.required {
        out.push(named(abi::PARAM_REQ, r));
    }
    for (o, _) in &params.optional {
        out.push(named(abi::PARAM_OPT, o));
    }
    // A TRAILING COMMA's rest is not in a block's signature at all --
    // `proc { |x,| }` reports just `x`.
    if let Some(rest) = &params.rest
        && !(block && params.implicit_rest)
    {
        out.push(sigil(abi::PARAM_REST, rest, "*"));
    }
    for p in &params.post {
        out.push(named(abi::PARAM_REQ, p));
    }
    for kw in &params.keywords {
        match kw {
            KeywordParam::Required(n) => out.push(named(abi::PARAM_KEYREQ, n)),
            KeywordParam::Optional(n, _) => out.push(named(abi::PARAM_KEY, n)),
        }
    }
    if let Some(kwrest) = &params.keyword_rest {
        out.push(sigil(abi::PARAM_KEYREST, kwrest, "**"));
    }
    if let Some(block) = &params.block {
        out.push(sigil(abi::PARAM_BLOCK, block, "&"));
    }
    out
}

/// What `define_method_body` compiles: any owner's ordinary method.
/// What runs at one class-body site: the declaration's
/// `const_source_location` record, then the compiled body (absent when
/// analyze consumed every statement -- the `class C; def a; end; end`
/// shape).
/// What a `class`/`module` marker EVALUATES to. A `class` is an
/// expression in Ruby (`x = class C; 7; end` binds 7), and almost every
/// site runs for effect -- so the body fn keeps its tail only where the
/// tail is a value at all.
#[derive(Clone, PartialEq)]
pub(crate) enum BodyTail {
    /// The body fn computes it: its last statement IS an expression.
    Own,
    /// Analyze CONSUMED the body's last source statement, so the emitted
    /// statements no longer end where ruby's value comes from -- a `def`
    /// answers its name, `private_constant` the module it hid it on.
    Sym(String),
    OwnClass,
    /// A definition-level construct with no value zeo can name: nil in
    /// tail position (where nothing necessarily reads it), a refusal in
    /// expression position.
    Unknown(&'static str),
}

#[derive(Clone)]
pub(crate) struct ClassBodyCall {
    pub class: u32,
    pub func: Option<FuncId>,
    /// `(owner, leaf, file, line)` -- recorded only by the DECLARING site.
    pub const_loc: Option<(u32, String, String, u32)>,
    /// `(owner, leaf)` -- the `const_added` this declaration announces, from
    /// the DECLARING site only. `None` for a reopen, which creates nothing.
    pub const_added: Option<(u32, String)>,
    /// The superclass to announce this declaration to (`Super.inherited(C)`)
    /// -- `None` unless this site DECLARES the class and the superclass
    /// chain answers `inherited` by then.
    pub inherited: Option<u32>,
    /// Whether this site must REVEAL its runtime-conditional class: the
    /// guarded definition just ran, so the constant exists from here on.
    pub reveal: bool,
    /// The FROZEN-REOPEN guard's name list: the methods THIS site would
    /// install that no earlier site for the same class already did. Empty
    /// when the program freezes nothing, when this is the class's first
    /// site, or when the site installs nothing new.
    pub freeze_guard: Vec<String>,
    /// The site's Ruby value -- read only by a marker in value or tail
    /// position; a statement marker discards it.
    pub tail: BodyTail,
    /// A trailing `if`/`unless` on the `class` keyword (`class Set ... end if
    /// set_pp`). Ruby evaluates it in the ENCLOSING scope -- the oracle's
    /// backtrace for a raise inside one says `<main>`, not `<class:Set>` --
    /// so it stays OUT of the lifted body and runs at the marker, where the
    /// enclosing locals it reads are in scope. `(cond, run_when)`: `unless`
    /// puts the body in the else branch and runs when the condition is
    /// FALSE. Everything the site registers rides inside the branch too: a
    /// class whose guard failed was never defined.
    pub guard: Option<(crate::hir::NodeId, bool)>,
}

/// One compiled class body (a separate Ruby scope, lifted to its own
/// function exactly as the rustc backend lifts it).
pub(crate) struct ClassBodySpec {
    pub call: ClassBodyCall,
    pub class: zeo_abi::ClassId,
    pub label: String,
    pub stmts: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    /// Marker reachable INLINE from the statement stream: the body runs at
    /// its marker. Hoisted otherwise (a `class` inside a `def`): the body
    /// runs once in the toplevel prelude, the rustc backend's rule.
    pub inline: bool,
}

/// The markers whose class bodies run AT their document position -- the
/// rustc backend's `inline_class_markers` walk: statement containers
/// descend, a `def`'s body waits to be called (so its markers hoist),
/// except a block-bodied `define_method` def, whose body is a block.
fn inline_markers(
    compiler: &crate::compiler::Compiler,
    main_statements: &[crate::hir::NodeId],
) -> std::collections::HashSet<crate::hir::NodeId> {
    use crate::hir::HirNode;
    let site_stmts: HashMap<crate::hir::NodeId, &[crate::hir::NodeId]> = compiler
        .class_body_sites
        .iter()
        .filter_map(|s| s.def_node.map(|n| (n, s.stmts.as_slice())))
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut work: Vec<crate::hir::NodeId> = main_statements.to_vec();
    for &def in compiler.hir.block_bodied_defs() {
        if let HirNode::DefMethod { body, .. } = &compiler.hir[def] {
            work.extend(body.iter().copied());
        }
    }
    #[allow(
        clippy::wildcard_enum_match_arm,
        reason = "structural: every other node is a plain statement container -- the walk descends via for_each_child, the same bucket the rustc twin uses"
    )]
    while let Some(n) = work.pop() {
        match &compiler.hir[n] {
            HirNode::ClassDef { .. } => {
                if seen.insert(n)
                    && let Some(stmts) = site_stmts.get(&n)
                {
                    work.extend(stmts.iter().copied());
                }
            }
            HirNode::BoxScope { body, .. } => work.extend(body.iter().copied()),
            HirNode::DefMethod { body, .. }
                if compiler
                    .hir
                    .has_flag(n, crate::hir::NodeFlag::BLOCK_BODIED_DEF) =>
            {
                work.extend(body.iter().copied());
            }
            HirNode::DefMethod { .. } => {}
            other => other.for_each_child(&mut |c| work.push(c)),
        }
    }
    seen
}

/// Collect + declare every class-body site; refusals are loud. Mirrors
/// `emit_class_body_site_lifted`'s head registrations: the shapes whose
/// registrations the slice cannot emit yet (const_added/inherited hooks,
/// the frozen-reopen guard) refuse rather than drop.
fn collect_class_bodies(
    em: &mut Emitter,
    analyzed: &Analyzed,
) -> Result<Vec<ClassBodySpec>, String> {
    let compiler = &analyzed.compiler;
    let inline = inline_markers(compiler, &analyzed.main_statements);
    let mut specs = Vec::new();
    for (i, site) in compiler.class_body_sites.iter().enumerate() {
        let ci = compiler.class(site.class);
        let name = compiler.fq_name(site.class);
        let refuse = |what: &str| {
            Err(format!(
                "the CLIF backend cannot lower {what} yet (class {name})"
            ))
        };
        // A BUILTIN reopen's body runs like any other -- but the class was
        // never DECLARED by it (the constant pre-exists), so nothing below
        // that hangs off "declares" (const-location record, const_added /
        // inherited announcements) applies.
        let builtin = ci.is_builtin || ci.is_bootstrap || site.class.0 == 0;
        if builtin && site.stmts.is_empty() {
            continue;
        }
        if builtin && !compiler.feature_active(site.class) {
            return refuse("a require-gated builtin reopen body");
        }
        if site.def_node.is_none() {
            if site.stmts.is_empty() {
                continue;
            }
            return refuse("a synthetic class body");
        }
        let declares = !builtin
            && compiler
                .class_body_sites
                .iter()
                .find(|s| s.class == site.class)
                .is_some_and(|s| std::ptr::eq(s, site));
        // `class Foo; end` DEFINES a constant, so ruby announces it; a
        // hook observing that announcement is not emitted yet.
        let decl_owner = ci.lexical_parent.unwrap_or(crate::compiler::OBJECT_CLASS);
        // `class Foo; end` DEFINES a constant, so ruby announces it on the
        // lexically enclosing module -- only from the site that CREATES it.
        let const_added = (declares
            && (compiler.global_def_hooks.contains("const_added")
                || compiler
                    .class_method_in_chain(decl_owner, "const_added")
                    .is_some_and(|(_, hook)| {
                        crate::codegen::hook_installed_before(compiler, hook, site.def_node)
                    })))
        .then(|| (decl_owner.0, compiler.leaf_name(site.class).to_string()));
        // `Super.inherited(C)` fires when the class is CREATED, so only its
        // FIRST site announces; a reopen creates nothing. A hook written
        // BELOW this declaration is not installed yet and stays silent.
        let inherited = declares
            .then_some(ci.parent)
            .flatten()
            .filter(|&parent| {
                compiler
                    .class_method_in_chain(parent, "inherited")
                    .is_some_and(|(_, hook)| {
                        crate::codegen::hook_installed_before(compiler, hook, site.def_node)
                    })
            })
            .map(|parent| parent.0);
        // The frozen-reopen guard: a REOPEN under a program that freezes
        // classes raises `FrozenError` for the names it would newly
        // install. Registration order IS document order, so "earlier" is
        // simply the sites for this class before this one (rustc's
        // `emit_frozen_reopen_guard`).
        let freeze_guard = if compiler.program_freezes {
            let earlier: Vec<&crate::compiler::ClassBodySite> = compiler.class_body_sites[..i]
                .iter()
                .filter(|s| s.class == site.class)
                .collect();
            if earlier.is_empty() {
                Vec::new()
            } else {
                let mut names: Vec<String> = site
                    .installs
                    .iter()
                    .filter(|n| !earlier.iter().any(|s| s.installs.contains(n)))
                    .cloned()
                    .collect();
                names.sort_unstable();
                names.dedup();
                names
            }
        } else {
            Vec::new()
        };
        let const_loc = (declares)
            .then(|| {
                site.def_node
                    .and_then(|n| crate::codegen::source_location(compiler, n))
                    .map(|(file, line)| {
                        (
                            decl_owner.0,
                            compiler.leaf_name(site.class).to_string(),
                            file.to_string(),
                            line,
                        )
                    })
            })
            .flatten();
        let tail = body_tail(compiler, site);
        // A trailing `if`/`unless` on the `class` keyword arrives as the
        // body's ONE statement, wrapping everything. It belongs to the
        // ENCLOSING scope, so split it back out: the lifted body gets the
        // taken branch, the marker gets the condition.
        let (guard, body_stmts) = split_guard(compiler, &site.stmts);
        let func = if body_stmts.is_empty() {
            None
        } else {
            let sig = super::params::body_sig(em, 0, false);
            Some(
                em.module
                    .declare_function(&format!("zeo_cb_{i}"), Linkage::Local, &sig)
                    .map_err(|e| format!("declaring the {name} class body: {e}"))?,
            )
        };
        // A `class << self` body is homed on a surrogate whose reserved
        // name is unwritable; ruby SPELLS its frame label instead.
        let label = if compiler.is_singleton_surrogate(site.class) {
            "singleton class".to_string()
        } else {
            let kind = if ci.is_module { "module" } else { "class" };
            format!("<{kind}:{}>", compiler.leaf_name(site.class))
        };
        let call = ClassBodyCall {
            class: site.class.0,
            func,
            const_loc,
            const_added,
            inherited,
            reveal: ci.runtime_conditional,
            freeze_guard,
            tail,
            guard,
        };
        let is_inline = site.def_node.is_some_and(|n| inline.contains(&n));
        if is_inline && let Some(marker) = site.def_node {
            em.class_bodies.insert(marker, call.clone());
        }
        specs.push(ClassBodySpec {
            call,
            class: site.class,
            label,
            stmts: body_stmts,
            node: site.def_node,
            inline: is_inline,
        });
    }
    Ok(specs)
}

/// A class body whose ONE statement is an `If` is a `class ... end if cond`
/// (or `unless`): analyze wraps the whole body in the guard, because the
/// rustc backend splices the body inline where the condition's locals are in
/// scope. CLIF lifts the body to its own function, so the condition has to
/// come back out -- which is also where ruby runs it (the oracle's backtrace
/// for a raise in one reads `<main>`, never `<class:X>`).
///
/// One branch of such an `If` is always empty: `if` fills the then branch,
/// `unless` the else. Anything else is an ordinary `if` the body wrote, and
/// stays in the body.
fn split_guard(
    compiler: &crate::compiler::Compiler,
    stmts: &[crate::hir::NodeId],
) -> (Option<(crate::hir::NodeId, bool)>, Vec<crate::hir::NodeId>) {
    let keep = || (None, stmts.to_vec());
    let [only] = stmts else { return keep() };
    let crate::hir::HirNode::If {
        cond,
        then_body,
        else_body,
    } = &compiler.hir[*only]
    else {
        return keep();
    };
    match (then_body.is_empty(), else_body.is_empty()) {
        (false, true) => (Some((*cond, true)), then_body.clone()),
        (true, false) => (Some((*cond, false)), else_body.clone()),
        // Both empty: nothing to run either way, but the condition still
        // has to be evaluated. Both full: an ordinary `if` written as the
        // body's only statement, which the body lowers itself.
        (true, true) | (false, false) => keep(),
    }
}

/// [`BodyTail`] for one site: what the body's LAST SOURCE statement is
/// worth (rustc's `consumed_tail_value` plus its value/statement split).
#[allow(
    clippy::wildcard_enum_match_arm,
    reason = "structural: three classifiers over HirNode -- the listed kinds are the ones with no value of their own, and every other kind is an ordinary expression the tail lowering computes (refusing loudly where it cannot)"
)]
fn body_tail(
    compiler: &crate::compiler::Compiler,
    site: &crate::compiler::ClassBodySite,
) -> BodyTail {
    use crate::hir::HirNode;
    let last = site
        .def_node
        .and_then(|n| match &compiler.hir[n] {
            HirNode::ClassDef { body, .. } => body.last().copied(),
            _ => None,
        })
        .or_else(|| site.stmts.last().copied());
    let Some(last) = last else {
        return BodyTail::Own;
    };
    // Not the emitted tail: analyze consumed the source's last statement.
    if site.stmts.last() != Some(&last) {
        return match &compiler.hir[last] {
            HirNode::DefMethod { name, .. } => BodyTail::Sym(name.clone()),
            // `private_constant :Hidden` answers the module it hid the
            // constant on, which is the body's own class.
            HirNode::ConstantVisibility { .. } => BodyTail::OwnClass,
            node => BodyTail::Unknown(crate::codegen::definition_kind(node)),
        };
    }
    // A definition-level construct that survived into the statement list
    // runs for effect; everything else is an ordinary expression the tail
    // lowering computes (and refuses loudly where it cannot).
    match &compiler.hir[last] {
        HirNode::Program(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ConstantVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::MethodRedefine { .. }
        | HirNode::DefHook { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_) => {
            BodyTail::Unknown(crate::codegen::definition_kind(&compiler.hir[last]))
        }
        _ => BodyTail::Own,
    }
}

pub(crate) struct BodyFnSpec<'a> {
    pub func: FuncId,
    pub owner: zeo_abi::ClassId,
    pub owner_name: &'a str,
    pub name: &'a str,
    pub hir_params: &'a crate::hir::Params,
    pub body: &'a [crate::hir::NodeId],
    pub node: Option<crate::hir::NodeId>,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
    /// A class-method body: `self` is the Class value (frame label
    /// `Owner.name`, ivars are civars).
    pub self_is_class: bool,
    /// A non-method frame label (`<class:Foo>` for a class body); `None`
    /// derives the ordinary `Owner#name`/`Owner.name` label.
    pub label_override: Option<String>,
    /// The caller discards `out` (a class-body fn: the marker call pools
    /// it unread), so the tail runs as a STATEMENT and `out` gets nil --
    /// statement-only shapes (`include`, a nested `class`) may sit last.
    pub discard_value: bool,
    /// Ivars in this body are NAME-KEYED at runtime (a native-backed
    /// owner has no compiled slot layout).
    pub dyn_ivars: bool,
    /// The class the `def` was WRITTEN in (rustc's `cx.defining_class`;
    /// a module method keeps the module) -- None where `super` refuses.
    pub defining_class: Option<zeo_abi::ClassId>,
    /// The singleton-class SURROGATE this body was lexically written in,
    /// when the `def` sat in a constant-bearing `class << self` body.
    /// Everything LEXICAL resolves through it -- bare constants,
    /// `Module.nesting` -- while the owner keeps dispatch and ivars. See
    /// `Scope::lexical_home`.
    pub lexical_home: Option<zeo_abi::ClassId>,
}

/// A method's frame facts: `(file, label, line, end_line)` -- shared by
/// the body prologue and the trampoline's `ParamDescC`. `class_method`
/// picks ruby's `.` label separator over `#`.
fn method_frame(
    analyzed: &Analyzed,
    owner_name: &str,
    name: &str,
    node: Option<crate::hir::NodeId>,
    class_method: bool,
) -> (Option<String>, String, u32, u32) {
    let sep = if class_method { "." } else { "#" };
    let label = format!("{owner_name}{sep}{name}");
    let (line, end_line) = match node {
        Some(node) => (
            crate::codegen::source_location(&analyzed.compiler, node).map_or(0, |(_, l)| l),
            crate::codegen::source_end_line(&analyzed.compiler, node),
        ),
        None => (0, 0),
    };
    let file = analyzed.compiler.hir.files.first().map(|f| f.name.clone());
    (file, label, line, end_line)
}

/// An optional parameter whose binding waits for the frame: `ptr` null =
/// run the default; else copy the given value. A `duplicate` slot (a
/// repeated `_` name) has no storage of its own -- its default still runs
/// for side effects and WRITES the owning local (CRuby compiles a default
/// as an assignment to the local), but a given value is ignored.
struct DeferredOpt {
    name: String,
    default: crate::hir::NodeId,
    ptr: ir::Value,
    duplicate: bool,
}

/// Phase A of param binding (pre-frame, no user code): always-present
/// slots copy into their storage (cells when captured), optionals get
/// nil storage and a [`DeferredOpt`], `&b` binds from the block channel.
/// Only the FIRST slot of a repeated `_` name owns the readable local.
fn bind_param_slots(
    fx: &mut Fx,
    p: &crate::hir::Params,
    entry: &[ir::Value],
    blk_ptr: Option<ir::Value>,
    captured: &crate::compiler::FSet<String>,
) -> Vec<DeferredOpt> {
    let mut bound: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deferred = Vec::new();
    let mut s = 0usize;
    let always =
        |fx: &mut Fx, bound: &mut std::collections::HashSet<String>, name: &str, slot: usize| {
            let ptr = entry[1 + slot];
            if !bound.insert(name.to_string()) {
                return; // a later duplicate slot binds nothing
            }
            let src = super::operand::Operand::Ptr {
                addr: ptr,
                owned: false,
                tag: super::operand::TagInfo::Unknown,
            };
            if captured.contains(name) {
                let seed = fx.temp_slot();
                let seed_addr = fx.slot_addr(seed, 0);
                super::ownership::write_move_into(fx, &src, seed_addr);
                init_cell_local(fx, name.to_string(), Some(seed_addr));
            } else {
                let ss = fx.new_value_slot();
                let dst = fx.slot_addr(ss, 0);
                super::ownership::write_move_into(fx, &src, dst);
                fx.locals
                    .insert(name.to_string(), super::ctx::Local::Slot(ss));
            }
        };
    let defer = |fx: &mut Fx,
                 bound: &mut std::collections::HashSet<String>,
                 deferred: &mut Vec<DeferredOpt>,
                 name: &str,
                 default: crate::hir::NodeId,
                 slot: usize| {
        let duplicate = !bound.insert(name.to_string());
        if !duplicate {
            if captured.contains(name) {
                init_cell_local(fx, name.to_string(), None);
            } else {
                let ss = fx.new_value_slot();
                fx.locals
                    .insert(name.to_string(), super::ctx::Local::Slot(ss));
            }
        }
        deferred.push(DeferredOpt {
            name: name.to_string(),
            default,
            ptr: entry[1 + slot],
            duplicate,
        });
    };

    for name in &p.required {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    for (name, default) in &p.optional {
        defer(fx, &mut bound, &mut deferred, name, *default, s);
        s += 1;
    }
    // An anonymous `*` has no slot.
    if let Some(Some(name)) = &p.rest {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    for name in &p.post {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    for kw in &p.keywords {
        match kw {
            crate::hir::KeywordParam::Required(name) => always(fx, &mut bound, name, s),
            crate::hir::KeywordParam::Optional(name, default) => {
                defer(fx, &mut bound, &mut deferred, name, *default, s);
            }
        }
        s += 1;
    }
    // An anonymous `**` has no slot.
    if let Some(Some(name)) = &p.keyword_rest {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    let _ = s;
    // `&b`: nil when called blockless (real Ruby), else another reference
    // to the moved-in block (the body's own is still released at exit).
    if let (Some(Some(name)), Some(blk)) = (&p.block, blk_ptr)
        && bound.insert(name.to_string())
    {
        if captured.contains(name) {
            init_cell_local(fx, name.to_string(), None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals
                .insert(name.to_string(), super::ctx::Local::Slot(ss));
        }
        let got =
            fx.b.ins()
                .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, blk, 0);
        let yes = fx.b.create_block();
        let join = fx.b.create_block();
        fx.b.ins().brif(got, yes, &[], join, &[]);
        fx.b.switch_to_block(yes);
        let src = super::operand::Operand::Ptr {
            addr: blk,
            owned: false,
            tag: super::operand::TagInfo::Unknown,
        };
        super::ownership::write_local(fx, name, &src);
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(join);
    }
    deferred
}

/// Phase B (the frame exists): each deferred optional either copies its
/// given value or evaluates its default -- user code, in declared order,
/// so a later default reads every earlier binding.
fn bind_deferred(fx: &mut Fx, deferred: &[DeferredOpt]) -> Result<(), String> {
    for d in deferred {
        let given = fx.b.create_block();
        let absent = fx.b.create_block();
        let join = fx.b.create_block();
        let nonnull =
            fx.b.ins()
                .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, d.ptr, 0);
        fx.b.ins().brif(nonnull, given, &[], absent, &[]);
        fx.b.switch_to_block(given);
        if !d.duplicate {
            let src = super::operand::Operand::Ptr {
                addr: d.ptr,
                owned: false,
                tag: super::operand::TagInfo::Unknown,
            };
            super::ownership::write_local(fx, &d.name, &src);
        }
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(absent);
        let op = super::expr::lower_expr(fx, d.default)?;
        super::ownership::write_local(fx, &d.name, &op);
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(join);
    }
    Ok(())
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
    let layout = super::params::layout_of(def.hir_params)?;
    let sig = super::params::body_sig(em, layout.n_slots, def.has_blk);
    let idx = em.next_fn_index();
    let (file, label, line, end_line) = method_frame(
        analyzed,
        def.owner_name,
        def.name,
        def.node,
        def.self_is_class,
    );
    let label = def.label_override.clone().unwrap_or(label);

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
    fx.defining_class = def.defining_class;
    fx.lexical_home = def.lexical_home;
    // A body that came from a literal `define_method(:name) { .. }` rather
    // than a `def`: a BARE `super` is an error in it, and a `break` returns.
    fx.define_method_body = def.node.is_some_and(|n| {
        matches!(
            &analyzed.compiler.hir[n],
            crate::hir::HirNode::DefMethod { is_def: false, .. }
        )
    });
    fx.method_name = (!def.name.is_empty()).then(|| def.name.to_string());
    fx.method_params = Some(def.hir_params.clone());
    fx.self_is_class = def.self_is_class;
    fx.dyn_ivars = def.dyn_ivars;
    fx.frame_label = label.clone();
    fx.blk_ptr = blk_ptr;
    fx.ruby2_keywords = def.ruby2_keywords;
    let ret_ok = fx.b.create_block();
    fx.ret = Some((out_ptr, ret_ok));
    // The non-local-return home: pushed when a Proc built in this body (or
    // one running under a begin) can aim a `Signal::Return` here -- the
    // same predicate as the rustc wrapper's needs_return_catch.
    let needs_return_catch =
        crate::analyze::captures::body_contains_escaping_return(&analyzed.compiler, def.body)
            || crate::analyze::captures::body_contains_begin(&analyzed.compiler, def.body);

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
    if needs_return_catch {
        fx.call("zeo_rt_home_push", &[]);
    }

    // What escaping blocks capture becomes a cell instead of a slot.
    let captured = crate::analyze::captures::collect_escaping_captures(
        &analyzed.compiler,
        def.body,
        def.hir_params,
        crate::analyze::class_query::SelfClass::new(Some(def.owner), None),
    )
    .locals;
    // Always-present params bind now (no user code); optionals get their
    // storage and defer to after the frame exists (a default is user code
    // that can raise, and CRuby attributes it to the method).
    let deferred = bind_param_slots(&mut fx, def.hir_params, &entry_params, blk_ptr, &captured);
    // The body's other locals, nil-initialized (cells when captured) --
    // including what the DEFAULT expressions themselves assign.
    let mut locals = crate::analyze::local_storage::Locals::default();
    for &stmt in def.body {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, stmt, &mut locals);
    }
    for id in def.hir_params.default_ids() {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, id, &mut locals);
    }
    let mut hoisted_names = locals.names().to_vec();
    for (_, group) in &def.hir_params.destructures {
        group.collect_local_names(&mut hoisted_names);
    }
    for name in hoisted_names {
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

    bind_deferred(&mut fx, &deferred)?;
    // Parenthesized destructuring params replay as the multi-assignments
    // they are, after every slot is bound and before the body runs.
    for (read, group) in &def.hir_params.destructures {
        let op = super::expr::lower_expr(&mut fx, *read)?;
        let tag = op.tag();
        let ptr = super::ownership::borrow_ptr(&mut fx, &op);
        if op.owned() {
            super::ownership::pool_owned(&mut fx, ptr, tag);
        }
        super::stmt::lower_multi_group(&mut fx, *read, group, ptr)?;
    }
    if def.discard_value {
        super::stmt::lower_stmts(&mut fx, def.body)?;
        super::ownership::write_move_into(&mut fx, &super::operand::Operand::Nil, out_ptr);
    } else {
        super::stmt::lower_value_body_into(&mut fx, def.body, out_ptr)?;
    }
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
        if needs_return_catch {
            fx.call("zeo_rt_home_pop", &[]);
        }
        let code = fx.b.ins().iconst(types::I32, status);
        fx.b.ins().return_(&[code]);
    };
    fx.b.switch_to_block(ret_ok);
    epilogue(&mut fx, 0);
    let land = fx.land;
    fx.b.switch_to_block(land);
    if needs_return_catch {
        // A `Signal::Return` aimed at THIS activation (asked before the
        // home pops) folds into the method's own value.
        let kind = fx.call("zeo_rt_signal_kind", &[]).expect("kind answers");
        let is_ret = fx.b.ins().icmp_imm_u(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            kind,
            i64::from(zeo_abi::abi::SignalKind::Return as u8),
        );
        let ask = fx.b.create_block();
        let normal = fx.b.create_block();
        fx.b.ins().brif(is_ret, ask, &[], normal, &[]);
        fx.b.switch_to_block(ask);
        let mine = fx
            .call("zeo_rt_return_targets_here", &[])
            .expect("targets answers");
        let fold = fx.b.create_block();
        fx.b.ins().brif(mine, fold, &[], normal, &[]);
        fx.b.switch_to_block(fold);
        fx.call("zeo_rt_signal_take", &[out_ptr]);
        fx.b.ins().jump(ret_ok, &[]);
        fx.b.switch_to_block(normal);
        epilogue(&mut fx, 1);
    } else {
        epilogue(&mut fx, 1);
    }

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
fn define_toplevel(
    em: &mut Emitter,
    analyzed: &Analyzed,
    hoisted: &[ClassBodyCall],
) -> Result<FuncId, String> {
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

    // Alias-carrying classes with no body of their own validate here
    // (`NameError` for a source resolving nowhere); a class WITH a body
    // site validates at its body's end instead -- rustc's split.
    let unbodied: Vec<u32> = analyzed
        .compiler
        .classes
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            !c.builtin_aliases.is_empty()
                && !analyzed
                    .compiler
                    .class_body_sites
                    .iter()
                    .any(|site| site.class.0 as usize == *i)
        })
        .map(|(i, _)| i as u32)
        .collect();
    for id in unbodied {
        let cid = fx.b.ins().iconst(types::I32, i64::from(id));
        let st = fx
            .call("zeo_rt_validate_class_aliases", &[cid])
            .expect("validate_class_aliases returns a status");
        fx.fallible(st);
    }
    // Class bodies whose markers sit inside `def`s run ONCE here, before
    // the main body, in document order -- the rustc backend hoists them
    // the same way (`inline_class_markers`'s complement).
    for call in hoisted {
        stmt::emit_class_body_call(&mut fx, call)?;
    }
    // Defs registered through the row tables run nothing in statement
    // position (the rustc backend's shape: registration precedes the
    // body); a ClassDef marker now runs its body site inline.
    let runnable: Vec<crate::hir::NodeId> = analyzed
        .main_statements
        .iter()
        .copied()
        .filter(|&s| {
            !matches!(
                analyzed.compiler.hir[s],
                crate::hir::HirNode::DefMethod { .. }
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
