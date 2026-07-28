//! Compile-time `require`/`require_relative`/`load` resolution: an
//! HIR-level graft, not text surgery. When the FILE-LEVEL
//! statement loop below hits one of the three call shapes (receiver-less,
//! single string-literal argument, direct top-level statement position --
//! anywhere else, `lower_node`'s unconditional rejection fires instead),
//! the target file is resolved, parsed, and lowered into the SAME `Hir`
//! arena, its statements spliced into the requiring file's statement list
//! at the call's position, in document order. Dedup and provenance are
//! per-`Hir` (`Hir::loaded_files`), and each spliced file's top-level locals
//! are renamed for real per-file isolation (see `parse::rename`).
//!
//! Faithfulness contract (each verified against CRuby source and/or
//! empirically -- see the plan's Part 12 addendum):
//! - `require` dedup is by canonicalized (symlink-resolved) path, mirroring
//!   CRuby's realpath layer: every spelling of one file loads once.
//! - The dedup entry is inserted BEFORE the file is lowered -- CRuby's
//!   `loading_table` behavior -- so a circular require splices nothing the
//!   second time and the statement order comes out exactly like Ruby's
//!   execution order (A-start, all of B, A-rest).
//! - `require` appends `.rb` unless already present; a plain feature is
//!   searched against the ordered `-I` roots as `<root>/<feature>.rb`,
//!   first hit wins (CRuby's `-I`-before-everything ordering; package
//!   roots are appended AFTER these).
//! - `require_relative` resolves against the requiring FILE's directory
//!   (never cwd); with no input-path context the error is CRuby's own
//!   "cannot infer basepath".
//! - `load` never appends `.rb` and never dedups: every `load` statement
//!   re-splices the file fresh (top-level side effects re-run, defs
//!   re-register under the ordinary reopening rules, and the per-INSTANCE
//!   local rename gives each execution the fresh local scope real Ruby
//!   gives it). A `load` cycle -- infinite recursion at runtime in real
//!   Ruby -- is a loud compile error here instead.
//! - A plain `require "feature"` zeo can't resolve is NOT a compile error: a
//!   resolvability pre-scan records it (`Hir::unresolvable_requires`) and its
//!   CALL lowers to a runtime `Kernel#require` (raising CRuby's `LoadError`,
//!   `cannot load such file -- <name>`). So a genuinely-missing feature crashes
//!   at its require site and the optional-dependency idiom (`begin; require
//!   "x"; rescue LoadError`) is caught at RUNTIME -- exactly CRuby's semantics
//!   (zeo has a runtime loader that raises; it does not resolve `require` names
//!   at compile time only). A missing `require_relative` DOES stay a compile
//!   error: it names a project-local file that must exist, never an optional
//!   dependency.
//!
//! GEMS sit on top of that: a `.gemspec`-manifested directory
//! contributing one or more search roots (`require_paths`, default
//! `["lib"]` -- read from a real gemspec, see `Gem`'s docs),
//! searched AFTER every `-I` root, with a feature provided by more than one
//! gem a loud error and first-NAME-wins shadowing across gem dirs. `load`
//! deliberately does NOT search gem roots (its compile-time uses are
//! project-local; a gem's own files arrive via `require`).
//!
//! Documented divergences: a `require` that must SPLICE a file is still
//! statement-position-only, so its return value is unobservable there (a
//! `require` of a natively-provided feature does return real Ruby's
//! true/false from any position -- see `parse::mod`); `load`'s
//! plain relative names resolve against the roots then the requiring
//! file's directory (real Ruby: `$LOAD_PATH` then cwd -- cwd is
//! meaningless at compile time); `load`'s `wrap` parameter, `.so`/native
//! features, and `~`/`./`-prefixed `require` forms are clean rejections;
//! per-file magic comments (`frozen_string_literal`) and `__END__`/`DATA`
//! are pre-existing unsupported territory, unchanged by splicing.

use crate::hir::{Hir, HirNode, LoadedFile, NodeId};
use crate::lower_error::LowerError;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use crate::lower::context::{BindingsFrame, SourceFileFrame, current_box_binding};
use crate::lower::features::{canonical_ext_feature, is_builtin_feature};
use crate::lower::{PResult, lower_node};
use crate::rename;

/// One gem: a named directory with a `.gemspec`, contributing one or more
/// `require` search roots.
///
/// This is a gem in RubyGems' own sense, not a zeo invention -- the
/// manifest is a real gemspec (see `parse::gemspec`), so a bundled zeo
/// library, a vendored gem, and a gem out of a real installed store all read
/// through one code path. A gem reduces to exactly a name plus its
/// `require_paths` (PLURAL, default `["lib"]`), which is why an installed
/// store's `specifications/` directory can feed the same values later with no
/// resolver changes.
pub(super) struct Gem {
    name: String,
    /// Absolute, existence-checked root directories, in `require_paths` order.
    roots: Vec<PathBuf>,
}

impl Gem {
    /// Build a gem from already-resolved parts -- the external gem store
    /// provider's path (`gem_store`), which has done its own existence checks.
    pub(super) fn from_parts(name: String, roots: Vec<PathBuf>) -> Self {
        Gem { name, roots }
    }
}

pub(super) struct Loader {
    /// Ordered `-I` search roots (first hit wins, and they win over
    /// packages entirely -- CRuby's own "-I beats even default gems" rule).
    roots: Vec<PathBuf>,
    /// Discovered packages, in package-dir order with first-NAME-wins
    /// shadowing across dirs (see `discover_packages`).
    packages: Vec<Gem>,
    /// `require` dedup, keyed `(box_id, canonical path)` -- the box
    /// dimension is always 0 for now (per-box feature tables are
    /// exactly how real `Ruby::Box` re-executes a file per box, so the key
    /// shape is decided now to make later box support additive).
    required: HashSet<(u32, PathBuf)>,
    /// Every file currently being spliced (innermost last) -- a `load`
    /// cycle is the one shape with no natural termination (require's dedup
    /// terminates require cycles), so it's detected here and rejected.
    splicing: Vec<PathBuf>,
    /// External-store gems zeo can't provide, `name -> reason`:
    /// a `require` of one fails with the store's precise reason (which native
    /// layout, why) instead of the generic "cannot load such file".
    store_exclusions: HashMap<String, String>,
    /// How each `require`d library was satisfied, in require order.
    /// A LOG of what resolution did, not a property of the program -- it used
    /// to hang off `Hir`, which made the IR depend on the gem reporter for
    /// bookkeeping no consumer of the arena ever reads.
    gem_records: Vec<crate::gem_report::GemRecord>,
}

