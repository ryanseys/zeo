//! Register a merged package's compile-time INTERFACE.
//!
//! A `--with-package` manifest carries `iface` rows -- what the
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

/// A HOST static definition on a BUILTIN method that a merged package also
/// defines (a top-level def both write, a builtin method both reopen) is
/// two bodies for one name on the one class the two share with no id
/// boundary. The host's row and the package's row would race at boot
/// instead of installing in document order, so the shape refuses -- naming
/// the PACKAGE, which is what lets the drop-to-splice tier retry that gem
/// from source.
pub(super) fn refuse_host_spine_redefinitions(compiler: &Compiler) -> Result<(), String> {
    if compiler.hir.pkg_merge.is_empty() {
        return Ok(());
    }
    let feature_of = |cid: u32, name: &str, class_side: bool| -> String {
        compiler
            .hir
            .pkg_merge
            .iter()
            .find(|m| {
                if class_side {
                    m.cm.iter().any(|r| r.class == cid && r.name == name)
                } else {
                    m.vm.iter().any(|r| r.class == cid && r.name == name)
                }
            })
            .map(|m| m.feature.clone())
            .unwrap_or_else(|| "?".into())
    };
    // The registration walk records each displacement at its one
    // replacement site (`add_own_method_at`): the host's def took the
    // list slot the package's body held, so a scan here could no longer
    // see both.
    if let Some((class, name, class_side)) = compiler.pkg_spine_redefs.first() {
        let sep = if *class_side { "." } else { "#" };
        return Err(format!(
            "this program defines `{}{sep}{name}`, which package '{}' also \
             defines; two static bodies for one shared-class name cannot \
             hold their document order yet (compile the gem from source \
             instead)",
            compiler.class(*class).name,
            feature_of(class.0, name, *class_side)
        ));
    }
    // A HOST `using` of a merged package's module: the refinements live in
    // the package's compiled bodies, not in its interface, so the host's
    // refined-send rewrite would silently miss them. Refuse by package
    // name, which drops that gem to its source splice.
    for a in &compiler.activations {
        if let Some(pi) = compiler.class(a.module).imported_pkg {
            return Err(format!(
                "this program writes `using {}`, whose refinements package \
                 '{}' compiled internally; a host cannot activate a \
                 package's refinements yet (compile the gem from source \
                 instead)",
                compiler.class(a.module).name,
                compiler.hir.pkg_merge[pi as usize].feature
            ));
        }
    }
    Ok(())
}

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
        // resolve in any order. A name another PACKAGE already claimed is
        // not an error -- `module Rack` in two gems is ordinary Ruby -- it
        // ALIASES: both local ids map onto the one host id, and passes 2-3
        // hold the compatibility line (one shape, disjoint methods).
        for ic in &m.iface {
            // A BUILTIN row is a package REOPEN of a class the host already
            // has, under the same untranslated id: no class to mint, no
            // ancestry to wire -- pass 3 registers its added methods.
            if ic.id < m.first_class_id {
                continue;
            }
            let Some(mc) = m.classes.iter().find(|c| c.id == ic.id) else {
                return Err(format!(
                    "package '{}': interface class {} has no class row",
                    m.feature, ic.id
                ));
            };
            // By QUALIFIED name: an earlier package's nested class carries a
            // leaf name plus a lexical parent (pass 1b), so the raw `name`
            // field no longer spells the manifest's qualified string.
            let taken_at = (0..compiler.classes.len()).find(|&i| {
                let c = &compiler.classes[i];
                if mc.name.contains("::") {
                    (c.name.contains("::") || c.lexical_parent.is_some())
                        && compiler.fq_name(ClassId(i as u32)) == mc.name
                } else {
                    c.name == mc.name && c.lexical_parent.is_none()
                }
            });
            if let Some(taken_idx) = taken_at {
                let taken = &compiler.classes[taken_idx];
                if taken.imported_pkg.is_none() {
                    return Err(format!(
                        "package '{}' defines `{}`, which this program already \
                         has (a builtin); a package reopen of a builtin is \
                         not supported yet",
                        m.feature, mc.name
                    ));
                }
                if taken.is_module != ic.is_module {
                    let kind = |m: bool| if m { "module" } else { "class" };
                    return Err(format!(
                        "package '{}' defines `{}` as a {}, but another merged \
                         package defines it as a {}",
                        m.feature,
                        mc.name,
                        kind(ic.is_module),
                        kind(taken.is_module)
                    ));
                }
                // The ivar LAYOUT is ABI: each package's bodies compiled
                // slot indices from its own list, so the lists must agree
                // (or one side must carry none at all).
                let merge_ivars = |mine: &Vec<String>,
                                   theirs: &mut Vec<String>|
                 -> bool {
                    if theirs.is_empty() {
                        *theirs = mine.clone();
                        true
                    } else {
                        mine.is_empty() || mine == theirs
                    }
                };
                let ci = &mut compiler.classes[taken_idx];
                if !merge_ivars(&mc.ivars, &mut ci.ivars)
                    || !merge_ivars(&ic.hidden_ivars, &mut ci.hidden_ivars)
                {
                    return Err(format!(
                        "package '{}' reopens `{}` with a different instance-\
                         variable layout than another merged package compiled \
                         against; compile one of them from source",
                        m.feature, mc.name
                    ));
                }
                compiler
                    .pkg_class_map
                    .insert((pi as u32, ic.id), ClassId(taken_idx as u32));
                continue;
            }
            let cid = compiler.add_class(mc.name.clone(), None, ic.is_module);
            let ci = &mut compiler.classes[cid.0 as usize];
            ci.imported_pkg = Some(pi as u32);
            ci.ivars = mc.ivars.clone();
            ci.hidden_ivars = ic.hidden_ivars.clone();
            ci.explicit_superclass = ic.parent.is_some() && !ic.is_module;
            compiler.pkg_class_map.insert((pi as u32, ic.id), cid);
        }
        // Pass 1b: wire this package's freshly minted NESTED classes into
        // their lexical scopes (leaf name + `lexical_parent`), so a scoped
        // path in host code resolves (`class ReadTimeout < Timeout::Error`
        // against a merged timeout). Registered under the qualified string
        // alone, `Timeout::Error` was a top-level class named with two
        // colons, and `resolve_class`'s descent could never reach it.
        let by_fq: std::collections::HashMap<&str, ClassId> = m
            .classes
            .iter()
            .filter_map(|c| {
                compiler
                    .pkg_class_map
                    .get(&(pi as u32, c.id))
                    .map(|cid| (c.name.as_str(), *cid))
            })
            .collect();
        for mc in &m.classes {
            let Some(&cid) = compiler.pkg_class_map.get(&(pi as u32, mc.id)) else {
                continue;
            };
            // An ALIASED class belongs to the package that minted it, which
            // already wired it; and the mint's name must still be qualified
            // for this rewrite to apply.
            if compiler.class(cid).imported_pkg != Some(pi as u32)
                || !compiler.class(cid).name.contains("::")
            {
                continue;
            }
            let Some((prefix, leaf)) = mc.name.rsplit_once("::") else {
                continue;
            };
            let Some(parent) = by_fq
                .get(prefix)
                .copied()
                .or_else(|| compiler.resolve_class(prefix, &[], 0))
            else {
                continue;
            };
            let ci = &mut compiler.classes[cid.0 as usize];
            ci.name = leaf.to_string();
            ci.lexical_parent = Some(parent);
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
            if ic.id < m.first_class_id {
                continue; // a builtin reopen wires no ancestry
            }
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
            let owned = compiler.class(cid).imported_pkg == Some(pi as u32);
            if owned {
                let ci = &mut compiler.classes[cid.0 as usize];
                ci.parent = parent;
                ci.mixin_order = mixins;
                ci.extends = extends;
                continue;
            }
            // ALIASED onto an earlier package's class. The ANCESTRY --
            // parent plus mixin order -- must come whole from ONE side: a
            // chain assembled from both would match neither package's
            // compiled class row, and the merged desc keeps exactly one.
            // A bare opening (no `< Super`, no mixins) contributes nothing
            // and merges onto anything; `extend`s union, their runtime
            // rows being additive.
            let bare = |p: &Option<ClassId>, mix: &[(ClassId, bool)]| {
                mix.is_empty()
                    && match p {
                        None => true,
                        Some(c) => *c == crate::compiler::OBJECT_CLASS,
                    }
            };
            let taken_name = compiler.class(cid).name.clone();
            let ci = &mut compiler.classes[cid.0 as usize];
            let same =
                ci.parent == parent && ci.mixin_order == mixins;
            if !same && !bare(&parent, &mixins) {
                if bare(&ci.parent, &ci.mixin_order) {
                    ci.parent = parent;
                    ci.mixin_order = mixins;
                    ci.explicit_superclass = ic.parent.is_some() && !ic.is_module;
                } else {
                    return Err(format!(
                        "package '{}' reopens `{taken_name}` with a different \
                         ancestry (superclass or mixins) than another merged \
                         package; compile one of them from source",
                        m.feature
                    ));
                }
            }
            let ci = &mut compiler.classes[cid.0 as usize];
            for e in extends {
                if !ci.extends.contains(&e) {
                    ci.extends.push(e);
                }
            }
        }
        // Pass 3: the methods, as body-less scopes. The SHAPE numbers come
        // from the manifest -- the package's own `layout_of` verdict -- so
        // the host cannot disagree with the compiled body's ABI.
        for ic in &m.iface {
            // A BUILTIN row registers onto the host's class of the same
            // untranslated id -- a package reopen of a shared class.
            let cid = if ic.id < m.first_class_id {
                ClassId(ic.id)
            } else {
                compiler.pkg_class_map[&(pi as u32, ic.id)]
            };
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
                    let taken_name = compiler.class(cid).name.clone();
                    let aliased = compiler.class(cid).imported_pkg != Some(pi as u32);
                    // A package OPERATOR reopen on a numeric lane stands
                    // down the native fast paths, exactly as the host's own
                    // reopen would at registration.
                    if ic.id < m.first_class_id
                        && !class_side
                        && !im.name.starts_with(|c: char| c.is_alphabetic() || c == '_')
                    {
                        match taken_name.as_str() {
                            "Integer" => {
                                compiler.redefined_int_ops.insert(im.name.clone());
                            }
                            "Float" => {
                                compiler.redefined_float_ops.insert(im.name.clone());
                            }
                            "Numeric" | "Comparable" | "Object" | "Kernel"
                            | "BasicObject" => {
                                compiler.redefined_int_ops.insert(im.name.clone());
                                compiler.redefined_float_ops.insert(im.name.clone());
                            }
                            _ => {}
                        }
                    }
                    let ci = &mut compiler.classes[cid.0 as usize];
                    let (list, index) = if class_side {
                        (&mut ci.own_class_methods, &mut ci.own_class_method_at)
                    } else {
                        (&mut ci.own_methods, &mut ci.own_method_at)
                    };
                    // On an ALIASED class, the two packages' method sets
                    // union like a reopen -- but the SAME name in both is a
                    // static redefinition the earlier package's typed sites
                    // never guarded against. A PRELUDE body (ruby's own,
                    // `native_default`) is not a competitor: reopening it is
                    // an ordinary builtin reopen, and the standing entry
                    // gives way.
                    if aliased && let Some(&at) = index.get(&im.name) {
                        if compiler.scopes[list[at].0 as usize].native_default {
                            list[at] = sid;
                            continue;
                        }
                        let sep = if class_side { "." } else { "#" };
                        return Err(format!(
                            "package '{}' defines `{taken_name}{sep}{}`, which \
                             another merged package also defines; a cross-\
                             package redefinition is not supported yet -- \
                             compile one of them from source",
                            m.feature, im.name
                        ));
                    }
                    index.insert(im.name.clone(), list.len());
                    list.push(sid);
                }
            }
        }
    }
    // Pass 1b RENAMED classes the lazy name index may already hold.
    compiler.reindex_classes();
    Ok(())
}

