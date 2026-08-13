//! Compile-time `require`/`require_relative`/`load` resolution: an HIR-level
//! graft, not text surgery. A recognised call (receiver-less, one string
//! literal, top-level statement position) has its target parsed and lowered
//! into the SAME `Hir` arena and spliced in at the call's position. Dedup and
//! provenance are per-`Hir`; each spliced file's top-level locals are renamed
//! for per-file isolation (`parse::rename`).
//!
//! Faithfulness contract, each verified against CRuby:
//! - `require` dedup is by canonicalized path, mirroring CRuby's realpath
//!   layer: every spelling of one file loads once.
//! - The dedup entry is inserted BEFORE the file is lowered, matching CRuby's
//!   `loading_table`, so a circular require splices nothing the second time
//!   and statement order matches Ruby's execution order.
//! - `require` appends `.rb` unless present and searches the ordered `-I`
//!   roots first, then package roots; first hit wins.
//! - `require_relative` resolves against the requiring FILE's directory, never
//!   cwd; with no input-path context the error is CRuby's "cannot infer
//!   basepath".
//! - `load` never appends `.rb` and never dedups: each statement re-splices
//!   the file, and the per-instance local rename gives each execution the
//!   fresh scope Ruby gives it. A `load` cycle is a compile error here rather
//!   than Ruby's runtime infinite recursion.
//! - An unresolvable plain `require` is NOT a compile error. It lowers to a
//!   runtime `Kernel#require`, so a missing feature raises `LoadError` at its
//!   require site and `begin; require "x"; rescue LoadError` still works. A
//!   missing `require_relative` IS a compile error: it names a project-local
//!   file, never an optional dependency.
//! - A plain `require` named only in a METHOD BODY is not spliced. Loading it
//!   early would order it ahead of the file's own top-level requires and pull
//!   every lazy dependency into the binary, so it becomes a runtime
//!   `Kernel#require` and the gem report discloses the omission.
//!
//! Gems contribute additional search roots from a `.gemspec`
//! (`require_paths`), searched after every `-I` root. A feature provided by
//! more than one gem is an error. `load` does not search gem roots: its
//! compile-time uses are project-local.
//!
//! Divergences: a spliced `require`'s return value is unobservable, since it
//! is statement-position-only; `load`'s relative names resolve against the
//! roots then the requiring file's directory, cwd being meaningless at compile
//! time; `load`'s `wrap` parameter, native features, and `~`/`./`-prefixed
//! `require` are clean rejections.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use crate::hir::{Hir, HirNode, LoadedFile, NodeId};
use crate::lower::context::{BindingsFrame, SourceFileFrame, current_box_binding};
use crate::lower::features::{canonical_ext_feature, is_builtin_feature};
use crate::lower::{PResult, lower_node};
use crate::lower_error::LowerError;
use crate::rename;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

/// Every `.rb` under `dir`, recursively. Symlinked directories are followed
/// like `require` follows them; a cycle is bounded by the filesystem, since a
/// canonicalized repeat is skipped by the caller's dedup table.
fn collect_rb_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rb_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rb") {
            out.push(path);
        }
    }
}

/// The feature name `path` answers to under load-path root `root`: the
/// relative path without its `.rb`, always `/`-separated (a `require` string
/// is not a platform path).
fn feature_name_under(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let rel = rel.with_extension("");
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

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
    /// The gemspec's declared version, when it states one -- disclosure and
    /// diagnostics; resolution never version-solves (the lockfile already
    /// did).
    version: Option<String>,
    /// Where this gem came from -- what an ambiguity warning names, and what
    /// separates zeo's own stdlib tier from caller-supplied code.
    provenance: GemProvenance,
}

/// Which tier provided a gem. Precedence between tiers is positional (the
/// `packages` list is precedence-ordered, see `lower_main_file`); this is
/// carried for reporting, not consulted for ranking.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GemProvenance {
    /// A caller-supplied package dir (`--gems`).
    PackageDir,
    /// zeo's own bundled library (`gems/`) -- the compiler's stdlib tier.
    Bundled,
    /// An external installed store, admitted by a lockfile.
    Store,
}

impl Gem {
    /// Build a gem from already-resolved parts -- the external gem store
    /// provider's path (`gem_store`), which has done its own existence checks.
    pub(super) fn from_parts(name: String, roots: Vec<PathBuf>, version: Option<String>) -> Self {
        Gem {
            name,
            roots,
            version,
            provenance: GemProvenance::Store,
        }
    }