/// Lowers the MAIN file's statements, resolving require/require_relative/
/// load recursively -- the entry point `parse_and_lower_with` uses for the
/// user's own source (the exception prelude and `eval` bodies keep going
/// through plain `parse_and_lower_into`, where the three call shapes are
/// rejected by `lower_node` instead).
pub(super) fn lower_main_file(
    hir: &mut Hir,
    source: &str,
    input_path: Option<&Path>,
    load_roots: &[PathBuf],
    package_dirs: &[PathBuf],
    gem_path: Option<&Path>,
    lockfile: Option<&Path>,
) -> PResult<(Vec<NodeId>, Vec<crate::gem_report::GemRecord>)> {
    // The requiring-file directory for the main file's own require_relative
    // calls -- canonicalized so require_relative composes with the dedup
    // layer exactly like CRuby's realpath-of-the-requiring-iseq base.
    let dir = match input_path {
        Some(p) => Some(
            p.canonicalize()
                .map_err(|e| format!("resolving input path {}: {e}", p.display()))?
                .parent()
                .ok_or("input path has no parent directory")?
                .to_path_buf(),
        ),
        None => None,
    };
    let mut loader = Loader {
        roots: load_roots.to_vec(),
        // The gems zeo itself ships are ALWAYS discoverable, appended
        // last so any caller-supplied dir shadows them (first-name-wins).
        // They are part of the compiler the way CRuby's rubylibdir is part of
        // ruby -- not ambient machine state a caller opts into. A caller that
        // forgot them would resolve a two-half gem's NATIVE half and silently
        // miss its Ruby half, which is how `StringScanner::Error` would go
        // missing and turn a raise into a panic.
        packages: discover_packages(
            &package_dirs
                .iter()
                .cloned()
                .chain(bundled_gems_dir())
                .collect::<Vec<_>>(),
        )?,
        required: HashSet::new(),
        splicing: Vec::new(),
        store_exclusions: HashMap::new(),
        gem_records: Vec::new(),
    };
    // The external gem store: a `--gem-path` + `--lockfile` pair adds
    // the pure-Ruby gems zeo can compile as extra roots, and records a
    // disclosure for every gem it satisfies natively or can't provide. Store
    // gems are APPENDED, so a bundled zeo gem of the same name shadows them
    // (first-name-wins), and never override the compiler's own libraries.
    if let (Some(store), Some(lock)) = (gem_path, lockfile) {
        let parsed = super::lockfile::parse_file(lock)?;
        let resolution = super::gem_store::resolve(store, &parsed)?;
        for (name, roots) in resolution.roots {
            if !loader.packages.iter().any(|g| g.name == name) {
                loader.packages.push(Gem::from_parts(name, roots));
            }
        }
        for record in resolution.disclosures {
            // An excluded gem's reason is kept so a `require` of it fails
            // precisely; every record is also disclosed in the report.
            if let crate::gem_report::SatisfiedBy::Excluded { reason, .. } = &record.by {
                loader
                    .store_exclusions
                    .insert(record.name.clone(), reason.clone());
            }
            loader.record_gem(record);
        }
    }
    let result = ruby_prism::parse(source.as_bytes());
    if let Some(err) = result.errors().next() {
        return Err(LowerError::syntax(format!(
            "parse error: {}",
            err.message()
        )));
    }
    let main_name = input_path.map_or_else(|| "-e".to_string(), |p| p.display().to_string());
    collect_parse_warnings(hir, &result, &main_name, source);
    let program = result
        .node()
        .as_program_node()
        .ok_or("expected a top-level ProgramNode")?;
    // The main file is `__FILE__`'s answer for its own statements -- held
    // for exactly this lowering, and restored by the guard's Drop.
    let _file = SourceFileFrame::push(input_path);
    // Span provenance: the main file's name AS GIVEN (matching `__FILE__`),
    // `"-e"` for a pathless source string.
    let main_file = hir.add_file(main_name, source);
    let prev_file = hir.lowering_file.replace(main_file);
    let lowered = loader.preload_ambient_features(hir).and_then(|mut all| {
        all.extend(loader.lower_file_statements(
            hir,
            &result,
            program.statements().body(),
            dir.as_deref(),
            None,
            0,
        )?);
        Ok(all)
    });
    hir.lowering_file = prev_file;
    Ok((lowered?, loader.gem_records))
}

/// What `ruby` has already loaded before the main script's first line: it
/// requires rubygems at startup, which requires `rbconfig`. So `RbConfig` is
/// ambient in every Ruby process, and libraries read it without requiring it
/// (`resolv.rb` consults `::RbConfig::CONFIG['host_os']` at load time).
const PRELOADED_FEATURES: &[&str] = &["rbconfig"];

impl Loader {
    /// Records how one `require`d library was satisfied, deduped by
    /// name (first-wins): a bundled gem's user-facing `.rb` is recorded before
    /// its internal `.so` require, so the entry point wins.
    fn record_gem(&mut self, record: crate::gem_report::GemRecord) {
        if self.gem_records.iter().any(|r| r.name == record.name) {
            return;
        }
        self.gem_records.push(record);
    }

