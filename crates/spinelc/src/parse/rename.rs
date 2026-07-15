//! Per-splice-instance local-variable isolation for `require`/`load`
//! (Phase 14.1). Real Ruby gives every loaded file its OWN top-level local
//! scope (each file is compiled as its own iseq with its own local table --
//! `vm_set_top_stack` sizes the frame per-iseq; verified empirically that
//! locals leak in NEITHER direction between files), but the HIR splice
//! merges every file into one `Program` scope, which would silently share
//! same-named locals across files (`eval`'s splice shares scope too, but
//! there sharing is the CORRECT semantics). Fix: after lowering a spliced
//! file's own statements, rename that file's top-level locals to
//! `__f<file_id>_<name>` -- `file_id` is the SPLICE INSTANCE index, not the
//! canonical file, because `load` re-splices the same file and real Ruby
//! gives each `load` execution a fresh local scope (and Phase 14.5's
//! per-box re-execution needs the same per-instance freshness).
//!
//! Why renaming is sound here, in three load-bearing observations:
//!
//! 1. `ruby-prism` resolves bare-identifier-vs-method-call PER PARSE UNIT
//!    (a name never assigned in a file lowers as a `Call` in that file,
//!    regardless of other files' locals) -- so a `LocalRead` in a spliced
//!    file can only ever refer to a name assigned somewhere in that SAME
//!    file, and renaming the file's full assigned-name set consistently
//!    can never re-point a reference across files.
//! 2. The rename set is collected by descending into EVERY block/lambda
//!    body (unlike `codegen::hoisting::collect_locals`, which deliberately
//!    skips escaping-block-own names). Over-collecting a block-OWN name is
//!    harmless: it's renamed consistently at its every visible site, and a
//!    fresh-per-invocation binding behaves identically under any name. What
//!    over-collection buys is one simple rule with no need for
//!    `is_times_fast_path`/type inference at parse time.
//! 3. Names bound by a block/lambda PARAMETER shadow an enclosing local of
//!    the same name (real Ruby scoping, and codegen binds them as fresh
//!    `let`s) -- so while renaming under a block whose param re-binds a
//!    renamed name, that name is suspended from the active set for the
//!    subtree (its references belong to the param, not the file local).
//!
//! `DefMethod`/`ClassDef` bodies are never descended into by either pass:
//! they're genuinely fresh Ruby scopes that cannot capture file-level
//! locals (prism classifies any such name inside them as a call).
//!
//! The synthetic gensym temps lowering mints per file (`Seq`'s `__recvN`,
//! `MultiTarget::Call`'s `tmp_name`) are collected and renamed like any
//! other assigned name -- which is itself load-bearing: each file's parse
//! restarts those counters, so two spliced files' `__recv0`s would
//! otherwise collide in the merged scope.

use crate::hir::{
    ArrayElem, HashPatternRest, Hir, HirNode, MultiTarget, MultiTargetGroup, NodeId, Params,
    Pattern, StrPart,
};
use std::collections::HashSet;

/// Renames `roots`' (one spliced file's own top-level statements --
/// deliberately NOT including any child file's statements spliced between
/// them, which are separate roots already renamed by their own pass and
/// unreachable from these subtrees) local variables to
/// `__f<file_id>_<name>`. Two passes over the same walker: collect the
/// file's assigned-name set, then rename every reference to it.
pub(super) fn isolate_file_locals(hir: &mut Hir, roots: &[NodeId], file_id: usize) {
    let mut w = Walker {
        collecting: true,
        names: HashSet::new(),
        file_id,
    };
    for &r in roots {
        w.visit(hir, r);
    }
    if w.names.is_empty() {
        return;
    }
    w.collecting = false;
    for &r in roots {
        w.visit(hir, r);
    }
}

struct Walker {
    /// Pass 1 (`true`): `names` accumulates every locally-assigned name.
    /// Pass 2 (`false`): `names` is the ACTIVE rename set (entries are
    /// temporarily suspended under a shadowing block param -- see the
    /// `Block`/`Lambda` arm).
    collecting: bool,
    names: HashSet<String>,
    file_id: usize,
}

impl Walker {
    fn mangled(&self, name: &str) -> String {
        format!("__f{}_{}", self.file_id, name)
    }

    /// A site that BINDS a local name (assignment target, rescue binding,
    /// pattern bind, ...): collected in pass 1, renamed in pass 2.
    fn bind(&mut self, name: &mut String) {
        if self.collecting {
            self.names.insert(name.clone());
        } else if self.names.contains(name.as_str()) {
            *name = self.mangled(name);
        }
    }

    /// A site that READS a local name: renamed in pass 2 only (reads
    /// introduce nothing in pass 1).
    fn read(&mut self, name: &mut String) {
        if !self.collecting && self.names.contains(name.as_str()) {
            *name = self.mangled(name);
        }
    }

