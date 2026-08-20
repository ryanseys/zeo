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

use super::fastpath::is_inline_block_fast_path;
use crate::compiler::Compiler;
use crate::compiler::{FMap, FSet};
use crate::hir::{ArrayElem, HirNode, NodeId, Params, ScopeKind};

#[derive(Default)]
pub struct Captures {
    /// Enclosing-scope local/parameter names referenced (read or written)
    /// inside some escaping block, excluding every block's own params
    /// (that block's own, AND every ancestor inline `.times` block's --
    /// see `walk`'s `param_exclusions`, which is what makes this
    /// exclusion correct even for a name shadowed two levels deep).
    pub locals: FSet<String>,
    /// The subset of `locals` that is ASSIGNED somewhere in the walked
    /// subtree (`LocalWrite`, a multi-assign/`for` target, a pattern's
    /// bound name, a rescue binding) -- as opposed to only ever read.
    /// `emit_proc_or_lambda_value`'s nested-Proc guard consumes this: a
    /// name a nested block assigns is (at worst) that block's own fresh
    /// local, while a name NOBODY under this block assigns can only be a
    /// read of some enclosing block's plain (non-cell) per-invocation
    /// `let`, the one genuinely unsupported capture shape.
    pub assigned: FSet<String>,
    /// Whether any escaping block needs the receiver captured: an
    /// `@ivar` reference, a bare/explicit `self`, or a receiverless call
    /// resolving to a method on the enclosing class (`walk`'s `self_class`
    /// -- an implicit-self dispatch).
    pub self_captured: bool,
    /// Whether an escaping block contains a BARE `super`, which forwards the
    /// enclosing method's parameters as currently bound.
    ///
    /// Those parameters are read by the generated code but appear NOWHERE in
    /// the block's HIR -- the forwarding argument list is synthesized at emit
    /// time (`call::super_calls`) -- so the ordinary local-read walk cannot
    /// see them and they were captured by no one. The block's `move` closure
    /// then consumed the enclosing parameter outright, and any later use in
    /// the method was a borrow-after-move: 14 rustc errors on
    /// `require "active_model"`, over `name`, `options` and `url_safe`.
    pub zsuper_forwards: bool,
    /// Per captured name, the LATEST `(file, offset)` at which an escaping
    /// construct touching it BEGINS -- the position half of Ruby's textual
    /// block-local rule (see `collect_escaping_captures`'s order filter).
    pub locals_at: FMap<String, (crate::hir::FileId, u32)>,
    /// Names touched under a NESTED escaping construct (an escaping block
    /// inside another). The order filter never demotes these: a nested
    /// closure over an enclosing block's own local is only expressible as a
    /// scope-level Captured cell (the shape `procs.rs` otherwise refuses --
    /// optparse's completion lambdas live on it).
    pub nested_touch: FSet<String>,
    /// Per name, the EARLIEST `(file, offset)` of a plain assignment OUTSIDE
    /// any escaping block -- the other half of the same rule. Names assigned
    /// only through shapes this walk doesn't position (params, rescue
    /// bindings, `for` targets) are simply absent, and the order filter
    /// keeps them shared (the pre-order-awareness behavior).
    pub outer_assigned_at: FMap<String, (crate::hir::FileId, u32)>,
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
pub(crate) fn own_param_names(params: &Params) -> FSet<String> {
    params.bound_names().into_iter().collect()
}

/// Everything that runs in a scope's OWN frame: its parameter default
/// expressions, then its body.
///
/// A default is evaluated in the callee's frame when the argument is omitted,
/// so a local it assigns belongs to that scope -- `|name = (given = true)|`,
/// ruby's idiom for "was an argument passed?", which pantheios and roda both
/// write inside a `define_method`. Walking the body alone saw the READ of
/// `given` and no assignment, and read it as a capture of the enclosing
/// block's own local, which is the one nesting shape codegen refuses.
fn scope_nodes(params: &Params, body: &[NodeId]) -> Vec<NodeId> {
    let mut nodes = params.default_ids();
    nodes.extend_from_slice(body);
    nodes
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
    self_class: super::class_query::SelfClass<'_>,
) -> Captures {
    let mut raw = Captures::default();
    for &n in body {
        walk(compiler, n, None, &FSet::default(), &mut raw, self_class);
    }
    let mut outer_names = super::local_storage::Locals::default();
    for &n in body {
        super::local_storage::collect_locals(compiler, n, &mut outer_names);
    }
    // A parameter DEFAULT can assign a local, and ruby scopes it to the whole
    // method: `def fetch(name, default = (no_default = true))` -- memoizable's
    // shape -- makes `no_default` a method local that the body's blocks read.
    // Defaults are not part of `body`, so without this the name never counted
    // as one of the method's own: a block capturing it fresh-declared its own
    // and read `nil`, and a NESTED one hit `procs.rs`'s escaping-block refusal.
    // `hoisting::emit_hoisted_body_with_roots` already walks these for the same
    // reason.
    for id in params.default_ids() {
        super::local_storage::collect_locals(compiler, id, &mut outer_names);
    }
    let mut outer: FSet<String> = outer_names.into_set();
    let outer_params = own_param_names(params);
    outer.extend(outer_params.iter().cloned());
    let mut locals: FSet<String> = raw
        .locals
        .into_iter()
        .filter(|n| outer.contains(n))
        // Ruby's block-local rule is TEXTUAL: a name assigned inside a block
        // is block-local unless the enclosing scope assigned it EARLIER in
        // the source (`b = proc { x = 1 }; x = 5` leaves the outer `x`
        // untouched). Keep a name shared only when some escaping construct
        // touching it begins at or after the scope's first plain outer
        // assignment, in the same file. Everything without both positions --
        // params, rescue/`for` bindings, cross-file splices, synthetic nodes
        // -- stays shared, the conservative pre-order-awareness behavior.
        .filter(|n| {
            // A parameter of THIS scope is bound at its entry, textually
            // before any block -- a mid-scope reassignment must not read as
            // the "first" binding (optparse's `.each do |o| ... o = notwice`
            // reassigns the block's own param below a block that reads it).
            if outer_params.contains(n) || raw.nested_touch.contains(n) {
                return true;
            }
            let (Some(&(tf, tstart)), Some(&(af, astart))) =
                (raw.locals_at.get(n), raw.outer_assigned_at.get(n))
            else {
                return true;
            };
            let keep = tf != af || tstart >= astart;
            if !keep {
                tracing::debug!(
                    name = n,
                    touched = tstart,
                    assigned = astart,
                    "order filter demotes block-local"
                );
            }
            keep
        })
        .collect();
    // A bare `super` in an escaping block reads every parameter of THIS scope,
    // so they all have to be cell-promoted -- otherwise the closure moves the
    // plain binding and the rest of the method cannot use it.
    if raw.zsuper_forwards {
        locals.extend(own_param_names(params));
    }
    Captures {
        locals,
        assigned: raw.assigned,
        self_captured: raw.self_captured,
        zsuper_forwards: raw.zsuper_forwards,
        locals_at: raw.locals_at,
        outer_assigned_at: raw.outer_assigned_at,
        nested_touch: raw.nested_touch,
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
    self_class: super::class_query::SelfClass<'_>,
) -> Captures {
    let mut caps = Captures::default();
    let own = own_param_names(params);
    for n in scope_nodes(params, body) {
        walk(
            compiler,
            n,
            Some((crate::hir::FileId(0), 0, false)),
            &own,
            &mut caps,
            self_class,
        );
    }
    caps
}

/// Whether anything in this scope needs the scope itself as a value:
///
/// - a bare, receiver-less, argument-less `binding` -- the only form that
///   names the CURRENT scope;
/// - a dynamic `eval`, receiver-less or reached through a literal
///   `send(:eval, ...)`, which CRuby runs in the caller's own frame and zeo
///   therefore compiles into a Binding of it;
/// - any block or lambda literal, when the program can ask a `Proc` for its
///   `#binding` ([`crate::hir::Hir::uses_proc_binding`]) -- what that answers
///   is a Binding of the scope the block was WRITTEN in, which is this one.
///
/// Descends through blocks and lambdas (a `binding` taken inside one still
/// exposes the enclosing scope's locals) but stops at a `def`/`class` body,
/// which is a scope of its own.
fn scope_calls_binding(compiler: &Compiler, id: NodeId) -> bool {
    match compiler.hir[id].scope_kind() {
        // A `def`/`class` body is a scope of its own.
        ScopeKind::Definition => return false,
        // A `binding` taken inside a block or lambda still exposes THIS
        // scope's locals, so those are descended through, not stopped at.
        ScopeKind::Block | ScopeKind::Lambda if compiler.hir.uses_proc_binding() => return true,
        ScopeKind::Ffi | ScopeKind::Lambda | ScopeKind::Block | ScopeKind::None => {}
    }
    match &compiler.hir[id] {
        HirNode::Call {
            receiver,
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe: _,
        } if kwargs.is_empty() && block.is_none() && block_arg.is_none() => {
            let bare = receiver.is_none()
                && (name == "binding" && args.is_empty()
                    || name == "eval" && (1..=4).contains(&args.len()));
            let ids: Vec<NodeId> = args.iter().map(|a| a.node_id()).collect();
            if bare || is_sent_eval(compiler, name, &ids) {
                return true;
            }
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

/// Whether this call is `send(:eval, src, ...)` -- the reflective spelling of
/// `Kernel#eval`, which CRuby runs in the caller's frame exactly as the direct
/// one. `public_send` is excluded because `Kernel#eval` is private, so CRuby
/// raises `NoMethodError` there rather than evaluating anything.
pub(crate) fn is_sent_eval(compiler: &Compiler, name: &str, args: &[NodeId]) -> bool {
    matches!(name, "send" | "__send__")
        && (2..=5).contains(&args.len())
        && compiler.hir.sent_name(args[0]) == Some("eval")
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
    // A parenthesized destructuring param's internal slot (`__destr_<i>`) is
    // zeo's own bookkeeping, not a Ruby local: CRuby's `local_variables`
    // reports only the names the destructure BINDS, which `bound_names`
    // already lists beside it. Leaving the slot in also promoted it to a
    // cell, and the destructure that reads it runs in the prologue -- ahead
    // of the promotion -- so the read found a plain value where the cell was
    // promised.
    let mut names: Vec<String> = params
        .bound_names()
        .into_iter()
        .filter(|n| !crate::hir::is_internal_local(n))
        .collect();
    let mut assigned = super::local_storage::Locals::default();
    for &n in body {
        super::local_storage::collect_locals(compiler, n, &mut assigned);
    }
    for n in assigned.into_names() {
        if !crate::hir::is_internal_local(&n) && !names.contains(&n) {
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
    let node = &compiler.hir[id];
    if matches!(node, HirNode::Eval(_)) {
        return true;
    }
    match node.scope_kind() {
        // A `def`/`class` body is a scope of its own.
        ScopeKind::Definition => return false,
        ScopeKind::Ffi | ScopeKind::Lambda | ScopeKind::Block | ScopeKind::None => {}
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

fn node_contains_escaping_return(compiler: &Compiler, id: NodeId, in_escaping: bool) -> bool {
    let node = &compiler.hir[id];
    if let HirNode::Return(v) = node {
        return in_escaping
            || v.is_some_and(|n| node_contains_escaping_return(compiler, n, in_escaping));
    }
    match node.scope_kind() {
        // An FFI wrapper body contains no `return`. A lambda catches its own
        // `Signal::Return` -- a `return` inside it belongs to the lambda,
        // never the enclosing method (see `hir::HirNode::Lambda`'s docs). A
        // `class`/`def` body is a fresh Ruby scope with its own catch.
        ScopeKind::Ffi | ScopeKind::Lambda | ScopeKind::Definition => return false,
        ScopeKind::Block | ScopeKind::None => {}
    }
    // A literal block's body runs at a different escaping-ness than its
    // siblings, and it is the ONLY child that does -- which is what lets one
    // descent replace the arm-per-variant match this used to be. An escaping
    // (non-inline) block's body runs as a proc homed to this method, so a
    // `return` anywhere inside it needs the catch; an inline `.times` block
    // shares this scope, so its `return` is literal and only counts if the
    // walk was already inside an escaping one. `new` and `super` have no
    // inline form, so their blocks always escape.
    let block = match node {
        HirNode::Call { block, .. }
        | HirNode::New { block, .. }
        | HirNode::SuperCall { block, .. } => *block,
        _ => None,
    };
    let in_block = match node {
        HirNode::Call {
            receiver,
            name,
            kwargs,
            ..
        } => {
            in_escaping || !is_inline_block_fast_path(compiler, *receiver, name, kwargs.is_empty())
        }
        _ => true,
    };
    let mut found = false;
    node.for_each_child(&mut |c| {
        let escaping = if Some(c) == block {
            in_block
        } else {
            in_escaping
        };
        found = found || node_contains_escaping_return(compiler, c, escaping);
    });
    found
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

/// Whether the subtree at `id` contains a literal `return`, stopping at a
/// LAMBDA boundary (a `return` there belongs to the lambda). What decides
/// whether a `begin` really needs the per-method catch: the closure boundary
/// only traps a `Signal::Return` this method's own `return` raised, and a
/// method with none can only intercept one meant for somebody else.
fn contains_return(compiler: &Compiler, id: NodeId) -> bool {
    let node = &compiler.hir[id];
    if matches!(node, HirNode::Return(_)) {
        return true;
    }
    match node.scope_kind() {
        // A lambda catches its own `Signal::Return`; an FFI wrapper body has
        // no `return` in it.
        ScopeKind::Lambda | ScopeKind::Ffi => return false,
        // NOTE: unlike its callers, this walk does NOT stop at a `Definition`.
        // A `return` inside a nested `def` belongs to that def, so counting it
        // here over-approximates -- which only ever installs a catch that is
        // never entered. Kept as-is rather than tightened blind; the shape is
        // now visible next to the policies that do stop there.
        ScopeKind::Definition | ScopeKind::Block | ScopeKind::None => {}
    }
    let mut found = false;
    compiler.hir[id].for_each_child(&mut |c| {
        found = found || contains_return(compiler, c);
    });
    found
}

/// Whether a `begin`/`rescue` under `id` could catch a `Signal::Return`, so
/// the enclosing method must install its own catch.
///
/// Descends through `for_each_child` -- the same shape as `contains_return`
/// above -- rather than re-listing every `HirNode` variant, which is what
/// this used to do across 256 lines.
fn node_contains_begin(compiler: &Compiler, id: NodeId) -> bool {
    let node = &compiler.hir[id];
    // The whole subtree is searched for the `return` this `begin` would
    // catch, so there is nothing further to descend for here.
    if matches!(node, HirNode::Begin { .. }) {
        return contains_return(compiler, id);
    }
    match node.scope_kind() {
        // An FFI wrapper body contains no `begin`. A lambda's own body is a
        // fully self-contained closure boundary, so a `begin`/`rescue`
        // lexically inside one never requires the ENCLOSING method to install
        // its own `Signal::Return` catch -- and a `class`/`def` body is a
        // fresh Ruby scope with its own.
        ScopeKind::Ffi | ScopeKind::Lambda | ScopeKind::Definition => return false,
        ScopeKind::Block | ScopeKind::None => {}
    }
    let mut found = false;
    node.for_each_child(&mut |c| {
        found = found || node_contains_begin(compiler, c);
    });
    found
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
/// Whether the class this block is being compiled under has a real method of
/// this name -- in which case a receiver-less call to it is NOT the Kernel free
/// function [`is_kernel_free_fn`] assumes. `Object` is excluded: every class
/// inherits Kernel through it, so a hit there proves nothing.
fn self_class_overrides(
    compiler: &crate::compiler::Compiler,
    self_class: super::class_query::SelfClass<'_>,
    name: &str,
) -> bool {
    self_class
        .ask(
            compiler,
            super::class_query::ClassQuery::ShadowsKernel(name.to_string()),
        )
        .is_some_and(|a| a.yes())
}

/// A block or lambda written INSIDE an escaping block makes that block's
/// scope a `Proc#binding` scope -- [`scope_calls_binding`] answers true for
/// exactly this shape -- and the nested Proc's construction reads the
/// enclosing `self` for that binding, in the enclosing block's own prelude,
/// which sits inside its `move` closure. So the receiver must arrive as the
/// closure's self PARAMETER: captured by move, an `Fn` closure cannot consume
/// it, and the enclosing method loses it outright.
/// The `(file, start offset)` of `id`'s span -- the position half of the
/// textual block-local rule. `None` for a synthetic node, which the order
/// filter treats as "keep shared" (the conservative pre-order behavior).
fn start_of(compiler: &Compiler, id: NodeId) -> Option<(crate::hir::FileId, u32)> {
    compiler
        .hir
        .span(id)
        .and_then(|s| s.known())
        .map(|k| (k.file, k.start))
}

/// Records the LATEST same-file escaping-construct start touching `name`
/// (a different file simply overwrites -- the order filter then sees a
/// cross-file pair and keeps the name shared).
fn note_touch(
    map: &mut FMap<String, (crate::hir::FileId, u32)>,
    name: &str,
    at: (crate::hir::FileId, u32),
) {
    let e = map.entry(name.to_string()).or_insert(at);
    if at.0 != e.0 || at.1 > e.1 {
        *e = at;
    }
}

/// Records the EARLIEST same-file plain outer assignment of `name`.
fn note_first_assign(
    map: &mut FMap<String, (crate::hir::FileId, u32)>,
    name: &str,
    pos: (crate::hir::FileId, u32),
) {
    let e = map.entry(name.to_string()).or_insert(pos);
    if pos.0 == e.0 && pos.1 < e.1 {
        *e = pos;
    }
}

/// The position the textual block-local rule compares an escaping block
/// against: the start of its BODY. A `Block` node's own span begins at the
/// CALL that owns it (`recv.m(args) do ... end` spans from `recv`), which
/// sits textually BEFORE the argument list -- so an assignment inside an
/// argument of the same call (`fetch(name = expr) { name }`, activerecord's
/// calculations.rb shape) compared as "after" the block and was wrongly
/// demoted to block-local. The first body statement is the first position
/// genuinely inside the block; an empty body falls back to the block span
/// (it touches no names, so the position never matters).
fn block_body_pos(
    compiler: &Compiler,
    body: &[NodeId],
    block: NodeId,
) -> Option<(crate::hir::FileId, u32)> {
    body.iter()
        .find_map(|&n| start_of(compiler, n))
        .or_else(|| start_of(compiler, block))
}

/// The escaping context one level deeper: entering an escaping construct
/// keeps the OUTERMOST position (that is the one the textual rule compares)
/// and marks everything below as nested once a second level begins.
fn deepen(
    escaping_at: Option<(crate::hir::FileId, u32, bool)>,
    enter: impl FnOnce() -> Option<(crate::hir::FileId, u32)>,
) -> Option<(crate::hir::FileId, u32, bool)> {
    match escaping_at {
        Some((f, s, _)) => Some((f, s, true)),
        None => enter().map(|(f, s)| (f, s, false)),
    }
}

fn nested_proc_binding_needs_self(
    compiler: &crate::compiler::Compiler,
    in_escaping: bool,
    caps: &mut Captures,
) {
    if in_escaping && compiler.hir.uses_proc_binding() {
        caps.self_captured = true;
    }
}

/// Whether `name` is a Kernel free function, which a receiver-less call can
/// reach without consulting `self`.
///
/// Deliberately CONSERVATIVE. A name wrongly listed here loses its receiver,
/// which is a real bug: an `instance_exec`'d block calling it would dispatch
/// on the wrong object. A name wrongly missing only makes some block carry a
/// self it never reads, which costs one moved `RubyValue`. So when in doubt,
/// leave it out. `method`, `send` and `raise` are absent on purpose, because
/// they genuinely consult the implicit receiver.
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
    escaping_at: Option<(crate::hir::FileId, u32, bool)>,
    param_exclusions: &FSet<String>,
    caps: &mut Captures,
    self_class: super::class_query::SelfClass<'_>,
) {
    match &compiler.hir[id] {
        // An FFI wrapper body has no escaping block, so nothing of the
        // enclosing scope is captured through it.
        HirNode::Ffi(_) => {}
        HirNode::LocalRead(name) => {
            if let Some((f, start, nested)) = escaping_at
                && !param_exclusions.contains(name)
            {
                caps.locals.insert(name.clone());
                note_touch(&mut caps.locals_at, name, (f, start));
                if nested {
                    caps.nested_touch.insert(name.clone());
                }
            }
        }
        HirNode::LocalWrite(name, value) => {
            match escaping_at {
                Some((f, start, nested)) if !param_exclusions.contains(name) => {
                    caps.locals.insert(name.clone());
                    caps.assigned.insert(name.clone());
                    note_touch(&mut caps.locals_at, name, (f, start));
                    if nested {
                        caps.nested_touch.insert(name.clone());
                    }
                }
                // A plain OUTER assignment: its position is what the textual
                // block-local rule compares block starts against.
                None => {
                    if let Some(pos) = start_of(compiler, id) {
                        note_first_assign(&mut caps.outer_assigned_at, name, pos);
                    }
                }
                _ => {}
            }
            walk(compiler, *value, escaping_at, param_exclusions, caps, self_class);
        }
        HirNode::IvarRead(_) => {
            if escaping_at.is_some() {
                caps.self_captured = true;
            }
        }
        HirNode::IvarWrite(_, value) => {
            if escaping_at.is_some() {
                caps.self_captured = true;
            }
            walk(compiler, *value, escaping_at, param_exclusions, caps, self_class);
        }
        // A bare/explicit `self` reference is another way an escaping block
        // needs the receiver captured -- same flag `IvarRead`/`IvarWrite`
        // already set above (they're really just `self`-via-ivar-sugar).
        HirNode::SelfRef => {
            if escaping_at.is_some() {
                caps.self_captured = true;
            }
        }
        // A class variable's storage is keyed by a compile-time-resolved
        // OWNER CLASS id (see `analyze::mro::resolve_cvars`), never by
        // `self` -- unlike an ivar, referencing `@@x` inside an escaping
        // block needs no `self` capture at all.
        HirNode::ClassVarRead(_) => {}
        // PURE DESCENT: every child inherits this node's state unchanged, so
        // `for_each_child` expresses them all. Listed by variant rather than
        // behind a `_`, deliberately: this walk RECORDS things, and a new
        // `HirNode` that needs to record something must fail to compile here
        // rather than silently inherit plain descent.
        HirNode::ClassVarWrite(..)
        | HirNode::GlobalWrite(..)
        | HirNode::ConstWrite { .. }
        | HirNode::DynConstRead { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::Defined(_)
        | HirNode::And(..)
        | HirNode::Or(..)
        | HirNode::FlipFlop { .. }
        | HirNode::If { .. }
        | HirNode::CaseWhen { .. }
        | HirNode::While { .. }
        | HirNode::Loop { .. }
        | HirNode::Break(_)
        | HirNode::Next(_)
        | HirNode::Return(_)
        | HirNode::PreExec(_)
        | HirNode::Seq(_)
        | HirNode::Eval(_)
        | HirNode::BoxScope { .. }
        | HirNode::Yield(_)
        | HirNode::Raise(..)
        | HirNode::ArrayLit(_)
        | HirNode::HashLit(_)
        | HirNode::RangeLit { .. }
        | HirNode::StringLit(_)
        | HirNode::RegexpLit(..) => {
            compiler.hir[id].for_each_child(&mut |n| {
                walk(compiler, n, escaping_at, param_exclusions, caps, self_class)
            });
        }
        HirNode::Lambda {
            params,
            body,
            method_body: _,
        } => {
            nested_proc_binding_needs_self(compiler, escaping_at.is_some(), caps);
            let next_exclusions: FSet<String> =
                param_exclusions.union(&own_param_names(params)).cloned().collect();
            for n in scope_nodes(params, body) {
                walk(compiler, n, deepen(escaping_at, || start_of(compiler, id)), &next_exclusions, caps, self_class);
            }
        }
        HirNode::For { target, iterable, body } => {
            walk_multi_target(
                compiler,
                target,
                escaping_at,
                start_of(compiler, id),
                param_exclusions,
                caps,
                self_class,
            );
            walk(compiler, *iterable, escaping_at, param_exclusions, caps, self_class);
            for &n in body {
                walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
            }
        }
        HirNode::Redo | HirNode::BlockGiven => {}
        HirNode::MultiWrite { targets, value } => {
            walk_multi_target_group(
                compiler,
                targets,
                escaping_at,
                start_of(compiler, id),
                param_exclusions,
                caps,
                self_class,
            );
            walk(compiler, *value, escaping_at, param_exclusions, caps, self_class);
        }
        // A global/constant's storage doesn't depend on `self`/enclosing
        // locals at all -- no capture registration needed, same posture as
        // `ClassVarWrite` just above.
        HirNode::CaseIn { subject, arms, else_body } => {
            walk(compiler, *subject, escaping_at, param_exclusions, caps, self_class);
            for arm in arms {
                // A pattern's bound names are a fresh binding, same
                // treatment as `LocalWrite` just above -- only registered
                // as a capture candidate while inside an escaping block; the
                // later intersection with `collect_locals`'s whole-scope
                // result (see `collect_escaping_captures`) is what decides
                // whether it's GENUINELY shared with code outside the block.
                if escaping_at.is_some() {
                    arm.pattern.for_each_bound_name(&mut |n| {
                        if !param_exclusions.contains(n) {
                            caps.locals.insert(n.to_string());
                            caps.assigned.insert(n.to_string());
                        }
                    });
                }
                arm.pattern
                    .for_each_node(&mut |n| walk(compiler, n, escaping_at, param_exclusions, caps, self_class));
                if let Some((g, _)) = arm.guard {
                    walk(compiler, g, escaping_at, param_exclusions, caps, self_class);
                }
                for &n in &arm.body {
                    walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            walk(compiler, *subject, escaping_at, param_exclusions, caps, self_class);
            if escaping_at.is_some() {
                pattern.for_each_bound_name(&mut |n| {
                    if !param_exclusions.contains(n) {
                        caps.locals.insert(n.to_string());
                        caps.assigned.insert(n.to_string());
                    }
                });
            }
            pattern.for_each_node(&mut |n| walk(compiler, n, escaping_at, param_exclusions, caps, self_class));
        }
        HirNode::Begin { body, rescues, else_body, ensure_body } => {
            for &n in body {
                walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
            }
            for r in rescues {
                // A splatted exception list (`rescue *errs`) reads outer locals
                // -- they must be captured when this `begin` is inside a closure.
                for &n in &r.splats {
                    walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
                }
                // A rescue binding is a fresh name, same treatment as
                // `LocalWrite`/a pattern's bound names just above.
                if escaping_at.is_some()
                    && let Some(name) = &r.binding
                        && !param_exclusions.contains(name) {
                            caps.locals.insert(name.clone());
                            caps.assigned.insert(name.clone());
                        }
                for &n in &r.body {
                    walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
                }
            }
        }
        HirNode::Retry => {}
        HirNode::New {
            class_name: _,
            args,
            kwargs,
            block,
        } => {
            for &a in args {
                walk(compiler, a, escaping_at, param_exclusions, caps, self_class);
            }
            for a in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                walk(compiler, a, escaping_at, param_exclusions, caps, self_class);
            }
            // A literal block forwarded to `initialize` is a real escaping
            // Proc -- same treatment as `super { ... }` below.
            if let Some(b) = block
                && let HirNode::Block { params, body } = &compiler.hir[*b] {
                    nested_proc_binding_needs_self(compiler, escaping_at.is_some(), caps);
                    let next_exclusions: FSet<String> =
                        param_exclusions.union(&own_param_names(params)).cloned().collect();
                    for n in scope_nodes(params, body) {
                        walk(compiler, n, deepen(escaping_at, || start_of(compiler, *b)), &next_exclusions, caps, self_class);
                    }
                }
        }
        HirNode::SuperCall {
            args,
            kwargs,
            zsuper,
            block,
            block_arg,
        } => {
            // `super` implicitly dispatches on the receiver, so an escaping
            // block containing one must capture `self` -- same flag `SelfRef`/
            // `IvarRead` set above. Without this a `super` in a method-body
            // lambda (a `def` in a `Class.new`/`Struct.new` block) would emit
            // a self reference the closure never binds.
            if escaping_at.is_some() {
                caps.self_captured = true;
                // A BARE `super` also forwards the enclosing method's
                // parameters, and those reads exist only in the emitted
                // forwarding list -- there is no HIR node here to walk. Record
                // the fact so the caller can capture them; see
                // `Captures::zsuper_forwards`.
                if *zsuper {
                    caps.zsuper_forwards = true;
                }
            }
            for a in args {
                walk(compiler, a.node_id(), escaping_at, param_exclusions, caps, self_class);
            }
            for a in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                walk(compiler, a, escaping_at, param_exclusions, caps, self_class);
            }
            if let Some(b) = block_arg {
                walk(compiler, *b, escaping_at, param_exclusions, caps, self_class);
            }
            // A literal `super { ... }` block is always a real, escaping
            // Proc (no `.times`-style inline fast path exists for `super`)
            // -- same treatment as `Call`'s escaping-block arm above.
            if let Some(b) = block
                && let HirNode::Block { params, body } = &compiler.hir[*b] {
                    let next_exclusions: FSet<String> =
                        param_exclusions.union(&own_param_names(params)).cloned().collect();
                    for n in scope_nodes(params, body) {
                        walk(compiler, n, deepen(escaping_at, || block_body_pos(compiler, body, *b)), &next_exclusions, caps, self_class);
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
            if receiver.is_none() && escaping_at.is_some() && !kernel_free {
                caps.self_captured = true;
            }
            if let Some(r) = receiver {
                walk(compiler, *r, escaping_at, param_exclusions, caps, self_class);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                walk(compiler, *n, escaping_at, param_exclusions, caps, self_class);
            }
            for n in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                walk(compiler, n, escaping_at, param_exclusions, caps, self_class);
            }
            if let Some(b) = block_arg {
                walk(compiler, *b, escaping_at, param_exclusions, caps, self_class);
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
                nested_proc_binding_needs_self(compiler, escaping_at.is_some(), caps);
                let next_exclusions: FSet<String> =
                    param_exclusions.union(&own_param_names(params)).cloned().collect();
                let next_escaping_at = if is_inline {
                    escaping_at
                } else {
                    deepen(escaping_at, || block_body_pos(compiler, body, *b))
                };
                for n in scope_nodes(params, body) {
                    walk(compiler, n, next_escaping_at, &next_exclusions, caps, self_class);
                }
            }
        }
        // A `def`/literal `define_method` compiles to
        // `self.define_method(:name, ->(params){ body })` (see
        // `codegen::expr`'s `DefMethod` arm): it USES `self` (the install
        // target) and its body is an escaping proc.
        //
        // A `define_method` body is a CLOSURE over this scope, so what it reads
        // is captured wherever the definition sits -- including at the top
        // level of a block, which is the ordinary class-macro shape
        // (`class_exec(5) { |n| define_method(:n) { n } }`) and the one
        // position an `in_escaping`-only test misses.
        //
        // A real `def` body is a scope of its own and cannot see an enclosing
        // local at all, so it captures nothing by itself; it is still walked
        // inside an escaping block, where it needs `self`.
        HirNode::DefMethod {
            name: _,
            params,
            body,
            is_class_method: _,
            visibility: _,
            is_def,
        } => {
            if escaping_at.is_some() || !is_def {
                caps.self_captured = true;
                let next_exclusions: FSet<String> =
                    param_exclusions.union(&own_param_names(params)).cloned().collect();
                for n in scope_nodes(params, body) {
                    walk(compiler, n, deepen(escaping_at, || start_of(compiler, id)), &next_exclusions, caps, self_class);
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
    escaping_at: Option<(crate::hir::FileId, u32, bool)>,
    outer_at: Option<(crate::hir::FileId, u32)>,
    param_exclusions: &FSet<String>,
    caps: &mut Captures,
    self_class: super::class_query::SelfClass<'_>,
) {
    use crate::hir::MultiTarget;
    match target {
        MultiTarget::Local(name) => {
            match escaping_at {
                Some(_) if !param_exclusions.contains(name) => {
                    caps.locals.insert(name.clone());
                    caps.assigned.insert(name.clone());
                }
                // A multi-assign/`for` target OUTSIDE any escaping block is a
                // plain outer assignment for the textual block-local rule,
                // positioned at the enclosing statement (its desugared writes
                // are synthetic, so the `LocalWrite` arm never sees a span).
                // Without this, minitest's `level, n_combos = 1, 1` above a
                // `loop do ... find { level } ... level = 1 ... end` counted
                // the RE-assignment inside the loop as the first, demoting
                // `level` to loop-block-local and refusing the nested `find`.
                None => {
                    if let Some(pos) = outer_at {
                        note_first_assign(&mut caps.outer_assigned_at, name, pos);
                    }
                }
                _ => {}
            }
        }
        MultiTarget::Ivar(_) => {
            if escaping_at.is_some() {
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
            if escaping_at.is_some() && !param_exclusions.contains(tmp_name) {
                caps.locals.insert(tmp_name.clone());
                caps.assigned.insert(tmp_name.clone());
            }
            walk(
                compiler,
                *write_call,
                escaping_at,
                param_exclusions,
                caps,
                self_class,
            );
        }
        MultiTarget::Nested(group) => walk_multi_target_group(
            compiler,
            group,
            escaping_at,
            outer_at,
            param_exclusions,
            caps,
            self_class,
        ),
    }
}

fn walk_multi_target_group(
    compiler: &Compiler,
    group: &crate::hir::MultiTargetGroup,
    escaping_at: Option<(crate::hir::FileId, u32, bool)>,
    outer_at: Option<(crate::hir::FileId, u32)>,
    param_exclusions: &FSet<String>,
    caps: &mut Captures,
    self_class: super::class_query::SelfClass<'_>,
) {
    for t in group.before.iter().chain(&group.after) {
        walk_multi_target(
            compiler,
            t,
            escaping_at,
            outer_at,
            param_exclusions,
            caps,
            self_class,
        );
    }
    if let Some(Some(t)) = &group.splat {
        walk_multi_target(
            compiler,
            t,
            escaping_at,
            outer_at,
            param_exclusions,
            caps,
            self_class,
        );
    }
}