    /// Lowers one file's top-level statement list, splicing require/load
    /// targets in place. `file_idx` is `Some` for a spliced (non-main)
    /// file: its index into `Hir::loaded_files`, which is also its
    /// local-rename namespace -- the main file's locals are never renamed
    /// (the merged `Program` scope IS the main file's scope).
    fn lower_file_statements(
        &mut self,
        hir: &mut Hir,
        result: &ruby_prism::ParseResult,
        body: ruby_prism::NodeList<'_>,
        dir: Option<&Path>,
        file_idx: Option<usize>,
        current_box: u32,
    ) -> PResult<Vec<NodeId>> {
        // `combined` is the spliced statement list in document order;
        // `own` is only THIS file's statements -- the rename pass walks
        // `own` alone, so already-renamed child splices (separate roots,
        // unreachable from this file's own subtrees) are never touched
        // twice.
        let frame = BindingsFrame::push();
        let mut combined = Vec::new();
        let mut own = Vec::new();

        // Collect every require/require_relative in the file (any nesting) once,
        // for two uses below: (1) a resolvability pre-scan HERE, and (2) the
        // eager splice of non-top-level requires AFTER the statement loop.
        let mut requires = RequireCollector::default();
        {
            use ruby_prism::Visit as _;
            for n in body.iter() {
                requires.visit(&n);
            }
        }
        // Resolvability pre-scan (MUST precede the statement loop, which folds
        // require calls in `lower_call_general`): a plain `require "feature"`
        // zeo can't resolve to a file, builtin, or shim is recorded so its CALL
        // lowers to a runtime `Kernel#require` (raising `LoadError`) instead of
        // a loaded-no-op `true`. That makes a genuinely-missing feature crash at
        // its require site and the `begin; require "x"; rescue LoadError` idiom
        // catch at runtime -- CRuby's semantics. (A missing `require_relative`
        // stays a compile error, resolved in the loop / hoist below.)
        for call in &requires.calls {
            if call.name().as_slice() != b"require" {
                continue;
            }
            // Extract the literal feature EXACTLY as `lower_require_statement`
            // and `lower_call_general` do (lower the arg, read its folded string
            // literal), so all three agree on which requires are literal. The
            // throwaway arg node is harmless append-only arena bookkeeping.
            let arg_list: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            let [arg] = arg_list.as_slice() else { continue };
            let arg_id = lower_node(result, hir, arg)?;
            let Some(feature) = crate::lower::eval_splice::literal_string_text(hir, arg_id) else {
                continue;
            };
            if !self.require_resolvable(&feature) {
                hir.unresolvable_requires.insert(feature);
            }
        }

        for n in body.iter() {
            if let Some(call) = n.as_call_node() {
                let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
                if call.receiver().is_none()
                    && matches!(name.as_str(), "require" | "require_relative" | "load")
                {
                    if let Some(spliced) = self.lower_require_statement(
                        hir,
                        result,
                        &call,
                        &name,
                        dir,
                        file_idx,
                        current_box,
                    )? {
                        combined.extend(spliced);
                        continue;
                    }
                    // A dynamic `load`/`require` (computed target): not spliced.
                    // Fall through to the general lowering so it becomes a
                    // runtime `Kernel#{require,load}` call (raises LoadError).
                }
                // `box.require "f"` / `box.require_relative` / `box.load`
                // / `box.eval "src"` at top-level statement position
                //: resolve like the receiver-less forms, splice
                // with the BOX's id, wrap in one BoxScope. Statement-
                // position `box.eval` may define classes (real Ruby's
                // Box#eval compiles a top-level iseq); expression-position
                // eval is `parse::mod`'s recognizer, defs rejected there.
                if let Some(recv) = call.receiver() {
                    if let Some(lv) = recv.as_local_variable_read_node() {
                        let lname = String::from_utf8_lossy(lv.name().as_slice()).into_owned();
                        if let Some(bx) = current_box_binding(&lname) {
                            if matches!(name.as_str(), "require" | "require_relative" | "load") {
                                let Some(spliced) = self.lower_require_statement(
                                    hir, result, &call, &name, dir, file_idx, bx,
                                )? else {
                                    return Err(format!(
                                        "`box.{name}` needs a compile-time-resolvable literal target (zeo limitation) -- a box's require graph is spliced at compile time"
                                    ).into());
                                };
                                combined.push(hir.push(HirNode::BoxScope {
                                    box_id: bx,
                                    body: spliced,
                                }));
                                continue;
                            }
                            if name == "eval" {
                                let node = crate::lower::eval_splice::lower_box_eval(
                                    hir, result, &call, bx, true,
                                )?;
                                combined.push(node);
                                continue;
                            }
                        }
                    }
                }
            }
            // `box = Ruby::Box.new` -- allocates a fresh compile-time box,
            // records the file-local binding, AND binds the local to the
            // handle VALUE (the box's top-level surrogate as a Class), so
            // `p box` works.
            if let Some(lw) = n.as_local_variable_write_node() {
                if crate::lower::eval_splice::is_ruby_box_new(&lw.value()) {
                    let lname = String::from_utf8_lossy(lw.name().as_slice()).into_owned();
                    hir.boxes += 1;
                    let box_id = hir.boxes;
                    frame.bind(lname.clone(), box_id);
                    let handle = hir.push(HirNode::BoxHandle(box_id));
                    let id = hir.push(HirNode::LocalWrite(lname, handle));
                    combined.push(id);
                    own.push(id);
                    continue;
                }
            }
            // A top-level `include Mod` (constant arguments) mixes Mod into
            // `Object` -- the top-level self's class. Lowered to `HirNode::
            // Include` so `analyze` registers it on `Object` exactly as a
            // `class Object; include Mod; end` reopen would, rather than
            // emitting a (nonexistent) runtime `include` call on `main`.
            if let Some(call) = n.as_call_node() {
                if call.receiver().is_none() && call.name().as_slice() == b"include" {
                    let arg_list: Vec<_> = call
                        .arguments()
                        .map(|a| a.arguments().iter().collect())
                        .unwrap_or_default();
                    let all_constants = !arg_list.is_empty()
                        && arg_list.iter().all(|a| {
                            a.as_constant_read_node().is_some()
                                || a.as_constant_path_node().is_some()
                        });
                    if all_constants {
                        // Stamped with the `include` line, as the class-body
                        // form is: an unresolvable target defers to a runtime
                        // `NameError` raised from this node (see
                        // `analyze::defer_unresolved_directive`), and these
                        // nodes are built outside `lower_node`'s span frame.
                        hir.push_span(crate::lower::span_of(hir, &n));
                        for a in &arg_list {
                            let module = crate::lower::consts::constant_path_name(a)?;
                            let id = hir.push(crate::hir::HirNode::Include(module));
                            combined.push(id);
                            own.push(id);
                        }
                        hir.pop_span();
                        continue;
                    }
                }
            }
            // Top-level `undef m` / `alias n m`, the same Object reopen the
            // `include` above is: top-level `def` is a private method ON
            // Object, so the keyword that undefines or aliases one targets
            // Object too. Both are class-body keywords in `lower_class_body`,
            // and handling them HERE rather than in `lower_node` keeps them
            // statement-only -- in expression position they have no value to
            // produce and would silently do nothing.
            if let Some(undef) = n.as_undef_node() {
                let names = undef
                    .names()
                    .iter()
                    .map(|name| crate::lower::defs::alias_target_name(&name))
                    .collect::<PResult<Vec<_>>>()?;
                let id = hir.push(crate::hir::HirNode::Undef(names));
                combined.push(id);
                own.push(id);
                continue;
            }
            if let Some(alias) = n.as_alias_method_node() {
                // Deferred rather than resolved to a second `DefMethod` here:
                // at top level the target may be an inherited Kernel method,
                // which only `mro::resolve_aliases` can see.
                let id = hir.push(crate::hir::HirNode::AliasMethod {
                    new_name: crate::lower::defs::alias_target_name(&alias.new_name())?,
                    old_name: crate::lower::defs::alias_target_name(&alias.old_name())?,
                    is_class_method: false,
                });
                combined.push(id);
                own.push(id);
                continue;
            }
            let id = lower_node(result, hir, &n)?;
            combined.push(id);
            own.push(id);
        }
        if let Some(idx) = file_idx {
            rename::isolate_file_locals(hir, &own, idx);
        }
        drop(frame);

        // Eager `autoload` (any structural nesting): every `autoload :C, path`
        // names a file that PROVIDES `C`; splice each so `C` is defined, like a
        // `require`. The autoload CALL itself lowers to a no-op (see
        // `parse::autoload_feature`). This is the compile-time stand-in for
        // CRuby's lazy first-access trigger -- documented divergences: the
        // file loads relative to THIS file's position (not at first constant
        // access), and even if `C` is never referenced. `SourceFileFrame` is
        // still this file (the splice guard outlives this call), so the
        // `File.expand_path("...", __dir__)` form resolves against the right
        // directory. Deduped through the shared `required` table, so two
        // constants autoloaded from one file splice it once.
        let mut autoloads = Vec::new();
        for n in body.iter() {
            collect_autoloads(&n, &mut autoloads);
        }
        for call in &autoloads {
            let feature = crate::lower::autoload_feature(call)?;
            let spliced =
                self.splice_feature(hir, &feature, "require", dir, file_idx, current_box)?;
            combined.extend(spliced);
        }

        // Non-top-level `require`/`require_relative` (inside a method,
        // conditional, `begin`, block -- anywhere the file-level loop above
        // does NOT resolve): exactly like `autoload`, splice each RESOLVABLE
        // literal target eagerly here (the CALL itself folds to a bool no-op in
        // `lower_call_general`). An unresolvable plain `require` isn't hoisted
        // (`lower_require_statement` returns `None` for it, from the pre-scan
        // set); its CALL lowers to a runtime `Kernel#require` instead. Deduped
        // through the shared `required` table, so a target already loaded at top
        // level (or shared across sites) splices exactly once. Over-
        // approximation: a require in a never-taken branch still loads -- benign,
        // since every extension is statically linked anyway.
        let mut hoisted = Vec::new();
        for call in &requires.calls {
            let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
            if let Some(spliced) =
                self.lower_require_statement(hir, result, call, &name, dir, file_idx, current_box)?
            {
                hoisted.extend(spliced);
            }
        }
        // The hoisted targets run BEFORE this file's own statements, so a method
        // that `require`s-and-uses one of them works even when it is called
        // during this same file's top-level load (top-level requires the file
        // itself makes are already inside `combined`, spliced by the loop
        // above). Narrow over-approximation: a hoisted target that depends on a
        // SAME-FILE top-level require would see it unloaded -- unheard of in
        // practice (nested requires name self-contained lazy deps).
        hoisted.append(&mut combined);
        Ok(hoisted)
    }

