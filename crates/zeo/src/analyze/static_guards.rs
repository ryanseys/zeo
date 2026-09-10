//! The statically-decidable guard/splice machinery for top-level and
//! class-body statements: dead-rescue splicing, decidable `if` splicing,
//! conditional reopens, guarded top defs, and the `static_top_cond`
//! condition folder they share. Split out of `top_stmts.rs`, whose
//! `process_top_stmt` walk is the caller.
//!
//! Distinct from `parse/loader/static_guards.rs`, which decides loader-time
//! platform guards while files are still being loaded, before any
//! `Compiler` exists.

use super::*;
use crate::diagnostics::analyze::AnalyzeError;

/// If `stmt` is a `begin/rescue` whose body provably cannot raise -- the
/// `require` a `begin; require "x"; rescue LoadError` guard lowers to when the
/// feature RESOLVED (a shim/builtin/file folded it to a bool), so the rescue is
/// DEAD, exactly CRuby's behavior when the extension IS present -- returns the
/// LIVE statements (begin + else + ensure), the rescue clauses dropped. `None`
/// for anything else, so a `begin` that can raise is kept whole. Dropping the
/// dead rescue keeps its unregistered fallback defs from reaching codegen.
pub(super) fn dead_rescue_live_body(compiler: &Compiler, stmt: NodeId) -> Option<Vec<NodeId>> {
    let HirNode::Begin {
        body,
        rescues,
        else_body,
        ensure_body,
    } = &compiler.hir[stmt]
    else {
        return None;
    };
    if rescues.is_empty() || !body_cannot_raise(compiler, body) {
        return None;
    }
    let mut live = body.clone();
    live.extend(else_body.clone().unwrap_or_default());
    live.extend(ensure_body.clone().unwrap_or_default());
    Some(live)
}

/// Expand every dead-rescue `begin` (see `dead_rescue_live_body`) in `stmts`
/// to its live statements, recursively, leaving all other statements in place.
/// Applied to a class body before the walk so a `begin; require "strscan";
/// rescue LoadError; class SimpleScanner` fallback registers/emits correctly.
pub(super) fn splice_dead_rescues(compiler: &Compiler, stmts: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &s in stmts {
        match dead_rescue_live_body(compiler, s) {
            Some(live) => out.extend(splice_dead_rescues(compiler, &live)),
            None => out.push(s),
        }
    }
    out
}

/// Inline a class-body `if`/`unless` whose guard is compile-time decidable
/// (`static_top_cond` -- the same superset the top level folds with,
/// resolved in this class body's `cref`) AND whose taken branch holds a
/// nested `class`/`module`/`include`/`prepend` definition -- the class-body
/// analogue of `process_top_stmt`'s top-level conditional-def handling. Only
/// the taken branch's statements survive, registered exactly as if written
/// directly in the class body; the folded `If` never reaches the site's
/// statement list, so emission reads the same spliced body and cannot
/// disagree. A `def`-only branch is left alone (its runtime `define_method`
/// handles it via `register_conditional_defs`), and an UNDECIDABLE guard
/// over a definition is left to fail loudly downstream rather than silently
/// mis-registered. This is what lets a target-version / feature-probe gate
/// wrapping a module def (bundler's `ForkTracker`) register.
pub(super) fn splice_decidable_ifs(
    compiler: &Compiler,
    stmts: &[NodeId],
    cref: &[ClassId],
    box_id: u32,
) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &s in stmts {
        if let HirNode::If {
            cond,
            then_body,
            else_body,
        } = &compiler.hir[s]
        {
            let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
            if branch_has_top_defs(compiler, &then_body)
                || branch_has_top_defs(compiler, &else_body)
            {
                // The SAME superset the top level folds with
                // (`static_top_cond`), in this body's cref -- a gem writing
                // the identical `defined?` guard one nesting level in used
                // to fold strictly less. Sound here because the folded `If`
                // never reaches the site's statement list, so registration
                // and emission read the same spliced body.
                let guarded: Vec<NodeId> = then_body.iter().chain(&else_body).copied().collect();
                if let Some(taken) = static_top_cond(compiler, cond, &guarded, cref, box_id, true) {
                    let branch = if taken { then_body } else { else_body };
                    out.extend(splice_decidable_ifs(compiler, &branch, cref, box_id));
                    continue;
                }
            }
        }
        out.push(s);
    }
    out
}

/// Whether every statement in `body` is provably non-raising -- the folded
/// `require`(s) a `begin; require "x"; rescue LoadError` guard lowers to when
/// the feature IS resolvable, making the rescue dead. Deliberately narrow
/// (literals only): a false negative just keeps the `begin` whole.
fn body_cannot_raise(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter().all(|&s| match &compiler.hir[s] {
        HirNode::BoolLit(_)
        | HirNode::NilLit
        | HirNode::IntegerLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_) => true,
        // The record a resolved `require` of a native feature leaves at its
        // own line: an array append and two flags, and it is exactly what a
        // guarded `require` lowers to beside its load result.
        //
        // `CExtLoaded` is deliberately NOT here: it dlopens and runs the
        // extension's `Init_`, and either can raise.
        HirNode::FeatureLoaded { .. } => true,
        HirNode::Seq(parts) => body_cannot_raise(compiler, parts),
        _ => false,
    })
}

/// Whether any statement in `body` (descending nested `if` branches) is a
/// node only the top-level walk can register -- exactly the set
/// `clif::expr::lower_expr` has no expression form for, minus
/// `DefMethod` (which has a runtime `define_method` emission and so
/// survives inside an ordinary undecided `if` unchanged).
pub(super) fn branch_has_top_defs(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter().any(|&s| {
        let node = &compiler.hir[s];
        // The class-body directives, asked of the one exhaustive list rather
        // than re-spelled here -- `HirNode::is_class_body_directive` records
        // what re-spelling it cost the last time.
        node.is_class_body_directive()
            || match node {
                // A `def` counts for exactly the reason every directive does.
                // Left out, an `if RUBY_ENGINE == "ruby"` whose branches hold
                // only methods never folded: BOTH branches registered, and the
                // one written last silently won -- so a compat gate picked the
                // branch for the engine zeo is not (prism's deserializer has
                // two `def load_node`s exactly this way).
                //
                // The three that are NOT directives but still register: `using`
                // edits the compile-time refinement set, and the two definition
                // REPORTS name methods the walk has to have seen.
                HirNode::DefMethod { .. }
                | HirNode::ClassDef { .. }
                | HirNode::Using(_)
                | HirNode::DefHook { .. }
                | HirNode::MethodRedefine { .. } => true,
                HirNode::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    branch_has_top_defs(compiler, then_body)
                        || branch_has_top_defs(compiler, else_body)
                }
                _ => false,
            }
    })
}

