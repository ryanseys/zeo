//! The two halves of a package link.
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
use crate::package::{
    MClass, MCmRow, MIfaceClass, MIfaceMethod, MMetaRow, MObjRow, MRegRow, MVisRow, Manifest,
};
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

/// The package side: refuse the shapes a package cannot carry yet, export the
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
            format!("a package build cannot carry {what} yet"),
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
    if !compiler.refinements.is_empty() {
        return refuse("a refinement (its candidate rows carry class ids in rodata)");
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

    // The compile-time interface: every program class with its OWN methods,
    // enough for a host to register real (body-less) scopes. A method whose
    // plain body sits in `typed_methods` names it, and that body is exported
    // here -- the ONE addition to the row-referenced export sweep above.
    let mut iface = Vec::new();
    for (idx, class) in compiler
        .classes
        .iter()
        .enumerate()
        .skip(compiler.first_program_class_id as usize)
    {
        let cid = idx as u32;
        // A singleton-class surrogate is minted by the singleton machinery,
        // not registrable as an ordinary class; its ids stay unmapped and the
        // merge assigns them a fresh band as before.
        if compiler.is_singleton_surrogate(crate::compiler::ClassId(cid)) {
            continue;
        }
        let vis_of = |v: crate::hir::Visibility| match v {
            crate::hir::Visibility::Public => 0u8,
            crate::hir::Visibility::Private => 1,
            crate::hir::Visibility::Protected => 2,
        };
        let extract = |em: &mut Emitter,
                           sids: &[crate::compiler::ScopeId],
                           class_side: bool|
         -> CResult<Vec<MIfaceMethod>> {
            let mut out = Vec::new();
            for sid in sids {
                let scope = compiler.scope(*sid);
                let layout = super::params::layout_of(&scope.params)?;
                let body = if class_side {
                    None
                } else if let Some(decl) = em.typed_methods.get(&(cid, scope.name.clone())) {
                    let f = decl.body;
                    Some(f)
                } else {
                    None
                };
                let body = match body {
                    Some(f) => {
                        export(em, f)?;
                        Some(symbol_of(em, f)?)
                    }
                    None => None,
                };
                out.push(MIfaceMethod {
                    name: scope.name.clone(),
                    visibility: vis_of(scope.visibility),
                    plain: layout.plain,
                    arity: scope.params.required.len() as u32,
                    has_blk: scope.needs_block_param(),
                    runtime_conditional: scope.runtime_conditional,
                    body,
                });
            }
            Ok(out)
        };
        let methods = extract(em, &class.own_methods, false)?;
        let class_methods = extract(em, &class.own_class_methods, true)?;
        iface.push(MIfaceClass {
            id: cid,
            parent: class.parent.map(|p| p.0),
            is_module: class.is_module,
            mixin_order: class.mixin_order.iter().map(|(m, p)| (m.0, *p)).collect(),
            extends: class.extends.iter().map(|e| e.0).collect(),
            hidden_ivars: class.hidden_ivars.clone(),
            methods,
            class_methods,
        });
    }

    // The identity half: what a cache key and a host validation read.
    // The source digest hashes TEXTS only, in file order -- names carry the
    // build directory, and two checkouts of one gem must key the same.
    let source_digest = {
        let mut h: u64 = 0;
        for f in &compiler.hir.files {
            h ^= crate::package::fnv64(f.source.as_bytes()).rotate_left(17);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        format!("{h:016x}")
    };
    let iface_hash = format!(
        "{:016x}",
        crate::package::fnv64(
            serde_json::to_string(&iface).expect("an iface serializes").as_bytes()
        )
    );
    let manifest = Manifest {
        manifest_version: crate::package::MANIFEST_VERSION,
        abi_version: zeo_abi::abi::ABI_VERSION,
        compiler: format!("zeo {}", env!("CARGO_PKG_VERSION")),
        target: em.module.isa().triple().to_string(),
        source_digest,
        iface_hash,
        prefix: pkg.prefix(),
        feature: pkg.feature.clone(),
        first_class_id: compiler.first_program_class_id,
        n_class_ids: compiler.classes.len() as u32 - compiler.first_program_class_id,
        n_units,
        n_regexp_sites: em.regexp_sites,
        n_flip_flops: compiler.hir.flip_flops,
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
        callers: em.callsites.clone(),
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
        iface,
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
    let _ = vm_rows; // packages carry no vm rows yet
    if analyzed.compiler.hir.pkg_merge.is_empty() {
        return Ok(());
    }
    let manifests: Vec<Manifest> = analyzed.compiler.hir.pkg_merge.clone();
    // A feature spelling two packages -- or a package and the host -- both
    // claim would resolve by link order, which is silently wrong (the
    // pub_grub `require_relative "rubygems"` shape). A collision between
    // OBJECTS is a hard error; within one program it stays the warning
    // `warn_on_colliding_unit_features` already emits.
    let mut claimed_spellings: std::collections::HashSet<&str> =
        unit_rows.iter().map(|(s, _)| s.as_str()).collect();
    // Identity: no stable ABI tag exists yet, so the
    // contract is an EXACT match on compiler version and ISA triple --
    // anything else is a refusal that names the package, which the
    // drop-to-splice tier turns into a source recompile when it can.
    let host_triple = em.module.isa().triple().to_string();
    let host_compiler = format!("zeo {}", env!("CARGO_PKG_VERSION"));
    for m in &manifests {
        if m.target != host_triple {
            return Err(CodegenError::unsupported(
                format!(
                    "package '{}' was compiled for {}, and this build targets \
                     {host_triple}; rebuild the package",
                    m.feature, m.target
                ),
                None,
            ));
        }
        if m.compiler != host_compiler {
            return Err(CodegenError::unsupported(
                format!(
                    "package '{}' was built by {}, and this is {host_compiler}; \
                     no stable package ABI is promised yet -- rebuild the package",
                    m.feature, m.compiler
                ),
                None,
            ));
        }
        for (spelling, _) in &m.units {
            if !claimed_spellings.insert(spelling.as_str()) {
                return Err(CodegenError::unsupported(
                    format!(
                        "feature '{spelling}' is provided by package '{}' and by \
                         another object in this program; rename one or drop the \
                         package",
                        m.feature
                    ),
                    None,
                ));
            }
        }
    }
    // A shared namespace (`module Rack` in two packages) ALIASES: the
    // registration pass mapped both local ids onto one host id and held
    // the compatibility line (one ancestry side, disjoint methods), so
    // here the second package's class row folds into the first's -- the
    // merged desc keeps ONE row per final id, patched per field with
    // whichever side actually contributed that field.
    let mut merged_class_row_at: std::collections::HashMap<u32, usize> = Default::default();
    // A singleton-class SURROGATE seeds the runtime's mint for its owner,
    // and the runtime keeps one; two packages each bringing a surrogate
    // for one aliased owner would race that seed.
    let mut surrogate_owner_from: std::collections::HashMap<u32, String> = Default::default();

    let vsig = super::params::value_fn_sig(em);
    let usig = unit_sig(em);
    let import = |em: &mut Emitter, name: &str, sig| -> CResult<FuncId> {
        em.module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| CodegenError::internal(format!("importing {name}: {e}")))
    };
    // Band assignment. The host's own classes end where its compiler's
    // table does; each package's band follows in merge order, and the
    // id-translation table its object imports is DEFINED here with the
    // final ids -- the moment a position-independent object becomes
    // correct in THIS program. Reveal-group strides stack the same way
    // (the host's own groups already start past the packages' total).
    let mut next_band = analyzed.compiler.classes.len() as u32;
    let mut next_stride: u32 = 0;
    let mut next_regexp: u32 = 0;
    let mut next_flip_flop: u32 = 0;
    for (pi, m) in manifests.into_iter().enumerate() {
        let first = m.first_class_id;
        let stride = next_stride;
        next_stride += m.n_units;
        let regexp_stride = next_regexp;
        next_regexp += m.n_regexp_sites;
        let flip_flop_stride = next_flip_flop;
        next_flip_flop += m.n_flip_flops;
        // Local id -> final id. An INTERFACE-REGISTERED class already has
        // its host id (`pkg_class_map`, minted right after the bootstrap
        // band); only the unregistered residue -- singleton surrogates --
        // takes a fresh id here. Builtins (below the package's own band)
        // and the u32::MAX no-caller sentinel pass through.
        let mut fresh: std::collections::HashMap<u32, u32> = Default::default();
        for i in 0..m.n_class_ids {
            let local = first + i;
            if !analyzed
                .compiler
                .pkg_class_map
                .contains_key(&(pi as u32, local))
            {
                fresh.insert(local, next_band);
                next_band += 1;
            }
        }
        let rb = |id: u32| -> u32 {
            if id == u32::MAX || id < first {
                id
            } else if let Some(c) = analyzed.compiler.pkg_class_map.get(&(pi as u32, id)) {
                c.0
            } else {
                fresh[&id]
            }
        };
        let cids: Vec<u32> = (0..m.n_class_ids).map(|i| rb(first + i)).collect();
        define_u32s(em, &format!("{}_cids", m.prefix), &cids)?;
        let mut bases = [0u32; super::module::N_BASES];
        bases[super::module::BASE_UNIT as usize] = stride;
        bases[super::module::BASE_REGEXP as usize] = regexp_stride;
        bases[super::module::BASE_FLIPFLOP as usize] = flip_flop_stride;
        define_u32s(em, &format!("{}_bases", m.prefix), &bases)?;
        if !m.callers.is_empty() {
            let callers: Vec<u32> = m.callers.iter().map(|&c| rb(c)).collect();
            define_u32s(em, &format!("{}_callers", m.prefix), &callers)?;
        }
        for c in m.classes {
            let spec = super::classes::ClassSpec {
                id: rb(c.id),
                name: c.name,
                ancestors: c.ancestors.iter().map(|&a| rb(a)).collect(),
                ivars: c.ivars,
                hidden: c.hidden,
                members: c.members,
                kind: c.kind,
            };
            match merged_class_row_at.entry(spec.id) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(class_specs.len());
                    class_specs.push(spec);
                }
                std::collections::hash_map::Entry::Occupied(e) => {
                    // An aliased class: fold onto the standing row. Each
                    // field was contributed by at most one side (the
                    // registration pass refused anything else), so the
                    // fuller side of each is the merged truth.
                    let row = &mut class_specs[*e.get()];
                    debug_assert_eq!(row.name, spec.name, "aliased rows share a name");
                    if spec.ancestors.len() > row.ancestors.len() {
                        row.ancestors = spec.ancestors;
                    }
                    if row.ivars.is_empty() {
                        row.ivars = spec.ivars;
                    }
                    // `hidden` counts the trailing hidden members, so it
                    // travels with the members list it describes.
                    if row.members.is_empty() {
                        row.members = spec.members;
                        row.hidden = spec.hidden;
                    }
                }
            }
        }
        for r in m.obj {
            let f = import(em, &r.f, &vsig)?;
            obj_rows.push(ObjRowSpec {
                class: rb(r.class),
                name: r.name,
                f,
            });
        }
        for r in m.cm {
            let f = import(em, &r.f, &vsig)?;
            cm_rows.push(CmRowSpec {
                class: rb(r.class),
                box_id: r.box_id,
                name: r.name,
                f,
            });
        }
        for r in m.reg {
            // What a reg row's `ids` MEAN depends on its kind: class ids
            // for the mixin/ancestry/surrogate kinds, reveal-group ids for
            // a concealment, and plain scalars (a slot, a redef index)
            // everywhere else. Rebasing the wrong space is a silently
            // wrong program, so the kinds are named here one by one.
            let ids: Vec<u32> = match r.kind {
                zeo_abi::abi::REG_EXTENDS
                | zeo_abi::abi::REG_SINGLETON_SUPER_TARGET
                | zeo_abi::abi::REG_SET_ANCESTORS
                | zeo_abi::abi::REG_REGISTER_BUILTIN
                | zeo_abi::abi::REG_SINGLETON_SURROGATE => {
                    r.ids.iter().map(|&i| rb(i)).collect()
                }
                zeo_abi::abi::REG_CONCEAL_METHOD => {
                    r.ids.iter().map(|&i| i + stride).collect()
                }
                _ => r.ids,
            };
            // The shared-bootstrap rows both sides emit (a builtin's boot
            // `extend`, a require-gated builtin's registration): the host's
            // copy stands, because a SECOND `REG_REGISTER_BUILTIN` would
            // re-register without the native constructor. A row only the
            // package emits (a gated builtin the host never reaches) stays.
            let class = rb(r.class);
            if r.kind == zeo_abi::abi::REG_SINGLETON_SURROGATE {
                let owner = ids[0]; // already remapped by the kind match
                if let Some(other) =
                    surrogate_owner_from.insert(owner, m.feature.clone())
                {
                    return Err(CodegenError::unsupported(
                        format!(
                            "packages '{other}' and '{}' both bring a singleton-\
                             class surrogate for one aliased class; the runtime \
                             mint seeds once -- compile one of them from source",
                            m.feature
                        ),
                        None,
                    ));
                }
            }
            let bootstrap_dup = matches!(
                r.kind,
                zeo_abi::abi::REG_EXTENDS | zeo_abi::abi::REG_REGISTER_BUILTIN
            ) && reg_rows
                .iter()
                .any(|h| h.kind == r.kind && h.class == class && h.ids == ids);
            if bootstrap_dup {
                continue;
            }
            let f = match r.f {
                Some(name) => Some(import(em, &name, &vsig)?),
                None => None,
            };
            reg_rows.push(RegRowSpec {
                kind: r.kind,
                class,
                a: r.a,
                b: r.b,
                f,
                ids,
                flag: r.flag,
            });
        }
        for r in m.vis {
            vis_rows.push(VisRowSpec {
                class: rb(r.class),
                name: r.name,
                verb: r.verb,
            });
        }
        foreign.extend(m.foreign.into_iter().map(|(class, name)| (rb(class), name)));
        for r in m.meta {
            meta_rows.push(MetaRowSpec {
                class: rb(r.class),
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
    // Past the patched-class bitmap's span, a typed-direct guard has no
    // bit to read. The package compiled its guards against LOCAL ids under
    // the span; the FINAL ids must stay under it too, or those guards read
    // some other class's bit. Refused rather than slowed: widening the
    // bitmap is a one-constant, backward-compatible change.
    if next_band > zeo_abi::abi::PATCHED_BITS_IDS {
        return Err(CodegenError::unsupported(
            format!(
                "the merged class-id space ({next_band} ids) exceeds the patched-class \
                 bitmap span ({}); widen PATCHED_BITS_IDS or merge fewer packages",
                zeo_abi::abi::PATCHED_BITS_IDS
            ),
            None,
        ));
    }
    // The run-time compiler hands eval snippets site ids from its own band;
    // link-time strides growing into it would share latches and regexp
    // caches with snippets, which is silently wrong, not slow.
    let strides_fit = next_regexp <= zeo_abi::abi::EVAL_SITE_BASE
        && next_flip_flop <= zeo_abi::abi::EVAL_SITE_BASE;
    if !strides_fit {
        return Err(CodegenError::unsupported(
            format!(
                "the merged site-id strides ({next_regexp} regexp, {next_flip_flop} \
                 flip-flop) exceed the eval band floor ({})",
                zeo_abi::abi::EVAL_SITE_BASE
            ),
            None,
        ));
    }
    Ok(())
}

/// The `packaged-ids` bench mode's identity id-translation table: the
/// program is its own "package" whose band lands exactly where it
/// compiled, so every table load answers the id an immediate would have
/// been. Defined only when some site actually loaded through the table.
pub(crate) fn define_identity_cids(em: &mut Emitter, analyzed: &Analyzed) -> CResult<()> {
    let Some(id) = em.cids_id else {
        return Ok(());
    };
    let super::module::IdMode::Packaged { first } = em.id_mode else {
        return Ok(());
    };
    if em.pkg.is_some() {
        return Ok(()); // a package IMPORTS its table; the host defines it
    }
    let n = analyzed.compiler.classes.len() as u32;
    let mut bytes = Vec::with_capacity(((n - first) as usize) * 4);
    for i in first..n {
        bytes.extend_from_slice(&i.to_le_bytes());
    }
    use cranelift_module::DataDescription;
    let mut data = DataDescription::new();
    data.define(bytes.into_boxed_slice());
    data.set_align(4);
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining zeo_cids: {e}")))?;
    Ok(())
}

/// One little-endian `u32` array as EXPORTED initialized data -- the id
/// tables and base cells a package object imports, defined by the host
/// with the values only the host knows.
fn define_u32s(em: &mut Emitter, name: &str, vals: &[u32]) -> CResult<()> {
    use cranelift_module::DataDescription;
    let id = em
        .module
        .declare_data(name, Linkage::Export, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring {name}: {e}")))?;
    let mut bytes = Vec::with_capacity(vals.len() * 4);
    for v in vals {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let mut data = DataDescription::new();
    data.define(bytes.into_boxed_slice());
    data.set_align(4);
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {name}: {e}")))?;
    Ok(())
}
