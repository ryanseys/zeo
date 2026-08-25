//! Top-level statement processing: the per-statement registration driver
//! and the statically-decidable guard/splice machinery around it (dead
//! rescues, decidable ifs, conditional reopens, guarded top defs).

use super::*;

/// One top-level statement of the program walk: intercepts the
/// definition-shaped nodes (`ClassDef`/`DefMethod`/`Include`/...) for
/// compile-time registration and pushes everything else onto
/// `main_statements` for ordinary emission. `bootstrap` marks the
/// `BUILTIN_EXCEPTIONS_RB` prefix of the program (see `analyze`'s loop);
/// recursive calls always pass `false` -- nothing nested is bootstrap.
///
/// A top-level `if` whose branch CONTAINS such definition nodes -- the
/// `if defined?(Ractor) ... module M ... end` idiom gems use to guard
/// version-dependent definitions -- is handled here too: when the
/// condition is compile-time decidable (`static_top_cond`), the taken
/// branch's statements are spliced through this same walk recursively and
/// the untaken branch is dropped, which is exactly the branch real Ruby
/// would or wouldn't execute at that point in the program. An undecidable
/// condition over such a branch is a clean compile error: the definitions
/// couldn't be registered, and codegen has no expression form for them.
///
/// This is also where a rejection gets LOCATED. Every site below raises a bare
/// message, and the wrapper stamps the statement it was handed -- so a gap
/// reported anywhere in the walk names the line of Ruby that provoked it,
/// without the site having to carry a span itself. The statement is the right
/// granularity because analyze refuses DEFINITIONS, and a definition is a
/// statement. The recursive splice calls stamp too, and the innermost frame
/// wins, so a definition inside a decidable `if` reports its own line rather
/// than the guard's.
pub(super) fn process_top_stmt(
    compiler: &mut Compiler,
    stmt: NodeId,
    bootstrap: bool,
    main_statements: &mut Vec<NodeId>,
    pre_exec: &mut Vec<NodeId>,
) -> Result<(), AnalyzeError> {
    let span = compiler.hir.span(stmt);
    process_top_stmt_inner(compiler, stmt, bootstrap, main_statements, pre_exec)
        .map_err(|e: AnalyzeError| e.with_span_if_missing(span))
}

