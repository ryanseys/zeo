//! The libraries the compiler ships, flattened to the one `lib/ruby/`
//! directory every install channel carries.
//!
//! The dev tree keeps them in three tiers -- zeo's own halves under
//! `crates/zeo-rt/ext/`, the committed rubygems/bundler bootstrap under
//! `lib/ruby/`, and everything else resolved out of `vendor/bundle` from
//! `Gemfile.lock`. `zeo::bundled` decides that list, and both `dist` and
//! `stage-publish` read it from there, so neither they nor the compiler can
//! disagree about what ships.
//!
//! That makes `bundle install` a prerequisite of building a distribution.
//! It already is one for the ruby oracle, and the alternative -- vendoring
//! the trees again -- is the drift this replaced.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Error, root};

/// `("json/lib/json.rb", <abs path>)` pairs, sorted, for every library.
/// Everything in a library directory ships -- gemspec, licence text, the
/// `lib/` tree -- except the Rust a colocated half sits beside.
pub fn files() -> Result<Vec<(String, PathBuf)>, Error> {
    let mut out = Vec::new();
    for (name, dir) in library_dirs()? {
        for rel in shipped(&dir)? {
            let path = dir.join(&rel);
            out.push((format!("{name}/{rel}"), path));
        }
    }
    out.sort();
    Ok(out)
}

/// Every library, name-keyed, with the tier precedence already applied -- so
/// a name zeo implements resolves to zeo's own half, exactly as it does in a
/// compile.
pub fn library_dirs() -> Result<BTreeMap<String, PathBuf>, Error> {
    let libs = zeo::bundled::dev_tree_libraries(root());
    let missing: Vec<String> = zeo::bundled::vendored_names(root())
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !libs.iter().any(|lib| &lib.name == name))
        .collect();
    if !missing.is_empty() {
        return Err(Error::new(format!(
            "{} locked librar{} are not unpacked under vendor/bundle, so a \
             distribution built now would silently ship without them. Run \
             `make install-deps`.\n  {}",
            missing.len(),
            if missing.len() == 1 { "y" } else { "ies" },
            missing.join(" ")
        )));
    }
    Ok(libs.into_iter().map(|lib| (lib.name, lib.dir)).collect())
}

/// Every file in a library directory, as a `/`-joined relative path, except
/// the Rust and the noise macOS leaves behind.
fn shipped(dir: &Path) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((at, prefix)) = stack.pop() {
        let entries = std::fs::read_dir(&at)
            .map_err(|e| Error::new(format!("reading {}: {e}", at.display())))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", at.display())))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, rel));
            } else if !rel.ends_with(".rs") && name != ".DS_Store" {
                out.push(rel);
            }
        }
    }
    out.sort();
    Ok(out)
}
