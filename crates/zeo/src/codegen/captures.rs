//! Free-variable/`self` capture analysis for escaping blocks. See
//! `codegen::call::is_times_fast_path`'s docs: there is no genuine "escape
//! analysis" decision procedure here -- a block escapes (becomes a real,
//! heap-allocated `Proc`) iff it's NOT the `.times` inline fast path, which
//! is exactly the same check `codegen::call::dispatch` already makes. This
//! module answers a different, purely mechanical question: for a scope
//! (method/top-level body) containing zero or more escaping blocks, which
//! enclosing LOCAL NAMES (and possibly `self`) do they collectively need to
//! capture, so `codegen::hoisting` knows which locals need the `Captured`
//! (`Arc<parking_lot::Mutex<RubyValue>>`) storage class instead of a plain hoisted `let
//! mut`?

use super::call::is_inline_block_fast_path;
use crate::compiler::Compiler;
use crate::hir::{ArrayElem, HirNode, NodeId, Params, StrPart};
use std::collections::HashSet;

#[derive(Default)]
pub struct Captures {
    /// Enclosing-scope local/parameter names referenced (read or written)
    /// inside some escaping block, excluding every block's own params
    /// (that block's own, AND every ancestor inline `.times` block's --
    /// see `walk`'s `param_exclusions`, which is what makes this
    /// exclusion correct even for a name shadowed two levels deep).
    pub locals: HashSet<String>,
    /// The subset of `locals` that is ASSIGNED somewhere in the walked
    /// subtree (`LocalWrite`, a multi-assign/`for` target, a pattern's
    /// bound name, a rescue binding) -- as opposed to only ever read.
    /// `emit_proc_or_lambda_value`'s nested-Proc guard consumes this: a
    /// name a nested block assigns is (at worst) that block's own fresh
    /// local, while a name NOBODY under this block assigns can only be a
    /// read of some enclosing block's plain (non-cell) per-invocation
    /// `let`, the one genuinely unsupported capture shape.
    pub assigned: HashSet<String>,
    /// Whether any escaping block needs the receiver captured: an
    /// `@ivar` reference, a bare/explicit `self`, or a receiverless call
    /// resolving to a method on the enclosing class (`walk`'s `self_class`
    /// -- an implicit-self dispatch).
    pub self_captured: bool,
}

/// Every name a `Params` list itself binds -- the exclusion set for "is this
/// name a BLOCK's OWN parameter, not something captured from its enclosing
/// scope". Also reused by `codegen::params::emit_prologue` to decide which of
/// a METHOD's own parameter names need the additional
/// `Arc<parking_lot::Mutex<_>>`-wrapping shadow (when captured by one of ITS
/// OWN escaping blocks).
///
/// Delegates to `Params::bound_names` rather than re-enumerating the param
/// kinds: a separate copy of that walk would miss destructuring params --
/// the names inside `|(a, b)|` are bound by the params but live in
/// `Params::destructures`, so they would be classified as ordinary locals
/// rather than captured ones --
/// silently making `def m((a, b)); -> { a += 1 }; end` read a nil `a` inside
/// the block. One enumeration, one place to update.
pub(super) fn own_param_names(params: &Params) -> HashSet<String> {
    params.bound_names().into_iter().collect()
}

/// Scans a WHOLE scope's body (a method's or the top level's) for every
/// escaping block, unioning their capture requirements -- but ONLY names
/// genuinely shared with the enclosing scope: assigned somewhere outside
/// the block(s), OR one of the enclosing method's OWN PARAMETERS
/// (`params`). The params half was missing until the `set`
/// package surfaced it: a parameter referenced ONLY inside an escaping
/// block (`def add_all(n); each { |x| puts x + n }; end`, or an
/// Enumerable-shaped `&blk` forwarded into an inner block) failed the
/// assigned-outside filter and was misclassified as a block-OWN local --
/// silently re-declared `Nil` inside the closure, shadowing the real
/// argument. Verified against real Ruby both ways: the parameter IS shared
/// enclosing-scope state, while `proc { |x| tmp ||= 0; tmp += x }.call`
/// genuinely resets `tmp` per call (NOT a capture when nothing outside the
/// block uses the name; see `hoisting::collect_locals`'s `Call` arm, which
/// does not descend into an escaping block's body, making its result
/// exactly "names used outside any escaping block").
/// `self` has no such distinction (an ivar always means the same object),
/// so `self_captured` is passed through unfiltered.
pub fn collect_escaping_captures(
    compiler: &Compiler,
    body: &[NodeId],
    params: &Params,
    self_class: Option<crate::compiler::ClassId>,
) -> Captures {
    let mut raw = Captures::default();
    for &n in body {
        walk(compiler, n, false, &HashSet::new(), &mut raw, self_class);
    }
    let mut outer_names = Vec::new();
    for &n in body {
        super::hoisting::collect_locals(compiler, n, &mut outer_names);
    }
    let mut outer: HashSet<String> = outer_names.into_iter().collect();
    outer.extend(own_param_names(params));
    Captures {
        locals: raw
            .locals
            .into_iter()
            .filter(|n| outer.contains(n))
            .collect(),
        assigned: raw.assigned,
        self_captured: raw.self_captured,
    }
}

/// The PRECISE capture set for ONE SPECIFIC escaping block, given its own
/// declared params -- unlike `collect_escaping_captures` (which unions every
/// escaping block in a whole scope, used to decide `codegen::hoisting`'s
/// storage classes), this is what `codegen::call`'s Proc-construction site
/// needs: exactly which of the enclosing scope's (already-`Captured`)
/// names THIS closure must clone-capture, so it doesn't clone names it
/// never references (which would otherwise show up as unused-variable
/// warnings in the generated program).
pub fn block_captures(
    compiler: &Compiler,
    params: &Params,
    body: &[NodeId],
    self_class: Option<crate::compiler::ClassId>,
) -> Captures {
    let mut caps = Captures::default();
    let own = own_param_names(params);
    for &n in body {
        walk(compiler, n, true, &own, &mut caps, self_class);
    }
    caps
}

