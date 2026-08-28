//! The libraries the compiler ships, flattened to the one `gems/` directory
//! every install channel carries.
//!
//! The dev tree keeps them in two places -- the gem-shaped directories zeo
//! owns under `crates/zeo-rt/ext/`, and the vendored upstream copies in
//! `gems/`. This is where the two become one, so `dist` and `stage-publish`
//! cannot disagree about what ships.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Error, root_join};

const EXT: &str = "crates/zeo-rt/ext";
const GEMS: &str = "gems";

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

/// zeo's own libraries first, so a name both tiers carry resolves to zeo's --
/// the same precedence the loader applies (`bundled_gems_dirs`).
pub fn library_dirs() -> Result<BTreeMap<String, PathBuf>, Error> {
    let mut dirs = BTreeMap::new();
    for tier in [EXT, GEMS] {
        for (name, path) in libraries_in(&root_join(tier))? {
            dirs.entry(name).or_insert(path);
        }
    }
    Ok(dirs)
}

/// A library is a directory with a `lib/`. Under `ext/` that skips every
/// extension whose Rust needs no Ruby half.
fn libraries_in(dir: &Path) -> Result<Vec<(String, PathBuf)>, Error> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
    let mut out = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?
            .path();
        if !path.join("lib").is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .expect("a directory name")
            .to_string_lossy()
            .into_owned();
        out.push((name, path));
    }
    out.sort();
    Ok(out)
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
