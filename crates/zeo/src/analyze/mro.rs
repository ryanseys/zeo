//! Real MRO: ancestor linearization, method/class-method materialization,
//! and class-variable ownership resolution -- run once, after every
//! `ClassDef` has been registered (`analyze::register_class`), since
//! `include`/`extend`/`prepend`/`< Super` targets must already exist by the
//! same "defined earlier in the file" rule `superclass` resolution already
//! enforces today.
//!
//! In short:
//! rather than zeo's clone-with-mangled-shadow-name scheme (needed only
//! because C has no generics), a module's method body gets MATERIALIZED --
//! re-run through the exact same per-method analysis pipeline
//! (`register_method`) that an ordinary `def` already goes through, once
//! per including/inheriting class -- onto the concrete class that will
//! actually call it. This is DRY at the Ruby-source/HIR level (one
//! `NodeId`, safely referenced from multiple `Scope`s) even though the
//! generated Rust text ends up with one copy per class, exactly the same
//! trade a generic Rust function already makes via monomorphization.

use crate::analyze_error::AnalyzeError;
use crate::compiler::{ClassId, Compiler, MethodEntry, NameId, OBJECT_CLASS, ScopeId};
use crate::hir::{HirNode, NodeId, Span, Visibility};
use std::collections::{HashMap, HashSet};

/// Real Ruby's actual linearization (not classic C3): for `class_id` with
/// prepends `P1..Pk` and includes `M1..Mn`, both in source order,
/// `ancestors = expand(Pk)..expand(P1) ++ [self] ++ expand(Mn)..expand(M1)
/// ++ ancestors(parent)`, recursively expanding each module's own
/// prepends/includes the same way, deduped keeping the FIRST occurrence.
/// This is the one place zeo deliberately does NOT copy spinel's own
/// shortcut -- spinel's separately generated `.ancestors` reflection only
/// expands a class's own DIRECT includes, which is measurably wrong for a
/// module-including-module diamond (confirmed by running a repro against
/// spinel's own binary -- see the plan). Consulting this SAME list for
/// dispatch, reflection, AND class-variable ownership avoids that class of
/// bug entirely.
pub fn compute_ancestors(compiler: &Compiler, class_id: ClassId) -> Vec<ClassId> {
    let mut out = Vec::new();
    expand_into(compiler, class_id, &mut out);
    out
}

fn expand_into(compiler: &Compiler, class_id: ClassId, out: &mut Vec<ClassId>) {
    if out.contains(&class_id) {
        return;
    }
    let info = compiler.class(class_id);
    for &p in info.prepends.iter().rev() {
        expand_into(compiler, p, out);
    }
    if !out.contains(&class_id) {
        out.push(class_id);
    }
    for &m in info.includes.iter().rev() {
        expand_into(compiler, m, out);
    }
    if let Some(parent) = info.parent {
        // A module reached twice is an ordinary diamond and the `contains`
        // check above absorbs it. A SUPERCLASS reached twice is a cycle in a
        // chain that is supposed to be a list, and every walk over it that
        // isn't guarded the way this one is runs forever -- which is how
        // `require "active_record"` died. Say so once, here, where the shape is
        // visible.
        if out.contains(&parent) {
            tracing::warn!(
                class = compiler.fq_name(class_id),
                superclass = compiler.fq_name(parent),
                "mro: superclass cycle -- the chain already holds this class"
            );
        }
        expand_into(compiler, parent, out);
    }
}

/// Stamps a per-class pass's rejection with where that class was written.
///
/// `process_top_stmt` locates everything raised during the statement walk, but
/// this pass runs after it and iterates CLASSES -- so a refusal here (an alias
/// with no source, a method that cannot be materialized) would otherwise name
/// only the construct. `Compiler::class_def_span` is the nearest true answer.
fn at_class<T>(r: Result<T, String>, at: Option<Span>) -> Result<T, AnalyzeError> {
    r.map_err(|e| AnalyzeError::from(e).with_span_if_missing(at))
}