    /// Suspends the intersection of `params`' bound names with the active
    /// set for a block/lambda subtree (shadowing -- see module docs);
    /// returns what was removed so the caller can restore it. No-op while
    /// collecting (param names are never collected).
    fn suspend_params(&mut self, params: &Params) -> Vec<String> {
        if self.collecting {
            return Vec::new();
        }
        let mut suspended = Vec::new();
        let mut consider = |n: &str| {
            if let Some(taken) = self.names.take(n) {
                suspended.push(taken);
            }
        };
        for n in &params.required {
            consider(n);
        }
        for (n, _) in &params.optional {
            consider(n);
        }
        for n in &params.post {
            consider(n);
        }
        for kw in &params.keywords {
            let (crate::hir::KeywordParam::Required(n)
            | crate::hir::KeywordParam::Optional(n, _)) = kw;
            consider(n);
        }
        for slot in [&params.rest, &params.keyword_rest, &params.block] {
            if let Some(Some(n)) = slot {
                consider(n);
            }
        }
        suspended
    }

    fn visit_all(&mut self, hir: &mut Hir, ids: &[NodeId]) {
        for &n in ids {
            self.visit(hir, n);
        }
    }

    fn visit_opt(&mut self, hir: &mut Hir, id: &Option<NodeId>) {
        if let Some(n) = id {
            self.visit(hir, *n);
        }
    }

