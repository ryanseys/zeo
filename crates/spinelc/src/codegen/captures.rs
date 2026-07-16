//! Free-variable/`self` capture analysis for escaping blocks (Phase 6). See
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

use super::call::is_times_fast_path;
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
/// kinds. It used to keep its own copy of that walk, and the copy drifted the
/// moment destructuring params arrived: the names inside `|(a, b)|` are bound
/// by the params but live in `Params::destructures`, so this missed them and
/// they were classified as ordinary locals rather than captured ones --
/// silently making `def m((a, b)); -> { a += 1 }; end` read a nil `a` inside
/// the block. One enumeration, one place to update.
pub(super) fn own_param_names(params: &Params) -> HashSet<String> {
    params.bound_names().into_iter().collect()
}

/// Scans a WHOLE scope's body (a method's or the top level's) for every
/// escaping block, unioning their capture requirements -- but ONLY names
/// genuinely shared with the enclosing scope: assigned somewhere outside
/// the block(s), OR one of the enclosing method's OWN PARAMETERS
/// (`params`). The params half was missing until Phase 14.4's `set`
/// package surfaced it: a parameter referenced ONLY inside an escaping
/// block (`def add_all(n); each { |x| puts x + n }; end`, or an
/// Enumerable-shaped `&blk` forwarded into an inner block) failed the
/// assigned-outside filter and was misclassified as a block-OWN local --
/// silently re-declared `Nil` inside the closure, shadowing the real
/// argument. Verified against real Ruby both ways: the parameter IS shared
/// enclosing-scope state, while `proc { |x| tmp ||= 0; tmp += x }.call`
/// genuinely resets `tmp` per call (NOT a capture when nothing outside the
/// block uses the name; see `hoisting::collect_locals`'s `Call` arm, which
/// -- as of Phase 6 -- no longer descends into an escaping block's body,
/// making its result exactly "names used outside any escaping block").
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
        locals: raw.locals.into_iter().filter(|n| outer.contains(n)).collect(),
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

/// Whether `body` (a WHOLE method's own body) lexically contains at least
/// one escaping block, ANYWHERE (including nested inside `if`/`while`/an
/// inline `.times` block/etc) -- decides whether `codegen::mod`'s per-
/// method `Signal::Return` catch is needed at all.
///
/// Confirmed the hard way this can't be "wrap every method unconditionally"
/// (the original, simpler design): a method with NO escaping block of its
/// own (e.g. `each_num`, which just yields to whatever block it's given)
/// must NOT catch `Signal::Return` -- doing so would incorrectly intercept
/// a `return` that belongs to a DIFFERENT method (wherever the block it's
/// currently invoking was actually WRITTEN, e.g. `find_even`), silently
/// turning "return from find_even" into "each_num returns normally instead"
/// instead of propagating further. Only a method whose OWN body contains an
/// escaping block can ever be the right place to catch a `Signal::Return`
/// that block raises.
pub fn body_contains_escaping_block(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter().any(|&n| node_contains_escaping_block(compiler, n))
}