/// Whether anything in this scope needs the scope itself as a value: a bare,
/// receiver-less, argument-less `binding` (the only form that names the
/// CURRENT scope), or a receiver-less dynamic `eval`, which CRuby runs in the
/// caller's own frame and zeo therefore compiles into a Binding of it.
/// Descends through blocks and lambdas (a `binding` taken inside one still
/// exposes the enclosing scope's locals) but stops at a `def`/`class` body,
/// which is a scope of its own.
fn scope_calls_binding(compiler: &Compiler, id: NodeId) -> bool {
    match &compiler.hir[id] {
        HirNode::DefMethod { .. } | HirNode::ClassDef { .. } => return false,
        HirNode::Call {
            receiver: None,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: _,
        } if (name == "binding" && args.is_empty()
            || name == "eval" && (1..=4).contains(&args.len()))
            && kwargs.is_empty()
            && block.is_none()
            && block_arg.is_none() =>
        {
            return true;
        }
        _ => {}
    }
    let mut found = false;
    compiler.hir[id].for_each_child(&mut |n| {
        if !found {
            found = scope_calls_binding(compiler, n);
        }
    });
    found
}

/// A scope that materializes a `Binding` has to hand out every one of its own
/// locals BY REFERENCE, so this promotes them all to the `Captured` cell
/// storage class (`codegen::hoisting::LocalStorage`) and answers the ordered
/// name list the `binding` call site emits -- CRuby's `local_variables` order:
/// parameters first, then locals by first assignment.
///
/// `None`, and no promotion at all, when nothing in the scope calls `binding`
/// -- which is every scope in almost every program, so the cost is paid only
/// by the frames that actually ask for it. `force` is `TOPLEVEL_BINDING`'s
/// door in: the top-level frame has to be materialized when anything anywhere
/// can READ that constant, with no `binding` call in sight.
///
/// An AOT-spliced `eval("literal")` (`HirNode::Eval`) gets the NAME LIST
/// without the promotion: `codegen::call` reads it to resolve a bare name in
/// the snippet back to the scope's local (see `Ctx::in_eval_splice`), and
/// nothing there needs a cell.
pub fn binding_scope_names(
    compiler: &Compiler,
    body: &[NodeId],
    params: &Params,
    captures: &mut Captures,
    force: bool,
) -> Option<std::rc::Rc<Vec<String>>> {
    let promote = force || body.iter().any(|&n| scope_calls_binding(compiler, n));
    if !promote && !body.iter().any(|&n| scope_splices_eval(compiler, n)) {
        return None;
    }
    let mut names = params.bound_names();
    let mut assigned = Vec::new();
    for &n in body {
        super::hoisting::collect_locals(compiler, n, &mut assigned);
    }
    for n in assigned {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    if promote {
        captures.locals.extend(names.iter().cloned());
    }
    Some(std::rc::Rc::new(names))
}

/// Whether this scope contains an AOT-spliced `eval("literal")` -- the shape
/// that needs the name list but no cell promotion. Same walk boundary as
/// [`scope_calls_binding`].
fn scope_splices_eval(compiler: &Compiler, id: NodeId) -> bool {
    match &compiler.hir[id] {
        HirNode::DefMethod { .. } | HirNode::ClassDef { .. } => return false,
        HirNode::Eval(_) => return true,
        _ => {}
    }
    let mut found = false;
    compiler.hir[id].for_each_child(&mut |n| {
        if !found {
            found = scope_splices_eval(compiler, n);
        }
    });
    found
}

/// Whether `body` (a WHOLE method's own body) contains a `return` that is
/// lexically INSIDE an escaping (non-inline, non-lambda) block -- the only
/// shape that raises a `Signal::Return` homed to THIS method, which its
/// `codegen::mod` per-method catch must intercept.
///
/// A `return` written directly in the method body (or in an inline `.times`
/// block, which shares the method's Rust scope) compiles to a literal Rust
/// `return`, needs no catch, and does not count. A `return` inside a real
/// escaping block compiles to `Err(Signal::Return(..))` homed here. A `return`
/// inside a nested LAMBDA belongs to the lambda (it catches its own), so the
/// walk stops at a lambda boundary.
///
/// Confirmed the hard way this can't be "any escaping block, return or not":
/// a pure relay like `each_item` -- which yields to a caller's block and has
/// its OWN escaping block but no `return` lexically inside it -- must NOT
/// catch, or it intercepts a `return` homed to the CALLER (turning the
/// caller's non-local return into a normal one). See the
/// `inline_yield_method_with_return` conformance case.
pub fn body_contains_escaping_return(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter()
        .any(|&n| node_contains_escaping_return(compiler, n, false))
}

/// Body-list helper threading the `in_escaping` state (whether we are already
/// lexically inside an escaping block).
fn body_contains_escaping_return_in(compiler: &Compiler, body: &[NodeId], in_escaping: bool) -> bool {
    body.iter()
        .any(|&n| node_contains_escaping_return(compiler, n, in_escaping))
}

fn node_contains_escaping_return(compiler: &Compiler, id: NodeId, in_escaping: bool) -> bool {
    let sub = |n: NodeId| node_contains_escaping_return(compiler, n, in_escaping);
    match &compiler.hir[id] {
        // An FFI wrapper body contains no block.
        HirNode::Ffi(_) => false,
        // A lambda literal catches its own `Signal::Return` unconditionally --
        // a `return` inside it belongs to the lambda, never the enclosing
        // method (see `hir::HirNode::Lambda`'s docs), so the walk stops here.
        HirNode::Lambda {
            params: _,
            body: _,
            method_body: _,
        } => false,
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: _,
        } => {
            if let Some(b) = block {
                let HirNode::Block { params: _, body } = &compiler.hir[*b] else {
                    panic!("internal error: a Block node should only be reached via the Call that invokes it");
                };
                // An escaping (non-inline) block's body runs as a proc homed to
                // this method, so a `return` anywhere inside it needs the catch;
                // an inline `.times` block shares this scope, so its `return` is
                // literal and only counts if we were already inside one.
                let block_escaping =
                    !is_inline_block_fast_path(compiler, *receiver, name, kwargs.is_empty());
                if body_contains_escaping_return_in(compiler, body, in_escaping || block_escaping) {
                    return true;
                }
            }
            receiver.is_some_and(&sub)
                || args.iter().any(|a| {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    sub(*n)
                })
                || kwargs.iter().flat_map(|kw| kw.node_ids()).any(&sub)
                || block_arg.is_some_and(&sub)
        }
        HirNode::LocalWrite(_, v) | HirNode::IvarWrite(_, v) | HirNode::ClassVarWrite(_, v) | HirNode::Defined(v) => {
            sub(*v)
        }
        HirNode::And(l, r)
        | HirNode::Or(l, r)
        | HirNode::FlipFlop {
            state: _,
            left: l,
            right: r,
            exclusive: _,
        } => sub(*l) || sub(*r),
        HirNode::If { cond, then_body, else_body } => {
            sub(*cond)
                || body_contains_escaping_return_in(compiler, then_body, in_escaping)
                || body_contains_escaping_return_in(compiler, else_body, in_escaping)
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            subject.is_some_and(&sub)
                || arms.iter().any(|(values, body)| {
                    values.iter().any(|e| {
                        let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                        sub(*v)
                    })
                        || body_contains_escaping_return_in(compiler, body, in_escaping)
                })
                || body_contains_escaping_return_in(compiler, else_body, in_escaping)
        }
        HirNode::While {
            cond,
            body,
            negate: _,
            post: _,
        } => {
            sub(*cond) || body_contains_escaping_return_in(compiler, body, in_escaping)
        }
        HirNode::Loop { body } => body_contains_escaping_return_in(compiler, body, in_escaping),
        HirNode::For { target, iterable, body } => {
            let mut found = false;
            target.for_each_node(&mut |n| found |= sub(n));
            found
                || sub(*iterable)
                || body_contains_escaping_return_in(compiler, body, in_escaping)
        }
        // The one that matters: a `return` inside an escaping block needs the
        // catch. Its value expression is walked too (it may hold another).
        HirNode::Return(v) => in_escaping || v.is_some_and(&sub),
        HirNode::Break(v) | HirNode::Next(v) => v.is_some_and(&sub),
        HirNode::MultiWrite { targets, value } => {
            let mut found = false;
            targets.for_each_node(&mut |n| found |= sub(n));
            found || sub(*value)
        }
        HirNode::GlobalWrite(_, value) => sub(*value),
        HirNode::ConstWrite {
            scope: _,
            name: _,
            value,
        } => sub(*value),
        HirNode::Yield(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            sub(*n)
        }),
        HirNode::Raise(args, cause) => args
            .iter()
            .chain(crate::hir::raise_cause_node(cause).iter())
            .any(|&a| sub(a)),
        HirNode::PreExec(body)
        | HirNode::Seq(body)
        | HirNode::Eval(body)
        | HirNode::BoxScope { box_id: _, body } => {
            body_contains_escaping_return_in(compiler, body, in_escaping)
        }
        // A block attached to `.new` / `super` runs as a proc that may be homed
        // to this method, so its body is walked as escaping.
        HirNode::New {
            class_name: _,
            args,
            kwargs,
            block,
        } => {
            block.is_some_and(|b| {
                matches!(&compiler.hir[b], HirNode::Block { params: _, body }
                    if body_contains_escaping_return_in(compiler, body, true))
            }) || args.iter().any(|&a| sub(a))
                || kwargs.iter().flat_map(|kw| kw.node_ids()).any(&sub)
        }
        HirNode::SuperCall {
            args,
            kwargs,
            zsuper: _,
            block,
            block_arg,
        } => {
            block.is_some_and(|b| {
                matches!(&compiler.hir[b], HirNode::Block { params: _, body }
                    if body_contains_escaping_return_in(compiler, body, true))
            }) || args.iter().any(|a| sub(a.node_id()))
                || kwargs.iter().flat_map(|kw| kw.node_ids()).any(&sub)
                || block_arg.is_some_and(&sub)
        }
        HirNode::ArrayLit(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            sub(*n)
        }),
        HirNode::HashLit(pairs) => pairs.iter().flat_map(|kw| kw.node_ids()).any(&sub),
        HirNode::RangeLit {
            start,
            end,
            exclusive: _,
        } => start.is_some_and(&sub) || end.is_some_and(&sub),
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => parts.iter().any(|p| match p {
            StrPart::Interp(n) => sub(*n),
            StrPart::Lit(_) | StrPart::Bytes(_) => false,
        }),
        HirNode::CaseIn { subject, arms, else_body } => {
            sub(*subject)
                || arms.iter().any(|arm| {
                    let mut pattern_found = false;
                    arm.pattern.for_each_node(&mut |n| pattern_found |= sub(n));
                    pattern_found
                        || arm.guard.is_some_and(|(g, _)| sub(g))
                        || body_contains_escaping_return_in(compiler, &arm.body, in_escaping)
                })
                || else_body
                    .as_deref()
                    .is_some_and(|b| body_contains_escaping_return_in(compiler, b, in_escaping))
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            if sub(*subject) {
                return true;
            }
            let mut found = false;
            pattern.for_each_node(&mut |n| found |= sub(n));
            found
        }
        HirNode::Begin { body, rescues, else_body, ensure_body } => {
            body_contains_escaping_return_in(compiler, body, in_escaping)
                || rescues.iter().any(|r| body_contains_escaping_return_in(compiler, &r.body, in_escaping))
                || else_body.as_deref().is_some_and(|b| body_contains_escaping_return_in(compiler, b, in_escaping))
                || ensure_body.as_deref().is_some_and(|b| body_contains_escaping_return_in(compiler, b, in_escaping))
        }
        HirNode::Retry
        | HirNode::Redo
        | HirNode::BlockGiven
        | HirNode::SelfRef
        | HirNode::Block { params: _, body: _ }
        | HirNode::Program(_)
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
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::Undef(_)
        | HirNode::AliasMethod {
            new_name: _,
            old_name: _,
            is_class_method: _,
        }
        | HirNode::MethodVisibility {
            name: _,
            visibility: _,
        }
        | HirNode::ModuleFunction(_)
        | HirNode::AliasGlobal(_, _)
        | HirNode::QualifiedConstRead(_, _)
        | HirNode::ConstReadOrNil(_, _)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
        } => false,
    }
}

