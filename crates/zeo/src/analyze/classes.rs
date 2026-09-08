//! Class registration and the class-body walk: shells, reopens,
//! definition targets, compatibility checks, and the mixin deferrals.

use super::*;

/// Resolve a compact-path CONTAINER (`Gem::Security` in `Gem::Security::Policy`)
/// or, when it is defined ELSEWHERE in the program (present in
/// `Compiler::shell_kinds`) but not yet registered at this list position,
/// create it -- and any missing ancestor segment -- as a bare SHELL. A later
/// real definition reopens the shell through `register_class`'s reopen arm,
/// establishing its superclass/body (the same bare-open-then-reopen path a
/// `class Sub` held only to nest a class already takes). Returns `None` for a
/// genuinely-undefined container, so the caller keeps the "unknown
/// class/module" error that catches typos.
pub(super) fn resolve_or_create_container(
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
pub(super) fn resolve_or_create_lexical(
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
    // Spelled one at a time rather than as a list built up front: the first
    // candidate usually answers, and the rest were `format!`ed only to be
    // dropped. The bare name is the last candidate, where every lookup ends.
    for i in 0..=scopes.len() {
        let cand = match scopes.get(i) {
            Some(&s) => format!("{}::{name}", compiler.fq_name(s)),
            None => name.to_string(),
        };
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

/// One `class`/`module` definition site, threaded whole through the
/// registration pipeline (target resolution, reopen checking, creation,
/// body walk). Each field is a distinct piece of the site (same posture
/// as `register_method`).
pub(super) struct ClassRegistration<'a> {
    /// May be a qualified path (`"Store::Item"` -- the `class
    /// Store::Item ... end` form): the prefix must already resolve (real
    /// Ruby's own NameError posture), the leaf registers under it as
    /// namespace parent, and the class is marked `qualified_def` so its
    /// cref is just itself (see `ClassInfo::qualified_def`'s docs). A
    /// leading `::` anchors the definition at the top level from any
    /// nesting depth.
    pub(super) name: &'a str,
    pub(super) superclass: &'a Option<String>,
    pub(super) is_module: bool,
    pub(super) body: &'a [NodeId],
    /// The ENCLOSING lexical chain (outermost first, `Compiler::cref_of`'s
    /// order) -- empty at the top level -- used to resolve the
    /// qualified-form prefix, the superclass, and include/extend/prepend
    /// targets, exactly as real Ruby resolves each of those in the scope
    /// ENCLOSING the definition.
    pub(super) cref: &'a [ClassId],
    pub(super) box_id: u32,
    /// The site's own `ClassDef` marker, for document-order body
    /// execution (`Compiler::class_body_sites`).
    pub(super) def_node: Option<NodeId>,
    /// Rides through to every `def` the body walk registers -- a class
    /// under a guard zeo cannot decide registers, but nothing in it is
    /// promised (see `Scope::runtime_conditional`).
    pub(super) conditional: Conditional,
}

/// Registers one `class`/`module` definition (or REOPENING) into the
/// `Compiler`, recursively descending nested `ClassDef`s.
pub(super) fn register_class(
    compiler: &mut Compiler,
    reg: &ClassRegistration<'_>,
) -> Result<(), String> {
    tracing::trace!(
        class = reg.name,
        superclass = reg.superclass,
        at = ?reg.def_node.and_then(|n| crate::analyze::source::source_location(compiler, n)),
        "analyze: registering"
    );
    let Some(target) = resolve_definition_target(compiler, reg)? else {
        // Deferred to a runtime constant read -- nothing registered.
        return Ok(());
    };
    check_builtin_superclass_restatement(
        compiler,
        reg.name,
        reg.superclass,
        &target,
        reg.def_node,
    )?;
    let class_id = match target.existing {
        Some(cid) => check_reopen_compatibility(compiler, cid, reg, &target)?,
        None => create_class(compiler, reg, target)?,
    };
    walk_class_body(compiler, class_id, reg)
}

/// What a definition site names, resolved before any registration state is
/// written: the superclass, the lexical container, and the slot (if any)
/// this definition attaches to. `resolve_definition_target` answers `None`
/// when the whole site was deferred to a runtime constant read.
struct DefinitionTarget {
    resolved_superclass: Option<ClassId>,
    lexical_parent: Option<ClassId>,
    leaf: String,
    qualified_def: bool,
    existing: Option<ClassId>,
    overlay_root: Option<ClassId>,
}

fn resolve_definition_target(
    compiler: &mut Compiler,
    reg: &ClassRegistration<'_>,
) -> Result<Option<DefinitionTarget>, String> {
    let &ClassRegistration {
        name,
        superclass,
        is_module,
        cref,
        box_id,
        def_node,
        ..
    } = reg;
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
        .and_then(|s| resolve_superclass(compiler, s, name, cref, box_id));
    if let (Some(s), Some(def_node)) = (superclass, def_node)
        && resolved_superclass.is_none()
        && !compiler.assigns_const_path(s)
    {
        // A PACKAGE build must refuse instead: its world deliberately
        // excludes the foreign gems the HOST provides, so "resolves to
        // nothing here" says nothing about the program the artifact will
        // run in -- net-http's `class HTTP < Protocol` is real when
        // net-protocol loads. A dropped definition would ship a package
        // silently missing the class; the refusal drops the gem to its
        // source splice, where the whole-program answer is right.
        if compiler.hir.pkg_build.is_some() {
            return Err(format!(
                "unknown superclass `{s}` (defined outside this package)"
            ));
        }
        // Loud at `--log-level debug`, because this DISCARDS a whole
        // definition -- body, nested classes and all -- and the program
        // then behaves as if the `class` keyword were a bare constant read.
        // When the guess is right that is exactly ruby (irb's `class
        // CallTracer < ::CallTracer` after a failed require); when it is
        // wrong the class is silently missing, and nothing else says so.
        tracing::debug!(
            class = name,
            superclass = s,
            unit_walk = compiler.unit_walk,
            at = ?crate::analyze::source::source_location(compiler, def_node),
            "analyze: definition DROPPED -- superclass resolves to nothing"
        );
        defer_unresolved_directive(compiler, def_node, s);
        return Ok(None);
    }
    let path = crate::constpath::ConstPath::parse(name);
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
                // Same rule as the superclass arm above: the container may
                // be a foreign gem's class the host provides, so a package
                // build refuses rather than discard the definition.
                if compiler.hir.pkg_build.is_some() {
                    return Err(format!(
                        "unknown class/module `{prefix}` in `{name}` (defined outside this \
                         package)"
                    ));
                }
                defer_unresolved_directive(compiler, def_node, prefix);
                return Ok(None);
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
        None => (cref.last().copied(), name.to_string(), false),
    };
    // Reopening a BUILTIN class: `class String ... end` at the
    // top level ATTACHES to the existing builtin `ClassInfo` -- its methods
    // dispatch as value methods on the `RubyValue` itself (registered by
    // `clif::classes`), its `@@cvar`/`CONST` body
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
        })
        // A definition whose KIND disagrees with a still-GATED builtin slot
        // is no collision at all: without the require, CRuby has no such
        // constant, so `class JSON` in a program that never requires json
        // mints a fresh user class that shadows the dormant slot. A
        // MATCHING kind still attaches and materializes the slot (the
        // reopen arm below clears the gate) -- only the mismatch, which
        // that arm would reject with a TypeError CRuby never raises, is
        // exempted here.
        //
        // And a gated slot whose implementation this build does NOT provide
        // (its cargo feature is off, or ZEO_DISABLE_BUILTIN retired it)
        // never attaches at all: the native constructor behind the slot is
        // not there, so `class StringScanner` in a pure-Ruby replacement
        // must mint a real user class -- which is also CRuby's world, where
        // no such constant exists until a file defines one.
        .filter(|&cid| {
            let ci = compiler.class(cid);
            let provided = zeo_abi::feature_of_gated_class(cid)
                .is_none_or(crate::lower::features::zeo_provides);
            provided && (compiler.feature_active(cid) || ci.is_module == is_module)
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
    Ok(Some(DefinitionTarget {
        resolved_superclass,
        lexical_parent,
        leaf,
        qualified_def,
        existing,
        overlay_root,
    }))
}

/// A definition attaching to a builtin (or `Object`) may RESTATE the
/// builtin's superclass; CRuby accepts a matching clause and raises
/// `superclass mismatch` on a wrong one.
fn check_builtin_superclass_restatement(
    compiler: &mut Compiler,
    name: &str,
    superclass: &Option<String>,
    target: &DefinitionTarget,
    def_node: Option<NodeId>,
) -> Result<(), String> {
    if let Some(cid) = target.existing.or(target.overlay_root) {
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
            if let Some(s) = superclass {
                let want = target.resolved_superclass.ok_or_else(|| {
                    format!("unknown superclass `{s}` (must be defined earlier in the file)")
                })?;
                if compiler.class(cid).parent != Some(want) {
                    // A NAMESPACE PLACEHOLDER (`WeakRef`) carries no class of
                    // its own -- the vendored Ruby file is the definition, and
                    // its `< Delegator` ESTABLISHES the parent the ABI row
                    // deliberately left at the default. A second, different
                    // clause is a real mismatch again.
                    if !zeo_abi::is_namespace_placeholder(cid)
                        || compiler.class(cid).explicit_superclass
                    {
                        return Err(superclass_mismatch(compiler, def_node, name));
                    }
                    compiler.classes[cid.0 as usize].parent = Some(want);
                }
                compiler.classes[cid.0 as usize].explicit_superclass = true;
            }
        }
    }
    Ok(())
}

