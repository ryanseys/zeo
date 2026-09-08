//! Per-splice-instance local-variable isolation for `require`/`load`
//!. Real Ruby gives every loaded file its OWN top-level local
//! scope (each file is compiled as its own iseq with its own local table --
//! `vm_set_top_stack` sizes the frame per-iseq; verified empirically that
//! locals leak in NEITHER direction between files), but the HIR splice
//! merges every file into one `Program` scope, which would silently share
//! same-named locals across files (`eval`'s splice shares scope too, but
//! there sharing is the CORRECT semantics). Fix: after lowering a spliced
//! file's own statements, rename that file's top-level locals to
//! `__f<file_id>_<name>` -- `file_id` is the SPLICE INSTANCE index, not the
//! canonical file, because `load` re-splices the same file and real Ruby
//! gives each `load` execution a fresh local scope (and `Ruby::Box`'s
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
//!    body (unlike `analyze::local_storage::collect_locals`, which deliberately
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
    Pattern,
};
use std::collections::HashSet;

/// Renames `roots`' (one spliced file's own top-level statements --
/// deliberately NOT including any child file's statements spliced between
/// them, which are separate roots already renamed by their own pass and
/// unreachable from these subtrees) local variables to
/// `__f<file_id>_<name>`. Two passes over the same walker: collect the
/// file's assigned-name set, then rename every reference to it.
pub fn isolate_file_locals(hir: &mut Hir, roots: &[NodeId], file_id: usize) {
    isolate_locals(hir, roots, format!("__f{file_id}_"));
}

/// The same isolation for a class body that must run as a BLOCK -- `class
/// SidekiqAdapter < parent`, whose superclass is a local, so the class is built
/// by `Class.new(parent) { ... }`.
///
/// Ruby gives a class body its own local scope; a block shares its enclosing
/// one. Renaming every name the body assigns restores the difference in both
/// directions, and by the same argument the file splice rests on: prism scopes
/// the class body separately, so a bare name the body never assigns already
/// lowered as a CALL there, not as a read of the outer local. Nothing outside
/// can see what the body binds, and nothing the body reads was ever the outer
/// binding.
///
/// `seq` only has to be unique per body; the node arena's length at the time is.
pub fn isolate_runtime_class_locals(hir: &mut Hir, roots: &[NodeId], seq: usize) {
    isolate_locals(hir, roots, format!("__rc{seq}_"));
}

