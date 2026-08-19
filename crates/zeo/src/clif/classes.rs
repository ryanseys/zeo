//! User-class compilation: the plain-class slice (`class Node ... end`) --
//! `ClassDesc` collection, method/accessor rows on the OBJECT channel, and
//! the eligibility rules that refuse everything the M0 slice cannot carry
//! (modules, mixins, class methods, runtime class bodies, non-Object
//! superclass machinery).

use super::emit::Emitter;
use super::{names, params, statics};
use crate::analyze::Analyzed;
use crate::compiler::AccessorKind;
use cranelift_module::{Linkage, Module};
use zeo_abi::ClassId;

/// One compiled user class.
pub(crate) struct ClassSpec {
    pub id: u32,
    pub name: String,
    pub ancestors: Vec<u32>,
    pub ivars: Vec<String>,
    pub hidden: u16,
    /// `module M` -- registered `CLASS_MODULE` (no allocator, no layout);
    /// its methods ride the VALUE channel instead of the object channel.
    /// A `zeo_abi::abi::CLASS_*` registrar selector.
    pub kind: u8,
}

/// One object-channel method to compile for a user class.
pub(crate) struct ObjMethodSpec {
    pub owner: ClassId,
    pub owner_name: String,
    pub name: String,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub tramp: cranelift_module::FuncId,
    /// `Some` = an `attr_*` accessor (no body fn at all -- the trampoline
    /// IS the method); `None` = an ordinary body + trampoline pair.
    pub accessor: Option<(usize, AccessorKind)>,
    pub body_fn: Option<cranelift_module::FuncId>,
    pub hir_params: crate::hir::Params,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
    pub dyn_ivars: bool,
    /// The class the `def` was WRITTEN in (a module method keeps the
    /// module) -- where its `super` resumes.
    pub defining_class: ClassId,
    /// This entry is the class's OWN write (not a materialized ancestor
    /// copy): its trampoline doubles as the own-`super`-target row.
    pub is_own: bool,
}

/// One class method (`def self.x`) to compile -- a `CmRow` on the
/// class-method channel. The body ALWAYS receives the runtime receiver as
/// `self` (rustc's `__dynself` twin behavior; its receiverless `Drop` mode
/// is an optimization for self-free bodies, not a semantic difference), so
/// a subclass inheriting the method runs under its own `self`.
pub(crate) struct CmMethodSpec {
    pub owner: ClassId,
    pub owner_name: String,
    pub name: String,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub tramp: cranelift_module::FuncId,
    pub body_fn: cranelift_module::FuncId,
    pub hir_params: crate::hir::Params,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
}

/// One module method: body + `ValueFn` trampoline, registered as a
/// `VmRow` on the module's id. The body's `self` is whatever receiver
/// dispatch hands over (a value pointer, as every body here takes).
pub(crate) struct ModMethodSpec {
    pub owner: ClassId,
    pub owner_name: String,
    pub name: String,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub tramp: cranelift_module::FuncId,
    pub body_fn: cranelift_module::FuncId,
    pub hir_params: crate::hir::Params,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
    pub dyn_ivars: bool,
    /// The class the `def` was WRITTEN in (a module method keeps the
    /// module) -- where its `super` resumes.
    pub defining_class: ClassId,
}

/// What `collect_classes` hands back: the class table plus its method and
/// visibility rows.
pub(crate) struct CollectedClasses {
    pub classes: Vec<ClassSpec>,
    pub methods: Vec<ObjMethodSpec>,
    /// A module's OWN methods as VALUE-channel rows on the module id --
    /// rustc's `__um_` bridges. An includer dispatches through its own
    /// materialized object-channel copies first; these rows are what a
    /// dynamic receiver (and the ancestor walk) finds.
    pub module_methods: Vec<ModMethodSpec>,
    pub class_methods: Vec<CmMethodSpec>,
    /// `(class, name)` pairs a `def self.x` WROTE on the class itself --
    /// reflection's `Method#owner` truth (`mark_own_class_method_rows`).
    pub own_cm: Vec<(u32, String)>,
    /// The instance-method twin: names each class's own body wrote
    /// (`mark_own_rows` -- `instance_methods(false)`/`Method#owner`).
    pub own_rows: Vec<(u32, String)>,
    pub vis: Vec<statics::VisRowSpec>,
}

