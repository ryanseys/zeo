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

use super::sidecar::{ALL_TABLES, BOOT_ROWS, PARAM_KINDS, RegEntry, RegRow, SEED_TABLES, Sidecar};
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
    em.callsites = sidecar.callsites.clone();

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
    for (name, id, mut func) in declared {
        resolve_names(&mut em, &ids, &name, &mut func)?;
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
    let (vm_rows, vis_rows, meta_rows) = def_rows(sidecar, &ids)?;
    let reg_rows = reg_rows(&sidecar.reg, &ids)?;
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
                vm: &vm_rows,
                vis: &vis_rows,
                classes: &[],
                obj: &[],
                cm: &[],
                reg: &reg_rows,
                foreign: &[],
                meta: &meta_rows,
                redef_metas: &[],
                unit: &[],
            },
        },
    )?;
    emit::define_main(&mut em, desc)?;
    statics::define_rodata(&mut em)?;
    emit::finish_object(em)
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
        let data = data_symbol(em, &symbol).ok_or_else(|| {
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
fn data_symbol(em: &Emitter, symbol: &str) -> Option<DataId> {
    Some(match symbol {
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

/// The dispatch, visibility and reflection rows of the sidecar's `def`s.
fn def_rows(
    sidecar: &Sidecar,
    in_file: &HashMap<String, FuncId>,
) -> CResult<(
    Vec<statics::VmRowSpec>,
    Vec<statics::VisRowSpec>,
    Vec<statics::MetaRowSpec>,
)> {
    let mut vm = Vec::new();
    let mut vis = Vec::new();
    let mut meta = Vec::new();
    for def in &sidecar.defs {
        let f = *in_file.get(&def.tramp).ok_or_else(|| {
            CodegenError::internal(format!(
                "the sidecar's `{}` names the trampoline `{}`, which the text does not define",
                def.name, def.tramp
            ))
        })?;
        vm.push(statics::VmRowSpec {
            class: 0,
            box_id: 0,
            name: def.name.clone(),
            f,
        });
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
                class: 0,
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
            class: 0,
            singleton: false,
            name: def.name.clone(),
            params,
            file: def.file.clone(),
            line: def.line,
            aliased_from: def.aliased_from.clone(),
        });
    }
    Ok((vm, vis, meta))
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
