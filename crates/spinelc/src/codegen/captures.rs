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
use crate::hir::{ArrayElem, HirNode, KeywordParam, NodeId, Params, StrPart};
use std::collections::HashSet;

#[derive(Default)]
pub struct Captures {
    /// Enclosing-scope local/parameter names referenced (read or written)
    /// inside some escaping block, excluding every block's own params
    /// (that block's own, AND every ancestor inline `.times` block's --
    /// see `walk`'s `param_exclusions`, which is what makes this
    /// exclusion correct even for a name shadowed two levels deep).
    pub locals: HashSet<String>,
    /// Whether any escaping block references `@ivar`/an instance variable --
    /// the only way `self` is reachable in this spike's surface syntax
    /// today (see `parse/mod.rs`: there's no explicit-`self`/implicit-self-
    /// call HIR node at all yet), so this is the complete self-capture
    /// condition, not an approximation of a larger one.
    pub self_captured: bool,
}

/// Every name a `Params` list itself binds -- the exclusion set for "is this
/// name a BLOCK's OWN parameter, not something captured from its enclosing
/// scope". Mirrors `codegen::params`'s own per-kind enumeration. Also reused
/// by `codegen::params::emit_prologue` to decide which of a METHOD's own
/// parameter names need the additional `Arc<parking_lot::Mutex<_>>`-wrapping shadow
/// (when captured by one of ITS OWN escaping blocks).
pub(super) fn own_param_names(params: &Params) -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();
    names.extend(params.required.iter().cloned());
    names.extend(params.optional.iter().map(|(n, _)| n.clone()));
    if let Some(Some(n)) = &params.rest {
        names.insert(n.clone());
    }
    names.extend(params.post.iter().cloned());
    for kw in &params.keywords {
        match kw {
            KeywordParam::Required(n) | KeywordParam::Optional(n, _) => {
                names.insert(n.clone());
            }
        }
    }
    if let Some(Some(n)) = &params.keyword_rest {
        names.insert(n.clone());
    }
    if let Some(Some(n)) = &params.block {
        names.insert(n.clone());
    }
    names
}