/// Computes `ancestors` for every registered class/module, then
/// materializes `methods`/`ivars` for every non-module class and
/// `class_methods` for EVERY class/module (a module can have its own `def
/// self.x` "module functions", e.g. `Math.sqrt`, which need no instance/
/// struct at all -- see `codegen::mod::emit_class_methods`), then resolves
/// class-variable ownership. Call once, after `analyze::analyze`'s
/// top-level registration loop has processed every `ClassDef`.
pub fn materialize(
    compiler: &mut Compiler,
    main_statements: &[NodeId],
) -> Result<(), AnalyzeError> {
    let started = std::time::Instant::now();
    record_top_level_consts(compiler, main_statements);
    index_document_order(compiler, main_statements);
    compiler.freeze_identity_caches();
    let all_ids: Vec<ClassId> = (0..compiler.classes.len() as u32).map(ClassId).collect();

    for &cid in &all_ids {
        let ancestors = compute_ancestors(compiler, cid);
        compiler.classes[cid.0 as usize].ancestors = ancestors;
    }
    // Every stage of this pass reports as it FINISHES, not at the end: a
    // Rails-sized graph can be killed mid-pass, and a log that only prints on
    // success says nothing at all about where the time and the memory went.
    // `--log-level info` turns them on.
    tracing::info!(
        classes = all_ids.len(),
        links = all_ids
            .iter()
            .map(|&c| compiler.class(c).ancestors.len())
            .sum::<usize>(),
        ms = started.elapsed().as_millis(),
        "mro: ancestors"
    );

    // Deferred `alias`/`alias_method` of an inherited method -- resolved here,
    // after ancestors are linearized but BEFORE method materialization, so the
    // cloned alias flattens onto this class AND its subclasses the same way any
    // own method does. Processed in class-id order (a superclass precedes its
    // subclasses), so an alias OF an alias resolves against the already-added
    // one. See `HirNode::AliasMethod`.
    for &cid in &all_ids {
        let at = compiler.class_def_span(cid);
        at_class(resolve_aliases(compiler, cid), at)?;
        at_class(resolve_module_functions(compiler, cid), at)?;
    }
    tracing::info!(
        defs = compiler.scopes.len(),
        ms = started.elapsed().as_millis(),
        "mro: aliases and module functions"
    );

    // Which `@ivar` names each class's OWN method bodies touch, walked once per
    // DEFINITION rather than once per (class, ancestor, method).
    //
    // `materialize_methods` needs, for every class, the union over its whole
    // ancestry -- and it used to get there by re-walking each ancestor's bodies
    // from scratch for every descendant. That is O(classes x visible methods x
    // HIR nodes), the last superlinear term the entry/definition split left
    // behind, and on a Rails-sized graph it is millions of node visits with a
    // `String` compare at each one. The union is a concatenation, so computing
    // the pieces once and composing them gives the identical answer: see
    // `own_ivars`.
    let own_ivars = own_ivars(compiler);
    for (done, &cid) in all_ids.iter().enumerate() {
        let at = compiler.class_def_span(cid);
        let class_started = std::time::Instant::now();
        if !compiler.class(cid).is_module {
            at_class(materialize_methods(compiler, cid, &own_ivars), at)?;
        }
        let instance_ms = class_started.elapsed().as_millis();
        at_class(materialize_class_methods(compiler, cid), at)?;
        apply_visibility_overrides(compiler, cid);
        // One class that costs more than the whole pass should, named. A
        // per-class cost is a product -- ancestors x their own methods -- and
        // at Rails scale one bad product is the difference between a compile
        // and a kill; the summary line cannot show which class it was.
        if class_started.elapsed().as_millis() >= 50 {
            tracing::warn!(
                class = compiler.fq_name(cid),
                ancestors = compiler.class(cid).ancestors.len(),
                methods = compiler.class(cid).methods.len(),
                class_methods = compiler.class(cid).class_methods.len(),
                super_targets = compiler.class(cid).singleton_super_targets.len(),
                instance_ms,
                total_ms = class_started.elapsed().as_millis(),
                "mro: slow class"
            );
        }
        // Progress, not a summary: this loop is where a Rails-scale compile
        // spends its memory, and a line every so often is what tells a live
        // `tail` that it is advancing rather than stuck -- and WHERE it stopped
        // when it doesn't finish.
        if done % 512 == 511 {
            let (entries, super_targets) = totals(compiler);
            tracing::info!(
                done = done + 1,
                of = all_ids.len(),
                entries,
                super_targets,
                ms = started.elapsed().as_millis(),
                "mro: materializing"
            );
        }
    }

    // A reopened builtin MODULE is never run through `materialize_methods`
    // (modules aren't), so its `methods` stays empty and the builtin-reopen
    // emitter would find nothing. Surface its OWN reopen methods as `methods`
    // so they register as value methods on the module id, where the MRO walk
    // finds them for every includer (`Enumerable`/`Comparable`/`Kernel`/...).
    // (A USER module's own methods reach the runtime by id through a separate,
    // self-contained bridge -- see `codegen::emit_user_module_bridges` -- so
    // they need no change here.)
    for &cid in &all_ids {
        let is_builtin_module = {
            let ci = compiler.class(cid);
            ci.is_builtin && ci.is_module
        };
        if is_builtin_module {
            let own = compiler.class(cid).own_methods.clone();
            if !own.is_empty() {
                let entries = own
                    .into_iter()
                    .map(|sid| {
                        let name = compiler.names.intern(&compiler.scope(sid).name.clone());
                        entry_for(compiler, name, sid, cid)
                    })
                    .collect();
                compiler.classes[cid.0 as usize].methods = entries;
            }
        }
    }

    // Nothing rewrites a method table past this point, so the name indexes can
    // be built once -- every `method_in_chain` from here on is a binary search.
    compiler.index_methods();

    // The shape of the program the rest of the compiler works over, and the one
    // number this pass exists to keep small: `entries` is what used to be
    // `defs` -- one cloned Scope per (class, visible method) -- and the ratio
    // between them is how much the entry/definition split is buying.
    let (entries, super_targets) = totals(compiler);
    tracing::info!(
        classes = compiler.classes.len(),
        defs = compiler.scopes.len(),
        entries,
        super_targets,
        ivars = compiler
            .classes
            .iter()
            .map(|c| c.ivars.len())
            .sum::<usize>(),
        ms = started.elapsed().as_millis(),
        "mro: materialized"
    );

    reinfer_local_types(compiler);
    tracing::info!(
        defs = compiler.scopes.len(),
        ms = started.elapsed().as_millis(),
        "mro: local types re-inferred"
    );

    resolve_cvars_and_consts(compiler, main_statements)?;
    tracing::info!(ms = started.elapsed().as_millis(), "mro: done");

    Ok(())
}

/// `(method entries, singleton super targets)` across every class -- the two
/// per-class tables that grow with classes x visible methods, so the two worth
/// watching while the pass runs.
fn totals(compiler: &Compiler) -> (usize, usize) {
    compiler.classes.iter().fold((0, 0), |(e, s), c| {
        (
            e + c.methods.len() + c.class_methods.len(),
            s + c.singleton_super_targets.len(),
        )
    })
}

/// Re-run every method scope's local-type inference now that ancestors and
/// class-method tables are final. The first pass runs at REGISTRATION time,
/// while classes are still arriving, so an answer that depends on the finished
/// picture is unreliable there: `X.new` types as an unboxed `Arc<X>` only when
/// `X` has no `def self.new` of its own, and that method may be registered
/// after the caller's body was walked. Codegen reads this map and resolves the
/// same question against the finished tables, so the two disagreeing is a
/// mismatched-types error on the generated program.
fn reinfer_local_types(compiler: &mut Compiler) {
    // Infer-all-then-write: `method_local_types` never reads another scope's
    // `local_types` (nothing does until codegen), so the two phases see the
    // same picture and the per-scope params/body clones the interleaved
    // mutable write used to force disappear entirely.
    let all_types: Vec<_> = compiler
        .scopes
        .iter()
        .map(|scope| {
            crate::analyze::method_local_types(
                compiler,
                scope.defining_class,
                &scope.params,
                &scope.body,
            )
        })
        .collect();
    for (scope, types) in compiler.scopes.iter_mut().zip(all_types) {
        scope.local_types = types;
    }
}

