//! Fetching an upstream tree at a pinned revision, and comparing trees.
//!
//! The model is VENDOR-ON-FETCH: a git URL plus a tag is the source, and the
//! committed tree is the storage the build reads. A fresh clone of zeo builds
//! offline; only re-vendoring runs the network. Vendoring needs only `git`.

use std::path::{Path, PathBuf};

use crate::Error;
use crate::exec::{self, Capture};

/// One pinned upstream tree.
pub struct Pin {
    pub name: String,
    pub repo: String,
    pub tag: String,
    pub rev: String,
    pub subdir: Option<String>,
}

impl Pin {
    /// The MRI C API headers, from the C-API crate's own `ruby-headers.lock`.
    pub fn ruby_headers() -> Result<Pin, Error> {
        let p = zeo_capi::headers::pin();
        Ok(Pin {
            name: "ruby".into(),
            repo: p.repo,
            tag: p.tag,
            rev: p.rev,
            subdir: Some(p.subdir),
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
        &[
            "fetch",
            "--depth",
            "1",
            &pin.repo,
            &format!("refs/tags/{}", pin.tag),
        ],
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
                Error::new(format!(
                    "copying {} to {}: {e}",
                    from.display(),
                    to.display()
                ))
            })?;
        }
    }
    Ok(())
}

pub fn remove_dir_all(dir: &Path) -> Result<(), Error> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::new(format!("removing {}: {e}", dir.display()))),
    }
}
