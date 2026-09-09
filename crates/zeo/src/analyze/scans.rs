//! Whole-arena scans and collectors: `ArenaFacts` (the one-sweep answers),
//! the shared body/statement scan helpers, and the ivar collector.

use super::*;

/// What [`collect_arena_facts`] answers in its one sweep. A struct rather than
/// nested tuples because the sweep is the natural home for any whole-arena
/// question, and each new one otherwise deepens the tuple at every call site.
#[derive(Default)]
pub(super) struct ArenaFacts {
    /// [`Compiler::assigned_const_names`].
    pub(super) assigned_consts: FSet<String>,
    /// [`Compiler::runtime_patches`].
    pub(super) patched_names: FSet<String>,
    /// [`Compiler::runtime_patches_any_name`].
    pub(super) patches_any_name: bool,
    /// [`Compiler::program_freezes`].
    pub(super) freezes: bool,
    /// [`Compiler::unique_top_const_inits`]. `None` records a name seen more
    /// than once, which is how uniqueness is decided in a single pass.
    pub(super) top_const_inits: FMap<String, Option<NodeId>>,
    /// [`Compiler::const_write_sites`].
    pub(super) const_write_sites: FMap<String, Vec<NodeId>>,
    /// [`Compiler::global_write_sites`].
    pub(super) global_write_sites: FMap<String, Vec<NodeId>>,
    /// [`Compiler::const_set_sites`].
    pub(super) const_set_sites: Vec<NodeId>,
}

/// ONE flat sweep of the whole node arena answering every whole-arena
/// question -- see [`ArenaFacts`].
///
/// A flat sweep rather than a tree walk: every reachable node is in the
/// arena by construction, and a site on a dead branch still counts -- for
/// const names because over-collection is the safe direction (see the
/// field's docs), for patches because the answer is "could this name change
/// under us?".
pub(super) fn collect_arena_facts(hir: &Hir) -> ArenaFacts {
    let mut facts = ArenaFacts::default();
    let ArenaFacts {
        assigned_consts: consts,
        patched_names: names,
        patches_any_name: any,
        freezes,
        top_const_inits,
        const_write_sites,
        global_write_sites,
        const_set_sites,
    } = &mut facts;
    for (id, node) in hir.iter_with_ids() {
        match node {
            // A `CONST = ...` written outside any class body is a statement of
            // a `Program` rather than of a class body, so it is in none of the
            // per-class tables -- and the top level is where every bare
            // constant lookup ends. Only DIRECT statements count: a write
            // nested in a top-level `if` may never run.
            HirNode::Program(stmts) => {
                for &s in stmts {
                    if let HirNode::ConstWrite {
                        name,
                        value,
                        scope: None,
                    } = &hir[s]
                    {
                        top_const_inits
                            .entry(name.clone())
                            .and_modify(|e| *e = None)
                            .or_insert(Some(*value));
                    }
                }
            }
            // `name` is already the leaf -- an explicit `Foo::NAME = ...`
            // keeps its namespace in the separate `scope` field.
            HirNode::ConstWrite { scope, name, .. } => {
                const_write_sites.entry(name.clone()).or_default().push(id);
                consts.insert(name.clone());
                // The QUALIFIED spelling as well, so a reader that names a
                // scope can ask about that scope rather than settling for
                // "some constant with this leaf exists somewhere". Anchors
                // are stripped: `::A::B` and `A::B` name the same constant,
                // and there is only one top level.
                if let Some(scope) = scope {
                    let scope = crate::constpath::ConstPath::parse(scope).unanchored();
                    consts.insert(format!("{scope}::{name}"));
                }
            }
            // A `def` inside a BLOCK installs when the block runs, not when
            // the class body does -- `N.class_eval { def e; end }`, and the
            // same desugared `define_method(:e) { }`. The subtree walk
            // reaches a def nested several blocks deep.
            HirNode::Lambda { body, .. } | HirNode::Block { body, .. } => {
                for &id in body {
                    names.extend(defs_in_subtree(hir, id));
                }
            }
            HirNode::GlobalWrite(name, _) | HirNode::AliasGlobal(name, _) => {
                global_write_sites.entry(name.clone()).or_default().push(id);
            }
            HirNode::Call { name, args, .. } => {
                if name == "const_set" {
                    const_set_sites.push(id);
                }
                if name == "freeze" {
                    *freezes = true;
                }
                collect_patch_call(hir, name, args, names, any);
            }
            _ => {}
        }
    }
    facts
}

