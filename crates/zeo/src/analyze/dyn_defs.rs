//! A `def` in a class body that also installs methods AT RUN TIME.
//!
//! Ruby installs each `def` where it stands, so a `def x` written below an
//! `attr_accessor :x` that only a running block knows the name of wins:
//!
//! ```text
//! class C
//!   [:x].each { |n| attr_accessor(n) }   # installs when the body runs
//!   def x = "the def"                    # installs HERE, and wins
//! end
//! ```
//!
//! zeo registers a compiled `def` at program START and the block's install
//! lands afterwards, so the accessor won every time -- and the same for the
//! `self.included` macro pattern, `class_eval`, and a class method of the
//! class's own that expands to an `attr_*`.
//!
//! The fix reuses [`crate::analyze::redefs`]' machinery: the `def` re-installs
//! at its own document position through a spliced
//! [`HirNode::MethodRedefine`], which puts it back on top of whatever ran
//! before it. Only a class whose body can install a method at run time pays;
//! [`installs_at_run_time`] is what decides, and it names the four shapes.
//!
//! Runs AFTER `redefs::resolve`, and skips any body that pass already made
//! positional, so one `def` never splices twice.

use crate::compiler::{ClassId, Compiler, DefEvent, ScopeId};
use crate::hir::{HirNode, Span};

/// The rows whose call installs a method on the receiver. A LITERAL
/// `attr_accessor :x` never reaches here -- the analyze walk consumes it into
/// the method table and it is not a statement any more -- so a call left in
/// the tree is one zeo could not fold.
const INSTALLERS: &[&str] = &[
    "attr",
    "attr_reader",
    "attr_writer",
    "attr_accessor",
    "define_method",
    "define_singleton_method",
    "alias_method",
    "remove_method",
    "undef_method",
    "class_eval",
    "module_eval",
    "class_exec",
    "module_exec",
    "instance_eval",
    "instance_exec",
    "eval",
    "send",
    "__send__",
    "public_send",
];

/// The hooks a mixin uses to write on the class that mixes it in. A module
/// carrying one of them can install anything, so the body it joins is
/// treated as dynamic without reading the hook.
const MIXIN_HOOKS: &[&str] = &["included", "extended", "prepended", "inherited"];

pub fn resolve(compiler: &mut Compiler) {
    let installers = InstallerSpans::collect(compiler);
    if installers.is_empty() {
        return;
    }

    // Which sites belong to each class, so a class-level answer is computed
    // once and every one of its bodies reads it.
    let mut dynamic: crate::compiler::FMap<ClassId, bool> = Default::default();
    let mut work: Vec<(usize, usize)> = Vec::new(); // (site, def index)
    for si in 0..compiler.class_body_sites.len() {
        let cid = compiler.class_body_sites[si].class;
        let is_dynamic = match dynamic.get(&cid) {
            Some(&d) => d,
            None => {
                let d = eligible(compiler, cid)
                    && class_installs_at_run_time(compiler, cid, &installers);
                dynamic.insert(cid, d);
                d
            }
        };
        if !is_dynamic {
            continue;
        }
        for di in 0..compiler.class_body_sites[si].defs.len() {
            if compiler.class_body_sites[si].defs[di].event == DefEvent::Added {
                work.push((si, di));
            }
        }
    }

    for (si, di) in work {
        let cid = compiler.class_body_sites[si].class;
        let (name, singleton, seq, def_at) = {
            let d = &compiler.class_body_sites[si].defs[di];
            (d.name.clone(), d.singleton, d.seq, d.at)
        };
        // The body this definition installed. The two lists carry DIFFERENT
        // counters -- `SiteDef::seq` counts definitions, the history counts
        // registrations -- so they are paired by ORDER, which is what
        // `redefs` does with the same pair.
        let mut ordinal = 0usize;
        for site in &compiler.class_body_sites {
            if site.class != cid {
                continue;
            }
            ordinal += site
                .defs
                .iter()
                .filter(|d| {
                    d.event == DefEvent::Added
                        && d.singleton == singleton
                        && d.name == name
                        && d.seq < seq
                })
                .count();
        }
        let Some(scope) = compiler.classes[cid.0 as usize]
            .method_history
            .iter()
            .filter(|&&(ref m, s, _, _)| s == singleton && *m == name)
            .nth(ordinal)
            .map(|&(_, _, _, sid)| sid)
        else {
            continue;
        };
        // A PREPENDED module's copy of the name outranks this body, and the
        // overlay install would put the body on top of it. The compiled
        // tables already have the prepend in the right place, so leave the
        // name alone.
        if shadowed_by_a_prepend(compiler, cid, &name, singleton) {
            continue;
        }
        // `redefs` already gave this body a position; a second splice would
        // install it twice and move the definition report with it.
        if compiler.classes[cid.0 as usize]
            .redef_scopes
            .iter()
            .any(|&(s, _)| s == scope)
        {
            continue;
        }
        tracing::debug!(class = cid.0, %name, singleton, "def made positional");
        splice(compiler, cid, si, def_at, seq, &name, singleton, scope);
    }
}