fn process_top_stmt_inner(
    compiler: &mut Compiler,
    stmt: NodeId,
    bootstrap: bool,
    main_statements: &mut Vec<NodeId>,
    pre_exec: &mut Vec<NodeId>,
) -> Result<(), AnalyzeError> {
    // An `eval` snippet REGISTERS NOTHING (plan G6): the program whose
    // class table these rows would join is already running, so a `def`
    // here has to install at its own document position through the
    // runtime -- which is exactly what the emitter does for a `def`
    // analyze could not register, and a mixin is the ordinary send ruby
    // writes. The prelude still registers: it is the bootstrap set every
    // compile starts from, snippet or not.
    if compiler.hir.mode.is_eval()
        && !bootstrap
        && matches!(
            compiler.hir[stmt],
            HirNode::DefMethod { .. }
                | HirNode::ClassDef { .. }
                | HirNode::Include(_)
                | HirNode::Extend(_)
                | HirNode::Prepend(_)
                | HirNode::AliasMethod { .. }
                | HirNode::Undef(_)
                | HirNode::ClassMethodUndef(_)
                | HirNode::MethodVisibility { .. }
                | HirNode::ClassMethodVisibility { .. }
                | HirNode::ModuleFunction(_)
                | HirNode::ConstantVisibility { .. }
        )
    {
        main_statements.push(stmt);
        return Ok(());
    }
    // A `BEGIN { ... }` body's statements ARE top-level statements -- ruby
    // hoists them to run before the main program, in the same scope and the
    // same cref. They take the same walk, into the hoisted list instead of the
    // main one: without it a `class` written there registered nothing and
    // reached codegen as an unregistered definition (sekrets and telesign both
    // define their whole API inside one). A nested `BEGIN` keeps flowing to
    // the same list, which is where ruby runs it.
    if let HirNode::PreExec(body) = &compiler.hir[stmt] {
        if let Some(span) = compiler.hir.span(stmt) {
            compiler.pre_exec_spans.push(span);
        }
        for s in body.clone() {
            let mut nested = Vec::new();
            process_top_stmt(compiler, s, bootstrap, pre_exec, &mut nested)?;
            pre_exec.append(&mut nested);
        }
        return Ok(());
    }
    // A `Seq` at statement position is a lowering wrapper, not a construct --
    // `class << obj` at the top level desugars to a list of statements and has
    // to hand back ONE node, so it wraps them. Walk through it, or the
    // statements inside are never seen as top-level statements: a
    // `class << Base; prepend M; end` would reach neither
    // `try_prepend_call_edit` nor `register_nested_class_defs`, and the
    // compile-time ancestry edit it spells would be dropped with no diagnostic.
    // Inside a class body the same desugar splices its statements directly
    // (`lower_class_body_stmt`), which is why only this path needed it.
    if let HirNode::Seq(body) = &compiler.hir[stmt] {
        for s in body.clone() {
            process_top_stmt(compiler, s, bootstrap, main_statements, pre_exec)?;
        }
        return Ok(());
    }
    if let HirNode::ClassDef {
        name,
        superclass,
        body,
        is_module,
    } = &compiler.hir[stmt]
    {
        let name = name.clone();
        let superclass = superclass.clone();
        let body = body.clone();
        let is_module = *is_module;
        let before = compiler.classes.len();
        let scopes_before = compiler.scopes.len();
        register_class_or_raise(
            compiler,
            name,
            superclass,
            is_module,
            &body,
            &[],
            0,
            Some(stmt),
            Conditional::No,
        )?;
        // The marker STAYS in the top-level statement stream (non-bootstrap
        // only -- the prelude's bodies keep their hoisted splice): real Ruby
        // executes a class body at its document position, interleaved with
        // the surrounding top-level code, and `clif::stmt::lower_stmt`'s
        // `ClassDef` arm emits this site's body right here.
        if !bootstrap {
            main_statements.push(stmt);
        }
        // Built-in exception classes are BOOTSTRAP: the "defined before
        // any user program runs" set every `Ruby::Box` sees (see
        // `Compiler::resolve_class`'s fallback and `Hir::builtin_exceptions_len`).
        if bootstrap {
            for c in &mut compiler.classes[before..] {
                c.is_bootstrap = true;
            }
            // Every method body registered by this bootstrap `ClassDef` IS a
            // pristine `BUILTIN_EXCEPTIONS_RB` body -- installed at runtime by
            // `register_exceptions`, so codegen must not re-emit it. A later
            // USER reopen of the same class registers a FRESH scope (after
            // this point), which stays `native_default: false` and so emits.
            for s in &mut compiler.scopes[scopes_before..] {
                s.native_default = true;
            }
        }
    } else if let HirNode::BoxScope { box_id, body } = &compiler.hir[stmt] {
        // A top-level box splice: its `ClassDef`s register
        // under the BOX (real Ruby: a class defined in a box is a
        // distinct class object); everything else stays in the
        // BoxScope for ordinary emission under the box context.
        let (bx, body) = (*box_id, body.clone());
        let mut rest = Vec::new();
        for s in body {
            // A top-level `def` in a box is a private instance method of
            // the BOX's `Object`, exactly as one in main is of main's. The
            // arm below registers a `ClassDef` under the box, so the def
            // becomes the `class Object` reopen it already means -- without
            // this it fell through to the run-time install, which knows one
            // `Object` and let main call a method the box wrote.
            let s = match &mut compiler.hir[s] {
                HirNode::DefMethod {
                    is_class_method: false,
                    visibility,
                    ..
                } => {
                    *visibility = crate::hir::Visibility::Private;
                    let span = compiler.hir.span(s).unwrap_or(crate::hir::Span::SYNTH);
                    compiler.hir.push_span(span);
                    let wrapped = compiler.hir.push(HirNode::ClassDef {
                        name: "Object".to_string(),
                        superclass: None,
                        body: vec![s],
                        is_module: false,
                    });
                    compiler.hir.pop_span();
                    wrapped
                }
                _ => s,
            };
            if let HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } = &compiler.hir[s]
            {
                let (name, superclass, body, is_module) =
                    (name.clone(), superclass.clone(), body.clone(), *is_module);
                register_class_or_raise(
                    compiler,
                    name,
                    superclass,
                    is_module,
                    &body,
                    &[],
                    bx,
                    Some(s),
                    Conditional::No,
                )?;
            }
            // The `ClassDef` marker stays in the box body too (document
            // order inside the box, same as the top level).
            rest.push(s);
        }
        compiler.hir[stmt] = HirNode::BoxScope {
            box_id: bx,
            body: rest,
        };
        main_statements.push(stmt);
    } else if let HirNode::DefMethod {
        name,
        params,
        body,
        is_class_method,
        ..
    } = &compiler.hir[stmt]
    {
        // A TOP-LEVEL `def` (regular or endless): real Ruby defines it
        // as a PRIVATE instance method of `Object` -- registered here on
        // the arena's slot 0 exactly like a `class Object` reopen would,
        // so `mro::materialize` spreads it into every class (any method
        // body can call it via implicit self) and codegen emits
        // `Object`'s own copy through the builtin-reopen container
        // (`__bm_Object`), dispatched on the runtime `main` object at
        // top-level call sites.
        let (name, params, body, is_class_method) = (
            name.clone(),
            (**params).clone(),
            body.clone(),
            *is_class_method,
        );
        // A `define_method(:x) { module M; end }` body is a block, and ruby
        // accepts the `module` keyword in one -- see `collect_nested_bodies`.
        if compiler
            .hir
            .has_flag(stmt, crate::hir::NodeFlag::BLOCK_BODIED_DEF)
        {
            register_nested_class_defs(compiler, stmt, &[], 0)?;
        }
        if is_class_method {
            // A TOP-LEVEL `def self.name` is a SINGLETON method on the
            // `main` object -- CRuby's asymmetry with `def name` above (a
            // private `Object` instance method callable via implicit self
            // anywhere): `def self.name` is callable only where `self` is
            // `main`, i.e. at the top level itself, NOT from inside another
            // object's method. Desugar to the same runtime install a
            // `def obj.name` uses, with `self` (which is `main` here) as the
            // receiver, so a block-taking body threads its block too (the
            // `method_body` lambda -- Batch G).
            let self_ref = compiler.hir.push(HirNode::SelfRef);
            let lambda = compiler.hir.push(HirNode::Lambda {
                params: Box::new(params),
                body,
                method_body: true,
            });
            let sym = compiler.hir.push(HirNode::SymbolLit(name));
            let call = compiler.hir.push(HirNode::Call {
                receiver: Some(self_ref),
                name: "define_singleton_method".to_string(),
                args: vec![ArrayElem::Single(sym), ArrayElem::Single(lambda)],
                kwargs: vec![],
                block: None,
                block_arg: None,
                safe: false,
            });
            main_statements.push(call);
        } else {
            // Ruby announces it as `Object.method_added(:foo)`, at this
            // position. See `Compiler::top_level_defs`.
            let seq = next_def_seq(compiler);
            compiler.top_level_defs.push(crate::compiler::SiteDef {
                seq,
                at: main_statements.len(),
                // A unit-walk def's real stream index is assigned when the
                // unit SURVIVES (`analyze_impl`'s unit loop); the sentinel
                // only marks it as not-main until then.
                unit: compiler.unit_walk.then_some(u32::MAX),
                node: stmt,
                name: name.clone(),
                event: crate::compiler::DefEvent::Added,
                singleton: false,
            });
            let sid = register_method(
                compiler,
                OBJECT_CLASS,
                OBJECT_CLASS,
                name,
                Some(stmt),
                params,
                body,
                crate::hir::Visibility::Private,
            )?;
            add_own_method(compiler, OBJECT_CLASS, sid, false);
        }
    } else if let HirNode::Include(m) = &compiler.hir[stmt] {
        // A TOP-LEVEL `include M` mixes M into `Object` (real Ruby: the
        // main object's class is Object, so `include` there adds M to
        // every object's ancestry) -- registered here exactly like a
        // `class Object; include M; end` reopen, so `mro::materialize`
        // spreads M's instance methods (and constants) program-wide and a
        // bare `M`-method call resolves through implicit self.
        let m = m.clone();
        match resolve_module_target(compiler, &m, &[], 0) {
            MixinTarget::Static(target) => {
                let ci = &mut compiler.classes[OBJECT_CLASS.0 as usize];
                ci.mixin_order.push((target, false));
            }
            // Registration-only otherwise, so the deferred read has to be added
            // to the statements codegen emits or it would never run.
            MixinTarget::DeferredRead => {
                defer_unresolved_directive(compiler, stmt, &m);
                main_statements.push(stmt);
            }
            // Top-level `include M` is `Object.include(M)` -- ruby puts a
            // private `include` on the main object that forwards there, and
            // zeo's main has no such row, so the receiver is spelled out.
            MixinTarget::Runtime => {
                compiler.runtime_patches_any_name = true;
                let object = compiler.hir.push(HirNode::ClassRef("Object".to_string()));
                let module = compiler.hir.push(HirNode::ClassRef(m));
                compiler.hir[stmt] = HirNode::Call {
                    receiver: Some(object),
                    name: "include".to_string(),
                    args: vec![ArrayElem::Single(module)],
                    kwargs: Vec::new(),
                    block: None,
                    block_arg: None,
                    safe: false,
                };
                main_statements.push(stmt);
            }
        }
    } else if let HirNode::Using(m) = &compiler.hir[stmt] {
        // A TOP-LEVEL `using M` covers the rest of the FILE -- which is
        // `u32::MAX` here, since the range is only ever compared against
        // spans from that same file.
        let m = m.clone();
        record_activation(compiler, stmt, &m, &[], 0, u32::MAX);
        // In a SNIPPET the module is a run-time constant and its
        // refinements live in the running program's registry, so the
        // activation above resolves nothing: the site is recorded by span
        // and RUNS, filling a slot the covered call sites read.
        if compiler.hir.mode.is_eval()
            && let Some(span) = compiler.hir.span(stmt).and_then(|s| s.known())
        {
            compiler
                .eval_activations
                .push(crate::compiler::EvalActivation {
                    marker: stmt,
                    file: span.file,
                    start: span.start,
                    end: u32::MAX,
                });
            main_statements.push(stmt);
        }
    } else if let HirNode::Undef(names) = &compiler.hir[stmt] {
        // Top-level `undef m` -- Object's reopen, exactly like the
        // `include` above and like the class-body arm in `walk_class_body`.
        let names = names.clone();
        compiler.classes[OBJECT_CLASS.0 as usize]
            .undefined
            .extend(names);
    } else if let HirNode::AliasMethod {
        new_name,
        old_name,
        is_class_method,
    } = &compiler.hir[stmt]
    {
        let entry = (new_name.clone(), old_name.clone(), *is_class_method);
        let seq = next_def_seq(compiler);
        compiler.classes[OBJECT_CLASS.0 as usize]
            .pending_aliases
            .push((entry.0, entry.1, entry.2, seq));
    } else if let HirNode::If {
        cond,
        then_body,
        else_body,
    } = &compiler.hir[stmt]
    {
        // Only intercept an `if` when a branch holds definition nodes the
        // walk must register; a plain top-level `if` stays a normal
        // statement (codegen's `constfold::static_cond` already folds
        // decidable conditions at emission time).
        if !branch_has_top_defs(compiler, then_body) && !branch_has_top_defs(compiler, else_body) {
            register_nested_class_defs(compiler, stmt, &[], 0)?;
            main_statements.push(stmt);
            return Ok(());
        }
        let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
        // Everything this guard -- and only this guard -- decides, for the arms
        // that must tell a constant the guarded branch itself would define from
        // one the rest of the program defines.
        let guarded: Vec<NodeId> = then_body.iter().chain(&else_body).copied().collect();
        let taken = match static_top_cond(compiler, cond, &guarded, &[], 0, false) {
            Some(true) => {
                tracing::debug!(
                    guard = cond_kind(compiler, cond),
                    "top-level conditional def: guard TRUE, taking then-branch"
                );
                then_body
            }
            Some(false) => {
                tracing::debug!(
                    guard = cond_kind(compiler, cond),
                    "top-level conditional def: guard FALSE, taking else-branch"
                );
                else_body
            }
            None => {
                // A conditional REOPENING of an already-defined class
                // (`class Set ... end if set_pp`) doesn't need a decidable
                // guard: push the guard INTO the class body and let the ordinary
                // class-body walk register its methods conditionally.
                if let Some(rewritten) =
                    try_conditional_reopen(compiler, cond, &then_body, &else_body)
                {
                    tracing::debug!(
                        guard = cond_kind(compiler, cond),
                        defs = rewritten.len(),
                        "top-level conditional def: undecidable guard over class REOPENINGS -- pushing the guard into each class body"
                    );
                    for def in rewritten {
                        process_top_stmt(compiler, def, false, main_statements, pre_exec)?;
                    }
                    return Ok(());
                }
                // Everything else that can honor the guard at RUNTIME does:
                // a `def self.x` rewrites to `define_singleton_method` on
                // `main` (rack gates two variants of
                // `def self.separate_testing` on `ENV['SEPARATE']`), and a
                // whole `class`/`module` registers as runtime-conditional --
                // concealed until its body runs at document position inside
                // this very `if` (concurrent-ruby's platform-gated executor
                // classes). Both branches, plain statements included, keep
                // their written order.
                if register_guarded_top_defs(compiler, stmt)? {
                    tracing::debug!(
                        guard = cond_kind(compiler, cond),
                        "top-level conditional def: undecidable guard -- runtime-conditional registration"
                    );
                    main_statements.push(stmt);
                    return Ok(());
                }
                tracing::debug!(
                    guard = cond_kind(compiler, cond),
                    "top-level conditional def: guard UNDECIDABLE -- compile error"
                );
                return Err(
                    "a mixin (`include`/`prepend`) inside a top-level `if` needs a \
                     compile-time-decidable condition (a `defined?` probe, a \
                     version/platform/engine gate, a feature test) -- a conditionally-edited \
                     ancestry of an EXISTING class has no answer under a compile-time MRO. A \
                     guarded NEW class/module, a guarded REOPENING, and a guarded \
                     `def`/`alias`/visibility change all compile with the guard deciding at \
                     runtime"
                        .into(),
                );
            }
        };
        for s in taken {
            process_top_stmt(compiler, s, false, main_statements, pre_exec)?;
        }
    } else if let Some(live) = dead_rescue_live_body(compiler, stmt) {
        // A top-level `begin; require "x"; rescue LoadError; <fallback def>`
        // whose body cannot raise has DEAD rescue clauses (see
        // `dead_rescue_live_body`); walk the live begin/else/ensure statements
        // and drop the rescues.
        for s in live {
            process_top_stmt(compiler, s, false, main_statements, pre_exec)?;
        }
    } else if try_prepend_call_edit(compiler, stmt, &[], 0) {
        // A reachable `C.prepend(M)` -- recorded as a compile-time ancestry edit
        // (see `try_prepend_call_edit`); the call emits nothing, exactly as a
        // class-body `prepend M` produces no runtime statement.
    } else {
        // A singleton prepend on a statically-registered class runs as the
        // ordinary send it is, AFTER de-optimizing the call sites its
        // modules can override -- see `defer_singleton_prepend`.
        if declines_a_singleton_prepend(compiler, stmt, &[], 0) {
            defer_singleton_prepend(compiler, stmt, &[], 0);
        }
        register_nested_class_defs(compiler, stmt, &[], 0)?;
        main_statements.push(stmt);
    }
    Ok(())
}

