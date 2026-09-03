//! Compiled artifacts on disk: a library's, and a whole program's.
//!
//! | Module | Artifact |
//! |---|---|
//! | [`package`] | one library, compiled once and linked into many programs |
//! | [`autopkg`] | building one on first use, from a bundled gem |
//! | [`progcache`] | a whole program, so `zeo prog.rb` links once and execs after |
//!
//! Both caches key on an exact identity -- this compiler, this target -- and
//! neither is ever read across a mismatch.

pub mod autopkg;
pub mod package;
pub mod progcache;
