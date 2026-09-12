//! A method a BUILTIN reopen ADDS is not there until its `def`'s own line.
//!
//! ```text
//! p Dir.respond_to?(:mktmpdir)   # false -- tmpdir.rb has not run
//! require "tmpdir"
//! p Dir.respond_to?(:mktmpdir)   # true
//! ```
//!
//! zeo registers a compiled `def` at program START, so the row answered every
//! probe from boot. The CALL already refuses above the line -- a builtin
//! reopen installs under a positional flag (`collect_reopen_flags`) -- but the
//! flag is a compile-side byte the run time cannot read, so reflection and
//! dispatch disagreed.
//!
//! The row is CONCEALED instead, on the machinery a swept unit's methods use
//! ([`crate::clif::classes`]' conceal rows and `dispatch::concealed`), with a
//! reveal group spliced at the definition's document position. A concealed
//! name never terminates a lookup, so the walk falls through to the ancestors
//! -- which is what ruby does with a method not yet defined.
//!
//! Only a name the builtin does NOT already answer is held back. A reopen
//! that REPLACES a native row (`class String; def upcase`) leaves the name
//! answering natively above the line, which is ruby's order and is what the
//! positional flag already does.
//!
//! Must run after `redefs::resolve` and beside [`super::alias_reveals`]: all
//! three splice into a site's statements and bump the `at` of every later
//! `def`, and they share one reveal-group number space.

use crate::compiler::{ClassId, Compiler, DefEvent};
use crate::hir::HirNode;

pub fn resolve(compiler: &mut Compiler) {
    // Past the units' groups AND past every group `alias_reveals` minted:
    // both index one table at run time (`dispatch::concealed`'s `REVEALED`).
    let mut group = compiler.hir.loader.feature_units.len() as u32;
    for &(_, _, _, g) in &compiler.positional_reveals {
        group = group.max(g + 1);
    }

    // Sorted, because the group numbers minted below reach the emitted code:
    // an unordered walk gives one program two different objects across two
    // compiles.
    let mut names: Vec<(ClassId, String, bool)> = Vec::new();
    for site in &compiler.class_body_sites {
        let ci = compiler.class(site.class);
        // A per-box OVERLAY registers on the root builtin's entry, where a
        // conceal would hide the root's own row from every other box too.
        if !ci.is_builtin || ci.builtin_overlay.is_some() {
            continue;
        }
        names.extend(
            site.defs
                .iter()
                .filter(|d| d.event == DefEvent::Added)
                .map(|d| (site.class, d.name.clone(), d.singleton)),
        );
    }
    names.sort_by(|a, b| (a.0.0, &a.1, a.2).cmp(&(b.0.0, &b.1, b.2)));
    names.dedup();

    for (cid, name, singleton) in names {
        if !name_is_new_to_the_builtin(compiler, cid, &name, singleton) {
            continue;
        }
        let Some((si, def_at, def_seq)) = first_position(compiler, cid, &name, singleton) else {
            continue;
        };
        let g = group;
        group += 1;
        compiler
            .positional_reveals
            .push((cid, name.clone(), singleton, g));

        let node = compiler.hir.push(HirNode::MethodReveal(g));
        compiler.class_body_sites[si].stmts.insert(def_at, node);
        for d in &mut compiler.class_body_sites[si].defs {
            if d.at > def_at || (d.at == def_at && d.seq >= def_seq) {
                d.at += 1;
            }
        }
        tracing::debug!(class = cid.0, %name, singleton, "builtin row waits for its line");
    }
}

/// Whether ruby's own `cid` answers `name` already. Only a definitive absence
/// holds a row back: a class the builtin projection has not migrated has no
/// list to be missing from, so "not found" there would be a fact about zeo's
/// build rather than about ruby.
fn name_is_new_to_the_builtin(
    compiler: &Compiler,
    cid: ClassId,
    name: &str,
    singleton: bool,
) -> bool {
    // A GATED row arrives with the require that arms it, so ruby has no such
    // method above that line either -- `Dir.mktmpdir` is both a gated native
    // row and a `def` the spliced tmpdir.rb writes, and the compiled one is
    // what answers a probe.
    if !singleton {
        if crate::builtin_surface::instance_method_is_gated(cid, name) {
            return true;
        }
        return crate::guard_fold::builtin_provides_instance_method(compiler, cid, name)
            == Some(false);
    }
    if crate::builtin_surface::class_method_is_gated(cid, name) {
        return true;
    }
    if crate::builtin_surface::provides_class_method(cid, name) {
        return false;
    }
    // A class method also comes from the singleton chain -- every instance
    // method of `Class` (of `Module` for a module) is one, `name` and `new`
    // among them.
    let meta = match compiler.class(cid).is_module {
        true => crate::compiler::MODULE_CLASS,
        false => crate::compiler::CLASS_CLASS,
    };
    crate::guard_fold::builtin_provides_instance_method(compiler, meta, name) == Some(false)
}

/// The site and statement index of the one body of `name`, or `None` when the
/// name is not safe to hold back.
///
/// The body must have a site record: one nested in an `if` branch registers in
/// the history and leaves no position at all. `dyn_defs` refuses the same
/// mismatch for the same reason.
fn first_position(
    compiler: &Compiler,
    cid: ClassId,
    name: &str,
    singleton: bool,
) -> Option<(usize, usize, u32)> {
    let mut sited: Vec<(u32, usize, usize)> = Vec::new();
    for (si, site) in compiler.class_body_sites.iter().enumerate() {
        if site.class != cid {
            continue;
        }
        for d in &site.defs {
            if d.event == DefEvent::Added && d.singleton == singleton && d.name == name {
                sited.push((d.seq, si, d.at));
            }
        }
    }
    let bodies: Vec<crate::compiler::ScopeId> = compiler.classes[cid.0 as usize]
        .method_history
        .iter()
        .filter(|&&(ref m, s, _, _)| s == singleton && m == name)
        .map(|&(_, _, _, sid)| sid)
        .collect();
    // ONE body only. A name the program defines TWICE already has a positional
    // timeline of its own -- `redefs` installs the first body before the first
    // statement runs and re-installs each later one at its line -- and holding
    // the row back cuts across it: `module Kernel; def helper = :first` read
    // `:second` on every call above the second `def`.
    if sited.len() != 1 || bodies.len() != 1 {
        return None;
    }
    // A body a UNIT wrote is concealed by the unit's own rows already
    // (`conceal_unit_methods`), and one under a guard zeo cannot decide is
    // registered but not promised -- neither is this pass's to place.
    if bodies.iter().any(|&sid| {
        let sc = compiler.scope(sid);
        sc.unit.is_some() || sc.runtime_conditional
    }) {
        return None;
    }
    let (seq, si, at) = sited[0];
    Some((si, at, seq))
}
