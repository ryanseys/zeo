//! The minimal analyze pass: walks the top-level `Hir::Program` statements
//! once (no fixpoint loop -- see the plan's stated scope-cut) and registers
//! every `ClassDef`/`DefMethod` into a `Compiler`, mirroring zeo's
//! `walk_scope`/`register_locals`/`resolve_parents` (a tiny slice of them).
//! Structured as a single pass function rather than the predecessor's
//! 128-iteration fixpoint loop because nothing yet needs mutual
//! recursion between inference results -- but the shape (one function that
//! walks the whole program and mutates a `Compiler`) is exactly what a real
//! fixpoint would wrap in `for iter in 0..128 { ... }` later.

mod locals;
mod mro;

use crate::compiler::{ClassId, Compiler, OBJECT_CLASS, Scope};
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
}

pub fn analyze(hir: Hir, root: NodeId) -> Result<Analyzed, crate::diagnostics::CompileError> {
    analyze_impl(hir, root).map_err(crate::diagnostics::CompileError::analyze)
}

/// The whole pass, with the `String` errors its sites raise -- typed (and
/// eventually located) at the public boundary above.
fn analyze_impl(hir: Hir, root: NodeId) -> Result<Analyzed, String> {
    let mut compiler = Compiler::new(hir);
    let HirNode::Program(statements) = &compiler.hir[root] else {
        return Err("expected a Program root".to_string());
    };
    let statements = statements.clone();
    let builtin_exceptions_len = compiler.hir.builtin_exceptions_len;

    // Whole-program kind map for forward-container resolution (see
    // `Compiler::shell_kinds` and `resolve_or_create_container`): a read-only
    // scan of every class/module definition, run before the ordered
    // registration walk below.
    let mut shell_kinds = HashMap::new();
    collect_shell_kinds(&compiler.hir, &statements, &[], 0, &mut shell_kinds);
    compiler.shell_kinds = shell_kinds;

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
    // count. The box surrogates used to be created before the whole analyze
    // pass, which stole those ids the moment a program allocated a box. See
    // `pin_builtin_exceptions_tail`.
    let mut tail_pinned = false;
    for (idx, stmt) in statements.into_iter().enumerate() {
        if idx >= builtin_exceptions_len && !tail_pinned {
            pin_builtin_exceptions_tail(&mut compiler)?;
            tail_pinned = true;
        }
        process_top_stmt(
            &mut compiler,
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
        pin_builtin_exceptions_tail(&mut compiler)?;
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

    // Ancestor linearization + method/class-method materialization + class
    // variable ownership -- must run AFTER every `ClassDef` above has been
    // registered, since `include`/`extend`/`prepend`/`< Super` targets must
    // already exist (same "defined earlier in the file" rule `superclass`
    // resolution already enforces). See `mro`'s module docs.
    mro::materialize(&mut compiler, &main_statements)?;

    let main_local_types = locals::infer_locals(&compiler, None, 0, &main_statements);

    Ok(Analyzed {
        compiler,
        main_statements,
        main_local_types,
    })
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
fn process_top_stmt(
    compiler: &mut Compiler,
    stmt: NodeId,
    bootstrap: bool,
    main_statements: &mut Vec<NodeId>,
    pre_exec: &mut Vec<NodeId>,
) -> Result<(), String> {
    if let HirNode::PreExec(body) = &compiler.hir[stmt] {
        pre_exec.extend(body.clone());
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
        let target = resolve_module_target(compiler, m, &[], 0)?;
        compiler.classes[OBJECT_CLASS.0 as usize]
            .includes
            .push(target);
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
        compiler.classes[OBJECT_CLASS.0 as usize]
            .pending_aliases
            .push(entry);
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
            main_statements.push(stmt);
            return Ok(());
        }
        let (cond, then_body, else_body) = (*cond, then_body.clone(), else_body.clone());
        let taken = match static_top_cond(compiler, cond) {
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
                        "top-level conditional def: undecidable guard over a class REOPENING -- pushing the guard into the class body"
                    );
                    return process_top_stmt(compiler, rewritten, false, main_statements, pre_exec);
                }
                tracing::debug!(
                    guard = cond_kind(compiler, cond),
                    "top-level conditional def: guard UNDECIDABLE -- compile error"
                );
                return Err(
                    "class/module definition inside a top-level `if` is only supported when \
                     the condition is compile-time decidable (e.g. `defined?(SomeConstant)`), \
                     or a reopening of an already-defined class that only adds methods"
                        .to_string(),
                );
            }
        };
        for s in taken {
            process_top_stmt(compiler, s, false, main_statements, pre_exec)?;
        }
    } else {
        main_statements.push(stmt);
    }
    Ok(())
}

