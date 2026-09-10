//! The two halves of a package link.
//!
//! `finish_package` is the package side -- the spec rows every compile
//! already builds are written to a MANIFEST instead of a desc, and every
//! row-referenced body is re-declared `Export` so a host can name it.
//! `merge_rows` is the host side -- a manifest's rows are appended to the
//! host's own spec vecs (its functions declared as imports), so the ONE
//! `ProgramDesc` the runtime sees carries both programs' registrations.

use super::module::Emitter;
use super::statics::{
    CmRowSpec, DescRows, MetaRowSpec, ObjRowSpec, RegRowSpec, VisRowSpec, VmRowSpec,
};
use crate::analyze::Analyzed;
use crate::diagnostics::clif::{CResult, CodegenError};
use crate::packages::package::{
    MClass, MCmRow, MIfaceClass, MIfaceMethod, MMetaRow, MObjRow, MRegRow, MVisRow, MVmRow,
    Manifest,
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
    let (name, sig) = (decl.linkage_name(f).into_owned(), decl.signature.clone());
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
    if compiler.hir.boxes > 0 {
        return refuse("Ruby::Box");
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
    if compiler.hir.data_section.is_some() {
        return refuse("an __END__ data section");
    }
    if let Some(target) = compiler.pkg_unresolved_refinements.first() {
        return refuse(&format!(
            "a refinement of unknown `{target}` (defined outside this package)"
        ));
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
    for row in rows.vm {
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
    for (idx, class) in compiler.classes.iter().enumerate() {
        let cid = idx as u32;
        // A BUILTIN appears only when the package REOPENS it -- its native
        // surface is the host's own, and the prelude's bodies are skipped
        // below -- so the row carries just the package's added methods,
        // which the host registers onto its own class of the same id.
        if cid < compiler.first_program_class_id
            && class
                .own_methods
                .iter()
                .chain(&class.own_class_methods)
                .all(|&sid| compiler.scope(sid).native_default)
        {
            continue;
        }
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
                // The prelude's own bodies (`BUILTIN_EXCEPTIONS_RB`) are the
                // host's already; only what the package WROTE crosses.
                if scope.native_default {
                    continue;
                }
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
                    unit: scope.unit,
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
            h ^= crate::packages::package::fnv64(f.source.as_bytes()).rotate_left(17);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        format!("{h:016x}")
    };
    let iface_hash = format!(
        "{:016x}",
        crate::packages::package::fnv64(
            serde_json::to_string(&iface)
                .expect("an iface serializes")
                .as_bytes()
        )
    );
    let manifest = Manifest {
        manifest_version: crate::packages::package::MANIFEST_VERSION,
        abi_version: zeo_abi::abi::ABI_VERSION,
        compiler: crate::packages::package::compiler_identity(),
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
        vm: rows
            .vm
            .iter()
            .map(|r| {
                Ok(MVmRow {
                    class: r.class,
                    box_id: r.box_id,
                    name: r.name.clone(),
                    f: symbol_of(em, r.f)?,
                })
            })
            .collect::<CResult<_>>()?,
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
        meta: rows.meta.iter().map(m_meta_row).collect(),
        redef_metas: rows.redef_metas.iter().map(m_meta_row).collect(),
        units: rows
            .unit
            .iter()
            .map(|(feature, f)| Ok((feature.clone(), symbol_of(em, *f)?)))
            .collect::<CResult<_>>()?,
        unit_init: unit_init.map(|f| symbol_of(em, f)).transpose()?,
        host_features: compiler
            .hir
            .loader
            .pkg_foreign_requires
            .iter()
            .cloned()
            .collect(),
        cov_active: em.cov_active,
        cov: super::statics::cov_rows(em, analyzed)
            .into_iter()
            .map(
                |(file, total, stmt, def)| crate::packages::package::MCovRow {
                    file,
                    total,
                    stmt,
                    def,
                },
            )
            .collect(),
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
            crate::packages::package::MFacts {
                patched_names: sorted(Box::new(compiler.runtime_patches.iter().cloned())),
                patches_any_name: compiler.runtime_patches_any_name,
                freezes: compiler.program_freezes,
                defines_bang: compiler.defines_bang(),
                blank_slate_possible: compiler.blank_slate_possible(),
                moved_receiver_possible: compiler.moved_receiver_possible(),
                const_names: sorted(Box::new(compiler.assigned_const_names.iter().cloned())),
                global_names: sorted(Box::new(compiler.global_write_sites.keys().cloned())),
                runtime_eval: compiler.compiles_at_runtime(),
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

    rows.unit.first().map(|(_, f)| *f).ok_or_else(|| {
        CodegenError::unsupported("the package entry produced no unit".to_string(), None)
    })
}

/// A method as the merged program names it: `(final class, name, class side)`.
type MethodKey = (u32, String, bool);

/// Per `(package, unit)`, the methods that unit installs when it runs:
/// `(final class, name, class side, dispatch symbol)`.
type UnitInstalls = std::collections::HashMap<(u32, u32), Vec<(u32, String, bool, String)>>;

/// Per method, every `(package, local class, dispatch symbol)` that defines
/// it statically -- more than one is the clash this pass resolves.
type StaticDefiners = std::collections::HashMap<MethodKey, Vec<(u32, u32, String)>>;

fn m_meta_row(r: &MetaRowSpec) -> MMetaRow {
    MMetaRow {
        class: r.class,
        singleton: r.singleton,
        name: r.name.clone(),
        params: r.params.clone(),
        file: r.file.clone(),
        line: r.line,
        aliased_from: r.aliased_from.clone(),
    }
}

/// The host side: append each merged manifest's rows to this program's
/// own, declaring every named symbol as an import. A no-op when nothing
/// merges, which keeps ordinary compiles byte-identical.
#[expect(
    clippy::too_many_arguments,
    reason = "one spec vec per desc table; a struct would only relocate the argument list into emit_program"
)]
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
    redef_meta_rows: &mut Vec<MetaRowSpec>,
    unit_rows: &mut Vec<(String, FuncId)>,
) -> CResult<()> {
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
    let host_compiler = crate::packages::package::compiler_identity();
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
    // ONE method name defined statically by TWO packages is ruby's
    // ordinary cross-gem monkey-patch, but the flat table can hold one
    // body. The LAST manifest's row stays static; every clashing
    // package's body installs POSITIONALLY instead -- a host thunk on the
    // defining unit replaces the runtime overlay body when that unit runs
    // (the channel a spliced redefinition uses), so require order
    // decides, exactly ruby's install-where-it-stands. The registration
    // pass already made every such name runtime-patched, so no host site
    // folds against either body.
    let mut clash: std::collections::HashMap<MethodKey, u32> = Default::default();
    let mut unit_installs: UnitInstalls = Default::default();
    {
        let mapped = |pi: usize, m: &Manifest, id: u32| -> Option<u32> {
            if id < m.first_class_id {
                Some(id)
            } else {
                analyzed
                    .compiler
                    .pkg_class_map
                    .get(&(pi as u32, id))
                    .map(|c| c.0)
            }
        };
        // (final class, name, class side) -> every (package, local class,
        // dispatch symbol) that statically defines it.
        let mut seen: StaticDefiners = Default::default();
        for (pi, m) in manifests.iter().enumerate() {
            for r in &m.vm {
                if let Some(fc) = mapped(pi, m, r.class) {
                    seen.entry((fc, r.name.clone(), false)).or_default().push((
                        pi as u32,
                        r.class,
                        r.f.clone(),
                    ));
                }
            }
            for r in &m.cm {
                if let Some(fc) = mapped(pi, m, r.class) {
                    seen.entry((fc, r.name.clone(), true)).or_default().push((
                        pi as u32,
                        r.class,
                        r.f.clone(),
                    ));
                }
            }
        }
        for (key, rows) in seen {
            let pis: std::collections::HashSet<u32> = rows.iter().map(|r| r.0).collect();
            if pis.len() < 2 {
                continue;
            }
            for (pi, local, fsym) in &rows {
                let m = &manifests[*pi as usize];
                // The defining unit, from the package's interface -- the
                // moment its file runs is the moment the install must land.
                let unit = m
                    .iface
                    .iter()
                    .find(|ic| ic.id == *local)
                    .and_then(|ic| {
                        let list = if key.2 {
                            &ic.class_methods
                        } else {
                            &ic.methods
                        };
                        list.iter().find(|im| im.name == key.1)
                    })
                    .and_then(|im| im.unit);
                let Some(unit) = unit else {
                    return Err(CodegenError::unsupported(
                        format!(
                            "two packages define `{}` on one shared class, and \
                             package '{}' carries no unit row to install its body \
                             from; compile one of them from source",
                            key.1, m.feature
                        ),
                        None,
                    ));
                };
                unit_installs.entry((*pi, unit)).or_default().push((
                    key.0,
                    key.1.clone(),
                    key.2,
                    fsym.clone(),
                ));
            }
            let kept = rows.iter().map(|r| r.0).max().unwrap_or_default();
            clash.insert(key, kept);
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
    let mut next_redef: u32 = 0;
    for (pi, m) in manifests.into_iter().enumerate() {
        let first = m.first_class_id;
        let stride = next_stride;
        next_stride += m.n_units;
        let regexp_stride = next_regexp;
        next_regexp += m.n_regexp_sites;
        let flip_flop_stride = next_flip_flop;
        next_flip_flop += m.n_flip_flops;
        let redef_stride = next_redef;
        next_redef += m.redef_metas.len() as u32;
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
        // Bind the package's virtual source root to the real gem root the
        // resolution tier recorded, so a packaged frame's backtrace shows
        // the path the spliced world would have baked. A bare
        // `--with-package` knows no root and keeps the virtual spelling.
        if let Some(root) = analyzed
            .compiler
            .hir
            .pkg_source_roots
            .get(pi)
            .and_then(|r| r.as_ref())
        {
            reg_rows.push(RegRowSpec {
                kind: zeo_abi::abi::REG_BIND_PKG_ROOT,
                class: 0,
                a: format!("/zeopkg/{}/", m.feature),
                b: format!("{}/", root.display()),
                f: None,
                ids: vec![],
                flag: 0,
            });
        }
        let cids: Vec<u32> = (0..m.n_class_ids).map(|i| rb(first + i)).collect();
        define_u32s(em, &format!("{}_cids", m.prefix), &cids)?;
        let mut bases = [0u32; super::module::N_BASES];
        bases[super::module::BASE_UNIT as usize] = stride;
        bases[super::module::BASE_REGEXP as usize] = regexp_stride;
        bases[super::module::BASE_FLIPFLOP as usize] = flip_flop_stride;
        bases[super::module::BASE_REDEF as usize] = redef_stride;
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
        for r in m.vm {
            let class = rb(r.class);
            // A cross-package clash: only the kept package's row stays
            // static; the rest install through their unit thunks.
            if let Some(&kept) = clash.get(&(class, r.name.clone(), false))
                && kept != pi as u32
            {
                continue;
            }
            let f = import(em, &r.f, &vsig)?;
            vm_rows.push(VmRowSpec {
                class,
                box_id: r.box_id,
                name: r.name,
                f,
            });
        }
        for r in m.cm {
            let class = rb(r.class);
            if let Some(&kept) = clash.get(&(class, r.name.clone(), true))
                && kept != pi as u32
            {
                continue;
            }
            let f = import(em, &r.f, &vsig)?;
            cm_rows.push(CmRowSpec {
                class,
                box_id: r.box_id,
                name: r.name,
                f,
            });
        }
        for r in m.reg {
            // A dropped clash row's conceal row goes with it: the conceal
            // map holds ONE unit per (class, name), and it must be the
            // kept row's -- the thunks own the other packages' installs.
            if r.kind == zeo_abi::abi::REG_CONCEAL_METHOD
                && let Some(&kept) = clash.get(&(rb(r.class), r.a.clone(), r.flag != 0))
                && kept != pi as u32
            {
                continue;
            }
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
                | zeo_abi::abi::REG_MARK_REFINEMENT
                | zeo_abi::abi::REG_SINGLETON_SURROGATE => r.ids.iter().map(|&i| rb(i)).collect(),
                zeo_abi::abi::REG_CONCEAL_METHOD => r.ids.iter().map(|&i| i + stride).collect(),
                zeo_abi::abi::REG_BOOT_REDEF => r.ids.iter().map(|&i| i + redef_stride).collect(),
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
                if let Some(other) = surrogate_owner_from.insert(owner, m.feature.clone()) {
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
        // Redef-meta rows sit at the FRONT of the one table, package by
        // package in merge order -- the host's own were indexed past the
        // total when its map was filled, so document order needs no
        // rewrite there.
        for (i, r) in m.redef_metas.into_iter().enumerate() {
            redef_meta_rows.insert(
                (redef_stride as usize) + i,
                MetaRowSpec {
                    class: rb(r.class),
                    singleton: r.singleton,
                    name: r.name,
                    params: r.params,
                    file: r.file,
                    line: r.line,
                    aliased_from: r.aliased_from,
                },
            );
        }
        for (ui, (spelling, sym)) in m.units.into_iter().enumerate() {
            let mut f = import(em, &sym, &usig)?;
            if let Some(installs) = unit_installs.get(&(pi as u32, ui as u32)) {
                let resolved: Vec<(u32, String, bool, FuncId)> = installs
                    .iter()
                    .map(|(cid, name, side, fsym)| {
                        Ok((*cid, name.clone(), *side, import(em, fsym, &vsig)?))
                    })
                    .collect::<CResult<_>>()?;
                f = define_redef_thunk(em, f, &usig, &resolved)?;
            }
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

/// A host wrapper on a package UNIT whose file defines a method ANOTHER
/// package also defines statically: before the unit body runs, each such
/// body is installed over the runtime overlay -- the channel a spliced
/// redefinition uses (`HirNode::MethodRedefine`) -- so the unit that ran
/// LAST answers, ruby's install-where-it-stands across two precompiled
/// objects. Same granularity as `zeo_rt_reveal_unit_methods`, which also
/// fires at the unit's head.
fn define_redef_thunk(
    em: &mut Emitter,
    orig: FuncId,
    usig: &cranelift_codegen::ir::Signature,
    installs: &[(u32, String, bool, FuncId)],
) -> CResult<FuncId> {
    use cranelift_codegen::ir::{self, InstBuilder, UserFuncName};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
    let name = format!("zeo_pkg_redef_thunk_{}", orig.as_u32());
    let id = em
        .module
        .declare_function(&name, Linkage::Local, usig)
        .map_err(|e| CodegenError::internal(format!("declaring {name}: {e}")))?;
    let mut func =
        ir::Function::with_name_signature(UserFuncName::user(1, id.as_u32()), usig.clone());
    let mut refs = Vec::new();
    for (cid, mname, side, tramp) in installs {
        let entry = if *side {
            "zeo_rt_runtime_replace_class_method"
        } else {
            "zeo_rt_runtime_replace_method"
        };
        let eid = em.import(entry);
        let e = em.module.declare_func_in_func(eid, &mut func);
        let t = em.module.declare_func_in_func(*tramp, &mut func);
        let off = em.intern_rodata(mname.as_bytes());
        refs.push((e, t, off, *cid, mname.len()));
    }
    let oref = em.module.declare_func_in_func(orig, &mut func);
    let rodata_gv = em.module.declare_data_in_func(em.rodata_id, &mut func);
    let ptr = em.ptr;
    let mut fbc = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fbc);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let out = b.block_params(entry)[0];
    let rodata = b.ins().symbol_value(ptr, rodata_gv);
    for (e, t, off, cid, len) in refs {
        let cidv = b.ins().iconst(types::I32, i64::from(cid));
        let nptr = b.ins().iadd_imm_u(rodata, i64::from(off));
        let nlen = b.ins().iconst(ptr, len as i64);
        let taddr = b.ins().func_addr(ptr, t);
        b.ins().call(e, &[cidv, nptr, nlen, taddr]);
    }
    let call = b.ins().call(oref, &[out]);
    let ret = b.func.dfg.inst_results(call)[0];
    b.ins().return_(&[ret]);
    b.seal_all_blocks();
    let cfg = em.module.target_config();
    b.finalize(cfg);
    em.define(id, func, &name, false)?;
    Ok(id)
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
