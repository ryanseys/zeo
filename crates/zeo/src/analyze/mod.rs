//! The minimal analyze pass: walks the top-level `Hir::Program` statements
//! once (no fixpoint loop -- see the plan's stated scope-cut) and registers
//! every `ClassDef`/`DefMethod` into a `Compiler`, mirroring zeo's
//! `walk_scope`/`register_locals`/`resolve_parents` (a tiny slice of them).
//! Structured as a single pass function rather than the predecessor's
//! 128-iteration fixpoint loop because nothing yet needs mutual
//! recursion between inference results -- but the shape (one function that
//! walks the whole program and mutates a `Compiler`) is exactly what a real
//! fixpoint would wrap in `for iter in 0..128 { ... }` later.

pub(crate) mod def_hooks;
mod locals;
pub(crate) mod mro;
pub(crate) mod redefs;
pub(crate) mod share;

use crate::analyze_error::AnalyzeError;
use crate::compiler::{AccessorKind, AccessorShape, ClassId, Compiler, OBJECT_CLASS, Scope};
use crate::hir::{
    ArrayElem, Hir, HirNode, NodeId, Params, Pattern, PatternArm, StrPart, Visibility,
};
use crate::types::TyKind;
use std::collections::HashMap;

pub struct Analyzed {
    pub compiler: Compiler,
    /// Top-level statements that aren't class definitions -- the body of
    /// generated `fn main()`.
    pub main_statements: Vec<NodeId>,
    /// The same per-local `TyKind` tracking `Scope::local_types` does for a
    /// method body, but for `main_statements` -- there's no `Scope` for the
    /// top level to hang this off of.
    pub main_local_types: HashMap<String, TyKind>,
    /// The compiled-in load path (see `Hir::feature_units`): each unit's
    /// top-level statements under the feature name a `require` spells. Walked
    /// exactly like `main_statements` -- their classes register at startup --
    /// but emitted as a function the runtime calls on demand.
    pub feature_units: Vec<(String, String, Vec<NodeId>)>,
    /// Load-path files zeo could not lower -- see `Hir::declined_units`.
    pub declined_units: Vec<(String, String, String)>,
}

pub fn analyze(hir: Hir, root: NodeId) -> Result<Analyzed, crate::diagnostics::CompileError> {
    // The `Compiler` is built HERE, not inside the pass, so that a failure
    // still has `Hir::files` to resolve its span against -- the pass moves
    // everything else into its result, and an error is exactly the case where
    // that result never arrives.
    let mut compiler = Compiler::new(hir);
    match analyze_impl(&mut compiler, root) {
        Ok(parts) => Ok(Analyzed {
            compiler,
            main_statements: parts.main_statements,
            main_local_types: parts.main_local_types,
            feature_units: parts.feature_units,
            declined_units: parts.declined_units,
        }),
        Err(e) => Err(crate::diagnostics::CompileError::analyze_located(
            e,
            &compiler.hir.files,
        )),
    }
}

/// Everything [`Analyzed`] holds except the `Compiler`, which the caller owns
/// so that the error path can still read its source table.
struct AnalyzedParts {
    main_statements: Vec<NodeId>,
    main_local_types: HashMap<String, TyKind>,
    feature_units: Vec<(String, String, Vec<NodeId>)>,
    declined_units: Vec<(String, String, String)>,
}

/// The whole pass. Its sites raise bare messages; the statement walk locates
/// them (see `analyze_error`), and the boundary above types them.
fn analyze_impl(compiler: &mut Compiler, root: NodeId) -> Result<AnalyzedParts, AnalyzeError> {
    let HirNode::Program(statements) = &compiler.hir[root] else {
        return Err("expected a Program root".into());
    };
    let statements = statements.clone();
    let builtin_exceptions_len = compiler.hir.builtin_exceptions_len;

    // Whole-program kind map for forward-container resolution (see
    // `Compiler::shell_kinds` and `resolve_or_create_container`): a read-only
    // scan of every class/module definition, run before the ordered
    // registration walk below.
    let mut shell_kinds = HashMap::new();
    collect_shell_kinds(&compiler.hir, &statements, &[], 0, &mut shell_kinds);
    // A DEFERRED require's body is not in `statements` -- it is walked later,
    // out of this loop (see `feature_units` below) -- so scanning only the main
    // list made every class a deferred unit defines invisible to forward
    // resolution. Two units that reference each other then cannot both be
    // walked: activemodel's `type/integer.rb` opens `class Integer < Value`
    // with `type/value.rb` still unwalked, `resolve_or_create_lexical` found no
    // `ActiveModel::Type::Value` to shell, and the unit was declined -- taking
    // most of the Rails corpus with it. Each unit body is its own top-level
    // scope in box 0, exactly as `process_top_stmt` walks it.
    for unit in &compiler.hir.feature_units {
        collect_shell_kinds(&compiler.hir, &unit.body, &[], 0, &mut shell_kinds);
    }
    compiler.shell_kinds = shell_kinds;
    compiler.assigned_const_names = collect_assigned_const_names(&compiler.hir);
    let mut aliases = HashMap::new();
    collect_top_level_const_aliases(&compiler.hir, &statements, &mut aliases);
    compiler.top_level_const_aliases = aliases;
    let mut scoped_aliases = HashMap::new();
    collect_const_aliases(&compiler.hir, &statements, &[], 0, &mut scoped_aliases);
    for unit in &compiler.hir.feature_units {
        collect_const_aliases(&compiler.hir, &unit.body, &[], 0, &mut scoped_aliases);
    }
    compiler.const_aliases = scoped_aliases;
    (compiler.runtime_patches, compiler.runtime_patches_any_name) =
        collect_runtime_patches(&compiler.hir);

    // Register native-extension constants (`Socket::AF_INET6`, ...) into their
    // builtin class's compile-time const table so `const_defined?`/`defined?`/
    // const-read/guard folding sees them exactly as the runtime `seed_*` will
    // install them. Must precede the walk below, which decides class-def guards.
    seed_ext_const_owners(compiler);

    let mut main_statements = Vec::new();
    // `BEGIN { ... }` bodies, hoisted to run before ANY main statement --
    // collected in source order here and prepended below, which is the
    // order real Ruby runs several of them in (oracle-verified). See
    // `HirNode::PreExec`.
    let mut pre_exec = Vec::new();
    // Everything that must be registered right after the built-in exceptions
    // and before any user class -- `Math::DomainError` and the per-box
    // surrogates -- so the exceptions keep the FIXED id block
    // `zeo-abi` reserves for them (63..108) regardless of box or user-class
    // count. Creating the box surrogates before the analyze pass would
    // steal those ids the moment a program allocated a box. See
    // `pin_builtin_exceptions_tail`.
    let mut tail_pinned = false;
    for (idx, stmt) in statements.into_iter().enumerate() {
        if idx >= builtin_exceptions_len && !tail_pinned {
            pin_builtin_exceptions_tail(compiler)?;
            tail_pinned = true;
        }
        process_top_stmt(
            compiler,
            stmt,
            idx < builtin_exceptions_len,
            &mut main_statements,
            &mut pre_exec,
        )?;
    }
    // Every `BEGIN` body runs first, ahead of the main program -- see
    // `pre_exec`'s declaration.
    if !pre_exec.is_empty() {
        pre_exec.append(&mut main_statements);
        main_statements = pre_exec;
    }

    // A program with no user statements after the built-in exceptions never
    // tripped the in-loop pin above -- run it now.
    if !tail_pinned {
        pin_builtin_exceptions_tail(compiler)?;
    }

    // The compiled-in load path. A unit's statements walk the SAME
    // `process_top_stmt` the main file's do -- that is what registers its
    // classes, materializes its methods, and files its class-body sites -- but
    // they collect into the unit's own list, which codegen emits as a function
    // instead of inlining. A `BEGIN` block inside a lazily-loaded file has no
    // sensible meaning (there is no "before the program" left to run at), so
    // its statements simply join the unit's body in place.
    let mut declined_units = std::mem::take(&mut compiler.hir.declined_units);
    let mut feature_units = Vec::new();
    for unit in std::mem::take(&mut compiler.hir.feature_units) {
        let mut stmts = Vec::new();
        let mut unit_pre_exec = Vec::new();
        let mut failed = None;
        let sites_before = compiler.class_body_sites.len();
        for stmt in unit.body {
            if let Err(e) = process_top_stmt(compiler, stmt, false, &mut stmts, &mut unit_pre_exec)
            {
                failed = Some(e);
                break;
            }
        }
        match failed {
            None => {
                unit_pre_exec.append(&mut stmts);
                feature_units.push((unit.feature, unit.absolute, unit_pre_exec));
            }
            // Declined like a lowering failure: requiring it raises LoadError
            // naming the gap. The classes its statements BEFORE the failure
            // already registered stay registered -- inert unless the program
            // names them, which it can only do by requiring the feature it
            // just refused.
            //
            // Its definition SITES do not stay, though. A site is hoisted by
            // CLASS, not by statement stream (`hoisted_sites_for`), so a
            // leftover made codegen emit the body of a `class`/`module` whose
            // nested definition the very same failure had already stopped from
            // registering -- and the error then named that leftover instead of
            // the refusal, deep in a require graph, with no way back to the
            // cause. Dropping them is also what the declined unit MEANS: its
            // statements never run, so its class bodies never run either.
            Some(e) => {
                compiler.class_body_sites.truncate(sites_before);
                tracing::warn!(
                    feature = unit.feature,
                    reason = e.message,
                    "analyze: unit declined"
                );
                declined_units.push((unit.feature, unit.absolute, e.message));
            }
        }
    }

    // Stage C invariant: the ids the compiler just assigned the built-in
    // exceptions MUST match `zeo-abi`'s table, because `zeo-rt`'s
    // `register_exceptions` installs those classes at those ids and generated
    // code bakes them in. A drift (someone reorders `BUILTIN_EXCEPTIONS_RB`
    // without updating the table) would make `rescue`/`raise`/`is_a?` silently
    // target the wrong class -- so fail the compile loudly instead.
    for row in zeo_abi::EXCEPTION_CLASSES {
        let got = compiler.fq_name(ClassId(row.id.0));
        assert_eq!(
            got, row.name,
            "built-in exception id {} drift: compiler assigned {:?}, zeo-abi::EXCEPTION_CLASSES expects {:?} -- update one to match",
            row.id.0, got, row.name
        );
    }

    // What the statement walk found, before `mro` turns it into method tables.
    // Reported here rather than only at the end because these three numbers
    // decide everything downstream, and a compile that dies in `mro` never
    // reaches a summary. `--log-level info`.
    tracing::info!(
        classes = compiler.classes.len(),
        defs = compiler.scopes.len(),
        hir_nodes = compiler.hir.iter().count(),
        statements = main_statements.len(),
        "analyze: walk"
    );

    // Ancestor linearization + method/class-method materialization + class
    // variable ownership -- must run AFTER every `ClassDef` above has been
    // registered, since `include`/`extend`/`prepend`/`< Super` targets must
    // already exist (same "defined earlier in the file" rule `superclass`
    // resolution already enforces). See `mro`'s module docs.
    mro::materialize(compiler, &main_statements)?;

    // Which redefinition timelines are observable and must be applied at
    // their document position. Before `def_hooks::resolve`: its `at` bumps
    // are what order a redefinition's install before its own hook report.
    redefs::resolve(compiler);

    // Which compiled definitions announce themselves. Runs here because it
    // needs `class_methods` flattened over the ancestry to see the hook, and
    // before `mark_inline_iter_sites` because it feeds `runtime_patches`.
    def_hooks::resolve(compiler, &mut main_statements);

    let main_local_types = locals::infer_locals(compiler, None, 0, &main_statements);

    // With every scope's local types final (reinfer ran inside materialize)
    // and the main scope's just computed, mark the typed-receiver iterator
    // sites codegen can fuse into native loops.
    mark_inline_iter_sites(compiler, &main_statements, &main_local_types);

    Ok(AnalyzedParts {
        main_statements,
        main_local_types,
        feature_units,
        declined_units,
    })
}

