//! Repo-shape checks that need no golden corpus: filesystem hygiene over
//! `tests/` and its sidecars, the projected class surface vs the ABI table,
//! the miette diagnostic renderings, and `Gemfile.lock` against the vendored
//! gem trees. One binary -- each was its own integration-test target once,
//! which cost a full link apiece for 14 tests.

mod builtin_shape;
mod diagnostics;
mod gem_versions;
mod goldens_hygiene;
