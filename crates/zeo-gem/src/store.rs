//! A RubyGems store on disk.
//!
//! ```text
//! <root>/gems/<full_name>/            the gem's own files
//! <root>/specifications/<full_name>.gemspec
//! ```
//!
//! That is the whole layout a loader needs: the spec says which
//! `require_paths` to add, and the gem directory holds them. RubyGems also
//! keeps `cache/`, `bin/`, `doc/` and `extensions/` under the same root; this
//! type neither writes nor requires them.
//!
//! Installing is atomic. A gem is unpacked into `.staging/` and renamed into
//! place, and its gemspec is written last, so a store that holds a spec always
//! holds the files that spec names -- an interrupted install leaves a
//! directory to clean, never a half-installed gem a loader would find.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::gemspec::Gemspec;
use crate::package::Package;

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `<full_name>`'s files live.
    pub fn gem_dir(&self, full_name: &str) -> PathBuf {
        self.root.join("gems").join(full_name)
    }

    /// Where `<full_name>`'s gemspec lives.
    pub fn gemspec_path(&self, full_name: &str) -> PathBuf {
        self.root
            .join("specifications")
            .join(format!("{full_name}.gemspec"))
    }

    /// Whether the store holds this gem, spec and files both.
    pub fn holds(&self, full_name: &str) -> bool {
        self.gemspec_path(full_name).is_file() && self.gem_dir(full_name).is_dir()
    }

    /// Unpack `package` into the store, replacing any copy already there.
    /// Answers the gem's directory.
    pub fn install(&self, package: &Package) -> Result<PathBuf> {
        let full_name = package.full_name();
        let staging = self.root.join(".staging").join(&full_name);
        remove_dir(&staging)?;
        package.unpack_data_to(&staging)?;

        let target = self.gem_dir(&full_name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        // The spec goes last, so it never names files that are not there yet.
        // Removing the old spec first keeps that true through the swap.
        let spec_path = self.gemspec_path(&full_name);
        remove_file(&spec_path)?;
        remove_dir(&target)?;
        std::fs::rename(&staging, &target).map_err(|e| Error::io(&target, e))?;
        self.write_gemspec(&package.spec)?;
        // `.staging` is empty again once every install in a run has finished.
        let _ = std::fs::remove_dir(self.root.join(".staging"));
        Ok(target)
    }

    /// Write one gem's stub gemspec, atomically.
    pub fn write_gemspec(&self, spec: &Gemspec) -> Result<PathBuf> {
        let path = self.gemspec_path(&spec.full_name());
        let dir = path.parent().expect("a gemspec path has a parent");
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let temp = path.with_extension("gemspec.new");
        std::fs::write(&temp, spec.to_stub_source()).map_err(|e| Error::io(&temp, e))?;
        std::fs::rename(&temp, &path).map_err(|e| Error::io(&path, e))?;
        Ok(path)
    }

    /// Every gem the store holds, name-sorted. A spec that does not parse is
    /// an error, not a gem quietly left out of the answer.
    pub fn installed(&self) -> Result<Vec<Gemspec>> {
        let dir = self.root.join("specifications");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // No store yet is an empty store, not a failure.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(&dir, e)),
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "gemspec"))
            .collect();
        paths.sort();
        paths.iter().map(|p| Gemspec::parse_file(p)).collect()
    }

    /// Remove a gem: its spec first, so a loader never sees a spec without
    /// files.
    pub fn remove(&self, full_name: &str) -> Result<()> {
        remove_file(&self.gemspec_path(full_name))?;
        remove_dir(&self.gem_dir(full_name))
    }

    /// The `require_paths` of every installed gem, absolute -- what a loader
    /// puts on its search path.
    pub fn load_paths(&self) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for spec in self.installed()? {
            let dir = self.gem_dir(&spec.full_name());
            out.extend(spec.require_paths.iter().map(|rp| dir.join(rp)));
        }
        Ok(out)
    }
}

fn remove_dir(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path, e)),
    }
}

