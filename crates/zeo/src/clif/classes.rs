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
    /// A compiled `Struct`/`Data`'s member names, in declaration order
    /// (`register_compiled_struct`); empty for every other class.
    pub members: Vec<String>,
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
    /// An own method SHADOWED by a `prepend` winner: its body compiles and
    /// its `REG_SUPER_TARGET_VALUE` row registers, but no `ObjRow` -- the
    /// object channel carries the module's materialized copy (rustc's
    /// `__own_` bridge shape).
    pub super_target_only: bool,
}

/// One class method (`def self.x`) to compile -- a `CmRow` on the
/// class-method channel. The body ALWAYS receives the runtime receiver as
/// `self` (rustc's `__dynself` twin behavior; its receiverless `Drop` mode
/// is an optimization for self-free bodies, not a semantic difference), so
/// a subclass inheriting the method runs under its own `self`.
pub(crate) struct CmMethodSpec {
    /// `false` = a singleton-super-target-only body (a shadowed `extend`
    /// copy): compiled and registered under `(module, name)`, but no
    /// `CmRow` on the class-method channel.
    pub cm_row: bool,
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
    /// `(class, module ids)` -- `register_extends` rows: the modules on
    /// each class's SINGLETON chain (`extend M`, `extend self`).
    pub extends: Vec<(u32, Vec<u32>)>,
    /// `(class, name)` -- `undef` marks (`mark_undefined`).
    pub undef_rows: Vec<(u32, String)>,
    /// Runtime-conditional class ids, concealed until their guarded body
    /// reveals them.
    pub conceal: Vec<u32>,
    /// `(class, new, old, is_class_side)` -- builtin-source alias rows
    /// (`register_alias` / `register_class_alias` name indirections).
    pub alias_rows: Vec<(u32, String, String, bool)>,
    /// `(class, module, name, trampoline)` -- singleton-chain super
    /// targets: every `extend`ed method copy (winner AND shadowed) plus
    /// inherited class methods a subclass's own `def self.x` shadowed
    /// (`define_singleton_super_target`).
    pub sst: Vec<(u32, u32, String, cranelift_module::FuncId)>,
    /// `(class, name)` value rows a builtin reopen INHERITED (a module
    /// method materialized onto the builtin) -- marked foreign so a
    /// `super` walk skips them at that position (`mark_foreign_value_rows`).
    pub foreign: Vec<(u32, String)>,
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
    let mut foreign = Vec::new();
    let mut own_rows = Vec::new();
    let mut vis = Vec::new();
    let mut extends: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut sst: Vec<(u32, u32, String, cranelift_module::FuncId)> = Vec::new();
    let mut alias_rows: Vec<(u32, String, String, bool)> = Vec::new();
    let mut conceal: Vec<u32> = Vec::new();
    // Builtin-source alias rows, every class including the toplevel (rustc's
    // `alias_registration_rows`; the boxed-overlay target case is refused
    // with its class). A require-gated builtin whose feature never fired
    // registers nothing, aliases included.
    for (idx, class) in compiler.classes.iter().enumerate() {
        if class.builtin_aliases.is_empty() && class.class_aliases.is_empty() {
            continue;
        }
        if (class.is_builtin || class.is_bootstrap)
            && !compiler.feature_active(crate::compiler::ClassId(idx as u32))
        {
            continue;
        }
        for (new, old) in &class.builtin_aliases {
            alias_rows.push((idx as u32, new.clone(), old.clone(), false));
        }
        for (new, old) in &class.class_aliases {
            alias_rows.push((idx as u32, new.clone(), old.clone(), true));
        }
    }