/// Collect + declare every user class and its methods; refusals are loud
/// and name the class.
pub(crate) fn collect_classes(
    em: &mut Emitter,
    analyzed: &Analyzed,
) -> Result<CollectedClasses, String> {
    let compiler = &analyzed.compiler;
    let mut classes = Vec::new();
    let mut methods = Vec::new();
    let mut module_methods = Vec::new();
    let mut class_methods = Vec::new();
    let mut own_cm = Vec::new();
    let mut own_rows = Vec::new();
    let mut vis = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap {
            continue;
        }
        let name = compiler.fq_name(crate::compiler::ClassId(idx as u32));
        let refuse = |what: &str| {
            Err(format!(
                "--backend aot is an M0 vertical slice: cannot lower {what} yet (class {name})"
            ))
        };
        if class.box_id != 0 {
            return refuse("a boxed class");
        }
        if class.runtime_conditional {
            return refuse("a conditionally-defined class");
        }
        // `include` works through the two mechanisms below (materialized
        // copies on the includer + value rows on the module + the module in
        // `ancestors`); the rest of the mixin surface still refuses.
        if !(class.prepends.is_empty()
            && class.extends.is_empty()
            && class.class_method_prepends.is_empty()
            && class.imported_modules.is_empty())
        {
            return refuse("a mixin");
        }
        if !(class.pending_aliases.is_empty()
            && class.builtin_aliases.is_empty()
            && class.class_aliases.is_empty()
            && class.undefined.is_empty()
            && class.class_undefined.is_empty()
            && class.runtime_undefs.is_empty()
            && class.pending_module_functions.is_empty()
            && class.visibility_overrides.is_empty()
            && class.class_visibility_overrides.is_empty()
            && class.singleton_super_targets.is_empty())
        {
            return refuse("this class-surface shape");
        }
        if class.feature_gate.is_some() {
            return refuse("a feature-gated class");
        }
        // The registrar this class needs. An exception subclass's chain
        // holds builtin superclasses BY DESIGN -- the registrar installs
        // the native default method set and the class's own defs layer
        // over it as deltas (rustc's `register_exception_subclass` arm).
        let cid = ClassId(idx as u32);
        let native_backed = compiler.is_native_backed(cid);
        let immediate = compiler.is_immediate_subclass(cid);
        // rustc's registrar chain, in its order.
        let kind = if class.is_module {
            zeo_abi::abi::CLASS_MODULE
        } else if compiler.is_exception_backed(cid) {
            zeo_abi::abi::CLASS_EXCEPTION
        } else if compiler.is_value_subclass(cid) {
            zeo_abi::abi::CLASS_VALUE_SUBCLASS
        } else if compiler.is_date_subclass(cid) || compiler.is_proc_subclass(cid) {
            zeo_abi::abi::CLASS_RECV_HONOURING
        } else if compiler.is_weakmap_subclass(cid) {
            zeo_abi::abi::CLASS_WEAK_MAP
        } else if compiler.is_module_subclass(cid) {
            zeo_abi::abi::CLASS_MODULE_SUBCLASS
        } else if immediate {
            zeo_abi::abi::CLASS_IMMEDIATE
        } else {
            zeo_abi::abi::CLASS_PLAIN
        };
        // Every ancestor past self must be a user class or the plain
        // Object/Kernel/BasicObject spine -- a builtin superclass outside
        // the shapes above selects a registrar the slice does not emit.
        for &a in class
            .ancestors
            .iter()
            .skip(1)
            .filter(|_| kind == zeo_abi::abi::CLASS_PLAIN)
        {
            let plain_spine = matches!(a.0, 0 | 24 | 25);
            let user = (a.0 as usize) < compiler.classes.len()
                && !compiler.classes[a.0 as usize].is_builtin
                && !compiler.classes[a.0 as usize].is_bootstrap;
            // A builtin MODULE in the chain (`include Comparable`) is fine:
            // registration carries the id and the dispatch walk probes the
            // module's builtin table at its MRO position, exactly as it does
            // for Kernel. Only a builtin SUPERCLASS selects a registrar the
            // slice does not emit yet.
            let builtin_module =
                (a.0 as usize) < compiler.classes.len() && compiler.classes[a.0 as usize].is_module;
            if !(plain_spine || user || builtin_module) {
                return refuse("a builtin superclass");
            }
        }

        classes.push(ClassSpec {
            id: idx as u32,
            name: name.clone(),
            ancestors: class.ancestors.iter().map(|c| c.0).collect(),
            ivars: class.ivars.clone(),
            hidden: u16::try_from(class.hidden_ivars.len()).expect("hidden ivars fit u16"),
            kind,
        });
        // What this class's own body wrote -- `instance_methods(false)` /
        // `Method#owner` truth, exactly the rustc `mark_own_rows` list
        // (plus re-scoped `zsuper` entries, refused above with the
        // visibility overrides they ride in on).
        let mut own: Vec<&String> = class
            .own_methods
            .iter()
            .map(|&sid| &compiler.scope(sid).name)
            .collect();
        own.sort();
        for n in own {
            own_rows.push((idx as u32, n.clone()));
        }

        if class.is_module {
            // A module's own methods ride the VALUE channel on its own id.
            for &sid in &class.own_methods {
                let scope = compiler.scope(sid);
                if scope.native_default {
                    continue;
                }
                let mname = scope.name.clone();
                let refuse_m = |what: &str| {
                    Err(format!(
                        "--backend aot is an M0 vertical slice: cannot lower {what} yet ({name}#{mname})"
                    ))
                };
                if scope.runtime_conditional {
                    return refuse_m("a conditionally-defined method");
                }
                if scope.alias_of.is_some() {
                    return refuse_m("an alias");
                }
                if scope.accessor.is_some() {
                    return refuse_m("a module accessor");
                }
                let p = &scope.params;
                if let Err(what) = super::emit::check_params(p) {
                    return refuse_m(what);
                }
                let layout = super::params::layout_of(p)?;
                let has_blk = scope.needs_block_param();
                let tramp = em
                    .module
                    .declare_function(
                        &names::trampoline_symbol(&name, &mname),
                        Linkage::Local,
                        &params::value_fn_sig(em),
                    )
                    .map_err(|e| format!("declaring {name}#{mname}: {e}"))?;
                let sig = params::body_sig(em, layout.n_slots, has_blk);
                let body_fn = em
                    .module
                    .declare_function(&names::method_symbol(&name, &mname), Linkage::Local, &sig)
                    .map_err(|e| format!("declaring {name}#{mname}: {e}"))?;
                match scope.visibility {
                    crate::hir::Visibility::Private => vis.push(statics::VisRowSpec {
                        class: idx as u32,
                        name: mname.clone(),
                        verb: 0,
                    }),
                    crate::hir::Visibility::Protected => vis.push(statics::VisRowSpec {
                        class: idx as u32,
                        name: mname.clone(),
                        verb: 1,
                    }),
                    crate::hir::Visibility::Public => {}
                }
                module_methods.push(ModMethodSpec {
                    dyn_ivars: false,
                    defining_class: scope.defining_class,
                    owner: ClassId(idx as u32),
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    body_fn,
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                });
            }
        }

        // An immediate-builtin subclass has NO instances -- rustc emits no
        // method rows for it (the definition is allowed, `.new` raises).
        let module_subclass = kind == zeo_abi::abi::CLASS_MODULE_SUBCLASS;
        for entry in class
            .methods
            .iter()
            .filter(|_| !class.is_module && !immediate)
        {
            let scope = compiler.scope(entry.def);
            if scope.native_default {
                continue;
            }
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(format!(
                    "--backend aot is an M0 vertical slice: cannot lower {what} yet ({name}#{mname})"
                ))
            };
            if scope.runtime_conditional {
                return refuse_m("a conditionally-defined method");
            }
            if scope.alias_of.is_some() {
                return refuse_m("an alias");
            }
            let p = &scope.params;
            if let Err(what) = super::emit::check_params(p) {
                return refuse_m(what);
            }
            let layout = super::params::layout_of(p)?;
            // A native-backed instance has NO slots: its accessor runs as
            // an ordinary body (a lone name-keyed ivar read/write), the
            // rustc delta shape.
            let accessor = match (&scope.accessor, native_backed) {
                (Some(_), true) | (None, _) => None,
                (Some(shape), false) => {
                    let slot = crate::analyze::class_query::slot_of(
                        compiler,
                        ClassId(idx as u32),
                        &shape.ivar,
                    )
                    .ok_or_else(|| {
                        format!("accessor ivar @{} has no slot on {name}", shape.ivar)
                    })?;
                    Some((slot, shape.kind))
                }
            };
            let has_blk = scope.needs_block_param();
            let tramp = em
                .module
                .declare_function(
                    &names::trampoline_symbol(&name, &mname),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| format!("declaring {name}#{mname}: {e}"))?;
            let body_fn = if accessor.is_none() {
                let sig = params::body_sig(em, layout.n_slots, has_blk);
                Some(
                    em.module
                        .declare_function(
                            &names::method_symbol(&name, &mname),
                            Linkage::Local,
                            &sig,
                        )
                        .map_err(|e| format!("declaring {name}#{mname}: {e}"))?,
                )
            } else {
                None
            };
            let verb = match scope.visibility {
                crate::hir::Visibility::Private => Some(0),
                crate::hir::Visibility::Protected => Some(1),
                crate::hir::Visibility::Public => None,
            };
            if let Some(verb) = verb {
                vis.push(statics::VisRowSpec {
                    class: idx as u32,
                    name: mname.clone(),
                    verb,
                });
            }
            if module_subclass {
                // A `class X < Module` instance is a `RubyValue::Class`: the
                // object channel never sees it, so X's defs register as VALUE
                // rows on X's id (rustc's `define_value_method` arm); ivars
                // resolve through `ivar_set_dyn`'s Class arm (civars).
                module_methods.push(ModMethodSpec {
                    dyn_ivars: true,
                    defining_class: scope.defining_class,
                    owner: ClassId(idx as u32),
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    body_fn: body_fn.expect("module-subclass accessors run as bodies"),
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                });
                continue;
            }
            methods.push(ObjMethodSpec {
                is_own: class.own_methods.contains(&entry.def),
                dyn_ivars: native_backed,
                defining_class: scope.defining_class,
                owner: ClassId(idx as u32),
                owner_name: name.clone(),
                name: mname,
                body: scope.body.clone(),
                node: scope.def_node,
                tramp,
                accessor,
                body_fn,
                hir_params: p.clone(),
                has_blk,
                ruby2_keywords: scope.ruby2_keywords,
            });
        }
        // Class methods (`def self.x`): every entry registers on the
        // CLASS-METHOD channel (a `CmRow`), so both a literal `Foo.run`
        // and a variable-held class dispatch through it. With mixins
        // refused above, `class_methods` is the class's own writes.
        for entry in &class.class_methods {
            let scope = compiler.scope(entry.def);
            if scope.native_default {
                continue;
            }
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(format!(
                    "--backend aot is an M0 vertical slice: cannot lower {what} yet ({name}.{mname})"
                ))
            };
            if scope.runtime_conditional {
                return refuse_m("a conditionally-defined class method");
            }
            if scope.alias_of.is_some() {
                return refuse_m("a class-method alias");
            }
            if scope.accessor.is_some() {
                return refuse_m("a singleton accessor");
            }
            let p = &scope.params;
            if let Err(what) = super::emit::check_params(p) {
                return refuse_m(what);
            }
            let layout = super::params::layout_of(p)?;
            let has_blk = scope.needs_block_param();
            let tramp = em
                .module
                .declare_function(
                    &names::class_trampoline_symbol(&name, &mname),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| format!("declaring {name}.{mname}: {e}"))?;
            let sig = params::body_sig(em, layout.n_slots, has_blk);
            let body_fn = em
                .module
                .declare_function(
                    &names::class_method_symbol(&name, &mname),
                    Linkage::Local,
                    &sig,
                )
                .map_err(|e| format!("declaring {name}.{mname}: {e}"))?;
            // Only Private gets a row (verb 3) -- exactly the rustc
            // `emit_class_method_visibility_rows` rule.
            if entry.visibility == crate::hir::Visibility::Private {
                vis.push(statics::VisRowSpec {
                    class: idx as u32,
                    name: mname.clone(),
                    verb: 3,
                });
            }
            class_methods.push(CmMethodSpec {
                owner: ClassId(idx as u32),
                owner_name: name.clone(),
                name: mname,
                body: scope.body.clone(),
                node: scope.def_node,
                tramp,
                body_fn,
                hir_params: p.clone(),
                has_blk,
                ruby2_keywords: scope.ruby2_keywords,
            });
        }
        for &sid in &class.own_class_methods {
            own_cm.push((idx as u32, compiler.scope(sid).name.clone()));
        }
    }
    Ok(CollectedClasses {
        classes,
        methods,
        module_methods,
        class_methods,
        own_cm,
        own_rows,
        vis,
    })
}
