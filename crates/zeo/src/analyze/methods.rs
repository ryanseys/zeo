//! Method registration: own-method rows, conditional defs, accessor
//! shapes, module/mixin targets, superclass + const-alias resolution.

use super::*;

/// The next [`crate::compiler::SiteDef::seq`]. The walk visits bodies in the
/// order they run, so a plain counter IS execution order.
pub(super) fn next_def_seq(compiler: &mut Compiler) -> u32 {
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
pub(super) fn add_own_method(
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
    // `own_method_at` is this list's own name index. A scan comparing every
    // entry's scope name would make registering a class's methods quadratic
    // in their count.
    let replaced = if is_class_method {
        ci.own_class_method_at.get(&mname).copied()
    } else {
        ci.own_method_at.get(&mname).copied()
    };
    // Last-`def`-wins is a fact about `def`s that RAN. A conditional one may
    // not have, so it does not displace a body that always does -- dropping
    // that body here would leave the name answering nothing whenever the guard
    // is false, and the runtime overlay the conditional `def` writes already
    // outranks the static row when the guard IS true.
    //
    // A UNIT's def yields to an EAGER def the same way: the unit walk runs
    // after the whole eager stream, but the unit's body EXECUTES at its
    // runtime require -- before any eager statement written below that
    // require -- so walk order inverts execution order. A spec file's
    // stub over a lazily-required rspec class is the corpus case.
    let yields = replaced.is_some_and(|i| {
        let mine_conditional = compiler.scope(sid).runtime_conditional;
        let theirs_conditional = compiler.scope(list[i]).runtime_conditional;
        // ... and the unit rule does NOT apply over a CONDITIONAL incumbent.
        // The two rules meet in bundler: rubygems' `def initialize` is in a
        // unit, bundler reopens the class and redefines it under a guard zeo
        // cannot decide. Yielding to the guarded body left the name answering
        // NOTHING whenever the guard was false -- `undefined method` for a
        // method the unit plainly defines. An unconditional body is the one
        // that always ran, whichever walk found it.
        (mine_conditional && !theirs_conditional)
            || (compiler.unit_walk
                && !compiler.unit_scopes.contains(&list[i])
                && !theirs_conditional)
    });
    let ci = &mut compiler.classes[class_id.0 as usize];
    let (list, index) = if is_class_method {
        (&mut ci.own_class_methods, &mut ci.own_class_method_at)
    } else {
        (&mut ci.own_methods, &mut ci.own_method_at)
    };
    let mut displaced = None;
    match replaced {
        Some(_) if yields => {}
        Some(i) => {
            displaced = Some(list[i]);
            list[i] = sid;
        }
        None => {
            index.insert(mname.clone(), list.len());
            list.push(sid);
        }
    }
    // Displacing a merged PACKAGE's body on a BUILTIN class is two static
    // bodies for one shared-class name; recorded here (the one site every
    // def registration passes), refused by analyze with the providing
    // package's name.
    if let Some(d) = displaced
        && class_id.0 < compiler.first_program_class_id
        && compiler.scope(d).extern_symbol.is_some()
    {
        compiler
            .pkg_spine_redefs
            .push((class_id, mname.clone(), is_class_method));
    }
    let ci = &mut compiler.classes[class_id.0 as usize];
    ci.method_history.push((mname, is_class_method, seq, sid));
    if compiler.unit_walk {
        compiler.unit_scopes.insert(sid);
    }
    if let Some(stream) = compiler.unit_stream {
        compiler.scope_stream.insert(sid, stream);
    }
}

/// Whether a `def` runs whenever its class body does, or only when a guard zeo
/// cannot decide says so. See [`crate::compiler::Scope::runtime_conditional`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Conditional {
    No,
    Yes,
}