fn remove_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{gem_bytes, metadata_yaml};

    /// A scratch store under `target/`, so a killed run leaves nothing on the
    /// developer's machine outside the build directory.
    fn scratch(name: &str) -> Store {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/zeo-gem-tests")
            .join(name);
        std::fs::remove_dir_all(&root).ok();
        Store::new(root)
    }

    fn package(name: &str, version: &str, platform: &str) -> Package {
        let bytes = gem_bytes(
            &metadata_yaml(name, version, platform),
            &[
                (&format!("lib/{name}.rb"), "OK = true\n"),
                ("README.md", "readme\n"),
            ],
        );
        Package::from_bytes(&bytes).expect("the built gem reads back")
    }

    #[test]
    fn installing_writes_the_files_and_the_gemspec() {
        let store = scratch("install");
        let dir = store.install(&package("rake", "13.4.2", "ruby")).unwrap();
        assert_eq!(dir, store.gem_dir("rake-13.4.2"));
        assert_eq!(
            std::fs::read_to_string(dir.join("lib/rake.rb")).unwrap(),
            "OK = true\n"
        );
        assert!(store.holds("rake-13.4.2"));
        let spec = std::fs::read_to_string(store.gemspec_path("rake-13.4.2")).unwrap();
        assert!(spec.contains("# stub: rake 13.4.2 ruby lib\n"), "{spec}");
        assert!(spec.contains("add_runtime_dependency"), "{spec}");
    }

    #[test]
    fn what_is_installed_reads_back_as_gemspecs_in_name_order() {
        let store = scratch("installed");
        store.install(&package("rake", "13.4.2", "ruby")).unwrap();
        store.install(&package("json", "2.21.2", "ruby")).unwrap();
        let names: Vec<String> = store
            .installed()
            .unwrap()
            .into_iter()
            .map(|s| s.full_name())
            .collect();
        assert_eq!(names, ["json-2.21.2", "rake-13.4.2"]);
        let paths = store.load_paths().unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|p| p.ends_with("lib")));
        assert!(paths[0].is_dir());
    }

    #[test]
    fn a_store_that_does_not_exist_yet_is_an_empty_store() {
        let store = scratch("missing");
        assert!(store.installed().unwrap().is_empty());
        assert!(!store.holds("rake-13.4.2"));
        assert!(store.load_paths().unwrap().is_empty());
    }

    /// Reinstalling replaces the gem's files. A file the old version had and
    /// the new one does not must be gone, which is why the install renames a
    /// fresh directory in rather than unpacking over the old one.
    #[test]
    fn reinstalling_replaces_the_files_instead_of_merging_them() {
        let store = scratch("replace");
        store.install(&package("rake", "13.4.2", "ruby")).unwrap();
        let stale = store.gem_dir("rake-13.4.2").join("lib/stale.rb");
        std::fs::write(&stale, "gone soon\n").unwrap();

        store.install(&package("rake", "13.4.2", "ruby")).unwrap();
        assert!(!stale.exists(), "the old file survived the reinstall");
        assert!(store.gem_dir("rake-13.4.2").join("lib/rake.rb").is_file());
        // Nothing is left staged.
        assert!(!store.root().join(".staging").exists());
    }

    #[test]
    fn a_platform_gem_installs_under_its_own_full_name() {
        let store = scratch("platform");
        store
            .install(&package("nokogiri", "1.19.4", "arm64-darwin"))
            .unwrap();
        assert!(store.holds("nokogiri-1.19.4-arm64-darwin"));
        assert!(!store.holds("nokogiri-1.19.4"));
        let spec = &store.installed().unwrap()[0];
        assert_eq!(spec.platform.as_deref(), Some("arm64-darwin"));
        assert!(spec.is_native());
    }

    #[test]
    fn removing_takes_the_spec_and_the_files_and_is_idempotent() {
        let store = scratch("remove");
        store.install(&package("rake", "13.4.2", "ruby")).unwrap();
        store.remove("rake-13.4.2").unwrap();
        assert!(!store.holds("rake-13.4.2"));
        assert!(!store.gem_dir("rake-13.4.2").exists());
        assert!(!store.gemspec_path("rake-13.4.2").exists());
        // Removing what is not there is not an error.
        store.remove("rake-13.4.2").unwrap();
        assert!(store.installed().unwrap().is_empty());
    }

    /// The store's own gemspec must read back as the same spec, or a loader
    /// would resolve a require differently from the installer.
    #[test]
    fn the_written_gemspec_reads_back_as_what_was_installed() {
        let store = scratch("roundtrip");
        let package = package("nokogiri", "1.19.4", "arm64-darwin");
        store.install(&package).unwrap();
        let read = &store.installed().unwrap()[0];
        assert_eq!(read.name, package.spec.name);
        assert_eq!(read.version, package.spec.version);
        assert_eq!(read.platform, package.spec.platform);
        assert_eq!(read.require_paths, package.spec.require_paths);
        assert_eq!(read.full_name(), package.full_name());
    }
}