/// Walk populating [`Compiler::top_level_const_aliases`]: `NAME = <value>`
/// reached without ever entering a `class`/`module` body.
///
/// Descends through the statement wrappers a top-level write can hide behind
/// (`if`, `begin`, `Seq`, a box scope) and stops at `ClassDef`, which is
/// exactly the boundary that makes a write "top-level". First write wins, so a
/// later reassignment does not change which class a reopen attaches to -- the
/// arena scan this replaces had the same first-match-wins behaviour.
pub(super) fn collect_top_level_const_aliases(
    hir: &Hir,
    stmts: &[NodeId],
    out: &mut FMap<String, NodeId>,
) {
    for &s in stmts {
        match &hir[s] {
            HirNode::ConstWrite {
                scope: None,
                name,
                value,
            } => {
                out.entry(name.clone()).or_insert(*value);
            }
            // A definition's body is a different scope; nothing inside it can
            // be what a top-level `class CONST` reopens.
            HirNode::ClassDef { .. } => {}
            _ => hir[s].for_each_child(&mut |c| {
                collect_top_level_const_aliases(hir, std::slice::from_ref(&c), out)
            }),
        }
    }
}

/// The runtime definition verbs -- the calls that install (or RETIRE) a method
/// body the overlay holds and only DYNAMIC dispatch consults. Each names ONE
/// method, in its first argument. A literal-name `define_method`/
/// `define_singleton_method` inside a class body never reaches here: lowering
/// already desugared it into a `DefMethod`, so what survives as a `Call` is
/// exactly the runtime half.
///
/// `undef_method`/`remove_method` are here for the RECEIVER-BEARING spelling
/// only. `ClassInfo::runtime_undefs` covers the receiverless one written in a
/// class body, where the class it retires from is known; `g.singleton_class.
/// undef_method(:close)` retires the name for ONE object and names no class a
/// scan could resolve, so the name goes program-wide instead.
const REDEF_VERBS: &[&str] = &[
    "define_method",
    "define_singleton_method",
    "alias_method",
    "undef_method",
    "remove_method",
];

/// The runtime VISIBILITY verbs. Visibility is a runtime property in ruby --
/// `private :name` re-marks a method that already exists -- and these take a
/// LIST, so every argument names a method. A class-body `private :m` never
/// reaches here either: `lower::defs` turns it into a `MethodVisibility` node
/// or retags the `def` in place.
const VIS_VERBS: &[&str] = &[
    "private",
    "public",
    "protected",
    "private_class_method",
    "public_class_method",
    "module_function",
];

/// The `send` family, which reaches a verb above through a symbol argument
/// (`Node.send(:define_method, name)`) and so shifts every argument by one.
const SEND_VERBS: &[&str] = &["send", "__send__", "public_send"];

/// [`collect_arena_facts`]'s runtime-patch half for one `Call` node: a
/// definition/visibility verb (possibly through `send`) marks the method
/// names it could install or re-scope at runtime.
fn collect_patch_call(
    hir: &Hir,
    name: &str,
    args: &[ArrayElem],
    names: &mut FSet<String>,
    any: &mut bool,
) {
    // The names start at the verb's first argument, one slot later when the
    // verb itself arrives as `send`'s first argument.
    let verb = |v: &str| REDEF_VERBS.contains(&v) || VIS_VERBS.contains(&v);
    let (at, verb_name) = if verb(name) {
        (0, name)
    } else if SEND_VERBS.contains(&name)
        && let Some(ArrayElem::Single(a)) = args.first()
        && let Some(sent) = hir.sent_name(*a).filter(|v| verb(v))
    {
        (1, sent)
    } else {
        return;
    };
    // A definition verb names one method; a visibility verb names a list.
    let named = match VIS_VERBS.contains(&verb_name) {
        true => &args[at.min(args.len())..],
        false => &args[at.min(args.len())..(at + 1).min(args.len())],
    };
    // No argument at all: a bare `private` sets the DEFAULT for later defs,
    // which names nothing this scan can read.
    if named.is_empty() {
        *any = true;
    }
    for arg in named {
        match arg {
            ArrayElem::Single(a) => match hir.sent_name(*a) {
                Some(patched) => {
                    names.insert(patched.to_owned());
                }
                None => *any = true,
            },
            // A splat: nothing to read the names from.
            _ => *any = true,
        }
    }
}

/// Every `def`/desugared `define_method` name in `root`'s subtree.
fn defs_in_subtree(hir: &Hir, root: NodeId) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let HirNode::DefMethod { name, .. } = &hir[id] {
            out.push(name.clone());
        }
        hir[id].for_each_child(&mut |child| stack.push(child));
    }
    out
}

