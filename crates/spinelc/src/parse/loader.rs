//! Compile-time `require`/`require_relative`/`load` resolution (Phase
//! 14.1): an HIR-level graft, not text surgery. When the FILE-LEVEL
//! statement loop below hits one of the three call shapes (receiver-less,
//! single string-literal argument, direct top-level statement position --
//! anywhere else, `lower_node`'s unconditional rejection fires instead),
//! the target file is resolved, parsed, and lowered into the SAME `Hir`
//! arena, its statements spliced into the requiring file's statement list
//! at the call's position, in document order -- the same contract as the
//! reference spinel's `sp_included_paths` textual splice, with two
//! deliberate improvements over it: dedup + provenance are per-`Hir`
//! (`Hir::loaded_files`) instead of process-global C statics, and each
//! spliced file's top-level locals are renamed for real per-file isolation
//! (see `parse::rename`) where spinel's text splice silently leaks them.
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
//!   first hit wins (CRuby's `-I`-before-everything ordering; Phase 14.2
//!   appends package roots AFTER these).
//! - `require_relative` resolves against the requiring FILE's directory
//!   (never cwd); with no input-path context the error is CRuby's own
//!   "cannot infer basepath".
//! - `load` never appends `.rb` and never dedups: every `load` statement
//!   re-splices the file fresh (top-level side effects re-run, defs
//!   re-register under the ordinary reopening rules, and the per-INSTANCE
//!   local rename gives each execution the fresh local scope real Ruby
//!   gives it). A `load` cycle -- infinite recursion at runtime in real
//!   Ruby -- is a loud compile error here instead.
//! - A missing file is a compile error with CRuby's message (`cannot load
//!   such file -- <name>`), the reference project's halt-the-compile
//!   choice: at AOT-resolution time there is no runtime `rescue LoadError`
//!   to defer to, so the optional-dependency idiom is a LOUD error (the
//!   `require` inside `begin` is rejected as non-top-level first), never a
//!   silent skip.
//!
//! Phase 14.2 adds PACKAGES on top: a `spin.toml`-manifested directory
//! contributing one or more search roots (`require_paths`, default
//! `["lib"]` -- deliberately gemspec-shaped, see `Package`'s docs),
//! searched AFTER every `-I` root, with cross-package feature ambiguity a
//! loud error and first-NAME-wins shadowing across package dirs. `load`
//! deliberately does NOT search package roots (its compile-time uses are
//! project-local; a package's own files arrive via `require`).
//!
//! Documented divergences: `require`'s return value is unobservable (only
//! statement position is accepted; real Ruby returns true/false); `load`'s
//! plain relative names resolve against the roots then the requiring
//! file's directory (real Ruby: `$LOAD_PATH` then cwd -- cwd is
//! meaningless at compile time); `load`'s `wrap` parameter, `.so`/native
//! features, and `~`/`./`-prefixed `require` forms are clean rejections;
//! per-file magic comments (`frozen_string_literal`) and `__END__`/`DATA`
//! are pre-existing unsupported territory, unchanged by splicing.

use super::{lower_node, rename, PResult};
use crate::hir::{Hir, LoadedFile, NodeId};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One `spin.toml` package (Phase 14.2): a named directory contributing one
/// or more `require` search roots. Deliberately gem-shaped (see the plan's
/// Part 12 gems research): a gem reduces to exactly this -- a name plus its
/// `require_paths` (PLURAL, default `["lib"]`) -- so a future "compile
/// against a locked bundle" roots-provider (Gemfile.lock + installed gem
/// store) can produce these same `Package` values with no resolver changes.
pub(super) struct Package {
    name: String,
    /// Absolute, existence-checked root directories, in manifest order.
    roots: Vec<PathBuf>,
    /// `[native] crate = "..."` (Phase 14.3): the WORKSPACE lib crate the
    /// compiled program must link when this package is `require`d --
    /// recorded into `Hir::native_deps` at first resolution into this
    /// package, consumed by `build::build_binary_with_deps` as an extra
    /// `--extern` against the shared `target/` (the same pre-built-rlib
    /// shape `spinel-rt` itself is linked with).
    native_crate: Option<String>,
}