    /// One recognized require/require_relative/load statement: validate the
    /// shape, resolve the target, splice (or skip, for a deduped require).
    #[allow(clippy::too_many_arguments)] // one context param per resolution
    // dimension (dir/file/box) -- bundling them into a struct would obscure
    // the loading-box propagation this fn exists to thread.
    fn lower_require_statement(
        &mut self,
        hir: &mut Hir,
        result: &ruby_prism::ParseResult,
        call: &ruby_prism::CallNode<'_>,
        name: &str,
        dir: Option<&Path>,
        file_idx: Option<usize>,
        current_box: u32,
    ) -> PResult<Option<Vec<NodeId>>> {
        if call.block().is_some() {
            return Err(format!("`{name}` doesn't take a block").into());
        }
        let arg_list: Vec<_> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        // A non-resolvable shape -- a computed argument, or `load`'s two-arg
        // `wrap` form -- has no compile-time meaning. `Ok(None)` signals the
        // caller to leave the call in place so the ordinary lowering dispatches
        // it to the runtime `Kernel#{require,load}` (which raises `LoadError`
        // when actually run); a guarded dynamic load never fails the whole
        // compile. A LITERAL one-arg `require`/`require_relative`/`load`
        // still resolves and splices at compile time below.
        if arg_list.len() != 1 {
            return Ok(None);
        }
        // Lower the argument through the ordinary path first (same trick as
        // `eval`'s recognizer): prism's adjacent-literal folding is picked
        // up for free, and the one throwaway node on the accepted path is
        // harmless append-only arena bookkeeping.
        let arg_id = lower_node(result, hir, &arg_list[0])?;
        let Some(feature) = crate::lower::eval_splice::literal_string_text(hir, arg_id) else {
            return Ok(None);
        };
        // A plain `require` the resolvability pre-scan marked unresolvable is not
        // spliced: `Ok(None)` leaves the CALL in place, and `lower_call_general`
        // lowers it to a runtime `Kernel#require` (raising `LoadError`). A
        // missing `require_relative` is NOT in that set and still fails loudly
        // in `splice_feature` below (a missing project file is a real error).
        if name == "require" && hir.unresolvable_requires.contains(&feature) {
            return Ok(None);
        }
        Ok(Some(self.splice_feature(
            hir,
            &feature,
            name,
            dir,
            file_idx,
            current_box,
        )?))
    }

    /// Whether a plain `require "feature"` has a compile-time VERDICT -- either
    /// it resolves to something zeo can splice or activate (a file on a load-path
    /// root, a synthesized shim, a statically linked extension), OR it names a
    /// gem the external store located but zeo must REJECT (a native extension or
    /// wrong-platform gem, `store_exclusions`), which stays a loud compile error
    /// with its specific reason. Only a require with NEITHER verdict has nothing
    /// to say at compile time and defers to a runtime `Kernel#require` (raising
    /// `LoadError`). Mirrors `splice_feature`'s `require` resolution.
    fn require_resolvable(&self, feature: &str) -> bool {
        match self.resolve_require(feature) {
            // Resolves to a file on a load-path/gem root.
            Ok(Some(_)) => true,
            // A definite resolution ERROR (e.g. a feature found in multiple gems
            // -- `Gem::LoadError "found in multiple gems"`): a verdict, so it
            // stays a loud compile error via `splice_feature`, never a defer.
            Err(_) => true,
            // Not on disk: a synthesized shim, a statically linked extension, or
            // a store-located-but-rejected gem (native ext / wrong platform) all
            // count; anything else has no compile-time verdict and defers.
            Ok(None) => {
                synthetic_shim_source(feature).is_some() || {
                    let bare = feature
                        .strip_suffix(".so")
                        .or_else(|| feature.strip_suffix(".bundle"))
                        .or_else(|| feature.strip_suffix(".o"))
                        .unwrap_or(feature);
                    is_builtin_feature(bare) || self.store_exclusions.contains_key(bare)
                }
            }
        }
    }

    /// The resolve-and-splice core shared by `require`/`require_relative`/
    /// `load` and eager top-level `autoload`: given an already-extracted
    /// literal `feature` and the resolution flavor `name`, short-circuit a
    /// built-in feature, resolve the file, dedup, and splice it into the
    /// arena.
    fn splice_feature(
        &mut self,
        hir: &mut Hir,
        feature: &str,
        name: &str,
        dir: Option<&Path>,
        file_idx: Option<usize>,
        current_box: u32,
    ) -> PResult<Vec<NodeId>> {
        // Conformance-harness support (mspec_lite): ruby/spec files
        // `require_relative '../spec_helper'`, which pulls in the real `mspec`
        // framework zeo can't compile. Under `ZEO_MSPEC_STUBS` (set only
        // by the rubyspec conformance suite), such a require is a no-op -- the
        // driver's own `require_relative <mspec_lite>` supplies the DSL. Off by
        // default, so ordinary compiles are unaffected.
        if is_mspec_stub_feature(feature) {
            return Ok(Vec::new());
        }

        // Gem attribution: a `require` resolved out of a
        // package's roots belongs to that package; `require_relative`/
        // `load` INHERIT the requiring file's package (a package's internal
        // files are part of the package, however they're reached).
        let inherited = file_idx.and_then(|i| hir.loaded_files[i].package.clone());
        let (path, package) = match name {
            // CRuby's `search_required` ORDER, and it is the inverse of the
            // obvious one: every load-path root is tried for `<feature>.rb`
            // BEFORE the statically-linked-extension table is consulted
            // (`load.c:1159-1181`; `rb_find_file_ext` loops extension-outer,
            // path-inner, `file.c:7173`). This is what lets a gem have a Ruby
            // HALF sitting on top of a native half: `require "strscan"` finds
            // `gems/strscan/lib/strscan.rb`, and that file pulls its native
            // half in with `require "strscan.so"` -- CRuby's loader idiom,
            // exactly as `ext/digest/lib/digest.rb` does.
            //
            // Short-circuiting builtin features first (the previous order)
            // made a Ruby half unreachable: the require returned before any
            // filesystem search could find it.
            "require" => match self.resolve_require(feature)? {
                Some(found) => found,
                // Not on disk. A synthesized stdlib shim built into zeo (e.g.
                // `rbconfig`, which real Ruby generates at build time), then a
                // statically linked extension?
                None => {
                    if let Some(spliced) =
                        self.splice_synthetic_shim(hir, feature, current_box)?
                    {
                        return Ok(spliced);
                    }
                    return self.activate_static_ext(hir, feature);
                }
            },
            "require_relative" => (resolve_require_relative(feature, dir)?, inherited),
            _ => (self.resolve_load(feature, dir)?, inherited),
        };
        // Disclosure: a plain `require` of a real library (not the
        // internal `.so` loader idiom, not `require_relative`/`load` of an
        // owned file) records HOW it was satisfied -- out of a bundled gem's
        // roots (`package: Some`) or off a `-I` stdlib root (`package: None`).
        // Deduped by name in `record_gem`, so a re-require is a no-op.
        if name == "require" && !is_native_feature(feature) {
            let by = match &package {
                Some(_) => crate::gem_report::SatisfiedBy::BundledGem {
                    path: display_path(&path),
                },
                None => crate::gem_report::SatisfiedBy::StdlibRoot {
                    path: display_path(&path),
                },
            };
            self.record_gem(crate::gem_report::GemRecord {
                name: feature.to_string(),
                by,
            });
        }

        let canonical = path
            .canonicalize()
            .map_err(|e| format!("resolving {}: {e}", path.display()))?;

        // A rubygems-/bundler-vendored copy of a library zeo already provides
        // natively is redirected to zeo's own shim rather than lowered from the
        // vendored source. `vendor/securerandom` is a verbatim copy of the
        // securerandom gem whose load-time entropy probe (a `class << self`
        // begin/rescue installing one of two `gen_random` aliases) zeo can't
        // express -- and zeo ships securerandom natively anyway. See
        // `shims/gem_securerandom.rb`.
        if let Some(shim) = vendored_shim_feature(&canonical) {
            if let Some(spliced) = self.splice_synthetic_shim(hir, shim, current_box)? {
                return Ok(spliced);
            }
        }

        if name != "load" {
            // Insert BEFORE lowering (CRuby's loading-table rule): a
            // circular require splices nothing and continues, in exactly
            // Ruby's execution order. Keyed per BOX: the same
            // file `box.require`d into two boxes re-executes in each --
            // real Ruby's per-box loaded-features tables.
            if !self.required.insert((current_box, canonical.clone())) {
                return Ok(Vec::new());
            }
        }
        self.splice_file(hir, &canonical, file_idx, package, current_box)
    }

