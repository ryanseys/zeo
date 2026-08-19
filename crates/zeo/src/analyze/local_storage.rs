//! `Locals` + `collect_locals`: the ordered local-name collection every
//! hoisting prelude and capture scan is built from. Pure `Compiler`+HIR
//! analysis -- the emission side (storage classes, declaration/read/write
//! shapes) stays in `codegen::hoisting`.

use crate::analyze::fastpath::is_spliced_block_body;
use crate::compiler::Compiler;
use crate::compiler::FSet;
use crate::hir::{ArrayElem, HirNode, NodeId, StrPart};

/// Mirrors `analyze::collect_ivars`'s traversal shape (recursing into every
/// sub-expression position a `LocalWrite` could appear in), but collects
/// local-variable names instead of ivar names, and additionally descends
/// into loop bodies and `MultiWrite`/`For` targets. `pub(crate)`: also used
/// by `codegen::captures` to compute which names are genuinely shared with
/// an escaping block (as opposed to owned only by that block -- see the
/// `Call` arm's docs below).
/// An ORDERED, deduped local-name accumulator -- [`collect_locals`]' output.
///
/// The order is load-bearing (it is the hoisting prelude's declaration
/// order), so the `Vec` stays; what rides beside it is a set, because the
/// dedup used to be `Vec::contains` -- a linear scan per assignment over a
/// list that grows to the scope's local count, run 4+ times per (class x
/// method) across the FLATTENED ancestry. Callers that then asked "is this
/// name in there?" per parameter were paying the same scan again, and now
/// ask the set.
#[derive(Default)]
pub(crate) struct Locals {
    seen: FSet<String>,
    order: Vec<String>,
}

impl Locals {
    fn add(&mut self, name: &str) {
        if !self.seen.contains(name) {
            self.seen.insert(name.to_string());
            self.order.push(name.to_string());
        }
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.seen.contains(name)
    }

    pub(crate) fn names(&self) -> &[String] {
        &self.order
    }

    pub(crate) fn into_names(self) -> Vec<String> {
        self.order
    }

    pub(crate) fn into_set(self) -> FSet<String> {
        self.seen
    }
}