/// `clif::stmt::static_cond`'s analyze-time sibling: compile-time
/// truthiness of a top-level `if` condition, decided against the classes
/// REGISTERED SO FAR in the walk -- which is exactly the set real Ruby has
/// defined when execution reaches this statement, since the walk mirrors
/// top-level execution order (a class defined LATER in the file is not
/// `defined?` yet at this point in real Ruby either, and a require-gated
/// builtin is invisible before its `require`, matching CRuby's undefined
/// constant there too).
///
/// Differences from the codegen fold, which runs after
/// `mro::resolve_consts`: VALUE constants aren't registered yet here, so a
/// name that might be one -- any name the program `ConstWrite`s, or a
/// qualified read whose scope resolves without a nested class match --
/// degrades to `None` (undecidable) rather than a confident `false`.
/// If an undecidable-guard top-level `if` holds nothing but conditional
/// REOPENINGS of already-registered classes -- `class Existing ... end if cond`,
/// or an `unless` wrapping a run of them (one branch all `ClassDef`s naming
/// known classes, the other empty) -- rewrite each by pushing the guard INTO
/// the class body: `class Existing; if cond; <body>; end; end`. The ordinary
/// class-body walk already handles that shape (a conditional `def` becomes a
/// runtime `define_method`, still registered for reflection via
/// `register_conditional_defs` -- the fileutils platform-`def` path), so no new
/// codegen is needed. Returns the rewritten `ClassDef`s in source order, or
/// `None` when the pattern doesn't match -- a NEW class, a non-empty other
/// branch, or a non-`ClassDef` statement stay a clean compile error.
///
/// Valid ONLY for a reopening: for a NEW class the transform would define it
/// unconditionally (`class X; if cond; ...` always creates `X`), changing
/// semantics. pp.rb's `class Set ... end if set_pp` monkeypatch is the
/// one-statement case; activesupport's `unless methods_are_duplicable`, which
/// reopens `Method` and `UnboundMethod` together behind a `begin/rescue` probe
/// no compile-time analysis can settle, is why the run is not capped at one.
pub(super) fn try_conditional_reopen(
    compiler: &mut Compiler,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
) -> Option<Vec<NodeId>> {
    let mut guards = Vec::new();
    let defs = peel_one_sided_guards(compiler, cond, then_body, else_body, &mut guards)?;
    // What one guarded statement becomes. Cloned parts, so the
    // `&compiler.hir` borrow ends before the `resolve_class` read and the
    // `hir.push` writes below. Every statement must qualify: a partial
    // rewrite would drop the rest of the branch.
    enum Guarded {
        /// A reopening of an already-registered class: the guard pushes into
        /// its body.
        Reopen(String, Option<String>, Vec<NodeId>, bool),
        /// A definition-level directive whose runtime self-send spelling the
        /// class-body machinery already provides (`alias_method`, `private`,
        /// a conditional `def`): the guard rides into a synthesized `Object`
        /// reopen, which is the class a top-level directive targets. NOT
        /// `include`/`prepend`: a runtime mixin is invisible to the static
        /// MRO, and compiling one would miss at statically-resolved sites.
        Directive(NodeId),
    }
    let mut parts = Vec::with_capacity(defs.len());
    for &def_stmt in &defs {
        match &compiler.hir[def_stmt] {
            HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } => parts.push(Guarded::Reopen(
                name.clone(),
                superclass.clone(),
                body.clone(),
                *is_module,
            )),
            HirNode::DefMethod { .. }
            | HirNode::AliasMethod { .. }
            | HirNode::MethodVisibility { .. }
            | HirNode::ClassMethodVisibility { .. }
            | HirNode::ModuleFunction { .. }
            | HirNode::Undef { .. }
            | HirNode::ClassMethodUndef { .. } => parts.push(Guarded::Directive(def_stmt)),
            _ => return None,
        }
    }
    // A reopening must name an already-registered UNCONDITIONAL class
    // (top-level cref/box). A new class -- and a class that is itself
    // runtime-conditional, whose reveal must stay inside the live `if` --
    // goes through `register_guarded_top_defs` instead, which keeps the
    // whole guard as runtime code rather than pushing it into the body.
    if !parts.iter().all(|p| match p {
        Guarded::Reopen(name, ..) => compiler
            .resolve_class(name, &[], 0)
            .is_some_and(|cid| !compiler.class(cid).runtime_conditional),
        Guarded::Directive(_) => true,
    }) {
        return None;
    }
    let wrap_guards = |compiler: &mut Compiler, body: Vec<NodeId>| {
        // Innermost guard first, so the chain comes back out in the order it
        // was written: `if outer; if inner; <body>; end; end`.
        let mut body = body;
        for &(cond, on_then) in guards.iter().rev() {
            // The guard's own position and its body's, so the wrapper CONTAINS
            // what it wraps -- `eval_body_source`'s slice is taken only then.
            let mut covered = body.clone();
            covered.push(cond);
            let span = covering_span(compiler, &covered);
            let (then_body, else_body) = if on_then {
                (body, Vec::new())
            } else {
                (Vec::new(), body)
            };
            compiler.hir.push_span(span);
            let wrapped = compiler.hir.push(HirNode::If {
                cond,
                then_body,
                else_body,
            });
            compiler.hir.pop_span();
            compiler
                .hir
                .set_flag(wrapped, crate::hir::NodeFlag::HOISTED_CLASS_GUARD);
            body = vec![wrapped];
        }
        body
    };
    Some(
        parts
            .into_iter()
            .map(|part| match part {
                Guarded::Reopen(name, superclass, body, is_module) => {
                    let span = covering_span(compiler, &body);
                    let body = wrap_guards(compiler, body);
                    compiler.hir.push_span(span);
                    let id = compiler.hir.push(HirNode::ClassDef {
                        name,
                        superclass,
                        body,
                        is_module,
                    });
                    compiler.hir.pop_span();
                    id
                }
                Guarded::Directive(stmt) => {
                    let span = covering_span(compiler, &[stmt]);
                    let body = wrap_guards(compiler, vec![stmt]);
                    compiler.hir.push_span(span);
                    let id = compiler.hir.push(HirNode::ClassDef {
                        name: "Object".to_string(),
                        superclass: None,
                        body,
                        is_module: false,
                    });
                    compiler.hir.pop_span();
                    id
                }
            })
            .collect(),
    )
}

