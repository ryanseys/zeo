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
}

/// One object-channel method to compile for a user class.
pub(crate) struct ObjMethodSpec {
    pub owner: ClassId,
    pub owner_name: String,
    pub name: String,
    pub arity: usize,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub params: Vec<String>,
    pub tramp: cranelift_module::FuncId,
    /// `Some` = an `attr_*` accessor (no body fn at all -- the trampoline
    /// IS the method); `None` = an ordinary body + trampoline pair.
    pub accessor: Option<(usize, AccessorKind)>,
    pub body_fn: Option<cranelift_module::FuncId>,
}

/// What `collect_classes` hands back: the class table plus its method and
/// visibility rows.
pub(crate) struct CollectedClasses {
    pub classes: Vec<ClassSpec>,
    pub methods: Vec<ObjMethodSpec>,
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
    let mut vis = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap {
            continue;
        }
        let name = class.name.clone();
        let refuse = |what: &str| {
            Err(format!(
                "--backend aot is an M0 vertical slice: cannot lower {what} yet (class {name})"
            ))
        };
        if class.is_module {
            return refuse("a module");
        }
        if class.box_id != 0 {
            return refuse("a boxed class");
        }
        if class.runtime_conditional {
            return refuse("a conditionally-defined class");
        }
        if !class.class_body_stmts.is_empty() {
            return refuse("a class body with runtime statements");
        }
        if !(class.prepends.is_empty()
            && class.includes.is_empty()
            && class.extends.is_empty()
            && class.class_method_prepends.is_empty()
            && class.imported_modules.is_empty())
        {
            return refuse("a mixin");
        }
        if !class.class_methods.is_empty() || !class.own_class_methods.is_empty() {
            return refuse("a class method");
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
        // Every ancestor past self must be a user class or the plain
        // Object/Kernel/BasicObject spine -- an exception/value/module
        // superclass selects a registrar the slice does not emit.
        for &a in class.ancestors.iter().skip(1) {
            let plain_spine = matches!(a.0, 0 | 24 | 25);
            let user = (a.0 as usize) < compiler.classes.len()
                && !compiler.classes[a.0 as usize].is_builtin
                && !compiler.classes[a.0 as usize].is_bootstrap;
            if !(plain_spine || user) {
                return refuse("a builtin superclass");
            }
        }

        classes.push(ClassSpec {
            id: idx as u32,
            name: name.clone(),
            ancestors: class.ancestors.iter().map(|c| c.0).collect(),
            ivars: class.ivars.clone(),
            hidden: u16::try_from(class.hidden_ivars.len()).expect("hidden ivars fit u16"),
        });

        for entry in &class.methods {
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
            if !(p.destructures.is_empty()
                && p.optional.is_empty()
                && p.rest.is_none()
                && !p.implicit_rest
                && p.post.is_empty()
                && p.keywords.is_empty()
                && p.keyword_rest.is_none()
                && p.block.is_none()
                && p.block_locals.is_empty()
                && p.implicit_block_locals.is_empty())
            {
                return refuse_m("a def with non-required parameters");
            }
            let accessor = match &scope.accessor {
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
                None => None,
            };
            let arity = p.required.len();
            let tramp = em
                .module
                .declare_function(
                    &names::trampoline_symbol(&name, &mname),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| format!("declaring {name}#{mname}: {e}"))?;
            let body_fn = if accessor.is_none() {
                let sig = params::body_sig(em, arity);
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
            methods.push(ObjMethodSpec {
                owner: ClassId(idx as u32),
                owner_name: name.clone(),
                name: mname,
                arity,
                body: scope.body.clone(),
                node: scope.def_node,
                params: p.required.clone(),
                tramp,
                accessor,
                body_fn,
            });
        }
    }
    Ok(CollectedClasses {
        classes,
        methods,
        vis,
    })
}