pub(crate) fn collect_locals(compiler: &Compiler, id: NodeId, out: &mut Locals) {
    match &compiler.hir[id] {
        // An FFI wrapper body declares no hoistable locals (only param
        // reads).
        HirNode::Ffi(_) => {}
        // A lambda's own body is a fresh, independent local-variable scope
        // (like a non-`.times` escaping block) -- never hoisted into the
        // ENCLOSING scope's prelude.
        HirNode::Lambda {
            params: _,
            body: _,
            method_body: _,
        } => {}
        HirNode::LocalWrite(name, value) => {
            out.add(name);
            collect_locals(compiler, *value, out);
        }
        HirNode::IvarWrite(_, value) | HirNode::ClassVarWrite(_, value) => {
            collect_locals(compiler, *value, out)
        }
        HirNode::And(l, r)
        | HirNode::Or(l, r)
        | HirNode::FlipFlop {
            state: _,
            left: l,
            right: r,
            exclusive: _,
        } => {
            collect_locals(compiler, *l, out);
            collect_locals(compiler, *r, out);
        }
        HirNode::Defined(v) => collect_locals(compiler, *v, out),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_locals(compiler, *cond, out);
            for &n in then_body {
                collect_locals(compiler, n, out);
            }
            for &n in else_body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            if let Some(s) = subject {
                collect_locals(compiler, *s, out);
            }
            for (values, body) in arms {
                for e in values {
                    let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                    collect_locals(compiler, *v, out);
                }
                for &n in body {
                    collect_locals(compiler, n, out);
                }
            }
            for &n in else_body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::While {
            cond,
            body,
            negate: _,
            post: _,
        } => {
            collect_locals(compiler, *cond, out);
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::Loop { body } => {
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::For { target, iterable, body } => {
            target.for_each_node(&mut |n| collect_locals(compiler, n, out));
            target.for_each_local_name(&mut |n| {
                out.add(n);
            });
            collect_locals(compiler, *iterable, out);
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                collect_locals(compiler, *v, out);
            }
        }
        HirNode::Redo => {}
        HirNode::MultiWrite { targets, value } => {
            targets.for_each_local_name(&mut |n| {
                out.add(n);
            });
            targets.for_each_node(&mut |n| collect_locals(compiler, n, out));
            collect_locals(compiler, *value, out);
        }
        HirNode::GlobalWrite(_, value) => collect_locals(compiler, *value, out),
        HirNode::ConstWrite {
            scope: _,
            name: _,
            value,
        } => collect_locals(compiler, *value, out),
        HirNode::DynConstRead { scope, .. } => collect_locals(compiler, *scope, out),
        HirNode::DynConstWrite { scope, value, .. } => {
            collect_locals(compiler, *scope, out);
            collect_locals(compiler, *value, out);
        }
        HirNode::PreExec(body) | HirNode::Seq(body) => {
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: _,
        } => {
            if let Some(r) = receiver {
                collect_locals(compiler, *r, out);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                collect_locals(compiler, *n, out);
            }
            for n in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                collect_locals(compiler, n, out);
            }
            if let Some(b) = block {
                // An ESCAPING block (anything but the `.times` inline fast
                // path) is a genuinely separate Ruby scope now --
                // a local first introduced INSIDE one is fresh per
                // invocation (confirmed against real Ruby: a Proc's own
                // internal local resets on every separate `.call()`, it
                // does NOT persist like a captured one), so it must NOT be
                // hoisted into THIS (enclosing) scope's prelude at all --
                // `codegen::captures::block_captures`/`emit_proc_own_locals_prelude`
                // give it its own fresh declaration INSIDE the closure
                // instead. Only a name genuinely shared with code outside
                // the block (which this same traversal will still find,
                // since it walks the rest of the method) ends up hoisted
                // here. `.times` stays inline (unchanged): its block is
                // spliced directly into whichever Rust scope encloses it,
                // so its locals still need to be part of THAT hoisting pass.
                if is_spliced_block_body(
                    compiler,
                    *receiver,
                    name,
                    kwargs.is_empty(),
                    *b,
                ) {
                    collect_locals(compiler, *b, out);
                }
            }
            if let Some(b) = block_arg {
                collect_locals(compiler, *b, out);
            }
        }
        // A block on `.new` is an escaping block: its own locals stay inside
        // it, exactly as in the `Call` arm above.
        HirNode::New {
            class_name: _,
            args,
            kwargs,
            block: _,
        } => {
            for &a in args {
                collect_locals(compiler, a, out);
            }
            for a in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                collect_locals(compiler, a, out);
            }
        }
        HirNode::SuperCall {
            args,
            kwargs,
            zsuper: _,
            block: _,
            block_arg,
        } => {
            for a in args {
                collect_locals(compiler, a.node_id(), out);
            }
            for a in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                collect_locals(compiler, a, out);
            }
            if let Some(b) = block_arg {
                collect_locals(compiler, *b, out);
            }
        }
        HirNode::Block { params: _, body } => {
            // Params intentionally NOT collected -- see module docs.
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                collect_locals(compiler, *n, out);
            }
        }
        HirNode::HashLit(pairs) => {
            for n in pairs.iter().flat_map(|kw| kw.node_ids()) {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::RangeLit {
            start,
            end,
            exclusive: _,
        } => {
            if let Some(s) = start {
                collect_locals(compiler, *s, out);
            }
            if let Some(e) = end {
                collect_locals(compiler, *e, out);
            }
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    collect_locals(compiler, *n, out);
                }
            }
        }
        HirNode::Eval(body) | HirNode::BoxScope { box_id: _, body } => {
            for &n in body {
                collect_locals(compiler, n, out);
            }
        }
        HirNode::Yield(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                collect_locals(compiler, *n, out);
            }
        }
        HirNode::Raise(args, cause) => {
            for &a in args.iter().chain(crate::hir::raise_cause_node(cause).iter()) {
                collect_locals(compiler, a, out);
            }
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            collect_locals(compiler, *subject, out);
            for arm in arms {
                // A pattern's bound names leak into the enclosing METHOD
                // scope exactly like an `if`/`case` branch's locals do (no
                // new Ruby scope) -- collected here so `emit_hoisted_body`'s
                // prelude declares them, same as `MultiWrite`'s targets just
                // above.
                arm.pattern.for_each_bound_name(&mut |n| {
                    out.add(n);
                });
                arm.pattern.for_each_node(&mut |n| collect_locals(compiler, n, out));
                if let Some((g, _)) = arm.guard {
                    collect_locals(compiler, g, out);
                }
                for &n in &arm.body {
                    collect_locals(compiler, n, out);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    collect_locals(compiler, n, out);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            collect_locals(compiler, *subject, out);
            pattern.for_each_bound_name(&mut |n| {
                out.add(n);
            });
            pattern.for_each_node(&mut |n| collect_locals(compiler, n, out));
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            for &n in body {
                collect_locals(compiler, n, out);
            }
            for r in rescues {
                // A rescue binding (`=> e`) leaks into the enclosing METHOD
                // scope exactly like a `case/in` pattern's bound names do --
                // same treatment as that arm just above.
                if let Some(name) = &r.binding {
                    out.add(name);
                }
                for &n in &r.body {
                    collect_locals(compiler, n, out);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    collect_locals(compiler, n, out);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    collect_locals(compiler, n, out);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit {
            negative: _,
            digits: _,
        }
        | HirNode::RationalLit {
            negative: _,
            num_digits: _,
            den_digits: _,
        }
        // An imaginary literal's inner node is itself a numeric
        // literal by syntax -- a leaf for this walk's purposes.
        | HirNode::ImaginaryLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoxHandle(_)
        | HirNode::BoolLit(_)
        | HirNode::SelfRef
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::Undef(_)
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
        | HirNode::BlockGiven
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::DefHook { .. }
        | HirNode::MethodRedefine { .. }
        | HirNode::ClassDef {
            name: _,
            superclass: _,
            body: _,
            is_module: _,
        }
        | HirNode::DefMethod {
            name: _,
            params: _,
            body: _,
            is_class_method: _,
            visibility: _,
            is_def: _,
        } => {}
    }
}
