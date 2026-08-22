//! Decides which method REDEFINITIONS must be applied positionally.
//!
//! Ruby installs each `def` where it stands: code running between two
//! same-name `def`s -- a call before a reopen, a `method_added` hook body --
//! dispatches to the FIRST body. zeo's static tables carry only the
//! last-`def`-wins winner, so that window used to see the final body.
//!
//! This pass keeps every compile-time fact on the final body (the live
//! `own_methods` row is untouched, so super inlining and materialization
//! never move) and puts the TIMELINE in the runtime overlay instead: the
//! first body is installed at boot ([`crate::compiler::Compiler::positional_redefs`]),
//! and each later redefinition re-installs at its own document position via a
//! spliced [`HirNode::MethodRedefine`]. The name joins `runtime_patches`, so
//! every call site dispatches dynamically and the overlay decides.
//!
//! Only an OBSERVABLE timeline pays: the redefinition must cross class-body
//! sites (a reopen -- code can run between them), have statements between the
//! two `def`s in one body, or race a `method_added` hook. The common
//! unobservable shape -- `attr_accessor :x` immediately overridden by
//! `def x` in the same body -- stays fully static.
//!
//! Must run after `mro::materialize` (hook detection needs `class_methods`
//! flattened) and before `def_hooks::resolve`, whose spliced hook for a
//! redefinition has to land AFTER the install at the same position -- the
//! `at` bumps here keep that ordering.

use crate::compiler::{ClassId, Compiler, DefEvent, ScopeId};
use crate::hir::HirNode;

pub fn resolve(compiler: &mut Compiler) {
    let global_method_added = [crate::compiler::MODULE_CLASS, crate::compiler::CLASS_CLASS]
        .iter()
        .any(|&o| compiler.method_in_chain(o, "method_added").is_some());

    // (class, name) pairs with more than one Added instance-method row.
    let mut candidates: Vec<(ClassId, String)> = Vec::new();
    for (idx, ci) in compiler.classes.iter().enumerate() {
        let cid = ClassId(idx as u32);
        // Plain generated-struct classes only -- the one registration shape
        // whose methods are reachable as inherent fns for a trampoline.
        if idx == 0
            || ci.is_builtin
            || ci.is_bootstrap
            || compiler.is_exception_backed(cid)
            || compiler.is_value_subclass(cid)
            || compiler.is_immediate_subclass(cid)
        {
            continue;
        }
        // Counted in one pass rather than re-filtering the whole history per
        // entry: with both the dedup scan and the count scan linear in the
        // history, finding a class's redefined names cost a pass per
        // definition it had.
        let mut counts: crate::compiler::FMap<&str, usize> = Default::default();
        for (name, is_class_method, _, _) in &ci.method_history {
            if !*is_class_method {
                *counts.entry(name.as_str()).or_default() += 1;
            }
        }
        // Still walked in history order, so `candidates` keeps the order the
        // scan produced -- the node ids and patch sets downstream follow it.
        let mut seen: crate::compiler::FSet<&str> = Default::default();
        for (name, is_class_method, _, _) in &ci.method_history {
            if *is_class_method || !seen.insert(name.as_str()) {
                continue;
            }
            if counts[name.as_str()] >= 2 {
                candidates.push((cid, name.clone()));
            }
        }
    }

    // Which class-body sites belong to each class, so the per-candidate search
    // below reads a short list instead of every site in the program.
    let mut sites_by_class: crate::compiler::FMap<ClassId, Vec<usize>> = Default::default();
    for (si, site) in compiler.class_body_sites.iter().enumerate() {
        sites_by_class.entry(site.class).or_default().push(si);
    }

    for (cid, name) in candidates {
        // The bodies, oldest first. `compiler.scopes` is append-only, so
        // every superseded row is still there to emit.
        let mut rows: Vec<(u32, ScopeId)> = compiler.classes[cid.0 as usize]
            .method_history
            .iter()
            .filter(|(m, s, _, _)| !s && *m == name)
            .map(|&(_, _, seq, sid)| (seq, sid))
            .collect();
        rows.sort_by_key(|&(seq, _)| seq);

        // The definitions' document positions, matched to the history rows
        // by execution order (`SiteDef::seq` and the history counter are the
        // same walk). A mismatch means some body has no top-level site record
        // (a `def` inside an `if` branch) -- leave that name fully static.
        let mut defs: Vec<(usize, usize, u32)> = Vec::new(); // (site, def_idx, seq)
        for &si in sites_by_class.get(&cid).map_or(&[][..], Vec::as_slice) {
            let site = &compiler.class_body_sites[si];
            for (di, d) in site.defs.iter().enumerate() {
                if d.event == DefEvent::Added && !d.singleton && d.name == name {
                    defs.push((si, di, d.seq));
                }
            }
        }
        defs.sort_by_key(|&(_, _, seq)| seq);
        if defs.len() != rows.len() {
            continue;
        }

        let observable = {
            let multi_site = defs.iter().any(|&(si, ..)| si != defs[0].0);
            let stmts_between = {
                let at =
                    |&(si, di, _): &(usize, usize, u32)| compiler.class_body_sites[si].defs[di].at;
                at(defs.last().unwrap()) != at(&defs[0])
            };
            let hooked = global_method_added
                || compiler
                    .class_method_in_chain(cid, "method_added")
                    .is_some();
            multi_site || stmts_between || hooked
        };
        if !observable {
            continue;
        }

        compiler.runtime_patches.insert(name.clone());
        compiler
            .positional_redefs
            .push((cid, name.clone(), rows[0].1));
        // Every body, the final one included: even the last redefinition
        // re-installs at its position, and all installs go through the
        // receiver-generic free-function emission.
        for &(_, sid) in &rows {
            let list = &mut compiler.classes[cid.0 as usize].redef_scopes;
            if !list.contains(&sid) {
                list.push(sid);
            }
        }

        // Splice one install per LATER body at its definition's position.
        // Bump every later-or-equal `at` in the site so `def_hooks`' splice
        // -- which reads these same records afterwards -- puts the
        // redefinition's own hook AFTER its install.
        for k in 1..rows.len() {
            let (si, di, _) = defs[k];
            let (def_at, def_seq) = {
                let d = &compiler.class_body_sites[si].defs[di];
                (d.at, d.seq)
            };
            let node = compiler.hir.push(HirNode::MethodRedefine {
                class: cid.0,
                name: name.clone(),
                scope: rows[k].1.0,
            });
            compiler.class_body_sites[si].stmts.insert(def_at, node);
            for d in &mut compiler.class_body_sites[si].defs {
                if d.at > def_at || (d.at == def_at && d.seq >= def_seq) {
                    d.at += 1;
                }
            }
        }
    }
}