/// A span COVERING every statement in `body`, for a `ClassDef` this pass
/// synthesizes rather than reads off the source.
///
/// The node needs one at all because a class body compiled at RUN time is
/// re-evaluated from its own SOURCE TEXT, sliced between the first and last
/// statement (`clif::eval::eval_body_source`) -- and the slice is taken only
/// when the class's own span CONTAINS them. A span-less wrapper made the
/// run-time compiler refuse the whole file: tomlrb, reached through
/// `bundler/setup`, died on "a span-less `class` (`Object`)".
///
/// `None` (a synthetic or empty body, or statements from two files) keeps the
/// old `Span::SYNTH`, which is what the wrapper had before.
fn covering_span(compiler: &Compiler, body: &[NodeId]) -> crate::hir::Span {
    let mut it = body.iter().filter_map(|&n| compiler.hir.span(n));
    let Some(first) = it.next() else {
        return crate::hir::Span::SYNTH;
    };
    let mut span = first;
    for s in it {
        if s.file != span.file {
            return crate::hir::Span::SYNTH;
        }
        span.start = span.start.min(s.start);
        span.end = span.end.max(s.end);
    }
    span
}

/// An undecidable-guard top-level `if` whose definitions are `def`s on
/// the main object/`Object` and/or whole `class`/`module` definitions. A
/// `def self.x` rewrites in place to the same runtime
/// `self.define_singleton_method(:x, lambda)` the unguarded arm desugars
/// to; a plain `def x` registers on `Object` as `Conditional::Yes` (a
/// private instance method exactly when its branch runs). A
/// `class`/`module` -- new or reopening -- REGISTERS here: a fresh
/// one as `ClassInfo::runtime_conditional` (concealed until its body
/// runs), every `def` in either as `Conditional::Yes`, while the node
/// stays put so the body executes at document position inside the
/// still-live `if`. Interleaved plain statements keep their order.
/// `Ok(true)` means the caller keeps the whole `if` as ordinary runtime
/// code; a branch holding any OTHER definition shape (a top-level mixin)
/// answers `Ok(false)` and keeps the compile-time error.
pub(super) fn register_guarded_top_defs(
    compiler: &mut Compiler,
    stmt: NodeId,
) -> Result<bool, AnalyzeError> {
    fn qualifies(compiler: &Compiler, body: &[NodeId]) -> bool {
        body.iter().all(|&s| match &compiler.hir[s] {
            HirNode::DefMethod { .. } | HirNode::ClassDef { .. } => true,
            HirNode::If {
                then_body,
                else_body,
                ..
            } => qualifies(compiler, then_body) && qualifies(compiler, else_body),
            _ => !branch_has_top_defs(compiler, std::slice::from_ref(&s)),
        })
    }
    fn rewrite_body(
        compiler: &mut Compiler,
        body: Vec<NodeId>,
    ) -> Result<Vec<NodeId>, AnalyzeError> {
        body.into_iter()
            .map(|s| match &compiler.hir[s] {
                HirNode::DefMethod {
                    name,
                    params,
                    body,
                    is_class_method: true,
                    ..
                } => {
                    let (name, params, body) = (name.clone(), params.clone(), body.clone());
                    let self_ref = compiler.hir.push(HirNode::SelfRef);
                    let lambda = compiler.hir.push(HirNode::Lambda {
                        params,
                        body,
                        method_body: true,
                    });
                    let sym = compiler.hir.push(HirNode::SymbolLit(name));
                    Ok(compiler.hir.push(HirNode::Call {
                        receiver: Some(self_ref),
                        name: "define_singleton_method".to_string(),
                        args: vec![ArrayElem::Single(sym), ArrayElem::Single(lambda)],
                        kwargs: vec![],
                        block: None,
                        block_arg: None,
                        safe: false,
                    }))
                }
                // A guarded top-level PLAIN `def` is a private `Object`
                // instance method exactly when its branch runs (myrrha gates
                // `def Boolean(s)` on `Myrrha.core_ext?`). Register it the way
                // a class-body conditional `def` registers -- visible to
                // reflection but not promised, the name de-optimized -- and
                // leave the node in place for its runtime emission at document
                // position inside the still-live `if`.
                HirNode::DefMethod {
                    is_class_method: false,
                    ..
                } => {
                    if let HirNode::DefMethod { visibility, .. } = &mut compiler.hir[s] {
                        *visibility = crate::hir::Visibility::Private;
                    }
                    register_body_def_method(compiler, OBJECT_CLASS, s, Conditional::Yes)?;
                    Ok(s)
                }
                // A whole `class`/`module` under the guard: REGISTER it here
                // -- a fresh one as `ClassInfo::runtime_conditional`, a
                // reopening as-is, every `def` in either `Conditional::Yes`
                // -- and leave the node in place, so the body runs at its
                // document position inside the still-live `if`.
                HirNode::ClassDef {
                    name,
                    superclass,
                    body,
                    is_module,
                } => {
                    let (name, superclass, body, is_module) =
                        (name.clone(), superclass.clone(), body.clone(), *is_module);
                    register_class_or_raise(
                        compiler,
                        &ClassRegistration {
                            name: &name,
                            superclass: &superclass,
                            is_module,
                            body: &body,
                            cref: &[],
                            box_id: 0,
                            def_node: Some(s),
                            conditional: Conditional::Yes,
                        },
                    )?;
                    Ok(s)
                }
                HirNode::If { .. } => {
                    rewrite_if(compiler, s)?;
                    Ok(s)
                }
                // An interleaved plain statement stays as it is, but a
                // `class`/`module` REACHABLE from it still has to register --
                // core_ex wraps `module Inflector` in a `silence_warnings do`
                // block, inside a `unless defined? CORE_EX_LOADED` whose
                // condition nothing decides. The top level already registers
                // through blocks this way; only the guarded branch did not,
                // and the definition reached codegen unregistered.
                _ => {
                    register_nested_class_defs_as(compiler, s, &[], 0, Conditional::Yes)?;
                    Ok(s)
                }
            })
            .collect()
    }
    fn rewrite_if(compiler: &mut Compiler, stmt: NodeId) -> Result<(), AnalyzeError> {
        let HirNode::If {
            cond,
            then_body,
            else_body,
        } = &compiler.hir[stmt]
        else {
            return Ok(());
        };
        let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
        let then_body = rewrite_body(compiler, then_body)?;
        let else_body = rewrite_body(compiler, else_body)?;
        compiler.hir[stmt] = HirNode::If {
            cond,
            then_body,
            else_body,
        };
        Ok(())
    }
    let HirNode::If {
        then_body,
        else_body,
        ..
    } = &compiler.hir[stmt]
    else {
        return Ok(false);
    };
    if !qualifies(compiler, then_body) || !qualifies(compiler, else_body) {
        return Ok(false);
    }
    rewrite_if(compiler, stmt)?;
    Ok(true)
}