/// Whether `body` lexically contains a `begin`/`rescue`/`else`/`ensure`
/// construct ANYWHERE (including inside a `.times` inline block, which
/// shares this same Rust function scope, and inside a real escaping block --
/// redundant with that case already being caught by
/// `body_contains_escaping_block` at the call site, but harmless to also
/// detect here). See `codegen::exceptions::emit_begin`'s docs: `begin`'s own
/// body/rescue-clause bodies/`else` are captured via a NON-move,
/// immediately-invoked closure to test their `Result` against `rescue`
/// clauses -- a `return` lexically inside one raises `Signal::Return`
/// instead of a literal Rust `return` (since a literal `return` there would
/// only exit that inner closure, not the enclosing method), so the SAME
/// per-method catch `codegen::mod::emit_class` installs for an escaping
/// block must also trigger here, independent of whether any block is
/// involved at all.
pub fn body_contains_begin(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter().any(|&n| node_contains_begin(compiler, n))
}

fn node_contains_begin(compiler: &Compiler, id: NodeId) -> bool {
    match &compiler.hir[id] {
        // An FFI wrapper body contains no `begin`.
        HirNode::Ffi(_) => false,
        HirNode::Begin {
            body: _,
            rescues: _,
            else_body: _,
            ensure_body: _,
        } => true,
        // Same reasoning as `node_contains_escaping_block`'s `Lambda` arm --
        // a lambda's own body is a fully self-contained closure boundary,
        // so a `begin`/`rescue` lexically inside one never requires the
        // ENCLOSING method to install its own `Signal::Return` catch.
        HirNode::Lambda {
            params: _,
            body: _,
            method_body: _,
        } => false,
        HirNode::Call {
            receiver,
            name: _,
            args,
            kwargs,
            block,
            block_arg,
            safe: _,
        } => {
            receiver.is_some_and(|r| node_contains_begin(compiler, r))
                || args.iter().any(|a| {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    node_contains_begin(compiler, *n)
                })
                || kwargs
                    .iter()
                    .flat_map(|kw| kw.node_ids())
                    .any(|n| node_contains_begin(compiler, n))
                || block_arg.is_some_and(|b| node_contains_begin(compiler, b))
                || block.is_some_and(|b| {
                    let HirNode::Block { params: _, body } = &compiler.hir[b] else {
                        panic!("internal error: a Block node should only be reached via the Call that invokes it");
                    };
                    body_contains_begin(compiler, body)
                })
        }
        HirNode::LocalWrite(_, v) | HirNode::IvarWrite(_, v) | HirNode::ClassVarWrite(_, v) | HirNode::Defined(v) => {
            node_contains_begin(compiler, *v)
        }
        HirNode::And(l, r)
        | HirNode::Or(l, r)
        | HirNode::FlipFlop {
            state: _,
            left: l,
            right: r,
            exclusive: _,
        } => {
            node_contains_begin(compiler, *l) || node_contains_begin(compiler, *r)
        }
        HirNode::If { cond, then_body, else_body } => {
            node_contains_begin(compiler, *cond)
                || body_contains_begin(compiler, then_body)
                || body_contains_begin(compiler, else_body)
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            subject.is_some_and(|s| node_contains_begin(compiler, s))
                || arms.iter().any(|(values, body)| {
                    values.iter().any(|e| {
                        let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                        node_contains_begin(compiler, *v)
                    }) || body_contains_begin(compiler, body)
                })
                || body_contains_begin(compiler, else_body)
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            node_contains_begin(compiler, *subject)
                || arms.iter().any(|arm| {
                    let mut pattern_found = false;
                    arm.pattern.for_each_node(&mut |n| pattern_found |= node_contains_begin(compiler, n));
                    pattern_found
                        || arm.guard.is_some_and(|(g, _)| node_contains_begin(compiler, g))
                        || body_contains_begin(compiler, &arm.body)
                })
                || else_body.as_deref().is_some_and(|b| body_contains_begin(compiler, b))
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            let mut found = node_contains_begin(compiler, *subject);
            pattern.for_each_node(&mut |n| found |= node_contains_begin(compiler, n));
            found
        }
        HirNode::While {
            cond,
            body,
            negate: _,
            post: _,
        } => node_contains_begin(compiler, *cond) || body_contains_begin(compiler, body),
        HirNode::Loop { body } => body_contains_begin(compiler, body),
        HirNode::For { target, iterable, body } => {
            let mut found = false;
            target.for_each_node(&mut |n| found |= node_contains_begin(compiler, n));
            found || node_contains_begin(compiler, *iterable) || body_contains_begin(compiler, body)
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => v.is_some_and(|v| node_contains_begin(compiler, v)),
        HirNode::MultiWrite { targets, value } => {
            let mut found = false;
            targets.for_each_node(&mut |n| found |= node_contains_begin(compiler, n));
            found || node_contains_begin(compiler, *value)
        }
        HirNode::GlobalWrite(_, value) => node_contains_begin(compiler, *value),
        HirNode::ConstWrite {
            scope: _,
            name: _,
            value,
        } => node_contains_begin(compiler, *value),
        HirNode::Yield(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_begin(compiler, *n)
        }),
        HirNode::Raise(args, cause) => args
            .iter()
            .chain(crate::hir::raise_cause_node(cause).iter())
            .any(|&a| node_contains_begin(compiler, a)),
        HirNode::New {
            class_name: _,
            args,
            kwargs,
            block,
        } => {
            args.iter().any(|&a| node_contains_begin(compiler, a))
                || kwargs
                    .iter()
                    .flat_map(|kw| kw.node_ids())
                    .any(|a| node_contains_begin(compiler, a))
                || block.is_some_and(|b| match &compiler.hir[b] {
                    HirNode::Block { params: _, body } => body_contains_begin(compiler, body),
                    _ => false,
                })
        }
        HirNode::SuperCall {
            args,
            kwargs,
            zsuper: _,
            block,
            block_arg,
        } => {
            args.iter().any(|a| node_contains_begin(compiler, a.node_id()))
                || kwargs
                    .iter()
                    .flat_map(|kw| kw.node_ids())
                    .any(|a| node_contains_begin(compiler, a))
                || block_arg.is_some_and(|b| node_contains_begin(compiler, b))
                || block.is_some_and(|b| match &compiler.hir[b] {
                    HirNode::Block { params: _, body } => body_contains_begin(compiler, body),
                    _ => false,
                })
        }
        HirNode::ArrayLit(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_begin(compiler, *n)
        }),
        HirNode::HashLit(pairs) => {
            pairs.iter().flat_map(|kw| kw.node_ids()).any(|n| node_contains_begin(compiler, n))
        }
        HirNode::RangeLit {
            start,
            end,
            exclusive: _,
        } => {
            start.is_some_and(|s| node_contains_begin(compiler, s))
                || end.is_some_and(|e| node_contains_begin(compiler, e))
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => parts.iter().any(|p| match p {
            StrPart::Interp(n) => node_contains_begin(compiler, *n),
            StrPart::Lit(_) | StrPart::Bytes(_) => false,
        }),
        HirNode::PreExec(body)
        | HirNode::Seq(body)
        | HirNode::Eval(body)
        | HirNode::BoxScope { box_id: _, body } => body_contains_begin(compiler, body),
        HirNode::Retry
        | HirNode::Redo
        | HirNode::BlockGiven
        | HirNode::SelfRef
        | HirNode::Block { params: _, body: _ }
        | HirNode::Program(_)
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
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::Undef(_)
        | HirNode::AliasMethod {
            new_name: _,
            old_name: _,
            is_class_method: _,
        }
        | HirNode::MethodVisibility {
            name: _,
            visibility: _,
        }
        | HirNode::ModuleFunction(_)
        | HirNode::AliasGlobal(_, _)
        | HirNode::QualifiedConstRead(_, _)
        | HirNode::ConstReadOrNil(_, _)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
        } => false,
    }
}