pub(super) struct Loader {
    /// Ordered `-I` search roots (first hit wins, and they win over
    /// packages entirely -- CRuby's own "-I beats even default gems" rule).
    roots: Vec<PathBuf>,
    /// Discovered packages, in package-dir order with first-NAME-wins
    /// shadowing across dirs (see `discover_packages`).
    packages: Vec<Package>,
    /// `require` dedup, keyed `(box_id, canonical path)` -- the box
    /// dimension is always 0 until Phase 14.5 (per-box feature tables are
    /// exactly how real `Ruby::Box` re-executes a file per box, so the key
    /// shape is decided now to make 14.5 additive).
    required: HashSet<(u32, PathBuf)>,
    /// Every file currently being spliced (innermost last) -- a `load`
    /// cycle is the one shape with no natural termination (require's dedup
    /// terminates require cycles), so it's detected here and rejected.
    splicing: Vec<PathBuf>,
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
) -> PResult<Vec<NodeId>> {
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
        packages: discover_packages(package_dirs)?,
        required: HashSet::new(),
        splicing: Vec::new(),
    };
    let result = ruby_prism::parse(source.as_bytes());
    if let Some(err) = result.errors().next() {
        return Err(format!("parse error: {}", err.message()));
    }
    let program = result
        .node()
        .as_program_node()
        .ok_or("expected a top-level ProgramNode")?;
    loader.lower_file_statements(
        hir,
        &result,
        program.statements().body(),
        dir.as_deref(),
        None,
    )
}

