//! EXPERIMENTAL (M0): the two halves of a package link.
//!
//! `finish_package` is the package side -- the spec rows every compile
//! already builds are written to a MANIFEST instead of a desc, and every
//! row-referenced body is re-declared `Export` so a host can name it.
//! `merge_rows` is the host side -- a manifest's rows are appended to the
//! host's own spec vecs (its functions declared as imports), so the ONE
//! `ProgramDesc` the runtime sees carries both programs' registrations.

use super::module::Emitter;
use super::statics::{CmRowSpec, DescRows, MetaRowSpec, ObjRowSpec, RegRowSpec, VisRowSpec, VmRowSpec};
use crate::analyze::Analyzed;
use crate::codegen_error::{CResult, CodegenError};
use crate::package::{MClass, MCmRow, MMetaRow, MObjRow, MRegRow, MVisRow, Manifest};
use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::ir::types;
use cranelift_module::{FuncId, Linkage, Module};

/// The `UnitFn` signature (`extern "C" fn(out) -> i32`).
fn unit_sig(em: &Emitter) -> cranelift_codegen::ir::Signature {
    let mut sig = em.module.make_signature();
    sig.params.push(AbiParam::new(em.ptr));
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// The declared symbol name behind a `FuncId`.
fn symbol_of(em: &Emitter, f: FuncId) -> CResult<String> {
    Ok(em
        .module
        .declarations()
        .get_function_decl(f)
        .linkage_name(f)
        .into_owned())
}

/// Re-declare `f`'s name with `Export` linkage, so the host object's desc
/// relocations can name it. `cranelift-module` merges linkage upward, so
/// this strengthens a `Local` declaration in place.
fn export(em: &mut Emitter, f: FuncId) -> CResult<()> {
    let decl = em.module.declarations().get_function_decl(f);
    let (name, sig) = (
        decl.linkage_name(f).into_owned(),
        decl.signature.clone(),
    );
    em.module
        .declare_function(&name, Linkage::Export, &sig)
        .map_err(|e| CodegenError::internal(format!("exporting {name}: {e}")))?;
    Ok(())
}

/// The package side: refuse the shapes M0 does not carry, export the
/// row-referenced bodies, and write the manifest beside the object.
/// Returns a FuncId for `emit_program`'s signature; the object path
/// ignores it.
pub(crate) fn finish_package(
    em: &mut Emitter,
    analyzed: &Analyzed,
    rows: &DescRows<'_>,
    unit_init: Option<FuncId>,
) -> CResult<FuncId> {
    let pkg = em.pkg.clone().expect("finish_package runs in package mode");
    let compiler = &analyzed.compiler;
    let refuse = |what: &str| {
        Err(CodegenError::unsupported(
            format!("a package build cannot carry {what} yet (M0)"),
            None,
        ))
    };
    if compiler.compiles_at_runtime() {
        return refuse("eval (the package would need the run-time compiler)");
    }
    if compiler.hir.boxes > 0 {
        return refuse("Ruby::Box");
    }
    if !rows.redef_metas.is_empty() {
        return refuse("an observable redefinition timeline");
    }
    if !rows.vm.is_empty() {
        return refuse("a builtin reopen (value-channel rows)");
    }
    if !analyzed.main_statements.is_empty() {
        return refuse("top-level code outside its unit");
    }
    if let Some((feature, _, reason)) = analyzed.declined_units.first() {
        return Err(CodegenError::unsupported(
            format!("the package's unit '{feature}' was declined: {reason}"),
            None,
        ));
    }
    if compiler.hir.flip_flops > 0 {
        return refuse("a flip-flop");
    }
    if em.regexp_sites > 0 {
        return refuse("a regexp literal (its site id is program-dense)");
    }
    if em.ffi_sites > 0 || compiler.hir.ffi.ffi_lib_slots > 0 || compiler.hir.ffi.ffi_enum_slots > 0
    {
        return refuse("an FFI declaration");
    }
    if em.cov_active {
        return refuse("coverage");
    }
    if compiler.hir.data_section.is_some() {
        return refuse("an __END__ data section");
    }

    // Reveal groups: the package's units plus its alias-reveal groups
    // share one id space starting at 0.
    let mut n_units = analyzed.feature_units.len() as u32;
    for (_, _, _, group) in &compiler.alias_source_reveals {
        n_units = n_units.max(group + 1);
    }

    for row in rows.obj {
        export(em, row.f)?;
    }
    for row in rows.cm {
        export(em, row.f)?;
    }
    for row in rows.reg {
        if let Some(f) = row.f {
            export(em, f)?;
        }
    }
    for (_, f) in rows.unit {
        export(em, *f)?;
    }
    // The unit fns and unit_init were declared Export already (their names
    // carry the prefix); the sweep above is what catches everything else a
    // row can name.

    let manifest = Manifest {
        manifest_version: crate::package::MANIFEST_VERSION,
        abi_version: zeo_abi::abi::ABI_VERSION,
        prefix: pkg.prefix(),
        feature: pkg.feature.clone(),
        first_class_id: compiler.first_program_class_id,
        n_class_ids: compiler.classes.len() as u32 - compiler.first_program_class_id,
        n_units,
        classes: rows
            .classes
            .iter()
            .map(|c| MClass {
                id: c.id,
                name: c.name.clone(),
                ancestors: c.ancestors.clone(),
                ivars: c.ivars.clone(),
                hidden: c.hidden,
                members: c.members.clone(),
                kind: c.kind,
            })
            .collect(),
        vm: vec![],
        vis: rows
            .vis
            .iter()
            .map(|r| MVisRow {
                class: r.class,
                name: r.name.clone(),
                verb: r.verb,
            })
            .collect(),
        obj: rows
            .obj
            .iter()
            .map(|r| {
                Ok(MObjRow {
                    class: r.class,
                    name: r.name.clone(),
                    f: symbol_of(em, r.f)?,
                })
            })
            .collect::<CResult<_>>()?,
        cm: rows
            .cm
            .iter()
            .map(|r| {
                Ok(MCmRow {
                    class: r.class,
                    box_id: r.box_id,
                    name: r.name.clone(),
                    f: symbol_of(em, r.f)?,
                })
            })
            .collect::<CResult<_>>()?,
        reg: rows
            .reg
            .iter()
            .map(|r| {
                Ok(MRegRow {
                    kind: r.kind,
                    class: r.class,
                    a: r.a.clone(),
                    b: r.b.clone(),
                    f: r.f.map(|f| symbol_of(em, f)).transpose()?,
                    ids: r.ids.clone(),
                    flag: r.flag,
                })
            })
            .collect::<CResult<_>>()?,
        foreign: rows.foreign.to_vec(),
        meta: rows
            .meta
            .iter()
            .map(|r| MMetaRow {
                class: r.class,
                singleton: r.singleton,
                name: r.name.clone(),
                params: r.params.clone(),
                file: r.file.clone(),
                line: r.line,
                aliased_from: r.aliased_from.clone(),
            })
            .collect(),
        units: rows
            .unit
            .iter()
            .map(|(feature, f)| Ok((feature.clone(), symbol_of(em, *f)?)))
            .collect::<CResult<_>>()?,
        unit_init: unit_init.map(|f| symbol_of(em, f)).transpose()?,
        class_tables: super::statics::needed_class_tables(analyzed)
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        facts: {
            let sorted = |it: Box<dyn Iterator<Item = String>>| -> Vec<String> {
                let mut v: Vec<String> = it.collect();
                v.sort();
                v
            };
            crate::package::MFacts {
                patched_names: sorted(Box::new(compiler.runtime_patches.iter().cloned())),
                patches_any_name: compiler.runtime_patches_any_name,
                freezes: compiler.program_freezes,
                defines_bang: compiler.defines_bang(),
                blank_slate_possible: compiler.blank_slate_possible(),
                moved_receiver_possible: compiler.moved_receiver_possible(),
                const_names: sorted(Box::new(compiler.assigned_const_names.iter().cloned())),
                global_names: sorted(Box::new(compiler.global_write_sites.keys().cloned())),
            }
        },
    };
    std::fs::write(&pkg.manifest_out, manifest.to_json()).map_err(|e| {
        CodegenError::internal(format!(
            "writing the package manifest {}: {e}",
            pkg.manifest_out.display()
        ))
    })?;

    rows.unit
        .first()
        .map(|(_, f)| *f)
        .ok_or_else(|| {
            CodegenError::unsupported("the package entry produced no unit".to_string(), None)
        })
}

/// The host side: append each merged manifest's rows to this program's
/// own, declaring every named symbol as an import. A no-op when nothing
/// merges, which keeps ordinary compiles byte-identical.
#[allow(clippy::too_many_arguments)] // one spec vec per desc table; a struct
// would only relocate the argument list into emit_program.
pub(crate) fn merge_rows(
    em: &mut Emitter,
    analyzed: &Analyzed,
    class_specs: &mut Vec<super::classes::ClassSpec>,
    vm_rows: &mut Vec<VmRowSpec>,
    vis_rows: &mut Vec<VisRowSpec>,
    obj_rows: &mut Vec<ObjRowSpec>,
    cm_rows: &mut Vec<CmRowSpec>,
    reg_rows: &mut Vec<RegRowSpec>,
    foreign: &mut Vec<(u32, String)>,
    meta_rows: &mut Vec<MetaRowSpec>,
    unit_rows: &mut Vec<(String, FuncId)>,
) -> CResult<()> {
    let _ = vm_rows; // packages carry no vm rows in M0
    if analyzed.compiler.hir.pkg_merge.is_empty() {
        return Ok(());
    }
    let manifests: Vec<Manifest> = analyzed.compiler.hir.pkg_merge.clone();
    // A feature spelling two packages -- or a package and the host -- both
    // claim would resolve by link order, which is silently wrong (the
    // pub_grub `require_relative "rubygems"` shape). A collision between
    // OBJECTS is a hard error; within one program it stays the warning
    // `warn_on_colliding_unit_features` already emits.
    let host_spellings: std::collections::HashSet<&str> =
        unit_rows.iter().map(|(s, _)| s.as_str()).collect();
    let host_class_names: std::collections::HashSet<&str> =
        class_specs.iter().map(|c| c.name.as_str()).collect();
    for m in &manifests {
        for (spelling, _) in &m.units {
            if host_spellings.contains(spelling.as_str()) {
                return Err(CodegenError::unsupported(
                    format!(
                        "feature '{spelling}' is provided by both package '{}' and this \
                         program; rename one or drop the package",
                        m.feature
                    ),
                    None,
                ));
            }
        }
        // A shared namespace (`module Rack` in both objects) is ordinary
        // Ruby, but merging it needs the id-translation tier's ALIASING
        // (M2): two ClassSpecs under one name would resolve by
        // registration order, silently. Refuse it by name until then.
        for c in &m.classes {
            if host_class_names.contains(c.name.as_str()) {
                return Err(CodegenError::unsupported(
                    format!(
                        "class {} is defined by both package '{}' and this program; \
                         a cross-object reopen needs the id-translation tier (M2)",
                        c.name, m.feature
                    ),
                    None,
                ));
            }
        }
    }

    let vsig = super::params::value_fn_sig(em);
    let usig = unit_sig(em);
    let import = |em: &mut Emitter, name: &str, sig| -> CResult<FuncId> {
        em.module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| CodegenError::internal(format!("importing {name}: {e}")))
    };
    for m in manifests {
        for c in m.classes {
            class_specs.push(super::classes::ClassSpec {
                id: c.id,
                name: c.name,
                ancestors: c.ancestors,
                ivars: c.ivars,
                hidden: c.hidden,
                members: c.members,
                kind: c.kind,
            });
        }
        for r in m.obj {
            let f = import(em, &r.f, &vsig)?;
            obj_rows.push(ObjRowSpec {
                class: r.class,
                name: r.name,
                f,
            });
        }
        for r in m.cm {
            let f = import(em, &r.f, &vsig)?;
            cm_rows.push(CmRowSpec {
                class: r.class,
                box_id: r.box_id,
                name: r.name,
                f,
            });
        }
        for r in m.reg {
            // The shared-bootstrap rows both sides emit (a builtin's boot
            // `extend`, a require-gated builtin's registration): the host's
            // copy stands, because a SECOND `REG_REGISTER_BUILTIN` would
            // re-register without the native constructor. A row only the
            // package emits (a gated builtin the host never reaches) stays.
            let bootstrap_dup = matches!(
                r.kind,
                zeo_abi::abi::REG_EXTENDS | zeo_abi::abi::REG_REGISTER_BUILTIN
            ) && reg_rows
                .iter()
                .any(|h| h.kind == r.kind && h.class == r.class && h.ids == r.ids);
            if bootstrap_dup {
                continue;
            }
            let f = match r.f {
                Some(name) => Some(import(em, &name, &vsig)?),
                None => None,
            };
            reg_rows.push(RegRowSpec {
                kind: r.kind,
                class: r.class,
                a: r.a,
                b: r.b,
                f,
                ids: r.ids,
                flag: r.flag,
            });
        }
        for r in m.vis {
            vis_rows.push(VisRowSpec {
                class: r.class,
                name: r.name,
                verb: r.verb,
            });
        }
        foreign.extend(m.foreign);
        for r in m.meta {
            meta_rows.push(MetaRowSpec {
                class: r.class,
                singleton: r.singleton,
                name: r.name,
                params: r.params,
                file: r.file,
                line: r.line,
                aliased_from: r.aliased_from,
            });
        }
        for (spelling, sym) in m.units {
            let f = import(em, &sym, &usig)?;
            unit_rows.push((spelling, f));
        }
    }
    Ok(())
}
