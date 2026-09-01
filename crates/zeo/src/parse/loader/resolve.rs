//! Feature-name resolution: `require`/`require_relative`/`load` to
//! the file each names, against the `-I` roots and the gem roots.

use super::*;

/// Every `.rb` under `dir`, recursively. Symlinked directories are followed
/// like `require` follows them; a cycle is bounded by the filesystem, since a
/// canonicalized repeat is skipped by the caller's dedup table.
pub(super) fn collect_rb_files(dir: &Path, out: &mut Vec<PathBuf>) {
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
pub(super) fn feature_name_under(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let rel = rel.with_extension("");
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

impl Loader {
    /// Whether a plain `require "feature"` is answered by zeo's OWN
    /// implementation rather than by a file.
    ///
    /// [`is_builtin_feature`] alone used to decide this, and it is only half
    /// the question. zeo reimplements a number of default gems -- `psych`,
    /// `strscan`, `json` -- and a project whose lockfile names one, against a
    /// store that actually holds it, asked for THAT release: the version it
    /// pinned is the version it expects to run. So a store gem wins its own
    /// name, and zeo's implementation stays the answer for everything else,
    /// which is what a bare `zeo -e 'require "psych"'` gets with no Gemfile,
    /// no store, and no compiler.
    pub(super) fn builtin_wins(&self, feature: &str) -> bool {
        zeo_provides(feature) && !self.store_overrides.contains(feature)
    }

    /// The file this literal `require`/`require_relative` names, canonicalized
    /// -- the key `Kernel#require`'s already-loaded answer is read from.
    ///
    /// Resolution mirrors `splice_feature`'s, and deliberately records
    /// nothing: this is a QUESTION, asked before the statement lowers, and a
    /// gem disclosure or a memo write here would make asking it change the
    /// program. `None` for every shape without one compile-time file --
    /// a computed name, a builtin (whose re-require answers off
    /// `activated_features`, in the fold itself), a target that resolves to
    /// nothing.
    pub(super) fn require_target(
        &mut self,
        hir: &mut Hir,
        result: &ruby_prism::ParseResult,
        call: &ruby_prism::CallNode<'_>,
        name: &str,
        dir: Option<&Path>,
    ) -> PResult<Option<PathBuf>> {
        let Some(feature) = literal_feature(result, hir, call)? else {
            return Ok(None);
        };
        if name == "require" && self.builtin_wins(&feature) {
            return Ok(None);
        }
        // A synthesized shim has no file on disk; its dedup slot is the
        // virtual spelling `splice_synthetic_shim` inserts. Answering it here
        // is what lets the rerequire pre-pass see a second require of one,
        // so the fold says false the way ruby does for a loaded feature.
        if name == "require" && synthetic_shim_source(&feature).is_some() {
            return Ok(Some(PathBuf::from(format!("<zeo-shim>/{feature}.rb"))));
        }
        let path = match name {
            "require_relative" => resolve_require_relative(&feature, dir).ok(),
            "require" if feature.starts_with("./") || feature.starts_with("../") => {
                resolve_require_relative(&feature, dir).ok()
            }
            "require" => self
                .resolve_require(&feature)
                .ok()
                .flatten()
                .map(|(p, _)| p),
            _ => None,
        };
        Ok(path.and_then(|p| p.canonicalize().ok()))
    }

    /// Whether a plain `require "feature"` has a compile-time VERDICT -- either
    /// it resolves to something zeo can splice or activate (a file on a load-path
    /// root, a synthesized shim, a statically linked extension), OR it names a
    /// gem the external store located but zeo must REJECT (a native extension or
    /// wrong-platform gem, `store_exclusions`), which stays a loud compile error
    /// with its specific reason. Only a require with NEITHER verdict has nothing
    /// to say at compile time and defers to a runtime `Kernel#require` (raising
    /// `LoadError`). Mirrors `splice_feature`'s `require` resolution.
    pub(super) fn require_resolvable(&self, feature: &str) -> bool {
        // The isolation dial. A library named in `ZEO_DEBUG_RUNTIME_LOAD` has
        // no compile-time verdict on purpose, so every route below -- splice,
        // unit, static ext -- is refused in one place and the require lands on
        // the runtime loader instead.
        if crate::debug_flags::loads_at_runtime(feature) {
            return false;
        }
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
                // A merged package carries the feature: deliberately NO
                // compile-time verdict, so the require stays a call and the
                // package's unit rows answer it at run time. Checked before
                // the builtin arm -- a packaged feature that is ALSO a
                // builtin name (tmpdir, a store override with an artifact)
                // would otherwise claim a verdict, take the splice road, and
                // die in the static-ext fallthrough where the override has
                // already dismissed the builtin.
                if self.packaged_features.contains(feature) {
                    return false;
                }
                synthetic_shim_source(feature).is_some() || {
                    let bare = feature
                        .strip_suffix(".so")
                        .or_else(|| feature.strip_suffix(".bundle"))
                        .or_else(|| feature.strip_suffix(".o"))
                        .unwrap_or(feature);
                    (is_builtin_feature(bare)
                        && crate::lower::features::build_carries_ext(bare))
                        || self.store_exclusions.contains_key(bare)
                        // A gem shipping its C as source: zeo builds it, so
                        // the require HAS a compile-time verdict.
                        || self.cext_gem(bare).is_some()
                        // A compiled extension already sitting on a `-I` root.
                        || self.resolve_native_root(bare).is_some()
                }
            }
        }
    }

    /// The compiled extension `feature` names on a `-I` root, and the stem its
    /// `Init_` is named after.
    ///
    /// The compile-time half of `features::resolve_native_on_disk`. A store
    /// gem's extension reaches the loader through its `extensions` entry
    /// (`build_cext`); an extension that is simply ON the load path -- what
    /// `-I` and a hand-built `.bundle` give -- had no compile-time route at
    /// all, so the require deferred to the runtime loader. That loader then
    /// refused it, because a program publishes the C API only when its COMPILE
    /// saw an extension load. Seeing it here is what publishes it.
    ///
    /// Roots only. A gem root is `build_cext`'s to answer, and searching them
    /// here would load a prebuilt `.so` from a store gem whose source zeo
    /// wants to compile itself.
    pub(super) fn resolve_native_root(&self, feature: &str) -> Option<(PathBuf, String)> {
        let named = |p: PathBuf| {
            let stem = p.file_stem()?.to_str()?.to_string();
            Some((p, stem))
        };
        if is_native_feature(feature) {
            return self
                .roots
                .iter()
                .map(|r| r.join(feature))
                .find(|c| c.is_file())
                .and_then(named);
        }
        NATIVE_SUFFIXES
            .iter()
            .find_map(|s| {
                let fname = format!("{feature}.{s}");
                self.roots.iter().map(|r| r.join(&fname)).find(|c| c.is_file())
            })
            .and_then(named)
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
    pub(super) fn resolve_require(
        &self,
        feature: &str,
    ) -> PResult<Option<(PathBuf, Option<String>)>> {
        let resolved = self.probe_require(feature)?;
        if let Some((_, Some(name))) = &resolved
            && let Some(pkg) = self.packages.iter().find(|g| &g.name == name)
        {
            self.activate(pkg);
        }
        Ok(resolved)
    }

    /// The same search, asked as a QUESTION: no activation, so the answer
    /// costs `$LOAD_PATH` nothing.
    ///
    /// `lower_main_file` asks, on every compile, which gated builtins also
    /// have a vendored Ruby half. Through `resolve_require` that probe put 16
    /// package roots on the `$LOAD_PATH` of a program that requires nothing --
    /// measured, `puts $LOAD_PATH` printed tmpdir, json, psych, openssl and
    /// twelve more. It also blocked memoizing the loop, because a memo hit
    /// would have skipped the side effect and made `$:` depend on how many
    /// compiles a process had already done.
    pub(super) fn probe_require(
        &self,
        feature: &str,
    ) -> PResult<Option<(PathBuf, Option<String>)>> {
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
        // A merged package already compiled this feature: no provider may
        // splice its source again (the artifact and a store tree could
        // diverge, and both bodies would run). "Not on disk" leaves the
        // require a call, which the package's merged unit rows answer.
        if self.packaged_features.contains(feature) {
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
        let mut foreign_hit = false;
        for pkg in &self.packages {
            // A package build carries ONE gem. A feature another gem owns
            // is the host's to answer -- from its own splice or that gem's
            // own package -- so it resolves to nothing here and the require
            // stays a call. WHICH features took this road is recorded: the
            // artifact's manifest names them (`host_features`), so a
            // consumer can tell "the host must provide this" apart from a
            // require that resolves nowhere on any road.
            if self
                .pkg_own_gem
                .as_ref()
                .is_some_and(|own| own != &pkg.name)
            {
                foreign_hit = foreign_hit
                    || pkg.roots.iter().any(|r| r.join(&fname).is_file());
                continue;
            }
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
            0 => {
                if foreign_hit {
                    self.pkg_foreign.borrow_mut().insert(feature.to_string());
                }
                Ok(None)
            }
            1 => {
                let (path, pkg) = hits.remove(0);
                if self.defer_to_cached_artifact(feature, pkg) {
                    return Ok(None);
                }
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
                if self.defer_to_cached_artifact(feature, pkg) {
                    return Ok(None);
                }
                Ok(Some((path, Some(pkg.name.clone()))))
            }
        }
    }

    /// Whether the machine package cache should answer this require
    /// instead of the gem's source tree: `pkg` is a BUNDLED gem, the cache
    /// holds a servable artifact for it, and no discovery round rejected
    /// it. True defers the feature -- "not on disk", so nothing splices --
    /// and records it for the discovery loop, which merges the artifact
    /// and re-parses. The verdict is per GEM and memoized: one probe per
    /// parse, every feature of the gem follows it.
    fn defer_to_cached_artifact(&self, feature: &str, pkg: &Gem) -> bool {
        if !self.auto_consult || pkg.provenance != GemProvenance::Bundled {
            return false;
        }
        let has = *self
            .auto_verdicts
            .borrow_mut()
            .entry(pkg.name.clone())
            .or_insert_with(|| crate::autopkg::cache_has(&pkg.name, &pkg.roots, feature));
        if !has {
            return false;
        }
        self.auto_pending
            .borrow_mut()
            .entry(pkg.name.clone())
            .or_default()
            .push(feature.to_string());
        true
    }

    /// Record that a `require` reached `pkg`, so its roots join `$LOAD_PATH`.
    /// Once per package, in resolution order -- CRuby's activation order.
    fn activate(&self, pkg: &Gem) {
        {
            let mut seen = self.activated.borrow_mut();
            if !seen.iter().any(|n| n == &pkg.name) {
                seen.push(pkg.name.clone());
            }
        }
        if pkg.name == "rubygems" {
            self.publish_default_gems();
        }
    }

    /// Publish the bundled libraries as RubyGems DEFAULT GEMS.
    ///
    /// Triggered by the `require` that reaches RubyGems, and nowhere else:
    /// the store is a directory tree to build, so a program that never loads
    /// RubyGems must not pay for it. `zeo -e 'p 1'` compiles in 0.02s and has
    /// no business writing a gem store.
    ///
    /// Only the BUNDLED tier is published. A gem from an external store is
    /// already a real installed gem with a real gemspec, and re-publishing it
    /// as a default gem would give RubyGems two rows for one library --
    /// default gems lose to installed ones, but the duplicate would still
    /// show in `gem list`.
    fn publish_default_gems(&self) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        let mut gems = Vec::new();
        for pkg in &self.packages {
            if pkg.provenance != GemProvenance::Bundled {
                continue;
            }
            // A version is not optional here: `<name>-<version>` IS the
            // gemspec's file name and the stub's identity. zeo's `socket`,
            // `pty` and `monitor` have no gemspec (ruby ships them with none
            // either), so they stay invisible to `gem list` rather than
            // appear under an invented version.
            let Some(version) = pkg.version.as_deref() else {
                continue;
            };
            // Every root has to sit directly under one gem directory for the
            // symlink to stand in for all of them.
            let Some(dir) = pkg.roots.first().and_then(|r| r.parent()) else {
                continue;
            };
            let paths: Vec<String> = pkg
                .roots
                .iter()
                .filter(|r| r.parent() == Some(dir))
                .filter_map(|r| Some(r.file_name()?.to_string_lossy().into_owned()))
                .collect();
            if paths.len() != pkg.roots.len() {
                continue;
            }
            gems.push(crate::default_gems::DefaultGem {
                name: &pkg.name,
                version,
                dir,
                require_paths: paths,
            });
        }
        ONCE.call_once(|| crate::default_gems::materialize(&gems));
    }

    /// What `$LOAD_PATH` holds at run time: the `-I` roots as given, then the
    /// roots of every package a `require` actually activated.
    ///
    /// COSMETIC for resolution -- every compile-time require was already
    /// resolved -- but load-bearing for code that READS `$:` and expects a
    /// file to be there. `IRB::Locale#load` searches it with `File.readable?`
    /// and dies without it, and `Kernel#load` resolves against it at run time.
    pub(super) fn load_path(&self) -> Vec<String> {
        let mut out: Vec<String> = self.roots.iter().map(|p| p.display().to_string()).collect();
        for name in self.activated.borrow().iter() {
            let Some(pkg) = self.packages.iter().find(|g| &g.name == name) else {
                continue;
            };
            out.extend(pkg.roots.iter().map(|p| p.display().to_string()));
        }
        out
    }

    /// Write the activation summary the auto-packaging tier reads onto the
    /// Hir: which BUNDLED gems this compile reached (with the features each
    /// answered), and which features resolved out of any gem tier at all.
    /// Runs once, after every resolution, beside the `$LOAD_PATH` fill.
    pub(super) fn record_activation_summary(&self, hir: &mut crate::hir::Hir) {
        let memo = self.require_memo.borrow();
        hir.loader.pkg_foreign_requires = self.pkg_foreign.borrow().clone();
        let pending = self.auto_pending.borrow();
        hir.loader.activated_bundled = self
            .activated
            .borrow()
            .iter()
            .map(String::as_str)
            // A gem whose features DEFERRED to a cached artifact never
            // activates -- nothing spliced -- but it is exactly what the
            // discovery loop needs to consult, so it joins the rows.
            .chain(
                pending
                    .keys()
                    .map(String::as_str)
                    .filter(|n| !self.activated.borrow().iter().any(|a| a == n)),
            )
            .filter_map(|name| {
                let pkg = self.packages.iter().find(|g| g.name == name)?;
                if pkg.provenance != GemProvenance::Bundled {
                    return None;
                }
                let mut features: Vec<String> = memo
                    .iter()
                    .filter_map(|(f, r)| match r {
                        Some((_, Some(n))) if n == name => Some(f.clone()),
                        _ => None,
                    })
                    .collect();
                features.extend(pending.get(name).into_iter().flatten().cloned());
                features.sort();
                features.dedup();
                Some(crate::hir::ActivatedBundled {
                    name: pkg.name.clone(),
                    version: pkg.version.clone(),
                    roots: pkg.roots.clone(),
                    features,
                    deferred: pending.contains_key(name),
                })
            })
            .collect();
    }

    /// `load "path"`: no `.rb` appending, ever (CRuby's `rb_find_file` has
    /// no ext parameter). Roots first, then the requiring file's directory
    /// -- the latter is a documented divergence from CRuby's cwd fallback
    /// (cwd is meaningless at compile time; the requiring file's own
    /// directory is the deterministic analogue).
    pub(super) fn resolve_load(&self, arg: &str, dir: Option<&Path>) -> PResult<PathBuf> {
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
pub(super) fn resolve_require_relative(feature: &str, dir: Option<&Path>) -> PResult<PathBuf> {
    let Some(dir) = dir else {
        // CRuby's exact error when the requiring context has no file
        // (eval/irb): here, a source compiled without an input path.
        return Err("cannot infer basepath -- `require_relative` needs the requiring file's directory (compile from a file path)".to_string().into());
    };
    // A `require_relative` of a `.so`/`.bundle` names a compiled object by
    // PATH rather than by feature, so the store's build never sees it -- zeo
    // compiles a gem's extension from the `s.extensions` its gemspec
    // declares, and nothing here says which gem this file belongs to.
    if is_native_feature(feature) {
        return Err(format!(
            "`require_relative \"{feature}\"`: zeo builds a gem's C extension from its \
             gemspec's `extensions`, not from a path -- a `require_relative` of a compiled \
             object names no gem to build"
        )
        .into());
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
            other @ (Component::Prefix(_)
            | Component::RootDir
            | Component::ParentDir
            | Component::Normal(_)) => out.push(other),
        }
    }
    out
}