/// Registers every `class`/`module` reachable from a statement the walk keeps
/// whole -- a `begin` clause, a block body, a loop body, a `case` arm.
/// Registration is a compile-time fact about shape, so the marker stays put and
/// `clif::stmt::lower_stmt`'s `ClassDef` arm still runs the body at its
/// document position; the `BoxScope` arm above splits it the same way.
/// Records that this refusal is one ruby has an EXCEPTION for, so a `rescue`
/// around the definition can be given the raise instead of the compile error.
/// See [`Compiler::pending_ruby_raise`] and [`raise_instead_of_defining`].
///
/// A definition with no node behind it (a synthesized/bootstrap registration)
/// records nothing: there is no site to rewrite, and nothing wraps it.
/// CRuby's SECOND line for a kind mismatch: `\n<file>:<line>: previous
/// definition of <name> was here`.
///
/// `unmatched_redefinition` (vm_insnhelper.c) reads
/// `rb_const_source_location_at`, so the position is the one stamped when the
/// CONSTANT was created -- the first declaring site, the same rule
/// `zeo_rt`'s `record_const_location` follows, which is why a reopen
/// leaves the first declaration's line standing.
///
/// A constant with NO recorded location is still reported, with both fields
/// empty: `module String` raises `String is not a module\n:: previous
/// definition of String was here` (oracle-verified). Ruby only drops the line
/// when the location is nil outright, which a defined constant never is.
pub(super) fn previous_definition_of(
    compiler: &Compiler,
    cid: crate::compiler::ClassId,
    name: &str,
) -> String {
    let (file, line) = compiler
        .class_body_sites
        .iter()
        .find(|s| s.class == cid)
        .and_then(|s| s.def_node)
        .and_then(|n| crate::analyze::source::source_location(compiler, n))
        .map_or((String::new(), String::new()), |(f, l)| {
            (f.to_string(), l.to_string())
        });
    format!("\n{file}:{line}: previous definition of {name} was here")
}

pub(super) fn ruby_raises(
    compiler: &mut Compiler,
    def_node: Option<NodeId>,
    class: &'static str,
    msg: &str,
) {
    if let Some(node) = def_node {
        compiler.pending_ruby_raise = Some((node, class, msg.to_string()));
    }
}

/// The three ways a reopen can disagree with a class's recorded parent. Ruby
/// says the same sentence for all of them, and says it as a `TypeError`, so
/// the runtime raise and the compile diagnostic are one string.
pub(super) fn superclass_mismatch(
    compiler: &mut Compiler,
    def_node: Option<NodeId>,
    name: &str,
) -> String {
    let msg = format!("superclass mismatch for class {name}");
    ruby_raises(compiler, def_node, "TypeError", &msg);
    msg
}

/// [`register_class`], with a refusal ruby has an exception for turned into
/// that exception. Every registration site goes through here.
///
/// The rewrite used to be offered only where a `rescue` was lexically visible
/// inside the definition's own subtree, on the reasoning that an uncatchable
/// raise aborts and a compile error naming the same problem is the better
/// version of aborting. Both halves of that were wrong. The scan could not see
/// the `rescue TypeError` a CALLER wraps the `require` in, which catches these
/// perfectly well; and four of the five registration sites never consulted it,
/// so a top-level `class Foo` after `module Foo` -- the whole of the `hola_*`
/// gem family -- was a compile error rather than the `TypeError: Foo is not a
/// class` ruby raises with that exact string. Compiling the program and
/// aborting where ruby aborts is the same observable behaviour and one fewer
/// way to be wrong.
#[allow(clippy::too_many_arguments)] // one wrapper, one signature: `register_class`'s
pub(super) fn register_class_or_raise(
    compiler: &mut Compiler,
    name: String,
    superclass: Option<String>,
    is_module: bool,
    body: &[NodeId],
    cref: &[ClassId],
    box_id: u32,
    def_node: Option<NodeId>,
    conditional: Conditional,
) -> Result<(), String> {
    let registered = register_class(
        compiler,
        name,
        superclass,
        is_module,
        body,
        cref,
        box_id,
        def_node,
        conditional,
    );
    if let Err(e) = registered
        && !raise_instead_of_defining(compiler)
    {
        compiler.pending_ruby_raise = None;
        return Err(e);
    }
    Ok(())
}

