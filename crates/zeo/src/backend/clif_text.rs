//! `zeo backend`: a binary from CLIF text another front end wrote.
//!
//! The Rust emitter lowers Ruby to Cranelift IR in memory. A front end
//! written in something else -- `ze0/`, the Ruby one -- writes the same IR
//! as TEXT, one function per Ruby body, naming the runtime's entry points
//! and the program's data symbols by name. This reads that text back with
//! `cranelift-reader`, resolves every name against the same emitter the
//! Rust path uses, materialises the registration-shaped half a `.zeodata`
//! sidecar describes (`super::sidecar`), and links the result exactly as
//! `zeo build` would.
//!
//! Names are the whole contract. The reader parses `%name` to
//! `ExternalName::TestCase`, which `cranelift-module` refuses to relocate,
//! so every callee and every data symbol is rewritten to the module's own
//! `User` names before a function is defined. A callee that is neither a
//! function in the file nor a runtime entry point is an error naming it,
//! and a runtime entry point called with the wrong shape is an error too:
//! a front end's ABI slip fails the build rather than the program.

use std::collections::HashMap;
use std::path::Path;

use cranelift_codegen::ir::{self, ExternalName, GlobalValueData, UserExternalName, UserFuncName};
use cranelift_module::{DataId, FuncId, Linkage, Module};

use super::sidecar::{
    ALL_FEATURES, ALL_TABLES, BASIC_OBJECT_SUPERCLASS, BOOT_ROWS, CLASS_KINDS, CallerClass,
    OBJECT_SUPERCLASS,
    PARAM_KINDS, RegEntry, RegRow, SEED_TABLES, Sidecar,
};
use crate::clif::module::Emitter;
use crate::clif::{capi_names, emit, names, statics};
use crate::diagnostics::clif::{CResult, CodegenError};

/// Link `clif` (with `sidecar`, or the `.zeodata` beside it, or none) into
/// `output`.
pub fn build(clif: &Path, sidecar: Option<&Path>, output: &Path) -> Result<(), String> {
    let text =
        std::fs::read_to_string(clif).map_err(|e| format!("reading {}: {e}", clif.display()))?;
    let beside = clif.with_extension("zeodata");
    let sidecar = match sidecar {
        Some(path) => Sidecar::read(path)?,
        None if beside.is_file() => Sidecar::read(&beside)?,
        None => Sidecar::default(),
    };
    let object = compile(&text, &sidecar).map_err(|e| format!("{}: {e}", clif.display()))?;
    super::object::object_to_binary(&object, false, false, &[], &[], output)
}

/// The object file for `text` and `sidecar`.
pub fn compile(text: &str, sidecar: &Sidecar) -> CResult<Vec<u8>> {
    if sidecar.abi_version != zeo_abi::abi::ABI_VERSION {
        return Err(CodegenError::internal(format!(
            "the sidecar is ABI version {}; this zeo is {}",
            sidecar.abi_version,
            zeo_abi::abi::ABI_VERSION
        )));
    }
    let functions = cranelift_reader::parse_functions(text)
        .map_err(|e| CodegenError::internal(format!("parsing the CLIF: {e}")))?;
    let mut em = Emitter::new(false)?;
    em.seed_rodata(sidecar.rodata_bytes().map_err(CodegenError::internal)?);
    for sym in &sidecar.syms {
        em.syms.intern(sym);
    }

    // Every function is declared before any is defined, so a call forward
    // in the file resolves like one backward.
    let call_conv = em.module.isa().default_call_conv();
    let mut ids: HashMap<String, FuncId> = HashMap::new();
    let mut declared = Vec::new();
    for mut func in functions {
        let name = own_name(&func.name)?;
        // The two the sidecar materialises: `--emit-clif` prints them so the
        // text is whole, and they are rebuilt here from the tables.
        if name == "main" || name == names::UNIT_INIT {
            continue;
        }
        func.signature.call_conv = call_conv;
        let id = em
            .module
            .declare_function(&name, Linkage::Local, &func.signature)
            .map_err(|e| CodegenError::internal(format!("declaring {name}: {e}")))?;
        if ids.insert(name.clone(), id).is_some() {
            return Err(CodegenError::internal(format!("`{name}` is defined twice")));
        }
        declared.push((name, id, func));
    }
    let (class_specs, class_ids) = class_specs(sidecar)?;
    em.callsites = caller_classes(&sidecar.callsites, &class_ids)?;
    let class_ids_data = define_class_ids(&mut em, sidecar, &class_ids)?;
    for (name, id, mut func) in declared {
        resolve_names(&mut em, &ids, class_ids_data, &name, &mut func)?;
        cranelift_codegen::verify_function(&func, em.module.isa())
            .map_err(|e| CodegenError::internal(format!("{name}: {e}")))?;
        em.define(id, func, &name, false)?;
    }

    let toplevel = *ids.get(&sidecar.toplevel).ok_or_else(|| {
        CodegenError::internal(format!(
            "the sidecar names `{}` as the top level, which the text does not define",
            sidecar.toplevel
        ))
    })?;
    let unit_init = statics::define_unit_init(&mut em, &[])?;
    statics::define_syms(&mut em)?;
    statics::define_callsites(&mut em)?;
    statics::define_cm_sites(&mut em)?;
    statics::define_ffi_sites(&mut em)?;
    statics::define_const_sites(&mut em)?;
    statics::define_new_sites(&mut em)?;
    statics::define_dyn_sites(&mut em)?;
    statics::define_proc_shapes(&mut em)?;
    statics::define_reopen_flags(&mut em)?;
    let mut defs = def_rows(sidecar, &ids, &class_ids)?;
    defs.cm.extend(inherited_class_methods(sidecar, &class_ids, &ids)?);
    let mut reg_rows = reg_rows(&sidecar.reg, &ids)?;
    reg_rows.extend(own_method_rows(sidecar, &class_ids));
    reg_rows.extend(super_target_rows(sidecar, &ids, &class_ids)?);
    reg_rows.extend(feature_rows(&sidecar.features)?);
    reg_rows.extend(sidecar.classes.iter().flat_map(|c| {
        c.private_constants.iter().map(|name| statics::RegRowSpec {
            kind: zeo_abi::abi::REG_CONST_PRIVATE,
            class: class_ids[&c.name],
            a: name.clone(),
            b: String::new(),
            f: None,
            ids: Vec::new(),
            flag: 0,
        })
    }));
    let unit_rows = unit_rows(&sidecar.units, &ids)?;
    let program = statics::DescProgram {
        warnings: sidecar.warnings.clone(),
        load_path: sidecar.load_path.clone(),
        n_load_path_search: sidecar.n_load_path_search,
        embedded_sources: Vec::new(),
        class_tables: class_tables(&sidecar.class_tables)?,
        data_section: None,
        coverage: Vec::new(),
    };
    let desc = statics::define_desc(
        &mut em,
        &program,
        &statics::DescSpec {
            toplevel,
            unit_init,
            eval_install: sidecar.eval_install,
            rows: statics::DescRows {
                vm: &defs.vm,
                vis: &defs.vis,
                classes: &class_specs,
                obj: &defs.obj,
                cm: &defs.cm,
                reg: &reg_rows,
                foreign: &[],
                meta: &defs.meta,
                redef_metas: &[],
                unit: &unit_rows,
            },
        },
    )?;
    emit::define_main(&mut em, desc)?;
    statics::define_rodata(&mut em)?;
    emit::finish_object(em)
}