impl Loader {
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
    ) -> PResult<Vec<NodeId>> {
        // `combined` is the spliced statement list in document order;
        // `own` is only THIS file's statements -- the rename pass walks
        // `own` alone, so already-renamed child splices (separate roots,
        // unreachable from this file's own subtrees) are never touched
        // twice.
        let mut combined = Vec::new();
        let mut own = Vec::new();
        for n in body.iter() {
            if let Some(call) = n.as_call_node() {
                let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
                if call.receiver().is_none()
                    && matches!(name.as_str(), "require" | "require_relative" | "load")
                {
                    combined.extend(self.lower_require_statement(
                        hir, result, &call, &name, dir, file_idx,
                    )?);
                    continue;
                }
            }
            let id = lower_node(result, hir, &n)?;
            combined.push(id);
            own.push(id);
        }
        if let Some(idx) = file_idx {
            rename::isolate_file_locals(hir, &own, idx);
        }
        Ok(combined)
    }

    /// One recognized require/require_relative/load statement: validate the
    /// shape, resolve the target, splice (or skip, for a deduped require).
    fn lower_require_statement(
        &mut self,
        hir: &mut Hir,
        result: &ruby_prism::ParseResult,
        call: &ruby_prism::CallNode<'_>,
        name: &str,
        dir: Option<&Path>,
        file_idx: Option<usize>,
    ) -> PResult<Vec<NodeId>> {
        if call.block().is_some() {
            return Err(format!("`{name}` doesn't take a block"));
        }
        let arg_list: Vec<_> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        if name == "load" && arg_list.len() == 2 {
            return Err(
                "`load` with a `wrap` argument isn't supported (spike scope) -- a wrap module needs load-time anonymous-module scoping, which doesn't exist yet"
                    .to_string(),
            );
        }
        if arg_list.len() != 1 {
            return Err(format!(
                "`{name}` is only supported with exactly one string-literal argument (spike scope)"
            ));
        }
        // Lower the argument through the ordinary path first (same trick as
        // `eval`'s recognizer): prism's adjacent-literal folding is picked
        // up for free, and the one throwaway node on the accepted path is
        // harmless append-only arena bookkeeping.
        let arg_id = lower_node(result, hir, &arg_list[0])?;
        let Some(feature) = super::literal_string_text(hir, arg_id) else {
            return Err(format!(
                "`{name}` with a non-literal argument isn't supported (spike scope) -- the target must be resolvable at compile time, e.g. `{name} \"some/feature\"`"
            ));
        };

        // Package attribution (Phase 14.2): a `require` resolved out of a
        // package's roots belongs to that package; `require_relative`/
        // `load` INHERIT the requiring file's package (a package's internal
        // files are part of the package, however they're reached).
        let inherited = file_idx.and_then(|i| hir.loaded_files[i].package.clone());
        let (path, package) = match name {
            "require" => self.resolve_require(&feature)?,
            "require_relative" => (resolve_require_relative(&feature, dir)?, inherited),
            _ => (self.resolve_load(&feature, dir)?, inherited),
        };
        // "Only link the crate when the feature actually fired" (the
        // reference project's `.o` rule): a resolved-into-native-package
        // require records its `[native]` crate as a link dependency.
        if let Some(pkg_name) = &package {
            if let Some(nc) = self
                .packages
                .iter()
                .find(|p| p.name == *pkg_name)
                .and_then(|p| p.native_crate.as_deref())
            {
                hir.add_native_dep(nc);
            }
        }
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("resolving {}: {e}", path.display()))?;

        if name != "load" {
            // Insert BEFORE lowering (CRuby's loading-table rule): a
            // circular require splices nothing and continues, in exactly
            // Ruby's execution order.
            if !self.required.insert((0, canonical.clone())) {
                return Ok(Vec::new());
            }
        }
        self.splice_file(hir, &canonical, file_idx, package)
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
    ) -> PResult<Vec<NodeId>> {
        if self.splicing.iter().any(|p| p == canonical) {
            return Err(format!(
                "`load` cycle detected: {} is already being loaded (real Ruby would recurse forever at runtime; this compiler rejects it at compile time instead)",
                canonical.display()
            ));
        }
        let source = std::fs::read_to_string(canonical)
            .map_err(|e| format!("reading {}: {e}", canonical.display()))?;
        let idx = hir.loaded_files.len();
        hir.loaded_files.push(LoadedFile {
            canonical: canonical.to_path_buf(),
            required_from,
            package,
            box_id: 0,
        });
        let result = ruby_prism::parse(source.as_bytes());
        if let Some(err) = result.errors().next() {
            return Err(format!(
                "{}: parse error: {}",
                canonical.display(),
                err.message()
            ));
        }
        let program = result
            .node()
            .as_program_node()
            .ok_or_else(|| format!("{}: expected a top-level ProgramNode", canonical.display()))?;
        self.splicing.push(canonical.to_path_buf());
        let statements = self
            .lower_file_statements(
                hir,
                &result,
                program.statements().body(),
                canonical.parent(),
                Some(idx),
            )
            .map_err(|e| {
                if e.starts_with(&canonical.display().to_string()) {
                    e // already prefixed by a nested splice
                } else {
                    format!("{}: {e}", canonical.display())
                }
            })?;
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
    fn resolve_require(&self, feature: &str) -> PResult<(PathBuf, Option<String>)> {
        if feature.starts_with("./") || feature.starts_with("../") || feature.starts_with('~') {
            return Err(format!(
                "`require \"{feature}\"`: `./`/`../`/`~` paths resolve against the runtime working directory in real Ruby, which doesn't exist at compile time -- use `require_relative` instead"
            ));
        }
        if is_native_feature(feature) {
            return Err(format!(
                "`require \"{feature}\"`: native (.so/.bundle) features aren't supported (spike scope)"
            ));
        }
        let fname = with_rb_ext(feature);
        if Path::new(&fname).is_absolute() {
            let p = PathBuf::from(&fname);
            if p.is_file() {
                return Ok((p, None));
            }
            return Err(cannot_load(feature));
        }
        for root in &self.roots {
            let cand = root.join(&fname);
            if cand.is_file() {
                return Ok((cand, None));
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
            0 => Err(cannot_load(feature)),
            1 => {
                let (path, pkg) = hits.remove(0);
                Ok((path, Some(pkg.to_string())))
            }
            _ => {
                let names: Vec<&str> = hits.iter().map(|(_, n)| *n).collect();
                Err(format!(
                    "`require \"{feature}\"` is ambiguous: found in multiple packages ({})",
                    names.join(", ")
                ))
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
            return Err(cannot_load(arg));
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
        Err(cannot_load(arg))
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
        return Err("cannot infer basepath -- `require_relative` needs the requiring file's directory (compile from a file path)".to_string());
    };
    if is_native_feature(feature) {
        return Err(format!(
            "`require_relative \"{feature}\"`: native (.so/.bundle) features aren't supported (spike scope)"
        ));
    }
    let cand = dir.join(with_rb_ext(feature));
    if cand.is_file() {
        return Ok(cand);
    }
    Err(cannot_load(&cand.display().to_string()))
}

/// Discovers packages under each dir, in dir order: every subdirectory
/// containing a `spin.toml` manifest is a package (subdirectories without
/// one are silently ignored -- not packages). Within one dir, discovery is
/// name-sorted (deterministic); across dirs, the FIRST occurrence of a
/// package NAME wins entirely (a project-local package shadows a
/// same-named compiler-bundled one -- the "one version per name, nearest
/// wins" rule, the same shape as Bundler's lockfile picking exactly one
/// version). A missing/unreadable packages dir contributes nothing (the
/// CLI passes default candidate locations that often don't exist).
fn discover_packages(package_dirs: &[PathBuf]) -> PResult<Vec<Package>> {
    let mut packages: Vec<Package> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in package_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut pkg_dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.join("spin.toml").is_file())
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

/// Parses one `spin.toml`. Deliberately minimal, mirroring the reference
/// project's "identity + convention" manifest philosophy AND the gemspec
/// subset that actually matters for load-path resolution (the Part 12
/// research: name + `require_paths`, everything else is metadata):
///
/// ```toml
/// [package]
/// name = "base64"                # required, must match the directory name
/// require_paths = ["lib"]       # optional; this IS the default
/// ```
///
/// Unknown keys/tables are tolerated (forward-compat: 14.3's `[native]`
/// table lands here next). Every declared root must exist -- a package
/// whose `lib/` is missing is a loud configuration error, not an empty
/// search root.
fn parse_manifest(pkg_dir: &Path) -> PResult<Package> {
    let manifest_path = pkg_dir.join("spin.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let package = table
        .get("package")
        .and_then(|v| v.as_table())
        .ok_or_else(|| format!("{}: missing [package] table", manifest_path.display()))?;
    let name = package
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            format!(
                "{}: [package] needs a string `name`",
                manifest_path.display()
            )
        })?;
    let dir_name = pkg_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name != dir_name {
        return Err(format!(
            "{}: package name \"{name}\" doesn't match its directory name \"{dir_name}\"",
            manifest_path.display()
        ));
    }
    let require_paths: Vec<String> = match package.get("require_paths") {
        None => vec!["lib".to_string()],
        Some(v) => {
            let arr = v.as_array().ok_or_else(|| {
                format!(
                    "{}: `require_paths` must be an array of strings",
                    manifest_path.display()
                )
            })?;
            arr.iter()
                .map(|e| {
                    e.as_str().map(String::from).ok_or_else(|| {
                        format!(
                            "{}: `require_paths` must be an array of strings",
                            manifest_path.display()
                        )
                    })
                })
                .collect::<PResult<Vec<String>>>()?
        }
    };
    if require_paths.is_empty() {
        return Err(format!(
            "{}: `require_paths` must not be empty",
            manifest_path.display()
        ));
    }
    let roots = require_paths
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
                ))
            }
        })
        .collect::<PResult<Vec<PathBuf>>>()?;
    let native_crate = match table.get("native") {
        None => None,
        Some(v) => {
            let native = v.as_table().ok_or_else(|| {
                format!("{}: [native] must be a table", manifest_path.display())
            })?;
            Some(
                native
                    .get("crate")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| {
                        format!(
                            "{}: [native] needs a string `crate` (the workspace lib crate to link)",
                            manifest_path.display()
                        )
                    })?
                    .to_string(),
            )
        }
    };
    Ok(Package {
        name: name.to_string(),
        roots,
        native_crate,
    })
}

/// CRuby's exact missing-feature message (`load_failed` -> `rb_load_fail`).
fn cannot_load(name: &str) -> String {
    format!("cannot load such file -- {name}")
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
