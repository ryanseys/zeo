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

use crate::compiler::{ClassId, Compiler, DefEvent, SiteDef};
use crate::compiler::{FMap, FSet};
use crate::hir::{HirNode, NodeId, Span};

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

/// Which feature units a file's own `require`s have already run by a given
/// point in it -- the fact that lets a hook in ANOTHER file be ordered against
/// a definition at all.
///
/// A unit's body runs when its `require` runs, so "installed first" cannot be
/// read off raw offsets across two files, and without this graph [`fires`]
/// refuses every such pairing. But a file that requires another AT ITS TOP
/// LEVEL, ABOVE the definition, has run that file whole by the time the
/// definition executes --
/// whenever, and however often, this file itself runs. That is a fact rather
/// than an ordering guess, and it is what bundler needs: `bundler/cli.rb`
/// opens with `require_relative "vendored_thor"` and declares `class CLI <
/// Thor` below it, so Thor's `method_added` really has seen every command.
///
/// DELIBERATELY INCOMPLETE, always in the direction of refusing. Only a bare
/// top-level `require`/`require_relative` with a literal name counts: one
/// under a guard, in a `begin`/`rescue`, in a class body or in a method body
/// may never run, and a method-body `require` is exactly the shape that
/// breaks `require "rake"` when trusted (see [`global_hooks`]). Anything this
/// graph cannot prove is refused.
#[derive(Default)]
struct RequireGraph {
    /// Per FILE (`Span::file`), its top-level requires as
    /// `(byte offset, unit)`, in no particular order.
    first_hop: FMap<u32, Vec<(u32, usize)>>,
    /// Per unit, every unit its body loads, transitively. Position-free on
    /// purpose: once a `require` runs, the file it names runs whole.
    loads: Vec<FSet<usize>>,
    /// The file each unit was compiled from, for walking `first_hop` during
    /// the closure.
    unit_file: Vec<Option<u32>>,
}

impl RequireGraph {
    /// Whether unit `hook` has certainly run by the time the definition at
    /// `def_at` does.
    fn runs_before(&self, hook: usize, def_at: Span) -> bool {
        let Some(hops) = self.first_hop.get(&def_at.file.0) else {
            return false;
        };
        hops.iter().any(|&(at, target)| {
            at < def_at.start && (target == hook || self.loads[target].contains(&hook))
        })
    }
}

/// Reads the top-level statement streams -- the main list and every unit body
/// -- and builds [`RequireGraph`].
fn require_graph(
    compiler: &Compiler,
    main_statements: &[NodeId],
    feature_units: &[(Vec<String>, String, Vec<NodeId>)],
) -> RequireGraph {
    // Every spelling a `require` can reach a unit under: its own features and
    // aliases, plus the absolute path `require_relative` resolves to.
    let mut by_name: FMap<&str, usize> = FMap::default();
    let mut by_path: FMap<String, usize> = FMap::default();
    for (k, (names, absolute, _)) in feature_units.iter().enumerate() {
        for n in names {
            by_name.entry(n.as_str()).or_insert(k);
        }
        by_path.entry(absolute.clone()).or_insert(k);
    }
    let mut g = RequireGraph {
        unit_file: vec![None; feature_units.len()],
        loads: vec![FSet::default(); feature_units.len()],
        ..Default::default()
    };
    // Per unit, the targets its whole body names -- the closure's edges,
    // where `first_hop` is the per-file, position-carrying view.
    let mut body_edges: Vec<Vec<usize>> = vec![Vec::new(); feature_units.len()];
    let streams = std::iter::once((None, main_statements)).chain(
        feature_units
            .iter()
            .enumerate()
            .map(|(k, u)| (Some(k), u.2.as_slice())),
    );
    for (unit, stmts) in streams {
        for &stmt in stmts {
            let Some(span) = compiler.hir.span(stmt) else {
                continue;
            };
            let Some(target) = require_target(compiler, stmt, span, &by_name, &by_path) else {
                continue;
            };
            g.first_hop
                .entry(span.file.0)
                .or_default()
                .push((span.start, target));
            if let Some(k) = unit {
                body_edges[k].push(target);
                g.unit_file[k].get_or_insert(span.file.0);
            }
        }
    }
    for k in 0..feature_units.len() {
        let mut stack = body_edges[k].clone();
        while let Some(t) = stack.pop() {
            if g.loads[k].insert(t) {
                stack.extend_from_slice(&body_edges[t]);
            }
        }
    }
    g
}