/// `in_escaping`: whether the walk has descended into some escaping
/// block's own body (directly, or through an inline `.times` block nested
/// inside one) -- only then do `LocalRead`/`LocalWrite`/`IvarRead`/
/// `IvarWrite` actually register as captures. `param_exclusions`: every
/// The receiverless names codegen emits as DIRECT calls to a runtime free
/// function, never as a dispatch on `self` (`codegen::call`'s
/// `fallible_fn`/`never_fn`/`plain_fn` tables and the `proc`/`lambda`/
/// `at_exit`/`__method__` forms it intercepts just above them). Mentioning
/// one inside a block therefore doesn't make the block need a receiver.
///
/// Kept deliberately CONSERVATIVE: a name wrongly listed here loses its
/// receiver (a real bug -- an `instance_exec`'d block calling it would
/// dispatch on the wrong object); a name wrongly MISSING just makes some
/// block carry a self it never reads, which costs nothing but a moved
/// `RubyValue`. So when in doubt, leave it out. `method`/`send`/`raise` are
/// deliberately absent: they genuinely consult the implicit receiver.
/// Whether the class this block is being compiled under has a real method of
/// this name -- in which case a receiver-less call to it is NOT the Kernel free
/// function [`is_kernel_free_fn`] assumes. `Object` is excluded: every class
/// inherits Kernel through it, so a hit there proves nothing.
fn self_class_overrides(
    compiler: &crate::compiler::Compiler,
    self_class: Option<crate::compiler::ClassId>,
    name: &str,
) -> bool {
    self_class.is_some_and(|cid| {
        cid != crate::compiler::OBJECT_CLASS && compiler.method_in_chain(cid, name).is_some()
    })
}

