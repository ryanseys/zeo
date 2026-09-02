//! Code emission: ISA/flags, the module (an object file for AOT,
//! in-process code memory for JIT -- same lowering either way), the capi
//! import cache, and the per-program orchestration -- prologue/epilogue of
//! the compiled `<main>`, the emitted C `main`, and the statics (see
//! `statics`).

use super::ctx::Fx;
use super::module::{ClifModule, Emitter};
use super::{statics, stmt};
use crate::analyze::Analyzed;
use crate::codegen_error::{CResult, CodegenError};
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, UserFuncName, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{DataId, FuncId, Linkage, Module};

/// Lower `analyzed` to one object file's bytes. `debuginfo` adds DWARF
/// line tables (`-g`); see `clif::debuginfo`.
pub fn compile(analyzed: &Analyzed, debuginfo: bool) -> CResult<Vec<u8>> {
    compile_inner(analyzed, false, debuginfo).map(|(bytes, _)| bytes)
}

/// `compile` plus the per-function CLIF text (`--emit-clif`, snapshots).
pub fn compile_with_clif(analyzed: &Analyzed) -> CResult<(Vec<u8>, String)> {
    compile_inner(analyzed, true, false).map(|(bytes, text)| (bytes, text.expect("collected")))
}

fn compile_inner(
    analyzed: &Analyzed,
    collect_clif: bool,
    debuginfo: bool,
) -> CResult<(Vec<u8>, Option<String>)> {
    let mut em = Emitter::new(false)?;
    em.clif_text = collect_clif.then(String::new);
    em.debug = debuginfo.then(super::debuginfo::DebugInfo::default);
    let started = std::time::Instant::now();
    emit_program(&mut em, analyzed)?;
    em.flush_pending()?;
    report_codegen_time(&em, started);
    let clif = em.clif_text.take();
    let debug = em.debug.take();
    let ClifModule::Object(module) = em.module else {
        unreachable!("Emitter::new(false) builds an object module")
    };
    let mut product = module.finish();
    if let Some(debug) = &debug {
        debug.emit(&mut product)?;
    }
    let bytes = product
        .emit()
        .map_err(|e| CodegenError::internal(format!("emitting the object file: {e}")))?;
    Ok((bytes, clif))
}

/// Split one emission into the two halves that cost anything: the lowering
/// that builds each function's CLIF, and Cranelift's own backend turning that
/// CLIF into machine code. Only the second half can be run on more than one
/// core, so the split is what says whether doing so is worth anything.
fn report_codegen_time(em: &Emitter, started: std::time::Instant) {
    let whole = started.elapsed().as_secs_f64();
    let backend = em.codegen_nanos as f64 / 1e9;
    tracing::info!(
        functions = em.codegen_fns,
        "emission {whole:.2}s: cranelift {backend:.2}s ({:.0}%), lowering {:.2}s",
        match whole > 0.0 {
            true => backend / whole * 100.0,
            false => 0.0,
        },
        whole - backend,
    );
}

/// A JIT-compiled program: finalized in-process code plus the emitted C
/// `main`'s address. The module OWNS the code memory -- it must outlive
/// every call into `main`.
pub struct Jitted {
    pub module: JITModule,
    pub main: *const u8,
}

/// Lower `analyzed` straight into executable memory (`--backend jit`).
pub fn compile_jit(analyzed: &Analyzed) -> CResult<Jitted> {
    let mut em = Emitter::new(true)?;
    let started = std::time::Instant::now();
    let main = emit_program(&mut em, analyzed)?;
    em.flush_pending()?;
    report_codegen_time(&em, started);
    let ClifModule::Jit(mut module) = em.module else {
        unreachable!("Emitter::new(true) builds a JIT module")
    };
    module
        .finalize_definitions()
        .map_err(|e| CodegenError::internal(format!("finalizing jitted code: {e}")))?;
    let main = module.get_finalized_function(main);
    Ok(Jitted { module, main })
}