    /// A `require` that found nothing on disk MAY be one of the stdlib features
    /// zeo synthesizes in-tree (currently just `rbconfig`, which real Ruby
    /// generates at build time). Splice the embedded shim source, deduped per
    /// box like any other require. Returns `None` when `feature` names no shim,
    /// so the caller falls through to the static-ext table.
    /// Splices `PRELOADED_FEATURES` ahead of the main file. A later explicit
    /// `require` of one then answers false, exactly as in CRuby.
    fn preload_ambient_features(&mut self, hir: &mut Hir) -> PResult<Vec<NodeId>> {
        let mut out = Vec::new();
        for feature in PRELOADED_FEATURES {
            if let Some(spliced) = self.splice_synthetic_shim(hir, feature, 0)? {
                out.extend(spliced);
            }
        }
        Ok(out)
    }

    fn splice_synthetic_shim(
        &mut self,
        hir: &mut Hir,
        feature: &str,
        box_id: u32,
    ) -> PResult<Option<Vec<NodeId>>> {
        let Some(source) = synthetic_shim_source(feature) else {
            return Ok(None);
        };
        // A virtual path (no file on disk) standing in for `__FILE__`/provenance.
        let canonical = PathBuf::from(format!("<zeo-shim>/{feature}.rb"));
        if !self.required.insert((box_id, canonical.clone())) {
            return Ok(Some(Vec::new()));
        }
        self.record_gem(crate::gem_report::GemRecord {
            name: feature.to_string(),
            by: crate::gem_report::SatisfiedBy::BuiltinExt {
                feature: feature.to_string(),
            },
        });
        Ok(Some(self.splice_source(
            hir,
            &canonical,
            source.to_string(),
            None,
            None,
            box_id,
        )?))
    }

    /// The static-ext fallthrough: a `require` that found nothing on disk.
    ///
    /// This is zeo's `vm->static_ext_inits` (`load.c:1510`). zeo is a
    /// Ruby built with `--with-static-linked-ext` -- every extension it
    /// supports is linked into the runtime, so there is no `.so` to dlopen and
    /// "activating the feature" is the whole of what loading one means. The
    /// ABI `feature` table is zeo's `ext/Setup`.
    ///
    /// A `.so`/`.bundle` spelling resolves the same way after dropping the
    /// suffix, because CRuby registers static exts under `"<feature>.so"` and
    /// rewrites an explicit suffix to `DLEXT` before looking them up
    /// (`load.c:1129`, `template/extinit.c.tmpl`). That is what makes the
    /// loader idiom -- a Ruby half doing `require "strscan.so"` -- work.
    fn activate_static_ext(&mut self, hir: &mut Hir, feature: &str) -> PResult<Vec<NodeId>> {
        let bare = feature
            .strip_suffix(".so")
            .or_else(|| feature.strip_suffix(".bundle"))
            .or_else(|| feature.strip_suffix(".o"))
            .unwrap_or(feature);
        if !is_builtin_feature(bare) {
            // A gem the external store locked but zeo can't provide gets its
            // precise reason (which native layout, why), not the generic miss.
            if let Some(reason) = self.store_exclusions.get(bare) {
                return Err(format!("cannot load such file -- {bare}: {reason}").into());
            }
            return Err(cannot_load(feature).into());
        }
        // Disclosure: a DIRECT `require` of a statically-linked ext
        // (no Ruby half on disk, e.g. `require "base64"`). The `.so` loader
        // idiom -- a bundled gem's Ruby half pulling its own native half in --
        // is NOT recorded here: that gem's entry point was already recorded
        // when its `.rb` spliced, and recording the `.so` would double-count.
        if !is_native_feature(feature) {
            let canonical = canonical_ext_feature(bare).to_string();
            self.record_gem(crate::gem_report::GemRecord {
                name: canonical.clone(),
                by: crate::gem_report::SatisfiedBy::BuiltinExt { feature: canonical },
            });
        }
        // An in-tree `ext/` feature's `require` ACTIVATES its gated builtin
        // (`require "base64"` -> `Base64` resolves). Always-on core no-ops
        // (`set`/`tmpdir`) name no gated class, so recording them gates
        // nothing -- but it is recorded all the same, because the same set
        // doubles as the loaded-features table the non-top-level `require`
        // (`parse::mod`) reads to decide whether it evaluates to `true` or
        // `false`. Alias spellings collapse first, so `require "yaml"`
        // activates the same `psych` feature `require "psych"` does.
        hir.activate_feature(canonical_ext_feature(bare));
        Ok(Vec::new())
    }

    /// Parses and lowers one resolved file into the arena, recording its
    /// provenance -- one `LoadedFile` per SPLICE INSTANCE (see
    /// `Hir::loaded_files`' docs for why instance, not canonical file, is
    /// the unit).
    fn splice_file(
        &mut self,
        hir: &mut Hir,
        canonical: &Path,
        required_from: Option<usize>,
        package: Option<String>,
        box_id: u32,
    ) -> PResult<Vec<NodeId>> {
        if self.splicing.iter().any(|p| p == canonical) {
            return Err(format!(
                "`load` cycle detected: {} is already being loaded (real Ruby would recurse forever at runtime; this compiler rejects it at compile time instead)",
                canonical.display()
            ).into());
        }
        let source = std::fs::read_to_string(canonical)
            .map_err(|e| format!("reading {}: {e}", canonical.display()))?;
        self.splice_source(hir, canonical, source, required_from, package, box_id)
    }

    /// The lowering half of [`Self::splice_file`], over already-read `source`.
    /// Split out so a SYNTHESIZED feature (an embedded shim like `rbconfig`,
    /// which has no file on disk) can splice through the same path, under a
    /// virtual `canonical` name that stands in for `__FILE__`/provenance.
    fn splice_source(
        &mut self,
        hir: &mut Hir,
        canonical: &Path,
        source: String,
        required_from: Option<usize>,
        package: Option<String>,
        box_id: u32,
    ) -> PResult<Vec<NodeId>> {
        let idx = hir.loaded_files.len();
        hir.loaded_files.push(LoadedFile {
            canonical: canonical.to_path_buf(),
            required_from,
            package,
            box_id,
        });
        let result = ruby_prism::parse(source.as_bytes());
        if let Some(err) = result.errors().next() {
            return Err(LowerError::syntax(format!(
                "{}: parse error: {}",
                canonical.display(),
                err.message()
            )));
        }
        let program = result
            .node()
            .as_program_node()
            .ok_or_else(|| format!("{}: expected a top-level ProgramNode", canonical.display()))?;
        // A VENDORED gem's warnings are not the user's to act on, and zeo's
        // vendored copy may not even be the one CRuby would have parsed --
        // so only files outside a package report.
        if hir.loaded_files[idx].package.is_none() {
            collect_parse_warnings(hir, &result, &canonical.display().to_string(), &source);
        }
        self.splicing.push(canonical.to_path_buf());
        // A required file's `__FILE__` is ITSELF, not whoever required it.
        // Popped by the guard's Drop, so the parent's own statements after
        // the splice see their own path again.
        let _file = SourceFileFrame::push(Some(canonical));
        let file_id = hir.add_file(canonical.display().to_string(), source.clone());
        let prev_file = hir.lowering_file.replace(file_id);
        let statements = self
            .lower_file_statements(
                hir,
                &result,
                program.statements().body(),
                canonical.parent(),
                Some(idx),
                // Loading-box propagation (CRuby's frame-flag walk, as a
                // compile-time parameter): plain `require`s inside a
                // box-required subtree stay in that box.
                box_id,
            )
            .map_err(|e| {
                if e.message().starts_with(&canonical.display().to_string()) {
                    e // already prefixed by a nested splice
                } else {
                    // Prefix the message; the KIND and the span (pointing
                    // into the spliced file) ride along untouched.
                    LowerError {
                        message: format!("{}: {e}", canonical.display()),
                        ..e
                    }
                }
            });
        hir.lowering_file = prev_file;
        let statements = statements?;
        self.splicing.pop();
        Ok(statements)
    }

