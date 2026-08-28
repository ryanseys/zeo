//! Fetching an upstream tree at a pinned revision, and comparing trees.
//!
//! The model is VENDOR-ON-FETCH: a git URL plus a tag is the source, and the
//! committed tree is the storage the build reads. A fresh clone of zeo builds
//! offline; only re-vendoring runs the network. Vendoring needs only `git`.

use std::path::{Path, PathBuf};

use crate::exec::{self, Capture};
use crate::{Error, root, root_join};

/// One pinned upstream tree.
pub struct Pin {
    pub name: String,
    pub repo: String,
    pub tag: String,
    pub rev: String,
    pub subdir: Option<String>,
}

impl Pin {
    /// The MRI C API headers, from `ruby-headers.lock`.
    ///
    /// A `key = value` file rather than a lockfile format: there is exactly
    /// one pin in it, and a parser for one record should be readable in one
    /// screen.
    pub fn ruby_headers() -> Result<Pin, Error> {
        let path = root_join("ruby-headers.lock");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::new(format!("reading {}: {e}", path.display())))?;
        let mut fields = std::collections::HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| Error::new(format!("{}: {line:?} is not `key = value`", path.display())))?;
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
        let mut take = |key: &str| {
            fields
                .remove(key)
                .ok_or_else(|| Error::new(format!("{} names no {key}", path.display())))
        };
        Ok(Pin {
            name: "ruby".into(),
            repo: take("repo")?,
            tag: take("tag")?,
            rev: take("rev")?,
            subdir: fields.remove("subdir"),
        })
    }

    pub fn source_root(&self, checkout: &Path) -> PathBuf {
        match &self.subdir {
            Some(sub) => checkout.join(sub),
            None => checkout.to_path_buf(),
        }
    }
}

/// Fetch the pinned revision into a SHA-stamped cache directory. A stamped
/// hit short-circuits the network entirely.
pub fn fetch_checkout(pin: &Pin) -> Result<PathBuf, Error> {
    let cache = cache_dir().join(format!("{}-{}", pin.name, pin.rev));
    let stamp = cache.join(".zeo-vendor-stamp");
    if stamp.is_file() {
        return Ok(cache);
    }
    remove_dir_all(&cache)?;
    std::fs::create_dir_all(&cache)
        .map_err(|e| Error::new(format!("creating {}: {e}", cache.display())))?;
    git(&["init", "--quiet"], &cache)?;
    git(
        &["fetch", "--depth", "1", &pin.repo, &format!("refs/tags/{}", pin.tag)],
        &cache,
    )?;
    git(&["checkout", "--quiet", "--detach", "FETCH_HEAD"], &cache)?;
    let head = git(&["rev-parse", "HEAD"], &cache)?.trim().to_string();
    if head != pin.rev {
        return Err(Error::new(format!(
            "{}: tag {} is pinned at {} but the fetched commit is {head} -- the upstream tag \
             moved, and the pin in ruby-headers.lock must be reviewed rather than followed",
            pin.name, pin.tag, pin.rev
        )));
    }
    // Strip the git metadata: the cache holds a plain, read-only tree.
    remove_dir_all(&cache.join(".git"))?;
    std::fs::write(&stamp, &pin.rev)
        .map_err(|e| Error::new(format!("writing {}: {e}", stamp.display())))?;
    Ok(cache)
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("zeo/gems")
}

pub fn git(args: &[&str], dir: &Path) -> Result<String, Error> {
    let mut argv = vec!["git"];
    argv.extend_from_slice(args);
    let out = exec::run(&argv, dir, &[], Capture::Both)?;
    if !out.success() {
        return Err(Error::new(format!(
            "git {} failed: {}",
            args.join(" "),
            out.stderr_text().trim()
        )));
    }
    Ok(out.stdout_text().into_owned())
}

pub fn copy_tree(src: &Path, dest: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dest)
        .map_err(|e| Error::new(format!("creating {}: {e}", dest.display())))?;
    let entries = std::fs::read_dir(src)
        .map_err(|e| Error::new(format!("reading {}: {e}", src.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", src.display())))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| {
                Error::new(format!("copying {} to {}: {e}", from.display(), to.display()))
            })?;
        }
    }
    Ok(())
}

/// Relative paths of every file under `dir`, sorted, dotfiles included.
/// Empty when there is no such directory.
pub fn list_files(dir: &Path) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((at, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in entries {
            let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", at.display())))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, rel));
            } else {
                out.push(rel);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Recursive byte-for-byte comparison: same file set, same contents.
pub fn dirs_equal(a: &Path, b: &Path) -> Result<bool, Error> {
    let files = list_files(a)?;
    if files != list_files(b)? {
        return Ok(false);
    }
    for rel in &files {
        if std::fs::read(a.join(rel)).ok() != std::fs::read(b.join(rel)).ok() {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn remove_dir_all(dir: &Path) -> Result<(), Error> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::new(format!("removing {}: {e}", dir.display()))),
    }
}

/// `git diff --no-index` between two trees, as a patch. It exits nonzero when
/// the trees differ, which is the whole reason it was run, so the exit code
/// says nothing and the empty output is what a failure looks like.
pub fn diff_trees(a: &Path, b: &Path) -> Result<String, Error> {
    let out = exec::run(
        &[
            Path::new("git"),
            Path::new("diff"),
            Path::new("--no-index"),
            Path::new("--src-prefix=a/"),
            Path::new("--dst-prefix=b/"),
            a,
            b,
        ],
        root(),
        &[],
        Capture::Both,
    )?;
    let text = out.stdout_text().into_owned();
    if text.is_empty() {
        return Err(Error::new(format!(
            "git diff --no-index produced nothing: {}",
            out.stderr_text().trim()
        )));
    }
    Ok(text)
}
