//! The minimal analyze pass: walks the top-level `Hir::Program` statements
//! once (no fixpoint loop -- see the plan's stated scope-cut) and registers
//! every `ClassDef`/`DefMethod` into a `Compiler`, mirroring zeo's
//! `walk_scope`/`register_locals`/`resolve_parents` (a tiny slice of them).
//! Structured as a single pass function rather than the predecessor's
//! 128-iteration fixpoint loop because nothing yet needs mutual
//! recursion between inference results -- but the shape (one function that
//! walks the whole program and mutates a `Compiler`) is exactly what a real
//! fixpoint would wrap in `for iter in 0..128 { ... }` later.

pub(crate) mod captures;
pub(crate) mod class_query;
pub(crate) mod class_reach;
mod classes;
pub(crate) mod constfold;
pub(crate) mod coverage;
pub(crate) mod def_hooks;
pub(crate) mod fastpath;
pub(crate) mod local_storage;
mod locals;
mod methods;
mod scans;
pub(crate) mod source;
mod static_guards;
pub(crate) mod svars;
mod top_stmts;
use classes::*;
pub(crate) use methods::*;
pub(crate) use scans::*;
use static_guards::*;
use top_stmts::*;
pub(crate) mod mro;
mod pkg_iface;
pub(crate) mod alias_reveals;
pub(crate) mod dyn_defs;
pub(crate) mod redefs;

use crate::analyze_error::AnalyzeError;
use crate::compiler::{AccessorKind, AccessorShape, ClassId, Compiler, OBJECT_CLASS, Scope};
use crate::compiler::{FMap, FSet};
use crate::hir::{ArrayElem, Hir, HirNode, NodeId, Params, ScopeKind, StrPart, Visibility};
use crate::types::TyKind;

pub struct Analyzed {
    pub compiler: Compiler,
    /// Top-level statements that aren't class definitions -- the body of
    /// generated `fn main()`.
    pub main_statements: Vec<NodeId>,
    /// The same per-local `TyKind` tracking `Scope::local_types` does for a
    /// method body, but for `main_statements` -- there's no `Scope` for the
    /// top level to hang this off of.
    pub main_local_types: FMap<String, TyKind>,
    /// The compiled-in load path (see `LoaderState::feature_units`): each unit's
    /// top-level statements under EVERY feature name a `require` can spell
    /// for it. Walked exactly like `main_statements` -- their classes
    /// register at startup -- but emitted as a function the runtime calls on
    /// demand.
    pub feature_units: Vec<(Vec<String>, String, Vec<NodeId>)>,
    /// Load-path files zeo could not lower -- see `LoaderState::declined_units`.
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
    main_local_types: FMap<String, TyKind>,
    feature_units: Vec<(Vec<String>, String, Vec<NodeId>)>,
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
    let mut shell_kinds = FMap::default();
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
    for unit in &compiler.hir.loader.feature_units {
        collect_shell_kinds(&compiler.hir, &unit.body, &[], 0, &mut shell_kinds);
    }
    compiler.shell_kinds = shell_kinds;
    let mut aliases = FMap::default();
    collect_top_level_const_aliases(&compiler.hir, &statements, &mut aliases);
    compiler.top_level_const_aliases = aliases;
    let mut scoped_aliases = FMap::default();
    collect_const_aliases(&compiler.hir, &statements, &[], 0, &mut scoped_aliases);
    for unit in &compiler.hir.loader.feature_units {
        collect_const_aliases(&compiler.hir, &unit.body, &[], 0, &mut scoped_aliases);
    }
    compiler.const_aliases = scoped_aliases;
    // `ruby2_keywords def m(*a)` -- flag the `def` before anything registers
    // it, so the Scope carries the mark. Deliberately NOT a lowering rewrite:
    // the directive is a real `Module#ruby2_keywords` call, and consuming it
    // changed how the class-body walk saw the statement (delegate.rb's
    // `method_missing` stopped reaching WeakRef's instances).
    mark_ruby2_keywords_defs(&mut compiler.hir);

    // ONE flat sweep of the node arena serves every whole-arena question --
    // assigned const names, runtime patch verbs, top-level const initializers.
    let facts = collect_arena_facts(&compiler.hir);
    compiler.assigned_const_names = facts.assigned_consts;
    compiler.runtime_patches = facts.patched_names;
    compiler.runtime_patches_any_name = facts.patches_any_name;
    compiler.program_freezes = facts.freezes;
    compiler.unique_top_const_inits = facts
        .top_const_inits
        .into_iter()
        .filter_map(|(name, value)| Some((name, value?)))
        .collect();
    compiler.const_write_sites = facts.const_write_sites;
    compiler.global_write_sites = facts.global_write_sites;
    compiler.const_set_sites = facts.const_set_sites;