/// Turns a definition ruby would have RAISED on into the raise itself, in
/// place, so an enclosing `rescue` sees exactly what it sees in ruby.
///
/// `class A; class B < A; class A < B` is `TypeError: superclass mismatch for
/// class A` -- a real exception at the definition, not a broken program. Wrapped
/// in `begin ... rescue TypeError`, ruby prints the message and carries on, so
/// zeo has to compile that program and carry on too. The class stays
/// unregistered, which is also ruby's outcome: a definition that raised changed
/// nothing.
fn raise_instead_of_defining(compiler: &mut Compiler) -> bool {
    let Some((node, class, message)) = compiler.pending_ruby_raise.take() else {
        return false;
    };
    let class_ref = compiler.hir.push(HirNode::ClassRef(class.to_string()));
    let message = compiler
        .hir
        .push(HirNode::StringLit(vec![StrPart::Lit(message)]));
    compiler.hir[node] = HirNode::Call {
        receiver: None,
        name: "raise".to_string(),
        args: vec![ArrayElem::Single(class_ref), ArrayElem::Single(message)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    };
    true
}

pub(super) fn register_nested_class_defs(
    compiler: &mut Compiler,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) -> Result<(), String> {
    register_nested_class_defs_as(compiler, stmt, cref, box_id, Conditional::No)
}

/// [`register_nested_class_defs`] with the conditionality stated. A statement
/// interleaved in an UNDECIDED guard's branch may not run at all, so a class
/// its blocks define is registered-but-not-promised -- the same standing a
/// `class` written directly in that branch already gets.
fn register_nested_class_defs_as(
    compiler: &mut Compiler,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
    conditional: Conditional,
) -> Result<(), String> {
    register_nested_class_defs_in(
        compiler,
        &nested_stmts(&compiler.hir, stmt),
        cref,
        box_id,
        conditional,
    )
}

/// [`register_nested_class_defs_as`] over an already-collected nesting, so a
/// caller that also wants the refinements beside those classes walks once.
pub(super) fn register_nested_class_defs_in(
    compiler: &mut Compiler,
    nested: &[(NodeId, Reach)],
    cref: &[ClassId],
    box_id: u32,
    conditional: Conditional,
) -> Result<(), String> {
    for &(s, _) in nested {
        let HirNode::ClassDef {
            name,
            superclass,
            body,
            is_module,
        } = &compiler.hir[s]
        else {
            continue;
        };
        let (name, superclass, body, is_module) =
            (name.clone(), superclass.clone(), body.clone(), *is_module);
        register_class_or_raise(
            compiler,
            name,
            superclass,
            is_module,
            &body,
            cref,
            box_id,
            Some(s),
            conditional,
        )?;
    }
    Ok(())
}

/// Records one `refine Target do ... end` against the module that wrote it.
/// The holder module is already registered (the `ClassDef` this marker
/// follows) and owns the methods; all that is left is what they refine.
///
/// A target or holder that resolves nowhere registers nothing, and the marker
/// then stays unregistered -- which is what makes
/// [`Compiler::refinement_marker_registered`] a real test rather than a
/// formality.
pub(super) fn register_refinement(
    compiler: &mut Compiler,
    class_id: ClassId,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) {
    let HirNode::Refine {
        target,
        holder,
        singleton,
    } = &compiler.hir[stmt]
    else {
        return;
    };
    let (target, holder, singleton) = (target.clone(), holder.clone(), *singleton);
    let target = compiler.resolve_class(&target, cref, box_id);
    let holder = compiler.class_in_scope(Some(class_id), &holder, box_id);
    let (Some(target), Some(holder)) = (target, holder) else {
        return;
    };
    compiler.refinements.push(crate::compiler::Refinement {
        module: class_id,
        target,
        holder,
        singleton,
        marker: stmt,
    });
    // A refinement is active inside its OWN block, so an explicit-receiver
    // call there sees the module's whole set. (A bare name reaches the same
    // set through `Compiler::refinements_beside`, which needs no span.)
    if let Some(span) = compiler.hir.span(stmt).and_then(|s| s.known()) {
        compiler.activations.push(crate::compiler::Activation {
            module: class_id,
            file: span.file,
            start: span.start,
            end: span.end,
        });
    }
}

/// The `refine` markers nested inside a class-body statement the walk keeps
/// whole -- power_assert writes its whole refinement set inside a
/// `module PowerAssert` reopened under a runtime `if`. Which class a
/// refinement refines is a compile-time fact about shape, exactly like the
/// nested `class`/`module` registered beside it, so it is recorded here even
/// though the branch may never run. Call this AFTER
/// [`register_nested_class_defs`]: the holder module has to exist first.
pub(super) fn register_nested_refinements(
    compiler: &mut Compiler,
    class_id: ClassId,
    nested: &[(NodeId, Reach)],
    cref: &[ClassId],
    box_id: u32,
) {
    for &(s, _) in nested {
        if matches!(compiler.hir[s], HirNode::Refine { .. }) {
            register_refinement(compiler, class_id, s, cref, box_id);
        }
    }
}

/// How the nested walk arrived at a statement. Every consumer that wants only
/// part of the nesting expresses the restriction here rather than by omitting
/// node kinds from a walk of its own -- omitting kinds is what let a `class`
/// in an unusual position reach codegen unregistered three times over.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Reach {
    /// Through a branch that may not be taken: an `if`/`unless` arm, a
    /// `case`/`when` arm, a `rescue` clause.
    pub(super) conditional: bool,
    /// Through a body that runs later, never, or many times: a block, a
    /// lambda, a loop.
    pub(super) through_block: bool,
}

impl Reach {
    /// Straight-line: the statement runs exactly when its enclosing body does.
    pub(super) const DIRECT: Reach = Reach {
        conditional: false,
        through_block: false,
    };

    pub(super) fn conditional(self) -> Reach {
        Reach {
            conditional: true,
            ..self
        }
    }

    pub(super) fn through_block(self) -> Reach {
        Reach {
            through_block: true,
            ..self
        }
    }
}

