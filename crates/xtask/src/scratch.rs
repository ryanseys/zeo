//! A temporary directory that removes itself.
//!
//! The removal is recursive, so it is guarded: it refuses any path that is
//! not under the system temp directory and does not carry the prefix this
//! module writes. A recursive delete driven by a computed path deserves a
//! check that the path is one we made.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::Error;

const PREFIX: &str = "zeo-xtask-";

/// A directory under the system temp dir, deleted when this value drops.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// A fresh directory named `zeo-xtask-<what>-<pid>-<n>`.
    pub fn new(what: &str) -> Result<Self, Error> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let base = std::env::temp_dir();
        let pid = std::process::id();
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{PREFIX}{what}-{pid}-{n}"));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Scratch { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(Error::new(format!("creating {}: {e}", path.display()))),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A drop cannot report, so a failure to clean up is a warning rather
        // than a panic: the work itself already succeeded or failed on its
        // own terms, and leaving temp files is not worth losing that answer.
        if let Err(e) = remove(&self.path) {
            eprintln!("xtask: {e}");
        }
    }
}

fn remove(path: &Path) -> Result<(), Error> {
    let named_by_us = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(PREFIX));
    if !path.starts_with(std::env::temp_dir()) || !named_by_us {
        return Err(Error::new(format!(
            "refusing to delete {} -- not a scratch directory this process made",
            path.display()
        )));
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::new(format!("removing {}: {e}", path.display()))),
    }
}