/// REOPENING: a second `class Foo`/`module Foo` MERGES into the
/// existing `ClassInfo` -- the body walk appends includes/body-statements
/// and registers methods with real Ruby's last-`def`-wins rule (see the
/// method arm). Guards mirror CRuby's own (all oracle-verified): the
/// definition KIND must match (`TypeError: Foo is not a module`), and a
/// superclass clause, if written at all, must resolve to the original
/// parent (`TypeError: superclass mismatch for class Foo`).
fn check_reopen_compatibility(
    compiler: &mut Compiler,
    cid: ClassId,
    reg: &ClassRegistration<'_>,
    target: &DefinitionTarget,
) -> Result<ClassId, String> {
    let &ClassRegistration {
        name,
        superclass,
        is_module,
        def_node,
        ..
    } = reg;
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
    if let Some(s) = superclass {
        // Same rule as a fresh definition: this "reopen" may be the
        // FIRST real definition, of a name some other file forward-
        // referenced into a shell, and ruby resolves the superclass
        // before binding the name. See `resolve_superclass`.
        let want = target.resolved_superclass.ok_or_else(|| {
            format!("unknown superclass `{s}` (must be defined earlier in the file)")
        })?;
        if compiler.class(cid).parent != Some(want) {
            // Two shapes reach here with `parent == Object` and no clause
            // ever written, and ruby tells them apart:
            //
            //   class Sub; end; class Sub < Base    a real bare DEFINITION,
            //                                       then a conflicting reopen
            //                                       -- ruby's own TypeError.
            //   <forward shell>; class Sub < Base   a name a later file
            //                                       defines, minted early so
            //                                       an earlier one can name it
            //                                       -- no ruby equivalent, and
            //                                       the reopen ESTABLISHES the
            //                                       link rather than
            //                                       conflicting with it.
            //
            // A genuine `class Sub < A` then `class Sub < B` errors on
            // `explicit_superclass` before either.
            // A bare definition does NOT conflict when the declaration
            // carrying the superclass is inside a UNIT. Which of the two runs
            // first is a runtime fact the compiler cannot know -- the same
            // reason a bare class in a unit does not fix the superclass (see
            // `bare_definition`'s own `!unit_walk`) -- so the declaration that
            // NAMES a parent is the real one and establishes the link.
            // bundler is the case: its `module Gem; class StubSpecification`
            // reopen registers bare, and rubygems' own
            // `class Gem::StubSpecification < Gem::BasicSpecification` arrives
            // from a unit, which read as a mismatch and killed
            // `require "bundler"`.
            let bare_conflict = compiler.class(cid).bare_definition && !compiler.unit_walk;
            if compiler.class(cid).explicit_superclass || bare_conflict {
                return Err(superclass_mismatch(compiler, def_node, name));
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
                return Err(superclass_mismatch(compiler, def_node, name));
            }
            compiler.classes[cid.0 as usize].parent = Some(want);
        }
        compiler.classes[cid.0 as usize].explicit_superclass = true;
    }
    // A NESTED reopen carries the full lexical chain a compact or
    // shell first sighting lacked (`module RSpec::Core::Formatters`
    // in one file, `module RSpec; module Core; module Formatters;
    // class BaseFormatter` in another): upgrade the class's cref so
    // bare-name resolution inside every body -- and inside every
    // class registered UNDER it -- walks the real enclosing scopes.
    // CRuby's nesting is per definition SITE; zeo's is per class,
    // and nested-wins is the approximation that keeps working code
    // working: the compact site's body then resolves MORE names than
    // CRuby's cut allows, never fewer.
    if !target.qualified_def && compiler.class(cid).qualified_def {
        let ci = &mut compiler.classes[cid.0 as usize];
        ci.qualified_def = false;
        ci.cref_parent = target.lexical_parent;
    }
    Ok(cid)
}