    // A merged package's whole-program facts union in
    // BEFORE any fold fires -- its writers are as real as this arena's,
    // they just have no nodes here. Every field is monotone-conservative:
    // the union can only fold less, never differently.
    let merged: Vec<crate::package::MFacts> = compiler
        .hir
        .pkg_merge
        .iter()
        .map(|m| m.facts.clone())
        .collect();
    for f in merged {
        compiler.runtime_patches.extend(f.patched_names);
        compiler.runtime_patches_any_name |= f.patches_any_name;
        compiler.program_freezes |= f.freezes;
        compiler.seed_world_bits(
            f.defines_bang,
            f.blank_slate_possible,
            f.moved_receiver_possible,
        );
        // The package's constants land when its units run, exactly the
        // shape `unrun_unit_consts` already describes (parse seeded that
        // set for the loader's own folds); `assigned_const_names` is what
        // `const_ever_written` and the existence folds consult, and
        // over-collection is its safe direction.
        compiler
            .assigned_const_names
            .extend(f.const_names.iter().cloned());
        compiler
            .hir
            .loader
            .unrun_unit_consts
            .extend(f.const_names);
        compiler.external_global_writers.extend(f.global_names);
    }

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
    let statements: Vec<NodeId> = statements.into_iter().collect();
    let is_pre_exec =
        |compiler: &Compiler, s: NodeId| matches!(compiler.hir[s], HirNode::PreExec(_));
    for (idx, &stmt) in statements.iter().enumerate() {
        if idx >= builtin_exceptions_len {
            break;
        }
        process_top_stmt(compiler, stmt, true, &mut main_statements, &mut pre_exec)?;
    }
    if builtin_exceptions_len < statements.len() {
        pin_builtin_exceptions_tail(compiler)?;
        tail_pinned = true;
    }
    // The `BEGIN` bodies BEFORE the main statements, because that is the order
    // ruby runs them in and the walk's own order is what every def-ordering
    // fact is counted from -- `seq`, and through it which of a class's
    // definitions `method_added` still has in its future. Walking them at
    // their source position instead reported a `def` inside a `BEGIN` to a
    // `method_added` hook the main program had not installed yet.
    //
    // Nothing moves for a program without a `BEGIN` block: the partition is
    // stable and the predicate is false for every statement.
    let hoisted_start = compiler.top_level_defs.len();
    for &stmt in statements.iter().skip(builtin_exceptions_len) {
        if is_pre_exec(compiler, stmt) {
            process_top_stmt(compiler, stmt, false, &mut main_statements, &mut pre_exec)?;
        }
    }
    let hoisted_defs = hoisted_start..compiler.top_level_defs.len();
    for &stmt in statements.iter().skip(builtin_exceptions_len) {
        if !is_pre_exec(compiler, stmt) {
            process_top_stmt(compiler, stmt, false, &mut main_statements, &mut pre_exec)?;
        }
    }
    // Every `BEGIN` body runs first, ahead of the main program -- see
    // `pre_exec`'s declaration. The prepend moves every def recorded against
    // the MAIN list, so their `at` indices move with it; a def hoisted out of
    // a `BEGIN` body already counts from the prefix and stays put.
    if !pre_exec.is_empty() {
        let shift = pre_exec.len();
        for (i, d) in compiler.top_level_defs.iter_mut().enumerate() {
            if !hoisted_defs.contains(&i) {
                d.at += shift;
            }
        }
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
    let mut declined_units = std::mem::take(&mut compiler.hir.loader.declined_units);
    let mut feature_units = Vec::new();
    // Everything a unit's walk REGISTERS is registered-but-not-promised --
    // see `register_method`'s `runtime_conditional` marking.
    compiler.unit_walk = true;
    for (stream, unit) in std::mem::take(&mut compiler.hir.loader.feature_units)
        .into_iter()
        .enumerate()
    {
        compiler.unit_stream = Some(stream as u32);
        let mut stmts = Vec::new();
        let mut unit_pre_exec = Vec::new();
        let mut failed = None;
        let sites_before = compiler.class_body_sites.len();
        let defs_before = compiler.top_level_defs.len();
        let classes_before = compiler.classes.len();
        let scopes_before = compiler.scopes.len();
        for stmt in unit.body {
            if let Err(e) = process_top_stmt(compiler, stmt, false, &mut stmts, &mut unit_pre_exec)
            {
                failed = Some(e);
                break;
            }
        }
        match failed {
            None => {
                // This unit's top-level defs recorded their `at` against
                // `stmts`; resolve the stream sentinel to the unit's final
                // index and shift past the prepended pre_exec (see the main
                // list's own prepend above).
                let uidx = feature_units.len() as u32;
                for d in &mut compiler.top_level_defs[defs_before..] {
                    d.unit = Some(uidx);
                    d.at += unit_pre_exec.len();
                }
                // Same resolution for the classes this unit DEFINED: their
                // names appear when it runs. A class the main file already
                // registered is not in this range, so a unit that merely
                // REOPENS one leaves it eager, which is right -- it existed
                // before the unit did.
                for c in &mut compiler.classes[classes_before..] {
                    if c.unit == Some(crate::compiler::Compiler::UNIT_UNRESOLVED) {
                        c.unit = Some(uidx);
                    }
                }
                // ...and the same for its METHODS, wherever they landed: a
                // unit reopening a class the main file already defined adds
                // rows to a class outside the range above.
                for s in &mut compiler.scopes[scopes_before..] {
                    if s.unit == Some(crate::compiler::Compiler::UNIT_UNRESOLVED) {
                        s.unit = Some(uidx);
                    }
                }
                unit_pre_exec.append(&mut stmts);
                let mut names = vec![unit.feature];
                names.extend(unit.aliases);
                feature_units.push((names, unit.absolute, unit_pre_exec));
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
                // ...and its top-level defs roll back with its sites: their
                // `at` indices point into a statement list that was just
                // dropped, and a declined unit's defs never announce.
                compiler.top_level_defs.truncate(defs_before);
                tracing::warn!(
                    feature = unit.feature,
                    reason = e.message,
                    "analyze: unit declined"
                );
                declined_units.push((unit.feature, unit.absolute, e.message));
            }
        }
    }
    compiler.unit_walk = false;
    compiler.unit_stream = None;
    warn_on_colliding_unit_features(compiler, &feature_units);

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
    // Every `obj.extend(M)` in the program, whose call sites must ask the
    // overlay rather than fold against the receiver class's own body. Before
    // `materialize`, which is where the folded tables are built.
    defer_object_extends(compiler);

    // A REOPEN of a frozen class raises and its body never runs, so a method
    // only that body installs has to be retractable at run time.
    defer_reopen_only_defs(compiler);

    // A namespace the program ASKS about (`Outer.constants`) must not list a
    // class whose declaration has not run yet.
    conceal_observed_namespace_members(compiler);

    // Two static bodies for one BUILTIN-class name -- the host's and a
    // merged package's -- cannot hold their document order, so the shape
    // refuses by package name here, before materialization folds anything
    // against either body.
    pkg_iface::refuse_host_spine_redefinitions(compiler)?;

    mro::materialize(compiler, &main_statements)?;

    // Which redefinition timelines are observable and must be applied at
    // their document position. Before `def_hooks::resolve`: its `at` bumps
    // are what order a redefinition's install before its own hook report.
    redefs::resolve(compiler);

    // ...and which definitions a RUNTIME alias makes positional for the same
    // reason. After `redefs`: both splice into a site's statements and bump
    // the `at` of every later `def`, and the reveal has to land where the
    // redefinition splice left the position.
    alias_reveals::resolve(compiler);

    // ...and every `def` in a class body that installs methods at run time,
    // which is the third way a definition's position becomes observable.
    dyn_defs::resolve(compiler);

    // Which compiled definitions announce themselves. Runs here because it
    // needs `class_methods` flattened over the ancestry to see the hook, and
    // before `mark_inline_iter_sites` because it feeds `runtime_patches`.
    def_hooks::resolve(compiler, &mut main_statements, &mut feature_units);

    let main_local_types = locals::infer_locals(compiler, None, 0, &main_statements);

    // With every scope's local types final (reinfer ran inside materialize)
    // and the main scope's just computed, mark the typed-receiver iterator
    // sites codegen can fuse into native loops.
    mark_inline_iter_sites(compiler, &main_statements, &main_local_types);
    mark_accessor_sites(compiler, &main_statements, &main_local_types);
    mark_typed_call_sites(compiler, &main_statements, &main_local_types);
    compiler.runtime_eval = narrow_runtime_eval(compiler, &main_statements, &feature_units)
        || compiler
            .hir
            .pkg_merge
            .iter()
            .any(|m| m.facts.runtime_eval);
    // After `runtime_eval`: a program that compiles Ruby at run time can name
    // any class at all, and this reads that answer.
    compiler.reachable_builtins = class_reach::resolve(compiler);

    Ok(AnalyzedParts {
        main_statements,
        main_local_types,
        feature_units,
        declined_units,
    })
}

/// Two units claiming ONE feature spelling is a silent wrong answer, and it
/// has to be loud. `zeo_rt::features::UNITS` is a map, so a duplicate key used
/// to last-win without a word: `pub_grub/static_package_source.rb`'s
/// `require_relative 'rubygems'` registered the bare spelling `rubygems` for
/// pub_grub's own file, and a later `require "rubygems"` therefore ran that
/// file, answered TRUE, and recorded the feature loaded -- so no second
/// require could recover and `Gem` stayed undefined. Eight files are named
/// `version.rb` across the two vendored trees alone.
///
/// The registration order is the demand order, so the FIRST claim wins (see
/// `features::install_feature_units_c`) and the later one is what the warning
/// names. The demand recording is the real bug whenever this fires;
/// `repo_checks` asserts the bundled gems produce none.
fn warn_on_colliding_unit_features(
    compiler: &mut Compiler,
    feature_units: &[(Vec<String>, String, Vec<NodeId>)],
) {
    let mut claimed: FMap<&str, &str> = FMap::default();
    let mut collisions: Vec<(String, String, String)> = Vec::new();
    for (names, absolute, _) in feature_units {
        for name in names {
            match claimed.get(name.as_str()) {
                Some(&winner) if winner != absolute.as_str() => {
                    collisions.push((name.clone(), winner.to_string(), absolute.clone()));
                }
                Some(_) => {}
                None => {
                    claimed.insert(name.as_str(), absolute.as_str());
                }
            }
        }
    }
    for (feature, winner, loser) in collisions {
        compiler.hir.warnings.push(crate::diagnostics::CompileWarning {
            file: format!("{loser}.rb"),
            line: 0,
            message: format!(
                "`require \"{feature}\"` is claimed by two compiled-in files ({winner}.rb and {loser}.rb); it loads the first"
            ),
        });
    }
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
    main_local_types: &FMap<String, TyKind>,
) {
    use crate::compiler::InlineIterKind;

    /// One fused iterator method, as data: the receiver type that nominates
    /// it, the builtin class a redefinition would have to land on, every name
    /// that reaches it (aliases are SEPARATE method entries in Ruby, so each
    /// is listed and redefining any one suppresses the kind -- conservative,
    /// and vanishingly rare), the argument counts it accepts, and the block
    /// param counts it can bind.
    ///
    /// One table, two consumers: `scan` nominates from it and the suppression
    /// pass below filters by it. They used to be a match and a hand-written
    /// `Vec` of `reopened(..) || reopened(..)` unions -- two lists of the same
    /// twelve methods, which is exactly the shape that drifts.
    struct Fused {
        kind: InlineIterKind,
        ty: TyKind,
        class: &'static str,
        names: &'static [&'static str],
        args: &'static [usize],
        params: &'static [usize],
    }

