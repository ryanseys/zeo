//! Definition sites and events, refinements/`using` activations, and
//! [`Compiler`]'s document-position queries.

use super::*;

/// One `refine Target do ... end`. The refined methods are ordinary
/// instance methods of `holder`, a module the source cannot name; nothing
/// is ever registered ON `target`, which is what keeps the refinement out
/// of `Target.instance_methods` and out of an unrefined call's answer.
pub(crate) struct Refinement {
    /// The module whose body wrote the `refine` -- what a `using` names.
    pub module: ClassId,
    /// The class being refined.
    pub target: ClassId,
    /// The hidden module holding the refined methods.
    pub holder: ClassId,
    /// `refine Target.singleton_class` -- the holder refines Target's CLASS
    /// methods, so a covered site matches a CLASS receiver descending from
    /// `target` where the plain form matches an instance of it.
    pub singleton: bool,
    /// The [`crate::hir::HirNode::Refine`] marker this came from. A
    /// refinement runs nothing where it was written, so codegen drops the
    /// marker -- but only one it can prove reached registration here.
    pub marker: crate::hir::NodeId,
}

/// One `using M`, as the byte range of source it covers: from the `using`
/// itself to the end of the enclosing body (the end of the file at the top
/// level). Real Ruby scopes a refinement lexically, so a byte range IS the
/// rule -- a `def` written after the `using` is covered because its body
/// sits inside the range, and one written above it is not.
pub(crate) struct Activation {
    pub module: ClassId,
    pub file: crate::hir::FileId,
    pub start: u32,
    pub end: u32,
}

/// One `using` written in a SNIPPET. The module is a run-time constant and
/// what it refines lives in the running program's registry, so nothing here
/// names either: the site is recorded by SPAN, reserves an activation slot
/// (`zeo_rt::eval::reserve_using_slots`) and fills it when it runs. Every
/// call site the span covers reads the slot.
pub(crate) struct EvalActivation {
    pub marker: crate::hir::NodeId,
    pub file: crate::hir::FileId,
    pub start: u32,
    pub end: u32,
}

/// See [`Compiler::class_body_sites`].
pub struct ClassBodySite {
    pub def_node: Option<crate::hir::NodeId>,
    pub class: ClassId,
    pub stmts: Vec<crate::hir::NodeId>,
    /// Every definition this site's walk CONSUMED -- a `def`, one name of an
    /// `attr_*` expansion, an alias, an `undef` -- in source order. None of
    /// them reaches `stmts`, because zeo compiles a definition into a method
    /// table rather than running it. Ruby still announces each one at its
    /// position, so [`crate::analyze::def_hooks`] keeps them here until it
    /// knows whether any hook body will answer, and splices a
    /// [`crate::hir::HirNode::DefHook`] into `stmts` for the ones that will.
    pub defs: Vec<SiteDef>,
    /// The method names this site's body INSTALLS, kept apart from `defs`
    /// because `def_hooks` takes that list away once it has decided which
    /// definitions get a hook report. Read by codegen's frozen-reopen guard,
    /// which needs to know what a body that never runs would have defined.
    pub installs: Vec<String>,
}

/// One consumed definition, and where its report would go.
pub struct SiteDef {
    /// Where this definition sits in the program's EXECUTION order, counted
    /// across every site and the top level. The analyze walk visits bodies in
    /// the order they run, so a plain counter is exact -- which raw spans are
    /// not, since a spliced `require` puts another file's statements in the
    /// middle of this one. [`crate::analyze::def_hooks`] orders a class's own
    /// definitions by this to work out which are still in the future.
    pub seq: u32,
    /// The index in the site's `stmts` the report belongs BEFORE.
    pub at: usize,
    /// Which STATEMENT STREAM `at` indexes, for a top-level def: `None` is
    /// the main list, `Some(k)` is `feature_units[k]`'s body. The walk runs
    /// the main stream and then every unit through the same
    /// `process_top_stmt`, all pushing into the one `top_level_defs` -- and
    /// splicing a unit's index into the main list panicked the compiler
    /// (autosub: "insertion index (is 58) should be <= len (is 18)").
    /// Class-body defs live in their site's own `defs` list and leave this
    /// `None`.
    pub unit: Option<u32>,
    /// The definition's own node, whose span decides whether a hook installed
    /// later in the same file ever saw it.
    pub node: crate::hir::NodeId,
    pub name: String,
    pub event: DefEvent,
    /// A `def self.x` / `class << self` definition, which reports through
    /// `singleton_method_added` on the class object rather than `method_added`.
    pub singleton: bool,
}

/// Which of Ruby's three definition events a [`SiteDef`] is.
#[derive(Clone, Copy, PartialEq)]
pub enum DefEvent {
    Added,
    Removed,
    Undefined,
}

impl DefEvent {
    /// The hook this event fires, for a definition on a class (`singleton`
    /// false) or on its singleton (`singleton` true).
    pub fn hook(self, singleton: bool) -> &'static str {
        match (self, singleton) {
            (DefEvent::Added, false) => "method_added",
            (DefEvent::Removed, false) => "method_removed",
            (DefEvent::Undefined, false) => "method_undefined",
            (DefEvent::Added, true) => "singleton_method_added",
            (DefEvent::Removed, true) => "singleton_method_removed",
            (DefEvent::Undefined, true) => "singleton_method_undefined",
        }
    }
}