/// Resolves this class's `pending_aliases` (see `HirNode::AliasMethod`): for
/// each `(new, old)`, find `old` as the OWN method of some MRO ancestor
/// (self first), clone its params/body/visibility under `new`, and add it as
/// an own method of `class_id`. `methods` isn't materialized yet, so the
/// search walks each ancestor's `own_methods` directly. A source that
/// resolves nowhere is a clean compile error, mirroring real Ruby's
/// `NameError: undefined method`.
///
/// An alias binds the body that existed WHEN IT RAN: on each ancestor,
/// `method_history` entries older than the alias's seq are preferred over
/// the final row, newest-first. That is the alias-chaining idiom (reopen,
/// `alias_method :old, :m`, redefine `m` in terms of `old`) -- resolving
/// against the final table bound the alias to the NEW body, and the compiled
/// method called itself until the native stack overflowed. When no
/// strictly-older entry exists the final row still wins, preserving the old
/// behavior for an alias that names a method only defined later (ruby raises
/// NameError there at class-body time; zeo's story for that shape is
/// unchanged by this filter).
fn resolve_aliases(compiler: &mut Compiler, class_id: ClassId) -> Result<(), String> {
    let pending = std::mem::take(&mut compiler.classes[class_id.0 as usize].pending_aliases);
    for (new_name, old_name, is_class_method, alias_seq) in pending {
        let ancestors = compiler.class(class_id).ancestors.clone();
        // A class-method alias (`class << self; alias split shellsplit`)
        // resolves against `own_class_methods`; an ordinary alias against
        // `own_methods`.
        let source = ancestors.iter().find_map(|&anc_id| {
            let anc = compiler.class(anc_id);
            let historical = anc
                .method_history
                .iter()
                .filter(|(n, cm, seq, _)| {
                    *cm == is_class_method && *seq < alias_seq && *n == old_name
                })
                .max_by_key(|(_, _, seq, _)| *seq)
                .map(|&(_, _, _, sid)| sid);
            historical.or_else(|| {
                // The seq-blind fallback must not bind a def the ALIASING
                // class only registers AFTER the alias: `alias find_items_for
                // items_for` written BEFORE an `items_for` override aliases
                // the INHERITED body (rspec's `QueryOptimized` memoizes
                // through exactly this, and binding the override made the
                // memo call itself forever). History records every
                // registration, so a name present there but not in the
                // seq-filtered search above exists only LATER -- resolution
                // moves on to the ancestors, where document order is the
                // order their bodies already ran in.
                if anc_id == class_id
                    && anc
                        .method_history
                        .iter()
                        .any(|(n, cm, _, _)| *cm == is_class_method && *n == old_name)
                {
                    return None;
                }
                let list = if is_class_method {
                    &anc.own_class_methods
                } else {
                    &anc.own_methods
                };
                list.iter()
                    .find(|&&s| compiler.scope(s).name == old_name)
                    .copied()
            })
        });
        let Some(sid) = source else {
            if is_class_method {
                // No user class-method scope in the chain: the source is a
                // BUILTIN singleton method -- `Class#new` for the pervasive
                // `class << self; alias [] new`, the `Klass[...]` constructor
                // shorthand. Recorded as a name indirection in the SINGLETON
                // table, the exact treatment the instance side gets below;
                // `register_class_alias` installs it and
                // `validate_class_aliases` raises `NameError` at program start
                // if the source resolves nowhere.
                let terminal = compiler
                    .class_alias_target(class_id, &old_name)
                    .unwrap_or(&old_name)
                    .to_string();
                compiler.classes[class_id.0 as usize]
                    .class_aliases
                    .push((new_name, terminal));
                continue;
            }
            // No user `Scope` anywhere in the chain: the source is a BUILTIN
            // (Kernel's `raise`, Object's `dup`, ...) -- or a typo. There is
            // no body to clone either way, so record a NAME indirection
            // (terminal: an alias of a builtin alias resolves through the
            // already-recorded entry) and let the emitted `register_alias`
            // validate at program start -- a nonexistent source is NameError
            // exactly when real Ruby raises it (the class body executing).
            // See `ClassInfo::builtin_aliases`.
            let terminal = compiler
                .builtin_alias_target(class_id, &old_name)
                .unwrap_or(&old_name)
                .to_string();
            compiler.classes[class_id.0 as usize]
                .builtin_aliases
                .push((new_name, terminal));
            continue;
        };
        let scope = compiler.scope(sid);
        let (def_node, params, body, visibility) = (
            scope.def_node,
            scope.params.clone(),
            scope.body.clone(),
            scope.visibility,
        );
        let new_sid = super::register_method(
            compiler, class_id, class_id, new_name, def_node, params, body, visibility,
        )?;
        // `def_node` is the SOURCE's, shared with the original method, so the
        // birth name can't be read off it -- record it on the new scope.
        compiler.scopes[new_sid.0 as usize].alias_of = Some(old_name);
        // At the ALIAS's seq, not a fresh one: a later alias naming this one
        // must see it as defined where the `alias` statement ran.
        super::add_own_method_at(compiler, class_id, new_sid, is_class_method, alias_seq);
    }
    Ok(())
}

/// Resolves this module's `pending_module_functions`: a `module_function :m`
/// whose `m` came in through an `include` (`erb/util.rb`'s `include
/// ERB::Escape; module_function :html_escape`). The source is an INSTANCE
/// method of some ancestor, and the result is a class method of this module,
/// which is why this can't ride `resolve_aliases`' single flag.
///
/// It takes TWO copies, which is what `module_function` means: a public class
/// method, and a PRIVATE instance method for the `include`-mixin half. The
/// private copy is why an includer answers `respond_to?` false while `send`
/// still reaches it (oracle-verified), and it has to be an own copy rather than
/// the inherited original, whose visibility belongs to the module that defined
/// it. A name that resolves nowhere is left alone -- a `module_function` naming
/// a builtin has no body to clone.
fn resolve_module_functions(compiler: &mut Compiler, class_id: ClassId) -> Result<(), String> {
    let pending =
        std::mem::take(&mut compiler.classes[class_id.0 as usize].pending_module_functions);
    for name in pending {
        let ancestors = compiler.class(class_id).ancestors.clone();
        let source = ancestors.iter().find_map(|&anc| {
            compiler
                .class(anc)
                .own_methods
                .iter()
                .find(|&&s| compiler.scope(s).name == name)
                .copied()
        });
        let Some(sid) = source else { continue };
        let scope = compiler.scope(sid);
        let (def_node, params, body) = (scope.def_node, scope.params.clone(), scope.body.clone());
        for (is_class_method, visibility) in
            [(true, Visibility::Public), (false, Visibility::Private)]
        {
            let new_sid = super::register_method(
                compiler,
                class_id,
                class_id,
                name.clone(),
                def_node,
                params.clone(),
                body.clone(),
                visibility,
            )?;
            super::add_own_method(compiler, class_id, new_sid, is_class_method);
        }
    }
    Ok(())
}

/// One clean rule handles override precedence for prepend/include/plain
/// inheritance uniformly, with no special-casing: walk `ancestors(class_id)`
/// in strict MRO order; the FIRST ancestor with an `own_methods` entry for a
/// given name wins. A prepended module sits BEFORE the class in the list
/// (so it naturally wins over the class's own definition); an included
/// module sits AFTER (so the class's own definition naturally wins over
/// it); plain inheritance is just "nothing closer defines it". Reusing the
/// SAME `register_method` pipeline an ordinary `def` goes through means a
/// materialized method's ivars/local-types/bare-block-use are all correctly
/// (re)computed against `class_id` as the owner, with ZERO aliasing/casting
/// trick needed (unlike zeo's C "common initial sequence" struct-prefix
/// hack) -- `self.#ivar` inside it is trivially valid Rust, since the body
/// is freshly re-typechecked against `class_id`'s own concrete struct.
/// The `@ivar` names each class's own method bodies touch, in first-encounter
/// order, indexed by class id.
///
/// One walk per definition. The order matters as much as the contents: a
/// class's slot list is the concatenation of its ancestors' lists furthest-first
/// with duplicates dropped, and dropping the LATER duplicate is what keeps
/// `ivars(C) == ivars(parent(C)) ++ C's own new names` -- the property
/// `analyze::share` leans on so one body can index a slot by a constant across
/// a whole hierarchy. Collecting per class in the same order the old nested
/// walk did, then concatenating, reproduces that sequence exactly.
fn own_ivars(compiler: &Compiler) -> Vec<Vec<String>> {
    compiler
        .classes
        .iter()
        .map(|class| {
            let mut names = Vec::new();
            for &sid in &class.own_methods {
                let scope = compiler.scope(sid);
                for &n in &scope.body {
                    super::collect_ivars(&compiler.hir, n, &mut names);
                }
                for id in scope.params.default_ids() {
                    super::collect_ivars(&compiler.hir, id, &mut names);
                }
            }
            names
        })
        .collect()
}