    /// How an ambiguity warning names this gem -- `hashie 5.0.0`,
    /// `openssl 3.3.0 (bundled)` -- enough to tell squatting providers apart.
    fn describe(&self) -> String {
        let mut s = self.name.clone();
        if let Some(v) = &self.version {
            s.push(' ');
            s.push_str(v);
        }
        match self.provenance {
            GemProvenance::PackageDir => {}
            GemProvenance::Bundled => s.push_str(" (bundled)"),
            GemProvenance::Store => s.push_str(" (gem store)"),
        }
        s
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
    /// Whether the loader is inside `materialize_units`. A static require in
    /// a UNIT body must not splice its target inline: the target becomes its
    /// own unit and the call stays live, so it loads when the unit body
    /// actually EXECUTES -- CRuby's order. Splicing inline wove one unit's
    /// body into another's, and the `required` dedup then handed a shared
    /// dependency to whichever unit the sweep reached first, leaving every
    /// later requirer with nothing (`rbconfig` inside rspec-support's
    /// `ruby_features.rb` was the corpus case).
    in_unit_sweep: bool,
    /// External-store gems zeo can't provide, `name -> reason`:
    /// a `require` of one fails with the store's precise reason (which native
    /// layout, why) instead of the generic "cannot load such file".
    store_exclusions: HashMap<String, String>,
    /// How each `require`d library was satisfied, in require order.
    /// A LOG of what resolution did, not a property of the program -- it used
    /// to hang off `Hir`, which made the IR depend on the gem reporter for
    /// bookkeeping no consumer of the arena ever reads.
    gem_records: Vec<crate::gem_report::GemRecord>,
    /// `gem_records`' names, for the first-wins test -- see `record_gem`.
    gem_names: std::collections::HashSet<String>,
    /// `resolve_require` results, per feature. Sound as a plain memo because
    /// every input the search reads (`roots`, `packages`,
    /// `store_exclusions`) is fully constructed before lowering starts and
    /// never mutated after; the same feature is otherwise re-searched across
    /// every file's resolvability pre-scan AND again at its splice.
    /// `Err` verdicts (only the strict-mode ambiguity error remains) are not
    /// cached -- they abort the compile at first sight.
    require_memo: std::cell::RefCell<HashMap<String, ResolvedRequire>>,
    /// Features that resolved out of MULTIPLE providing gems, with every
    /// provider's description -- recorded once at the (memoized) resolution,
    /// drained into a compile warning at the require statement that splices
    /// the feature (the site that owns a file/line to warn at).
    ambiguous_features: std::cell::RefCell<HashMap<String, Vec<String>>>,
}

/// `resolve_require`'s success shape: the found path plus the owning
/// package's name, or `None` for not-on-disk.
type ResolvedRequire = Option<(PathBuf, Option<String>)>;

/// Reads a Ruby file as CRuby reads it: a stream of BYTES, not text. A byte
/// that is not valid UTF-8 is a syntax error where Ruby reads code, and is
/// simply skipped inside a comment -- so a file that is not valid UTF-8 goes
/// to prism exactly as it lies on disk, and prism (the same parser CRuby
/// itself runs) gives the verdict and the message.
///
/// Only a file prism accepts becomes a Rust `String`, one placeholder per
/// invalid byte. That is lossless for the compile: prism accepting it is
/// proof every one of those bytes sat in a comment. The placeholder is a
/// single byte so every prism span offset still addresses the same character.
pub fn read_source(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let bytes = match String::from_utf8(bytes) {
        Ok(text) => return Ok(text),
        Err(e) => e.into_bytes(),
    };
    if let Some(err) = ruby_prism::parse(&bytes).errors().next() {
        return Err(format!(
            "{}: parse error: {}",
            path.display(),
            err.message()
        ));
    }
    let mut out = String::with_capacity(bytes.len());
    let mut rest = &bytes[..];
    loop {
        match std::str::from_utf8(rest) {
            Ok(tail) => {
                out.push_str(tail);
                return Ok(out);
            }
            Err(e) => {
                let (good, bad) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(good).unwrap_or_default());
                let skip = e.error_len().unwrap_or(bad.len());
                out.extend(std::iter::repeat_n('?', skip));
                rest = &bad[skip..];
            }
        }
    }
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
    gem_paths: &[PathBuf],
    lockfile: Option<&Path>,
    root_gem: Option<&crate::Gem>,
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
    // The search roots, kept for the runtime's cosmetic `$LOAD_PATH` (see
    // `Hir::search_roots`) -- as given, the way `ruby -I` reports them.
    hir.search_roots = load_roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let bundled_dir = bundled_gems_dir();
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
                .chain(bundled_dir.clone())
                .collect::<Vec<_>>(),
            bundled_dir.as_deref(),
        )?,
        required: HashSet::new(),
        splicing: Vec::new(),
        in_unit_sweep: false,
        store_exclusions: HashMap::new(),
        gem_records: Vec::new(),
        gem_names: std::collections::HashSet::new(),
        require_memo: std::cell::RefCell::new(HashMap::new()),
        ambiguous_features: std::cell::RefCell::new(HashMap::new()),
    };
    // The external gem store: store dirs (`--gem-path`/`GEM_PATH`) plus a
    // lockfile (via `--bundle-gemfile`/`BUNDLE_GEMFILE`) add the pure-Ruby
    // gems zeo can compile as extra roots, and record a disclosure for every
    // gem it satisfies natively or can't provide. Store gems are APPENDED, so
    // a bundled zeo gem of the same name shadows them (first-name-wins), and
    // never override the compiler's own libraries.
    if let Some(lock) = lockfile
        && !gem_paths.is_empty()
    {
        let parsed = super::lockfile::parse_file(lock)?;
        let resolution = super::gem_store::resolve(gem_paths, &parsed)?;
        for (name, roots) in resolution.roots {
            if !loader.packages.iter().any(|g| g.name == name) {
                let version = parsed
                    .gems
                    .iter()
                    .find(|g| g.name == name)
                    .map(|g| g.version.clone());
                loader.packages.push(Gem::from_parts(name, roots, version));
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
        // Require-precedence, Bundler's semantics: the lockfile's gems rank
        // in reverse-topological order (Gemfile DEPENDENCIES roots first,
        // each gem ahead of its own dependencies, ties by name), AHEAD of
        // every unlocked package -- Bundler activates locked gems and an
        // activated gem's load path beats a merely-installed one, zeo's
        // bundled stdlib copies included (`rubygems_integration.rb`). Ranked
        // by NAME, so a locked gem zeo satisfies from its own bundled copy
        // still ranks as the lockfile places it. The sort is stable:
        // unlocked packages keep their discovery order after the ranked
        // block.
        let rank = lockfile_precedence(&parsed);
        loader
            .packages
            .sort_by_key(|g| rank.get(&g.name).copied().unwrap_or(usize::MAX));
    } else {
        // No lockfile: RubyGems' own `find_by_path` order -- specs sorted by
        // name ascending, byte-wise (`specification_record.rb`), so an
        // ambiguous feature resolves to the alphabetically first provider.
        // Names are unique here (first-name-wins shadowing already applied),
        // so the version-descending half of RubyGems' `_resort!` never
        // reaches a comparison.
        loader.packages.sort_by(|a, b| a.name.cmp(&b.name));
    }
    // The distinguished ROOT package outranks everything: Bundler's root
    // semantics, where the app's own gem is activated first. The gem probe
    // names its subject here, so a feature the subject squats resolves to
    // the gem actually under test rather than to an alphabetically earlier
    // dependency.
    if let Some(root) = root_gem
        && let Some(pos) = loader.packages.iter().position(|g| g.name == root.name())
    {
        let subject = loader.packages.remove(pos);
        loader.packages.insert(0, subject);
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
    hir.data_section = input_path.and_then(|p| data_section(&result, p));
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
    if input_path.is_some() {
        hir.main_file = Some(main_file);
    }
    let prev_file = hir.lowering_file.replace(main_file);
    // The MAIN file deliberately sets no `lowering_dir`: a dynamic require
    // here demands only the `-I` roots, not the file's own directory. A main
    // file sits wherever the user ran from -- a scratch dir, a checkout root
    // -- and a recursive walk under it compiles in whatever else lives
    // there. A loader that globs its own tree is a REQUIRED file (tzinfo's
    // ts_all.rb, this repo's tests/glob_require_units/loader.rb), and
    // `splice_file` gives those their directory.
    let prev_dir = std::mem::replace(&mut hir.lowering_dir, None);
    let lowered = loader
        .lower_file_statements(
            hir,
            &result,
            program.statements().body(),
            dir.as_deref(),
            None,
            0,
        )
        .and_then(|main_stmts| {
            // Demand-driven: the shim's module code rides along only when
            // something can actually reach RbConfig. Splicing AFTER
            // the main lowering (but prepending the statements, so the shim
            // still executes first) is what lets the decision see every
            // spliced file; an explicit `require "rbconfig"` already spliced
            // it mid-main and dedups here. The `$LOADED_FEATURES` entry stays
            // unconditional either way -- see codegen's seed_loaded_features.
            let mut all = if wants_ambient_rbconfig(hir) {
                loader.preload_ambient_features(hir)?
            } else {
                Vec::new()
            };
            all.extend(main_stmts);
            Ok(all)
        });
    hir.lowering_file = prev_file;
    hir.lowering_dir = prev_dir;
    let lowered = lowered?;
    // AFTER every splice: the demanded load paths are compiled in as units,
    // and `required` now holds every file that runs at a fixed position, so
    // nothing is compiled in twice. A unit lowered here may itself demand more
    // (a gem whose lazily-loaded files compute targets of their own), so this
    // runs to fixpoint.
    while !hir.unit_demand.is_empty() || !hir.single_unit_demand.is_empty() {
        loader.materialize_units(hir)?;
    }
    // Disclose the libraries no position outside a method body required, so
    // zeo left them out. `record_gem` is first-wins, and every real
    // satisfaction is already recorded, so only the truly absent ones land.
    let mut deferred: Vec<&String> = hir.deferred_requires.iter().collect();
    deferred.sort();
    for name in deferred {
        loader.record_gem(crate::gem_report::GemRecord {
            name: name.clone(),
            by: crate::gem_report::SatisfiedBy::Excluded {
                kind: crate::gem_report::DEFERRED_KIND.to_string(),
                reason: "required only from a method body, which whole-program AOT does not \
                         load; require it at top level to compile it in"
                    .to_string(),
            },
        });
    }
    Ok((lowered, loader.gem_records))
}

/// Whether the ambient rbconfig shim must be compiled in: some spliced
/// source names `RbConfig` (or the `rbconfig` feature -- a plain substring
/// scan, deliberately over-approximate: a comment mention costs only the
/// shim's inclusion), or runtime `eval` exists and could reach it.
fn wants_ambient_rbconfig(hir: &Hir) -> bool {
    hir.uses_runtime_eval()
        || hir
            .files
            .iter()
            .any(|f| f.source.contains("RbConfig") || f.source.contains("rbconfig"))
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
        // The set decides first-wins; `gem_records` keeps the order, which the
        // report depends on. Scanning the list per record made recording a
        // program's gems quadratic in their count.
        if !self.gem_names.insert(record.name.clone()) {
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
        // for three uses below: (1) a resolvability pre-scan HERE, (2) recording
        // the method-body-only requires zeo declines to load, and (3) the eager
        // splice of the rest AFTER the statement loop.
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
            let Some(feature) = literal_feature(result, hir, call)? else {
                continue;
            };
            // A cwd-shaped feature that the requiring file's directory
            // resolves (the `require "./lib/foo"` analogue below in
            // `lower_require_statement`) is resolvable, just not through
            // `resolve_require`'s dir-less search.
            let cwd_shape = feature.starts_with("./") || feature.starts_with("../");
            if !self.require_resolvable(&feature)
                && !(cwd_shape && resolve_require_relative(&feature, dir).is_ok())
            {
                hir.unresolvable_requires.insert(feature);
            }
        }
        // A plain `require` only a METHOD BODY reaches: its target, when it
        // resolves to a file, is compiled in as a LAZY unit -- the call stays
        // a runtime `Kernel#require` and loads the unit at first execution,
        // which is CRuby's order exactly (the same treatment a unit body's
        // own requires get, and the same reason: whether the method ever
        // runs is a runtime fact). `deferred_requires` keeps the call from
        // folding to `true` either way; a target that does NOT resolve stays
        // the honest runtime `LoadError` (rspec's optional `simplecov`).
        // A builtin is exempt: it needs no splice, so deferring it would
        // turn a working require into a `LoadError`.
        for call in &requires.deferred {
            let Some(feature) = literal_feature(result, hir, call)? else {
                continue;
            };
            if is_builtin_feature(&feature) {
                continue;
            }
            if let Ok(Some((path, package))) = self.resolve_require(&feature) {
                // Disclosed as satisfied (first-wins preempts the
                // "not compiled in" record the deferred set would
                // otherwise produce at the end of the load).
                let by = match &package {
                    Some(_) => crate::gem_report::SatisfiedBy::BundledGem {
                        path: display_path(&path),
                    },
                    None => crate::gem_report::SatisfiedBy::StdlibRoot {
                        path: display_path(&path),
                    },
                };
                self.record_gem(crate::gem_report::GemRecord {
                    name: feature.clone(),
                    by,
                });
                hir.single_unit_demand.insert((
                    package.or_else(|| hir.lowering_package.clone()),
                    path,
                    feature.clone(),
                ));
            }
            hir.deferred_requires.insert(feature);
        }
        // A `require_relative` of a NATIVE (.so/.bundle) feature names a
        // compiled extension zeo cannot load, protected or not: keep the
        // CALL and raise the catchable runtime `LoadError` -- the same
        // deferral its plain-`require` spelling gets through the static-ext
        // fallthrough -- instead of failing the whole compile.
        for call in &requires.calls {
            if call.name().as_slice() != b"require_relative" {
                continue;
            }
            let Some(feature) = literal_feature(result, hir, call)? else {
                continue;
            };
            if is_native_feature(&feature)
                && let Some(file) = hir.lowering_file
            {
                hir.optional_require_sites
                    .insert((file, call.location().start_offset() as u32));
            }
        }
        // A `require_relative` under a `rescue LoadError` is the pure-Ruby
        // fallback idiom: the gem ships an optional native half and CATCHES
        // its absence. When the target is missing, the call site is recorded
        // so it lowers to a runtime `Kernel#require_relative` raising the
        // catchable `LoadError` -- CRuby's behaviour -- instead of failing
        // the whole compile. A missing `require_relative` OUTSIDE that
        // protection stays the loud error it always was.
        for call in &requires.optional_rel {
            let Some(feature) = literal_feature(result, hir, call)? else {
                continue;
            };
            if resolve_require_relative(&feature, dir).is_err()
                && let Some(file) = hir.lowering_file
            {
                hir.optional_require_sites
                    .insert((file, call.location().start_offset() as u32));
            }
        }
        // A require under an UNDECIDED guard runs only when the guard
        // passes. CRuby's order of events is restored by compiling the
        // target in as a GATED unit and keeping the CALL: the guard decides
        // at runtime whether the unit ever executes. Eagerly splicing these
        // ran them unconditionally -- minitest's `require_relative "hell" if
        // ENV["MT_HELL"]` fired its side effects in every compile. A builtin
        // needs no unit, and an unresolvable target is already a runtime
        // LoadError through the pre-scan above.
        for call in &requires.conditional {
            let Some(feature) = literal_feature(result, hir, call)? else {
                continue;
            };
            let relative = call.name().as_slice() == b"require_relative";
            if !relative && is_builtin_feature(&feature) {
                continue;
            }
            let resolved = if relative {
                resolve_require_relative(&feature, dir)
                    .ok()
                    .map(|p| (p, None))
            } else {
                self.resolve_require(&feature).ok().flatten()
            };
            let Some((path, package)) = resolved else {
                // Unresolvable: the call stays and raises at runtime; for a
                // plain require the resolvability pre-scan already recorded
                // it. A missing require_relative under a guard defers the
                // same way (the guard may never pass).
                if relative && let Some(file) = hir.lowering_file {
                    hir.optional_require_sites
                        .insert((file, call.location().start_offset() as u32));
                }
                continue;
            };
            if let Some(file) = hir.lowering_file {
                hir.conditional_require_sites
                    .insert((file, call.location().start_offset() as u32));
                hir.single_unit_demand.insert((
                    package.or_else(|| hir.lowering_package.clone()),
                    path,
                    feature,
                ));
            }
        }
        // ... and the requiring file's OWN declarations are recorded before
        // they do, because the dependency runs both ways. libuv writes
        // `require 'libuv/ext/types'` three lines above the `attach_function`s
        // that spend its enums, which is what the pre-lower below is for;
        // ethon writes `extend ::FFI::Library` in `curl.rb` and requires
        // `curls/constants.rb`, which REOPENS that module to declare its
        // enums, thirteen lines lower. Lowering the required file first left
        // the reopen looking like a plain namespace, so no directive in it was
        // recognized at all.
        //
        // Only the MARKS are taken here, not the declarations themselves: what
        // a body IS (an FFI library, a struct class) is a syntactic fact this
        // walk can read, where what it DECLARES needs the alias tables that
        // only real lowering has.
        mark_ffi_bodies(hir, &body, &mut Vec::new());
        // Class/module-body requires pre-LOWER here, ahead of this file's own
        // statements: CRuby runs them MID-body, so their declarations -- the
        // FFI vocabulary above all -- exist before the statements below them.
        //
        // The lowered body then becomes its own FEATURE UNIT rather than a
        // splice, and the CALL stays live: a unit is emitted as a free
        // function at the top-level cref and the runtime `require` resolves
        // and calls it, so the body runs at the require's own document
        // position with its constants landing on `Object` -- exactly CRuby's
        // order. Appending to `trailing` instead ran it after the whole
        // enclosing file, so `module M; require_relative "x"; p X; end`
        // printed before/after/inner and raised `uninitialized constant
        // M::X`. A target with no unit spelling (a builtin activation, a
        // synthesized shim) still splices trailing.
        let mut nested_spliced = Vec::new();
        for call in &requires.nested {
            let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
            let unit = self.nested_unit_target(hir, result, call, &name, dir)?;
            // Whether THIS statement is the one that loads the target, asked
            // before the splice takes the dedup slot. A re-require keeps its
            // call and answers `false` off the runtime's loaded set, which is
            // only right if the first one really did register a unit.
            let fresh = unit
                .as_ref()
                .is_some_and(|(_, _, c)| !self.required.contains(&(current_box, c.clone())));
            let Some(spliced) =
                self.lower_require_statement(hir, result, call, &name, dir, file_idx, current_box)?
            else {
                continue;
            };
            let registered = unit.as_ref().is_some_and(|(_, _, c)| {
                hir.loaded_files
                    .iter()
                    .any(|lf| &lf.canonical == c && lf.is_unit)
            });
            match unit {
                Some((feature, absolute, canonical)) if fresh || registered => {
                    if let Some(file) = hir.lowering_file {
                        hir.conditional_require_sites
                            .insert((file, call.location().start_offset() as u32));
                    }
                    if fresh {
                        for lf in hir
                            .loaded_files
                            .iter_mut()
                            .filter(|lf| lf.canonical == canonical)
                        {
                            lf.is_unit = true;
                        }
                        hir.feature_units.push(crate::hir::FeatureUnit {
                            feature,
                            absolute,
                            body: spliced,
                        });
                    }
                }
                _ => nested_spliced.extend(spliced),
            }
        }

        for n in body.iter() {
            if let Some(call) = n.as_call_node() {
                let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
                if call.receiver().is_none()
                    && matches!(name.as_str(), "require" | "require_relative" | "load")
                    && let Some(spliced) = self.lower_require_statement(
                        hir,
                        result,
                        &call,
                        &name,
                        dir,
                        file_idx,
                        current_box,
                    )?
                {
                    combined.extend(spliced);
                    continue;
                }
                // A dynamic `load`/`require` (computed target): not spliced.
                // Fall through to the general lowering so it becomes a
                // runtime `Kernel#{require,load}` call (raises LoadError).
                // `box.require "f"` / `box.require_relative` / `box.load`
                // / `box.eval "src"` at top-level statement position
                //: resolve like the receiver-less forms, splice
                // with the BOX's id, wrap in one BoxScope. Statement-
                // position `box.eval` may define classes (real Ruby's
                // Box#eval compiles a top-level iseq); expression-position
                // eval is `parse::mod`'s recognizer, defs rejected there.
                if let Some(recv) = call.receiver()
                    && let Some(lv) = recv.as_local_variable_read_node()
                {
                    let lname = String::from_utf8_lossy(lv.name().as_slice()).into_owned();
                    if let Some(bx) = current_box_binding(&lname) {
                        if matches!(name.as_str(), "require" | "require_relative" | "load") {
                            let Some(spliced) = self.lower_require_statement(
                                hir, result, &call, &name, dir, file_idx, bx,
                            )?
                            else {
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
            // `box = Ruby::Box.new` -- allocates a fresh compile-time box,
            // records the file-local binding, AND binds the local to the
            // handle VALUE (the box's top-level surrogate as a Class), so
            // `p box` works.
            if let Some(lw) = n.as_local_variable_write_node()
                && crate::lower::eval_splice::is_ruby_box_new(&lw.value())
            {
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
            // A top-level `include Mod` (constant arguments) mixes Mod into
            // `Object` -- the top-level self's class. Lowered to `HirNode::
            // Include` so `analyze` registers it on `Object` exactly as a
            // `class Object; include Mod; end` reopen would, rather than
            // emitting a (nonexistent) runtime `include` call on `main`.
            if let Some(call) = n.as_call_node()
                && call.receiver().is_none()
                && call.name().as_slice() == b"include"
            {
                let arg_list: Vec<_> = call
                    .arguments()
                    .map(|a| a.arguments().iter().collect())
                    .unwrap_or_default();
                let all_constants = !arg_list.is_empty()
                    && arg_list.iter().all(|a| {
                        a.as_constant_read_node().is_some() || a.as_constant_path_node().is_some()
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
            if let Some(alias) = n.as_alias_method_node()
                // An INTERPOLATED name has no compile-time spelling to defer
                // on -- fall through to `lower_node`, whose general-context
                // alias arm installs it at runtime on the default definee.
                && alias.new_name().as_interpolated_symbol_node().is_none()
                && alias.old_name().as_interpolated_symbol_node().is_none()
            {
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
            // A require this statement CONTAINS rather than IS, and that
            // LOADING the file runs -- a conditional (`require_relative
            // "hell" if ENV["MT_HELL"]`), a `begin`, a class body, a block.
            // Spliced at the position of the statement holding it, so the
            // file loads in the order it is written. (These used to splice at
            // the HEAD of the file, ahead of every top-level require:
            // minitest/autorun.rb's third line then loaded before its first,
            // and hell.rb's `class Minitest::Test` reopen became that class's
            // earliest body -- firing `Runnable.inherited` before
            // `Runnable`'s own body had a registry to add to.) A method-body
            // require is `lazy` instead, and lands at the end of the file.
            //
            // AFTER `lower_node`, which folds each require CALL to its load
            // result: splicing first would mark the feature loaded and turn a
            // first `require "digest"` from true into false.
            //
            // A require in the BODY of a `begin` whose rescue names LoadError
            // splices inside a synthesized `Begin` carrying the SAME lowered
            // rescue clauses: a raise at the required file's own top level
            // runs during `require` in CRuby, inside the written handler's
            // reach. power_assert opens with exactly that -- a TracePoint
            // probe that raises LoadError -- and test-unit degrades by
            // rescuing it; unwrapped, the probe's raise escaped to main.
            // (The handler lowers twice -- once here, once in the original
            // begin -- but only one copy can ever see a given raise.)
            let rescued_body_calls: Vec<usize> = match n.as_begin_node() {
                Some(begin) if rescues_load_error(&begin) => {
                    let mut body_reqs = RequireCollector::default();
                    if let Some(stmts) = begin.statements() {
                        use ruby_prism::Visit as _;
                        for s in stmts.body().iter() {
                            body_reqs.visit(&s);
                        }
                    }
                    body_reqs
                        .calls
                        .iter()
                        .map(|c| c.location().start_offset())
                        .collect()
                }
                _ => Vec::new(),
            };
            let mut nested = RequireCollector::default();
            {
                use ruby_prism::Visit as _;
                nested.visit(&n);
            }
            let mut rescued_spliced: Vec<crate::hir::NodeId> = Vec::new();
            for call in &nested.calls {
                let cname = String::from_utf8_lossy(call.name().as_slice()).into_owned();
                if let Some(spliced) = self.lower_require_statement(
                    hir,
                    result,
                    call,
                    &cname,
                    dir,
                    file_idx,
                    current_box,
                )? {
                    match rescued_body_calls.contains(&call.location().start_offset()) {
                        true => rescued_spliced.extend(spliced),
                        false => combined.extend(spliced),
                    }
                }
            }
            if !rescued_spliced.is_empty() {
                // Wrap ONLY when the spliced statements can actually raise a
                // LoadError at load time. The common rescued require (`begin;
                // require "json"; rescue LoadError; <fallback>`) loads
                // cleanly, and its fallback must stay DEAD -- the cloned
                // rescue would re-register fallback classes the dead-rescue
                // elimination exists to keep out of `defined?`.
                match spliced_may_raise_load_error(hir, &rescued_spliced) {
                    true => {
                        let begin = n.as_begin_node().expect("only a begin collects these");
                        let rescues =
                            crate::lower::control::lower_rescue_clauses(result, hir, &begin)?;
                        combined.push(hir.push(crate::hir::HirNode::Begin {
                            body: rescued_spliced,
                            rescues,
                            else_body: None,
                            ensure_body: None,
                        }));
                    }
                    false => combined.extend(rescued_spliced),
                }
            }
            // `autoload` (at any structural nesting): every `autoload :C,
            // path` names a file that PROVIDES `C`. A target that resolves to
            // a load-path file is compiled in as a LAZY unit; the call itself
            // stays a real runtime call whose `Module#autoload` row loads the
            // unit at declaration time (see `builtins::rmodule`). The
            // remaining divergence from CRuby is that the file runs at the
            // `autoload` statement rather than at first constant access, and
            // even if `C` is never referenced.
            //
            // It must NOT be spliced eagerly here: that ran the target's body
            // before the very statements preceding the `autoload` itself.
            // rspec-support's differ.rb is the case -- its first line calls a
            // singleton method that `module Support`'s body installs a few
            // statements BEFORE its `autoload :Differ`, and the eager splice
            // hoisted differ.rb ahead of the whole module statement.
            let mut autoloads = Vec::new();
            collect_autoloads(&n, &mut autoloads);
            for call in &autoloads {
                // Only a target this pass can NAME is registered. A computed
                // one -- including the one-argument form an `autoload` DSL
                // defines over `Module#autoload` -- is left to run: its
                // lowering demands the load path as units, and the runtime row
                // resolves the string the program actually builds
                // (`zeo_rt::features`).
                let Ok(feature) = crate::lower::autoload_feature(call) else {
                    continue;
                };
                // A target that is not on the load path is left ENTIRELY to
                // the runtime row. An autoload is weaker than a require:
                // CRuby does not load the file at all until the constant is
                // read, so an absent one is not an error until then -- and
                // most never are read. actionpack's
                // `autoload :Test, "rack/test"` is the case, and eager-
                // splicing it failed the whole compile of every gem that
                // reaches action_dispatch without rack-test alongside.
                if !self.require_resolvable(&feature) {
                    continue;
                }
                if let Ok(Some((path, package))) = self.resolve_require(&feature) {
                    hir.single_unit_demand.insert((
                        package.or_else(|| hir.lowering_package.clone()),
                        path,
                        feature,
                    ));
                    continue;
                }
                // The non-file verdicts keep the eager path: a built-in
                // feature/shim is pure activation with no body to mis-order,
                // and a store-excluded gem stays the loud error it always was.
                let spliced =
                    self.splice_feature(hir, &feature, "require", dir, file_idx, current_box)?;
                combined.extend(spliced);
            }

            combined.push(id);
            own.push(id);
        }
        if let Some(idx) = file_idx {
            rename::isolate_file_locals(hir, &own, idx);
        }
        drop(frame);

        // `Dir.glob("#{__dir__}/smtp/auth_*.rb") { |r| require_relative r }` --
        // net/smtp's authenticators, rubygems' plugins. The pattern is a
        // compile-time fact, so the matches are spliced here exactly like an
        // `autoload`; the CALL itself still lowers below, where the glob finds
        // nothing at runtime (an AOT binary has no source tree) and the block
        // never runs. Deduped through the shared `required` table.
        let mut globs = Vec::new();
        for n in body.iter() {
            collect_glob_requires(&n, &mut globs);
        }
        for call in &globs {
            let Some(glob) = glob_require_call(call, dir) else {
                continue;
            };
            for path in expand_glob(&glob.pattern) {
                let spliced =
                    self.splice_feature(hir, &path, &glob.flavor, dir, file_idx, current_box)?;
                combined.extend(spliced);
            }
        }

        // The method-body `require_relative`s, plus any require the statement
        // loop could not reach (one written inside an autoload/glob target).
        // The shared `required` table dedups, so everything already spliced
        // above is a no-op here and only the stragglers land. The
        // pre-lowered class-body splices land FIRST -- their position in this
        // list is the one the trailing pass always gave them.
        let mut trailing = nested_spliced;
        for call in requires.lazy.iter().chain(&requires.calls) {
            let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
            if let Some(spliced) =
                self.lower_require_statement(hir, result, call, &name, dir, file_idx, current_box)?
            {
                trailing.extend(spliced);
            }
        }
        combined.append(&mut trailing);
        Ok(combined)
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
        // A splat or `...` is the same non-resolvable shape with an arg
        // count of one -- `require(*args)` (wagons' optional-require helper,
        // always under a `rescue LoadError`) has no compile-time feature
        // name, and lowering the splat node itself dies in the generic
        // rejection. Leave the call for the runtime `Kernel#require`.
        if arg_list[0].as_splat_node().is_some()
            || arg_list[0].as_forwarding_arguments_node().is_some()
        {
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
        // The optional-native-half idiom: this exact call site was recorded
        // as missing-but-rescued, so it keeps its CALL and raises a runtime
        // `LoadError` for the rescue to catch. A guard-gated site keeps its
        // call the same way -- its target is a unit the guard may load.
        if let Some(file) = hir.lowering_file {
            let key = (file, call.location().start_offset() as u32);
            if (name == "require_relative" && hir.optional_require_sites.contains(&key))
                || hir.conditional_require_sites.contains(&key)
            {
                return Ok(None);
            }
        }
        // `require "./x"` / `require "../x"`: CRuby resolves these against
        // the runtime cwd. The compile-time analogue is the requiring
        // file's directory (`resolve_load`'s documented divergence -- the
        // shape only ever worked run from where the two coincide), so a
        // hit splices exactly as the `require_relative` spelling would --
        // same lexical normalization, same absolute-path dedup. A miss was
        // already recorded by the pre-scan and deferred to a runtime
        // `LoadError` before reaching here.
        if name == "require"
            && (feature.starts_with("./") || feature.starts_with("../"))
            && resolve_require_relative(&feature, dir).is_ok()
        {
            return self
                .splice_feature(
                    hir,
                    &feature,
                    "require_relative",
                    dir,
                    file_idx,
                    current_box,
                )
                .map(Some);
        }
        // Inside `materialize_units`, a require whose target is a real file
        // stays LIVE: the target registers as its own unit and the call loads
        // it when the unit body actually executes -- CRuby's order. See
        // `Loader::in_unit_sweep` for the inline-splice hazard this replaces.
        // Non-file verdicts (builtin activation, a synthesized shim, a static
        // ext) fall through to the eager path: activation is registration,
        // and a shim body is spliced per requiring unit (see
        // `splice_synthetic_shim`).
        if self.in_unit_sweep && name != "load" {
            let resolved = if name == "require_relative" {
                resolve_require_relative(&feature, dir).ok().map(|p| {
                    // Registered under its ABSOLUTE spelling (extension
                    // stripped, matching `materialize_units`): a bare
                    // relative name like "version" recurs in every gem, and
                    // the runtime `require_relative` absolutizes before it
                    // asks (`zeo_rt`'s `dynamic_require_relative`).
                    let abs = p.with_extension("").to_string_lossy().into_owned();
                    (p, hir.lowering_package.clone(), abs)
                })
            } else {
                self.resolve_require(&feature)
                    .ok()
                    .flatten()
                    .map(|(p, pkg)| {
                        (
                            p,
                            pkg.or_else(|| hir.lowering_package.clone()),
                            feature.clone(),
                        )
                    })
            };
            if let Some((path, package, unit_feature)) = resolved {
                if let Some(file) = hir.lowering_file {
                    hir.conditional_require_sites
                        .insert((file, call.location().start_offset() as u32));
                }
                hir.single_unit_demand.insert((package, path, unit_feature));
                return Ok(None);
            }
        }
        let spliced = self.splice_feature(hir, &feature, name, dir, file_idx, current_box)?;
        // A feature that resolved out of multiple gems warns HERE -- the
        // first require statement that splices it, the one site that owns a
        // file/line. Drained (not peeked), so re-requires stay quiet, like
        // any once-per-event Ruby warning.
        if let Some(providers) = self.ambiguous_features.borrow_mut().remove(&feature) {
            let (file, line) = match hir.lowering_file {
                Some(f) => {
                    let sf = &hir.files[f.0 as usize];
                    (
                        sf.name.clone(),
                        sf.line_at(call.location().start_offset() as u32),
                    )
                }
                None => ("-e".to_string(), 0),
            };
            hir.warnings.push(crate::diagnostics::CompileWarning {
                file,
                line,
                message: format!(
                    "`require \"{feature}\"` is provided by multiple gems ({}); resolved to {}",
                    providers.join(", "),
                    providers[0],
                ),
            });
        }
        Ok(Some(spliced))
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
    /// The `(feature, absolute, canonical)` a class-body require's target
    /// registers a FEATURE UNIT under, or `None` when the target has no unit
    /// spelling (a computed argument, an unresolvable name, a builtin
    /// activation, a synthesized shim) and must keep the trailing splice.
    ///
    /// `require_relative` registers under its ABSOLUTE spelling in both
    /// slots: a bare relative name like "version" recurs in every gem, and
    /// the runtime `require_relative` absolutizes before it asks -- the same
    /// rule `lower_require_statement`'s unit-sweep arm applies.
    fn nested_unit_target(
        &mut self,
        hir: &mut Hir,
        result: &ruby_prism::ParseResult,
        call: &ruby_prism::CallNode<'_>,
        name: &str,
        dir: Option<&Path>,
    ) -> PResult<Option<(String, String, PathBuf)>> {
        if name == "load" {
            return Ok(None);
        }
        let Some(feature) = literal_feature(result, hir, call)? else {
            return Ok(None);
        };
        let resolved = if name == "require_relative" {
            resolve_require_relative(&feature, dir).ok().map(|p| {
                let abs = p.with_extension("").to_string_lossy().into_owned();
                (p, abs)
            })
        } else {
            if is_builtin_feature(&feature) {
                return Ok(None);
            }
            self.resolve_require(&feature)
                .ok()
                .flatten()
                .map(|(p, _)| (p, feature.clone()))
        };
        let Some((path, unit_feature)) = resolved else {
            return Ok(None);
        };
        let Ok(canonical) = path.canonicalize() else {
            return Ok(None);
        };
        let absolute = canonical.with_extension("").to_string_lossy().into_owned();
        Ok(Some((unit_feature, absolute, canonical)))
    }

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
                    if let Some(spliced) = self.splice_synthetic_shim(hir, feature, current_box)? {
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
        if let Some(shim) = vendored_shim_feature(&canonical)
            && let Some(spliced) = self.splice_synthetic_shim(hir, shim, current_box)?
        {
            return Ok(spliced);
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
        // Inside the unit sweep, EVERY requiring unit carries its own copy of
        // the shim body rather than deduping to the first: which unit runs
        // first (if at all) is a runtime fact, and a shim is zeo-authored,
        // self-contained, and idempotent to re-run -- the cheap way to keep
        // `require "rbconfig"` meaning "RbConfig is defined after this line"
        // in whichever unit executes it.
        if !self.required.insert((box_id, canonical.clone())) && !self.in_unit_sweep {
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
            source.into(),
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
    /// Compiles in every `.rb` under the demanded load paths as a UNIT (see
    /// `Hir::feature_units`), skipping the files already spliced.
    ///
    /// The demand comes from a file that computes a `require`/`autoload`
    /// target: zeo cannot know which string it will build, so the honest
    /// answer is to compile in everything that string could name -- which is
    /// exactly its load path, and exactly what CRuby would have searched. The
    /// walk is bounded to the DEMANDING package's own roots, so a gem that
    /// resolves its own targets dynamically pays for itself and nothing else.
    fn materialize_units(&mut self, hir: &mut Hir) -> PResult<()> {
        self.in_unit_sweep = true;
        let result = self.materialize_units_inner(hir);
        self.in_unit_sweep = false;
        result
    }

    fn materialize_units_inner(&mut self, hir: &mut Hir) -> PResult<()> {
        // Single-file demands first: a conditional require names exactly one
        // target, and registering it under the feature AS REQUIRED is what
        // lets the runtime call find it.
        for (package, path, feature) in std::mem::take(&mut hir.single_unit_demand) {
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            if !self.required.insert((0, canonical.clone())) {
                continue; // already spliced: it runs at its own position
            }
            let absolute = canonical.with_extension("").to_string_lossy().into_owned();
            match self.splice_file(hir, &canonical, None, package.clone(), 0) {
                Ok(body) => {
                    for lf in hir
                        .loaded_files
                        .iter_mut()
                        .filter(|lf| lf.canonical == canonical)
                    {
                        lf.is_unit = true;
                    }
                    hir.feature_units.push(crate::hir::FeatureUnit {
                        feature,
                        absolute,
                        body,
                    })
                }
                Err(e) => hir
                    .declined_units
                    .push((feature, absolute, e.message().to_string())),
            }
        }
        for (package, dir) in std::mem::take(&mut hir.unit_demand) {
            let roots: Vec<PathBuf> = match &package {
                // No package: the `-I` roots the program was given, plus the
                // demanding file's own directory -- what a `__dir__`-relative
                // target names.
                None => {
                    let mut r = self.roots.clone();
                    if dir.is_dir() {
                        r.push(dir.clone());
                    }
                    r
                }
                Some(name) => match self.packages.iter().find(|g| &g.name == name) {
                    Some(g) => g.roots.clone(),
                    None => continue,
                },
            };
            for root in roots {
                tracing::debug!(?package, ?root, "unit sweep root");
                let mut files = Vec::new();
                collect_rb_files(&root, &mut files);
                files.sort();
                for path in files {
                    let Ok(canonical) = path.canonicalize() else {
                        continue;
                    };
                    if !self.required.insert((0, canonical.clone())) {
                        continue; // already spliced: it runs at its own position
                    }
                    let Some(feature) = feature_name_under(&root, &path) else {
                        continue;
                    };
                    let absolute = canonical.with_extension("").to_string_lossy().into_owned();
                    match self.splice_file(hir, &canonical, None, package.clone(), 0) {
                        Ok(body) => {
                            // The file was lowered through the splice path,
                            // but it RUNS only when required -- mark it so
                            // `$LOADED_FEATURES` is not seeded with it.
                            for lf in hir
                                .loaded_files
                                .iter_mut()
                                .filter(|lf| lf.canonical == canonical)
                            {
                                lf.is_unit = true;
                            }
                            hir.feature_units.push(crate::hir::FeatureUnit {
                                feature,
                                absolute,
                                body,
                            })
                        }
                        // Never reached by a require -> never observed. Reached
                        // by one -> a LoadError naming the gap, at the require.
                        Err(e) => {
                            hir.declined_units
                                .push((feature, absolute, e.message().to_string()))
                        }
                    }
                }
            }
        }
        Ok(())
    }

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
        let source: std::sync::Arc<str> = read_source(canonical)?.into();
        self.splice_source(hir, canonical, source, required_from, package, box_id)
    }

    /// The lowering half of [`Self::splice_file`], over already-read `source`.
    /// Split out so a SYNTHESIZED feature (an embedded shim like `rbconfig`,
    /// which has no file on disk) can splice through the same path, under a
    /// virtual `canonical` name that stands in for `__FILE__`/provenance.
    ///
    /// `source` is shared rather than owned: `ruby_prism` borrows it for as
    /// long as the lowering below reads the parse tree, so handing `add_file`
    /// an owned `String` meant copying every byte of every Ruby file the
    /// loader touched.
    fn splice_source(
        &mut self,
        hir: &mut Hir,
        canonical: &Path,
        source: std::sync::Arc<str>,
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
            is_unit: false,
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
        let file_id = hir.add_file(
            canonical.display().to_string(),
            std::sync::Arc::clone(&source),
        );
        let prev_file = hir.lowering_file.replace(file_id);
        // `loaded_files` and `files` are indexed independently (splice
        // instances vs. span provenance), so the owning package rides
        // alongside rather than being looked up from `lowering_file`.
        let prev_package = std::mem::replace(
            &mut hir.lowering_package,
            hir.loaded_files[idx].package.clone(),
        );
        let prev_dir = std::mem::replace(
            &mut hir.lowering_dir,
            canonical.parent().map(Path::to_path_buf),
        );
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
        hir.lowering_package = prev_package;
        hir.lowering_dir = prev_dir;
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
        if let Some(hit) = self.require_memo.borrow().get(feature) {
            return Ok(hit.clone());
        }
        let resolved = self.resolve_require_uncached(feature)?;
        self.require_memo
            .borrow_mut()
            .insert(feature.to_string(), resolved.clone());
        Ok(resolved)
    }

    /// The actual search behind [`Loader::resolve_require`]'s memo.
    fn resolve_require_uncached(
        &self,
        feature: &str,
    ) -> PResult<Option<(PathBuf, Option<String>)>> {
        // `./`/`../`/`~` paths resolve against the runtime working directory
        // in real Ruby, which doesn't exist at compile time. No VERDICT here
        // (`Ok(None)`, not `Err`): `lower_require_statement` first tries the
        // requiring file's directory (`resolve_load`'s documented cwd
        // analogue -- the dominant `require "./lib/foo"` shape only ever
        // worked run from the gem root, where the two coincide), and a miss
        // defers to a catchable runtime `LoadError` via the resolvability
        // pre-scan instead of failing the whole compile.
        if feature.starts_with("./") || feature.starts_with("../") || feature.starts_with('~') {
            return Ok(None);
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
        let mut hits: Vec<(PathBuf, &Gem)> = Vec::new();
        for pkg in &self.packages {
            // At most one hit per package: a package's OWN roots are
            // ordered by its manifest (first wins within the package).
            if let Some(cand) = pkg
                .roots
                .iter()
                .map(|r| r.join(&fname))
                .find(|c| c.is_file())
            {
                hits.push((cand, pkg));
            }
        }
        match hits.len() {
            0 => Ok(None),
            1 => {
                let (path, pkg) = hits.remove(0);
                Ok(Some((path, Some(pkg.name.clone()))))
            }
            // Multiple providers: the FIRST wins, because `packages` is in
            // require-precedence order (see `lower_main_file` -- Bundler's
            // activation semantics under a lockfile, RubyGems' name-ascending
            // `find_by_path` order without one). That is what real Ruby does
            // with a squatted feature name; its "found in multiple gems"
            // error is unreachable for top-level requires and disabled
            // outright under Bundler. The old unconditional error survives
            // behind `ZEO_STRICT_AMBIGUOUS_REQUIRE=1` for callers who want
            // squatting surfaced loudly; everyone else gets a warning at the
            // require site (via `ambiguous_features`).
            _ => {
                if strict_ambiguous_require() {
                    let names: Vec<&str> = hits.iter().map(|(_, g)| g.name.as_str()).collect();
                    return Err(format!(
                        "`require \"{feature}\"` is ambiguous: found in multiple gems ({})",
                        names.join(", ")
                    )
                    .into());
                }
                let providers: Vec<String> = hits.iter().map(|(_, g)| g.describe()).collect();
                self.ambiguous_features
                    .borrow_mut()
                    .insert(feature.to_string(), providers);
                let (path, pkg) = hits.remove(0);
                Ok(Some((path, Some(pkg.name.clone()))))
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
        // Rendered by build.rs from shims/rbconfig.rb.in with the build
        // target's platform facts (arch, darwin major, dlext, ...).
        "rbconfig" => Some(include_str!(concat!(env!("OUT_DIR"), "/rbconfig.rb"))),
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
/// Located through the resolved home (see `crate::home`): the repo's `gems/`
/// in the dev tree, the payload's `gems/` in an installed prefix. An absent
/// dir contributes no bundled gems, in either mode.
pub(super) fn bundled_gems_dir() -> Option<PathBuf> {
    let dir = match crate::home::zeo_home() {
        crate::home::ZeoHome::DevTree { root } => root.join("gems"),
        crate::home::ZeoHome::Installed { payload, .. } => payload.join("gems"),
        // Embedded in the binary at publish time; extracted once per version.
        crate::home::ZeoHome::Registry { cache } => {
            return crate::home::registry_gems_dir(cache);
        }
    };
    dir.is_dir().then_some(dir)
}

/// Whether `ZEO_DEBUG=strict-ambiguous-require` is set: a feature found in
/// multiple gems becomes the old hard compile error instead of resolving to
/// the precedence-first provider with a warning. Real Ruby never errors here
/// (see `resolve_require_uncached`'s multi-hit arm), so strictness is opt-in.
fn strict_ambiguous_require() -> bool {
    crate::debug_flags::debug(crate::debug_flags::DebugFlag::StrictAmbiguousRequire)
}

/// The lockfile's require-precedence ranks: gem name -> position, roots
/// first, then breadth-first down the dependency edges with each gem's deps
/// visited in name order. This is Bundler's activation order REVERSED --
/// Bundler activates dependencies before dependents (`spec_set.rb`'s tsort)
/// and a later activation inserts its load path ahead, so the DEPENDENT wins
/// an ambiguous feature; ranking dependents first expresses that as plain
/// first-match. Locked gems no edge reaches (a lockfile always connects, but
/// a hand-edited one may not) follow in name order, still ahead of anything
/// unlocked.
fn lockfile_precedence(lock: &super::lockfile::Lockfile) -> HashMap<String, usize> {
    let by_name: HashMap<&str, &super::lockfile::LockedGem> =
        lock.gems.iter().map(|g| (g.name.as_str(), g)).collect();
    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut queue: std::collections::VecDeque<&str> =
        lock.roots.iter().map(String::as_str).collect();
    while let Some(name) = queue.pop_front() {
        if !seen.insert(name) {
            continue;
        }
        order.push(name);
        if let Some(gem) = by_name.get(name) {
            let mut deps: Vec<&str> = gem.deps.iter().map(String::as_str).collect();
            deps.sort_unstable();
            queue.extend(deps);
        }
    }
    // `lock.gems` is already name-sorted (the parser's BTreeMap).
    for gem in &lock.gems {
        if !seen.contains(gem.name.as_str()) {
            order.push(&gem.name);
        }
    }
    order
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), i))
        .collect()
}

fn discover_packages(package_dirs: &[PathBuf], bundled_dir: Option<&Path>) -> PResult<Vec<Gem>> {
    let mut packages: Vec<Gem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in package_dirs {
        let provenance = if Some(dir.as_path()) == bundled_dir {
            GemProvenance::Bundled
        } else {
            GemProvenance::PackageDir
        };
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
            let pkg = parse_manifest(&pkg_dir, provenance)?;
            if seen.insert(pkg.name.clone()) {
                packages.push(pkg);
                continue;
            }
            // First-name-wins, EXCEPT for the curated set where zeo's own
            // native half IS the gem (`gem_report::substitution_note` -- ffi,
            // json, psych, openssl, ...). There the upstream Ruby half is
            // dead code: it opens by requiring a C extension zeo does not
            // have, and every class it then defines is one zeo's native half
            // already owns. Letting a caller-supplied copy shadow zeo's
            // compiled a program that died at load -- the real ffi gem's
            // `ffi/types.rb` raising `uninitialized constant FFI::TypeDefs`
            // -- and, once that file's computed `require RUBY_VERSION... +
            // "/ffi_c"` demanded the gem's whole load path as units, dragged
            // in `ffi/struct_layout.rb`'s `class Enum < Field` and failed the
            // compile outright. The `zeo-builtin-substitute` warning has
            // always CLAIMED zeo's implementation is the one in use; this is
            // what makes the claim true.
            if provenance == GemProvenance::Bundled
                && crate::gem_report::substitution_note(&pkg.name).is_some()
                && let Some(slot) = packages.iter_mut().find(|g| g.name == pkg.name)
            {
                *slot = pkg;
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
/// A declared `require_paths` entry that doesn't exist contributes no search
/// root, and is not an error. RubyGems puts the directory on `$LOAD_PATH`
/// whether or not it is there, and `require` simply never matches inside it --
/// oracle-verified against ruby 4.0.6. Metagems rely on this: `rails` itself
/// declares `require_paths: [lib]` and ships only a README and a licence, so
/// rejecting the gem would refuse to compile every program that depends on it.
/// An unsatisfiable `require` still fails, which is where the real error is.
fn parse_manifest(pkg_dir: &Path, provenance: GemProvenance) -> PResult<Gem> {
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
    let roots: Vec<PathBuf> = spec
        .require_paths
        .iter()
        .map(|rp| pkg_dir.join(rp))
        .filter(|root| root.is_dir())
        .collect();
    Ok(Gem {
        name: spec.name,
        roots,
        version: spec.version,
        provenance,
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
    if let Some(reason) = zeo_abi::declined_feature_reason(name) {
        return format!("cannot load such file -- {name}: {reason}");
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

/// The feature a `require`/`require_relative` names, or `None` if the argument
/// is not one constant string. Lowers the argument and reads its folded
/// literal, exactly as `lower_require_statement` and `lower_call_general` do,
/// so all three agree on which requires are literal.
fn literal_feature(
    result: &ruby_prism::ParseResult,
    hir: &mut Hir,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<Option<String>> {
    let arg_list: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [arg] = arg_list.as_slice() else {
        return Ok(None);
    };
    // One argument syntactically, but no compile-time text -- and a bare
    // `SplatNode`/`...` doesn't lower as an expression, so it must be
    // answered here (`require(*names)` in an optional-dependency helper).
    if arg.as_splat_node().is_some() || arg.as_forwarding_arguments_node().is_some() {
        return Ok(None);
    }
    let arg_id = lower_node(result, hir, arg)?;
    Ok(crate::lower::eval_splice::literal_string_text(hir, arg_id))
}

/// Collects every receiver-less `require`/`require_relative` call in a file's
/// tree, split by whether LOADING the file runs it. Top-level requires land in
/// `calls` too, but dedup skips the second splice attempt. `load` and
/// receiver-bearing (`box.require`) forms are excluded: they cannot be
/// eager-spliced.
#[derive(Default)]
struct RequireCollector<'a> {
    /// Requires to splice where they are written: everything LOADING the
    /// file runs -- top level, a conditional, a `begin`, a class body, a
    /// block.
    calls: Vec<ruby_prism::CallNode<'a>>,
    /// A `require_relative` inside a method BODY. Spliced (it names a file of
    /// this same program, which must stay loaded), but at the END of the
    /// file: the method cannot run before the file that defines it has
    /// finished loading, and the target routinely reopens a class this file
    /// is still building -- irb's `ext/eval_history.rb` pushes onto a
    /// `NOPRINTING_IVARS` that `context.rb` assigns below the `def` that
    /// requires it.
    lazy: Vec<ruby_prism::CallNode<'a>>,
    /// A plain `require` only a method BODY reaches. These are not spliced, but
    /// their names are still wanted (see `Hir::deferred_requires`).
    deferred: Vec<ruby_prism::CallNode<'a>>,
    /// `require_relative`s lexically inside a `begin` body whose rescue
    /// catches `LoadError` -- candidates for `Hir::optional_require_sites`
    /// when their target turns out not to exist.
    optional_rel: Vec<ruby_prism::CallNode<'a>>,
    /// Requires under a runtime-UNDECIDABLE `if`/`unless`/`case` branch --
    /// CRuby runs these only when the guard passes, so they become gated
    /// feature units rather than eager splices.
    conditional: Vec<ruby_prism::CallNode<'a>>,
    /// Requires written inside a `class`/`module` BODY (unguarded, outside
    /// any `def`). CRuby runs these MID-body, so the target's declarations
    /// -- FFI vocabulary above all (libuv's `require 'libuv/ext/types'`
    /// three lines above the `attach_function`s that spend its enums) --
    /// exist before the statements below them. These pre-LOWER ahead of the
    /// file's own statements; their nodes still emit at the file-trailing
    /// position, so runtime order is unchanged.
    nested: Vec<ruby_prism::CallNode<'a>>,
    /// Enclosing `class`/`module` bodies. Nonzero puts a require in
    /// `nested`.
    class_depth: u32,
    /// Enclosing `def`s. Nonzero means a `require` here is deferred.
    defs: u32,
    /// Enclosing `begin` bodies whose rescue catches `LoadError`. Nonzero
    /// means a `require_relative` here is allowed to be missing.
    load_error_rescues: u32,
    /// Enclosing branches whose guard `eval_static_guard` could NOT decide.
    /// Nonzero means a `require` here may or may not run.
    runtime_cond: u32,
}

/// Whether one of this begin's rescue clauses catches `LoadError`. Named
/// classes only: a BARE `rescue` catches `StandardError`, and `LoadError <
/// ScriptError < Exception` sits outside that tree.
/// Whether any of `stmts`' subtrees holds a `raise` naming `LoadError` (or a
/// superclass) OUTSIDE method bodies -- a raise the require itself would run.
/// power_assert's TracePoint probe is the shape: `begin ... rescue; raise
/// LoadError, '...'; end` at the file's top level. Method and lambda bodies
/// don't run at load, so they don't count.
/// Records which `module` bodies in this file are FFI LIBRARIES, before any
/// file they require from inside one gets to lower.
///
/// `cref` carries the enclosing names, so a nested `module Curl` inside
/// `module Ethon` is marked under `Ethon::Curl` -- the same key
/// `lower::defs` builds through `Hir::cref_path`. A compact path
/// (`module A::B`) contributes both segments, matching how the real lowering
/// walks it.
///
/// Struct classes are deliberately NOT marked here. A struct name means
/// different things in different positions -- by-reference in a signature, the
/// inline layout in a field -- and the per-body alias table takes the first
/// answer it is given. Marking the class before its `layout` lowered seeded
/// `by_value` with the by-reference entry, which is a wrong ABI rather than a
/// missing one.
fn mark_ffi_bodies(hir: &mut Hir, body: &ruby_prism::NodeList<'_>, cref: &mut Vec<String>) {
    for node in body.iter() {
        let (name, inner, superclass) = if let Some(m) = node.as_module_node() {
            (
                crate::lower::consts::constant_path_name(&m.constant_path()).ok(),
                m.body(),
                None,
            )
        } else if let Some(c) = node.as_class_node() {
            (
                crate::lower::consts::constant_path_name(&c.constant_path()).ok(),
                c.body(),
                c.superclass()
                    .and_then(|s| crate::lower::consts::constant_path_name(&s).ok()),
            )
        } else {
            continue;
        };
        let Some(name) = name else { continue };
        let depth = cref.len();
        cref.extend(
            name.trim_start_matches("::")
                .split("::")
                .map(str::to_string),
        );
        let path = match name.strip_prefix("::") {
            Some(absolute) => absolute.to_string(),
            None => cref.join("::"),
        };
        if let Some(inner) = inner.as_ref().and_then(|b| b.as_statements_node()) {
            if inner
                .body()
                .iter()
                .any(|s| crate::lower::ffi::is_extend_ffi_library(&s))
            {
                hir.mark_ffi_library(&path);
            }
            let union = matches!(superclass.as_deref(), Some("FFI::Union" | "::FFI::Union"));
            let is_struct = union
                || matches!(
                    superclass.as_deref(),
                    Some("FFI::Struct" | "::FFI::Struct" | "FFI::ManagedStruct")
                );
            for stmt in inner.body().iter() {
                match is_struct {
                    true => crate::lower::ffi::prescan_layout(hir, &stmt, &path, union),
                    false => crate::lower::ffi::prescan_declaration(hir, &stmt),
                }
            }
            mark_ffi_bodies(hir, &inner.body(), cref);
        }
        cref.truncate(depth);
    }
}

fn spliced_may_raise_load_error(hir: &Hir, stmts: &[crate::hir::NodeId]) -> bool {
    use crate::hir::HirNode;
    fn mentions(hir: &Hir, id: crate::hir::NodeId) -> bool {
        match &hir[id] {
            HirNode::ClassRef(n)
                if matches!(n.as_str(), "LoadError" | "ScriptError" | "Exception") =>
            {
                return true;
            }
            HirNode::QualifiedConstRead(_, n)
                if matches!(n.as_str(), "LoadError" | "ScriptError" | "Exception") =>
            {
                return true;
            }
            _ => {}
        }
        let mut hit = false;
        hir[id].for_each_child(&mut |c| hit = hit || mentions(hir, c));
        hit
    }
    fn walk(hir: &Hir, id: crate::hir::NodeId) -> bool {
        match &hir[id] {
            HirNode::DefMethod { .. } | HirNode::Lambda { .. } => false,
            HirNode::Raise(args, _) => args.first().is_some_and(|&a| mentions(hir, a)),
            node => {
                let mut hit = false;
                node.for_each_child(&mut |c| hit = hit || walk(hir, c));
                hit
            }
        }
    }
    stmts.iter().any(|&s| walk(hir, s))
}

fn rescues_load_error(node: &ruby_prism::BeginNode<'_>) -> bool {
    let mut clause = node.rescue_clause();
    while let Some(rescue) = clause {
        for ex in rescue.exceptions().iter() {
            let name = match (ex.as_constant_read_node(), ex.as_constant_path_node()) {
                (Some(read), _) => Some(read.name().as_slice().to_vec()),
                // `::LoadError`: root-anchored, no parent.
                (None, Some(path)) if path.parent().is_none() => {
                    path.name().map(|n| n.as_slice().to_vec())
                }
                _ => None,
            };
            if name.is_some_and(|n| {
                matches!(n.as_slice(), b"LoadError" | b"ScriptError" | b"Exception")
            }) {
                return true;
            }
        }
        clause = rescue.subsequent();
    }
    false
}

/// A build-time-decidable platform guard's answer, or `None` for every other
/// condition. Deliberately much narrower than [`crate::guard_fold`]: this runs
/// while FILES ARE STILL BEING LOADED, before any `Compiler` exists, so it can
/// only read constants the build bakes in.
///
/// The one shape it needs is the one gems actually write:
/// `RUBY_PLATFORM =~ /mswin|mingw|windows/`, and its `match?` and `==`
/// spellings. That decides whether a windows-only file is part of this program
/// -- mixlib-shellout requires one from inside `class ShellOut`, and its
/// `:dword` FFI types exist nowhere else.
fn baked_constant(node: &ruby_prism::Node<'_>) -> Option<&'static str> {
    let name = node.as_constant_read_node()?;
    match name.name().as_slice() {
        b"RUBY_PLATFORM" => Some(env!("ZEO_RUBY_PLATFORM")),
        // Mirrors `zeo_rt::bootstrap`'s `ENGINE = "ruby"`: zeo targets CRuby
        // semantics, so an engine-gated require is statically decidable.
        b"RUBY_ENGINE" => Some("ruby"),
        b"RUBY_VERSION" | b"RUBY_ENGINE_VERSION" => Some(zeo_abi::RUBY_VERSION),
        _ => None,
    }
}

/// A build-time-known STRING subject: a baked constant, or a
/// `RbConfig::CONFIG['host_os']`-style read of an entry this build bakes --
/// the older spelling of the same platform question, answered from the very
/// value the generated program's `RbConfig::CONFIG` will hold.
fn baked_subject(node: &ruby_prism::Node<'_>) -> Option<&'static str> {
    if let Some(s) = baked_constant(node) {
        return Some(s);
    }
    let key = crate::lower::defs::rbconfig_key(node)?;
    crate::guard_fold::rbconfig_string(&key)
}

/// Whether this build's platform is a windows one -- the answer every
/// spelling of the oldest platform test resolves to.
fn build_is_windows() -> bool {
    let plat = env!("ZEO_RUBY_PLATFORM");
    ["mswin", "mingw", "windows"]
        .iter()
        .any(|w| plat.contains(w))
}

/// `Some(<baked constant> matches <pattern>)` for `RUBY_PLATFORM =~ /mswin|
/// mingw|windows/` and its `match?` spelling, either operand order.
///
/// Only a pattern of `|`-separated LITERAL text folds, so the match is a
/// substring test that cannot disagree with a regexp engine -- the same rule,
/// and the same reason, as `guard_fold::literal_alternatives`.
fn platform_match(node: &ruby_prism::Node<'_>) -> Option<bool> {
    let call = node.as_call_node()?;
    if !matches!(call.name().as_slice(), b"=~" | b"match?") {
        return None;
    }
    let recv = call.receiver()?;
    let mut args = call.arguments()?.arguments().iter();
    let (arg, None) = (args.next()?, args.next()) else {
        return None;
    };
    let (subject, pattern) = match baked_subject(&recv) {
        Some(s) => (s, arg),
        None => (baked_subject(&arg)?, recv),
    };
    literal_alt_match(subject, &pattern)
}

/// `Some(subject matches re)` for a regexp of `|`-separated LITERAL text --
/// a substring test that cannot disagree with a regexp engine. Anything with
/// syntax or flags is `None`.
fn literal_alt_match(subject: &str, pattern: &ruby_prism::Node<'_>) -> Option<bool> {
    let re = pattern.as_regular_expression_node()?;
    if re.is_ignore_case() || re.is_extended() || re.is_multi_line() {
        return None;
    }
    let src = String::from_utf8_lossy(re.unescaped()).into_owned();
    const SYNTAX: &[char] = &[
        '\\', '^', '$', '.', '[', ']', '(', ')', '*', '+', '?', '{', '}',
    ];
    if src.is_empty() || src.contains(SYNTAX) {
        return None;
    }
    let alts: Vec<&str> = src.split('|').collect();
    if alts.iter().any(|a| a.is_empty()) {
        return None;
    }
    Some(alts.iter().any(|a| subject.contains(a)))
}

impl<'pr> ruby_prism::Visit<'pr> for RequireCollector<'pr> {
    fn visit_branch_node_enter(&mut self, node: ruby_prism::Node<'pr>) {
        if let Some(call) = node.as_call_node()
            && call.receiver().is_none()
            && matches!(call.name().as_slice(), b"require" | b"require_relative")
        {
            if call.name().as_slice() == b"require_relative"
                && self.load_error_rescues > 0
                && let Some(again) = node.as_call_node()
            {
                self.optional_rel.push(again);
            }
            // A method-body `require_relative` under an UNDECIDED guard is
            // conditional twice over -- CRuby loads it only when the method
            // runs AND the guard passes -- so it takes the gated-unit route,
            // never the eager splice (puppet's suidmanager loads
            // `windows/user` this way, and eager splicing compiled the
            // windows-only FFI vocabulary into every build).
            if self.runtime_cond > 0
                && (self.defs == 0 || call.name().as_slice() == b"require_relative")
                && let Some(again) = node.as_call_node()
            {
                self.conditional.push(again);
            }
            if self.defs == 0 {
                if self.class_depth > 0
                    && self.runtime_cond == 0
                    && let Some(again) = node.as_call_node()
                {
                    self.nested.push(again);
                }
                self.calls.push(call);
            } else if call.name().as_slice() == b"require_relative" {
                if self.runtime_cond == 0 {
                    self.lazy.push(call);
                }
            } else {
                self.deferred.push(call);
            }
        }
    }

    // Only the `begin` BODY is protected by its rescues; the rescue, else and
    // ensure clauses run outside that protection and visit at the old depth.
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        let optional = rescues_load_error(node) as u32;
        if let Some(stmts) = node.statements() {
            self.load_error_rescues += optional;
            self.visit(&stmts.as_node());
            self.load_error_rescues -= optional;
        }
        if let Some(r) = node.rescue_clause() {
            self.visit(&r.as_node());
        }
        if let Some(e) = node.else_clause() {
            self.visit(&e.as_node());
        }
        if let Some(en) = node.ensure_clause() {
            self.visit(&en.as_node());
        }
    }

    // A method body does not run at load time, so a `require` there names a
    // lazy LIBRARY: CRuby loads it only when the method is called, and eager
    // splicing gets both the order and the binary size wrong. Descend, but keep
    // those requires out of the splice. A `require_relative` is exempt -- it
    // names a file of this same program, not a library boundary, and it must
    // stay loaded for `def x; require_relative "part"; end` to work at all.
    // `def self.x` and a `class << self` body are the same case: both are
    // `DefNode`s.
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        self.defs += 1;
        ruby_prism::visit_def_node(self, node);
        self.defs -= 1;
    }

    // A `class`/`module` body runs at load time, statement by statement --
    // a require written there must have DECLARED before the statements below
    // it lower. See `RequireCollector::nested`.
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        self.class_depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.class_depth -= 1;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        self.class_depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.class_depth -= 1;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'pr>) {
        self.class_depth += 1;
        ruby_prism::visit_singleton_class_node(self, node);
        self.class_depth -= 1;
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
        // An UNDECIDED guard means either branch may or may not run: its
        // requires are collected as conditional so they load only when the
        // guard actually passes, the way CRuby runs them.
        let bump = (guard.is_none()) as u32;
        if guard != Some(false)
            && let Some(stmts) = node.statements()
        {
            self.runtime_cond += bump;
            self.visit(&stmts.as_node());
            self.runtime_cond -= bump;
        }
        if guard != Some(true)
            && let Some(sub) = node.subsequent()
        {
            self.runtime_cond += bump;
            self.visit(&sub);
            self.runtime_cond -= bump;
        }
    }

    // `unless C` runs `statements` when C is FALSE and `else_clause` when TRUE
    // -- the mirror of `visit_if_node`.
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        self.visit(&node.predicate());
        let guard = eval_static_guard(&node.predicate());
        let bump = (guard.is_none()) as u32;
        if guard != Some(true)
            && let Some(stmts) = node.statements()
        {
            self.runtime_cond += bump;
            self.visit(&stmts.as_node());
            self.runtime_cond -= bump;
        }
        if guard != Some(false)
            && let Some(els) = node.else_clause()
        {
            self.runtime_cond += bump;
            self.visit(&els.as_node());
            self.runtime_cond -= bump;
        }
    }

    // `case RUBY_ENGINE when 'jruby'` is the third spelling of the same
    // platform gate (psych requires `psych_jars` under exactly this one), so
    // it prunes the same way the `if` does. Arms are decided in document
    // order, as ruby tests them: a decided-false arm's body is skipped, a
    // decided-true arm ends the walk (later arms and the `else` never run),
    // and any undecidable condition keeps its arm live without killing the
    // arms after it. A subject the build doesn't bake descends exactly as
    // before.
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        let subject = node.predicate().as_ref().and_then(baked_subject);
        let Some(subject) = subject else {
            // An undecided subject: every arm may or may not run.
            self.runtime_cond += 1;
            ruby_prism::visit_case_node(self, node);
            self.runtime_cond -= 1;
            return;
        };
        if let Some(pred) = node.predicate() {
            self.visit(&pred);
        }
        for cond in node.conditions().iter() {
            let Some(when) = cond.as_when_node() else {
                // Not a shape this prunes; fall back to full descent of the
                // remainder by visiting the node itself.
                self.visit(&cond);
                continue;
            };
            // `Some(false)` until a condition matches or declines to answer.
            let mut arm = Some(false);
            for c in when.conditions().iter() {
                self.visit(&c);
                match literal_when_match(subject, &c) {
                    Some(true) => {
                        arm = Some(true);
                        break;
                    }
                    Some(false) => {}
                    None => arm = None,
                }
            }
            match arm {
                Some(false) => continue,
                None => {
                    if let Some(stmts) = when.statements() {
                        self.runtime_cond += 1;
                        self.visit(&stmts.as_node());
                        self.runtime_cond -= 1;
                    }
                }
                Some(true) => {
                    if let Some(stmts) = when.statements() {
                        self.visit(&stmts.as_node());
                    }
                    return;
                }
            }
        }
        if let Some(els) = node.else_clause() {
            self.visit(&els.as_node());
        }
    }
}

/// Whether one `when` condition matches the baked subject: a string literal
/// by equality, a `|`-literal regexp by substring. Anything else is `None`.
fn literal_when_match(subject: &str, cond: &ruby_prism::Node<'_>) -> Option<bool> {
    if let Some(s) = cond.as_string_node() {
        return Some(s.unescaped() == subject.as_bytes());
    }
    literal_alt_match(subject, cond)
}

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
        return match (
            eval_static_guard(&and.left()),
            eval_static_guard(&and.right()),
        ) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        };
    }
    if let Some(or) = node.as_or_node() {
        return match (
            eval_static_guard(&or.left()),
            eval_static_guard(&or.right()),
        ) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        };
    }
    if let Some(b) = platform_match(node) {
        return Some(b);
    }
    // `defined?(RUBY_ENGINE)` -- the interpreter always defines it, so the
    // question is only ever "am I old enough to have it", answered at build
    // time. binding_of_caller opens on `defined?(RUBY_ENGINE) && RUBY_ENGINE ==
    // "ruby"`, and without this the `and` stayed unfolded and every `elsif`
    // branch loaded -- including the JRuby one, whose `class
    // org::jruby::runtime::ThreadContext` no CRuby build could ever compile.
    if let Some(d) = node.as_defined_node()
        && baked_constant(&d.value()).is_some()
    {
        return Some(true);
    }
    // `if File::ALT_SEPARATOR` -- the oldest "am I on windows" test there is,
    // and how sys-filesystem picks its half. The constant is `"\\"` on windows
    // and `nil` everywhere else, so its TRUTH is a property of the build.
    if let Some(path) = node.as_constant_path_node()
        && path
            .name()
            .is_some_and(|n| n.as_slice() == b"ALT_SEPARATOR")
        && path
            .parent()
            .and_then(|p| {
                p.as_constant_read_node()
                    .map(|c| c.name().as_slice() == b"File")
            })
            .unwrap_or(false)
    {
        return Some(build_is_windows());
    }
    // `defined?(JRUBY_VERSION)` and its family: a version constant only
    // ANOTHER interpreter defines. zeo targets CRuby, where the answer is a
    // build-time fact -- no gem defines a foreign engine's version constant
    // on CRuby, and thread_safe/concurrent-ruby gate whole require graphs on
    // exactly this test.
    if let Some(d) = node.as_defined_node()
        && let Some(read) = d.value().as_constant_read_node()
        && matches!(
            read.name().as_slice(),
            b"JRUBY_VERSION" | b"RUBINIUS_VERSION" | b"MACRUBY_VERSION" | b"MRUBY_VERSION"
        )
    {
        return Some(false);
    }
    if let Some(call) = node.as_call_node() {
        let name = call.name().as_slice();
        if name == b"!" && call.arguments().is_none() {
            return call
                .receiver()
                .and_then(|r| eval_static_guard(&r))
                .map(|b| !b);
        }
        if matches!(name, b"==" | b"!=")
            && let (Some(recv), Some(args)) = (call.receiver(), call.arguments())
        {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let [only] = arg_list.as_slice() {
                let eq = baked_string_eq(&recv, only).or_else(|| baked_string_eq(only, &recv));
                if let Some(eq) = eq {
                    return Some(if name == b"==" { eq } else { !eq });
                }
            }
        }
        if let Some(b) = platform_predicate(&call) {
            return Some(b);
        }
    }
    None
}

/// `Some(<baked constant> == lit)` when `a` reads a build-baked constant and
/// `b` is a string literal; `None` otherwise. Order-sensitive -- the caller
/// tries both operand orders so `RUBY_ENGINE == 'x'` and `'x' == RUBY_ENGINE`
/// both resolve. Baking the SUBJECT rather than one constant name is what
/// decides `RUBY_PLATFORM == "java"` too, JRuby's other spelling.
fn baked_string_eq(a: &ruby_prism::Node<'_>, b: &ruby_prism::Node<'_>) -> Option<bool> {
    let subject = baked_subject(a)?;
    Some(b.as_string_node()?.unescaped() == subject.as_bytes())
}

/// The zero-argument platform predicates whose answer is a property of the
/// build: `Gem.win_platform?`, and the `FFI::Platform.windows?/mac?/unix?`
/// family zeo's own ffi gem provides.
fn platform_predicate(call: &ruby_prism::CallNode<'_>) -> Option<bool> {
    if call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    let recv = call.receiver()?;
    let name = call.name().as_slice();
    if let Some(c) = recv.as_constant_read_node()
        && c.name().as_slice() == b"Gem"
        && name == b"win_platform?"
    {
        return Some(build_is_windows());
    }
    let path = recv.as_constant_path_node()?;
    if path.name()?.as_slice() != b"Platform"
        || !path
            .parent()
            .and_then(|p| {
                p.as_constant_read_node()
                    .map(|c| c.name().as_slice() == b"FFI")
            })
            .unwrap_or(false)
    {
        return None;
    }
    match name {
        b"windows?" => Some(build_is_windows()),
        b"mac?" => Some(env!("ZEO_RUBY_PLATFORM").contains("darwin")),
        b"unix?" => Some(!build_is_windows()),
        _ => None,
    }
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
    } else if let Some(call) = node.as_call_node()
        && call.receiver().is_none()
        && call.name().as_slice() == b"autoload"
    {
        out.push(call);
    }
}

/// The `Dir.glob`/`Dir[]` calls whose block just requires each match --
/// `Dir.glob("#{__dir__}/smtp/auth_*.rb") { |r| require_relative r }`, how
/// net/smtp loads its authenticators and how rubygems loads its plugins.
/// Collected exactly like `collect_autoloads`, and expanded the same way: the
/// pattern is a compile-time fact, so the matching files are spliced as
/// ordinary requires.
fn collect_glob_requires<'a>(node: &ruby_prism::Node<'a>, out: &mut Vec<ruby_prism::CallNode<'a>>) {
    if let Some(stmts) = node.as_statements_node() {
        for n in stmts.body().iter() {
            collect_glob_requires(&n, out);
        }
    } else if let Some(m) = node.as_module_node() {
        if let Some(body) = m.body() {
            collect_glob_requires(&body, out);
        }
    } else if let Some(c) = node.as_class_node() {
        if let Some(body) = c.body() {
            collect_glob_requires(&body, out);
        }
    } else if let Some(call) = node.as_call_node() {
        out.push(call);
    }
}

/// The files a glob-and-require pattern matches, sorted, as absolute paths.
///
/// Deliberately narrow: only a `*` in the FINAL segment is expanded (no `**`,
/// no character classes, no brace expansion), because that is the shape the
/// require idiom uses -- `auth_*.rb`, `*.rb` -- and a partial implementation of
/// the rest would be worse than none. A pattern with a wildcard anywhere else
/// matches nothing here and lowers as ordinary runtime code.
fn expand_glob(pattern: &str) -> Vec<String> {
    let (dir, leaf) = match pattern.rsplit_once('/') {
        Some((d, l)) => (Path::new(d), l),
        None => return Vec::new(),
    };
    if dir.to_string_lossy().contains('*') || !leaf.contains('*') {
        return Vec::new();
    }
    let (prefix, suffix) = leaf.split_once('*').expect("checked above");
    if suffix.contains('*') {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let matches = name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix);
            matches.then(|| dir.join(&name).to_string_lossy().into_owned())
        })
        .collect();
    // Sorted so the splice order is deterministic across filesystems -- the
    // requires land in the program in a fixed sequence, as `Dir.glob`'s own
    // (sorted by default since ruby 3.0) does.
    out.sort();
    out
}

/// One recognized glob-and-require: the shell pattern (with `__dir__` already
/// resolved against `dir`) and the require flavor its block uses.
struct GlobRequire {
    pattern: String,
    flavor: String,
}

/// Whether `call` is a glob-and-require. Accepts the direct block form and the
/// `.each`/`.sort.each` chains, which are the spellings in the wild; the block
/// must do nothing but require its own parameter, so nothing else is silently
/// dropped. `None` for every other `Dir.glob`, which lowers as ordinary code.
fn glob_require_call(call: &ruby_prism::CallNode<'_>, dir: Option<&Path>) -> Option<GlobRequire> {
    // The block rides on the OUTERMOST call; the pattern is under whatever
    // `.each`/`.sort` chain leads back to `Dir.glob`.
    let block = call.block()?.as_block_node()?;
    let pattern = dir_glob_pattern(call, dir)?;

    // The block must be exactly `require_relative <its own param>`.
    let params = block.parameters()?.as_block_parameters_node()?;
    let param = params.parameters()?.requireds().iter().next()?;
    let param = param.as_required_parameter_node()?;
    let body = block.body()?;
    let body = body.as_statements_node()?;
    let stmts: Vec<_> = body.body().iter().collect();
    let [only] = stmts.as_slice() else {
        return None;
    };
    let req = only.as_call_node()?;
    let flavor = String::from_utf8_lossy(req.name().as_slice()).into_owned();
    if req.receiver().is_some() || !matches!(flavor.as_str(), "require" | "require_relative") {
        return None;
    }
    let req_args: Vec<_> = req
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [req_arg] = req_args.as_slice() else {
        return None;
    };
    (req_arg.as_local_variable_read_node()?.name().as_slice() == param.name().as_slice())
        .then_some(GlobRequire { pattern, flavor })
}

/// The shell pattern behind `Dir.glob(...)` / `Dir[...]`, peeling any
/// `.each`/`.sort`/`.to_a` chain in front of it.
fn dir_glob_pattern(call: &ruby_prism::CallNode<'_>, dir: Option<&Path>) -> Option<String> {
    match call.name().as_slice() {
        b"each" | b"sort" | b"to_a" => {
            let recv = call.receiver()?;
            dir_glob_pattern(&recv.as_call_node()?, dir)
        }
        b"glob" | b"[]" => {
            if call.receiver()?.as_constant_read_node()?.name().as_slice() != b"Dir" {
                return None;
            }
            let args: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            let [pattern] = args.as_slice() else {
                return None;
            };
            glob_pattern_text(pattern, dir)
        }
        _ => None,
    }
}

/// A glob pattern's text, if every piece is known at compile time: a plain
/// string literal, or an interpolation whose only computed part is `__dir__`
/// (`"#{__dir__}/smtp/auth_*.rb"`), which resolves to the requiring file's own
/// directory.
fn glob_pattern_text(node: &ruby_prism::Node<'_>, dir: Option<&Path>) -> Option<String> {
    if let Some(s) = node.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    let interp = node.as_interpolated_string_node()?;
    let mut out = String::new();
    for part in interp.parts().iter() {
        if let Some(s) = part.as_string_node() {
            out.push_str(&String::from_utf8_lossy(s.unescaped()));
            continue;
        }
        let embedded = part.as_embedded_statements_node()?;
        let stmts: Vec<_> = embedded.statements()?.body().iter().collect();
        let [only] = stmts.as_slice() else {
            return None;
        };
        let call = only.as_call_node()?;
        if call.receiver().is_some() || call.name().as_slice() != b"__dir__" {
            return None;
        }
        out.push_str(dir?.to_str()?);
    }
    Some(out)
}

/// Where the main script's `DATA` starts, if it has an `__END__` marker.
///
/// prism's `data_loc` spans the marker AND the bytes after it, so the offset
/// is past `__END__` plus its line terminator -- CRuby skips at most one `\r`
/// and one `\n` there (`ruby.c:2287`), never more, so a blank line after the
/// marker is DATA's first line.
///
/// A source with no `__END__` answers `None`, which is what leaves `DATA`
/// undefined rather than empty.
fn data_section(
    result: &ruby_prism::ParseResult<'_>,
    path: &std::path::Path,
) -> Option<crate::hir::DataSection> {
    const MARKER: usize = "__END__".len();
    let loc = result.data_loc()?;
    let after_marker = &result.as_slice(&loc)[MARKER..];
    let terminator = usize::from(after_marker.starts_with(b"\r")) + 1;
    Some(crate::hir::DataSection {
        // Absolute: the compiled binary can run from anywhere, and this path is
        // reopened at startup.
        path: std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string(),
        offset: (loc.start_offset() + MARKER + terminator) as u64,
    })
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
        let line = 1 + source.as_bytes()[..upto]
            .iter()
            .filter(|&&b| b == b'\n')
            .count() as u32;
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
/// A message is safe to forward only when NO verbose-level diagnostic renders
/// the same text. `literal in condition` is the shape that fails that test:
/// prism's default and verbose rows share the format `%sliteral in %s`, so a
/// message-keyed filter cannot tell them apart, and forwarding a verbose-only
/// warning by mistake is worse than forwarding none. Reading the real level
/// would need the binding to expose `pm_diagnostic_t::level`.
///
/// The two shapes below have no such twin: `equal_in_conditional` (`= literal`
/// in a conditional -- the classic `if x = 1` typo) is spelled two ways, one
/// per parser version, and both rows are default-level.
fn is_default_level(message: &str) -> bool {
    message.starts_with("key ") && message.contains(" is duplicated and overwritten on line ")
        || message.ends_with("literal' in conditional, should be ==")
}

#[cfg(test)]
mod tests {
    use super::read_source;

    /// Roots first (Gemfile order), then breadth-first down the edges with
    /// each gem's deps in name order -- the dependent always outranks its
    /// dependency, expressing Bundler's last-activated-wins as first-match.
    #[test]
    fn lockfile_precedence_ranks_dependents_ahead_of_their_deps() {
        let lock = crate::parse::lockfile::parse(
            "GEM\n  remote: https://rubygems.org/\n  specs:\n    hashie (5.0.0)\n    pronto (0.11.4)\n      rugged (>= 0.23.0)\n      thor (>= 0.20.3)\n    rugged (1.9.0)\n    thor (1.4.0)\n\nDEPENDENCIES\n  pronto\n",
        )
        .unwrap();
        let rank = super::lockfile_precedence(&lock);
        assert_eq!(rank["pronto"], 0);
        // pronto's deps follow, name-sorted.
        assert_eq!(rank["rugged"], 1);
        assert_eq!(rank["thor"], 2);
        // A locked gem nothing depends on still ranks, after the graph.
        assert_eq!(rank["hashie"], 3);
    }

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("zeo-read-source-{name}.rb"));
        std::fs::write(&path, bytes).expect("writing the fixture");
        path
    }

    #[test]
    fn an_invalid_byte_in_a_comment_reads_as_a_same_width_placeholder() {
        let path = write("comment", b"# (c) \xa9 2007\nputs 1\n");
        let text = read_source(&path).expect("prism accepts a comment's stray byte");
        assert_eq!(text, "# (c) ? 2007\nputs 1\n");
    }

    #[test]
    fn an_invalid_byte_in_code_is_the_syntax_error_cruby_reports() {
        let path = write("code", b"s = \"a\xa9b\"\n");
        let err = read_source(&path).expect_err("CRuby rejects this file");
        assert!(
            err.contains("invalid multibyte character"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn a_truncated_multibyte_sequence_at_end_of_file_is_still_one_byte_per_byte() {
        // `error_len() == None`: the bytes run out mid-sequence, so the whole
        // remainder is invalid and each byte still owes one placeholder.
        let path = write("truncated", b"puts 1 # \xe0\xa4");
        let text = read_source(&path).expect("prism accepts a comment's stray bytes");
        assert_eq!(text, "puts 1 # ??");
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    use ruby_prism::Visit;

    /// The static answer for `if <src> ...`'s predicate.
    fn guard(pred: &str) -> Option<bool> {
        let src = format!("if {pred}\n  1\nend\n");
        let res = ruby_prism::parse(src.as_bytes());
        let root = res.node();
        let prog = root.as_program_node().unwrap();
        let first = prog.statements().body().iter().next().unwrap();
        eval_static_guard(&first.as_if_node().unwrap().predicate())
    }

    /// The features the collector would SPLICE from `src` (load-time
    /// requires; deferred/lazy are not the question here).
    fn spliced(src: &str) -> Vec<String> {
        let res = ruby_prism::parse(src.as_bytes());
        let mut c = RequireCollector::default();
        c.visit(&res.node());
        c.calls
            .iter()
            .filter_map(|call| {
                let args = call.arguments()?;
                let first = args.arguments().iter().next()?;
                Some(String::from_utf8_lossy(first.as_string_node()?.unescaped()).into_owned())
            })
            .collect()
    }

    /// Every spelling of "am I another engine" answers `false` at build time,
    /// and the windows family answers whatever this build is.
    #[test]
    fn the_platform_guards_fold_to_build_facts() {
        assert_eq!(guard("RUBY_ENGINE == 'jruby'"), Some(false));
        assert_eq!(guard("RUBY_PLATFORM == 'java'"), Some(false));
        assert_eq!(guard("'java' == RUBY_PLATFORM"), Some(false));
        assert_eq!(guard("defined?(JRUBY_VERSION)"), Some(false));
        assert_eq!(guard("defined?(RUBINIUS_VERSION)"), Some(false));
        assert_eq!(guard("Gem.win_platform?"), Some(build_is_windows()));
        assert_eq!(guard("FFI::Platform.windows?"), Some(build_is_windows()));
        assert_eq!(guard("FFI::Platform.unix?"), Some(!build_is_windows()));
        assert_eq!(
            guard("RbConfig::CONFIG['host_os'] =~ /mswin|mingw/"),
            Some(build_is_windows())
        );
        // Runtime state stays three-valued.
        assert_eq!(guard("ENV['FAST']"), None);
        assert_eq!(guard("defined?(SomeGemConstant)"), None);
    }

    /// psych's shape: the JRuby arm of a `case RUBY_ENGINE` must not splice
    /// -- its target is another engine's native code.
    #[test]
    fn a_case_on_a_baked_subject_prunes_its_dead_arms() {
        let live = spliced(
            "case RUBY_ENGINE\n\
             when 'jruby' then require 'psych_jars'\n\
             when 'ruby' then require 'psych_native'\n\
             else require 'psych_fallback'\n\
             end\n",
        );
        assert_eq!(live, vec!["psych_native"]);

        // No arm matches: only the `else` runs.
        let fallback = spliced(
            "case RUBY_PLATFORM\n\
             when /java/ then require 'a'\n\
             else require 'b'\n\
             end\n",
        );
        assert_eq!(fallback, vec!["b"]);
    }

    /// A subject the build does not bake descends exactly as before, and an
    /// undecidable arm keeps the arms after it live.
    #[test]
    fn an_undecided_case_keeps_every_arm_live() {
        let all = spliced(
            "case adapter\n\
             when 'jruby' then require 'a'\n\
             else require 'b'\n\
             end\n",
        );
        assert_eq!(all, vec!["a", "b"]);

        let mixed = spliced(
            "case RUBY_ENGINE\n\
             when computed then require 'a'\n\
             when 'ruby' then require 'b'\n\
             end\n",
        );
        assert_eq!(mixed, vec!["a", "b"]);
    }
}