/// Fresh creation: resolves the parent (with the subclassable-builtin
/// gate), mints the `ClassInfo`, and stamps the definition-site facts
/// (`lexical_parent`/`cref_parent`/`qualified_def`/overlay). A fresh class
/// minted under `Conditional::Yes` is registered but not PROMISED -- see
/// `ClassInfo::runtime_conditional`.
fn create_class(
    compiler: &mut Compiler,
    reg: &ClassRegistration<'_>,
    target: DefinitionTarget,
) -> Result<ClassId, String> {
    let &ClassRegistration {
        superclass,
        is_module,
        cref,
        box_id,
        def_node,
        conditional,
        ..
    } = reg;
    let parent = if is_module {
        None
    } else {
        Some(match superclass {
            None => OBJECT_CLASS,
            // Resolved in the ENCLOSING scope (`cref`, not the
            // class being opened): real Ruby evaluates the
            // superclass expression before the new class exists.
            Some(s) => {
                let cid = target.resolved_superclass.ok_or_else(|| {
                    format!("unknown superclass `{s}` (must be defined earlier in the file)")
                })?;
                // Every built-in CLASS can be subclassed. The instance
                // shape differs -- a generic `ValueSubclass` payload for
                // most (`zeo_abi::is_payload_root`), a generated struct for
                // `Struct`/`Data`/`FFI::Struct`, a registry entry alone for
                // the immediates, the native type itself for the
                // receiver-honouring constructors -- and
                // `zeo_abi::NOT_PAYLOAD_ROOTS` is the one place that says
                // which is which.
                //
                // Only two superclasses are refused, and they are RUBY's own
                // refusals rather than a zeo limitation -- so each compiles
                // into the raise ruby makes (see `ruby_raises`).
                if compiler.class(cid).is_module {
                    let msg =
                        "superclass must be an instance of Class (given an instance of Module)"
                            .to_string();
                    ruby_raises(compiler, def_node, "TypeError", &msg);
                    return Err(msg);
                }
                // The one class CRuby itself refuses: a `Class` subclass
                // would need an allocator for class objects, and there is
                // none.
                if cid == crate::compiler::CLASS_CLASS {
                    let msg = "can't make subclass of Class".to_string();
                    ruby_raises(compiler, def_node, "TypeError", &msg);
                    return Err(msg);
                }
                cid
            }
        })
    };
    let cid = compiler.add_class(target.leaf, parent, is_module);
    let ci = &mut compiler.classes[cid.0 as usize];
    ci.runtime_conditional = conditional == Conditional::Yes;
    // Record whether `< Super` was actually WRITTEN, so a later reopen
    // can tell a bare opening (parent defaulted to Object) from a real
    // declaration -- see the reopen arm above.
    ci.explicit_superclass = superclass.is_some();
    // A bare `class X` FIXES the superclass at Object -- but only where the
    // order is known. In a lazily-loaded unit it is not: a gem that writes
    // `class X < Node` in one file and bare-reopens `class X` in an
    // autoload target registers both at analyze time, and which unit RUNS
    // first is a run-time fact. Registering the bare one first then made
    // the real declaration a `superclass mismatch`. Prism is the shape that
    // does it; the hazard belongs to every gem written that way.
    ci.bare_definition = superclass.is_none() && !is_module && !compiler.unit_walk;
    ci.lexical_parent = target.lexical_parent;
    ci.qualified_def = target.qualified_def;
    // The scope this definition is WRITTEN in, which is the naming
    // parent for a nested form and the enclosing scope for a
    // qualified one -- see `ClassInfo::cref_parent`.
    ci.cref_parent = match target.qualified_def {
        true => cref.last().copied(),
        false => target.lexical_parent,
    };
    ci.box_id = box_id;
    if let Some(root) = target.overlay_root {
        // The overlay carries the box's patches; instances keep
        // the root builtin's identity. `is_builtin` makes the
        // 16.3 machinery (operator/@ivar guards, value-method
        // emission, `__bm_` containers) apply unchanged.
        ci.is_builtin = true;
        ci.builtin_overlay = Some(root);
    }
    Ok(cid)
}