fn materialize_methods(
    compiler: &mut Compiler,
    class_id: ClassId,
    own_ivars: &[Vec<String>],
) -> Result<(), String> {
    let ancestors = compiler.class(class_id).ancestors.clone();
    let mut seen: HashSet<crate::compiler::NameId> = HashSet::new();
    let mut materialized: Vec<MethodEntry> = Vec::new();
    // Every `private :inherited_method` seen so far in the walk. Ancestors run
    // nearest-first and `or_insert` keeps the first, so by the time a name's
    // definition turns up, this holds the NEAREST re-declaration above it --
    // which is the entry ruby would have found. Collected from each ancestor's
    // own (small) override list rather than asked of its finished method table:
    // one lookup per inherited name, against a table with one entry per
    // inherited name, is quadratic per class, and at Rails scale that is one
    // class taking minutes.
    let mut rescoped: HashMap<crate::compiler::NameId, Visibility> = HashMap::new();
    for &anc_id in &ancestors {
        for (name, visibility) in compiler.class(anc_id).visibility_overrides.clone() {
            let id = compiler.names.intern(&name);
            rescoped.entry(id).or_insert(visibility);
        }
        // A BUILTIN class never materializes `Object`'s methods (top-level
        // `def`s / `Object` reopens): re-emitting each body per builtin
        // would multiply generated code ~30x and re-type `self` as every
        // builtin kind (a body fine on the class it's actually called on
        // could fail rustc when typed as, say, `Str`). Dynamic dispatch
        // still finds them -- `send_value_in`/`send_in`'s MRO walk probes
        // `value_method(ancestor)` per ancestor, and Object's methods are
        // registered as value methods on `ClassId(0)`, the tail every
        // chain ends with.
        if compiler.class(class_id).is_builtin && anc_id == crate::compiler::OBJECT_CLASS {
            continue;
        }
        let own = compiler.class(anc_id).own_methods.clone();
        for sid in own {
            // A `def` under a guard zeo cannot decide contributes nothing to
            // this table. It does not claim the name -- so a further ancestor's
            // definition materializes here and answers while the guard is
            // false -- and it does not shadow one, so no row promises a body
            // that may never have been installed. What the guard DOES install
            // goes into the runtime overlay, which outranks this table.
            // Deliberately not `seen`-inserted, for exactly that reason.
            // See `Scope::runtime_conditional`.
            if compiler.scope(sid).runtime_conditional {
                continue;
            }
            let name = compiler.scope(sid).name.clone();
            let name_id = compiler.names.intern(&name);
            if !seen.insert(name_id) {
                continue; // a closer ancestor already won this name
            }
            // `undef name` in THIS class's body: the name is not
            // materialized onto it at all, from any ancestor -- which
            // removes it from the one table both dispatch paths and
            // `respond_to?` read, so it raises NoMethodError here while
            // staying live on whichever ancestor defined it (that ancestor's
            // own materialization is a separate pass over its own
            // `undefined`, which doesn't contain the name).
            //
            // `seen` is inserted FIRST, deliberately: an undef'd name must
            // also block a FURTHER ancestor from supplying it. `class C < B;
            // undef m; end` where both B and Object define `m` must find
            // neither.
            if compiler.class(class_id).undefined.contains(&name) {
                continue;
            }
            // Inheriting a method BINDS its one definition here; it does not
            // copy it. A pristine exception body stays pristine when inherited
            // (`register_exceptions` serves the subclass's too, so codegen
            // skips it), while a reopen/override body propagates as a real
            // delta onto each descendant -- which is now just the entry
            // carrying the definition's own flag.
            let mut entry = entry_for(compiler, name_id, sid, class_id);
            // What this class inherits is the ancestor's ENTRY, whose
            // visibility an ancestor may have re-declared without redefining
            // (`private :inherited_method` up the chain).
            if let Some(&visibility) = rescoped.get(&name_id) {
                entry.visibility = visibility;
            }
            materialized.push(entry);
        }
    }

    // Ivars are collected from EVERY ancestor's OWN methods -- not just the
    // MRO-winning ones in `materialized` above. `super` can splice in a
    // shadowed ancestor's body at codegen time (`emit_super`) even
    // though that body never gets its own entry in `methods[class_id]` --
    // only the winning override does -- so an ivar only ever touched
    // through a `super`-reachable (but not otherwise winning) method would
    // otherwise be missing from this class's own generated struct entirely.
    // Over-including a field for a method that's shadowed and never even
    // reachable via `super` is harmless (an unused, always-`Nil` field),
    // so this doesn't try to be more precise than "every ancestor's own
    // body, unconditionally".
    //
    // The one ancestor skipped is the same one the method loop above skips,
    // and for the same reason: a builtin never materializes `Object`'s
    // methods, so their ivars are not its ivars either. Collecting them
    // anyway made a top-level `def m; @x; end` attribute `@x` to every
    // builtin in the program and trip the builtin-ivar rejection below --
    // a top-level `def` touching any ivar failed the whole compile, blaming
    // `Integer`. The two loops must agree on what belongs to this class.
    //
    // Ancestors are walked FURTHEST-FIRST here, the reverse of the method loop
    // above, which makes each class's slot list start with its parent's list
    // verbatim: `ivars(C) == ivars(parent(C)) ++ C's own new names`. Since a
    // parent's `ancestors` is a suffix of its child's (MRO keeps the relative
    // order of everything it inherits), reversing is all the property needs.
    // `analyze::share` depends on it -- one body shared by a base and 151
    // descendants indexes a slot by a compile-time constant, which is only the
    // same constant everywhere if the base's names sit at the same indices on
    // every descendant. Slot ORDER is otherwise unobservable: `instance_
    // variables` and `inspect` report FIRST-ASSIGNMENT order, which
    // `IvarCell`'s per-slot stamp carries independently of the layout.
    let mut ivars: Vec<String> = Vec::new();
    for &anc_id in ancestors.iter().rev() {
        if compiler.class(class_id).is_builtin && anc_id == crate::compiler::OBJECT_CLASS {
            continue;
        }
        for name in &own_ivars[anc_id.0 as usize] {
            if !ivars.iter().any(|n| n == name) {
                ivars.push(name.clone());
            }
        }
    }

    // A compiled `Struct`'s MEMBERS are collected here like any other `@x`
    // (its synthesized `initialize` assigns them), but they are not instance
    // variables. Subtracting them once, here, is what lets every consumer --
    // `ivar_slot`, `__IVAR_NAMES`, `emit_class` -- read the two lists as
    // disjoint without re-deriving the split.
    //
    // A SUBCLASS of a compiled struct inherits the member list the same way it
    // inherits the methods that reach it: `class Point3 < Point` gets Point's
    // `x`/`y` as members, not as ordinary ivars, or `to_a` on a `Point3` would
    // read nothing and `instance_variables` would report two names CRuby does
    // not. Nearest ancestor wins, and only a class with none of its own asks.
    if compiler.class(class_id).hidden_ivars.is_empty()
        && let Some(inherited) = ancestors
            .iter()
            .skip(1)
            .map(|&a| &compiler.class(a).hidden_ivars)
            .find(|h| !h.is_empty())
            .cloned()
    {
        compiler.classes[class_id.0 as usize].hidden_ivars = inherited;
    }
    let hidden = compiler.class(class_id).hidden_ivars.clone();
    if !hidden.is_empty() {
        ivars.retain(|iv| !hidden.contains(iv));
    }

    // A reopened BUILTIN class has no generated struct, so an `@ivar` in one
    // of its methods cannot become a struct field -- but it does not need to.
    // `emit_builtin_method_fn` already marks every such body's self dynamic
    // (its receiver is the free function's `__self: RubyValue`), which routes
    // the access through `ivar_get_dyn`/`ivar_set_dyn`. Those pick the tier
    // from the receiver: `RObj`'s name-keyed map for an Object-payload builtin,
    // `civars` for a class object, and `value_ivars`' identity-keyed side table
    // for a bare heap value. `Object` -- where top-level `def`s live -- has
    // always taken this path; the other builtins differ only in never having
    // been allowed to.
    //
    // The `ci.ivars` recorded below stays unread for all of them: it exists to
    // drive generated STRUCT fields, and `has_struct` excludes every builtin.
    //
    // Cost, where a user object would have had a struct field: an `RwLock` hash
    // lookup plus a linear scan of that value's slots, and `value_ivars`'
    // `OWNERS` pins the receiver for the life of the process.
    let ci = &mut compiler.classes[class_id.0 as usize];
    ci.methods = materialized;
    ci.ivars = ivars;
    Ok(())
}

