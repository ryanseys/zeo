//! EXPERIMENTAL (M2): register a merged package's compile-time INTERFACE.
//!
//! A `--experimental-use-pkg` manifest carries `iface` rows -- what the
//! package IS. This pass turns each into a real `ClassInfo` plus BODY-LESS
//! `Scope`s (`Scope::extern_symbol`), so the host's own analysis resolves
//! the package's constants, infers receiver types, and nominates typed
//! direct calls into the package's exported bodies -- through exactly the
//! machinery a spliced gem's classes use. Codegen never emits anything for
//! these classes: their desc rows travel in the manifest, remapped onto the
//! ids minted here (`Compiler::pkg_class_map`).
//!
//! Runs from `pin_builtin_exceptions_tail`, the one moment after the shared
//! bootstrap band and before any user class -- which is also why
//! `first_class_id` must MATCH: builtin ids below it cross the boundary
//! untranslated.

use crate::compiler::{ClassId, Compiler, Scope};
use crate::hir::Visibility;

pub(super) fn register_package_interfaces(compiler: &mut Compiler) -> Result<(), String> {
    if compiler.hir.pkg_merge.is_empty() {
        return Ok(());
    }
    let manifests = compiler.hir.pkg_merge.clone();
    for (pi, m) in manifests.iter().enumerate() {
        if m.first_class_id != compiler.first_program_class_id {
            return Err(format!(
                "package '{}' was built against a different bootstrap band \
                 (its first class id is {}, this zeo's is {}); rebuild the package",
                m.feature, m.first_class_id, compiler.first_program_class_id
            ));
        }
        // Pass 1: mint every interface class, so references between them
        // resolve in any order.
        for ic in &m.iface {
            let Some(mc) = m.classes.iter().find(|c| c.id == ic.id) else {
                return Err(format!(
                    "package '{}': interface class {} has no class row",
                    m.feature, ic.id
                ));
            };
            if let Some(taken) = compiler.classes.iter().find(|c| c.name == mc.name) {
                return Err(format!(
                    "package '{}' defines `{}`, which this program already has \
                     ({}); a shared namespace between packages lands with \
                     id-table aliasing",
                    m.feature,
                    mc.name,
                    if taken.imported_pkg.is_some() {
                        "from another package"
                    } else {
                        "a builtin"
                    }
                ));
            }
            let cid = compiler.add_class(mc.name.clone(), None, ic.is_module);
            let ci = &mut compiler.classes[cid.0 as usize];
            ci.imported_pkg = Some(pi as u32);
            ci.ivars = mc.ivars.clone();
            ci.hidden_ivars = ic.hidden_ivars.clone();
            ci.explicit_superclass = ic.parent.is_some() && !ic.is_module;
            compiler.pkg_class_map.insert((pi as u32, ic.id), cid);
        }
        // Pass 2: wire parents, mixins and extends through the id map.
        let map = |compiler: &Compiler, id: u32| -> Result<ClassId, String> {
            if id < m.first_class_id {
                return Ok(ClassId(id));
            }
            compiler
                .pkg_class_map
                .get(&(pi as u32, id))
                .copied()
                .ok_or_else(|| {
                    format!(
                        "package '{}': id {} is referenced by the interface but \
                         not registered (a singleton surrogate cannot be a \
                         parent or mixin)",
                        m.feature, id
                    )
                })
        };
        for ic in &m.iface {
            let cid = compiler.pkg_class_map[&(pi as u32, ic.id)];
            let parent = match ic.parent {
                Some(p) => Some(map(compiler, p)?),
                None => None,
            };
            let mut mixins = Vec::new();
            for (mid, prepend) in &ic.mixin_order {
                mixins.push((map(compiler, *mid)?, *prepend));
            }
            let mut extends = Vec::new();
            for e in &ic.extends {
                extends.push(map(compiler, *e)?);
            }
            let ci = &mut compiler.classes[cid.0 as usize];
            ci.parent = parent;
            ci.mixin_order = mixins;
            ci.extends = extends;
        }
        // Pass 3: the methods, as body-less scopes. The SHAPE numbers come
        // from the manifest -- the package's own `layout_of` verdict -- so
        // the host cannot disagree with the compiled body's ABI.
        for ic in &m.iface {
            let cid = compiler.pkg_class_map[&(pi as u32, ic.id)];
            for (class_side, list) in [(false, &ic.methods), (true, &ic.class_methods)] {
                for im in list {
                    let params = if im.plain {
                        crate::hir::Params {
                            required: (0..im.arity).map(|i| format!("a{i}")).collect(),
                            ..Default::default()
                        }
                    } else {
                        // A catch-all shape: never plain, so nothing bakes an
                        // arity or a binding decision against it; the compiled
                        // body's own prologue owns both.
                        crate::hir::Params {
                            rest: Some(None),
                            keyword_rest: Some(None),
                            ..Default::default()
                        }
                    };
                    let visibility = match im.visibility {
                        1 => Visibility::Private,
                        2 => Visibility::Protected,
                        _ => Visibility::Public,
                    };
                    let sid = compiler.push_scope(Scope {
                        name: im.name.clone(),
                        class: Some(cid),
                        defining_class: cid,
                        lexical_home: None,
                        def_node: None,
                        alias_of: None,
                        params,
                        body: Vec::new(),
                        local_types: Default::default(),
                        uses_bare_block: im.has_blk,
                        visibility,
                        native_default: false,
                        runtime_conditional: im.runtime_conditional,
                        unit: None,
                        ruby2_keywords: false,
                        accessor: None,
                        extern_symbol: Some(im.body.clone().unwrap_or_default()),
                    });
                    let ci = &mut compiler.classes[cid.0 as usize];
                    let (list, index) = if class_side {
                        (&mut ci.own_class_methods, &mut ci.own_class_method_at)
                    } else {
                        (&mut ci.own_methods, &mut ci.own_method_at)
                    };
                    index.insert(im.name.clone(), list.len());
                    list.push(sid);
                }
            }
        }
    }
    Ok(())
}

