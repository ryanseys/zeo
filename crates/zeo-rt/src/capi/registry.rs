//! `register_program`: walk a [`ProgramDesc`]'s tables in today's generated
//! `main` order, calling exactly the registrars the rustc backend calls.
//! Table strings are the program's `.rodata` (the boundary contract), so
//! they borrow as `&'static str`; the few structures the runtime keeps by
//! `'static` reference (`ClassLayout`, the meta-row table) are leaked once
//! here, at registration.

use crate::compiled_object::{self, ClassLayout, CompiledObject};
use crate::dispatch::ClassRegistry;
use crate::method_meta::{MetaRow, ParamKind};
use crate::{RObj, RubyValue, Signal, Symbol};
use zeo_abi::ClassId;
use zeo_abi::abi::{self, ProgramDesc};

/// A table view; `len == 0` tolerates a null base (an empty table).
unsafe fn rows<'a, T>(ptr: *const T, n: usize) -> &'a [T] {
    if n == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr, n) }
}

fn text(s: abi::Str) -> &'static str {
    unsafe { super::static_str(s.ptr, s.len) }
}

/// An `abi::ValueFn` as the runtime's own [`super::ValueFn`] -- identical
/// ABI (the `abi_layout` test asserts `abi::Value` == `RubyValue` in size
/// and alignment); only the nominal parameter types differ.
fn value_fn(f: abi::ValueFn) -> super::ValueFn {
    unsafe { std::mem::transmute::<abi::ValueFn, super::ValueFn>(f) }
}

/// The shared `AllocatorFn` for every compiled class -- `Class#allocate`'s
/// storage half, identical to `zeo_rt_object_alloc`'s.
fn compiled_allocate(id: ClassId) -> RObj {
    let layout = compiled_object::alloc_layout_of(id)
        .unwrap_or_else(|| panic!("compiled_allocate: no layout registered for class {}", id.0));
    CompiledObject::alloc(id, layout)
}

/// The shared `ConstructorFn`: allocate, then `initialize` -- what
/// `ruby_class!`'s per-class `__construct` does, made generic by the
/// `LAYOUTS` table.
fn compiled_construct(
    id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let obj = compiled_allocate(id);
    crate::dispatch::run_initialize(id, &obj, args, block)?;
    Ok(RubyValue::Object(obj))
}

fn param_kind(kind: u8) -> ParamKind {
    match kind {
        abi::PARAM_REQ => ParamKind::Req,
        abi::PARAM_OPT => ParamKind::Opt,
        abi::PARAM_REST => ParamKind::Rest,
        abi::PARAM_KEYREQ => ParamKind::KeyReq,
        abi::PARAM_KEY => ParamKind::Key,
        abi::PARAM_KEYREST => ParamKind::KeyRest,
        abi::PARAM_BLOCK => ParamKind::Block,
        k => panic!("register_program: unknown ParamC kind {k}"),
    }
}