/// The lazily-run feature units, resolved to the functions the text
/// defines. The runtime keys them by spelling with any `.rb` stripped, so
/// several rows may name one function.
fn unit_rows(
    units: &[super::sidecar::Unit],
    in_file: &HashMap<String, FuncId>,
) -> CResult<Vec<(String, FuncId)>> {
    units
        .iter()
        .map(|unit| {
            let f = *in_file.get(&unit.symbol).ok_or_else(|| {
                CodegenError::internal(format!(
                    "the unit `{}` names `{}`, which the text does not define",
                    unit.feature, unit.symbol
                ))
            })?;
            Ok((unit.feature.clone(), f))
        })
        .collect()
}

/// The `%name` a function was written under.
fn own_name(name: &UserFuncName) -> CResult<String> {
    match name {
        UserFuncName::Testcase(tc) => Ok(String::from_utf8_lossy(tc.raw()).into_owned()),
        UserFuncName::User(u) => Err(CodegenError::internal(format!(
            "a function is named u{}:{} rather than %<symbol>",
            u.namespace, u.index
        ))),
    }
}

/// Rewrite every `%name` `func` refers to into the module's own `User`
/// name: a callee to the function in the file or the runtime entry point,
/// a data symbol to the emitter's table of that name.
fn resolve_names(
    em: &mut Emitter,
    in_file: &HashMap<String, FuncId>,
    class_ids: DataId,
    in_fn: &str,
    func: &mut ir::Function,
) -> CResult<()> {
    let call_conv = em.module.isa().default_call_conv();
    for sig in func.dfg.signatures.values_mut() {
        sig.call_conv = call_conv;
    }
    let callees: Vec<ir::FuncRef> = func.dfg.ext_funcs.keys().collect();
    for fref in callees {
        let ExternalName::TestCase(tc) = &func.dfg.ext_funcs[fref].name else {
            continue;
        };
        let callee = String::from_utf8_lossy(tc.raw()).into_owned();
        let target = match in_file.get(&callee) {
            Some(&id) => id,
            None => {
                let have = &func.dfg.signatures[func.dfg.ext_funcs[fref].signature];
                import(em, in_fn, &callee, have)?
            }
        };
        let colocated = em
            .module
            .declarations()
            .get_function_decl(target)
            .linkage
            .is_final();
        let user = func.declare_imported_user_function(UserExternalName::new(0, target.as_u32()));
        let ext = &mut func.dfg.ext_funcs[fref];
        ext.name = ExternalName::User(user);
        ext.colocated = colocated;
    }
    let symbols: Vec<ir::GlobalValue> = func.global_values.keys().collect();
    for gv in symbols {
        let GlobalValueData::Symbol {
            name: ExternalName::TestCase(tc),
            offset,
            ..
        } = &func.global_values[gv]
        else {
            continue;
        };
        let symbol = String::from_utf8_lossy(tc.raw()).into_owned();
        let offset = *offset;
        let data = data_symbol(em, class_ids, &symbol).ok_or_else(|| {
            CodegenError::internal(format!(
                "{in_fn} names the data symbol `{symbol}`, which is not one a program carries"
            ))
        })?;
        let colocated = em
            .module
            .declarations()
            .get_data_decl(data)
            .linkage
            .is_final();
        let user = func.declare_imported_user_function(UserExternalName::new(1, data.as_u32()));
        func.global_values[gv] = GlobalValueData::Symbol {
            name: ExternalName::User(user),
            offset,
            colocated,
            tls: false,
        };
    }
    Ok(())
}

