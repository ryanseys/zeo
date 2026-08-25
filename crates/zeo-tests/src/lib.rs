//! The shared support library for this crate's `tests/` targets -- the golden
//! harness and the e2e in-process harness. It exists so the five test
//! binaries stop each compiling their own `#[path]` copy: one build, one run
//! of the support code's own unit tests, and `zeo_cli()` /
//! `newer_compiler_source()` in one place for every consumer.
//!
//! Nothing here is API: the crate is `publish = false` and exists only for
//! its test targets.

pub mod e2e_support;
pub mod golden;