/// Registers one class/module-body `def` as an own method: builds its `Scope`
/// and files it under `own_methods`/`own_class_methods`. Shared by the
/// top-level class-body walk and `register_conditional_defs` (a `def` nested in
/// an `if`/`case` branch). A no-op if `stmt` isn't a `DefMethod`.
pub(super) fn register_body_def_method(
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
        (**params).clone(),
        body.clone(),
        *is_class_method,
        *visibility,
    );
    // An OPERATOR definition on a builtin reopen (`class Integer; def +`)
    // STANDS DOWN the native operator fast paths for that operator: a fast
    // path is emitted at static call sites AHEAD of the reopened-builtin
    // arm, so leaving it in place would silently bypass the user operator.
    // The suppression is whole-program per (numeric lane, operator) --
    // recorded here at registration, consulted by `clif::expr`'s operator
    // fast path before it emits an inline `Int`/`Float` arm. A
    // suppressed call site falls through to the reopened-builtin arm (a
    // statically-typed receiver) or `send_value_in`'s MRO walk (a dynamic
    // one), both of which read the reopened row -- the user operator wins
    // everywhere, at the price of that operator's fast path, paid only by
    // programs that redefine it.
    //
    // Only `Int` and `Float` have such paths, so only their MRO feeds the
    // sets: a definition on `Numeric`/`Comparable`/`Object`/`Kernel`/
    // `BasicObject` sits under both lanes. Every OTHER builtin's operator
    // reopen already won via the reopened-builtin arm's placement.
    //
    // A CLASS-method operator needs none of this whatever the class:
    // `JSON[str]` is `def self.[]` on a module, and no fast path exists for
    // a call whose receiver is a class object -- those dispatch through the
    // ordinary class-method tables.
    if compiler.class(class_id).is_builtin
        && !is_class_method
        && !name.starts_with(|c: char| c.is_alphabetic() || c == '_')
    {
        match compiler.class(class_id).name.as_str() {
            "Integer" => {
                compiler.redefined_int_ops.insert(name.clone());
            }
            "Float" => {
                compiler.redefined_float_ops.insert(name.clone());
            }
            "Numeric" | "Comparable" | "Object" | "Kernel" | "BasicObject" => {
                compiler.redefined_int_ops.insert(name.clone());
                compiler.redefined_float_ops.insert(name.clone());
            }
            _ => {}
        }
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
    // A def on a class a MERGED PACKAGE provides installs at run time, at
    // its document position: the package's rows travel in its compiled
    // object, so a static row here could not hold the reopen timeline
    // (the package body answers before this line runs, this body after).
    // The install patches the class, which is what deoptimizes the
    // package's own guarded call sites on it.
    let reopens_import = compiler.class(class_id).imported_pkg.is_some();
    if conditional == Conditional::Yes || reopens_import {
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

/// Registers every `def` in `subtree` that a straight-line or branching reach
/// gets to as an own method. A conditional `def` still runs at document
/// position via its runtime `define_method` emission (which supplies the taken
/// branch's body); this makes its NAME visible to `instance_methods`/`extend`,
/// which are resolved at compile time and would otherwise miss it. When more
/// than one branch defines the same name, last-wins picks the branch the
/// static target takes (fileutils' RbConfig is a compile-time shim).
///
/// A `Reach::through_block` def is deliberately SKIPPED -- one written in a
/// loop or a block may never run at all, and CRuby has no method there either
/// until it does, so it stays runtime-only.
pub(super) fn register_conditional_defs(
    compiler: &mut Compiler,
    class_id: ClassId,
    subtree: &[(NodeId, Reach)],
) -> Result<(), String> {
    for &(s, reach) in subtree {
        if reach.through_block || !matches!(compiler.hir[s], HirNode::DefMethod { .. }) {
            continue;
        }
        register_body_def_method(compiler, class_id, s, Conditional::Yes)?;
    }
    Ok(())
}

/// What an `include`/`extend`/`prepend` target name resolves to.
pub(super) enum MixinTarget {
    /// A class/module zeo compiled: the ancestry edit is a compile-time fact.
    Static(ClassId),
    /// A name nowhere in the program. The directive runs as ordinary code so
    /// the constant read raises `NameError` the way ruby raises it -- see
    /// [`defer_unresolved_directive`].
    DeferredRead,
    /// A name the program DOES assign, to something no static MRO can name --
    /// a module MINTED at run time. See [`defer_runtime_mixin`].
    Runtime,
}

/// Resolves an `include`/`extend`/`prepend` target name.
///
/// Ruby doesn't reject `extend FFI` when `FFI` is undefined: `include`/`extend`/
/// `prepend` are ordinary method calls, so the constant is READ first and raises
/// `NameError` from inside the class body -- and only if the body runs. Reporting
/// it at compile time instead means a program whose module comes from a native
/// extension zeo doesn't have, or from a branch that never executes, can't be
/// built at all.
///
/// A name the program DOES assign (`M = Module.new; include M`) is not an
/// error: a compiled class dispatches off a static MRO, but the directive
/// keeps its runtime self-send and every call site widens, so a runtime
/// splice reaches it. Unknown SUPERCLASSES stay loud, because zeo has to lay
/// out a struct for one.
pub(super) fn resolve_module_target(
    compiler: &mut Compiler,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
) -> MixinTarget {
    let resolved = compiler
        .resolve_class(name, cref, box_id)
        // A mixin names a MODULE. A CLASS of that name in an outer scope is
        // not what the statement means, so the search continues -- the dual
        // of the superclass rule above. rexml is the case: `include Encoding`
        // inside `REXML::Output` found the BUILTIN Encoding class whenever
        // rexml arrived as a lazy unit walked before `REXML::Encoding`, and
        // the chain then carried a builtin class no registrar serves. The
        // lexical road below reaches the real module through `shell_kinds`.
        .filter(|&c| compiler.class(c).is_module)
        // A module defined LATER in the flattened list (hoisted deferred
        // require) is created as a forward shell; its methods are added when the
        // real definition reopens the shell (seen at `mro::materialize`).
        .or_else(|| resolve_or_create_lexical(compiler, name, cref, box_id, None));
    let resolved = resolved.or_else(|| resolve_const_alias(compiler, name, cref, box_id));
    match resolved {
        Some(cid) => MixinTarget::Static(cid),
        // A PACKAGE build's world excludes the foreign gems the host
        // provides, so "resolves nowhere here" says nothing about the
        // merged program. The directive stays a REAL runtime self-send:
        // csv's `extend Forwardable` lands when the forwardable artifact
        // answers, and a truly missing name still raises ruby's NameError
        // from the send's constant argument. `DeferredRead` would rewrite
        // the directive to a bare read and silently drop the mixin. Free
        // here, because a package's own sites are blanket-dynamic.
        None if compiler.hir.pkg_build.is_some() => MixinTarget::Runtime,
        None if !compiler.assigns_const_path(name) => MixinTarget::DeferredRead,
        // A second NAME for a module (`Constants = ::Socket::Constants`) that
        // resolves to nothing zeo compiled: spelled directly, that same include
        // defers to the runtime constant read above, and reaching the module
        // through an alias must not change the answer.
        None if is_const_path_alias(compiler, name, cref, box_id) => MixinTarget::DeferredRead,
        None => MixinTarget::Runtime,
    }
}

/// A mixin whose module zeo cannot name, kept as the runtime self-send its
/// class body serves. `runtime_meta::splice_mixin` performs the ancestry edit
/// and fires the hook when the body executes; the call-site widening has to
/// happen NOW, or a call folded at compile time reaches the class's own body
/// underneath an override the splice installed.
///
/// The shape is a constant the program assigns a module VALUE to. net-ssh
/// picks its `Prompt` out of three candidates behind a rescued require;
/// rspec-rails builds `ControllerAssertionDelegator` by calling
/// `AssertionDelegator.new(...)`; coderunner's `SYSTEM_MODULE` names whichever
/// batch system the host runs. All of them then `include` the constant.
///
/// Widening EVERY name is the blunt version -- the module's own method list is
/// what wants de-optimizing, and a runtime module has none to read. It costs
/// the static fast path in programs that do this and nothing anywhere else.
fn defer_runtime_mixin(compiler: &mut Compiler, stmt: NodeId) -> NodeId {
    compiler.runtime_patches_any_name = true;
    crate::lower::defs::runtime_directive_spelling(&mut compiler.hir, stmt)
        .expect("a mixin directive is not `refine`")
        .expect("every mixin directive has a runtime spelling")
}

/// [`defer_runtime_mixin`] for a directive inside a `class`/`module` body:
/// the send runs at the site's own document position.
/// Whether `module`'s body installs methods at RUN time, so its method set is
/// not knowable when the mixin is compiled.
///
/// A static `include`/`extend` folds the module's methods into the target's
/// table right here, which is only sound when that table is complete. i18n
/// writes its delegators as a string eval in a loop:
///
/// ```text
/// %w(locale backend default_locale ...).each do |method|
///   module_eval <<-DELEGATORS
///     def #{method} ... end
///     def #{method}= ... end
///   DELEGATORS
/// end
/// ...
/// extend Base
/// ```
///
/// The literal `def`s in `Base` reached `I18n`; the eight delegator pairs did
/// not, so `I18n.backend = ...` was a NoMethodError while `I18n.translate`
/// worked. The mixin becomes a runtime send instead, which installs whatever
/// the module holds by the time it RUNS -- and also puts the module in the
/// target's ancestry, where reflection can see it.
///
/// Deliberately conservative in the safe direction: a module that merely
/// MENTIONS one of these names is compiled the slower way, which costs
/// dispatch speed rather than an answer.
pub(super) fn module_defines_methods_dynamically(compiler: &Compiler, module: ClassId) -> bool {
    fn installs_at_runtime(hir: &crate::hir::Hir, id: NodeId) -> bool {
        // The EVAL family only. `attr_accessor`, `alias_method` and a
        // `define_method(:literal)` all expand at COMPILE time, so a module
        // using them has a complete table and must keep the static edit --
        // listing them here would push almost every module in the corpus onto
        // the slow path for nothing.
        let hit = match &hir[id] {
            HirNode::Call { name, .. } => matches!(
                name.as_str(),
                "module_eval" | "class_eval" | "module_exec" | "class_exec" | "eval"
            ),
            _ => false,
        };
        if hit {
            return true;
        }
        let mut found = false;
        hir[id].for_each_child(&mut |c| found = found || installs_at_runtime(hir, c));
        found
    }
    compiler
        .class_body_sites
        .iter()
        .filter(|s| s.class == module)
        .any(|s| {
            s.stmts
                .iter()
                .any(|&n| installs_at_runtime(&compiler.hir, n))
        })
}

pub(super) fn defer_runtime_mixin_in_body(compiler: &mut Compiler, site_idx: usize, stmt: NodeId) {
    let send = defer_runtime_mixin(compiler, stmt);
    compiler.class_body_sites[site_idx].stmts.push(send);
}

/// The module names a `self.included(base)` hook extends its includer with.
///
/// The ClassMethods idiom, and it is everywhere:
///
/// ```text
/// module Hook
///   def self.included(base) = base.extend(ClassMethods)
///   module ClassMethods; def method_added(n); ...; end; end
/// end
/// ```
///
/// `include Hook` therefore gives the including class `ClassMethods`' methods
/// as CLASS methods. zeo records an `extends` edge for an `extend` written in
/// a class body, so the direct spelling already worked; this one happens
/// inside a method that runs at mixin time, so nothing static saw it and the
/// edge was missing.
///
/// What it cost: Thor registers every command by defining a method and
/// letting `method_added` catch it, and `method_added` arrives exactly this
/// way. Without the edge `analyze::def_hooks` could not see a hook body, so
/// no definition announced itself and `Bundler::CLI` finished with 3 of its
/// ~30 commands -- `zeo bundle install` answered `Could not find command
/// "install"` with the method sitting right there on the class.
///
/// Read conservatively: only a direct `<base>.extend(Const, ...)` on the
/// hook's own first parameter, at any depth in the body so a guarded one
/// still counts. Anything else -- a computed module, a different receiver,
/// `base.send(:extend, m)` -- is left to the runtime path, which is correct
/// and merely unoptimized.
pub(super) fn extends_from_included_hook(compiler: &Compiler, module: ClassId) -> Vec<String> {
    let Some(&hook) = compiler.class(module).own_class_methods.iter().find(|&&s| {
        let scope = compiler.scope(s);
        scope.name == "included" && scope.params.required.len() == 1
    }) else {
        return Vec::new();
    };
    let scope = compiler.scope(hook);
    let base = scope.params.required[0].as_str();

    fn walk(hir: &crate::hir::Hir, id: NodeId, base: &str, out: &mut Vec<String>) {
        if let HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } = &hir[id]
            && name == "extend"
            && matches!(&hir[*recv], HirNode::LocalRead(l) if l == base)
        {
            for a in args {
                if let HirNode::ClassRef(n) = &hir[a.node_id()] {
                    out.push(n.clone());
                }
            }
        }
        hir[id].for_each_child(&mut |c| walk(hir, c, base, out));
    }

    let mut out = Vec::new();
    for &stmt in &scope.body {
        walk(&compiler.hir, stmt, base, &mut out);
    }
    out
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
pub(super) fn resolve_superclass(
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
        let registered = compiler.resolve_class(superclass, scopes, box_id);
        let found = registered
            // A superclass clause names a CLASS. A MODULE of that name in an
            // OUTER scope is not what the clause means, so the search
            // continues rather than stopping on it -- and the inner scope,
            // which the program does define as a class, is reached.
            //
            // minitest is the case: `class Benchmark < Test` inside `module
            // Minitest` found test-unit's TOP-LEVEL `module Test` whenever
            // `minitest/test.rb` had not been walked yet, so the definition
            // was refused with "superclass must be an instance of Class
            // (given an instance of Module)" and `Minitest::Benchmark` became
            // a class the program defines and no read can find. Ruby resolves
            // `Test` at that line to `Minitest::Test`.
            //
            // Narrow on purpose. Preferring the inner scope OUTRIGHT was
            // tried and is wrong: rspec-mocks writes `class BasicObject`
            // under an `unless defined?(BasicObject)` that never runs, and
            // `class FluentInterfaceProxy < BasicObject` beneath it means the
            // BUILTIN. An outer candidate that is already a class stays the
            // answer.
            .filter(|&c| !compiler.class(c).is_module)
            // Ruby's own second half of a bare constant lookup: after the
            // lexical scopes' own tables, the ANCESTORS of the innermost cref
            // (`rb_const_search`). rake writes `class Scope < LinkedList` and
            // nests `class EmptyScope < EmptyLinkedList` in it, where
            // `EmptyLinkedList` is a constant of `LinkedList` -- the enclosing
            // class's superclass. Without this the clause resolved to nothing,
            // the definition was rewritten to a runtime class, and the very
            // next line's `EmptyScope.new` raised.
            .or_else(|| resolve_in_ancestry(compiler, superclass, scopes, box_id, &defining))
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
            .or_else(|| resolve_const_alias(compiler, superclass, scopes, box_id))
            // Nothing better than the module: hand it back so the caller
            // raises ruby's own TypeError, which is what ruby does for a
            // clause that really does name one.
            .or(registered);
        match found {
            Some(cid) if !scopes.is_empty() && compiler.fq_name(cid) == defining => {
                scopes = &scopes[..scopes.len() - 1];
            }
            other => return other,
        }
    }
}

/// `name` as a constant of the innermost enclosing class's ANCESTRY -- the
/// second half of ruby's bare-constant search, after the lexical scopes' own
/// tables have all missed.
///
/// Walks the DECLARED `parent` edges rather than `ClassInfo::ancestors`, which
/// `mro::materialize` fills only after this whole walk: reading it here would
/// see an empty chain. `defining` is the path this very definition binds and
/// can never be its own superclass.
fn resolve_in_ancestry(
    compiler: &mut Compiler,
    name: &str,
    cref: &[ClassId],
    box_id: u32,
    defining: &str,
) -> Option<ClassId> {
    // Walk the DECLARED parent AND mixin edges breadth-first: ruby's
    // lookup consults the linearized ancestors of the innermost cref, and
    // an included module's constants are part of them -- rss's
    // `ITunesCategories < ITunesCategoriesBase` inside a `ChannelBase`
    // that `include`s the module defining the base is the mixin shape.
    // The bound covers a cycle in the declared edges, which is a compile
    // error elsewhere.
    let mut queue: std::collections::VecDeque<ClassId> = cref.last().copied().into_iter().collect();
    let mut visited: Vec<ClassId> = Vec::new();
    while let Some(owner) = queue.pop_front() {
        if visited.contains(&owner) || visited.len() > 64 {
            continue;
        }
        visited.push(owner);
        let path = format!("{}::{name}", compiler.fq_name(owner));
        if let Some(found) = compiler.resolve_class(&path, &[], box_id)
            && compiler.fq_name(found) != defining
        {
            return Some(found);
        }
        // The inherited constant may be an ALIAS of a class defined elsewhere
        // -- rss's `AuthorsBase = ChannelBase::AuthorsBase` inside ItemBase,
        // read by `class Authors < AuthorsBase` under `Item < ItemBase`.
        if compiler.const_aliases.contains_key(&(box_id, path.clone()))
            && let Some(found) = resolve_const_alias(compiler, &path, cref, box_id)
            && compiler.fq_name(found) != defining
        {
            return Some(found);
        }
        // The constant may live in a unit the walk has not reached yet: the
        // pre-collected shell kinds know every class-shaped spelling, so
        // mint the forward shell the lexical road would mint. A package
        // build of rss is the shape -- maker/1.0's unit is walked before
        // maker/base's, and `Categories < CategoriesBase` reads a class of
        // the (shelled) superclass ChannelBase.
        let key = crate::constpath::ConstPath::parse(&path)
            .unanchored()
            .to_string();
        // `defining` is checked BEFORE the mint, not after like the arms
        // above: minting first would leave the excluded shell REGISTERED,
        // defeating the caller's deferral -- `class Logger < Logger` with
        // logger unrequired would resolve every later read to the phantom
        // instead of raising ruby's NameError.
        if path != defining
            && compiler.shell_kinds.contains_key(&(box_id, key))
            && let Some(found) =
                super::classes::resolve_or_create_container(compiler, &path, box_id)
            && compiler.fq_name(found) != defining
        {
            return Some(found);
        }
        for &(m, _) in &compiler.class(owner).mixin_order {
            queue.push_back(m);
        }
        // A positionally staged include is still a DECLARED edge the
        // lookup below it sees -- rss reopens ChannelBase, includes
        // ITunesChannelModel, and subclasses its bases three lines down.
        if let Some(ms) = compiler.declared_positional_mixins.get(&owner) {
            queue.extend(ms.iter().copied());
        }
        if let Some(p) = compiler.class(owner).parent {
            queue.push_back(p);
        }
    }
    None
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
/// the lookup above can ask about a scope rather than a bare leaf.
///
/// Every position counts, not just an `if` branch: a write in a `case` arm, a
/// `begin` body, a `rescue` clause or an `each` block binds the name just the
/// same, and the sole reason to record it is that a later definition may READ
/// it. The three arms below are the ones that change WHERE a write lands --
/// a `class`/`module` deepens the scope path, a box restarts it -- and every
/// other node descends into its children.
pub(super) fn collect_const_aliases(
    hir: &Hir,
    stmts: &[NodeId],
    scope: &[String],
    box_id: u32,
    out: &mut FMap<(u32, String), NodeId>,
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
                // `M = (class Inner; X = 1; end)` binds `Inner::X` too.
                collect_const_aliases(hir, std::slice::from_ref(value), scope, box_id, out);
            }
            HirNode::ClassDef { name, body, .. } => {
                let mut inner = scope.to_vec();
                inner.push(name.clone());
                collect_const_aliases(hir, body, &inner, box_id, out);
            }
            HirNode::BoxScope { box_id: bx, body } => {
                collect_const_aliases(hir, body, &[], *bx, out);
            }
            // A method body is a separate scope that runs when it is CALLED,
            // and ruby rejects a constant assignment written in one outright,
            // so nothing there binds a name here. A block-bodied `def` is a
            // block, and descends like one.
            HirNode::DefMethod { body, .. } => {
                if hir.has_flag(id, crate::hir::NodeFlag::BLOCK_BODIED_DEF) {
                    collect_const_aliases(hir, body, scope, box_id, out);
                }
            }
            other => {
                let mut children = Vec::new();
                other.for_each_child(&mut |c| children.push(c));
                collect_const_aliases(hir, &children, scope, box_id, out);
            }
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
pub(super) fn defer_unresolved_directive(compiler: &mut Compiler, stmt: NodeId, name: &str) {
    compiler.hir[stmt] = HirNode::ClassRef(name.to_string());
}

/// Files the lexical range a `using M` covers -- from the statement's own
/// span to `end`. A module that resolves nowhere records nothing: with no
/// refinements to activate there is nothing for a call site to consult, and
/// the unresolved name is already whatever error the program deserves.
pub(super) fn record_activation(
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
pub(super) fn defer_in_class_body(
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
) -> FMap<String, TyKind> {
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
    // A parameter arrives through the function signature as a `RubyValue`,
    // so a body assignment can never narrow it to an unboxed value: every
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
pub(super) fn register_method(
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
    let local_types = FMap::default();
    let mut uses_bare_block = false;
    // Default-parameter expressions run inside the method too -- rspec's
    // `def register_ordering(name, strategy = Custom.new(Proc.new { |l|
    // yield l }))` yields from one, so they need the same scan as the body.
    for n in body.iter().copied().chain(params.default_ids()) {
        if scan_bare_block_use(&compiler.hir, n) {
            uses_bare_block = true;
        }
    }
    // A run-time `eval` written here may itself say `yield` or
    // `block_given?`, and CRuby answers both from the CALLER's frame --
    // so the method keeps its block channel and publishes it for the
    // length of the call (`zeo_rt::eval::EvalHome`).
    if !uses_bare_block && crate::analyze::captures::body_contains_runtime_eval(compiler, &body) {
        uses_bare_block = true;
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
        .filter(|n| {
            compiler
                .hir
                .has_flag(*n, crate::hir::NodeFlag::SINGLETON_BODY_DEF)
        })
        .and_then(|_| compiler.singleton_surrogate_of(defining_class));
    // A `def` in a LAZILY-LOADED unit is registered but not PROMISED --
    // whether it exists at any point of the run is decided by whether its
    // unit has loaded by then, which only the runtime knows. Its row stays
    // in the static tables (the runtime MRO walk must find the real body
    // once the unit loads), but the NAME de-optimizes every call site
    // (`runtime_patches`): the dynamic walk probes the overlay first, so a
    // runtime definition wins, and no compile-time decision -- devirt,
    // visibility, arity -- is baked against a definition that may never
    // load. minitest's spec DSL (`Kernel#describe` + `private :describe`)
    // sat in the compiled-in load path of an rspec program and made the
    // static tables bake a visibility raise into `RSpec.describe`.
    if compiler.unit_walk {
        // A PACKAGE build keeps the blanket out of `runtime_patches`, whose
        // contents become the manifest's fact vector -- the split set feeds
        // `may_be_patched_at_runtime` all the same. See
        // `Compiler::unit_blanket_names`.
        if compiler.hir.pkg_build.is_some() {
            compiler.unit_blanket_names.insert(name.clone());
        } else {
            compiler.runtime_patches.insert(name.clone());
        }
    }
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
        // The sentinel `ClassInfo::unit` uses: the real index lands when the
        // unit survives, and a DECLINED unit keeps it -- that unit never
        // runs, so its methods stay concealed for the whole program.
        unit: compiler
            .unit_walk
            .then_some(crate::compiler::Compiler::UNIT_UNRESOLVED),
        // `ruby2_keywords def fwd(*a)`: the directive marked the `def` node at
        // lowering; carry it onto the scope, where codegen reads it.
        ruby2_keywords: def_node.is_some_and(|n| {
            compiler
                .hir
                .has_flag(n, crate::hir::NodeFlag::RUBY2_KEYWORDS)
        }),
        accessor,
        extern_symbol: None,
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
        attr_generated: def_node
            .is_some_and(|n| hir.has_flag(n, crate::hir::NodeFlag::ATTR_GENERATED)),
    })
}