fn isolate_locals(hir: &mut Hir, roots: &[NodeId], prefix: String) {
    let mut w = Walker {
        collecting: true,
        names: HashSet::new(),
        prefix,
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
    /// What every renamed name is prefixed with -- one per isolated scope.
    prefix: String,
}

impl Walker {
    fn mangled(&self, name: &str) -> String {
        format!("{}{}", self.prefix, name)
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
        for n in params.bound_names() {
            if let Some(taken) = self.names.take(n.as_str()) {
                suspended.push(taken);
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
        let mut descend = false;
        match &mut node {
            HirNode::LocalRead(name) => self.read(name),
            // An `attach_function` wrapper body: visit its argument reads
            // (synthetic `__ffi_a*` params) so renaming stays consistent.
            HirNode::Ffi(call) => {
                let args: Vec<NodeId> = call.args.iter().map(|(a, _)| *a).collect();
                for a in args {
                    self.visit(hir, a);
                }
            }
            HirNode::LocalWrite(name, value) => {
                self.bind(name);
                self.visit(hir, *value);
            }
            // PURE DESCENT: every child is visited in this same rename scope.
            // Handled after the match rather than here, because descending
            // needs `node` back immutably -- which is also what lets it go
            // through `for_each_child` instead of cloning each body vector to
            // end the `&mut node` borrow, as these arms used to.
            //
            // Listed by variant rather than behind a `_`, deliberately: this
            // walk REWRITES names, and a new `HirNode` carrying one must fail
            // to compile here rather than silently inherit plain descent.
            HirNode::Program(_)
            | HirNode::PreExec(_)
            | HirNode::Seq(_)
            | HirNode::BoxScope { .. }
            | HirNode::And(..)
            | HirNode::Or(..)
            | HirNode::FlipFlop { .. }
            | HirNode::Defined(_)
            | HirNode::NotNil(_)
            | HirNode::If { .. }
            | HirNode::CaseWhen { .. }
            | HirNode::While { .. }
            | HirNode::Loop { .. }
            | HirNode::Break(_)
            | HirNode::Next(_)
            | HirNode::Return(_)
            | HirNode::ArrayLit(_)
            | HirNode::HashLit(_)
            | HirNode::RangeLit { .. }
            | HirNode::StringLit(_)
            | HirNode::RegexpLit(..)
            | HirNode::ClassVarWrite(..)
            | HirNode::ConstWrite { .. }
            | HirNode::GlobalWrite(..)
            | HirNode::IvarWrite(..)
            | HirNode::DynConstRead { .. }
            | HirNode::DynConstWrite { .. }
            | HirNode::New { .. }
            | HirNode::SuperCall { .. }
            | HirNode::Yield(_) => descend = true,
            HirNode::Call {
                receiver,
                name: _,
                args,
                kwargs,
                block,
                block_arg,
                safe: _,
            } => {
                self.visit_opt(hir, receiver);
                for a in args.iter() {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    self.visit(hir, *n);
                }
                for n in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                    self.visit(hir, n);
                }
                self.visit_opt(hir, block);
                self.visit_opt(hir, block_arg);
            }
            HirNode::Block { params, body }
            | HirNode::Lambda {
                params,
                body,
                method_body: _,
            } => {
                let suspended = self.suspend_params(params);
                // The IMPLICIT block-locals are the one own-name list
                // `bound_names` does not carry, so they are not suspended --
                // and observation 2 says renaming a block-own name is
                // harmless as long as it happens at EVERY site. This list is
                // a site: it names what the prelude resets to nil per
                // invocation, and a stale spelling there names a local no
                // scope hoisted.
                if !self.collecting {
                    for n in &mut params.implicit_block_locals {
                        if self.names.contains(n.as_str()) {
                            *n = format!("{}{}", self.prefix, n);
                        }
                    }
                }
                for d in params.default_ids() {
                    self.visit(hir, d);
                }
                self.visit_all(hir, &body.clone());
                self.names.extend(suspended);
            }
            // Genuinely fresh Ruby scopes -- can't capture file-level
            // locals, never descended into (see module docs).
            HirNode::ClassDef {
                name: _,
                superclass: _,
                body: _,
                is_module: _,
            }
            // ... but `define_method(:x) { ... }` lowers to a `DefMethod` with
            // `is_def: false`, and its body was written as a BLOCK. Ruby lets
            // that block capture the enclosing scope -- that is the whole point
            // of the form, and prism classifies a captured name inside it as a
            // read, not a call -- so the module doc's "fresh scope, nothing to
            // rename" reasoning covers only a real `def`.
            | HirNode::DefMethod {
                is_def: true,
                name: _,
                params: _,
                body: _,
                is_class_method: _,
                visibility: _,
            } => {}
            HirNode::DefMethod {
                is_def: false,
                params,
                body,
                ..
            } => {
                let params = params.clone();
                let body = body.clone();
                let suspended = self.suspend_params(&params);
                self.visit_all(hir, &body);
                self.names.extend(suspended);
            }
            HirNode::For {
                target,
                iterable,
                body,
            } => {
                self.visit_target(hir, target);
                self.visit(hir, *iterable);
                self.visit_all(hir, &body.clone());
            }
            HirNode::MultiWrite { targets, value } => {
                self.visit_group(hir, targets);
                self.visit(hir, *value);
            }
            HirNode::Raise(args, cause) => {
                // The `cause:` expression can reference locals, so it must be
                // renamed along with the operands -- omitting it would leave a
                // stale name behind after per-file local isolation.
                let mut ids = args.clone();
                if let crate::hir::RaiseCause::Explicit(c) = cause {
                    ids.push(*c);
                }
                self.visit_all(hir, &ids)
            }
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
                    // Splatted exception exprs (`rescue *errs`) read the OUTER
                    // scope -- visit them before the clause binds `=> e`.
                    self.visit_all(hir, &r.splats.clone());
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
            | HirNode::BigIntegerLit {
                negative: _,
                digits: _,
            }
            | HirNode::RationalLit {
                negative: _,
                num_digits: _,
                den_digits: _,
            }
            // `ImaginaryLit`'s child is prism's `numeric()` -- always a
            // numeric literal, so it holds no name to rename.
            | HirNode::ImaginaryLit(_)
            | HirNode::FloatLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::NilLit
            | HirNode::BoxHandle(_)
            | HirNode::FileEnd(_)
            | HirNode::BoolLit(_)
            | HirNode::IvarRead(_)
            | HirNode::ClassVarRead(_)
            | HirNode::ClassRef(_)
            | HirNode::GlobalRead(_)
            | HirNode::LastMatchRef(_)
            | HirNode::Undef(_)
            | HirNode::FeatureLoaded { .. }
            | HirNode::CExtLoaded { .. }
            | HirNode::ClassMethodUndef(_)
            | HirNode::AliasMethod {
                new_name: _,
                old_name: _,
                is_class_method: _,
            }
            | HirNode::MethodVisibility {
                name: _,
                visibility: _,
            }
            | HirNode::ClassMethodVisibility {
                name: _,
                visibility: _,
            }
            | HirNode::ModuleFunction(_)
            | HirNode::ConstantVisibility { .. }
            | HirNode::AliasGlobal(_, _)
            | HirNode::QualifiedConstRead(_, _)
            | HirNode::ConstReadOrNil(_, _)
            | HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::ClassMethodPrepend(_)
            | HirNode::Refine { .. }
            | HirNode::Using(_)
            | HirNode::DefHook { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::MethodReveal(..)
            | HirNode::Redo
            | HirNode::Retry
            | HirNode::BlockGiven
            | HirNode::SelfRef => {}
        }
        // `node` is checked out of the arena above, so descending through it
        // borrows nothing the recursion touches -- no copy of the child list
        // is needed to satisfy the borrow checker, which is what the arms
        // this replaces were cloning a body vector each to do.
        if descend {
            node.for_each_child(&mut |c| self.visit(hir, c));
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
            Pattern::Range {
                start,
                end,
                exclusive: _,
            } => {
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
                constant: _,
                pre,
                rest,
                post,
            } => {
                for p in pre.iter_mut().chain(post.iter_mut()) {
                    self.visit_pattern(hir, p);
                }
                if let Some(Some(n)) = rest {
                    self.bind(n);
                }
            }
            Pattern::Find {
                constant: _,
                pre_rest,
                mid,
                post_rest,
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
            Pattern::Hash {
                constant: _,
                pairs,
                rest,
            } => {
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