/// Whether any statement in `body` (descending nested `if` branches) is a
/// node only the top-level walk can register -- exactly the set
/// `codegen::expr::emit_expr` has no expression form for, minus
/// `DefMethod` (which has a runtime `define_method` emission and so
/// survives inside an ordinary undecided `if` unchanged).
fn branch_has_top_defs(compiler: &Compiler, body: &[NodeId]) -> bool {
    body.iter().any(|&s| match &compiler.hir[s] {
        HirNode::ClassDef { .. }
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
        | HirNode::Undef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. } => true,
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
/// If an undecidable-guard top-level `if` is exactly a conditional REOPENING of
/// an already-registered class -- `class Existing ... end if cond` (one branch a
/// lone `ClassDef` naming a known class, the other empty) -- rewrite it by
/// pushing the guard INTO the class body: `class Existing; if cond; <body>; end;
/// end`. The ordinary class-body walk already handles that shape (a conditional
/// `def` becomes a runtime `define_method`, still registered for reflection via
/// `register_conditional_defs` -- the fileutils platform-`def` path), so no new
/// codegen is needed. Returns the rewritten `ClassDef`, or `None` when the
/// pattern doesn't match -- a NEW class, multiple statements, or a non-`ClassDef`
/// branch stay a clean compile error.
///
/// Valid ONLY for a reopening: for a NEW class the transform would define it
/// unconditionally (`class X; if cond; ...` always creates `X`), changing
/// semantics. pp.rb's `class Set ... end if set_pp` monkeypatch is the case.
fn try_conditional_reopen(
    compiler: &mut Compiler,
    cond: NodeId,
    then_body: &[NodeId],
    else_body: &[NodeId],
) -> Option<NodeId> {
    // Exactly one branch is a lone statement; the other is empty (a modifier
    // `class ... end if/unless cond`, which is all this idiom ever is).
    let (def_stmt, on_then) = match (then_body, else_body) {
        ([only], []) => (*only, true),
        ([], [only]) => (*only, false),
        _ => return None,
    };
    // Clone the ClassDef's parts so the `&compiler.hir` borrow ends before the
    // `resolve_class` read and the `hir.push` writes below.
    let (name, superclass, body, is_module) = match &compiler.hir[def_stmt] {
        HirNode::ClassDef {
            name,
            superclass,
            body,
            is_module,
        } => (name.clone(), superclass.clone(), body.clone(), *is_module),
        _ => return None,
    };
    // Must REOPEN an already-registered class (top-level cref/box). A new class
    // can't be defined conditionally -- codegen has no runtime create form here.
    compiler.resolve_class(&name, &[], 0)?;
    let (then_body, else_body) = if on_then {
        (body, Vec::new())
    } else {
        (Vec::new(), body)
    };
    let guarded = compiler.hir.push(HirNode::If {
        cond,
        then_body,
        else_body,
    });
    Some(compiler.hir.push(HirNode::ClassDef {
        name,
        superclass,
        body: vec![guarded],
        is_module,
    }))
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

fn static_top_cond(compiler: &Compiler, id: NodeId) -> Option<bool> {
    match &compiler.hir[id] {
        HirNode::Defined(inner) => match &compiler.hir[*inner] {
            HirNode::ClassRef(name) => {
                if compiler.resolve_class(name, &[], 0).is_some() {
                    Some(true)
                } else if const_ever_written(compiler, name) {
                    None
                } else {
                    Some(false)
                }
            }
            HirNode::QualifiedConstRead(scope, name) => {
                match compiler.resolve_class(scope, &[], 0) {
                    Some(sid) => {
                        // Same probe order as `constfold::class_const_in`:
                        // a class nested in `scope`, else (const lookup
                        // inherits through Object) a top-level class.
                        let fq = format!("{}::{name}", compiler.fq_name(sid));
                        if compiler.resolve_class(&fq, &[], 0).is_some()
                            || compiler.resolve_class(name, &[], 0).is_some()
                            // A VALUE constant `scope` assigns in its OWN body
                            // (`module Psych; VERSION = "5.4.0"`) IS defined here,
                            // even though value constants aren't fully resolved
                            // until `resolve_consts` -- the `ConstWrite` is
                            // already in `scope`'s registered body. Lets a
                            // `defined?(Psych::VERSION)`-gated definition fold.
                            || mro::directly_defines_const(compiler, sid, name)
                        {
                            Some(true)
                        } else {
                            // Could name a VALUE constant on `scope` assigned
                            // elsewhere/at runtime -- not decidable here.
                            None
                        }
                    }
                    None if const_ever_written(compiler, scope) => None,
                    None => Some(false),
                }
            }
            _ => None,
        },
        HirNode::And(l, r) => match static_top_cond(compiler, *l) {
            Some(false) => Some(false),
            Some(true) => static_top_cond(compiler, *r),
            None => None,
        },
        _ => None,
    }
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
    // `SyntaxError < ScriptError` (#97 stage 2) -- the eval VM's parse-failure
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
    // The remaining core `Exception`-tree classes, ids matching
    // `zeo-abi::EXCEPTION_CLASSES` exc_id(51..56). Each parent is already
    // registered (a builtin exception or, for the nested names, a core class).
    for (name, superclass) in [
        ("NoMemoryError", "Exception"),
        ("SecurityError", "Exception"),
        ("SystemStackError", "Exception"),
        ("NoMatchingPatternKeyError", "NoMatchingPatternError"),
        ("Regexp::TimeoutError", "RegexpError"),
        ("IO::TimeoutError", "IOError"),
        ("Errno::EDOM", "SystemCallError"),
        ("Errno::ESRCH", "SystemCallError"),
        ("Errno::EPERM", "SystemCallError"),
        ("Errno::ECONNREFUSED", "SystemCallError"),
        // `WeakRef::RefError` -- nests under the `WeakRef` builtin, a plain
        // `StandardError` (matches `zeo-abi::EXCEPTION_CLASSES` exc_id(61)).
        ("WeakRef::RefError", "StandardError"),
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
/// (the #1036 `INTEGER_KLASS = 1.class; class INTEGER_KLASS; ...` shape),
/// rather than minting a fresh one named `CONST`.
fn const_alias_target(compiler: &Compiler, leaf: &str, box_id: u32) -> Option<ClassId> {
    let value = compiler.hir.nodes().iter().find_map(|n| match n {
        HirNode::ConstWrite {
            scope: None,
            name,
            value,
        } if name == leaf => Some(*value),
        _ => None,
    })?;
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
fn resolve_or_create_container(compiler: &mut Compiler, path: &str, box_id: u32) -> Option<ClassId> {
    if let Some(cid) = compiler.resolve_class(path, &[], box_id) {
        return Some(cid);
    }
    // Only a name the program defines somewhere gets a forward shell; a truly
    // unknown container falls through to the caller's error.
    let is_module = *compiler.shell_kinds.get(&(box_id, path.to_string()))?;
    let cp = crate::constpath::ConstPath::parse(path);
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
    ci.box_id = box_id;
    Some(cid)
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
    let path = crate::constpath::ConstPath::parse(&name);
    let (lexical_parent, leaf, qualified_def) = match path.scope() {
        // `A::B` / `::A::B` -- defined INSIDE a named scope, which must
        // already exist.
        Some(prefix) => {
            let parent = compiler
                .resolve_class(prefix, cref, box_id)
                // A container defined LATER in the flattened statement list
                // than this definition (a deferred require's reopen preceding
                // the forward-declaration it depends on) is created on demand as
                // a shell -- see `resolve_or_create_container`.
                .or_else(|| resolve_or_create_container(compiler, prefix, box_id))
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
        // A bare `class CONST` where CONST aliases an existing class reopens
        // it (`INT_ALIAS = 1.class; class INT_ALIAS; include M; end`).
        .or_else(|| {
            if qualified_def {
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
            use crate::compiler::{CLASS_CLASS, MODULE_CLASS};
            // A KIND mismatch (`module String`) falls through to the
            // ordinary reopen guard below instead, which produces real
            // Ruby's own TypeError message shape ("String is not a module").
            // `Object` is NOT in this reject list (since top-level `def`
            // support): reopening it merges into arena slot 0 exactly like
            // any builtin-class reopen -- an Object reopen is top-level
            // `def` by another name.
            // `Class`/`Module` themselves have no per-value dispatch to hang a
            // reopen method on, so they stay unsupported. Every OTHER builtin
            // module (`Enumerable`/`Comparable`/`Kernel`/`Math`) now accepts a
            // reopen (D3): its added methods register as value methods on the
            // module id, found by the MRO walk for every includer.
            if ci.is_module == is_module && (cid == CLASS_CLASS || cid == MODULE_CLASS) {
                return Err(format!(
                    "reopening the built-in {} `{name}` isn't supported yet (zeo limitation)",
                    if ci.is_module { "module" } else { "class" }
                ));
            }
            // A reopen may RESTATE the builtin's superclass (`class String <
            // Object`); CRuby accepts a matching clause and raises `superclass
            // mismatch` on a wrong one (D3). Mirrors the user-class reopen guard.
            if let Some(s) = &superclass {
                let want = compiler.resolve_class(s, cref, box_id).ok_or_else(|| {
                    format!("unknown superclass `{s}` (must be defined earlier in the file)")
                })?;
                if compiler.class(cid).parent != Some(want) {
                    return Err(format!("superclass mismatch for class {name}"));
                }
            }
        }
    }
    let class_id = match existing {
        // REOPENING (replacing the old silent-no-op duplicate
        // registration): a second `class Foo`/`module Foo` MERGES into the
        // existing `ClassInfo` -- the body loop below appends
        // includes/body-statements and registers methods with real Ruby's
        // last-`def`-wins rule (see the method arm). Guards mirror CRuby's
        // own (all oracle-verified): the definition KIND must match
        // (`TypeError: Foo is not a module`), and a superclass clause, if
        // written at all, must resolve to the original parent
        // (`TypeError: superclass mismatch for class Foo`).
        Some(cid) => {
            if compiler.class(cid).is_module != is_module {
                return Err(format!(
                    "{name} is not a {}",
                    if is_module { "module" } else { "class" }
                ));
            }
            // A user `module OpenSSL; ...; end` reopening a feature-gated
            // builtin slot MATERIALIZES the constant: clear the gate so the
            // name resolves even though the ext was never `require`d (the
            // behavior comes from the user's own methods registered here).
            if compiler.class(cid).feature_gate.is_some() {
                compiler.classes[cid.0 as usize].feature_gate = None;
            }
            if let Some(s) = &superclass {
                let want = compiler.resolve_class(s, cref, box_id).ok_or_else(|| {
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
                        let cid = compiler.resolve_class(s, cref, box_id).ok_or_else(|| {
                            format!(
                                "unknown superclass `{s}` (must be defined earlier in the file)"
                            )
                        })?;
                        // Subclassable builtins (D3):
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
                        // Still rejected: `Range` (no runtime constructor) and
                        // `Class`/`Module` (no per-value dispatch).
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
                                | STRUCT_CLASS
                                // `FFI::Struct` (#204): a subclass is a plain
                                // ivar object (no native payload) whose `[]`/
                                // `[]=`/`size`/`offset_of` are synthesized from
                                // its `layout` over an `FFI::MemoryPointer` ivar
                                // -- see `parse`'s `synthesize_ffi_struct`.
                                | FFI_STRUCT_CLASS
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

    // This definition site's own record -- see `Compiler::class_body_sites`.
    let site_idx = compiler.class_body_sites.len();
    compiler
        .class_body_sites
        .push(crate::compiler::ClassBodySite {
            def_node,
            class: class_id,
            stmts: Vec::new(),
        });

    for &stmt in body {
        match &compiler.hir[stmt] {
            HirNode::DefMethod { .. } => {
                register_body_def_method(compiler, class_id, stmt)?;
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
            HirNode::Include(m) => {
                let target = resolve_module_target(compiler, m, &child_cref, box_id)?;
                compiler.classes[class_id.0 as usize].includes.push(target);
            }
            HirNode::Extend(m) => {
                let target = resolve_module_target(compiler, m, &child_cref, box_id)?;
                compiler.classes[class_id.0 as usize].extends.push(target);
            }
            // `undef foo, bar` -- recorded here, honored by
            // `mro::materialize_methods`. See `HirNode::Undef`.
            HirNode::Undef(names) => {
                let names = names.clone();
                compiler.classes[class_id.0 as usize]
                    .undefined
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
                let entry = (new_name.clone(), old_name.clone(), *is_class_method);
                compiler.classes[class_id.0 as usize]
                    .pending_aliases
                    .push(entry);
            }
            // A `private`/`public`/`protected :m` re-declaring an INHERITED
            // method's visibility -- applied by codegen after materialization.
            // See `HirNode::MethodVisibility`.
            HirNode::MethodVisibility { name, visibility } => {
                compiler.classes[class_id.0 as usize]
                    .visibility_overrides
                    .push((name.clone(), *visibility));
            }
            HirNode::Prepend(m) => {
                let target = resolve_module_target(compiler, m, &child_cref, box_id)?;
                compiler.classes[class_id.0 as usize].prepends.push(target);
            }
            // `IvarWrite`: a bare `@x = expr` in a class body is an ivar on
            // the CLASS OBJECT (`self` in a class body is the class), i.e.
            // the same storage `def self.x; @x; end` reads -- the ordinary
            // way a class-level `@registry = []` gets initialized. Before
            // this it fell into the `_ => {}` arm below and was SILENTLY
            // DROPPED, so the reader saw a bare nil with no diagnostic.
            HirNode::IvarWrite(..) | HirNode::ClassVarWrite(..) | HirNode::ConstWrite { .. } => {
                compiler.classes[class_id.0 as usize]
                    .class_body_stmts
                    .push(stmt);
                compiler.class_body_sites[site_idx].stmts.push(stmt);
            }
            // Any OTHER class-body statement -- a method call, conditional,
            // loop, a runtime `define_method` inside an `each`, etc. -- is real
            // code that runs ONCE at class-definition time with `self` = the
            // class object (#97 F2a). Collected here (flat list AND this
            // site's own record) and executed at the site's document
            // position. Before this it fell through and was SILENTLY
            // DROPPED, so a class-body `[:a].each { define_method(...) }`
            // never ran.
            _ => {
                // A `def` nested in an `if`/`case` branch also runs at document
                // position (the taken branch's runtime `define_method` gives the
                // real body), but must ALSO be registered as an own method so
                // `instance_methods`/`extend` -- both resolved at COMPILE time --
                // can see it. This is what lets fileutils' platform-conditional
                // `StreamUtils_#fu_windows?` reach `FileUtils` via `extend`.
                register_conditional_defs(compiler, class_id, &[stmt])?;
                compiler.classes[class_id.0 as usize]
                    .class_body_stmts
                    .push(stmt);
                compiler.class_body_sites[site_idx].stmts.push(stmt);
            }
        }
    }
    Ok(())
}

/// Files one registered method `Scope` under its class's own-method list --
/// REPLACING any earlier same-name entry rather than appending a shadowed
/// duplicate: real Ruby's last-`def`-wins rule (oracle-verified), which
/// applies identically to a redefinition within one class body and to one
/// arriving via reopening (`method_in_chain` resolves the FIRST name match,
/// so append-only registration would silently keep dispatching the OLD
/// body). Instance and class methods are separate namespaces, hence the
/// separate lists.
fn add_own_method(
    compiler: &mut Compiler,
    class_id: ClassId,
    sid: crate::compiler::ScopeId,
    is_class_method: bool,
) {
    let mname = compiler.scope(sid).name.clone();
    let ci = &compiler.classes[class_id.0 as usize];
    let list = if is_class_method {
        &ci.own_class_methods
    } else {
        &ci.own_methods
    };
    let replaced = list.iter().position(|&s| compiler.scope(s).name == mname);
    let ci = &mut compiler.classes[class_id.0 as usize];
    let list = if is_class_method {
        &mut ci.own_class_methods
    } else {
        &mut ci.own_methods
    };
    match replaced {
        Some(i) => list[i] = sid,
        None => list.push(sid),
    }
}

/// Registers one class/module-body `def` as an own method: builds its `Scope`
/// and files it under `own_methods`/`own_class_methods`. Shared by the
/// top-level class-body walk and `register_conditional_defs` (a `def` nested in
/// an `if`/`case` branch). A no-op if `stmt` isn't a `DefMethod`.
fn register_body_def_method(
    compiler: &mut Compiler,
    class_id: ClassId,
    stmt: NodeId,
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
    // rejected outright (zeo limitation): the native `Int`/`Float`/`Str`
    // operator fast paths are emitted unconditionally at every static call
    // site, so a user operator would be silently bypassed there -- a loud
    // rejection beats dispatch that only sometimes honors the override.
    if compiler.class(class_id).is_builtin
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
        name,
        Some(stmt),
        params,
        body,
        visibility,
    )?;
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
            HirNode::DefMethod { .. } => register_body_def_method(compiler, class_id, s)?,
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

fn resolve_module_target(
    compiler: &Compiler,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
) -> Result<ClassId, String> {
    compiler
        .resolve_class(name, cref, box_id)
        .ok_or_else(|| format!("unknown module `{name}` (must be defined earlier in the file)"))
}

/// Registers one method BODY (a class/module's own literal `def`, or a
/// winning ancestor's body being materialized onto a descendant -- see
/// `mro::materialize_methods`/`materialize_class_methods`) as a fresh
/// `Scope`, running the full per-method analysis pipeline (local-type
/// inference, named-`*rest`/`**kwrest`/`&block`-param type seeding,
/// bare-`yield`/`block_given?` scanning) that used to live directly inline
/// in `register_class` before materialization needed to reuse it too.
/// `owner` is whichever class/module this Scope is filed under (and, for a
/// materialized method, whose concrete struct it'll be generated into);
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
    let defining_box = compiler.class(defining_class).box_id;
    let mut local_types = locals::infer_locals(compiler, Some(defining_class), defining_box, &body);
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
    let mut uses_bare_block = false;
    for &n in &body {
        if scan_bare_block_use(&compiler.hir, n) {
            uses_bare_block = true;
        }
    }
    Ok(compiler.push_scope(Scope {
        name,
        class: Some(owner),
        defining_class,
        def_node,
        params,
        body,
        local_types,
        uses_bare_block,
        visibility,
        // Ordinary methods are never native defaults; the bootstrap-marking
        // pass and `mro` set this true for the pristine exception bodies.
        native_default: false,
    }))
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
        // An FFI wrapper body (#204) uses no block.
        HirNode::Ffi(_) => false,
        HirNode::Yield(_) | HirNode::BlockGiven => true,
        HirNode::IvarWrite(_, value)
        | HirNode::LocalWrite(_, value)
        | HirNode::ClassVarWrite(_, value) => scan_bare_block_use(hir, *value),
        HirNode::And(l, r) | HirNode::Or(l, r) => {
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
            if let Some(b) = block {
                if let HirNode::Block { body, .. } = &hir[*b] {
                    found |= scan_bare_block_use_body(hir, body);
                }
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
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
        // An FFI wrapper body (#204) uses no block.
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
        HirNode::And(l, r) | HirNode::Or(l, r) => {
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
            if let Some(b) = block {
                if let HirNode::Block { body, .. } = &hir[*b] {
                    found |= scan_contains_super_body(hir, body);
                }
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
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_)
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
pub(crate) fn collect_ivars(hir: &Hir, id: NodeId, out: &mut Vec<String>) {
    match &hir[id] {
        // An FFI wrapper body (#204) references no instance variables.
        HirNode::Ffi(_) => {}
        HirNode::IvarRead(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        HirNode::IvarWrite(name, value) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
            collect_ivars(hir, *value, out);
        }
        HirNode::LocalWrite(_, value) => collect_ivars(hir, *value, out),
        HirNode::And(l, r) | HirNode::Or(l, r) => {
            collect_ivars(hir, *l, out);
            collect_ivars(hir, *r, out);
        }
        HirNode::Defined(v) => collect_ivars(hir, *v, out),
        HirNode::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_ivars(hir, *cond, out);
            for &n in then_body {
                collect_ivars(hir, n, out);
            }
            for &n in else_body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::CaseWhen {
            subject,
            arms,
            else_body,
        } => {
            if let Some(s) = subject {
                collect_ivars(hir, *s, out);
            }
            for (values, body) in arms {
                for e in values {
                    let (ArrayElem::Single(v) | ArrayElem::Splat(v)) = e;
                    collect_ivars(hir, *v, out);
                }
                for &n in body {
                    collect_ivars(hir, n, out);
                }
            }
            for &n in else_body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Call {
            receiver,
            args,
            kwargs,
            block,
            block_arg,
            ..
        } => {
            if let Some(r) = receiver {
                collect_ivars(hir, *r, out);
            }
            for a in args {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = a;
                collect_ivars(hir, *n, out);
            }
            for n in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                collect_ivars(hir, n, out);
            }
            if let Some(b) = block {
                collect_ivars(hir, *b, out);
            }
            if let Some(b) = block_arg {
                collect_ivars(hir, *b, out);
            }
        }
        HirNode::New { args, block, .. } => {
            for &a in args {
                collect_ivars(hir, a, out);
            }
            if let Some(b) = block {
                collect_ivars(hir, *b, out);
            }
        }
        HirNode::SuperCall { args, kwargs, block_arg, .. } => {
            for a in args {
                collect_ivars(hir, a.node_id(), out);
            }
            for a in kwargs.iter().flat_map(|kw| kw.node_ids()) {
                collect_ivars(hir, a, out);
            }
            if let Some(b) = block_arg {
                collect_ivars(hir, *b, out);
            }
        }
        HirNode::Block { body, .. } | HirNode::Lambda { body, .. } => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::ArrayLit(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                collect_ivars(hir, *n, out);
            }
        }
        HirNode::HashLit(pairs) => {
            for n in pairs.iter().flat_map(|kw| kw.node_ids()) {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::RangeLit { start, end, .. } => {
            if let Some(s) = start {
                collect_ivars(hir, *s, out);
            }
            if let Some(e) = end {
                collect_ivars(hir, *e, out);
            }
        }
        HirNode::StringLit(parts) | HirNode::RegexpLit(parts, _) => {
            for p in parts {
                if let StrPart::Interp(n) = p {
                    collect_ivars(hir, *n, out);
                }
            }
        }
        HirNode::While { cond, body, .. } => {
            collect_ivars(hir, *cond, out);
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Loop { body } => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::For { target, iterable, body } => {
            target.for_each_node(&mut |n| collect_ivars(hir, n, out));
            collect_ivars(hir, *iterable, out);
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
            if let Some(v) = v {
                collect_ivars(hir, *v, out);
            }
        }
        HirNode::Redo | HirNode::BlockGiven => {}
        HirNode::MultiWrite { targets, value } => {
            collect_ivars(hir, *value, out);
            targets.for_each_node(&mut |n| collect_ivars(hir, n, out));
        }
        HirNode::GlobalWrite(_, value) => collect_ivars(hir, *value, out),
        HirNode::ConstWrite { value, .. } => collect_ivars(hir, *value, out),
        HirNode::PreExec(body) | HirNode::Seq(body) | HirNode::Eval(body) | HirNode::BoxScope { body, .. } => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
        }
        HirNode::Yield(elems) => {
            for e in elems {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                collect_ivars(hir, *n, out);
            }
        }
        HirNode::Raise(args, cause) => {
            for &a in args.iter().chain(crate::hir::raise_cause_node(cause).iter()) {
                collect_ivars(hir, a, out);
            }
        }
        HirNode::ClassVarWrite(_, value) => collect_ivars(hir, *value, out),
        HirNode::CaseIn { subject, arms, else_body } => {
            collect_ivars(hir, *subject, out);
            for arm in arms {
                arm.pattern.for_each_node(&mut |n| collect_ivars(hir, n, out));
                if let Some((g, _)) = arm.guard {
                    collect_ivars(hir, g, out);
                }
                for &n in &arm.body {
                    collect_ivars(hir, n, out);
                }
            }
            if let Some(body) = else_body {
                for &n in body {
                    collect_ivars(hir, n, out);
                }
            }
        }
        HirNode::MatchPredicate { subject, pattern } | HirNode::MatchRequired { subject, pattern } => {
            collect_ivars(hir, *subject, out);
            pattern.for_each_node(&mut |n| collect_ivars(hir, n, out));
        }
        HirNode::Begin {
            body,
            rescues,
            else_body,
            ensure_body,
        } => {
            for &n in body {
                collect_ivars(hir, n, out);
            }
            for r in rescues {
                for &n in &r.body {
                    collect_ivars(hir, n, out);
                }
            }
            if let Some(b) = else_body {
                for &n in b {
                    collect_ivars(hir, n, out);
                }
            }
            if let Some(b) = ensure_body {
                for &n in b {
                    collect_ivars(hir, n, out);
                }
            }
        }
        HirNode::Retry => {}
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
        | HirNode::SelfRef
        | HirNode::LocalRead(_)
        | HirNode::ClassVarRead(_)
        | HirNode::ClassRef(_)
        | HirNode::GlobalRead(_)
        | HirNode::LastMatchRef(_)
        | HirNode::Undef(_)
        | HirNode::AliasMethod { .. }
        | HirNode::MethodVisibility { .. }
        | HirNode::AliasGlobal(..)
        | HirNode::QualifiedConstRead(..)
        | HirNode::ConstReadOrNil(..)
        | HirNode::Include(_)
        | HirNode::Extend(_)
        | HirNode::Prepend(_) => {}
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => {}
    }
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

    /// Reopening a builtin MODULE (Enumerable/Comparable) is supported (D3):
    /// its added methods register as value methods on the module id, found by
    /// the MRO walk for every includer.
    #[test]
    fn reopening_a_builtin_module_is_supported() {
        let a = analyze_src("module Enumerable\n  def stat\n    0\n  end\nend\n");
        let ci = a.compiler.class(class_named(&a, "Enumerable"));
        let names: Vec<&str> = ci
            .methods
            .iter()
            .map(|&sid| a.compiler.scope(sid).name.as_str())
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
            .map(|&sid| a.compiler.scope(sid).name.as_str())
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
            .filter(|&&sid| a.compiler.scope(sid).name == "tag")
            .count();
        assert_eq!(tags, 1);
    }

    #[test]
    fn only_class_and_module_stay_rejected() {
        // `Object` is no longer in this list: reopening it is top-level `def`
        // by another name. Builtin MODULES (Comparable/Enumerable) became
        // reopenable in D3; only `Class`/`Module` themselves -- which have no
        // per-value dispatch to hang a method on -- stay rejected.
        for src in [
            "class Class\n  def probe\n    1\n  end\nend\n",
            "class Module\n  def probe\n    1\n  end\nend\n",
        ] {
            assert!(
                analyze_err(src).contains("isn't supported yet (zeo limitation)"),
                "expected rejection for: {src}"
            );
        }
    }

    #[test]
    fn kind_mismatch_keeps_the_typeerror_message_shape() {
        assert!(analyze_err("module String\nend\n").contains("String is not a module"));
        assert!(analyze_err("class Enumerable\nend\n").contains("Enumerable is not a class"));
    }

    #[test]
    fn superclass_clause_on_a_builtin_reopen_must_match() {
        // A MATCHING clause (`String < Object`) is accepted (D3); a wrong one
        // raises CRuby's `superclass mismatch`.
        let a = analyze_src("class String < Object\n  def x\n    1\n  end\nend\n");
        assert_eq!(class_named(&a, "String"), STRING_CLASS);
        assert!(
            analyze_err("class String < Array\n  def x\n    1\n  end\nend\n")
                .contains("superclass mismatch for class String")
        );
    }

    #[test]
    fn operator_definitions_on_builtins_are_rejected() {
        assert!(
            analyze_err("class Integer\n  def +(other)\n    0\n  end\nend\n")
                .contains("defining operator `+`")
        );
        assert!(
            analyze_err("class String\n  def ==(other)\n    true\n  end\nend\n")
                .contains("defining operator `==`")
        );
    }

    #[test]
    fn ivars_in_a_builtin_reopen_are_rejected() {
        assert!(
            analyze_err("class String\n  def remember\n    @seen = 1\n  end\nend\n")
                .contains("no ivar storage")
        );
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

    /// The two definition forms differ in CREF only: textual nesting sees
    /// the enclosing scope, the qualified form does not (oracle-verified
    /// NameError in real Ruby).
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

    #[test]
    fn qualified_definition_with_unknown_prefix_is_an_error() {
        assert!(
            analyze_err("class Nowhere::Item\nend\n").contains("unknown class/module `Nowhere`")
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

    /// The full CRuby-exact chains for the classes whose hierarchy Phase
    /// 17.1 corrected (oracle: ruby 4.0.5 `.ancestors`).
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
