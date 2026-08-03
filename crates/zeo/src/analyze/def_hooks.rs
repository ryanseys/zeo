//! Decides which of a program's compiled definitions announce themselves.
//!
//! Ruby tells a class what was just defined in it -- `method_added`,
//! `singleton_method_added`, and their `_removed`/`_undefined` siblings -- and
//! the announcement runs at the definition's own position. zeo consumes a
//! class-body `def` at analyze time and emits nothing there, so
//! [`crate::compiler::ClassBodySite::defs`] holds each consumed definition
//! until this pass, which runs after `mro::materialize` (it needs
//! `class_methods` flattened over the ancestry) and splices a
//! [`HirNode::DefHook`] back into the statement list for the ones that speak.
//!
//! The gate is the one `emit_inherited_hook` already uses: emit nothing unless
//! the compiler can SEE a hook body. `Module`'s own rows are no-ops, and
//! `class_method_in_chain` searches user scopes only, so they never satisfy it.
//! A program that defines no hook comes out of this pass byte-identical.

use crate::compiler::{ClassId, Compiler, SiteDef};
use crate::hir::{HirNode, NodeId};

/// The seven names. A definition of one of these ON `Module`/`Class` (the
/// `method_*` trio and `const_added`) or on `BasicObject` (the
/// `singleton_method_*` trio) applies to every class in the program, which no
/// per-class scan can see.
const HOOKS: [&str; 7] = [
    "method_added",
    "method_removed",
    "method_undefined",
    "const_added",
    "singleton_method_added",
    "singleton_method_removed",
    "singleton_method_undefined",
];

pub fn resolve(compiler: &mut Compiler, main_statements: &mut Vec<NodeId>) {
    let global = global_hooks(compiler);
    if !global.is_empty() {
        for hook in &global {
            compiler.global_def_hooks.insert((*hook).to_string());
        }
    }

    // Top level first: its definitions land on `Object`, whose hook every
    // class inherits, so this is also the widest gate in the program.
    let top = std::mem::take(&mut compiler.top_level_defs);
    let sends = surviving(compiler, crate::compiler::OBJECT_CLASS, &top, &global);
    splice(compiler, main_statements, crate::compiler::OBJECT_CLASS, sends);

    for i in 0..compiler.class_body_sites.len() {
        let defs = std::mem::take(&mut compiler.class_body_sites[i].defs);
        let class = compiler.class_body_sites[i].class;
        let sends = surviving(compiler, class, &defs, &global);
        if sends.is_empty() {
            continue;
        }
        let mut stmts = std::mem::take(&mut compiler.class_body_sites[i].stmts);
        splice(compiler, &mut stmts, class, sends);
        compiler.class_body_sites[i].stmts = stmts;
    }
}

/// A `(at, hook, name)` per definition that will announce itself.
type Send = (usize, &'static str, String);

fn surviving(
    compiler: &Compiler,
    class: ClassId,
    defs: &[SiteDef],
    global: &[&'static str],
) -> Vec<Send> {
    defs.iter()
        .filter_map(|d| {
            let hook = d.event.hook(d.singleton);
            fires(compiler, class, d, hook, global).then(|| (d.at, hook, d.name.clone()))
        })
        .collect()
}

/// Whether a body the user wrote will answer this definition's hook.
fn fires(
    compiler: &Compiler,
    class: ClassId,
    def: &SiteDef,
    hook: &str,
    global: &[&'static str],
) -> bool {
    if global.contains(&hook) {
        return true;
    }
    // The same question `emit_inherited_hook` asks, over the same table: a
    // `def self.method_added` here or anywhere up the superclass chain, or one
    // an `extend`ed module supplied. `Module`'s no-op default is an INSTANCE
    // method of Module and never appears here, so it costs nothing.
    let Some((_, scope)) = compiler.class_method_in_chain(class, hook) else {
        return false;
    };
    // A hook INSTALLED after this definition never saw it -- ruby's own rule,
    // and `emit_inherited_hook`'s. Compared by span, and only within one file:
    // a spliced `require` puts another file's statements in the middle of this
    // one, so raw offsets do not order across files. `<=`, not `<`, because a
    // `def self.singleton_method_added` DOES report itself (oracle-verified).
    let where_ = |n: Option<NodeId>| n.and_then(|n| compiler.hir.span(n)).and_then(|s| s.known());
    let (installed, defined) = (where_(compiler.scope(scope).def_node), where_(Some(def.node)));
    match (installed, defined) {
        (Some(i), Some(d)) if i.file == d.file => i.start <= d.start,
        _ => true,
    }
}

/// Splices one `DefHook` node per surviving report into `stmts`, back to front
/// so the earlier recorded indices stay valid.
///
/// Also records every announced name in [`Compiler::runtime_patches`]. A hook
/// body's whole purpose is often to redefine the method it was just told about
/// (`define_method(name) { ... }` wrapping it), and a direct call folded at
/// compile time would keep reaching the original. Only the names a hook is
/// actually told about lose their fold, and only in a program that has one.
fn splice(compiler: &mut Compiler, stmts: &mut Vec<NodeId>, class: ClassId, sends: Vec<Send>) {
    for (at, hook, name) in sends.into_iter().rev() {
        compiler.runtime_patches.insert(name.clone());
        let node = compiler.hir.push(HirNode::DefHook {
            class: class.0,
            hook: hook.to_string(),
            name,
        });
        stmts.insert(at, node);
    }
}

/// The hook names defined directly on `Module`/`Class`/`BasicObject`. Such a
/// definition applies to EVERY class, and cannot be found by the per-class
/// scan: the reopen registers an ordinary instance method whose owner is the
/// very class the no-op default lives on.
fn global_hooks(compiler: &Compiler) -> Vec<&'static str> {
    HOOKS
        .into_iter()
        .filter(|hook| {
            let owners: &[ClassId] = if hook.starts_with("singleton_") {
                &[crate::compiler::BASIC_OBJECT_CLASS]
            } else {
                &[crate::compiler::MODULE_CLASS, crate::compiler::CLASS_CLASS]
            };
            owners
                .iter()
                .any(|&o| compiler.method_in_chain(o, hook).is_some())
        })
        .collect()
}
