//! Decides which of a program's compiled definitions announce themselves.
//!
//! Ruby tells a class what was just defined in it -- `method_added`,
//! `singleton_method_added`, and their `_removed`/`_undefined` siblings -- and
//! the announcement runs at the definition's own position. zeo consumes a
//! class-body `def` at analyze time and emits nothing there, so
//! [`crate::compiler::ClassBodySite::defs`] holds each consumed definition
//! until this pass, which runs after `mro::materialize` (it needs
//! `class_methods` flattened over the ancestry) and splices a
//! [`HirNode::DefHook`] back into the statement list at exactly the index the
//! definition held.
//!
//! The gate is `emit_inherited_hook`'s: emit nothing unless the compiler can
//! SEE a hook body. `Module`'s own rows are no-ops, and `class_method_in_chain`
//! searches user scopes only, so they never satisfy it. A program that defines
//! no hook comes out of this pass byte-identical.

use crate::compiler::FMap;
use crate::compiler::{ClassId, Compiler, DefEvent, SiteDef};
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

/// Where a group of defs splices: the main list, a feature unit's body, or a
/// class-body site's statements. A [`SiteDef::at`] is an index into exactly
/// one of these vectors, and splicing it into any other is the panic the
/// `unit` tag exists to prevent.
enum Target {
    Main,
    Unit(usize),
    Site(usize),
}

pub fn resolve(
    compiler: &mut Compiler,
    main_statements: &mut Vec<NodeId>,
    feature_units: &mut [(String, String, Vec<NodeId>)],
) {
    for hook in global_hooks(compiler) {
        compiler.global_def_hooks.insert(hook.to_string());
    }
    let global: Vec<&'static str> = HOOKS
        .into_iter()
        .filter(|h| compiler.global_def_hooks.contains(*h))
        .collect();

    // Take every class's definitions out first, so the watermark below can see
    // a class's WHOLE program-wide sequence -- a reopen adds methods the hook
    // in the first body has not seen yet. Top-level defs group by STREAM
    // (main vs each unit) -- `future_names` merges the groups back per class,
    // so the watermark still spans the whole program. A BTreeMap keeps the
    // group order deterministic (node ids and patch sets follow it).
    let mut taken: Vec<(ClassId, Vec<SiteDef>, Target)> = Vec::new();
    let mut by_stream: std::collections::BTreeMap<Option<u32>, Vec<SiteDef>> =
        std::collections::BTreeMap::new();
    for d in std::mem::take(&mut compiler.top_level_defs) {
        by_stream.entry(d.unit).or_default().push(d);
    }
    for (stream, defs) in by_stream {
        let target = match stream {
            None => Target::Main,
            Some(k) => Target::Unit(k as usize),
        };
        taken.push((crate::compiler::OBJECT_CLASS, defs, target));
    }
    for i in 0..compiler.class_body_sites.len() {
        let class = compiler.class_body_sites[i].class;
        let defs = std::mem::take(&mut compiler.class_body_sites[i].defs);
        taken.push((class, defs, Target::Site(i)));
    }
    let future = future_names(&taken);

    // A PRELUDE definition (`BUILTIN_EXCEPTIONS_RB`) is ruby's own,
    // installed before the program's first statement runs, so no hook the
    // program writes can have been there to see it. Without this, a
    // `class Module; def method_added(n); end; end` announced
    // `Exception#initialize` and its siblings ahead of everything the user
    // wrote.
    let prelude: std::collections::HashSet<NodeId> = compiler
        .scopes
        .iter()
        .filter(|s| s.native_default)
        .filter_map(|s| s.def_node)
        .collect();
    for (class, defs, target) in taken {
        let sends = surviving(compiler, class, &defs, &global, &future, &prelude);
        if sends.is_empty() {
            continue;
        }
        match target {
            Target::Main => splice(compiler, main_statements, class, sends),
            Target::Unit(k) => {
                let mut stmts = std::mem::take(&mut feature_units[k].2);
                splice(compiler, &mut stmts, class, sends);
                feature_units[k].2 = stmts;
            }
            Target::Site(i) => {
                let mut stmts = std::mem::take(&mut compiler.class_body_sites[i].stmts);
                splice(compiler, &mut stmts, class, sends);
                compiler.class_body_sites[i].stmts = stmts;
            }
        }
    }
}

/// One definition that will announce itself: where to splice it, the hook, the
/// defined name, and the names of `class` not yet defined at that point.
struct Send {
    at: usize,
    hook: &'static str,
    name: String,
    pending: Vec<String>,
}