/// Records the body as a positional install and puts the install itself in
/// the statement stream at the `def`'s own place. The `at` bump matches
/// `redefs`': `def_hooks` reads these records afterwards and its report has
/// to land after the install.
#[allow(clippy::too_many_arguments)]
fn splice(
    compiler: &mut Compiler,
    cid: ClassId,
    si: usize,
    def_at: usize,
    def_seq: u32,
    name: &str,
    singleton: bool,
    scope: ScopeId,
) {
    compiler.classes[cid.0 as usize]
        .redef_scopes
        .push((scope, singleton));
    let node = compiler.hir.push(HirNode::MethodRedefine {
        class: cid.0,
        name: name.to_string(),
        scope: scope.0,
        singleton,
    });
    compiler.class_body_sites[si].stmts.insert(def_at, node);
    for d in &mut compiler.class_body_sites[si].defs {
        if d.at > def_at || (d.at == def_at && d.seq >= def_seq) {
            d.at += 1;
        }
    }
}

/// Whether a module prepended onto `cid` -- onto its singleton class for a
/// class method -- writes `name` itself. Its copy sits ahead of the class's
/// own body in both chains, and an overlay install has no way to land under
/// it.
fn shadowed_by_a_prepend(compiler: &Compiler, cid: ClassId, name: &str, singleton: bool) -> bool {
    let ci = &compiler.classes[cid.0 as usize];
    let mods: Vec<ClassId> = match singleton {
        true => ci.class_method_prepends.clone(),
        false => ci.prepends().collect(),
    };
    mods.iter().any(|&m| {
        std::iter::once(m)
            .chain(compiler.classes[m.0 as usize].ancestors.iter().copied())
            .any(|c| compiler.classes[c.0 as usize].own_method_at.contains_key(name))
    })
}

/// Whether `cid`'s definitions may be given a position at all. A BUILTIN's
/// rows are not the runtime overlay's to install -- its first body answers
/// through the native table, and `redefs` has a whole separate rule for
/// that -- and the toplevel plus the value-shaped subclasses reach their
/// methods as inherent fns rather than through a trampoline. The same
/// filter `redefs` opens with.
fn eligible(compiler: &Compiler, cid: ClassId) -> bool {
    let ci = &compiler.classes[cid.0 as usize];
    cid.0 != 0
        && !ci.is_builtin
        && !ci.is_bootstrap
        && !compiler.is_exception_backed(cid)
        && !compiler.is_value_subclass(cid)
        && !compiler.is_immediate_subclass(cid)
}

/// Whether anything `cid`'s body runs can install a method on it.
///
/// Four shapes, and each one is a real program: a call the walk left in the
/// statement stream that names an installer (a block-wrapped `attr_*`, a
/// `class_eval`); one of the class's OWN class methods that expands to an
/// installer (`def self.field(n) = attr_accessor(n)`); a module the class
/// `extend`s whose instance methods do (a macro pack); and a mixin carrying
/// an `included`/`extended`/`prepended`/`inherited` hook, which can write
/// anything at all on the class that takes it.
fn class_installs_at_run_time(
    compiler: &Compiler,
    cid: ClassId,
    installers: &InstallerSpans,
) -> bool {
    let sites = compiler
        .class_body_sites
        .iter()
        .filter(|s| s.class == cid)
        .flat_map(|s| s.stmts.iter().copied());
    if sites.into_iter().any(|n| installers.inside(compiler, n)) {
        return true;
    }
    let ci = &compiler.classes[cid.0 as usize];
    if ci
        .own_class_methods
        .iter()
        .any(|&sid| scope_installs(compiler, sid, installers))
    {
        return true;
    }
    let mixins = ci
        .extends
        .iter()
        .copied()
        .chain(ci.mixin_order.iter().map(|&(m, _)| m));
    mixins.into_iter().any(|m| {
        MIXIN_HOOKS
            .iter()
            .any(|h| compiler.class_method_in_chain(m, h).is_some())
            || compiler.classes[m.0 as usize]
                .own_methods
                .iter()
                .any(|&sid| scope_installs(compiler, sid, installers))
    })
}

fn scope_installs(compiler: &Compiler, sid: ScopeId, installers: &InstallerSpans) -> bool {
    compiler
        .scope(sid)
        .def_node
        .is_some_and(|n| installers.inside(compiler, n))
}

/// Where every installer-named call sits, by file and offset. A call is
/// found by SPAN rather than by walking the tree: the arena has no generic
/// child iterator, and a subtree is exactly the nodes its span covers.
#[derive(Default)]
struct InstallerSpans {
    by_file: crate::compiler::FMap<u32, Vec<(u32, u32)>>,
}

impl InstallerSpans {
    fn collect(compiler: &Compiler) -> Self {
        let mut by_file: crate::compiler::FMap<u32, Vec<(u32, u32)>> = Default::default();
        for (id, node) in compiler.hir.iter_with_ids() {
            let HirNode::Call { name, .. } = node else {
                continue;
            };
            if !INSTALLERS.contains(&name.as_str()) {
                continue;
            }
            if let Some(s) = compiler.hir.span(id) {
                by_file.entry(s.file.0).or_default().push((s.start, s.end));
            }
        }
        for v in by_file.values_mut() {
            v.sort_unstable();
        }
        Self { by_file }
    }

    fn is_empty(&self) -> bool {
        self.by_file.is_empty()
    }

    /// Whether an installer call sits inside `node`'s own span.
    fn inside(&self, compiler: &Compiler, node: crate::hir::NodeId) -> bool {
        let Some(outer) = compiler.hir.span(node) else {
            return false;
        };
        self.covers(outer)
    }

    fn covers(&self, outer: Span) -> bool {
        let Some(spans) = self.by_file.get(&outer.file.0) else {
            return false;
        };
        let from = spans.partition_point(|&(start, _)| start < outer.start);
        spans[from..]
            .iter()
            .take_while(|&&(start, _)| start < outer.end)
            .any(|&(_, end)| end <= outer.end)
    }
}