/// Read-only scan populating `Compiler::shell_kinds`: records every
/// `class`/`module` definition's fully-qualified name -> `is_module`,
/// descending through nested bodies, BOTH `if` branches (over-collection is
/// harmless -- it is only a lookup table, and shells are created on demand by
/// TAKEN definitions), box scopes, and statement-group wrappers.
pub(super) fn collect_shell_kinds(
    hir: &Hir,
    stmts: &[NodeId],
    scope: &[String],
    box_id: u32,
    out: &mut FMap<(u32, String), bool>,
) {
    for &id in stmts {
        collect_shell_kinds_node(hir, id, scope, box_id, out);
    }
}

/// One node of [`collect_shell_kinds`]'s walk. The descent MUST cover every
/// position the registration walk ([`collect_nested_bodies`]) reaches, and it
/// makes the same stops: this pre-pass and that walk answer the same "where
/// can a definition hide" question, and any position only the registration
/// walk descended produced a class that registered but could never be
/// forward-referenced (a `class` inside `begin/rescue` or a block was
/// "unknown superclass" to every earlier file). Hence the generic
/// `for_each_child` default rather than an allowlist of container nodes.
fn collect_shell_kinds_node(
    hir: &Hir,
    id: NodeId,
    scope: &[String],
    box_id: u32,
    out: &mut FMap<(u32, String), bool>,
) {
    match &hir[id] {
        HirNode::ClassDef {
            name,
            body,
            is_module,
            ..
        } => {
            let fq = if scope.is_empty() {
                name.clone()
            } else {
                format!("{}::{}", scope.join("::"), name)
            };
            out.insert((box_id, fq), *is_module);
            let mut inner = scope.to_vec();
            inner.push(name.clone());
            collect_shell_kinds(hir, body, &inner, box_id, out);
        }
        // A box's body is a fresh top-level scope under the box's id.
        HirNode::BoxScope { box_id: bx, body } => {
            collect_shell_kinds(hir, body, &[], *bx, out);
        }
        // ...and a `define_method(:x) { module M; end }` is a BLOCK wearing a
        // `DefMethod`'s shape, so it descends like one -- the exception
        // `collect_nested_bodies` grew in f51b9b40 and this walk did not,
        // despite the doc above binding the two together. Without it a module
        // defined in a `define_method` block REGISTERS but is absent from
        // `shell_kinds`, so no earlier file can forward-resolve it.
        HirNode::DefMethod { body, .. }
            if hir.has_flag(id, crate::hir::NodeFlag::BLOCK_BODIED_DEF) =>
        {
            collect_shell_kinds(hir, body, scope, box_id, out);
        }
        // The registration walk's own stop: a method body is a separate
        // function ruby rejects a `class` inside. A `Lambda` descends via
        // the generic default, same as the registration walk descends it.
        HirNode::DefMethod { .. } => {}
        other => {
            other.for_each_child(&mut |c| collect_shell_kinds_node(hir, c, scope, box_id, out));
        }
    }
}

/// Whether any node under `id` satisfies `hit`, descending through
/// `HirNode::for_each_child` and stopping where a new Ruby scope begins.
///
/// The two callers below are the whole reason this exists. A per-caller
/// copy of the 81-variant match -- ~270 lines apiece, byte-identical except
/// for the arms that answer the question -- is precisely the drift
/// `for_each_child`'s own docs describe. Such copies skip a `New`'s block
/// and keyword arguments, so `def f; Hash.new { |h, k| yield k }; end`
/// reports no bare block use and the method never gets its `__blk`
/// parameter; or treat a bare `Block` node as a stop while special-casing a
/// call's block argument to descend into it, so the same block is walked or
/// skipped depending on how the walk arrived at it.
///
/// Three stops. `Ffi` is a synthesized
/// wrapper body with none of this in it. `ClassDef` and `DefMethod` open a
/// fresh Ruby scope, so a `yield` or `super` inside one belongs to that
/// scope, not to the body being scanned. `Block` and `Lambda` are NOT stops:
/// neither has a block or a super target of its own, so both constructs
/// refer to the enclosing method -- which is what both callers' own docs
/// already claimed.
fn scan_body(hir: &Hir, id: NodeId, hit: &impl Fn(&HirNode) -> bool) -> bool {
    let node = &hir[id];
    if hit(node) {
        return true;
    }
    match node.scope_kind() {
        // `Ffi` is a synthesized wrapper body; a `Definition` is a fresh Ruby
        // scope, so a `yield` or `super` inside one belongs to it.
        ScopeKind::Ffi | ScopeKind::Definition => return false,
        // A block and a lambda have neither an implicit block nor a `super`
        // target of their own, so both refer to the enclosing method -- which
        // is what both callers' docs already claimed.
        ScopeKind::Block | ScopeKind::Lambda | ScopeKind::None => {}
    }
    let mut found = false;
    node.for_each_child(&mut |n| found |= scan_body(hir, n, hit));
    found
}