fn is_kernel_free_fn(name: &str) -> bool {
    matches!(
        name,
        "puts"
            | "p"
            | "pp"
            | "print"
            | "warn"
            | "format"
            | "sprintf"
            | "printf"
            | "rand"
            | "srand"
            | "sleep"
            | "exit"
            | "abort"
            | "Integer"
            | "Float"
            | "Rational"
            | "Complex"
            | "String"
            | "Array"
            | "Hash"
    )
}

/// name bound by an ENCLOSING block's own params, of ANY kind (`.times`
/// inline or a real escaping block) -- accumulated (unioned) as the walk
/// descends into ANY block, regardless of `in_escaping`, so a name shadowed
/// by an ancestor `.times` block's own parameter is never mistaken for an
/// enclosing-METHOD-scope capture even two levels down (the bug a simpler
/// "only track the innermost escaping block's own params" design would
/// have: an escaping block nested inside a `.times` block, referencing the
/// `.times` block's OWN iteration variable, would otherwise wrongly try to
/// capture it as if it were a real enclosing local).
fn walk(
    compiler: &Compiler,
    id: NodeId,
    in_escaping: bool,
    param_exclusions: &HashSet<String>,
    caps: &mut Captures,
    self_class: Option<crate::compiler::ClassId>,
) {
    match &compiler.hir[id] {
        // An FFI wrapper body has no escaping block, so nothing of the
        // enclosing scope is captured through it.
        HirNode::Ffi(_) => {}
        HirNode::LocalRead(name) => {
            if in_escaping && !param_exclusions.contains(name) {
                caps.locals.insert(name.clone());
            }
        }
        HirNode::LocalWrite(name, value) => {
            if in_escaping && !param_exclusions.contains(name) {
                caps.locals.insert(name.clone());
                caps.assigned.insert(name.clone());
            }
            walk(compiler, *value, in_escaping, param_exclusions, caps, self_class);
        }
        HirNode::IvarRead(_) => {
            if in_escaping {
                caps.self_captured = true;
            }
        }
        HirNode::IvarWrite(_, value) => {
            if in_escaping {
                caps.self_captured = true;
            }
            walk(compiler, *value, in_escaping, param_exclusions, caps, self_class);
        }
        // A bare/explicit `self` reference is another way an escaping block
        // needs the receiver captured -- same flag `IvarRead`/`IvarWrite`
        // already set above (they're really just `self`-via-ivar-sugar).
        HirNode::SelfRef => {
            if in_escaping {
                caps.self_captured = true;
            }
        }
        // A class variable's storage is keyed by a compile-time-resolved
        // OWNER CLASS id (see `analyze::mro::resolve_cvars`), never by
        // `self` -- unlike an ivar, referencing `@@x` inside an escaping
        // block needs no `self` capture at all.
        HirNode::ClassVarRead(_) => {}
        HirNode::ClassVarWrite(_, value) => walk(compiler, *value, in_escaping, param_exclusions, caps, self_class),
        // A lambda literal ALWAYS escapes (never an inline fast path, unlike
        // `.times`'s block) -- so it recurses with `in_escaping = true`
        // unconditionally, exactly like the escaping `Call.block`/`New`/
        // `SuperCall` arms. A lambda nested inside another escaping construct
        // COMPOSES through this same walk (its captures flow into the outer
        // block's capture set); the one genuinely unsupported sub-case is
        // rejected downstream at `emit_proc_or_lambda_value`, not here.
        HirNode::Lambda {
            params,
            body,
            method_body: _,
        } => {
            let next_exclusions: HashSet<String> =
                param_exclusions.union(&own_param_names(params)).cloned().collect();
            for &n in body {
                walk(compiler, n, true, &next_exclusions, caps, self_class);
            }
        }
        HirNode::And(l, r)
        | HirNode::Or(l, r)
        | HirNode::FlipFlop {
            state: _,
            left: l,
            right: r,
            exclusive: _,
        } => {
            walk(compiler, *l, in_escaping, param_exclusions, caps, self_class);
            walk(compiler, *r, in_escaping, param_exclusions, caps, self_class);
        }
        HirNode::Defined(v) => walk(compiler, *v, in_escaping, param_exclusions, caps, self_class),
        HirNode::If { cond, then_body, else_body } => {
            walk(compiler, *cond, in_escaping, param_exclusions, caps, self_class);
            for &n in then_body.iter().chain(else_body) {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            if let Some(s) = subject {
                walk(compiler, *s, in_escaping, param_exclusions, caps, self_class);
            }
            for (values, body) in arms {
                for e in values {
                    let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                    walk(compiler, *v, in_escaping, param_exclusions, caps, self_class);
                }
                for &n in body {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
            }
            for &n in else_body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::While {
            cond,
            body,
            negate: _,
            post: _,
        } => {
            walk(compiler, *cond, in_escaping, param_exclusions, caps, self_class);
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::Loop { body } => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::For { target, iterable, body } => {
            walk_multi_target(compiler, target, in_escaping, param_exclusions, caps, self_class);
            walk(compiler, *iterable, in_escaping, param_exclusions, caps, self_class);
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                walk(compiler, *v, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::Redo | HirNode::BlockGiven => {}
        HirNode::MultiWrite { targets, value } => {
            walk_multi_target_group(compiler, targets, in_escaping, param_exclusions, caps, self_class);
            walk(compiler, *value, in_escaping, param_exclusions, caps, self_class);
        }
        // A global/constant's storage doesn't depend on `self`/enclosing
        // locals at all -- no capture registration needed, same posture as
        // `ClassVarWrite` just above.
        HirNode::GlobalWrite(_, value) => walk(compiler, *value, in_escaping, param_exclusions, caps, self_class),
        HirNode::ConstWrite {
            scope: _,
            name: _,
            value,
        } => walk(compiler, *value, in_escaping, param_exclusions, caps, self_class),
        HirNode::PreExec(body) | HirNode::Seq(body) => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::Yield(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                walk(compiler, *n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::Raise(args, cause) => {
            for &a in args.iter().chain(crate::hir::raise_cause_node(cause).iter()) {
                walk(compiler, a, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            walk(compiler, *subject, in_escaping, param_exclusions, caps, self_class);
            for arm in arms {
                // A pattern's bound names are a fresh binding, same
                // treatment as `LocalWrite` just above -- only registered
                // as a capture candidate while inside an escaping block; the
                // later intersection with `collect_locals`'s whole-scope
                // result (see `collect_escaping_captures`) is what decides
                // whether it's GENUINELY shared with code outside the block.
                if in_escaping {
                    arm.pattern.for_each_bound_name(&mut |n| {
                        if !param_exclusions.contains(n) {
                            caps.locals.insert(n.to_string());
                            caps.assigned.insert(n.to_string());
                        }
                    });
                }
                arm.pattern
                    .for_each_node(&mut |n| walk(compiler, n, in_escaping, param_exclusions, caps, self_class));
                if let Some((g, _)) = arm.guard {
                    walk(compiler, g, in_escaping, param_exclusions, caps, self_class);
                }
                for &n in &arm.body {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            walk(compiler, *subject, in_escaping, param_exclusions, caps, self_class);
            if in_escaping {
                pattern.for_each_bound_name(&mut |n| {
                    if !param_exclusions.contains(n) {
                        caps.locals.insert(n.to_string());
                        caps.assigned.insert(n.to_string());
                    }
                });
            }
            pattern.for_each_node(&mut |n| walk(compiler, n, in_escaping, param_exclusions, caps, self_class));
        }
        HirNode::Begin { body, rescues, else_body, ensure_body } => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
            for r in rescues {
                // A splatted exception list (`rescue *errs`) reads outer locals
                // -- they must be captured when this `begin` is inside a closure.
                for &n in &r.splats {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
                // A rescue binding is a fresh name, same treatment as
                // `LocalWrite`/a pattern's bound names just above.
                if in_escaping {
                    if let Some(name) = &r.binding {
                        if !param_exclusions.contains(name) {
                            caps.locals.insert(name.clone());
                            caps.assigned.insert(name.clone());
                        }
                    }
                }
                for &n in &r.body {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::Eval(body) | HirNode::BoxScope { box_id: _, body } => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::New {
            class_name: _,
            args,
            kwargs,
            block,
        } => {
            for &a in args {
                walk(compiler, a, in_escaping, param_exclusions, caps, self_class);
            }
            for a in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                walk(compiler, a, in_escaping, param_exclusions, caps, self_class);
            }
            // A literal block forwarded to `initialize` is a real escaping
            // Proc -- same treatment as `super { ... }` below.
            if let Some(b) = block {
                if let HirNode::Block { params, body } = &compiler.hir[*b] {
                    let next_exclusions: HashSet<String> =
                        param_exclusions.union(&own_param_names(params)).cloned().collect();
                    for &n in body {
                        walk(compiler, n, true, &next_exclusions, caps, self_class);
                    }
                }
            }
        }
        HirNode::SuperCall {
            args,
            kwargs,
            zsuper: _,
            block,
            block_arg,
        } => {
            // `super` implicitly dispatches on the receiver, so an escaping
            // block containing one must capture `self` -- same flag `SelfRef`/
            // `IvarRead` set above. Without this a `super` in a method-body
            // lambda (a `def` in a `Class.new`/`Struct.new` block) would emit
            // a self reference the closure never binds.
            if in_escaping {
                caps.self_captured = true;
            }
            for a in args {
                walk(compiler, a.node_id(), in_escaping, param_exclusions, caps, self_class);
            }
            for a in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                walk(compiler, a, in_escaping, param_exclusions, caps, self_class);
            }
            if let Some(b) = block_arg {
                walk(compiler, *b, in_escaping, param_exclusions, caps, self_class);
            }
            // A literal `super { ... }` block is always a real, escaping
            // Proc (no `.times`-style inline fast path exists for `super`)
            // -- same treatment as `Call`'s escaping-block arm above.
            if let Some(b) = block {
                if let HirNode::Block { params, body } = &compiler.hir[*b] {
                    let next_exclusions: HashSet<String> =
                        param_exclusions.union(&own_param_names(params)).cloned().collect();
                    for &n in body {
                        walk(compiler, n, true, &next_exclusions, caps, self_class);
                    }
                }
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                walk(compiler, *n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::HashLit(pairs) => {
            for n in pairs.iter().flat_map(|kw| kw.node_ids()) {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::RangeLit {
            start,
            end,
            exclusive: _,
        } => {
            if let Some(s) = start {
                walk(compiler, *s, in_escaping, param_exclusions, caps, self_class);
            }
            if let Some(e) = end {
                walk(compiler, *e, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    walk(compiler, *n, in_escaping, param_exclusions, caps, self_class);
                }
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
            // A receiverless call dispatches on `self` (see `emit_call`'s
            // implicit-self branch) -- inside an escaping block that's a
            // `self` capture exactly like an ivar reference, otherwise the
            // closure `move`s the method's own `self` binding out from under
            // the code after it (E0382).
            //
            // NOT conditioned on the name resolving against the enclosing
            // class: a name that resolves nowhere lexically is exactly the
            // `instance_exec` case (`obj.instance_exec { helper }` -- `helper`
            // is on OBJ's class, invisible from where the block is written),
            // and it still dispatches on self, so the block still needs one.
            // Kernel free functions (`puts`) are excluded: codegen emits them
            // as direct calls that never consult a receiver, so capturing self
            // for them would be dead weight on almost every block in a program.
            //
            // ...unless the enclosing class OVERRIDES the Kernel name. Then it
            // is an ordinary method after all, and codegen emits a direct call
            // on `self` -- which a `move` closure would move out of, breaking
            // `Fn`. `Net::WriteAdapter#puts` is the shape that found it: a
            // top-level `def` materialized onto a class that defines `puts`,
            // with an escaping block inside.
            let kernel_free =
                is_kernel_free_fn(name) && !self_class_overrides(compiler, self_class, name);
            if receiver.is_none() && in_escaping && !kernel_free {
                caps.self_captured = true;
            }
            if let Some(r) = receiver {
                walk(compiler, *r, in_escaping, param_exclusions, caps, self_class);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                walk(compiler, *n, in_escaping, param_exclusions, caps, self_class);
            }
            for n in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
            if let Some(b) = block_arg {
                walk(compiler, *b, in_escaping, param_exclusions, caps, self_class);
            }
            if let Some(b) = block {
                let HirNode::Block { params, body } = &compiler.hir[*b] else {
                    panic!("internal error: a Block node should only be reached via the Call that invokes it");
                };
                let is_inline = is_inline_block_fast_path(compiler, *receiver, name, kwargs.is_empty());
                // A real escaping block nested inside another escaping block
                // (Proc-within-Proc, e.g. `Thread.new { m.synchronize { } }`,
                // a canonical idiom) COMPOSES through this walk
                // unchanged: captured-ness is a property of the NAME across
                // the whole enclosing scope, an inner closure's same-named
                // `Arc::clone` shadows resolve to the outer closure's
                // moved-in cells, and `self`-capture chains through
                // `cx.self_ident` (`__self` shadowing `__self`). The one
                // genuinely unsupported sub-case -- an inner block capturing
                // its enclosing BLOCK's own (non-cell) local -- is rejected
                // at the Proc-construction site instead
                // (`emit_proc_or_lambda_value`), where it can be detected
                // precisely rather than banning all nesting wholesale.
                let next_exclusions: HashSet<String> =
                    param_exclusions.union(&own_param_names(params)).cloned().collect();
                let next_in_escaping = in_escaping || !is_inline;
                for &n in body {
                    walk(compiler, n, next_in_escaping, &next_exclusions, caps, self_class);
                }
            }
        }
        // A `def`/literal `define_method` INSIDE an escaping block
        // (e.g. `Class.new { define_method(:x) { ... } }`) compiles to
        // `self.define_method(:name, ->(params){ body })` (see
        // `codegen::expr`'s `DefMethod` arm): it USES `self` (the install
        // target) and its body is a nested escaping proc. Outside an escaping
        // block it's an ordinary method definition with its own scope --
        // nothing to capture -- so this only fires when `in_escaping`.
        HirNode::DefMethod {
            name: _,
            params,
            body,
            is_class_method: _,
            visibility: _,
            is_def: _,
        } => {
            if in_escaping {
                caps.self_captured = true;
                let next_exclusions: HashSet<String> =
                    param_exclusions.union(&own_param_names(params)).cloned().collect();
                for &n in body {
                    walk(compiler, n, true, &next_exclusions, caps, self_class);
                }
            }
        }
        HirNode::Block {
            params: _,
            body: _,
        } => {
            panic!("internal error: a Block node should only be reached via the Call that invokes it")
        }
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
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::Undef(_)
        | HirNode::AliasMethod {
            new_name: _,
            old_name: _,
            is_class_method: _,
        }
        | HirNode::MethodVisibility {
            name: _,
            visibility: _,
        }
        | HirNode::ModuleFunction(_)
        | HirNode::AliasGlobal(_, _)
        | HirNode::QualifiedConstRead(_, _)
        | HirNode::ConstReadOrNil(_, _)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassDef {
            name: _,
            superclass: _,
            body: _,
            is_module: _,
        } => {}
    }
}

/// See `MultiTarget::for_each_node`'s docs on the shape being walked; unlike
/// that generic traversal, THIS walk additionally needs to know about
/// capture-registration semantics (a `Local`/`Call` target's synthetic
/// `tmp_name` is a FRESH binding, same treatment `walk`'s own `LocalWrite`
/// arm gives an ordinary assignment; an `Ivar` target needs `self` captured,
/// same as `IvarWrite`; `ClassVar`/`Global`/`Const` targets need neither,
/// same as `ClassVarWrite`).
fn walk_multi_target(
    compiler: &Compiler,
    target: &crate::hir::MultiTarget,
    in_escaping: bool,
    param_exclusions: &HashSet<String>,
    caps: &mut Captures,
    self_class: Option<crate::compiler::ClassId>,
) {
    use crate::hir::MultiTarget;
    match target {
        MultiTarget::Local(name) => {
            if in_escaping && !param_exclusions.contains(name) {
                caps.locals.insert(name.clone());
                caps.assigned.insert(name.clone());
            }
        }
        MultiTarget::Ivar(_) => {
            if in_escaping {
                caps.self_captured = true;
            }
        }
        MultiTarget::ClassVar(_)
        | MultiTarget::Global(_)
        | MultiTarget::Const(_)
        | MultiTarget::ScopedConst { .. } => {}
        MultiTarget::Call {
            write_call,
            tmp_name,
        } => {
            if in_escaping && !param_exclusions.contains(tmp_name) {
                caps.locals.insert(tmp_name.clone());
                caps.assigned.insert(tmp_name.clone());
            }
            walk(
                compiler,
                *write_call,
                in_escaping,
                param_exclusions,
                caps,
                self_class,
            );
        }
        MultiTarget::Nested(group) => walk_multi_target_group(
            compiler,
            group,
            in_escaping,
            param_exclusions,
            caps,
            self_class,
        ),
    }
}

fn walk_multi_target_group(
    compiler: &Compiler,
    group: &crate::hir::MultiTargetGroup,
    in_escaping: bool,
    param_exclusions: &HashSet<String>,
    caps: &mut Captures,
    self_class: Option<crate::compiler::ClassId>,
) {
    for t in group.before.iter().chain(&group.after) {
        walk_multi_target(compiler, t, in_escaping, param_exclusions, caps, self_class);
    }
    if let Some(Some(t)) = &group.splat {
        walk_multi_target(compiler, t, in_escaping, param_exclusions, caps, self_class);
    }
}