    /// `require "feature"` -> the first `<root>/<feature>.rb` that exists:
    /// `-I` roots in order first (they win over packages entirely, CRuby's
    /// own rule), then the packages -- where a feature provided by MORE
    /// THAN ONE package is a loud error (mirroring RubyGems' own
    /// `Gem::LoadError "found in multiple gems"`, and stricter/safer than
    /// silent `$LOAD_PATH`-order shadowing, which Bundler-locked apps never
    /// rely on). Returns the resolved path plus the owning package's name
    /// (provenance -- see `LoadedFile::package`). Absolute paths bypass all
    /// roots, exactly like `rb_find_file`'s absolute branch.
    ///
    /// `Ok(None)` means NOT FOUND, which is not yet an error: the caller falls
    /// through to the static-ext table, mirroring CRuby's `search_required`
    /// (`load.c:1161`), where the statically-linked-extension lookup runs only
    /// after `rb_find_file_ext` has failed on disk. `Err` is reserved for a
    /// genuine problem (an ambiguous feature, an unsupported path shape).
    fn resolve_require(&self, feature: &str) -> PResult<Option<(PathBuf, Option<String>)>> {
        if feature.starts_with("./") || feature.starts_with("../") || feature.starts_with('~') {
            return Err(format!(
                "`require \"{feature}\"`: `./`/`../`/`~` paths resolve against the runtime working directory in real Ruby, which doesn't exist at compile time -- use `require_relative` instead"
            ).into());
        }
        // A `.so`/`.bundle` feature never has a file on disk here -- it names
        // a STATICALLY LINKED extension, so it belongs to the caller's
        // static-ext fallthrough, not to the filesystem search.
        if is_native_feature(feature) {
            return Ok(None);
        }
        let fname = with_rb_ext(feature);
        if Path::new(&fname).is_absolute() {
            let p = PathBuf::from(&fname);
            if p.is_file() {
                return Ok(Some((p, None)));
            }
            return Ok(None);
        }
        for root in &self.roots {
            let cand = root.join(&fname);
            if cand.is_file() {
                return Ok(Some((cand, None)));
            }
        }
        let mut hits: Vec<(PathBuf, &str)> = Vec::new();
        for pkg in &self.packages {
            // At most one hit per package: a package's OWN roots are
            // ordered by its manifest (first wins within the package).
            if let Some(cand) = pkg
                .roots
                .iter()
                .map(|r| r.join(&fname))
                .find(|c| c.is_file())
            {
                hits.push((cand, &pkg.name));
            }
        }
        match hits.len() {
            0 => Ok(None),
            1 => {
                let (path, pkg) = hits.remove(0);
                Ok(Some((path, Some(pkg.to_string()))))
            }
            _ => {
                let names: Vec<&str> = hits.iter().map(|(_, n)| *n).collect();
                Err(format!(
                    "`require \"{feature}\"` is ambiguous: found in multiple gems ({})",
                    names.join(", ")
                )
                .into())
            }
        }
    }

    /// `load "path"`: no `.rb` appending, ever (CRuby's `rb_find_file` has
    /// no ext parameter). Roots first, then the requiring file's directory
    /// -- the latter is a documented divergence from CRuby's cwd fallback
    /// (cwd is meaningless at compile time; the requiring file's own
    /// directory is the deterministic analogue).
    fn resolve_load(&self, arg: &str, dir: Option<&Path>) -> PResult<PathBuf> {
        let p = Path::new(arg);
        if p.is_absolute() {
            if p.is_file() {
                return Ok(p.to_path_buf());
            }
            return Err(cannot_load(arg).into());
        }
        if !(arg.starts_with("./") || arg.starts_with("../")) {
            for root in &self.roots {
                let cand = root.join(arg);
                if cand.is_file() {
                    return Ok(cand);
                }
            }
        }
        if let Some(dir) = dir {
            let cand = dir.join(arg);
            if cand.is_file() {
                return Ok(cand);
            }
        }
        Err(cannot_load(arg).into())
    }
}

/// `require_relative "feature"` -> `<requiring file's dir>/<feature>.rb`.
/// CRuby raises the missing-file LoadError with the ABSOLUTIZED path (not
/// the feature as written) for require_relative specifically -- matched
/// here.
fn resolve_require_relative(feature: &str, dir: Option<&Path>) -> PResult<PathBuf> {
    let Some(dir) = dir else {
        // CRuby's exact error when the requiring context has no file
        // (eval/irb): here, a source compiled without an input path.
        return Err("cannot infer basepath -- `require_relative` needs the requiring file's directory (compile from a file path)".to_string().into());
    };
    if is_native_feature(feature) {
        return Err(format!(
            "`require_relative \"{feature}\"`: native (.so/.bundle) features aren't supported (zeo limitation)"
        ).into());
    }
    // ORDER MATTERS, and CRuby's is the inverse of the obvious one.
    // `rb_require_relative_entrypoint` (load.c:1054) absolutizes against the
    // requiring file's directory as its very FIRST step, and
    // `rb_find_file_ext` (file.c:7173) expands the path BEFORE its
    // `rb_str_cat2(fname, ext[i])` loop. So `require_relative ".."` from
    // `views/articles/index.rb` resolves `views/articles/..` -> `views` and
    // only then appends `.rb`, giving `views.rb`. Appending first would glue
    // `.rb` onto a literal `..` and search for the nonexistent `...rb`.
    let base = lexically_normalize(&dir.join(feature));
    let cand = if feature.ends_with(".rb") {
        base.clone()
    } else {
        PathBuf::from(format!("{}.rb", base.display()))
    };
    if cand.is_file() {
        return Ok(cand);
    }
    // `require_relative` reports the ABSOLUTIZED, extension-less path --
    // `require_relative "nope"` from /tmp says `cannot load such file --
    // /tmp/nope`. (Plain `require` is the one that echoes the feature as
    // written; the two differ because require_relative has already
    // absolutized by the time the search fails.)
    Err(cannot_load(&base.display().to_string()).into())
}

/// Collapses `.` and `..` components without touching the filesystem.
///
/// CRuby's expansion is likewise purely lexical (`file.c:4432`): `..` rewinds
/// the write pointer in the buffer, with no `lstat` and no symlink
/// resolution. That difference is observable -- `a/symlink_to_b/..` is `a`
/// here and to CRuby, but `b`'s real parent to `canonicalize` -- so this must
/// NOT be `Path::canonicalize`, which would also fail outright on the
/// not-yet-extended path.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            // Only a real directory name can be rewound over. A leading `..`
            // (or one following another) has nothing above it to cancel, so
            // it is kept and the path stays relative to wherever it started.
            Component::ParentDir
                if out
                    .components()
                    .next_back()
                    .is_some_and(|c| matches!(c, Component::Normal(_))) =>
            {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Discovers packages under each dir, in dir order: every subdirectory
/// containing a `.gemspec` is a gem (subdirectories without
/// one are silently ignored -- not gems). Within one dir, discovery is
/// name-sorted (deterministic); across dirs, the FIRST occurrence of a
/// package NAME wins entirely (a project-local package shadows a
/// same-named compiler-bundled one -- the "one version per name, nearest
/// wins" rule, the same shape as Bundler's lockfile picking exactly one
/// version). A missing/unreadable packages dir contributes nothing (the
/// CLI passes default candidate locations that often don't exist).
/// The embedded source for a stdlib feature zeo synthesizes instead of loading
/// from disk. Real Ruby generates these at build time (`rbconfig`); zeo ships a
/// static stand-in describing the target it emulates. `None` for any other
/// feature.
fn synthetic_shim_source(feature: &str) -> Option<&'static str> {
    match feature {
        "rbconfig" => Some(include_str!("shims/rbconfig.rb")),
        "securerandom" => Some(include_str!("shims/securerandom.rb")),
        "gem-securerandom" => Some(include_str!("shims/gem_securerandom.rb")),
        // CRuby's C `erb/escape` extension -- defined as a pure-Ruby shim over
        // the native `CGI.escapeHTML` (see `shims/erb_escape.rb`).
        "erb/escape" => Some(include_str!("shims/erb_escape.rb")),
        _ => None,
    }
}

