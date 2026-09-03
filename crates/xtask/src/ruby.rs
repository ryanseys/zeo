//! Running the pinned CRuby oracle from a chore that wants an argv.
//!
//! Which ruby it is, and the version check, are the corpus harness's answer
//! (`crate::oracle`): `ZEO_RUBY` names it, else the `ruby` on PATH, and its
//! version has to be the one `.ruby-version` pins. A bare `ruby` off PATH is
//! whatever the shell happens to offer, and a ledger recorded against a
//! different ruby than the goldens came from is a fiction.
//!
//! What this adds is the shape `exec::run` wants: an argv and an environment
//! list, rather than the `Command` the harness builds.

use crate::{Error, root, root_join};

pub struct Oracle {
    bin: String,
    gemfile: String,
}

impl Oracle {
    pub fn find() -> Result<Oracle, Error> {
        let found = crate::oracle::find(root()).map_err(Error::new)?;
        Ok(Oracle {
            bin: found.bin.display().to_string(),
            gemfile: root_join("Gemfile").display().to_string(),
        })
    }

    pub fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut argv = vec![self.bin.clone()];
        argv.extend(crate::oracle::FLAGS.iter().map(|f| f.to_string()));
        argv.extend(args.iter().map(|a| a.to_string()));
        argv
    }

    /// The oracle resolves `Gemfile.lock` -- the same set the compiler
    /// ships, so neither side can answer a `require` with a version the other
    /// does not have. `-rbundler/setup` is what `bundle exec` does, one
    /// process cheaper. The `None`s unset: whatever anybody has `gem
    /// install`ed, or points RUBYLIB at, must not reach a comparison.
    ///
    /// Needs `cargo xtask deps --oracle` to have run.
    pub fn env(&self) -> Vec<(&'static str, Option<&str>)> {
        vec![
            ("BUNDLE_GEMFILE", Some(self.gemfile.as_str())),
            ("RUBYOPT", Some("-rbundler/setup")),
            ("RUBYLIB", None),
            ("GEM_HOME", None),
            ("GEM_PATH", None),
            ("GEM_SPEC_CACHE", None),
        ]
    }
}
