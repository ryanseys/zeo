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

use std::path::{Path, PathBuf};

use crate::{Error, root};

/// `("json/lib/json.rb", <abs path>)` pairs, sorted, for every library.
/// Everything in a library directory ships -- gemspec, licence text, the
/// `lib/` tree -- except the Rust a colocated half sits beside.
///
/// One substitution, and it is load-bearing. A gem unpacked into a RubyGems
/// store keeps its own SOURCE gemspec, which is a Ruby program: abbrev's
/// computes its name from `__FILE__` and reads its version out of another
/// file. zeo parses gemspecs statically, so it refuses that one outright --
/// in the store it never sees it, because the store's own
/// `specifications/<name>-<version>.gemspec` is the serialized form and
/// that is what the loader reads. The flattened payload has no
/// `specifications/`, so the serialized spec must REPLACE the source one
/// here, under the conventional `<name>/<name>.gemspec` name.
pub fn files() -> Result<Vec<(String, PathBuf)>, Error> {
    let mut out = Vec::new();
    for lib in libraries()? {
        let name = &lib.name;
        for rel in shipped(&lib.dir)? {
            // The source gemspec of a store gem, dropped in favour of the
            // serialized one appended below.
            if lib.gemspec.is_some() && rel.ends_with(".gemspec") && !rel.contains('/') {
                continue;
            }
            out.push((format!("{name}/{rel}"), lib.dir.join(&rel)));
        }
        if let Some(spec) = &lib.gemspec {
            out.push((format!("{name}/{name}.gemspec"), spec.clone()));
        }
    }
    out.sort();
    Ok(out)
}

/// Every library the payload carries, with the tier precedence already
/// applied -- so a name zeo implements resolves to zeo's own half, exactly as
/// it does in a compile.
///
/// Refuses rather than shipping a short set: a locked library whose directory
/// is missing means `vendor/bundle` is behind `Gemfile.lock`, and a payload
/// assembled from that is not the locked set. Every path is
/// `<name>-<version>` read out of the lock, so this is the whole of the
/// reproducibility question -- there is no way for a stale disk to
/// contribute a library at a version the lock does not name.
fn libraries() -> Result<Vec<zeo::bundled::Library>, Error> {
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
    Ok(libs)
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