/// Class methods are inherited via the plain SUPERCLASS chain only -- never
/// through `include`d modules (`include` only ever affects instance-method
/// resolution; that's exactly what `extend` is for on the class side) --
/// mirroring real Ruby's own singleton-class-inherits-from-singleton-class
/// rule, without actually modeling singleton classes. At each level: this
/// class/ancestor's own `def self.x` always wins; otherwise its most
/// recently `extend`ed module wins (mirrors `include`'s "closer" rule,
/// pulling in the module's INSTANCE methods, `own_methods` -- matching real
/// Ruby, where `def self.x` on the module itself stays put). The nearest
/// level (`class_id` itself) always wins over anything found further up.
///
/// The walk is guarded: `parent` is a graph the front end builds, not a
/// verified list, and a cycle in it (a reopen that establishes a superclass
/// already below this one) turns this loop into an infinite one that appends
/// entries until the process is killed. That is exactly how `require
/// "active_record"` died -- 12 minutes, no diagnostic, RSS climbing 13MB a
/// second, every sample inside this function.
///
/// Binds one definition onto `owner`. Everything but the owner comes from the
/// definition itself, which is the whole content of the entry/definition split:
/// inheriting a method is an entry, not a copy.
fn entry_for(compiler: &Compiler, name: NameId, def: ScopeId, owner: ClassId) -> MethodEntry {
    let scope = compiler.scope(def);
    MethodEntry {
        name,
        def,
        owner,
        visibility: scope.visibility,
        native_default: scope.native_default,
        zsuper: false,
    }
}

/// Applies this class's `private`/`public`/`protected :m` re-declarations onto
/// its own entries.
///
/// A re-declaration that names a method the class INHERITED is ruby's ZSUPER
/// entry: the name becomes the subclass's own, at the new visibility, still
/// running the ancestor's body. It has to land on the entry rather than only in
/// a runtime row, because a Path-1 call site resolves visibility at compile
/// time and would otherwise let a private call through -- `Sub.new.visible`
/// after `private :visible` ran fine instead of raising NoMethodError.
fn apply_visibility_overrides(compiler: &mut Compiler, class_id: ClassId) {
    for (name, visibility) in compiler.class(class_id).visibility_overrides.clone() {
        let Some(id) = compiler.names.get(&name) else {
            continue;
        };
        let entries = &mut compiler.classes[class_id.0 as usize].methods;
        if let Some(entry) = entries.iter_mut().find(|e| e.name == id) {
            let inherited = entry.owner != compiler.scopes[entry.def.0 as usize].defining_class;
            entry.visibility = visibility;
            entry.zsuper |= inherited;
        }
    }
    for (name, visibility) in compiler.class(class_id).class_visibility_overrides.clone() {
        let Some(id) = compiler.names.get(&name) else {
            continue;
        };
        let entries = &mut compiler.classes[class_id.0 as usize].class_methods;
        if let Some(entry) = entries.iter_mut().find(|e| e.name == id) {
            entry.visibility = visibility;
        }
    }
}

