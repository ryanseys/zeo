use super::*;

/// A single multi-assignment target slot (`a, b = ...`'s `a`/`b`, or a
/// nested `(a, b), c = ...`'s `(a, b)`) -- generalizes the original
/// plain-local-only shape to every real Ruby assignment target kind, mirrored
/// directly from `MultiWriteNode`/`MultiTargetNode`'s own recursive
/// `lefts`/`rest`/`rights` grammar (see `MultiTargetGroup`). `Call` covers
/// BOTH `obj.attr = ...` and `arr[i] = ...` targets uniformly: both are
/// ordinary Ruby method calls (`attr=`/`[]=`), so lowering PRE-BUILDS the
/// actual write `Call` node (`write_call`) with a synthetic hidden local
/// (`tmp_name`) standing in for "the value this target will receive" as its
/// final argument -- the emitter only has to bind `tmp_name` to the runtime-
/// destructured value BEFORE emitting `write_call` via the ordinary
/// `lower_expr` path, reusing the EXACT same static/dynamic dispatch
/// the call lowering (`clif/call.rs`) already provides for every other call,
/// with no bespoke attr/index-write codegen of its own.
/// See `lower/assign.rs`'s `lower_multi_target`.
// `Clone` because `Params` is `Clone` and now carries destructuring groups
// (`Params::destructures`); the targets themselves are small, owned data.
#[derive(Debug, Clone)]
pub enum MultiTarget {
    Local(String),
    Ivar(String),
    ClassVar(String),
    Global(String),
    /// A bare (lexically-scoped) constant target -- see `ConstWrite`'s docs
    /// for the same `scope: None` resolution rule.
    Const(String),
    /// An explicit `Foo::BAR` target -- `scope` resolves like
    /// `ConstWrite`'s `Some(class_name)`.
    ScopedConst {
        scope: String,
        name: String,
    },
    /// `obj.attr = tmp_name` / `arr[i] = tmp_name` -- see this enum's own
    /// docs above.
    Call {
        write_call: NodeId,
        tmp_name: String,
    },
    /// `(a, b)` -- a nested destructuring group; the value distributed to
    /// this slot is itself further split via `zeo_rt::multi_assign`,
    /// recursively.
    Nested(MultiTargetGroup),
}

/// The `before`/`splat`/`after` shape EVERY multi-assignment target list
/// has, at every nesting level (Ruby's own grammar allows at most one splat
/// per group, anchoring plain targets before/after it) -- shared by the
/// top-level `HirNode::MultiWrite`, a `MultiTarget::Nested` group, and a
/// multi-target `for a, b in ...`'s own index (`HirNode::For`). `splat:
/// None` = no `*` at all; `Some(None)` = an anonymous `*` (discards the
/// middle slice -- still unsupported, matching the pre-existing plain-local
/// restriction, a clean lowering error); `Some(Some(target))` = `*target`.
#[derive(Debug, Clone)]
pub struct MultiTargetGroup {
    pub before: Vec<MultiTarget>,
    pub splat: Option<Option<Box<MultiTarget>>>,
    pub after: Vec<MultiTarget>,
}

impl MultiTargetGroup {
    /// Every plain LOCAL name this group binds, at any nesting depth,
    /// appended to `out`. Only locals: an ivar/global/constant/attr-write
    /// target isn't a name the enclosing scope binds, so it is irrelevant to
    /// the callers (`Params::bound_names`, which answers "what does this
    /// parameter list introduce as locals?").
    pub fn collect_local_names(&self, out: &mut Vec<String>) {
        let mut visit = |t: &MultiTarget| match t {
            MultiTarget::Local(n) => out.push(n.clone()),
            MultiTarget::Nested(g) => g.collect_local_names(out),
            MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. }
            | MultiTarget::Call { .. } => {}
        };
        self.before.iter().for_each(&mut visit);
        if let Some(Some(t)) = &self.splat {
            visit(t);
        }
        self.after.iter().for_each(&mut visit);
    }
}

impl MultiTarget {
    /// Every `NodeId` directly embedded in this target (a `Call` target's
    /// pre-built `write_call`, or a nested group's own embedded nodes) --
    /// NOT the multi-assignment's own `value` (a sibling concern) -- mirrors
    /// `Pattern::for_each_node`'s exact shape/purpose, shared by every
    /// traversal that needs to walk sub-expressions nested inside a target
    /// (ivar/cvar collection, local-type tracking, hoisting, capture
    /// analysis).
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        match self {
            MultiTarget::Local(_)
            | MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. } => {}
            MultiTarget::Call { write_call, .. } => visit(*write_call),
            MultiTarget::Nested(group) => group.for_each_node(visit),
        }
    }

    /// Every LEAF target this one denotes, at any nesting depth. A `Nested`
    /// group recurses; everything else yields itself.
    ///
    /// The companion to `for_each_node`, which yields the sub-EXPRESSIONS
    /// embedded in a target. This yields the targets themselves, so a pass
    /// collecting names by storage class -- the ivar/cvar/const registration
    /// that decides a struct's fields and a constant's owning scope -- has
    /// something to match on.
    ///
    /// `for_each_node` silently dropped every ivar, cvar and constant
    /// written only via a multi-assignment or a `for` target, because those
    /// arms are leaves with no embedded expression. An ivar with no
    /// collected name gets no struct field, and its write does not compile.
    /// A cvar or constant registers against the wrong owner, which is worse:
    /// it compiles.
    ///
    /// `Global` targets need nothing from this. `emit_target_write` lowers
    /// them through `zeo_rt::global_set`, which declares no storage.
    pub fn for_each_target(&self, visit: &mut impl FnMut(&MultiTarget)) {
        match self {
            MultiTarget::Nested(group) => group.for_each_target(visit),
            leaf @ (MultiTarget::Local(_)
            | MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. }
            | MultiTarget::Call { .. }) => visit(leaf),
        }
    }

    /// Every LOCAL-like name this target binds, for hoisting/local-type-
    /// tracking purposes: a plain `Local`, or a `Call` target's own hidden
    /// `tmp_name` synthetic local (bound once per multi-assignment, read
    /// back inside `write_call` -- see `MultiTarget::Call`'s docs). Ivar/
    /// cvar/global/const targets have their own, separate storage and don't
    /// participate in the enclosing scope's LOCAL-variable bookkeeping at
    /// all, so they're excluded here (mirrors `Pattern::for_each_bound_name`'s
    /// same "leaks into the enclosing scope" contract, narrowed to only the
    /// target kinds that actually use local-variable storage).
    pub fn for_each_local_name(&self, visit: &mut impl FnMut(&str)) {
        match self {
            MultiTarget::Local(n) => visit(n),
            MultiTarget::Call { tmp_name, .. } => visit(tmp_name),
            MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. } => {}
            MultiTarget::Nested(group) => group.for_each_local_name(visit),
        }
    }
}

impl MultiTargetGroup {
    /// See `MultiTarget::for_each_node`'s docs.
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_node(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_node(visit);
        }
    }

    /// See `MultiTarget::for_each_target`'s docs.
    pub fn for_each_target(&self, visit: &mut impl FnMut(&MultiTarget)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_target(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_target(visit);
        }
    }

    /// See `MultiTarget::for_each_local_name`'s docs.
    pub fn for_each_local_name(&self, visit: &mut impl FnMut(&str)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_local_name(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_local_name(visit);
        }
    }
}
