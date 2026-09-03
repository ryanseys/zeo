//! RubyGems' formats, in Rust.
//!
//! Everything zeo needs to read what Bundler and RubyGems wrote, and to build
//! a gem store without running Ruby:
//!
//! | Module | Format |
//! |---|---|
//! | [`version`] | `Gem::Version` and `Gem::Requirement` |
//! | [`platform`] | `Gem::Platform` |
//! | [`lockfile`] | `Gemfile.lock`, every section, round-trip exact |
//! | [`gemspec`] | a gemspec, read statically -- no Ruby is evaluated |
//! | [`package`] | a `.gem` file |
//! | [`store`] | a store on disk: `gems/` and `specifications/` |
//!
//! There is no HTTP client here and no resolver. This crate reads and writes
//! the formats; deciding WHICH gem to fetch is the lockfile's answer, and
//! fetching it is the caller's business.

pub mod error;
pub mod gemspec;
pub mod lockfile;
pub mod package;
pub mod platform;
pub mod store;
pub mod version;

pub use error::{Error, Result};
pub use gemspec::Gemspec;
pub use lockfile::{Checksum, GemSource, LockedGem, Lockfile, Spec};
pub use package::Package;
pub use platform::Platform;
pub use store::Store;
pub use version::{Requirement, Version};

#[cfg(test)]
mod testing;
