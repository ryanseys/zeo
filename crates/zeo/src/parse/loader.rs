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

use crate::hir::{Hir, HirNode, LoadedFile, NodeId};
use crate::lower::context::{BindingsFrame, SourceFileFrame, current_box_binding};
use crate::lower::features::{canonical_ext_feature, is_builtin_feature};
use crate::lower::{PResult, lower_node};
use crate::lower_error::LowerError;
use crate::rename;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

mod cext;
mod gems;
mod glob;
mod resolve;
mod scan;
mod shims;
mod splice;
mod static_guards;
use gems::*;
use glob::*;
use resolve::*;
use scan::*;
use shims::*;
use static_guards::{baked_subject, eval_static_guard, literal_when_match};

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
    /// Every single-file unit already made, `canonical path -> (index into
    /// `LoaderState::feature_units`, the constants its body assigns)`. Demands for
    /// one file arrive in DIFFERENT ROUNDS of the materialize loop -- rack's
    /// guarded `require_relative "../lib/rack/media_type"` in round one, and
    /// the `autoload :MediaType, "rack/media_type"` inside `rack.rb` only
    /// once rack.rb itself is spliced as a unit in round two. The second
    /// demand must reach the unit the first one made, or its spelling
    /// resolves to nothing and the autoload silently never runs.
    single_units: HashMap<PathBuf, usize>,
    /// Canonical paths some site requires under a runtime-undecided guard.
    /// A file like this is a UNIT and never an inline splice, at every site
    /// that names it -- including the unguarded ones. `abbrev` is required
    /// both from inside a block and at top level in
    /// `tests/require_from_a_block_and_top_level.rb`: splicing the second
    /// site inline would run the body there, at a position the guarded site
    /// (which comes first) has already passed.
    unit_only_targets: HashSet<PathBuf>,
    /// External-store gems zeo can't provide, `name -> reason`:
    /// a `require` of one fails with the store's precise reason (which native
    /// layout, why) instead of the generic "cannot load such file".
    store_exclusions: HashMap<String, String>,
    /// External-store gems whose C is shipped as SOURCE, by gem name. A
    /// `require` that reaches one builds it -- see [`Loader::build_cext`].
    native_exts: HashMap<String, super::gem_store::NativeExt>,
    /// Extensions already built in this compile, `gem name -> (library,
    /// init)`. A gem's Ruby half often requires its native half from more
    /// than one file, and building twice would relink for nothing.
    built_cexts: HashMap<String, (String, String)>,
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
    opts: &crate::CompileOptions,
) -> PResult<(Vec<NodeId>, Vec<crate::gem_report::GemRecord>)> {
    let input_path = opts.input_path.as_deref();
    let line_offset = opts.line_offset;
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
    // `LoaderState::search_roots`) -- as given, the way `ruby -I` reports them.
    hir.loader.search_roots = opts
        .load_roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let bundled_dir = bundled_gems_dir();
    let mut loader = Loader {
        roots: opts.load_roots.clone(),
        // The gems zeo itself ships are ALWAYS discoverable, appended
        // last so any caller-supplied dir shadows them (first-name-wins).
        // They are part of the compiler the way CRuby's rubylibdir is part of
        // ruby -- not ambient machine state a caller opts into. A caller that
        // forgot them would resolve a two-half gem's NATIVE half and silently
        // miss its Ruby half, which is how `StringScanner::Error` would go
        // missing and turn a raise into a panic.
        packages: discover_packages(
            &opts
                .package_dirs
                .iter()
                .cloned()
                .chain(bundled_dir.clone())
                .collect::<Vec<_>>(),
            bundled_dir.as_deref(),
        )?,
        required: HashSet::new(),
        splicing: Vec::new(),
        in_unit_sweep: false,
        single_units: HashMap::new(),
        unit_only_targets: HashSet::new(),
        store_exclusions: HashMap::new(),
        native_exts: HashMap::new(),
        built_cexts: HashMap::new(),
        gem_records: Vec::new(),
        gem_names: std::collections::HashSet::new(),
        require_memo: std::cell::RefCell::new(HashMap::new()),
        ambiguous_features: std::cell::RefCell::new(HashMap::new()),
    };
    // Which gated builtins ALSO have a vendored Ruby half. Probed once here
    // rather than asked per call site, because the answer is a property of
    // the tree and the question is asked from three places -- the deferred
    // loop, the splice, and the fold in `lower::calls`, which has no
    // resolver of its own.
    //
    // `tmpdir` is the shape: the ext supplies the gated constants and the
    // GEM supplies `Dir::Tmpname`, so a program that requires it from a
    // method body needs BOTH, and folding the call away left the second
    // half unloaded. A PURE builtin (`digest`) resolves to nothing and is
    // not here, so its require still folds.
    for feature in crate::lower::features::builtin_feature_names() {
        if matches!(loader.resolve_require(feature), Ok(Some(_))) {
            hir.loader.dual_homed_requires.insert(feature.to_string());
        }
    }
    // The external gem store: store dirs (`--gem-path`/`GEM_PATH`) plus a
    // lockfile (via `--bundle-gemfile`/`BUNDLE_GEMFILE`) add the pure-Ruby
    // gems zeo can compile as extra roots, and record a disclosure for every
    // gem it satisfies natively or can't provide. Store gems are APPENDED, so
    // a bundled zeo gem of the same name shadows them (first-name-wins), and
    // never override the compiler's own libraries.
    if let Some(lock) = opts.lockfile.as_deref()
        && !opts.gem_paths.is_empty()
    {
        let parsed = super::lockfile::parse_file(lock)?;
        let resolution = super::gem_store::resolve(&opts.gem_paths, &parsed)?;
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
        for ext in resolution.native_exts {
            loader.native_exts.insert(ext.name.clone(), ext);
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
    if let Some(root) = &opts.root_gem
        && let Some(pos) = loader.packages.iter().position(|g| g.name == root.name())
    {
        let subject = loader.packages.remove(pos);
        loader.packages.insert(0, subject);
    }
    let result = ruby_prism::parse(source.as_bytes());
    // What `__FILE__` and every span report. A source with no path on
    // disk can still have a NAME -- a run-time `eval`'s is
    // `(eval at f.rb:14)`, which is what its backtrace rows must say.
    let named = opts.file_name.as_deref().or(input_path);
    let main_name = named.map_or_else(|| "-e".to_string(), |p| p.display().to_string());
    // A snippet is parsed in a method's context, so `yield` is valid there
    // -- CRuby says so by handing prism a scope, and prism's own check is
    // the one thing that changes (the node is built either way). Every
    // other jump keyword stays refused: CRuby refuses those in an `eval`
    // too.
    let ignore = |m: &str| opts.mode.is_eval() && m == "Invalid yield";
    if let Some(err) = result.errors().find(|e| !ignore(e.message())) {
        return Err(LowerError::syntax_reported(
            format!("parse error: {}", err.message()),
            crate::parse::syntax_report::report(
                source,
                &result,
                &main_name,
                line_offset as i32 + 1,
                &ignore,
            ),
        ));
    }
    collect_parse_warnings(hir, &result, &main_name, source);
    hir.data_section = input_path.and_then(|p| data_section(&result, p));
    let program = result
        .node()
        .as_program_node()
        .ok_or("expected a top-level ProgramNode")?;
    // The main file is `__FILE__`'s answer for its own statements -- held
    // for exactly this lowering, and restored by the guard's Drop.
    let _file = SourceFileFrame::push(named, line_offset);
    // Span provenance: the main file's name AS GIVEN (matching `__FILE__`),
    // `"-e"` for a pathless source string.
    let main_file = hir.add_file_at(main_name, source, line_offset);
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
    let prev_dir = hir.lowering_dir.take();
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
    while !hir.loader.unit_demand.is_empty() || !hir.loader.single_unit_demand.is_empty() {
        loader.materialize_units(hir)?;
    }
    // Disclose the libraries no position outside a method body required, so
    // zeo left them out. `record_gem` is first-wins, and every real
    // satisfaction is already recorded, so only the truly absent ones land.
    let mut deferred: Vec<&String> = hir.loader.deferred_requires.iter().collect();
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
    // Never for a SNIPPET: the running program already loaded whatever it
    // loads, and a snippet resolves every constant at run time. Splicing
    // the shim into one also RECURSES -- the shim's own module body runs
    // as one more snippet, whose source names `RbConfig` and would splice
    // it again.
    if hir.mode.is_eval() {
        return false;
    }
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
    /// file: its index into `LoaderState::loaded_files`, which is also its
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
                hir.loader.unresolvable_requires.insert(feature);
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
            // NOT exempt for a builtin any more. `splice_feature` resolves
            // a plain `require` FILESYSTEM-FIRST and only falls back to the
            // static ext, precisely so a gem's Ruby half can sit on top of a
            // native one -- and this loop skipped that order entirely, so a
            // DUAL-HOMED feature's gem half was never compiled as a unit.
            // `tmpdir` is the case: the ext gives the constants, the gem
            // gives `Dir::Tmpname`.
            //
            // A builtin with no gem half resolves to nothing here and falls
            // through unchanged, so the exemption it used to need is now
            // just what happens.
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
                hir.loader.single_unit_demand.insert((
                    package.or_else(|| hir.lowering_package.clone()),
                    path,
                    feature.clone(),
                ));
                // A feature that is BOTH a gated builtin and a resolvable
                // gem: the ext half must still activate, and the call must
                // still stand to load the gem half. `is_builtin_feature`
                // alone cannot say this -- a pure builtin like `digest`
                // resolves to nothing here and must keep folding.
                if is_builtin_feature(&feature) {
                    hir.loader.dual_homed_requires.insert(feature.clone());
                }
            }
            hir.loader.deferred_requires.insert(feature);
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
                hir.loader
                    .optional_require_sites
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
                hir.loader
                    .optional_require_sites
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
                    hir.loader
                        .optional_require_sites
                        .insert((file, call.location().start_offset() as u32));
                }
                continue;
            };
            if let Some(file) = hir.lowering_file {
                hir.loader
                    .conditional_require_sites
                    .insert((file, call.location().start_offset() as u32));
                if let Ok(canonical) = path.canonicalize() {
                    self.unit_only_targets.insert(canonical);
                }
                hir.loader.single_unit_demand.insert((
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
                hir.loader
                    .loaded_files
                    .iter()
                    .any(|lf| &lf.canonical == c && lf.is_unit)
            });
            match unit {
                Some((feature, absolute, canonical)) if fresh || registered => {
                    if let Some(file) = hir.lowering_file {
                        hir.loader
                            .conditional_require_sites
                            .insert((file, call.location().start_offset() as u32));
                    }
                    if fresh {
                        for lf in hir
                            .loader
                            .loaded_files
                            .iter_mut()
                            .filter(|lf| lf.canonical == canonical)
                        {
                            lf.is_unit = true;
                        }
                        hir.loader.feature_units.push(crate::hir::FeatureUnit {
                            feature,
                            aliases: Vec::new(),
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
            // ... but not in a SNIPPET: a compile-time box is one the
            // PROGRAM's own file declares, and a snippet minting one would
            // hand back a handle to a box nothing else can reach. Left as
            // an ordinary call, it answers what a run-time `Ruby::Box.new`
            // answers anywhere else in zeo.
            if let Some(lw) = n.as_local_variable_write_node()
                && !hir.mode.is_eval()
                && crate::lower::eval_splice::is_ruby_box_new(&lw.value())
            {
                let lname = String::from_utf8_lossy(lw.name().as_slice()).into_owned();
                hir.boxes += 1;
                let box_id = hir.boxes;
                frame.bind(lname.clone(), box_id);
                // The handle asks the RUN TIME for the box (the `RUBY_BOX`
                // gate is the program's own), so the statement's span is
                // what a refusal reports.
                let span = crate::lower::span_of(hir, &n);
                hir.push_span(span);
                let handle = hir.push(HirNode::BoxHandle(box_id));
                let id = hir.push(HirNode::LocalWrite(lname, handle));
                hir.pop_span();
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
            let mut nested = RequireCollector::default();
            {
                use ruby_prism::Visit as _;
                nested.visit(&n);
            }
            // A `require` of a feature already loaded answers FALSE. The fold
            // that turns a resolvable literal require into a boolean sees only
            // the NAME, so the sites naming a file this compile has already
            // spliced are marked here. Before `lower_node`, because the fold
            // is in it -- and before this statement's own splice, which is
            // what takes the dedup slot the answer is read from. (Two
            // requires of one file inside a SINGLE statement both read the
            // slot as free and both answer true; ruby answers true then
            // false.)
            if let Some(file) = hir.lowering_file {
                let mut here: Vec<PathBuf> = Vec::new();
                for call in &nested.calls {
                    let cname = String::from_utf8_lossy(call.name().as_slice()).into_owned();
                    let Some(target) = self.require_target(hir, result, call, &cname, dir)? else {
                        continue;
                    };
                    // Already spliced by an earlier statement, or named twice
                    // by THIS one -- `p [require_relative("x"),
                    // require_relative("x")]` loads once and answers true
                    // then false, and this statement's own splice has not
                    // happened yet, so the second reads the slot as free.
                    let again = self.required.contains(&(current_box, target.clone()))
                        || here.contains(&target);
                    // A file some guarded site requires is a UNIT, so THIS
                    // site keeps its call too -- whichever runs first loads
                    // it, which is ruby's rule and the only one that puts the
                    // body at the right position for both.
                    if self.unit_only_targets.contains(&target) {
                        hir.loader
                            .conditional_require_sites
                            .insert((file, call.location().start_offset() as u32));
                    }
                    here.push(target);
                    if again {
                        hir.loader
                            .rerequire_sites
                            .insert((file, call.location().start_offset() as u32));
                    }
                }
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
            for Autoload { call, scope } in &autoloads {
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
                    // The constant this target provides. A read of it must
                    // run the unit, and it cannot miss to ask -- so record
                    // the path the emitter gates.
                    if let Some(name) = crate::lower::autoload_const_name(call) {
                        let mut full = scope.clone();
                        full.push(name);
                        hir.loader.autoload_consts.insert(full.join("::"));
                        hir.loader.autoload_features.insert(feature.clone());
                    }
                    hir.loader.single_unit_demand.insert((
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

            // `Dir.glob("#{__dir__}/smtp/auth_*.rb") { |r| require_relative r }`
            // -- net/smtp's authenticators, rubygems' plugins. The pattern is
            // a compile-time fact, so the matches splice here exactly like an
            // ordinary require; the CALL itself still lowers, and finds every
            // match already loaded. Deduped through the shared `required`
            // table.
            //
            // At the statement's own position, not at the end of the file: a
            // dev tree still HAS the source the glob names, so the call really
            // does require each match at run time -- and a splice that landed
            // after it would define everything the file defines twice.
            let mut globs = Vec::new();
            collect_glob_requires(&n, &mut globs);
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

            combined.push(id);
            own.push(id);
        }
        if let Some(idx) = file_idx {
            rename::isolate_file_locals(hir, &own, idx);
        }
        drop(frame);

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
        if name == "require" && hir.loader.unresolvable_requires.contains(&feature) {
            return Ok(None);
        }
        // A SNIPPET (an `eval`, or a file the load path compiled at run
        // time) has no compile-time file context, so `require_relative` has
        // no directory to resolve against here. CRuby resolves it at run
        // time against the CALLING file, which the frame carries -- so the
        // call is left in place for `dynamic_require_relative` rather than
        // failing the compile with "cannot infer basepath".
        if hir.mode.is_eval() && name == "require_relative" && dir.is_none() {
            return Ok(None);
        }
        // The optional-native-half idiom: this exact call site was recorded
        // as missing-but-rescued, so it keeps its CALL and raises a runtime
        // `LoadError` for the rescue to catch. A guard-gated site keeps its
        // call the same way -- its target is a unit the guard may load.
        if let Some(file) = hir.lowering_file {
            let key = (file, call.location().start_offset() as u32);
            if (name == "require_relative" && hir.loader.optional_require_sites.contains(&key))
                || hir.loader.conditional_require_sites.contains(&key)
            {
                return Ok(None);
            }
        }
        // A file some GUARDED site requires is a UNIT, and so is every other
        // site that names it -- whichever runs first loads it. Splicing an
        // unguarded site inline instead runs the body THERE, at a position
        // the guarded site may already have passed: `abbrev` is required
        // from inside a block and again at top level in
        // `tests/require_from_a_block_and_top_level.rb`, and the block runs
        // first.
        if name != "load"
            && let Some(file) = hir.lowering_file
        {
            let resolved = match name {
                "require_relative" => resolve_require_relative(&feature, dir).ok(),
                _ => self
                    .resolve_require(&feature)
                    .ok()
                    .flatten()
                    .map(|(p, _)| p),
            };
            if let Some(target) = resolved.and_then(|p| p.canonicalize().ok())
                && self.unit_only_targets.contains(&target)
            {
                hir.loader
                    .conditional_require_sites
                    .insert((file, call.location().start_offset() as u32));
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
                    hir.loader
                        .conditional_require_sites
                        .insert((file, call.location().start_offset() as u32));
                }
                hir.loader
                    .single_unit_demand
                    .insert((package, path, unit_feature));
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