/// Walk down a chain of ONE-SIDED `if`s to the statements at the bottom,
/// recording each guard and which way it has to go, outermost first.
///
/// One-sided because a guard with BOTH branches populated is a real either/or,
/// not a reopening. Nested because ruby lets a definition carry more than one
/// trailing modifier: sexp_processor closes its whole `Sexp` reopen with
/// `end unless Sexp.new.respond_to? :safe_asgn if ENV["STRICT_SEXP"]`, which is
/// two guards around one `class`, and the pair pushes into the class body
/// exactly as a single one does.
fn peel_one_sided_guards(
    compiler: &Compiler,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
    guards: &mut Vec<(NodeId, bool)>,
) -> Option<Vec<NodeId>> {
    let (body, on_then) = match (then_body, else_body) {
        (body, []) if !body.is_empty() => (body, true),
        ([], body) if !body.is_empty() => (body, false),
        _ => return None,
    };
    guards.push((cond, on_then));
    // Only a LONE nested `if` descends. An `if` beside other statements is not
    // one definition under two guards, and rewriting it would have to say which
    // statements each guard covers.
    if let [only] = body
        && let HirNode::If {
            cond,
            then_body,
            else_body,
        } = &compiler.hir[*only]
    {
        return peel_one_sided_guards(compiler, *cond, then_body, else_body, guards);
    }
    Some(body.to_vec())
}

/// A short, human-readable label for a guard expression -- what shape of
/// condition gated a top-level definition. Only for `tracing` output when
/// debugging why `static_top_cond` couldn't decide a guard (turn it on with
/// `ZEO_LOG=zeo::analyze=debug`); names the idiom (e.g. `defined?(Foo::BAR)`,
/// `RUBY_VERSION.<cmp>`, `local(x)`) so a new require-graph blocker is legible.
pub(super) fn cond_kind(compiler: &Compiler, id: NodeId) -> String {
    match &compiler.hir[id] {
        HirNode::Defined(inner) => match &compiler.hir[*inner] {
            HirNode::ClassRef(n) => format!("defined?({n})"),
            HirNode::QualifiedConstRead(s, n) => format!("defined?({s}::{n})"),
            HirNode::Call { name, .. } => format!("defined?(.{name})"),
            _ => "defined?(expr)".to_string(),
        },
        HirNode::Call { receiver, name, .. } => {
            let recv = match receiver.map(|r| &compiler.hir[r]) {
                Some(HirNode::ClassRef(n)) => n.as_str(),
                Some(_) => "expr",
                None => "self",
            };
            format!("{recv}.{name}")
        }
        HirNode::And(..) => "&&".to_string(),
        HirNode::Or(..) => "||".to_string(),
        HirNode::BoolLit(b) => format!("literal {b}"),
        HirNode::ClassRef(n) => format!("const {n}"),
        HirNode::QualifiedConstRead(s, n) => format!("{s}::{n}"),
        HirNode::IvarRead(n) => format!("ivar {n}"),
        HirNode::GlobalRead(n) => format!("global {n}"),
        HirNode::LocalRead(n) => format!("local {n}"),
        _ => "other".to_string(),
    }
}

/// Register the constants each ACTIVE native extension defines onto its builtin
/// class's compile-time `const_owners`, from the single-source name tables in
/// `zeo-abi` (the runtime `seed_*` installs the same names' values). Gated on
/// the feature actually being required, mirroring CRuby: `Socket::AF_INET6` is
/// undefined until `require "socket"`. Lets `static_top_cond`/`constfold` decide
/// the platform guards gems write (`unless Socket.const_defined? :AF_INET6`).
pub(super) fn seed_ext_const_owners(compiler: &mut Compiler) {
    if compiler.hir.activated_features.contains("socket") {
        // Both tables, the way `sock_define_const` writes both -- see
        // `ext::socket::socket::seed_socket`, the runtime half.
        for owner in [
            crate::compiler::ClassId(zeo_abi::SOCKET_CLASS.0),
            crate::compiler::ClassId(zeo_abi::SOCKET_CONSTANTS_MODULE.0),
        ] {
            for &name in zeo_abi::SOCKET_CONSTANT_NAMES {
                compiler.classes[owner.0 as usize]
                    .const_owners
                    .entry(name.to_string())
                    .or_insert(owner);
            }
        }
    }
}