    // REOPENED builtins first (rustc's builtin-registration loop): a
    // non-bootstrap builtin's user methods ride the VALUE channel on the
    // builtin's own id (they dispatch FIRST, before the native table); a
    // BOOTSTRAP (exception) reopen's ride the OBJECT channel as deltas
    // over the native set `with_core` installed. Bodies take a
    // `RubyValue` self, so ivars are name-keyed (`dyn_ivars`).
    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || !(class.is_builtin || class.is_bootstrap) {
            continue;
        }
        // The entries this id will actually carry. A BOOTSTRAP (exception)
        // reopen keeps only its DELTAS -- a body defined on a native-backed
        // class, or on a module mixed in ABOVE Object (rustc's
        // `emit_exception_deltas` filter); a top-level `include M` reaches
        // every exception through Object and is deliberately nobody's
        // delta. Guards below fire only when something will emit.
        let deltas: Vec<&crate::compiler::MethodEntry> = class
            .methods
            .iter()
            .filter(|e| {
                let scope = compiler.scope(e.def);
                if scope.native_default {
                    return false;
                }
                if !class.is_bootstrap {
                    return true;
                }
                let dc = e.defined_class(compiler);
                let above_object = {
                    let anc = &class.ancestors;
                    let cut = anc
                        .iter()
                        .position(|&a| a == crate::compiler::OBJECT_CLASS)
                        .unwrap_or(anc.len());
                    &anc[..cut]
                };
                compiler.is_native_backed(dc)
                    || (compiler.class(dc).is_module && above_object.contains(&dc))
            })
            .collect();
        let cms: Vec<&crate::compiler::MethodEntry> = class
            .class_methods
            .iter()
            .filter(|e| !compiler.scope(e.def).native_default)
            .collect();
        if deltas.is_empty() && cms.is_empty() {
            continue;
        }
        let name = compiler.fq_name(crate::compiler::ClassId(idx as u32));
        let refuse = |what: &str| {
            Err(format!(
                "the CLIF backend cannot lower {what} yet (class {name})"
            ))
        };
        // A require-gated builtin whose feature never fired: no code can
        // resolve its constant, so its rows would be dead weight -- rustc
        // skips it entirely (the register-only-enabled-features rule).
        if !compiler.feature_active(crate::compiler::ClassId(idx as u32)) {
            continue;
        }
        if class.builtin_overlay.is_some() || class.box_id != 0 {
            return refuse("a boxed builtin overlay");
        }
        // A builtin carries its natural includes (String includes
        // Comparable); only a reopen that CHANGED the ancestry -- an
        // ancestors list differing from the declared default -- selects
        // the set_ancestors patch the slice does not emit.
        if class.ancestors != zeo_abi::declared_ancestors(crate::compiler::ClassId(idx as u32)) {
            return refuse("an ancestry-changing builtin reopen");
        }
        if !class.singleton_super_targets.is_empty() {
            return refuse("a singleton super target on a builtin reopen");
        }
        // No `mark_own_rows` here: a VALUE row self-records ownership at
        // insert (`own_value_names`), and a bootstrap delta's object-channel
        // row follows rustc, which marks nothing for reopens either.
        for entry in deltas {
            let scope = compiler.scope(entry.def);
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(format!(
                    "the CLIF backend cannot lower {what} yet ({name}#{mname})"
                ))
            };
            let dc = entry.defined_class(compiler);
            if !class.is_bootstrap && dc.0 != idx as u32 {
                // A module method materialized onto this builtin: the row
                // registers here (compiled in this class's context) and is
                // marked FOREIGN so `super` skips this position.
                foreign.push((idx as u32, mname.clone()));
            }
            // A conditional `def` emits a RUNTIME install at its document
            // position instead (analyze left it in the body's statements),
            // so it contributes no static row here.
            if scope.runtime_conditional {
                continue;
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
            if class.is_bootstrap {
                // An exception reopen: an OBJECT-channel delta.
                methods.push(ObjMethodSpec {
                    is_own: class.own_methods.contains(&entry.def),
                    super_target_only: false,
                    dyn_ivars: true,
                    defining_class: scope.defining_class,
                    owner: ClassId(idx as u32),
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    accessor: None,
                    body_fn: Some(body_fn),
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                });
            } else {
                // A value-channel row on the builtin's own id.
                module_methods.push(ModMethodSpec {
                    dyn_ivars: true,
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
        for entry in cms {
            let scope = compiler.scope(entry.def);
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(format!(
                    "the CLIF backend cannot lower {what} yet ({name}.{mname})"
                ))
            };
            if scope.runtime_conditional {
                continue;
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
            if entry.visibility == crate::hir::Visibility::Private {
                vis.push(statics::VisRowSpec {
                    class: idx as u32,
                    name: mname.clone(),
                    verb: 3,
                });
            }
            class_methods.push(CmMethodSpec {
                cm_row: true,
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
            if compiler.scope(sid).native_default {
                continue;
            }
            own_cm.push((idx as u32, compiler.scope(sid).name.clone()));
        }
    }

    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap {
            continue;
        }
        let name = compiler.fq_name(crate::compiler::ClassId(idx as u32));
        let refuse = |what: &str| {
            Err(format!(
                "the CLIF backend cannot lower {what} yet (class {name})"
            ))
        };
        if class.box_id != 0 {
            return refuse("a boxed class");
        }
        // A runtime-CONDITIONAL class registers its shape (the static MRO
        // needs one) but starts CONCEALED: the constant does not exist
        // until the guarded body runs and reveals it.
        if class.runtime_conditional {
            conceal.push(idx as u32);
        }
        // `include` works through the two mechanisms below (materialized
        // copies on the includer + value rows on the module + the module in
        // `ancestors`); the rest of the mixin surface still refuses.
        if !(class.class_method_prepends.is_empty() && class.imported_modules.is_empty()) {
            return refuse("a mixin");
        }
        // The rest of the class surface needs no refusal: `pending_aliases`,
        // `pending_module_functions` and `class_undefined` are DRAINED or
        // applied by analyze, `runtime_undefs` is compile-side fold
        // suppression, and `undefined`/visibility overrides emit as rows in
        // the uniform pass below.
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
        // Prepend winners flow through the MATERIALIZED `methods` table
        // (analyze picked them; `ancestors` already orders the module
        // before the class); the shadowed own defs register below as
        // super-target-only rows. Native-backed shapes would need rustc's
        // `promote_own_impl` twin instead -- not built yet.
        if !class.prepends.is_empty() && kind != zeo_abi::abi::CLASS_PLAIN {
            return refuse("a prepend on a native-backed class");
        }
        if !class.extends.is_empty() {
            extends.push((idx as u32, class.extends.iter().map(|m| m.0).collect()));
        }
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
            // The two ordinary-object builtins from the payload-root
            // DENYLIST that no other registrar claims (`Numeric`,
            // `WeakRef`): rustc emits the plain generated struct for a
            // subclass and inherited behavior comes from the walk probing
            // their builtin tables at MRO position (`Date::Infinity <
            // Numeric` is the corpus shape).
            // `Struct`/`Data` are on the payload-root denylist for the same
            // reason: their subclasses are ORDINARY ivar objects whose
            // members are hidden ivars, so the plain registrar serves them
            // and `register_compiled_struct` (below) hands `Struct`'s one
            // shared protocol the member list.
            let plain_builtin = matches!(
                a,
                zeo_abi::NUMERIC_CLASS
                    | zeo_abi::WEAKREF_CLASS
                    | zeo_abi::STRUCT_CLASS
                    | zeo_abi::DATA_CLASS
            );
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
            if !(plain_spine || plain_builtin || user || builtin_module) {
                return refuse("a builtin superclass");
            }
        }

        classes.push(ClassSpec {
            id: idx as u32,
            name: name.clone(),
            ancestors: class.ancestors.iter().map(|c| c.0).collect(),
            ivars: class.ivars.clone(),
            hidden: u16::try_from(class.hidden_ivars.len()).expect("hidden ivars fit u16"),
            members: class.hidden_ivars.clone(),
            kind,
        });
        // What this class's own body wrote -- `instance_methods(false)` /
        // `Method#owner` truth, exactly the rustc `mark_own_rows` list
        // (plus re-scoped `zsuper` entries, refused above with the
        // visibility overrides they ride in on).
        // A conditional `def` is left out: `instance_methods(false)` must
        // report what the class HAS, and whether it has this one is settled
        // at run time by the row its install writes.
        let mut own: Vec<&String> = class
            .own_methods
            .iter()
            .filter(|&&sid| !compiler.scope(sid).runtime_conditional)
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
                        "the CLIF backend cannot lower {what} yet ({name}#{mname})"
                    ))
                };
                if scope.runtime_conditional {
                    continue;
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
                    // A module's VALUE-channel row takes whatever receiver
                    // dispatch hands over, so its ivars are name-keyed (the
                    // includer's materialized object-channel copy keeps the
                    // slot-indexed fast path).
                    dyn_ivars: true,
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
                    "the CLIF backend cannot lower {what} yet ({name}#{mname})"
                ))
            };
            // A conditional `def` emits a RUNTIME install at its document
            // position instead (analyze left it in the body's statements),
            // so it contributes no static row here.
            if scope.runtime_conditional {
                continue;
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
                super_target_only: false,
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
        // An own method a `prepend` SHADOWED never won its name in the
        // materialized table -- its body still compiles, reachable ONLY
        // through the module copy's `super` (rustc's `__own_` bridge +
        // `define_super_target_value`).
        if !class.prepends.is_empty() && !class.is_module && !immediate {
            let winners: std::collections::HashSet<crate::compiler::ScopeId> =
                class.methods.iter().map(|e| e.def).collect();
            for &sid in &class.own_methods {
                if winners.contains(&sid) {
                    continue;
                }
                let scope = compiler.scope(sid);
                if scope.native_default {
                    continue;
                }
                let mname = scope.name.clone();
                let refuse_m = |what: &str| {
                    Err(format!(
                        "the CLIF backend cannot lower {what} yet ({name}#{mname})"
                    ))
                };
                if scope.runtime_conditional {
                    continue;
                }

                let p = &scope.params;
                if let Err(what) = super::emit::check_params(p) {
                    return refuse_m(what);
                }
                let layout = super::params::layout_of(p)?;
                let accessor = match &scope.accessor {
                    None => None,
                    Some(shape) => {
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
                        &names::trampoline_symbol(&name, &format!("__own_{mname}")),
                        Linkage::Local,
                        &params::value_fn_sig(em),
                    )
                    .map_err(|e| format!("declaring {name}#{mname}: {e}"))?;
                let body_fn = if accessor.is_none() {
                    let sig = params::body_sig(em, layout.n_slots, has_blk);
                    Some(
                        em.module
                            .declare_function(
                                &names::method_symbol(&name, &format!("__own_{mname}")),
                                Linkage::Local,
                                &sig,
                            )
                            .map_err(|e| format!("declaring {name}#{mname}: {e}"))?,
                    )
                } else {
                    None
                };
                methods.push(ObjMethodSpec {
                    is_own: true,
                    super_target_only: true,
                    dyn_ivars: false,
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
        }
        // Class methods (`def self.x`): every entry registers on the
        // CLASS-METHOD channel (a `CmRow`), so both a literal `Foo.run`
        // and a variable-held class dispatch through it. With mixins
        // refused above, `class_methods` is the class's own writes.
        let mut cm_def_tramps: Vec<(crate::compiler::ScopeId, cranelift_module::FuncId)> =
            Vec::new();
        for entry in &class.class_methods {
            let scope = compiler.scope(entry.def);
            if scope.native_default {
                continue;
            }
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(format!(
                    "the CLIF backend cannot lower {what} yet ({name}.{mname})"
                ))
            };
            if scope.runtime_conditional {
                continue;
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
            cm_def_tramps.push((entry.def, tramp));
            class_methods.push(CmMethodSpec {
                cm_row: true,
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
        // Singleton-chain super targets (rustc's `__sst_` containers +
        // winner reuse): every `(module, def)` pair in
        // `singleton_super_targets`, deduped. A pair whose def IS a
        // materialized winner reuses that `CmRow` trampoline (every CLIF
        // class-method body is receiver-generic); a shadowed copy gets its
        // own body under a per-(class,module) symbol.
        let mut sst_seen: Vec<(ClassId, crate::compiler::ScopeId)> = Vec::new();
        for &(m, sid) in &class.singleton_super_targets {
            if sst_seen.contains(&(m, sid)) {
                continue;
            }
            sst_seen.push((m, sid));
            let scope = compiler.scope(sid);
            if scope.native_default {
                continue;
            }
            let mname = scope.name.clone();
            if let Some(&(_, tramp)) = cm_def_tramps.iter().find(|(d, _)| *d == sid) {
                sst.push((idx as u32, m.0, mname, tramp));
                continue;
            }
            let refuse_m = |what: &str| {
                Err(format!(
                    "the CLIF backend cannot lower {what} yet ({name}.{mname})"
                ))
            };
            if scope.runtime_conditional {
                continue;
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
                    &names::class_trampoline_symbol(&name, &format!("__sst_{}_{mname}", m.0)),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| format!("declaring {name}.{mname}: {e}"))?;
            let sig = params::body_sig(em, layout.n_slots, has_blk);
            let body_fn = em
                .module
                .declare_function(
                    &names::class_method_symbol(&name, &format!("__sst_{}_{mname}", m.0)),
                    Linkage::Local,
                    &sig,
                )
                .map_err(|e| format!("declaring {name}.{mname}: {e}"))?;
            sst.push((idx as u32, m.0, mname.clone(), tramp));
            class_methods.push(CmMethodSpec {
                cm_row: false,
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
    // Visibility overrides (`private :m` retagging an inherited method) and
    // `undef` marks, every class including the toplevel. Vis rows apply in
    // order, and this pass runs after every per-method stamp above, so the
    // override wins -- rustc's append-after-the-loop rule.
    let mut undef_rows: Vec<(u32, String)> = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if (class.is_builtin || class.is_bootstrap)
            && !compiler.feature_active(crate::compiler::ClassId(idx as u32))
        {
            continue;
        }
        for (mname, v) in &class.visibility_overrides {
            let verb = match v {
                crate::hir::Visibility::Private => 0,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => 2,
            };
            vis.push(statics::VisRowSpec {
                class: idx as u32,
                name: mname.clone(),
                verb,
            });
        }
        for (mname, v) in &class.class_visibility_overrides {
            let verb = if *v == crate::hir::Visibility::Private {
                3
            } else {
                4
            };
            vis.push(statics::VisRowSpec {
                class: idx as u32,
                name: mname.clone(),
                verb,
            });
        }
        let mut undefined: Vec<&String> = class.undefined.iter().collect();
        undefined.sort();
        for n in undefined {
            undef_rows.push((idx as u32, n.clone()));
        }
    }
    Ok(CollectedClasses {
        classes,
        methods,
        module_methods,
        class_methods,
        own_cm,
        foreign,
        own_rows,
        vis,
        extends,
        sst,
        alias_rows,
        undef_rows,
        conceal,
    })
}