/// A rubygems-/bundler-vendored file that duplicates a library zeo already
/// provides natively, mapped to the synthetic shim that stands in for it (see
/// `synthetic_shim_source`). Currently just the vendored `securerandom` copy,
/// whose load-time `class << self` entropy probe zeo can't lower; matched by
/// path suffix so both the rubygems and bundler copies (identical files under
/// different `vendor/` roots) redirect to the same native-backed shim.
fn vendored_shim_feature(canonical: &Path) -> Option<&'static str> {
    let path = canonical.to_string_lossy().replace('\\', "/");
    path.ends_with("vendor/securerandom/lib/securerandom.rb")
        .then_some("gem-securerandom")
}

/// The `gems/` directory shipped with the compiler, if it exists.
///
/// Baked in via `CARGO_MANIFEST_DIR` -- honest for a dev-tree compiler (both
/// `cargo run` and the test harness live in the repo); an installed
/// distribution would locate it relative to the executable instead.
pub(super) fn bundled_gems_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("gems");
    dir.is_dir().then_some(dir)
}

fn discover_packages(package_dirs: &[PathBuf]) -> PResult<Vec<Gem>> {
    let mut packages: Vec<Gem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in package_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut pkg_dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && gemspec_path(p).is_some())
            .collect();
        pkg_dirs.sort();
        for pkg_dir in pkg_dirs {
            let pkg = parse_manifest(&pkg_dir)?;
            if seen.insert(pkg.name.clone()) {
                packages.push(pkg);
            }
        }
    }
    Ok(packages)
}

/// Finds and parses a gem directory's `.gemspec`.
///
/// The manifest is a real gemspec, not an invented format -- see
/// `parse::gemspec` for why a static parse is safe (RubyGems' serialized form
/// has a closed grammar, enforced by a corpus test over the installed store).
/// That is what lets one code path read a bundled zeo library, a vendored
/// gem, and a gem out of a real installed store.
///
/// The gemspec's `name` must match its directory, mirroring RubyGems' own
/// `<name>-<version>/` convention: a mismatch means a `require` would resolve
/// out of a directory that doesn't name the gem it provides.
///
/// Every declared `require_paths` entry must exist -- a gem whose `lib/` is
/// missing is a loud configuration error, not a silently empty search root.
fn parse_manifest(pkg_dir: &Path) -> PResult<Gem> {
    let manifest_path =
        gemspec_path(pkg_dir).ok_or_else(|| format!("{}: no `.gemspec`", pkg_dir.display()))?;
    let spec = super::gemspec::parse_file(&manifest_path)?;
    let dir_name = pkg_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if spec.name != dir_name {
        return Err(format!(
            "{}: gem name \"{}\" doesn't match its directory name \"{dir_name}\"",
            manifest_path.display(),
            spec.name
        )
        .into());
    }
    let roots = spec
        .require_paths
        .iter()
        .map(|rp| {
            let root = pkg_dir.join(rp);
            if root.is_dir() {
                Ok(root)
            } else {
                Err(format!(
                    "{}: require_paths entry \"{rp}\" doesn't exist under {}",
                    manifest_path.display(),
                    pkg_dir.display()
                )
                .into())
            }
        })
        .collect::<PResult<Vec<PathBuf>>>()?;
    Ok(Gem {
        name: spec.name,
        roots,
    })
}

/// The `.gemspec` in a gem directory, if there is exactly one candidate.
///
/// Prefers `<dir>/<dir-name>.gemspec` (the convention every real gem follows)
/// and otherwise takes any single `.gemspec` present, so a directory whose
/// gemspec is named differently still loads and fails with the clearer
/// name-mismatch error above rather than a bare "no `.gemspec`".
fn gemspec_path(pkg_dir: &Path) -> Option<PathBuf> {
    let dir_name = pkg_dir.file_name()?;
    let conventional = pkg_dir.join(dir_name).with_extension("gemspec");
    if conventional.is_file() {
        return Some(conventional);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(pkg_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "gemspec"))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// CRuby's exact missing-feature message (`load_failed` -> `rb_load_fail`).
/// A path for the disclosure record, made relative to the current directory
/// when it sits under it (so a bundled gem reads `gems/json/lib/json.rb`
/// rather than an absolute machine path), else left absolute.
fn display_path(path: &Path) -> String {
    // Canonicalize first so a bundled path baked with `../..`
    // (`CARGO_MANIFEST_DIR/../../gems/...`) collapses before the CWD strip,
    // yielding a clean `gems/json/lib/json.rb` rather than `crates/zeo/../..`.
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .and_then(|cwd| resolved.strip_prefix(&cwd).ok().map(Path::to_path_buf))
        .unwrap_or(resolved)
        .display()
        .to_string()
}

fn cannot_load(name: &str) -> String {
    // mruby's unclaimed win: when the name is a well-known gem with a NATIVE
    // half zeo has no static ext for, say so -- otherwise it reads like an
    // unsupported language feature rather than "this gem isn't linked in".
    // FFI is the future escape hatch this points at.
    if is_known_native_gem(name) {
        return format!(
            "cannot load such file -- {name}: this gem has a native (C) extension \
             zeo does not provide a built-in for. See docs/EXTENSIONS.md; the FFI \
             path is the intended escape hatch."
        );
    }
    format!("cannot load such file -- {name}")
}

/// A small allowlist of popular gems whose real implementation is a C
/// extension -- named so a failed `require` explains itself (see `cannot_load`).
/// Not exhaustive and not load-bearing: an unrecognized native gem still fails,
/// just with the plainer message.
fn is_known_native_gem(name: &str) -> bool {
    matches!(
        name,
        // `ffi` is NOT here: zeo provides it (the compile-time FFI frontend),
        // so `require "ffi"` succeeds via `is_builtin_feature`.
        "sqlite3"
            | "nokogiri"
            | "pg"
            | "mysql2"
            | "bcrypt"
            | "nio4r"
            | "puma"
            | "grpc"
            | "protobuf"
            | "oj"
            | "msgpack"
            | "eventmachine"
            | "sass"
            | "rmagick"
    )
}

/// Whether `feature` is a mspec/spec_helper require the rubyspec conformance
/// suite stubs out (see `splice_feature`). Gated on `ZEO_MSPEC_STUBS` so it
/// never affects a normal compile. Matched by basename, so `../spec_helper`,
/// `spec/spec_helper`, `mspec`, and `mspec/guards/version` all collapse.
fn is_mspec_stub_feature(feature: &str) -> bool {
    if std::env::var_os("ZEO_MSPEC_STUBS").is_none() {
        return false;
    }
    let base = feature.rsplit('/').next().unwrap_or(feature);
    base == "spec_helper" || feature == "mspec" || feature.starts_with("mspec/")
}

fn with_rb_ext(feature: &str) -> String {
    if feature.ends_with(".rb") {
        feature.to_string()
    } else {
        format!("{feature}.rb")
    }
}

fn is_native_feature(feature: &str) -> bool {
    feature.ends_with(".so") || feature.ends_with(".o") || feature.ends_with(".bundle")
}

/// Collects every receiver-less `autoload` call in a statement tree,
/// descending through the STRUCTURAL containers stdlib nests them in --
/// `module`/`class`/`class << self` bodies (e.g. `module URI; autoload
/// :Generic, "uri/generic"; end`, `module Bundler; class Settings; autoload
/// :Mirror, File.expand_path("mirror", __dir__); end; end`). An `autoload`
/// inside a method/block/conditional body isn't collected here (it's
/// genuinely runtime-dynamic, like a non-top-level `require`); it lowers to a
/// no-op without a splice, so its constant stays undefined -- a loud
/// NameError on reference, not silent, and documented.
/// Collects every receiver-less `require`/`require_relative` call ANYWHERE in a
/// file's tree -- method bodies, conditionals, `begin`, blocks included -- via
/// prism's generic traversal (`Visit`), so the loader can eager-splice targets
/// a non-top-level require names. Top-level requires are collected too but are
/// harmless: they were already spliced by the file-level loop and dedup skips
/// the second attempt. `load` and receiver-bearing (`box.require`) forms are
/// excluded -- they are not eager-splice-able.
#[derive(Default)]
struct RequireCollector<'a> {
    calls: Vec<ruby_prism::CallNode<'a>>,
}