/// Every statement nested inside `node`'s sub-bodies, in document order, each
/// with the [`Reach`] that got there. Built on
/// [`HirNode::for_each_child`], with the two scope stops named explicitly, so
/// a new statement-bearing variant descends by default instead of being
/// silently skipped.
fn for_each_nested_stmt(
    hir: &Hir,
    node: NodeId,
    reach: Reach,
    visit: &mut impl FnMut(NodeId, Reach),
) {
    // Document order throughout: `register_nested_class_defs_as` registers as
    // it goes and the last write wins, so a reordering here would change which
    // of two same-named nested classes stands.
    let children: Vec<(NodeId, Reach)> = match &hir[node] {
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => body
            .iter()
            .map(|&s| (s, reach))
            // A `rescue` clause runs only if the body raised.
            .chain(
                rescues
                    .iter()
                    .flat_map(|r| r.body.iter())
                    .map(|&s| (s, reach.conditional())),
            )
            .chain(
                else_body
                    .iter()
                    .flatten()
                    .chain(ensure_body.iter().flatten())
                    .map(|&s| (s, reach)),
            )
            .collect(),
        HirNode::Block { params: _, body } | HirNode::Loop { body } => {
            body.iter().map(|&s| (s, reach.through_block())).collect()
        }
        HirNode::While {
            cond: _,
            body,
            negate: _,
            post: _,
        } => body.iter().map(|&s| (s, reach.through_block())).collect(),
        HirNode::For {
            target: _,
            iterable: _,
            body,
        } => body.iter().map(|&s| (s, reach.through_block())).collect(),
        HirNode::If {
            cond: _,
            then_body,
            else_body,
        } => then_body
            .iter()
            .chain(else_body)
            .map(|&s| (s, reach.conditional()))
            .collect(),
        HirNode::CaseWhen {
            subject: _,
            arms,
            else_body,
        } => arms
            .iter()
            .flat_map(|(_, body)| body.iter())
            .chain(else_body)
            .map(|&s| (s, reach.conditional()))
            .collect(),
        // A definition is an EXPRESSION in ruby, so it also reaches here as a
        // value: `__skip__ = module M ... end` (elasticgraph, dodging a type
        // checker), `M = (class Inner; 7; end)`, an argument. It defines the
        // class right where it is written, exactly as the statement form does,
        // so the only difference is that something reads the body's value.
        //
        // The two stops are the nodes that register their OWN bodies or
        // reject one: `register_class` recurses into a `ClassDef`, and a
        // `DefMethod` body is a separate function that waits to be called
        // (ruby rejects a `class` written in one). A `Lambda` descends like
        // a `Block` does -- both run later, maybe never, and the class a
        // body defines is a registration fact either way (`define_method`
        // bodies and `-> { class Object; ... }` reopens both live here;
        // the marker still executes only when the lambda runs).
        // ...and a `define_method(:x) { module M; end }` is a BLOCK wearing a
        // `DefMethod`'s shape, so it descends like one: ruby accepts the
        // `module` keyword there and gives it the enclosing lexical cref.
        HirNode::DefMethod { body, .. }
            if hir.has_flag(node, crate::hir::NodeFlag::BLOCK_BODIED_DEF) =>
        {
            body.iter().map(|&s| (s, reach.through_block())).collect()
        }
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => return,
        HirNode::Lambda { body, .. } => body.iter().map(|&s| (s, reach.through_block())).collect(),
        other @ (HirNode::Program(_)
        | HirNode::IntegerLit(_)
        | HirNode::BigIntegerLit { .. }
        | HirNode::RationalLit { .. }
        | HirNode::ImaginaryLit(_)
        | HirNode::FloatLit(_)
        | HirNode::SymbolLit(_)
        | HirNode::NilLit
        | HirNode::BoolLit(_)
        | HirNode::And(..)
        | HirNode::Or(..)
        | HirNode::Defined(_)
        | HirNode::ArrayLit(_)
        | HirNode::HashLit(_)
        | HirNode::RangeLit { .. }
        | HirNode::StringLit(_)
        | HirNode::RegexpLit(..)
        | HirNode::LocalRead(_)
        | HirNode::LocalWrite(..)
        | HirNode::IvarRead(_)
        | HirNode::IvarWrite(..)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassVarWrite(..)
        | HirNode::ClassRef(_)
        | HirNode::Call { .. }
        | HirNode::New { .. }
        | HirNode::SuperCall { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_)
        | HirNode::DefHook { .. }
        | HirNode::MethodRedefine { .. }
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::Break(_)
        | HirNode::Next(_)
        | HirNode::Redo
        | HirNode::MultiWrite { .. }
        | HirNode::Ffi(_)
        | HirNode::BoxScope { .. }
        | HirNode::BoxHandle(_)
        | HirNode::Return(_)
        | HirNode::Yield(_)
        | HirNode::BlockGiven
        | HirNode::SelfRef
        | HirNode::Raise(..)
        | HirNode::CaseIn { .. }
        | HirNode::MatchPredicate { .. }
        | HirNode::MatchRequired { .. }
        | HirNode::Retry
        | HirNode::GlobalRead(_)
        | HirNode::GlobalWrite(..)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::ConstWrite { .. }
        | HirNode::DynConstRead { .. }
        | HirNode::DynConstWrite { .. }
        | HirNode::PreExec(_)
        | HirNode::AliasGlobal(..)
        | HirNode::Undef(_)
        | HirNode::FeatureLoaded { .. }
        | HirNode::CExtLoaded { .. }
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::ConstantVisibility { .. }
        | HirNode::LastMatchRef(_)
        | HirNode::Seq(_)
        | HirNode::FlipFlop { .. }) => {
            let mut children = Vec::new();
            other.for_each_child(&mut |c| children.push((c, reach)));
            children
        }
    };
    // Each child is itself a candidate and may nest further -- the
    // `File.open { begin ... rescue; module M; end; end }` shape.
    for (child, reach) in children {
        visit(child, reach);
        for_each_nested_stmt(hir, child, reach, visit);
    }
}

/// [`for_each_nested_stmt`] collected, for the callers that then need
/// `&mut Compiler` and so cannot hold the `&Hir` borrow across the visit.
pub(super) fn nested_stmts(hir: &Hir, node: NodeId) -> Vec<(NodeId, Reach)> {
    let mut out = Vec::new();
    for_each_nested_stmt(hir, node, Reach::DIRECT, &mut |id, r| out.push((id, r)));
    out
}