/// The unit a top-level statement `require`s, when the statement is exactly a
/// bare `require`/`require_relative` of a literal name.
fn require_target(
    compiler: &Compiler,
    stmt: NodeId,
    span: Span,
    by_name: &FMap<&str, usize>,
    by_path: &FMap<String, usize>,
) -> Option<usize> {
    let HirNode::Call {
        receiver: None,
        name,
        args,
        block: None,
        block_arg: None,
        ..
    } = &compiler.hir[stmt]
    else {
        return None;
    };
    let relative = match name.as_str() {
        "require" => false,
        "require_relative" => true,
        _ => return None,
    };
    let [arg] = args.as_slice() else { return None };
    let crate::hir::ArrayElem::Single(text) = arg else {
        return None;
    };
    let feature = crate::lower::eval_splice::literal_string_text(&compiler.hir, *text)?;
    if !relative && let Some(&k) = by_name.get(feature.as_str()) {
        return Some(k);
    }
    // A KEPT `require_relative` carries an ABSOLUTE argument: lowering rewrites
    // it, because the call resolves at run time against the frame's file and a
    // unit's body has no frame of its own (`lower::calls`' `site_kept` arm).
    // The written-relative spelling still has to work, and it resolves against
    // the file the statement was WRITTEN in -- the one its span names, not the
    // unit whose body it now sits in, since a splice puts one file's
    // statements inside another's.
    let target = feature.trim_end_matches(".rb");
    let path = match std::path::Path::new(target).is_absolute() {
        true => target.to_string(),
        false => {
            let name = &compiler.hir.files.get(span.file.0 as usize)?.name;
            lexical_join(std::path::Path::new(name).parent()?, target)
        }
    };
    by_path.get(&path).copied()
}