/// Fills [`Compiler::inline_iter_sites`]: every block call whose receiver is
/// a LOCAL with a statically-known collection/counter type and whose block
/// has a plain positional signature. The static type only nominates the
/// site -- the emitted code still match-guards the live receiver and falls
/// back to the ordinary dynamic dispatch (an Int-typed local can hold a
/// BigInt after overflow; a shadowing block param can hold anything), so a
/// wrong nomination costs size, never correctness.
fn mark_inline_iter_sites(
    compiler: &mut Compiler,
    main_statements: &[NodeId],
    main_local_types: &HashMap<String, TyKind>,
) {
    use crate::compiler::InlineIterKind;

    fn plain_positional(params: &Params) -> bool {
        params.optional.is_empty()
            && params.rest.is_none()
            && params.post.is_empty()
            && params.keywords.is_empty()
            && params.keyword_rest.is_none()
            && params.block.is_none()
            && params.required.iter().all(|p| !p.starts_with("__destr"))
    }

    fn scan(
        compiler: &Compiler,
        id: NodeId,
        locals: &HashMap<String, TyKind>,
        out: &mut HashMap<NodeId, InlineIterKind>,
    ) {
        if let HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block: Some(block),
            block_arg: None,
            ..
        } = &compiler.hir[id]
            && kwargs.is_empty()
            && let (HirNode::LocalRead(rn), HirNode::Block { params, .. }) =
                (&compiler.hir[*recv], &compiler.hir[*block])
            && plain_positional(params)
        {
            use InlineIterKind as K;
            let key = (locals.get(rn), name.as_str(), args.len());
            let kind = match (key, params.required.len()) {
                ((Some(TyKind::Int), "times", 0), 0 | 1) => Some(K::TimesInt),
                ((Some(TyKind::Int), "upto", 1), 0 | 1) => Some(K::UptoInt),
                ((Some(TyKind::Int), "downto", 1), 0 | 1) => Some(K::DowntoInt),
                ((Some(TyKind::Int), "step", 1 | 2), 0 | 1) => Some(K::StepInt),
                ((Some(TyKind::Range), "each", 0), 0 | 1) => Some(K::RangeEachInt),
                ((Some(TyKind::Array), "each", 0), 0 | 1) => Some(K::ArrayEach),
                ((Some(TyKind::Array), "each_with_index", 0), 1 | 2) => Some(K::ArrayEachWithIndex),
                ((Some(TyKind::Array), "map" | "collect", 0), 0 | 1) => Some(K::ArrayMap),
                ((Some(TyKind::Array), "select" | "filter" | "find_all", 0), 0 | 1) => {
                    Some(K::ArraySelect)
                }
                ((Some(TyKind::Array), "reject", 0), 0 | 1) => Some(K::ArrayReject),
                ((Some(TyKind::Array), "sum", 0), 0 | 1) => Some(K::ArraySum),
                ((Some(TyKind::Hash), "each" | "each_pair", 0), 0..=2) => Some(K::HashEach),
                _ => None,
            };
            if let Some(k) = kind {
                out.insert(*block, k);
            }
        }
        compiler.hir[id].for_each_child(&mut |n| scan(compiler, n, locals, out));
    }

    // A user REDEFINITION of the builtin iterator wins at every call site --
    // never nominate that method's sites. Checked across the whole ancestry,
    // so a prepended module or an inherited override (`Numeric#step`) counts
    // too. The literal fast paths obey the same verdict via the two
    // `Compiler` flags below.
    //
    // A RUNTIME redefinition counts the same way: a fused loop is not a call
    // at all, so unlike an ordinary Path 1 site there is nothing for
    // `may_be_patched_at_runtime` to de-optimize later -- the nomination has
    // to be withheld here. `runtime_patches` is already populated (it is
    // collected before this pass runs).
    let reopened = |cname: &str, m: &str| {
        compiler.may_be_patched_at_runtime(m)
            || compiler.resolve_class(cname, &[], 0).is_some_and(|cid| {
                mro::compute_ancestors(compiler, cid).iter().any(|&a| {
                    compiler
                        .class(a)
                        .own_methods
                        .iter()
                        .any(|&s| compiler.scope(s).name == m)
                })
            })
    };
    use InlineIterKind as K;
    let sup: Vec<(K, bool)> = vec![
        (K::TimesInt, reopened("Integer", "times")),
        (K::UptoInt, reopened("Integer", "upto")),
        (K::DowntoInt, reopened("Integer", "downto")),
        (K::StepInt, reopened("Integer", "step")),
        (K::RangeEachInt, reopened("Range", "each")),
        (K::ArrayEach, reopened("Array", "each")),
        (K::ArrayEachWithIndex, reopened("Array", "each_with_index")),
        // Aliases are SEPARATE method entries in Ruby; redefining either one
        // suppresses the whole kind (conservative, and vanishingly rare).
        (
            K::ArrayMap,
            reopened("Array", "map") || reopened("Array", "collect"),
        ),
        (
            K::ArraySelect,
            reopened("Array", "select")
                || reopened("Array", "filter")
                || reopened("Array", "find_all"),
        ),
        (K::ArrayReject, reopened("Array", "reject")),
        (K::ArraySum, reopened("Array", "sum")),
        (
            K::HashEach,
            reopened("Hash", "each") || reopened("Hash", "each_pair"),
        ),
    ];
    let suppressed = |k: &K| sup.iter().any(|(sk, s)| sk == k && *s);
    compiler.times_literal_suppressed = suppressed(&K::TimesInt);
    compiler.range_each_literal_suppressed = suppressed(&K::RangeEachInt);

    let mut sites = HashMap::new();
    for scope in &compiler.scopes {
        for &n in &scope.body {
            scan(compiler, n, &scope.local_types, &mut sites);
        }
    }
    for &n in main_statements {
        scan(compiler, n, main_local_types, &mut sites);
    }
    sites.retain(|_, k| !suppressed(k));
    compiler.inline_iter_sites = sites;
}

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
fn process_top_stmt(
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
    if let HirNode::PreExec(body) = &compiler.hir[stmt] {
        pre_exec.extend(body.clone());
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
        register_class(
            compiler,
            name,
            superclass,
            is_module,
            &body,
            &[],
            0,
            Some(stmt),
        )?;
        // The marker STAYS in the top-level statement stream (non-bootstrap
        // only -- the prelude's bodies keep their hoisted splice): real Ruby
        // executes a class body at its document position, interleaved with
        // the surrounding top-level code, and `codegen::stmt`'s `ClassDef`
        // arm emits this site's body right here.
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
            if let HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } = &compiler.hir[s]
            {
                let (name, superclass, body, is_module) =
                    (name.clone(), superclass.clone(), body.clone(), *is_module);
                register_class(
                    compiler,
                    name,
                    superclass,
                    is_module,
                    &body,
                    &[],
                    bx,
                    Some(s),
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
        let (name, params, body, is_class_method) =
            (name.clone(), params.clone(), body.clone(), *is_class_method);
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
                params,
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
        match resolve_module_target(compiler, &m, &[], 0)? {
            Some(target) => compiler.classes[OBJECT_CLASS.0 as usize]
                .includes
                .push(target),
            // Registration-only otherwise, so the deferred read has to be added
            // to the statements codegen emits or it would never run.
            None => {
                defer_unresolved_directive(compiler, stmt, &m);
                main_statements.push(stmt);
            }
        }
    } else if let HirNode::Using(m) = &compiler.hir[stmt] {
        // A TOP-LEVEL `using M` covers the rest of the FILE -- which is
        // `u32::MAX` here, since the range is only ever compared against
        // spans from that same file.
        let m = m.clone();
        record_activation(compiler, stmt, &m, &[], 0, u32::MAX);
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
                tracing::debug!(
                    guard = cond_kind(compiler, cond),
                    "top-level conditional def: guard UNDECIDABLE -- compile error"
                );
                return Err(
                    "a NEW class/module or a mixin (`include`/`prepend`) inside a top-level \
                     `if` needs a compile-time-decidable condition (a `defined?` probe, a \
                     version/platform/engine gate, a feature test). A guarded REOPENING of an \
                     existing class, and a guarded `def`/`alias`/visibility change, compile \
                     with the guard deciding at runtime -- only a conditionally-EXISTING class \
                     or ancestry has no answer under a compile-time MRO"
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
    } else if declines_a_singleton_prepend(compiler, stmt, &[], 0) {
        return Err(UNHONOURED_SINGLETON_PREPEND.into());
    } else {
        register_nested_class_defs(compiler, stmt, &[], 0)?;
        main_statements.push(stmt);
    }
    Ok(())
}

/// Registers every `class`/`module` reachable from a statement the walk keeps
/// whole -- a `begin` clause, a block body, a loop body, a `case` arm.
/// Registration is a compile-time fact about shape, so the marker stays put and
/// `codegen::stmt`'s `ClassDef` arm still runs the body at its document
/// position; the `BoxScope` arm above splits it the same way.
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
/// `codegen::emit_declaration_const_location` follows, which is why a reopen
/// leaves the first declaration's line standing.
///
/// A constant with NO recorded location is still reported, with both fields
/// empty: `module String` raises `String is not a module\n:: previous
/// definition of String was here` (oracle-verified). Ruby only drops the line
/// when the location is nil outright, which a defined constant never is.
fn previous_definition_of(
    compiler: &Compiler,
    cid: crate::compiler::ClassId,
    name: &str,
) -> String {
    let (file, line) = compiler
        .class_body_sites
        .iter()
        .find(|s| s.class == cid)
        .and_then(|s| s.def_node)
        .and_then(|n| crate::codegen::source_location(compiler, n))
        .map_or((String::new(), String::new()), |(f, l)| {
            (f.to_string(), l.to_string())
        });
    format!("\n{file}:{line}: previous definition of {name} was here")
}

fn ruby_raises(compiler: &mut Compiler, def_node: Option<NodeId>, class: &'static str, msg: &str) {
    if let Some(node) = def_node {
        compiler.pending_ruby_raise = Some((node, class, msg.to_string()));
    }
}

/// Whether a raise from inside `stmt` could be caught there -- i.e. whether
/// `stmt` is a `begin` with `rescue` clauses, anywhere in its subtree.
///
/// Only that shape earns the rewrite in [`raise_instead_of_defining`]: with
/// nothing to catch it, ruby's own behaviour is to abort, and a compile error
/// that names the same problem is the better version of aborting.
fn is_rescuable(compiler: &Compiler, stmt: NodeId) -> bool {
    let mut stack = vec![stmt];
    while let Some(id) = stack.pop() {
        if matches!(&compiler.hir[id], HirNode::Begin { rescues, .. } if !rescues.is_empty()) {
            return true;
        }
        compiler.hir[id].for_each_child(&mut |child| stack.push(child));
    }
    false
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

fn register_nested_class_defs(
    compiler: &mut Compiler,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) -> Result<(), String> {
    let rescuable = is_rescuable(compiler, stmt);
    let mut nested = Vec::new();
    collect_nested_bodies(compiler, stmt, &mut nested);
    for s in nested {
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
        let registered = register_class(
            compiler,
            name,
            superclass,
            is_module,
            &body,
            cref,
            box_id,
            Some(s),
        );
        // A definition ruby RAISES on, somewhere a `rescue` can see it, becomes
        // that raise -- see `raise_instead_of_defining`. Anywhere else the
        // refusal stands.
        if let Err(e) = registered
            && !(rescuable && raise_instead_of_defining(compiler))
        {
            compiler.pending_ruby_raise = None;
            return Err(e);
        }
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
fn register_refinement(
    compiler: &mut Compiler,
    class_id: ClassId,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) {
    let HirNode::Refine { target, holder } = &compiler.hir[stmt] else {
        return;
    };
    let (target, holder) = (target.clone(), holder.clone());
    let target = compiler.resolve_class(&target, cref, box_id);
    let holder = compiler.class_in_scope(Some(class_id), &holder, box_id);
    let (Some(target), Some(holder)) = (target, holder) else {
        return;
    };
    compiler.refinements.push(crate::compiler::Refinement {
        module: class_id,
        target,
        holder,
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
fn register_nested_refinements(
    compiler: &mut Compiler,
    class_id: ClassId,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) {
    let mut nested = Vec::new();
    collect_nested_bodies(compiler, stmt, &mut nested);
    for s in nested {
        if matches!(compiler.hir[s], HirNode::Refine { .. }) {
            register_refinement(compiler, class_id, s, cref, box_id);
        }
    }
}

/// Every statement nested inside `node`'s sub-bodies, in document order. Bound
/// with explicit fields rather than a `..` rest so a new statement-bearing
/// variant can't join silently.
fn collect_nested_bodies(compiler: &Compiler, node: NodeId, out: &mut Vec<NodeId>) {
    let children: Vec<NodeId> = match &compiler.hir[node] {
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => body
            .iter()
            .chain(rescues.iter().flat_map(|r| r.body.iter()))
            .chain(else_body.iter().flatten())
            .chain(ensure_body.iter().flatten())
            .copied()
            .collect(),
        HirNode::Block { params: _, body } | HirNode::Loop { body } => body.clone(),
        HirNode::While {
            cond: _,
            body,
            negate: _,
            post: _,
        } => body.clone(),
        HirNode::For {
            target: _,
            iterable: _,
            body,
        } => body.clone(),
        HirNode::If {
            cond: _,
            then_body,
            else_body,
        } => then_body.iter().chain(else_body).copied().collect(),
        HirNode::CaseWhen {
            subject: _,
            arms,
            else_body,
        } => arms
            .iter()
            .flat_map(|(_, body)| body.iter())
            .chain(else_body)
            .copied()
            .collect(),
        // A definition is an EXPRESSION in ruby, so it also reaches here as a
        // value: `__skip__ = module M ... end` (elasticgraph, dodging a type
        // checker), `M = (class Inner; 7; end)`, an argument. It defines the
        // class right where it is written, exactly as the statement form does,
        // so the only difference is that something reads the body's value.
        //
        // The three stops are the nodes that register their OWN bodies:
        // `register_class` recurses into a `ClassDef`, a `DefMethod` body is a
        // separate function that waits to be called (and ruby rejects a
        // `class` written in one), and a `Lambda` likewise runs later, maybe
        // never -- a definition there stays the clean error it is today.
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } | HirNode::Lambda { .. } => return,
        other => {
            let mut children = Vec::new();
            other.for_each_child(&mut |c| children.push(c));
            children
        }
    };
    // Each child is itself a candidate and may nest further -- the
    // `File.open { begin ... rescue; module M; end; end }` shape.
    for child in children {
        out.push(child);
        collect_nested_bodies(compiler, child, out);
    }
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
fn splice_dead_rescues(compiler: &Compiler, stmts: &[NodeId]) -> Vec<NodeId> {
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
fn try_prepend_call_edit(
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
    // Registered in REVERSE argument order so `mro`'s uniform
    // "later-registered-is-closer" flatten yields source order in the ancestry
    // (`prepend A, B` -> [A, B, self]) -- the same rule the class-body multi-arg
    // lowering follows (see `lower/defs.rs`).
    for m in modules.into_iter().rev() {
        let ci = &mut compiler.classes[target.0 as usize];
        if on_singleton {
            ci.class_method_prepends.push(m);
        } else {
            ci.prepends.push(m);
        }
    }
    true
}

/// The class/module a bare-constant expression names (resolved in `cref`), or
/// `None` for anything that isn't a compile-time-resolvable class reference.
/// Used to statically resolve a `prepend` call's receiver and module arguments.
const UNHONOURED_SINGLETON_PREPEND: &str = "`prepend` onto the singleton class of a statically-compiled class needs \
     modules zeo can name at compile time (constants) -- a runtime module \
     writes a singleton method table that statically resolved calls never \
     consult, so it would compile and then override nothing (zeo limitation)";

/// A `<expr>.singleton_class.prepend(...)` that [`try_prepend_call_edit`] just
/// DECLINED, where letting it run as an ordinary send would come out WRONG --
/// because the receiver names a STATICALLY-REGISTERED class. Statically
/// resolved `X.m` call sites on such a class never consult the runtime
/// singleton tables the send would write, so the prepend would compile and
/// then override nothing. The whole point of `prepend` is to override a method
/// that already exists, which is exactly the case that would come out wrong.
/// Refused for the same reason `lower::defs` refuses `undef :close` in a
/// singleton body: a silent wrong answer is worse than a rejection.
///
/// A constant-shaped receiver that does NOT resolve to a static class is the
/// mirror image: the class only ever exists at runtime (an autoloaded rails
/// class behind a computed feature name, a `K = Class.new` minting), so every
/// call site on it is already dynamic and the runtime singleton-prepend
/// machinery (`prepend_into_class_singleton`) is exactly what CRuby does.
/// Those pass through as the ordinary send they are -- the
/// `SomeRailsClass.singleton_class.prepend(TheirPatch)` shape behind most of
/// the rails-plugin band, activerecord-jdbc-adapter and friends.
fn declines_a_singleton_prepend(
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

fn const_node_class(
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
fn splice_decidable_ifs(
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
                let guarded: Vec<NodeId> =
                    then_body.iter().chain(&else_body).copied().collect();
                if let Some(taken) = static_top_cond(compiler, cond, &guarded, cref, box_id, true)
                {
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
    body.iter().all(|&s| {
        matches!(
            &compiler.hir[s],
            HirNode::BoolLit(_)
                | HirNode::NilLit
                | HirNode::IntegerLit(_)
                | HirNode::FloatLit(_)
                | HirNode::SymbolLit(_)
        )
    })
}

/// Whether any statement in `body` (descending nested `if` branches) is a
/// node only the top-level walk can register -- exactly the set
/// `codegen::expr::emit_expr` has no expression form for, minus
/// `DefMethod` (which has a runtime `define_method` emission and so
/// survives inside an ordinary undecided `if` unchanged).
fn branch_has_top_defs(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter().any(|&s| match &compiler.hir[s] {
        // A `def` counts for exactly the reason every other definition here
        // does. Left out, an `if RUBY_ENGINE == "ruby"` whose branches hold
        // only methods never folded: BOTH branches registered, and the one
        // written last silently won -- so a compat gate picked the branch
        // for the engine zeo is not (prism's deserializer has two
        // `def load_node`s exactly this way).
        HirNode::DefMethod { .. }
        | HirNode::ClassDef { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::DefHook { .. }
        | HirNode::MethodRedefine { .. }
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::ConstantVisibility { .. } => true,
        HirNode::If {
            then_body,
            else_body,
            ..
        } => branch_has_top_defs(compiler, then_body) || branch_has_top_defs(compiler, else_body),
        _ => false,
    })
}

/// `codegen::constfold::static_cond`'s analyze-time sibling: compile-time
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
    // A reopening must name an already-registered class (top-level
    // cref/box). A new class can't be defined conditionally -- codegen has
    // no runtime create form here.
    if !parts.iter().all(|p| match p {
        Guarded::Reopen(name, ..) => compiler.resolve_class(name, &[], 0).is_some(),
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
fn seed_ext_const_owners(compiler: &mut Compiler) {
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
    if compiler.resolve_class(&nested, &[], 0).is_some()
        || compiler.class(target).const_owners.contains_key(&cname)
        // `inherit` is true by default, and every ancestry ends at Object, so a
        // TOP-LEVEL constant answers too -- `Object.const_defined?("Decimal")`
        // is the whole point of the idiom, and `M.const_defined?(:String)` is
        // true for a bare module as well.
        || compiler.resolve_class(&cname, &[], 0).is_some()
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
    let inside = nodes_under(compiler, guarded);
    compiler.hir.iter_with_ids().any(|(id, node)| {
        !inside.contains(&id) && matches!(node, HirNode::ConstWrite { name, .. } if name == leaf)
    })
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
    let inside = nodes_under(compiler, guarded);
    compiler.hir.iter_with_ids().any(|(id, node)| {
        !inside.contains(&id)
            && matches!(node, HirNode::GlobalWrite(w, _) | HirNode::AliasGlobal(w, _) if w == name)
    })
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
    let inside = nodes_under(compiler, guarded);
    compiler.hir.iter_with_ids().any(|(id, node)| {
        if inside.contains(&id) {
            return false;
        }
        match node {
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
fn nodes_under(compiler: &Compiler, roots: &[NodeId]) -> std::collections::HashSet<NodeId> {
    let mut seen: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
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
    let suffix = format!("::{name}");
    compiler
        .shell_kinds
        .keys()
        .any(|(bx, k)| *bx == box_id && (k == name || k.ends_with(&suffix)))
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
    // `crate::guard_fold` and `codegen::constfold::static_cond`. The cref is
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
            HirNode::ClassRef(name) => {
                if compiler.resolve_class(name, cref, box_id).is_some() {
                    Some(true)
                } else if const_ever_written(compiler, name)
                    || (sibling_lag && class_shaped_anywhere(compiler, box_id, name))
                {
                    None
                } else {
                    Some(false)
                }
            }
            HirNode::QualifiedConstRead(scope, name) => {
                match compiler.resolve_class(scope, cref, box_id) {
                    Some(sid) => {
                        // Same probe order as `constfold::class_const_in`:
                        // a class nested in `scope`, else (const lookup
                        // inherits through Object) a lexically-visible one.
                        let fq = format!("{}::{name}", compiler.fq_name(sid));
                        if compiler.resolve_class(&fq, &[], 0).is_some()
                            || compiler.resolve_class(name, cref, box_id).is_some()
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
        HirNode::And(l, r) => match static_top_cond(compiler, *l, guarded, cref, box_id, sibling_lag) {
            Some(false) => Some(false),
            Some(true) => static_top_cond(compiler, *r, guarded, cref, box_id, sibling_lag),
            None => None,
        },
        HirNode::Or(l, r) => match static_top_cond(compiler, *l, guarded, cref, box_id, sibling_lag) {
            Some(true) => Some(true),
            Some(false) => static_top_cond(compiler, *r, guarded, cref, box_id, sibling_lag),
            None => None,
        },
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
fn const_ever_written(compiler: &Compiler, name: &str) -> bool {
    compiler
        .hir
        .iter()
        .any(|n| matches!(n, HirNode::ConstWrite { name: w, .. } if w == name))
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
fn pin_builtin_exceptions_tail(compiler: &mut Compiler) -> Result<(), String> {
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
    )?;
    // `SyntaxError < ScriptError` -- the eval VM's parse-failure
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
        )?;
    }
    // `IO::WaitReadable`/`WaitWritable` -- marker MODULES, so a would-block
    // errno can be rescued by protocol. Registered before the classes that
    // mix them in, and the `include` is pushed directly: `register_class`
    // reads includes out of a class BODY, and these have none.
    for name in ["IO::WaitReadable", "IO::WaitWritable"] {
        register_class(compiler, name.to_string(), None, true, &[], &[], 0, None)?;
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
        )?;
        let (Some(cls), Some(module)) = (
            compiler.resolve_class(name, &[], 0),
            compiler.resolve_class(marker, &[], 0),
        ) else {
            return Err(format!(
                "{name} or {marker} went missing right after registration"
            ));
        };
        compiler.classes[cls.0 as usize].includes.push(module);
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
        register_class(compiler, alias.to_string(), None, false, &[], &[], 0, None)?;
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
fn const_alias_target(compiler: &Compiler, leaf: &str, box_id: u32) -> Option<ClassId> {
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
fn collect_runtime_undefs(compiler: &Compiler, stmt: NodeId) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![stmt];
    while let Some(id) = stack.pop() {
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

/// Read-only scan populating [`Compiler::assigned_const_names`]. A flat sweep
/// of the whole node arena rather than a tree walk: every reachable `ConstWrite`
/// is in there by construction, and a name written only on a dead branch still
/// counts (see the field's docs -- over-collection is the safe direction).
fn collect_assigned_const_names(hir: &Hir) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for node in hir.all_nodes() {
        // `name` is already the leaf -- an explicit `Foo::NAME = ...` keeps its
        // namespace in the separate `scope` field.
        let HirNode::ConstWrite { scope, name, .. } = node else {
            continue;
        };
        out.insert(name.clone());
        // The QUALIFIED spelling as well, so a reader that names a scope can
        // ask about that scope rather than settling for "some constant with
        // this leaf exists somewhere". Anchors are stripped: `::A::B` and
        // `A::B` name the same constant, and there is only one top level.
        if let Some(scope) = scope {
            let scope = crate::constpath::ConstPath::parse(scope).unanchored();
            out.insert(format!("{scope}::{name}"));
        }
    }
    out
}

/// Walk populating [`Compiler::top_level_const_aliases`]: `NAME = <value>`
/// reached without ever entering a `class`/`module` body.
///
/// Descends through the statement wrappers a top-level write can hide behind
/// (`if`, `begin`, `Seq`, a box scope) and stops at `ClassDef`, which is
/// exactly the boundary that makes a write "top-level". First write wins, so a
/// later reassignment does not change which class a reopen attaches to -- the
/// arena scan this replaces had the same first-match-wins behaviour.
fn collect_top_level_const_aliases(hir: &Hir, stmts: &[NodeId], out: &mut HashMap<String, NodeId>) {
    for &s in stmts {
        match &hir[s] {
            HirNode::ConstWrite {
                scope: None,
                name,
                value,
            } => {
                out.entry(name.clone()).or_insert(*value);
            }
            // A definition's body is a different scope; nothing inside it can
            // be what a top-level `class CONST` reopens.
            HirNode::ClassDef { .. } => {}
            _ => {
                let mut children = Vec::new();
                hir[s].for_each_child(&mut |c| children.push(c));
                collect_top_level_const_aliases(hir, &children, out);
            }
        }
    }
}

/// The runtime definition verbs -- the calls that install (or RETIRE) a method
/// body the overlay holds and only DYNAMIC dispatch consults. Each names ONE
/// method, in its first argument. A literal-name `define_method`/
/// `define_singleton_method` inside a class body never reaches here: lowering
/// already desugared it into a `DefMethod`, so what survives as a `Call` is
/// exactly the runtime half.
///
/// `undef_method`/`remove_method` are here for the RECEIVER-BEARING spelling
/// only. `ClassInfo::runtime_undefs` covers the receiverless one written in a
/// class body, where the class it retires from is known; `g.singleton_class.
/// undef_method(:close)` retires the name for ONE object and names no class a
/// scan could resolve, so the name goes program-wide instead.
const REDEF_VERBS: &[&str] = &[
    "define_method",
    "define_singleton_method",
    "alias_method",
    "undef_method",
    "remove_method",
];

/// The runtime VISIBILITY verbs. Visibility is a runtime property in ruby --
/// `private :name` re-marks a method that already exists -- and these take a
/// LIST, so every argument names a method. A class-body `private :m` never
/// reaches here either: `lower::defs` turns it into a `MethodVisibility` node
/// or retags the `def` in place.
const VIS_VERBS: &[&str] = &[
    "private",
    "public",
    "protected",
    "private_class_method",
    "public_class_method",
    "module_function",
];

/// The `send` family, which reaches a verb above through a symbol argument
/// (`Node.send(:define_method, name)`) and so shifts every argument by one.
const SEND_VERBS: &[&str] = &["send", "__send__", "public_send"];

/// Read-only scan populating [`Compiler::runtime_patches`] and
/// [`Compiler::runtime_patches_any_name`]. Flat over the whole arena, like
/// [`collect_assigned_const_names`]: a site on a dead branch still counts,
/// because the answer is "could this name change under us?".
fn collect_runtime_patches(hir: &Hir) -> (std::collections::HashSet<String>, bool) {
    let mut names = std::collections::HashSet::new();
    let mut any = false;
    for node in hir.all_nodes() {
        // A `def` inside a BLOCK installs when the block runs, not when the
        // class body does -- `N.class_eval { def e; end }`, and the same
        // desugared `define_method(:e) { }`. The subtree walk reaches a def
        // nested several blocks deep.
        if let HirNode::Lambda { body, .. } | HirNode::Block { body, .. } = node {
            for &id in body {
                names.extend(defs_in_subtree(hir, id));
            }
        }
        let HirNode::Call { name, args, .. } = node else {
            continue;
        };
        // The names start at the verb's first argument, one slot later when the
        // verb itself arrives as `send`'s first argument.
        let verb = |v: &str| REDEF_VERBS.contains(&v) || VIS_VERBS.contains(&v);
        let (at, verb_name) = if verb(name) {
            (0, name.as_str())
        } else if SEND_VERBS.contains(&name.as_str())
            && let Some(ArrayElem::Single(a)) = args.first()
            && let Some(sent) = hir.sent_name(*a).filter(|v| verb(v))
        {
            (1, sent)
        } else {
            continue;
        };
        // A definition verb names one method; a visibility verb names a list.
        let named = match VIS_VERBS.contains(&verb_name) {
            true => &args[at.min(args.len())..],
            false => &args[at.min(args.len())..(at + 1).min(args.len())],
        };
        // No argument at all: a bare `private` sets the DEFAULT for later defs,
        // which names nothing this scan can read.
        if named.is_empty() {
            any = true;
        }
        for arg in named {
            match arg {
                ArrayElem::Single(a) => match hir.sent_name(*a) {
                    Some(patched) => {
                        names.insert(patched.to_owned());
                    }
                    None => any = true,
                },
                // A splat: nothing to read the names from.
                _ => any = true,
            }
        }
    }
    (names, any)
}

/// Every `def`/desugared `define_method` name in `root`'s subtree.
fn defs_in_subtree(hir: &Hir, root: NodeId) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let HirNode::DefMethod { name, .. } = &hir[id] {
            out.push(name.clone());
        }
        hir[id].for_each_child(&mut |child| stack.push(child));
    }
    out
}

/// Read-only scan populating `Compiler::shell_kinds`: records every
/// `class`/`module` definition's fully-qualified name -> `is_module`,
/// descending through nested bodies, BOTH `if` branches (over-collection is
/// harmless -- it is only a lookup table, and shells are created on demand by
/// TAKEN definitions), box scopes, and statement-group wrappers.
fn collect_shell_kinds(
    hir: &Hir,
    stmts: &[NodeId],
    scope: &[String],
    box_id: u32,
    out: &mut HashMap<(u32, String), bool>,
) {
    for &id in stmts {
        match &hir[id] {
            HirNode::ClassDef {
                name,
                body,
                is_module,
                ..
            } => {
                let fq = if scope.is_empty() {
                    name.clone()
                } else {
                    format!("{}::{}", scope.join("::"), name)
                };
                out.insert((box_id, fq), *is_module);
                let mut inner = scope.to_vec();
                inner.push(name.clone());
                collect_shell_kinds(hir, body, &inner, box_id, out);
            }
            HirNode::If {
                then_body,
                else_body,
                ..
            } => {
                collect_shell_kinds(hir, then_body, scope, box_id, out);
                collect_shell_kinds(hir, else_body, scope, box_id, out);
            }
            // A box's body is a fresh top-level scope under the box's id.
            HirNode::BoxScope { box_id: bx, body } => {
                collect_shell_kinds(hir, body, &[], *bx, out);
            }
            HirNode::Seq(body) | HirNode::PreExec(body) | HirNode::Eval(body) => {
                collect_shell_kinds(hir, body, scope, box_id, out);
            }
            _ => {}
        }
    }
}

/// Resolve a compact-path CONTAINER (`Gem::Security` in `Gem::Security::Policy`)
/// or, when it is defined ELSEWHERE in the program (present in
/// `Compiler::shell_kinds`) but not yet registered at this list position,
/// create it -- and any missing ancestor segment -- as a bare SHELL. A later
/// real definition reopens the shell through `register_class`'s reopen arm,
/// establishing its superclass/body (the same bare-open-then-reopen path a
/// `class Sub` held only to nest a class already takes). Returns `None` for a
/// genuinely-undefined container, so the caller keeps the "unknown
/// class/module" error that catches typos.
fn resolve_or_create_container(
    compiler: &mut Compiler,
    path: &str,
    box_id: u32,
) -> Option<ClassId> {
    if let Some(cid) = compiler.resolve_class(path, &[], box_id) {
        return Some(cid);
    }
    // Only a name the program defines somewhere gets a forward shell; a truly
    // unknown container falls through to the caller's error.
    let cp = crate::constpath::ConstPath::parse(path);
    let is_module = *compiler
        .shell_kinds
        .get(&(box_id, cp.unanchored().to_string()))?;
    let (lexical_parent, leaf, qualified) = match cp.scope() {
        Some(prefix) => {
            let parent = resolve_or_create_container(compiler, prefix, box_id)?;
            (Some(parent), cp.base().to_string(), true)
        }
        None if cp.is_top_anchored() => (None, cp.base().to_string(), true),
        None => (None, path.to_string(), false),
    };
    let parent = if is_module { None } else { Some(OBJECT_CLASS) };
    let cid = compiler.add_class(leaf, parent, is_module);
    let ci = &mut compiler.classes[cid.0 as usize];
    ci.lexical_parent = lexical_parent;
    ci.qualified_def = qualified;
    // A shell is built from a fully-qualified path with no enclosing scope, so
    // a qualified one is written at the top level and its cref is just itself.
    ci.cref_parent = match qualified {
        true => None,
        false => lexical_parent,
    };
    ci.box_id = box_id;
    Some(cid)
}

/// Like `resolve_or_create_container`, but for a SUPERCLASS or include/prepend/
/// extend module TARGET that may be a bare name resolved through the enclosing
/// lexical chain (`< Error` / `prepend BetterPermissionError` inside
/// `Gem::CompactIndexClient::Updater` meaning the `Gem::…`-scoped one). Walks
/// `cref` innermost-first, then the top level, for the first candidate
/// fully-qualified name the program defines (in `shell_kinds`), and creates it
/// as a forward shell. A later real definition reopens the shell -- adding its
/// methods (seen at `mro::materialize` time) and establishing its superclass
/// through `register_class`'s reopen arm.
///
/// `off_limits` is one fully-qualified name this search may neither answer with
/// nor create -- the path a definition currently being registered binds. Ruby
/// evaluates a superclass expression BEFORE the class exists, so `class Logger
/// < Logger` cannot mean itself; without this the search minted a shell of the
/// name and answered with it, which is both a wrong answer and a class the
/// program then sees even when the definition is deferred.
fn resolve_or_create_lexical(
    compiler: &mut Compiler,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
    off_limits: Option<&str>,
) -> Option<ClassId> {
    // Lexical scopes to try, innermost first: each `cref` entry, then the
    // innermost class's LEXICAL-PARENT chain -- the enclosing namespaces a
    // NESTED reopen sees (`module Gem; class StubSpecification; prepend X`)
    // that `cref_of` drops when the class was FIRST defined compact
    // (`class Gem::StubSpecification`, `qualified_def`). Real Ruby resolves the
    // bare name against the reopen site's `Module.nesting`, not the first
    // definition's.
    // `::Gem::Timeout::Error` names the top level and nothing else -- the
    // anchor is precisely a request to SKIP the chain walked below.
    let anchored = crate::constpath::ConstPath::parse(name).is_top_anchored();
    let cref: &[ClassId] = if anchored { &[] } else { cref };
    let mut scopes: Vec<ClassId> = cref.iter().rev().copied().collect();
    let mut parent = cref.last().and_then(|&c| compiler.class(c).lexical_parent);
    // A repeat is ordinary here -- `cref` already holds the lexical parents of
    // its own innermost entry -- so the walk SKIPS rather than stops, and it is
    // the step count that keeps a `lexical_parent` cycle from spinning.
    let mut steps = 0;
    while let Some(c) = parent {
        if !scopes.contains(&c) {
            scopes.push(c);
        }
        steps += 1;
        if steps > crate::compiler::MAX_NESTING {
            break;
        }
        parent = compiler.class(c).lexical_parent;
    }
    let mut candidates: Vec<String> = scopes
        .iter()
        .map(|&s| format!("{}::{name}", compiler.fq_name(s)))
        .collect();
    candidates.push(name.to_string());
    for cand in candidates {
        if off_limits == Some(cand.as_str()) {
            continue;
        }
        // Already registered under this scope (the lexical-parent walk found it)
        // -> use it; else a forward shell if the program defines it elsewhere.
        if let Some(cid) = compiler.resolve_class(&cand, &[], box_id) {
            return Some(cid);
        }
        let key = crate::constpath::ConstPath::parse(&cand)
            .unanchored()
            .to_string();
        if compiler.shell_kinds.contains_key(&(box_id, key)) {
            return resolve_or_create_container(compiler, &cand, box_id);
        }
    }
    None
}

// Every parameter is a distinct piece of the definition site (same
// posture as `register_method`); `def_node` is the site's own `ClassDef`
// marker for document-order body execution (`Compiler::class_body_sites`).
#[allow(clippy::too_many_arguments)]
fn register_class(
    compiler: &mut Compiler,
    name: String,
    superclass: Option<String>,
    is_module: bool,
    body: &[NodeId],
    cref: &[ClassId],
    box_id: u32,
    def_node: Option<NodeId>,
) -> Result<(), String> {
    // A superclass naming a constant NOTHING in the program defines (irb's
    // `class CallTracer < ::CallTracer`, whose `require "tracer"` already
    // raised `LoadError`): real Ruby evaluates that expression when the
    // definition RUNS, raises `NameError` there, and never brings the class
    // into being. So rewrite the whole definition to the bare constant READ
    // -- `defer_unresolved_directive`'s treatment of an unresolvable
    // `include` -- and register nothing. No struct is laid out for a class no
    // instance can reach, which is the objection `resolve_module_target`
    // records against deferring a superclass; a name the program DOES assign
    // still errors loudly there, for the reason given in the same place.
    //
    // Resolved ONCE, here, and carried to the three sites below that need it.
    // Asking twice is not free: `resolve_superclass` MINTS forward shells as
    // it searches, so a probe that asks separately leaves one behind -- and a
    // probe that asks WITHOUT the `defining` exclusion answers with the shell
    // of the very class being defined. That is what made
    // `class Logger < Logger` inside `module ApiNotify::ActiveRecord` a hard
    // error: the probe said known, the resolution said unknown, and neither
    // deferred (api_notify's `require "logger"` is commented out, so ruby
    // raises `NameError` at that line too).
    let resolved_superclass = superclass
        .as_ref()
        .and_then(|s| resolve_superclass(compiler, s, &name, cref, box_id));
    if let (Some(s), Some(def_node)) = (&superclass, def_node)
        && resolved_superclass.is_none()
        && !compiler.assigns_const_path(s)
    {
        defer_unresolved_directive(compiler, def_node, s);
        return Ok(());
    }
    let path = crate::constpath::ConstPath::parse(&name);
    let (lexical_parent, leaf, qualified_def) = match path.scope() {
        // `A::B` / `::A::B` -- defined INSIDE a named scope, which must
        // already exist.
        Some(prefix) => {
            // A container NOTHING in the program defines: the same fact the
            // superclass arm above defers on, one clause further into the same
            // definition. `class ActiveRecord::Associations::CollectionProxy`
            // with activerecord absent is CRuby's `NameError` at this very
            // line, not a compile failure -- and this arm could not say so,
            // because the defer above fires only when a `< Super` was written.
            // 68 gems, mostly Rails extensions naming a host they don't depend
            // on. A container the program DOES assign still errors loudly,
            // matching the superclass policy.
            if let Some(def_node) = def_node
                && compiler.resolve_class(prefix, cref, box_id).is_none()
                && resolve_or_create_lexical(compiler, prefix, cref, box_id, None).is_none()
                && !compiler.assigns_const_path(prefix)
            {
                defer_unresolved_directive(compiler, def_node, prefix);
                return Ok(());
            }
            let parent = compiler
                .resolve_class(prefix, cref, box_id)
                // A container defined LATER in the flattened statement list than
                // this definition (a deferred require's reopen preceding the
                // forward-declaration it depends on), or one reached through the
                // enclosing lexical scope (`class CLI::Common` inside
                // `module Bundler` -> `Bundler::CLI`), is resolved or created as
                // a shell -- see `resolve_or_create_lexical`.
                .or_else(|| resolve_or_create_lexical(compiler, prefix, cref, box_id, None))
                .ok_or_else(|| {
                    format!(
                        "unknown class/module `{prefix}` in `{name}` (must be defined earlier in the file)"
                    )
                })?;
            (Some(parent), path.base().to_string(), true)
        }
        // `::Foo` -- anchored at the top level, so NOT nested in the enclosing
        // lexical scope, but still an explicitly qualified definition.
        None if path.is_top_anchored() => (None, path.base().to_string(), true),
        // `Foo` -- an ordinary definition in the enclosing lexical scope.
        None => (cref.last().copied(), name.clone(), false),
    };
    // Reopening a BUILTIN class: `class String ... end` at the
    // top level ATTACHES to the existing builtin `ClassInfo` -- its methods
    // dispatch as value methods on the `RubyValue` itself (see
    // `codegen::mod::emit_builtin_reopen`), its `@@cvar`/`CONST` body
    // statements ride the ordinary ownership machinery. Only a TOP-LEVEL
    // name can collide: `module Store; class String; end; end` defines a
    // fresh, unrelated `Store::String` (real Ruby's rule), which then
    // lexically shadows the builtin inside `Store` -- also real Ruby's
    // rule, falling out of `resolve_class`'s scope walk. Still rejected
    // (clean errors, zeo limitation): `Object` (per-box TOP-LEVEL methods
    // aren't supported yet -- an Object reopen is top-level `def` by another
    // name), `Class`/`Module` (no per-class-value dispatch exists), and
    // the builtin MODULES (`Enumerable`/`Comparable` -- patching one would
    // need re-materialization onto every includer, including the Rust-
    // backed builtin fallbacks).
    let existing = compiler
        .class_in_scope(lexical_parent, &leaf, box_id)
        // A bare TOP-LEVEL `class CONST` where CONST aliases an existing class
        // reopens it (`INT_ALIAS = 1.class; class INT_ALIAS; include M; end`).
        // A nested definition never consults it: Ruby binds the leaf in the
        // enclosing scope and does not search outward, so an alias written
        // elsewhere is a different constant. See `const_alias_target`.
        .or_else(|| {
            if qualified_def || lexical_parent.is_some() {
                None
            } else {
                const_alias_target(compiler, &leaf, box_id)
            }
        });
    // A top-level `class String ... end` INSIDE a box with no
    // same-box definition to attach to: when the name reaches a builtin
    // through the bootstrap fallback, this is a PER-BOX builtin reopen --
    // an OVERLAY `ClassInfo` whose methods register as value methods under
    // the box's id (visible only from box code; `box::String == String`
    // stays true because instances keep the ROOT builtin's ClassId).
    let overlay_root =
        if box_id != 0 && existing.is_none() && lexical_parent.is_none() && superclass.is_none() {
            compiler
                .resolve_class(&leaf, &[], box_id)
                .filter(|&c| compiler.class(c).is_builtin || c == OBJECT_CLASS)
        } else {
            None
        };
    if let Some(cid) = existing.or(overlay_root) {
        let ci = compiler.class(cid);
        // NOTE: the implicit `Object` root (id 0) is NOT `is_builtin` (it
        // predates the `zeo_abi::BUILTINS` placeholders -- see
        // `Compiler::new`), so it's checked by id alongside them.
        if ci.is_builtin || cid == OBJECT_CLASS {
            // A KIND mismatch (`module String`) falls through to the
            // ordinary reopen guard below instead, which produces real
            // Ruby's own TypeError message shape ("String is not a module").
            // `Object` merges into arena slot 0 exactly like any
            // builtin-class reopen -- an Object reopen is top-level `def` by
            // another name. `Class`/`Module` reopen like any other builtin:
            // the added methods register as VALUE methods on their id, and a
            // `RubyValue::Class` receiver's own ancestry runs
            // `Class -> Module -> Object`, so the MRO walk finds them for
            // every class and module in the program. minitest defines
            // `Module#infect_an_assertion` this way.
            // A reopen may RESTATE the builtin's superclass (`class String <
            // Object`); CRuby accepts a matching clause and raises `superclass
            // mismatch` on a wrong one. Mirrors the user-class reopen guard.
            if let Some(s) = &superclass {
                let want = resolved_superclass.ok_or_else(|| {
                    format!("unknown superclass `{s}` (must be defined earlier in the file)")
                })?;
                if compiler.class(cid).parent != Some(want) {
                    ruby_raises(
                        compiler,
                        def_node,
                        "TypeError",
                        &format!("superclass mismatch for class {name}"),
                    );
                    return Err(format!("superclass mismatch for class {name}"));
                }
            }
        }
    }
    let class_id = match existing {
        // REOPENING: a second `class Foo`/`module Foo` MERGES into the
        // existing `ClassInfo` -- the body loop below appends
        // includes/body-statements and registers methods with real Ruby's
        // last-`def`-wins rule (see the method arm). Guards mirror CRuby's
        // own (all oracle-verified): the definition KIND must match
        // (`TypeError: Foo is not a module`), and a superclass clause, if
        // written at all, must resolve to the original parent
        // (`TypeError: superclass mismatch for class Foo`).
        Some(cid) => {
            if compiler.class(cid).is_module != is_module {
                // CRuby names the LEAF (`unmatched_redefinition` takes
                // `rb_id2str(id)`, the id off the cpath), so `module
                // Outer::Inner` reports `Inner is not a module`.
                let leaf = compiler.leaf_name(cid).to_string();
                let kind = if is_module { "module" } else { "class" };
                let previously = previous_definition_of(compiler, cid, &leaf);
                ruby_raises(
                    compiler,
                    def_node,
                    "TypeError",
                    &format!("{leaf} is not a {kind}{previously}"),
                );
                // The COMPILE error keeps the first line only: the
                // diagnostic already points a span at the definition that
                // conflicts, and the second line is a runtime message.
                return Err(format!("{leaf} is not a {kind}"));
            }
            // A user `module OpenSSL; ...; end` reopening a feature-gated
            // builtin slot MATERIALIZES the constant: clear the gate so the
            // name resolves even though the ext was never `require`d (the
            // behavior comes from the user's own methods registered here).
            if compiler.class(cid).feature_gate.is_some() {
                compiler.classes[cid.0 as usize].feature_gate = None;
            }
            if let Some(s) = &superclass {
                // Same rule as a fresh definition: this "reopen" may be the
                // FIRST real definition, of a name some other file forward-
                // referenced into a shell, and ruby resolves the superclass
                // before binding the name. See `resolve_superclass`.
                let want = resolved_superclass.ok_or_else(|| {
                    format!("unknown superclass `{s}` (must be defined earlier in the file)")
                })?;
                if compiler.class(cid).parent != Some(want) {
                    // A class opened BARE first (`class Sub`, often just to
                    // hold a nested class) defaulted its parent to Object
                    // without any `< Super` ever being written. A later reopen
                    // that does declare one ESTABLISHES the link rather than
                    // conflicting with it -- otherwise the parent would be
                    // silently wrong and a subclass override would dispatch
                    // against the wrong chain.
                    //
                    // MRI rejects this split too; it arises in zeo from
                    // wholesale-inlined libraries, so accepting it is a
                    // deliberate divergence (see the checked-in expectation
                    // for `reopen_split_superclass_dispatch`). A genuine
                    // conflict -- `class Sub < A` then `class Sub < B` -- still
                    // errors, because `explicit_superclass` is set by then.
                    if compiler.class(cid).explicit_superclass {
                        ruby_raises(
                            compiler,
                            def_node,
                            "TypeError",
                            &format!("superclass mismatch for class {name}"),
                        );
                        ruby_raises(
                            compiler,
                            def_node,
                            "TypeError",
                            &format!("superclass mismatch for class {name}"),
                        );
                        return Err(format!("superclass mismatch for class {name}"));
                    }
                    // ... and establishing one that already descends from THIS
                    // class would close a loop: `class A; class B < A; class A <
                    // B` is `superclass mismatch` in ruby too, so this is the
                    // same rejection, not a zeo limitation. It has to be checked
                    // rather than assumed -- a `parent` chain that points back
                    // at itself is what every walk over it runs forever on, and
                    // the walk that noticed was `require "active_record"` dying
                    // after twelve minutes.
                    if compiler.superclass_chain_contains(want, cid) {
                        ruby_raises(
                            compiler,
                            def_node,
                            "TypeError",
                            &format!("superclass mismatch for class {name}"),
                        );
                        ruby_raises(
                            compiler,
                            def_node,
                            "TypeError",
                            &format!("superclass mismatch for class {name}"),
                        );
                        return Err(format!("superclass mismatch for class {name}"));
                    }
                    compiler.classes[cid.0 as usize].parent = Some(want);
                }
                compiler.classes[cid.0 as usize].explicit_superclass = true;
            }
            cid
        }
        None => {
            let parent = if is_module {
                None
            } else {
                Some(match &superclass {
                    None => OBJECT_CLASS,
                    // Resolved in the ENCLOSING scope (`cref`, not the
                    // class being opened): real Ruby evaluates the
                    // superclass expression before the new class exists.
                    Some(s) => {
                        let cid = resolved_superclass.ok_or_else(|| {
                            format!(
                                "unknown superclass `{s}` (must be defined earlier in the file)"
                            )
                        })?;
                        // Subclassable builtins:
                        //  - `Struct`/`Data`: subclasses are ordinary
                        //    ivar-carrying objects (generated struct).
                        //  - `Numeric`: abstract, so a subclass is likewise a plain
                        //    ivar object (user-implemented `<=>`/`coerce`, Comparable
                        //    via the ancestor chain) -- the same struct machinery.
                        //  - `Array`/`String`/`Hash`: the native `ValueSubclass`
                        //    (a payload RObj), no struct.
                        //  - `Integer`/`Float`/`Symbol`/`Nil`/`True`/`FalseClass`
                        //    (immediates): the DEFINITION is allowed but has no
                        //    instances -- registry-entry-only, `.new` raises
                        //    NoMethodError (`is_immediate_subclass`).
                        // Still rejected: `Class`/`Module` (no per-value
                        // dispatch to hang a payload on).
                        use crate::compiler::{
                            ARRAY_CLASS, BASIC_OBJECT_CLASS, DATA_CLASS, FALSE_CLASS,
                            FFI_STRUCT_CLASS, FLOAT_CLASS, HASH_CLASS, INTEGER_CLASS, NIL_CLASS,
                            NUMERIC_CLASS, STRING_CLASS, STRUCT_CLASS, SYMBOL_CLASS, TRUE_CLASS,
                        };
                        let subclassable = matches!(
                            cid,
                            // `BasicObject`: the blank-slate root. Its subclass
                            // is a plain ivar-carrying object with NO payload,
                            // and the blank slate needs no special gate -- it
                            // falls out of chain position alone, since CRuby
                            // splices Kernel in as an ICLASS BETWEEN Object and
                            // BasicObject (object.c:4550 -> class.c:1853) and
                            // MRO walks only go up. So `[BO, BasicObject]` is
                            // the whole ancestry and the Object/Kernel surface
                            // is simply absent.
                            BASIC_OBJECT_CLASS
                                // `CGI`: a plain `Object` subclass with no
                                // payload of its own -- the escape methods are
                                // all class methods -- so a subclass is an
                                // ordinary ivar object. thin wraps rails in
                                // `class CGIWrapper < ::CGI`.
                                | zeo_abi::CGI_MODULE
                                | STRUCT_CLASS
                                // `FFI::Struct`: a subclass is a plain
                                // ivar object (no native payload) whose `[]`/
                                // `[]=`/`size`/`offset_of` are synthesized from
                                // its `layout` over an `FFI::MemoryPointer` ivar
                                // -- see `parse`'s `synthesize_ffi_struct`.
                                | FFI_STRUCT_CLASS
                                // `FFI::Union`: the same synthesis with every
                                // member at offset 0.
                                | zeo_abi::FFI_UNION_CLASS
                                | DATA_CLASS
                                | NUMERIC_CLASS
                                | ARRAY_CLASS
                                | STRING_CLASS
                                | HASH_CLASS
                                | INTEGER_CLASS
                                | FLOAT_CLASS
                                | SYMBOL_CLASS
                                | NIL_CLASS
                                | TRUE_CLASS
                                | FALSE_CLASS
                                // `StringScanner`: the same `ValueSubclass`
                                // payload shape as the collection roots, its
                                // payload being the native scanner object --
                                // csv's `class Scanner < StringScanner` adds
                                // ivars on top of the inherited behaviour.
                                | zeo_abi::STRING_SCANNER_CLASS
                                // `StringIO`/`File`: same payload shape again.
                                // puma's `IOBuffer < StringIO` and aws-sdk's
                                // `ManagedFile < File` are the whole aws-sdk-*
                                // family's blocker between them.
                                | zeo_abi::STRINGIO_CLASS
                                // `Pathname`: the payload is the path value.
                                // `Pathname.new("")` is a real empty form, so
                                // a subclass without its own `initialize`
                                // seeds straight from the root -- and one WITH
                                // an `initialize` re-seats through `super`,
                                // which replaces the stored path in place.
                                | zeo_abi::PATHNAME_CLASS
                                | zeo_abi::FILE_CLASS
                                | zeo_abi::SET_CLASS
                                // `Enumerator`: the payload is the
                                // `RubyValue::Enumerator` handle. Like `File`
                                // it has no empty form -- the block IS the
                                // sequence -- so a subclass seats one through
                                // `super() { |y| ... }`. cucumber-messages'
                                // `NdjsonToMessageEnumerator` does exactly
                                // that; the aws-sdk `EventStream` classes just
                                // add a method to the inherited behaviour.
                                | zeo_abi::ENUMERATOR_CLASS
                                // `Time`: the payload is the time value, and
                                // `Time.new` with no arguments is a real empty
                                // form (`now`). rubyzip's `DOSTime < Time`
                                // reaches the ledger through four gems.
                                | zeo_abi::TIME_CLASS
                                // `Thread`: the `File` shape -- a blockless
                                // thread cannot be built, so a subclass seats
                                // the real one through `super`, which is
                                // exactly why these gems subclass it.
                                | zeo_abi::THREAD_CLASS
                                // `Mutex`/`Monitor`: the payload is the lock,
                                // and both constructors take no arguments, so
                                // the empty form is real. A gem subclasses
                                // these to bolt a lock onto state of its own,
                                // which is exactly the payload shape.
                                | zeo_abi::MUTEX_CLASS
                                | zeo_abi::MONITOR_CLASS
                                // `Dir`: the `File` shape -- a directory that
                                // exists is needed, so a subclass opens the
                                // real one through `super(path)`.
                                | zeo_abi::DIR_CLASS
                                // `SizedQueue`: the `File` shape -- a maximum
                                // cannot be invented, so a subclass seats the
                                // real queue through `super(max)`.
                                | zeo_abi::SIZED_QUEUE_CLASS
                                // `Queue`: the payload is the
                                // `RubyValue::Queue` handle, and `Queue.new`
                                // takes no arguments, so the empty form is a
                                // real one. actionpool's `Queue < ::Queue`
                                // adds a `@pool` on top of the inherited
                                // behaviour and reaches the ledger through
                                // four gems.
                                | zeo_abi::QUEUE_CLASS
                                // The IO family, `File`'s shape one level up:
                                // no descriptor can be conjured, so a subclass
                                // seats the real one through `super`, which is
                                // the whole reason these are subclassed.
                                // kgio's `Kgio::Pipe < IO` and serialport's
                                // `SerialPort < IO`; dalli's `TCP < TCPSocket`
                                // and `UNIX < UNIXSocket`.
                                | zeo_abi::IO_CLASS
                                | zeo_abi::BASIC_SOCKET_CLASS
                                | zeo_abi::IP_SOCKET_CLASS
                                | zeo_abi::SOCKET_CLASS
                                | zeo_abi::TCPSOCKET_CLASS
                                | zeo_abi::UDP_SOCKET_CLASS
                                | zeo_abi::UNIX_SOCKET_CLASS
                                // `OpenSSL::SSL::SSLSocket`: the same shape --
                                // it wraps an existing socket, so `super` is
                                // the only way to build one. Nine ledger rows
                                // go through bunny's, dalli's and mongo's
                                // `SSLSocket` between them, the single biggest
                                // subclassing bucket.
                                | zeo_abi::OPENSSL_SSL_SOCKET_CLASS
                                // `OpenSSL::Cipher`: a cipher is named at
                                // construction, so there is no empty form
                                // either. openssl's OWN `AES`/`DES`/... are
                                // `Class.new(Cipher)` subclasses.
                                | zeo_abi::OPENSSL_CIPHER_CLASS
                                // `OpenSSL::Digest`: same again -- the
                                // algorithm is the argument. openssl's own
                                // deprecated `Digest::Digest` alias class is
                                // one, and hits two gems.
                                | zeo_abi::OPENSSL_DIGEST_CLASS
                                // `Fiber`: the `Thread` shape -- the block IS
                                // the body, so a blockless fiber cannot be
                                // built and a subclass seats the real one
                                // through `super(&block)`. hexapdf's
                                // `FiberWithLength` adds the length it knows
                                // up front.
                                | zeo_abi::FIBER_CLASS
                                // `Range`: chronic's `Span < Range`. Its
                                // class-method table had no `new` row until
                                // one was added for exactly this path -- the
                                // same prerequisite `Enumerator` needed.
                                | zeo_abi::RANGE_CLASS
                                // `Module`: NOT a payload root -- a wrapper
                                // holding a module is not a module. A user
                                // `class X < Module` is a module FACTORY whose
                                // instances are real runtime module ids tagged
                                // as belonging to X. See
                                // `Compiler::is_module_subclass`.
                                | crate::compiler::MODULE_CLASS
                                // `ObjectSpace::WeakMap`: the subclass IS
                                // the native type -- `weakmap_construct`
                                // already builds `WeakMap::new(class)` from
                                // the receiver. activesupport's `WeakSet`.
                                | zeo_abi::WEAKMAP_CLASS
                                // `WeakRef`: the same shape one root over --
                                // its constructor takes the receiver class
                                // too, and the delegation rows are inherited.
                                | zeo_abi::WEAKREF_CLASS
                                // `Date`/`DateTime`: the rows already
                                // allocate through the receiver, so the
                                // subclass is the native `RDate` tagged with
                                // its own id. tzinfo's `DateTimeWithOffset`.
                                | zeo_abi::DATE_CLASS
                                | zeo_abi::DATETIME_CLASS
                                // `Proc`: the class rides IN the proc, so a
                                // subclass instance is still a
                                // `RubyValue::Proc` and every call-site fast
                                // path keeps working on it.
                                | crate::compiler::PROC_CLASS
                                // `Regexp`: a value-backed payload root, like
                                // String and Array. Ruby has no `to_regexp`
                                // conversion protocol, so unlike those two the
                                // husk cannot be unwrapped by the conversion
                                // path -- every site that takes a pattern
                                // reads it through `regexp::as_regexp`.
                                | crate::compiler::REGEXP_CLASS
                        );
                        if compiler.class(cid).is_builtin && !subclassable {
                            return Err(format!(
                                "subclassing the built-in type `{s}` isn't supported yet (zeo limitation, no generated Rust struct exists for it)"
                            ));
                        }
                        cid
                    }
                })
            };
            let cid = compiler.add_class(leaf, parent, is_module);
            let ci = &mut compiler.classes[cid.0 as usize];
            // Record whether `< Super` was actually WRITTEN, so a later reopen
            // can tell a bare opening (parent defaulted to Object) from a real
            // declaration -- see the reopen arm above.
            ci.explicit_superclass = superclass.is_some();
            ci.lexical_parent = lexical_parent;
            ci.qualified_def = qualified_def;
            // The scope this definition is WRITTEN in, which is the naming
            // parent for a nested form and the enclosing scope for a
            // qualified one -- see `ClassInfo::cref_parent`.
            ci.cref_parent = match qualified_def {
                true => cref.last().copied(),
                false => lexical_parent,
            };
            ci.box_id = box_id;
            if let Some(root) = overlay_root {
                // The overlay carries the box's patches; instances keep
                // the root builtin's identity. `is_builtin` makes the
                // 16.3 machinery (operator/@ivar guards, value-method
                // emission, `__bm_` containers) apply unchanged.
                ci.is_builtin = true;
                ci.builtin_overlay = Some(root);
            }
            cid
        }
    };
    // The chain this class's OWN body resolves names against -- what nested
    // definitions and include/extend/prepend targets see. Derived from the
    // registered class (not `cref` + push) so a qualified-def class
    // correctly contributes a cut chain.
    let child_cref = compiler.cref_of(Some(class_id));

    // A class `lower::defs::synthesize_struct_class` built from a
    // `NAME = Struct.new(:a, :b)`: its `@a`/`@b` are MEMBERS, not instance
    // variables. Recorded here; `mro::materialize` fills `ivars` later from
    // the method bodies and subtracts these, so the two lists reach codegen
    // already disjoint.
    if let Some(members) = def_node.and_then(|n| compiler.hir.struct_members.get(&n)) {
        compiler.classes[class_id.0 as usize].hidden_ivars = members.clone();
    }

    // This definition site's own record -- see `Compiler::class_body_sites`.
    let site_idx = compiler.class_body_sites.len();
    compiler
        .class_body_sites
        .push(crate::compiler::ClassBodySite {
            def_node,
            class: class_id,
            stmts: Vec::new(),
            defs: Vec::new(),
        });

    // Expand any dead-rescue `begin` (a resolved `require` guard) so a fallback
    // def inside it registers/emits like an ordinary class-body definition, and
    // inline any decidable-guard `if` wrapping a nested definition (a
    // target-version / feature-probe compat gate) into its taken branch.
    let body = splice_dead_rescues(compiler, body);
    if compiler.fq_name(class_id) == "O" {
        for &s in &body {
            eprintln!(
                "SPLICE-DEBUG: O body stmt is If={} ClassDef={}",
                matches!(&compiler.hir[s], HirNode::If { .. }),
                matches!(&compiler.hir[s], HirNode::ClassDef { .. }),
            );
        }
    }
    let body = splice_decidable_ifs(compiler, &body, &child_cref, box_id);
    // Whatever `if` is LEFT has a condition no compile-time fold can decide, so
    // both branches survive to run at this site -- and a directive in one has no
    // expression form of its own. Rewrite the directives INSIDE those, to the
    // runtime self-sends this body serves. Only inside them: a directive written
    // straight in the class body is registered at compile time, and rewriting it
    // here would throw that away.
    let body: Vec<NodeId> = body
        .iter()
        .map(|&stmt| match &compiler.hir[stmt] {
            HirNode::If { .. } => {
                crate::lower::defs::transform_conditional_class_body(&mut compiler.hir, &[stmt])
                    .pop()
                    .expect("one statement in, one out")
            }
            _ => stmt,
        })
        .collect();
    for &stmt in &body {
        match &compiler.hir[stmt] {
            HirNode::DefMethod {
                name,
                is_class_method,
                ..
            } => {
                // The definition is consumed -- it emits nothing here. Its
                // REPORT still belongs at this position, so record it; see
                // `ClassBodySite::defs`. `attr_*` and a resolvable `alias`
                // reach this arm too: lowering expands both into ordinary
                // `DefMethod` nodes, one per generated name and in order, so
                // ruby's "`attr_accessor :c` reports `:c` then `:c=`" needs
                // nothing extra here.
                let name = name.clone();
                let is_class_method = *is_class_method;
                let def = crate::compiler::SiteDef {
                    seq: next_def_seq(compiler),
                    at: compiler.class_body_sites[site_idx].stmts.len(),
                    node: stmt,
                    name,
                    event: crate::compiler::DefEvent::Added,
                    singleton: is_class_method,
                };
                compiler.class_body_sites[site_idx].defs.push(def);
                register_body_def_method(compiler, class_id, stmt, Conditional::No)?;
            }
            // A nested `class`/`module` definition -- registered
            // recursively under this class's own cref. The `ClassDef` node
            // stays out of the flat `class_body_stmts` (nested classes are
            // ordinary `ClassInfo`s, not statements to re-execute through
            // that path) but IS recorded as a marker in this site's list:
            // real Ruby runs the inner body at its position inside the
            // outer body, and `codegen::stmt`'s `ClassDef` arm recurses
            // into the child's own site there.
            HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } => {
                let (name, superclass, body, is_module) =
                    (name.clone(), superclass.clone(), body.clone(), *is_module);
                compiler.class_body_sites[site_idx].stmts.push(stmt);
                register_class(
                    compiler,
                    name,
                    superclass,
                    is_module,
                    &body,
                    &child_cref,
                    box_id,
                    Some(stmt),
                )?;
            }
            // The ancestry edit itself is compile-time; the node ALSO stays on
            // the site so codegen can fire the module's `included`/`extended`/
            // `prepended` hook at the mixin's own document position. Ruby runs
            // the hook after the edit, which is automatic here.
            HirNode::Include(m) => {
                let m = m.clone();
                match resolve_module_target(compiler, &m, &child_cref, box_id)? {
                    Some(target) => {
                        // A module that overrides the PRIMITIVE decides for
                        // itself whether the mixin happens at all -- so the
                        // ancestry edit stops being a compile-time fact and
                        // codegen sends `append_features` at this position instead.
                        if compiler.overrides_mixin_primitive(target, "append_features") {
                            defer_mixin_to_runtime(compiler, target);
                        } else {
                            compiler.classes[class_id.0 as usize].includes.push(target);
                        }
                        compiler.class_body_sites[site_idx].stmts.push(stmt);
                    }
                    None => defer_in_class_body(compiler, class_id, site_idx, stmt, &m),
                }
            }
            HirNode::Extend(m) => {
                let m = m.clone();
                match resolve_module_target(compiler, &m, &child_cref, box_id)? {
                    Some(target) => {
                        // Same rule as `Include`'s: an `extend_object` override
                        // owns the decision, so the static edit gives way to a
                        // send at this position.
                        if compiler.overrides_mixin_primitive(target, "extend_object") {
                            defer_mixin_to_runtime(compiler, target);
                        } else {
                            compiler.classes[class_id.0 as usize].extends.push(target);
                        }
                        compiler.class_body_sites[site_idx].stmts.push(stmt);
                    }
                    None => defer_in_class_body(compiler, class_id, site_idx, stmt, &m),
                }
            }
            // `refine Target do ... end`. The holder module registered just
            // above (the `ClassDef` this marker follows) already owns the
            // methods; all that is left is to record what they refine. The
            // marker never joins the site's statements: a refinement runs
            // nothing where it was written.
            HirNode::Refine { .. } => {
                register_refinement(compiler, class_id, stmt, &child_cref, box_id)
            }
            // `using M` inside a class/module body scopes to THAT body, so
            // the activation ends where the enclosing definition does.
            HirNode::Using(m) => {
                let m = m.clone();
                let end = def_node
                    .and_then(|n| compiler.hir.span(n))
                    .map_or(u32::MAX, |s| s.end);
                record_activation(compiler, stmt, &m, &child_cref, box_id, end);
            }
            // `undef foo, bar` -- recorded here, honored by
            // `mro::materialize_methods`. See `HirNode::Undef`.
            HirNode::Undef(names) => {
                let names = names.clone();
                let at = compiler.class_body_sites[site_idx].stmts.len();
                for name in &names {
                    let def = crate::compiler::SiteDef {
                        seq: next_def_seq(compiler),
                        at,
                        node: stmt,
                        name: name.clone(),
                        event: crate::compiler::DefEvent::Undefined,
                        singleton: false,
                    };
                    compiler.class_body_sites[site_idx].defs.push(def);
                }
                compiler.classes[class_id.0 as usize]
                    .undefined
                    .extend(names);
            }
            // The class-method half. See `HirNode::ClassMethodUndef`. No
            // `SiteDef` rows: those drive the instance-side `method_undefined`
            // hook and reopen ordering, and the singleton form has neither.
            HirNode::ClassMethodUndef(names) => {
                let names = names.clone();
                compiler.classes[class_id.0 as usize]
                    .class_undefined
                    .extend(names);
            }
            // A deferred `alias`/`alias_method` of an INHERITED method --
            // resolved by `mro::resolve_aliases` once ancestors are computed.
            // See `HirNode::AliasMethod`.
            HirNode::AliasMethod {
                new_name,
                old_name,
                is_class_method,
            } => {
                // An alias IS a definition, and ruby reports the NEW name.
                // (The resolvable form never reaches here -- lowering turns it
                // into a second `DefMethod`, which the arm above records.)
                let (new_name, old_name, singleton) =
                    (new_name.clone(), old_name.clone(), *is_class_method);
                let seq = next_def_seq(compiler);
                let entry = (new_name.clone(), old_name, singleton, seq);
                let new_name_owned = new_name;
                let def = crate::compiler::SiteDef {
                    seq,
                    at: compiler.class_body_sites[site_idx].stmts.len(),
                    node: stmt,
                    name: new_name_owned,
                    event: crate::compiler::DefEvent::Added,
                    singleton,
                };
                compiler.class_body_sites[site_idx].defs.push(def);
                compiler.classes[class_id.0 as usize]
                    .pending_aliases
                    .push(entry);
            }
            // A `module_function :m` naming an INHERITED method -- resolved by
            // `mro::resolve_module_functions`. See `HirNode::ModuleFunction`.
            HirNode::ModuleFunction(name) => {
                compiler.classes[class_id.0 as usize]
                    .pending_module_functions
                    .push(name.clone());
            }
            // A `private`/`public`/`protected :m` naming a method with no
            // `def` in this body. Re-marking the class's OWN method (defined
            // in an EARLIER body -- a reopen) is POSITIONAL: ruby applies it
            // where it stands, so calls made while the method was still
            // public succeed. The node joins the site's statements (codegen
            // emits `runtime_set_visibility` there) and the name loses
            // devirtualization (`runtime_patches`) so every call site asks
            // the runtime barrier. An INHERITED method's re-mark keeps the
            // static override: it runs before any instance exists, so
            // start-of-program application is observationally identical.
            HirNode::MethodVisibility { name, visibility } => {
                let (name, visibility) = (name.clone(), *visibility);
                // `own_methods`, not `method_in_chain`: the flattened
                // `methods` list is MRO-materialized after this walk.
                let own = compiler.classes[class_id.0 as usize]
                    .own_methods
                    .iter()
                    .any(|&s| compiler.scope(s).name == name);
                if own {
                    compiler.runtime_patches.insert(name);
                    compiler.class_body_sites[site_idx].stmts.push(stmt);
                } else {
                    compiler.classes[class_id.0 as usize]
                        .visibility_overrides
                        .push((name, visibility));
                }
            }
            // The class-method half. See `HirNode::ClassMethodVisibility`.
            HirNode::ClassMethodVisibility { name, visibility } => {
                compiler.classes[class_id.0 as usize]
                    .class_visibility_overrides
                    .push((name.clone(), *visibility));
            }
            // `private_constant :A` / `public_constant :A`. Applied in source
            // order, so a later `public_constant` restores the name.
            HirNode::ConstantVisibility { names, private } => {
                let set = &mut compiler.classes[class_id.0 as usize].private_constants;
                for name in names {
                    if *private {
                        set.insert(name.clone());
                    } else {
                        set.remove(name);
                    }
                }
            }
            HirNode::Prepend(m) => {
                let m = m.clone();
                match resolve_module_target(compiler, &m, &child_cref, box_id)? {
                    Some(target) => {
                        // A module that overrides the PRIMITIVE decides for
                        // itself whether the mixin happens at all -- so the
                        // ancestry edit stops being a compile-time fact and
                        // codegen sends `prepend_features` at this position instead.
                        if compiler.overrides_mixin_primitive(target, "prepend_features") {
                            defer_mixin_to_runtime(compiler, target);
                        } else {
                            compiler.classes[class_id.0 as usize].prepends.push(target);
                        }
                        compiler.class_body_sites[site_idx].stmts.push(stmt);
                    }
                    None => defer_in_class_body(compiler, class_id, site_idx, stmt, &m),
                }
            }
            // The singleton half. See `HirNode::ClassMethodPrepend`. A module
            // that overrides `prepend_features` decides for itself whether the
            // mixin happens at all, and the runtime deferral the instance side
            // uses for that sends `prepend_features` to the CLASS -- the wrong
            // receiver here -- so the pair stays a clean rejection rather than
            // a silently wrong ancestry.
            HirNode::ClassMethodPrepend(m) => {
                let m = m.clone();
                match resolve_module_target(compiler, &m, &child_cref, box_id)? {
                    Some(target) => {
                        // Both hooks take the SINGLETON class as their
                        // argument, which zeo has no compile-time class for --
                        // so a module that defines either stays a clean
                        // rejection instead of being handed the wrong receiver.
                        if compiler.overrides_mixin_primitive(target, "prepend_features")
                            || compiler
                                .class_method_in_chain(target, "prepended")
                                .is_some()
                        {
                            return Err(format!(
                                "`prepend {m}` inside `class << self` isn't supported when {m} \
                                 defines `prepended`/`prepend_features` (zeo limitation: the hook \
                                 takes the singleton class, which zeo cannot name)"
                            ));
                        }
                        compiler.classes[class_id.0 as usize]
                            .class_method_prepends
                            .push(target);
                    }
                    None => defer_in_class_body(compiler, class_id, site_idx, stmt, &m),
                }
            }
            // `IvarWrite`: a bare `@x = expr` in a class body is an ivar on
            // the CLASS OBJECT (`self` in a class body is the class), i.e.
            // the same storage `def self.x; @x; end` reads -- the ordinary
            // way a class-level `@registry = []` gets initialized. Before
            // this it fell into the `_ => {}` arm below and was SILENTLY
            // DROPPED, so the reader saw a bare nil with no diagnostic.
            HirNode::IvarWrite(..) | HirNode::ClassVarWrite(..) | HirNode::ConstWrite { .. } => {
                // The written VALUE may itself be a definition -- `M = (class
                // Inner; 7; end)` defines `Inner` and binds its body's value --
                // so the same registration the fall-through does applies here.
                register_nested_class_defs(compiler, stmt, &child_cref, box_id)?;
                compiler.classes[class_id.0 as usize]
                    .class_body_stmts
                    .push(stmt);
                compiler.class_body_sites[site_idx].stmts.push(stmt);
            }
            // Any OTHER class-body statement -- a method call, conditional,
            // loop, a runtime `define_method` inside an `each`, etc. -- is real
            // code that runs ONCE at class-definition time with `self` = the
            // class object. Collected here (flat list AND this site's own
            // record) and executed at the site's document position.
            _ => {
                // A reachable `C.prepend(M)` / `C.singleton_class.prepend(M)` in
                // a class body (e.g. connection_pool's
                // `Process.singleton_class.prepend(ForkTracker)`) is a static
                // ancestry edit on `C`, independent of the enclosing class --
                // recorded here (resolving names in this body's cref), emitting
                // nothing, exactly as at the top level.
                if try_prepend_call_edit(compiler, stmt, &child_cref, box_id) {
                    continue;
                }
                if declines_a_singleton_prepend(compiler, stmt, &child_cref, box_id) {
                    return Err(UNHONOURED_SINGLETON_PREPEND.into());
                }
                // A `def` nested in an `if`/`case` branch also runs at document
                // position (the taken branch's runtime `define_method` gives the
                // real body), but must ALSO be registered as an own method so
                // `instance_methods`/`extend` -- both resolved at COMPILE time --
                // can see it. This is what lets fileutils' platform-conditional
                // `StreamUtils_#fu_windows?` reach `FileUtils` via `extend`.
                register_conditional_defs(compiler, class_id, &[stmt])?;
                // ...and a `class`/`module` nested in one of those branches --
                // an undecided `if`, a `begin` whose body may raise
                // (rubyntlm's `begin; OpenSSL::Cipher.new("rc4"); rescue;
                // class Rc4`), a block. Registration is a compile-time fact
                // about shape; the marker stays put and the body still runs at
                // its document position, exactly as at the top level.
                register_nested_class_defs(compiler, stmt, &child_cref, box_id)?;
                // ...and the `refine` markers beside those holder modules,
                // which is how power_assert's whole refinement set reaches
                // registration from inside a runtime `if`.
                register_nested_refinements(compiler, class_id, stmt, &child_cref, box_id);
                // ...and, symmetrically, a guarded `undef` (`undef :to_a if
                // respond_to?(:to_a)`, drb) reaches here as a runtime
                // `undef_method` send. Whether it fires is a runtime fact, so
                // the names go on record and codegen stops emitting a DIRECT
                // call for them. See `ClassInfo::runtime_undefs`.
                let undefs = collect_runtime_undefs(compiler, stmt);
                compiler.classes[class_id.0 as usize]
                    .runtime_undefs
                    .extend(undefs);
                compiler.classes[class_id.0 as usize]
                    .class_body_stmts
                    .push(stmt);
                compiler.class_body_sites[site_idx].stmts.push(stmt);
            }
        }
    }
    Ok(())
}

/// A mixin whose module overrides the PRIMITIVE is no longer a compile-time
/// ancestry fact: whether it happens at all is decided at run time. The
/// ancestry itself is handled -- `splice_mixin` writes the overlay chain that
/// `ancestors_of_value` prefers -- but a call folded at COMPILE time would
/// still reach the class's own body, so the module's method names have to
/// leave the fold. That is exactly what `runtime_patches` is for.
fn defer_mixin_to_runtime(compiler: &mut Compiler, module: ClassId) {
    let names: Vec<String> = compiler.classes[module.0 as usize]
        .own_methods
        .iter()
        .map(|&s| compiler.scopes[s.0 as usize].name.clone())
        .collect();
    compiler.runtime_patches.extend(names);
}

/// The next [`crate::compiler::SiteDef::seq`]. The walk visits bodies in the
/// order they run, so a plain counter IS execution order.
fn next_def_seq(compiler: &mut Compiler) -> u32 {
    compiler.def_seq += 1;
    compiler.def_seq
}

/// Files one registered method `Scope` under its class's own-method list --
/// REPLACING any earlier same-name entry rather than appending a shadowed
/// duplicate: real Ruby's last-`def`-wins rule (oracle-verified), which
/// applies identically to a redefinition within one class body and to one
/// arriving via reopening (`method_in_chain` resolves the FIRST name match,
/// so append-only registration would silently keep dispatching the OLD
/// body). Instance and class methods are separate namespaces, hence the
/// separate lists.
///
/// The superseded row is not lost: every registration also lands in
/// `ClassInfo::method_history` in execution order, which is what lets a
/// deferred alias bind the body that existed when it ran. See
/// `mro::resolve_aliases`.
fn add_own_method(
    compiler: &mut Compiler,
    class_id: ClassId,
    sid: crate::compiler::ScopeId,
    is_class_method: bool,
) {
    let seq = next_def_seq(compiler);
    add_own_method_at(compiler, class_id, sid, is_class_method, seq);
}

/// [`add_own_method`] with an explicit history position -- used by
/// `mro::resolve_aliases`, whose clone must sit at the ALIAS's own seq so a
/// later alias of the alias resolves position-correctly.
pub(crate) fn add_own_method_at(
    compiler: &mut Compiler,
    class_id: ClassId,
    sid: crate::compiler::ScopeId,
    is_class_method: bool,
    seq: u32,
) {
    let mname = compiler.scope(sid).name.clone();
    let ci = &compiler.classes[class_id.0 as usize];
    let list = if is_class_method {
        &ci.own_class_methods
    } else {
        &ci.own_methods
    };
    let replaced = list.iter().position(|&s| compiler.scope(s).name == mname);
    // Last-`def`-wins is a fact about `def`s that RAN. A conditional one may
    // not have, so it does not displace a body that always does -- dropping
    // that body here would leave the name answering nothing whenever the guard
    // is false, and the runtime overlay the conditional `def` writes already
    // outranks the static row when the guard IS true.
    let yields = replaced.is_some_and(|i| {
        compiler.scope(sid).runtime_conditional && !compiler.scope(list[i]).runtime_conditional
    });
    let ci = &mut compiler.classes[class_id.0 as usize];
    let list = if is_class_method {
        &mut ci.own_class_methods
    } else {
        &mut ci.own_methods
    };
    match replaced {
        Some(_) if yields => {}
        Some(i) => list[i] = sid,
        None => list.push(sid),
    }
    ci.method_history.push((mname, is_class_method, seq, sid));
}

/// Whether a `def` runs whenever its class body does, or only when a guard zeo
/// cannot decide says so. See [`crate::compiler::Scope::runtime_conditional`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conditional {
    No,
    Yes,
}

/// Registers one class/module-body `def` as an own method: builds its `Scope`
/// and files it under `own_methods`/`own_class_methods`. Shared by the
/// top-level class-body walk and `register_conditional_defs` (a `def` nested in
/// an `if`/`case` branch). A no-op if `stmt` isn't a `DefMethod`.
fn register_body_def_method(
    compiler: &mut Compiler,
    class_id: ClassId,
    stmt: NodeId,
    conditional: Conditional,
) -> Result<(), String> {
    let HirNode::DefMethod {
        name,
        params,
        body,
        is_class_method,
        visibility,
        is_def: _,
    } = &compiler.hir[stmt]
    else {
        return Ok(());
    };
    let (name, params, body, is_class_method, visibility) = (
        name.clone(),
        params.clone(),
        body.clone(),
        *is_class_method,
        *visibility,
    );
    // An OPERATOR definition on a builtin reopen (`class Integer; def +`) is
    // rejected outright (zeo limitation): a native operator fast path is
    // emitted at the static call site AHEAD of the reopened-builtin arm, so
    // the user operator would be silently bypassed there -- a loud rejection
    // beats dispatch that only sometimes honors the override.
    //
    // Only `Int` and `Float` have such a path. `codegen::call::dispatch`
    // emits the four `ops::{INT,FLOAT}_{BINARY,UNARY}_OPS` arms and then the
    // reopened-builtin arm, which is placed deliberately BEFORE the
    // collection, Proc, Regexp and String fast paths precisely so a user
    // redefinition wins there. The poly runtime-checked fallback below it
    // matches the same two `RubyValue::Int`/`Float` shapes and sends
    // everything else through `send_value_in`'s MRO walk, which reads the
    // reopened row. So every OTHER builtin is safe to reopen, and the list
    // here is the MRO of `Integer` and `Float` -- the classes a statically
    // `Int`- or `Float`-typed receiver would consult.
    //
    // A CLASS-method operator is exempt whatever the class: `JSON[str]` is
    // `def self.[]` on a module, and no fast path exists for a call whose
    // receiver is a class object -- those dispatch through the ordinary
    // class-method tables.
    const NUMERIC_FAST_PATH_MRO: &[&str] = &[
        "Integer",
        "Float",
        "Numeric",
        "Comparable",
        "Object",
        "Kernel",
        "BasicObject",
    ];
    if compiler.class(class_id).is_builtin
        && !is_class_method
        && NUMERIC_FAST_PATH_MRO.contains(&compiler.class(class_id).name.as_str())
        && !name.starts_with(|c: char| c.is_alphabetic() || c == '_')
    {
        return Err(format!(
            "defining operator `{name}` on the built-in class `{}` isn't supported yet (zeo limitation: static operator fast paths would bypass it)",
            compiler.class(class_id).name
        ));
    }
    let sid = register_method(
        compiler,
        class_id,
        class_id,
        name.clone(),
        Some(stmt),
        params,
        body,
        visibility,
    )?;
    if conditional == Conditional::Yes {
        compiler.scopes[sid.0 as usize].runtime_conditional = true;
        // Whether this `def` ran is a runtime fact, so every call site for the
        // name has to ask at run time rather than bind to the body emitted
        // here. This is the same de-optimization a runtime `define_method` or
        // `private` gets, and for the same reason.
        compiler.runtime_patches.insert(name);
    }
    add_own_method(compiler, class_id, sid, is_class_method);
    Ok(())
}

/// Registers every `def` reachable through `if`/`case` (`when`) branches in
/// `stmts` as an own method. A conditional `def` still runs at document
/// position via its runtime `define_method` emission (which supplies the taken
/// branch's body); this makes its NAME visible to `instance_methods`/`extend`,
/// which are resolved at compile time and would otherwise miss it. When more
/// than one branch defines the same name, last-wins picks the branch the
/// static target takes (fileutils' RbConfig is a compile-time shim). Loops and
/// blocks are deliberately NOT descended -- a `def` whose branch may never run
/// stays runtime-only, matching CRuby.
fn register_conditional_defs(
    compiler: &mut Compiler,
    class_id: ClassId,
    stmts: &[NodeId],
) -> Result<(), String> {
    for &s in stmts {
        // Clone child bodies before recursing: `register_body_def_method`
        // borrows `compiler` mutably.
        match &compiler.hir[s] {
            HirNode::DefMethod { .. } => {
                register_body_def_method(compiler, class_id, s, Conditional::Yes)?
            }
            HirNode::If {
                then_body,
                else_body,
                ..
            } => {
                let (then_body, else_body) = (then_body.clone(), else_body.clone());
                register_conditional_defs(compiler, class_id, &then_body)?;
                register_conditional_defs(compiler, class_id, &else_body)?;
            }
            HirNode::CaseWhen {
                arms, else_body, ..
            } => {
                let arm_bodies: Vec<Vec<NodeId>> =
                    arms.iter().map(|(_, body)| body.clone()).collect();
                let else_body = else_body.clone();
                for body in &arm_bodies {
                    register_conditional_defs(compiler, class_id, body)?;
                }
                register_conditional_defs(compiler, class_id, &else_body)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Resolves an `include`/`extend`/`prepend` target name to the class/module it
/// mixes in -- or `Ok(None)` when the name is nowhere in the program at all, in
/// which case the caller must let the directive run as ordinary code so the miss
/// surfaces the way Ruby surfaces it.
///
/// Ruby doesn't reject `extend FFI` when `FFI` is undefined: `include`/`extend`/
/// `prepend` are ordinary method calls, so the constant is READ first and raises
/// `NameError` from inside the class body -- and only if the body runs. Reporting
/// it at compile time instead means a program whose module comes from a native
/// extension zeo doesn't have, or from a branch that never executes, can't be
/// built at all. See `defer_unresolved_directive` for the rewrite.
///
/// A name the program DOES assign (`M = Module.new; include M`) stays a hard
/// error: the constant would read back fine at runtime and the mixin would then
/// vanish silently, because a compiled class dispatches off a static MRO that a
/// runtime splice can't reach. Unknown SUPERCLASSES stay loud for the same
/// reason -- zeo has to lay out a struct for one.
fn resolve_module_target(
    compiler: &mut Compiler,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
) -> Result<Option<ClassId>, String> {
    let resolved = compiler
        .resolve_class(name, cref, box_id)
        // A module defined LATER in the flattened list (hoisted deferred
        // require) is created as a forward shell; its methods are added when the
        // real definition reopens the shell (seen at `mro::materialize`).
        .or_else(|| resolve_or_create_lexical(compiler, name, cref, box_id, None));
    let resolved = resolved.or_else(|| resolve_const_alias(compiler, name, cref, box_id));
    match resolved {
        Some(cid) => Ok(Some(cid)),
        None if !compiler.assigns_const_path(name) => Ok(None),
        // A second NAME for a module (`Constants = ::Socket::Constants`) that
        // resolves to nothing zeo compiled: spelled directly, that same include
        // defers to the runtime constant read above, and reaching the module
        // through an alias must not change the answer. The hard error below is
        // for a constant holding a module MINTED at run time (`M =
        // Module.new`), which no static MRO can ever reach.
        None if is_const_path_alias(compiler, name, cref, box_id) => Ok(None),
        None => Err(format!(
            "unknown module `{name}` (must be defined earlier in the file)"
        )),
    }
}

/// The class a `class Name < Super` clause names, resolved the way ruby
/// resolves it: the superclass expression runs BEFORE `Name` is bound, so the
/// class being defined is never a candidate for its own superclass.
///
/// Rails writes, literally, `class SchemaDumper < SchemaDumper` inside
/// `ActiveRecord::ConnectionAdapters`, and means `ActiveRecord::SchemaDumper` --
/// the lexical search skips the unbound inner name and continues OUTWARD. zeo
/// handed back the forward shell it had already minted for
/// `ConnectionAdapters::SchemaDumper` (an earlier adapter file referenced it),
/// which made the class its own parent. `materialize_class_methods` then walked
/// that superclass chain forever: `require "active_record"` ran 12 minutes,
/// climbed past 5GB and was killed with no diagnostic.
fn resolve_superclass(
    compiler: &mut Compiler,
    superclass: &str,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
) -> Option<ClassId> {
    // The one name that is off-limits, fixed BEFORE the search narrows: it is
    // the path this definition binds, not whatever the current scope would bind.
    let defining = match cref.last() {
        Some(&enclosing) => format!("{}::{name}", compiler.fq_name(enclosing)),
        None => name.to_string(),
    };
    let mut scopes = cref;
    loop {
        let found = compiler
            .resolve_class(superclass, scopes, box_id)
            // A superclass defined LATER in the flattened list (a hoisted
            // deferred require whose subclass precedes its base, e.g. `class
            // MismatchedChecksumError < Error` before `class Error`): create
            // the base as a forward shell, resolving the bare name lexically.
            // `defining` is off limits there for the same reason the loop
            // below skips past it -- and it must never be CREATED either, or
            // the class exists even when the caller goes on to defer the
            // whole definition.
            .or_else(|| {
                resolve_or_create_lexical(compiler, superclass, scopes, box_id, Some(&defining))
            })
            // A superclass named through a constant ALIAS (`Base =
            // Some::Other::Class`) is that class.
            .or_else(|| resolve_const_alias(compiler, superclass, scopes, box_id));
        match found {
            Some(cid) if !scopes.is_empty() && compiler.fq_name(cid) == defining => {
                scopes = &scopes[..scopes.len() - 1];
            }
            other => return other,
        }
    }
}

/// Whether `name` is a constant ALIAS naming another constant path -- true even
/// when that path names nothing zeo compiled. See `resolve_const_alias`.
fn is_const_path_alias(compiler: &Compiler, name: &str, cref: &[ClassId], box_id: u32) -> bool {
    let Some(&value) = lexical_const_alias(compiler, name, cref, box_id) else {
        return false;
    };
    matches!(
        &compiler.hir[value],
        HirNode::ClassRef(_) | HirNode::QualifiedConstRead(..)
    )
}

/// A constant that ALIASES a class or module (oauth2's `FilteredAttributes =
/// OAuth2::AUTH_SANITIZER::FilteredAttributes`), resolved to what it names.
///
/// Ruby has no separate alias form -- naming a module twice IS assigning its
/// value to a second constant -- and the second name then works everywhere the
/// first does: `include`, a superclass clause, `.new`. Statically it is just a
/// second name for the same `ClassId`, which is exactly what a compiled MRO can
/// carry. The assignment itself still emits, so reading the alias back still
/// answers the module object.
///
/// The alias is looked up the way ruby looks up the name that reads it:
/// innermost enclosing scope first, then outward, then the top level -- a write
/// in some unrelated namespace is never what this name means (the rule
/// `const_alias_target`'s docs were bought with 30 gems). A chain of aliases
/// follows through, capped so a cycle (`A = B; B = A`) terminates. A value that
/// is anything but a constant path answers `None`: `M = Module.new` mints a
/// module at run time, which no static MRO can reach, and that stays the loud
/// error `resolve_module_target` documents.
fn resolve_const_alias(
    compiler: &mut Compiler,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
) -> Option<ClassId> {
    let mut name = name.to_string();
    for _ in 0..8 {
        let value = *lexical_const_alias(compiler, &name, cref, box_id)?;
        // `::Socket::Constants` lowers with the explicit scope `Object` (see
        // `constant_path_scope_and_name`), which is how the source anchored it:
        // resolve the rest at the top level, not against this cref.
        let written = match &compiler.hir[value] {
            HirNode::ClassRef(target) => target.clone(),
            HirNode::QualifiedConstRead(scope, target) => format!("{scope}::{target}"),
            _ => return None,
        };
        // Anchored spellings resolve at the TOP level, not against this cref:
        // `::Socket::Constants` keeps its leading `::`, while a plain `::Real`
        // arrives with the explicit scope `Object` (see
        // `constant_path_scope_and_name`). Both say the same thing.
        let (target, from) = match written
            .strip_prefix("::")
            .or(written.strip_prefix("Object::"))
        {
            Some(anchored) => (anchored.to_string(), &[][..]),
            None => (written, cref),
        };
        // The same two-step every other resolution site takes: a name defined
        // later in the flattened require graph is a forward shell, not a miss.
        let resolved = compiler
            .resolve_class(&target, from, box_id)
            .or_else(|| resolve_or_create_lexical(compiler, &target, from, box_id, None));
        if let Some(cid) = resolved {
            return Some(cid);
        }
        name = target;
    }
    None
}

/// The `NAME = <value>` write that `name` reads from `cref`, searched
/// innermost-scope-outward. `Scope::NAME` (already qualified) and `::NAME`
/// (anchored at the top) ask exactly one question each.
fn lexical_const_alias<'a>(
    compiler: &'a Compiler,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
) -> Option<&'a NodeId> {
    let path = crate::constpath::ConstPath::parse(name);
    if !path.is_bare() {
        return compiler
            .const_aliases
            .get(&(box_id, path.unanchored().to_string()));
    }
    (0..=cref.len()).rev().find_map(|depth| {
        let key = match depth {
            0 => path.base().to_string(),
            d => format!("{}::{}", compiler.fq_name(cref[d - 1]), path.base()),
        };
        compiler.const_aliases.get(&(box_id, key))
    })
}

/// Read-only scan populating [`Compiler::const_aliases`]: every `NAME = <...>`
/// write in the program, recorded under the fully-qualified name it binds, so
/// the lookup above can ask about a scope rather than a bare leaf. Descends the
/// same wrappers `collect_shell_kinds` does, and for the same reason -- a write
/// inside a conditional still binds the name.
fn collect_const_aliases(
    hir: &Hir,
    stmts: &[NodeId],
    scope: &[String],
    box_id: u32,
    out: &mut HashMap<(u32, String), NodeId>,
) {
    for &id in stmts {
        match &hir[id] {
            HirNode::ConstWrite {
                scope: at,
                name,
                value,
            } => {
                let mut path = scope.to_vec();
                if let Some(at) = at {
                    path.push(at.clone());
                }
                path.push(name.clone());
                // First write wins, matching `collect_top_level_const_aliases`:
                // a later reassignment does not change what the name meant to
                // the definitions that already read it.
                out.entry((box_id, path.join("::"))).or_insert(*value);
            }
            HirNode::ClassDef { name, body, .. } => {
                let mut inner = scope.to_vec();
                inner.push(name.clone());
                collect_const_aliases(hir, body, &inner, box_id, out);
            }
            HirNode::If {
                then_body,
                else_body,
                ..
            } => {
                collect_const_aliases(hir, then_body, scope, box_id, out);
                collect_const_aliases(hir, else_body, scope, box_id, out);
            }
            HirNode::BoxScope { box_id: bx, body } => {
                collect_const_aliases(hir, body, &[], *bx, out);
            }
            HirNode::Seq(body) | HirNode::PreExec(body) | HirNode::Eval(body) => {
                collect_const_aliases(hir, body, scope, box_id, out);
            }
            _ => {}
        }
    }
}

/// Turns an `include`/`extend`/`prepend` whose target `resolve_module_target`
/// couldn't find into the bare constant READ Ruby evaluates in that argument
/// position, rewritten IN PLACE so it keeps the directive's own source span --
/// which is what puts the `NameError` on `include`'s line, inside
/// `<module:Sock>`, with `<main>` below it.
///
/// The read is all that's needed: it always raises (nothing in the program
/// assigns the name, or `resolve_module_target` would have errored), so no
/// mixin is lost by not emitting the send itself.
fn defer_unresolved_directive(compiler: &mut Compiler, stmt: NodeId, name: &str) {
    compiler.hir[stmt] = HirNode::ClassRef(name.to_string());
}

/// Files the lexical range a `using M` covers -- from the statement's own
/// span to `end`. A module that resolves nowhere records nothing: with no
/// refinements to activate there is nothing for a call site to consult, and
/// the unresolved name is already whatever error the program deserves.
fn record_activation(
    compiler: &mut Compiler,
    stmt: NodeId,
    module: &str,
    cref: &[ClassId],
    box_id: u32,
    end: u32,
) {
    let (Some(module), Some(span)) = (
        compiler.resolve_class(module, cref, box_id),
        compiler.hir.span(stmt).and_then(|s| s.known()),
    ) else {
        return;
    };
    compiler.activations.push(crate::compiler::Activation {
        module,
        file: span.file,
        start: span.start,
        end,
    });
}

/// [`defer_unresolved_directive`] for a directive inside a `class`/`module`
/// body, filed as an ordinary body statement so it runs at the site's document
/// position -- the same treatment `walk_class_body`'s catch-all arm gives any
/// other class-body expression.
fn defer_in_class_body(
    compiler: &mut Compiler,
    class_id: ClassId,
    site_idx: usize,
    stmt: NodeId,
    name: &str,
) {
    defer_unresolved_directive(compiler, stmt, name);
    compiler.classes[class_id.0 as usize]
        .class_body_stmts
        .push(stmt);
    compiler.class_body_sites[site_idx].stmts.push(stmt);
}

/// Registers one method BODY (a class/module's own literal `def`, or a
/// winning ancestor's body being materialized onto a descendant -- see
/// `mro::materialize_methods`/`materialize_class_methods`) as a fresh
/// `Scope`, running the full per-method analysis pipeline (local-type
/// inference, named-`*rest`/`**kwrest`/`&block`-param type seeding,
/// bare-`yield`/`block_given?` scanning).
/// `owner` is whichever class/module this Scope is filed under (and, for a
/// materialized method, whose concrete struct it'll be generated into);
/// One method scope's whole-scope local-type map: the body walk, the
/// parameter defaults, and the parameter shapes that carry a type of their own.
/// Shared by `register_method` and [`mro::reinfer_local_types`], which re-runs
/// it once the class tables are final -- codegen reads this map, so the two
/// must produce the same answer.
pub(crate) fn method_local_types(
    compiler: &Compiler,
    defining_class: ClassId,
    params: &Params,
    body: &[NodeId],
) -> HashMap<String, TyKind> {
    let defining_box = compiler.class(defining_class).box_id;
    let mut local_types = locals::infer_locals(compiler, Some(defining_class), defining_box, body);
    for id in params.default_ids() {
        locals::track_extra(
            compiler,
            Some(defining_class),
            defining_box,
            &mut local_types,
            id,
        );
    }
    if let Some(Some(n)) = &params.rest {
        local_types.entry(n.clone()).or_insert(TyKind::Array);
    }
    if let Some(Some(n)) = &params.keyword_rest {
        local_types.entry(n.clone()).or_insert(TyKind::Hash);
    }
    if let Some(Some(n)) = &params.block {
        local_types.entry(n.clone()).or_insert(TyKind::Proc);
    }
    // A parameter arrives through the Rust signature as a `RubyValue`, so a
    // body assignment can never narrow it to an unboxed `Arc<Concrete>`: every
    // read BEFORE that assignment still sees the signature's binding.
    // `source_uri = Gem::Uri.new(source_uri)` is the shape -- rubygems rebinds
    // a parameter to a wrapper built FROM it, and the argument read would
    // otherwise be boxed as though it were already the wrapper.
    for n in params.bound_names() {
        if matches!(local_types.get(&n), Some(TyKind::Object(_))) {
            local_types.insert(n, TyKind::Poly);
        }
    }
    local_types
}

/// `defining_class` is whichever class/module's HIR body `params`/`body`
/// actually came from -- equal to `owner` for an ordinary own-body method,
/// an ancestor otherwise (see `compiler::Scope::defining_class`'s docs).
#[allow(clippy::too_many_arguments)] // one fact per parameter; a bundle struct would just rename them
fn register_method(
    compiler: &mut Compiler,
    owner: ClassId,
    defining_class: ClassId,
    name: String,
    def_node: Option<NodeId>,
    params: Params,
    body: Vec<NodeId>,
    visibility: Visibility,
) -> Result<crate::compiler::ScopeId, String> {
    // Deferred to `mro::reinfer_local_types`, which recomputes every scope
    // against the finished class tables anyway and is the first thing to read
    // the map -- inferring here too would only be thrown away.
    let local_types = HashMap::new();
    let mut uses_bare_block = false;
    for &n in &body {
        if scan_bare_block_use(&compiler.hir, n) {
            uses_bare_block = true;
        }
    }
    // A same-body `alias` clones its source's `DefMethod`; the clone is what
    // carries the birth name, and materializing this method onto a descendant
    // reuses the same node, so the alias stays an alias all the way down.
    let alias_of = def_node.and_then(|n| compiler.hir.alias_origin(n).map(str::to_string));
    let accessor = accessor_shape(&compiler.hir, def_node, &params, &body);
    // A def from a constant-bearing `class << self` body: its lexical home
    // is the singleton's surrogate, registered just before it in the same
    // body walk (lowering pushes the surrogate `ClassDef` first).
    let lexical_home = def_node
        .filter(|n| compiler.hir.singleton_body_defs.contains(n))
        .and_then(|_| {
            compiler.classes.iter().position(|c| {
                c.lexical_parent == Some(defining_class)
                    && c.name == crate::compiler::SINGLETON_SURROGATE
            })
        })
        .map(|i| ClassId(i as u32));
    Ok(compiler.push_scope(Scope {
        name,
        class: Some(owner),
        defining_class,
        lexical_home,
        def_node,
        alias_of,
        params,
        body,
        local_types,
        uses_bare_block,
        visibility,
        // Ordinary methods are never native defaults; the bootstrap-marking
        // pass and `mro` set this true for the pristine exception bodies.
        native_default: false,
        // Set by `register_conditional_defs`, the one caller that registers a
        // `def` whose branch may not run.
        runtime_conditional: false,
        accessor,
    }))
}

/// The `AccessorShape` of a method whose entire body is one ivar access, or
/// `None` for everything else.
///
/// Matched on the HIR SHAPE, so `attr_reader :x` and a hand-written
/// `def x; @x; end` are indistinguishable here -- which is the point, since
/// the benchmarks that pay the most for a virtual accessor call
/// (`bm_rbtree`, `bm_inline`, `bm_linked_list`) write the second form. It
/// also means the property is preserved by `mro::materialize_methods`, which
/// copies the same body nodes onto every descendant.
///
/// Deliberately strict: a block parameter, a default, a splat, a keyword, or
/// any second statement all disqualify. An accessor's whole value is that the
/// call has NOTHING else in it, so a near-miss is worth nothing and only
/// widens what the devirtualized paths must reproduce.
fn accessor_shape(
    hir: &Hir,
    def_node: Option<NodeId>,
    params: &Params,
    body: &[NodeId],
) -> Option<AccessorShape> {
    let [only] = body else { return None };
    let plain = params.optional.is_empty()
        && params.rest.is_none()
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && params.block.is_none()
        && params.destructures.is_empty();
    if !plain {
        return None;
    }
    let (ivar, kind) = match &hir[*only] {
        HirNode::IvarRead(name) if params.required.is_empty() => (name, AccessorKind::Reader),
        // `@x = v` EVALUATES to `v`, and so does the Ruby call -- a writer
        // method's return value is its argument, not the assignment's
        // receiver -- so replacing the call with the write loses nothing.
        HirNode::IvarWrite(name, value) => match (&params.required[..], &hir[*value]) {
            ([p], HirNode::LocalRead(v)) if p == v => (name, AccessorKind::Writer),
            _ => return None,
        },
        _ => return None,
    };
    Some(AccessorShape {
        ivar: ivar.clone(),
        kind,
        // An `alias` CLONES the `DefMethod` into a fresh node, so an alias of
        // an `attr_reader` reads as hand-written here. That is the safe
        // direction (it only keeps today's call shape while tracing) and it
        // matches CRuby, which gives the alias its own method entry.
        attr_generated: def_node.is_some_and(|n| hir.attr_generated.contains(&n)),
    })
}

/// Scans a method's own control flow (NOT descending into a nested `Block`'s
/// body -- see below) for a bare `yield`/`block_given?`, returning whether
/// any was found. Mirrors `collect_ivars`'s traversal shape.
///
/// Descends into nested block and lambda literals too: their
/// `yield`/`block_given?` refers to THIS enclosing method's implicit block
/// in real Ruby (blocks and lambdas have none of their own), so a method
/// whose only `yield` sits inside a `.each { ... }` still needs its
/// `__blk` parameter -- and the emitted closure clone-captures it (see
/// `codegen::call::emit_proc_or_lambda_value`).
fn scan_bare_block_use(hir: &Hir, id: NodeId) -> bool {
    match &hir[id] {
        // An FFI wrapper body uses no block.
        HirNode::Ffi(_) => false,
        HirNode::Yield(_) | HirNode::BlockGiven => true,
        HirNode::IvarWrite(_, value)
        | HirNode::LocalWrite(_, value)
        | HirNode::ClassVarWrite(_, value) => scan_bare_block_use(hir, *value),
        HirNode::And(l, r)
        | HirNode::Or(l, r)
        | HirNode::FlipFlop {
            state: _,
            left: l,
            right: r,
            exclusive: _,
        } => {
            scan_bare_block_use(hir, *l) || scan_bare_block_use(hir, *r)
        }
        HirNode::Defined(v) => scan_bare_block_use(hir, *v),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            scan_bare_block_use(hir, *cond)
                || scan_bare_block_use_body(hir, then_body)
                || scan_bare_block_use_body(hir, else_body)
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            let mut found = false;
            if let Some(s) = subject {
                found |= scan_bare_block_use(hir, *s);
            }
            for (values, body) in arms {
                for e in values {
                    let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                    found |= scan_bare_block_use(hir, *v);
                }
                found |= scan_bare_block_use_body(hir, body);
            }
            found || scan_bare_block_use_body(hir, else_body)
        }
        HirNode::While { cond, body, .. } => {
            scan_bare_block_use(hir, *cond) || scan_bare_block_use_body(hir, body)
        }
        HirNode::Loop { body } => scan_bare_block_use_body(hir, body),
        HirNode::For { target, iterable, body } => {
            let mut found = scan_bare_block_use(hir, *iterable);
            let mut ids = Vec::new();
            target.for_each_node(&mut |n| ids.push(n));
            for n in ids {
                found |= scan_bare_block_use(hir, n);
            }
            found || scan_bare_block_use_body(hir, body)
        }
        HirNode::Call {
            receiver,
            args,
            kwargs,
            block,
            block_arg,
            ..
        } => {
            let mut found = false;
            if let Some(r) = receiver {
                found |= scan_bare_block_use(hir, *r);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                found |= scan_bare_block_use(hir, *n);
            }
            for n in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                found |= scan_bare_block_use(hir, n);
            }
            if let Some(b) = block_arg {
                found |= scan_bare_block_use(hir, *b);
            }
            // A nested block literal's `yield`/`block_given?` refers to THIS
            // enclosing method's block in real Ruby (blocks have no implicit
            // block of their own) -- counted here so the method gets its
            // `__blk` param, which the emitted closure then clone-captures
            // (see `codegen::call::emit_proc_or_lambda_value`).
            if let Some(b) = block
                && let HirNode::Block { body, .. } = &hir[*b] {
                    found |= scan_bare_block_use_body(hir, body);
                }
            found
        }
        HirNode::New { args, .. } => {
            let mut found = false;
            for &a in args {
                found |= scan_bare_block_use(hir, a);
            }
            found
        }
        HirNode::Raise(args, cause) => {
            let mut found = false;
            for &a in args.iter().chain(crate::hir::raise_cause_node(cause).iter()) {
                found |= scan_bare_block_use(hir, a);
            }
            found
        }
        HirNode::SuperCall { args, kwargs, block, .. } => {
            match block {
                // A literal `super { ... }` block's own `yield` refers to
                // THIS method's block, same as any nested block literal.
                Some(b) => {
                    let mut found = args.iter().any(|a| scan_bare_block_use(hir, a.node_id()))
                        || kwargs
                            .iter()
                            .flat_map(|kw| kw.node_ids())
                            .any(|a| scan_bare_block_use(hir, a));
                    if let HirNode::Block { body, .. } = &hir[*b] {
                        found |= scan_bare_block_use_body(hir, body);
                    }
                    found
                }
                // No literal block: real Ruby forwards the current method's
                // own block to the parent, whose body may `yield` it -- the
                // splice references `__blk` directly (see
                // `codegen::call::emit_super`), so this method needs
                // the parameter whether or not the parent turns out to use
                // it (an unused `Option` costs nothing).
                None => true,
            }
        }
        HirNode::ArrayLit(elems) => {
            let mut found = false;
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                found |= scan_bare_block_use(hir, *n);
            }
            found
        }
        HirNode::HashLit(pairs) => {
            let mut found = false;
            for n in pairs.iter().flat_map(|kw| kw.node_ids()) {
                found |= scan_bare_block_use(hir, n);
            }
            found
        }
        HirNode::RangeLit { start, end, .. } => {
            let mut found = false;
            if let Some(s) = start {
                found |= scan_bare_block_use(hir, *s);
            }
            if let Some(e) = end {
                found |= scan_bare_block_use(hir, *e);
            }
            found
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            let mut found = false;
            for p in parts {
                if let StrPart::Interp(n) = p {
                    found |= scan_bare_block_use(hir, *n);
                }
            }
            found
        }
        HirNode::MultiWrite { targets, value } => {
            let mut found = scan_bare_block_use(hir, *value);
            let mut ids = Vec::new();
            targets.for_each_node(&mut |n| ids.push(n));
            for n in ids {
                found |= scan_bare_block_use(hir, n);
            }
            found
        }
        HirNode::GlobalWrite(_, value) => scan_bare_block_use(hir, *value),
        HirNode::ConstWrite { value, .. } => scan_bare_block_use(hir, *value),
        HirNode::DynConstRead { scope, .. } => scan_bare_block_use(hir, *scope),
        HirNode::DynConstWrite { scope, value, .. } => {
            scan_bare_block_use(hir, *scope) | scan_bare_block_use(hir, *value)
        }
        HirNode::PreExec(body) | HirNode::Seq(body) | HirNode::Eval(body) | HirNode::BoxScope { body, .. } => scan_bare_block_use_body(hir, body),
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => match v {
            Some(v) => scan_bare_block_use(hir, *v),
            None => false,
        },
        HirNode::CaseIn { subject, arms, else_body } => {
            let mut found = scan_bare_block_use(hir, *subject);
            for arm in arms {
                found |= scan_bare_block_use_pattern_arm(hir, arm);
            }
            if let Some(body) = else_body {
                found |= scan_bare_block_use_body(hir, body);
            }
            found
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            scan_bare_block_use(hir, *subject) || scan_bare_block_use_pattern(hir, pattern)
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let mut found = scan_bare_block_use_body(hir, body);
            for r in rescues {
                found |= scan_bare_block_use_body(hir, &r.body);
            }
            if let Some(b) = else_body {
                found |= scan_bare_block_use_body(hir, b);
            }
            if let Some(b) = ensure_body {
                found |= scan_bare_block_use_body(hir, b);
            }
            found
        }
        // A lambda is its own separate scope for locals, but a bare
        // `yield`/`block_given?` lexically inside one still refers to THIS
        // enclosing method's block in real Ruby, same as inside an ordinary
        // nested block literal -- counted for the same reason as `Call`'s
        // own block-literal recursion above.
        HirNode::Lambda { body, .. } => scan_bare_block_use_body(hir, body),
        HirNode::Retry => false,
        HirNode::Redo
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
        | HirNode::SelfRef
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::ConstantVisibility { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::DefHook { .. }
        | HirNode::MethodRedefine { .. }
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => false,
    }
}

/// Every `NodeId` embedded in `pattern` (see `Pattern::for_each_node`), OR'd
/// through `scan_bare_block_use`.
fn scan_bare_block_use_pattern(hir: &Hir, pattern: &Pattern) -> bool {
    let mut ids = Vec::new();
    pattern.for_each_node(&mut |n| ids.push(n));
    let mut found = false;
    for n in ids {
        found |= scan_bare_block_use(hir, n);
    }
    found
}

fn scan_bare_block_use_pattern_arm(hir: &Hir, arm: &PatternArm) -> bool {
    let mut found = scan_bare_block_use_pattern(hir, &arm.pattern);
    if let Some((g, _)) = arm.guard {
        found |= scan_bare_block_use(hir, g);
    }
    found |= scan_bare_block_use_body(hir, &arm.body);
    found
}

pub(crate) fn scan_bare_block_use_body(hir: &Hir, body: &[NodeId]) -> bool {
    let mut found = false;
    for &n in body {
        found |= scan_bare_block_use(hir, n);
    }
    found
}

/// Whether a method body (or anything nested under it -- blocks and
/// lambdas included, since a `super` written inside one still targets the
/// ENCLOSING method) contains a `super` call. Mirrors
/// `scan_bare_block_use`'s traversal arm-for-arm; consumed by codegen's
/// super-reachability analysis (which method NAMES need dynamic-self
/// `super`-target bridges).
pub(crate) fn scan_contains_super(hir: &Hir, id: NodeId) -> bool {
    match &hir[id] {
        // An FFI wrapper body uses no block.
        HirNode::Ffi(_) => false,
        HirNode::BlockGiven => false,
        HirNode::Yield(args) => args.iter().any(|e| match e {
            crate::hir::ArrayElem::Single(a) | crate::hir::ArrayElem::Splat(a) => {
                scan_contains_super(hir, *a)
            }
        }),
        HirNode::IvarWrite(_, value)
        | HirNode::LocalWrite(_, value)
        | HirNode::ClassVarWrite(_, value) => scan_contains_super(hir, *value),
        HirNode::And(l, r)
        | HirNode::Or(l, r)
        | HirNode::FlipFlop {
            state: _,
            left: l,
            right: r,
            exclusive: _,
        } => {
            scan_contains_super(hir, *l) || scan_contains_super(hir, *r)
        }
        HirNode::Defined(v) => scan_contains_super(hir, *v),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            scan_contains_super(hir, *cond)
                || scan_contains_super_body(hir, then_body)
                || scan_contains_super_body(hir, else_body)
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            let mut found = false;
            if let Some(s) = subject {
                found |= scan_contains_super(hir, *s);
            }
            for (values, body) in arms {
                for e in values {
                    let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                    found |= scan_contains_super(hir, *v);
                }
                found |= scan_contains_super_body(hir, body);
            }
            found || scan_contains_super_body(hir, else_body)
        }
        HirNode::While { cond, body, .. } => {
            scan_contains_super(hir, *cond) || scan_contains_super_body(hir, body)
        }
        HirNode::Loop { body } => scan_contains_super_body(hir, body),
        HirNode::For { target, iterable, body } => {
            let mut found = scan_contains_super(hir, *iterable);
            let mut ids = Vec::new();
            target.for_each_node(&mut |n| ids.push(n));
            for n in ids {
                found |= scan_contains_super(hir, n);
            }
            found || scan_contains_super_body(hir, body)
        }
        HirNode::Call {
            receiver,
            args,
            kwargs,
            block,
            block_arg,
            ..
        } => {
            let mut found = false;
            if let Some(r) = receiver {
                found |= scan_contains_super(hir, *r);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                found |= scan_contains_super(hir, *n);
            }
            for n in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                found |= scan_contains_super(hir, n);
            }
            if let Some(b) = block_arg {
                found |= scan_contains_super(hir, *b);
            }
            // A nested block literal's `yield`/`block_given?` refers to THIS
            // enclosing method's block in real Ruby (blocks have no implicit
            // block of their own) -- counted here so the method gets its
            // `__blk` param, which the emitted closure then clone-captures
            // (see `codegen::call::emit_proc_or_lambda_value`).
            if let Some(b) = block
                && let HirNode::Block { body, .. } = &hir[*b] {
                    found |= scan_contains_super_body(hir, body);
                }
            found
        }
        HirNode::New { args, .. } => {
            let mut found = false;
            for &a in args {
                found |= scan_contains_super(hir, a);
            }
            found
        }
        HirNode::Raise(args, cause) => {
            let mut found = false;
            for &a in args.iter().chain(crate::hir::raise_cause_node(cause).iter()) {
                found |= scan_contains_super(hir, a);
            }
            found
        }
        HirNode::SuperCall { .. } => true,
        HirNode::ArrayLit(elems) => {
            let mut found = false;
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                found |= scan_contains_super(hir, *n);
            }
            found
        }
        HirNode::HashLit(pairs) => {
            let mut found = false;
            for n in pairs.iter().flat_map(|kw| kw.node_ids()) {
                found |= scan_contains_super(hir, n);
            }
            found
        }
        HirNode::RangeLit { start, end, .. } => {
            let mut found = false;
            if let Some(s) = start {
                found |= scan_contains_super(hir, *s);
            }
            if let Some(e) = end {
                found |= scan_contains_super(hir, *e);
            }
            found
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            let mut found = false;
            for p in parts {
                if let StrPart::Interp(n) = p {
                    found |= scan_contains_super(hir, *n);
                }
            }
            found
        }
        HirNode::MultiWrite { targets, value } => {
            let mut found = scan_contains_super(hir, *value);
            let mut ids = Vec::new();
            targets.for_each_node(&mut |n| ids.push(n));
            for n in ids {
                found |= scan_contains_super(hir, n);
            }
            found
        }
        HirNode::GlobalWrite(_, value) => scan_contains_super(hir, *value),
        HirNode::ConstWrite { value, .. } => scan_contains_super(hir, *value),
        HirNode::DynConstRead { scope, .. } => scan_contains_super(hir, *scope),
        HirNode::DynConstWrite { scope, value, .. } => {
            scan_contains_super(hir, *scope) | scan_contains_super(hir, *value)
        }
        HirNode::PreExec(body) | HirNode::Seq(body) | HirNode::Eval(body) | HirNode::BoxScope { body, .. } => scan_contains_super_body(hir, body),
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => match v {
            Some(v) => scan_contains_super(hir, *v),
            None => false,
        },
        HirNode::CaseIn { subject, arms, else_body } => {
            let mut found = scan_contains_super(hir, *subject);
            for arm in arms {
                found |= scan_contains_super_pattern_arm(hir, arm);
            }
            if let Some(body) = else_body {
                found |= scan_contains_super_body(hir, body);
            }
            found
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            scan_contains_super(hir, *subject) || scan_contains_super_pattern(hir, pattern)
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            let mut found = scan_contains_super_body(hir, body);
            for r in rescues {
                found |= scan_contains_super_body(hir, &r.body);
            }
            if let Some(b) = else_body {
                found |= scan_contains_super_body(hir, b);
            }
            if let Some(b) = ensure_body {
                found |= scan_contains_super_body(hir, b);
            }
            found
        }
        // A lambda is its own separate scope for locals, but a bare
        // `yield`/`block_given?` lexically inside one still refers to THIS
        // enclosing method's block in real Ruby, same as inside an ordinary
        // nested block literal -- counted for the same reason as `Call`'s
        // own block-literal recursion above.
        HirNode::Lambda { body, .. } => scan_contains_super_body(hir, body),
        HirNode::Retry => false,
        HirNode::Redo
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
        | HirNode::SelfRef
        | HirNode::LocalRead(_)
        | HirNode::IvarRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::Undef(_)
        | HirNode::ClassMethodUndef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::ClassMethodVisibility { .. }
        | HirNode::ModuleFunction(_)
        | HirNode::ConstantVisibility { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::ClassMethodPrepend(_)
        | HirNode::Refine { .. }
        | HirNode::Using(_)
        | HirNode::DefHook { .. }
        | HirNode::MethodRedefine { .. }
        | HirNode::ClassDef { .. }
        | HirNode::DefMethod { .. } => false,
    }
}

/// Every `NodeId` embedded in `pattern` (see `Pattern::for_each_node`), OR'd
/// through `scan_contains_super`.
fn scan_contains_super_pattern(hir: &Hir, pattern: &Pattern) -> bool {
    let mut ids = Vec::new();
    pattern.for_each_node(&mut |n| ids.push(n));
    let mut found = false;
    for n in ids {
        found |= scan_contains_super(hir, n);
    }
    found
}

fn scan_contains_super_pattern_arm(hir: &Hir, arm: &PatternArm) -> bool {
    let mut found = scan_contains_super_pattern(hir, &arm.pattern);
    if let Some((g, _)) = arm.guard {
        found |= scan_contains_super(hir, g);
    }
    found |= scan_contains_super_body(hir, &arm.body);
    found
}

pub(crate) fn scan_contains_super_body(hir: &Hir, body: &[NodeId]) -> bool {
    let mut found = false;
    for &n in body {
        found |= scan_contains_super(hir, n);
    }
    found
}

/// Recursively scans a method body for `@ivar` reads/writes so the class's
/// `ruby_class!` invocation knows which fields to declare. Mirrors zeo's
/// ivar-registration passes, minus the whole-program fixpoint (a single
/// bottom-up scan is enough here because ivar *names* -- unlike ivar
/// *types* -- don't depend on inference, only on which `@name` tokens
/// appear).
/// An ivar named by a multi-assignment or `for` TARGET -- `@a, @b = 1, 2` and
/// `for @x in ...`, where the name appears nowhere else in the body. Such an
/// ivar still needs a struct field: `emit_target_write` lowers the write to
/// `self.<name>.lock()` regardless. Collecting it only through
/// `MultiTarget::for_each_node` missed it entirely (an ivar target embeds no
/// sub-expression, so that traversal yields nothing), and the write then
/// referenced a field that was never declared.
fn collect_ivar_target(target: &crate::hir::MultiTarget, out: &mut Vec<String>) {
    if let crate::hir::MultiTarget::Ivar(name) = target
        && !out.contains(name)
    {
        out.push(name.clone());
    }
}

pub(crate) fn collect_ivars(hir: &Hir, id: NodeId, out: &mut Vec<String>) {
    let mut record = |name: &str| {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    };
    match &hir[id] {
        // An FFI wrapper body reads only its synthetic parameters.
        HirNode::Ffi(_) => return,
        // A `class`/`def` body is a fresh Ruby scope, scanned under its own
        // owner rather than the one this walk is filling.
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => return,
        HirNode::IvarRead(name) | HirNode::IvarWrite(name, _) => record(name),
        HirNode::For { target, .. } => target.for_each_target(&mut |t| collect_ivar_target(t, out)),
        HirNode::MultiWrite { targets, .. } => {
            targets.for_each_target(&mut |t| collect_ivar_target(t, out))
        }
        _ => {}
    }
    hir[id].for_each_child(&mut |n| collect_ivars(hir, n, out));
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn analyze_src(src: &str) -> Analyzed {
        let (hir, root) = crate::parse::parse_and_lower(src).expect("parse");
        analyze(hir, root).expect("analyze")
    }

    pub(super) fn analyze_err(src: &str) -> String {
        let (hir, root) = crate::parse::parse_and_lower(src).expect("parse");
        match analyze(hir, root) {
            Ok(_) => panic!("expected an analyze error"),
            Err(e) => e.to_string(),
        }
    }

    pub(super) fn class_named(a: &Analyzed, name: &str) -> ClassId {
        a.compiler
            .resolve_class(name, &[], 0)
            .unwrap_or_else(|| panic!("class `{name}` not registered"))
    }

    /// Reopening merges into ONE `ClassInfo` (an earlier version pushed a
    /// shadowed duplicate whose members never dispatched).
    #[test]
    fn reopening_merges_into_the_existing_class() {
        let a = analyze_src(
            "class Foo\n  def a\n    1\n  end\nend\nclass Foo\n  def b\n    2\n  end\nend\n",
        );

        let dupes = a
            .compiler
            .classes
            .iter()
            .filter(|c| c.name == "Foo")
            .count();
        assert_eq!(dupes, 1, "one merged registration, not a shadowed pair");

        let ci = a.compiler.class(class_named(&a, "Foo"));
        let mut names: Vec<&str> = ci
            .own_methods
            .iter()
            .map(|&s| a.compiler.scope(s).name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["a", "b"]);
    }

    /// Real Ruby's last-`def`-wins rule (oracle-verified), across reopen
    /// AND within a single class body: the retained scope must be the
    /// LATER definition, distinguishable here by its parameter shape.
    #[test]
    fn redefinition_keeps_the_later_body() {
        for src in [
            // Across a reopen...
            "class Foo\n  def a\n    1\n  end\nend\nclass Foo\n  def a(x)\n    x\n  end\nend\n",
            // ...and within one body.
            "class Foo\n  def a\n    1\n  end\n  def a(x)\n    x\n  end\nend\n",
        ] {
            let a = analyze_src(src);
            let ci = a.compiler.class(class_named(&a, "Foo"));
            let sids: Vec<_> = ci
                .own_methods
                .iter()
                .filter(|&&s| a.compiler.scope(s).name == "a")
                .collect();
            assert_eq!(sids.len(), 1, "no shadowed duplicate entry");
            assert_eq!(
                a.compiler.scope(*sids[0]).params.required.len(),
                1,
                "the LATER def (the one taking a param) won"
            );
        }
    }

    /// Instance and class methods are separate namespaces: a class method
    /// must never replace a same-named instance method.
    #[test]
    fn class_and_instance_methods_do_not_replace_each_other() {
        let a = analyze_src("class Foo\n  def a\n    1\n  end\n  def self.a\n    2\n  end\nend\n");
        let ci = a.compiler.class(class_named(&a, "Foo"));
        assert_eq!(ci.own_methods.len(), 1);
        assert_eq!(ci.own_class_methods.len(), 1);
    }

    #[test]
    fn reopening_appends_includes() {
        let a = analyze_src(
            "module M1\nend\nmodule M2\nend\nclass Foo\n  include M1\nend\nclass Foo\n  include M2\nend\n",
        );
        let ci = a.compiler.class(class_named(&a, "Foo"));
        assert_eq!(ci.includes.len(), 2);
    }

    /// The reopen guards, each mirroring CRuby's own TypeError
    /// (oracle-verified messages).
    #[test]
    fn reopen_guards_mirror_ruby_type_errors() {
        assert!(
            analyze_err(
                "class Base\nend\nclass Other\nend\nclass Sub < Base\nend\nclass Sub < Other\nend\n"
            )
            .contains("superclass mismatch for class Sub")
        );

        assert!(analyze_err("class Foo\nend\nmodule Foo\nend\n").contains("Foo is not a module"));
        assert!(analyze_err("module Bar\nend\nclass Bar\nend\n").contains("Bar is not a class"));
    }

    /// A reopen may RESTATE the original superclass (real Ruby allows it).
    #[test]
    fn reopen_with_matching_superclass_is_allowed() {
        let a = analyze_src(
            "class Base\nend\nclass Sub < Base\nend\nclass Sub < Base\n  def ok\n    1\n  end\nend\n",
        );
        let ci = a.compiler.class(class_named(&a, "Sub"));
        assert_eq!(ci.own_methods.len(), 1);
    }

    /// Reopening a builtin MODULE (Enumerable/Comparable) is supported:
    /// its added methods register as value methods on the module id, found by
    /// the MRO walk for every includer.
    #[test]
    fn reopening_a_builtin_module_is_supported() {
        let a = analyze_src("module Enumerable\n  def stat\n    0\n  end\nend\n");
        let ci = a.compiler.class(class_named(&a, "Enumerable"));
        let names: Vec<&str> = ci
            .methods
            .iter()
            .map(|e| a.compiler.names.str(e.name))
            .collect();
        assert!(
            names.contains(&"stat"),
            "reopen method surfaced as a value method"
        );
    }
}

/// Reopening a BUILTIN value class attaches methods/cvars/
/// consts to the existing builtin `ClassInfo`; the still-rejected set
/// (Object, Class/Module, builtin modules, superclass clauses, operators,
/// ivars) rejects with clean messages.
#[cfg(test)]
mod builtin_reopen_tests {
    use super::tests::{analyze_err, analyze_src, class_named};
    use crate::compiler::{INTEGER_CLASS, STRING_CLASS};

    #[test]
    fn reopening_string_attaches_methods_to_the_builtin() {
        let a = analyze_src(
            "class String\n  def blank?\n    length == 0\n  end\n  def shout\n    self\n  end\nend\n",
        );
        assert_eq!(class_named(&a, "String"), STRING_CLASS);
        let ci = a.compiler.class(STRING_CLASS);
        assert!(ci.is_builtin, "reopening must NOT create a new class");
        let names: Vec<&str> = ci
            .methods
            .iter()
            .map(|e| a.compiler.names.str(e.name))
            .collect();
        assert!(names.contains(&"blank?") && names.contains(&"shout"));
        assert!(a.compiler.method_in_chain(STRING_CLASS, "blank?").is_some());
    }

    #[test]
    fn reopen_registers_class_methods_cvars_and_consts() {
        let a = analyze_src(
            "class Array\n  @@made = 0\n  LIMIT = 3\n  def self.tally_up\n    @@made = @@made + 1\n    @@made\n  end\nend\n",
        );
        let ci = a.compiler.class(class_named(&a, "Array"));
        assert_eq!(ci.class_methods.len(), 1);
        assert_eq!(ci.class_body_stmts.len(), 2, "@@made = 0 and LIMIT = 3");
    }

    #[test]
    fn last_def_wins_when_reopening_twice() {
        // 15.2's reopen-merge rule composes with builtins: the SECOND
        // definition replaces the first, one entry total.
        let a = analyze_src(
            "class Integer\n  def tag\n    1\n  end\nend\nclass Integer\n  def tag\n    2\n  end\nend\n",
        );
        let ci = a.compiler.class(INTEGER_CLASS);
        let tags = ci
            .methods
            .iter()
            .filter(|e| a.compiler.names.str(e.name) == "tag")
            .count();
        assert_eq!(tags, 1);
    }

    #[test]
    fn class_and_module_reopen_like_any_other_builtin() {
        // These two were the last builtins excluded, on the reasoning that a
        // `RubyValue::Class` receiver has no per-value dispatch to hang a
        // method on. It has: a class value's own ancestry runs `Class ->
        // Module -> Object`, which is the chain the MRO walk already takes.
        for (src, cid) in [
            (
                "class Class\n  def probe\n    1\n  end\nend\n",
                crate::compiler::CLASS_CLASS,
            ),
            (
                "class Module\n  def probe\n    1\n  end\nend\n",
                crate::compiler::MODULE_CLASS,
            ),
        ] {
            let a = analyze_src(src);
            let probes = a
                .compiler
                .class(cid)
                .methods
                .iter()
                .filter(|e| a.compiler.names.str(e.name) == "probe")
                .count();
            assert_eq!(probes, 1, "expected one `probe` for: {src}");
        }
    }

    #[test]
    fn kind_mismatch_keeps_the_typeerror_message_shape() {
        assert!(analyze_err("module String\nend\n").contains("String is not a module"));
        assert!(analyze_err("class Enumerable\nend\n").contains("Enumerable is not a class"));
    }

    #[test]
    fn superclass_clause_on_a_builtin_reopen_must_match() {
        // A MATCHING clause (`String < Object`) is accepted; a wrong one
        // raises CRuby's `superclass mismatch`.
        let a = analyze_src("class String < Object\n  def x\n    1\n  end\nend\n");
        assert_eq!(class_named(&a, "String"), STRING_CLASS);
        assert!(
            analyze_err("class String < Array\n  def x\n    1\n  end\nend\n")
                .contains("superclass mismatch for class String")
        );
    }

    /// Only a receiver a call site can type `Int` or `Float` has an operator
    /// fast path ahead of the reopened-builtin dispatch arm, so only those
    /// classes -- and the rest of their MRO -- refuse the definition.
    #[test]
    fn operator_definitions_are_rejected_on_the_numeric_mro_only() {
        assert!(
            analyze_err("class Integer\n  def +(other)\n    0\n  end\nend\n")
                .contains("defining operator `+`")
        );
        assert!(
            analyze_err("module Comparable\n  def <(other)\n    true\n  end\nend\n")
                .contains("defining operator `<`")
        );
        analyze_src("class String\n  def %(other)\n    self\n  end\nend\n");
        analyze_src("class Set\n  def <<(other)\n    self\n  end\nend\n");
    }

    /// A reopened builtin has no generated struct, but `@x` in one of its
    /// methods does not need one: the body's self is dynamic and
    /// `ivar_get_dyn`/`ivar_set_dyn` pick the storage tier at run time.
    #[test]
    fn ivars_in_a_builtin_reopen_are_accepted() {
        analyze_src("class String\n  def remember\n    @seen = 1\n  end\nend\n");
    }

    #[test]
    fn nested_class_string_still_shadows_lexically_not_reopens() {
        // 15.3's rule is unchanged: `module Store; class String` is a
        // FRESH nested class, not a builtin reopen.
        let a = analyze_src("module Store\n  class String\n  end\nend\n");
        let nested = class_named(&a, "Store::String");
        assert_ne!(nested, STRING_CLASS);
        assert!(!a.compiler.class(nested).is_builtin);
    }
}

#[cfg(test)]
mod namespacing_tests {
    use super::tests::{analyze_err, analyze_src, class_named};

    /// Nested definitions register with `lexical_parent`, resolve
    /// scope-exactly, and display fully qualified.
    #[test]
    fn nested_definition_registers_under_its_namespace() {
        let a = analyze_src(
            "module Store\n  class Item\n    def price\n      1\n    end\n  end\nend\n",
        );
        let store = class_named(&a, "Store");
        let item = a
            .compiler
            .resolve_class("Store::Item", &[], 0)
            .expect("qualified path resolves");

        assert_eq!(a.compiler.class(item).lexical_parent, Some(store));
        assert_eq!(a.compiler.fq_name(item), "Store::Item");
        assert_eq!(
            a.compiler.resolve_class("Item", &[], 0),
            None,
            "a nested class is invisible at the top level by bare name"
        );
        assert_eq!(
            a.compiler.resolve_class("Item", &[store], 0),
            Some(item),
            "...but resolves lexically from inside its namespace"
        );
    }

    /// The two definition forms differ in CREF only: textual nesting sees the
    /// enclosing scope, the qualified form skips the PREFIX IT SPELLS
    /// (oracle-verified NameError in real Ruby). It still sees whatever scope
    /// it is written inside -- see the sibling test.
    #[test]
    fn qualified_definition_form_cuts_the_cref_chain() {
        let a = analyze_src("module Store\n  class Inner\n  end\nend\nclass Store::Cart\nend\n");
        let store = class_named(&a, "Store");
        let inner = a.compiler.resolve_class("Store::Inner", &[], 0).unwrap();
        let cart = a.compiler.resolve_class("Store::Cart", &[], 0).unwrap();

        assert!(!a.compiler.class(inner).qualified_def);
        assert_eq!(a.compiler.cref_of(Some(inner)), vec![store, inner]);

        assert!(a.compiler.class(cart).qualified_def);
        assert_eq!(
            a.compiler.cref_of(Some(cart)),
            vec![cart],
            "the qualified form's body does not see `Store` lexically"
        );
        assert_eq!(
            a.compiler.fq_name(cart),
            "Store::Cart",
            "naming still qualifies"
        );
    }

    /// A qualified definition written INSIDE a scope keeps that scope: ruby
    /// skips the prefix the path spelled, not the nesting it sits in. Cutting
    /// the chain outright made every constant such a body reads resolve
    /// against the top level -- `class Error < Error` inside
    /// `module HTTPX; class Connection::HTTP2` among them.
    #[test]
    fn a_qualified_definition_still_sees_the_scope_it_is_written_in() {
        let a = analyze_src("module Store\n  class Bin\n  end\n  class Bin::Slot\n  end\nend\n");
        let store = class_named(&a, "Store");
        let slot = a
            .compiler
            .resolve_class("Store::Bin::Slot", &[], 0)
            .unwrap();

        assert!(a.compiler.class(slot).qualified_def);
        assert_eq!(
            a.compiler.cref_of(Some(slot)),
            vec![store, slot],
            "`Store` is kept, `Store::Bin` is skipped"
        );
    }

    #[test]
    fn same_leaf_name_in_two_namespaces_stays_distinct() {
        let a = analyze_src(
            "module A1\n  class Widget\n  end\nend\nmodule B1\n  class Widget\n  end\nend\n",
        );
        let wa = a.compiler.resolve_class("A1::Widget", &[], 0).unwrap();
        let wb = a.compiler.resolve_class("B1::Widget", &[], 0).unwrap();
        assert_ne!(wa, wb);
    }

    /// Reopening composes with nesting: both the textual and the
    /// qualified reopen merge into the one registration.
    #[test]
    fn nested_class_reopens_through_both_forms() {
        let a = analyze_src(
            "module Store\n  class Item\n    def a\n      1\n    end\n  end\nend\nmodule Store\n  class Item\n    def b\n      2\n    end\n  end\nend\nclass Store::Item\n  def c\n    3\n  end\nend\n",
        );
        let item = a.compiler.resolve_class("Store::Item", &[], 0).unwrap();
        let count = a
            .compiler
            .classes
            .iter()
            .filter(|c| c.name == "Item")
            .count();
        assert_eq!(count, 1, "one merged registration across all three bodies");
        assert_eq!(a.compiler.class(item).own_methods.len(), 3);
    }

    /// A container NOTHING in the program defines is not a compile error:
    /// ruby evaluates `Nowhere` when the definition runs and raises
    /// `NameError` there. The definition is rewritten to that bare constant
    /// read -- see `defer_unresolved_directive` -- and registers no class.
    #[test]
    fn qualified_definition_with_unknown_prefix_defers_to_a_runtime_name_error() {
        let a = analyze_src("class Nowhere::Item\nend\n");
        assert!(
            a.compiler.classes.iter().all(|c| c.name != "Item"),
            "no class is registered for a definition that never runs"
        );
        assert!(
            a.main_statements.iter().any(|&s| matches!(
                &a.compiler.hir[s],
                crate::hir::HirNode::ClassRef(n) if n == "Nowhere"
            )),
            "the definition became a read of the missing constant"
        );
    }

    /// A container the program DOES assign stays loud: the constant would read
    /// back fine at runtime, so deferring would hide a real gap rather than
    /// reproduce ruby's error.
    #[test]
    fn qualified_definition_under_an_assigned_container_is_still_an_error() {
        assert!(
            analyze_err("Nowhere = Object.new\nclass Nowhere::Item\nend\n")
                .contains("unknown class/module `Nowhere`")
        );
    }

    /// `module Store; class String; end; end` defines a fresh, unrelated
    /// nested class (real Ruby) -- it shadows the builtin lexically inside
    /// `Store` and nowhere else.
    #[test]
    fn a_nested_class_may_shadow_a_builtin_lexically() {
        let a = analyze_src("module Store\n  class String\n  end\nend\n");
        let store = class_named(&a, "Store");
        let nested = a.compiler.resolve_class("Store::String", &[], 0).unwrap();

        assert!(!a.compiler.class(nested).is_builtin);
        assert_eq!(
            a.compiler.resolve_class("String", &[], 0),
            Some(crate::compiler::STRING_CLASS),
            "top-level `String` is still the builtin"
        );
        assert_eq!(
            a.compiler.resolve_class("String", &[store], 0),
            Some(nested)
        );
    }

    /// A leading `::` anchors a definition at the top level from any depth.
    #[test]
    fn top_anchored_definition_escapes_its_namespace() {
        let a = analyze_src("module M\n  class ::Escaped\n  end\nend\n");
        let escaped = class_named(&a, "Escaped");
        assert_eq!(a.compiler.class(escaped).lexical_parent, None);
        assert_eq!(a.compiler.fq_name(escaped), "Escaped");
    }
}

#[cfg(test)]
mod class_value_tests {
    use super::tests::analyze_src;
    use crate::compiler::{
        BASIC_OBJECT_CLASS, CLASS_CLASS, COMPARABLE_CLASS, ClassId, ENUMERABLE_CLASS,
        INTEGER_CLASS, KERNEL_CLASS, MODULE_CLASS, NUMERIC_CLASS, OBJECT_CLASS, RATIONAL_CLASS,
        STRING_CLASS, STRUCT_CLASS,
    };
    use crate::types::TyKind;

    /// A local assigned a bare class name is statically typed
    /// as a class VALUE of that class -- what keeps `x.new`/class-method
    /// calls through the variable on Path 1.
    #[test]
    fn a_class_assigned_to_a_local_types_as_class_obj() {
        let a = analyze_src("class Widget\nend\nx = Widget\n");
        let widget = a.compiler.resolve_class("Widget", &[], 0).unwrap();
        assert_eq!(a.main_local_types.get("x"), Some(&TyKind::ClassObj(widget)));
    }

    /// `x.new` through the class-value-typed local types exactly like a
    /// literal `Widget.new` (both emit the same unboxed construction).
    #[test]
    fn new_through_a_class_value_types_as_the_instance() {
        let a = analyze_src("class Widget\nend\nx = Widget\ny = x.new\n");
        let widget = a.compiler.resolve_class("Widget", &[], 0).unwrap();
        assert_eq!(a.main_local_types.get("y"), Some(&TyKind::Object(widget)));
    }

    /// `Class < Module < Object` (the ABI's declarative parent edges), so
    /// `Widget.is_a?(Module)` answers true through the ordinary ancestry
    /// machinery -- and the chain carries real Ruby's universal tail:
    /// `..., Object, Kernel, BasicObject`.
    #[test]
    fn class_class_linearizes_under_module() {
        let a = analyze_src("");
        let ancestors = &a.compiler.class(CLASS_CLASS).ancestors;
        assert_eq!(
            ancestors,
            &vec![
                CLASS_CLASS,
                MODULE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
    }

    /// The full CRuby-exact chains for the builtin classes
    /// (oracle: ruby 4.0.6 `.ancestors`).
    #[test]
    fn builtin_ancestors_match_cruby() {
        let a = analyze_src("");
        let chain = |cid: ClassId| a.compiler.class(cid).ancestors.clone();
        let tail = [OBJECT_CLASS, KERNEL_CLASS, BASIC_OBJECT_CLASS];
        assert_eq!(
            chain(INTEGER_CLASS),
            [
                [INTEGER_CLASS, NUMERIC_CLASS, COMPARABLE_CLASS].as_slice(),
                &tail
            ]
            .concat()
        );
        assert_eq!(
            chain(RATIONAL_CLASS),
            [
                [RATIONAL_CLASS, NUMERIC_CLASS, COMPARABLE_CLASS].as_slice(),
                &tail
            ]
            .concat()
        );
        assert_eq!(
            chain(STRING_CLASS),
            [[STRING_CLASS, COMPARABLE_CLASS].as_slice(), &tail].concat()
        );
        assert_eq!(
            chain(STRUCT_CLASS),
            [[STRUCT_CLASS, ENUMERABLE_CLASS].as_slice(), &tail].concat()
        );
        assert_eq!(
            chain(OBJECT_CLASS),
            vec![OBJECT_CLASS, KERNEL_CLASS, BASIC_OBJECT_CLASS]
        );
        assert_eq!(chain(BASIC_OBJECT_CLASS), vec![BASIC_OBJECT_CLASS]);
        assert_eq!(chain(KERNEL_CLASS), vec![KERNEL_CLASS]);
    }
}