/// Compile-time truth of `Recv.const_defined?(:NAME)` for a resolvable class
/// receiver and a literal symbol/string name: `Some(true)` when the constant is
/// known-defined (a nested class/module, or a value constant in the receiver's
/// own const table -- user-written or `seed_ext_const_owners`'d), `Some(false)`
/// when the guarded branch is the program's ONLY definition of that name (see
/// `const_defined_outside`). Otherwise `None` (undecidable), so a guard over a
/// genuinely-missing constant fails loudly rather than silently taking the
/// wrong branch -- surfacing a constant zeo still needs to seed rather than
/// mis-compiling.
fn static_const_defined(
    compiler: &Compiler,
    recv: NodeId,
    args: &[ArrayElem],
    guarded: &[NodeId],
) -> Option<bool> {
    let HirNode::ClassRef(recv_name) = &compiler.hir[recv] else {
        return None;
    };
    let target = compiler.resolve_class(recv_name, &[], 0)?;
    let [ArrayElem::Single(arg)] = args else {
        return None;
    };
    static_const_defined_in(compiler, target, *arg, guarded)
}

/// [`static_const_defined`] once the receiver has resolved -- shared with the
/// `Object.constants.include?(:Name)` spelling of the same question.
fn static_const_defined_in(
    compiler: &Compiler,
    target: ClassId,
    arg: NodeId,
    guarded: &[NodeId],
) -> Option<bool> {
    let arg = &arg;
    let cname = match &compiler.hir[*arg] {
        HirNode::SymbolLit(s) => s.clone(),
        // A single-literal string (`const_defined?("AF_INET6")`); interpolated
        // or multi-part strings aren't compile-time names.
        HirNode::StringLit(parts) => match parts.as_slice() {
            [StrPart::Lit(s)] => s.clone(),
            _ => return None,
        },
        _ => return None,
    };
    let nested = format!("{}::{cname}", compiler.fq_name(target));
    let nested_class = compiler.resolve_class(&nested, &[], 0);
    let bare_class = compiler.resolve_class(&cname, &[], 0);
    // A runtime-conditional class is registered but whether the constant
    // EXISTS is settled at run time -- not foldable in either direction.
    if nested_class
        .into_iter()
        .chain(bare_class)
        .any(|c| compiler.class(c).runtime_conditional)
    {
        return None;
    }
    if nested_class.is_some()
        || compiler.class(target).const_owners.contains_key(&cname)
        // `inherit` is true by default, and every ancestry ends at Object, so a
        // TOP-LEVEL constant answers too -- `Object.const_defined?("Decimal")`
        // is the whole point of the idiom, and `M.const_defined?(:String)` is
        // true for a bare module as well.
        || bare_class.is_some()
    {
        return Some(true);
    }
    (!const_defined_outside(compiler, guarded, &cname)).then_some(false)
}

/// Whether anything OUTSIDE `guarded` -- the branch bodies the guard under
/// test decides -- defines the constant `leaf`.
///
/// `defined?` asks a question about a MOMENT: has this name been bound yet? For
/// a name whose only definition site in the whole program is the branch this
/// very guard controls, the answer at the guard is no -- that branch has not
/// run. Both readings then fall out right. activefacts' `unless
/// Object.const_defined?("Money"); class Money < Decimal` TAKES its branch and
/// defines Money, exactly as ruby does on a first load. pg's `if
/// defined?(PG::CancelConnection); class PG::CancelConnection` skips its
/// branch, which is also what the compiled program wants: that branch REOPENS a
/// class the C extension would have supplied, and zeo has no C extension to
/// supply it, so there is nothing there to reopen.
///
/// Only a VALUE constant counts here. A `class`/`module` definition does not,
/// because the caller has already asked the better question: `resolve_class`
/// answers with exactly what the walk has registered SO FAR, and the walk
/// visits top-level statements in the order the program runs them. A
/// definition it has not reached is one that has not run, which is the moment
/// `defined?` asks about -- guard-compat's `unless Object.const_defined?
/// ('Guard'); module Guard; end` is written precisely so the module comes into
/// being on that first pass, and answering "already defined" because a LATER
/// file also opens `Guard` dropped the branch that creates it.
///
/// A `ConstWrite` has no such oracle: value constants are resolved in a later
/// pass, so the walk cannot say whether one has run. It keeps the conservative
/// "undecidable" answer.
///
/// Matching is by LEAF name over the whole arena, ignoring lexical scope: an
/// unrelated `Foo::MONEY` elsewhere costs only a `None` (the pre-existing
/// "undecidable" answer), whereas missing a real definition would silently drop
/// one of the two branches.
fn const_defined_outside(compiler: &Compiler, guarded: &[NodeId], leaf: &str) -> bool {
    // A merged package's write has no node in THIS arena -- the name rides
    // in from its manifest via `unrun_unit_consts`, and it always counts as
    // outside every branch here.
    compiler.hir.loader.unrun_unit_consts.contains(leaf)
        || any_site_outside(
            compiler,
            guarded,
            write_sites(&compiler.const_write_sites, leaf),
        )
}

/// The indexed sites for `name`, or an empty slice when there are none.
fn write_sites<'a>(sites: &'a FMap<String, Vec<NodeId>>, name: &str) -> &'a [NodeId] {
    sites.get(name).map_or(&[], Vec::as_slice)
}

/// Whether any of `sites` lies outside the branches `guarded` decides.
///
/// [`nodes_under`] walks the guard's whole subtree, so it runs only once
/// there is a candidate to place -- for most guards the index answers "no
/// such write anywhere in the program" and the walk never happens.
fn any_site_outside(compiler: &Compiler, guarded: &[NodeId], sites: &[NodeId]) -> bool {
    if sites.is_empty() {
        return false;
    }
    let inside = nodes_under(compiler, guarded);
    sites.iter().any(|id| !inside.contains(id))
}