impl Compiler {
    /// The `(target, holder)` pairs a call site at `node` must consult
    /// before ordinary dispatch, MOST RECENTLY activated first -- real
    /// Ruby's own precedence when two `using`s refine the same class.
    ///
    /// Empty for the overwhelming majority of programs, which write no
    /// `using` at all; empty too for a node with no source position (a
    /// synthesized desugaring), where there is no lexical question to ask.
    pub(crate) fn refinements_active_at(
        &self,
        node: crate::hir::NodeId,
    ) -> Vec<(ClassId, ClassId, bool)> {
        if self.activations.is_empty() {
            return Vec::new();
        }
        let Some(span) = self.hir.span(node).and_then(|s| s.known()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for a in self.activations.iter().rev() {
            if a.file != span.file || span.start < a.start || span.start >= a.end {
                continue;
            }
            out.extend(
                self.refinements
                    .iter()
                    .filter(|r| r.module == a.module)
                    .map(|r| (r.target, r.holder, r.singleton)),
            );
        }
        out
    }

    /// The `using` sites covering `node` in a SNIPPET, outermost first --
    /// the activation SLOT ids a refined call there reads. Empty for a
    /// program, where refinements resolve at compile time.
    pub(crate) fn eval_activations_at(&self, node: crate::hir::NodeId) -> Vec<u32> {
        if self.eval_activations.is_empty() {
            return Vec::new();
        }
        let Some(span) = self.hir.span(node).and_then(|s| s.known()) else {
            return Vec::new();
        };
        self.eval_activations
            .iter()
            .enumerate()
            .filter(|(_, a)| a.file == span.file && span.start >= a.start && span.start < a.end)
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// The slot index one `using` marker reserved.
    pub(crate) fn eval_activation_slot(&self, marker: crate::hir::NodeId) -> Option<u32> {
        self.eval_activations
            .iter()
            .position(|a| a.marker == marker)
            .map(|i| i as u32)
    }

    /// `(refining module, refined target)` for a `refine` holder -- what
    /// codegen registers so `Module#refinements` and `Refinement#target` can
    /// answer at run time. `None` for an ordinary module.
    pub(crate) fn refinement_of(&self, holder: ClassId) -> Option<(ClassId, ClassId)> {
        self.refinements
            .iter()
            .find(|r| r.holder == holder)
            .map(|r| (r.module, r.target))
    }

    /// Whether `node` is a `refine` marker registration already consumed --
    /// what lets codegen emit nothing for it. An UNREGISTERED marker stays a
    /// loud rejection: dropping one would lose the refinement silently.
    pub(crate) fn refinement_marker_registered(&self, node: crate::hir::NodeId) -> bool {
        self.refinements.iter().any(|r| r.marker == node)
    }

    /// Whether `holder` defines `name` as an instance method of its own --
    /// the question that decides whether a call site routes through the
    /// refinement at all.
    pub(crate) fn refinement_defines(&self, holder: ClassId, name: &str) -> bool {
        let defines = |cid: ClassId| {
            self.class(cid)
                .own_methods
                .iter()
                .any(|&sid| self.scope(sid).name == name)
        };
        defines(holder)
            || self
                .class(holder)
                .imported_modules
                .iter()
                .any(|&m| defines(m))
    }

    /// Where `node` runs in the program, or `None` when that is not a static
    /// fact -- inside a `def` body, which runs at call time. See
    /// [`Compiler::doc_order`].
    pub(crate) fn doc_position(&self, node: crate::hir::NodeId) -> Option<u32> {
        self.doc_order.get(&node).copied()
    }

    /// Whether `owner::name` is already defined by the time a statically
    /// positioned `at` runs. `None` means "no static answer" -- either the
    /// query has no position, or nothing recorded a position for the
    /// definition (a `const_set`, a name only an ancestor supplies), in which
    /// case the caller keeps whatever whole-program answer it had.
    pub(crate) fn const_defined_before(
        &self,
        owner: ClassId,
        name: &str,
        at: crate::hir::NodeId,
    ) -> Option<bool> {
        let at = self.doc_position(at)?;
        let defined = *self.const_def_order.get(&(owner, name.to_string()))?;
        Some(defined < at)
    }

    /// Where `class`'s body was written, for locating a rejection raised after
    /// the statement walk has finished.
    ///
    /// The passes that run then (`mro`, chiefly) iterate CLASSES rather than
    /// statements, so they have no statement to stamp -- but a class was
    /// written somewhere, and its body site remembers the node. A class with no
    /// site is one nothing declared in Ruby source: a builtin, or a forward
    /// shell minted for a not-yet-seen superclass.
    pub fn class_def_span(&self, class: ClassId) -> Option<crate::hir::Span> {
        let node = self
            .class_body_sites
            .iter()
            .find(|s| s.class == class)
            .and_then(|s| s.def_node)?;
        self.hir.span(node)
    }
}