/// The runtime entry point `callee`, once its shape in the text matches
/// the runtime's.
fn import(em: &mut Emitter, in_fn: &str, callee: &str, have: &ir::Signature) -> CResult<FuncId> {
    let Some(row) = capi_names::CAPI.iter().find(|r| r.name == callee) else {
        return Err(CodegenError::internal(format!(
            "{in_fn} calls `{callee}`, which is neither a function in this file nor a runtime \
             entry point (clif/capi_names.rs)"
        )));
    };
    let want = em.capi_signature(row);
    if want.params != have.params || want.returns != have.returns {
        return Err(CodegenError::internal(format!(
            "{in_fn} calls `{callee}` as `{have}`; the runtime's shape is `{want}`"
        )));
    }
    Ok(em.import(row.name))
}

/// The data tables a program carries, by symbol.
fn data_symbol(em: &Emitter, class_ids: DataId, symbol: &str) -> Option<DataId> {
    Some(match symbol {
        CLASS_IDS => class_ids,
        names::RODATA => em.rodata_id,
        names::SYMS => em.syms_id,
        names::CALLSITES => em.callsites_id,
        names::CM_SITES => em.cm_sites_id,
        names::FFI_SITES => em.ffi_sites_id,
        names::CONST_SITES => em.const_sites_id,
        names::NEW_SITES => em.new_sites_id,
        names::DYN_SITES => em.dyn_sites_id,
        names::PROC_SHAPES => em.proc_shapes_id,
        names::REOPEN_FLAGS => em.reopen_flags_id,
        "zeo_rt_pending_interrupts" => em.pending_id,
        "zeo_rt_gates" => em.gates_id,
        zeo_abi::abi::PATCHED_BITS_SYM => em.patched_bits_id,
        _ => return None,
    })
}


/// The classes the sidecar declares, with their ids -- assigned after the
/// last id the EMPTY program reaches, which is the one number a front end
/// cannot know and must not guess.
fn class_specs(sidecar: &Sidecar) -> CResult<(Vec<crate::clif::classes::ClassSpec>, ClassIds)> {
    let base = boot()?.first_user_class;
    let mut ids: ClassIds = HashMap::new();
    let mut minted = 0u32;
    for c in &sidecar.classes {
        let id = match reopened_builtin(c, sidecar)? {
            // The row REOPENS the builtin: it takes the builtin's own id,
            // so the program's `def`s register on the module the runtime
            // already carries, and it mints nothing.
            Some(id) => id.0,
            None => {
                minted += 1;
                base + minted - 1
            }
        };
        if ids.insert(c.name.clone(), id).is_some() {
            return Err(CodegenError::internal(format!(
                "the sidecar declares the class `{}` twice",
                c.name
            )));
        }
    }
    let mut specs = Vec::with_capacity(sidecar.classes.len());
    for c in &sidecar.classes {
        // A reopen needs no spec: the class exists, and `feature_rows`
        // already registers it with the ancestry the ABI declares.
        if reopened_builtin(c, sidecar)?.is_some() {
            continue;
        }
        let kind = match CLASS_KINDS.iter().position(|k| *k == c.kind) {
            Some(0) => zeo_abi::abi::CLASS_PLAIN,
            Some(_) => zeo_abi::abi::CLASS_MODULE,
            None => {
                return Err(CodegenError::internal(format!(
                    "`{}` is a `{}`; the kinds are {}",
                    c.name,
                    c.kind,
                    CLASS_KINDS.join(", ")
                )));
            }
        };
        let ancestors = if kind == zeo_abi::abi::CLASS_MODULE {
            vec![ids[&c.name]]
        } else {
            let mut chain = vec![ids[&c.name]];
            chain.extend(ancestors_of(&c.superclass, sidecar, &ids, &c.name, &mut vec![
                c.name.as_str(),
            ])?);
            chain
        };
        // A class whose chain reaches `Exception` needs the registrar that
        // installs the native `RubyException` and its typed accessors, so
        // the class's own `def`s layer over them as deltas. The sidecar
        // does not spell this: it follows from the superclass the front
        // end named, exactly as `is_exception_backed` derives it.
        let kind = match chain_is_exception(&ancestors) {
            true => zeo_abi::abi::CLASS_EXCEPTION,
            false => kind,
        };
        specs.push(crate::clif::classes::ClassSpec {
            id: ids[&c.name],
            name: c.name.clone(),
            ancestors,
            ivars: c.ivars.clone(),
            hidden: 0,
            members: Vec::new(),
            kind,
        });
    }
    Ok((specs, ids))
}