/// `dir` + `rel` with `.` and `..` resolved by TEXT. The real path may not
/// exist on this machine at all (a unit is compiled in), and canonicalizing
/// would ask the filesystem a question the answer must not depend on.
fn lexical_join(dir: &std::path::Path, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.to_str().unwrap_or_default().split('/').collect();
    for c in rel.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(c),
        }
    }
    parts.join("/")
}

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
    feature_units: &mut [(Vec<String>, String, Vec<NodeId>)],
) {
    // A unit's body runs when its `require` runs, not at boot, so a hook it
    // installs has not seen anything the main program defined first -- unless
    // the defining file required it above the definition, which is what
    // `RequireGraph` decides.
    let mut file_ids: FMap<&str, u32> = FMap::default();
    for (i, f) in compiler.hir.files.iter().enumerate() {
        file_ids.entry(f.name.as_str()).or_insert(i as u32);
    }
    let unit_of_file: FMap<u32, usize> = feature_units
        .iter()
        .enumerate()
        .filter_map(|(k, (_, absolute, _))| {
            file_ids
                .get(format!("{absolute}.rb").as_str())
                .map(|&f| (f, k))
        })
        .collect();
    let graph = require_graph(compiler, main_statements, feature_units);
    for hook in global_hooks(compiler, &unit_of_file) {
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
    // Cloned, not taken: codegen still reads a site's defs to work out which
    // builtin-alias rows the site itself wrote (`site_alias_checks`).
    for i in 0..compiler.class_body_sites.len() {
        let class = compiler.class_body_sites[i].class;
        let defs = compiler.class_body_sites[i].defs.clone();
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
        .filter_map(|s| s.def_node)
        .filter(|&n| {
            compiler
                .scopes
                .iter()
                .any(|s| s.def_node == Some(n) && s.native_default)
        })
        .collect();
    for (class, defs, target) in taken {
        let sends = surviving(
            compiler,
            class,
            &defs,
            &global,
            &future,
            &prelude,
            &graph,
            &unit_of_file,
        );
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
    /// The hook body is not in THIS compile: a package build announces
    /// through the run-time probe, which fires only when the linking
    /// program carries a body. False = an unconditional send.
    probe: bool,
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

#[allow(clippy::too_many_arguments)] // one lowering fact per parameter
fn surviving(
    compiler: &Compiler,
    class: ClassId,
    defs: &[SiteDef],
    global: &[&'static str],
    future: &Future,
    prelude: &std::collections::HashSet<NodeId>,
    graph: &RequireGraph,
    unit_of_file: &FMap<u32, usize>,
) -> Vec<Send> {
    defs.iter()
        .filter(|d| !prelude.contains(&d.node))
        .filter_map(|d| {
            let hook = d.event.hook(d.singleton);
            if fires(compiler, class, d, hook, global, graph, unit_of_file) {
                return Some(Send {
                    at: d.at,
                    hook,
                    name: d.name.clone(),
                    pending: future.pending(class, d.seq),
                    probe: false,
                });
            }
            // A package build's world is open: the program it links into
            // may carry the hook this compile does not, so every remaining
            // definition announces through the run-time probe instead.
            if compiler.hir.pkg_build.is_some() {
                return Some(Send {
                    at: d.at,
                    hook,
                    name: d.name.clone(),
                    pending: future.pending(class, d.seq),
                    probe: true,
                });
            }
            None
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
    graph: &RequireGraph,
    unit_of_file: &FMap<u32, usize>,
) -> bool {
    if global.contains(&hook) {
        return true;
    }
    // The same question `emit_inherited_hook` asks, over the same table: a
    // `def self.method_added` here or anywhere up the superclass chain, or one
    // an `extend`ed module supplied. `Module`'s no-op default is an INSTANCE
    // method of Module and never appears here, so it costs nothing.
    if compiler.class_method_in_chain(class, hook).is_none() {
        return false;
    }
    // A hook INSTALLED after this definition never saw it -- ruby's own rule,
    // and `emit_inherited_hook`'s. Compared by span, and only within one file:
    // a spliced `require` puts another file's statements in the middle of this
    // one, so raw offsets do not order across files. `<=`, not `<`, because a
    // `def self.singleton_method_added` DOES report itself (oracle-verified).
    // The install position is the `extend` when a module supplied the hook,
    // which is why this asks `class_method_install_node` rather than reading
    // the winning body's own `def` -- see that method.
    let where_ = |n: Option<NodeId>| n.and_then(|n| compiler.hir.span(n)).and_then(|s| s.known());
    let (installed, defined) = (
        where_(compiler.class_method_install_node(class, hook)),
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
        // A hook in a FEATURE UNIT is installed by a runtime `require`, so a
        // definition in another file cannot be ordered against it by offset:
        // the unit body may not have run yet, and zeo's own emission agrees --
        // a unit's reopen of a builtin forwards to the native row until then.
        // It CAN be ordered when the defining file required that unit above
        // the definition, which is what `RequireGraph` proves. A hook the main
        // program installs is there before any unit runs, so it keeps firing.
        _ => match installed.and_then(|i| unit_of_file.get(&i.file.0)) {
            None => true,
            Some(&u) => defined.is_some_and(|d| graph.runs_before(u, d)),
        },
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
        // A PROBE does not dynamize the name: if the linking program's hook
        // redefines it at run time, the install patches the class and every
        // guarded site deopts -- the same contract every runtime definition
        // rides. An unconditional send's hook is in THIS compile, and its
        // folds must lose statically.
        if !send.probe {
            compiler.runtime_patches.insert(send.name.clone());
        }
        let node = compiler.hir.push(HirNode::DefHook {
            class: class.0,
            hook: send.hook.to_string(),
            name: send.name,
            pending: send.pending,
            probe: send.probe,
        });
        stmts.insert(send.at, node);
    }
}

/// The hook names defined directly on `Module`/`Class`/`BasicObject`. Such a
/// definition applies to EVERY class, and cannot be found by the per-class
/// scan: the reopen registers an ordinary instance method whose owner is the
/// very class the no-op default lives on.
///
/// A definition in a FEATURE UNIT does not count. A unit body runs when its
/// `require` runs, so a hook it installs never saw what the main program
/// defined before that -- and zeo's own emission agrees, since a unit's reopen
/// of a builtin is guarded and forwards to the native row until the unit runs.
/// Counting one made every `def` in the program announce to a hook that was
/// not there: rake's `--debugger` option carries a method-body
/// `require "debug/session"`, whose `class ::Module; undef method_added; def
/// method_added mid; end` is exactly this shape, and `require "rake"` died in
/// `fileutils`'s module body with `undefined method 'method_added'`.
fn global_hooks(compiler: &Compiler, unit_of_file: &FMap<u32, usize>) -> Vec<&'static str> {
    HOOKS
        .into_iter()
        .filter(|hook| {
            let owners: &[ClassId] = if hook.starts_with("singleton_") {
                &[crate::compiler::BASIC_OBJECT_CLASS]
            } else {
                &[crate::compiler::MODULE_CLASS, crate::compiler::CLASS_CLASS]
            };
            owners.iter().any(|&o| {
                compiler
                    .method_in_chain(o, hook)
                    .is_some_and(|(_, scope)| !defined_in_a_unit(compiler, scope, unit_of_file))
            })
        })
        .collect()
}

/// Whether `scope`'s body was written in a file that compiles to a feature
/// unit -- so it is installed by a runtime `require`, not at boot.
fn defined_in_a_unit(
    compiler: &Compiler,
    scope: crate::compiler::ScopeId,
    unit_of_file: &FMap<u32, usize>,
) -> bool {
    compiler.scope(scope).def_node.is_some_and(|n| {
        compiler
            .hir
            .span(n)
            .is_some_and(|s| unit_of_file.contains_key(&s.file.0))
    })
}

/// Whether `class`'s `hook` exists AND was already installed at position `at`.
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
/// keep firing. [`fires`] asks the same question about a definition and
/// refuses instead where this one allows, because a `method_added` it emits
/// wrongly announces a method that plainly exists, where a missing
/// `inherited`/`const_added` only stays quiet.
pub(crate) fn hook_answers(
    compiler: &Compiler,
    class: ClassId,
    hook: &str,
    at: Option<crate::hir::NodeId>,
) -> bool {
    if compiler.class_method_in_chain(class, hook).is_none() {
        return false;
    }
    let where_ = |n: Option<crate::hir::NodeId>| {
        n.and_then(|n| compiler.hir.span(n)).and_then(|s| s.known())
    };
    // The `extend` site, not the module's own `def` -- see
    // `Compiler::class_method_install_node`.
    let installed = where_(compiler.class_method_install_node(class, hook));
    match (installed, where_(at)) {
        (Some(installed), Some(at)) if installed.file == at.file => installed.start <= at.start,
        _ => true,
    }
}