/// If `stmt` is a `begin/rescue` whose body provably cannot raise -- the
/// `require` a `begin; require "x"; rescue LoadError` guard lowers to when the
/// feature RESOLVED (a shim/builtin/file folded it to a bool), so the rescue is
/// DEAD, exactly CRuby's behavior when the extension IS present -- returns the
/// LIVE statements (begin + else + ensure), the rescue clauses dropped. `None`
/// for anything else, so a `begin` that can raise is kept whole. Dropping the
/// dead rescue keeps its unregistered fallback defs from reaching codegen.
fn dead_rescue_live_body(compiler: &Compiler, stmt: NodeId) -> Option<Vec<NodeId>> {
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

/// Recognizes a reachable `C.prepend(M, ...)` / `C.singleton_class.prepend(M,
/// ...)` CALL and applies it as the same compile-time ancestry edit the
/// corresponding class-body form makes:
///  - `C.prepend(M)` pushes each module onto `C`'s `prepends`, which `mro`
///    flattens BEFORE `C` (M's INSTANCE methods override C's own, `super`
///    reaching C).
///  - `C.singleton_class.prepend(M)` pushes onto `C`'s `class_method_prepends`,
///    so M's instance methods become C's CLASS methods at higher priority than
///    its own `def self.x` (`super` reaching the original -- the ForkTracker /
///    fork-hook shape).
///
/// A whole-program AOT target has a FIXED load-time reachability, so a reachable
/// `prepend` call is a static structural fact about the class ancestry, not a
/// runtime metaprogramming event. Recognizing the call form here (instead of
/// leaving it a runtime send, which zeo has no static MRO to honor) means
/// dispatch and `super` see the prepend through the ordinary flattened tables.
///
/// Precise: the receiver must resolve to a known class/module (optionally via a
/// bare `.singleton_class`) and EVERY argument must be a bare module constant
/// (no splat, block, kwargs, or `&.`). Anything else returns `false`, leaving it
/// an ordinary runtime call. Returns `true` when it recorded the edit, so the
/// caller drops the call from the emitted stream (`prepend` returns its
/// receiver, virtually never used at these statement-position load-time sites).
pub(super) fn try_prepend_call_edit(
    compiler: &mut Compiler,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) -> bool {
    // Resolve the receiver class and every module argument while `compiler` is
    // only shared-borrowed (reading the HIR + the class registry), then apply
    // the ancestry edit under a fresh mutable borrow.
    let (target, on_singleton, modules) = {
        let HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block,
            block_arg,
            safe,
        } = &compiler.hir[stmt]
        else {
            return false;
        };
        if name != "prepend"
            || !kwargs.is_empty()
            || block.is_some()
            || block_arg.is_some()
            || *safe
        {
            return false;
        }
        // The receiver is either a class constant (instance-side prepend) or a
        // `singleton_class` call (class-method-side prepend), whose own
        // receiver may be a constant or implicit -- a bare `singleton_class`
        // in a class body IS that class. activesupport writes
        // `singleton_class.prepend ActiveSupport::CoreExt::ERBUtil` inside
        // `module ERB::Util`, and pundit and every gem behind activesupport
        // reach the ledger through it.
        let (target, on_singleton) = match &compiler.hir[*recv] {
            HirNode::Call {
                receiver: inner,
                name,
                args,
                kwargs,
                block,
                block_arg,
                safe,
            } if name == "singleton_class"
                && args.is_empty()
                && kwargs.is_empty()
                && block.is_none()
                && block_arg.is_none()
                && !*safe =>
            {
                let resolved = match inner {
                    Some(inner) => const_node_class(compiler, *inner, cref, box_id),
                    None => cref.last().copied(),
                };
                match resolved {
                    Some(c) => (c, true),
                    None => return false,
                }
            }
            _ => match const_node_class(compiler, *recv, cref, box_id) {
                Some(c) => (c, false),
                None => return false,
            },
        };
        let mut modules = Vec::with_capacity(args.len());
        for a in args {
            let ArrayElem::Single(node) = a else {
                return false;
            };
            match const_node_class(compiler, *node, cref, box_id) {
                Some(cid) => modules.push(cid),
                None => return false,
            }
        }
        if modules.is_empty() {
            return false;
        }
        (target, on_singleton, modules)
    };
    // The INSTANCE side runs as the ordinary send it is, so the ancestry
    // changes where the call is WRITTEN -- a compile-time edit applied the
    // prepend from the program's first line, and `K.ancestors` read above
    // the call already showed it. What the edit bought was reach: a
    // statically dispatched `K#m` never consults the runtime tables the
    // send writes, so the prepend would override nothing. That is exactly
    // what `defer_mixin_to_runtime` pays for, and the singleton side has
    // paid it since `defer_singleton_prepend`.
    if !on_singleton {
        for &m in &modules {
            super::classes::defer_mixin_to_runtime(compiler, m);
        }
        return false;
    }
    // The SINGLETON side takes the same route, and for the same two reasons.
    // A compile-time edit emits no statement, so it can call no `prepended`
    // hook -- and it teaches the COMPILER's ancestry without teaching the
    // RUNTIME's, so `K.singleton_class.ancestors` never showed the module at
    // all. Both are what `defer_singleton_prepend` (in the caller) already
    // pays for on the shapes it declines; paying it here too makes one
    // spelling behave one way.
    //
    // `class << self; prepend M; end` is NOT this shape and keeps the edit:
    // it is written inside the body it edits, where there is no send to run.
    let _ = (target, modules);
    false
}

/// The class/module a bare-constant expression names (resolved in `cref`), or
/// A `<expr>.singleton_class.prepend(...)` that [`try_prepend_call_edit`] just
/// DECLINED, where letting it run as an ordinary send WITHOUT bookkeeping
/// would come out wrong -- because the receiver names a STATICALLY-REGISTERED
/// class. Statically resolved `X.m` call sites on such a class never consult
/// the runtime singleton tables the send writes
/// (`prepend_into_class_singleton`), so the prepend would compile and then
/// override nothing -- and the whole point of `prepend` is to override a
/// method that already exists. The caller answers a hit with
/// [`defer_singleton_prepend`], which de-optimizes those call sites and lets
/// the send run.
///
/// A constant-shaped receiver that does NOT resolve to a static class needs
/// none of that: the class only ever exists at runtime (an autoloaded rails
/// class behind a computed feature name, a `K = Class.new` minting), so every
/// call site on it is already dynamic and the runtime machinery is exactly
/// what CRuby does. Those pass through as the ordinary send they are -- the
/// `SomeRailsClass.singleton_class.prepend(TheirPatch)` shape behind most of
/// the rails-plugin band, activerecord-jdbc-adapter and friends.
pub(super) fn declines_a_singleton_prepend(
    compiler: &Compiler,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) -> bool {
    let HirNode::Call {
        receiver: Some(recv),
        name,
        ..
    } = &compiler.hir[stmt]
    else {
        return false;
    };
    if name != "prepend" {
        return false;
    }
    let HirNode::Call {
        receiver: inner,
        name,
        args,
        ..
    } = &compiler.hir[*recv]
    else {
        return false;
    };
    if name != "singleton_class" || !args.is_empty() {
        return false;
    }
    match inner {
        // A bare `singleton_class` names the enclosing class -- statically
        // known by construction.
        None => true,
        Some(n) => match &compiler.hir[*n] {
            HirNode::ClassRef(_) | HirNode::QualifiedConstRead(..) => {
                const_node_class(compiler, *n, cref, box_id).is_some()
            }
            // An arbitrary expression's value may still be a static class
            // (whose call sites bypass runtime tables) -- keep declining.
            _ => true,
        },
    }
}

/// Accept a singleton prepend [`declines_a_singleton_prepend`] flagged, by
/// de-optimizing the call sites it can override: every own instance method of
/// each compile-time-resolvable module argument joins `runtime_patches` (the
/// [`defer_mixin_to_runtime`] currency), so statically resolved `X.m` sites
/// route through `send_value` and consult the overlay the send writes. A
/// module zeo cannot name at compile time (a variable, a splat) de-optimizes
/// EVERY name instead -- the static fast path is not worth a wrong override,
/// and the shape is rare.
pub(super) fn defer_singleton_prepend(
    compiler: &mut Compiler,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) {
    let HirNode::Call { args, .. } = &compiler.hir[stmt] else {
        return;
    };
    for arg in args.clone() {
        let resolved = match arg {
            crate::hir::ArrayElem::Single(n) => const_node_class(compiler, n, cref, box_id),
            // A splatted module list is opaque -- the arm below widens.
            crate::hir::ArrayElem::Splat(_) => None,
        };
        match resolved {
            Some(m) => defer_mixin_to_runtime(compiler, m),
            None => compiler.runtime_patches_any_name = true,
        }
    }
}