/// The v1 boundary: a HOST definition that would change a packaged class
/// needs M4's patch rows to stay sound, so it refuses by name today. Runs
/// after the whole registration walk, when every reopen and superclass is
/// known.
pub(super) fn refuse_host_edits_of_imports(compiler: &Compiler) -> Result<(), String> {
    for class in &compiler.classes {
        if class.imported_pkg.is_some() {
            // A host `def` on the class lands as an arena-backed (non-extern)
            // scope in the own-method tables; a body statement of any other
            // kind lands in `class_body_stmts`. Either one is a reopen.
            let host_def = class
                .own_methods
                .iter()
                .chain(&class.own_class_methods)
                .any(|sid| compiler.scope(*sid).extern_symbol.is_none());
            if host_def || !class.class_body_stmts.is_empty() {
                return Err(format!(
                    "this program reopens `{}`, which a merged package provides; \
                     a host reopen of a packaged class is not supported yet \
                     (compile the gem from source instead)",
                    class.name
                ));
            }
            continue;
        }
        let touches = |id: &ClassId| {
            compiler
                .class(*id)
                .imported_pkg
                .is_some()
        };
        if class.parent.as_ref().is_some_and(touches) {
            return Err(format!(
                "`{}` subclasses `{}`, which a merged package provides; \
                 subclassing a packaged class is not supported yet \
                 (compile the gem from source instead)",
                class.name,
                compiler.class(class.parent.unwrap()).name
            ));
        }
        if let Some((mid, _)) = class.mixin_order.iter().find(|(mid, _)| touches(mid)) {
            return Err(format!(
                "`{}` mixes in `{}`, which a merged package provides; mixing \
                 in a packaged module is not supported yet (compile the gem \
                 from source instead)",
                class.name,
                compiler.class(*mid).name
            ));
        }
        if let Some(e) = class.extends.iter().find(|e| touches(e)) {
            return Err(format!(
                "`{}` extends `{}`, which a merged package provides; extending \
                 a packaged module is not supported yet (compile the gem from \
                 source instead)",
                class.name,
                compiler.class(*e).name
            ));
        }
    }
    Ok(())
}