/// The body walk: records this definition site (`Compiler::class_body_sites`)
/// and registers everything the body declares -- methods, nested classes,
/// mixins, visibility, aliases -- pushing whatever must run at document
/// position onto the site's statement list.
fn walk_class_body(
    compiler: &mut Compiler,
    class_id: ClassId,
    reg: &ClassRegistration<'_>,
) -> Result<(), String> {
    let &ClassRegistration {
        body,
        box_id,
        def_node,
        conditional,
        ..
    } = reg;
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
            installs: Vec::new(),
        });

    // Expand any dead-rescue `begin` (a resolved `require` guard) so a fallback
    // def inside it registers/emits like an ordinary class-body definition, and
    // inline any decidable-guard `if` wrapping a nested definition (a
    // target-version / feature-probe compat gate) into its taken branch.
    let body = splice_dead_rescues(compiler, body);
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
    // A class-body `extend M` is a compile-time ancestry edit applied at class
    // registration, so it runs before line 1. Ruby runs it where it is written,
    // and the difference is observable the moment the same body ALSO edits the
    // singleton chain at run time: `rb_include_module` searches the whole chain
    // and `rb_prepend_module` only the prepend area, so whichever verb runs
    // first is the one that finds an empty scope. A `singleton_class.prepend M`
    // above the `extend M` therefore gives the module ONE position in ruby and
    // two here.
    //
    // The runtime spelling is already right in either order, so a body that
    // does this sends the extend at its own position instead of registering it.
    let runtime_singleton_edit = body
        .iter()
        .any(|&stmt| mutates_own_singleton_at_runtime(compiler, stmt));

    // Whether the `attr_*` statement being walked gave way to a macro call --
    // its remaining accessors go with it.
    let mut attr_put_back = false;
    for &stmt in &body {
        // A `define_method(:x) { module M; end }` body is a block, so the
        // `module` keyword in it is legal and lands in THIS body's cref.
        // Registered up front, exactly as the `_` arm below registers one
        // nested in a plain block.
        if compiler
            .hir
            .has_flag(stmt, crate::hir::NodeFlag::BLOCK_BODIED_DEF)
        {
            register_nested_class_defs(compiler, stmt, &child_cref, box_id)?;
        }
        match &compiler.hir[stmt] {
            HirNode::DefMethod {
                name,
                is_class_method,
                ..
            } => {
                // The `attr_*` fold gives way when a module the class has
                // ALREADY `extend`ed writes its own macro: there the name is
                // an ordinary method call, and the accessors ruby ends up
                // with are whatever that method defined. Written before the
                // `extend`, the fold still stands -- ruby dispatches at the
                // statement's own position.
                match compiler.hir.attr_macro.get(&stmt).cloned() {
                    Some(Some((macro_name, macro_args))) => {
                        attr_put_back = extends_supply_macro(compiler, class_id, &macro_name);
                        if attr_put_back {
                            let args = macro_args
                                .iter()
                                .map(|a| {
                                    crate::hir::ArrayElem::Single(
                                        compiler.hir.push(HirNode::SymbolLit(a.clone())),
                                    )
                                })
                                .collect();
                            let call = compiler.hir.push(HirNode::Call {
                                receiver: None,
                                name: macro_name,
                                args,
                                kwargs: vec![],
                                block: None,
                                block_arg: None,
                                safe: false,
                            });
                            compiler.class_body_sites[site_idx].stmts.push(call);
                            continue;
                        }
                    }
                    // A later accessor of a statement already put back.
                    Some(None) if attr_put_back => continue,
                    _ => {}
                }
                // Under `Conditional::Yes` the whole body runs only if the
                // guard passed, so the def stays a STATEMENT: it emits a
                // runtime define at its document position -- the same
                // treatment a def inside a class-body `if` gets -- and no
                // `SiteDef` report row (the runtime define drives the hooks).
                // A def on a class a MERGED PACKAGE provides takes the same
                // road: its static rows live in the package's object, so
                // the reopen's body must install at run time, where the
                // install also patches the class and deoptimizes the
                // package's own guarded sites.
                if conditional == Conditional::Yes
                    || compiler.class(class_id).imported_pkg.is_some()
                {
                    register_body_def_method(compiler, class_id, stmt, conditional)?;
                    compiler.class_body_sites[site_idx].stmts.push(stmt);
                    continue;
                }
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
                    unit: None,
                    node: stmt,
                    name,
                    event: crate::compiler::DefEvent::Added,
                    singleton: is_class_method,
                };
                compiler.class_body_sites[site_idx]
                    .installs
                    .push(def.name.clone());
                compiler.class_body_sites[site_idx].defs.push(def);
                register_body_def_method(compiler, class_id, stmt, conditional)?;
            }
            // A nested `class`/`module` definition -- registered
            // recursively under this class's own cref. The `ClassDef` node
            // stays out of the flat `class_body_stmts` (nested classes are
            // ordinary `ClassInfo`s, not statements to re-execute through
            // that path) but IS recorded as a marker in this site's list:
            // real Ruby runs the inner body at its position inside the
            // outer body, and `clif::stmt::lower_stmt`'s `ClassDef` arm
            // recurses into the child's own site there.
            HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } => {
                let (name, superclass, body, is_module) =
                    (name.clone(), superclass.clone(), body.clone(), *is_module);
                compiler.class_body_sites[site_idx].stmts.push(stmt);
                register_class_or_raise(
                    compiler,
                    &ClassRegistration {
                        name: &name,
                        superclass: &superclass,
                        is_module,
                        body: &body,
                        cref: &child_cref,
                        box_id,
                        def_node: Some(stmt),
                        conditional,
                    },
                )?;
            }
            // The ancestry edit itself is compile-time; the node ALSO stays on
            // the site so codegen can fire the module's `included`/`extended`/
            // `prepended` hook at the mixin's own document position. Ruby runs
            // the hook after the edit, which is automatic here.
            HirNode::Include(m) => {
                let m = m.clone();
                if defer_guarded_mixin(
                    compiler,
                    site_idx,
                    stmt,
                    &m,
                    conditional,
                    &child_cref,
                    box_id,
                )? {
                    continue;
                }
                match resolve_module_target(compiler, &m, &child_cref, box_id) {
                    MixinTarget::Static(target) => {
                        // A module that overrides the PRIMITIVE decides for
                        // itself whether the mixin happens at all -- so the
                        // ancestry edit stops being a compile-time fact and
                        // codegen sends `append_features` at this position instead.
                        // The DECLARED edge still guides name resolution
                        // below the include (every such override calls
                        // `super` -- rss's ITunesChannelModel subclasses
                        // its own bases three lines under the include).
                        if compiler.overrides_mixin_primitive(target, "append_features") {
                            defer_mixin_to_runtime(compiler, target);
                            compiler
                                .declared_positional_mixins
                                .entry(class_id)
                                .or_default()
                                .push(target);
                        } else if is_positional_mixin_site(compiler, class_id, site_idx) {
                            // The edge belongs to the INCLUDE, not to how the
                            // mixin is staged, so it is recorded on this path
                            // too. `class Bundler::Thor; include Thor::Base`
                            // is a reopen (the nested Thor::* files created
                            // the shell first), and skipping it here is what
                            // left `Bundler::CLI` with 3 of its ~30 commands.
                            record_included_hook_extends(compiler, class_id, target, box_id, stmt);
                            defer_positional_mixin(compiler, site_idx, stmt, target);
                            continue;
                        } else {
                            let ci = &mut compiler.classes[class_id.0 as usize];
                            ci.mixin_order.push((target, false));
                            record_included_hook_extends(compiler, class_id, target, box_id, stmt);
                        }
                        compiler.class_body_sites[site_idx].stmts.push(stmt);
                    }
                    MixinTarget::DeferredRead => {
                        defer_in_class_body(compiler, class_id, site_idx, stmt, &m)
                    }
                    MixinTarget::Runtime => defer_runtime_mixin_in_body(compiler, site_idx, stmt),
                }
            }
            HirNode::Extend(m) => {
                let m = m.clone();
                if defer_guarded_mixin(
                    compiler,
                    site_idx,
                    stmt,
                    &m,
                    conditional,
                    &child_cref,
                    box_id,
                )? {
                    continue;
                }
                match resolve_module_target(compiler, &m, &child_cref, box_id) {
                    MixinTarget::Static(target) => {
                        // A module that installs methods at RUN time has no
                        // complete table to fold in here -- the send at this
                        // position takes whatever it holds when it runs.
                        if module_defines_methods_dynamically(compiler, target) {
                            defer_runtime_mixin_in_body(compiler, site_idx, stmt);
                            continue;
                        }
                        // Same rule as `Include`'s: an `extend_object` override
                        // owns the decision, so the static edit gives way to a
                        // send at this position.
                        if compiler.overrides_mixin_primitive(target, "extend_object") {
                            defer_mixin_to_runtime(compiler, target);
                        } else if runtime_singleton_edit {
                            defer_runtime_mixin_in_body(compiler, site_idx, stmt);
                            continue;
                        } else {
                            compiler.classes[class_id.0 as usize].extends.push(target);
                            compiler.extend_sites.insert((class_id, target), stmt);
                        }
                        compiler.class_body_sites[site_idx].stmts.push(stmt);
                    }
                    MixinTarget::DeferredRead => {
                        defer_in_class_body(compiler, class_id, site_idx, stmt, &m)
                    }
                    MixinTarget::Runtime => defer_runtime_mixin_in_body(compiler, site_idx, stmt),
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
                // A FEATURE UNIT's body may never run, and its `undef` of an
                // INHERITED row would otherwise retire that row from program
                // start -- a debug tracer nobody required took
                // `Module#method_added` away from every program. Same rule
                // the visibility arms below use: in a unit the retirement
                // stays positional and runs if and when the unit loads.
                let in_unit = compiler.unit_walk;
                if in_unit {
                    for n in &names {
                        compiler.runtime_patches.insert(n.clone());
                    }
                    let send =
                        crate::lower::defs::runtime_directive_spelling(&mut compiler.hir, stmt)?
                            .expect("`undef` has a runtime spelling");
                    compiler.class_body_sites[site_idx].stmts.push(send);
                }
                let at = compiler.class_body_sites[site_idx].stmts.len();
                for name in &names {
                    let def = crate::compiler::SiteDef {
                        seq: next_def_seq(compiler),
                        at,
                        unit: None,
                        node: stmt,
                        name: name.clone(),
                        event: crate::compiler::DefEvent::Undefined,
                        singleton: false,
                    };
                    compiler.class_body_sites[site_idx]
                        .installs
                        .push(def.name.clone());
                    compiler.class_body_sites[site_idx].defs.push(def);
                }
                if !in_unit {
                    compiler.classes[class_id.0 as usize]
                        .undefined
                        .extend(names);
                }
            }
            // The class-method half. See `HirNode::ClassMethodUndef`. No
            // `SiteDef` rows: those drive the instance-side `method_undefined`
            // hook and reopen ordering, and the singleton form has neither.
            HirNode::ClassMethodUndef(names) => {
                let names = names.clone();
                // In a REOPEN the retirement has a position: a call written
                // between the two bodies still answers. Recording it in
                // `class_undefined` applies it from program start.
                if is_reopen_site(compiler, class_id, site_idx) {
                    for n in &names {
                        compiler.runtime_patches.insert(n.clone());
                    }
                    compiler.class_body_sites[site_idx].stmts.push(stmt);
                } else {
                    // The compile-time half only reaches names a user `def
                    // self.x` wrote: the walk it feeds skips SCOPES, and a
                    // builtin class method has none. `undef_method :new` has
                    // to retire `new` too, so the runtime tombstone rides
                    // along -- the same row the reopen branch above writes,
                    // and the one that already makes `class << Bar;
                    // undef_method :new` work.
                    for n in &names {
                        compiler.runtime_patches.insert(n.clone());
                    }
                    compiler.class_body_sites[site_idx].stmts.push(stmt);
                    compiler.classes[class_id.0 as usize]
                        .class_undefined
                        .extend(names);
                }
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
                let entry = (
                    new_name.clone(),
                    old_name,
                    singleton,
                    seq,
                    compiler.unit_stream,
                );
                let new_name_owned = new_name;
                let def = crate::compiler::SiteDef {
                    seq,
                    at: compiler.class_body_sites[site_idx].stmts.len(),
                    unit: None,
                    node: stmt,
                    name: new_name_owned,
                    event: crate::compiler::DefEvent::Added,
                    singleton,
                };
                compiler.class_body_sites[site_idx]
                    .installs
                    .push(def.name.clone());
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
                // A FEATURE UNIT's body may never run (or run late), so its
                // re-marks cannot join the start-of-program override rows --
                // they stay positional, applied if and when the unit loads.
                if own || compiler.unit_walk {
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
                // Same OWN and unit rules as the instance half: a re-mark of
                // a class method an EARLIER body defined is positional, so
                // `Vault.combination` between a `private_class_method` and a
                // reopen's `public_class_method` still raises. `Protected`
                // has no runtime class-method application path and no corpus
                // case, so it keeps the static row either way.
                let own = compiler.classes[class_id.0 as usize]
                    .own_class_methods
                    .iter()
                    .any(|&s| compiler.scope(s).name == *name);
                if (own || compiler.unit_walk)
                    && !matches!(visibility, crate::hir::Visibility::Protected)
                {
                    compiler.runtime_patches.insert(name.clone());
                    compiler.class_body_sites[site_idx].stmts.push(stmt);
                } else {
                    compiler.classes[class_id.0 as usize]
                        .class_visibility_overrides
                        .push((name.clone(), *visibility));
                }
            }
            // `private_constant :A` / `public_constant :A`. Privacy is a
            // RUN-TIME flag a later directive restores, so the statement
            // stays and runs where it is written; the compile-time sets
            // are the folds' view of it (`const_visibility_names` says the
            // answer is positional, so a read of that name asks).
            HirNode::ConstantVisibility { names, private } => {
                let names = names.clone();
                let private = *private;
                let info = &mut compiler.classes[class_id.0 as usize];
                for name in &names {
                    info.const_visibility_names.insert(name.clone());
                    if private {
                        info.private_constants.insert(name.clone());
                    } else {
                        info.private_constants.remove(name);
                    }
                }
                compiler.class_body_sites[site_idx].stmts.push(stmt);
            }
            HirNode::Prepend(m) => {
                let m = m.clone();
                if defer_guarded_mixin(
                    compiler,
                    site_idx,
                    stmt,
                    &m,
                    conditional,
                    &child_cref,
                    box_id,
                )? {
                    continue;
                }
                match resolve_module_target(compiler, &m, &child_cref, box_id) {
                    MixinTarget::Static(target) => {
                        // A module that overrides the PRIMITIVE decides for
                        // itself whether the mixin happens at all -- so the
                        // ancestry edit stops being a compile-time fact and
                        // codegen sends `prepend_features` at this position instead.
                        if compiler.overrides_mixin_primitive(target, "prepend_features") {
                            defer_mixin_to_runtime(compiler, target);
                        } else if is_positional_mixin_site(compiler, class_id, site_idx) {
                            defer_positional_mixin(compiler, site_idx, stmt, target);
                            continue;
                        } else {
                            let ci = &mut compiler.classes[class_id.0 as usize];
                            ci.mixin_order.push((target, true));
                        }
                        compiler.class_body_sites[site_idx].stmts.push(stmt);
                    }
                    MixinTarget::DeferredRead => {
                        defer_in_class_body(compiler, class_id, site_idx, stmt, &m)
                    }
                    MixinTarget::Runtime => defer_runtime_mixin_in_body(compiler, site_idx, stmt),
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
                // The guarded deferral serves this arm too, and BETTER than
                // the static path below: its `singleton_class.prepend(M)`
                // send hands the hooks the real singleton class at runtime,
                // so the hook-defining modules the static path must reject
                // simply work.
                if defer_guarded_mixin(
                    compiler,
                    site_idx,
                    stmt,
                    &m,
                    conditional,
                    &child_cref,
                    box_id,
                )? {
                    continue;
                }
                match resolve_module_target(compiler, &m, &child_cref, box_id) {
                    MixinTarget::Static(target) => {
                        // Both hooks take the SINGLETON class as their
                        // argument, which zeo has no compile-time class for --
                        // so a module that defines either stays a clean
                        // rejection instead of being handed the wrong receiver.
                        if defines_prepend_hook(compiler, target) {
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
                    MixinTarget::DeferredRead => {
                        defer_in_class_body(compiler, class_id, site_idx, stmt, &m)
                    }
                    // The singleton spelling (`singleton_class.prepend(M)`)
                    // hands the hooks the real singleton class at runtime, so
                    // the rejection above has nothing to guard here.
                    MixinTarget::Runtime => defer_runtime_mixin_in_body(compiler, site_idx, stmt),
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
            HirNode::Program(_)
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
            | HirNode::NotNil(_)
            | HirNode::If { .. }
            | HirNode::CaseWhen { .. }
            | HirNode::ArrayLit(_)
            | HirNode::HashLit(_)
            | HirNode::RangeLit { .. }
            | HirNode::StringLit(_)
            | HirNode::RegexpLit(..)
            | HirNode::LocalRead(_)
            | HirNode::LocalWrite(..)
            | HirNode::IvarRead(_)
            | HirNode::ClassVarRead(_)
            | HirNode::ClassRef(_)
            | HirNode::Call { .. }
            | HirNode::New { .. }
            | HirNode::SuperCall { .. }
            | HirNode::Block { .. }
            | HirNode::Lambda { .. }
            | HirNode::DefHook { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::MethodReveal(..)
            | HirNode::While { .. }
            | HirNode::Loop { .. }
            | HirNode::For { .. }
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
            | HirNode::Begin { .. }
            | HirNode::Retry
            | HirNode::GlobalRead(_)
            | HirNode::GlobalWrite(..)
            | HirNode::QualifiedConstRead(..)
            | HirNode::ConstReadOrNil(..)
            | HirNode::DynConstRead { .. }
            | HirNode::DynConstWrite { .. }
            | HirNode::PreExec(_)
            | HirNode::AliasGlobal(..)
            | HirNode::FeatureLoaded { .. }
            | HirNode::CExtLoaded { .. }
            | HirNode::LastMatchRef(_)
            | HirNode::Seq(_)
            | HirNode::FlipFlop { .. } => {
                // A reachable `C.prepend(M)` / `C.singleton_class.prepend(M)` in
                // a class body (e.g. connection_pool's
                // `Process.singleton_class.prepend(ForkTracker)`) is a static
                // ancestry edit on `C`, independent of the enclosing class --
                // recorded here (resolving names in this body's cref), emitting
                // nothing, exactly as at the top level.
                if try_prepend_call_edit(compiler, stmt, &child_cref, box_id) {
                    continue;
                }
                // `import_methods M` inside a `refine` block. The RUNTIME does
                // the copying; what the COMPILER needs is the name list, or the
                // call sites a `using` covers are never nominated and the
                // imported method is unreachable however well it was copied.
                record_imported_methods(compiler, class_id, stmt, &child_cref, box_id);
                if declines_a_singleton_prepend(compiler, stmt, &child_cref, box_id) {
                    // Runs at document position as an ordinary send; the
                    // de-opt makes static call sites see what it writes.
                    defer_singleton_prepend(compiler, stmt, &child_cref, box_id);
                }
                // One walk of the statement's nesting answers everything below.
                let nested = nested_stmts(&compiler.hir, stmt);
                // The statement ITSELF also counts for the questions that are
                // about a node rather than about what it contains -- a folded
                // guard can leave a bare mixin self-send standing AT `stmt`.
                let subtree: Vec<(NodeId, Reach)> = std::iter::once((stmt, Reach::DIRECT))
                    .chain(nested.iter().copied())
                    .collect();
                // A `def` nested in an `if`/`case` branch also runs at document
                // position (the taken branch's runtime `define_method` gives the
                // real body), but must ALSO be registered as an own method so
                // `instance_methods`/`extend` -- both resolved at COMPILE time --
                // can see it. This is what lets fileutils' platform-conditional
                // `StreamUtils_#fu_windows?` reach `FileUtils` via `extend`.
                register_conditional_defs(compiler, class_id, &subtree)?;
                // ...and a `class`/`module` nested in one of those branches --
                // an undecided `if`, a `begin` whose body may raise
                // (rubyntlm's `begin; OpenSSL::Cipher.new("rc4"); rescue;
                // class Rc4`), a block. Registration is a compile-time fact
                // about shape; the marker stays put and the body still runs at
                // its document position, exactly as at the top level.
                register_nested_class_defs_in(
                    compiler,
                    &nested,
                    &child_cref,
                    box_id,
                    Conditional::No,
                )?;
                // ...and the `refine` markers beside those holder modules,
                // which is how power_assert's whole refinement set reaches
                // registration from inside a runtime `if`. Same nesting, walked
                // once: the classes have to be registered before the markers,
                // not found by a second traversal.
                register_nested_refinements(compiler, class_id, &nested, &child_cref, box_id);
                // ...and, symmetrically, a guarded `undef` (`undef :to_a if
                // respond_to?(:to_a)`, drb) reaches here as a runtime
                // `undef_method` send. Whether it fires is a runtime fact, so
                // the names go on record and codegen stops emitting a DIRECT
                // call for them. See `ClassInfo::runtime_undefs`.
                // ...and the mixin SELF-SENDS in those branches -- the runtime
                // spellings `transform_conditional_class_body` rewrote the
                // branch's `include`/`extend`/`prepend` directives to. The
                // splice runs when the branch does; the call-site widening has
                // to happen NOW, or a statically-resolved call reaches the
                // class's own body underneath a prepend override.
                defer_runtime_mixin_sends(compiler, &subtree, &child_cref, box_id);
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

/// A mixin under a runtime-undecidable guard into an EXISTING class is a
/// runtime ancestry fact -- the guard decides whether the edit happens at
/// all, so no compile-time MRO can carry it. The directive rewrites to the
/// runtime self-send its class body serves (`runtime_meta::splice_mixin`
/// performs the edit and fires the hook, exactly when the guard passes),
/// after de-optimizing every call site the module's methods could newly
/// answer or override -- the `defer_singleton_prepend` treatment.
///
/// A runtime-conditional class skips all of this (`Ok(false)`, the static
/// edge stands): it is concealed until the same guard passes, so its static
/// edges are unobservable while the guard is false. `Ok(true)` means the
/// directive was consumed; the send now sits at its document position.
fn defer_guarded_mixin(
    compiler: &mut Compiler,
    site_idx: usize,
    stmt: NodeId,
    module: &str,
    conditional: Conditional,
    cref: &[ClassId],
    box_id: u32,
) -> Result<bool, String> {
    // The site owns its class (`ClassBodySite::class`), so the caller does
    // not restate the id.
    let class_id = compiler.class_body_sites[site_idx].class;
    if conditional != Conditional::Yes || compiler.class(class_id).runtime_conditional {
        return Ok(false);
    }
    match resolve_module_target(compiler, module, cref, box_id) {
        MixinTarget::Static(target) => {
            defer_mixin_to_runtime(compiler, target);
            let send = crate::lower::defs::runtime_directive_spelling(&mut compiler.hir, stmt)
                .expect("a mixin directive is not `refine`")
                .expect("every mixin directive has a runtime spelling");
            compiler.class_body_sites[site_idx].stmts.push(send);
        }
        // A module zeo cannot name at compile time de-optimizes EVERY name
        // instead -- a static fast path is not worth a wrong override.
        MixinTarget::DeferredRead | MixinTarget::Runtime => {
            defer_runtime_mixin_in_body(compiler, site_idx, stmt);
        }
    }
    Ok(true)
}

/// De-optimizes every mixin SELF-SEND anywhere in a class-body statement --
/// the `include M`/`extend M`/`prepend M`/`singleton_class.prepend(M)` calls
/// `transform_conditional_class_body` rewrote a branch's directives to. Every
/// reach counts, conditional or not: an `include` in a `case` arm, a `rescue`
/// clause or an `each` block edits the ancestry just as one in an `if` branch
/// does, and only walking `if` left the others' call sites folded against a
/// chain the module had since overridden. A module the send names but zeo
/// cannot resolve widens to every name, exactly as `defer_singleton_prepend`
/// widens.
fn defer_runtime_mixin_sends(
    compiler: &mut Compiler,
    subtree: &[(NodeId, Reach)],
    cref: &[ClassId],
    box_id: u32,
) {
    /// Whether the receiver is a mixin-capable `self` -- absent (the class
    /// body's own `self`) or the receiverless `singleton_class` read the
    /// singleton spelling dispatches through.
    fn self_like(compiler: &Compiler, receiver: Option<NodeId>) -> bool {
        match receiver {
            None => true,
            Some(r) => matches!(
                &compiler.hir[r],
                HirNode::Call {
                    receiver: None,
                    name,
                    args,
                    ..
                } if name == "singleton_class" && args.is_empty()
            ),
        }
    }
    for &(s, _) in subtree {
        let HirNode::Call {
            receiver,
            name,
            args,
            ..
        } = &compiler.hir[s]
        else {
            continue;
        };
        if !matches!(name.as_str(), "include" | "extend" | "prepend")
            || !self_like(compiler, *receiver)
        {
            continue;
        }
        for arg in args.clone() {
            let resolved = match arg {
                crate::hir::ArrayElem::Single(n) => const_node_class(compiler, n, cref, box_id),
                crate::hir::ArrayElem::Splat(_) => None,
            };
            match resolved {
                Some(m) => defer_mixin_to_runtime(compiler, m),
                None => compiler.runtime_patches_any_name = true,
            }
        }
    }
}

/// An `obj.extend(M)` with an EXPLICIT receiver -- the object half of the mixin
/// verbs, which [`defer_runtime_mixin_sends`] does not see because it only
/// walks class-body statements whose receiver is `self`.
///
/// `extend` splices M ahead of the receiver's own class in that ONE object's
/// lookup, so a call site zeo folded against the class body answers the wrong
/// body: `o.extend(Deco); o.render(x)` must reach `Deco#render`, not
/// `Plain#render`. The overlay already holds the copied rows
/// (`extend_object_default`); what is missing is that the site asks at all.
///
/// The cost is exactly proportional to the risk: a name no class in the
/// program defines is already a dynamic site, so listing it changes nothing.
///
/// An argument zeo cannot resolve to one module falls back to every MODULE's
/// own method names rather than [`Compiler::runtime_patches_any_name`] -- the
/// union is a superset of anything a real module argument could contribute,
/// and a module minted at run time (`Module.new { define_method ... }`) is
/// already covered by the redefinition verbs.
pub(super) fn defer_object_extends(compiler: &mut Compiler) {
    let mut resolved: Vec<ClassId> = Vec::new();
    let mut widen = false;
    for (_, node) in compiler.hir.iter_with_ids() {
        let HirNode::Call {
            receiver: Some(_),
            name,
            args,
            ..
        } = node
        else {
            continue;
        };
        if name != "extend" {
            continue;
        }
        for arg in args {
            let ArrayElem::Single(n) = arg else {
                widen = true;
                continue;
            };
            // Resolved against the ROOT cref: this sweep is flat, so a module
            // named relative to an enclosing body falls to the leaf-name search
            // below rather than mis-resolving.
            if let Some(m) = const_node_class(compiler, *n, &[], 0) {
                resolved.push(m);
                continue;
            }
            match leaf_const_name(&compiler.hir, *n) {
                Some(leaf) => {
                    let matches: Vec<ClassId> = compiler
                        .classes
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| {
                            c.name == leaf || c.name.rsplit("::").next() == Some(leaf.as_str())
                        })
                        .map(|(i, _)| ClassId(i as u32))
                        .collect();
                    // A constant naming nothing zeo compiled cannot shadow a
                    // statically resolved body -- there is no such module.
                    resolved.extend(matches);
                }
                None => widen = true,
            }
        }
    }
    if widen {
        resolved.extend(
            (0..compiler.classes.len())
                .map(|i| ClassId(i as u32))
                .filter(|&c| compiler.class(c).is_module),
        );
    }
    resolved.sort_unstable_by_key(|c| c.0);
    resolved.dedup();
    for m in resolved {
        defer_mixin_to_runtime(compiler, m);
    }
}

/// Records a `import_methods M, ...` written in a refinement holder's body --
/// see [`ClassInfo::imported_modules`]. The call still runs (the runtime does
/// the actual copying); this is only what makes `refinement_defines` answer for
/// the imported names, which is what nominates a call site.
fn record_imported_methods(
    compiler: &mut Compiler,
    class_id: ClassId,
    stmt: NodeId,
    cref: &[ClassId],
    box_id: u32,
) {
    let HirNode::Call {
        receiver: None,
        name,
        args,
        ..
    } = &compiler.hir[stmt]
    else {
        return;
    };
    if name != "import_methods" {
        return;
    }
    let resolved: Vec<ClassId> = args
        .clone()
        .iter()
        .filter_map(|a| match a {
            crate::hir::ArrayElem::Single(n) => const_node_class(compiler, *n, cref, box_id),
            crate::hir::ArrayElem::Splat(_) => None,
        })
        .collect();
    compiler.classes[class_id.0 as usize]
        .imported_modules
        .extend(resolved);
}

/// Names that only a class's SECOND-or-later body installs, when the program
/// freezes anything at all. `Fz.freeze; class Fz; def x; end; end` is a
/// `FrozenError` in ruby and `x` never exists -- but zeo's compile-time tables
/// already carry it, so `guard_class_reopen` retires it at run time and the
/// call sites have to be asking rather than folded.
///
/// Gated on [`Compiler::program_freezes`]: without a `freeze` anywhere the
/// shape is unreachable and no name loses its static dispatch.
pub(super) fn defer_reopen_only_defs(compiler: &mut Compiler) {
    if !compiler.program_freezes {
        return;
    }
    let mut deferred: Vec<(ClassId, String)> = Vec::new();
    for mine in 0..compiler.class_body_sites.len() {
        let cid = compiler.class_body_sites[mine].class;
        let earlier: Vec<usize> = (0..mine)
            .filter(|&i| compiler.class_body_sites[i].class == cid)
            .collect();
        if earlier.is_empty() {
            continue;
        }
        for name in &compiler.class_body_sites[mine].installs {
            if !earlier
                .iter()
                .any(|&i| compiler.class_body_sites[i].installs.contains(name))
            {
                deferred.push((cid, name.clone()));
            }
        }
    }
    for (cid, name) in deferred {
        compiler.classes[cid.0 as usize].runtime_undefs.insert(name);
    }
}

/// An ancestry edit written in a REOPEN, applied where it stands instead of at
/// program start: the directive becomes its runtime send spelling (which
/// splices the overlay chain `ancestors_of_value` prefers) and the module's
/// method names leave the fold, so a call site asks rather than answering from
/// a table the edit is not in yet.
///
/// Narrower than [`defer_runtime_mixin`], which widens to EVERY name: the
/// module is resolved here, so only its own names need to leave.
/// Give `class_id` the class methods `target`'s `included` hook would extend
/// it with -- the ClassMethods idiom, resolved at compile time.
///
/// See [`extends_from_included_hook`] for why the edge cannot be seen any
/// other way. Constants are resolved in the MODULE's nesting, because that is
/// where the name is written.
fn record_included_hook_extends(
    compiler: &mut Compiler,
    class_id: ClassId,
    target: ClassId,
    box_id: u32,
    stmt: NodeId,
) {
    for name in extends_from_included_hook(compiler, target) {
        if let MixinTarget::Static(cm) = resolve_module_target(compiler, &name, &[target], box_id) {
            let ci = &mut compiler.classes[class_id.0 as usize];
            if !ci.extends.contains(&cm) {
                ci.extends.push(cm);
                // The `include`, not the `extend` inside the hook body: the
                // hook runs where the module is mixed in.
                compiler.extend_sites.insert((class_id, cm), stmt);
            }
        }
    }
}

fn defer_positional_mixin(compiler: &mut Compiler, site_idx: usize, stmt: NodeId, target: ClassId) {
    defer_mixin_to_runtime(compiler, target);
    // The DECLARED edge stays visible to name resolution: a superclass
    // written below this include resolves through the module's constants
    // at ruby's own document position. See
    // `Compiler::declared_positional_mixins`.
    let class = compiler.class_body_sites[site_idx].class;
    compiler
        .declared_positional_mixins
        .entry(class)
        .or_default()
        .push(target);
    let send = crate::lower::defs::runtime_directive_spelling(&mut compiler.hir, stmt)
        .expect("a mixin directive is not `refine`")
        .expect("every mixin directive has a runtime spelling");
    compiler.class_body_sites[site_idx].stmts.push(send);
}

/// Whether `site_idx` is a REOPEN of `class_id` -- some earlier site already
/// ran a body for it, so code could have run in between.
///
/// An ancestry edit written in a reopen is a runtime event with a position:
/// `class Thing; end; p Thing.new.respond_to?(:tag); class Thing; include
/// Extra; end` must answer `false` first. Recording the edit at compile time
/// applied it from program start. The `Include`/`Prepend` node stays in the
/// site's statements either way, so the splice happens where it is written;
/// what changes is that the compile-time tables no longer carry it.
fn is_reopen_site(compiler: &Compiler, class_id: ClassId, site_idx: usize) -> bool {
    compiler.class_body_sites[..site_idx]
        .iter()
        .any(|s| s.class == class_id)
}

/// Whether this mixin has to be spliced at RUN time rather than recorded in
/// the compile-time tables.
///
/// A reopen is the general reason (see [`is_reopen_site`]). A per-box builtin
/// OVERLAY is the second: it registers no entry of its own, so a compile-time
/// chain written against it names a class the runtime never hears of, and the
/// mixin was simply lost. The runtime splice keys the ROOT's chain by box,
/// which is where every reader looks.
fn is_positional_mixin_site(compiler: &Compiler, class_id: ClassId, site_idx: usize) -> bool {
    compiler.class(class_id).builtin_overlay.is_some()
        || is_reopen_site(compiler, class_id, site_idx)
}

/// Sets [`crate::hir::NodeFlag::RUBY2_KEYWORDS`] on every `def` a
/// `ruby2_keywords` directive names as its argument.
///
/// The CALL stays exactly where it was -- it is an ordinary
/// `Module#ruby2_keywords` send, and the class-body walk's treatment of the
/// statement must not change. Only the flag is added, and only the codegen
/// that decides whether a splat clears a captured keyword mark reads it.
pub(super) fn mark_ruby2_keywords_defs(hir: &mut Hir) {
    let mut defs: Vec<NodeId> = Vec::new();
    for (_, node) in hir.iter_with_ids() {
        let HirNode::Call {
            receiver: None,
            name,
            args,
            ..
        } = node
        else {
            continue;
        };
        if name != "ruby2_keywords" {
            continue;
        }
        for a in args {
            if let ArrayElem::Single(n) = a
                && matches!(hir[*n], HirNode::DefMethod { .. })
            {
                defs.push(*n);
            }
        }
    }
    for d in defs {
        hir.set_flag(d, crate::hir::NodeFlag::RUBY2_KEYWORDS);
    }
}

/// The verbs that read a namespace's constant LIST, so a class declared later
/// in the program must not already be in it.
const CONST_OBSERVERS: &[&str] = &["constants", "const_defined?"];

/// Conceal every class nested DIRECTLY under a namespace the program asks
/// about, so its constant appears where the declaration stands rather than at
/// program start. `class Outer::Late; end` written after an
/// `Outer.constants` had that constant already listed.
///
/// Precise on both sides: only a namespace named by a resolvable constant
/// receiver is observed, and only its DIRECT members are concealed. A
/// concealed class pays a runtime constant read
/// (`ClassInfo::runtime_conditional`), which is exactly the price of being
/// able to say "not yet" -- so the set is kept as small as the question is.
pub(super) fn conceal_observed_namespace_members(compiler: &mut Compiler) {
    let mut observed: Vec<ClassId> = Vec::new();
    for (_, node) in compiler.hir.iter_with_ids() {
        let HirNode::Call {
            receiver: Some(recv),
            name,
            ..
        } = node
        else {
            continue;
        };
        if !CONST_OBSERVERS.contains(&name.as_str()) {
            continue;
        }
        let recv = *recv;
        if let Some(cid) = const_node_class(compiler, recv, &[], 0) {
            observed.push(cid);
        } else if let Some(leaf) = leaf_const_name(&compiler.hir, recv) {
            observed.extend(
                (0..compiler.classes.len() as u32)
                    .map(ClassId)
                    .filter(|&c| compiler.class(c).name == leaf),
            );
        }
    }
    // `Object` is EXCLUDED: it is every top-level class's lexical parent, so
    // observing it would conceal the whole program -- and concealment costs a
    // runtime constant read at every reference. `Object.constants` written
    // before a later top-level `class` therefore still over-reports, a far
    // narrower divergence than the one it would buy.
    observed.retain(|&c| c != crate::compiler::OBJECT_CLASS);
    if observed.is_empty() {
        return;
    }
    for i in 0..compiler.classes.len() {
        let cid = ClassId(i as u32);
        let ci = compiler.class(cid);
        // A BUILTIN's constant is there from program start in ruby too, and a
        // class already concealed for a guard keeps its own reason.
        if ci.is_builtin || ci.is_bootstrap || ci.runtime_conditional {
            continue;
        }
        if ci.lexical_parent.is_some_and(|p| observed.contains(&p)) {
            compiler.classes[i].runtime_conditional = true;
        }
    }
}

/// The bare constant NAME a node reads, for the leaf-name fallback above.
fn leaf_const_name(hir: &Hir, node: NodeId) -> Option<String> {
    match &hir[node] {
        HirNode::ClassRef(name) => Some(name.trim_start_matches("::").to_string()),
        HirNode::QualifiedConstRead(_, name) => Some(name.clone()),
        _ => None,
    }
}

/// A mixin whose module overrides the PRIMITIVE is no longer a compile-time
/// ancestry fact: whether it happens at all is decided at run time. The
/// ancestry itself is handled -- `splice_mixin` writes the overlay chain that
/// `ancestors_of_value` prefers -- but a call folded at COMPILE time would
/// still reach the class's own body, so the module's method names have to
/// leave the fold. That is exactly what `runtime_patches` is for.
pub(super) fn defer_mixin_to_runtime(compiler: &mut Compiler, module: ClassId) {
    let mut names: Vec<String> = Vec::new();
    for &s in &compiler.classes[module.0 as usize].own_methods {
        let scope = &compiler.scopes[s.0 as usize];
        // A body that writes `super` will walk the TARGET's chain when the
        // splice runs -- a runtime fact, so every owner of the name needs
        // the receiver-generic bridge. See
        // `Compiler::runtime_mixin_super_names`.
        if scan_contains_super_body(&compiler.hir, &scope.body) {
            compiler
                .runtime_mixin_super_names
                .insert(scope.name.clone());
        }
        names.push(scope.name.clone());
    }
    // A BUILTIN module (`include Comparable` at a reopen site, or inside a
    // box) has no user scopes at all, so `own_methods` names nothing and the
    // call sites stayed folded: `[1] < [2]` compiled to "Array has no `<`"
    // and raised, while `send(:<)` -- which always asks the run time --
    // answered. The surface table is what the compiler knows about a
    // builtin's rows.
    if let Some(surface) = crate::builtin_surface::surface_for(zeo_abi::ClassId(module.0)) {
        names.extend(surface.instance_methods.iter().map(|n| (*n).to_string()));
    }
    compiler.runtime_patches.extend(names);
}

/// Whether `module` wants to be told it was prepended -- it defines
/// `prepended`, or overrides the `prepend_features` primitive that performs
/// the mix-in.
///
/// A compile-time ancestry edit emits no statement, so it can never call
/// either. Both callers use this to hand such a module to the run time
/// instead (or, for the `class << self` spelling, to reject).
pub(super) fn defines_prepend_hook(compiler: &Compiler, module: ClassId) -> bool {
    // `own_class_methods`, not `class_method_in_chain`: both callers run
    // DURING the statement walk, before `mro::materialize_class_methods` has
    // flattened anything, so the chain answers no for every module. The
    // module's own `def self.prepended` is registered by then, and an
    // INHERITED hook on a mixin module is not a shape ruby code writes.
    compiler.overrides_mixin_primitive(module, "prepend_features")
        || compiler
            .class(module)
            .own_class_methods
            .iter()
            .any(|&sid| compiler.scope(sid).name == "prepended")
}

/// Whether this class-body statement edits the class's OWN singleton chain at
/// run time -- `singleton_class.prepend M` and its `include`/`extend` twins,
/// with the implicit receiver a class body gives them.
///
/// A qualified spelling (`K.singleton_class.prepend M`) is left alone: it names
/// a class by constant, which the body cannot do for itself before the body
/// ends, so it cannot race the registration this guards.
fn mutates_own_singleton_at_runtime(compiler: &Compiler, stmt: NodeId) -> bool {
    let HirNode::Call {
        receiver: Some(recv),
        name,
        ..
    } = &compiler.hir[stmt]
    else {
        return false;
    };
    if !matches!(name.as_str(), "prepend" | "include" | "extend") {
        return false;
    }
    matches!(
        &compiler.hir[*recv],
        HirNode::Call { receiver, name, .. }
            if name == "singleton_class"
                && receiver.is_none_or(|r| matches!(compiler.hir[r], HirNode::SelfRef))
    )
}

/// Whether a module this class has ALREADY `extend`ed writes its own `name` --
/// an `attr_accessor` macro override, which makes the fold wrong.
///
/// Reads `extends` as the body walk has filled it so far, so the answer is
/// positional: an `extend` written below the `attr_accessor` has not happened
/// yet and does not count. A module the extended module itself includes counts
/// too, which is how a macro pack layered over another one is found.
fn extends_supply_macro(compiler: &Compiler, class_id: ClassId, name: &str) -> bool {
    let extends = compiler.classes[class_id.0 as usize].extends.clone();
    extends.iter().any(|&m| {
        std::iter::once(m)
            .chain(compiler.classes[m.0 as usize].mixin_order.iter().map(|&(t, _)| t))
            .any(|c| compiler.classes[c.0 as usize].own_method_at.contains_key(name))
    })
}