fn materialize_class_methods(compiler: &mut Compiler, class_id: ClassId) -> Result<(), String> {
    // `undef_method :m` inside this class's `class << self`: the name is not
    // materialized onto it from any position, so it raises NoMethodError here
    // while staying live on the ancestor that defined it. `seen` still claims
    // the name first, so an undef also blocks a FURTHER ancestor from
    // supplying it -- the instance-side rule in `materialize_methods`.
    let undefined = compiler.class(class_id).class_undefined.clone();
    let mut seen: HashSet<crate::compiler::NameId> = HashSet::new();
    let mut materialized: Vec<MethodEntry> = Vec::new();
    let mut singleton_targets: Vec<(ClassId, crate::compiler::ScopeId)> = Vec::new();
    let mut level = Some(class_id);
    let mut walked: HashSet<ClassId> = HashSet::new();

    while let Some(cid) = level {
        if !walked.insert(cid) {
            tracing::warn!(
                class = compiler.fq_name(class_id),
                at = compiler.fq_name(cid),
                "mro: superclass chain revisits a class -- ending the singleton walk"
            );
            break;
        }
        // Singleton-PREPENDED modules (`C.singleton_class.prepend(M)`): their
        // INSTANCE methods become this level's class methods at HIGHER priority
        // than its own `def self.x`, which they shadow (`super` reaching the
        // original). Processed BEFORE own_class_methods so they win -- the
        // class-method mirror of `prepends` on the instance side. Most recently
        // prepended is closest (reverse, as `extends`/`prepends` expand). Every
        // position's own copy joins `singleton_targets` so a `super` chain finds
        // the receiver's own copy at any position.
        for &m in compiler
            .class(cid)
            .class_method_prepends
            .clone()
            .iter()
            .rev()
        {
            for sid in compiler.class(m).own_methods.clone() {
                let name = compiler.scope(sid).name.clone();
                let name_id = compiler.names.intern(&name);
                let is_winner = seen.insert(name_id);
                if undefined.contains(&name) {
                    continue;
                }
                if is_winner {
                    materialized.push(entry_for(compiler, name_id, sid, class_id));
                }
                singleton_targets.push((m, sid));
            }
        }
        for sid in compiler.class(cid).own_class_methods.clone() {
            let name = compiler.scope(sid).name.clone();
            let name_id = compiler.names.intern(&name);
            let is_winner = seen.insert(name_id);
            if undefined.contains(&name) {
                continue;
            }
            if cid == class_id {
                if is_winner {
                    materialized.push(entry_for(compiler, name_id, sid, class_id));
                } else {
                    // SHADOWED by a singleton prepend above: this class's own
                    // `def self.x` is no longer the dispatched method, but the
                    // prepended module's `super` resolves to it. Keep it as a
                    // super TARGET keyed under this class's OWN id (the
                    // module_instance-side lookup `call_singleton_super_target`
                    // consults; see `codegen`'s shadowed-target registration).
                    singleton_targets.push((class_id, sid));
                }
            } else if is_winner {
                materialized.push(entry_for(compiler, name_id, sid, class_id));
            } else {
                singleton_targets.push((cid, sid));
            }
        }
        for &m in compiler.class(cid).extends.clone().iter().rev() {
            for sid in compiler.class(m).own_methods.clone() {
                let name = compiler.scope(sid).name.clone();
                // SHADOWED copies register too (into the singleton-super
                // pool below, not the flattened winner set): a sibling-
                // extend `super` chain needs every position's own copy,
                // emitted in THIS class's context so its own `super`
                // resumes the chain here -- the `own_impls` distinction,
                // on the singleton side.
                let name_id = compiler.names.intern(&name);
                let is_winner = seen.insert(name_id);
                if undefined.contains(&name) {
                    continue;
                }
                if is_winner {
                    materialized.push(entry_for(compiler, name_id, sid, class_id));
                }
                singleton_targets.push((m, sid));
            }
        }
        level = compiler.class(cid).parent;
    }

    compiler.classes[class_id.0 as usize].class_methods = materialized;
    compiler.classes[class_id.0 as usize].singleton_super_targets = singleton_targets;
    Ok(())
}

/// `@@x` ownership (nearest ancestor -- including self -- that ever claimed
/// the name first owns the runtime storage; fixes a real bug found in zeo's
/// own C implementation, where a subclass writing a superclass-only cvar
/// silently allocated fresh, WRONG per-class storage) and bare-constant
/// ownership (the exact same scheme -- see `const_owner_of` for its extra
/// lexical/`Object` steps), resolved together: both families' name sets come
/// out of ONE walk over each class's own bodies, where they previously each
/// traversed every body in the program. A bare `@@x` or `NAME = ...` written
/// outside any class/module body lives on `Object` (real Ruby stores
/// top-level constants on `Object` itself), so `main_statements` -- otherwise
/// never scanned by anything in this module -- seeds it first, making
/// `Object` the owner every later reference resolves to. Per-class
/// resolution runs in declaration order (`compiler.classes`' index order IS
/// file order, since `resolve_class` already requires a target to be defined
/// earlier), so every ancestor's own map is fully resolved by the time a
/// later class searches it.
fn resolve_cvars_and_consts(
    compiler: &mut Compiler,
    main_statements: &[NodeId],
) -> Result<(), String> {
    let mut top_consts = NameList::default();
    let mut top_cvars = NameList::default();
    for &n in main_statements {
        collect_ownership_names(compiler, n, &[], &mut top_consts, &mut top_cvars);
    }
    for name in top_cvars.names {
        compiler.classes[OBJECT_CLASS.0 as usize]
            .cvar_owners
            .entry(name)
            .or_insert(OBJECT_CLASS);
    }
    for name in top_consts.names {
        compiler.classes[OBJECT_CLASS.0 as usize]
            .const_owners
            .entry(name)
            .or_insert(OBJECT_CLASS);
    }

    let per_class: Vec<(NameList, NameList)> = (0..compiler.classes.len() as u32)
        .map(|i| own_ownership_names(compiler, ClassId(i)))
        .collect();
    for (i, (_, cvars)) in per_class.iter().enumerate() {
        for name in &cvars.names {
            owner_of(compiler, ClassId(i as u32), name);
        }
    }
    for (i, (consts, _)) in per_class.iter().enumerate() {
        for name in &consts.names {
            const_owner_of(compiler, ClassId(i as u32), name);
        }
    }
    // A `class << self` def's bare constants resolve from the SURROGATE
    // (`Scope::lexical_home`), whose map the per-class loop above never
    // fills -- the scope itself is owned by the enclosing class. Resolve
    // those scopes' names against the surrogate too, so codegen's map
    // lookup finds the lexical owner (`expr::const_owner_id_opt`
    // self-defaults on a miss, which would wrongly claim the surrogate).
    let tagged: Vec<(ClassId, Vec<crate::hir::NodeId>)> = compiler
        .scopes
        .iter()
        .filter_map(|s| s.lexical_home.map(|h| (h, s.body.clone())))
        .collect();
    for (home, body) in tagged {
        let mut consts = NameList::default();
        let mut cvars = NameList::default();
        for &n in &body {
            collect_ownership_names(compiler, n, &[], &mut consts, &mut cvars);
        }
        for name in consts.names {
            const_owner_of(compiler, home, &name);
        }
    }
    Ok(())
}

