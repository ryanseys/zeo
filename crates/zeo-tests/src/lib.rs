//! The shared support library for the golden and e2e suites, which live in
//! `crates/zeo/tests/` -- the package that owns the `zeo` binary and
//! `libzeo.a`, so cargo rebuilds both before any suite runs. This crate
//! exists so that support code compiles once and its own unit tests run
//! once, instead of every test binary carrying a `#[path]` copy.
//!
//! Nothing here is API: the crate is `publish = false`.

pub mod e2e_support;
pub mod golden;
