//! What MRI's C API headers ARE, as data: the pin, the edits zeo makes to
//! them, and the two files it adds.
//!
//! zeo is source-compatible with MRI and ABI-incompatible with it: a gem's
//! `ext/**/*.c` compiles against MRI's own headers, and a prebuilt MRI `.so`
//! never loads. Nothing is vendored. The compiler crate fetches upstream's
//! `include/` tree at the pinned rev the first time an extension is built
//! and calls [`finish`] on it; `cargo xtask cext` materializes the same tree
//! from a checkout to run its checks. Both go through this module, so there
//! is one owner of what the tree contains.
//!
//! The three pieces:
//!
//! * [`hunks`] -- the edits. A zeo heap object is an opaque handle with no
//!   `struct RString` behind it, so every macro that reads object layout
//!   becomes a call into the view entries. Each hunk is `(file, old, new)`
//!   and must match exactly once, so a header bump surfaces here by name.
//! * [`zeo_h`] -- `ruby/internal/zeo.h`, the header that declares those
//!   entries, rendered from the same list the runtime defines them from.
//! * [`config_h`] -- `ruby/config.h`, which MRI generates with autoconf on
//!   the machine that builds the interpreter. zeo has no such step, so it
//!   renders one for the host from a table of facts.
//!
//! Nothing here touches the network.

pub mod config_h;
pub mod hunks;
pub mod zeo_h;

use std::path::Path;

/// `ruby-headers.lock`, the crate's one pin that is not a gem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub repo: String,
    pub tag: String,
    pub rev: String,
    /// The directory of the checkout the headers are: `include`.
    pub subdir: String,
    /// The GitHub archive of `rev`, which is what the compiler fetches.
    pub sha256: String,
}

const LOCK: &str = include_str!("../../ruby-headers.lock");

/// The pin. The lock is this crate's own file, so a malformed one is a bug
/// caught by the test below, not a run-time condition.
pub fn pin() -> Pin {
    parse_pin(LOCK).unwrap_or_else(|e| panic!("ruby-headers.lock: {e}"))
}

fn parse_pin(text: &str) -> Result<Pin, String> {
    let mut fields = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{line:?} is not `key = value`"))?;
        fields.insert(key.trim().to_string(), value.trim().to_string());
    }
    let mut take = |key: &str| fields.remove(key).ok_or_else(|| format!("names no {key}"));
    Ok(Pin {
        repo: take("repo")?,
        tag: take("tag")?,
        rev: take("rev")?,
        subdir: take("subdir")?,
        sha256: take("sha256")?,
    })
}

impl Pin {
    /// The tarball GitHub serves for the pinned commit.
    pub fn archive_url(&self) -> String {
        format!("{}/archive/{}.tar.gz", self.repo, self.rev)
    }

    /// The one directory that tarball unpacks to.
    pub fn archive_root(&self) -> String {
        let name = self.repo.rsplit('/').next().unwrap_or("ruby");
        format!("{name}-{}", self.rev)
    }
}

/// The platform `config.h` is rendered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub os: Os,
    pub arch: Arch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Darwin,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Aarch64,
    X86_64,
}

impl Target {
    /// The machine this binary runs on -- the only one zeo builds an
    /// extension for.
    pub fn host() -> Result<Target, String> {
        let os = if cfg!(target_os = "macos") {
            Os::Darwin
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            return Err("zeo builds C extensions on macOS and Linux only".into());
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else {
            return Err("zeo builds C extensions on aarch64 and x86_64 only".into());
        };
        Ok(Target { os, arch })
    }
}

/// Turn upstream's pristine `include/` tree into zeo's, in place: apply the
/// hunks, write `ruby/internal/zeo.h` into it, and write `ruby/config.h`
/// under `config`.
pub fn finish(include: &Path, config: &Path, target: Target) -> Result<(), String> {
    hunks::apply_all(include)?;
    write(&include.join("ruby/internal/zeo.h"), &zeo_h::render())?;
    write(&config.join("ruby/config.h"), &config_h::render(target))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_names_every_field() {
        let p = pin();
        assert_eq!(p.repo, "https://github.com/ruby/ruby");
        assert_eq!(p.subdir, "include");
        assert_eq!(p.rev.len(), 40, "a full commit sha");
        assert_eq!(p.sha256.len(), 64);
        assert_eq!(
            p.archive_url(),
            format!("https://github.com/ruby/ruby/archive/{}.tar.gz", p.rev)
        );
        assert_eq!(p.archive_root(), format!("ruby-{}", p.rev));
    }

    #[test]
    fn a_lock_missing_a_field_is_named() {
        let err = parse_pin("repo = x\ntag = y\n").unwrap_err();
        assert!(err.contains("rev"), "{err}");
    }
}