/// Scans a WHOLE scope's body (a method's or the top level's) for every
/// escaping block, unioning their capture requirements -- but ONLY names
/// genuinely shared with code OUTSIDE the block(s) that reference them
/// (verified against real Ruby: `proc { |x| tmp ||= 0; tmp += x }.call`
/// resets `tmp` on every separate `.call()` -- it is NOT a capture at all
/// when nothing outside the block ever uses that name, just a fresh local
/// owned by the block itself; see `hoisting::collect_locals`'s `Call` arm,
/// which -- as of Phase 6 -- no longer descends into an escaping block's
/// body, making its result exactly "names used outside any escaping
/// block"). `self` has no such distinction (an ivar always means the same
/// object), so `self_captured` is passed through unfiltered.
pub fn collect_escaping_captures(compiler: &Compiler, body: &[NodeId]) -> Captures {
    let mut raw = Captures::default();
    for &n in body {
        walk(compiler, n, false, &HashSet::new(), &mut raw);
    }
    let mut outer_names = Vec::new();
    for &n in body {
        super::hoisting::collect_locals(compiler, n, &mut outer_names);
    }
    let outer: HashSet<String> = outer_names.into_iter().collect();
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
pub fn block_captures(compiler: &Compiler, params: &Params, body: &[NodeId]) -> Captures {
    let mut caps = Captures::default();
    let own = own_param_names(params);
    for &n in body {
        walk(compiler, n, true, &own, &mut caps);
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
        HirNode::Seq(body) | HirNode::Eval(body) => body_contains_escaping_block(compiler, body),
        HirNode::New { args, .. } | HirNode::SuperCall { args } => {
            args.iter().any(|&a| node_contains_escaping_block(compiler, a))
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
        HirNode::StringLit(parts) => parts.iter().any(|p| match p {
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
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::QualifiedConstRead(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
        HirNode::New { args, .. } | HirNode::SuperCall { args } => args.iter().any(|&a| node_contains_begin(compiler, a)),
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
        HirNode::StringLit(parts) => parts.iter().any(|p| match p {
            StrPart::Interp(n) => node_contains_begin(compiler, *n),
            StrPart::Lit(_) => false,
        }),
        HirNode::Seq(body) | HirNode::Eval(body) => body_contains_begin(compiler, body),
        HirNode::Retry
        | HirNode::Redo
        | HirNode::BlockGiven
        | HirNode::SelfRef
        | HirNode::Block { .. }
        | HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::QualifiedConstRead(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => false,
    }
}

/// `in_escaping`: whether the walk has descended into some escaping
/// block's own body (directly, or through an inline `.times` block nested
/// inside one) -- only then do `LocalRead`/`LocalWrite`/`IvarRead`/
/// `IvarWrite` actually register as captures. `param_exclusions`: every
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
            walk(compiler, *value, in_escaping, param_exclusions, caps);
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
            walk(compiler, *value, in_escaping, param_exclusions, caps);
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
        HirNode::ClassVarWrite(_, value) => walk(compiler, *value, in_escaping, param_exclusions, caps),
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
                walk(compiler, n, true, &next_exclusions, caps);
            }
        }
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            walk(compiler, *l, in_escaping, param_exclusions, caps);
            walk(compiler, *r, in_escaping, param_exclusions, caps);
        }
        HirNode::Defined(v) => walk(compiler, *v, in_escaping, param_exclusions, caps),
        HirNode::If { cond, then_body, else_body } => {
            walk(compiler, *cond, in_escaping, param_exclusions, caps);
            for &n in then_body.iter().chain(else_body) {
                walk(compiler, n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::CaseWhen { subject, arms, else_body } => {
            if let Some(s) = subject {
                walk(compiler, *s, in_escaping, param_exclusions, caps);
            }
            for (values, body) in arms {
                for &v in values {
                    walk(compiler, v, in_escaping, param_exclusions, caps);
                }
                for &n in body {
                    walk(compiler, n, in_escaping, param_exclusions, caps);
                }
            }
            for &n in else_body {
                walk(compiler, n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::While { cond, body, .. } => {
            walk(compiler, *cond, in_escaping, param_exclusions, caps);
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::Loop { body } => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::For { target, iterable, body } => {
            walk_multi_target(compiler, target, in_escaping, param_exclusions, caps);
            walk(compiler, *iterable, in_escaping, param_exclusions, caps);
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                walk(compiler, *v, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::Redo | HirNode::BlockGiven => {}
        HirNode::MultiWrite { targets, value } => {
            walk_multi_target_group(compiler, targets, in_escaping, param_exclusions, caps);
            walk(compiler, *value, in_escaping, param_exclusions, caps);
        }
        // A global/constant's storage doesn't depend on `self`/enclosing
        // locals at all -- no capture registration needed, same posture as
        // `ClassVarWrite` just above.
        HirNode::GlobalWrite(_, value) => walk(compiler, *value, in_escaping, param_exclusions, caps),
        HirNode::ConstWrite { value, .. } => walk(compiler, *value, in_escaping, param_exclusions, caps),
        HirNode::Seq(body) => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::Yield(args) | HirNode::Raise(args) => {
            for &a in args {
                walk(compiler, a, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::CaseIn { subject, arms, else_body } => {
            walk(compiler, *subject, in_escaping, param_exclusions, caps);
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
                    .for_each_node(&mut |n| walk(compiler, n, in_escaping, param_exclusions, caps));
                if let Some((g, _)) = arm.guard {
                    walk(compiler, g, in_escaping, param_exclusions, caps);
                }
                for &n in &arm.body {
                    walk(compiler, n, in_escaping, param_exclusions, caps);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    walk(compiler, n, in_escaping, param_exclusions, caps);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            walk(compiler, *subject, in_escaping, param_exclusions, caps);
            if in_escaping {
                pattern.for_each_bound_name(&mut |n| {
                    if !param_exclusions.contains(n) {
                        caps.locals.insert(n.to_string());
                    }
                });
            }
            pattern.for_each_node(&mut |n| walk(compiler, n, in_escaping, param_exclusions, caps));
        }
        HirNode::Begin { body, rescues, else_body, ensure_body } => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps);
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
                    walk(compiler, n, in_escaping, param_exclusions, caps);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    walk(compiler, n, in_escaping, param_exclusions, caps);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    walk(compiler, n, in_escaping, param_exclusions, caps);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::Eval(body) => {
            for &n in body {
                walk(compiler, n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::New { args, .. } | HirNode::SuperCall { args } => {
            for &a in args {
                walk(compiler, a, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                walk(compiler, *n, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::HashLit(pairs) => {
            for pair in pairs {
                walk(compiler, pair.0, in_escaping, param_exclusions, caps);
                walk(compiler, pair.1, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
            if let Some(s) = start {
                walk(compiler, *s, in_escaping, param_exclusions, caps);
            }
            if let Some(e) = end {
                walk(compiler, *e, in_escaping, param_exclusions, caps);
            }
        }
        HirNode::StringLit(parts) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    walk(compiler, *n, in_escaping, param_exclusions, caps);
                }
            }
        }
        HirNode::Call { receiver, name, args, kwargs, kwargs_splat, block, block_arg, .. } => {
            if let Some(r) = receiver {
                walk(compiler, *r, in_escaping, param_exclusions, caps);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                walk(compiler, *n, in_escaping, param_exclusions, caps);
            }
            for pair in kwargs {
                walk(compiler, pair.0, in_escaping, param_exclusions, caps);
                walk(compiler, pair.1, in_escaping, param_exclusions, caps);
            }
            if let Some(s) = kwargs_splat {
                walk(compiler, *s, in_escaping, param_exclusions, caps);
            }
            if let Some(b) = block_arg {
                walk(compiler, *b, in_escaping, param_exclusions, caps);
            }
            if let Some(b) = block {
                let HirNode::Block { params, body } = &compiler.hir[*b] else {
                    panic!("a Block should only be reached via the Call that invokes it");
                };
                let is_inline = is_times_fast_path(compiler, *receiver, name, kwargs.is_empty());
                if !is_inline && in_escaping {
                    // A real escaping block nested inside another escaping
                    // block -- Proc-within-Proc, a genuinely harder case
                    // (nested closures capturing across two levels) this
                    // spike doesn't support yet. Clean rejection, matching
                    // this project's "unsupported (spike scope)" posture
                    // elsewhere in codegen.
                    panic!("a block escaping from inside another escaping block isn't supported yet (spike scope)");
                }
                let next_exclusions: HashSet<String> =
                    param_exclusions.union(&own_param_names(params)).cloned().collect();
                let next_in_escaping = in_escaping || !is_inline;
                for &n in body {
                    walk(compiler, n, next_in_escaping, &next_exclusions, caps);
                }
            }
        }
        HirNode::Block { .. } => {
            panic!("a Block should only be reached via the Call that invokes it")
        }
        HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::QualifiedConstRead(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
        MultiTarget::ClassVar(_) | MultiTarget::Global(_) | MultiTarget::Const(_) => {}
        MultiTarget::Call { write_call, tmp_name } => {
            if in_escaping && !param_exclusions.contains(tmp_name) {
                caps.locals.insert(tmp_name.clone());
            }
            walk(compiler, *write_call, in_escaping, param_exclusions, caps);
        }
        MultiTarget::Nested(group) => walk_multi_target_group(compiler, group, in_escaping, param_exclusions, caps),
    }
}

fn walk_multi_target_group(
    compiler: &Compiler,
    group: &crate::hir::MultiTargetGroup,
    in_escaping: bool,
    param_exclusions: &HashSet<String>,
    caps: &mut Captures,
) {
    for t in group.before.iter().chain(&group.after) {
        walk_multi_target(compiler, t, in_escaping, param_exclusions, caps);
    }
    if let Some(Some(t)) = &group.splat {
        walk_multi_target(compiler, t, in_escaping, param_exclusions, caps);
    }
}