/// The builtin a class row REOPENS, by id, or `None` when the row mints a
/// class of its own.
///
/// A gated builtin is the runtime's half of a library whose other half is
/// Ruby -- `prism.so` defines the `Prism` module and its native entry
/// points, and `prism.rb` then writes the rest of the module in Ruby. A
/// program that requires the feature and carries that Ruby is reopening
/// what it just loaded, so the row resolves to the builtin's id and the
/// `def`s land where the native methods already are.
///
/// Everything else is refused. An always-on builtin (`String`, `Integer`)
/// has a native instance shape a class row cannot describe, and a gated
/// one the program never required is concealed -- the name is free, but a
/// row that took the builtin's id would reveal it by the back door.
fn reopened_builtin(
    c: &super::sidecar::Class,
    sidecar: &Sidecar,
) -> CResult<Option<zeo_abi::ClassId>> {
    let builtin = zeo_abi::BUILTINS.iter().find(|b| b.name == c.name);
    // An exception class is always on and carries no feature, so it never
    // reopens; naming one still has to be refused.
    if builtin.is_none() && !zeo_abi::EXCEPTION_CLASSES.iter().any(|b| b.name == c.name) {
        return Ok(None);
    }
    let required = builtin.is_some_and(|b| {
        b.feature.is_some_and(|f| {
            sidecar.features.iter().any(|r| zeo_abi::canonical_ext_feature(r) == f)
        })
    });
    let carried = builtin.is_some_and(|b| crate::lower::features::build_carries_class(b.id));
    if !required || !carried {
        return Err(CodegenError::internal(format!(
            "{}`{}` is a builtin: a class row defines a NEW class, so this would mint a shadow \
             beside it, and reopening a builtin has to be written as runtime definitions on it",
            written_at(c),
            c.name
        )));
    }
    // The builtin's instances are the runtime's, laid out where it put
    // them, so a reopen cannot add a slot to them.
    if !c.ivars.is_empty() {
        return Err(CodegenError::internal(format!(
            "{}the reopen of `{}` declares the instance variable `{}`; a builtin's layout is the \
             runtime's, so a reopen can add methods to it but no slots",
            written_at(c),
            c.name,
            c.ivars[0]
        )));
    }
    Ok(builtin.map(|b| b.id))
}

/// `file:line: ` for a class row that carries a location, empty otherwise.
fn written_at(c: &super::sidecar::Class) -> String {
    match c.file.is_empty() {
        true => String::new(),
        false => format!("{}:{}: ", c.file, c.line),
    }
}

/// Whether a linearized chain reaches `Exception`, which is what makes a
/// class exception-backed.
fn chain_is_exception(ancestors: &[u32]) -> bool {
    ancestors.contains(&zeo_abi::EXCEPTION_CLASS.0)
}

/// The builtin exception `name` names, by id. Every OTHER builtin stays
/// unavailable as a superclass: `class Foo < String` is a different native
/// shape, and the Rust emitter refuses one too.
fn builtin_exception(name: &str) -> Option<zeo_abi::ClassId> {
    zeo_abi::EXCEPTION_CLASSES
        .iter()
        .find(|e| e.name == name && !e.is_module)
        .map(|e| e.id)
}

/// The linearized chain of `name`, which is another sidecar class, a
/// builtin exception, or `Object`.
fn ancestors_of<'a>(
    name: &'a str,
    sidecar: &'a Sidecar,
    ids: &ClassIds,
    of: &str,
    seen: &mut Vec<&'a str>,
) -> CResult<Vec<u32>> {
    // A front end writes the superclass by name, so a chain that loops back
    // on itself is a sidecar the backend has to reject rather than recurse
    // into until the stack ends.
    if seen.contains(&name) {
        return Err(CodegenError::internal(format!(
            "the superclass chain of `{name}` reaches `{name}` again"
        )));
    }
    seen.push(name);
    if name == OBJECT_SUPERCLASS {
        return Ok(vec![
            zeo_abi::OBJECT_CLASS.0,
            zeo_abi::KERNEL_CLASS.0,
            zeo_abi::BASIC_OBJECT_CLASS.0,
        ]);
    }
    if name == BASIC_OBJECT_SUPERCLASS {
        return Ok(vec![zeo_abi::BASIC_OBJECT_CLASS.0]);
    }
    // A builtin exception's chain is the ABI's own, so the gates that pick
    // each native default method decide the same way here as they do for
    // the built-in tree. Its ids are constants, not this program's.
    if !ids.contains_key(name) {
        if let Some(id) = builtin_exception(name) {
            return Ok(zeo_abi::declared_ancestors(id).iter().map(|c| c.0).collect());
        }
    }
    let Some(&id) = ids.get(name) else {
        return Err(CodegenError::internal(format!(
            "`{of}` names the superclass `{name}`, which is not a class in this sidecar, a \
             builtin exception, `{OBJECT_SUPERCLASS}` or `{BASIC_OBJECT_SUPERCLASS}` (every \
             other builtin superclass is a different native shape)"
        )));
    };
    let parent = sidecar
        .classes
        .iter()
        .find(|c| c.name == name)
        .expect("the id map and the list agree");
    if parent.kind != CLASS_KINDS[0] {
        return Err(CodegenError::internal(format!(
            "`{of}` inherits from the module `{name}`"
        )));
    }
    let mut chain = vec![id];
    chain.extend(ancestors_of(&parent.superclass, sidecar, ids, name, seen)?);
    Ok(chain)
}