fn node_contains_escaping_block(compiler: &Compiler, id: NodeId) -> bool {
    match &compiler.hir[id] {
        // A lambda literal's own body is a fully self-contained closure
        // boundary (it ALWAYS catches its own `Signal::Return`/`Break`,
        // unconditionally, unlike an ordinary method -- see
        // `hir::HirNode::Lambda`'s docs) -- its mere presence never requires
        // the ENCLOSING method to install its own catch, so this is a leaf.
        HirNode::Lambda { .. } => false,
        HirNode::Call { receiver, name, args, kwargs, kwargs_splat, block, block_arg, .. } => {
            if let Some(b) = block {
                let HirNode::Block { body, .. } = &compiler.hir[*b] else {
                    panic!("a Block should only be reached via the Call that invokes it");
                };
                if !is_times_fast_path(compiler, *receiver, name, kwargs.is_empty()) {
                    return true;
                }
                // Still inline (`.times`) -- keep looking inside it (and in
                // every other position this call touches).
                if body_contains_escaping_block(compiler, body) {
                    return true;
                }
            }
            receiver.is_some_and(|r| node_contains_escaping_block(compiler, r))
                || args.iter().any(|a| {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    node_contains_escaping_block(compiler, *n)
                })
                || kwargs.iter().any(|p| {
                    node_contains_escaping_block(compiler, p.0) || node_contains_escaping_block(compiler, p.1)
                })
                || kwargs_splat.is_some_and(|s| node_contains_escaping_block(compiler, s))
                || block_arg.is_some_and(|b| node_contains_escaping_block(compiler, b))
        }
        HirNode::LocalWrite(_, v) | HirNode::IvarWrite(_, v) | HirNode::ClassVarWrite(_, v) | HirNode::Defined(v) => {
            node_contains_escaping_block(compiler, *v)
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            node_contains_escaping_block(compiler, *l) || node_contains_escaping_block(compiler, *r)
        }
        HirNode::If { cond, then_body, else_body } => {
            node_contains_escaping_block(compiler, *cond)
                || body_contains_escaping_block(compiler, then_body)
                || body_contains_escaping_block(compiler, else_body)
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            subject.is_some_and(|s| node_contains_escaping_block(compiler, s))
                || arms.iter().any(|(values, body)| {
                    values.iter().any(|&v| node_contains_escaping_block(compiler, v))
                        || body_contains_escaping_block(compiler, body)
                })
                || body_contains_escaping_block(compiler, else_body)
        }
        HirNode::While { cond, body, .. } => {
            node_contains_escaping_block(compiler, *cond) || body_contains_escaping_block(compiler, body)
        }
        HirNode::Loop { body } => body_contains_escaping_block(compiler, body),
        HirNode::For { target, iterable, body } => {
            let mut found = false;
            target.for_each_node(&mut |n| found |= node_contains_escaping_block(compiler, n));
            found
                || node_contains_escaping_block(compiler, *iterable)
                || body_contains_escaping_block(compiler, body)
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            v.is_some_and(|v| node_contains_escaping_block(compiler, v))
        }
        HirNode::MultiWrite { targets, value } => {
            let mut found = false;
            targets.for_each_node(&mut |n| found |= node_contains_escaping_block(compiler, n));
            found || node_contains_escaping_block(compiler, *value)
        }
        HirNode::GlobalWrite(_, value) => node_contains_escaping_block(compiler, *value),
        HirNode::ConstWrite { value, .. } => node_contains_escaping_block(compiler, *value),
        HirNode::Yield(args) | HirNode::Raise(args) => {
            args.iter().any(|&a| node_contains_escaping_block(compiler, a))
        }
        HirNode::Seq(body) | HirNode::Eval(body) | HirNode::BoxScope { body, .. } => body_contains_escaping_block(compiler, body),
        HirNode::New { args, .. } => {
            args.iter().any(|&a| node_contains_escaping_block(compiler, a))
        }
        // A literal `super { ... }` block always escapes (see `walk`'s
        // `SuperCall` arm).
        HirNode::SuperCall { args, block, .. } => {
            block.is_some() || args.iter().any(|&a| node_contains_escaping_block(compiler, a))
        }
        HirNode::ArrayLit(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_escaping_block(compiler, *n)
        }),
        HirNode::HashLit(pairs) => pairs
            .iter()
            .any(|p| node_contains_escaping_block(compiler, p.0) || node_contains_escaping_block(compiler, p.1)),
        HirNode::RangeLit { start, end, .. } => {
            start.is_some_and(|s| node_contains_escaping_block(compiler, s))
                || end.is_some_and(|e| node_contains_escaping_block(compiler, e))
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => parts.iter().any(|p| match p {
            StrPart::Interp(n) => node_contains_escaping_block(compiler, *n),
            StrPart::Lit(_) => false,
        }),
        HirNode::CaseIn { subject, arms, else_body } => {
            node_contains_escaping_block(compiler, *subject)
                || arms.iter().any(|arm| {
                    let mut pattern_found = false;
                    arm.pattern
                        .for_each_node(&mut |n| pattern_found |= node_contains_escaping_block(compiler, n));
                    pattern_found
                        || arm.guard.is_some_and(|(g, _)| node_contains_escaping_block(compiler, g))
                        || body_contains_escaping_block(compiler, &arm.body)
                })
                || else_body.as_deref().is_some_and(|b| body_contains_escaping_block(compiler, b))
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            if node_contains_escaping_block(compiler, *subject) {
                return true;
            }
            let mut found = false;
            pattern.for_each_node(&mut |n| found |= node_contains_escaping_block(compiler, n));
            found
        }
        HirNode::Begin { body, rescues, else_body, ensure_body } => {
            body_contains_escaping_block(compiler, body)
                || rescues.iter().any(|r| body_contains_escaping_block(compiler, &r.body))
                || else_body.as_deref().is_some_and(|b| body_contains_escaping_block(compiler, b))
                || ensure_body.as_deref().is_some_and(|b| body_contains_escaping_block(compiler, b))
        }
        HirNode::Retry
        | HirNode::Redo
        | HirNode::BlockGiven
        | HirNode::SelfRef
        | HirNode::Block { .. }
        | HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
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
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. }
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => false,
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
        HirNode::Begin { .. } => true,
        // Same reasoning as `node_contains_escaping_block`'s `Lambda` arm --
        // a lambda's own body is a fully self-contained closure boundary,
        // so a `begin`/`rescue` lexically inside one never requires the
        // ENCLOSING method to install its own `Signal::Return` catch.
        HirNode::Lambda { .. } => false,
        HirNode::Call { receiver, args, kwargs, kwargs_splat, block, block_arg, .. } => {
            receiver.is_some_and(|r| node_contains_begin(compiler, r))
                || args.iter().any(|a| {
                    let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                    node_contains_begin(compiler, *n)
                })
                || kwargs.iter().any(|p| node_contains_begin(compiler, p.0) || node_contains_begin(compiler, p.1))
                || kwargs_splat.is_some_and(|s| node_contains_begin(compiler, s))
                || block_arg.is_some_and(|b| node_contains_begin(compiler, b))
                || block.is_some_and(|b| {
                    let HirNode::Block { body, .. } = &compiler.hir[b] else {
                        panic!("a Block should only be reached via the Call that invokes it");
                    };
                    body_contains_begin(compiler, body)
                })
        }
        HirNode::LocalWrite(_, v) | HirNode::IvarWrite(_, v) | HirNode::ClassVarWrite(_, v) | HirNode::Defined(v) => {
            node_contains_begin(compiler, *v)
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => node_contains_begin(compiler, *l) || node_contains_begin(compiler, *r),
        HirNode::If { cond, then_body, else_body } => {
            node_contains_begin(compiler, *cond)
                || body_contains_begin(compiler, then_body)
                || body_contains_begin(compiler, else_body)
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            subject.is_some_and(|s| node_contains_begin(compiler, s))
                || arms.iter().any(|(values, body)| {
                    values.iter().any(|&v| node_contains_begin(compiler, v)) || body_contains_begin(compiler, body)
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
        HirNode::While { cond, body, .. } => node_contains_begin(compiler, *cond) || body_contains_begin(compiler, body),
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
        HirNode::ConstWrite { value, .. } => node_contains_begin(compiler, *value),
        HirNode::Yield(args) | HirNode::Raise(args) => args.iter().any(|&a| node_contains_begin(compiler, a)),
        HirNode::New { args, .. } => args.iter().any(|&a| node_contains_begin(compiler, a)),
        HirNode::SuperCall { args, block, .. } => {
            args.iter().any(|&a| node_contains_begin(compiler, a))
                || block.is_some_and(|b| match &compiler.hir[b] {
                    HirNode::Block { body, .. } => body_contains_begin(compiler, body),
                    _ => false,
                })
        }
        HirNode::ArrayLit(elems) => elems.iter().any(|e| {
            let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
            node_contains_begin(compiler, *n)
        }),
        HirNode::HashLit(pairs) => {
            pairs.iter().any(|p| node_contains_begin(compiler, p.0) || node_contains_begin(compiler, p.1))
        }
        HirNode::RangeLit { start, end, .. } => {
            start.is_some_and(|s| node_contains_begin(compiler, s)) || end.is_some_and(|e| node_contains_begin(compiler, e))
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => parts.iter().any(|p| match p {
            StrPart::Interp(n) => node_contains_begin(compiler, *n),
            StrPart::Lit(_) => false,
        }),
        HirNode::Seq(body) | HirNode::Eval(body) | HirNode::BoxScope { body, .. } => body_contains_begin(compiler, body),
        HirNode::Retry
        | HirNode::Redo
        | HirNode::BlockGiven
        | HirNode::SelfRef
        | HirNode::Block { .. }
        | HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
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
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. }
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => false,
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
        HirNode::LocalRead(name) => {
            if in_escaping && !param_exclusions.contains(name) {
                caps.locals.insert(name.clone());
            }
        }
        HirNode::LocalWrite(name, value) => {
            if in_escaping && !param_exclusions.contains(name) {
                caps.locals.insert(name.clone());
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
        // `.times`'s block) -- same shape as an escaping `Call.block` below,
        // minus the `is_inline` branch. Nested inside another escaping
        // construct is the same unsupported "two-level closure capture"
        // shape that block-within-escaping-block already rejects.
        HirNode::Lambda { params, body } => {
            if in_escaping {
                panic!("a lambda escaping from inside another escaping block isn't supported yet (spike scope)");
            }
            let next_exclusions: HashSet<String> =
                param_exclusions.union(&own_param_names(params)).cloned().collect();
            for &n in body {
                walk(compiler, n, true, &next_exclusions, caps, self_class);
            }
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => {
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
                for &v in values {
                    walk(compiler, v, in_escaping, param_exclusions, caps, self_class);
                }
                for &n in body {
                    walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
                }
            }
            for &n in else_body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::While { cond, body, .. } => {
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
        HirNode::ConstWrite { value, .. } => walk(compiler, *value, in_escaping, param_exclusions, caps, self_class),
        HirNode::Seq(body) => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::Yield(args) | HirNode::Raise(args) => {
            for &a in args {
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
                // A rescue binding is a fresh name, same treatment as
                // `LocalWrite`/a pattern's bound names just above.
                if in_escaping {
                    if let Some(name) = &r.binding {
                        if !param_exclusions.contains(name) {
                            caps.locals.insert(name.clone());
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
        HirNode::Eval(body) | HirNode::BoxScope { body, .. } => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::New { args, .. } => {
            for &a in args {
                walk(compiler, a, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::SuperCall { args, block, .. } => {
            for &a in args {
                walk(compiler, a, in_escaping, param_exclusions, caps, self_class);
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
            for pair in pairs {
                walk(compiler, pair.0, in_escaping, param_exclusions, caps, self_class);
                walk(compiler, pair.1, in_escaping, param_exclusions, caps, self_class);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
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
        HirNode::Call { receiver, name, args, kwargs, kwargs_splat, block, block_arg, .. } => {
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
            if receiver.is_none() && in_escaping && !is_kernel_free_fn(name) {
                caps.self_captured = true;
            }
            if let Some(r) = receiver {
                walk(compiler, *r, in_escaping, param_exclusions, caps, self_class);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                walk(compiler, *n, in_escaping, param_exclusions, caps, self_class);
            }
            for pair in kwargs {
                walk(compiler, pair.0, in_escaping, param_exclusions, caps, self_class);
                walk(compiler, pair.1, in_escaping, param_exclusions, caps, self_class);
            }
            if let Some(s) = kwargs_splat {
                walk(compiler, *s, in_escaping, param_exclusions, caps, self_class);
            }
            if let Some(b) = block_arg {
                walk(compiler, *b, in_escaping, param_exclusions, caps, self_class);
            }
            if let Some(b) = block {
                let HirNode::Block { params, body } = &compiler.hir[*b] else {
                    panic!("a Block should only be reached via the Call that invokes it");
                };
                let is_inline = is_times_fast_path(compiler, *receiver, name, kwargs.is_empty());
                // A real escaping block nested inside another escaping block
                // (Proc-within-Proc, e.g. `Thread.new { m.synchronize { } }`,
                // Phase 13.5's canonical idiom) COMPOSES through this walk
                // unchanged: captured-ness is a property of the NAME across
                // the whole enclosing scope, an inner closure's same-named
                // `Arc::clone` shadows resolve to the outer closure's
                // moved-in cells, and `self`-capture chains through
                // `cx.self_ident` (`__self` shadowing `__self`). The one
                // genuinely unsupported sub-case -- an inner block capturing
                // its enclosing BLOCK's own (non-cell) local -- is rejected
                // at the Proc-construction site instead
                // (`emit_proc_or_lambda_value`), where it can be detected
                // precisely rather than banning all nesting wholesale (the
                // blanket panic that previously lived here).
                let next_exclusions: HashSet<String> =
                    param_exclusions.union(&own_param_names(params)).cloned().collect();
                let next_in_escaping = in_escaping || !is_inline;
                for &n in body {
                    walk(compiler, n, next_in_escaping, &next_exclusions, caps, self_class);
                }
            }
        }
        HirNode::Block { .. } => {
            panic!("a Block should only be reached via the Call that invokes it")
        }
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
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
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::NativeCrate(_)
        | HirNode::NativeFunc { .. }
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => {}
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
        MultiTarget::Call { write_call, tmp_name } => {
            if in_escaping && !param_exclusions.contains(tmp_name) {
                caps.locals.insert(tmp_name.clone());
            }
            walk(compiler, *write_call, in_escaping, param_exclusions, caps, self_class);
        }
        MultiTarget::Nested(group) => walk_multi_target_group(compiler, group, in_escaping, param_exclusions, caps, self_class),
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