impl<'pr> ruby_prism::Visit<'pr> for RequireCollector<'pr> {
    fn visit_branch_node_enter(&mut self, node: ruby_prism::Node<'pr>) {
        if let Some(call) = node.as_call_node() {
            if call.receiver().is_none()
                && matches!(call.name().as_slice(), b"require" | b"require_relative")
            {
                self.calls.push(call);
            }
        }
    }

    // A `require` under a statically-false guard (`require 'open3/jruby_windows'
    // if RUBY_ENGINE == 'jruby'`) names another engine's native code (its own
    // `require 'jruby'` cannot resolve/lower) and must NOT be spliced. Unlike
    // the default descent, this prunes a provably-dead branch: only the live
    // branch(es) are visited. The predicate is still visited (a `require` inside
    // the guard EXPRESSION is pathological but stays collected). The guard is
    // only decidable for the platform-detection idioms `eval_static_guard`
    // models; every runtime-conditional require descends exactly as before.
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        self.visit(&node.predicate());
        let guard = eval_static_guard(&node.predicate());
        if guard != Some(false) {
            if let Some(stmts) = node.statements() {
                self.visit(&stmts.as_node());
            }
        }
        if guard != Some(true) {
            if let Some(sub) = node.subsequent() {
                self.visit(&sub);
            }
        }
    }

    // `unless C` runs `statements` when C is FALSE and `else_clause` when TRUE
    // -- the mirror of `visit_if_node`.
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        self.visit(&node.predicate());
        let guard = eval_static_guard(&node.predicate());
        if guard != Some(true) {
            if let Some(stmts) = node.statements() {
                self.visit(&stmts.as_node());
            }
        }
        if guard != Some(false) {
            if let Some(els) = node.else_clause() {
                self.visit(&els.as_node());
            }
        }
    }
}

/// zeo's compile-time `RUBY_ENGINE`. Mirrors `zeo_rt::bootstrap`'s
/// `ENGINE = "ruby"`: zeo targets CRuby semantics, so an engine-gated require
/// (`require X if RUBY_ENGINE == 'jruby'`) is statically decidable here.
const RUBY_ENGINE: &str = "ruby";

/// Three-valued evaluation of a `require`-guard expression. `Some(true)`/
/// `Some(false)` when a platform guard is statically decidable; `None` when it
/// depends on runtime state (the branch stays live and its requires splice as
/// before). Only the idioms that gate platform-specific requires in the stdlib/
/// gem graph are modeled -- literal `true`/`false`/`nil`, `RUBY_ENGINE == "..."`
/// (and `!=`), `!x`, and `&&`/`||` over those. Everything else is `None`, so
/// this only ever PRUNES a provably-dead branch; it never drops a live require.
fn eval_static_guard(node: &ruby_prism::Node<'_>) -> Option<bool> {
    if node.as_true_node().is_some() {
        return Some(true);
    }
    if node.as_false_node().is_some() || node.as_nil_node().is_some() {
        return Some(false);
    }
    if let Some(paren) = node.as_parentheses_node() {
        let stmts = paren.body()?.as_statements_node()?;
        let body: Vec<_> = stmts.body().iter().collect();
        return match body.as_slice() {
            [only] => eval_static_guard(only),
            _ => None,
        };
    }
    if let Some(and) = node.as_and_node() {
        return match (eval_static_guard(&and.left()), eval_static_guard(&and.right())) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        };
    }
    if let Some(or) = node.as_or_node() {
        return match (eval_static_guard(&or.left()), eval_static_guard(&or.right())) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        };
    }
    if let Some(call) = node.as_call_node() {
        let name = call.name().as_slice();
        if name == b"!" && call.arguments().is_none() {
            return call.receiver().and_then(|r| eval_static_guard(&r)).map(|b| !b);
        }
        if matches!(name, b"==" | b"!=") {
            if let (Some(recv), Some(args)) = (call.receiver(), call.arguments()) {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let [only] = arg_list.as_slice() {
                    let eq =
                        engine_string_eq(&recv, only).or_else(|| engine_string_eq(only, &recv));
                    if let Some(eq) = eq {
                        return Some(if name == b"==" { eq } else { !eq });
                    }
                }
            }
        }
    }
    None
}

/// `Some(RUBY_ENGINE == lit)` when `a` reads the `RUBY_ENGINE` constant and `b`
/// is a string literal; `None` otherwise. Order-sensitive -- the caller tries
/// both operand orders so `RUBY_ENGINE == 'x'` and `'x' == RUBY_ENGINE` both
/// resolve.
fn engine_string_eq(a: &ruby_prism::Node<'_>, b: &ruby_prism::Node<'_>) -> Option<bool> {
    if a.as_constant_read_node()?.name().as_slice() != b"RUBY_ENGINE" {
        return None;
    }
    Some(b.as_string_node()?.unescaped() == RUBY_ENGINE.as_bytes())
}

fn collect_autoloads<'a>(node: &ruby_prism::Node<'a>, out: &mut Vec<ruby_prism::CallNode<'a>>) {
    if let Some(stmts) = node.as_statements_node() {
        for n in stmts.body().iter() {
            collect_autoloads(&n, out);
        }
    } else if let Some(m) = node.as_module_node() {
        if let Some(body) = m.body() {
            collect_autoloads(&body, out);
        }
    } else if let Some(c) = node.as_class_node() {
        if let Some(body) = c.body() {
            collect_autoloads(&body, out);
        }
    } else if let Some(sc) = node.as_singleton_class_node() {
        if let Some(body) = sc.body() {
            collect_autoloads(&body, out);
        }
    } else if let Some(call) = node.as_call_node() {
        if call.receiver().is_none() && call.name().as_slice() == b"autoload" {
            out.push(call);
        }
    }
}

/// Ruby's own parse-time warnings for one file, as prism reports them --
/// `key :k is duplicated and overwritten on line 24` and friends. Collected
/// here rather than re-derived: prism already knows the rules (which keys
/// count, and which line the surviving one is on), and re-implementing them
/// would be a second source of truth.
fn collect_parse_warnings(
    hir: &mut Hir,
    result: &ruby_prism::ParseResult<'_>,
    file: &str,
    source: &str,
) {
    for warning in result.warnings() {
        let message = warning.message();
        if !is_default_level(message) {
            continue;
        }
        let upto = warning.location().start_offset().min(source.len());
        let line = 1 + source.as_bytes()[..upto].iter().filter(|&&b| b == b'\n').count() as u32;
        hir.warnings.push(crate::diagnostics::CompileWarning {
            file: file.to_string(),
            line,
            message: message.to_string(),
        });
    }
}

/// prism reports its parse warnings at two levels: `default` (what plain
/// `ruby foo.rb` prints) and `verbose` (what only `ruby -w` prints). Its Rust
/// binding exposes a warning's message and location but NOT its level, and
/// zeo has no `-w` to justify the verbose tier -- so the shapes worth
/// forwarding are named here explicitly.
///
/// Only the duplicated-key warning so far. Of prism's ten other default-level
/// shapes, two (`literal in condition`) render IDENTICALLY to a verbose-level
/// one, so a message-keyed filter cannot tell those apart -- and forwarding a
/// verbose-only warning by mistake is worse than forwarding none. Reading the
/// real level would need the binding to expose `pm_diagnostic_t::level`.
fn is_default_level(message: &str) -> bool {
    message.starts_with("key ") && message.contains(" is duplicated and overwritten on line ")
}
