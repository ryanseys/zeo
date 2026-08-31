//! Locating and running the pinned CRuby oracle.
//!
//! A bare `ruby` off PATH is whatever version the shell happens to offer, and
//! a ledger recorded against a different ruby than the goldens came from is a
//! fiction. Everything here resolves through `mise`, the same way the golden
//! harness does.

use crate::exec::{self, Capture};
use crate::{root, root_join};

/// The oracle's own flags. `error_highlight` and `did_you_mean` rewrite an
/// exception message and zeo implements neither, so every comparison runs
/// without them.
const FLAGS: &[&str] = &["--disable-error_highlight", "--disable-did_you_mean"];

pub struct Oracle {
    bin: String,
    gemfile: String,
}

impl Oracle {
    /// `mise which ruby`, falling back to a bare `ruby`.
    pub fn find() -> Oracle {
        let bin = exec::run(&["mise", "which", "ruby"], root(), &[], Capture::Both)
            .ok()
            .filter(|out| out.success())
            .map(|out| out.stdout_text().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ruby".into());
        Oracle {
            bin,
            gemfile: root_join("Gemfile").display().to_string(),
        }
    }

    pub fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut argv = vec![self.bin.clone()];
        argv.extend(FLAGS.iter().map(|f| f.to_string()));
        argv.extend(args.iter().map(|a| a.to_string()));
        argv
    }

    /// The oracle resolves `Gemfile.lock` -- the same set the compiler
    /// vendors, so neither side can answer a `require` with a version the
    /// other does not have. `-rbundler/setup` is what `bundle exec` does, one
    /// process cheaper. The `None`s unset: whatever anybody has `gem
    /// install`ed, or points RUBYLIB at, must not reach a comparison.
    ///
    /// Needs `make deps` to have run.
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
