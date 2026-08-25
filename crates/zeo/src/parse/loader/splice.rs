//! The splice half: lowering a resolved file into the arena, the
//! synthetic shims, and feature-unit materialization.

use super::*;

impl Loader {
    /// The resolve-and-splice core shared by `require`/`require_relative`/
    /// `load` and eager top-level `autoload`: given an already-extracted
    /// literal `feature` and the resolution flavor `name`, short-circuit a
    /// built-in feature, resolve the file, dedup, and splice it into the
    /// arena.
    pub(super) fn splice_feature(
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
        let inherited = file_idx.and_then(|i| hir.loader.loaded_files[i].package.clone());
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

    /// Splices `PRELOADED_FEATURES` ahead of the main file. A later explicit
    /// `require` of one then answers false, exactly as in CRuby.
    pub(super) fn preload_ambient_features(&mut self, hir: &mut Hir) -> PResult<Vec<NodeId>> {
        let mut out = Vec::new();
        for feature in PRELOADED_FEATURES {
            if let Some(spliced) = self.splice_synthetic_shim(hir, feature, 0)? {
                out.extend(spliced);
            }
        }
        Ok(out)
    }

    /// A `require` that found nothing on disk MAY be one of the stdlib features
    /// zeo synthesizes in-tree (currently just `rbconfig`, which real Ruby
    /// generates at build time). Splice the embedded shim source, deduped per
    /// box like any other require. Returns `None` when `feature` names no shim,
    /// so the caller falls through to the static-ext table.
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

    /// Compiles in every `.rb` under the demanded load paths as a UNIT (see
    /// `LoaderState::feature_units`), skipping the files already spliced.
    ///
    /// The demand comes from a file that computes a `require`/`autoload`
    /// target: zeo cannot know which string it will build, so the honest
    /// answer is to compile in everything that string could name -- which is
    /// exactly its load path, and exactly what CRuby would have searched. The
    /// walk is bounded to the DEMANDING package's own roots, so a gem that
    /// resolves its own targets dynamically pays for itself and nothing else.
    pub(super) fn materialize_units(&mut self, hir: &mut Hir) -> PResult<()> {
        self.in_unit_sweep = true;
        let result = self.materialize_units_inner(hir);
        self.in_unit_sweep = false;
        result
    }

    fn materialize_units_inner(&mut self, hir: &mut Hir) -> PResult<()> {
        // Single-file demands first: a conditional require names exactly one
        // target, and registering it under the feature AS REQUIRED is what
        // lets the runtime call find it.
        //
        // GROUPED BY FILE, because one file can be demanded under more than
        // one spelling: rack writes `autoload :MediaType, "rack/media_type"`
        // and a guarded `require_relative "../lib/rack/media_type"` for the
        // same file. Splicing whichever demand the set yielded first and
        // dropping the rest lost the other spelling -- and when the one lost
        // was the autoload's, the read that should have run the unit found no
        // unit under that name. The class was still there (a unit's classes
        // register at startup) and its body's constants were not, so
        // `Rack::MediaType.type` raised `uninitialized constant
        // SPLIT_PATTERN`.
        let mut grouped: Vec<(PathBuf, Option<String>, Vec<String>)> = Vec::new();
        for (package, path, feature) in std::mem::take(&mut hir.loader.single_unit_demand) {
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            match grouped.iter_mut().find(|(p, _, _)| p == &canonical) {
                Some((_, pkg, features)) => {
                    // A package attribution decides which roots the file's
                    // own requires resolve against, so a named one outranks
                    // the `None` a `require_relative` from outside any
                    // package carries.
                    if pkg.is_none() {
                        *pkg = package;
                    }
                    if !features.contains(&feature) {
                        features.push(feature);
                    }
                }
                None => grouped.push((canonical, package, vec![feature])),
            }
        }
        for (canonical, package, mut features) in grouped {
            // A unit an EARLIER round already made: this round's spellings
            // join it rather than being dropped.
            if let Some(idx) = self.single_units.get(&canonical) {
                let unit = &mut hir.loader.feature_units[*idx];
                let fresh: Vec<String> = features
                    .into_iter()
                    .filter(|f| f != &unit.feature && !unit.aliases.contains(f))
                    .collect();
                unit.aliases.extend(fresh);
                continue;
            }
            if !self.required.insert((0, canonical.clone())) {
                continue; // already spliced: it runs at its own position
            }
            let absolute = canonical.with_extension("").to_string_lossy().into_owned();
            // A single-file unit has NOT run when the program starts, so
            // every constant its body assigns is undefined until something
            // loads it -- whatever spelling reaches it. `defined?` and every
            // constant fold ask the run time for these names.
            let before: std::collections::BTreeSet<String> =
                hir.const_write_names().cloned().collect();
            match self.splice_file(hir, &canonical, None, package.clone(), 0) {
                Ok(body) => {
                    let fresh: Vec<String> = hir
                        .const_write_names()
                        .filter(|k| !before.contains(*k))
                        .cloned()
                        .collect();
                    hir.loader.unrun_unit_consts.extend(fresh.iter().cloned());
                    for lf in hir
                        .loader
                        .loaded_files
                        .iter_mut()
                        .filter(|lf| lf.canonical == canonical)
                    {
                        lf.is_unit = true;
                    }
                    self.single_units
                        .insert(canonical, hir.loader.feature_units.len());
                    let feature = features.remove(0);
                    hir.loader.feature_units.push(crate::hir::FeatureUnit {
                        feature,
                        aliases: features,
                        absolute,
                        body,
                    })
                }
                Err(e) => hir.loader.declined_units.push((
                    features.remove(0),
                    absolute,
                    e.message().to_string(),
                )),
            }
        }
        for (package, dir) in std::mem::take(&mut hir.loader.unit_demand) {
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
                                body,
                            })
                        }
                        // Never reached by a require -> never observed. Reached
                        // by one -> a LoadError naming the gap, at the require.
                        Err(e) => hir.loader.declined_units.push((
                            feature,
                            absolute,
                            e.message().to_string(),
                        )),
                    }
                }
            }
        }
        Ok(())
    }

    /// Parses and lowers one resolved file into the arena, recording its
    /// provenance -- one `LoadedFile` per SPLICE INSTANCE (see
    /// `LoaderState::loaded_files`' docs for why instance, not canonical file, is
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
        let idx = hir.loader.loaded_files.len();
        hir.loader.loaded_files.push(LoadedFile {
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
        if hir.loader.loaded_files[idx].package.is_none() {
            collect_parse_warnings(hir, &result, &canonical.display().to_string(), &source);
        }
        self.splicing.push(canonical.to_path_buf());
        // A required file's `__FILE__` is ITSELF, not whoever required it.
        // Popped by the guard's Drop, so the parent's own statements after
        // the splice see their own path again.
        let _file = SourceFileFrame::push(Some(canonical), 0);
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
            hir.loader.loaded_files[idx].package.clone(),
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
        let mut statements = statements?;
        self.splicing.pop();
        // CRuby records a feature BEFORE it evaluates the file (which is what
        // makes a circular require answer `false` rather than recurse), so
        // the marker leads the spliced statements.
        statements.insert(
            0,
            hir.push(HirNode::FeatureLoaded {
                entry: canonical.display().to_string(),
                feature: None,
            }),
        );
        Ok(statements)
    }
}