    use InlineIterKind as K;
    #[rustfmt::skip]
    const FUSED: &[Fused] = &[
        Fused { kind: K::TimesInt, ty: TyKind::Int, class: "Integer",
                names: &["times"], args: &[0], params: &[0, 1] },
        Fused { kind: K::UptoInt, ty: TyKind::Int, class: "Integer",
                names: &["upto"], args: &[1], params: &[0, 1] },
        Fused { kind: K::DowntoInt, ty: TyKind::Int, class: "Integer",
                names: &["downto"], args: &[1], params: &[0, 1] },
        Fused { kind: K::StepInt, ty: TyKind::Int, class: "Integer",
                names: &["step"], args: &[1, 2], params: &[0, 1] },
        Fused { kind: K::RangeEachInt, ty: TyKind::Range, class: "Range",
                names: &["each"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayEach, ty: TyKind::Array, class: "Array",
                names: &["each"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayEachWithIndex, ty: TyKind::Array, class: "Array",
                names: &["each_with_index"], args: &[0], params: &[1, 2] },
        Fused { kind: K::ArrayMap, ty: TyKind::Array, class: "Array",
                names: &["map", "collect"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArraySelect, ty: TyKind::Array, class: "Array",
                names: &["select", "filter", "find_all"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayReject, ty: TyKind::Array, class: "Array",
                names: &["reject"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArraySum, ty: TyKind::Array, class: "Array",
                names: &["sum"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayFind, ty: TyKind::Array, class: "Array",
                names: &["find", "detect"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayAll, ty: TyKind::Array, class: "Array",
                names: &["all?"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayAny, ty: TyKind::Array, class: "Array",
                names: &["any?"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayNone, ty: TyKind::Array, class: "Array",
                names: &["none?"], args: &[0], params: &[0, 1] },
        Fused { kind: K::ArrayCount, ty: TyKind::Array, class: "Array",
                names: &["count"], args: &[0], params: &[0, 1] },
        // The accumulator and the element, both required: `inject` yields two
        // values, so a block that declares any other count takes the dynamic
        // row rather than binding a param the splice never fills.
        Fused { kind: K::ArrayInject, ty: TyKind::Array, class: "Array",
                names: &["inject", "reduce"], args: &[1], params: &[2] },
        Fused { kind: K::HashEach, ty: TyKind::Hash, class: "Hash",
                names: &["each", "each_pair"], args: &[0], params: &[0, 1, 2] },
    ];

    fn plain_positional(params: &Params) -> bool {
        params.optional.is_empty()
            && params.rest.is_none()
            && params.post.is_empty()
            && params.keywords.is_empty()
            && params.keyword_rest.is_none()
            && params.block.is_none()
            && params.required.iter().all(|p| !p.starts_with("__destr"))
    }

    /// The literal (or frozen-literal) type a value node pins, for the
    /// constant-receiver nomination below.
    fn literal_ty(compiler: &Compiler, id: NodeId) -> Option<TyKind> {
        match &compiler.hir[id] {
            HirNode::ArrayLit(..) => Some(TyKind::Array),
            HirNode::HashLit(..) => Some(TyKind::Hash),
            HirNode::IntegerLit(..) => Some(TyKind::Int),
            HirNode::RangeLit { .. } => Some(TyKind::Range),
            // `[...].freeze` -- the idiomatic constant spelling.
            HirNode::Call {
                receiver: Some(r),
                name,
                args,
                kwargs,
                block: None,
                block_arg: None,
                ..
            } if name == "freeze" && args.is_empty() && kwargs.is_empty() => {
                literal_ty(compiler, *r)
            }
            _ => None,
        }
    }

    fn scan(
        compiler: &Compiler,
        id: NodeId,
        locals: &FMap<String, TyKind>,
        const_types: &FMap<String, TyKind>,
        out: &mut FMap<NodeId, InlineIterKind>,
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
            && let HirNode::Block { params, .. } = &compiler.hir[*block]
            && plain_positional(params)
            // A local the scope types, or a constant a single top-level
            // literal assignment types. Both are BELIEFS the emitted
            // guard re-checks per execution -- a wrong one costs size,
            // never correctness (the tag test and `iter_inline_ok_for`
            // fall back to the dynamic row).
            && let Some(ty) = match &compiler.hir[*recv] {
                HirNode::LocalRead(rn) => locals.get(rn).copied(),
                HirNode::ClassRef(cn) => const_types.get(cn).copied(),
                _ => None,
            }
            && let Some(f) = FUSED.iter().find(|f| {
                f.ty == ty
                    && f.names.contains(&name.as_str())
                    && f.args.contains(&args.len())
                    && f.params.contains(&params.required.len())
            })
        {
            out.insert(*block, f.kind);
        }
        compiler.hir[id].for_each_child(&mut |n| scan(compiler, n, locals, const_types, out));
    }

    // Constant receivers (`OFFSETS.each { .. }`): a bare constant written
    // EXACTLY once, anywhere, with a literal value types its readers. A
    // second write -- or a scoped/dynamic one under the same name --
    // disqualifies the name rather than guessing which write wins.
    let mut const_types: FMap<String, TyKind> = FMap::default();
    let mut const_disqualified: FSet<String> = FSet::default();
    {
        let record = |compiler: &Compiler,
                      const_types: &mut FMap<String, TyKind>,
                      const_disqualified: &mut FSet<String>,
                      id: NodeId| {
            match &compiler.hir[id] {
                HirNode::ConstWrite {
                    scope: None,
                    name,
                    value,
                } => {
                    let dup = const_types.contains_key(name);
                    match (dup, literal_ty(compiler, *value)) {
                        (false, Some(ty)) => {
                            const_types.insert(name.clone(), ty);
                        }
                        _ => {
                            const_disqualified.insert(name.clone());
                        }
                    }
                }
                HirNode::ConstWrite {
                    scope: Some(_),
                    name,
                    ..
                }
                | HirNode::DynConstWrite { name, .. }
                | HirNode::ConstReadOrNil(_, name) => {
                    const_disqualified.insert(name.clone());
                }
                _ => {}
            }
        };
        fn walk(compiler: &Compiler, id: NodeId, f: &mut impl FnMut(&Compiler, NodeId)) {
            f(compiler, id);
            compiler.hir[id].for_each_child(&mut |n| walk(compiler, n, f));
        }
        let mut f =
            |c: &Compiler, id: NodeId| record(c, &mut const_types, &mut const_disqualified, id);
        for scope in &compiler.scopes {
            for &n in &scope.body {
                walk(compiler, n, &mut f);
            }
        }
        for &n in main_statements {
            walk(compiler, n, &mut f);
        }
    }
    for name in &const_disqualified {
        const_types.remove(name);
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
    //
    // Reads the ancestry `mro::materialize` already linearized and stored --
    // nothing between there and here touches `includes`/`prepends`/`parent`
    // (`redefs` and `def_hooks` rewrite method tables only), and re-expanding
    // it per query re-walked every module's own prepends and includes and
    // allocated a fresh `Vec`, twelve times over.
    //
    // Deliberately NOT `method_in_chain`, which searches the MATERIALIZED
    // table and would answer a different question: a `def each` under an
    // undecided guard is `runtime_conditional` and so is absent from that
    // table by design, yet it must still suppress fusion -- a fused loop is
    // not a call, so there is nothing left to de-optimize once the guard
    // turns out to be true.
    let reopened = |cname: &str, m: &str| {
        compiler.may_be_patched_at_runtime(m)
            || compiler.resolve_class(cname, &[], 0).is_some_and(|cid| {
                compiler.class(cid).ancestors.iter().any(|&a| {
                    compiler
                        .class(a)
                        .own_methods
                        .iter()
                        .any(|&s| compiler.scope(s).name == m)
                })
            })
    };
    let sup: Vec<K> = FUSED
        .iter()
        .filter(|f| f.names.iter().any(|m| reopened(f.class, m)))
        .map(|f| f.kind)
        .collect();
    let suppressed = |k: &K| sup.contains(k);
    compiler.times_literal_suppressed = suppressed(&K::TimesInt);
    compiler.range_each_literal_suppressed = suppressed(&K::RangeEachInt);

    let mut sites = FMap::default();
    for scope in &compiler.scopes {
        for &n in &scope.body {
            scan(compiler, n, &scope.local_types, &const_types, &mut sites);
        }
    }
    for &n in main_statements {
        scan(compiler, n, main_local_types, &const_types, &mut sites);
    }
    sites.retain(|_, k| !suppressed(k));
    compiler.inline_iter_sites = sites;
}

/// Nominate explicit-receiver accessor calls on statically-classed locals
/// (`node.nxt`, `obj.attr = v`) for the guarded runtime fold. The
/// receiver is a `LocalRead` typed `Object(cid)`, the name resolves in
/// cid's MATERIALIZED chain (which excludes `runtime_conditional` defs by
/// design) to a PUBLIC accessor scope, and the ivar has a real slot in
/// cid's layout. Writers only when attr-GENERATED: a hand-written
/// writer's raise carries its own frame, which the slot store cannot.
///
/// No patched/runtime suppression here, unlike the iterator pass: the
/// fold IS a call -- the runtime entry re-checks the gates, the class'
/// patched set, and the receiver's exact class per call, and its slow
/// arm is the full explicit send with the visibility barrier.
fn mark_accessor_sites(
    compiler: &mut Compiler,
    main_statements: &[NodeId],
    main_local_types: &FMap<String, TyKind>,
) {
    use crate::compiler::{AccessorKind, AccessorSite, ClassId};

    fn site_of(compiler: &Compiler, cid: ClassId, name: &str, argc: usize) -> Option<AccessorSite> {
        let (_owner, scope_id) = compiler.method_in_chain(cid, name)?;
        let scope = compiler.scope(scope_id);
        if scope.visibility != crate::hir::Visibility::Public {
            return None;
        }
        let shape = compiler.accessor_shape(cid, scope)?;
        let writer = match (shape.kind, argc) {
            (AccessorKind::Reader, 0) => false,
            (AccessorKind::Writer, 1) if shape.attr_generated => true,
            _ => return None,
        };
        let slot = class_query::slot_of(compiler, cid, &shape.ivar)?;
        Some(AccessorSite {
            cid,
            slot: slot as u32,
            writer,
        })
    }

    fn scan(
        compiler: &Compiler,
        id: NodeId,
        locals: &FMap<String, TyKind>,
        out: &mut FMap<NodeId, AccessorSite>,
    ) {
        if let HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block: None,
            block_arg: None,
            safe: false,
        } = &compiler.hir[id]
            && kwargs.is_empty()
            && args
                .iter()
                .all(|a| matches!(a, crate::hir::ArrayElem::Single(_)))
            && let HirNode::LocalRead(rn) = &compiler.hir[*recv]
            && let Some(TyKind::Object(cid)) = locals.get(rn).copied()
            && let Some(site) = site_of(compiler, cid, name, args.len())
        {
            out.insert(id, site);
        }
        compiler.hir[id].for_each_child(&mut |n| scan(compiler, n, locals, out));
    }

    let mut sites = FMap::default();
    for scope in &compiler.scopes {
        for &n in &scope.body {
            scan(compiler, n, &scope.local_types, &mut sites);
        }
    }
    for &n in main_statements {
        scan(compiler, n, main_local_types, &mut sites);
    }
    compiler.accessor_sites = sites;
}

/// Nominate explicit-receiver calls on statically-classed locals
/// (`obj.step(a, b)`) for the typed DIRECT call. The receiver is a
/// `LocalRead` typed `Object(cid)` -- node-keyed, so a block parameter
/// shadowing the name (invisible to `local_types`) can never mis-type a
/// site: the nomination names THIS node, and a shadowed body's reads are
/// different nodes scanned under the same (outer) map, where the guard's
/// class compare simply misses. The name must resolve in cid's
/// MATERIALIZED chain (which excludes `runtime_conditional` defs by
/// design) to a PUBLIC non-accessor scope, and no ancestor may carry a
/// runtime `undef` of it (`may_be_undefined_at_runtime` -- the documented
/// direct-call precondition).
///
/// No patched/runtime suppression, the accessor precedent: the emitted
/// site re-checks the whole gate word (any armed gate routes to the
/// dispatch arm) and the receiver's EXACT class per call, so a wrong
/// nomination costs size and a slow path, never a wrong answer. The
/// emitter separately requires a compiled `(cid, name)` body of plain
/// shape (`Emitter::typed_methods`) and stands down under
/// `ZEO_DEBUG=no-typed-calls`.
fn mark_typed_call_sites(
    compiler: &mut Compiler,
    main_statements: &[NodeId],
    main_local_types: &FMap<String, TyKind>,
) {
    use crate::compiler::ClassId;

    fn site_of(compiler: &Compiler, cid: ClassId, name: &str) -> Option<ClassId> {
        let (_owner, scope_id) = compiler.method_in_chain(cid, name)?;
        let scope = compiler.scope(scope_id);
        if scope.visibility != crate::hir::Visibility::Public {
            return None;
        }
        // Accessor rows have their own guarded fold (`accessor_sites`).
        if compiler.accessor_shape(cid, scope).is_some() {
            return None;
        }
        if compiler.may_be_undefined_at_runtime(cid, name) {
            return None;
        }
        Some(cid)
    }

    fn scan(
        compiler: &Compiler,
        id: NodeId,
        locals: &FMap<String, TyKind>,
        out: &mut FMap<NodeId, ClassId>,
    ) {
        if let HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            kwargs,
            block: None,
            block_arg: None,
            safe: false,
        } = &compiler.hir[id]
            && kwargs.is_empty()
            && args
                .iter()
                .all(|a| matches!(a, crate::hir::ArrayElem::Single(_)))
            && let HirNode::LocalRead(rn) = &compiler.hir[*recv]
            && let Some(TyKind::Object(cid)) = locals.get(rn).copied()
            && let Some(cid) = site_of(compiler, cid, name)
        {
            out.insert(id, cid);
        }
        compiler.hir[id].for_each_child(&mut |n| scan(compiler, n, locals, out));
    }

    let mut sites = FMap::default();
    for scope in &compiler.scopes {
        for &n in &scope.body {
            scan(compiler, n, &scope.local_types, &mut sites);
        }
    }
    for &n in main_statements {
        scan(compiler, n, main_local_types, &mut sites);
    }
    if crate::debug_flags::debug(crate::debug_flags::DebugFlag::TraceTyped) {
        for (n, cid) in &sites {
            eprintln!(
                "typed site {:?} -> class {} {}",
                n,
                cid.0,
                compiler.class(*cid).name
            );
        }
    }
    compiler.typed_call_sites = sites;
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn analyze_src(src: &str) -> Analyzed {
        let (hir, root) = crate::parse::parse_and_lower(src).expect("parse");
        analyze(hir, root).expect("analyze")
    }

    /// A program's own `def times` stands the fused loop down.
    #[test]
    fn a_program_body_still_suppresses_fusion() {
        let a =
            analyze_src("class Integer\n  def times\n    self\n  end\nend\n3.times { |i| i }\n");
        assert!(
            a.compiler.times_literal_suppressed,
            "a program's own `def times` must still stand fusion down"
        );
    }

    pub(super) fn analyze_err(src: &str) -> String {
        let (hir, root) = crate::parse::parse_and_lower(src).expect("parse");
        match analyze(hir, root) {
            Ok(_) => panic!("expected an analyze error"),
            Err(e) => e.to_string(),
        }
    }

    /// `"<class>: <message>"` for the definition `raise` this program compiles
    /// to -- the reading side of [`raise_instead_of_defining`].
    pub(super) fn analyze_raise(src: &str) -> String {
        let a = analyze_src(src);
        let hir = &a.compiler.hir;
        let mut stack = a.main_statements.clone();
        while let Some(id) = stack.pop() {
            hir[id].for_each_child(&mut |c| stack.push(c));
            let HirNode::Call { name, args, .. } = &hir[id] else {
                continue;
            };
            if name != "raise" {
                continue;
            }
            let [ArrayElem::Single(class), ArrayElem::Single(message)] = args[..] else {
                continue;
            };
            let (HirNode::ClassRef(class), HirNode::StringLit(parts)) =
                (&hir[class], &hir[message])
            else {
                continue;
            };
            let text: String = parts
                .iter()
                .map(|p| match p {
                    StrPart::Lit(s) => s.as_str(),
                    StrPart::Bytes(_) | StrPart::Interp(_) => "",
                })
                .collect();
            return format!("{class}: {text}");
        }
        panic!("no definition raise emitted for: {src}");
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

    /// A REOPEN's `include` applies where it stands, so it lowers to the
    /// runtime send rather than joining `ClassInfo::includes` -- a call written
    /// between the two bodies must not already see the second module. Only the
    /// FIRST body's mixin is a compile-time ancestry edit.
    #[test]
    fn a_reopens_include_does_not_join_the_static_ancestry() {
        let a = analyze_src(
            "module M1\nend\nmodule M2\nend\nclass Foo\n  include M1\nend\nclass Foo\n  include M2\nend\n",
        );
        let ci = a.compiler.class(class_named(&a, "Foo"));
        assert_eq!(ci.includes().collect::<Vec<_>>(), [class_named(&a, "M1")]);
    }

    /// The reopen guards, each mirroring CRuby's own TypeError
    /// (oracle-verified messages).
    #[test]
    fn reopen_guards_mirror_ruby_type_errors() {
        assert!(
            analyze_raise(
                "class Base\nend\nclass Other\nend\nclass Sub < Base\nend\nclass Sub < Other\nend\n"
            )
            .contains("TypeError: superclass mismatch for class Sub")
        );

        assert!(analyze_raise("class Foo\nend\nmodule Foo\nend\n").contains("Foo is not a module"));
        assert!(analyze_raise("module Bar\nend\nclass Bar\nend\n").contains("Bar is not a class"));
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
    use super::tests::{analyze_raise, analyze_src, class_named};
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
        assert!(analyze_raise("module String\nend\n").contains("String is not a module"));
        assert!(analyze_raise("class Enumerable\nend\n").contains("Enumerable is not a class"));
    }

    #[test]
    fn superclass_clause_on_a_builtin_reopen_must_match() {
        // A MATCHING clause (`String < Object`) is accepted; a wrong one
        // raises CRuby's `superclass mismatch`.
        let a = analyze_src("class String < Object\n  def x\n    1\n  end\nend\n");
        assert_eq!(class_named(&a, "String"), STRING_CLASS);
        assert!(
            analyze_raise("class String < Array\n  def x\n    1\n  end\nend\n")
                .contains("TypeError: superclass mismatch for class String")
        );
    }

    /// Only a receiver a call site can type `Int` or `Float` has an operator
    /// fast path ahead of the reopened-builtin dispatch arm, so only those
    /// classes -- and the rest of their MRO -- suppress that operator's fast
    /// path when reopened. Other builtins' operator reopens already win via
    /// the reopened-builtin arm and record nothing.
    #[test]
    fn operator_definitions_on_the_numeric_mro_suppress_the_fast_path() {
        let a = analyze_src("class Integer\n  def +(other)\n    0\n  end\nend\n");
        assert!(a.compiler.redefined_int_ops.contains("+"));
        assert!(!a.compiler.redefined_float_ops.contains("+"));
        // `Comparable` sits on BOTH numeric MROs.
        let a = analyze_src("module Comparable\n  def <(other)\n    true\n  end\nend\n");
        assert!(a.compiler.redefined_int_ops.contains("<"));
        assert!(a.compiler.redefined_float_ops.contains("<"));
        let a = analyze_src("class String\n  def %(other)\n    self\n  end\nend\n");
        assert!(a.compiler.redefined_int_ops.is_empty());
        let a = analyze_src("class Set\n  def <<(other)\n    self\n  end\nend\n");
        assert!(a.compiler.redefined_int_ops.is_empty());
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

/// The narrowed answer to "can this program compile Ruby at RUN time" -- see
/// [`Compiler::runtime_eval`].
///
/// `Hir::uses_runtime_eval` matches a call's NAME. This walks the same nodes
/// with the class chain in hand and drops the ones a USER method answers: a
/// receiverless call resolves the way dispatch resolves it, so a `load` inside
/// a class that defines `load` never reaches Kernel's.
///
/// Only receiverless calls are narrowed. `Binding#eval` is a real eval on an
/// explicit receiver, and no static rule separates it from `obj.eval` on a
/// user object without types -- so those keep the conservative answer.
fn narrow_runtime_eval(
    compiler: &Compiler,
    main_statements: &[NodeId],
    units: &[(Vec<String>, String, Vec<NodeId>)],
) -> bool {
    let hir = &compiler.hir;
    let mut pending: crate::compiler::FSet<NodeId> = crate::compiler::FSet::default();
    // An explicit receiver is never narrowed -- `Binding#eval` is real -- so
    // those go straight in and stay.
    for id in hir.node_ids() {
        if hir.eval_shaped(id).is_some() {
            pending.insert(id);
        }
    }
    for id in compiled_in_requires(compiler, units) {
        pending.remove(&id);
    }
    if pending.is_empty() {
        return false;
    }
    let mut answered_by_user = |class: Option<crate::compiler::ClassId>, stmts: &[NodeId]| {
        let Some(class) = class else { return };
        for &stmt in stmts {
            discount_user_answers(compiler, class, stmt, &mut pending);
        }
    };
    answered_by_user(Some(crate::compiler::OBJECT_CLASS), main_statements);
    for unit in &hir.loader.feature_units {
        let body = unit.body.clone();
        answered_by_user(Some(crate::compiler::OBJECT_CLASS), &body);
    }
    for scope in &compiler.scopes {
        let body = scope.body.clone();
        answered_by_user(scope.class, &body);
    }
    // What is left is what the binary carries the compiler for. Naming each
    // one is the only way to tell a real `eval` from a name-only match --
    // `ZEO_LOG=zeo=debug` prints them.
    for &id in &pending {
        let (name, _) = compiler
            .hir
            .eval_shaped(id)
            .expect("pending is eval-shaped");
        match crate::analyze::source::source_location(compiler, id) {
            Some((file, line)) => {
                tracing::debug!(target: "zeo::eval", "runtime-eval site: {name} at {file}:{line}");
            }
            None => tracing::debug!(target: "zeo::eval", "runtime-eval site: {name}"),
        }
    }
    !pending.is_empty()
}

/// Every `require` node whose literal feature this compile already emitted as
/// a unit -- so the call reaches `zeo_rt::features::load_feature`, which runs
/// before the on-disk tier that needs a compiler.
///
/// Only a plain `require` with a LITERAL argument qualifies, and only when the
/// name matches a unit exactly. `require_relative` resolves against the
/// calling file's directory, which is a run-time fact; `load` re-executes and
/// asks DISK first, so its unit is not the row it takes. Both keep the
/// conservative answer.
fn compiled_in_requires(
    compiler: &Compiler,
    units: &[(Vec<String>, String, Vec<NodeId>)],
) -> Vec<NodeId> {
    let hir = &compiler.hir;
    if units.is_empty() {
        return Vec::new();
    }
    let mut names: crate::compiler::FSet<&str> = crate::compiler::FSet::default();
    for (spellings, absolute, _) in units {
        names.insert(absolute.as_str());
        names.extend(spellings.iter().map(String::as_str));
    }
    hir.node_ids()
        .filter(|&id| {
            let HirNode::Call {
                name,
                receiver: None,
                args,
                ..
            } = &hir[id]
            else {
                return false;
            };
            if name != "require" {
                return false;
            }
            let [crate::hir::ArrayElem::Single(arg)] = args[..] else {
                return false;
            };
            crate::lower::eval_splice::literal_string_text(hir, arg)
                .is_some_and(|f| names.contains(f.strip_suffix(".rb").unwrap_or(&f)))
        })
        .collect()
}

/// Drop from `pending` every receiverless eval-shaped call under `id` that a
/// user method of `class`'s chain answers. Stops at a nested `def`, which has
/// a `Scope` of its own and a class of its own.
fn discount_user_answers(
    compiler: &Compiler,
    class: crate::compiler::ClassId,
    id: NodeId,
    pending: &mut crate::compiler::FSet<NodeId>,
) {
    if pending.is_empty() {
        return;
    }
    if let Some((name, false)) = compiler.hir.eval_shaped(id)
        && compiler.method_in_chain(class, name).is_some()
    {
        pending.remove(&id);
    }
    if matches!(compiler.hir[id], crate::hir::HirNode::DefMethod { .. }) {
        return;
    }
    let mut kids = Vec::new();
    compiler.hir[id].for_each_child(&mut |c| kids.push(c));
    for c in kids {
        discount_user_answers(compiler, class, c, pending);
    }
}