/// Whether a chain still yields the TOP-LEVEL constant names, so asking it for
/// membership is asking `Object.const_defined?`.
///
/// `Object.constants` is one spelling; `Module.constants` is the other, and at
/// the top level -- the only place this runs, `static_top_cond` being the
/// top-level walk's own folder -- the two are the same list. `Module.constants`
/// is "the constants accessible from the point of call", and that point is the
/// top level, so it is Object's set exactly (oracle-verified on ruby 4.0.6).
/// Inside a class body the two would part, which is why this must not migrate
/// to `guard_fold`, where a guard carries a cref.
///
/// No other receiver answers: for a `constants` that does NOT inherit, "its own
/// constants" and "the constants visible here" are different sets, and the arm
/// declines rather than guess.
///
/// A `map` that only renames each entry passes through, because
/// `static_const_defined_in` matches by NAME and takes a symbol or a string
/// either way -- andand asks
/// `Module.constants.map { |c| c.to_s }.include?('BlankSlate')`.
fn top_level_constants_list(compiler: &Compiler, node: NodeId) -> bool {
    let HirNode::Call {
        receiver: Some(recv),
        name,
        args,
        kwargs,
        block,
        block_arg,
        ..
    } = &compiler.hir[node]
    else {
        return false;
    };
    if !kwargs.is_empty() || !args.is_empty() {
        return false;
    }
    match name.as_str() {
        "constants" => {
            block.is_none()
                && block_arg.is_none()
                && matches!(&compiler.hir[*recv], HirNode::ClassRef(n)
                    if matches!(n.trim_start_matches("::"), "Object" | "Module"))
        }
        "map" | "collect" => {
            renames_each_entry(compiler, *block, *block_arg)
                && top_level_constants_list(compiler, *recv)
        }
        _ => false,
    }
}

/// Whether a `map` block only respells each entry -- `{ |c| c.to_s }`, or the
/// `&:to_s` shorthand. A constant name reads the same as a symbol or a string,
/// so a list mapped this way holds the same names it did before.
fn renames_each_entry(
    compiler: &Compiler,
    block: Option<NodeId>,
    block_arg: Option<NodeId>,
) -> bool {
    const RESPELLINGS: &[&str] = &["to_s", "to_sym", "name"];
    if let Some(arg) = block_arg {
        return matches!(&compiler.hir[arg], HirNode::SymbolLit(s)
            if RESPELLINGS.contains(&s.as_str()));
    }
    let Some(block) = block else {
        return false;
    };
    let HirNode::Block { params, body } = &compiler.hir[block] else {
        return false;
    };
    let ([param], [stmt]) = (params.required.as_slice(), body.as_slice()) else {
        return false;
    };
    let HirNode::Call {
        receiver: Some(entry),
        name,
        args,
        block: None,
        block_arg: None,
        ..
    } = &compiler.hir[*stmt]
    else {
        return false;
    };
    RESPELLINGS.contains(&name.as_str())
        && args.is_empty()
        && matches!(&compiler.hir[*entry], HirNode::LocalRead(n) if n == param)
}

/// [`const_defined_outside`] for a GLOBAL -- whether anything outside the
/// branches this guard decides assigns `$name`. Globals have one flat
/// namespace, so the name alone is the whole question.
fn global_assigned_outside(compiler: &Compiler, guarded: &[NodeId], name: &str) -> bool {
    // A merged package's write has no node in THIS arena; its name alone
    // says "assigned somewhere outside every branch".
    compiler.external_global_writers.contains(name)
        || any_site_outside(
            compiler,
            guarded,
            write_sites(&compiler.global_write_sites, name),
        )
}

/// [`const_defined_outside`] for a SCOPED name -- `defined?(HTTP::VERSION)`.
///
/// The unscoped form matches by leaf name over the whole arena, which for
/// `VERSION`/`Error`/`Config` matches somewhere in every program and so always
/// answered "undecidable". Here the scope is already resolved, so the question
/// can be asked properly: is there a write that lands in THIS class?
///
/// Two shapes can, and only two. An explicitly scoped `Scope::NAME = v`, and a
/// `const_set` that could name the leaf on this class (see the arm below for
/// what "could" takes). A bare `NAME = v` inside the scope's own body is not
/// one of them: the caller already asked `directly_defines_const`, and a body
/// the walk has not reached yet has not run -- the same moment rule the class
/// arm applies.
fn const_written_into(
    compiler: &Compiler,
    guarded: &[NodeId],
    target: ClassId,
    leaf: &str,
) -> bool {
    // Both shapes come straight from the arena index: the scoped writes under
    // this leaf, and every `const_set` in the program. A `const_set` cannot be
    // keyed by name -- the arm below is what decides whether it could be
    // writing this one.
    let writes = write_sites(&compiler.const_write_sites, leaf);
    if writes.is_empty() && compiler.const_set_sites.is_empty() {
        return false;
    }
    let inside = nodes_under(compiler, guarded);
    writes.iter().chain(&compiler.const_set_sites).any(|&id| {
        if inside.contains(&id) {
            return false;
        }
        match &compiler.hir[id] {
            HirNode::ConstWrite {
                scope: Some(scope),
                name,
                ..
            } => name == leaf && compiler.resolve_class(scope, &[], 0) == Some(target),
            // `const_set` writes at run time, so it counts whenever it COULD
            // create `target::leaf` -- but that takes BOTH halves: the name it
            // sets could be `leaf`, AND the receiver it sets it on could be
            // `target`. Each half is `Some(_)` when the source pins it down and
            // `None` when it is computed.
            //
            // A call that pins NEITHER is evidence about no constant in
            // particular, and treating it as evidence about this one is what
            // made the whole fold unusable: webmock's
            // `@webMockNetHTTP.const_set(c[0], c[1])` -- an ivar receiver
            // nothing ties to `HTTP`, under a computed name -- declined every
            // scoped `defined?` in every program that reached it.
            HirNode::Call {
                receiver,
                name,
                args,
                ..
            } if name == "const_set" => {
                let names_leaf = match args.first() {
                    Some(ArrayElem::Single(v)) => match &compiler.hir[*v] {
                        HirNode::SymbolLit(s) => Some(s == leaf),
                        HirNode::StringLit(parts) => match parts.as_slice() {
                            [StrPart::Lit(s)] => Some(s == leaf),
                            _ => None,
                        },
                        _ => None,
                    },
                    // No argument at all, or a splat that hides the name.
                    _ => None,
                };
                // A bare `const_set` is `self`, which this arena-wide scan has
                // no lexical position to resolve -- unknown, not "not target".
                let on_target = receiver.and_then(|r| match &compiler.hir[r] {
                    HirNode::ClassRef(n) => Some(compiler.resolve_class(n, &[], 0)? == target),
                    HirNode::QualifiedConstRead(s, n) => {
                        Some(compiler.resolve_class(&format!("{s}::{n}"), &[], 0)? == target)
                    }
                    _ => None,
                });
                !matches!(
                    (names_leaf, on_target),
                    (Some(false), _) | (_, Some(false)) | (None, None)
                )
            }
            _ => false,
        }
    })
}