/// The registration half of the generated `main` sequence, step for step:
/// classes, method rows, visibility, the registry install, meta rows, core
/// constants, the loaded-features/load-path seeds, declined features,
/// coverage, parse warnings, and the `__END__` data section.
pub(crate) unsafe fn register_program(desc: &ProgramDesc) {
    assert_eq!(
        desc.abi_version,
        abi::ABI_VERSION,
        "register_program: program ABI version {} != runtime {}",
        desc.abi_version,
        abi::ABI_VERSION,
    );
    // The builtin class tables this program named. Installed BEFORE anything
    // dispatches: `registered_table` memoizes its id map on first use, and a
    // lookup that ran first would cache an empty one -- which is a class that
    // silently loses every method, seen as `uninitialized constant
    // File::RDWR` in every program that required anything.
    if desc.n_class_tables > 0 {
        let raw = unsafe { std::slice::from_raw_parts(desc.class_tables, desc.n_class_tables) };
        let tables: Vec<&'static crate::builtins::BuiltinClassTable> = raw
            .iter()
            .map(|p| unsafe { &*p.cast::<crate::builtins::BuiltinClassTable>() })
            .collect();
        crate::builtins::install_program_tables(Vec::leak(tables));
    }
    let mut registry = ClassRegistry::with_core();
    for c in unsafe { rows(desc.classes, desc.n_classes) } {
        let id = ClassId(c.id);
        let name = text(c.name);
        // A compiled `Struct`/`Data`: its member list, so `Struct`'s one
        // shared protocol (`to_a`/`[]`/`==`/`each`/`dig`/`inspect`/Marshal)
        // finds it by MRO and reaches the members by index, exactly as it
        // does for a runtime-minted one.
        if c.n_members > 0 {
            let members: Vec<&str> = unsafe { rows(c.members, c.n_members) }
                .iter()
                .map(|&m| text(m))
                .collect();
            crate::register_compiled_struct(id, &members, false, None);
        }
        let ancestors: Vec<ClassId> = unsafe { rows(c.ancestors, c.n_ancestors) }
            .iter()
            .map(|&i| ClassId(i))
            .collect();
        match c.kind {
            abi::CLASS_PLAIN => {
                registry.register(id, name, false, ancestors, Some(compiled_construct));
                registry.define_allocator(id, compiled_allocate);
                let names: &'static [&'static str] = Vec::leak(
                    unsafe { rows(c.ivar_names, c.n_ivars) }
                        .iter()
                        .map(|&s| text(s))
                        .collect(),
                );
                let layout = Box::leak(Box::new(ClassLayout {
                    names,
                    hidden: usize::from(c.hidden),
                }));
                compiled_object::register_layout(id, layout);
            }
            abi::CLASS_MODULE => registry.register(id, name, true, ancestors, None),
            // The native-backed shapes: instances are the runtime's own
            // types, methods arrive as deltas over the installed defaults.
            abi::CLASS_EXCEPTION => {
                crate::register_exception_subclass(&mut registry, id, name, ancestors)
            }
            abi::CLASS_VALUE_SUBCLASS => {
                crate::register_value_subclass(&mut registry, id, name, ancestors)
            }
            abi::CLASS_RECV_HONOURING => {
                crate::register_recv_honouring_subclass(&mut registry, id, name, ancestors)
            }
            abi::CLASS_MODULE_SUBCLASS => {
                crate::register_module_subclass(&mut registry, id, name, ancestors)
            }
            abi::CLASS_WEAK_MAP => {
                crate::register_weakmap_subclass(&mut registry, id, name, ancestors)
            }
            // An instance-less builtin's subclass: name + ancestors only, no
            // constructor -- `.new` resolves dynamically to the raise.
            abi::CLASS_IMMEDIATE => registry.register(id, name, false, ancestors, None),
            k => panic!("register_program: unknown class kind {k} ({name})"),
        }
    }
    // The two REGISTRY-SHAPE rows run BEFORE any method row, exactly where
    // rustc's `main` puts its builtin registrations: a row cannot be defined
    // on a class the registry has not seen, and a chain patch must be in
    // place before anything walks it.
    // The builtin class tables this program named. Installed BEFORE anything
    // dispatches: `registered_table` memoizes its id map on first use, and a
    // lookup that ran first would cache an empty one.
    if desc.n_class_tables > 0 {
        let raw = unsafe { std::slice::from_raw_parts(desc.class_tables, desc.n_class_tables) };
        let tables: Vec<&'static crate::builtins::BuiltinClassTable> = raw
            .iter()
            .map(|p| unsafe { &*p.cast::<crate::builtins::BuiltinClassTable>() })
            .collect();
        crate::builtins::install_program_tables(Vec::leak(tables));
    }

    // A boot redef's reflection row cannot be installed here: the meta table
    // it belongs to is registered below, and would overwrite it with the LAST
    // body's row. Collected and applied after.
    let mut boot_metas: Vec<u32> = Vec::new();
    for r in unsafe { rows(desc.reg_rows, desc.n_reg_rows) } {
        let ids = || {
            unsafe { rows(r.ids, r.n_ids) }
                .iter()
                .map(|&i| ClassId(i))
                .collect::<Vec<_>>()
        };
        match r.kind {
            abi::REG_REGISTER_BUILTIN => {
                registry.register(ClassId(r.class), text(r.a), r.flag != 0, ids(), None);
            }
            abi::REG_SET_ANCESTORS => registry.set_ancestors(ClassId(r.class), ids()),
            _ => {}
        }
    }
    for r in unsafe { rows(desc.obj_rows, desc.n_obj_rows) } {
        registry.define_method_c(
            ClassId(r.class),
            Symbol::intern(text(r.name)),
            value_fn(r.f),
        );
    }
    for r in unsafe { rows(desc.vm_rows, desc.n_vm_rows) } {
        assert!(
            r.flags == 0,
            "register_program: VmRow flags ({}) are not yet emitted (G8)",
            r.flags
        );
        registry.define_value_method_c(
            ClassId(r.class),
            r.box_id,
            Symbol::intern(text(r.name)),
            value_fn(r.f),
        );
    }
    let foreign: Vec<(u32, &str)> = unsafe { rows(desc.vm_foreign, desc.n_vm_foreign) }
        .iter()
        .map(|r| (r.class, text(r.name)))
        .collect();
    registry.mark_foreign_value_rows(&foreign);
    for r in unsafe { rows(desc.cm_rows, desc.n_cm_rows) } {
        registry.define_class_method_c(
            ClassId(r.class),
            Symbol::intern(text(r.name)),
            value_fn(r.f),
        );
    }
    for r in unsafe { rows(desc.reg_rows, desc.n_reg_rows) } {
        match r.kind {
            abi::REG_MARK_OWN_CLASS_METHOD_ROWS => {
                registry.mark_own_class_method_rows(ClassId(r.class), &[text(r.a)]);
            }
            abi::REG_SUPER_TARGET_VALUE => registry.define_super_target_value_c(
                ClassId(r.class),
                Symbol::intern(text(r.a)),
                value_fn(r.f.expect("a super-target row carries its fn")),
            ),
            abi::REG_CONCEAL_CLASS => crate::constants::conceal_class(r.class),
            // Both handled in the pre-pass above.
            abi::REG_REGISTER_BUILTIN | abi::REG_SET_ANCESTORS => {}
            abi::REG_MARK_REFINEMENT => {
                let module = unsafe { *r.ids };
                let target = unsafe { *r.ids.add(1) };
                registry.mark_refinement(ClassId(r.class), ClassId(module), ClassId(target));
            }
            abi::REG_MARK_BOX_CLASS => {
                crate::boxes::mark_box_class(ClassId(r.class), unsafe { *r.ids });
            }
            abi::REG_MARK_GLOBAL_DEF_HOOK => {
                crate::runtime_meta::mark_global_def_hook(text(r.a));
            }
            abi::REG_CONST_PRIVATE => {
                crate::constants::const_set_private(r.class, &[text(r.a)], true);
            }
            // `flag` is the CHANNEL: a `def self.x`'s first body installs on
            // the class-method side, whose overlay row is a different map.
            abi::REG_BOOT_REDEF => {
                let id = ClassId(r.class);
                let name = Symbol::intern(text(r.a));
                let f = value_fn(r.f.expect("a boot-redef row carries its fn"));
                // Bit 0 is the channel; bits 1-2 are this body's own
                // visibility. A `private :v` written between two bodies
                // retagged THIS `def` at lower time, so the first body's mark
                // has to be in place before the first statement runs.
                let singleton = r.flag & 1 != 0;
                match singleton {
                    false => crate::runtime_meta::runtime_replace_method_c(id, name, f),
                    true => crate::runtime_meta::runtime_replace_class_method_c(id, name, f),
                }
                let vis = match (r.flag >> 1) & 3 {
                    0 => crate::dispatch::MethodVisibility::Private,
                    1 => crate::dispatch::MethodVisibility::Protected,
                    _ => crate::dispatch::MethodVisibility::Public,
                };
                if vis != crate::dispatch::MethodVisibility::Public {
                    crate::runtime_meta::install_positional_visibility(id, name, vis, singleton);
                }
                if r.n_ids > 0 {
                    boot_metas.push(unsafe { *r.ids });
                }
            }
            abi::REG_SINGLETON_SURROGATE => {
                let owner = unsafe { *r.ids };
                crate::runtime_meta::register_singleton_surrogate(ClassId(owner), ClassId(r.class));
            }
            abi::REG_MARK_UNDEFINED => {
                registry.mark_undefined(ClassId(r.class), Symbol::intern(text(r.a)));
            }
            abi::REG_ALIAS => {
                registry.register_alias(ClassId(r.class), text(r.a), text(r.b));
            }
            abi::REG_CLASS_ALIAS => {
                registry.register_class_alias(ClassId(r.class), text(r.a), text(r.b));
            }
            abi::REG_SINGLETON_SUPER_TARGET => {
                let module = unsafe { *r.ids };
                registry.define_singleton_super_target_c(
                    ClassId(r.class),
                    ClassId(module),
                    Symbol::intern(text(r.a)),
                    value_fn(r.f.expect("a singleton-super-target row carries its fn")),
                );
            }
            abi::REG_EXTENDS => {
                let mods = if r.n_ids == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(r.ids, r.n_ids) }
                        .iter()
                        .map(|&i| ClassId(i))
                        .collect()
                };
                registry.register_extends(ClassId(r.class), mods);
            }
            abi::REG_MARK_OWN_ROWS => {
                registry.mark_own_rows(ClassId(r.class), &[text(r.a)]);
            }
            abi::REG_ACCESSOR_SLOT => {
                registry.register_accessor_slot(
                    ClassId(r.class),
                    Symbol::intern(text(r.a)),
                    unsafe { *r.ids },
                    r.flag == 1,
                );
            }
            k => panic!("register_program: unknown RegRow kind {k}"),
        }
    }
    let vis: Vec<(u32, &str, u8)> = unsafe { rows(desc.vis_rows, desc.n_vis_rows) }
        .iter()
        .map(|r| (r.class, text(r.name), r.verb))
        .collect();
    registry.mark_visibility_rows(&vis);
    crate::dispatch::install_class_registry(registry);

    let meta_row = |m: &abi::MetaRowC| {
        let mut row = if m.singleton != 0 {
            MetaRow::sing(m.class, text(m.name))
        } else {
            MetaRow::inst(m.class, text(m.name))
        };
        if m.n_params > 0 {
            let params: &'static [(ParamKind, Option<&'static str>)] = Vec::leak(
                unsafe { rows(m.params, m.n_params) }
                    .iter()
                    .map(|p| {
                        let name = (p.name.len > 0).then(|| text(p.name));
                        (param_kind(p.kind), name)
                    })
                    .collect(),
            );
            row = row.params(params);
        }
        if m.file.len > 0 {
            row = row.at(text(m.file), m.line);
        }
        if m.aliased_from.len > 0 {
            row = row.alias(text(m.aliased_from));
        }
        row
    };
    let metas: Vec<MetaRow> = unsafe { rows(desc.meta_rows, desc.n_meta_rows) }
        .iter()
        .map(meta_row)
        .collect();
    if !metas.is_empty() {
        crate::method_meta::register_meta_rows(Vec::leak(metas));
    }
    // The redefinition-timeline rows are seeded UNregistered: each is
    // registered by the install at its own body's position, and the first
    // body's install already ran above.
    if desc.n_redef_metas > 0 {
        let redefs: Vec<MetaRow> = unsafe { rows(desc.redef_metas, desc.n_redef_metas) }
            .iter()
            .map(meta_row)
            .collect();
        crate::method_meta::seed_redef_metas(Vec::leak(redefs));
        for idx in boot_metas {
            crate::method_meta::install_redef_meta(idx as usize);
        }
    }

    crate::bootstrap::install_core_constants();
    let loaded: Vec<&str> = unsafe { rows(desc.loaded_features, desc.n_loaded) }
        .iter()
        .map(|&s| text(s))
        .collect();
    crate::globals::seed_loaded_features(&loaded);
    if desc.n_load_path > 0 {
        let paths: Vec<&str> = unsafe { rows(desc.load_path, desc.n_load_path) }
            .iter()
            .map(|&s| text(s))
            .collect();
        crate::globals::seed_load_path(&paths, desc.n_load_path_search);
    }
    // Each COMPILE-TIME box gets its own copy of what main was just
    // seeded with. A run-time box seeds itself when it is minted; both go
    // through the one seeder, so the two cannot drift.
    crate::boxes::seed_compile_time_boxes();
    if desc.n_units > 0 {
        let units: Vec<(&'static str, crate::features::CUnitFn)> =
            unsafe { rows(desc.units, desc.n_units) }
                .iter()
                .map(|r| {
                    (text(r.feature), unsafe {
                        std::mem::transmute::<abi::UnitFn, crate::features::CUnitFn>(r.f)
                    })
                })
                .collect();
        crate::features::install_feature_units_c(Vec::leak(units));
    }
    if desc.n_sources > 0 {
        let pack: Vec<(&'static str, &'static str)> = unsafe { rows(desc.sources, desc.n_sources) }
            .iter()
            .map(|r| (text(r.path), text(r.text)))
            .collect();
        crate::features::install_sources(Vec::leak(pack));
    }
    if desc.n_declined > 0 {
        let declined: Vec<(&'static str, &'static str)> =
            unsafe { rows(desc.declined, desc.n_declined) }
                .iter()
                .map(|r| (text(r.feature), text(r.reason)))
                .collect();
        crate::features::install_declined_features(Vec::leak(declined));
    }
    // A runtime without the ext measures nothing, so the table it would fill
    // is never built.
    #[cfg(feature = "ext-coverage")]
    if desc.n_cov > 0 {
        let cov: Vec<(&'static str, u32, &'static [u32], &'static [u32])> =
            unsafe { rows(desc.coverage, desc.n_cov) }
                .iter()
                .map(|c| {
                    (
                        text(c.file),
                        c.total,
                        unsafe { rows(c.stmt_lines, c.n_stmt) },
                        unsafe { rows(c.def_lines, c.n_def) },
                    )
                })
                .collect();
        crate::ext::coverage::coverage_install(Vec::leak(cov));
    }
    if desc.n_warnings > 0 {
        let lines: Vec<&str> = unsafe { rows(desc.parse_warnings, desc.n_warnings) }
            .iter()
            .map(|&s| text(s))
            .collect();
        crate::emit_parse_warnings(&lines);
    }
    if desc.data_section.len > 0 {
        crate::bootstrap::install_data_section(text(desc.data_section), desc.data_offset);
    }
}

/// The top-level `self`: a fresh handle to the `main` object -- what a
/// compiled toplevel passes as the receiver of its direct calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_main_object(out: *mut RubyValue) {
    let v = crate::dispatch::main_object();
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}