/// Every call site's caller class, as an id. This is the class ruby's
/// visibility barrier compares an explicit receiver's method against, and
/// the ids are the ones `class_specs` just assigned.
fn caller_classes(sites: &[CallerClass], ids: &ClassIds) -> CResult<Vec<u32>> {
    sites
        .iter()
        .map(|site| match site {
            CallerClass::Id(id) => Ok(*id),
            // A receiverless call asks no visibility question at all --
            // ruby's `VM_CALL_FCALL`, which the runtime spells the same way.
            CallerClass::Named(name) if name.is_empty() => Ok(u32::MAX),
            CallerClass::Named(name) => named_caller(name, ids),
        })
        .collect()
}

/// A caller class by name: one this program declares, or one of the three
/// the language itself supplies. `Class` and `Module` are what a body whose
/// `self` IS a class compares against, because a class object is no kind of
/// the class whose instances a protected method belongs to.
fn named_caller(name: &str, ids: &ClassIds) -> CResult<u32> {
    if let Some(&id) = ids.get(name) {
        return Ok(id);
    }
    match name {
        OBJECT_SUPERCLASS => Ok(zeo_abi::OBJECT_CLASS.0),
        "Class" => Ok(zeo_abi::CLASS_CLASS.0),
        "Module" => Ok(zeo_abi::MODULE_CLASS.0),
        _ => Err(CodegenError::internal(format!(
            "a call site names `{name}` as its caller class, which is not a class in this sidecar,              `Object`, `Class` or `Module`"
        ))),
    }
}