/// Every node reachable from `roots`, roots included.
fn nodes_under(compiler: &Compiler, roots: &[NodeId]) -> FSet<NodeId> {
    let mut seen: FSet<NodeId> = FSet::default();
    let mut stack = roots.to_vec();
    while let Some(id) = stack.pop() {
        if seen.insert(id) {
            compiler.hir[id].for_each_child(&mut |child| stack.push(child));
        }
    }
    seen
}

/// Compile-time truth of a top-level `if`'s condition. `guarded` is every
/// statement the two branches hold -- what this guard, and only this guard,
/// decides -- so an arm can tell a constant the rest of the program defines
/// from one only the guarded branch would (see `const_defined_outside`).
/// Whether `name` is defined as a `class`/`module` anywhere in the program
/// (any branch, any file) -- `shell_kinds`' whole-program sweep, matched by
/// leaf or path suffix like `guard_fold`'s `defined_const_fold`.
fn class_shaped_anywhere(compiler: &Compiler, box_id: u32, name: &str) -> bool {
    compiler.class_shaped_anywhere(box_id, name)
}

pub(super) fn static_top_cond(
    compiler: &Compiler,
    id: NodeId,
    guarded: &[NodeId],
    cref: &[ClassId],
    box_id: u32,
    // The CLASS-BODY splice folds before the walk registers this body's own
    // statements, so its "registered so far" view LAGS its siblings: a
    // `defined?` probe of a class the body defines two lines up would
    // misfold false. In that mode a "not defined" claim declines whenever
    // the name is class-shaped anywhere. The top-level walk registers
    // statement by statement -- execution order exactly -- and keeps its
    // sharper claims.
    sibling_lag: bool,
) -> Option<bool> {
    // A build-time target-constant guard folds the same way here (deciding
    // what a conditional REGISTERS) as it does at emission -- see
    // `crate::guard_fold` and `clif::stmt::static_cond`. The cref is
    // the guard's lexical position: empty at top level, the enclosing chain
    // for a class-body guard, so a bare constant resolves exactly as it
    // would at that source position.
    if let Some(b) = crate::guard_fold::static_cond(compiler, cref, box_id, id) {
        return Some(b);
    }
    match &compiler.hir[id] {
        // A literal condition: `module English end if false`, the
        // documentation-anchor idiom the English gem opens with.
        HirNode::BoolLit(b) => Some(*b),
        HirNode::NilLit => Some(false),
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block,
            ..
        } if name == "const_defined?" && kwargs.is_empty() && block.is_none() => {
            static_const_defined(compiler, *recv, args, guarded)
        }
        // `Object.constants.include?(:Concurrent)` -- the same question as
        // `const_defined?`, spelled through the list. glimmer's concurrent
        // shim asks it this way. See `top_level_constants_list` for which
        // spellings of the list answer it.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block,
            ..
        } if name == "include?" && kwargs.is_empty() && block.is_none() => {
            if !top_level_constants_list(compiler, *recv) {
                return None;
            }
            let [ArrayElem::Single(arg)] = args.as_slice() else {
                return None;
            };
            static_const_defined_in(compiler, OBJECT_CLASS, *arg, guarded)
        }
        HirNode::Defined(inner) => match &compiler.hir[*inner] {
            HirNode::ClassRef(name) => match compiler.resolve_class(name, cref, box_id) {
                // Registered but not PROMISED: whether a runtime-conditional
                // class's constant exists is settled when (if) its guarded
                // body runs -- never foldable here.
                Some(cid) if compiler.class(cid).runtime_conditional => None,
                Some(_) => Some(true),
                None if const_ever_written(compiler, name)
                    || (sibling_lag && class_shaped_anywhere(compiler, box_id, name)) =>
                {
                    None
                }
                None => Some(false),
            },
            HirNode::QualifiedConstRead(scope, name) => {
                match compiler.resolve_class(scope, cref, box_id) {
                    // A concealed SCOPE makes the whole path a runtime
                    // question.
                    Some(sid) if compiler.class(sid).runtime_conditional => None,
                    Some(sid) => {
                        // Same probe order as `constfold::class_const_in`:
                        // a class nested in `scope`, else (const lookup
                        // inherits through Object) a lexically-visible one.
                        let fq = format!("{}::{name}", compiler.fq_name(sid));
                        let nested = compiler.resolve_class(&fq, &[], 0);
                        let lexical = compiler.resolve_class(name, cref, box_id);
                        if nested
                            .into_iter()
                            .chain(lexical)
                            .any(|c| compiler.class(c).runtime_conditional)
                        {
                            None
                        } else if nested.is_some()
                            || lexical.is_some()
                            // A VALUE constant `scope` assigns in its OWN body
                            // (`module Psych; VERSION = "5.4.0"`) IS defined here,
                            // even though value constants aren't fully resolved
                            // until `resolve_consts` -- the `ConstWrite` is
                            // already in `scope`'s registered body. Lets a
                            // `defined?(Psych::VERSION)`-gated definition fold.
                            || mro::directly_defines_const(compiler, sid, name)
                        {
                            Some(true)
                        } else if const_written_into(compiler, guarded, sid, name)
                            || (sibling_lag && class_shaped_anywhere(compiler, box_id, name))
                        {
                            // Could name a VALUE constant on `scope` assigned
                            // elsewhere/at runtime -- not decidable here.
                            None
                        } else {
                            // Nothing but the guarded branch itself defines it,
                            // so it is not defined YET -- see
                            // `const_defined_outside`.
                            Some(false)
                        }
                    }
                    None if const_ever_written(compiler, scope)
                        || (sibling_lag && class_shaped_anywhere(compiler, box_id, scope)) =>
                    {
                        None
                    }
                    None => Some(false),
                }
            }
            // `defined?($gvar)` -- nil unless something assigned it, and a
            // global nothing in the program writes is never assigned.
            //
            // A write inside the guarded branch doesn't count, for the reason
            // `const_defined_outside` spells out: the branch this very guard
            // controls has not run yet. lockfile opens with
            // `unless(defined?($__lockfile__) or defined?(Lockfile))` and sets
            // `$__lockfile__` on its last line -- the whole file IS the branch,
            // so reading its own reload stamp as already-set left the guard
            // undecidable and the file uncompilable.
            HirNode::GlobalRead(name) => {
                (!global_assigned_outside(compiler, guarded, name)).then_some(false)
            }
            // `defined?` of a LITERAL is the string "expression" -- truthy, and
            // never nil. faraday-stack's `if defined?("Faraday::Env")` means
            // this rather than the constant test its author had in mind, and
            // ruby takes that branch every time.
            HirNode::StringLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::IntegerLit(_)
            | HirNode::FloatLit(_)
            | HirNode::BoolLit(_)
            | HirNode::ArrayLit(_)
            | HirNode::HashLit(_) => Some(true),
            _ => None,
        },
        // `if !defined?(X)` -- the reload guard `unless defined?(X)` written the
        // other way round, and the spelling most gems use. `guard_fold` folds
        // `!` too, but only over ITS answer for the operand, which is the
        // deliberately weaker `const_name_fold`; the arms above decide strictly
        // more, so negation has to be available on this side as well.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block,
            ..
        } if name == "!" && args.is_empty() && kwargs.is_empty() && block.is_none() => {
            static_top_cond(compiler, *recv, guarded, cref, box_id, sibling_lag).map(|b| !b)
        }
        // `if __FILE__ == $0` -- the self-test block every second script ends
        // with. In a file some other file REQUIRED, this is false no matter what
        // `$0` holds: the entry point is, by definition, a different file. Left
        // undecided in the entry file itself, where the answer depends on how
        // `$0` is modeled rather than on where the code sits.
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block,
            ..
        } if name == "==" && kwargs.is_empty() && block.is_none() => {
            let [ArrayElem::Single(arg)] = args.as_slice() else {
                return None;
            };
            not_the_entry_file(compiler, *recv, *arg)
                .or_else(|| not_the_entry_file(compiler, *arg, *recv))
                .or_else(|| literal_class_name_is(compiler, *recv, *arg))
                .or_else(|| literal_class_name_is(compiler, *arg, *recv))
        }
        HirNode::And(l, r) => {
            match static_top_cond(compiler, *l, guarded, cref, box_id, sibling_lag) {
                Some(false) => Some(false),
                Some(true) => static_top_cond(compiler, *r, guarded, cref, box_id, sibling_lag),
                None => None,
            }
        }
        HirNode::Or(l, r) => {
            match static_top_cond(compiler, *l, guarded, cref, box_id, sibling_lag) {
                Some(true) => Some(true),
                Some(false) => static_top_cond(compiler, *r, guarded, cref, box_id, sibling_lag),
                None => None,
            }
        }
        _ => None,
    }
}