/// [`scan_body`] over a statement list.
fn scan_stmts(hir: &Hir, body: &[NodeId], hit: &impl Fn(&HirNode) -> bool) -> bool {
    body.iter().any(|&n| scan_body(hir, n, hit))
}

/// A node that reaches for the ENCLOSING method's implicit block.
///
/// `yield` and `block_given?` are the obvious two. The third is a bare
/// `super`: real Ruby forwards the current method's own block to the parent,
/// and the emitted forwarding reads the block parameter directly (see
/// `clif::call::build_zsuper_args`), so the method needs it whether or
/// not the parent turns out to use it -- an unused `Option` costs nothing.
/// A `super { ... }` with a literal block does NOT, though its block body
/// still might, which the ordinary descent covers.
fn wants_enclosing_block(node: &HirNode) -> bool {
    matches!(
        node,
        HirNode::Yield(_) | HirNode::BlockGiven | HirNode::SuperCall { block: None, .. }
    )
}

/// Scans a method's own control flow for a use of its implicit block.
///
/// Descends into nested block and lambda literals: their `yield`/
/// `block_given?` refers to THIS enclosing method's implicit block in real
/// Ruby (blocks and lambdas have none of their own), so a method whose only
/// `yield` sits inside a `.each { ... }` still needs its block parameter --
/// and the emitted closure captures it (see `clif::blocks`).
pub(super) fn scan_bare_block_use(hir: &Hir, id: NodeId) -> bool {
    scan_body(hir, id, &wants_enclosing_block)
}

pub(crate) fn scan_bare_block_use_body(hir: &Hir, body: &[NodeId]) -> bool {
    scan_stmts(hir, body, &wants_enclosing_block)
}

pub(crate) fn scan_contains_super_body(hir: &Hir, body: &[NodeId]) -> bool {
    scan_stmts(hir, body, &|n| matches!(n, HirNode::SuperCall { .. }))
}

/// Recursively scans a method body for `@ivar` reads/writes so the class's
/// `ruby_class!` invocation knows which fields to declare. Mirrors zeo's
/// ivar-registration passes, minus the whole-program fixpoint (a single
/// bottom-up scan is enough here because ivar *names* -- unlike ivar
/// *types* -- don't depend on inference, only on which `@name` tokens
/// appear).
/// An ivar named by a multi-assignment or `for` TARGET -- `@a, @b = 1, 2` and
/// `for @x in ...`, where the name appears nowhere else in the body. Such an
/// ivar still needs a struct field: `emit_target_write` lowers the write to
/// `self.<name>.lock()` regardless. Collecting it only through
/// `MultiTarget::for_each_node` missed it entirely (an ivar target embeds no
/// sub-expression, so that traversal yields nothing), and the write then
/// referenced a field that was never declared.
fn collect_ivar_target(target: &crate::hir::MultiTarget, out: &mut Vec<String>) {
    if let crate::hir::MultiTarget::Ivar(name) = target
        && !out.contains(name)
    {
        out.push(name.clone());
    }
}

pub(crate) fn collect_ivars(hir: &Hir, id: NodeId, out: &mut Vec<String>) {
    let mut record = |name: &str| {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    };
    match &hir[id] {
        // An FFI wrapper body reads only its synthetic parameters.
        HirNode::Ffi(_) => return,
        // A `class`/`def` body is a fresh Ruby scope, scanned under its own
        // owner rather than the one this walk is filling.
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => return,
        HirNode::IvarRead(name) | HirNode::IvarWrite(name, _) => record(name),
        HirNode::For { target, .. } => target.for_each_target(&mut |t| collect_ivar_target(t, out)),
        HirNode::MultiWrite { targets, .. } => {
            targets.for_each_target(&mut |t| collect_ivar_target(t, out))
        }
        _ => {}
    }
    hir[id].for_each_child(&mut |n| collect_ivars(hir, n, out));
}
