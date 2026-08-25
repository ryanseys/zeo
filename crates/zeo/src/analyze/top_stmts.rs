//! Top-level statement processing: the per-statement registration driver.
//! The statically-decidable guard/splice machinery it drives (dead rescues,
//! decidable ifs, conditional reopens, guarded top defs) lives in
//! `static_guards.rs`.

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
    // An `eval` snippet REGISTERS NOTHING: the program whose
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
            &ClassRegistration {
                name: &name,
                superclass: &superclass,
                is_module,
                body: &body,
                cref: &[],
                box_id: 0,
                def_node: Some(stmt),
                conditional: Conditional::No,
            },
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
                    &ClassRegistration {
                        name: &name,
                        superclass: &superclass,
                        is_module,
                        body: &body,
                        cref: &[],
                        box_id: bx,
                        def_node: Some(s),
                        conditional: Conditional::No,
                    },
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
pub(super) fn register_class_or_raise(
    compiler: &mut Compiler,
    reg: &ClassRegistration<'_>,
) -> Result<(), String> {
    let registered = register_class(compiler, reg);
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
pub(super) fn register_nested_class_defs_as(
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
            &ClassRegistration {
                name: &name,
                superclass: &superclass,
                is_module,
                body: &body,
                cref,
                box_id,
                def_node: Some(s),
                conditional,
            },
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

/// The `Exception`-tree tail `pin_builtin_exceptions_tail` registers, in
/// REGISTRATION ORDER -- the order IS each class's id, and
/// `zeo-abi::EXCEPTION_CLASSES` reserves the matching ids. Columns: name,
/// superclass (`None` only for the marker modules), is_module, and the
/// marker module the row mixes in right after registration. The `Errno`
/// block registers between the last pinned exception row and the first
/// marker-module row -- see the split in `pin_builtin_exceptions_tail`.
const EXCEPTION_TAIL: &[(&str, Option<&str>, bool, Option<&str>)] = &[
    // `Math::DomainError` -- bootstrap like the other exception classes.
    // `Math` is always present and `StandardError` is already registered.
    ("Math::DomainError", Some("StandardError"), false, None),
    // `SyntaxError < ScriptError` -- the eval's parse-failure
    // class. Pinned here (not in `BUILTIN_EXCEPTIONS_RB`) so it takes the id
    // immediately after `Math::DomainError`, leaving every other exception id
    // fixed.
    ("SyntaxError", Some("ScriptError"), false, None),
    // `UncaughtThrowError < ArgumentError` -- raised by `throw` with no live
    // `catch` for its tag. Pinned right after `SyntaxError` so it takes the
    // matching `zeo-abi::EXCEPTION_CLASSES` id, leaving every other fixed.
    ("UncaughtThrowError", Some("ArgumentError"), false, None),
    // The `Exception`-direct tail (`SystemExit`/`SignalException`/`Interrupt`):
    // uncaught by a bare `rescue`, so a program names them explicitly. Order
    // matches `zeo-abi::EXCEPTION_CLASSES` exc_id(48..50); `Interrupt`
    // follows its parent `SignalException`.
    ("SystemExit", Some("Exception"), false, None),
    ("SignalException", Some("Exception"), false, None),
    ("Interrupt", Some("SignalException"), false, None),
    // The remaining core `Exception`-tree classes. Each parent is already
    // registered (a builtin exception or, for the nested names, a core class).
    ("NoMemoryError", Some("Exception"), false, None),
    ("SecurityError", Some("Exception"), false, None),
    ("SystemStackError", Some("Exception"), false, None),
    (
        "NoMatchingPatternKeyError",
        Some("NoMatchingPatternError"),
        false,
        None,
    ),
    ("Regexp::TimeoutError", Some("RegexpError"), false, None),
    ("IO::TimeoutError", Some("IOError"), false, None),
    // `WeakRef::RefError` -- nests under the `WeakRef` builtin, a plain
    // `StandardError`.
    ("WeakRef::RefError", Some("StandardError"), false, None),
    // The `Ractor` error tree. zeo runs no ractors, so none of these is
    // ever raised; they exist so a `rescue Ractor::ClosedError` in
    // portable code resolves its constant. `ClosedError` descends from
    // `StopIteration`, not from `Ractor::Error`, which is what lets
    // `Kernel#loop` swallow it.
    ("Ractor::Error", Some("RuntimeError"), false, None),
    ("Ractor::ClosedError", Some("StopIteration"), false, None),
    ("Ractor::IsolationError", Some("Ractor::Error"), false, None),
    ("Ractor::MovedError", Some("Ractor::Error"), false, None),
    ("Ractor::RemoteError", Some("Ractor::Error"), false, None),
    ("Ractor::UnsafeError", Some("Ractor::Error"), false, None),
    // The `IO::Buffer` error tree (io_buffer.c's split).
    ("IO::Buffer::LockedError", Some("RuntimeError"), false, None),
    (
        "IO::Buffer::AllocationError",
        Some("RuntimeError"),
        false,
        None,
    ),
    ("IO::Buffer::AccessError", Some("RuntimeError"), false, None),
    (
        "IO::Buffer::InvalidatedError",
        Some("RuntimeError"),
        false,
        None,
    ),
    ("IO::Buffer::MaskError", Some("ArgumentError"), false, None),
    // CRuby's internal `fatal`, which a detected deadlock raises. The
    // lower-case name cannot be written in Ruby source at all -- a
    // constant must start upper-case -- so the class is reachable only
    // through `e.class`. It descends straight from `Exception`, which is
    // what makes `rescue => e` miss it and `rescue Exception` catch it.
    ("fatal", Some("Exception"), false, None),
    // `IO::WaitReadable`/`WaitWritable` -- marker MODULES, so a would-block
    // errno can be rescued by protocol. Registered before the classes that
    // mix them in.
    ("IO::WaitReadable", None, true, None),
    ("IO::WaitWritable", None, true, None),
    // The readiness classes: an `Errno` subclass wearing its marker module.
    (
        "IO::EAGAINWaitReadable",
        Some("Errno::EAGAIN"),
        false,
        Some("IO::WaitReadable"),
    ),
    (
        "IO::EAGAINWaitWritable",
        Some("Errno::EAGAIN"),
        false,
        Some("IO::WaitWritable"),
    ),
    (
        "IO::EINPROGRESSWaitReadable",
        Some("Errno::EINPROGRESS"),
        false,
        Some("IO::WaitReadable"),
    ),
    (
        "IO::EINPROGRESSWaitWritable",
        Some("Errno::EINPROGRESS"),
        false,
        Some("IO::WaitWritable"),
    ),
];

/// Registers one `EXCEPTION_TAIL` row. A `mixin` column pushes the marker
/// module directly after registration: `register_class` reads includes out
/// of a class BODY, and these rows have none.
fn register_exception_tail_row(
    compiler: &mut Compiler,
    &(name, superclass, is_module, mixin): &(&str, Option<&str>, bool, Option<&str>),
) -> Result<(), String> {
    let superclass = superclass.map(str::to_string);
    register_class(
        compiler,
        &ClassRegistration {
            name,
            superclass: &superclass,
            is_module,
            body: &[],
            cref: &[],
            box_id: 0,
            def_node: None,
            conditional: Conditional::No,
        },
    )?;
    let Some(marker) = mixin else {
        return Ok(());
    };
    let (Some(cls), Some(module)) = (
        compiler.resolve_class(name, &[], 0),
        compiler.resolve_class(marker, &[], 0),
    ) else {
        return Err(format!(
            "{name} or {marker} went missing right after registration"
        ));
    };
    compiler.classes[cls.0 as usize]
        .mixin_order
        .push((module, false));
    Ok(())
}

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
    // The `Errno` block registers between the pinned exception rows and the
    // `IO::Wait*` rows: split the table at its first marker-module row. The
    // readiness classes subclass `Errno::EAGAIN`/`EINPROGRESS`, so the block
    // goes first.
    let split = EXCEPTION_TAIL
        .iter()
        .position(|&(_, _, is_module, _)| is_module)
        .unwrap_or(EXCEPTION_TAIL.len());
    let (pinned, io_wait) = EXCEPTION_TAIL.split_at(split);
    for row in pinned {
        register_exception_tail_row(compiler, row)?;
    }
    // Every `Errno` class the platform names, in `zeo-abi::ERRNO_CLASSES`
    // order -- one contiguous block, so the ids follow from the table's length
    // and no name has to be restated here.
    let system_call_error = Some("SystemCallError".to_string());
    for row in zeo_abi::ERRNO_CLASSES {
        register_class(
            compiler,
            &ClassRegistration {
                name: row.name,
                superclass: &system_call_error,
                is_module: false,
                body: &[],
                cref: &[],
                box_id: 0,
                def_node: None,
                conditional: Conditional::No,
            },
        )?;
    }
    for row in io_wait {
        register_exception_tail_row(compiler, row)?;
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
            &ClassRegistration {
                name: alias,
                superclass: &None,
                is_module: false,
                body: &[],
                cref: &[],
                box_id: 0,
                def_node: None,
                conditional: Conditional::No,
            },
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