/// `Some(false)` when `file` is a `__FILE__` literal (lowering resolves
/// `__FILE__` to the path as written -- see `lower`'s source-file arm) naming a
/// file OTHER than the entry file, and `prog` reads `$0`/`$PROGRAM_NAME`. Any
/// other pair is `None`: this only ever answers "these cannot be equal".
fn not_the_entry_file(compiler: &Compiler, file: NodeId, prog: NodeId) -> Option<bool> {
    let HirNode::GlobalRead(g) = &compiler.hir[prog] else {
        return None;
    };
    if g != "$0" && g != "$PROGRAM_NAME" {
        return None;
    }
    let HirNode::StringLit(parts) = &compiler.hir[file] else {
        return None;
    };
    let [StrPart::Lit(path)] = parts.as_slice() else {
        return None;
    };
    (Some(path.as_str()) != compiler.hir.main_file_name()).then_some(false)
}

/// `Some(_)` for `<literal>.class.name == "Name"` -- msgpack's
/// `if 1.class.name == "Integer"`, which picks between an `Integer` and a
/// `Fixnum` reopening and so must be decided for either branch to register. A
/// literal's class is fixed at compile time, and so is its name.
fn literal_class_name_is(compiler: &Compiler, call: NodeId, expected: NodeId) -> Option<bool> {
    let HirNode::StringLit(parts) = &compiler.hir[expected] else {
        return None;
    };
    let [StrPart::Lit(want)] = parts.as_slice() else {
        return None;
    };
    let HirNode::Call {
        receiver: Some(inner),
        name,
        args,
        ..
    } = &compiler.hir[call]
    else {
        return None;
    };
    if name != "name" || !args.is_empty() {
        return None;
    }
    let HirNode::Call {
        receiver: Some(value),
        name,
        args,
        ..
    } = &compiler.hir[*inner]
    else {
        return None;
    };
    if name != "class" || !args.is_empty() {
        return None;
    }
    let class = match &compiler.hir[*value] {
        HirNode::IntegerLit(_) => "Integer",
        HirNode::FloatLit(_) => "Float",
        HirNode::StringLit(_) => "String",
        HirNode::SymbolLit(_) => "Symbol",
        HirNode::ArrayLit(_) => "Array",
        HirNode::HashLit(_) => "Hash",
        HirNode::NilLit => "NilClass",
        HirNode::BoolLit(true) => "TrueClass",
        HirNode::BoolLit(false) => "FalseClass",
        _ => return None,
    };
    Some(class == want)
}

/// Whether ANY `ConstWrite` in the program targets `name` -- scope ignored,
/// a deliberate over-approximation used only to keep `static_top_cond`
/// honest: a name that might be a value constant somewhere can't be
/// confidently folded to "undefined".
///
/// [`Compiler::assigned_const_names`] is this exact set, already collected by
/// the one arena sweep `collect_arena_facts` makes before the guard-deciding
/// walk starts.
///
/// A PATH-shaped query is answered `false` rather than looked up. The scan
/// this replaces compared against `ConstWrite::name`, which is always a leaf
/// (an explicit `Foo::NAME = ...` keeps its namespace in the separate `scope`
/// field), so it never matched a path -- while the set also holds qualified
/// `scope::name` spellings and would. Accepting those would answer `true`
/// strictly more often, and every extra `true` here turns a guard from decided
/// into undecidable, which costs compiled gems.
fn const_ever_written(compiler: &Compiler, name: &str) -> bool {
    !name.contains("::") && compiler.assigned_const_names.contains(name)
}
