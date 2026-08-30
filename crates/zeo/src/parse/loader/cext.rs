//! Statically linked C extensions: the static-ext table and the
//! from-source gem extension build.

use super::*;

impl Loader {
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
    pub(super) fn activate_static_ext(
        &mut self,
        hir: &mut Hir,
        feature: &str,
    ) -> PResult<Vec<NodeId>> {
        let bare = feature
            .strip_suffix(".so")
            .or_else(|| feature.strip_suffix(".bundle"))
            .or_else(|| feature.strip_suffix(".o"))
            .unwrap_or(feature);
        // `builtin_wins`, not `is_builtin_feature`: a store gem that supplies
        // this name, or `ZEO_DISABLE_BUILTIN` retiring it, both mean zeo's row
        // is not the answer and the gem's own C extension is built below.
        if !self.builtin_wins(bare) {
            // A gem that ships its C as SOURCE is built HERE, at the require
            // that reached it -- an AOT compiler builds only what a require
            // reaches, which is why this is not eager.
            if let Some(node) = self.build_cext(hir, bare)? {
                return Ok(vec![node]);
            }
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
        let feature = canonical_ext_feature(bare).to_string();
        hir.activate_feature(&feature);
        // Loading a statically linked extension is POSITIONAL:
        // `$LOADED_FEATURES` names it and its require-gated rows become
        // answerable from HERE, not from line 1. The compile-time set above
        // stays what it always was -- a NAME-RESOLUTION gate, so a class
        // nothing requires anywhere resolves nowhere -- and this marker is
        // what carries the ordering inside one program.
        let entry = format!("<zeo-builtin>/{feature}.rb");
        Ok(vec![hir.push(HirNode::FeatureLoaded {
            entry,
            feature: Some(feature),
        })])
    }

    /// Build the C extension `feature` names, if a store gem ships one.
    ///
    /// `require "foo/foo"` and `require "foo"` both belong to the gem `foo`:
    /// the first segment is the gem name, which is RubyGems' own convention
    /// and what `create_makefile("foo/foo")` produces. Anything else answers
    /// `None` and falls through to the ordinary miss.
    ///
    /// The build runs `extconf.rb` and then compiles and links, both through
    /// [`crate::cext`]. A failure is a COMPILE error naming the gem: the
    /// alternative is a program that builds and then cannot load, which is
    /// the failure mode the whole C0 design exists to avoid.
    fn build_cext(&mut self, hir: &mut Hir, feature: &str) -> PResult<Option<NodeId>> {
        let Some(gem) = self.cext_gem(feature).map(str::to_string) else {
            return Ok(None);
        };
        let gem = gem.as_str();
        if let Some((library, init)) = self.built_cexts.get(gem) {
            return Ok(Some(hir.push(HirNode::CExtLoaded {
                library: library.clone(),
                init: init.clone(),
            })));
        }
        let ext = &self.native_exts[gem];
        let zeo =
            crate::cext::zeo_binary().map_err(|e| format!("building {gem}'s C extension: {e}"))?;
        // A gem may ship more than one extension. Every one is built, and the
        // FIRST is what the feature names -- mkmf's own convention, since a
        // second extension has its own `create_makefile` and its own require.
        let mut first: Option<(String, String)> = None;
        for extconf in &ext.extconfs {
            let path = ext.gem_dir.join(extconf);
            let Some(dir) = path.parent() else { continue };
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            // Out of tree: the gem store is shared and often read only, and a
            // build that wrote into it would leave one project's artifacts
            // where another reads them.
            let library = crate::cext::build_out_of_tree(&zeo, gem, dir, &name)
                .map_err(|e| format!("building {gem}'s C extension: {e}"))?;
            if first.is_none() {
                let init = library
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(gem)
                    .to_string();
                first = Some((library.display().to_string(), init));
            }
        }
        let Some((library, init)) = first else {
            return Ok(None);
        };
        self.built_cexts
            .insert(gem.to_string(), (library.clone(), init.clone()));
        self.record_gem(crate::gem_report::GemRecord {
            name: gem.to_string(),
            by: crate::gem_report::SatisfiedBy::CompiledExt {
                library: library.clone(),
            },
        });
        Ok(Some(hir.push(HirNode::CExtLoaded { library, init })))
    }

    /// Which store gem, if any, would build `feature`.
    ///
    /// Two rules, and the second is not optional. `require "foo/foo"` and
    /// `require "foo"` belong to the gem `foo`: the first segment is the gem
    /// name, which is RubyGems' convention and what `create_makefile("foo/foo")`
    /// produces.
    ///
    /// But the convention is only a convention. `bcrypt`'s Ruby half does
    /// `require "bcrypt_ext"`, because its extconf says
    /// `create_makefile("bcrypt_ext")` -- so the feature shares no prefix
    /// with the gem at all. The extconf is where the answer is written down,
    /// so it is read.
    pub(super) fn cext_gem<'a>(&'a self, feature: &'a str) -> Option<&'a str> {
        let gem = feature.split('/').next().unwrap_or(feature);
        if self.native_exts.contains_key(gem) {
            return Some(gem);
        }
        self.native_exts
            .values()
            .find(|ext| ext.provides(feature))
            .map(|ext| ext.name.as_str())
    }
}