/// Both families' names from `class_id`'s own literal bodies -- its
/// class-body-top-level statements (`@@x = 0` directly inside `class Foo;
/// ... end`), every `own_methods` body, AND every `own_class_methods` body
/// (`def self.x` can reference `@@x` too) -- as `(consts, cvars)`, each in
/// first-reference order. Scanning READS too (not just writes) is what lets
/// a subclass's own method discover an inherited name's real owner: a name
/// only ever WRITTEN by an ancestor and merely READ by `class_id` would
/// otherwise silently resolve to `class_id` itself. Bare-`ClassRef`
/// classification resolves against THIS class's lexical chain: a bare `Item`
/// inside `module Store` naming the nested `Store::Item` class must not be
/// misclassified as a value-constant reference.
fn own_ownership_names(compiler: &Compiler, class_id: ClassId) -> (NameList, NameList) {
    let cref = compiler.cref_of_ref(class_id);
    let mut consts = NameList::default();
    let mut cvars = NameList::default();
    for &n in &compiler.class(class_id).class_body_stmts {
        collect_ownership_names(compiler, n, cref, &mut consts, &mut cvars);
    }
    for &sid in compiler
        .class(class_id)
        .own_methods
        .iter()
        .chain(compiler.class(class_id).own_class_methods.iter())
    {
        for &n in &compiler.scope(sid).body {
            collect_ownership_names(compiler, n, cref, &mut consts, &mut cvars);
        }
    }
    (consts, cvars)
}

/// Resolves (and memoizes onto `ClassInfo::cvar_owners`) which class/module
/// owns `@@name`'s storage as referenced from `class_id`: reuse an
/// already-resolved owner if `class_id` itself has one, else search
/// `ancestors` (excluding `class_id`, nearest first, guaranteed already
/// resolved by file-order) for the first ancestor that claims it, else
/// `class_id` becomes the new owner.
fn owner_of(compiler: &mut Compiler, class_id: ClassId, name: &str) -> ClassId {
    if let Some(&owner) = compiler.class(class_id).cvar_owners.get(name) {
        return owner;
    }
    let owner = compiler
        .class(class_id)
        .ancestors
        .iter()
        .skip(1) // ancestors[0] is class_id itself
        .find_map(|&anc| compiler.class(anc).cvar_owners.get(name).copied())
        .unwrap_or(class_id);
    compiler.classes[class_id.0 as usize]
        .cvar_owners
        .insert(name.to_string(), owner);
    owner
}

/// Does `class_id`'s own literal class body assign `NAME = ...` (bare) at
/// statement level -- real Ruby's "defined in this scope's own const
/// table"? Statement-level only: a constant assignment nested under
/// control flow inside a class body (`X = 1 if cond`) isn't discovered
/// here (a narrow, documented approximation; a bare assignment inside a
/// METHOD body is a Ruby SyntaxError -- "dynamic constant assignment" --
/// so class bodies are genuinely the only place to look).
/// A top-level `NAME = ...` belongs to `Object`, exactly as one written in a
/// `class Object` reopen does -- but it arrives in the main statement stream
/// rather than any class body, so nothing claimed it and `defined?(NAME)`
/// answered nil even after the assignment had run. Statement-level only, the
/// same rule a class body's own list follows: `NAME = 1 if cond` stays
/// unclaimed rather than naming an owner it may never get. `class_body_stmts`
/// is analysis-only (execution is per-site), so this adds no second run.
fn record_top_level_consts(compiler: &mut Compiler, main_statements: &[NodeId]) {
    let mut claims: Vec<(ClassId, NodeId)> = Vec::new();
    let mut frames: Vec<(ClassId, Vec<NodeId>)> = vec![(OBJECT_CLASS, main_statements.to_vec())];
    while let Some((owner, stmts)) = frames.pop() {
        for s in stmts {
            match &compiler.hir[s] {
                HirNode::ConstWrite { scope: None, .. } => claims.push((owner, s)),
                // A `Ruby::Box`'s top level is its own scope, with a surrogate
                // standing in for `Object`.
                HirNode::BoxScope { box_id, body } => {
                    let inner = compiler
                        .box_surrogates
                        .get(box_id)
                        .copied()
                        .unwrap_or(OBJECT_CLASS);
                    frames.push((inner, body.clone()));
                }
                _ => {}
            }
        }
    }
    for (owner, stmt) in claims {
        compiler.classes[owner.0 as usize]
            .class_body_stmts
            .push(stmt);
    }
}

/// Numbers the program's statements in EXECUTION order and records where each
/// constant first becomes defined, so `defined?` can tell a definition that
/// has already run from one written further down the file.
///
/// The walk stops at a `def`, a lambda and a block body: those run when they
/// are CALLED, which is not a position this walk can name. Leaving them
/// unnumbered is what keeps the whole thing a NARROWING -- a query with no
/// position, or a constant with no recorded definition position, falls back to
/// the whole-program answer that was there before.
fn index_document_order(compiler: &mut Compiler, main_statements: &[NodeId]) {
    let sites: std::collections::HashMap<NodeId, usize> = compiler
        .class_body_sites
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.def_node.map(|n| (n, i)))
        .collect();
    let mut next = 0u32;
    index_stmts(compiler, OBJECT_CLASS, main_statements, &sites, &mut next);
}

fn index_stmts(
    compiler: &mut Compiler,
    owner: ClassId,
    stmts: &[NodeId],
    sites: &std::collections::HashMap<NodeId, usize>,
    next: &mut u32,
) {
    for &s in stmts {
        index_node(compiler, owner, s, sites, next);
    }
}

fn index_node(
    compiler: &mut Compiler,
    owner: ClassId,
    node: NodeId,
    sites: &std::collections::HashMap<NodeId, usize>,
    next: &mut u32,
) {
    let pos = *next;
    *next += 1;
    compiler.doc_order.insert(node, pos);
    match &compiler.hir[node] {
        HirNode::DefMethod { .. } | HirNode::Lambda { .. } | HirNode::Block { .. } => {}
        HirNode::ClassDef { .. } => {
            // The class object exists before its body runs, so the marker's own
            // position is where the constant starts answering.
            let Some(&site) = sites.get(&node) else {
                return;
            };
            let class = compiler.class_body_sites[site].class;
            let ci = compiler.class(class);
            let short = ci.name.rsplit("::").next().unwrap_or(&ci.name).to_string();
            let const_owner = ci.lexical_parent.unwrap_or(OBJECT_CLASS);
            compiler
                .const_def_order
                .entry((const_owner, short))
                .or_insert(pos);
            let body = compiler.class_body_sites[site].stmts.clone();
            index_stmts(compiler, class, &body, sites, next);
        }
        HirNode::ConstWrite {
            scope: None,
            name,
            value,
        } => {
            // The name starts answering only once the VALUE has been computed,
            // which is what makes `X = defined?(X)` nil.
            let (name, value) = (name.clone(), *value);
            index_node(compiler, owner, value, sites, next);
            let after = *next;
            *next += 1;
            compiler
                .const_def_order
                .entry((owner, name))
                .or_insert(after);
        }
        // A box's top level is its own scope; everything else is an ordinary
        // statement container whose children run right here.
        HirNode::BoxScope { box_id, body } => {
            let (box_id, body) = (*box_id, body.clone());
            let inner = compiler
                .box_surrogates
                .get(&box_id)
                .copied()
                .unwrap_or(OBJECT_CLASS);
            index_stmts(compiler, inner, &body, sites, next);
        }
        _ => {
            let mut kids = Vec::new();
            compiler.hir[node].for_each_child(&mut |c| kids.push(c));
            index_stmts(compiler, owner, &kids, sites, next);
        }
    }
}