pub(super) fn const_node_class(
    compiler: &Compiler,
    node: NodeId,
    cref: &[ClassId],
    box_id: u32,
) -> Option<ClassId> {
    match &compiler.hir[node] {
        // A top-anchored `::Process` reads as an ordinary name at the root scope
        // -- resolvable against an empty cref regardless of the enclosing one.
        // As a value it lowers to `QualifiedConstRead("Object", "Process")` (a
        // top-level constant lives on `Object`); as a definition target it can
        // be `ClassRef("::Process")`.
        HirNode::ClassRef(name) => match name.strip_prefix("::") {
            Some(rooted) => compiler.resolve_class(rooted, &[], box_id),
            None => compiler.resolve_class(name, cref, box_id),
        },
        HirNode::QualifiedConstRead(scope, name) if scope == "Object" => {
            compiler.resolve_class(name, &[], box_id)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            compiler.resolve_class(&format!("{scope}::{name}"), cref, box_id)
        }
        _ => None,
    }
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
        // guarded `require` now lowers to beside its load result.
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
fn branch_has_top_defs(compiler: &Compiler, body: &[NodeId]) -> bool {
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
fn try_conditional_reopen(
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
            let (then_body, else_body) = if on_then {
                (body, Vec::new())
            } else {
                (Vec::new(), body)
            };
            body = vec![compiler.hir.push(HirNode::If {
                cond,
                then_body,
                else_body,
            })];
        }
        body
    };
    Some(
        parts
            .into_iter()
            .map(|part| match part {
                Guarded::Reopen(name, superclass, body, is_module) => {
                    let body = wrap_guards(compiler, body);
                    compiler.hir.push(HirNode::ClassDef {
                        name,
                        superclass,
                        body,
                        is_module,
                    })
                }
                Guarded::Directive(stmt) => {
                    let body = wrap_guards(compiler, vec![stmt]);
                    compiler.hir.push(HirNode::ClassDef {
                        name: "Object".to_string(),
                        superclass: None,
                        body,
                        is_module: false,
                    })
                }
            })
            .collect(),
    )
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
fn register_guarded_top_defs(compiler: &mut Compiler, stmt: NodeId) -> Result<bool, String> {
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
    fn rewrite_body(compiler: &mut Compiler, body: Vec<NodeId>) -> Result<Vec<NodeId>, String> {
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
                        name,
                        superclass,
                        is_module,
                        &body,
                        &[],
                        0,
                        Some(s),
                        Conditional::Yes,
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
    fn rewrite_if(compiler: &mut Compiler, stmt: NodeId) -> Result<(), String> {
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
fn cond_kind(compiler: &Compiler, id: NodeId) -> String {
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
    any_site_outside(
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
    any_site_outside(
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

fn static_top_cond(
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

/// Registers one `class`/`module` definition (or REOPENING) into the
/// `Compiler`, recursively descending nested `ClassDef`s. `cref` is the
/// ENCLOSING lexical chain (outermost first,
/// `Compiler::cref_of`'s order) -- empty at the top level -- used to
/// resolve the qualified-form prefix, the superclass, and
/// include/extend/prepend targets, exactly as real Ruby resolves each of
/// those in the scope ENCLOSING the definition.
///
/// `name` may be a qualified path (`"Store::Item"` -- the `class
/// Store::Item ... end` form): the prefix must already resolve (real
/// Ruby's own NameError posture), the leaf registers under it as
/// namespace parent, and the class is marked `qualified_def` so its cref
/// is just itself (see `ClassInfo::qualified_def`'s docs). A leading `::`
/// anchors the definition at the top level from any nesting depth.
/// Register everything that belongs in id-space right after the built-in
/// exceptions and before any user class, so those exceptions keep their fixed
/// `zeo-abi` id block (63..109):
///
/// 1. `Math::DomainError` -- the one exception class nested under a
///    BUILTIN module, registered programmatically (`BUILTIN_EXCEPTIONS_RB` is
///    ordinary Ruby source and `module Math` reopens are rejected). Bootstrap
///    like the other exception classes. `Math` is always present and
///    `StandardError` is already registered. It must land at id 108, immediately
///    after the last built-in exception class.
/// 2. One top-level surrogate per allocated box -- created here (not
///    before the whole pass) so a handle-only box (`box = Ruby::Box.new`) still
///    has its `BoxHandle`'s ClassId, WITHOUT stealing the exception ids. Boxes
///    thus start after `Math::DomainError`; their ids are looked up, never baked,
///    so the shift is invisible.
pub(super) fn pin_builtin_exceptions_tail(compiler: &mut Compiler) -> Result<(), String> {
    let before = compiler.classes.len();
    register_class(
        compiler,
        "Math::DomainError".to_string(),
        Some("StandardError".to_string()),
        false,
        &[],
        &[],
        0,
        None,
        Conditional::No,
    )?;
    // `SyntaxError < ScriptError` -- the eval's parse-failure
    // class. Pinned here (not in `BUILTIN_EXCEPTIONS_RB`) so it takes the id
    // immediately after `Math::DomainError`, leaving every other exception id
    // fixed; `zeo-abi::EXCEPTION_CLASSES` reserves the matching id.
    register_class(
        compiler,
        "SyntaxError".to_string(),
        Some("ScriptError".to_string()),
        false,
        &[],
        &[],
        0,
        None,
        Conditional::No,
    )?;
    // `UncaughtThrowError < ArgumentError` -- raised by `throw` with no live
    // `catch` for its tag. Pinned right after `SyntaxError` so it takes the
    // matching `zeo-abi::EXCEPTION_CLASSES` id, leaving every other fixed.
    register_class(
        compiler,
        "UncaughtThrowError".to_string(),
        Some("ArgumentError".to_string()),
        false,
        &[],
        &[],
        0,
        None,
        Conditional::No,
    )?;
    // The `Exception`-direct tail (`SystemExit`/`SignalException`/`Interrupt`):
    // uncaught by a bare `rescue`, so a program names them explicitly. Order
    // matches `zeo-abi::EXCEPTION_CLASSES` exc_id(48..50); `Interrupt`
    // follows its parent `SignalException`.
    register_class(
        compiler,
        "SystemExit".to_string(),
        Some("Exception".to_string()),
        false,
        &[],
        &[],
        0,
        None,
        Conditional::No,
    )?;
    register_class(
        compiler,
        "SignalException".to_string(),
        Some("Exception".to_string()),
        false,
        &[],
        &[],
        0,
        None,
        Conditional::No,
    )?;
    register_class(
        compiler,
        "Interrupt".to_string(),
        Some("SignalException".to_string()),
        false,
        &[],
        &[],
        0,
        None,
        Conditional::No,
    )?;
    // The remaining core `Exception`-tree classes. Each parent is already
    // registered (a builtin exception or, for the nested names, a core class).
    for (name, superclass) in [
        ("NoMemoryError", "Exception"),
        ("SecurityError", "Exception"),
        ("SystemStackError", "Exception"),
        ("NoMatchingPatternKeyError", "NoMatchingPatternError"),
        ("Regexp::TimeoutError", "RegexpError"),
        ("IO::TimeoutError", "IOError"),
        // `WeakRef::RefError` -- nests under the `WeakRef` builtin, a plain
        // `StandardError`.
        ("WeakRef::RefError", "StandardError"),
        // The `Ractor` error tree. zeo runs no ractors, so none of these is
        // ever raised; they exist so a `rescue Ractor::ClosedError` in
        // portable code resolves its constant. `ClosedError` descends from
        // `StopIteration`, not from `Ractor::Error`, which is what lets
        // `Kernel#loop` swallow it.
        ("Ractor::Error", "RuntimeError"),
        ("Ractor::ClosedError", "StopIteration"),
        ("Ractor::IsolationError", "Ractor::Error"),
        ("Ractor::MovedError", "Ractor::Error"),
        ("Ractor::RemoteError", "Ractor::Error"),
        ("Ractor::UnsafeError", "Ractor::Error"),
        // The `IO::Buffer` error tree (io_buffer.c's split).
        ("IO::Buffer::LockedError", "RuntimeError"),
        ("IO::Buffer::AllocationError", "RuntimeError"),
        ("IO::Buffer::AccessError", "RuntimeError"),
        ("IO::Buffer::InvalidatedError", "RuntimeError"),
        ("IO::Buffer::MaskError", "ArgumentError"),
        // CRuby's internal `fatal`, which a detected deadlock raises. The
        // lower-case name cannot be written in Ruby source at all -- a
        // constant must start upper-case -- so the class is reachable only
        // through `e.class`. It descends straight from `Exception`, which is
        // what makes `rescue => e` miss it and `rescue Exception` catch it.
        ("fatal", "Exception"),
    ] {
        register_class(
            compiler,
            name.to_string(),
            Some(superclass.to_string()),
            false,
            &[],
            &[],
            0,
            None,
            Conditional::No,
        )?;
    }
    // Every `Errno` class the platform names, in `zeo-abi::ERRNO_CLASSES`
    // order -- one contiguous block, so the ids follow from the table's length
    // and no name has to be restated here. The readiness classes below
    // subclass two of them, hence the block goes first.
    for row in zeo_abi::ERRNO_CLASSES {
        register_class(
            compiler,
            row.name.to_string(),
            Some("SystemCallError".to_string()),
            false,
            &[],
            &[],
            0,
            None,
            Conditional::No,
        )?;
    }
    // `IO::WaitReadable`/`WaitWritable` -- marker MODULES, so a would-block
    // errno can be rescued by protocol. Registered before the classes that
    // mix them in, and the `include` is pushed directly: `register_class`
    // reads includes out of a class BODY, and these have none.
    for name in ["IO::WaitReadable", "IO::WaitWritable"] {
        register_class(
            compiler,
            name.to_string(),
            None,
            true,
            &[],
            &[],
            0,
            None,
            Conditional::No,
        )?;
    }
    for (name, superclass, marker) in [
        (
            "IO::EAGAINWaitReadable",
            "Errno::EAGAIN",
            "IO::WaitReadable",
        ),
        (
            "IO::EAGAINWaitWritable",
            "Errno::EAGAIN",
            "IO::WaitWritable",
        ),
        (
            "IO::EINPROGRESSWaitReadable",
            "Errno::EINPROGRESS",
            "IO::WaitReadable",
        ),
        (
            "IO::EINPROGRESSWaitWritable",
            "Errno::EINPROGRESS",
            "IO::WaitWritable",
        ),
    ] {
        register_class(
            compiler,
            name.to_string(),
            Some(superclass.to_string()),
            false,
            &[],
            &[],
            0,
            None,
            Conditional::No,
        )?;
        let (Some(cls), Some(module)) = (
            compiler.resolve_class(name, &[], 0),
            compiler.resolve_class(marker, &[], 0),
        ) else {
            return Err(format!(
                "{name} or {marker} went missing right after registration"
            ));
        };
        let ci = &mut compiler.classes[cls.0 as usize];
        ci.mixin_order.push((module, false));
    }
    // The second spellings. `zeo-abi::ERRNO_ALIASES` holds the `Errno` half:
    // a name the platform gives the same value as an earlier one
    // (`EWOULDBLOCK` is `EAGAIN`, which is why `rescue Errno::EWOULDBLOCK`
    // catches an EAGAIN), or a name it does not define at all, which CRuby
    // binds to `Errno::NOERROR`. The `IO::` pair follows the same errno.
    // Registered as aliases (`builtin_overlay`), so each name resolves to the
    // class it duplicates and nothing extra reaches the runtime. These take no
    // `zeo-abi::EXCEPTION_CLASSES` id, so they come after every pinned row.
    let io_aliases = [
        ("IO::EWOULDBLOCKWaitReadable", "IO::EAGAINWaitReadable"),
        ("IO::EWOULDBLOCKWaitWritable", "IO::EAGAINWaitWritable"),
    ];
    for (alias, target) in zeo_abi::ERRNO_ALIASES
        .iter()
        .copied()
        .chain(io_aliases)
        .collect::<Vec<_>>()
    {
        let Some(target) = compiler.resolve_class(target, &[], 0) else {
            return Err(format!("{target} went missing right after registration"));
        };
        register_class(
            compiler,
            alias.to_string(),
            None,
            false,
            &[],
            &[],
            0,
            None,
            Conditional::No,
        )?;
        let Some(cls) = compiler.class_in_scope(
            compiler.class(target).lexical_parent,
            crate::constpath::ConstPath::parse(alias).base(),
            0,
        ) else {
            return Err(format!("{alias} went missing right after registration"));
        };
        compiler.classes[cls.0 as usize].builtin_overlay = Some(target);
    }
    for c in &mut compiler.classes[before..] {
        c.is_bootstrap = true;
    }
    for b in 1..=compiler.hir.boxes {
        compiler.ensure_box_surrogate(b);
    }
    Ok(())
}

/// If a top-level `CONST = <class value>` constant aliases an existing class
/// (`CONST = SomeClass`, `CONST = A::B`, or `CONST = <literal>.class`), the
/// aliased `ClassId`. Real Ruby's `class CONST; ...; end` REOPENS that class
/// (the `INTEGER_KLASS = 1.class; class INTEGER_KLASS; ...` shape),
/// rather than minting a fresh one named `CONST`.
///
/// TOP-LEVEL is load-bearing on both sides, and used not to be. Ruby binds a
/// definition's name in the immediately enclosing scope and never searches
/// outward for it, so a write in some other scope cannot be what a definition
/// reopens. Consulting this from a nested definition made
/// `module Mongoid::Criteria::Queryable::Extensions::Boolean` bind to the
/// unrelated `Mongoid::Boolean` -- a class, so `Boolean is not a module` --
/// and made optparse's nested `class ParseError < RuntimeError` bind to racc's
/// top-level `ParseError = Racc::ParseError`, whose parent is `StandardError`,
/// reported as `superclass mismatch`. 30 gems, five distinct constant names.
///
/// The caller supplies `lexical_parent` and only calls here when it is `None`;
/// this end filters the WRITES, which the `scope` field cannot do -- `scope` is
/// `None` for `NAME = ...` at any depth, recording only the explicit
/// `Foo::NAME = ...` prefix.
pub(super) fn const_alias_target(compiler: &Compiler, leaf: &str, box_id: u32) -> Option<ClassId> {
    let value = *compiler.top_level_const_aliases.get(leaf)?;
    match &compiler.hir[value] {
        HirNode::ClassRef(n) => compiler.resolve_class(n, &[], box_id),
        HirNode::QualifiedConstRead(scope, n) => {
            compiler.resolve_class(&format!("{scope}::{n}"), &[], box_id)
        }
        // `CONST = <literal>.class`
        HirNode::Call {
            receiver: Some(r),
            name,
            args,
            ..
        } if name == "class" && args.is_empty() => literal_class_id(&compiler.hir[*r]),
        _ => None,
    }
}

/// The builtin `ClassId` of a literal value (`1.class` -> Integer, etc.).
fn literal_class_id(node: &HirNode) -> Option<ClassId> {
    Some(match node {
        HirNode::IntegerLit(_) | HirNode::BigIntegerLit { .. } => zeo_abi::INTEGER_CLASS,
        HirNode::FloatLit(_) => zeo_abi::FLOAT_CLASS,
        HirNode::StringLit(_) => zeo_abi::STRING_CLASS,
        HirNode::SymbolLit(_) => zeo_abi::SYMBOL_CLASS,
        HirNode::ArrayLit(_) => zeo_abi::ARRAY_CLASS,
        HirNode::HashLit(_) => zeo_abi::HASH_CLASS,
        HirNode::NilLit => zeo_abi::NIL_CLASS,
        HirNode::BoolLit(true) => zeo_abi::TRUE_CLASS,
        HirNode::BoolLit(false) => zeo_abi::FALSE_CLASS,
        _ => return None,
    })
}

/// Every name a class-body statement could `undef_method` when it runs --
/// `undef :m` under an undecidable guard, which
/// `lower::lower_node`'s `UndefNode` arm lowers to a receiver-less send. The
/// whole subtree is scanned, so an `undef` nested several conditionals deep
/// still counts. See [`ClassInfo::runtime_undefs`].
pub(super) fn collect_runtime_undefs(compiler: &Compiler, stmt: NodeId) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![stmt];
    while let Some(id) = stack.pop() {
        // A NESTED class collects its own, through this same call on its own
        // body statements, so descending credited its undefs to the enclosing
        // class as well -- de-optimizing that name on a class that never
        // undefs it. A `def` body is NOT stopped at: `def self.setup;
        // undef_method :m; end` has `self` as the module, so its undef really
        // does target this class, and missing one emits a direct call to a
        // method that is gone.
        if id != stmt && matches!(compiler.hir[id], HirNode::ClassDef { .. }) {
            continue;
        }
        if let HirNode::Call {
            receiver: None,
            name,
            args,
            ..
        } = &compiler.hir[id]
            && name == "undef_method"
        {
            for arg in args {
                if let ArrayElem::Single(a) = arg
                    && let HirNode::SymbolLit(s) = &compiler.hir[*a]
                {
                    out.push(s.clone());
                }
            }
        }
        compiler.hir[id].for_each_child(&mut |child| stack.push(child));
    }
    out
}