/// The `zeo_class_ids` array: one `u32` per sidecar class, in the order the
/// sidecar lists them. Ids are assigned at LINK time, so a front end that
/// needs one in its code -- a constant's cref, an assignment's owner --
/// reads it out of here the way it reads a symbol out of `zeo_syms`.
///
/// The rows and not the specs are what this counts: a row that reopens a
/// builtin has the builtin's id and no spec of its own, and a front end
/// still indexes it by the position it wrote it in.
fn define_class_ids(em: &mut Emitter, sidecar: &Sidecar, ids: &ClassIds) -> CResult<DataId> {
    let mut bytes = Vec::with_capacity(sidecar.classes.len().max(1) * 4);
    for c in &sidecar.classes {
        bytes.extend_from_slice(&ids[&c.name].to_le_bytes());
    }
    if bytes.is_empty() {
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    let id = em
        .module
        .declare_data(CLASS_IDS, Linkage::Local, false, false)
        .map_err(|e| CodegenError::internal(format!("declaring {CLASS_IDS}: {e}")))?;
    let mut data = cranelift_module::DataDescription::new();
    data.define(bytes.into_boxed_slice());
    data.set_align(4);
    em.module
        .define_data(id, &data)
        .map_err(|e| CodegenError::internal(format!("defining {CLASS_IDS}: {e}")))?;
    Ok(id)
}

/// The data symbol [`define_class_ids`] writes.
pub const CLASS_IDS: &str = "zeo_class_ids";

type ClassIds = HashMap<String, u32>;

/// The dispatch, visibility and reflection rows of the sidecar's `def`s.
fn def_rows(
    sidecar: &Sidecar,
    in_file: &HashMap<String, FuncId>,
    class_ids: &ClassIds,
) -> CResult<DefRows> {
    let mut vm = Vec::new();
    let mut vis = Vec::new();
    let mut obj = Vec::new();
    let mut cm = Vec::new();
    let mut meta = Vec::new();
    for def in &sidecar.defs {
        // Which channel a method rides is not a choice the sidecar makes:
        // a top-level `def` and a module's method ride the VALUE channel,
        // a compiled class's instance method the OBJECT channel, and
        // `def self.x` the class-method one.
        let (class, is_module) = match def.class.as_str() {
            "" => (0, false),
            name => {
                let Some(&id) = class_ids.get(name) else {
                    return Err(CodegenError::internal(format!(
                        "`{}` is defined on `{name}`, which the sidecar does not declare",
                        def.name
                    )));
                };
                let kind = &sidecar
                    .classes
                    .iter()
                    .find(|c| c.name == name)
                    .expect("the id map and the list agree")
                    .kind;
                (id, kind == CLASS_KINDS[1])
            }
        };
        let f = *in_file.get(&def.tramp).ok_or_else(|| {
            CodegenError::internal(format!(
                "the sidecar's `{}` names the trampoline `{}`, which the text does not define",
                def.name, def.tramp
            ))
        })?;
        match (def.singleton, class, is_module) {
            (true, _, _) => cm.push(statics::CmRowSpec {
                class,
                box_id: 0,
                name: def.name.clone(),
                f,
            }),
            (false, 0, _) | (false, _, true) => vm.push(statics::VmRowSpec {
                class,
                box_id: 0,
                name: def.name.clone(),
                f,
            }),
            (false, _, false) => obj.push(statics::ObjRowSpec {
                class,
                name: def.name.clone(),
                f,
            }),
        }
        let verb = match def.visibility.as_str() {
            "public" => None,
            "private" => Some(0),
            "protected" => Some(1),
            other => {
                return Err(CodegenError::internal(format!(
                    "`{}` has the visibility `{other}`; public, private or protected",
                    def.name
                )));
            }
        };
        if let Some(verb) = verb {
            vis.push(statics::VisRowSpec {
                class,
                name: def.name.clone(),
                verb,
            });
        }
        let params = def
            .params
            .iter()
            .map(|(kind, name)| {
                let kind = PARAM_KINDS.iter().position(|k| k == kind).ok_or_else(|| {
                    CodegenError::internal(format!(
                        "`{}` has a `{kind}` parameter; the kinds are {}",
                        def.name,
                        PARAM_KINDS.join(", ")
                    ))
                })?;
                Ok((kind as u8, name.clone()))
            })
            .collect::<CResult<Vec<(u8, String)>>>()?;
        meta.push(statics::MetaRowSpec {
            class,
            singleton: def.singleton,
            name: def.name.clone(),
            params,
            file: def.file.clone(),
            line: def.line,
            aliased_from: def.aliased_from.clone(),
        });
    }
    Ok(DefRows {
        vm,
        vis,
        obj,
        cm,
        meta,
    })
}

/// What the sidecar's `def`s become, one list per channel they ride.
struct DefRows {
    vm: Vec<statics::VmRowSpec>,
    vis: Vec<statics::VisRowSpec>,
    obj: Vec<statics::ObjRowSpec>,
    cm: Vec<statics::CmRowSpec>,
    meta: Vec<statics::MetaRowSpec>,
}

/// What each class's own body wrote, which is `instance_methods(false)`
/// and `Method#owner` truth. The front end never says this: it follows
/// from which class each `def` names, so the backend derives it rather
/// than asking for a row whose `class` would have to be an id.
/// Every inherited class method, copied onto the subclass that inherits
/// it.
///
/// The runtime's class-method table walks no ancestry: a hit there is
/// always that exact class's own entry, so the flattening is the front
/// end's job (`analyze::mro::materialize_class_methods` is where the Rust
/// one does it). Without the copies `class Kid < Base` answered
/// NoMethodError for `Base`'s `def self.tag`.
///
/// A nearer definer shadows a farther one, and the copy carries the
/// SUBCLASS's id, which is what keeps a class-level `@x` on the class
/// that reads it.
fn inherited_class_methods(
    sidecar: &Sidecar,
    class_ids: &ClassIds,
    in_file: &HashMap<String, FuncId>,
) -> CResult<Vec<statics::CmRowSpec>> {
    let mut supers: HashMap<&str, &str> = HashMap::new();
    let mut own: HashMap<&str, Vec<&super::sidecar::Def>> = HashMap::new();
    for c in &sidecar.classes {
        supers.insert(c.name.as_str(), c.superclass.as_str());
    }
    for def in sidecar.defs.iter().filter(|d| d.singleton && !d.class.is_empty()) {
        own.entry(def.class.as_str()).or_default().push(def);
    }
    let mut rows = Vec::new();
    for c in &sidecar.classes {
        // The chain, nearest first. `class_specs` has already refused one
        // that loops, so this walk ends; the guard is belt and braces.
        let mut chain = vec![c.name.as_str()];
        while let Some(&up) = supers.get(chain[chain.len() - 1]) {
            if chain.contains(&up) {
                break;
            }
            chain.push(up);
        }
        for (at, ancestor) in chain.iter().enumerate().skip(1) {
            for def in own.get(ancestor).into_iter().flatten() {
                let shadowed = chain[..at].iter().any(|nearer| {
                    own.get(nearer)
                        .is_some_and(|ds| ds.iter().any(|d| d.name == def.name))
                });
                if shadowed {
                    continue;
                }
                let f = *in_file.get(&def.tramp).ok_or_else(|| {
                    CodegenError::internal(format!(
                        "the sidecar's `{}` names the trampoline `{}`, which the text does not \
                         define",
                        def.name, def.tramp
                    ))
                })?;
                rows.push(statics::CmRowSpec {
                    class: class_ids[&c.name],
                    box_id: 0,
                    name: def.name.clone(),
                    f,
                });
            }
        }
    }
    Ok(rows)
}

fn own_method_rows(sidecar: &Sidecar, class_ids: &ClassIds) -> Vec<statics::RegRowSpec> {
    let mut rows: Vec<(u32, u8, String)> = sidecar
        .defs
        .iter()
        .filter(|d| !d.class.is_empty())
        .map(|d| {
            let kind = if d.singleton {
                zeo_abi::abi::REG_MARK_OWN_CLASS_METHOD_ROWS
            } else {
                zeo_abi::abi::REG_MARK_OWN_ROWS
            };
            (class_ids[&d.class], kind, d.name.clone())
        })
        .collect();
    rows.sort();
    rows.into_iter()
        .map(|(class, kind, a)| statics::RegRowSpec {
            kind,
            class,
            a,
            b: String::new(),
            f: None,
            ids: Vec::new(),
            flag: 0,
        })
        .collect()
}

/// The registration a require-gated builtin feature needs: its classes
/// register per program, because `register_builtins` covers only the
/// always-on ones, and each starts CONCEALED -- the constant does not
/// exist until the `require` runs, which is a position in the program
/// rather than a whole-program fact.
///
/// A front end names the feature and never an id, so this is the only
/// side that can write the rows.
fn feature_rows(features: &[String]) -> CResult<Vec<statics::RegRowSpec>> {
    let mut rows = Vec::new();
    let named: Vec<String> = match features.iter().any(|f| f == ALL_FEATURES) {
        true => zeo_abi::BUILTINS
            .iter()
            .filter_map(|b| b.feature)
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        false => features.to_vec(),
    };
    for feature in &named {
        let canonical = zeo_abi::canonical_ext_feature(feature);
        if !zeo_abi::is_builtin_feature(canonical) {
            return Err(CodegenError::internal(format!(
                "the sidecar requires `{feature}`, which this zeo does not carry as a builtin \
                 (a front end can only require what the runtime already has)"
            )));
        }
        let gated: Vec<_> = zeo_abi::BUILTINS
            .iter()
            .filter(|b| b.feature == Some(canonical))
            .filter(|b| crate::lower::features::build_carries_class(b.id))
            .collect();
        if gated.is_empty() {
            return Err(CodegenError::internal(format!(
                "this build does not carry `{feature}`"
            )));
        }
        for b in gated {
            let ancestors = zeo_abi::declared_ancestors(b.id).iter().map(|c| c.0).collect();
            rows.push(statics::RegRowSpec {
                kind: zeo_abi::abi::REG_REGISTER_BUILTIN,
                class: b.id.0,
                a: b.name.to_string(),
                b: String::new(),
                f: None,
                ids: ancestors,
                flag: u8::from(b.is_module),
            });
            rows.push(statics::RegRowSpec {
                kind: zeo_abi::abi::REG_CONCEAL_CLASS,
                class: b.id.0,
                a: String::new(),
                b: String::new(),
                f: None,
                ids: Vec::new(),
                flag: 0,
            });
        }
    }
    Ok(rows)
}

/// A class's own contribution to a `super` walk, one row per instance
/// method it writes. A CLIF trampoline is a receiver-generic `ValueFn`, so
/// the ordinary trampoline serves as the target -- which is why the
/// sidecar needs no field for this and the backend derives the whole
/// ladder, ids included, from the `def`s.
///
/// Every own method gets a row, whether or not anything calls `super`
/// through it: the walk needs the ladder, not the rungs someone stands on.
fn super_target_rows(
    sidecar: &Sidecar,
    in_file: &HashMap<String, FuncId>,
    class_ids: &ClassIds,
) -> CResult<Vec<statics::RegRowSpec>> {
    let mut rows = Vec::new();
    for def in &sidecar.defs {
        if def.singleton || def.class.is_empty() {
            continue;
        }
        // A module's methods ride the VALUE channel, whose `super` walk
        // resolves by name rather than through a per-class row.
        let is_module = sidecar
            .classes
            .iter()
            .find(|c| c.name == def.class)
            .is_some_and(|c| c.kind == CLASS_KINDS[1]);
        if is_module {
            continue;
        }
        let f = *in_file.get(&def.tramp).ok_or_else(|| {
            CodegenError::internal(format!(
                "the sidecar's `{}` names the trampoline `{}`, which the text does not define",
                def.name, def.tramp
            ))
        })?;
        rows.push(statics::RegRowSpec {
            kind: zeo_abi::abi::REG_SUPER_TARGET_VALUE,
            class: class_ids[&def.class],
            a: def.name.clone(),
            b: String::new(),
            f: Some(f),
            ids: Vec::new(),
            flag: 0,
        });
    }
    Ok(rows)
}

/// The registration rows the sidecar names, `@boot` expanded to the rows
/// every program carries.
fn reg_rows(
    entries: &[RegEntry],
    in_file: &HashMap<String, FuncId>,
) -> CResult<Vec<statics::RegRowSpec>> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            RegEntry::Boot(word) if word == BOOT_ROWS => {
                out.extend(boot_rows()?.iter().map(|r| statics::RegRowSpec {
                    kind: r.kind,
                    class: r.class,
                    a: r.a.clone(),
                    b: r.b.clone(),
                    f: None,
                    ids: r.ids.clone(),
                    flag: r.flag,
                }));
            }
            RegEntry::Boot(word) => {
                return Err(CodegenError::internal(format!(
                    "`{word}` is not a registration row; the one word is `{BOOT_ROWS}`"
                )));
            }
            RegEntry::Row(r) => {
                let f = match &r.f {
                    None => None,
                    Some(symbol) => Some(*in_file.get(symbol).ok_or_else(|| {
                        CodegenError::internal(format!(
                            "a registration row names `{symbol}`, which the text does not define"
                        ))
                    })?),
                };
                out.push(statics::RegRowSpec {
                    kind: r.kind,
                    class: r.class,
                    a: r.a.clone(),
                    b: r.b.clone(),
                    f,
                    ids: r.ids.clone(),
                    flag: r.flag,
                });
            }
        }
    }
    Ok(out)
}