/// Per definition (keyed by [`SiteDef::seq`]), the names of its own class that
/// are still in the FUTURE when it announces itself.
///
/// Ruby's hook sees a half-built class: at `method_added(:a)`,
/// `instance_methods(false)` is `[:a]` alone. zeo has every method table
/// installed before the program's first statement runs, so the announcement
/// carries this set and the reflection rows subtract it.
///
/// Keyed on FIRST definition: `def x; end; def x; end` announces twice, and at
/// the first announcement `x` already exists, so a later redefinition must not
/// hide it.
fn future_names(taken: &[(ClassId, Vec<SiteDef>, Target)]) -> Future {
    let mut by_class: FMap<ClassId, Vec<&SiteDef>> = FMap::default();
    for (class, defs, _) in taken {
        by_class.entry(*class).or_default().extend(defs.iter());
    }
    let mut out = Future::default();
    for (class, defs) in &mut by_class {
        defs.sort_by_key(|d| d.seq);
        // A class's own instance methods, in the order they come into being.
        // A `def self.x` adds no instance method, and neither does an `undef`.
        let mut order: Vec<String> = Vec::new();
        let mut seen = crate::compiler::FSet::default();
        for d in defs.iter() {
            if d.event == DefEvent::Added && !d.singleton && seen.insert(d.name.as_str()) {
                order.push(d.name.clone());
            }
        }
        let mut installed = 0usize;
        for d in defs.iter() {
            if d.event == DefEvent::Added && !d.singleton {
                installed += 1;
            }
            out.installed.insert(d.seq, installed);
        }
        out.names.insert(*class, order);
    }
    out
}

/// [`future_names`]' answer, stored as a WATERMARK rather than as one name
/// list per definition.
///
/// The names still pending at a definition are always a SUFFIX of its class's
/// list -- a name enters the list exactly when it is first installed, so
/// everything from the install count onwards is what the hook cannot see yet.
/// Materializing that suffix per definition cost a `Vec<String>` and a clone
/// of every name in it for each of a class's definitions, which is quadratic
/// in the class's definition count and thrown away entirely for the common
/// program that defines no hook at all.
#[derive(Default)]
struct Future {
    /// Per class, its own instance-method names in first-definition order.
    names: FMap<ClassId, Vec<String>>,
    /// [`SiteDef::seq`] -> how far into that list has been installed.
    installed: FMap<u32, usize>,
}

impl Future {
    /// The names of `class` that are still in the future at `seq`.
    fn pending(&self, class: ClassId, seq: u32) -> Vec<String> {
        let (Some(names), Some(&at)) = (self.names.get(&class), self.installed.get(&seq)) else {
            return Vec::new();
        };
        names[at.min(names.len())..].to_vec()
    }
}

fn surviving(
    compiler: &Compiler,
    class: ClassId,
    defs: &[SiteDef],
    global: &[&'static str],
    future: &Future,
    prelude: &std::collections::HashSet<NodeId>,
) -> Vec<Send> {
    defs.iter()
        .filter(|d| !prelude.contains(&d.node))
        .filter_map(|d| {
            let hook = d.event.hook(d.singleton);
            fires(compiler, class, d, hook, global).then(|| Send {
                at: d.at,
                hook,
                name: d.name.clone(),
                pending: future.pending(class, d.seq),
            })
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
    let (installed, defined) = (
        where_(compiler.scope(scope).def_node),
        where_(Some(def.node)),
    );
    // A `BEGIN { ... }` body runs before the whole main program, so written
    // order is not run order when exactly one of the two sits inside one:
    // a hook installed in the main program never saw a definition hoisted out
    // of a `BEGIN`, and a hook installed INSIDE one sees every main
    // definition however early it is written.
    let hoisted = |s: &crate::hir::Span| {
        compiler
            .pre_exec_spans
            .iter()
            .any(|p| p.file == s.file && p.start <= s.start && s.end <= p.end)
    };
    match (installed, defined) {
        (Some(i), Some(d)) if i.file == d.file && hoisted(&i) != hoisted(&d) => hoisted(&i),
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
    for send in sends.into_iter().rev() {
        compiler.runtime_patches.insert(send.name.clone());
        let node = compiler.hir.push(HirNode::DefHook {
            class: class.0,
            hook: send.hook.to_string(),
            name: send.name,
            pending: send.pending,
        });
        stmts.insert(send.at, node);
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

/// Whether the hook body `hook` was already installed at position `at`.
///
/// A hook INSTALLED after the thing it would report never saw it. minitest
/// reopens `Runnable` at the very end of its main file purely to add
/// `inherited`, so that the `Test`/`Result` subclasses defined above stay out
/// of the runnables registry -- fire it for them and `Result`, which implements
/// no `runnable_methods`, joins the run and raises.
///
/// Compared by SPAN, and only within one file: `doc_order` numbers class-body
/// statements, and a `def` is not one of those (it is hoisted into the class's
/// method table). Two positions in different files are left alone -- a spliced
/// `require` puts another file's statements in the middle of this one, so raw
/// offsets do not order across files -- as is anything span-less. All of those
/// keep firing.
pub(crate) fn hook_installed_before(
    compiler: &Compiler,
    hook: crate::compiler::ScopeId,
    at: Option<crate::hir::NodeId>,
) -> bool {
    let where_ = |n: Option<crate::hir::NodeId>| {
        n.and_then(|n| compiler.hir.span(n)).and_then(|s| s.known())
    };
    match (where_(compiler.scope(hook).def_node), where_(at)) {
        (Some(installed), Some(at)) if installed.file == at.file => installed.start <= at.start,
        _ => true,
    }
}