/// The whole program into `em`'s module -- every function and data object,
/// mode-blind. Returns the emitted C `main`.
fn emit_program(em: &mut Emitter, analyzed: &Analyzed) -> CResult<FuncId> {
    em.cov_active = crate::analyze::coverage::active(&analyzed.compiler);
    // A package build swaps the desc for a manifest at
    // the end of this function; a host merging packages offsets every
    // reveal-group id it bakes past theirs. Zero/None on an ordinary
    // compile, which keeps the output byte-identical.
    em.pkg = analyzed.compiler.hir.pkg_build.clone();
    em.unit_base = analyzed
        .compiler
        .hir
        .pkg_merge
        .iter()
        .map(|m| m.n_units)
        .sum();
    // The other program-dense site spaces follow the reveal-group rule:
    // merged packages own `[0, base)`, and this program's own ids start
    // past their total. The regexp counter starts AT the base, so every
    // minted id is final; flip-flop ids come dense from the arena and are
    // offset at the one lowering site.
    em.regexp_sites = analyzed
        .compiler
        .hir
        .pkg_merge
        .iter()
        .map(|m| m.n_regexp_sites)
        .sum();
    em.flip_flop_base = analyzed
        .compiler
        .hir
        .pkg_merge
        .iter()
        .map(|m| m.n_flip_flops)
        .sum();
    em.redef_base = analyzed
        .compiler
        .hir
        .pkg_merge
        .iter()
        .map(|m| m.redef_metas.len() as u32)
        .sum();
    // A package reads its own-band class ids through the id-translation
    // table the host fills -- position independence. The `packaged-ids`
    // debug flag forces the same emission program-wide over an identity
    // table, which is the bench upper bound for the indirection.
    em.id_mode = if em.pkg.is_some()
        || crate::debug_flags::debug(crate::debug_flags::DebugFlag::PackagedIds)
    {
        super::module::IdMode::Packaged {
            first: analyzed.compiler.first_program_class_id,
        }
    } else {
        super::module::IdMode::Immediate
    };
    super::collect::collect_reopen_flags(em, analyzed);
    let defs = super::collect::collect_methods(em, analyzed)?;
    let collected = super::classes::collect_classes(em, analyzed)?;
    let class_bodies = super::collect::collect_class_bodies(em, analyzed)?;
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
        conceal_methods,
        singleton_surrogates,
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
        collected.conceal_methods,
        collected.singleton_surrogates,
        collected.redefs,
        collected.boot_redefs,
        collected.set_ancestors,
        collected.register_builtin,
    );
    // Object's own table carries every module mixed in ABOVE it, a `module
    // Kernel` reopen included. Those rows are PRESENT on Object but not
    // OWNED by it, so they take the same foreign mark a builtin carrier's
    // copies do. Without it `Object.instance_methods(false)` listed a Kernel
    // method, and the owner scan stopped at Object on its way up the chain.
    let mut foreign = foreign;
    for d in &defs {
        if d.defining_class != crate::compiler::OBJECT_CLASS {
            foreign.push((crate::compiler::OBJECT_CLASS.0, d.name.clone()));
        }
    }
    // Populated BEFORE any body is defined: a class body's
    // `MethodRedefine` statement reads it while its own fn is built.
    for (i, m) in redefs.iter().enumerate() {
        em.redef_tramps.insert((m.owner.0, m.scope.0), m.tramp);
        // Each body's reflection row rides the same key. It is the half of a
        // redefinition timeline the overlay did not carry: the body installed
        // where it stands, and `#arity`/`#parameters`/`#source_location`
        // still answering from the LAST one. Merged packages' rows sit at
        // the front of the one table, so this compile's start past them.
        em.redef_metas
            .insert((m.owner.0, m.scope.0), em.redef_base + i as u32);
    }
    for def in &defs {
        let func = em.methods[&def.name].body;
        let spec = super::body::BodyFnSpec {
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
            origin_name: def.alias_of.as_deref(),
            // A top-level `def` belongs to main.
            box_id: 0,
        };
        super::body::define_method_body(em, analyzed, &spec)?;
    }
    for m in &obj_methods {
        if let Some(func) = m.body_fn {
            let spec = super::body::BodyFnSpec {
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
                origin_name: m.alias_of.as_deref(),
                box_id: analyzed.compiler.class(m.owner).box_id,
            };
            super::body::define_method_body(em, analyzed, &spec)?;
        }
    }
    for m in cm_methods.iter().filter(|m| !m.shared) {
        let spec = super::body::BodyFnSpec {
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
            origin_name: m.alias_of.as_deref(),
            box_id: analyzed.compiler.class(m.owner).box_id,
        };
        super::body::define_method_body(em, analyzed, &spec)?;
    }
    for m in &redefs {
        let spec = super::body::BodyFnSpec {
            func: m.body_fn,
            owner: m.owner,
            owner_name: &m.owner_name,
            name: &m.name,
            hir_params: &m.hir_params,
            body: &m.body,
            node: m.node,
            has_blk: m.has_blk,
            ruby2_keywords: m.ruby2_keywords,
            // A `def self.x` body runs with the CLASS as `self`, so its `@x`
            // is a class-level ivar and its `@@x` resolves against the class.
            self_is_class: m.singleton,
            label_override: None,
            discard_value: false,
            dyn_ivars: false,
            defining_class: Some(m.owner),
            lexical_home: None,
            origin_name: None,
            box_id: analyzed.compiler.class(m.owner).box_id,
        };
        super::body::define_method_body(em, analyzed, &spec)?;
    }
    for m in &mod_methods {
        if m.shared {
            continue;
        }
        let spec = super::body::BodyFnSpec {
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
            origin_name: m.alias_of.as_deref(),
            box_id: analyzed.compiler.class(m.owner).box_id,
        };
        super::body::define_method_body(em, analyzed, &spec)?;
    }
    let empty_params = crate::hir::Params::default();
    for cb in &class_bodies {
        let Some(func) = cb.call.func else { continue };
        let owner_name = analyzed.compiler.fq_name(cb.class);
        let spec = super::body::BodyFnSpec {
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
            discard_value: cb.call.tail != super::collect::BodyTail::Own,
            dyn_ivars: false,
            defining_class: None,
            // A class body's OWN cref is its class; the surrogate case is
            // carried by the `def`s inside it, not by the body fn.
            lexical_home: None,
            origin_name: None,
            box_id: analyzed.compiler.class(cb.class).box_id,
        };
        super::body::define_method_body(em, analyzed, &spec)?;
    }
    for def in &defs {
        let decl = &em.methods[&def.name];
        let (tramp, body, has_blk) = (decl.tramp, decl.body, decl.has_blk);
        let idx = em.next_fn_index();
        let (file, label, line, end_line) =
            super::body::method_frame(analyzed, "Object", &def.name, def.node, false);
        // Object's table holds what a module MATERIALIZED onto it, so a
        // `def require` written in `module Kernel` arrives here -- and it
        // needs Kernel's reopen flag. With none it was live from BOOT, which
        // is how rubygems' `def require` ran before the `module Kernel` body
        // that declares the constant it reads.
        let reopen_flag = em
            .reopen_flags
            .get(&(def.defining_class.0, def.name.clone()))
            .copied();
        let spec = super::params::TrampSpec {
            tramp,
            body,
            params: &def.hir_params,
            has_blk,
            reopen_flag,
            reopen_unit: reopen_flag.is_some_and(|i| em.unit_reopen_flags.contains(&i)),
            name: &def.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    for m in &obj_methods {
        // A SHARED row reuses the body and trampoline `collect_methods`
        // already emitted on `Object` for the whole program. Its registration
        // row is real; there is nothing here to compile, and defining that
        // trampoline a second time is an error rather than a duplicate.
        if m.shared {
            continue;
        }
        let idx = em.next_fn_index();
        match (m.accessor, m.body_fn) {
            (Some((slot, kind, attr_generated)), None) => {
                // A hand-written accessor body folded to a slot trampoline
                // keeps its frame; an `attr_*`-generated one has none.
                let framed = (!attr_generated).then(|| {
                    super::body::method_frame(analyzed, &m.owner_name, &m.name, m.node, false)
                });
                let frame = framed
                    .as_ref()
                    .map(|(file, label, line, end)| (file.as_deref(), label.as_str(), *line, *end));
                super::params::define_accessor(em, m.tramp, slot, kind, idx, frame)?;
            }
            (None, Some(body)) => {
                let (file, label, line, end_line) =
                    super::body::method_frame(analyzed, &m.owner_name, &m.name, m.node, false);
                let spec = super::params::TrampSpec {
                    tramp: m.tramp,
                    body,
                    params: &m.hir_params,
                    has_blk: m.has_blk,
                    reopen_flag: em
                        .reopen_flags
                        .get(&(m.defining_class.0, m.name.clone()))
                        .copied(),
                    reopen_unit: em
                        .reopen_flags
                        .get(&(m.defining_class.0, m.name.clone()))
                        .is_some_and(|i| em.unit_reopen_flags.contains(i)),
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
            super::body::method_frame(analyzed, &m.owner_name, &m.name, m.node, false);
        let spec = super::params::TrampSpec {
            tramp: m.tramp,
            body: m.body_fn,
            params: &m.hir_params,
            has_blk: m.has_blk,
            reopen_flag: None,
            reopen_unit: false,
            name: &m.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    for m in &mod_methods {
        if m.shared {
            continue;
        }
        let idx = em.next_fn_index();
        let (file, label, line, end_line) =
            super::body::method_frame(analyzed, &m.owner_name, &m.name, m.node, false);
        let spec = super::params::TrampSpec {
            tramp: m.tramp,
            body: m.body_fn,
            params: &m.hir_params,
            has_blk: m.has_blk,
            reopen_flag: em
                .reopen_flags
                .get(&(m.defining_class.0, m.name.clone()))
                .copied(),
            reopen_unit: em
                .reopen_flags
                .get(&(m.defining_class.0, m.name.clone()))
                .is_some_and(|i| em.unit_reopen_flags.contains(i)),
            name: &m.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    for m in cm_methods.iter().filter(|m| !m.shared) {
        let idx = em.next_fn_index();
        let (file, label, line, end_line) =
            super::body::method_frame(analyzed, &m.owner_name, &m.name, m.node, true);
        let spec = super::params::TrampSpec {
            tramp: m.tramp,
            body: m.body_fn,
            params: &m.hir_params,
            has_blk: m.has_blk,
            // The CLASS-method side takes no flag: a native class-method row
            // is reached through the singleton chain, not the value channel
            // the forward walks.
            reopen_flag: None,
            reopen_unit: false,
            name: &m.name,
            file: file.as_deref(),
            label: &label,
            line,
            end_line,
        };
        super::params::define_trampoline(em, &spec, idx)?;
    }
    let hoisted: Vec<super::collect::ClassBodyCall> = class_bodies
        .iter()
        .filter(|cb| !cb.inline)
        .map(|cb| cb.call.clone())
        .collect();
    // A package has no `<main>`: its top-level code is its unit's body, and
    // the host program owns the one real main.
    let toplevel = if em.pkg.is_none() {
        Some(super::body::define_toplevel(
            em,
            analyzed,
            &super::body::TopScope::Main { hoisted: &hoisted },
            &analyzed.main_statements,
        )?)
    } else {
        None
    };
    // One fn per compiled-in load-path file, registered under BOTH spellings
    // a program can build: the load-path-relative feature name and the
    // absolute path `File.expand_path("x", __dir__)` produces.
    let mut unit_rows: Vec<(String, FuncId)> = Vec::new();
    for (i, (features, absolute, stmts)) in analyzed.feature_units.iter().enumerate() {
        let f = super::body::define_toplevel(
            em,
            analyzed,
            &super::body::TopScope::Unit {
                index: i,
                file: format!("{absolute}.rb"),
            },
            stmts,
        )?;
        unit_rows.extend(features.iter().map(|name| (name.clone(), f)));
        unit_rows.push((absolute.clone(), f));
    }
    // A merged package's exported initializer chains onto this object's.
    let extra_inits: Vec<String> = analyzed
        .compiler
        .hir
        .pkg_merge
        .iter()
        .filter_map(|m| m.unit_init.clone())
        .collect();
    let unit_init = statics::define_unit_init(em, &extra_inits)?;
    statics::define_syms(em)?;
    statics::define_callsites(em)?;
    statics::define_cm_sites(em)?;
    statics::define_ffi_sites(em)?;
    statics::define_const_sites(em)?;
    statics::define_new_sites(em)?;
    statics::define_dyn_sites(em)?;
    statics::define_proc_shapes(em)?;
    statics::define_reopen_flags(em)?;
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
        box_id: m.box_id,
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
            box_id: m.box_id,
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
    // contribution to a `super` walk (the trampoline IS the
    // dynamic-self bridge such a row needs).
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
    // Accessor slot rows: `(class, name) -> (slot, writer)` so the value
    // CallSite's fill can cache the SLOT instead of the trampoline.
    // attr-GENERATED rows only -- a hand-written accessor's trampoline
    // carries a frame the slot access would not.
    reg_rows.extend(obj_methods.iter().filter_map(|m| {
        let (slot, kind, attr_generated) = m.accessor?;
        if !attr_generated {
            return None;
        }
        Some(statics::RegRowSpec {
            kind: zeo_abi::abi::REG_ACCESSOR_SLOT,
            class: m.owner.0,
            a: m.name.clone(),
            b: String::new(),
            f: None,
            ids: vec![slot as u32],
            flag: u8::from(matches!(kind, crate::compiler::AccessorKind::Writer)),
        })
    }));
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
    // ...and so do the rows a compiled-in unit's `def`s own, until the
    // unit's own function reveals them.
    reg_rows.extend(
        conceal_methods
            .iter()
            .map(|(class, name, class_side, unit)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_CONCEAL_METHOD,
                class: *class,
                a: name.clone(),
                b: String::new(),
                f: None,
                // Reveal groups [0, unit_base) belong to merged packages.
                ids: vec![em.unit_base + *unit],
                flag: u8::from(*class_side),
            }),
    );
    // The verb byte `zeo_rt_runtime_set_visibility` and
    // `zeo_rt_install_positional_visibility` both read.
    fn vis_byte(v: crate::hir::Visibility) -> u8 {
        match v {
            crate::hir::Visibility::Private => 0,
            crate::hir::Visibility::Protected => 1,
            crate::hir::Visibility::Public => 2,
        }
    }
    // The FIRST body of every observable redefinition timeline installs
    // before the first statement runs.
    reg_rows.extend(
        boot_redefs
            .iter()
            .map(|(class, name, sid, singleton, vis)| statics::RegRowSpec {
                kind: zeo_abi::abi::REG_BOOT_REDEF,
                class: *class,
                a: name.clone(),
                b: String::new(),
                f: Some(em.redef_tramps[&(*class, sid.0)]),
                ids: vec![em.redef_metas[&(*class, sid.0)]],
                // Bit 0 is the CHANNEL: a `def self.x` installs on the
                // class-method side of the overlay, whose row is a different
                // map. Bits 1-2 are this body's own VISIBILITY, which the
                // first body needs marked before the first statement runs --
                // a `private :v` written between two bodies retagged this
                // `def` at lower time.
                flag: u8::from(*singleton) | (vis_byte(*vis) << 1),
            }),
    );
    // A class method that is really an `extend`ed module's row, flattened
    // onto the class for the steady state. It is retired until the `extend`
    // statement seats the module, so a call above the `extend` raises and a
    // hook it supplies does not fire for a `def` written earlier -- which is
    // where ruby's singleton chain gains the module.
    for (idx, _) in analyzed.compiler.classes.iter().enumerate() {
        let class = crate::compiler::ClassId(idx as u32);
        reg_rows.extend(
            analyzed
                .compiler
                .class_methods_deferred_by_extend(class)
                .into_iter()
                .map(|name| statics::RegRowSpec {
                    kind: zeo_abi::abi::REG_DEFER_EXTENDED_CLASS_METHOD,
                    class: class.0,
                    a: name,
                    b: String::new(),
                    f: None,
                    ids: vec![],
                    flag: 0,
                }),
        );
    }
    // A definition hook written on `Module`/`Class`/`BasicObject` itself
    // answers for every class, and no per-class owner scan can see one.
    {
        let mut names: Vec<&String> = analyzed.compiler.global_def_hooks.iter().collect();
        names.sort();
        reg_rows.extend(names.into_iter().map(|n| statics::RegRowSpec {
            kind: zeo_abi::abi::REG_MARK_GLOBAL_DEF_HOOK,
            class: 0,
            a: n.clone(),
            b: String::new(),
            f: None,
            ids: vec![],
            flag: 0,
        }));
    }
    // A `refine` holder is a module in every respect but one: its own
    // `.class` is `Refinement`, which is what a refined `Method#owner`
    // reports -- and the mark is what makes the runtime's refined lookup
    // find the holder's rows at all.
    for (idx, _) in analyzed.compiler.classes.iter().enumerate() {
        let holder = crate::compiler::ClassId(idx as u32);
        if let Some((module, target)) = analyzed.compiler.refinement_of(holder) {
            reg_rows.push(statics::RegRowSpec {
                kind: zeo_abi::abi::REG_MARK_REFINEMENT,
                class: holder.0,
                a: String::new(),
                b: String::new(),
                f: None,
                ids: vec![module.0, target.0],
                flag: 0,
            });
        }
    }
    // A class a BOX owns. Its ruby name is bare -- `Escapee` written in a
    // box is called `Escapee` -- so the registry's name table would let
    // main reach it as a nested class of `Object`. The mark is what keeps
    // the two apart; a program with no box emits none of these.
    for (idx, class) in analyzed.compiler.classes.iter().enumerate() {
        if class.box_id != 0 {
            reg_rows.push(statics::RegRowSpec {
                kind: zeo_abi::abi::REG_MARK_BOX_CLASS,
                class: idx as u32,
                a: String::new(),
                b: String::new(),
                f: None,
                // The box, then the SHARED class this one overlays (itself
                // when it overlays nothing). A per-box builtin overlay
                // registers no entry of its own, so state written against it
                // -- a class ivar is the case -- has to normalize to the
                // root, or the write and the read name different classes.
                ids: vec![
                    class.box_id,
                    class.builtin_overlay.map_or(idx as u32, |root| root.0),
                ],
                flag: 0,
            });
        }
    }
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
        alias_rows.iter().map(
            |(class, new, old, is_class, box_id, eager)| statics::RegRowSpec {
                kind: if *is_class {
                    zeo_abi::abi::REG_CLASS_ALIAS
                } else {
                    zeo_abi::abi::REG_ALIAS
                },
                class: *class,
                a: new.clone(),
                b: old.clone(),
                f: None,
                // The box the alias was WRITTEN in, so a box's
                // `class << self; alias_method :x, :y; end` on a shared
                // class does not rename anything for main.
                ids: vec![*box_id],
                // 1 = bind the ancestor's body once, at registration: the
                // aliasing class writes the SOURCE name later, and a live
                // indirection would follow that later `def`.
                flag: u8::from(*eager),
            },
        ),
    );
    // Reflection rows: one per emitted method scope, in the order the
    // tables above register them.
    let mut meta_rows: Vec<statics::MetaRowSpec> = Vec::new();
    for d in &defs {
        meta_rows.push(super::collect::meta_row(
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
        meta_rows.push(super::collect::meta_row(
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
        meta_rows.push(super::collect::meta_row(
            analyzed,
            m.owner.0,
            false,
            &m.name,
            &m.hir_params,
            m.node,
            m.alias_of.as_deref(),
        ));
    }
    // One row per BODY of a redefined method, in `redefs` order. They are not
    // registered at boot -- the position that is live installs its own.
    let redef_metas: Vec<statics::MetaRowSpec> = redefs
        .iter()
        .map(|m| {
            super::collect::meta_row(
                analyzed,
                m.owner.0,
                m.singleton,
                &m.name,
                &m.hir_params,
                m.node,
                None,
            )
        })
        .collect();
    // A re-scoped inherited method reflects on THIS class: after `private
    // :x`, ruby answers the subclass for `instance_method(:x).owner` while
    // still running the ancestor's body.
    for (idx, class) in analyzed.compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap {
            continue;
        }
        for e in analyzed
            .compiler
            .methods_of(crate::compiler::ClassId(idx as u32))
            .iter()
            .filter(|e| e.zsuper)
        {
            let scope = analyzed.compiler.scope(e.def);
            meta_rows.push(super::collect::meta_row(
                analyzed,
                idx as u32,
                false,
                &scope.name,
                &scope.params,
                scope.def_node,
                scope.alias_of.as_deref(),
            ));
        }
    }
    for m in cm_methods.iter().filter(|m| m.cm_row) {
        meta_rows.push(super::collect::meta_row(
            analyzed,
            m.owner.0,
            true,
            &m.name,
            &m.hir_params,
            m.node,
            m.alias_of.as_deref(),
        ));
    }
    // The JIT never needs the reference: the `zeo` process it runs in
    // installed the compiler itself. A LINKED program names the installer
    // so the linker keeps the compiler for it -- and only for it.
    // A host merging packages appends their manifest
    // rows to its own before the one desc is emitted; a no-op without any.
    let mut class_specs = class_specs;
    let mut obj_rows = obj_rows;
    let mut cm_rows = cm_rows;
    let mut redef_metas = redef_metas;
    super::pkg::merge_rows(
        em,
        analyzed,
        &mut class_specs,
        &mut vm_rows,
        &mut vis_rows,
        &mut obj_rows,
        &mut cm_rows,
        &mut reg_rows,
        &mut foreign,
        &mut meta_rows,
        &mut redef_metas,
        &mut unit_rows,
    )?;
    // A package build writes those same rows to a
    // MANIFEST beside its object instead of a desc, and emits no `main`.
    if em.pkg.is_some() {
        let f = super::pkg::finish_package(
            em,
            analyzed,
            &statics::DescRows {
                vm: &vm_rows,
                vis: &vis_rows,
                classes: &class_specs,
                obj: &obj_rows,
                cm: &cm_rows,
                reg: &reg_rows,
                foreign: &foreign,
                meta: &meta_rows,
                redef_metas: &redef_metas,
                unit: &unit_rows,
            },
            unit_init,
        )?;
        statics::define_rodata(em)?;
        return Ok(f);
    }
    super::pkg::define_identity_cids(em, analyzed)?;
    let eval_install =
        matches!(em.module, ClifModule::Object(_)) && analyzed.compiler.compiles_at_runtime();
    let desc = statics::define_desc(
        em,
        analyzed,
        &statics::DescSpec {
            toplevel: toplevel.expect("a non-package compile defines <main>"),
            unit_init,
            eval_install,
            rows: statics::DescRows {
                vm: &vm_rows,
                vis: &vis_rows,
                classes: &class_specs,
                obj: &obj_rows,
                cm: &cm_rows,
                reg: &reg_rows,
                foreign: &foreign,
                meta: &meta_rows,
                redef_metas: &redef_metas,
                unit: &unit_rows,
            },
        },
    )?;
    let main = define_main(em, desc)?;
    statics::define_rodata(em)?;
    Ok(main)
}

/// The parameter eligibility shared by top-level and class methods:
/// the full positional/keyword surface binds; destructures and the
/// block-only trailing-comma rest are still refusals.
pub(crate) fn check_params(p: &crate::hir::Params) -> Result<(), &'static str> {
    if p.implicit_rest {
        return Err("an implicit-rest parameter");
    }
    Ok(())
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

/// The installs that belong to the PROGRAM, not to a top-level scope:
/// alias validation for body-less classes, `TOPLEVEL_BINDING`, and the class
/// bodies whose markers sit inside `def`s. A required file runs none of them.
pub(super) fn main_installs(
    fx: &mut Fx,
    analyzed: &Analyzed,
    hoisted: &[super::collect::ClassBodyCall],
) -> CResult<()> {
    // Alias sources NO body site claims validate here (`NameError` for a
    // source resolving nowhere): every row of a class with no body of its
    // own, plus a toplevel-written alias on a class that has one. A row a
    // body site's own `alias` wrote validates at that body's end instead.
    let unclaimed: Vec<(u32, String)> = analyzed
        .compiler
        .classes
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            !c.builtin_aliases.is_empty()
                && !((c.is_builtin || c.is_bootstrap)
                    && !analyzed
                        .compiler
                        .builtin_is_reachable(zeo_abi::ClassId(*i as u32)))
        })
        .flat_map(|(i, c)| {
            let claimed: Vec<String> = analyzed
                .compiler
                .class_body_sites
                .iter()
                .filter(|site| site.class.0 as usize == i)
                .flat_map(|site| super::collect::site_alias_checks(&analyzed.compiler, site))
                .collect();
            let mut olds: Vec<String> = c
                .builtin_aliases
                .iter()
                .map(|(_, terminal, _)| terminal.clone())
                .filter(|old| !claimed.contains(old))
                .collect();
            olds.sort_unstable();
            olds.dedup();
            olds.into_iter().map(move |old| (i as u32, old))
        })
        .collect();
    for (id, old) in unclaimed {
        let cid = fx.cid_value(id);
        let (nptr, nlen) = super::expr::rodata_name(fx, &old);
        let st = fx.call_status("zeo_rt_validate_alias_source", &[cid, nptr, nlen]);
        fx.fallible(st);
    }
    // `TOPLEVEL_BINDING`, installed UNCONDITIONALLY so `Object.constants`
    // lists it (the census asks). A program that never names it gets the
    // cheap degraded form -- self = `main`, no locals -- because
    // `binding_names` stayed `None`; naming it anywhere upgrades both.
    {
        let op = super::expr::binding_value_at(fx, "<main>", 0);
        let vp = super::ownership::borrow_ptr(fx, &op);
        super::ownership::pool_owned(fx, vp, op.tag());
        let (nptr, nlen) = super::expr::rodata_name(fx, "TOPLEVEL_BINDING");
        let (fptr, flen) = super::expr::rodata_name(fx, "<main>");
        let owner = fx.b.ins().iconst(types::I32, 0);
        let line = fx.b.ins().iconst(types::I32, 0);
        let fxbox = fx.box_v();
        fx.call(
            "zeo_rt_const_set_at",
            &[owner, nptr, nlen, vp, fptr, flen, line, fxbox],
        );
    }
    // Class bodies whose markers sit inside `def`s run ONCE here, before
    // the main body, in document order (`inline_markers`'s complement).
    for call in hoisted {
        stmt::emit_class_body_call(fx, call)?;
    }
    Ok(())
}

/// The exported C `main(argc, argv)`: tail-calls `zeo_rt_main` with the
/// program description.
fn define_main(em: &mut Emitter, desc: DataId) -> CResult<FuncId> {
    let mut sig = em.module.make_signature();
    sig.params.push(AbiParam::new(types::I32));
    sig.params.push(AbiParam::new(em.ptr));
    sig.returns.push(AbiParam::new(types::I32));
    let func_id = em
        .module
        .declare_function("main", Linkage::Export, &sig)
        .map_err(|e| CodegenError::internal(format!("declaring main: {e}")))?;
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
    let call = b.ins().call(rt_main, &[argc, argv, desc_ptr]);
    let code = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[code]);
    b.seal_all_blocks();
    b.finalize(cfg);

    em.record_clif("main", &func);
    em.define(func_id, func, "main", true)?;
    Ok(func_id)
}
