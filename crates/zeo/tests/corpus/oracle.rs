//! The pinned CRuby oracle: the ruby that records every trailer.
//!
//! `ZEO_RUBY` names it; otherwise it is the `ruby` on PATH. Either way its
//! version has to be the one in `.ruby-version`, because a trailer recorded
//! by another ruby records that ruby's differences as zeo's bugs. Nothing
//! but a bless needs it: building and running the suite do not.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The oracle's own flags. `error_highlight` and `did_you_mean` rewrite an
/// exception message and zeo implements neither, so every recording runs
/// without them.
pub const FLAGS: &[&str] = &["--disable-error_highlight", "--disable-did_you_mean"];

pub struct Oracle {
    pub bin: PathBuf,
    pub version: String,
}

/// The version `.ruby-version` pins.
pub fn pinned_version(repo: &Path) -> Result<String, String> {
    let path = repo.join(".ruby-version");
    std::fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Find the oracle and check its version against the pin.
pub fn find(repo: &Path) -> Result<Oracle, String> {
    let pinned = pinned_version(repo)?;
    let bin = match std::env::var_os("ZEO_RUBY") {
        Some(p) => PathBuf::from(p),
        None => on_path("ruby").ok_or_else(|| {
            format!("no `ruby` on PATH and ZEO_RUBY is unset; the oracle is ruby {pinned}")
        })?,
    };
    let out = Command::new(&bin)
        .arg("-v")
        .output()
        .map_err(|e| format!("running {} -v: {e}", bin.display()))?;
    let banner = String::from_utf8_lossy(&out.stdout);
    let version = banner.split_whitespace().nth(1).unwrap_or("").to_string();
    if version != pinned {
        return Err(format!(
            "{} is ruby {version}, and the oracle is ruby {pinned} (.ruby-version); \
             install it and put it on PATH, or point ZEO_RUBY at it",
            bin.display(),
            version = if version.is_empty() { "?" } else { &version }
        ));
    }
    Ok(Oracle { bin, version })
}

/// Always an ABSOLUTE path: a caller that clears the child's environment has
/// no PATH left to resolve a bare name with.
fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|d| d.join(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.is_file())
}

impl Oracle {
    /// The oracle resolves `Gemfile.lock` -- the same set the compiler
    /// ships, so neither side can answer a `require` with a version the
    /// other does not have. `-rbundler/setup` is what `bundle exec` does, one
    /// process cheaper. The removals matter as much as the additions:
    /// whatever anybody has `gem install`ed, or points RUBYLIB at, must not
    /// decide what a trailer records.
    pub fn command(&self, repo: &Path) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.args(FLAGS)
            .env("BUNDLE_GEMFILE", repo.join("Gemfile"))
            .env("RUBYOPT", "-rbundler/setup")
            .env_remove("RUBYLIB")
            .env_remove("GEM_HOME")
            .env_remove("GEM_PATH")
            .env_remove("GEM_SPEC_CACHE");
        cmd
    }
}