/// What the EMPTY program carries: the class tables and registration rows
/// the boot prelude gives every program before a statement of its own.
/// Read off a compile of ``, once, so the set is this zeo's and never a
/// copy that drifts.
fn boot() -> CResult<&'static Sidecar> {
    static BOOT: std::sync::OnceLock<Result<Sidecar, String>> = std::sync::OnceLock::new();
    BOOT.get_or_init(|| {
        crate::compile_to_clif_text("", &crate::CompileOptions::default())
            .map_err(String::from)?
            .sidecar
    })
    .as_ref()
    .map_err(|e| CodegenError::internal(format!("the empty program: {e}")))
}

fn boot_rows() -> CResult<Vec<RegRow>> {
    boot()?
        .reg
        .iter()
        .map(|entry| match entry {
            RegEntry::Row(row) if row.f.is_none() => Ok(row.clone()),
            other => Err(CodegenError::internal(format!(
                "the empty program's rows are not all plain: {other:?}"
            ))),
        })
        .collect()
}

/// The builtin class tables the sidecar names, `@seed` and `@all`
/// expanded, each one a table this build carries.
fn class_tables(named: &[String]) -> CResult<Vec<&'static str>> {
    let mut out = Vec::new();
    for name in named {
        if name == SEED_TABLES {
            out.extend(class_tables(&boot()?.class_tables)?);
            continue;
        }
        if name == ALL_TABLES {
            out.extend(
                crate::builtin_surface::CLASS_TABLE_SYMBOLS
                    .iter()
                    .filter(|(id, _)| crate::lower::features::build_carries_class(*id))
                    .map(|(_, sym)| *sym),
            );
            continue;
        }
        let known = crate::builtin_surface::CLASS_TABLE_SYMBOLS
            .iter()
            .find(|(id, sym)| sym == name && crate::lower::features::build_carries_class(*id))
            .map(|(_, sym)| *sym)
            .ok_or_else(|| {
                CodegenError::internal(format!("`{name}` is not a class table this build carries"))
            })?;
        out.push(known);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest program: a top level that answers nil and returns 0.
    /// One call into the runtime, one data symbol.
    const NIL_TOPLEVEL: &str = r#"
function %zeo_toplevel(i64) -> i32 apple_aarch64 {
    gv0 = symbol colocated %zeo_rodata
    sig0 = (i64) -> i64 apple_aarch64
    fn0 = %zeo_rt_array_len sig0

block0(v0: i64):
    v1 = symbol_value.i64 gv0
    v2 = iconst.i64 0
    store notrap aligned v2, v0
    store notrap aligned v2, v0+8
    store notrap aligned v2, v0+16
    v3 = iconst.i32 0
    return v3
}
"#;

    #[test]
    fn a_program_with_no_sidecar_compiles_to_an_object() {
        let object = compile(NIL_TOPLEVEL, &Sidecar::default()).expect("compiles");
        assert!(!object.is_empty());
    }

    #[test]
    fn a_callee_that_is_nowhere_is_named() {
        let text = NIL_TOPLEVEL.replace("zeo_rt_array_len", "zeo_rt_no_such_row");
        let err = compile(&text, &Sidecar::default()).unwrap_err().to_string();
        assert!(err.contains("`zeo_rt_no_such_row`"), "{err}");
        assert!(err.contains("neither a function in this file"), "{err}");
    }

    #[test]
    fn a_runtime_entry_point_called_with_the_wrong_shape_is_refused() {
        let text = NIL_TOPLEVEL.replace("sig0 = (i64) -> i64", "sig0 = (i64, i64) -> i64");
        let err = compile(&text, &Sidecar::default()).unwrap_err().to_string();
        assert!(err.contains("`zeo_rt_array_len`"), "{err}");
        assert!(err.contains("the runtime's shape"), "{err}");
    }

    #[test]
    fn a_data_symbol_no_program_carries_is_refused() {
        let text = NIL_TOPLEVEL.replace("%zeo_rodata", "%zeo_nothing");
        let err = compile(&text, &Sidecar::default()).unwrap_err().to_string();
        assert!(err.contains("`zeo_nothing`"), "{err}");
    }

    #[test]
    fn the_top_level_must_be_in_the_text() {
        let sidecar = Sidecar {
            toplevel: "zeo_elsewhere".to_string(),
            ..Sidecar::default()
        };
        let err = compile(NIL_TOPLEVEL, &sidecar).unwrap_err().to_string();
        assert!(err.contains("`zeo_elsewhere`"), "{err}");
    }

    #[test]
    fn the_seed_tables_are_the_empty_programs() {
        let seed = class_tables(&[SEED_TABLES.to_string()]).unwrap();
        assert!(seed.contains(&"zeo_ctable_STRING_CLASS"), "{seed:?}");
        assert!(seed.contains(&"zeo_ctable_MONITOR_CLASS"), "{seed:?}");
        assert!(class_tables(&["zeo_ctable_Nowhere".to_string()]).is_err());
        let all = class_tables(&[ALL_TABLES.to_string()]).unwrap();
        assert!(all.len() > seed.len(), "{} vs {}", all.len(), seed.len());
        assert!(seed.iter().all(|t| all.contains(t)));
    }

    #[test]
    fn the_boot_rows_are_the_empty_programs() {
        let rows = boot_rows().unwrap();
        assert!(
            rows.iter()
                .any(|r| r.kind == zeo_abi::abi::REG_REGISTER_BUILTIN && r.a == "Monitor"),
            "{rows:?}"
        );
    }
}