    fn visit(&mut self, hir: &mut Hir, id: NodeId) {
        // Check the node out of the arena while visiting (children are
        // separate arena slots, so recursion never aliases this node); the
        // placeholder is restored at the end.
        let mut node = std::mem::replace(&mut hir[id], HirNode::NilLit);
        match &mut node {
            HirNode::LocalRead(name) => self.read(name),
            HirNode::LocalWrite(name, value) => {
                self.bind(name);
                self.visit(hir, *value);
            }
            HirNode::Program(body)
            | HirNode::Eval(body)
            | HirNode::Seq(body)
            | HirNode::BoxScope { body, .. } => {
                self.visit_all(hir, &body.clone())
            }
            HirNode::And(l, r) | HirNode::Or(l, r) => {
                self.visit(hir, *l);
                self.visit(hir, *r);
            }
            HirNode::Defined(v) => self.visit(hir, *v),
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => {
                self.visit(hir, *cond);
                self.visit_all(hir, &then_body.clone());
                self.visit_all(hir, &else_body.clone());
            }
            HirNode::CaseWhen {
                subject,
                arms,
                else_body,
            } => {
                self.visit_opt(hir, subject);
                for (values, body) in arms.iter() {
                    self.visit_all(hir, values);
                    self.visit_all(hir, body);
                }
                self.visit_all(hir, &else_body.clone());
            }
            HirNode::ArrayLit(elems) => {
                for e in elems.iter() {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                    self.visit(hir, *n);
                }
            }
            HirNode::HashLit(pairs) => {
                for pair in pairs.iter() {
                    self.visit(hir, pair.0);
                    self.visit(hir, pair.1);
                }
            }
            HirNode::RangeLit { start, end, .. } => {
                self.visit_opt(hir, start);
                self.visit_opt(hir, end);
            }
            HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
                for p in parts.iter() {
                    if let StrPart::Interp(n) = p {
                        self.visit(hir, *n);
                    }
                }
            }
            HirNode::IvarWrite(_, value)
            | HirNode::ClassVarWrite(_, value)
            | HirNode::GlobalWrite(_, value)
            | HirNode::ConstWrite { value, .. } => self.visit(hir, *value),
            HirNode::Call {
                receiver,
                args,
                kwargs,
                kwargs_splat,
                block,
                block_arg,
                ..
            } => {
                self.visit_opt(hir, receiver);
                for a in args.iter() {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    self.visit(hir, *n);
                }
                for pair in kwargs.iter() {
                    self.visit(hir, pair.0);
                    self.visit(hir, pair.1);
                }
                self.visit_opt(hir, kwargs_splat);
                self.visit_opt(hir, block);
                self.visit_opt(hir, block_arg);
            }
            HirNode::New { args, .. } | HirNode::SuperCall { args, .. } => {
                self.visit_all(hir, &args.clone())
            }
            HirNode::Block { params, body } | HirNode::Lambda { params, body } => {
                let suspended = self.suspend_params(params);
                for d in params.default_ids() {
                    self.visit(hir, d);
                }
                self.visit_all(hir, &body.clone());
                self.names.extend(suspended);
            }
            // Genuinely fresh Ruby scopes -- can't capture file-level
            // locals, never descended into (see module docs).
            HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => {}
            HirNode::While { cond, body, .. } => {
                self.visit(hir, *cond);
                self.visit_all(hir, &body.clone());
            }
            HirNode::Loop { body } => self.visit_all(hir, &body.clone()),
            HirNode::For {
                target,
                iterable,
                body,
            } => {
                self.visit_target(hir, target);
                self.visit(hir, *iterable);
                self.visit_all(hir, &body.clone());
            }
            HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => self.visit_opt(hir, v),
            HirNode::MultiWrite { targets, value } => {
                self.visit_group(hir, targets);
                self.visit(hir, *value);
            }
            HirNode::Yield(args) | HirNode::Raise(args) => self.visit_all(hir, &args.clone()),
            HirNode::CaseIn {
                subject,
                arms,
                else_body,
            } => {
                self.visit(hir, *subject);
                for arm in arms.iter_mut() {
                    self.visit_pattern(hir, &mut arm.pattern);
                    if let Some((g, _)) = arm.guard {
                        self.visit(hir, g);
                    }
                    self.visit_all(hir, &arm.body.clone());
                }
                if let Some(body) = else_body {
                    self.visit_all(hir, &body.clone());
                }
            }
            HirNode::MatchPredicate { subject, pattern }
            | HirNode::MatchRequired { subject, pattern } => {
                self.visit(hir, *subject);
                self.visit_pattern(hir, pattern);
            }
            HirNode::Begin {
                body,
                rescues,
                else_body,
                ensure_body,
            } => {
                self.visit_all(hir, &body.clone());
                for r in rescues.iter_mut() {
                    if let Some(binding) = &mut r.binding {
                        self.bind(binding);
                    }
                    self.visit_all(hir, &r.body.clone());
                }
                if let Some(b) = else_body {
                    self.visit_all(hir, &b.clone());
                }
                if let Some(b) = ensure_body {
                    self.visit_all(hir, &b.clone());
                }
            }
            HirNode::IntegerLit(_)
            | HirNode::BigIntegerLit { .. }
            | HirNode::RationalLit { .. }
            | HirNode::ImaginaryLit(_)
            | HirNode::FloatLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::NilLit
            | HirNode::BoxHandle(_)
            | HirNode::BoolLit(_)
            | HirNode::IvarRead(_)
            | HirNode::ClassVarRead(_)
            | HirNode::ClassRef(_)
            | HirNode::GlobalRead(_)
            | HirNode::QualifiedConstRead(..)
            | HirNode::ConstReadOrNil(..)
            | HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::NativeCrate(_)
            | HirNode::NativeFunc { .. }
            | HirNode::Redo
            | HirNode::Retry
            | HirNode::BlockGiven
            | HirNode::SelfRef => {}
        }
        hir[id] = node;
    }

    fn visit_group(&mut self, hir: &mut Hir, group: &mut MultiTargetGroup) {
        for t in group.before.iter_mut() {
            self.visit_target_inner(hir, t);
        }
        if let Some(Some(t)) = &mut group.splat {
            self.visit_target_inner(hir, t);
        }
        for t in group.after.iter_mut() {
            self.visit_target_inner(hir, t);
        }
    }

    fn visit_target(&mut self, hir: &mut Hir, target: &mut MultiTarget) {
        self.visit_target_inner(hir, target);
    }

    fn visit_target_inner(&mut self, hir: &mut Hir, target: &mut MultiTarget) {
        match target {
            MultiTarget::Local(n) => self.bind(n),
            // The hidden temp is bound once per multi-assignment and read
            // back inside `write_call` (as an ordinary `LocalRead`, renamed
            // by the `write_call` visit below) -- both sides must move
            // together.
            MultiTarget::Call {
                write_call,
                tmp_name,
            } => {
                self.bind(tmp_name);
                self.visit(hir, *write_call);
            }
            MultiTarget::Nested(group) => self.visit_group(hir, group),
            MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. } => {}
        }
    }

    fn visit_pattern(&mut self, hir: &mut Hir, pattern: &mut Pattern) {
        match pattern {
            Pattern::Bind(n) => self.bind(n),
            Pattern::Value(n) | Pattern::Pin(n) => self.visit(hir, *n),
            Pattern::ClassCheck(_) => {}
            Pattern::Range { start, end, .. } => {
                self.visit_opt(hir, start);
                self.visit_opt(hir, end);
            }
            Pattern::Or(pats) => {
                for p in pats.iter_mut() {
                    self.visit_pattern(hir, p);
                }
            }
            Pattern::Capture(inner, n) => {
                self.visit_pattern(hir, inner);
                self.bind(n);
            }
            Pattern::Array {
                pre, rest, post, ..
            } => {
                for p in pre.iter_mut().chain(post.iter_mut()) {
                    self.visit_pattern(hir, p);
                }
                if let Some(Some(n)) = rest {
                    self.bind(n);
                }
            }
            Pattern::Find {
                pre_rest,
                mid,
                post_rest,
                ..
            } => {
                if let Some(n) = pre_rest {
                    self.bind(n);
                }
                for p in mid.iter_mut() {
                    self.visit_pattern(hir, p);
                }
                if let Some(n) = post_rest {
                    self.bind(n);
                }
            }
            Pattern::Hash { pairs, rest, .. } => {
                for (key, val) in pairs.iter_mut() {
                    match val {
                        Some(p) => self.visit_pattern(hir, p),
                        // The `{key:}` shorthand binds a local NAMED `key`;
                        // the hash key itself must stay `key`, so a rename
                        // desugars the shorthand into an explicit
                        // `key: __fN_key` bind instead of touching the key.
                        None => {
                            if self.collecting {
                                self.names.insert(key.clone());
                            } else if self.names.contains(key.as_str()) {
                                *val = Some(Pattern::Bind(self.mangled(key)));
                            }
                        }
                    }
                }
                if let HashPatternRest::Rest(Some(n)) = rest {
                    self.bind(n);
                }
            }
        }
    }
}