pub(crate) fn directly_defines_const(compiler: &Compiler, class_id: ClassId, name: &str) -> bool {
    if let Some(defs) = &compiler.direct_const_defs {
        return defs
            .get(class_id.0 as usize)
            .is_some_and(|set| set.contains(name));
    }
    compiler.class(class_id).class_body_stmts.iter().any(|&n| {
        matches!(&compiler.hir[n], HirNode::ConstWrite { scope: None, name: w, .. } if w == name)
    })
}

/// Resolves (and memoizes onto `ClassInfo::const_owners`) which class/module
/// owns a bare constant `name` as referenced from `class_id` -- real Ruby's
/// resolution order (oracle-verified): the referencing scope's
/// OWN definition first, then the ENCLOSING lexical scopes (innermost
/// first, each checked for a DIRECT definition -- never their inherited/
/// memoized claims, which is what real Ruby's per-scope const-table check
/// means), then `ancestors` (nearest first, memo-based like `owner_of`),
/// then `Object` (real Ruby's final stop -- reachable through `ancestors`
/// for a class, but a MODULE's ancestors don't include `Object`, so it
/// needs its own step), else `class_id` self-claims (the runtime lookup
/// then correctly reports unset as `NameError`).
fn const_owner_of(compiler: &mut Compiler, class_id: ClassId, name: &str) -> ClassId {
    if let Some(&owner) = compiler.class(class_id).const_owners.get(name) {
        return owner;
    }
    let owner = if directly_defines_const(compiler, class_id, name) {
        class_id
    } else {
        compiler
            .cref_of_ref(class_id)
            .iter()
            .rev()
            .skip(1) // the chain ends with class_id itself, checked above
            .copied()
            .find(|&scope| directly_defines_const(compiler, scope, name))
            .or_else(|| {
                compiler
                    .class(class_id)
                    .ancestors
                    .iter()
                    .skip(1)
                    .find_map(|&anc| compiler.class(anc).const_owners.get(name).copied())
            })
            .or_else(|| compiler.class(OBJECT_CLASS).const_owners.get(name).copied())
            .unwrap_or(class_id)
    };
    compiler.classes[class_id.0 as usize]
        .const_owners
        .insert(name.to_string(), owner);
    owner
}

/// A constant or cvar named by a multi-assignment or `for` TARGET -- `X, @@y
/// = 1, 2` in a module body. Mirrors the walk arms' own rules: a bare
/// `Const`/`ClassVar` counts, a `ScopedConst` (`Foo::NAME, ... = ...`)
/// doesn't, its owner being already named rather than discovered. See
/// `collect_ivar_target` for why `MultiTarget::for_each_node` alone couldn't
/// see these; the cvar failure mode is the quieter and so worse one -- a
/// cvar missing from this list registers its ownership against the wrong
/// class, and the program still compiles.
fn collect_target_names(
    target: &crate::hir::MultiTarget,
    consts: &mut NameList,
    cvars: &mut NameList,
) {
    match target {
        crate::hir::MultiTarget::Const(name) => consts.push(name),
        crate::hir::MultiTarget::ClassVar(name) => cvars.push(name),
        _ => {}
    }
}

/// Insertion-ordered name accumulator: the `Vec` keeps first-reference order
/// (the ownership loops resolve in it), the set makes membership O(1) -- the
/// per-node `Vec::contains` these walks used was quadratic at gem scale.
#[derive(Default)]
struct NameList {
    names: Vec<String>,
    seen: crate::compiler::FSet<String>,
}

impl NameList {
    fn push(&mut self, name: &str) {
        if !self.seen.contains(name) {
            self.seen.insert(name.to_string());
            self.names.push(name.to_string());
        }
    }
}

/// Every bare-constant reference AND every class-variable name under `id`,
/// both families over ONE shared `HirNode::for_each_child` walk. A bare
/// `ClassRef` counts as a constant ONLY when `name` ISN'T actually a
/// registered class/module (a real class reference, e.g. `Foo.bar`, is
/// never a constant-ownership concern -- see `HirNode::ClassRef`'s dual
/// reuse, `codegen::expr`'s docs); a bare `ConstWrite { scope: None, .. }`
/// always counts (an explicit `Foo::NAME` write needs no ownership
/// DISCOVERY, its target is already named); `ClassVarRead`/`ClassVarWrite`
/// always count.
fn collect_ownership_names(
    compiler: &Compiler,
    id: crate::hir::NodeId,
    cref: &[ClassId],
    consts: &mut NameList,
    cvars: &mut NameList,
) {
    let hir = &compiler.hir;
    match &hir[id] {
        // An FFI wrapper body references no constants and reads only its
        // synthetic parameters.
        HirNode::Ffi(_) => return,
        // A `class`/`def` body is a fresh Ruby scope, scanned under its own
        // owner rather than the one this walk is filling.
        HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => return,
        HirNode::ClassRef(name) => {
            if compiler.resolve_class(name, cref, 0).is_none() {
                consts.push(name);
            }
        }
        // `Foo.new` names a constant just as a bare `Foo` does. Left out, a
        // class reached ONLY through `.new` -- `Fast = ::StringIO` in a
        // module body, then `Fast.new` from a class nested in it -- never
        // got an ownership entry, so codegen's runtime fallback read it off
        // the referencing class instead of the lexical parent that holds it
        // and raised `NameError`. Adding a bare `Fast` read anywhere in the
        // same class hid the bug, which is what made it look like a
        // resolution problem rather than a missing collector arm.
        HirNode::New { class_name, .. } => {
            if compiler.resolve_class(class_name, cref, 0).is_none() {
                consts.push(class_name);
            }
        }
        HirNode::ConstWrite {
            scope: None, name, ..
        } => consts.push(name),
        HirNode::ClassVarRead(name) | HirNode::ClassVarWrite(name, _) => cvars.push(name),
        HirNode::For { target, .. } => {
            target.for_each_target(&mut |t| collect_target_names(t, consts, cvars))
        }
        HirNode::MultiWrite { targets, .. } => {
            targets.for_each_target(&mut |t| collect_target_names(t, consts, cvars))
        }
        _ => {}
    }
    hir[id].for_each_child(&mut |n| collect_ownership_names(compiler, n, cref, consts, cvars));
}
